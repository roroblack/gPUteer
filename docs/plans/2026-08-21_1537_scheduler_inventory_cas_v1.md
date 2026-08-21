# 2026-08-21_1537 scheduler inventory CAS v1

- **대상:** DoD-46 뒤의 inventory revision 기반 CAS reservation
- **조사 방식:** 현재 코드와 DoD-40/44/46 및 scheduler 계획을 read-only로 대조
- **오늘의 판정:** revision 재조회만으로는 위험이 닫히지 않는다. 오늘 가능한 최소
  진짜 조각은 **선택 node의 snapshot revision을 검사하면서 node-exclusive
  reservation과 `QUEUED -> STAGING`을 한 SQLite write transaction에 기록하는
  로컬 kernel**이다.
- **production 연결:** 이번 조각에서 하지 않는다.

## 결론

`CoordinatorInventoryStore`에는 이미 Agent별 `inventory_revision: u64`가 있고,
parent inventory와 GPU/workload child 교체가 한 `BEGIN IMMEDIATE` 안에서 이
revision과 함께 원자적으로 일어난다. 따라서 이 값은 “선택에 사용한 inventory가
아직 같은가”를 검사하는 CAS token으로 쓸 수 있다.

그러나 revision 자체는 reservation version이 아니다. 첫 Job을 STAGING으로 옮겨도
inventory row와 revision은 바뀌지 않는다. 그러므로 staging 직전
`get_inventory()`로 revision이 같은지만 확인하면, 서로 다른 Job의 **순차 호출도**
모두 같은 revision을 보고 같은 자원을 다시 차지한다. 재조회와 staging이 별도
transaction이면 둘 사이 TOCTOU도 그대로다.

따라서 별도 durable reservation row가 반드시 필요하다. 오늘은 현재 scheduler가
GPU UUID 집합을 반환하지 않는 현실을 숨기지 않고, `node_id` 하나를 배타적으로
예약해 그 node의 모든 GPU/CPU/RAM/workspace를 한 Job만 쓰게 한다. 이는 활용률이
낮은 보수적 v0 계약이지만 같은 GPU 중복을 실제로 막는다. GPU별 allocation과
부분 자원 공유를 구현했다고 주장하지 않는다.

## 확인한 현재 코드

### 1. inventory row에는 CAS token이 이미 있다

- `AgentInventory.inventory_revision`은 `u64`다
  (`crates/coordinator/src/inventory_store.rs:42-56`).
- SQLite parent row `coordinator_agent_inventory.inventory_revision`은
  `BLOB NOT NULL`이고 big-endian 8바이트로 저장·검증된다
  (`inventory_store.rs:395-406`, `:684-714`, `:837-840`).
- `update_inventory()`는 `BEGIN IMMEDIATE`에서 낮은 revision을 거부하고, 같은
  revision의 다른 normalized payload를 거부하며, 더 높은 revision일 때 parent와
  GPU/workload child를 전량 교체한다 (`:204-347`). 같은 revision의 같은 payload만
  멱등 성공한다.
- 따라서 한 Agent의 GPU/CPU/RAM/workspace 사실 전체에 대한 per-Agent CAS token으로
  사용할 수 있다. per-GPU revision을 새로 만들 필요는 없다.
- 다만 revision은 Agent가 제공하는 telemetry 세대다. Coordinator reservation이
  생겼다고 자동 증가하지 않으므로 reservation 충돌 검사는 별도 상태가 맡아야 한다.

### 2. `PoolSnapshot`은 revision을 버린다

`pool_snapshot()`은 registry와 inventory를 한 deferred read transaction에서 읽지만
`project_candidate()`가 `observed_at_unix_ms`와 자원 사실만 옮긴다
(`inventory_store.rs:349-372`, `:780-820`). 현재
`CandidateSnapshot`/`PoolSnapshot`에도 revision 필드가 없다
(`crates/scheduler/src/model.rs:94-137`).

그러므로 오늘 다음처럼 추가한다.

```rust
pub struct CandidateSnapshot {
    // existing fields...
    pub inventory_revision: Option<u64>,
}
```

- inventory row가 있으면 `Some(inventory.inventory_revision)`, 없으면 `None`이다.
- revision은 filter/rank 점수가 아니라 이후 admission의 compare token이다.
- 선택된 node의 expected revision은 재조회해서 만들지 않고, filter/rank에 실제로 쓴
  **그 `PoolSnapshot` 후보**에서 꺼낸다.
- 선택된 후보에 revision이 없거나 node가 pool에서 유일하게 찾아지지 않으면 typed
  오류로 fail closed한다. 정상 hard filter상 inventory 없는 후보는 적격이 될 수
  없지만 orchestration 경계도 이를 독립 검증한다.

### 3. staging transaction에는 reservation이 없다

`StageQueuedRequest`에는 node만 있고 revision/GPU/resource가 없다
(`crates/coordinator/src/staging_store.rs:19-31`).
`stage_queued_with_lease()`의 `BEGIN IMMEDIATE`는 Job, Attempt, fence epoch, Lease와
operation idempotency만 원자화하며 inventory를 읽지 않는다 (`:162-246`).

inventory store와 staging store는 서로 다른 SQLite connection을 소유하므로,

```text
get_inventory(node) == expected_revision
  -> stage_queued_with_lease(request)
```

처럼 두 public API를 순서대로 호출하는 것은 CAS가 아니다. 정확한 linearization
point는 staging이 사용하는 동일 control DB의 동일 `BEGIN IMMEDIATE` transaction
안이어야 한다.

## 오늘 구현할 최소 조각 — node-exclusive CAS admission

### 이름

**DoD-47 후보: local node-exclusive inventory-CAS reservation kernel**

“GPU allocation 완료”나 “production scheduler”라고 부르지 않는다.

### 저장 계약

동일 control DB에 active row의 존재 자체가 reservation인 작은 테이블을 둔다.
개념 스키마는 다음과 같다.

```sql
CREATE TABLE coordinator_node_reservations (
    node_id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL UNIQUE,
    inventory_revision BLOB NOT NULL,
    reserved_at_unix_ms BLOB NOT NULL
);
```

정확한 FK와 corruption 검사는 구현 중 기존 staging schema 순서에 맞추되, 다음
불변식은 바꾸지 않는다.

1. `node_id`당 active row는 최대 하나다.
2. row는 owning `job_id`/`attempt_id`와 snapshot inventory revision을 보존한다.
3. reservation insert, Attempt/Lease/fence, Job STAGING 전이와 operation idempotency는
   모두 한 commit이거나 모두 rollback이다.
4. 다른 SQLite file의 inventory를 읽어 stage하는 조합은 허용하지 않는다. transaction
   안에서 해당 node의 inventory row를 직접 찾지 못하면 fail closed한다.

별도 `state` 컬럼과 release API는 오늘 넣지 않는다. 이 조각에서는 row가 존재하면
active이고 삭제 경로가 없으므로, production에 연결할 수 없는 제한이 눈에 보인다.

### API/transaction 경계

기존 DoD-43의 reservation 없는 `stage_queued_with_lease()` 의미를 조용히 바꾸지
않는다. 새 request/result와 새 method를 두거나, 동등하게 계약이 분리된 명시적 API를
둔다.

```rust
reserve_node_and_stage_queued_with_lease(
    stage: &StageQueuedRequest,
    expected_inventory_revision: u64,
) -> Result<ReservedStageResult, ReservedStageError>
```

새 method의 한 `BEGIN IMMEDIATE` 안에서 순서는 다음과 같다.

```text
exact operation replay 확인
  -> current inventory row의 revision == expected revision 확인
  -> 기존 active node reservation 없음 확인/UNIQUE insert
  -> 기존 stage_queued_with_lease 본문을 transaction helper로 실행
  -> operation 결과와 reservation을 함께 검증 가능하게 기록
  -> COMMIT
```

- stale/missing inventory는 `InventoryRevisionMismatch`/`InventoryMissing`처럼 typed
  실패한다.
- 이미 다른 attempt가 node를 예약했으면 `NodeAlreadyReserved`로 실패한다.
- exact operation replay는 현재 inventory가 이후 갱신됐더라도 최초 commit 결과를
  반환해야 한다. 반대로 같은 operation key에 expected revision 또는 stage payload가
  달라지면 conflict다. 따라서 expected revision도 idempotency payload에 포함한다.
- reservation을 insert한 뒤 staging이 실패하면 transaction rollback으로 orphan
  reservation, Attempt, Lease, fence 소비, Job 전이가 모두 없어야 한다.
- `orchestrate_placement_to_staging()`은 선택 후보의 revision을 같은 pool에서 꺼내 새
  method를 정확히 한 번 부른다. revision을 “직전 재조회”하는 호출은 추가하지 않는다.

### 명시적 실패만 하고 자동 retry는 하지 않는다

stale revision이나 occupied node가 나오면 이번 kernel은 typed 오류로 끝낸다. 새
snapshot/filter/rank bounded retry는 오늘 넣지 않는다. 현재 `pool_snapshot()`은
active reservation을 투영하지 않으므로 단순 retry를 넣으면 같은 best-fit node를
계속 고를 수 있고, operation key 재사용/교체 규칙도 함께 정해야 하기 때문이다.

이 제한 때문에 오늘 결과는 safety kernel이지 scheduler liveness 완성이 아니다.

## 왜 GPU별 allocation까지 오늘 넣지 않는가

장기적으로는 node-exclusive row가 아니라 적어도 `(node_id, gpu_id)`별 Exclusive
allocation과 CPU/RAM/workspace reservation 합계가 필요하다. 하지만 현재 계약에는
다음이 없다.

- `rank_best_fit()`은 tight GPU의 VRAM 값으로 node를 순위화하지만 선택한 GPU UUID
  집합을 반환하지 않는다 (`crates/scheduler/src/rank.rs:125-183`).
- `BestFitRanking.winner`에는 node와 aggregate `FitKey`만 있다
  (`scheduler/src/model.rs:249-268`).
- `StageQueuedRequest`, Attempt, Lease scope에도 GPU UUID와 예약 자원량이 없다.
- Agent inventory의 available 수치와 Coordinator active allocation을 언제 합산/차감할지,
  heartbeat가 실행 중 Job을 반영한 뒤 이중 차감을 어떻게 피할지 reconciliation 계약이
  없다.
- 실패/Grant 거부/timeout/Lease 만료 뒤 allocation release와 requeue 전이가 없다.

이를 한꺼번에 하루 CAS 조각에 넣으면 selected-GPU plan, resource accounting와
lifecycle까지 사실상 roadmap 조각 4/5 전체를 다시 여는 셈이다. node-exclusive
reservation은 이 미정 계약을 꾸며내지 않으면서 중복 GPU 사용만 보수적으로 막는
최소 안전 경계다.

## 완료 조건과 negative tests

실제 파일 기반 동일 control DB로 다음을 증명한다.

1. `pool_snapshot()`의 각 inventory 보유 후보에 저장된 revision이 정확히 투영되고,
   inventory 없는 후보는 `None`이다.
2. snapshot 뒤 inventory revision을 올리면 reservation/staging이 typed stale 오류로
   실패하고 Job은 QUEUED, Attempt/Lease/reservation은 0건, fence는 미소비다.
3. 서로 다른 두 QUEUED Job이 unchanged inventory의 같은 node를 순차 선택해도 첫
   Job만 stage되고 둘째는 `NodeAlreadyReserved`다.
4. 별도 SQLite connection 두 개가 같은 expected revision/node로 동시에 admission을
   시도해도 정확히 하나만 commit한다. loser Job은 QUEUED이며 reservation/Attempt/Lease
   owner는 winner와 완전히 일치한다.
5. 서로 다른 node는 각각 하나씩 예약할 수 있어 전역 singleton lock으로 잘못
   구현하지 않았음을 보인다.
6. exact operation replay는 새 reservation이나 fence epoch를 만들지 않고 최초
   `ReservedStageResult`를 반환한다. expected revision을 바꾼 same-key 요청은
   operation conflict다.
7. reservation insert 뒤의 fault injection은 reservation, Attempt, Lease, Job 전이,
   operation row와 fence epoch를 모두 rollback한다.
8. 손상되거나 길이가 잘못된 inventory revision/reservation owner row는 success로
   해석하지 않고 corruption 오류로 닫힌다.
9. mutation으로 revision predicate 제거, node uniqueness 제거, reservation을 stage
   transaction 밖으로 이동한 각각이 위 테스트에 실제로 잡힌다.

검증 명령은 최소 다음이다.

```text
cargo test -p gputeer-scheduler
cargo test -p gputeer-coordinator
```

정직한 예상 규모는 production Rust 약 180~300줄, 테스트 약 250~450줄, 문서/evidence
별도다. GPU UUID plan이나 release를 넣기 시작하면 오늘 범위를 넘은 것으로 보고
중단한다.

## production `run()`/accept-loop 연결 판단

**이번 조각에 포함하지 않는다.** reservation CAS 하나가 생겨도 현재 production
loop는 scheduler 호출부가 될 준비가 되어 있지 않다.

- `run()`은 Agent connection을 순차 accept하고, 현재 socket에 CLI로 고정된
  agent/job/attempt/lease ID의 server-first stub Grant를 보낸다
  (`crates/coordinator/src/lib.rs:275-357`, `:437-476`, `:1301-1326`).
- Job submission/Manifest 보관과 `JobRequirements` adapter가 없다.
- selected `node_id -> device_id -> active session/socket` routing과 session owner
  fencing이 없다. 현재 정상적으로 `node_id != device_id`일 수 있다.
- Grant payload의 manifest/plan/rationale와 selected GPU scope가 비어 있다.
- commit 뒤 send 실패, `accepted=false`, ACK timeout에 대한 durable outbox,
  rollback/requeue와 reservation/Lease release가 없다. 지금 연결하면 첫 실패가 node를
  영구 점유시킨다.

DoD-40의 “Coordinator는 의도적으로 순차 처리” 제약은 이 결정을 완화하지 않는다.
순차 accept는 동시 write race를 줄일 뿐, 첫 Job 뒤 inventory가 그대로이면 다음 Job도
같은 자원을 고르는 문제를 막지 않는다. 반대로 오늘의 SQLite 경쟁 테스트는 현재
loop가 순차라는 이유로 생략하지 않는다. 저장 계약은 향후 다중 session/process
caller에서도 보존돼야 한다.

production 연결은 다음의 별도 조각으로 남긴다.

```text
오늘: node-exclusive inventory-CAS + atomic STAGING, crate-internal
  -> selected GPU UUID/부분 resource allocation + reservation-aware projection/retry
  -> release/requeue와 durable Grant outbox
  -> manifest/requirements/plan/rationale + node/device/session routing
  -> 그 뒤에만 run()/accept-loop/E2E 연결
```

## 이번 조각의 Out

- GPU별 allocation, Shared GPU, CPU/RAM/workspace 부분 예약과 accounting
- active reservation을 뺀 `PoolSnapshot` projection과 bounded rerank retry
- reservation release, Lease 만료/거부/timeout cleanup, Job requeue
- Manifest 보관/adapter, full Grant/plan/rationale/scope 생성
- active session registry와 node/device/socket routing
- wire send/ACK/outbox 및 production entrypoint 연결
- Raft/ControlStore `COMMITTED`, 다중 Coordinator HA

이 Out이 남아 있으므로 완료 주장은 “로컬 node-exclusive inventory-CAS reservation
kernel”까지만 한다.

## 구현 결과 (2026-08-21 15:53 KST)

계획한 **로컬 node-exclusive inventory-CAS reservation kernel**을 구현했다. production
`run()`/accept-loop, GPU별 allocation, release/requeue, retry는 연결하지 않았다.

### 코드

- `crates/scheduler/src/model.rs`
  - `CandidateSnapshot.inventory_revision: Option<u64>`를 추가했다. filter/rank는 이
    필드를 사용하지 않는다.
- `crates/coordinator/src/inventory_store.rs`
  - inventory가 있는 후보에는 저장된 revision을 `Some`, 없는 후보에는 `None`으로
    `PoolSnapshot`에 투영한다.
- `crates/coordinator/src/staging_store.rs`
  - `coordinator_node_reservations(node_id PRIMARY KEY, job_id, attempt_id UNIQUE,
    inventory_revision, reserved_at_unix_ms)`를 기존 staging schema에 추가했다.
  - 기존 reservation 없는 `stage_queued_with_lease()` 계약은 유지했다.
  - 별도 `reserve_node_and_stage_queued_with_lease()`와
    `StoredNodeReservation`/`ReservedStageResult`/`ReservedStageError`를 추가했다.
  - 새 API의 단일 `BEGIN IMMEDIATE` 안에서 exact operation replay 확인, 현재 inventory
    revision 직접 조회/비교, 기존 node reservation 확인, reservation 삽입, 기존
    Attempt/Lease/fence/Job STAGING 본문, expected revision을 포함한 operation payload
    기록, commit을 순서대로 수행한다.
  - exact replay는 이후 inventory가 갱신돼도 최초 결과를 반환하며, 같은 operation
    key에서 expected revision이 달라지면 `OperationConflict`로 실패한다.
- `crates/coordinator/src/orchestrate.rs`
  - 선택된 node가 동일 `PoolSnapshot`에서 정확히 하나인지와 revision이 `Some`인지
    fail-closed로 확인한 뒤 새 reservation-aware API를 정확히 한 번 호출한다.
  - CAS/점유 충돌의 자동 snapshot/rank 재시도는 추가하지 않았다.

### 원자성 확인

revision 비교와 reservation insert는 `stage_new_in_transaction()` 호출 전 같은
`rusqlite::Transaction`에서 일어나며, Attempt/Lease/fence/Job/operation 기록도 그
transaction reference를 그대로 받는다. reservation 삽입 직후 fault injection에서
reservation, Attempt, Lease, fence, operation이 0건이고 Job이 QUEUED인 것을 확인했다.
두 별도 connection/스레드가 같은 한-GPU node를 경쟁하는 테스트와 전체 orchestration
경쟁 테스트에서는 정확히 하나만 STAGING으로 commit되고 패자는 명시적
`NodeAlreadyReserved`로 끝났다.

### negative/concurrency/경계 테스트

- stale revision 및 missing inventory: typed 오류, 쓰기 효과 0건.
- 동일 snapshot revision을 본 서로 다른 두 Job의 순차 같은-node 예약: 첫 요청만 성공.
- 두 connection/스레드의 동시 같은-GPU 예약: 정확히 하나만 성공.
- 두 스레드의 전체 placement-to-staging orchestration: STAGING 1건, QUEUED 1건.
- 서로 다른 node 예약: 둘 다 성공하여 전역 singleton reservation이 아님을 확인.
- exact replay/changed expected revision operation conflict.
- reservation insert 직후 fault: reservation을 포함한 모든 쓰기 rollback.
- 잘못된 길이의 inventory/reservation revision과 빈 reservation owner: corruption으로
  fail closed.
- `u64::MAX` inventory revision 예약/보존.
- 기존 reservation 없는 DoD-43 API의 성공/replay/fault/concurrency 테스트 재통과.

### 수동 뮤테이션

1. revision 불일치 predicate를 `false && ...`로 무력화했을 때
   `stale_or_missing_inventory_fails_before_any_staging_side_effect`가 실패했고 stale
   Job이 실제 STAGING된 것을 확인했다.
2. reservation의 `node_id PRIMARY KEY`와 기존 reservation 검사를 함께 제거했을 때
   `concurrent_distinct_jobs_reserving_one_gpu_commit_exactly_once`가 실패했고 성공 수가
   2가 됐다.

두 뮤테이션은 즉시 원복했고 targeted 테스트와 전체 검증을 재통과했다.

### 검증

- `cargo build`: PASS(Windows runtime 포함).
- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS.
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS — 452 passed,
  0 failed, 1 ignored.
- `cargo test -p gputeer-scheduler`: PASS — 48 passed.
- `cargo test -p gputeer-coordinator`: PASS — unit 82 passed + integration 4 passed.
- `git diff --check`: PASS(LF/CRLF 전환 예고 warning만 출력).

`cargo fmt --all -- --check`는 코드 실패가 아니라 현재 Rust toolchain에 `rustfmt`
component가 설치되지 않아 실행 불가였다. approval 없는 환경에서 toolchain을 변경하지
않았다. 같은 이유로 추가 실행한 `cargo clippy -p gputeer-scheduler
-p gputeer-coordinator --all-targets -- -D warnings`도 `clippy` component 부재로 실행
불가였다. 요청된 빌드/테스트 compiler 검증은 모두 통과했다.

### 자체 재검토

- 교착: inventory read transaction은 staging write transaction 전에 commit된다. CAS
  transaction은 `BEGIN IMMEDIATE` 하나이고 별도 lock 순서가 없어 새 교착 순환을 만들지
  않는다.
- fail-open: missing/stale/corrupt inventory와 corrupt reservation은 모두 오류이며
  reservation 또는 STAGING 성공으로 해석되지 않는다.
- 경계: zero와 `u64::MAX` revision은 8-byte big-endian BLOB으로 보존되고 잘못된 길이는
  거부된다.
- API 회귀: 기존 `stage_queued_with_lease()`는 reservation을 만들지 않는 DoD-43 의미와
  request encoding을 유지하며 기존 테스트가 전부 통과했다.
- 남은 한계: node-exclusive라 같은 node의 서로 다른 GPU도 동시에 쓰지 못한다. release가
  없어 row는 영구 active이며 따라서 production에는 연결하지 않았다. 이는 계획의 Out과
  일치한다.
