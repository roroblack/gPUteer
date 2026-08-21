# 2026-08-21_1123_scheduler_attempt_lease_v1

- 기준선: `../gputeer_master_plan_FINAL.md` §19.2, §20.2, §27.2~§27.3,
  §28, §33.1
- 상위 로드맵: `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md`
  조각 2
- 직전 조각: `docs/plans/2026-08-21_1049_scheduler_durable_job_v1.md`
  (`DoD-42`, 조각 2a)
- 조사 대상: `crates/coordinator/src/job_store.rs`,
  `crates/coordinator/src/lease_store.rs`
- 상태: **조각 2b-1 구현·로컬 검증 완료 — 독립 evidence/`COMMITTED` 주장은 하지 않음**
- 스트림: Coordinator

## 결론

현재 두 저장소를 그대로 순서대로 호출해서는 Attempt 생성과 Lease 발급을 같은
트랜잭션으로 만들 수 없다. 두 타입은 각각 private `rusqlite::Connection`을
소유하고 각 메서드가 자기 connection에서 별도 `BEGIN IMMEDIATE`와 `COMMIT`을
수행한다. 같은 SQLite 파일 경로를 두 번 열더라도 write lock으로 호출 순서만
직렬화될 뿐, 첫 commit과 둘째 commit 사이의 crash를 rollback하는 하나의
트랜잭션은 아니다. 서로 다른 DB 파일이면 SQLite 기본 트랜잭션으로는 더더욱
묶을 수 없다.

따라서 통합 경로는 **한 control DB 파일의 한 connection에서** Job 상태 변경,
Attempt 삽입, team-global fence epoch 채번, Lease 삽입을 하나의
`BEGIN IMMEDIATE`로 수행해야 한다. 기존 Lease renew/revoke/read API가 같은 파일을
별도 connection으로 여는 것은 가능하지만, 최초 staging 연산 자체는 두 store
메서드 호출로 나누면 안 된다.

오늘 하루에 정직하게 가능한 최소 조각은 **조각 2b-1: single-node local atomic
staging kernel**이다. 저장 계층에서 위 네 효과의 전부-or-none과 재시도·동시성만
증명한다. 실제 scheduler 선택, 자원 reservation, Grant 전송, Coordinator 실행
경로 연결, Raft/`COMMITTED`는 하지 않는다. 이 결과를 “durable Attempt/Lease
통합 완료”나 로드맵 조각 2 완료라고 부르지 않는다.

## 확인한 현재 구조

### `CoordinatorJobStore`

- `job_store.rs:242-285`에서 자체 `Connection`을 열고
  `coordinator_jobs`, `job_submission_idempotency`만 만든다.
- `JobState`는 `SUBMITTED`, `PLANNING`, `QUEUED`, `FAILED`뿐이다
  (`job_store.rs:19-47`). Attempt 테이블, `STAGING`, staging 시각, Lease 참조,
  fence counter가 없다.
- submit과 각 전이는 자기 connection의 `BEGIN IMMEDIATE` 안에서 검사·쓰기 후
  commit한다(`job_store.rs:321-408`, `555-570`). 이 경계 밖의 Lease store
  쓰기를 참가시킬 API나 transaction handle 노출은 없다.
- `job_store`는 `lib.rs`에 module로만 공개되어 있고 Coordinator `run()`이나 CLI에
  DB path가 연결되어 있지 않다. 현재는 저장 계층과 테스트만 존재한다.

### `CoordinatorLeaseStore`

- `lease_store.rs:171-210`에서 별도의 private `Connection`을 열고
  `coordinator_leases`만 만든다.
- `get_or_issue()`도 자기 connection의 별도 `BEGIN IMMEDIATE`에서 Lease 한 행만
  검사·삽입하고 commit한다(`lease_store.rs:281-349`). Job/Attempt 존재나 상태는
  검사하지 않는다.
- `fence_epoch`는 저장소가 채번하지 않는다. 호출자가 만든 `StoredLease`의 값을
  최초 삽입 때 그대로 저장한다. 현재 실행 경로에서는 CLI
  `--fence-epoch`가 후보값이다(`lib.rs:1391-1443`). 저장소에는 team-global
  counter나 `MAX(epoch)+1` 연산이 없다.
- 기존 store의 강점인 동일 `lease_id` identity 대조, 만료/revoke 거부,
  renew 시 epoch 불변은 유지할 가치가 있다. 그러나 이는 “이미 주어진 Lease
  사실 보존”이지 새 Attempt와 결합된 Lease 발급 정책이 아니다.

### 같은 파일만으로 충분한가

아니다. 조건은 두 가지 모두다.

1. Job, Attempt, fence counter, Lease 테이블이 **같은 SQLite database file**에
   있어야 한다.
2. 네 쓰기를 수행하는 복합 메서드는 **같은 `Connection`의 같은 transaction**을
   사용해야 한다.

같은 파일을 `CoordinatorJobStore::open(path)`와
`CoordinatorLeaseStore::open(path)`로 각각 열면 두 테이블은 공존할 수 있지만,
두 public 메서드의 commit은 여전히 분리된다. 반대로 connection 하나에서 별도
DB를 `ATTACH`하는 설계는 기존 API와 마이그레이션을 복잡하게 만들고, 향후
ControlStore 한 로그 항목이라는 모델에도 맞지 않으므로 이번 조각의 해법으로
선택하지 않는다.

## 규범과 상위 계획 재확인

- 상위 로드맵 조각 2는 원래 durable Job/Attempt/Queue, Attempt/fence 할당,
  재시도 idempotency를 합쳐 3~5일로 잡았다
  (`scheduler_전체_설계_v1.md:180-188`). 조각 2a가 하루에 전체 완료를 주장하지
  않고 Attempt를 이월한 판단은 맞다.
- 규범 Job 전이표는 `QUEUED -> STAGING` guard를 “선택 노드 lease 발급 성공”,
  effect를 “fence_epoch 증가”, durability를 `COMMITTED`로 둔다
  (`docs/protocol/state-machines.md:103-125`).
- 규범 Attempt 전이표는 Job의 STAGING 진입을 guard로 `CREATED`를 만들며, 같은
  effect에 fence 증가와 Lease 발급을 둔다
  (`docs/protocol/state-machines.md:161-187`). 즉 어느 하나도 독립 선행 commit이면
  안 된다.
- 마스터 플랜 §20.2는 epoch가 team-global이고 새 Attempt마다 증가하며
  ControlStore commit으로만 증가한다고 고정한다. §28의 Attempt 모델은
  `attempt_id/job_id/node_ids/status/lease/fence_epoch`를 요구한다.
- 마스터 플랜 §33.1은 fence 증가가 반드시 `propose`를 거치고, v0.1 local
  store에서도 단조 순서를 보장하되 `Committed`를 가장하지 말라고 한다.

추가로 실제 계약 blocker가 있다. `proto/control.proto:238-260`의
`ControlAction`은 oneof이며 `TransitionJob`, `CreateAttempt`, `IssueLease`가 서로
다른 action이다. 현재 wire 계약에는 이 셋을 한 commit으로 표현할 compound
action이나 transaction batch가 없다. 그러므로 SQLite 복합 메서드는 안전한
로컬 저장 kernel은 될 수 있어도 규범 `COMMITTED` ControlStore 통합을 완성하지
못한다. 진짜 통합 전에 계약 변경 절차로 `StageAttempt` 같은 단일 복합 action
또는 원자 batch 의미를 확정해야 한다.

## `QUEUED`에서 실제 Attempt를 만들기 위한 최소 상태

### 새 durable 상태

한 control DB에 최소 다음이 필요하다.

- `coordinator_attempts`
  - `attempt_id` PK, `job_id` FK, `state='CREATED'`, `fence_epoch`, `lease_id`,
    `created_at_unix_ms`, `revision`
  - 이번 조각은 single-node만 허용하되, proto의 repeated `node_ids`를 훼손하지
    않도록 `coordinator_attempt_nodes(attempt_id,node_id,ordinal)` child table을
    둔다. 입력 node는 정확히 하나만 허용한다.
- `coordinator_fence_state`
  - DB당 singleton `max_issued_epoch` 8바이트 big-endian BLOB
  - 이 DB가 한 team의 control truth라는 전제를 명시한다. 현재 store/API에
    `team_id`가 없으므로 여러 team을 한 파일에 섞는 것은 범위 밖이며 거부한다.
- `staging_operation_idempotency`
  - 16바이트 operation key를 `job_id/attempt_id/lease_id`와 결합해 ambiguous
    commit 재시도에 원래 결과를 돌려준다.
- `coordinator_jobs`
  - `STAGING` 상태와 `staging_at_unix_ms`를 추가하고 상태별 row shape 검사를
    확장한다. active Attempt를 Job 단일 컬럼으로 고정하지 않는다. 향후 한 Job에
    여러 Attempt가 정상이라는 §20.1과 충돌하기 때문이다.

### fence epoch 채번

호출자가 epoch를 주게 두면 안 된다. 복합 transaction 안에서 다음 순서로
저장소가 채번한다.

1. singleton counter와 기존 `coordinator_leases.fence_epoch` 전 행을 exact
   8-byte로 decode한다. 손상값은 fail closed한다.
2. `base = max(counter, existing lease epochs)`로 기존 Lease DB와의 호환
   watermark를 잡는다.
3. `base.checked_add(1)`을 새 epoch로 정하고 overflow면 아무것도 쓰지 않는다.
4. counter, Attempt, Lease, Job을 같은 transaction에서 기록한다.

기존 Lease 행을 함께 보는 이유는 현재 `get_or_issue()`가 임의의 caller epoch를
이미 저장할 수 있기 때문이다. counter만 0에서 시작하면 기존 고 epoch보다 낮은
값을 발급할 수 있다. 장기적으로 integrated control DB에서는 standalone 최초
`get_or_issue()`를 금지하고 복합 staging 연산만 새 Lease를 만들게 해야 하지만,
그 런타임 전환은 오늘 범위 밖이다.

### 원자 연산의 최소 API 의미

이름은 예를 들어 `stage_queued_with_lease(request)`로 하되 타입명보다 의미가
우선이다.

- 입력: 16-byte operation key, `job_id`, caller가 만든 opaque non-empty
  `attempt_id`/`lease_id`, 정확히 한 `node_id`, issuer/term, issued/renew/expiry/
  max-duration 값
- transaction 내부 guard:
  - Job 존재, row shape 정상, 현재 `QUEUED`, plan 존재
  - 시각 rollback 없음, Lease 수명 필드 순서 정상
  - attempt/lease ID 신규 또는 같은 operation의 정확한 retry
  - node/issuer 등 identity가 공백 아님
- 성공 효과:
  - epoch 1회 증가
  - Attempt `CREATED` 1행과 node 결합 저장
  - 같은 job/attempt/node/epoch의 active Lease 1행 저장
  - Job을 `STAGING`으로 바꾸고 revision 1 증가
  - operation 결과 저장 후 한 번만 commit
- retry:
  - 같은 key와 byte-identical 논리 입력은 최초 Job/Attempt/Lease/epoch를 반환
  - 같은 key의 다른 입력, 같은 attempt/lease ID의 다른 identity는 전체 거부

기존 `CoordinatorLeaseStore::get_or_issue()`를 복합 API 안에서 호출하지 않는다.
Lease insert/row decode/identity 검사 SQL helper를 transaction을 받는 내부 함수로
추출해 재사용한다. transaction의 소유자는 하나여야 한다.

## 오늘 착수할 최소 조각 — 2b-1

### In

- Job/Lease schema 초기화를 같은 control DB에서 안전하게 반복 실행할 수 있는
  내부 schema helper로 합친다.
- 위 세 테이블과 Job `STAGING` shape를 추가한다.
- single-node `stage_queued_with_lease()` storage API를 구현한다.
- epoch를 저장소가 원자 채번하고 caller-provided epoch를 받지 않는다.
- 기존 별도 Lease DB 파일을 열고 읽는 renew/revoke/resume 동작과 기존 Job 2a
  테스트는 회귀 없이 유지한다.
- production 약 220~360줄, 테스트 약 260~420줄을 상한으로 잡는다. 이 범위를
  넘기거나 `lib.rs` wire 경로 변경이 필요해지면 중단하고 다음 조각으로 넘긴다.

### Out

- `CoordinatorConfig`/CLI에 `--job-db` 또는 통합 `--control-db` 추가
- 현재 CLI 고정 `issue_grant()`를 새 API에 연결
- scheduler winner 선택, inventory, GPU reservation/admission
- Grant 서명/전송/ACK와 상태 전이
- multi-node Attempt/분산 Lease
- lease expiry 후 새 Attempt, revoke/cleanup/requeue, 이후 Attempt 상태기계
- full `ControlStore`, Raft, watch/cursor, `COMMITTED` 보증
- `proto/control.proto` compound action 결정 및 schema version 변경

### 필수 negative tests / DoD 후보

1. 성공 뒤 Job=`STAGING`, Attempt=`CREATED`, Lease와 Attempt의
   job/attempt/node/epoch가 모두 같고 counter도 그 epoch다.
2. Attempt insert 뒤, Lease insert 뒤, Job update 직전의 의도적 오류 각각에서
   네 상태가 전부 rollback되고 counter도 소비되지 않는다.
3. 두 connection이 같은 QUEUED Job을 서로 다른 attempt로 동시에 stage하면 정확히
   하나만 commit되고 패자 Attempt/Lease는 남지 않는다.
4. 동일 operation key retry는 새 epoch를 소비하지 않고 최초 결과를 반환한다.
5. 동일 key 변경 payload, attempt/lease identity 충돌, non-QUEUED Job, 공백 ID,
   잘못된 수명 순서, clock rollback은 상태 불변으로 실패한다.
6. 기존 Lease epoch가 counter보다 높으면 그보다 1 큰 값을 발급한다. 잘린 epoch
   BLOB과 `u64::MAX`는 전체 rollback한다.
7. reopen 뒤 Job/Attempt/Lease/counter가 보존되고, 기존 Lease store의
   renew/revoke/read가 새로 발급된 Lease를 같은 파일에서 읽는다.

DoD 이름은 **“local atomic STAGING commit”**처럼 좁게 잡는다. claim에는 로컬
SQLite `DURABLE`만 적고 `COMMITTED`, 실제 resource reservation, Grant dispatch,
전체 durable Attempt를 적지 않는다.

## 이후 게이트와 로드맵 판단

오늘 2b-1은 안전한 하위 조각이므로 scheduler 로드맵을 지금 멈출 필요는 없다.
조각 3의 다중 Agent inventory도 원래 의존성상 독립 진행 가능하다.

다만 2b 전체와 조각 4~5의 실제 통합에 들어가기 전에는 다음 결정을 반드시
별도 계약 조각으로 해결해야 한다.

1. `TransitionJob + CreateAttempt + IssueLease`를 한 ControlStore commit으로
   표현하는 compound action/batch 계약
2. local control DB의 canonical path와 기존 `--lease-db`/Job DB migration 정책
3. reservation과 staging commit의 경계: reservation 실패 시 Attempt/epoch/Lease가
   생기지 않고, 성공한 reservation이 Grant 대상 세션과 어떻게 결합되는지

이 게이트 없이 `lib.rs`에서 Job store 전이 후 기존 `issue_lease()`를 호출하는
방식으로 연결하는 것은 조각 2a가 피한 split authority를 그대로 되살린다. 그런
구현이라면 진행보다 중단이 맞다.

## 구현 결과 — 2026-08-21

### 구현한 로컬 atomic kernel

- `crates/coordinator/src/staging_store.rs`를 추가했다.
  `CoordinatorStagingStore::stage_queued_with_lease()`는 한 control DB 파일의
  한 connection에서 `BEGIN IMMEDIATE`를 열고 다음을 한 번만 commit한다.
  1. DB singleton counter와 기존 Lease 전 행의 exact 8-byte epoch를 읽어
     `max + 1`을 채번한다.
  2. `coordinator_attempts`와 단일 ordinal 0 node 결합을 삽입한다.
  3. 같은 job/attempt/node/epoch의 `coordinator_leases` 행을 삽입한다.
  4. Job을 `QUEUED -> STAGING`으로 바꾸고 revision을 1 증가시킨다.
  5. 16-byte operation key, job/attempt/lease ID, 충돌 없는 길이-prefix payload,
     결과 epoch를 idempotency 행에 저장한다.
- 새 durable 타입은 `StageQueuedRequest`, `StageQueuedResult`,
  `StoredAttempt`, `AttemptState`, `StagingStoreError`다. caller-provided epoch는
  API에 없다.
- `coordinator_attempts`, `coordinator_attempt_nodes`,
  `coordinator_fence_state`, `staging_operation_idempotency`를 추가했다. 이 DB
  하나가 한 team의 local control truth라는 전제이며 multi-team key는 추가하지
  않았다.
- `job_store.rs`는 `JobState::Staging`과 `staging_at_unix_ms` row shape를
  추가하고, 기존 조각 2a DB를 `ALTER TABLE`로 반복 안전하게 migration한다.
  비어 있는 queued/staging plan도 손상으로 거부한다.
- `job_store.rs`와 `lease_store.rs`의 schema 초기화를 각각 내부 helper로
  추출했다. 기존 public submit/transition/get-or-issue/renew/revoke/resume API의
  트랜잭션 소유권과 호출 형태는 바꾸지 않았다. Lease 조회·삽입 helper만
  transaction을 받은 복합 경로가 재사용한다.

### negative test와 fail-closed 결과

- 성공 후 Job/Attempt/node/Lease/counter의 job/attempt/node/epoch 일치를 검사했다.
- Attempt 삽입 뒤, Lease 삽입 뒤, Job update 직전의 세 오류 주입 모두에서
  Job/Attempt/Lease/counter/operation 행 전체가 rollback되고, 바로 재시도한 첫
  epoch가 1임을 검사했다.
- 같은 파일의 두 connection을 `Barrier` 뒤 동시에 실행해 성공 1건,
  `JobNotQueued(STAGING)` 1건, Attempt/node/Lease/operation 각 1행만 남음을
  검사했다.
- 동일 operation retry가 epoch를 소비하지 않고 최초 결과를 반환하며, 이후 기존
  Lease API가 renew/revoke한 뒤에도 가변 필드를 손상으로 오인하지 않고 최초
  operation 결과를 반환함을 검사했다.
- 같은 key의 변경 payload, 다른 operation의 attempt/lease ID 재사용,
  non-QUEUED Job, job/attempt/lease/node/issuer 공백, 수명 순서·max-duration
  위반, queue 시각보다 이른 issued 시각을 모두 상태 불변으로 거부했다.
- 기존 Lease epoch 41에서 42를 발급했다. 잘린 Lease/counter epoch BLOB과
  Lease/counter `u64::MAX`는 counter나 부분 행을 바꾸지 않고 실패했다.
- reopen 뒤 네 상태 보존, 조각 2a 이전 Job schema migration, 기존 Lease store의
  read/renew/revoke/reopen을 검사했다.

### 뮤테이션 테스트

1. fence 채번의 `checked_add(1)`을 임시로 `checked_add(0)`으로 바꾸자
   `every_injected_write_failure_rolls_back_job_attempt_lease_counter_and_operation`이
   `left: 0, right: 1`로 실패했다. 원복했다.
2. commit 직전 Job 상태 대입을 `STAGING`에서 `QUEUED`로 바꾸자
   `success_is_consistent_and_identical_retry_does_not_consume_epoch`이
   `left: Queued, right: Staging`으로 실패했다. 원복했다.

### 자체 재검토에서 발견하고 고친 것

첫 구현은 동일 operation retry 시 현재 Lease 행 전체를 최초 값과 비교했다.
기존 standalone `renew_existing()`/`mark_revoked()`가 합법적으로
`expires_at_unix_ms`·`renew_after_unix_ms`·`revoked_at_unix_ms`를 바꾸면 retry가
저장소 손상으로 오인하는 기존 API 결합 회귀였다. operation payload와 최초 epoch로
최초 Lease 결과를 재구성하고, 현재 Lease에서는 불변 identity·epoch만 검사하도록
고쳤으며 renew+revoke 뒤 retry 회귀 테스트를 추가했다. 또한 QUEUED/STAGING의
공백 plan을 “plan 존재”로 세던 row-shape 공백과 조각 2a 이전 schema migration
테스트 누락을 찾아 함께 닫았다.

### 검증 결과와 제한

- `cargo build --workspace --exclude gputeer-runtime-windows`: exit 0.
- `cargo test --workspace --exclude gputeer-runtime-windows`: exit 0,
  411 passed / 0 failed / 1 ignored. Coordinator는 unit 56개와 기존 integration
  4개가 통과했다.
- 뮤테이션 2건은 각각 예상 실패를 재현했고 모두 원복 후 Coordinator 테스트가
  다시 통과했다.
- `cargo fmt -p gputeer-coordinator`는 이 환경의 stable toolchain에
  `rustfmt` component가 없어 실행되지 않았다. `cargo build`와 compiler test는
  경고 없이 통과했으며 `git diff --check`도 오류가 없다.
- 계획의 기능 범위는 넓히지 않았지만, `staging_store.rs` production 부분은 상세
  손상 검사·retry 결과 검증·schema/error surface를 포함해 계획 당시 물리 줄 수
  추정 상한(360줄)을 넘었다. wire/CLI/예약/dispatch/프로토콜/Raft 작업으로
  확장한 결과는 아니다.
- 이 결과는 local SQLite `DURABLE` 원자성만 보인다. 독립 검수와 evidence는 이
  세션의 명시적 문서 제한 때문에 만들지 않았으므로 DoD `PASS`를 주장하지 않는다.
  resource reservation, Grant dispatch, multi-node, canonical control DB migration,
  compound ControlAction, Raft `COMMITTED`는 전부 여전히 Out이다.
