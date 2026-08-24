---
schema_version: 2
id: DoD-53
claim: "검증된 `ReplicaAck` 를 durable checkpoint/root anchor 에 묶어 재시작 뒤에도 보존되는 immutable signed observation으로 만들었다 — 공개 저장 API는 `&Verified<pb::ReplicaAck>`만 받고 `Verified::get()` 뒤에 body를 encode해 signature 포함 complete body의 BLAKE3 hash를 저장소가 직접 계산한 다음 `BEGIN IMMEDIATE`를 획득한다. 구조 검증, verified signer↔holder 대조, body/hash와 Attempt job/node/fence binding까지 재검사하는 DoD-52 anchor 조회, exact root 대조, replay 대조와 INSERT는 모두 같은 transaction 안에서 수행한다. `root_digest`는 BLAKE3-256이면서 정확히 32바이트인 경우만 허용하고, exact replay는 최초 observation을 유지하며 later `acked_at_unix_ms`는 별도 행으로 보존한다. raw load는 의도적으로 `Verified`가 아니고 전체 binding 손상을 fail closed한다. 독립 검수 1라운드 ACCEPTED와 감독자 coordinator 121 passed + 5 passed로 확인했다. `MIRRORED` 판정·전이, effective replica count와 membership/failure-domain 판정은 완료하지 않았다"
status: PASS
commit: 101f132dba92b861d37b63cbd71d9b7c23aeb63e

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — verified ReplicaAck durable checkpoint/root binding, replay·rollback·corruption negative test와 뮤테이션 검증"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-24T16:10:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 fresh-read-only 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "production 저장 API의 Verified type gate와 compile-fail doctest, Verified::get()·complete body encode·BLAKE3 계산이 BEGIN IMMEDIATE 전이라는 정확한 순서, 구조·signer/holder·validated DoD-52 anchor·exact root·replay·INSERT가 같은 transaction 안이라는 TOCTOU 경계, BLAKE3-256/32-byte root 제한, immutable observation replay와 later row, raw load·corruption·rollback·control state 무변경, acked_at PK 손상 test의 list 경로 수정, 실제 production anchor-root/body-hash guard 두 곳을 뒤집은 뮤테이션 판별력을 확인해 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-53_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-53_scheduler_verified_replica_ack_binding_2026-08-24.txt"
raw_output_digest: "sha256:cbdec94ab4427ffef9cc6d1f1ed635056c094172089117e53f0782434160ed9a"
raw_output_bytes: 8727

binary_digests:
  toolchain: "cargo 사용 — 제공된 감독자 재실행 이력에 cargo/rustc version과 binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — 기존 signed ReplicaAck, Evidence lifetime과 BLAKE3 Checkpoint root 규범을 coordinator local SQLite schema와 Rust 저장 API에 결합"
  canonical_spec: "기존 ReplicaAck Verified 서명 검증 경계를 계승 — load 시 현재 authoritative key directory 재검증과 membership-backed durability consumer는 범위 밖"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test / SQLite local store"
hardware: "GPU 미사용 — signed protobuf fixture와 SQLite anchor/binding/corruption/fault fixture로 검증"
network_profile: "네트워크 미사용 — coordinator 단위·통합·compile-fail doctest이며 production ReplicaAck routing은 미연결"
command: |
  cargo test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-53_scheduler_verified_replica_ack_binding_2026-08-24.txt,
   docs/evidence/_raw/DoD-53_review.txt 전문 참조)

  감독자 직접 확인: PASS — coordinator unit 121 + integration/doctest 5 = 126 passed,
  전체 0 failed, exit code 0
  독립 검수 1라운드: ACCEPTED — Verified 전용 저장 경계와 compile-fail doctest,
  get/body encode/hash의 transaction 전 실행, 구조·identity·validated DoD-52 anchor·root·
  replay·INSERT의 단일 BEGIN IMMEDIATE transaction, BLAKE3-256/32-byte root 제한,
  immutable observation replay, raw load, corruption·rollback·상태 무변경과 production guard
  뮤테이션을 확인
artifacts:
  - docs/plans/2026-08-24_1556_verified_replica_ack_durable_binding_v1.md
  - crates/coordinator/src/replica_ack_store.rs
  - crates/coordinator/src/checkpoint_manifest_store.rs
  - crates/coordinator/src/lib.rs
  - docs/evidence/_raw/DoD-53_scheduler_verified_replica_ack_binding_2026-08-24.txt
  - docs/evidence/_raw/DoD-53_review.txt
negative_tests:
  - "complete signed ACK and store-derived hash survive reopen: holder_signature 포함 complete body와 저장소 계산 BLAKE3 hash, checkpoint/root binding이 reopen 뒤 복원됨을 확인"
  - "missing anchor, wrong root, SHA-256 root와 31-byte root는 ACK row를 만들지 않음"
  - "exact replay는 최초 observation 1행을 유지하고 changed body/signature는 conflict이며 later acked_at은 별도 immutable row"
  - "list_ack_bindings는 (acked_at, holder) 결정 순서를 유지하고 count·eligibility 결론을 반환하지 않음"
  - "fsynced=false, hash_verified=false와 unspecified kind도 저장 단계에서는 signed claim으로 보존하고 유효 replica로 해석하지 않음"
  - "empty/truncated/undecodable body, hash encoding/mismatch, invalid body, checkpoint/holder/signer/time/root row binding 손상을 typed corruption으로 거부"
  - "missing/corrupt/changed CheckpointManifest anchor와 root binding 손상을 load에서 fail closed"
  - "failure after INSERT rolls back ACK row and preserves manifest plus Job/Attempt/Lease/reservation; schema has no state/count/mirrored column"
  - "self-review: acked_at PK BLOB 손상 행은 기존 key로 단건 조회할 수 없는 test 설계 결함을 list 경로의 AckedAtEncoding 검사로 수정"
  - "뮤테이션 1: production DoD-52 anchor root guard 제거 시 wrong-root negative test가 exit 101로 실패"
  - "뮤테이션 2: production load complete-body BLAKE3 hash guard 제거 시 HashMismatch case가 exit 101로 실패"
limitations:
  - "후속 소비자는 holder별 dedup/freshness를 반드시 수행해야 한다. PK에 acked_at_unix_ms를 포함한 immutable observation history는 later observation을 보존하므로, 그대로 replica 수로 세면 안 된다"
  - "별도 retention 정책 없이는 인증된 holder가 서로 다른 acked_at_unix_ms의 관측 행을 무제한 누적시킬 수 있다는 운영 위험이 남는다"
  - "load 시 서명을 재검증하지 않는 것은 의도된 설계 — 소비자가 당시 authoritative key directory로 다시 검증해야 한다"
  - "`MIRRORED` 판정·전이, effective replica count, membership/failure-domain 판정은 범위 밖"
  - "저장 결과의 production 소비자는 아직 없다"
  - "holder current liveness, ephemeral/동일-device 제외, fsync/hash/kind/stored_bytes eligibility 해석은 범위 밖"
  - "checkpoint/Job/Attempt/Lease/reservation 상태 전이, artifact COMMITTED, canonical selection, terminal transaction과 reservation release는 범위 밖"
  - "production ReplicaAck producer/session routing과 다중 Coordinator 합의 또는 Raft/ControlStore COMMITTED는 범위 밖"
decision: "DoD-52가 durable CheckpointManifest checkpoint/root authority를 제공하므로 그 위에 verified ReplicaAck observation 저장을 하루 선행 조각으로 선택했다. 공개 저장 경계는 `&Verified<pb::ReplicaAck>`로 제한하고 raw protobuf 호출은 compile-fail doctest로 막는다. 실제 순서는 `Verified::get()`, signature 포함 complete body encode와 저장소 BLAKE3 hash 계산 뒤 `BEGIN IMMEDIATE`를 획득한다. 구조 검증, verified signer↔holder 대조, validated DoD-52 anchor 조회, exact root 대조, observation replay 대조와 INSERT는 같은 transaction 안이므로 TOCTOU가 없다. anchor helper는 기존 manifest body/hash와 Attempt job/node/fence binding까지 재검사하며 외부 API나 검증 로직은 바꾸지 않았다. root는 BLAKE3-256이면서 정확히 32바이트일 때만 허용한다. primary key의 acked_at_unix_ms는 immutable observation history 설계이며 exact replay는 최초 row를 유지하고 later observation은 별도 행이다. 자체 재검토에서 PK time BLOB 손상 행이 기존 key 기반 get으로 보이지 않는 test 설계 결함을 찾아 list 경로의 fail-closed 검사로 바꿨다. load는 의도적으로 Verified가 아니며 complete body/hash와 row·anchor binding을 재검사한다. 독립 검수는 정확한 lock 전·후 순서, transaction anchor/root binding, replay/load·rollback·상태 무변경과 실제 production root/hash guard 뮤테이션을 확인해 1라운드 ACCEPTED했고 감독자는 coordinator 121 passed + 5 passed를 직접 재확인했다. 후속 소비자는 holder별 dedup/freshness와 retention을 책임져야 하며 이번 저장은 유효 replica나 MIRRORED 전이를 뜻하지 않는다. scheduler 로드맵 진행: `DoD-41`~`DoD-53` 완료. `DoD-52` anchor 위에 `ReplicaAck` 저장이 쌓여 `MIRRORED` 판정의 입력이 durable 해졌다. membership/ControlStore 계열은 `Signable`·signer identity·lifetime·member 상태 규범이 없어 규범 확정 약 2일, durable 저장까지 누적 약 3일로 별도 과제다."
---

# DoD-53 · verified ReplicaAck durable checkpoint/root binding

## 무엇을 입증하려 했는가

`DoD-52`는 검증된 `CheckpointManifest`를 현재 durable Attempt/reservation과 BLAKE3-256 root에
묶는 anchor를 만들었다. 그러나 signed `ReplicaAck`를 받아도 그 ACK observation을 해당
checkpoint/root에 묶어 재시작 뒤 보존하는 coordinator 저장 경계가 없었다. transient frame을
즉석에서 세는 대신 membership-backed consumer가 나중에 다시 검증할 durable 입력이 먼저
필요했다.

따라서 이 조각은 holder membership이나 effective replica count를 판정하지 않고, 검증 당시
holder key로 검증된 ACK claim을 현재 validated DoD-52 checkpoint/root anchor에 원자적으로
묶어 immutable observation history로 보존하는 선행 경계만 검증했다.

## 구현 — `Verified` 전용 inbox와 complete signed body

신규 `CoordinatorReplicaAckStore`에
`store_verified_ack(&Verified<pb::ReplicaAck>)`, raw `get_ack_binding()`과
`list_ack_bindings()`을 추가했다. `coordinator_replica_acks`는 primary key
`(checkpoint_id, holder_device_id, acked_at_unix_ms)`, verified signer, encoded root,
signature를 포함한 complete `ack_body`, 저장소가 body에서 직접 계산한 BLAKE3 `ack_hash`를
저장한다. `acked_at_unix_ms`는 u64 big-endian BLOB이다.

저장 API는 raw ACK를 받지 않으며 compile-fail doctest가 이 type gate를 고정한다.
`Verified::get()` 뒤에만 ACK 필드를 읽고 caller hash는 받지 않는다. `holder_signature`를 포함한
complete protobuf body에서 저장소가 hash를 직접 계산한다. load/list는 의도적으로 raw
`StoredReplicaAckBinding`을 반환한다.

## 핵심 안전 속성 — 정확한 lock 전·후 순서

production 순서는 다음과 같다.

1. `Verified::get()`으로 ACK를 연다.
2. signature 포함 complete body를 encode한다.
3. complete body의 BLAKE3 hash를 계산한다.
4. `BEGIN IMMEDIATE` transaction을 획득한다.
5. transaction 안에서 ACK 구조와 verified signer↔holder를 검사한다.
6. 같은 transaction 객체로 DoD-52 anchor를 validated fetch한다.
7. transaction 안에서 ACK root와 anchor root를 exact 비교한다.
8. transaction 안에서 observation replay를 대조한다.
9. 모든 검사를 통과한 뒤 같은 transaction에서 INSERT한다.

즉 `Verified::get()`, body 인코딩과 BLAKE3 계산은 `BEGIN IMMEDIATE` 이전이다. 모든 단계가
transaction 안이라고 뭉뚱그리면 실제 구현과 다르다. 그러나 durable 판단에 쓰이는 구조 검증,
signer↔holder, DoD-52 anchor, exact root, replay 대조와 INSERT는 전부 같은 write transaction
안이므로 stale anchor 판단을 write에 쓰는 TOCTOU 경로는 없다.

`checkpoint_manifest_store.rs`의 기존 schema 초기화와 완전 검증 fetch는 `pub(crate)` helper로
추출했다. 기존 SQL과 검증 로직은 그대로고 visibility만 넓혔다. helper는 manifest body/hash와
row identity/signer/fence/root뿐 아니라 현재 Attempt의 job/single-node/fence binding까지 다시
검사하므로 ACK store가 더 약한 anchor SQL을 복제하지 않는다.

## root와 immutable observation history

ACK `root_digest`는 BLAKE3-256이면서 정확히 32바이트인 경우만 허용한다. `common.proto`에
SHA-256 enum이 있어도 ACK validator는 SHA-256·unspecified·잘못된 길이를 INSERT 전에
거부한다. checkpoint가 없거나 root가 validated DoD-52 anchor와 exact match하지 않아도 row를
만들지 않는다.

같은 `(checkpoint_id, holder_device_id, acked_at_unix_ms)`와 complete body/signer의 exact replay는
최초 row를 `created=false`로 반환한다. changed body/signature는 typed conflict다. 더 늦은
`acked_at_unix_ms`는 계획이 정한 immutable observation history에 따라 별도 행으로 보존한다.
현재는 count나 production consumer API가 없어 즉시 중복 계산 구멍은 아니지만, 후속 소비자는
holder별 dedup/freshness를 수행해야 한다.

## replay와 load 경계

`get_ack_binding()`과 `list_ack_bindings()`은 raw evidence만 반환한다. loader는 body hash/decode,
row↔body checkpoint/holder/time/root, submission-time signer, u64 big-endian time encoding과 현재
validated DoD-52 anchor/root를 다시 검사한다. 손상은 typed corruption으로 fail closed한다.

load 시 서명을 재검증하지 않는 것은 의도된 설계다. 현재 authoritative key directory가 없는
저장소가 `Verified`를 재구성해서는 안 되며, 후속 소비자가 durability 판단 전에 다시 검증해야
한다. 이 raw 결과를 사용하는 production 소비자는 아직 없다.

## 자체 재검토 — PK time 손상 test의 설계 결함 수정

`acked_at_unix_ms` BLOB은 primary key 일부다. 이를 손상시킨 뒤 원래 시각 key로
`get_ack_binding()`을 호출하면 손상 행 자체를 찾을 수 없어 time decoding 검사가 실행되지 않는다.
자체 재검토에서 이 test 설계 결함을 발견하고, 해당 `AckedAtEncoding` case는 checkpoint 전체를
읽는 `list_ack_bindings()` 경로에서 실제 손상 행을 decode해 fail closed하도록 바꿨다.

BLAKE3-256/32-byte root 제한, exact replay와 later observation 분리, state/count/mirrored column
부재도 다시 확인했다.

## 뮤테이션과 독립 검수 1라운드 — **ACCEPTED**

production DoD-52 anchor root guard를 실제로 제거하자
`missing_anchor_wrong_root_sha256_and_wrong_length_create_no_row`가 wrong-root ACK 삽입을 관측하며
실패했다. load의 complete-body BLAKE3 hash guard를 제거하자
`corrupt_body_hash_identity_signer_time_and_root_fail_closed`의 `HashMismatch` case가 실패했다.
두 guard를 원복한 뒤 suite가 다시 통과했다.

독립 검수자는 `replica_ack_store.rs:232`의 Verified-only API와 `:224`의 compile-fail doctest,
`:246-251`의 `get()`·body encode·hash 계산 뒤 transaction 획득 순서를 확인했다. `:259-319`에서
구조·signer/holder·validated anchor·root·replay·INSERT가 같은 transaction 안임을 대조했다.
`checkpoint_manifest_store.rs:265,378-504`의 crate-private helper가 기존 body/hash와 Attempt
binding 검사를 유지하는지도 확인했다.

또한 root 제한, immutable replay/later observation, raw load와 corruption, PK time 손상 case의
list 경로 수정, rollback·상태 무변경과 두 production guard 뮤테이션을 확인했다. 초기 설명이
body/hash 계산까지 transaction 안이라고 뭉뚱그린 문서 불일치를 지적해 본 evidence에 실제
순서를 반영했고, 구현 자체는 수정 요청 없이 1라운드에서 `ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-coordinator
  PASS — unit 121 + integration/doctest 5 = 126 passed, 0 failed
```

감독자가 위 결과를 직접 재확인했다. 구현 세션의 집중 replica ACK store 테스트 7건,
Windows runtime 제외 workspace build/test와 canonical vector 48개 대조도 통과했다.
`rustfmt` component가 없어 `cargo fmt`는 실행하지 못했지만 build/test에는 영향이 없었다.

## 이 evidence가 증명하지 않는 것

- 후속 소비자는 holder별 dedup/freshness를 반드시 수행해야 한다. later observation이 별도 행인
  immutable history를 그대로 replica 수로 세면 안 된다.
- 별도 retention 정책 없이는 인증된 holder가 서로 다른 `acked_at_unix_ms`의 관측 행을
  무제한 누적시킬 수 있다는 운영 위험이 남는다.
- load 시 서명을 재검증하지 않는 것은 의도된 설계다. 소비자가 당시 authoritative key
  directory로 다시 검증해야 한다.
- `MIRRORED` 판정·전이, effective replica count, membership/failure-domain 판정은 범위 밖이다.
- 저장 결과의 production 소비자는 아직 없다.
- holder current liveness, ephemeral/동일-device 제외와 fsync/hash/kind/bytes eligibility 해석은
  범위 밖이다.
- checkpoint/Job/Attempt/Lease/reservation 상태 전이, artifact `COMMITTED`, canonical selection,
  terminal transaction과 reservation release는 범위 밖이다.
- production ReplicaAck producer/session routing과 다중 Coordinator 합의 또는
  Raft/ControlStore `COMMITTED`는 범위 밖이다.

## 결정

`DoD-52`가 durable CheckpointManifest checkpoint/root authority를 열었으므로, 그 다음 선행
슬라이스로 verified `ReplicaAck` observation 저장을 선택했다.

1. 저장 경계를 `&Verified<pb::ReplicaAck>`로 제한하고 raw 호출은 compile-fail doctest로 막는다.
2. `Verified::get()` 뒤 complete signed body를 encode하고 저장소가 BLAKE3 hash를 직접 계산한다.
3. 이 세 단계 뒤 `BEGIN IMMEDIATE`를 획득하며, 구조·signer/holder·validated DoD-52 anchor·
   exact root·replay 대조와 INSERT는 같은 transaction 안에서 수행한다.
4. root는 BLAKE3-256이면서 정확히 32바이트일 때만 허용한다.
5. exact replay는 최초 observation을 유지하고 later `acked_at`은 별도 immutable row로 보존한다.
6. PK time 손상 case는 원래 key 기반 get이 아니라 list 경로로 실제 손상 행을 검사한다.
7. load는 의도적으로 `Verified`가 아니며 전체 row·anchor binding 손상을 fail closed한다.
8. 독립 검수는 1라운드 `ACCEPTED`, 감독자 재실행은 coordinator 121 passed + 5 passed였다.

scheduler 로드맵 진행: `DoD-41`~`DoD-53` 완료. `DoD-52` anchor 위에 `ReplicaAck` 저장이 쌓여
`MIRRORED` 판정의 입력이 durable 해졌다. membership/ControlStore 계열은 `Signable`·signer identity·lifetime·
member 상태 규범이 없어 규범 확정 약 2일, durable 저장까지 누적 약 3일로 별도 과제다.

관련: `docs/plans/2026-08-24_1556_verified_replica_ack_durable_binding_v1.md`
