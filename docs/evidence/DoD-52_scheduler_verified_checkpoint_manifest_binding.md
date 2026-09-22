---
schema_version: 2
id: DoD-52
claim: "검증된 `CheckpointManifest` 를 durable Attempt/reservation 에 묶어 재시작 뒤 보존되는 first-write evidence로 만들고 stale manifest 혼입을 막았다 — 공개 저장 API는 `&Verified<pb::CheckpointManifest>`만 받고 `Verified::get()` 뒤에만 필드를 읽으며, 하나의 `BEGIN IMMEDIATE`를 획득한 뒤 같은 transaction 안에서 현재 durable Attempt·reservation의 job/attempt/producer/verified signer/fence/owner를 대조한 후에만 INSERT한다. signature 포함 complete body와 저장소가 직접 계산한 BLAKE3 hash를 저장하고, `root_digest`는 BLAKE3-256이면서 정확히 32바이트인 경우만 허용한다. exact replay는 최초 1행을 유지하고 changed replay·binding 불일치·손상은 fail closed하며 load는 의도적으로 `Verified`가 아니다. 자체 재검토에서 SHA-256 root를 허용하던 실제 결함을 고쳐 negative case로 고정했고, 독립 검수 1라운드 ACCEPTED와 감독자 coordinator 118 passed로 확인했다. `ReplicaAck` 저장·`MIRRORED` 및 checkpoint durability 전이와 Job/Attempt/Lease/reservation 상태 전이는 완료하지 않았다"
status: PASS
commit: 471fb077f2beb89fd31693ea320334ee7f2644c7

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — verified CheckpointManifest durable Attempt/reservation binding, replay·rollback·corruption negative test와 뮤테이션 검증"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-24T15:37:50+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 fresh-read-only 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "production 공개 저장 API와 INSERT가 각각 한 곳이고 raw 저장 경로가 없으며 manifest 최초 관찰이 verified.get() 이후임, signature 포함 complete body hash의 저장소 직접 계산, BLAKE3-256/32-byte root 전용 제한과 SHA-256 우회 부재, BEGIN IMMEDIATE 획득 뒤 같은 transaction 안의 durable Attempt·reservation job/attempt/producer/verified signer/fence/owner 대조와 transaction 밖 사전 판단 부재, exact replay 1행 유지·changed conflict, raw load가 Verified를 재구성하지 않고 production 소비자가 없음, load 전체 binding 재검사, fault 전체 rollback·상태 전이 미추가, 실제 production guard 두 곳을 뒤집은 뮤테이션 판별력을 확인해 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-52_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-52_scheduler_verified_checkpoint_manifest_binding_2026-08-24.txt"
raw_output_digest: "sha256:57274cc6297d92db6c821f64df42ca2a8476bc6b0cfe60bc2d96ed7df2e78681"
raw_output_bytes: 10114

binary_digests:
  toolchain: "cargo 사용 — 제공된 감독자 재실행 이력에 cargo/rustc version과 binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — 기존 signed CheckpointManifest와 BLAKE3 Checkpoint root 규범을 coordinator local SQLite schema와 Rust 저장 API에 결합"
  canonical_spec: "기존 CheckpointManifest Verified 서명 검증 경계를 계승 — load 시 현재 authoritative key directory 재검증과 ReplicaAck/durability consumer는 범위 밖"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test / SQLite local store"
hardware: "GPU 미사용 — signed protobuf fixture와 SQLite binding/corruption/fault fixture로 검증"
network_profile: "네트워크 미사용 — coordinator 단위·통합 테스트이며 production checkpoint ingress와 ReplicaAck routing은 미연결"
command: |
  cargo test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-52_scheduler_verified_checkpoint_manifest_binding_2026-08-24.txt,
   docs/evidence/_raw/DoD-52_review.txt 전문 참조)

  감독자 직접 확인: PASS — coordinator unit 114 + integration 4 = 118 passed,
  전체 0 failed, exit code 0
  독립 검수 1라운드: ACCEPTED — Verified 전용 저장 경계와 get() 이후 필드 관찰,
  signature 포함 body hash의 저장소 직접 계산, BLAKE3-256/32-byte root 제한,
  단일 BEGIN IMMEDIATE 안의 Attempt/reservation binding, replay 1행 유지, raw load,
  rollback·control state 무변경, corruption fail-closed와 production guard 뮤테이션을 확인
artifacts:
  - docs/plans/2026-08-24_1513_verified_checkpoint_manifest_durable_binding_v1.md
  - crates/coordinator/src/checkpoint_manifest_store.rs
  - crates/coordinator/src/lib.rs
  - docs/evidence/_raw/DoD-52_scheduler_verified_checkpoint_manifest_binding_2026-08-24.txt
  - docs/evidence/_raw/DoD-52_review.txt
negative_tests:
  - "complete_signed_manifest_and_store_derived_hash_survive_reopen: signature 포함 complete manifest body와 저장소 계산 BLAKE3 hash, root binding이 reopen 뒤 복원됨을 확인"
  - "missing_or_unspecified_root_and_blank_identity_create_no_row: blank checkpoint와 누락·unspecified·SHA-256·잘못된 길이 root digest를 row 없이 거부"
  - "wrong_job_attempt_and_stale_fence_create_no_row: manifest와 durable Attempt의 wrong job/attempt 및 stale fence는 row 없이 fail closed"
  - "producer_signer_mismatch_with_durable_owner_creates_no_row: producer·Verified signer·single-node Attempt·reservation owner의 일치를 강제"
  - "missing_or_different_reservation_owner_creates_no_row: reservation 부재 또는 job/attempt owner 불일치가 row를 만들지 않음"
  - "exact_replay_returns_first_row_and_changed_body_or_signature_conflicts: exact replay는 최초 1행과 body를 유지하고 changed body/signature는 conflict"
  - "storage_has_no_checkpoint_state_or_control_state_side_effects: checkpoint state 열이 없고 Job/Attempt/Lease/reservation이 불변"
  - "failure_after_insert_rolls_back_manifest_and_preserves_reservation: manifest INSERT 뒤 fault가 전체 rollback되고 Attempt/reservation은 보존"
  - "corrupt_body_hash_identity_signer_fence_and_root_fail_closed: body/hash/checkpoint/job/attempt/producer/signer/fence/root 손상을 typed corruption으로 거부"
  - "current_attempt_binding_corruption_fails_closed: load 때 현재 durable Attempt fence 변조를 fail closed"
  - "뮤테이션 1: producer/signer/Attempt/reservation owner production guard 제거 시 지정 negative test가 exit 101로 실패"
  - "뮤테이션 2: Attempt fence production guard 제거 시 지정 negative test가 exit 101로 실패"
limitations:
  - "load 시 서명을 재검증하지 않는 것은 의도된 설계 — 소비자가 당시 authoritative key directory로 다시 검증해야 한다"
  - "`ReplicaAck` 저장과 `MIRRORED` 전이, checkpoint durability 상태는 범위 밖"
  - "Job/Attempt/Lease/reservation 상태 전이 미추가"
  - "실제 checkpoint writer, 파일 BLAKE3/Merkle 검증, production checkpoint ingress와 routing은 범위 밖"
  - "membership/failure-domain/ephemeral 판정과 effective replica count는 범위 밖"
  - "artifact `COMMITTED`, canonical selection, Job/Attempt terminal transaction, runtime-stop proof와 reservation release는 범위 밖"
  - "local SQLite의 durable first-write evidence만 증명하며 다중 Coordinator 합의나 Raft/ControlStore `COMMITTED`를 증명하지 않는다"
decision: "직전 조사들은 계속 앞으로 나갈 조각이 하루 규모인가를 물었고, 선행 규범과 durable authority가 부족해 매번 없다고 판정했다. 이번에는 질문을 선행 조건의 첫 슬라이스가 하루 규모인가로 바꿨고, proto·Signable·framed Verified와 durable Attempt/reservation identity가 이미 있는 CheckpointManifest binding을 찾아 선택했다. public 저장 경계는 `&Verified<pb::CheckpointManifest>`로 제한하고 manifest 필드는 `Verified::get()` 뒤에만 관찰한다. signature 포함 complete protobuf body와 저장소가 직접 계산한 BLAKE3 hash를 저장한다. 신규 row 전에 하나의 `BEGIN IMMEDIATE`를 획득하고 같은 transaction 안에서 현재 durable Attempt와 reservation의 job/attempt/producer/verified signer/fence/owner를 대조하며 transaction 밖의 사전 owner 판단 경로를 두지 않는다. root는 BLAKE3-256이면서 정확히 32바이트일 때만 허용한다. 초기 구현이 SHA-256 root도 허용한 실제 결함을 자체 재검토에서 발견해 production 검사를 고치고 SHA-256 negative case로 고정했다. exact replay는 최초 1행을 생성·변경 없이 반환하고 changed body/signature는 conflict다. load는 의도적으로 Verified가 아니며 전체 durable binding 손상을 재검사하고 검증 없이 쓰는 production consumer가 없다. fault는 전체 rollback하고 Job/Attempt/Lease/reservation과 checkpoint durability 상태를 변경하지 않는다. 독립 검수는 public API·유일 INSERT·검증 순서·transaction 대조·root 제한·replay·raw load·corruption·rollback·상태 무변경과 실제 production guard 뮤테이션 두 건을 확인해 1라운드 ACCEPTED했고 감독자는 coordinator unit 114+integration 4=118 passed를 직접 재확인했다. scheduler 로드맵 진행: `DoD-41`~`DoD-52` 완료. 이 조각은 `ReplicaAck` 저장의 `checkpoint_id`/`root_digest` anchor 가 되어 `MIRRORED` 판정 경로를 연다. membership/ControlStore 계열(authoritative device→member 해석, `JobRequirements` projection)은 `Signable`·signer identity·lifetime·member 상태 규범이 없어 규범 확정 약 2일, durable 저장까지 누적 약 3일로 별도 과제다."
---

# DoD-52 · verified CheckpointManifest durable binding

## 무엇을 입증하려 했는가

`DoD-51`까지 scheduler는 hard-filter부터 verified JobManifest와 terminal AttemptReport의
durable binding까지 갖췄다. 그러나 signed `CheckpointManifest`를 현재 durable Attempt와
reservation owner에 묶어 보존하는 coordinator anchor가 없었다. 이 상태에서는 향후
`ReplicaAck`를 받아도 ACK의 `checkpoint_id`/`root_digest`가 어느 current producer
checkpoint를 가리키는지 durable authority와 대조할 수 없다.

직전 조사들은 계속 **앞으로 나갈 조각이 하루 규모인가**를 물었다. authoritative
device→member 해석, `JobRequirements` projection, `MIRRORED` 소비, Job/Attempt terminal
전이와 reservation release는 선행 규범이나 durable authority가 부족해 매번 답이
"없다"였다. 이번에는 질문을 **선행 조건의 첫 슬라이스가 하루 규모인가**로 바꿨다.
`CheckpointManifest`는 proto·`Signable<Lifetime::Evidence>`·framed `Verified`와 durable
Attempt/reservation identity가 이미 있어, durable anchor만 먼저 만드는 조각은 하루 범위에
들어왔다.

따라서 이 조각은 checkpoint 파일을 검증하거나 durability state를 전이하지 않고, 현재 owner가
제출한 검증된 manifest를 durable first-write fact로 묶는 선행 경계만 검증했다.

## 구현 — `Verified` 전용 inbox와 complete signed body

신규 `CoordinatorCheckpointManifestStore`에
`store_verified_manifest(&Verified<pb::CheckpointManifest>)`와 raw load
`get_manifest_binding()`을 추가했다. `coordinator_checkpoint_manifests`는 primary key
`checkpoint_id`, job/attempt/producer/verified signer, big-endian BLOB `fence_epoch`, encoded
`root_digest`, signature를 포함한 complete `manifest_body`, 저장소가 body에서 직접 계산한
BLAKE3 `manifest_hash`를 저장한다.

저장 API는 raw manifest를 받지 않는다. `Verified::get()` 이후에만 manifest 필드를 읽고
`Verified::signer_id()`를 꺼낸다. caller hash도 받지 않으며 signature를 포함한 complete
protobuf body에서 직접 hash를 계산한다.

## 핵심 안전 속성 — write lock 뒤 같은 transaction의 owner 대조

신규 manifest row는 `BEGIN IMMEDIATE`를 획득한 뒤 같은 transaction에서 durable Attempt와
현재 reservation을 읽고 다음을 모두 통과해야 한다.

1. manifest job/attempt = durable Attempt job/attempt = reservation job/attempt.
2. manifest producer = `Verified::signer_id()` = single-node Attempt node = reservation node.
3. manifest fence = durable Attempt fence.

모든 대조 뒤에만 INSERT한다. transaction 밖에서 owner나 fence를 미리 읽고 판단하는 경로가
없으므로 stale read를 write에 사용하는 TOCTOU가 없다. reservation 부재나 owner 불일치는
row 없이 거부한다.

## Checkpoint root — BLAKE3-256, 정확히 32바이트

`root_digest`는 `HASH_ALGORITHM_BLAKE3_256`이면서 값 길이가 정확히 32바이트일 때만
허용한다. `common.proto`의 SHA-256 허용은 OCI 호환 전용이고 Checkpoint Merkle root는
BLAKE3 규범을 따르므로, SHA-256·unspecified·잘못된 길이는 INSERT 전에 fail closed한다.

## 자체 재검토 — SHA-256 허용 결함 발견과 수정

초기 구현은 구조적으로 유효한 SHA-256 root digest도 허용했다. 자체 재검토에서 이 실제
결함을 발견했다. production root 검사를 BLAKE3-256/정확히 32바이트 전용으로 수정하고,
SHA-256 manifest가 row를 만들지 않는 negative case를 추가해 회귀를 고정했다.

load가 authoritative key directory 없이 `Verified`를 재구성하지 않는지, 모든 durable owner
read가 INSERT와 같은 `BEGIN IMMEDIATE` 안에 있는지, replay가 overwrite하지 않는지, 신규
테이블에 durability state 열이 없는지도 다시 확인했다.

## replay와 load 경계

exact semantic replay는 기존 row의 complete manifest 의미와 submission-time signer를
대조해 둘 다 같을 때 최초 binding을 `created=false`로 반환한다. 행을 늘리거나 덮어쓰지
않는다. body 또는 signature가 다르면 typed conflict다.

`get_manifest_binding()`은 의도적으로 raw `StoredCheckpointManifestBinding`을 반환한다.
loader는 body hash/decode, row↔body checkpoint/job/attempt/producer, signer, big-endian fence,
root와 현재 durable Attempt binding을 재검사하고 손상을 typed corruption으로 거부한다.
이 raw 결과를 서명 재검증 없이 checkpoint durability나 canonical selection에 쓰는 production
consumer는 없다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 `checkpoint_manifest_store.rs:178,241`에서 저장 API와 production INSERT가 각각
한 곳뿐이고 raw 저장 경로가 없음을 확인했다. manifest 최초 접근은 `:192`의
`verified.get()` 뒤고 signature 포함 body hash 계산은 `:195`다. `:203,223,239`에서
`BEGIN IMMEDIATE` 획득 뒤 같은 transaction의 Attempt/reservation read·대조·INSERT 순서를
확인해 transaction 밖 사전 owner 판정이 없음을 대조했다.

`:311`은 root를 BLAKE3-256/32-byte로만 제한해 SHA-256 우회가 없다. `:20`의 load 결과는
`Verified`가 아니며 production 소비자가 없고, `:376-507`은 body/hash/identity/signer/fence/
root/current Attempt binding을 재검사한다. replay 1행 유지·changed conflict·fault 전체
rollback·상태 전이 미추가와 실제 production guard `:357,332`를 뒤집은 뮤테이션 두 건도
확인했다. 수정 요청 없이 1라운드에서 `ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-coordinator
  PASS — unit 114 + integration 4 = 118 passed, 0 failed
```

감독자가 위 결과를 직접 재확인했다. 구현 세션의 Windows runtime 제외 workspace build/test와
집중 checkpoint manifest store 테스트 10건도 통과했다. `rustfmt` component가 없어
`cargo fmt`는 실행하지 못했지만 build/test에는 영향이 없었다.

## 이 evidence가 증명하지 않는 것

- load 시 서명을 재검증하지 않는 것은 의도된 설계다. 소비자가 당시 authoritative key
  directory로 다시 검증해야 한다.
- `ReplicaAck` 저장과 `MIRRORED` 전이, checkpoint durability 상태는 범위 밖이다.
- Job/Attempt/Lease/reservation 상태 전이 미추가다.
- 실제 checkpoint writer, 파일 BLAKE3/Merkle 검증과 production ingress/routing은 범위 밖이다.
- membership/failure-domain/ephemeral 판정과 effective replica count는 범위 밖이다.
- artifact `COMMITTED`, canonical selection, terminal transaction, runtime-stop proof와
  reservation release는 범위 밖이다.
- local SQLite durable first-write evidence만 증명하며 다중 Coordinator 합의나
  Raft/ControlStore `COMMITTED`를 증명하지 않는다.

## 결정

직전 조사들의 질문은 "앞으로 나갈 조각이 하루 규모인가"였고, 매번 선행 규범과 durable
authority 부족 때문에 "없다"였다. 이번에는 "선행 조건의 첫 슬라이스가 하루 규모인가"로
질문을 바꿨고, 그 결과 verified `CheckpointManifest` durable Attempt/reservation binding을
선택했다.

1. 공개 저장 경계를 `&Verified<pb::CheckpointManifest>`로 제한하고 manifest 필드는
   `Verified::get()` 뒤에만 관찰한다.
2. signature 포함 complete protobuf body와 저장소가 직접 계산한 BLAKE3 hash를 저장한다.
3. `BEGIN IMMEDIATE` 획득 뒤 같은 transaction에서 현재 Attempt·reservation의 job/attempt/
   producer/verified signer/fence/owner를 대조하고, transaction 밖 사전 판단 경로를 두지 않는다.
4. `root_digest`는 BLAKE3-256이면서 정확히 32바이트인 경우만 허용한다.
5. 초기 SHA-256 root 허용 결함을 자체 재검토에서 고치고 negative case로 고정했다.
6. exact replay는 최초 1행을 유지하고 changed replay와 corruption은 fail closed한다.
7. load는 의도적으로 `Verified`가 아니며 재검증 전 durability 결정에 쓰지 않는다.
8. 독립 검수는 1라운드 `ACCEPTED`, 감독자 재실행은 coordinator 118 passed였다.

scheduler 로드맵 진행: `DoD-41`~`DoD-52` 완료. 이 조각은 `ReplicaAck` 저장의 `checkpoint_id`/
`root_digest` anchor 가 되어 `MIRRORED` 판정 경로를 연다. membership/ControlStore 계열(authoritative device→member
해석, `JobRequirements` projection)은 `Signable`·signer identity·lifetime·member 상태 규범이 없어 규범 확정 약 2일,
durable 저장까지 누적 약 3일로 별도 과제다.

관련: `docs/plans/2026-08-24_1513_verified_checkpoint_manifest_durable_binding_v1.md`
