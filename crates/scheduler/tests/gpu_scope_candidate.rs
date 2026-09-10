use std::collections::BTreeSet;

use gputeer_scheduler::{
    gpu_scope_candidate, AvailableVramObservation, GpuAllocationMode, GpuObservationSnapshot,
    JobGpuRequirements, ProvenanceGate, ScopeCandidate, ScopeError, ScopeGpuObservation,
    ScopeMissingFact, ScopeResourceInput,
};

fn gpu(gpu_id: &str, available_vram_bytes: u64) -> ScopeGpuObservation {
    ScopeGpuObservation {
        gpu_id: gpu_id.into(),
        healthy: Some(true),
        available_vram: Some(AvailableVramObservation::Authoritative {
            bytes: available_vram_bytes,
        }),
        model: Some("NVIDIA GeForce RTX 4070 SUPER".into()),
        driver_version: Some(595),
        compute_capability: Some("8.9".into()),
        allocation_modes: Some([GpuAllocationMode::Exclusive].into_iter().collect()),
    }
}

fn snapshot(gpus: Vec<ScopeGpuObservation>) -> GpuObservationSnapshot {
    GpuObservationSnapshot {
        observed_at_unix_ms: Some(1_777_000_000_000),
        inventory_revision: Some(42),
        gpus: Some(gpus),
    }
}

fn requirements(count: u32, vram: u64) -> JobGpuRequirements {
    JobGpuRequirements {
        minimum_vram_bytes_per_gpu: Some(vram),
        minimum_gpu_count: Some(count),
        minimum_driver_version: Some(550),
        cuda_runtime_version: None,
        allowed_compute_capabilities: vec!["8.9".into()],
        allocation_mode: Some(GpuAllocationMode::Exclusive),
        allowed_gpu_models: vec!["NVIDIA GeForce RTX 4070 SUPER".into()],
    }
}

fn resources() -> ScopeResourceInput {
    ScopeResourceInput {
        cpu_cores: Some(8),
        ram_bytes: Some(32 * 1024 * 1024 * 1024),
        workspace_bytes: Some(100 * 1024 * 1024 * 1024),
        writable_prefixes: Some(vec![
            "jobs/job-1/checkpoints/".into(),
            "jobs/job-1/attempt-1/".into(),
        ]),
    }
}

fn evaluate(
    snapshot: &GpuObservationSnapshot,
    requirements: &JobGpuRequirements,
    resources: &ScopeResourceInput,
) -> Result<ScopeCandidate, ScopeError> {
    gpu_scope_candidate(snapshot, requirements, resources, ProvenanceGate::Verified)
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    fn visit<T: Clone>(remaining: Vec<T>, prefix: Vec<T>, output: &mut Vec<Vec<T>>) {
        if remaining.is_empty() {
            output.push(prefix);
            return;
        }
        for index in 0..remaining.len() {
            let mut next_remaining = remaining.clone();
            let item = next_remaining.remove(index);
            let mut next_prefix = prefix.clone();
            next_prefix.push(item);
            visit(next_remaining, next_prefix, output);
        }
    }

    let mut output = Vec::new();
    visit(items.to_vec(), Vec::new(), &mut output);
    output
}

#[test]
fn candidate_preserves_explicit_scope_values_and_selects_tight_gpus() {
    let input = snapshot(vec![
        gpu("gpu-z", 16_000),
        gpu("gpu-b", 12_000),
        gpu("gpu-a", 12_000),
    ]);
    let mut resource_input = resources();
    resource_input.writable_prefixes.as_mut().unwrap().reverse();

    assert_eq!(
        evaluate(&input, &requirements(2, 10_000), &resource_input),
        Ok(ScopeCandidate {
            observed_at_unix_ms: 1_777_000_000_000,
            inventory_revision: 42,
            selected_gpu_ids: vec!["gpu-a".into(), "gpu-b".into()],
            allocation_mode: GpuAllocationMode::Exclusive,
            cpu_cores: 8,
            ram_bytes: 32 * 1024 * 1024 * 1024,
            workspace_bytes: 100 * 1024 * 1024 * 1024,
            writable_prefixes: vec![
                "jobs/job-1/attempt-1/".into(),
                "jobs/job-1/checkpoints/".into(),
            ],
        })
    );
}

#[test]
fn all_four_gpu_input_permutations_produce_the_identical_candidate() {
    let inputs = vec![
        gpu("gpu-d", 40_000),
        gpu("gpu-b", 20_000),
        gpu("gpu-a", 20_000),
        gpu("gpu-c", 30_000),
    ];
    let expected = evaluate(
        &snapshot(inputs.clone()),
        &requirements(2, 10_000),
        &resources(),
    )
    .unwrap();

    for permutation in permutations(&inputs) {
        assert_eq!(
            evaluate(
                &snapshot(permutation),
                &requirements(2, 10_000),
                &resources(),
            )
            .unwrap(),
            expected
        );
    }
}

#[test]
fn requirement_allowlist_and_prefix_orders_do_not_change_the_candidate() {
    let input = snapshot(vec![gpu("gpu-a", 20_000)]);
    let mut forward_requirements = requirements(1, 10_000);
    forward_requirements
        .allowed_gpu_models
        .push("unused-model".into());
    forward_requirements
        .allowed_compute_capabilities
        .push("9.0".into());
    let mut reverse_requirements = forward_requirements.clone();
    reverse_requirements.allowed_gpu_models.reverse();
    reverse_requirements.allowed_compute_capabilities.reverse();
    let forward_resources = resources();
    let mut reverse_resources = forward_resources.clone();
    reverse_resources
        .writable_prefixes
        .as_mut()
        .unwrap()
        .reverse();

    assert_eq!(
        evaluate(&input, &forward_requirements, &forward_resources),
        evaluate(&input, &reverse_requirements, &reverse_resources)
    );
}

#[test]
fn unverified_provenance_is_an_explicit_gate_not_a_kernel_inference() {
    assert_eq!(
        gpu_scope_candidate(
            &snapshot(vec![gpu("gpu-a", 10_000)]),
            &requirements(1, 10_000),
            &resources(),
            ProvenanceGate::Unverified,
        ),
        Err(ScopeError::UnverifiedProvenance)
    );
}

#[test]
fn measured_rtx_fixture_does_not_invent_missing_time_health_or_available_vram() {
    let mib = 1024 * 1024;
    let measured = ScopeGpuObservation {
        gpu_id: "GPU-09a269a7-50a8-f5be-2a00-d20a1c281c93".into(),
        healthy: None,
        available_vram: Some(AvailableVramObservation::DerivedFromTotalAndReserved {
            total_bytes: 12_282 * mib,
            reserved_bytes: 283 * mib,
        }),
        model: Some("NVIDIA GeForce RTX 4070 SUPER".into()),
        driver_version: Some(595),
        compute_capability: Some("8.9".into()),
        allocation_modes: Some([GpuAllocationMode::Exclusive].into_iter().collect()),
    };
    let mut input = GpuObservationSnapshot {
        observed_at_unix_ms: None,
        inventory_revision: None,
        gpus: Some(vec![measured]),
    };

    assert_eq!(
        evaluate(&input, &requirements(1, 1), &resources()),
        Err(ScopeError::MissingFact(ScopeMissingFact::ObservedAt))
    );
    input.observed_at_unix_ms = Some(1);
    assert_eq!(
        evaluate(&input, &requirements(1, 1), &resources()),
        Err(ScopeError::MissingFact(ScopeMissingFact::InventoryRevision))
    );
    input.inventory_revision = Some(1);
    assert_eq!(
        evaluate(&input, &requirements(1, 1), &resources()),
        Err(ScopeError::MissingFact(ScopeMissingFact::GpuHealth {
            gpu_id: "GPU-09a269a7-50a8-f5be-2a00-d20a1c281c93".into(),
        }))
    );
    input.gpus.as_mut().unwrap()[0].healthy = Some(true);
    assert_eq!(
        evaluate(&input, &requirements(1, 1), &resources()),
        Err(ScopeError::NonAuthoritativeAvailableVram {
            gpu_id: "GPU-09a269a7-50a8-f5be-2a00-d20a1c281c93".into(),
        })
    );
}

#[test]
fn missing_required_gpu_facts_fail_closed_only_when_the_constraint_needs_them() {
    let mut input = snapshot(vec![gpu("gpu-a", 10_000)]);
    input.gpus.as_mut().unwrap()[0].healthy = None;
    assert_eq!(
        evaluate(&input, &requirements(1, 1), &resources()),
        Err(ScopeError::MissingFact(ScopeMissingFact::GpuHealth {
            gpu_id: "gpu-a".into(),
        }))
    );

    let cases = [
        (
            "model",
            ScopeMissingFact::GpuModel {
                gpu_id: "gpu-a".into(),
            },
        ),
        (
            "driver",
            ScopeMissingFact::GpuDriverVersion {
                gpu_id: "gpu-a".into(),
            },
        ),
        (
            "compute",
            ScopeMissingFact::GpuComputeCapability {
                gpu_id: "gpu-a".into(),
            },
        ),
        (
            "allocation",
            ScopeMissingFact::GpuAllocationModes {
                gpu_id: "gpu-a".into(),
            },
        ),
        (
            "vram",
            ScopeMissingFact::AuthoritativeAvailableVram {
                gpu_id: "gpu-a".into(),
            },
        ),
    ];
    for (field, expected) in cases {
        let mut input = snapshot(vec![gpu("gpu-a", 10_000)]);
        let gpu = &mut input.gpus.as_mut().unwrap()[0];
        match field {
            "model" => gpu.model = None,
            "driver" => gpu.driver_version = None,
            "compute" => gpu.compute_capability = None,
            "allocation" => gpu.allocation_modes = None,
            "vram" => gpu.available_vram = None,
            _ => unreachable!(),
        }
        assert_eq!(
            evaluate(&input, &requirements(1, 1), &resources()),
            Err(ScopeError::MissingFact(expected))
        );
    }

    let mut unconstrained = requirements(1, 1);
    unconstrained.minimum_driver_version = None;
    unconstrained.allowed_compute_capabilities.clear();
    unconstrained.allowed_gpu_models.clear();
    let mut input = snapshot(vec![gpu("gpu-a", 10_000)]);
    let gpu = &mut input.gpus.as_mut().unwrap()[0];
    gpu.model = None;
    gpu.driver_version = None;
    gpu.compute_capability = None;
    assert!(evaluate(&input, &unconstrained, &resources()).is_ok());
}

#[test]
fn missing_explicit_resource_values_never_receive_defaults() {
    let cases = [
        ("cpu", ScopeMissingFact::CpuCores),
        ("ram", ScopeMissingFact::RamBytes),
        ("workspace", ScopeMissingFact::WorkspaceBytes),
        ("prefixes", ScopeMissingFact::WritablePrefixes),
    ];
    for (field, expected) in cases {
        let mut input = resources();
        match field {
            "cpu" => input.cpu_cores = None,
            "ram" => input.ram_bytes = None,
            "workspace" => input.workspace_bytes = None,
            "prefixes" => input.writable_prefixes = None,
            _ => unreachable!(),
        }
        assert_eq!(
            evaluate(
                &snapshot(vec![gpu("gpu-a", 10_000)]),
                &requirements(1, 1),
                &input,
            ),
            Err(ScopeError::MissingFact(expected))
        );
    }
}

#[test]
fn missing_or_zero_gpu_requirements_fail_closed_without_proto_defaults() {
    let input = snapshot(vec![gpu("gpu-a", 10_000)]);
    let mut required = requirements(1, 1);
    required.minimum_vram_bytes_per_gpu = None;
    assert_eq!(
        evaluate(&input, &required, &resources()),
        Err(ScopeError::MissingFact(ScopeMissingFact::JobMinimumVram))
    );

    let mut required = requirements(1, 1);
    required.minimum_gpu_count = None;
    assert_eq!(
        evaluate(&input, &required, &resources()),
        Err(ScopeError::MissingFact(
            ScopeMissingFact::JobMinimumGpuCount
        ))
    );

    let mut required = requirements(0, 1);
    assert_eq!(
        evaluate(&input, &required, &resources()),
        Err(ScopeError::MinimumGpuCountMustBePositive)
    );
    required.minimum_gpu_count = Some(1);
    required.allocation_mode = None;
    assert_eq!(
        evaluate(&input, &required, &resources()),
        Err(ScopeError::MissingFact(ScopeMissingFact::JobAllocationMode))
    );
}

#[test]
fn cuda_runtime_requirement_is_not_silently_ignored_or_given_invented_compatibility() {
    let input = snapshot(vec![gpu("gpu-a", 10_000)]);
    let mut required = requirements(1, 1);
    required.cuda_runtime_version = Some("12.4".into());

    assert_eq!(
        evaluate(&input, &required, &resources()),
        Err(ScopeError::CudaRuntimeCompatibilityUnresolved)
    );
}

#[test]
fn blank_noncanonical_and_duplicate_gpu_ids_are_typed_canonical_errors() {
    assert_eq!(
        evaluate(
            &snapshot(vec![gpu(" ", 10_000)]),
            &requirements(1, 1),
            &resources(),
        ),
        Err(ScopeError::EmptyGpuId)
    );
    assert_eq!(
        evaluate(
            &snapshot(vec![gpu("gpu-a ", 10_000)]),
            &requirements(1, 1),
            &resources(),
        ),
        Err(ScopeError::NonCanonicalGpuId {
            gpu_id: "gpu-a ".into()
        })
    );

    let duplicates = vec![gpu("gpu-a", 10_000), gpu("gpu-a", 20_000)];
    let forward = evaluate(
        &snapshot(duplicates.clone()),
        &requirements(1, 1),
        &resources(),
    );
    let reverse = evaluate(
        &snapshot(duplicates.into_iter().rev().collect()),
        &requirements(1, 1),
        &resources(),
    );
    assert_eq!(forward, reverse);
    assert_eq!(
        forward,
        Err(ScopeError::DuplicateGpuId {
            gpu_id: "gpu-a".into()
        })
    );
}

#[test]
fn partitioned_allocation_is_rejected_even_when_input_claims_mig_support() {
    let mut input = snapshot(vec![gpu("gpu-a", 10_000)]);
    input.gpus.as_mut().unwrap()[0]
        .allocation_modes
        .as_mut()
        .unwrap()
        .insert(GpuAllocationMode::Partitioned);
    let mut required = requirements(1, 1);
    required.allocation_mode = Some(GpuAllocationMode::Partitioned);

    assert_eq!(
        evaluate(&input, &required, &resources()),
        Err(ScopeError::PartitionedAllocationUnproven)
    );
}

#[test]
fn shared_allocation_is_rejected_even_when_input_claims_shared_support() {
    // ★ 2026-09-09 — 이 자리가 **비어 있었다.** 바로 위 `Partitioned` 는
    //   거부를 검사하는데 `Shared` 는 아무도 안 쟀다. 그래서 관문이 없다는
    //   것조차 아무 테스트도 알려주지 않았다 — 51건이 전부 통과하면서.
    //
    //   ★★ 이 거부는 규범보다 **좁다**(2026-09-10 재검수). proto 가 SHARED
    //     를 여는 조건은 "동일 소유자 Job 간, **또는** Linux+MPS 확인" 둘인데
    //     이 커널에는 둘 다 판단할 입력 칸이 없어서 전부 거부한다.
    //     (가) 동일 소유자 경로까지 막는 **잠정 제한**이다 — `scope.rs` 의
    //     `SharedAllocationUnproven` 주석 참조.
    //   ★ (나) 는 실측(2026-09-08~09, **x600 WSL2**)이 거기서 만족시킬 수
    //     없음을 보였다 — 그 구성에서 MPS 를 구동하지 못했고(재검수 14 —
    //     전에는 "불가능" 이라 적었다), 유저스페이스 가로채기는
    //     카운터가 프로세스 로컬이라 노드 단위 예산을 못 지킨다.
    //     네이티브 Linux + MPS 는 재지 않았다(재검수 12 가 일반화를 짚었다).
    let mut input = snapshot(vec![gpu("gpu-a", 10_000)]);
    input.gpus.as_mut().unwrap()[0]
        .allocation_modes
        .as_mut()
        .unwrap()
        .insert(GpuAllocationMode::Shared);
    let mut required = requirements(1, 1);
    required.allocation_mode = Some(GpuAllocationMode::Shared);

    assert_eq!(
        evaluate(&input, &required, &resources()),
        Err(ScopeError::SharedAllocationUnproven)
    );
}

#[test]
fn refusing_shared_does_not_refuse_exclusive() {
    // ★ 대조군. 이게 없으면 "전부 거부한다" 로 고쳐도 위 테스트가 통과한다.
    //   그리고 **거부가 아니라 성공을 확인해야** 한다 — `is_err()` 가
    //   아닌 것만 보면 다른 이유로 실패하는 것과 구분되지 않는다.
    let input = snapshot(vec![gpu("gpu-a", 10_000)]);
    let required = requirements(1, 1);

    let candidate = evaluate(&input, &required, &resources())
        .expect("Exclusive 는 통과해야 한다 — 통과 못 하면 관문이 아니라 고장이다");
    assert_eq!(
        candidate.selected_gpu_ids,
        vec!["gpu-a".to_string()],
        "통과했다면 실제로 그 GPU 를 골라야 한다"
    );
}

#[test]
fn unhealthy_or_incompatible_gpus_never_enter_the_candidate() {
    let mut unhealthy = gpu("gpu-unhealthy", 10_000);
    unhealthy.healthy = Some(false);
    let mut wrong_model = gpu("gpu-wrong-model", 10_000);
    wrong_model.model = Some("other".into());
    let mut old_driver = gpu("gpu-old-driver", 10_000);
    old_driver.driver_version = Some(549);
    let mut wrong_compute = gpu("gpu-wrong-compute", 10_000);
    wrong_compute.compute_capability = Some("8.6".into());
    let mut wrong_mode = gpu("gpu-wrong-mode", 10_000);
    wrong_mode.allocation_modes = Some(BTreeSet::new());
    let too_small = gpu("gpu-too-small", 9_999);
    let valid = gpu("gpu-valid", 10_000);

    let candidate = evaluate(
        &snapshot(vec![
            unhealthy,
            wrong_model,
            old_driver,
            wrong_compute,
            wrong_mode,
            too_small,
            valid,
        ]),
        &requirements(1, 10_000),
        &resources(),
    )
    .unwrap();
    assert_eq!(candidate.selected_gpu_ids, ["gpu-valid"]);
}

#[test]
fn exact_requirement_boundaries_pass_and_one_byte_short_fails() {
    let input = snapshot(vec![gpu("gpu-a", 10_000)]);
    assert!(evaluate(&input, &requirements(1, 10_000), &resources()).is_ok());
    assert_eq!(
        evaluate(&input, &requirements(1, 10_001), &resources()),
        Err(ScopeError::InsufficientMatchingGpus {
            matching: 0,
            required: 1
        })
    );
}

#[test]
fn writable_prefixes_are_canonicalized_and_malformed_values_fail_closed() {
    let input = snapshot(vec![gpu("gpu-a", 10_000)]);
    let required = requirements(1, 1);

    let mut empty = resources();
    empty.writable_prefixes = Some(vec![" ".into()]);
    assert_eq!(
        evaluate(&input, &required, &empty),
        Err(ScopeError::EmptyWritablePrefix)
    );

    let mut noncanonical = resources();
    noncanonical.writable_prefixes = Some(vec!["jobs/a/ ".into()]);
    assert_eq!(
        evaluate(&input, &required, &noncanonical),
        Err(ScopeError::NonCanonicalWritablePrefix {
            prefix: "jobs/a/ ".into()
        })
    );

    let mut duplicate = resources();
    duplicate.writable_prefixes = Some(vec!["jobs/a/".into(), "jobs/a/".into()]);
    assert_eq!(
        evaluate(&input, &required, &duplicate),
        Err(ScopeError::DuplicateWritablePrefix {
            prefix: "jobs/a/".into()
        })
    );

    let mut no_write_access = resources();
    no_write_access.writable_prefixes = Some(vec![]);
    assert_eq!(
        evaluate(&input, &required, &no_write_access)
            .unwrap()
            .writable_prefixes,
        Vec::<String>::new()
    );
}
