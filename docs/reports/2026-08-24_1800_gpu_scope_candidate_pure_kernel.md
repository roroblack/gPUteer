# 2026-08-24 GPU ScopeCandidate pure kernel 구현

## 목표

`docs/plans/2026-08-24_1700_gpu_scope_first_slice_v1.md`의 첫 슬라이스대로 서명·저장·전이와
분리된 `ScopeCandidate` 계산 kernel만 구현한다. wire `ResourceScope`, Grant/Lease,
provenance 검증과 MIG partitioned allocation 주장은 범위 밖이다.

## 수행 내용

- `crates/scheduler/src/scope.rs`: 고정 관측/요구/명시 자원/provenance gate 입력 타입,
  typed fail-closed 오류, deterministic `gpu_scope_candidate()` 구현.
- `crates/scheduler/src/lib.rs`: 새 순수 API 공개와 crate 경계 주석 갱신.
- `crates/scheduler/tests/gpu_scope_candidate.rs`: 4! 전체-result 순열, collection 순서,
  strict missing/malformed 입력, RTX derived-VRAM, provenance, CUDA runtime, MIG 및
  compatibility negative 테스트 14건.
- 계획 문서에 구현 결과, 뮤테이션, 자체 재검토, 검증 결과와 한계를 기록했다.

kernel은 external I/O/clock/TTL/DB/network/random/crypto/state transition을 호출하지
않는다. `PARTITIONED`는 input claim과 무관하게 typed error이며, total-reserved 파생값도
authoritative available VRAM으로 세지 않는다.

## 검증

```text
cargo test -p gputeer-scheduler --test gpu_scope_candidate --no-fail-fast
PASS — 14 passed, 0 failed

cargo test -p gputeer-scheduler --no-fail-fast
PASS — 67 passed, 0 failed

cargo build --workspace --exclude gputeer-runtime-windows
PASS — exit 0

cargo test --workspace --exclude gputeer-runtime-windows
PASS — 0 failed, 기존 ignored 1건 유지
```

뮤테이션은 provenance gate 제거와 `PARTITIONED` 거부 제거 두 건 모두 지정 테스트 실패를
확인하고 원복 후 재통과했다. `cargo fmt --check`는 stable toolchain에 `cargo-fmt.exe`가
없어 실행하지 못했다.

사용자 지시로 `docs/evidence/`는 수정하지 않았으며 DoD/P0 PASS를 주장하지 않는다.

## 미해결 이슈·다음 작업

- `ProvenanceGate::Verified`를 생산할 signed observation/domain/membership verifier는 없다.
- CUDA runtime compatibility 규칙이 없어 요구가 있으면 typed unresolved error다.
- Shared mode 입력은 runtime MPS/동일-owner enforcement를 증명하지 않는다.
- MIG `[N/A]` 상태에서 `PARTITIONED`는 계속 명시적으로 차단된다.
- wire `ResourceScope`/GrantedExecutionPlan 변환, durable revision binding,
  Lease/Grant 서명·저장·전이·routing은 후속 범위다.
