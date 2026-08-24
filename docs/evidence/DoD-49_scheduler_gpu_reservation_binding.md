---
schema_version: 2
id: DoD-49
claim: "`DoD-48` 의 selected GPU 식별자를 inventory CAS·node reservation·STAGING 과 같은 트랜잭션에 영속화해, 재시작 시 유실되던 문제를 닫았다. canonical GPU ID 목록 검증, node-scoped inventory 존재 확인, operation payload binding, durable child row 복원·대조와 손상 fail-closed를 구현하고 binding 직후 fault 전체 rollback·replay/OperationConflict·Barrier 경쟁·DoD-43 무회귀·뮤테이션 2건·독립 검수 1라운드 ACCEPTED·감독자 coordinator 92 passed로 확인했다. 단 이는 local SQLite의 node-exclusive durable binding이며 Grant/Lease scope·NVML UUID provenance·GPU별 capacity accounting·reservation release·production wire는 완료하지 않았다"
status: PASS
commit: 3a85bed5236f8751e8ab616337abb6549704a964

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — selected GPU reservation child binding, operation payload·replay·손상 검증, fault/concurrency/negative test와 뮤테이션 검증"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-24T11:18:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "단일 BEGIN IMMEDIATE의 inventory revision CAS·node reservation·GPU binding·Attempt/Lease/fence·STAGING·operation 원자성과 binding 직후 fault 전체 rollback, node-scoped SQL WHERE node_id = ?1 AND gpu_id = ?2와 다른 node 동일 ID fixture, child 부재·ordinal gap·blank/duplicate/비정렬 손상 fail-closed, GPU count+length-delimited ID operation payload와 exact replay/OperationConflict, 기존 DoD-43 reservation-free API 무회귀, 실제 Barrier 경쟁의 성공 1건·loser QUEUED·winner row만 존재, orchestration single/N exact ID 전달, 뮤테이션 2건, staging_store.rs·orchestrate.rs+문서 2개로 제한되고 inventory_store/job_store/scheduler/proto/wire 무변경임을 확인해 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-49_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-49_scheduler_gpu_reservation_binding_2026-08-24.txt"
raw_output_digest: "sha256:033dba1fce5d7bf9d3086ac244b3c54cdbc3cda11121207efd00528bd17657f7"
raw_output_bytes: 7791

binary_digests:
  toolchain: "cargo 사용 — 제공된 감독자 재실행 이력에 cargo/rustc version과 binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — coordinator private staging request/result와 local SQLite schema만 확장"
  canonical_spec: "wire canonical/signature 계약 미사용 — snapshot gpu_id의 NVML UUID provenance와 GrantedExecutionPlan·ResourceScope·ExecutionGrant 생성/서명/전송은 범위 밖"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test / SQLite local store"
hardware: "GPU 미사용 — 합성 node inventory와 SQLite fixture로 selected GPU durable binding 검증"
network_profile: "네트워크 미사용 — coordinator 단위·통합 테스트만 실행하고 production run()/accept-loop/wire는 미변경"
command: |
  cargo test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-49_scheduler_gpu_reservation_binding_2026-08-24.txt,
   docs/evidence/_raw/DoD-49_review.txt 전문 참조)

  감독자 직접 확인: PASS — coordinator unit 88 + integration 4 = 92 passed,
  전체 0 failed, exit code 0
  독립 검수 1라운드: ACCEPTED — 단일 transaction 원자성, binding 직후 전체 rollback,
  node-scoped inventory 검사, 손상 fail-closed, replay/conflict, DoD-43 무회귀,
  Barrier 경쟁, single/N 전달, 뮤테이션 2건과 제한 범위를 확인
artifacts:
  - docs/plans/2026-08-24_1103_scheduler_selected_gpu_reservation_binding_v1.md
  - docs/reports/2026-08-24_1118_scheduler_selected_gpu_reservation_binding.md
  - crates/coordinator/src/staging_store.rs
  - crates/coordinator/src/orchestrate.rs
  - docs/evidence/_raw/DoD-49_scheduler_gpu_reservation_binding_2026-08-24.txt
  - docs/evidence/_raw/DoD-49_review.txt
negative_tests:
  - "invalid_or_missing_selected_gpu_ids_fail_before_staging: 빈/공백/중복/비정렬 ID와 선택 node에 없는 GPU ID를 typed error로 거부하고, 같은 ID가 다른 node에만 있어도 SelectedGpuMissing임을 확인"
  - "missing_blank_noncanonical_or_gapped_stored_gpu_binding_fails_closed: child row 없는 legacy reservation, ordinal gap, blank, duplicate, 비정렬 durable binding을 CorruptData로 거부"
  - "selected_gpu_binding_is_in_operation_payload_and_replay_returns_original: GPU count+length-delimited ID가 operation payload에 결합되고 exact replay/reopen은 최초 binding을 복원하며 changed ID는 OperationConflict임을 확인"
  - "binding 직후 injected fault: reservation·GPU binding·Attempt·Lease·fence·Job 전이·operation의 전체 rollback과 fence 미소비를 확인"
  - "revision 0, u64::MAX, 복수 GPU canonical ordering의 encoding·round-trip을 확인"
  - "Barrier 경쟁: 두 connection 중 성공 1건, loser QUEUED, winner reservation과 GPU child row만 각각 정확히 1개 존재"
  - "reservation-free DoD-43 regression: stage_queued_with_lease()가 빈 selected GPU 목록을 무시하고 기존 staging/replay 계약을 유지"
  - "뮤테이션 1: operation payload에서 GPU count/ID 제거 시 changed payload conflict test 실패"
  - "뮤테이션 2: inventory membership 검사 우회 시 missing GPU negative test가 실패하고 존재하지 않는 GPU STAGING 반례를 검출"
limitations:
  - "reservation release 경로는 여전히 없다 — 실행 종료 증명 없이 구현하면 중복 실행 위험이 생긴다는 설계 판단으로 후순위"
  - "Grant/Lease scope 생성, NVML UUID provenance, GPU 별 capacity accounting, production wire 연결은 범위 밖"
  - "node_id PRIMARY KEY의 node-exclusive reservation 정책은 그대로여서 같은 node의 서로 다른 GPU를 독립 할당하는 GPU-exclusive/shared/MIG reservation은 제공하지 않는다"
  - "원본 Manifest durable 저장·strict JobRequirements 변환기·submitter membership lookup·Job submit ingress는 없다"
  - "durable outbox, publish/ACK/timeout 상태, terminal Attempt report와 runtime stop 증명이 없다"
  - "local SQLite DURABLE만 증명하며 다중 Coordinator 합의나 Raft COMMITTED를 증명하지 않는다"
  - "private orchestrate module은 production run()/accept-loop에 연결되지 않았다"
  - "실제 GPU와 네트워크를 사용하지 않았고 반환 ID는 scheduler snapshot의 opaque gpu_id다"
decision: "이 조각을 GPU별 capacity allocation, Grant scope 완성, 안전한 release나 production dispatch로 과장하지 않고 selected GPU durable reservation binding으로 완료했다. `StageQueuedRequest`와 `StoredNodeReservation`은 canonical `selected_gpu_ids`를 보존하고, `(node_id, ordinal)` PRIMARY KEY·`(node_id, gpu_id)` UNIQUE child table이 순서를 durable하게 저장한다. 빈/공백/중복/비정렬 요청과 node-scoped inventory 부재를 typed error로 거부하며, child 부재·ordinal gap·blank/duplicate/비정렬 저장 손상은 `CorruptData`로 fail closed한다. GPU count와 length-delimited ID를 operation payload에 넣어 exact replay는 최초 binding을 복원하고 changed payload는 `OperationConflict`로 막는다. binding 직후 fault의 전체 rollback, 실제 Barrier 경쟁의 정확히 한 winner, DoD-43 reservation-free 경로 무회귀와 두 뮤테이션으로 판별력을 확인했다. 자체 재검토는 같은 ID가 다른 node에만 있는 fixture를 보강했고 독립 검수는 SQL `WHERE node_id = ?1 AND gpu_id = ?2`를 대조해 그 보고가 사실임을 확인한 뒤 1라운드 만에 ACCEPTED했다. 감독자는 coordinator 92 passed를 직접 재확인했다. reservation release는 실행 종료 또는 never-published 사실을 durable하게 증명할 경로가 생긴 뒤 구현해야 하며 Grant/Lease scope, NVML UUID provenance, GPU별 capacity accounting과 production wire도 후속이다. scheduler 로드맵 진행: 조각 1·2a·2b-1·3a·4·5·5b·GPU assignment·GPU reservation binding 완료 — Manifest→ JobRequirements 변환기, reservation release, Grant scope 생성, production 연결은 후속"
---

# DoD-49 · selected GPU durable reservation binding

## 무엇을 입증하려 했는가

`DoD-48`은 scheduler snapshot에서 선택한 canonical `selected_gpu_ids`를 orchestration의
`Staged` outcome까지 보존했지만, durable reservation에는 node/job/attempt/revision만
남았다. STAGING commit 직후 Coordinator가 재시작하면 그 Attempt가 CAS를 통과할 때
정확히 선택한 GPU 집합을 복원할 수 없었다. 최신 inventory에서 다시 계산하면 다른
revision이나 GPU를 가리킬 수 있어 admission 사실과 나중의 Grant 입력이 갈라진다.

이번 조각은 selected GPU IDs를 inventory revision CAS, node reservation,
Attempt/Lease/fence, `QUEUED -> STAGING`, operation idempotency와 같은 local SQLite
transaction에 저장하고 재시작·replay 뒤 복원하는 경계만 검증했다.

설계 조사에서 Manifest 변환기는 verified 원본 저장과 submitter membership source가
없는 상태라 durable submit 경계를 닫지 못한다고 판단했다. reservation release는 더
위험하다. durable outbox/publish 상태, terminal report 또는 runtime stop 증명 없이 row만
지우면 아직 실행 중인 workload와 새 Job이 겹쳐 중복 실행을 만들 수 있다. 따라서
release는 단독 구현하지 않고 실행 종료나 never-published 사실과 원자 결합할 후속으로
두었다.

## 구현 — selected GPU IDs를 reservation과 같은 transaction에 결합

`StageQueuedRequest`와 `StoredNodeReservation`에 `selected_gpu_ids`를 추가했다.
`coordinator_node_reservation_gpus(node_id, gpu_id, ordinal)` child table은
`(node_id, ordinal)`을 primary key로, `(node_id, gpu_id)`를 unique로 강제해 canonical
순서를 durable하게 보존한다. child row가 전혀 없는 기존 reservation은 암묵적 빈
assignment로 해석하지 않고 unbound legacy `CorruptData`로 거부한다.

reservation-aware 경로는 빈 목록, 공백 ID, 중복 ID와 `gpu_id` 오름차순이 아닌 목록을
거부한다. inventory revision CAS와 같은 transaction에서 각 ID가 선택 node의
inventory에 존재하는지 `(node_id, gpu_id)` 쌍으로 확인하고, 없으면
`ReservedStageError::SelectedGpuMissing`을 반환한다.

operation payload에는 GPU 개수와 length-delimited ID bytes를 포함한다. exact replay는
최초 durable binding을 복원해 요청과 대조하고, 같은 operation key에 다른 GPU 목록이
오면 `OperationConflict`로 실패한다. reservation, GPU binding, Attempt/Lease/fence, Job
STAGING, operation 기록은 한 `BEGIN IMMEDIATE`에서 쓰고 한 번만 commit한다.

`orchestrate.rs`의 single/N 후보는 각각 scheduler의 같은 canonical GPU IDs를
`StageQueuedRequest`에 전달하고, 저장된 reservation에서 같은 목록을 재조회한다.

## 자체 재검토 — 전역 GPU ID 검사로 약화될 가능성을 닫음

초기 missing-GPU fixture는 선택 node에 ID가 없는 경우를 검사했지만, 구현이 node 조건을
빼고 전역 GPU ID 존재만 확인해도 통과할 여지가 있었다. 자체 재검토에서 이를 발견해
같은 ID를 다른 node에만 등록한 fixture를 추가했다. 선택 node의 요청은 여전히
`SelectedGpuMissing`으로 거부되고 staging side effect가 남지 않는다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 `staging_store.rs:323,399,409`에서 단일 `BEGIN IMMEDIATE` transaction과 commit
경계를 대조하고 `:1351`의 binding 직후 fault가 reservation, child binding,
Attempt/Lease/fence, Job, operation을 전부 rollback함을 확인했다. `:741`의 실제 SQL은
`WHERE node_id = ?1 AND gpu_id = ?2`이고 `:1411`의 다른-node 동일-ID fixture가 이를
판별하므로 자체 재검토 보고가 사실이라고 확인했다.

`staging_store.rs:806,1470`의 child 부재·ordinal gap·blank/duplicate/비정렬 손상
fail-closed, `:711,337,882`의 GPU-bound operation payload·exact replay·
`OperationConflict`, `:293,1639`의 기존 `DoD-43` reservation-free 경로 무회귀도
대조했다. `:1198`의 실제 `Barrier` 경쟁은 성공 1건, loser QUEUED, winner에만
reservation과 GPU child row가 각각 정확히 1개임을 보였다.

두 뮤테이션은 GPU payload binding과 node inventory membership 검사가 장식이 아님을
증명했다. orchestration single/N 전달과 `staging_store.rs`·`orchestrate.rs`+문서 2개에
한정된 범위, `inventory_store.rs`·`job_store.rs`·scheduler·proto·wire 무변경까지 확인해
수정 요청 없이 1라운드에서 `ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-coordinator
  PASS — unit 88 + integration 4 = 92 passed, 0 failed
```

위 결과는 감독자가 직접 재확인했다. 구현자의 workspace build/test와
`git diff --check`도 PASS다. stable toolchain에 `cargo-fmt`/`rustfmt` component가 없어
`cargo fmt`는 실행하지 못했다.

## 이 실험이 증명하지 "않는" 것

- reservation release 경로는 여전히 없다 — 실행 종료 증명 없이 구현하면 중복 실행
  위험이 생긴다는 설계 판단으로 후순위다.
- Grant/Lease scope 생성, NVML UUID provenance, GPU 별 capacity accounting,
  production wire 연결은 범위 밖이다.
- node-exclusive reservation을 GPU-exclusive/shared/MIG allocation으로 바꾸지 않았다.
- 원본 Manifest 저장, strict `JobRequirements` 변환기와 Job submit ingress는 없다.
- durable outbox, publish/ACK/timeout과 terminal Attempt/runtime stop 증명은 없다.
- local SQLite `DURABLE`만 다루며 Raft `COMMITTED`를 증명하지 않는다.
- private orchestration은 production `run()`/accept-loop에 연결되지 않았다.

## 결정

1. `DoD-48`의 canonical selected GPU IDs를 inventory CAS·node reservation·STAGING과 같은
   transaction에 durable하게 결합해 restart/replay에서 assignment가 사라지는 문제를
   닫았다.
2. child table의 key/unique 제약, canonical request 검사, node-scoped inventory 대조와
   operation payload binding으로 저장·replay 양쪽을 fail closed하게 만들었다.
3. binding 직후 fault rollback, durable 손상 fixture, exact replay/conflict, 실제 Barrier
   경쟁, DoD-43 무회귀와 뮤테이션 2건으로 판별력을 확인했다.
4. 자체 재검토의 다른-node 동일-ID fixture와 독립 검수의 실제 SQL 대조로 node-scoped
   inventory 검사가 전역 ID 존재 검사로 약해지지 않았음을 확인했다. 독립 검수는
   1라운드 `ACCEPTED`, 감독자 재실행은 coordinator 92 passed였다.
5. scheduler 로드맵 진행: 조각 1·2a·2b-1·3a·4·5·5b·GPU assignment·GPU reservation binding 완료 — Manifest→ JobRequirements 변환기, reservation release, Grant scope 생성, production 연결은 후속

관련: `docs/plans/2026-08-24_1103_scheduler_selected_gpu_reservation_binding_v1.md`
