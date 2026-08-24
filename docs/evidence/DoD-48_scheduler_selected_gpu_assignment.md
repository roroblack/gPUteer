---
schema_version: 2
id: DoD-48
claim: "결정적 selected GPU assignment 순수 kernel 을 완료해, `DoD-46` 이 남긴 계약 불일치 3번(ranking 이 선택 GPU 식별자를 반환하지 않음)의 선행 작업을 닫았다. 순수 `resource_fit()`이 적격 GPU를 `(available_vram_bytes, gpu_id)` 오름차순으로 요구 개수만 선택하고 반환 ID를 `gpu_id` 오름차순으로 정규화하며, 단일/복수 후보가 같은 helper를 쓰고 `Staged` outcome이 ID를 보존해 STAGING 전에 개수를 재검증함을 구현·negative test·뮤테이션 2건·독립 검수 1라운드 ACCEPTED·감독자 scheduler 53/coordinator 87 passed로 확인했다. 단 반환 ID는 snapshot 식별자일 뿐 NVML UUID provenance가 아니며 Grant/Lease scope·GPU별 reservation/release·production wire는 범위 밖이다"
status: PASS
commit: 2ca2366da90c8687ece851a1be50daab9af6e90f

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — scheduler resource_fit/selected ID 보존, coordinator STAGING 전 재검증, 테스트와 뮤테이션 검증"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-24T10:48:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "resource_fit()의 candidate/job 입력만 쓰는 순수성, health→model→VRAM 적격 판정과 (available_vram_bytes, gpu_id) 결정 정렬·반환 ID 정규화, 단일/복수 후보의 동일 helper 사용과 별도 filter/sort 경로 부재, reverse 입력의 ResourceFit 전체와 BestFitRanking 전체 동등성, 자체 재검토로 추가한 부적격 GPU 세 경우 직접 제외, 빈/중복 ID와 개수 불일치 typed error, tie-break 제거와 selected ID 제거 뮤테이션 2건의 실제 경로 판별력, STAGING 전 SelectedGpuCountMismatch, scheduler 4개 파일+coordinator orchestrate.rs+문서 2개에 한정된 범위와 staging_store.rs·inventory_store.rs·job_store.rs 무변경, NVML UUID provenance·Grant/Lease scope·reservation/release·production wire 범위 밖을 확인해 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-48_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-48_scheduler_selected_gpu_assignment_2026-08-21.txt"
raw_output_digest: "sha256:32b1c86f58031fa57497f8c5b1fd7893d0895fd6ce999093ee1a3ad3d0eef9e7"
raw_output_bytes: 7134

binary_digests:
  toolchain: "cargo 사용 — 제공된 감독자 재실행 이력에 cargo/rustc version과 binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — scheduler process-local ResourceFit/RankedCandidate와 coordinator private orchestration outcome만 확장"
  canonical_spec: "wire canonical/signature 계약 미사용 — GrantedExecutionPlan·ResourceScope·ExecutionGrant 생성/서명/전송은 범위 밖"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test"
hardware: "GPU 미사용 — 합성 scheduler CandidateSnapshot으로 순수 선택 규칙과 coordinator 경계 검증"
network_profile: "네트워크 미사용 — scheduler/coordinator 단위·통합 테스트만 실행하고 production run()/accept-loop/wire는 미변경"
command: |
  cargo test -p gputeer-scheduler -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-48_scheduler_selected_gpu_assignment_2026-08-21.txt,
   docs/evidence/_raw/DoD-48_review.txt 전문 참조)

  감독자 직접 확인: PASS — scheduler best-fit 20 + hard-filter 33 = 53 passed,
  coordinator unit 83 + integration 4 = 87 passed, 전체 0 failed
  독립 검수 1라운드: ACCEPTED — 순수성, single/N 공용 helper, 입력 순서 독립성,
  부적격 GPU 제외, 뮤테이션 2건, STAGING 전 개수 재검증과 제한 범위를 확인
artifacts:
  - docs/plans/2026-08-21_1036_scheduler_selected_gpu_assignment_v1.md
  - docs/reports/2026-08-24_1048_scheduler_selected_gpu_assignment.md
  - crates/scheduler/src/lib.rs
  - crates/scheduler/src/model.rs
  - crates/scheduler/src/rank.rs
  - crates/scheduler/tests/best_fit.rs
  - crates/coordinator/src/orchestrate.rs
  - docs/evidence/_raw/DoD-48_scheduler_selected_gpu_assignment_2026-08-21.txt
  - docs/evidence/_raw/DoD-48_review.txt
negative_tests:
  - "equal_vram_gpu_tie_break_and_assignment_ignore_input_order: 같은 VRAM의 forward/reverse GPU vector에서 ResourceFit 전체 동등성과 작은 gpu_id 선택을 확인"
  - "gpu_inventory_permutations_produce_identical_ranking: 모든 candidate GPU vector reverse 전후의 BestFitRanking 전체 동등성을 확인"
  - "assignment_excludes_unhealthy_disallowed_and_too_small_gpus: 자체 재검토에서 발견한 공백을 닫아 unhealthy·불허 model·VRAM 부족 GPU 세 경우를 한 테스트로 직접 제외"
  - "empty_and_duplicate_gpu_ids_fail_closed_with_typed_errors: 빈/중복 GPU ID를 typed error로 거부"
  - "missing_gpu_health_model_and_vram_remain_fail_closed: health/model/VRAM missing fact의 기존 fail-closed 경계를 유지"
  - "single_and_ranked_winner_use_the_same_gpu_assignment_rule: 단일 후보와 복수 후보 winner가 같은 resource_fit 규칙과 selected ID를 사용"
  - "뮤테이션 1: gpu_id tie-break 제거 시 forward gpu-z/reverse gpu-a로 갈려 순서 독립성 테스트가 실패"
  - "뮤테이션 2: selected_gpu_ids 반환 제거 시 scheduler canonical ID assertion이 실패하고 coordinator가 STAGING 호출 전에 SelectedGpuCountMismatch를 반환"
limitations:
  - "반환 ID 는 scheduler snapshot 식별자이며 실제 NVML UUID provenance 는 아직 증명하지 않았다"
  - "Grant/Lease scope 생성, GPU 별 reservation/release, production wire 연결은 범위 밖"
  - "원본 Manifest 저장·검증, JobRequirements adapter, Job submit ingress와 submitter identity/key lookup은 없다"
  - "active session registry, node/device/session routing, durable outbox, send/ACK/거부/timeout 상태 전이는 없다"
  - "기존 node-exclusive CAS request/schema와 reservation row는 바꾸지 않아 GPU별 allocation이나 부분 자원 accounting을 제공하지 않는다"
  - "reservation release/requeue가 없어 production 연결 시 send 실패나 ACK timeout 뒤 영구 점유를 정리할 수 없다"
  - "private orchestrate module은 production run()/accept-loop에 연결되지 않았다"
  - "I/O·clock·randomness·protobuf를 변경하지 않은 순수 process-local 계산 조각이며 실제 GPU와 네트워크를 사용하지 않았다"
decision: "이 조각을 전체 GPU allocation, Grant scope 완성이나 production dispatch로 과장하지 않고 deterministic selected GPU assignment 순수 kernel로 완료했다. `ResourceFit`은 같은 선택 집합의 `FitKey`와 canonical `selected_gpu_ids`를 함께 반환하고, `rank_best_fit()`과 단일 후보 orchestration이 동일한 `resource_fit()`을 사용한다. health→model→VRAM 적격 판정 뒤 `(available_vram_bytes, gpu_id)` 정렬로 필요한 개수만 선택하며 빈/중복 ID와 개수 불일치는 typed error로 fail closed한다. reverse 입력의 `ResourceFit`·`BestFitRanking` 전체 동등성, 부적격 GPU 세 경우 직접 제외, tie-break 제거와 ID 반환 제거 뮤테이션, coordinator STAGING 전 개수 대조로 판별력을 확인했다. 독립 검수는 순수성·단일 계산 경로·결정성·뮤테이션·변경 범위·한계를 확인해 1라운드 만에 ACCEPTED했고 감독자는 scheduler 53 passed와 coordinator 87 passed를 직접 재확인했다. 반환 ID는 snapshot 식별자라 NVML UUID provenance가 아니며 Grant/Lease scope, GPU별 reservation/release와 production wire는 후속이다. scheduler 로드맵 진행: 조각 1·2a·2b-1·3a·4·5·5b 와 이번 GPU assignment 선행 조각 완료 — 실제 production 연결은 Job submit ingress·session routing·durable outbox·reservation release 가 모두 없어 아직 하루 규모를 넘는다(설계 조사 판정)"
---

# DoD-48 · deterministic selected GPU assignment 순수 kernel

## 무엇을 입증하려 했는가

`DoD-46`이 남긴 계약 불일치 3번은 ranking이 node와 aggregate `FitKey`만 반환하고
실제로 선택한 GPU 식별자를 버린다는 것이었다. 이 상태에서는 Agent가 사용할 GPU와
Lease/Grant가 허가할 GPU의 공통 입력을 만들 수 없다. 이번 조각은 production 연결이나
Grant scope 자체가 아니라, scheduler snapshot 안에서 선택한 GPU ID를 결정적으로
계산하고 잃지 않게 반환하는 순수 선행 kernel만 검증했다.

설계 조사는 나머지 계약 불일치와 실제 `run()`을 함께 판독했다. 선택 GPU 식별자 외에도
원본 Manifest 저장/`JobRequirements` 변환과 node/device/session routing이 모두 실제
dispatch에 필수이고, Job submit ingress·durable outbox·reservation release/requeue도
없다. 따라서 production 연결은 하루 규모를 넘으며 이번 조각으로 축소할 수 없다고
판정했다.

## 구현 — 한 `resource_fit()`에서 점수와 선택 ID를 함께 계산

`ResourceFit`은 한 candidate의 `FitKey`와 `selected_gpu_ids`를 함께 보존한다.
`resource_fit()`은 GPU health, allowed model, minimum VRAM 조건을 적용하고 적격 GPU를
`(available_vram_bytes, gpu_id)` 오름차순으로 정렬해 요구 개수만 선택한다. 첫 축은 기존
tight-VRAM 의미를 유지하고 같은 VRAM의 완전 동점은 `gpu_id`로 해소한다. 선택된 ID
목록 자체는 downstream canonical 입력으로 쓸 수 있게 `gpu_id` 오름차순으로 다시
정규화한다.

`rank_best_fit()`의 모든 eligible candidate는 이 helper를 호출해 `fit_key`와 ID를
함께 받는다. coordinator의 단일 후보 분기도 같은 helper를 직접 호출하고 복수 후보는
같은 helper를 이미 쓴 ranking winner의 ID를 사용한다. 별도 filter·sort 경로가 없다.
`PlacementToStagingOutcome::Staged`는 선택 ID를 보존하며, coordinator는 durable STAGING
API 호출 전에 요구 개수와 ID 수를 재검증한다.

## 자체 재검토 — 부적격 GPU 제외의 직접 증명 추가

초기 테스트는 tight VRAM, 동일 VRAM tie-break, 복수 GPU와 입력 순서 독립성을
검사했지만 unhealthy·불허 model·VRAM 부족 GPU가 선택 결과에서 빠지는지를 한 테스트가
직접 보여 주지 않았다. 자체 재검토에서 이 공백을 찾아
`assignment_excludes_unhealthy_disallowed_and_too_small_gpus`를 추가했다. 한 candidate의
세 부적격 GPU와 한 적격 GPU 가운데 적격 ID만 선택되고 잔여 자원도 그 선택에 맞음을
확인한다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 `resource_fit()`이 candidate/job 외 상태를 읽지 않는 순수 계산임을 확인했다.
single/N 경로가 같은 helper를 쓰며 coordinator에 별도 GPU filter/sort가 없고, reverse
입력 테스트가 `ResourceFit` 전체와 `BestFitRanking` 전체를 비교해 selected ID 차이도
놓치지 않음을 대조했다. 자체 재검토 테스트는 production의 health→model→VRAM 제외
단계를 세 경우 모두 직접 증명한다.

두 뮤테이션도 실제 경로를 판별했다. `gpu_id` tie-break를 제거하면 forward는 `gpu-z`,
reverse는 `gpu-a`를 선택했고, selected ID 반환을 제거하면 scheduler assertion과 함께
coordinator가 STAGING 호출 전에 `SelectedGpuCountMismatch`를 반환했다. 변경 범위가
scheduler 4개 파일, coordinator `orchestrate.rs`, 계획/리포트에 한정되고 staging,
inventory, job store와 wire가 바뀌지 않았음도 확인해 수정 요청 없이 1라운드에서
`ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-scheduler -p gputeer-coordinator
  PASS — scheduler 53 passed, coordinator 87 passed, 0 failed
```

위 결과는 감독자가 직접 재확인했다. 구현자 기록의 workspace build/test와
`git diff --check`도 PASS이며, stable toolchain에 `rustfmt` component가 없어
`cargo fmt`는 실행하지 못했다.

## 이 실험이 증명하지 "않는" 것

- 반환 ID는 scheduler snapshot 식별자이며 실제 NVML UUID provenance는 증명하지 않았다.
- Grant/Lease scope 생성과 GPU별 reservation/release는 없다.
- node-exclusive CAS를 GPU-exclusive allocation/accounting으로 바꾸지 않았다.
- 원본 Manifest 저장/adapter와 Job submit ingress는 없다.
- active session registry, node/device/session routing과 durable outbox는 없다.
- production `run()`/accept-loop, send/ACK/timeout과 실제 Agent 실행은 연결하지 않았다.

## 결정

1. `DoD-46` 계약 불일치 3번의 선행 작업을 deterministic selected GPU assignment 순수
   kernel로 완료했다.
2. `ResourceFit`이 같은 선택 집합의 점수와 canonical ID를 보존하고 단일/복수 후보가
   같은 helper를 쓰도록 해 selection algorithm 분기를 만들지 않았다.
3. 입력 순서 독립성 전체 비교, 부적격 GPU 세 경우 직접 제외, typed error와 두
   뮤테이션으로 선택 규칙과 STAGING 전 재검증의 판별력을 확인했다.
4. 독립 검수는 순수성·공용 경로·결정성·뮤테이션·범위·한계를 확인해 1라운드 만에
   `ACCEPTED`했고 감독자가 scheduler 53/coordinator 87 passed를 직접 재확인했다.
5. scheduler 로드맵 진행: 조각 1·2a·2b-1·3a·4·5·5b 와 이번 GPU assignment 선행 조각 완료 — 실제 production 연결은 Job submit ingress·session routing·durable outbox·reservation release 가 모두 없어 아직 하루 규모를 넘는다(설계 조사 판정)

관련: `docs/plans/2026-08-21_1036_scheduler_selected_gpu_assignment_v1.md`
