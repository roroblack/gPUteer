---
schema_version: 2
id: DoD-47
claim: "scheduler 로드맵 조각 5b(inventory revision 기반 CAS reservation)를 완료해, `DoD-46` 이 명시적으로 인정했던 자원 중복 선택 위험을 닫았다. CandidateSnapshot의 revision token을 inventory projection부터 선택까지 보존하고, operation replay→revision 비교→node-exclusive reservation→Attempt/Lease/fence→Job STAGING→operation 기록을 하나의 BEGIN IMMEDIATE transaction에서 원자 처리하며 CAS·점유 충돌을 재시도 없이 거부함을 구현·독립 검수 1라운드 ACCEPTED·감독자 coordinator 테스트 86/86으로 확인했다. 단 node-exclusive라 같은 node의 다른 GPU도 동시에 쓸 수 없고 release가 없으며 private orchestration kernel은 production run()에 연결되지 않았다"
status: PASS
commit: b929472989872443f64e84daa5c75b6d8729ffc2

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — inventory revision projection, node-exclusive CAS staging과 실제 파일 DB 동시성·뮤테이션 검증"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-21T16:04:20+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "단일 BEGIN IMMEDIATE transaction의 operation replay·inventory revision CAS·node reservation·Attempt/Lease/fence·Job STAGING·operation 기록·commit 원자성, rollback fence 미소비, CAS/점유 경쟁의 동일 transaction 강제, 실제 Barrier 경쟁에서 정확히 1건 성공과 loser QUEUED 유지, revision predicate와 node uniqueness 뮤테이션 2건의 판별력, DoD-43 기존 stage_queued_with_lease() 무회귀, scheduler model과 coordinator 3개 production 파일 중심의 제한된 범위 및 node-exclusive·release 없음·production 미연결 한계를 확인해 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-47_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-47_scheduler_inventory_cas_2026-08-21.txt"
raw_output_digest: "sha256:a66029d07e879d9229c71383534ce6a65dc16f3e6e23f37a918f27809f3f2659"
raw_output_bytes: 6692

binary_digests:
  toolchain: "C:\\Users\\playdata2\\.cargo\\bin\\cargo.exe 사용 — 제공된 이력과 이번 실행에 rustc/cargo version·binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — scheduler model의 process-local optional inventory revision과 Coordinator SQLite admission API"
  canonical_spec: "wire canonical/signature 계약 미사용 — Grant 생성·서명·전송이나 production run() 경로에 연결되지 않음"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test"
hardware: "GPU 미사용 — 합성 단일-GPU inventory와 임시 SQLite control DB로 CAS reservation 경계 검증"
network_profile: "네트워크 미사용 — private orchestration 및 저장소 단위 테스트만 실행하고 accept-loop/wire 경로는 미변경"
command: |
  C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-47_scheduler_inventory_cas_2026-08-21.txt,
   docs/evidence/_raw/DoD-47_review.txt 전문 참조)

  감독자 직접 확인: PASS — coordinator unit 82 + integration 4 = 86 passed, 0 failed
  독립 검수 1라운드: ACCEPTED — 단일 transaction 원자성, rollback fence 미소비,
  CAS·node 점유 경쟁, Barrier 동시성, 뮤테이션 2건, DoD-43 무회귀와 범위를 확인
artifacts:
  - docs/plans/2026-08-21_1537_scheduler_inventory_cas_v1.md
  - docs/reports/2026-08-21_1553_scheduler_inventory_cas.md
  - crates/scheduler/src/model.rs
  - crates/scheduler/tests/hard_filter.rs
  - crates/scheduler/tests/best_fit.rs
  - crates/coordinator/src/inventory_store.rs
  - crates/coordinator/src/staging_store.rs
  - crates/coordinator/src/orchestrate.rs
  - docs/evidence/_raw/DoD-47_scheduler_inventory_cas_2026-08-21.txt
  - docs/evidence/_raw/DoD-47_review.txt
negative_tests:
  - "stale_or_missing_inventory_fails_before_any_staging_side_effect: stale 또는 missing revision을 reservation·Attempt·Lease·operation·Job 전이 없이 거부하고 Job을 QUEUED로 유지"
  - "sequential_distinct_jobs_cannot_reserve_the_same_node_gpu: 먼저 예약된 node/GPU를 다른 Job이 순차 예약하지 못함"
  - "concurrent_distinct_jobs_reserving_one_gpu_commit_exactly_once: 별도 connection 두 개와 Barrier 경쟁에서 정확히 1건만 성공하고 loser는 QUEUED 유지"
  - "concurrent_orchestration_of_one_gpu_stages_exactly_one_job: 전체 snapshot-to-CAS-staging 경쟁에서도 정확히 한 Job만 STAGING"
  - "failure_after_reservation_insert_rolls_back_reservation_and_all_staging_state: reservation insert 뒤 fault가 모든 쓰기와 fence epoch 소비를 rollback"
  - "exact_reserved_replay_survives_inventory_refresh_but_changed_revision_conflicts: exact replay는 최초 결과를 반환하고 operation payload revision 변경은 conflict"
  - "corrupt_inventory_and_reservation_revision_or_owner_fail_closed: 잘못된 revision BLOB과 빈 owner를 fail closed"
  - "뮤테이션 1: revision predicate 제거 시 stale 요청이 실제 STAGING되어 stale negative test가 실패"
  - "뮤테이션 2: node PRIMARY KEY와 점유 검사 제거 시 Barrier 경쟁 성공 수가 2로 늘어 동시성 test가 실패"
limitations:
  - "reservation은 node-exclusive라 같은 node의 서로 다른 GPU도 동시에 사용할 수 없다. per-GPU allocation과 부분 자원 공유를 증명하지 않는다"
  - "release API가 없어 reservation row는 계속 active다. rollback/requeue와 Lease 종료에 따른 자원 해제를 구현하지 않았다"
  - "CAS revision mismatch와 node 점유 충돌은 자동 rerank/retry 없이 즉시 실패한다"
  - "orchestrate module은 private이고 production run()/accept-loop에는 연결되지 않았다"
  - "selected GPU UUID, Lease GPU scope, GrantedExecutionPlan과 PlacementRationale을 만들지 않는다"
  - "원본 Manifest adapter, node/device/session routing, live socket registry와 ACK identity 결합은 없다"
  - "wire Grant 생성·서명·전송, ACK/거부/timeout, outbox와 실제 Job 실행은 구현하지 않았다"
  - "단일 로컬 SQLite transaction의 DURABLE admission만 보이며 다중 Coordinator·Raft/ControlStore COMMITTED·HA를 증명하지 않는다"
decision: "scheduler 로드맵 조각 5b를 전체 GPU allocation이나 production dispatch로 과장하지 않고, snapshot inventory revision을 compare token으로 사용하면서 node-exclusive reservation과 durable STAGING을 한 로컬 SQLite linearization point에 묶는 CAS admission kernel로 완료했다. CandidateSnapshot의 optional revision을 inventory store가 결정적으로 투영하고, orchestration은 선택 node가 snapshot에 정확히 하나이며 revision이 존재할 때만 새 API를 정확히 한 번 호출한다. staging store는 operation replay를 먼저 처리한 뒤 같은 BEGIN IMMEDIATE transaction에서 현재 revision 비교, node_id PRIMARY KEY reservation, fence epoch·Attempt·Lease, QUEUED→STAGING, operation 기록을 전부 commit하거나 전부 rollback한다. CAS와 점유 충돌은 즉시 fail closed하고 자동 재시도하지 않는다. 독립 검수는 원자성·rollback fence 미소비·같은 transaction의 CAS/점유 강제·Barrier 경쟁의 정확히 한 성공과 loser QUEUED·두 뮤테이션·DoD-43 기존 API 무회귀·제한된 범위를 확인해 1라운드 만에 ACCEPTED했고 감독자는 coordinator 테스트 86/86을 직접 재확인했다. node-exclusive라 같은 node의 다른 GPU도 막히고 release가 없으며 per-GPU allocation·wire·HA는 후속이다. scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4·5·5b 완료 — orchestration kernel(`DoD-46`) 은 이제 CAS reservation 을 쓸 수 있지만 여전히 production `run()` 에는 미연결(orchestrate.rs 는 비공개 module), GPU 별 세부 allocation/release 도 없음 — 남은 조각 3 나머지·실제 wire 연결과 조각 6~9 는 후속"
---

# DoD-47 · scheduler inventory revision 기반 CAS reservation (로드맵 조각 5b)

## 무엇을 입증하려 했는가

`DoD-46`이 명시적으로 인정한 snapshot-to-stage TOCTOU와 서로 다른 Job의 같은 자원
중복 선택 위험을 닫기 위해, 선택 후보의 inventory revision 비교와 node-exclusive
reservation, durable STAGING이 하나의 SQLite write transaction에서 선형화되는지를
검증했다. 동시에 이 최소 조각이 per-GPU allocation이나 production 연결을 제공하지
않는다는 제한도 코드와 검수로 확인했다.

## 범위 결정 — per-GPU allocator가 아닌 node-exclusive CAS admission

`CandidateSnapshot.inventory_revision`은 filter/rank 점수가 아니라 선택 후 admission에
쓰는 compare token이다. inventory store는 실제 inventory가 있으면 `Some(revision)`,
없으면 `None`을 투영한다. orchestration은 선택된 `node_id`가 snapshot에 정확히 하나이고
revision이 있을 때만 staging으로 진행해 missing·ambiguous 사실을 fail closed한다.

현재 scheduler는 선택 GPU UUID 집합을 반환하지 않는다. 따라서 reservation key는
GPU가 아니라 `node_id`이고 한 node에는 active row가 하나뿐이다. 같은 node의 다른 GPU도
동시에 쓸 수 없으며 release도 없다. 이 보수적 v0 경계로 중복 선택은 막지만 GPU별
allocation이나 부분 자원 accounting을 완료했다고 주장하지 않는다.

## 구현 — 하나의 `BEGIN IMMEDIATE` linearization point

`reserve_node_and_stage_queued_with_lease()`는 내부 `reserve_and_stage()`로 들어가 한
transaction 안에서 operation replay, 현재 inventory revision 비교, node reservation
삽입, fence epoch 채번, Attempt·Lease 삽입, `QUEUED→STAGING`, operation 기록과 commit을
처리한다. 실패는 전부 rollback되어 reservation과 fence epoch를 남기지 않는다.

`orchestrate_placement_to_staging()`은 snapshot에서 선택한 후보의 revision을 새 API에
전달하고 정확히 한 번 호출한다. revision mismatch나 이미 점유된 node는 typed error로
즉시 반환하며 자동 rerank/retry하지 않는다. 기존 reservation 없는
`stage_queued_with_lease()`는 DoD-43의 시그니처와 동작을 유지한다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 `staging_store.rs`의 transaction 시작·reservation·staging·commit 경계를
추적하고 reservation insert 뒤 fault가 reservation과 모든 staging 상태를 rollback하며
fence epoch도 소비하지 않음을 확인했다. revision 비교와 node 점유 검사가 같은
transaction에서 강제되고 orchestration이 이 API를 한 번만 호출함도 대조했다.

별도 SQLite connection 두 개와 실제 `Barrier`를 쓴 경쟁 테스트는 정확히 한 요청만
성공시키고 loser Job을 QUEUED로 유지한다. revision predicate 제거 시 stale 요청이
STAGING하고 node PK와 점유 검사를 제거하면 성공 수가 2가 되는 두 뮤테이션도 판별력을
입증했다. 기존 DoD-43 API와 테스트 무회귀, 제한된 변경 범위와 production 미연결 한계도
확인해 잔여 요청 없이 1라운드에서 `ACCEPTED`했다.

## 결과

```text
C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-coordinator
  PASS — unit 82 + integration 4 = 86 passed, 0 failed
```

위 결과는 이 evidence 작성 중 감독자 경계에서 직접 재확인했다.

## 이 실험이 증명하지 "않는" 것

- GPU UUID별 allocation, 같은 node 안의 GPU별 동시 사용과 부분 자원 공유는 없다.
- reservation release/requeue와 Lease 종료 후 자원 정리는 없다.
- CAS·점유 충돌 뒤 자동 rerank/retry는 없다.
- private `orchestrate` module은 production `run()`/accept-loop에 연결되지 않았다.
- Manifest adapter, Grant plan/rationale, node/device/session routing은 없다.
- wire Grant·ACK·timeout·outbox와 실제 Job 실행은 없다.
- 다중 Coordinator·Raft/ControlStore `COMMITTED`·HA 안전성을 보장하지 않는다.

## 결정

1. scheduler 로드맵 조각 5b를 inventory revision 기반의 로컬 node-exclusive CAS
   reservation kernel로 완료했다.
2. operation replay부터 reservation·Attempt/Lease/fence·Job STAGING·operation 기록까지
   하나의 `BEGIN IMMEDIATE` transaction에서 전부-or-none 처리함을 검증했다.
3. 실제 `Barrier` 경쟁과 두 뮤테이션으로 stale CAS와 node 중복 점유 방지의 판별력을
   확인했고, rollback의 fence epoch 미소비와 DoD-43 API 무회귀도 확인했다.
4. 독립 검수는 원자성·경쟁·rollback·뮤테이션·범위·한계를 확인해 1라운드 만에
   `ACCEPTED`했고 감독자가 coordinator 테스트 86/86을 직접 재확인했다.
5. scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4·5·5b 완료 — orchestration kernel(`DoD-46`) 은 이제 CAS reservation 을 쓸 수 있지만 여전히 production `run()` 에는 미연결(orchestrate.rs 는 비공개 module), GPU 별 세부 allocation/release 도 없음 — 남은 조각 3 나머지·실제 wire 연결과 조각 6~9 는 후속.

관련: `docs/plans/2026-08-21_1537_scheduler_inventory_cas_v1.md`
