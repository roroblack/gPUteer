use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    BestFitPolicy, BestFitRanking, CandidateSnapshot, EligibilityReport, EligibilityResolution,
    FitAxis, FitKey, JobRequirements, MissingFact, PoolSnapshot, RankedCandidate, RankingError,
};

/// 복수 hard-filter 적격 후보를 resource-tight 순서로 정렬한다.
///
/// 이 함수는 입력 snapshot을 예약하지 않으며 외부 상태를 읽지 않는다. 정책 축의
/// 잔여량이 작은 후보가 먼저 오고, 모든 축이 같을 때만 `node_id` 오름차순을 쓴다.
pub fn rank_best_fit(
    pool: &PoolSnapshot,
    job: &JobRequirements,
    report: &EligibilityReport,
    policy: &BestFitPolicy,
) -> Result<BestFitRanking, RankingError> {
    validate_policy(policy)?;
    if report.resolution != EligibilityResolution::RankingRequired {
        return Err(RankingError::ResolutionNotRankingRequired);
    }
    if report.eligible.len() < 2 {
        return Err(RankingError::EligibleCandidateCountNotMultiple {
            actual: report.eligible.len(),
        });
    }

    let pool_by_id = validate_candidate_sets(pool, report)?;
    let requirements = RankRequirements::from_job(job)?;
    let mut ranked = report
        .eligible
        .iter()
        .map(|eligible| {
            let candidate = pool_by_id[eligible.node_id.as_str()];
            Ok(RankedCandidate {
                node_id: eligible.node_id.clone(),
                fit_key: fit_key(candidate, job, &requirements)?,
            })
        })
        .collect::<Result<Vec<_>, RankingError>>()?;

    ranked.sort_by(|left, right| compare_ranked(left, right, policy));
    let winner = ranked[0].clone();
    Ok(BestFitRanking { winner, ranked })
}

fn validate_policy(policy: &BestFitPolicy) -> Result<(), RankingError> {
    let unique = policy.axis_order.into_iter().collect::<BTreeSet<_>>();
    if unique.len() != policy.axis_order.len() {
        return Err(RankingError::InvalidPolicyAxisOrder);
    }
    Ok(())
}

fn validate_candidate_sets<'a>(
    pool: &'a PoolSnapshot,
    report: &EligibilityReport,
) -> Result<BTreeMap<&'a str, &'a CandidateSnapshot>, RankingError> {
    let mut pool_by_id = BTreeMap::new();
    for candidate in &pool.candidates {
        if candidate.node_id.is_empty() {
            return Err(RankingError::EmptyNodeId);
        }
        if pool_by_id.insert(candidate.node_id.as_str(), candidate).is_some() {
            return Err(RankingError::DuplicatePoolNodeId { node_id: candidate.node_id.clone() });
        }
    }

    let mut report_ids = BTreeSet::new();
    for node_id in report
        .eligible
        .iter()
        .map(|candidate| &candidate.node_id)
        .chain(report.rejected.iter().map(|candidate| &candidate.node_id))
    {
        if node_id.is_empty() {
            return Err(RankingError::EmptyNodeId);
        }
        if !report_ids.insert(node_id.as_str()) {
            return Err(RankingError::DuplicateReportNodeId { node_id: node_id.clone() });
        }
    }

    if let Some(node_id) = report_ids.iter().find(|node_id| !pool_by_id.contains_key(**node_id)) {
        return Err(RankingError::ReportCandidateMissingFromPool {
            node_id: (*node_id).to_string(),
        });
    }
    if let Some(node_id) = pool_by_id.keys().find(|node_id| !report_ids.contains(**node_id)) {
        return Err(RankingError::PoolCandidateMissingFromReport {
            node_id: (*node_id).to_string(),
        });
    }
    Ok(pool_by_id)
}

#[derive(Clone, Copy)]
struct RankRequirements {
    gpu_count: u32,
    vram_per_gpu: u64,
    cpu_cores: u32,
    ram_bytes: u64,
    workspace_bytes: u64,
}

impl RankRequirements {
    fn from_job(job: &JobRequirements) -> Result<Self, RankingError> {
        Ok(Self {
            gpu_count: job
                .minimum_gpu_count
                .ok_or_else(|| missing_job(MissingFact::JobMinimumGpuCount))?,
            vram_per_gpu: job
                .minimum_vram_bytes_per_gpu
                .ok_or_else(|| missing_job(MissingFact::JobMinimumVram))?,
            cpu_cores: job.cpu_cores.ok_or_else(|| missing_job(MissingFact::JobCpu))?,
            ram_bytes: job.ram_bytes.ok_or_else(|| missing_job(MissingFact::JobRam))?,
            workspace_bytes: job
                .workspace_bytes
                .ok_or_else(|| missing_job(MissingFact::JobWorkspace))?,
        })
    }
}

fn fit_key(
    candidate: &CandidateSnapshot,
    job: &JobRequirements,
    required: &RankRequirements,
) -> Result<FitKey, RankingError> {
    let node_id = candidate.node_id.as_str();
    let gpus = candidate
        .gpus
        .as_ref()
        .ok_or_else(|| missing_candidate(node_id, MissingFact::GpuInventory))?;
    let mut matching_vram = Vec::new();
    for gpu in gpus {
        match gpu.healthy {
            None => {
                return Err(missing_candidate(
                    node_id,
                    MissingFact::GpuHealth { gpu_id: gpu.gpu_id.clone() },
                ));
            }
            Some(false) => continue,
            Some(true) => {}
        }
        if !job.allowed_gpu_models.is_empty() {
            let model = gpu.model.as_ref().ok_or_else(|| {
                missing_candidate(node_id, MissingFact::GpuModel { gpu_id: gpu.gpu_id.clone() })
            })?;
            if !job.allowed_gpu_models.contains(model) {
                continue;
            }
        }
        let available = gpu.available_vram_bytes.ok_or_else(|| {
            missing_candidate(node_id, MissingFact::GpuVram { gpu_id: gpu.gpu_id.clone() })
        })?;
        if available >= required.vram_per_gpu {
            matching_vram.push(available);
        }
    }

    matching_vram.sort_unstable();
    let required_gpu_count = usize::try_from(required.gpu_count).map_err(|_| {
        RankingError::FitOverflow { node_id: node_id.to_owned(), axis: FitAxis::GpuCount }
    })?;
    if matching_vram.len() < required_gpu_count {
        return Err(mismatch(node_id, FitAxis::GpuCount));
    }
    let vram_remaining_bytes = matching_vram
        .iter()
        .take(required_gpu_count)
        .try_fold(0_u64, |total, available| {
            total.checked_add(*available - required.vram_per_gpu)
        })
        .ok_or_else(|| RankingError::FitOverflow {
            node_id: node_id.to_owned(),
            axis: FitAxis::Vram,
        })?;
    let gpu_count_remaining = u32::try_from(matching_vram.len() - required_gpu_count).map_err(
        |_| RankingError::FitOverflow { node_id: node_id.to_owned(), axis: FitAxis::GpuCount },
    )?;

    Ok(FitKey {
        vram_remaining_bytes,
        gpu_count_remaining,
        cpu_cores_remaining: remaining(
            candidate.available_cpu_cores,
            required.cpu_cores,
            node_id,
            MissingFact::AvailableCpu,
            FitAxis::Cpu,
        )?,
        ram_remaining_bytes: remaining(
            candidate.available_ram_bytes,
            required.ram_bytes,
            node_id,
            MissingFact::AvailableRam,
            FitAxis::Ram,
        )?,
        workspace_remaining_bytes: remaining(
            candidate.available_workspace_bytes,
            required.workspace_bytes,
            node_id,
            MissingFact::AvailableWorkspace,
            FitAxis::Workspace,
        )?,
    })
}

fn remaining<T: Copy + std::ops::Sub<Output = T> + Ord>(
    available: Option<T>,
    required: T,
    node_id: &str,
    missing: MissingFact,
    axis: FitAxis,
) -> Result<T, RankingError> {
    let available = available.ok_or_else(|| missing_candidate(node_id, missing))?;
    if available < required {
        return Err(mismatch(node_id, axis));
    }
    Ok(available - required)
}

fn compare_ranked(
    left: &RankedCandidate,
    right: &RankedCandidate,
    policy: &BestFitPolicy,
) -> Ordering {
    for axis in policy.axis_order {
        let ordering = match axis {
            FitAxis::Vram => left
                .fit_key
                .vram_remaining_bytes
                .cmp(&right.fit_key.vram_remaining_bytes),
            FitAxis::GpuCount => left
                .fit_key
                .gpu_count_remaining
                .cmp(&right.fit_key.gpu_count_remaining),
            FitAxis::Cpu => left
                .fit_key
                .cpu_cores_remaining
                .cmp(&right.fit_key.cpu_cores_remaining),
            FitAxis::Ram => {
                left.fit_key.ram_remaining_bytes.cmp(&right.fit_key.ram_remaining_bytes)
            }
            FitAxis::Workspace => left
                .fit_key
                .workspace_remaining_bytes
                .cmp(&right.fit_key.workspace_remaining_bytes),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.node_id.cmp(&right.node_id)
}

fn missing_job(fact: MissingFact) -> RankingError {
    RankingError::MissingRankFact { node_id: None, fact }
}

fn missing_candidate(node_id: &str, fact: MissingFact) -> RankingError {
    RankingError::MissingRankFact { node_id: Some(node_id.to_owned()), fact }
}

fn mismatch(node_id: &str, axis: FitAxis) -> RankingError {
    RankingError::EligibleCandidateMismatch { node_id: node_id.to_owned(), axis }
}
