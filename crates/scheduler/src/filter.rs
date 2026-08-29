use crate::model::{
    CandidateSnapshot, EligibilityReport, EligibilityResolution, EligibleCandidate, IsolationClass,
    JobRequirements, MissingFact, NodeState, Policy, PoolSnapshot, RejectedCandidate,
    RejectionReason, RiskState, SecurityTier, Sensitivity, SideEffectClass,
};

pub fn evaluate_eligibility(
    pool: &PoolSnapshot,
    job: &JobRequirements,
    policy: &Policy,
) -> EligibilityReport {
    let mut eligible = Vec::new();
    let mut rejected = Vec::new();

    for candidate in &pool.candidates {
        let mut reasons = evaluate_candidate(candidate, pool.evaluated_at_unix_ms, job, policy);
        reasons.sort();
        reasons.dedup();
        if reasons.is_empty() {
            eligible.push(EligibleCandidate {
                node_id: candidate.node_id.clone(),
            });
        } else {
            rejected.push(RejectedCandidate {
                node_id: candidate.node_id.clone(),
                reasons,
            });
        }
    }

    // node_id가 중복된 비정상 입력에서도 후보 입력 순서에 영향을 받지 않는다.
    eligible.sort();
    rejected.sort();
    let resolution = match eligible.as_slice() {
        [] => EligibilityResolution::NoEligibleCandidates,
        [only] => EligibilityResolution::SingleEligible {
            node_id: only.node_id.clone(),
        },
        _ => EligibilityResolution::RankingRequired,
    };
    EligibilityReport {
        eligible,
        rejected,
        resolution,
    }
}

fn evaluate_candidate(
    candidate: &CandidateSnapshot,
    evaluated_at_unix_ms: u64,
    job: &JobRequirements,
    policy: &Policy,
) -> Vec<RejectionReason> {
    let mut reasons = Vec::new();
    if candidate.node_id.is_empty() {
        reasons.push(RejectionReason::MissingFact(MissingFact::NodeId));
    }

    match candidate.node_state {
        None => missing(&mut reasons, MissingFact::NodeState),
        Some(NodeState::Online) => {}
        Some(actual) => reasons.push(RejectionReason::NodeNotOnline { actual }),
    }
    match candidate.risk_state {
        None => missing(&mut reasons, MissingFact::RiskState),
        Some(RiskState::Normal) => {}
        Some(actual) => reasons.push(RejectionReason::RiskNotNormal { actual }),
    }
    match candidate.observed_at_unix_ms {
        None => missing(&mut reasons, MissingFact::ObservedAt),
        Some(observed)
            if observed > evaluated_at_unix_ms
                || evaluated_at_unix_ms - observed > policy.maximum_snapshot_age_ms =>
        {
            reasons.push(RejectionReason::SnapshotNotFresh {
                observed_at_unix_ms: observed,
                evaluated_at_unix_ms,
                maximum_age_ms: policy.maximum_snapshot_age_ms,
            });
        }
        Some(_) => {}
    }

    compare_minimum(
        candidate.security_tier,
        job.minimum_security_tier,
        MissingFact::SecurityTier,
        MissingFact::JobMinimumSecurityTier,
        &mut reasons,
        |available, required| RejectionReason::SecurityTierTooLow {
            available,
            required,
        },
    );
    compare_minimum(
        candidate.isolation_class,
        job.minimum_isolation_class,
        MissingFact::IsolationClass,
        MissingFact::JobMinimumIsolationClass,
        &mut reasons,
        |available, required| RejectionReason::IsolationClassTooLow {
            available,
            required,
        },
    );
    compare_minimum(
        candidate.key_protection,
        job.minimum_key_protection,
        MissingFact::KeyProtection,
        MissingFact::JobMinimumKeyProtection,
        &mut reasons,
        |available, required| RejectionReason::KeyProtectionTooLow {
            available,
            required,
        },
    );

    evaluate_gpus(candidate, job, &mut reasons);
    compare_resource(
        candidate.available_cpu_cores,
        job.cpu_cores,
        MissingFact::AvailableCpu,
        MissingFact::JobCpu,
        &mut reasons,
        |available, required| RejectionReason::CpuInsufficient {
            available,
            required,
        },
    );
    compare_resource(
        candidate.available_ram_bytes,
        job.ram_bytes,
        MissingFact::AvailableRam,
        MissingFact::JobRam,
        &mut reasons,
        |available, required| RejectionReason::RamInsufficient {
            available,
            required,
        },
    );
    compare_resource(
        candidate.available_workspace_bytes,
        job.workspace_bytes,
        MissingFact::AvailableWorkspace,
        MissingFact::JobWorkspace,
        &mut reasons,
        |available, required| RejectionReason::WorkspaceInsufficient {
            available,
            required,
        },
    );

    evaluate_owner_policy(candidate, job, &mut reasons);
    reasons
}

fn evaluate_gpus(
    candidate: &CandidateSnapshot,
    job: &JobRequirements,
    reasons: &mut Vec<RejectionReason>,
) {
    let required_count = job.minimum_gpu_count;
    let required_vram = job.minimum_vram_bytes_per_gpu;
    if required_count.is_none() {
        missing(reasons, MissingFact::JobMinimumGpuCount);
    }
    if required_vram.is_none() {
        missing(reasons, MissingFact::JobMinimumVram);
    }
    let (Some(required_count), Some(required_vram)) = (required_count, required_vram) else {
        return;
    };
    let Some(gpus) = candidate.gpus.as_ref() else {
        missing(reasons, MissingFact::GpuInventory);
        return;
    };
    if gpus.len() < required_count as usize {
        reasons.push(RejectionReason::GpuCountInsufficient {
            available: saturating_u32(gpus.len()),
            required: required_count,
        });
        return;
    }

    let mut health_unknown = false;
    let healthy: Vec<_> = gpus
        .iter()
        .filter(|gpu| match gpu.healthy {
            Some(true) => true,
            Some(false) => false,
            None => {
                health_unknown = true;
                missing(
                    reasons,
                    MissingFact::GpuHealth {
                        gpu_id: gpu.gpu_id.clone(),
                    },
                );
                false
            }
        })
        .collect();
    if health_unknown {
        return;
    }
    if healthy.len() < required_count as usize {
        reasons.push(RejectionReason::HealthyGpuCountInsufficient {
            available: saturating_u32(healthy.len()),
            required: required_count,
        });
        return;
    }

    let model_matched: Vec<_> = if job.allowed_gpu_models.is_empty() {
        healthy
    } else {
        let mut model_unknown = false;
        let matched = healthy
            .into_iter()
            .filter(|gpu| match gpu.model.as_ref() {
                Some(model) => job.allowed_gpu_models.contains(model),
                None => {
                    model_unknown = true;
                    missing(
                        reasons,
                        MissingFact::GpuModel {
                            gpu_id: gpu.gpu_id.clone(),
                        },
                    );
                    false
                }
            })
            .collect::<Vec<_>>();
        if model_unknown {
            return;
        }
        if matched.len() < required_count as usize {
            let mut allowed = job.allowed_gpu_models.clone();
            allowed.sort();
            allowed.dedup();
            reasons.push(RejectionReason::GpuModelMismatch {
                matching: saturating_u32(matched.len()),
                required: required_count,
                allowed,
            });
            return;
        }
        matched
    };

    let mut vram_unknown = false;
    let matching_vram = model_matched
        .iter()
        .filter(|gpu| match gpu.available_vram_bytes {
            Some(available) => available >= required_vram,
            None => {
                vram_unknown = true;
                missing(
                    reasons,
                    MissingFact::GpuVram {
                        gpu_id: gpu.gpu_id.clone(),
                    },
                );
                false
            }
        })
        .count();
    if !vram_unknown && matching_vram < required_count as usize {
        reasons.push(RejectionReason::GpuVramInsufficient {
            matching: saturating_u32(matching_vram),
            required: required_count,
            minimum_bytes: required_vram,
        });
    }
}

fn evaluate_owner_policy(
    candidate: &CandidateSnapshot,
    job: &JobRequirements,
    reasons: &mut Vec<RejectionReason>,
) {
    let workload = match job.workload_class {
        Some(value) => Some(value),
        None => {
            missing(reasons, MissingFact::JobWorkloadClass);
            None
        }
    };
    match (&candidate.allowed_workload_classes, workload) {
        (None, _) => missing(reasons, MissingFact::AllowedWorkloadClasses),
        (Some(allowed), Some(class)) if !allowed.contains(&class) => {
            reasons.push(RejectionReason::WorkloadClassNotAllowed {
                workload_class: class,
            });
        }
        _ => {}
    }

    let owner = match candidate.owner_member_id.as_ref() {
        Some(value) if !value.is_empty() => Some(value),
        None | Some(_) => {
            missing(reasons, MissingFact::OwnerMemberId);
            None
        }
    };
    let submitter = match job.submitter_member_id.as_ref() {
        Some(value) if !value.is_empty() => Some(value),
        None | Some(_) => {
            missing(reasons, MissingFact::JobSubmitterMemberId);
            None
        }
    };
    if !matches!((owner, submitter), (Some(owner), Some(submitter)) if owner != submitter) {
        return;
    }

    if candidate.security_tier == Some(SecurityTier::S0) {
        reasons.push(RejectionReason::ThirdPartyJobForbiddenOnS0);
    }
    if candidate.isolation_class != Some(IsolationClass::Restricted) {
        return;
    }

    match candidate.third_party_workloads_opt_in {
        None => missing(reasons, MissingFact::ThirdPartyOptIn),
        Some(false) => {
            reasons.push(RejectionReason::ThirdPartyOptInRequiredOnRestrictedIsolation);
        }
        Some(true) => {}
    }
    match job.side_effect_class {
        None => missing(reasons, MissingFact::JobSideEffectClass),
        Some(SideEffectClass::Pure) => {}
        Some(actual) => {
            reasons.push(RejectionReason::ThirdPartyJobMustBePureOnRestrictedIsolation { actual })
        }
    }
    match job.sensitivity {
        None => missing(reasons, MissingFact::JobSensitivity),
        Some(Sensitivity::Sensitive) => {
            reasons.push(RejectionReason::ThirdPartySensitiveDataForbiddenOnRestrictedIsolation);
        }
        Some(Sensitivity::Public | Sensitivity::Internal) => {}
    }
}

fn compare_minimum<T: Copy + Ord>(
    available: Option<T>,
    required: Option<T>,
    missing_available: MissingFact,
    missing_required: MissingFact,
    reasons: &mut Vec<RejectionReason>,
    rejection: impl FnOnce(T, T) -> RejectionReason,
) {
    if available.is_none() {
        missing(reasons, missing_available);
    }
    if required.is_none() {
        missing(reasons, missing_required);
    }
    if let (Some(available), Some(required)) = (available, required) {
        if available < required {
            reasons.push(rejection(available, required));
        }
    }
}

fn compare_resource<T: Copy + Ord>(
    available: Option<T>,
    required: Option<T>,
    missing_available: MissingFact,
    missing_required: MissingFact,
    reasons: &mut Vec<RejectionReason>,
    rejection: impl FnOnce(T, T) -> RejectionReason,
) {
    compare_minimum(
        available,
        required,
        missing_available,
        missing_required,
        reasons,
        rejection,
    );
}

fn missing(reasons: &mut Vec<RejectionReason>, fact: MissingFact) {
    reasons.push(RejectionReason::MissingFact(fact));
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
