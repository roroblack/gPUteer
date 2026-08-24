# selected GPU durable reservation binding 구현 보고

## 목표

`docs/plans/2026-08-24_1103_scheduler_selected_gpu_reservation_binding_v1.md`의 조각 5c를
구현해 DoD-48의 `selected_gpu_ids`를 inventory CAS, node reservation, Attempt/Lease,
`QUEUED -> STAGING`, operation idempotency와 같은 local SQLite transaction에 보존한다.

## 수행 내용

- `crates/coordinator/src/staging_store.rs`
  - reservation-aware request와 stored reservation에 canonical GPU IDs 추가
  - `coordinator_node_reservation_gpus` schema와 검증 가능한 ordinal child rows 추가
  - request validation, node-scoped inventory membership validation, operation payload binding,
    replay/reopen 복원, legacy/corrupt binding fail-closed 구현
  - binding 직후 fault injection, negative/replay/corruption/boundary/concurrency 테스트 추가
- `crates/coordinator/src/orchestrate.rs`
  - single/N 선택 경로의 exact scheduler GPU IDs를 reservation-aware staging에 전달
  - 양 경로의 durable reservation 재조회 assertion 추가
- 계획 문서에 구현 결과, 검증, 자체 재검토, 제한을 기록했다.

사용자 지시에 따라 `docs/evidence/`, `CLAUDE.md`, `docs/history/HISTORY.md`는 수정하지
않았다. Manifest adapter, Grant/scope, release, GPU별 accounting, production wire도
범위 밖으로 유지했다.

## 검증 결과

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS.
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS, 0 failed
  (기존 ignored 1건).
- `cargo test -p gputeer-coordinator`: PASS, unit 88 + integration 4 = 92 passed.
- operation payload GPU binding 제거 mutation: 지정 conflict test가 실패해 검출.
- inventory membership 우회 mutation: 지정 negative test가 실패해 검출.
- `Barrier` 기반 node 경쟁: 성공 1건, reservation/GPU binding row는 winner에만 존재.
- `git diff --check`: PASS.

## 미해결·제한

- stable toolchain에 rustfmt component가 없어 `cargo fmt`는 실행하지 못했다.
- 이 조각은 local durable binding이며 Raft commit, UUID provenance, Grant 전송, release를
  구현하거나 증명하지 않는다.
- 독립 evidence와 history 갱신은 사용자 지시로 이번 세션에서 수행하지 않았다.
