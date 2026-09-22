---
schema_version: 2
id: DoD-51
claim: "검증된 terminal `AttemptReport` 를 durable `(job, attempt, node, fence)` 에 묶어, 재시작 시 증거 유실과 stale report 혼입을 막았다 — 하나의 `BEGIN IMMEDIATE` 안에서 report job/attempt와 durable Attempt job/attempt, report node와 single-node Attempt node, verified signer와 report/Attempt node, report fence와 Attempt fence, reservation job/attempt/node와 report identity를 5중 대조하고, reservation 부재·owner 불일치·non-terminal outcome·changed replay는 행 생성 없이 거부한다. 공개 저장 API는 `&Verified<pb::AttemptReport>`만 받고 필드는 `Verified::get()` 뒤에만 읽으며, signature 포함 body와 저장소가 직접 계산한 BLAKE3 hash를 first-write fact로 보존한다. 독립 검수 1라운드 ACCEPTED와 감독자 coordinator 108 passed로 확인했다. Job/Attempt terminal 전이와 artifact/runtime-stop guard, Lease revoke·reservation release·production wire는 완료하지 않았다"
status: PASS
commit: 8a58e97d0d1fef2db623910fa47f82b71c505458

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — verified terminal AttemptReport durable Attempt/reservation binding, replay·rollback·corruption negative test와 뮤테이션 검증"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-24T12:57:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 fresh-read-only 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "production 공개 API가 &Verified<pb::AttemptReport>만 받고 raw 저장 API·다른 production INSERT가 없으며 필드 최초 관찰이 verified.get() 이후임, BEGIN IMMEDIATE 안의 report↔Attempt job/attempt·single node·verified signer·fence·reservation owner 5중 대조, exact replay가 전체 protobuf 의미와 signer가 같은 기존 행만 반환하고 생성·변경하지 않아 stale 우회가 아님, raw load가 Verified가 아니고 미검증 production 소비자가 없음, terminal outcome 5종 제한, fault 전체 rollback과 Job/Attempt/Lease/reservation UPDATE 부재, typed corruption fail-closed, staging helper 추출의 기존 공개 API·SQL 의미 불변을 확인해 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-51_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-51_scheduler_terminal_attempt_report_binding_2026-08-24.txt"
raw_output_digest: "sha256:ac63e7c6842e8a36763e4caee1ed5413733bff38d7a2794e13c8c80e2aca06a8"
raw_output_bytes: 9214

binary_digests:
  toolchain: "cargo 사용 — 제공된 감독자 재실행 이력에 cargo/rustc version과 binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — 기존 signed AttemptReport를 coordinator local SQLite schema와 Rust 저장 API에 결합"
  canonical_spec: "기존 AttemptReport Verified 서명 검증 경계를 계승 — load 시 현재 authoritative key directory 재검증과 terminal consumer는 범위 밖"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test / SQLite local store"
hardware: "GPU 미사용 — signed protobuf fixture와 SQLite binding/corruption/fault fixture로 검증"
network_profile: "네트워크 미사용 — coordinator 단위·통합 테스트이며 production wire routing은 미연결"
command: |
  cargo test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-51_scheduler_terminal_attempt_report_binding_2026-08-24.txt,
   docs/evidence/_raw/DoD-51_review.txt 전문 참조)

  감독자 직접 확인: PASS — coordinator unit 104 + integration 4 = 108 passed,
  전체 0 failed, exit code 0
  독립 검수 1라운드: ACCEPTED — Verified 전용 저장 경계와 get() 이후 필드 관찰,
  단일 BEGIN IMMEDIATE 안의 Attempt/reservation 5중 대조, replay 비우회성, raw load,
  terminal outcome 5종, rollback·control state 무변경, corruption fail-closed와
  staging helper 무회귀를 확인
artifacts:
  - docs/plans/2026-08-24_1238_terminal_attempt_report_binding_v1.md
  - crates/coordinator/src/attempt_report_store.rs
  - crates/coordinator/src/staging_store.rs
  - crates/coordinator/src/lib.rs
  - docs/reports/2026-08-24_1257_terminal_attempt_report_binding.md
  - docs/evidence/_raw/DoD-51_scheduler_terminal_attempt_report_binding_2026-08-24.txt
  - docs/evidence/_raw/DoD-51_review.txt
negative_tests:
  - "all_five_known_terminal_outcomes_are_stored: COMPLETED·FAILED·INTERRUPTED·CANCELLED·STALE_COMPLETED 다섯 terminal outcome만 저장 가능함을 확인"
  - "unspecified_and_unknown_outcomes_create_no_row: UNSPECIFIED와 미지 enum은 report row를 만들지 않고 거부"
  - "wrong_job_attempt_and_stale_fence_create_no_row: report와 durable Attempt의 job/attempt/fence 불일치는 row 없이 fail closed"
  - "attempt_node_and_verified_signer_are_bound_to_durable_owner: report node·single-node Attempt node·Verified signer의 3자 일치를 강제"
  - "missing_or_different_reservation_owner_creates_no_row: reservation 부재 또는 job/attempt/node owner 불일치가 row를 만들지 않음"
  - "exact_replay_returns_first_row_and_changed_body_or_signature_conflicts: exact protobuf 의미와 signer replay만 기존 first-write binding을 반환하고 changed body/signature는 conflict"
  - "signature_and_binding_survive_reopen_without_state_or_release_side_effects: 재시작 뒤 signature 포함 body와 binding이 복원되며 control state와 reservation은 불변"
  - "failure_after_insert_rolls_back_report_and_preserves_control_state: report INSERT 뒤 fault가 전체 rollback되고 Job/Attempt/Lease/reservation은 보존"
  - "corrupt_body_hash_identity_signer_and_fence_fail_closed: body/hash/job/attempt/node/signer/fence 손상을 typed corruption으로 거부"
  - "뮤테이션 1: node/signer binding guard 제거 시 지정 negative test가 exit 101로 실패"
  - "뮤테이션 2: fence guard 제거 시 지정 negative test가 exit 101로 실패"
limitations:
  - "load 시 서명을 재검증하지 않는 것은 의도된 설계 — terminal consumer가 당시 authoritative key directory로 다시 검증해야 한다"
  - "Job/Attempt terminal 상태 전이, nested artifact 검증, runtime-stop 증명, Lease revoke/reservation release, production wire routing 은 범위 밖"
  - "Job terminal 전이는 규범상 RUNNING 상태, canonical Attempt와 final artifact COMMITTED guard를 선행해야 하므로 이 evidence 저장과 분리했다"
  - "저장 성공은 프로세스 종료나 artifact/checkpoint durability를 증명하지 않으며 reservation release의 충분조건이 아니다"
  - "authoritative device→member 해석, `JobRequirements` projection과 기본 `MIRRORED` durability 소비는 범위 밖"
  - "local SQLite DURABLE first-write fact만 증명하며 다중 Coordinator 합의나 Raft COMMITTED를 증명하지 않는다"
decision: "이 조각을 Job/Attempt terminal 상태 전이, artifact/checkpoint 검증, runtime-stop 증명, Lease revoke, reservation release 또는 production routing 완료로 과장하지 않고 verified terminal AttemptReport durable Attempt/reservation binding으로 완료했다. 공개 저장 API는 오직 `&Verified<pb::AttemptReport>`만 받고 report 필드는 `Verified::get()` 뒤에만 관찰한다. signature를 포함한 protobuf body와 저장소가 직접 계산한 BLAKE3 hash를 PK `(attempt_id, node_id)`의 first-write fact로 저장한다. 신규 row를 쓰기 전 하나의 `BEGIN IMMEDIATE` 안에서 report job/attempt와 durable Attempt job/attempt, report node와 single-node Attempt node, `Verified::signer_id()`와 report/Attempt node, report fence와 Attempt fence, reservation job/attempt/node와 report identity를 5중 대조한다. reservation이 없거나 owner가 다르면 행이 생기지 않는다. exact replay는 전체 protobuf 의미와 signer가 같은 기존 행을 생성·변경 없이 반환하므로 stale 우회가 아니며 changed body/signature는 conflict다. load는 의도적으로 `Verified`가 아니고 검증 없이 쓰는 production consumer가 없으며 손상은 fail closed한다. 허용 outcome은 COMPLETED·FAILED·INTERRUPTED·CANCELLED·STALE_COMPLETED 다섯 종뿐이다. fault는 전체 rollback하고 production 경로는 Job/Attempt/Lease/reservation을 갱신하거나 해제하지 않는다. 자체 재검토에서 미사용 fixture 필드 경고와 오류 문자열을 정리했다. 독립 검수는 이 경계와 staging helper의 기존 공개 API·SQL 의미 불변을 확인해 1라운드 만에 ACCEPTED했고 감독자는 coordinator unit 104+integration 4=108 passed를 직접 재확인했다. scheduler 로드맵 진행: `DoD-41`~`DoD-51` 완료 — authoritative device→member 해석, `JobRequirements` projection, `MIRRORED` 소비, Job terminal 전이(RUNNING·artifact guard 선행), reservation release, production 연결은 후속"
---

# DoD-51 · verified terminal AttemptReport durable binding

## 무엇을 입증하려 했는가

`DoD-50`까지 scheduler는 hard-filter, durable Job/Queue, local atomic STAGING, durable
inventory, deterministic best-fit, private orchestration, inventory CAS reservation, selected
GPU binding과 verified signed `JobManifest` binding을 갖췄다. 그러나 Agent가 보낸 signed
terminal `AttemptReport`를 현재 durable Attempt·reservation owner와 묶어 보존하는 inbox가
없어, coordinator 재시작 뒤 evidence를 잃거나 stale/wrong-attempt report가 향후 terminal
reconciliation과 reservation release 입력에 섞일 수 있었다.

설계 조사에서 authoritative device→member 해석은 committed member/device apply state와
revision-consistent key directory가 없어 1일을 넘고, 기본 `MIRRORED` 소비는 replica 목적지·
failure domain·저장/전송 producer가 없어 normalization만으로 admission 공백을 닫지 못한다고
판정했다. Job terminal 전이도 단순 enum/column 추가가 아니다. 규범상 Job은 먼저 `RUNNING`
이어야 하며 canonical Attempt와 final artifact `COMMITTED` guard를 건너뛸 수 없다. 현재
durable 상태는 Job `STAGING`, Attempt `CREATED`까지만 있으므로 곧바로 terminal로 바꾸면
상태기계를 우회한 거짓 truth가 된다.

따라서 이 조각은 report를 해석해 상태를 바꾸거나 reservation을 해제하지 않고, 현재 owner가
제출한 검증된 terminal evidence를 durable first-write fact로 보존하는 선행 경계만 검증했다.

## 구현 — `Verified` 전용 inbox와 signature 포함 durable body

신규 `CoordinatorAttemptReportStore`에
`store_verified_terminal_report(&Verified<pb::AttemptReport>)`와 raw load
`get_report_binding()`을 추가했다. `coordinator_attempt_reports`는 PK
`(attempt_id, node_id)`, `job_id`, big-endian BLOB `fence_epoch`,
`verified_signer_id`, signature를 포함한 `report_body`, 저장소가 body에서 직접 계산한
BLAKE3 `report_hash`를 저장한다.

저장 API는 raw report를 받지 않는다. `Verified::get()` 이후에만 report 필드를 읽고
`Verified::signer_id()`를 꺼낸다. caller hash도 받지 않으며 signature를 포함한 protobuf
body에서 직접 hash를 계산한다. terminal outcome은 `COMPLETED`, `FAILED`, `INTERRUPTED`,
`CANCELLED`, `STALE_COMPLETED` 다섯 종만 허용하고 `UNSPECIFIED`와 미지 enum은 INSERT 전에
거부한다.

`staging_store.rs`는 schema 초기화와 Attempt/reservation read helper 두 개만
`pub(crate)`로 추출했다. report store가 기존 durable truth를 같은 transaction에서 읽기 위한
crate 내부 경계이며, 기존 공개 API와 SQL 의미는 바뀌지 않았다.

## 핵심 안전 속성 — 한 transaction 안의 5중 대조

신규 report row는 하나의 `BEGIN IMMEDIATE` 안에서 다음 다섯 대조를 모두 통과해야 한다.

1. report `(job_id, attempt_id)` = durable Attempt `(job_id, attempt_id)`.
2. report `node_id` = single-node durable Attempt의 유일한 node.
3. `Verified::signer_id()` = report node = Attempt node.
4. report `fence_epoch` = durable Attempt fence epoch.
5. reservation `(job_id, attempt_id, node_id)` = report `(job_id, attempt_id, node_id)`.

reservation이 없거나 현재 owner가 다르면 report row는 생기지 않는다. report INSERT 직후
fault도 transaction 전체를 rollback한다. production 경로에는 Job, Attempt, Lease,
reservation UPDATE/DELETE가 없으므로 evidence 저장이 terminal state 전이나 release를
암묵적으로 일으키지 않는다.

## replay와 load 경계

exact replay는 기존 row의 protobuf 전체 의미와 submission-time signer를 대조해 둘 다
같을 때 최초 binding을 `created=false`로 반환한다. 기존 행을 생성하거나 변경하지 않으므로
stale report를 새로 넣는 우회가 아니다. body 또는 signature가 다르면 conflict다.

`get_report_binding()`은 의도적으로 raw `StoredAttemptReportBinding`을 반환한다. load
시점의 authoritative key directory에서 서명을 다시 검증하기 전에는 report를 Job/Attempt
terminal 결정이나 reservation release에 쓰면 안 된다. loader는 empty/undecodable body,
hash, job/attempt/node identity, signer, fence encoding·값과 durable Attempt 재대조 손상을
typed corruption으로 fail closed한다. 이 raw 결과를 검증 없이 소비하는 production 경로는
없다.

## 자체 재검토 — warning과 오류 문자열 정리

자체 재검토에서 fixture의 미사용 필드 경고를 제거하고 오류 문자열이 실제 실패 의미를
정확히 나타내도록 정리했다. 안전 guard나 production 동작을 완화하지 않았고 compiler
warning 없는 상태로 관련 suite를 다시 통과했다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 `attempt_report_store.rs:137,184`에서 공개 저장 API가
`&Verified<...>`만 받고 raw 저장 API·다른 production INSERT가 없음을 확인했다. report
필드 최초 관찰은 `:196`의 `verified.get()` 뒤다. `:204,225,300,329`에서 5중 대조가 같은
`BEGIN IMMEDIATE` transaction 안에 있고 reservation 부재·owner 불일치는 INSERT 전에
실패한다.

`:209`의 exact replay가 기존 행을 생성·변경하지 않고 protobuf 전체 의미와 signer를
대조하므로 stale 우회가 아님을 확인했다. `:171,431`의 load 결과는 `Verified`가 아니고
검증 없이 쓰는 production consumer가 없다. terminal outcome 5종 제한은 `:287`, fault 전체
rollback과 Job/Attempt/Lease/reservation UPDATE 부재는 `:239,256`, 손상 fail-closed는
`:386`에서 대조했다. `staging_store.rs:198,616,770`의 helper 추출도 기존 공개 API와 SQL
의미를 바꾸지 않았다. 수정 요청 없이 1라운드에서 `ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-coordinator
  PASS — unit 104 + integration 4 = 108 passed, 0 failed
```

감독자가 위 결과를 직접 재확인했다. 구현 세션의 workspace build와 Windows runtime 제외
workspace test도 통과했으며 기존 crypto crash-child fixture 1건만 ignored였다. `rustfmt`
component가 없어 `cargo fmt --check`는 실행하지 못했지만 Rust compiler warning은 없었다.

## 이 evidence가 증명하지 않는 것

- load 시 서명을 재검증하지 않는 것은 의도된 설계다. terminal consumer가 당시
  authoritative key directory로 다시 검증해야 한다.
- Job/Attempt terminal 상태 전이는 범위 밖이다. 특히 Job terminal은 `RUNNING`, canonical
  Attempt와 final artifact `COMMITTED` guard를 선행해야 한다.
- nested artifact/checkpoint validity와 durability 검증, runtime-stop 증명은 범위 밖이다.
- Lease revoke, reservation release와 production wire routing은 범위 밖이다.
- authoritative device→member 해석, `JobRequirements` projection과 기본 `MIRRORED`
  durability 소비는 범위 밖이다.
- local SQLite의 durable first-write evidence만 증명하며 다중 Coordinator 합의나 Raft
  `COMMITTED`를 증명하지 않는다.

## 결정

1. 공개 저장 경계를 `&Verified<pb::AttemptReport>`로 제한하고 report 필드는
   `Verified::get()` 뒤에만 관찰한다.
2. signature 포함 protobuf body와 저장소가 직접 계산한 BLAKE3 hash를 저장한다.
3. 신규 row 전에 하나의 `BEGIN IMMEDIATE` 안에서 Attempt identity·single node·signer·fence·
   reservation owner를 5중 대조하고 reservation 부재/불일치는 row 없이 거부한다.
4. exact replay는 전체 protobuf 의미와 signer가 같은 기존 first-write fact만 반환하고,
   changed body/signature와 corruption은 fail closed한다.
5. load는 의도적으로 `Verified`가 아니며 재검증 전 terminal/release에 쓰지 않는다.
6. Job/Attempt/Lease/reservation 상태와 ownership은 이 저장 경로에서 변경하지 않는다.
7. 독립 검수는 1라운드 `ACCEPTED`, 감독자 재실행은 coordinator 108 passed였다.
8. scheduler 로드맵 진행: `DoD-41`~`DoD-51` 완료 — authoritative device→member 해석,
   `JobRequirements` projection, `MIRRORED` 소비, Job terminal 전이(RUNNING·artifact guard 선행),
   reservation release, production 연결은 후속

관련: `docs/plans/2026-08-24_1238_terminal_attempt_report_binding_v1.md`
