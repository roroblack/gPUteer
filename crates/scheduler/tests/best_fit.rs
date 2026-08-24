use std::collections::BTreeSet;

use gputeer_scheduler::{
    evaluate_eligibility, rank_best_fit, resource_fit, BestFitPolicy, CandidateSnapshot,
    EligibilityResolution, FitAxis, GpuSnapshot, IsolationClass, JobRequirements, KeyProtection,
    MissingFact, NodeState, Policy, PoolSnapshot, RankingError, RiskState, SecurityTier, Sensitivity,
    SideEffectClass, WorkloadClass,
};

const NOW: u64 = 10_000;
const MAX_AGE: u64 = 100;
const REQUIRED_VRAM: u64 = 8;
const REQUIRED_RAM: u64 = 32;
const REQUIRED_WORKSPACE: u64 = 100;

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
        minimum_vram_bytes_per_gpu: Some(REQUIRED_VRAM),
        allowed_gpu_models: vec!["GPU".into()],
        cpu_cores: Some(8),
        ram_bytes: Some(REQUIRED_RAM),
        workspace_bytes: Some(REQUIRED_WORKSPACE),
    }
}

fn gpu(node_id: &str, suffix: usize, available_vram_bytes: u64) -> GpuSnapshot {
    GpuSnapshot {
        gpu_id: format!("{node_id}-gpu-{suffix}"),
        healthy: Some(true),
        available_vram_bytes: Some(available_vram_bytes),
        model: Some("GPU".into()),
    }
}

fn candidate(node_id: &str) -> CandidateSnapshot {
    CandidateSnapshot {
        node_id: node_id.into(),
        inventory_revision: Some(1),
        owner_member_id: Some("owner-a".into()),
        node_state: Some(NodeState::Online),
        risk_state: Some(RiskState::Normal),
        observed_at_unix_ms: Some(NOW),
        security_tier: Some(SecurityTier::S2),
        isolation_class: Some(IsolationClass::Contained),
        key_protection: Some(KeyProtection::K1),
        gpus: Some(vec![gpu(node_id, 0, REQUIRED_VRAM)]),
        available_cpu_cores: Some(8),
        available_ram_bytes: Some(REQUIRED_RAM),
        available_workspace_bytes: Some(REQUIRED_WORKSPACE),
        allowed_workload_classes: Some(BTreeSet::from([WorkloadClass::Training])),
        third_party_workloads_opt_in: None,
    }
}

fn hard_policy() -> Policy {
    Policy { maximum_snapshot_age_ms: MAX_AGE }
}

fn policy(first: FitAxis) -> BestFitPolicy {
    let axis_order = match first {
        FitAxis::Vram => [
            FitAxis::Vram,
            FitAxis::GpuCount,
            FitAxis::Cpu,
            FitAxis::Ram,
            FitAxis::Workspace,
        ],
        FitAxis::GpuCount => [
            FitAxis::GpuCount,
            FitAxis::Vram,
            FitAxis::Cpu,
            FitAxis::Ram,
            FitAxis::Workspace,
        ],
        FitAxis::Cpu => [
            FitAxis::Cpu,
            FitAxis::Vram,
            FitAxis::GpuCount,
            FitAxis::Ram,
            FitAxis::Workspace,
        ],
        FitAxis::Ram => [
            FitAxis::Ram,
            FitAxis::Vram,
            FitAxis::GpuCount,
            FitAxis::Cpu,
            FitAxis::Workspace,
        ],
        FitAxis::Workspace => [
            FitAxis::Workspace,
            FitAxis::Vram,
            FitAxis::GpuCount,
            FitAxis::Cpu,
            FitAxis::Ram,
        ],
    };
    BestFitPolicy { axis_order }
}

fn pool(candidates: Vec<CandidateSnapshot>) -> PoolSnapshot {
    PoolSnapshot { evaluated_at_unix_ms: NOW, candidates }
}

fn report(pool: &PoolSnapshot, job: &JobRequirements) -> gputeer_scheduler::EligibilityReport {
    evaluate_eligibility(pool, job, &hard_policy())
}

fn winner(pool: &PoolSnapshot, job: &JobRequirements, first: FitAxis) -> String {
    let report = report(pool, job);
    rank_best_fit(pool, job, &report, &policy(first)).unwrap().winner.node_id
}

#[test]
fn every_resource_axis_prefers_the_smaller_non_negative_remainder() {
    for axis in [
        FitAxis::Vram,
        FitAxis::GpuCount,
        FitAxis::Cpu,
        FitAxis::Ram,
        FitAxis::Workspace,
    ] {
        let tight = candidate("tight");
        let mut loose = candidate("loose");
        match axis {
            FitAxis::Vram => loose.gpus.as_mut().unwrap()[0].available_vram_bytes = Some(9),
            FitAxis::GpuCount => loose.gpus.as_mut().unwrap().push(gpu("loose", 1, 8)),
            FitAxis::Cpu => loose.available_cpu_cores = Some(9),
            FitAxis::Ram => loose.available_ram_bytes = Some(REQUIRED_RAM + 1),
            FitAxis::Workspace => {
                loose.available_workspace_bytes = Some(REQUIRED_WORKSPACE + 1);
            }
        }
        let input = pool(vec![loose, tight]);
        assert_eq!(winner(&input, &job(), axis), "tight", "axis {axis:?}");
    }
}

#[test]
fn policy_axis_order_changes_which_tradeoff_wins() {
    let mut vram_tight = candidate("vram-tight");
    vram_tight.available_cpu_cores = Some(12);
    let mut cpu_tight = candidate("cpu-tight");
    cpu_tight.gpus.as_mut().unwrap()[0].available_vram_bytes = Some(12);
    let input = pool(vec![cpu_tight, vram_tight]);

    assert_eq!(winner(&input, &job(), FitAxis::Vram), "vram-tight");
    assert_eq!(winner(&input, &job(), FitAxis::Cpu), "cpu-tight");
}

#[test]
fn gpu_subset_uses_the_tightest_adequate_required_count() {
    let mut j = job();
    j.minimum_gpu_count = Some(2);
    let mut a = candidate("node-a");
    a.gpus = Some(vec![gpu("node-a", 0, 30), gpu("node-a", 1, 10), gpu("node-a", 2, 12)]);
    let mut b = candidate("node-b");
    b.gpus = Some(vec![gpu("node-b", 0, 11), gpu("node-b", 1, 12)]);
    let input = pool(vec![a, b]);
    let actual = rank_best_fit(&input, &j, &report(&input, &j), &policy(FitAxis::Vram)).unwrap();

    assert_eq!(actual.winner.node_id, "node-a");
    assert_eq!(actual.winner.fit_key.vram_remaining_bytes, 6);
    assert_eq!(actual.winner.fit_key.gpu_count_remaining, 1);
    assert_eq!(
        actual.winner.selected_gpu_ids,
        ["node-a-gpu-1", "node-a-gpu-2"]
    );
}

#[test]
fn resource_fit_selects_tight_vram_and_returns_canonical_gpu_ids() {
    let mut j = job();
    j.minimum_gpu_count = Some(2);
    let mut node = candidate("node-a");
    node.gpus = Some(vec![
        GpuSnapshot {
            gpu_id: "gpu-z".into(),
            healthy: Some(true),
            available_vram_bytes: Some(9),
            model: Some("GPU".into()),
        },
        GpuSnapshot {
            gpu_id: "gpu-a".into(),
            healthy: Some(true),
            available_vram_bytes: Some(10),
            model: Some("GPU".into()),
        },
        GpuSnapshot {
            gpu_id: "gpu-unused".into(),
            healthy: Some(true),
            available_vram_bytes: Some(20),
            model: Some("GPU".into()),
        },
    ]);

    let actual = resource_fit(&node, &j).unwrap();

    assert_eq!(actual.fit_key.vram_remaining_bytes, 3);
    assert_eq!(actual.fit_key.gpu_count_remaining, 1);
    assert_eq!(actual.selected_gpu_ids, ["gpu-a", "gpu-z"]);
}

#[test]
fn equal_vram_gpu_tie_break_and_assignment_ignore_input_order() {
    let mut forward = candidate("node-a");
    forward.gpus = Some(vec![
        GpuSnapshot {
            gpu_id: "gpu-z".into(),
            healthy: Some(true),
            available_vram_bytes: Some(10),
            model: Some("GPU".into()),
        },
        GpuSnapshot {
            gpu_id: "gpu-a".into(),
            healthy: Some(true),
            available_vram_bytes: Some(10),
            model: Some("GPU".into()),
        },
    ]);
    let mut reversed = forward.clone();
    reversed.gpus.as_mut().unwrap().reverse();

    let forward_fit = resource_fit(&forward, &job()).unwrap();
    let reversed_fit = resource_fit(&reversed, &job()).unwrap();

    assert_eq!(forward_fit, reversed_fit);
    assert_eq!(forward_fit.selected_gpu_ids, ["gpu-a"]);
}

#[test]
fn assignment_excludes_unhealthy_disallowed_and_too_small_gpus() {
    let mut node = candidate("node-a");
    node.gpus = Some(vec![
        GpuSnapshot {
            gpu_id: "gpu-unhealthy".into(),
            healthy: Some(false),
            available_vram_bytes: Some(REQUIRED_VRAM),
            model: Some("GPU".into()),
        },
        GpuSnapshot {
            gpu_id: "gpu-disallowed".into(),
            healthy: Some(true),
            available_vram_bytes: Some(REQUIRED_VRAM),
            model: Some("OTHER".into()),
        },
        GpuSnapshot {
            gpu_id: "gpu-too-small".into(),
            healthy: Some(true),
            available_vram_bytes: Some(REQUIRED_VRAM - 1),
            model: Some("GPU".into()),
        },
        GpuSnapshot {
            gpu_id: "gpu-selected".into(),
            healthy: Some(true),
            available_vram_bytes: Some(REQUIRED_VRAM + 1),
            model: Some("GPU".into()),
        },
    ]);

    let actual = resource_fit(&node, &job()).unwrap();

    assert_eq!(actual.selected_gpu_ids, ["gpu-selected"]);
    assert_eq!(actual.fit_key.vram_remaining_bytes, 1);
    assert_eq!(actual.fit_key.gpu_count_remaining, 0);
}

#[test]
fn gpu_inventory_permutations_produce_identical_ranking() {
    let mut j = job();
    j.minimum_gpu_count = Some(2);
    let mut a = candidate("node-a");
    a.gpus = Some(vec![gpu("node-a", 0, 30), gpu("node-a", 1, 10), gpu("node-a", 2, 12)]);
    let mut b = candidate("node-b");
    b.gpus = Some(vec![gpu("node-b", 0, 11), gpu("node-b", 1, 12)]);
    let forward_pool = pool(vec![a, b]);
    let mut reversed_candidates = forward_pool.candidates.clone();
    for candidate in &mut reversed_candidates {
        candidate.gpus.as_mut().unwrap().reverse();
    }
    let reversed_pool = pool(reversed_candidates);

    let forward = rank_best_fit(
        &forward_pool,
        &j,
        &report(&forward_pool, &j),
        &policy(FitAxis::Vram),
    )
    .unwrap();
    let reversed = rank_best_fit(
        &reversed_pool,
        &j,
        &report(&reversed_pool, &j),
        &policy(FitAxis::Vram),
    )
    .unwrap();

    assert_eq!(forward, reversed);
    assert_eq!(
        forward.winner.selected_gpu_ids,
        ["node-a-gpu-1", "node-a-gpu-2"]
    );
}

#[test]
fn complete_tie_is_broken_only_by_node_id_ascending() {
    let input = pool(vec![candidate("node-z"), candidate("node-a"), candidate("node-m")]);
    let actual = rank_best_fit(
        &input,
        &job(),
        &report(&input, &job()),
        &policy(FitAxis::Workspace),
    )
    .unwrap();

    assert_eq!(actual.winner.node_id, "node-a");
    assert_eq!(
        actual.ranked.iter().map(|entry| entry.node_id.as_str()).collect::<Vec<_>>(),
        vec!["node-a", "node-m", "node-z"]
    );
}

#[test]
fn pool_and_report_permutations_produce_byte_equal_debug_output() {
    let mut a = candidate("node-a");
    a.available_cpu_cores = Some(9);
    let mut b = candidate("node-b");
    b.available_cpu_cores = Some(10);
    let c = candidate("node-c");
    let forward_pool = pool(vec![a.clone(), b.clone(), c.clone()]);
    let reverse_pool = pool(vec![c, b, a]);
    let forward_report = report(&forward_pool, &job());
    let mut reverse_report = report(&reverse_pool, &job());
    reverse_report.eligible.reverse();

    let forward = rank_best_fit(&forward_pool, &job(), &forward_report, &policy(FitAxis::Cpu))
        .unwrap();
    let reverse = rank_best_fit(&reverse_pool, &job(), &reverse_report, &policy(FitAxis::Cpu))
        .unwrap();
    assert_eq!(forward, reverse);
    assert_eq!(format!("{forward:?}").as_bytes(), format!("{reverse:?}").as_bytes());
}

#[test]
fn hard_filter_rejections_never_enter_the_ranking() {
    let a = candidate("node-a");
    let b = candidate("node-b");
    let mut rejected = candidate("node-rejected");
    rejected.node_state = Some(NodeState::Offline);
    let input = pool(vec![rejected, b, a]);
    let eligibility = report(&input, &job());
    let actual = rank_best_fit(&input, &job(), &eligibility, &policy(FitAxis::Vram)).unwrap();

    assert_eq!(actual.ranked.len(), 2);
    assert!(actual.ranked.iter().all(|entry| entry.node_id != "node-rejected"));
}

#[test]
fn zero_or_one_candidate_report_is_not_misrepresented_as_a_ranking() {
    for candidates in [vec![], vec![candidate("node-a")]] {
        let input = pool(candidates);
        let error = rank_best_fit(&input, &job(), &report(&input, &job()), &policy(FitAxis::Vram))
            .unwrap_err();
        assert_eq!(error, RankingError::ResolutionNotRankingRequired);
    }

    let input = pool(vec![candidate("node-a")]);
    let mut malformed = report(&input, &job());
    malformed.resolution = EligibilityResolution::RankingRequired;
    assert_eq!(
        rank_best_fit(&input, &job(), &malformed, &policy(FitAxis::Vram)).unwrap_err(),
        RankingError::EligibleCandidateCountNotMultiple { actual: 1 }
    );
}

#[test]
fn duplicate_pool_and_report_ids_fail_closed() {
    let duplicate_pool = pool(vec![candidate("same"), candidate("same")]);
    let duplicate_pool_report = report(&duplicate_pool, &job());
    assert_eq!(
        rank_best_fit(
            &duplicate_pool,
            &job(),
            &duplicate_pool_report,
            &policy(FitAxis::Vram),
        )
        .unwrap_err(),
        RankingError::DuplicatePoolNodeId { node_id: "same".into() }
    );

    let input = pool(vec![candidate("node-a"), candidate("node-b")]);
    let mut duplicate_report = report(&input, &job());
    duplicate_report.eligible.push(duplicate_report.eligible[0].clone());
    assert_eq!(
        rank_best_fit(&input, &job(), &duplicate_report, &policy(FitAxis::Vram)).unwrap_err(),
        RankingError::DuplicateReportNodeId { node_id: "node-a".into() }
    );
}

#[test]
fn report_only_and_pool_only_candidate_ids_fail_closed() {
    let input = pool(vec![candidate("node-a"), candidate("node-b"), candidate("node-c")]);
    let mut report_only = report(&input, &job());
    report_only.eligible[0].node_id = "node-x".into();
    assert_eq!(
        rank_best_fit(&input, &job(), &report_only, &policy(FitAxis::Vram)).unwrap_err(),
        RankingError::ReportCandidateMissingFromPool { node_id: "node-x".into() }
    );

    let mut pool_only = report(&input, &job());
    pool_only.eligible.pop();
    assert_eq!(
        rank_best_fit(&input, &job(), &pool_only, &policy(FitAxis::Vram)).unwrap_err(),
        RankingError::PoolCandidateMissingFromReport { node_id: "node-c".into() }
    );
}

#[test]
fn duplicate_policy_axis_fails_closed() {
    let input = pool(vec![candidate("node-a"), candidate("node-b")]);
    let invalid = BestFitPolicy {
        axis_order: [FitAxis::Vram, FitAxis::Vram, FitAxis::Cpu, FitAxis::Ram, FitAxis::Workspace],
    };
    assert_eq!(
        rank_best_fit(&input, &job(), &report(&input, &job()), &invalid).unwrap_err(),
        RankingError::InvalidPolicyAxisOrder
    );
}

#[test]
fn missing_job_and_candidate_rank_facts_fail_closed() {
    let input = pool(vec![candidate("node-a"), candidate("node-b")]);
    let eligibility = report(&input, &job());
    let mut missing_job = job();
    missing_job.cpu_cores = None;
    assert_eq!(
        rank_best_fit(&input, &missing_job, &eligibility, &policy(FitAxis::Cpu)).unwrap_err(),
        RankingError::MissingRankFact { node_id: None, fact: MissingFact::JobCpu }
    );

    let mut candidates = vec![candidate("node-a"), candidate("node-b")];
    let original_pool = pool(candidates.clone());
    let eligibility = report(&original_pool, &job());
    candidates[0].available_cpu_cores = None;
    let missing_candidate = pool(candidates);
    assert_eq!(
        rank_best_fit(&missing_candidate, &job(), &eligibility, &policy(FitAxis::Cpu)).unwrap_err(),
        RankingError::MissingRankFact {
            node_id: Some("node-a".into()),
            fact: MissingFact::AvailableCpu,
        }
    );
}

#[test]
fn empty_and_duplicate_gpu_ids_fail_closed_with_typed_errors() {
    let mut empty = candidate("node-a");
    empty.gpus.as_mut().unwrap()[0].gpu_id = "   ".into();
    assert_eq!(
        resource_fit(&empty, &job()).unwrap_err(),
        RankingError::EmptyGpuId {
            node_id: "node-a".into()
        }
    );

    let mut duplicate = candidate("node-b");
    duplicate.gpus.as_mut().unwrap().push(gpu("node-b", 0, REQUIRED_VRAM + 1));
    assert_eq!(
        resource_fit(&duplicate, &job()).unwrap_err(),
        RankingError::DuplicateGpuId {
            node_id: "node-b".into(),
            gpu_id: "node-b-gpu-0".into(),
        }
    );
}

#[test]
fn missing_gpu_health_model_and_vram_remain_fail_closed() {
    let cases = [
        (
            {
                let mut value = candidate("node-health");
                value.gpus.as_mut().unwrap()[0].healthy = None;
                value
            },
            MissingFact::GpuHealth {
                gpu_id: "node-health-gpu-0".into(),
            },
        ),
        (
            {
                let mut value = candidate("node-model");
                value.gpus.as_mut().unwrap()[0].model = None;
                value
            },
            MissingFact::GpuModel {
                gpu_id: "node-model-gpu-0".into(),
            },
        ),
        (
            {
                let mut value = candidate("node-vram");
                value.gpus.as_mut().unwrap()[0].available_vram_bytes = None;
                value
            },
            MissingFact::GpuVram {
                gpu_id: "node-vram-gpu-0".into(),
            },
        ),
    ];

    for (candidate, fact) in cases {
        assert_eq!(
            resource_fit(&candidate, &job()).unwrap_err(),
            RankingError::MissingRankFact {
                node_id: Some(candidate.node_id),
                fact,
            }
        );
    }
}

#[test]
fn report_from_resource_incompatible_snapshot_fails_closed() {
    let original = pool(vec![candidate("node-a"), candidate("node-b")]);
    let eligibility = report(&original, &job());
    let mut changed_candidates = original.candidates.clone();
    changed_candidates[0].gpus.as_mut().unwrap()[0].available_vram_bytes = Some(REQUIRED_VRAM - 1);
    let changed = pool(changed_candidates);

    assert_eq!(
        rank_best_fit(&changed, &job(), &eligibility, &policy(FitAxis::Vram)).unwrap_err(),
        RankingError::EligibleCandidateMismatch {
            node_id: "node-a".into(),
            axis: FitAxis::GpuCount,
        }
    );
}

#[test]
fn stale_report_cannot_underflow_a_non_gpu_resource_remainder() {
    let original = pool(vec![candidate("node-a"), candidate("node-b")]);
    let eligibility = report(&original, &job());
    let mut changed_candidates = original.candidates.clone();
    changed_candidates[0].available_cpu_cores = Some(7);
    let changed = pool(changed_candidates);

    assert_eq!(
        rank_best_fit(&changed, &job(), &eligibility, &policy(FitAxis::Cpu)).unwrap_err(),
        RankingError::EligibleCandidateMismatch {
            node_id: "node-a".into(),
            axis: FitAxis::Cpu,
        }
    );
}

#[test]
fn vram_remainder_overflow_fails_closed() {
    let mut j = job();
    j.minimum_gpu_count = Some(2);
    j.minimum_vram_bytes_per_gpu = Some(0);
    let mut a = candidate("node-a");
    a.gpus = Some(vec![gpu("node-a", 0, u64::MAX), gpu("node-a", 1, u64::MAX)]);
    let mut b = candidate("node-b");
    b.gpus = Some(vec![gpu("node-b", 0, 1), gpu("node-b", 1, 1)]);
    let input = pool(vec![a, b]);

    assert_eq!(
        rank_best_fit(&input, &j, &report(&input, &j), &policy(FitAxis::Vram)).unwrap_err(),
        RankingError::FitOverflow { node_id: "node-a".into(), axis: FitAxis::Vram }
    );
}
