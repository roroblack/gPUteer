# verified terminal AttemptReport durable binding

- 계획: `docs/plans/2026-08-24_1238_terminal_attempt_report_binding_v1.md`의 조각 5e
- 스트림: Coordinator

## 목표

서명 검증을 통과한 terminal `AttemptReport`를 기존 durable Attempt와 현재 reservation owner에
원자적으로 묶어 보존하고, 재시작 시 evidence 유실과 stale/wrong-attempt report 혼입을 막는다.
Job/Attempt terminal 전이, runtime-stop 증명, Lease revoke와 reservation release는 수행하지 않는다.

## 수행

- `crates/coordinator/src/attempt_report_store.rs`: `Verified<AttemptReport>` 전용 저장 API,
  raw `StoredAttemptReportBinding` load API, typed binding/conflict/corruption 오류, SQLite schema,
  replay·rollback·손상·상태 무변경 테스트 추가.
- `crates/coordinator/src/staging_store.rs`: 기존 Attempt/reservation schema와 transaction read helper를
  같은 coordinator crate 안에서 재사용하도록 `pub(crate)` 경계로 추출.
- `crates/coordinator/src/lib.rs`: 모듈 export.
- 계획 문서: 구현 결과·negative/mutation·검증·제한 기록.

## 검증 결과

- `cargo test -p gputeer-coordinator`: 108 passed(104 unit + integration 4), 0 failed.
- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS.
- `cargo test --workspace --exclude gputeer-runtime-windows --no-fail-fast`: PASS.
  기존 crypto crash-child fixture 1건 ignored.
- node/signer binding guard 제거 뮤테이션: 지정 negative test FAILED(exit 101), 원복 후 PASS.
- fence guard 제거 뮤테이션: 지정 negative test FAILED(exit 101), 원복 후 PASS.
- `rustfmt` component 미설치로 `cargo fmt --check`는 실행하지 못했다. Rust compiler warning은
  없었고, Cargo는 기존 환경의 `could not canonicalize path C:\Users\playdata2` 경고를 출력했다.
- 디스크 부족 오류는 발생하지 않았다.

## 미해결·다음 작업

- durable load는 `Verified`가 아니므로 terminal consumer가 당시 authoritative key directory로
  signature를 다시 검증해야 한다.
- nested artifact/checkpoint validity와 durability, runtime-stop proof, terminal state transaction,
  reservation release와 production routing은 계획의 명시적 범위 밖으로 남겼다.
- 사용자의 지시대로 `docs/evidence/`, `CLAUDE.md`, `docs/history/HISTORY.md`는 수정하지 않았다.
