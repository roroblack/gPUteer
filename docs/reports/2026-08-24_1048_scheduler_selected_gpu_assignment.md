# scheduler selected GPU assignment 구현 리포트

## 목표

`docs/plans/2026-08-21_1036_scheduler_selected_gpu_assignment_v1.md`의
“조각 4b / deterministic selected GPU assignment kernel”만 구현했다. Grant/scope,
GPU별 reservation/accounting, production wire 연결은 범위 밖으로 유지했다.

## 수행 내용

- `crates/scheduler/src/model.rs`: `ResourceFit`, selected GPU ID 보존 필드와 typed error.
- `crates/scheduler/src/rank.rs`: 순수 `resource_fit()` 및 ranking과의 단일 계산 경로.
- `crates/scheduler/src/lib.rs`: 새 타입/helper 공개.
- `crates/scheduler/tests/best_fit.rs`: 결정성·fail-closed·GPU 순서 독립성 테스트 5건 추가.
- `crates/coordinator/src/orchestrate.rs`: 1/N 공용 helper 사용, outcome ID 보존,
  STAGING 전 개수 대조와 단일/복수 동일 규칙 테스트.
- 계획서의 “구현 결과” 절에 구현·검증·뮤테이션·제한을 기록했다.

선택 규칙은 적격 GPU를 `(available_vram_bytes, gpu_id)` 오름차순으로 정렬해 요구
개수만 고른 뒤, 반환 ID 목록을 `gpu_id` 오름차순으로 정규화하는 것이다. 단일 후보는
`resource_fit()`을 직접 호출하고 복수 후보 ranking도 각 candidate에 같은 함수를
호출하므로 두 알고리즘이 갈라지지 않는다.

## 검증

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS.
- `cargo test -p gputeer-scheduler`: PASS, 53 passed / 0 failed.
- `cargo test -p gputeer-coordinator`: PASS, 87 passed / 0 failed.
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS, 모든 실행 suite
  0 failed, 기존 ignored 1건 유지.
- `git diff --check`: PASS.
- 뮤테이션 1 `(VRAM, gpu_id)`에서 `gpu_id` 제거: GPU 순서 독립성 테스트가
  `gpu-z` 대 `gpu-a` 차이로 실패함을 확인하고 원복.
- 뮤테이션 2 선택 ID 반환 제거: scheduler canonical ID 테스트와 coordinator
  `SelectedGpuCountMismatch` 경계가 실패함을 확인하고 원복.

사용자가 `docs/evidence/DoD-NN_*` 수정을 금지했으므로 evidence 문서는 생성하지
않았다. 같은 이유로 `docs/history/HISTORY.md`도 수정하지 않았다. `cargo fmt`는 현재
stable toolchain에 `rustfmt` component가 없어 실행하지 못했고, 빌드/테스트 및 수동
diff 검토로 형식을 확인했다.

## 자체 재검토와 미해결 범위

재검토에서 false health/불허 model/VRAM 부족 GPU의 선택 제외를 직접 검증하지 않던
공백을 찾아 테스트를 추가했다. I/O·clock·randomness, protobuf, Grant/Lease scope,
GPU별 reservation/release, production `run()`은 변경하지 않았다. 반환 ID는 snapshot의
식별자일 뿐 실제 NVML UUID provenance를 이 조각이 증명하지 않는다.
