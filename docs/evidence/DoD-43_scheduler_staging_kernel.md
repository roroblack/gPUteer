---
schema_version: 2
id: DoD-43
claim: "scheduler 로드맵 조각 2b 를 'single-node local atomic STAGING kernel'로 완료했다. 조각 2 전체나 분산 COMMITTED를 끝냈다고 주장하지 않고 2b-1로 한정했다 — SQLite CoordinatorStagingStore의 stage_queued_with_lease()가 한 BEGIN IMMEDIATE transaction 안에서 fence epoch 채번·Attempt/node/Lease 삽입·QUEUED→STAGING 전이·operation idempotency 기록을 전부 원자 처리하며, rollback epoch 미소비·최초 결과 재시도·불변 identity/epoch 손상 검출·기존 Lease API 무회귀·실제 두 connection 경쟁과 뮤테이션 2건을 자체 재검토 수정 5건, 독립 검수 1라운드 ACCEPTED 및 감독자 cargo test로 확인했다"
status: PASS
commit: d8146323f45c3c27931f40ed911de93d1284846a

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — 자체 재검토 5건 포함"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-21T11:58:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(ACCEPTED) — 한 connection의 BEGIN IMMEDIATE transaction 경계와 단일 commit(staging_store.rs:175,218,245), 세 오류 주입 rollback과 epoch 미소비(staging_store.rs:627,648), 기존 Lease 공개 API 무회귀(lease_store.rs:281,299), retry 시 정상 가변 필드만 제외하고 불변 identity/epoch를 계속 대조하며 최초 결과를 반환하는 수정(staging_store.rs:460,465,829), 실제 Barrier 기반 두 connection 경쟁(staging_store.rs:654), fence 증가·STAGING 전이 뮤테이션의 판별력(staging_store.rs:648,604)을 확인했다. 자체 재검토 수정 5건과 coordinator 프로덕션 4개 파일로 제한된 범위, staging_store.rs 862줄이 계획 상한 360줄을 넘었다는 자기 보고까지 대조해 잔여 지적 없이 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-43_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-43_scheduler_staging_kernel_2026-08-21.txt"
raw_output_digest: "sha256:e2ca27f75940e9c2422a664788c44839526debf86061eb046fd456dc8d39180f"
raw_output_bytes: 5220

binary_digests:
  toolchain: "제공된 이력 요약에 toolchain version·binary digest 없음 — supervisor cargo test 결과만 기록"
protocol_versions:
  schema_version: "proto 변경 없음 — Coordinator 내부 single-node local atomic STAGING 저장 kernel 신설"
  canonical_spec: "docs/protocol/state-machines.md Job·Attempt 전이 규범 참조 — compound ControlAction과 COMMITTED 계약은 후속"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test / SQLite"
hardware: "GPU 미사용 — 로컬 SQLite 저장소와 transaction·동시성·상태 전이 테스트"
network_profile: "네트워크 미사용 — 같은 SQLite 파일의 별도 connection 두 개를 사용한 in-process Barrier 경쟁 테스트"
command: |
  cargo test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-43_scheduler_staging_kernel_2026-08-21.txt,
   docs/evidence/_raw/DoD-43_review.txt 전문 참조)

  감독자 직접 확인: PASS — unit 56 + integration 4, 0 failed
  독립 검수 1라운드: ACCEPTED — 잔여 수정 요청 없음
artifacts:
  - docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md
  - docs/plans/2026-08-21_1049_scheduler_durable_job_v1.md
  - docs/plans/2026-08-21_1123_scheduler_attempt_lease_v1.md
  - crates/coordinator/src/lib.rs
  - crates/coordinator/src/job_store.rs
  - crates/coordinator/src/lease_store.rs
  - crates/coordinator/src/staging_store.rs
  - docs/evidence/_raw/DoD-43_scheduler_staging_kernel_2026-08-21.txt
  - docs/evidence/_raw/DoD-43_review.txt
negative_tests:
  - "Attempt 삽입 뒤·Lease 삽입 뒤·Job update 직전의 세 오류 주입에서 Job/Attempt/Lease/counter/operation 전체가 rollback되고 바로 재시도한 최초 epoch가 1"
  - "같은 control DB의 별도 connection 두 개를 실제 Barrier 뒤 동시에 실행해 성공 1건·JobNotQueued(STAGING) 1건과 Attempt/node/Lease/operation 각 1행만 남음"
  - "동일 operation key·동일 payload retry는 epoch를 소비하지 않고 최초 결과를 반환하며, renew·revoke로 정상 가변 필드가 바뀐 뒤에도 불변 identity/epoch를 대조하고 최초 결과를 반환"
  - "동일 key 변경 payload, 다른 operation의 attempt/lease ID 재사용, non-QUEUED Job, 공백 job/attempt/lease/node/issuer, 잘못된 수명 순서·max-duration·clock rollback을 상태 불변으로 거부"
  - "기존 Lease epoch 41 뒤 42를 발급하고, 잘린 Lease/counter epoch BLOB과 Lease/counter u64::MAX는 counter나 부분 행을 바꾸지 않고 실패"
  - "reopen 뒤 Job/Attempt/Lease/counter 보존, 조각 2a 이전 schema migration, 기존 Lease store read/renew/revoke/reopen을 검사"
  - "뮤테이션 2건 — checked_add(1)을 checked_add(0)으로 바꾸면 rollback 뒤 epoch=1 검사가 실패하고, commit 직전 STAGING 대입을 QUEUED로 바꾸면 성공 상태 검사가 실패; 각각 원복 후 coordinator 전체 테스트 통과"
limitations:
  - "scheduler 로드맵 조각 2 전체가 아니라 조각 2b-1의 single-node local atomic STAGING kernel만 증명한다 — multi-node Attempt와 2b의 분산 결합은 없다"
  - "SQLite 로컬 DURABLE all-or-none만 제공하며 Raft ControlStore·과반 합의·watch/cursor/snapshot과 규범의 COMMITTED 보증은 제공하지 않는다"
  - "scheduler winner 선택, inventory, GPU reservation/admission, plan/rationale 계산과 실제 resource reservation은 범위 밖이다"
  - "CoordinatorConfig/CLI의 canonical control DB 경로, 현재 issue_grant() 실행 경로 연결, Grant 서명·전송·ACK와 Agent entrypoint 실행은 없다"
  - "proto/control.proto의 TransitionJob·CreateAttempt·IssueLease를 한 commit으로 표현할 compound action 또는 atomic batch 계약은 아직 결정하지 않았다"
  - "기존 --lease-db와 Job DB의 canonical migration 정책, lease expiry 뒤 재시도·cleanup·requeue 및 이후 Attempt 상태기계는 후속이다"
  - "staging_store.rs는 production과 같은 파일의 단위 테스트를 포함해 물리 862줄이며 계획의 production 추정 상한 360줄을 넘었다 — 기능 범위를 wire/CLI/reservation/dispatch/proto/Raft로 확장한 결과는 아니다"
decision: "scheduler 로드맵 조각 2b 전체나 분산 ControlStore를 완료했다고 과장하지 않고, 한 control DB 파일·한 connection·한 BEGIN IMMEDIATE가 fence epoch 채번, Attempt/node/Lease 삽입, QUEUED→STAGING 전이와 operation idempotency를 소유하는 조각 2b-1로 제한했다. 세 오류 주입은 모든 부분 상태를 rollback하고 epoch를 소비하지 않았으며, 실제 별도 connection 경쟁은 정확히 한 staging만 commit했다. 자체 재검토로 renew/revoke의 정상 가변 필드를 손상으로 오인하던 retry 결합 버그를 고쳐 최초 결과 반환과 불변 identity/epoch 대조를 분리하고, 공백 plan row-shape·조각 2a 이전 migration·부분 commit 경로를 보강했다. 두 뮤테이션은 epoch 증가와 STAGING 전이 테스트의 판별력을 확인했고, 독립 검수는 공개 Lease API 무회귀·제한된 변경 범위·862줄 상한 초과 자기 보고까지 확인해 1라운드 만에 ACCEPTED, 감독자는 coordinator 테스트를 직접 재확인했다. scheduler 로드맵 9단계 중 조각 1·2a·2b-1 완료 — 남은 조각 2 의 나머지(다중 노드 결합· Raft COMMITTED)와 조각 3~9 는 후속"
---

# DoD-43 · scheduler single-node local atomic STAGING kernel (로드맵 조각 2b-1)

## 무엇을 입증하려 했는가

한 control DB 파일의 한 SQLite connection에서 Job의 `QUEUED -> STAGING`,
Attempt `CREATED`, 단일 node 결합, team-global fence epoch 채번, Lease 발급과
operation idempotency를 하나의 `BEGIN IMMEDIATE` transaction으로 처리해
전부-or-none을 보장하는지 검증했다. 오류·retry·두 connection 경쟁에서도 부분
상태나 epoch 누수가 없고 기존 Lease API와 함께 사용할 수 있는 것까지가 범위다.

## 범위 결정 — 조각 2b를 2b-1로 축소

조각 2b 전체에는 multi-node 결합과 실제 reservation, Grant dispatch,
ControlStore의 분산 `COMMITTED`까지 남아 있다. 이번에는 그 전체가 아니라
**single-node local atomic STAGING kernel**만 구현했다. 현재
`proto/control.proto`에는 Job 전이·Attempt 생성·Lease 발급을 한 commit으로
표현할 compound action이 없으므로, SQLite 로컬 `DURABLE`을 분산
`COMMITTED`로 부르지 않는다.

## 구현 — `CoordinatorStagingStore`

`crates/coordinator/src/staging_store.rs`를 신설했다.
`stage_queued_with_lease()`는 한 `BEGIN IMMEDIATE` transaction에서 기존 Lease
watermark를 포함한 fence epoch를 채번하고 Attempt/node/Lease를 삽입한 뒤,
Job을 `STAGING`으로 전이하고 operation 결과를 기록해 한 번만 commit한다.
동일 operation key와 동일 payload는 새 epoch 없이 최초 결과를 반환하고,
payload나 attempt/lease identity 충돌은 원본을 바꾸지 않고 거부한다.

`job_store.rs`에는 `JobState::Staging`과 staging row shape, 조각 2a 이전 schema의
반복 안전 migration을 추가했다. `lease_store.rs`는 transaction을 받는 내부
조회·삽입 helper만 추출했으며 공개 renew/revoke/resume/get-or-issue API 본문은
기존 계약을 유지한다.

## 자체 재검토 — 수정 5건

renew/revoke 뒤 operation retry가 expiry·renew-after·revoked 같은 정상 가변
필드를 손상으로 오인하던 버그를 고쳤다. 최초 payload와 epoch로 최초 Lease
결과를 재구성해 반환하고 현재 Lease에서는 불변 identity·epoch만 대조하도록
분리했다. 공백 plan row-shape 검사를 강화하고, 조각 2a 이전 schema migration
테스트를 추가했으며, 세 오류 주입과 단일 commit을 다시 추적해 부분 commit
경로가 없음을 재확인했다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 transaction 경계와 단일 commit, rollback epoch 미소비, 공개 Lease
API 무회귀, 정상 가변 필드만 제외한 identity/epoch 손상 대조, 실제 `Barrier`
경쟁, 두 뮤테이션의 판별력을 코드와 테스트로 확인했다. 변경 범위가 Coordinator
프로덕션 4개 파일과 문서뿐이며, `staging_store.rs` 862줄이 계획 상한 360줄을
넘었다는 자기 보고도 타당하다고 확인해 1라운드 만에 잔여 지적 없이
`ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-coordinator
  PASS — unit 56 + integration 4, 0 failed
```

위 결과는 감독자가 직접 확인했다.

## 이 실험이 증명하지 "않는" 것

- multi-node Attempt와 분산 Lease 결합은 없다.
- scheduler winner 선택, resource reservation과 실제 Grant dispatch는 없다.
- Coordinator wire/CLI 실행 경로와 Agent entrypoint 연결은 없다.
- compound `ControlAction` 계약과 canonical control DB migration은 없다.
- Raft `ControlStore`와 분산 `COMMITTED` 보증은 없다.

## 결정

1. scheduler 로드맵 조각 2b 전체가 아니라 조각 2b-1인 single-node local
   atomic STAGING kernel을 완료했다.
2. 자체 재검토 수정 5건과 rollback·idempotency·동시성·손상·뮤테이션 검증을
   독립 검수가 1라운드 만에 `ACCEPTED`했고 감독자가 coordinator 테스트를
   직접 재확인했다.
3. scheduler 로드맵 9단계 중 조각 1·2a·2b-1 완료 — 남은 조각 2 의 나머지(다중 노드 결합·
   Raft COMMITTED)와 조각 3~9 는 후속.

관련: `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md` ·
`docs/plans/2026-08-21_1049_scheduler_durable_job_v1.md` ·
`docs/plans/2026-08-21_1123_scheduler_attempt_lease_v1.md`
