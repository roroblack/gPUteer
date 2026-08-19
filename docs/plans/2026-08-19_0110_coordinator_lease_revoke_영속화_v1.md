# Coordinator Lease revoke 영속화 v1

- 작성 시각: 2026-08-20 01:10 (+09:00)
- 대상 조각: Coordinator Lease revoke 상태의 SQLite 영속화 및 재시작 후 발급/갱신 차단
- 관련 설계: `docs/plans/2026-08-19_1517_lease_revoke_최소_조각_v1.md`,
  `docs/plans/2026-08-19_1814_lease_재접속_최소_조각_v1.md`,
  `docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md`

## 범위와 설계

1. `StoredLease`와 `coordinator_leases`에 nullable `revoked_at_unix_ms BLOB`를
   추가한다. `CoordinatorLeaseStore::open()`은 `PRAGMA table_info`로 기존 DB의
   컬럼을 확인하고, 구 schema이면 `ALTER TABLE ... ADD COLUMN`을 즉시 수행한다.
   migration은 `BEGIN IMMEDIATE` 트랜잭션 안에서 수행하며, PRAGMA/ALTER/commit
   어느 단계라도 실패하면 `LeaseStoreError`로 열기를 실패시킨다. 기존 행의 NULL은
   active Lease로 해석한다.
2. `SELECT_LEASE_SQL`, raw row 변환, INSERT를 새 nullable 컬럼에 맞춘다.
   `mark_revoked(lease_id, revoked_at_unix_ms)`는 즉시 트랜잭션에서 조회하고,
   없으면 `NotFound`, 처음 revoke면 timestamp를 저장하고, 이미 revoke된 경우
   최초 timestamp를 유지하는 idempotent 동작으로 구현한다.
3. `get_or_issue()` 및 Coordinator의 저장소 기반 갱신 경로는
   `revoked_at_unix_ms`가 있으면 `LeaseStoreError::Revoked`를 반환한다. 저장된
   Lease와 timestamp는 덮어쓰지 않으며, signed revoked-denial 응답은 만들지 않는다.
4. `send_revoke_notice()`는 실제 Grant의 Lease ID로 먼저 `mark_revoked()`를
   성공시킨 뒤 notice 생성·서명·TCP 전송을 수행한다. notice의 test-only ID/epoch
   override는 wire payload에만 적용한다. 저장소가 없으면 기존 legacy 동작을 유지하며,
   commit 성공 뒤 전송 실패가 revoke 상태를 되돌리지는 않는다.
5. Agent와 proto는 변경하지 않는다. CLI `coordinator-agent-selftest`에는 시나리오
   35를 추가한다: 1차 프로세스 쌍에서 revoke 성공과 저장 timestamp를 확인하고,
   완전히 새 프로세스 쌍에서 같은 Lease/holder를 `revoked` 오류로 거부하며 Grant를
   보내지 않는지 확인한다.

## 테스트와 검증 계획

- 구 schema 수동 DB migration 후 기존 Lease가 `revoked_at_unix_ms == None`으로
  보존되는 단위 테스트
- `mark_revoked()` 저장, 최초 timestamp 보존(idempotency), revoked 재발급 거부,
  revoked 갱신 거부 단위 테스트
- `cargo build --workspace --exclude gputeer-runtime-windows`
- `cargo test --workspace --exclude gputeer-runtime-windows`
- selftest 35를 시도별 하드 타임아웃으로 5회 연속 실행
- `mark_revoked()` 호출 제거와 `get_or_issue()` revoked 검사 제거의 두 뮤테이션을
  각각 실행해 시나리오 35가 실패하는지 확인한 뒤 원복

## 제외

Agent/fence DB/proto schema 변경, 자동 재접속·retry/backoff, 만료 Lease 재접속 정책,
revoke 해제 API, signed revoked-denial 응답, evidence 문서 작성, commit/push는 포함하지
않는다.
