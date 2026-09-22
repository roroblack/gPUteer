use std::collections::BTreeSet;

use gputeer_scheduler::{
    evaluate_eligibility, CandidateSnapshot, EligibilityReport, EligibilityResolution, GpuSnapshot,
    IsolationClass, JobRequirements, KeyProtection, MissingFact, NodeState, Policy, PoolSnapshot,
    RejectionReason, RiskState, SecurityTier, Sensitivity, SideEffectClass, WorkloadClass,
};

const NOW: u64 = 1_000;
const MAX_AGE: u64 = 100;
const VRAM: u64 = 16 * 1024 * 1024 * 1024;
const RAM: u64 = 32 * 1024 * 1024 * 1024;
const WORKSPACE: u64 = 100 * 1024 * 1024 * 1024;

fn job() -> JobRequirements {
    JobRequirements {
        submitter_member_id: Some("owner-a".into()),
        workload_class: Some(WorkloadClass::Training),
        side_effect_class: Some(SideEffectClass::Pure),
        sensitivity: Some(Sensitivity::Public),
        minimum_security_tier: Some(SecurityTier::S2),
        minimum_isolation_class: Some(IsolationClass::Contained),
        minimum_key_protection: Some(KeyProtection::K1),
        minimum_gpu_count: Some(1),
        minimum_vram_bytes_per_gpu: Some(VRAM),
        allowed_gpu_models: vec!["RTX 4090".into()],
        cpu_cores: Some(8),
        ram_bytes: Some(RAM),
        workspace_bytes: Some(WORKSPACE),
    }
}

fn candidate(node_id: &str) -> CandidateSnapshot {
    CandidateSnapshot {
        reservation: None,
        node_id: node_id.into(),
        inventory_revision: Some(1),
        owner_member_id: Some("owner-a".into()),
        node_state: Some(NodeState::Online),
        risk_state: Some(RiskState::Normal),
        observed_at_unix_ms: Some(NOW - MAX_AGE),
        security_tier: Some(SecurityTier::S2),
        isolation_class: Some(IsolationClass::Contained),
        key_protection: Some(KeyProtection::K1),
        gpus: Some(vec![GpuSnapshot {
            gpu_id: format!("{node_id}-gpu-0"),
            healthy: Some(true),
            available_vram_bytes: Some(VRAM),
            model: Some("RTX 4090".into()),
        }]),
        available_cpu_cores: Some(8),
        available_ram_bytes: Some(RAM),
        available_workspace_bytes: Some(WORKSPACE),
        allowed_workload_classes: Some(BTreeSet::from([WorkloadClass::Training])),
        third_party_workloads_opt_in: None,
    }
}

fn report(candidate: CandidateSnapshot, job: &JobRequirements) -> EligibilityReport {
    evaluate_eligibility(
        &PoolSnapshot {
            evaluated_at_unix_ms: NOW,
            candidates: vec![candidate],
        },
        job,
        &Policy {
            maximum_snapshot_age_ms: MAX_AGE,
        },
    )
}

fn assert_eligible(candidate: CandidateSnapshot, job: &JobRequirements) {
    let actual = report(candidate, job);
    assert_eq!(actual.rejected, vec![]);
    assert_eq!(actual.eligible.len(), 1);
    assert!(matches!(
        actual.resolution,
        EligibilityResolution::SingleEligible { .. }
    ));
}

fn assert_rejected_with(
    candidate: CandidateSnapshot,
    job: &JobRequirements,
    expected: RejectionReason,
) {
    let actual = report(candidate, job);
    assert!(
        actual.eligible.is_empty(),
        "unexpected eligible report: {actual:?}"
    );
    assert!(
        actual.rejected[0].reasons.contains(&expected),
        "missing {expected:?} in {actual:?}"
    );
}

#[test]
fn exact_boundaries_pass() {
    // freshness, tier/isolation/key, VRAM/CPU/RAM/workspace are exactly at the boundary.
    assert_eligible(candidate("node-a"), &job());
}

#[test]
fn node_must_be_online() {
    let mut c = candidate("node-a");
    c.node_state = Some(NodeState::Suspect);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::NodeNotOnline {
            actual: NodeState::Suspect,
        },
    );
}

#[test]
fn risk_must_be_normal() {
    let mut c = candidate("node-a");
    c.risk_state = Some(RiskState::Suspect);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::RiskNotNormal {
            actual: RiskState::Suspect,
        },
    );
}

#[test]
fn snapshot_one_millisecond_over_age_limit_is_rejected() {
    let mut c = candidate("node-a");
    c.observed_at_unix_ms = Some(NOW - MAX_AGE - 1);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::SnapshotNotFresh {
            observed_at_unix_ms: NOW - MAX_AGE - 1,
            evaluated_at_unix_ms: NOW,
            maximum_age_ms: MAX_AGE,
        },
    );
}

#[test]
fn future_dated_snapshot_is_not_treated_as_fresh() {
    let mut c = candidate("node-a");
    c.observed_at_unix_ms = Some(NOW + 1);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::SnapshotNotFresh {
            observed_at_unix_ms: NOW + 1,
            evaluated_at_unix_ms: NOW,
            maximum_age_ms: MAX_AGE,
        },
    );
}

#[test]
fn security_tier_just_below_requirement_is_rejected() {
    let mut c = candidate("node-a");
    c.security_tier = Some(SecurityTier::S1);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::SecurityTierTooLow {
            available: SecurityTier::S1,
            required: SecurityTier::S2,
        },
    );
}

#[test]
fn isolation_class_just_below_requirement_is_rejected() {
    let mut c = candidate("node-a");
    c.isolation_class = Some(IsolationClass::Restricted);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::IsolationClassTooLow {
            available: IsolationClass::Restricted,
            required: IsolationClass::Contained,
        },
    );
}

#[test]
fn key_protection_just_below_requirement_is_rejected() {
    let mut c = candidate("node-a");
    c.key_protection = Some(KeyProtection::K0);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::KeyProtectionTooLow {
            available: KeyProtection::K0,
            required: KeyProtection::K1,
        },
    );
}

#[test]
fn gpu_count_just_below_requirement_is_rejected() {
    let mut j = job();
    j.minimum_gpu_count = Some(2);
    assert_rejected_with(
        candidate("node-a"),
        &j,
        RejectionReason::GpuCountInsufficient {
            available: 1,
            required: 2,
        },
    );
}

#[test]
fn unhealthy_gpu_is_rejected() {
    let mut c = candidate("node-a");
    c.gpus.as_mut().unwrap()[0].healthy = Some(false);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::HealthyGpuCountInsufficient {
            available: 0,
            required: 1,
        },
    );
}

#[test]
fn gpu_model_mismatch_is_rejected() {
    let mut c = candidate("node-a");
    c.gpus.as_mut().unwrap()[0].model = Some("RTX 3090".into());
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::GpuModelMismatch {
            matching: 0,
            required: 1,
            allowed: vec!["RTX 4090".into()],
        },
    );
}

#[test]
fn gpu_vram_one_byte_below_requirement_is_rejected() {
    let mut c = candidate("node-a");
    c.gpus.as_mut().unwrap()[0].available_vram_bytes = Some(VRAM - 1);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::GpuVramInsufficient {
            matching: 0,
            required: 1,
            minimum_bytes: VRAM,
        },
    );
}

#[test]
fn cpu_one_core_below_requirement_is_rejected() {
    let mut c = candidate("node-a");
    c.available_cpu_cores = Some(7);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::CpuInsufficient {
            available: 7,
            required: 8,
        },
    );
}

#[test]
fn ram_one_byte_below_requirement_is_rejected() {
    let mut c = candidate("node-a");
    c.available_ram_bytes = Some(RAM - 1);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::RamInsufficient {
            available: RAM - 1,
            required: RAM,
        },
    );
}

#[test]
fn workspace_one_byte_below_requirement_is_rejected() {
    let mut c = candidate("node-a");
    c.available_workspace_bytes = Some(WORKSPACE - 1);
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::WorkspaceInsufficient {
            available: WORKSPACE - 1,
            required: WORKSPACE,
        },
    );
}

#[test]
fn workload_class_must_be_owner_allowed() {
    let mut c = candidate("node-a");
    c.allowed_workload_classes = Some(BTreeSet::from([WorkloadClass::Inference]));
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::WorkloadClassNotAllowed {
            workload_class: WorkloadClass::Training,
        },
    );
}

#[test]
fn empty_owner_member_id_is_a_missing_fact() {
    let mut c = candidate("node-a");
    c.owner_member_id = Some(String::new());
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::MissingFact(MissingFact::OwnerMemberId),
    );
}

#[test]
fn empty_submitter_member_id_is_a_missing_fact() {
    let mut j = job();
    j.submitter_member_id = Some(String::new());
    assert_rejected_with(
        candidate("node-a"),
        &j,
        RejectionReason::MissingFact(MissingFact::JobSubmitterMemberId),
    );
}

#[test]
fn s0_allows_owner_job_but_forbids_third_party_job() {
    let mut j = job();
    j.minimum_security_tier = Some(SecurityTier::S0);
    j.minimum_isolation_class = Some(IsolationClass::Restricted);
    let mut c = candidate("node-a");
    c.security_tier = Some(SecurityTier::S0);
    c.isolation_class = Some(IsolationClass::Restricted);
    assert_eligible(c.clone(), &j);

    j.submitter_member_id = Some("member-b".into());
    assert_rejected_with(c, &j, RejectionReason::ThirdPartyJobForbiddenOnS0);
}

fn restricted_third_party(security_tier: SecurityTier) -> (CandidateSnapshot, JobRequirements) {
    let mut j = job();
    j.submitter_member_id = Some("member-b".into());
    j.minimum_security_tier = Some(security_tier);
    j.minimum_isolation_class = Some(IsolationClass::Restricted);
    let mut c = candidate("node-a");
    c.security_tier = Some(security_tier);
    c.isolation_class = Some(IsolationClass::Restricted);
    c.third_party_workloads_opt_in = Some(true);
    (c, j)
}

#[test]
fn s1_third_party_requires_device_opt_in() {
    let (mut c, j) = restricted_third_party(SecurityTier::S1);
    c.third_party_workloads_opt_in = Some(false);
    assert_rejected_with(
        c,
        &j,
        RejectionReason::ThirdPartyOptInRequiredOnRestrictedIsolation,
    );
}

#[test]
fn s1_third_party_must_be_pure() {
    let (c, mut j) = restricted_third_party(SecurityTier::S1);
    j.side_effect_class = Some(SideEffectClass::Idempotent);
    assert_rejected_with(
        c,
        &j,
        RejectionReason::ThirdPartyJobMustBePureOnRestrictedIsolation {
            actual: SideEffectClass::Idempotent,
        },
    );
}

#[test]
fn s1_third_party_forbids_sensitive_data() {
    let (c, mut j) = restricted_third_party(SecurityTier::S1);
    j.sensitivity = Some(Sensitivity::Sensitive);
    assert_rejected_with(
        c,
        &j,
        RejectionReason::ThirdPartySensitiveDataForbiddenOnRestrictedIsolation,
    );
}

#[test]
fn s1_third_party_passes_with_opt_in_pure_and_non_sensitive_data() {
    let (c, j) = restricted_third_party(SecurityTier::S1);
    assert_eligible(c, &j);
}

#[test]
fn s2_restricted_third_party_requires_device_opt_in() {
    let (mut c, j) = restricted_third_party(SecurityTier::S2);
    c.third_party_workloads_opt_in = Some(false);
    assert_rejected_with(
        c,
        &j,
        RejectionReason::ThirdPartyOptInRequiredOnRestrictedIsolation,
    );
}

#[test]
fn s2_restricted_third_party_must_be_pure() {
    let (c, mut j) = restricted_third_party(SecurityTier::S2);
    j.side_effect_class = Some(SideEffectClass::Idempotent);
    assert_rejected_with(
        c,
        &j,
        RejectionReason::ThirdPartyJobMustBePureOnRestrictedIsolation {
            actual: SideEffectClass::Idempotent,
        },
    );
}

#[test]
fn s2_restricted_third_party_forbids_sensitive_data() {
    let (c, mut j) = restricted_third_party(SecurityTier::S2);
    j.sensitivity = Some(Sensitivity::Sensitive);
    assert_rejected_with(
        c,
        &j,
        RejectionReason::ThirdPartySensitiveDataForbiddenOnRestrictedIsolation,
    );
}

#[test]
fn missing_candidate_fact_fails_closed() {
    let mut c = candidate("node-a");
    c.security_tier = None;
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::MissingFact(MissingFact::SecurityTier),
    );
}

#[test]
fn missing_job_fact_fails_closed() {
    let mut j = job();
    j.minimum_gpu_count = None;
    assert_rejected_with(
        candidate("node-a"),
        &j,
        RejectionReason::MissingFact(MissingFact::JobMinimumGpuCount),
    );
}

#[test]
fn missing_gpu_telemetry_fails_closed() {
    let mut c = candidate("node-a");
    let gpu_id = c.gpus.as_ref().unwrap()[0].gpu_id.clone();
    c.gpus.as_mut().unwrap()[0].healthy = None;
    assert_rejected_with(
        c,
        &job(),
        RejectionReason::MissingFact(MissingFact::GpuHealth { gpu_id }),
    );
}

#[test]
fn no_model_constraint_does_not_require_model_fact() {
    let mut j = job();
    j.allowed_gpu_models.clear();
    let mut c = candidate("node-a");
    c.gpus.as_mut().unwrap()[0].model = None;
    assert_eligible(c, &j);
}

#[test]
fn zero_one_and_multiple_eligible_candidates_are_distinguished_without_ranking() {
    let j = job();
    let policy = Policy {
        maximum_snapshot_age_ms: MAX_AGE,
    };
    let none = evaluate_eligibility(
        &PoolSnapshot {
            evaluated_at_unix_ms: NOW,
            candidates: vec![],
        },
        &j,
        &policy,
    );
    assert_eq!(none.resolution, EligibilityResolution::NoEligibleCandidates);

    let one = evaluate_eligibility(
        &PoolSnapshot {
            evaluated_at_unix_ms: NOW,
            candidates: vec![candidate("node-a")],
        },
        &j,
        &policy,
    );
    assert_eq!(
        one.resolution,
        EligibilityResolution::SingleEligible {
            node_id: "node-a".into()
        }
    );

    let multiple = evaluate_eligibility(
        &PoolSnapshot {
            evaluated_at_unix_ms: NOW,
            candidates: vec![candidate("node-b"), candidate("node-a")],
        },
        &j,
        &policy,
    );
    assert_eq!(multiple.resolution, EligibilityResolution::RankingRequired);
    assert_eq!(
        multiple
            .eligible
            .iter()
            .map(|c| c.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["node-a", "node-b"]
    );
}

#[test]
fn candidate_and_reason_order_is_independent_of_input_order() {
    let j = job();
    let policy = Policy {
        maximum_snapshot_age_ms: MAX_AGE,
    };
    let mut a = candidate("node-c");
    a.node_state = Some(NodeState::Offline);
    a.risk_state = Some(RiskState::Quarantined);
    a.available_cpu_cores = Some(0);
    let b = candidate("node-a");
    let mut c = candidate("node-b");
    c.key_protection = Some(KeyProtection::K0);

    let forward = evaluate_eligibility(
        &PoolSnapshot {
            evaluated_at_unix_ms: NOW,
            candidates: vec![a.clone(), b.clone(), c.clone()],
        },
        &j,
        &policy,
    );
    let reverse = evaluate_eligibility(
        &PoolSnapshot {
            evaluated_at_unix_ms: NOW,
            candidates: vec![c, b, a],
        },
        &j,
        &policy,
    );
    assert_eq!(forward, reverse);
    assert_eq!(
        format!("{forward:?}").as_bytes(),
        format!("{reverse:?}").as_bytes()
    );
    assert!(forward.rejected[1]
        .reasons
        .windows(2)
        .all(|pair| pair[0] <= pair[1]));
}

#[test]
fn duplicate_node_ids_do_not_reintroduce_input_order_dependence() {
    let j = job();
    let policy = Policy {
        maximum_snapshot_age_ms: MAX_AGE,
    };
    let mut first = candidate("same-node");
    first.node_state = Some(NodeState::Offline);
    let mut second = candidate("same-node");
    second.risk_state = Some(RiskState::Suspect);
    let forward = evaluate_eligibility(
        &PoolSnapshot {
            evaluated_at_unix_ms: NOW,
            candidates: vec![first.clone(), second.clone()],
        },
        &j,
        &policy,
    );
    let reverse = evaluate_eligibility(
        &PoolSnapshot {
            evaluated_at_unix_ms: NOW,
            candidates: vec![second, first],
        },
        &j,
        &policy,
    );
    assert_eq!(forward, reverse);
}
