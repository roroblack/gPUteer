# 2026-08-21_1049_scheduler_durable_job_v1

- 기준선: `../gputeer_master_plan_FINAL.md` §13.7, §27.2~§27.3, §33.1
- 상위 로드맵: `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md` 조각 2
- 상태: 조각 2a 구현 및 로컬 검증 완료
- 스트림: Coordinator

## 범위 결정

상위 로드맵은 조각 2 전체를 프로덕션 700~1,150줄, 테스트 650~1,050줄,
3~5일로 산정한다. 하루 조각으로 전부 완료했다고 주장하지 않는다. 이번 구현은
다음의 **조각 2a: durable Job/Queue truth**로 줄였다.

- 검증이 끝난 submit만 `SUBMITTED` Job으로 원자 저장
- `SUBMITTED -> PLANNING -> QUEUED` 규범 전이만 허용
- queue를 `(queued_at_unix_ms, job_id)` 순으로 결정적으로 조회
- `DEADLINE_PASSED`, `QUEUE_TIMEOUT`, `PERMANENTLY_INFEASIBLE`를 서로 다른
  durable 실패 사유로 보존
- 16바이트 submit idempotency key 재시도 및 동시 요청에서 중복 생성 방지
- SQLite `synchronous=FULL`, 1초 `busy_timeout`, 모든 read-check-write에
  `BEGIN IMMEDIATE` 사용
- u64은 SQLite signed integer 절단을 피하려고 기존
  `CoordinatorLeaseStore`와 같이 8바이트 big-endian BLOB으로 저장

## 왜 Attempt/fence를 이월했는가

상위 로드맵 조각 2에는 Attempt/fence 할당도 들어 있다. 그러나 규범
`docs/protocol/state-machines.md` §3은 Attempt를 **Job이 STAGING에 들어갈 때**
만들고, 같은 효과로 fence epoch 증가와 Lease 발급을 요구한다. Job 전이표도
`QUEUED -> STAGING` guard를 "선택 노드 lease 발급 성공"으로 둔다.

현재는 조각 4의 reservation과 조각 5의 실제 Grant dispatch가 없다. 별도 SQLite
트랜잭션으로 Attempt만 미리 만들면 Job/Attempt/Lease 사이에 split-brain 상태가
생기므로, 다음 **조각 2b**에서 `QUEUED -> STAGING`과 Attempt/fence/Lease를 한
권위 있는 연산으로 묶을 때 구현한다. 따라서 이 문서는 로드맵 조각 2 전체 완료나
durable Attempt 완료를 주장하지 않는다.

## 설계

### 타입과 API

`crates/coordinator/src/job_store.rs`에 다음을 둔다.

- `JobState::{Submitted, Planning, Queued, Failed}`
- `QueueFailure::{DeadlinePassed, QueueTimeout, PermanentlyInfeasible}`
- `AcceptedJobSubmission`: signature/quorum/hard-filter/policy 검증을 이미 통과한
  입력이라는 타입 경계
- `StoredJob`: immutable submit 사실, 상태별 시각, plan ID, 실패 사유, revision
- `CoordinatorJobStore::{open,is_durable,get,list_queued,submit_accepted,
  start_planning,enqueue,fail_queued}`

`submit_accepted()`는 submit 검증 자체를 흉내 내지 않는다. 상위 호출자가
`SUBMIT_UNAVAILABLE`, `SUBMIT_INFEASIBLE`, `SUBMIT_POLICY_VIOLATION`, 서명 실패를
판정한 뒤 accepted 입력만 전달해야 한다. 거부된 submit은 이 저장소에 Job을 만들지
않는다.

`enqueue()`의 `plan_id`는 조각 4가 만들 실행 가능 plan과
`PlacementRationale`의 durable 참조를 받을 자리일 뿐이며, 이번 조각이 plan이나
rationale를 계산·저장했다는 뜻이 아니다. 실제 rationale 저장 방식이 확정되기 전
이 API를 Coordinator 실행 경로에 연결하지 않는다.

### 멱등성과 동시성

`job_submission_idempotency` 테이블은 16바이트 key를 한 Job에만 결합한다.
동일 key와 동일 immutable 입력은 기존 레코드를 반환하고, 입력이 바뀌면
`IdempotencyConflict`, 다른 key가 기존 `job_id`를 재사용하면 `JobIdConflict`다.
조회와 삽입을 같은 `BEGIN IMMEDIATE`에 묶어 두 connection의 동시 최초 submit도
정확히 하나만 생성한다.

상태 전이는 현재 상태를 transaction 안에서 읽고 규범 source state와 대조한 뒤
update한다. 바로 직전 응답이 유실된 동일 목표 재호출은 저장된 결과를 반환한다.
완전한 `ControlStore` operation-result cache/10분 TTL/conformance suite는 이번
조각이 아니다.

### queue 포기 조건

마스터 플랜 §13.7과 상태 전이표의 엄격한 비교를 따른다.

- deadline: `now > deadline`; 같은 시각은 아직 실패 아님
- queue timeout: `now - queued_at > max_queue_duration`; 같은 경계는 실패 아님
- manifest `max_queue_minutes == 0`: domain에서는 `None`; 독립 timeout 없음
- permanent infeasible: 현재 offline인 것과 혼동하지 않도록 호출자가 증명 사유를
  비어 있지 않게 제공해야 함. 저장소는 빈 pool을 보고 이를 추론하지 않음
- clock rollback 및 손상된 BLOB/state/상태별 컬럼 조합: 변경 없이 fail closed

## 명시적 비범위

- Attempt 저장소, attempt ID/fence epoch 할당, `QUEUED -> STAGING`
- Lease/Grant/reservation과의 원자 결합
- manifest 서명, quorum, hard-filter, side-effect 정책 검증 producer
- `PLANNING -> FAILED`, 취소, 재계획, 실행 이후 전체 상태기계
- Raft `ControlStore`, `Committed` 보증, watch/cursor/snapshot
- Coordinator RPC/CLI/Agent 연결

특히 이 SQLite 저장소가 제공하는 것은 로컬 `DURABLE`뿐이다. 규범 전이표의
`COMMITTED`를 과반 합의 없이 제공한다고 주장하지 않는다. 현재 코드 경로와
연결되지 않은 저장 계층 조각이며, 실제 submit 서비스는 `Committed` 보증을 갖춘
ControlStore가 생기기 전까지 완성되지 않는다.

## negative tests

`job_store.rs` 단위 테스트와
`tests/job_store_concurrent_submit.rs`에 다음을 고정한다.

1. 공백 job/submitter, 0 queue duration, 공백 plan/영구 불가능 사유 거부
2. 동일 idempotency key의 변경 manifest 거부 및 원본 불변
3. 다른 key의 동일 job ID 거부
4. `SUBMITTED -> QUEUED` 건너뛰기 거부
5. enqueue 재시도의 다른 plan ID 거부 및 저장값 불변
6. event timestamp clock rollback 거부 및 상태 불변
7. deadline/queue timeout 정확한 경계 거부, +1ms에서만 실패
8. 독립 queue timeout 없음(`None`)을 0ms timeout으로 해석하지 않음
9. 영구 불가능 사유와 timeout/deadline 사유를 혼합·덮어쓰기하지 않음
10. terminal retry는 최초 timestamp/사유를 보존하고 다른 사유는 거부
11. unknown state, 잘린 u64 BLOB, 상태별 컬럼 shape 손상을 fail closed
12. 두 connection의 동일 submit 경쟁은 한 번 생성·한 번 replay
13. 두 connection의 동일 key/다른 payload 경쟁은 한 승자·한 conflict이며
    패자 Job은 생성되지 않음
14. u64 최대 시각 round-trip 및 결정적 FIFO/tie-break

## 뮤테이션 테스트

1. queue timeout guard를 `elapsed <= limit`에서 `elapsed < limit`로 바꿔
   정확한 경계가 실패 처리되게 했다. `queue_timeout_boundary_is_strict_and_
   one_millisecond_after_fails`가 assertion 실패(exit 1)했다. 원복했다.
2. 동일 key의 payload 대조를 `false && !stored.matches_submission(...)`으로
   무력화했다. `changed_payload_with_same_idempotency_key_is_rejected_without_
   overwrite`가 assertion 실패(exit 1)했다. 원복했다.

## 자체 재검토

구현 후 트랜잭션 lock 해제, 시간 경계, integer overflow, 손상 데이터,
fail-open, 상태/Lease 분리 위험을 다시 읽었다. 그 과정에서 초기 코드가
16/32바이트 값이 모두 0이라는 이유만으로 idempotency key/manifest digest를
거부했지만 규범에는 그런 금지가 없음을 발견해 그 과잉 검증을 제거했다. 또한
처음에는 FAILED row의 사유만 검사하고 timestamp 및 이전 상태 컬럼 조합을 전부
검사하지 않았으므로, 상태별 row shape 전체를 검증하도록 고쳤다.

`BEGIN IMMEDIATE` transaction 내부에서 guard 실패 시 `Transaction` drop으로
rollback과 lock 해제가 일어나며, 성공 시 update 뒤 commit한다. queue elapsed는
덧셈 대신 `checked_sub`를 써 overflow/clock rollback을 구분한다. Attempt를 미리
할당하지 않아 Job/Lease/fence의 비원자 분리를 만들지 않았다.

## 검증 결과

```text
cargo test -p gputeer-coordinator
  PASS — unit 47 + job concurrency 2 + lease concurrency 2, 0 failed

cargo fmt --all -- --check
  NOT RUN — stable toolchain에 cargo-fmt/rustfmt component가 설치되어 있지 않음

git diff --check
  PASS — whitespace 오류 없음 (기존 Windows LF/CRLF 경고만 있음)

cargo build --workspace --exclude gputeer-runtime-windows
  PASS — exit 0

cargo test --workspace --exclude gputeer-runtime-windows
  PASS — 모든 test target 통과, 0 failed, 기존 ignored 1건
```

## 제한과 다음 조각

이 구현은 실제 submit 검증 producer나 RPC가 호출하지 않으므로 E2E Job 제출을
증명하지 않는다. `PERMANENTLY_INFEASIBLE`의 사실성은 향후 다중 Agent inventory와
hard-filter 결과를 전달하는 호출자의 책임이며, 지금은 비어 있지 않은 사유만
강제한다. 조각 2b는 Attempt/fence/Lease의 단일 권위 연산과 full operation
idempotency를 설계해야 한다.
