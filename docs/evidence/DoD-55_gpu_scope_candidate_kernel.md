---
schema_version: 2
id: DoD-55
claim: "관측·GPU 요구·명시 자원값과 caller-supplied `ProvenanceGate`만 받는 `gpu_scope_candidate()`가 I/O·clock·환경변수·난수·crypto·전역 상태 없이 입력 검증·정렬·`BTreeMap`/`BTreeSet` 계산만 수행하고, GPU를 `(available_vram, gpu_id)` 전체 튜플로 선택한 뒤 결과 ID를 재정렬해 동일 VRAM 동점과 입력 순서에도 결정적인 `ScopeCandidate` 전체를 만들며, provenance를 스스로 검증하지 않고 unverified·PARTITIONED·CUDA runtime 미해소·파생 VRAM을 typed error로 닫고 unknown/N/A를 긍정 값으로 발명하지 않음을 신규 테스트 14건, production 조기 반환 뮤테이션 2건, 독립 검수 1라운드 ACCEPTED와 감독자 scheduler 회귀 67건으로 확인했다"
status: PASS
commit: 0a6ce9121adb73e2458b9b08b960a0cb2c38f618

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — ScopeCandidate 순수 kernel, 신규 테스트 14건과 production 조기 반환 뮤테이션 2건"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-24T18:00:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 fresh-read-only 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "`scope.rs:130-281`의 외부 효과 없는 계산 경계, `(available_vram, gpu_id)` 전체 튜플 정렬과 결과 ID 재정렬, `gpu_scope_candidate.rs:68-142`의 실제 4! 순열 생성·동점 포함·`ScopeCandidate` 전체 비교, provenance가 caller 입력이고 자체 signature/membership 검증이 없는 경계, mode claim과 무관한 PARTITIONED 상시 거부, CUDA 임의 호환 규칙 부재, `DerivedFromTotalAndReserved` 미사용, 자체 수정과 production 조기 반환 뮤테이션 2건을 확인했다. 신규 14건, DoD-41 hard-filter 33건, DoD-45 best-fit 20건을 직접 재실행해 67 passed, 0 failed를 확인하고 잔여 수정 요청 없이 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-55_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-55_gpu_scope_candidate_kernel_2026-08-24.txt"
raw_output_digest: "sha256:d032c7cb28e5af4fc17db807f4e8038c278718f52ede835acd3785c8d007b305"
raw_output_bytes: 9051

binary_digests:
  toolchain: "C:\\Users\\playdata2\\.cargo\\bin\\cargo.exe 사용 — cargo/rustc version과 binary digest는 제공된 구현 이력에 기록되지 않음"
protocol_versions:
  schema_version: "proto/schema 변경 없음 — common.proto의 기존 GpuRequest·ResourceScope 계약을 process-local ScopeCandidate 입력/출력 타입으로 보존"
  canonical_spec: "서명 canonical·domain·membership 규범을 구현하지 않음; `ProvenanceGate`는 caller가 이미 판정해 제공하는 입력이며 wire materialization도 없음"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test"
hardware: "실물 NVIDIA GeForce RTX 4070 SUPER NVML 재판정에서 출발했으나 kernel 검증은 합성 관측 입력이며 테스트 실행 중 GPU/NVML을 접근하지 않음"
network_profile: "네트워크 미사용 — I/O/clock/environment/random/crypto/DB/network/global state 없는 in-process 순수 함수"
command: |
  C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-scheduler --no-fail-fast
raw_output: |
  (docs/evidence/_raw/DoD-55_gpu_scope_candidate_kernel_2026-08-24.txt,
   docs/evidence/_raw/DoD-55_review.txt 전문 참조)

  감독자 직접 재실행: PASS — ScopeCandidate 14 + hard-filter 33 + best-fit 20,
  총 67 passed, 0 failed, exit code 0
  production 뮤테이션 1 — provenance gate 제거 시 unverified 테스트가 Err 대신 Ok를 받아 실패
  production 뮤테이션 2 — PARTITIONED 거부 제거 시 Partitioned 후보가 생성돼 MIG 테스트 실패
  두 분기 원복 후 전체 suite PASS
  독립 검수 1라운드: ACCEPTED — 순수성·동점/24개 순열/전체 결과·provenance 경계·
  PARTITIONED 상시 거부·CUDA 규칙 부재·파생 VRAM 미사용·67-test 회귀 직접 확인
artifacts:
  - docs/plans/2026-08-24_1700_gpu_scope_first_slice_v1.md
  - crates/scheduler/src/scope.rs
  - crates/scheduler/src/lib.rs
  - crates/scheduler/tests/gpu_scope_candidate.rs
  - docs/evidence/_raw/DoD-55_gpu_scope_candidate_kernel_2026-08-24.txt
  - docs/evidence/_raw/DoD-55_review.txt
negative_tests:
  - "all_four_gpu_input_permutations_produce_the_identical_candidate: 재귀 생성기가 동점 GPU를 포함한 4개 입력의 실제 4! = 24개 순열을 만들고 모든 순열의 `ScopeCandidate` 전체를 `assert_eq!`로 비교"
  - "unverified_provenance_is_an_explicit_gate_not_a_kernel_inference: `ProvenanceGate::Unverified`를 즉시 typed error로 닫으며 production gate 제거 뮤테이션은 Err 대신 Ok를 만들어 실패하고 원복 뒤 통과"
  - "partitioned_allocation_is_rejected_even_when_input_claims_mig_support: GPU가 PARTITIONED 지원을 주장해도 항상 typed error이며 production 거부 분기 제거 뮤테이션은 Partitioned 후보를 만들어 실패하고 원복 뒤 통과"
  - "cuda_runtime_requirement_is_not_silently_ignored_or_given_invented_compatibility: proto 요구를 타입에 보존하고 규범 없는 exact-match/driver mapping 대신 `CudaRuntimeCompatibilityUnresolved`로 닫음"
  - "measured_rtx_fixture_does_not_invent_missing_time_health_or_available_vram: 실측 RTX fixture의 누락 time/revision/health와 `total - reserved` 파생 VRAM을 순서대로 typed error로 확인"
  - "missing_required_gpu_facts_fail_closed_only_when_the_constraint_needs_them: model/driver/compute는 constraint가 있을 때만 요구하고 constraint가 없으면 irrelevant fact 누락을 과잉 배제하지 않음"
  - "blank_noncanonical_and_duplicate_gpu_ids_are_typed_canonical_errors: blank·공백 비정규·duplicate GPU ID를 입력 순서와 무관한 typed canonical error로 닫음"
  - "writable_prefixes_are_canonicalized_and_malformed_values_fail_closed: prefix를 canonical sort하고 blank·비정규·duplicate를 typed error로 닫되 명시적 빈 목록은 no-write scope로 보존"
limitations:
  - "`ProvenanceGate::Verified` 는 caller 입력일 뿐 kernel 이 서명·membership 을 검증하지 않는다"
  - "CUDA runtime compatibility 는 아직 판정할 수 없다"
  - "shared mode 입력은 실제 MPS/동일-owner enforcement 를 입증하지 않는다"
  - "wire `ResourceScope`, Grant/Lease, 저장·서명·전이는 구현하지 않았다"
  - "MIG partitioned allocation 은 입증할 수 없다"
  - "실물 NVML 실측은 GPU 하드웨어 값 부재만 해소했으며 observation의 authoritative provenance, signer/device membership, freshness와 durable inventory binding을 증명하지 않는다"
  - "`ScopeCandidate`의 GPU ID는 opaque input 식별자이며 wire UUID provenance나 runtime isolation truth가 아니다"
decision: "이 조각은 실물 GPU 실측 이후에 다시 판정해 나왔다. 이전까지 Grant/Lease scope는 NVML UUID provenance와 확정 자원값이 없다는 이유로 반복해서 막혀 있었다. 실물 GPU 호스트에서 RTX 4070 SUPER의 UUID·PCI·compute capability·VRAM 등을 NVML로 실측한 결과, 'GPU 하드웨어 값이 없다'는 차단만 해소되고 authoritative provenance 차단은 그대로임을 확인했다. 그래서 full scope 대신 계산 부분만 DoD-41 hard-filter, DoD-45 best-fit, DoD-54 effective-replica와 같은 순수 kernel 계열로 분리했다. 입력 GPU ID를 canonical 검사하고 적격 후보를 `(available_vram, gpu_id)` 전체 튜플로 정렬해 동점도 ID로 결정한 뒤 결과 ID를 재정렬한다. 재귀 순열 테스트는 동점 GPU를 포함한 4개 입력의 모든 24개 순열에서 `ScopeCandidate` 전체를 비교한다. provenance는 kernel의 판정 결과가 아니라 caller 입력이며 Unverified만 typed error로 닫는다. PARTITIONED는 mode claim과 무관하게 항상 거부하고, `[N/A]`를 false/unsupported/healthy로 바꾸지 않으며, CUDA runtime에는 규범 없는 호환 규칙을 만들지 않았다. `DerivedFromTotalAndReserved`도 authoritative VRAM 계산에 사용하지 않는다. 이는 `CLAUDE.md`의 unknown을 추정으로 채우지 않는 원칙과 `common.proto`의 GPU VRAM/count·ResourceScope 자원 계약을 따른다. 자체 재검토에서 proto의 `cuda_runtime_version` 요구가 타입에서 소실될 수 있는 결함을 찾아 필드를 보존하고 typed unresolved error와 negative test를 추가했다. model/driver/compute는 해당 constraint가 있을 때만 요구하도록 고쳐 irrelevant fact의 과잉 배제를 피했다. production provenance gate와 PARTITIONED 거부 조기 반환 뮤테이션 2건은 각각 지정 테스트를 실패시켰고 원복 뒤 통과했다. 독립 검수는 순수성·동점 포함 결정성·실제 4!와 전체 결과 비교·provenance 책임 경계·PARTITIONED 상시 거부·CUDA 임의 규칙 부재·파생 VRAM 미사용·14+33+20 회귀를 직접 확인해 1라운드 ACCEPTED했고, 감독자도 `cargo test -p gputeer-scheduler`의 67 passed를 재확인했다. scheduler 로드맵 진행: `DoD-41`~`DoD-55` 완료. 실물 NVML 실측으로 '하드웨어 값 부재' 차단은 해소됐고 계산 부분을 순수 kernel 로 떼어냈으나, **authoritative provenance 차단은 그대로**여서 full Grant/Lease scope 는 여전히 막혀 있다. membership/ControlStore 계열은 규범 확정 포함 누적 약 3일로 별도 과제다."
---

# DoD-55 · 서명·저장·전이와 분리된 `ScopeCandidate` 순수 kernel

## 무엇을 입증하려 했는가

고정 GPU 관측, GPU 요구, CPU/RAM/workspace/writable-prefix 자원값과 caller가 판정한
provenance gate를 입력하면 `gpu_scope_candidate()`가 외부 효과 없이 결정적인
`ScopeCandidate`를 만들고, 알 수 없거나 입증되지 않은 사실을 typed error로 닫는지 검증했다.

이 claim은 wire `ResourceScope`, Grant/Lease, GPU 관측의 서명·membership 검증, 실제 MPS/MIG
enforcement, 저장 또는 상태 전이를 주장하지 않는다. kernel에 전달된 입력에 대한 계산만
주장한다.

## 실물 GPU 실측 뒤의 재판정

이 조각 전까지 Grant/Lease scope는 NVML UUID provenance와 확정 자원값이 없다는 이유로
반복해서 차단됐다. 실물 GPU 호스트의 RTX 4070 SUPER에서 UUID, PCI, compute capability,
VRAM 등을 NVML로 실측하자 “GPU 하드웨어 값 자체가 없다”는 차단은 해소됐다.

그러나 NVML 출력만으로는 누가 언제 관측했는지, 승인된 device/member인지, 어떤 durable
inventory revision에 묶였는지를 증명할 수 없다. authoritative provenance 차단은 그대로였다.
따라서 full scope를 만들지 않고 DoD-41 hard-filter, DoD-45 best-fit, DoD-54
effective-replica와 같은 계열의 계산 부분만 순수 kernel로 분리했다.

## 규범 근거와 발명하지 않은 값

`CLAUDE.md:96`의 원칙은 값을 모르면 비워 두며 추정으로 채우면 그 오류가 조용히 scheduling
결정까지 간다고 경고한다. `proto/common.proto:187-193`의 `GpuRequest`는 GPU별 최소 VRAM과
개수, CUDA·allocation·model 요구를 두고, `:206-212`의 `ResourceScope`는 GPU와
CPU/RAM/workspace/writable-prefix 자원을 둔다.

이 kernel은 `[N/A]`를 false, unsupported 또는 healthy로 바꾸지 않는다. `PARTITIONED`는 GPU가
지원한다고 주장해도 언제나 `PartitionedAllocationUnproven`이다. CUDA runtime 요구는 타입에
보존하지만 호환 규범이 없으므로 exact match나 driver mapping을 만들지 않고
`CudaRuntimeCompatibilityUnresolved`로 닫는다. `total - reserved`도 available의 권위 있는
정의가 아니므로 `DerivedFromTotalAndReserved`는 선택 계산에 쓰지 않는다.

## 구현 — 외부 효과 없는 계산 경계

`crates/scheduler/src/scope.rs:5-75`에 관측·요구·자원 입력 타입과 `ProvenanceGate`,
`:77-107`에 `ScopeCandidate`, `:109-128`에 typed `ScopeError`, `:130-281`에
`gpu_scope_candidate()`를 추가했다. 함수는 참조 입력을 읽고 검증·정렬·`BTreeMap`/`BTreeSet`
계산과 결과 구성만 한다. I/O, clock, 환경변수, 난수, crypto, DB/network, 전역 상태에
접근하지 않는다.

`ProvenanceGate::Verified`는 caller 입력이다. kernel은 서명, signer, membership 또는
canonical observation을 스스로 검증하지 않는다. `Unverified`를 즉시 typed error로 닫는 것만
이 경계의 책임이다. `crates/scheduler/src/lib.rs:23`의 module/export 추가가 기존 production
파일 변경의 전부이며 wire materialization, 저장, Grant/Lease 또는 상태 전이는 없다.

## 결정성 — 동일 VRAM 동점까지 전체 결과 고정

GPU ID를 먼저 canonical 검사한 뒤 적격 후보를 `(available_vram, gpu_id)` 전체 튜플로
오름차순 정렬한다. 첫 필드인 VRAM이 같으면 `gpu_id`가 tie-break다. 요구 개수만 선택한 뒤
결과 GPU ID를 다시 정렬한다.

전용 테스트의 재귀 `permutations()`는 동점 GPU를 포함한 4개 입력의 실제 4! = 24개 순열을
모두 만든다. 모든 순열에서 selected ID만이 아니라 `ScopeCandidate` 전체를 `assert_eq!`로
비교한다. model/compute allowlist와 writable-prefix 순서를 뒤집은 별도 테스트도 전체 result가
같음을 확인한다.

## 자체 재검토에서 수정한 결함

초안의 `JobGpuRequirements`에는 proto의 `cuda_runtime_version` 요구가 없어 caller adapter가
이를 조용히 버릴 수 있었다. 요구 필드를 타입에 보존하고, 임의 호환 규칙 대신 typed
unresolved error와 negative test를 추가했다.

또한 model, driver, compute capability는 해당 constraint가 있을 때만 요구하도록 했다.
constraint가 없는데 irrelevant fact가 없다는 이유로 정상 GPU를 과잉 배제하지 않으며,
health=false나 앞선 compatibility gate에서 제외된 GPU의 뒤쪽 사실도 요구하지 않는다.

## 뮤테이션과 독립 검수 — **ACCEPTED**

production provenance gate 조기 반환을 제거하면 unverified 테스트가 `Err` 대신
`Ok(ScopeCandidate)`를 받아 실패했다. production `PARTITIONED` 거부 조기 반환을 제거하면
Partitioned 후보가 생성돼 MIG 테스트가 실패했다. 둘 다 원복한 뒤 전체 suite가 통과했다.

독립 검수는 순수성, 동일 VRAM의 `gpu_id` tie-break, 실제 24개 순열과 `ScopeCandidate` 전체
비교, provenance가 caller 입력이고 자체 검증이 없는 경계, mode claim과 무관한 PARTITIONED
상시 거부, CUDA 임의 호환 규칙 부재, `DerivedFromTotalAndReserved` 미사용, 자체 수정과
뮤테이션 판별력을 확인했다. 신규 14건, DoD-41 hard-filter 33건, DoD-45 best-fit 20건을
직접 재실행해 67 passed, 0 failed를 확인하고 1라운드 `ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-scheduler --no-fail-fast
  PASS — ScopeCandidate 14 + hard-filter 33 + best-fit 20
         총 67 passed, 0 failed, exit code 0
```

## 이 evidence가 증명하지 않는 것

- `ProvenanceGate::Verified`는 caller 입력일 뿐 kernel이 서명·membership을 검증하지 않는다.
- CUDA runtime compatibility는 아직 판정할 수 없다.
- shared mode 입력은 실제 MPS/동일-owner enforcement를 입증하지 않는다.
- wire `ResourceScope`, Grant/Lease, 저장·서명·전이는 구현하지 않았다.
- MIG partitioned allocation은 입증할 수 없다.
- 실물 NVML 실측은 하드웨어 값 부재만 해소했으며 authoritative observation provenance,
  freshness/revision binding 또는 runtime 하드웨어 진실성을 증명하지 않는다.

## 결정

실물 GPU 실측은 계산 fixture에 넣을 하드웨어 값은 제공했지만 그 값을 authority로 승격하지는
못했다. 그래서 불완전한 full scope 대신 알 수 없는 값을 보존하고 결정적인 후보만 만드는
순수 kernel을 완료했다.

scheduler 로드맵 진행: `DoD-41`~`DoD-55` 완료. 실물 NVML 실측으로 '하드웨어 값 부재'
차단은 해소됐고 계산 부분을 순수 kernel 로 떼어냈으나, **authoritative provenance 차단은
그대로**여서 full Grant/Lease scope 는 여전히 막혀 있다. membership/ControlStore 계열은
규범 확정 포함 누적 약 3일로 별도 과제다.
