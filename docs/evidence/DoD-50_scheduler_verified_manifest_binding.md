---
schema_version: 2
id: DoD-50
claim: "검증된 signed `JobManifest` 를 durable 하게 묶는 저장 경로를 만들었다 — 저장 API 는 `&Verified<pb::JobManifest>` 만 받아 미검증 Manifest 가 저장될 수 없고, load 결과는 의도적으로 `Verified` 가 아니라 재검증 없이는 쓸 수 없다. accepted Job·Manifest body·verified signer·재계산 hash·idempotency를 단일 `BEGIN IMMEDIATE` transaction에 결합하고 identity/hash 불일치·fault rollback·replay conflict·typed corruption·legacy migration·body device identity 손상·뮤테이션 2건·독립 검수 1라운드 ACCEPTED·감독자 coordinator 99 passed로 확인했다. membership resolution, `JobRequirements` projection, 기본 `MIRRORED` durability 소비, Grant/Lease scope, `COMMITTED` submission과 production wire는 완료하지 않았다"
status: PASS
commit: 5132e486cddb9865e55802dc9b0ae02c8c7a24af

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — verified signed JobManifest durable binding, replay·fault·corruption·migration negative test와 뮤테이션 검증"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-24T12:19:29+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "production 저장 API가 &Verified<pb::JobManifest>만 받고 fault inner 경로는 private이며 Manifest production INSERT가 한 곳뿐임, Verified 내부 필드가 private이고 정상 생성 경로가 서명 검증 함수뿐임, raw 필드 최초 관찰이 verified.get() 이후이고 그 전에는 durable side effect가 없음, Job/device/signer identity 대조와 blake3_256(signing_input(manifest)) 재계산·대조·저장, 단일 BEGIN IMMEDIATE와 Manifest insert 직후 fault 전체 rollback, raw StoredManifestBinding load와 scheduler/Grant production 소비 경로 부재, typed corruption fail-closed, replay 대조·충돌 거부, legacy exact-error와 body device identity 손상 보강, 뮤테이션 2건, job_store.rs 659 additions/0 deletions+계획 1개 범위와 DoD-42/43/47/49 구현 무변경을 확인해 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-50_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-50_scheduler_verified_manifest_binding_2026-08-24.txt"
raw_output_digest: "sha256:1d3e4444ef165f134944c50f7e4ba3e5449e67eda1b56d102d4887a78f95ec89"
raw_output_bytes: 7709

binary_digests:
  toolchain: "cargo 사용 — 제공된 감독자 재실행 이력에 cargo/rustc version과 binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — coordinator local SQLite schema와 Rust 저장 API만 확장"
  canonical_spec: "기존 JobManifest signing_input과 blake3_256을 사용 — load에서 현재 key directory 서명 재검증, Manifest projection, Grant 서명·wire 연결은 범위 밖"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test / SQLite local store"
hardware: "GPU 미사용 — signed protobuf fixture와 SQLite corruption/fault fixture로 durable Manifest binding 검증"
network_profile: "네트워크 미사용 — coordinator 단위·통합 테스트만 실행하고 production run()/accept-loop/wire는 미변경"
command: |
  cargo test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-50_scheduler_verified_manifest_binding_2026-08-24.txt,
   docs/evidence/_raw/DoD-50_review.txt 전문 참조)

  감독자 직접 확인: PASS — coordinator unit 95 + integration 4 = 99 passed,
  전체 0 failed, exit code 0
  독립 검수 1라운드: ACCEPTED — Verified 전용 API와 검증 순서, identity와 재계산 hash
  binding, 단일 transaction과 fault rollback, raw load 경계, corruption/replay fail-closed,
  legacy·body device identity 보강, 뮤테이션 2건과 제한 범위를 확인
  첫 workspace 실행의 기존 crypto lock-timeout timing test 1회 실패는 재실행 통과한
  알려진 flaky이며 판정 근거에서 제외
artifacts:
  - docs/plans/2026-08-24_1142_scheduler_verified_manifest_durable_binding_v1.md
  - crates/coordinator/src/job_store.rs
  - docs/evidence/_raw/DoD-50_scheduler_verified_manifest_binding_2026-08-24.txt
  - docs/evidence/_raw/DoD-50_review.txt
negative_tests:
  - "identity_and_supplied_hash_mismatches_create_no_rows: accepted job ID·device ID·caller hash 불일치를 typed error로 거부하고 Job/Manifest/idempotency row가 생기지 않음을 확인"
  - "verified_manifest_replay_returns_original_and_changed_manifest_conflicts: exact replay는 최초 binding을 반환하고 changed Manifest는 idempotency/job conflict로 거부됨을 확인"
  - "fault_after_manifest_insert_rolls_back_job_body_and_idempotency: Manifest insert 뒤 injected fault가 Job/body/idempotency 전체를 rollback함을 확인"
  - "empty_and_undecodable_manifest_bodies_fail_closed: 빈 body와 protobuf decode 불가 body를 typed ManifestCorruption으로 구분해 거부"
  - "legacy migration 뒤 Manifest row가 없는 Job은 job ID를 포함한 exact LegacyManifestMissing으로 거부"
  - "저장 body의 job ID·submitter device ID, verified signer ID와 hash 손상을 각각 typed ManifestCorruption으로 fail closed"
  - "뮤테이션 1: load의 recomputed hash 대조 제거 시 body corruption test가 실패"
  - "뮤테이션 2: submit의 verified signer 대조 제거 시 signer/device mismatch test가 실패"
limitations:
  - "load 시 서명을 재검증하지 않는 것은 의도된 설계 — 호출자가 authoritative key directory 로 재검증해야 한다"
  - "membership validity, `JobRequirements` projection, 기본 `MIRRORED` durability 소비, Grant/Lease scope, `COMMITTED` submission, production wire 연결은 범위 밖"
  - "submission-time verified signer ID 저장은 현재 membership validity나 key rotation 뒤의 권위를 증명하지 않는다"
  - "authoritative device→member 해석이 없으므로 Manifest를 scheduler 요구사항으로 projection하거나 orchestration에 연결하지 않았다"
  - "local SQLite transaction의 DURABLE binding만 증명하며 다중 Coordinator 합의나 Raft COMMITTED를 증명하지 않는다"
  - "실제 GPU와 네트워크를 사용하지 않았고 production run()/accept-loop/Grant 생성 경로는 바뀌지 않았다"
decision: "이 조각을 Manifest membership 승인, strict projection, Grant 생성, Raft COMMITTED submission 또는 production wire 완료로 과장하지 않고 verified signed JobManifest durable binding으로 완료했다. public 저장 API는 오직 `&Verified<pb::JobManifest>`만 받고 fault 경로는 private이며 production Manifest INSERT는 이 경로 하나뿐이다. `Verified::get()` 뒤에만 Manifest identity를 읽고 accepted job/device와 verified signer를 대조하며 caller hash는 `blake3_256(signing_input(manifest))` 재계산값과 일치할 때만 그 재계산값을 저장한다. Job/body/signer/idempotency는 하나의 `BEGIN IMMEDIATE` transaction에 저장되고 Manifest insert 직후 fault는 전부 rollback한다. exact replay만 최초 binding을 복원하고 changed input은 conflict이며, legacy row와 body/hash/job/device/signer 손상은 typed error로 fail closed한다. 자체 재검토에서 legacy exact-error assertion과 body device identity 손상 fixture를 보강했다. 독립 검수는 API type gate·서명 검증 순서·단일 production insert·원자성·raw load 경계·production 소비 경로 부재·손상/replay·뮤테이션 2건·제한 범위를 확인해 1라운드 만에 ACCEPTED했고 감독자는 coordinator unit 95+integration 4=99 passed를 직접 재확인했다. 첫 workspace 실행의 기존 crypto lock-timeout timing test 1회 실패는 알려진 flaky이며 재실행 통과했고 판정 근거가 아니다. load는 의도적으로 Verified가 아니므로 authoritative key directory로 재검증되기 전에는 scheduler나 Grant 로직에 사용할 수 없다. scheduler 로드맵 진행: `DoD-41`~`DoD-50` 완료 — authoritative device→member 해석, `JobRequirements` projection, reservation release(실행 종료 증명 필요), Grant scope 생성, production 연결은 후속"
---

# DoD-50 · verified signed JobManifest durable binding

## 무엇을 입증하려 했는가

`DoD-49`까지 scheduler는 hard-filter, durable Job/Queue, local atomic STAGING, durable
inventory, best-fit, private orchestration, inventory CAS reservation과 selected GPU durable
binding을 갖췄다. 그러나 accepted Job row는 Manifest hash만 보존해 재시작 뒤 원본 signed
`JobManifest`를 복원할 수 없었다. 이 상태에서는 나중에 strict projection이나 Grant
builder를 추가해도 동일한 accepted 입력을 다시 검증하고 소비할 durable fact가 없다.

설계 조사는 strict 변환기 자체는 하루에 만들 수 있지만 authoritative device→member
해석과 기본 `MIRRORED` durability 소비 경로가 없어 아직 orchestration에 안전하게 연결할
수 없다고 판정했다. 따라서 이번 조각은 converter나 production ingress가 아니라, 이미
서명이 검증된 Manifest를 Job·idempotency와 원자적으로 저장하고 재시작 뒤 raw durable
binding으로 복원하는 선행 경계만 검증했다.

## 구현 — `Verified` 전용 입력을 Job과 같은 transaction에 결합

`StoredManifestBinding`, `ManifestBoundSubmitResult`, typed `ManifestCorruption`을 추가하고
`submit_verified_manifest()`와 `get_manifest_binding()`을 신설했다. 신규
`coordinator_job_manifests(job_id PK/FK, verified_signer_id, manifest_body)` schema는
signed protobuf body와 submission-time signer ID를 Job에 결합한다.

저장 API는 raw `pb::JobManifest`를 받지 않고 `&Verified<pb::JobManifest>`만 받는다.
`Verified::get()` 이후에만 `job_id`와 `submitter_device_id`를 읽고 accepted submission과
대조하며, `Verified::signer_id()`도 accepted device identity와 같아야 한다. caller가 준
hash는 신뢰하지 않고 `blake3_256(signing_input(manifest))`로 재계산해 대조한 뒤 그
재계산값을 저장한다.

Job row, Manifest body/signer row와 idempotency row는 하나의 `BEGIN IMMEDIATE`
transaction에 저장된다. exact replay는 최초 durable binding을 복원하고, 같은 operation
identity의 다른 Manifest 또는 같은 Job ID의 다른 submission은 conflict로 거부한다.
Manifest insert 직후 fault는 세 durable row를 모두 rollback한다.

## load 결과가 의도적으로 `Verified`가 아닌 이유

`get_manifest_binding()`은 `StoredManifestBinding`을 반환한다. 이는
`Verified<JobManifest>`가 아니다. submission 시점에 서명이 유효했다는 durable signer
fact를 저장하는 것과, 재시작 시점의 authoritative key directory에서 그 서명이 지금도
유효한지 판정하는 것은 다른 권한 경계다. key rotation, revocation 또는 membership 변화가
있을 수 있으므로 caller가 현재 authoritative directory로 다시 검증하기 전에는 이 raw
Manifest를 scheduler 요구사항이나 Grant 입력으로 사용할 수 없다.

loader는 빈/해독 불가 body, hash mismatch, job ID, submitter device ID와 signer ID
mismatch를 typed `ManifestCorruption`으로 거부한다. 기존 schema에서 migration된 Job에
Manifest row가 없으면 암묵적으로 hash-only Job을 허용하지 않고 `LegacyManifestMissing`으로
fail closed한다.

## 자체 재검토 — legacy exact error와 body device identity fixture 보강

자체 재검토에서 pre-existing schema migration 뒤 legacy 오류 assertion이 전체 typed
오류와 job ID를 대조하지 않던 공백을 발견해 보강했다. 또한 저장 body의
`submitter_device_id`를 실제 변조해 `SubmitterDeviceIdMismatch`가 나는 직접 fixture가
없음을 찾아 추가했다. 이로써 row 부재와 body identity 손상을 서로 구분하면서 둘 다
fail closed함을 고정했다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 `job_store.rs:531,628`에서 production INSERT가 `&Verified<...>` 전용 API 한
곳에만 있고 inner fault 경로는 private임을 확인했다. `signing.rs:624,839`의 `Verified`
내부 필드는 private이며 정상 생성 경로는 서명 검증 함수뿐이다. raw Manifest 필드는
`job_store.rs:547`의 `verified.get()` 이후 처음 관찰되고, 그 전 검사는 durable side
effect를 만들지 않는다.

`job_store.rs:826`의 Job/device/signer identity 대조, `:553,847`의 hash 재계산·대조·
저장, `:559,625,1377`의 단일 transaction과 Manifest insert 뒤 전체 rollback을 대조했다.
`:149,401`에서 load 결과가 raw `StoredManifestBinding`이고 production scheduler/Grant
소비 경로가 없으며, `:875`의 손상 fail-closed와 `:573,1338`의 replay 대조·충돌 거부도
확인했다. `:1425,1306`의 두 뮤테이션이 지정 테스트에 검출되고, 변경이 `job_store.rs`
659 additions/0 deletions와 계획 문서 하나에 제한되며 기존 `DoD-42/43/47/49` 구현을
삭제·수정하지 않았음을 확인해 수정 요청 없이 1라운드 `ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-coordinator
  PASS — unit 95 + integration 4 = 99 passed, 0 failed
```

위 결과는 감독자가 직접 재확인했다. 첫 workspace 실행에서 기존 crypto lock-timeout
timing test가 1회 실패했으나 이 저장소의 알려진 flaky였고 재실행 시 통과했다. 이
일시적 실패는 `DoD-50` 판정 근거가 아니다.

## 이 실험이 증명하지 "않는" 것

- load 시 서명을 재검증하지 않는 것은 의도된 설계 — 호출자가 authoritative key
  directory 로 재검증해야 한다.
- membership validity, authoritative device→member 해석은 범위 밖이다.
- `JobRequirements` projection과 기본 `MIRRORED` durability 소비는 범위 밖이다.
- Grant/Lease scope, `COMMITTED` submission, production wire 연결은 범위 밖이다.
- submission-time signer ID는 현재 membership validity나 key rotation 뒤의 권위를
  증명하지 않는다.
- local SQLite `DURABLE` binding만 다루며 다중 Coordinator 합의나 Raft `COMMITTED`를
  증명하지 않는다.

## 결정

1. public 저장 경계를 `&Verified<pb::JobManifest>`로 제한해 raw·미검증 Manifest가 이
   production 경로로 durable 저장될 수 없게 했다.
2. `Verified::get()` 이후에만 Manifest identity를 관찰하고 Job/device/signer를 accepted
   identity와 대조하며 hash를 canonical signing input에서 재계산해 저장한다.
3. Job/body/signer/idempotency를 한 `BEGIN IMMEDIATE` transaction에 결합하고 fault
   rollback, exact replay/conflict와 typed corruption·legacy fail-closed를 검증했다.
4. load는 의도적으로 raw `StoredManifestBinding`이므로 현재 authoritative key directory로
   재검증하기 전에는 scheduler나 Grant에 사용할 수 없다. 독립 검수는 1라운드
   `ACCEPTED`, 감독자 재실행은 coordinator 99 passed였다.
5. scheduler 로드맵 진행: `DoD-41`~`DoD-50` 완료 — authoritative device→member 해석, `JobRequirements` projection, reservation release(실행 종료 증명 필요), Grant scope 생성, production 연결은 후속

관련: `docs/plans/2026-08-24_1142_scheduler_verified_manifest_durable_binding_v1.md`
