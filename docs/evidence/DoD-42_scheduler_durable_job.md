---
schema_version: 2
id: DoD-42
claim: "scheduler 로드맵 조각 2를 'durable Job/Queue truth'로 정직하게 축소해 완료했다. 원안의 3~5일 규모 durable Job/Attempt/Queue·Lease/Grant 결합·전체 ControlStore를 하루에 끝냈다고 주장하지 않고 조각 2a로 한정했다 — SQLite CoordinatorJobStore가 BEGIN IMMEDIATE read-check-write로 검증 완료 submit의 멱등 저장과 SUBMITTED→PLANNING→QUEUED 전이, 결정적 queue 조회, deadline/queue-timeout/영구 불가능 실패를 durable하게 보존하며, 실제 barrier 동시 경쟁·경계·손상·뮤테이션 테스트와 자체 재검토 수정 2건을 독립 검수 1라운드 ACCEPTED 및 감독자 cargo test로 확인했다"
status: PASS
commit: 60f06c455f23e44382dfa7d012a0577bdc6f925c

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — 자체 재검토 포함"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-21T11:02:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(ACCEPTED) — 모든 read-check-write의 BEGIN IMMEDIATE transaction 경계(job_store.rs:327,563)와 별도 SQLite connection·실제 Barrier 기반 동시 submit 경쟁(job_store_concurrent_submit.rs:31,63), immutable payload 전체를 대조하는 idempotency 계약(job_store.rs:124,332), SUBMITTED→PLANNING→QUEUED 및 QUEUED→FAILED source-state 거부(job_store.rs:422,454,503), deadline·queue-timeout의 엄격한 > 경계와 None timeout(job_store.rs:518,526,934,949,963)을 확인했다. queue 경계와 idempotency 대조를 각각 약화한 뮤테이션이 해당 회귀 테스트를 실제 실패시킴(job_store.rs:347,536,832)을 확인했고, 규범에 없는 all-zero key 거부 제거와 상태별 전체 row-shape 손상 검사라는 자체 수정 2건(job_store.rs:574,670,880)도 반영됐다. 변경 범위가 coordinator 크레이트 내 job_store.rs·경쟁 테스트·lib.rs 모듈 노출 1줄과 문서뿐이며 Attempt/fence/Lease 결합을 조각 2b로 이월한 것이 split authority를 피하는 정직한 축소임을 확인해 잔여 지적 없이 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-42_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-42_scheduler_durable_job_2026-08-21.txt"
raw_output_digest: "sha256:80ba743c414fae0a77e99e91f610caf3be0699707677cad7038c30212f9883b3"
raw_output_bytes: 4683

binary_digests:
  toolchain: "제공된 이력 요약에 toolchain version·binary digest 없음 — supervisor cargo test 결과만 기록"
protocol_versions:
  schema_version: "proto 변경 없음 — Coordinator 내부 durable Job/Queue 저장 계층 신설"
  canonical_spec: "docs/protocol/state-machines.md Job·Attempt 전이 규범 참조 — canonical 서명 경로 변경 없음"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test / SQLite"
hardware: "GPU 미사용 — 로컬 SQLite 저장소와 동시성·상태 전이 테스트"
network_profile: "네트워크 미사용 — 별도 SQLite connection을 사용한 in-process barrier 경쟁 테스트"
command: |
  cargo test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-42_scheduler_durable_job_2026-08-21.txt,
   docs/evidence/_raw/DoD-42_review.txt 전문 참조)

  감독자 직접 확인: PASS — unit 47 + job concurrency 2 + lease concurrency 2, 0 failed
  독립 검수 1라운드: ACCEPTED — 잔여 수정 요청 없음
artifacts:
  - docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md
  - docs/plans/2026-08-21_1049_scheduler_durable_job_v1.md
  - crates/coordinator/src/lib.rs
  - crates/coordinator/src/job_store.rs
  - crates/coordinator/tests/job_store_concurrent_submit.rs
  - docs/evidence/_raw/DoD-42_scheduler_durable_job_2026-08-21.txt
  - docs/evidence/_raw/DoD-42_review.txt
negative_tests:
  - "공백 job/submitter·0 queue duration·공백 plan/영구 불가능 사유를 fail-closed하고, 규범에 없는 all-zero idempotency key/manifest digest 거부는 제거해 정상 round-trip을 고정"
  - "동일 key의 변경 payload와 다른 key의 기존 job ID 재사용을 원본 불변으로 거부하고, 동일 key·동일 payload 재시도는 최초 저장 row를 반환"
  - "SUBMITTED→QUEUED 건너뛰기, 잘못된 source state, clock rollback, enqueue 재시도의 plan 변경, terminal 실패 사유 덮어쓰기를 상태 불변으로 거부"
  - "deadline과 queue timeout의 정확한 경계는 실패시키지 않고 +1ms에서만 실패하며, timeout None을 0ms로 해석하지 않음"
  - "unknown state·잘린 u64 BLOB·상태별 timestamp/plan/failure 전체 컬럼 shape 손상을 변경 없이 fail-closed"
  - "별도 connection·실제 Barrier의 동시 동일 submit은 한 created·한 replay, 동일 key/다른 payload는 한 승자·한 conflict이고 패자 Job은 생성되지 않음"
  - "뮤테이션 2건 — queue timeout 경계를 약화하면 경계 회귀 테스트 실패, 동일 key payload 대조를 무력화하면 변경 payload 거부 테스트 실패; 둘 다 원복 후 coordinator 전체 테스트 통과"
limitations:
  - "scheduler 로드맵 조각 2 전체가 아니라 조각 2a의 durable Job/Queue truth만 증명한다 — Attempt 저장·attempt ID/fence epoch 할당·QUEUED→STAGING은 없다"
  - "Attempt 생성은 STAGING 진입과 Lease 발급에 결합해야 하므로 조각 2b로 이월했다 — Job/Attempt/Lease 단일 권위 연산과 full operation idempotency는 아직 없다"
  - "SQLite 로컬 DURABLE만 제공하며 Raft ControlStore·과반 합의·watch/cursor/snapshot과 규범의 COMMITTED 보증은 제공하지 않는다"
  - "submit 서명·quorum·hard-filter·side-effect 정책 검증 producer, Coordinator RPC/CLI/Agent 연결과 실제 E2E Job 제출은 없다"
  - "plan·PlacementRationale 계산/저장, reservation, winner 선택, 실제 Grant dispatch와 Agent entrypoint 실행은 범위 밖이다"
  - "PERMANENTLY_INFEASIBLE의 사실성은 향후 inventory·hard-filter 호출자 책임이며 이 저장소는 비어 있지 않은 사유만 강제한다"
decision: "상위 scheduler 로드맵 조각 2를 하루에 전부 완료했다고 과장하지 않고, 검증이 끝난 submit부터 QUEUED/FAILED queue truth까지를 로컬 SQLite에 durable하게 보존하는 조각 2a로 제한했다. CoordinatorJobStore의 모든 read-check-write를 BEGIN IMMEDIATE에 묶고 immutable payload 멱등성, SUBMITTED→PLANNING→QUEUED 전이, 결정적 queue 순서, 서로 다른 실패 사유와 엄격한 deadline/timeout 경계를 강제했다. 별도 connection·실제 Barrier 경쟁 테스트는 동시 최초 submit의 단일 생성과 conflict 원자성을 확인했고, 두 뮤테이션은 경계·payload 대조 테스트의 판별력을 확인했다. 자체 재검토로 규범에 없는 all-zero 고정폭 값 거부를 제거하고 상태별 컬럼 전체 row-shape 손상 검사를 강화했으며 독립 검수는 코드·테스트·제한된 범위를 확인해 1라운드 만에 ACCEPTED, 감독자는 coordinator 테스트를 직접 재확인했다. Attempt를 미리 만들지 않고 STAGING 진입·fence epoch·Lease 발급의 단일 권위 연산으로 조각 2b에 이월한다. scheduler 로드맵 9단계 중 조각 1·2a 완료 — 남은 durable Attempt/Lease 결합(2b)과 이후 7단계는 후속"
---

# DoD-42 · scheduler durable Job/Queue truth (로드맵 조각 2a)

## 무엇을 입증하려 했는가

검증을 마친 submit만 SQLite에 원자 저장하고, `CoordinatorJobStore`가
`SUBMITTED -> PLANNING -> QUEUED`와 queue 실패 truth를 프로세스 재시작을
넘어 보존하는지 검증했다. 동일 요청 재시도와 두 connection의 동시 최초
submit에서도 Job이 중복 생성되지 않고, 잘못된 전이·경계·손상 데이터는
변경 없이 fail-closed하는 것까지가 범위다.

## 범위 결정 — 조각 2를 2a로 축소

상위 로드맵 조각 2는 durable Job/Attempt/Queue, Attempt/fence 할당,
Lease/Grant 결합과 전체 ControlStore를 포함한 3~5일 규모다. 이번에는
그 전체가 아니라 **durable Job/Queue truth**만 구현했다. 규범상 Attempt는
Job이 `STAGING`에 들어갈 때 fence epoch 증가·Lease 발급과 함께 생성해야
하므로, 독립 저장을 앞당겨 split authority를 만들지 않고 조각 2b로
이월했다.

## 구현 — `CoordinatorJobStore`

`crates/coordinator/src/job_store.rs`에 SQLite 저장소를 신설했다.
`submit_accepted()`·`start_planning()`·`enqueue()`·`fail_queued()`의 모든
read-check-write는 `BEGIN IMMEDIATE` transaction 안에서 수행한다. 동일
idempotency key와 동일 immutable payload는 최초 row를 반환하고, payload
변경이나 다른 key의 기존 job ID 재사용은 원본을 바꾸지 않고 거부한다.
`list_queued()`는 `(queued_at_unix_ms, job_id)` 순으로 결정적이다.

## 자체 재검토 — 수정 2건

규범에 없는 all-zero idempotency key/manifest digest 거부를 제거했다.
또한 FAILED 사유 중심의 부분 손상 검사에서 상태별 timestamp·plan·failure
컬럼 전체 shape 검사로 강화했다. all-zero 고정폭 값 수용과 손상된
row-shape fail-closed를 회귀 테스트로 고정했다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 `BEGIN IMMEDIATE` transaction 경계, 별도 connection과 실제
`Barrier`를 사용한 경쟁 테스트, idempotency payload 대조, 상태 전이 거부,
deadline·queue-timeout 경계, 두 뮤테이션의 판별력, 자체 수정 2건의 반영을
코드로 확인했다. 범위가 `job_store.rs`·경쟁 테스트·`lib.rs` 모듈 노출
1줄과 문서뿐이고 Attempt/Lease 결합 이월이 split authority를 피한다는
점까지 확인해 1라운드 만에 잔여 지적 없이 `ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-coordinator
  PASS — unit 47 + job concurrency 2 + lease concurrency 2, 0 failed
```

위 결과는 감독자가 직접 확인했다.

## 이 실험이 증명하지 "않는" 것

- durable Attempt, fence epoch 할당, `QUEUED -> STAGING`은 없다.
- Attempt/Lease/Grant/reservation의 원자 결합과 실제 dispatch는 없다.
- Raft `ControlStore`와 분산 `COMMITTED` 보증은 없다.
- submit 검증 producer·RPC·CLI·Agent 연결과 E2E 제출은 없다.
- plan/rationale 계산, winner 선택, 실제 entrypoint 실행은 없다.

## 결정

1. scheduler 로드맵 조각 2 전체가 아니라 조각 2a인 durable Job/Queue
   truth를 완료했다.
2. 자체 재검토 수정 2건과 동시성·경계·손상·뮤테이션 검증을 독립 검수가
   1라운드 만에 `ACCEPTED`했고 감독자가 coordinator 테스트를 직접
   재확인했다.
3. scheduler 로드맵 9단계 중 조각 1·2a 완료 — 남은 durable
   Attempt/Lease 결합(2b)과 이후 7단계는 후속.

관련: `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md` ·
`docs/plans/2026-08-21_1049_scheduler_durable_job_v1.md`
