# scheduler inventory CAS 구현 보고

## 목표

`docs/plans/2026-08-21_1537_scheduler_inventory_cas_v1.md`가 정한 최소 조각인 로컬
node-exclusive inventory revision CAS reservation과 원자적 `QUEUED -> STAGING`을
구현했다.

## 변경

- scheduler `CandidateSnapshot`에 admission token인 `inventory_revision`을 추가했다.
- inventory 저장소가 저장 revision을 `PoolSnapshot`에 `Some/None`으로 투영한다.
- 기존 staging 저장소에 node reservation table과 reservation-aware staging API를
  추가했다.
- revision 비교, node reservation, Attempt/Lease/fence, Job STAGING, operation
  idempotency를 하나의 `BEGIN IMMEDIATE` transaction으로 묶었다.
- orchestration은 같은 snapshot의 선택 후보 revision을 새 API에 전달하며 충돌을
  자동 재시도하지 않는다.

## 검증 결과

- `cargo build`: PASS(Windows runtime 포함).
- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS.
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS — 452 passed,
  0 failed, 1 ignored.
- scheduler 48 tests, coordinator unit 82 tests와 integration 4 tests: PASS.
- stale/missing/corrupt 입력, 순차 중복, rollback, replay/conflict, 서로 다른 node,
  `u64::MAX` 경계를 포함한 negative tests: PASS.
- Barrier 기반 두-thread same-GPU reservation과 전체 orchestration 경쟁: 각각 정확히
  한 Job만 성공.
- 수동 mutation 2건(revision predicate 제거, node uniqueness 제거): 대응 테스트가
  각각 예상대로 실패했고 원복 후 전체 검증 PASS.
- `git diff --check`: PASS(LF/CRLF warning만 있음).

`cargo fmt --all -- --check`는 설치된 stable toolchain에 `rustfmt` component가 없어
실행하지 못했다. 추가 `cargo clippy ... -D warnings`도 같은 toolchain의 `clippy`
component 부재로 실행하지 못했다. 코드 compiler/build/test 결과와는 별도의 환경
제한이다.

## 자체 검토와 한계

inventory snapshot transaction은 staging write 전에 종료되고, admission write는 하나의
SQLite immediate transaction만 사용하므로 새 lock-order cycle은 없다. 모든 CAS/corrupt
경로는 fail closed이고 기존 reservation 없는 DoD-43 API 테스트도 재통과했다.

이번 구현은 node-exclusive이고 release가 없으므로 production 연결 대상이 아니다. GPU별
allocation, 부분 자원 accounting, release/requeue, reservation-aware rerank/retry,
Grant/outbox/run 연결, Raft/HA는 후속 범위다.
