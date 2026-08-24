# 2026-08-24_1513 verified CheckpointManifest durable binding v1

- **조사 질문:** 큰 선행 조건의 "전체"가 아니라 가장 아래의 첫 슬라이스가 1일인가
- **최우선 조사:** authoritative membership directory / ControlStore
- **오늘 착수 결론:** **검증된 signed `CheckpointManifest`의 durable Attempt/reservation binding**
- **예상 규모:** coordinator production Rust 120~220줄, 재시작·손상·replay·rollback
  테스트 220~340줄, 문서/evidence 별도 — **1일**
- **proto/protocol 변경:** 없음
- **production producer/routing:** 없음

## 결론

membership에 `DoD-50`/`DoD-51` 패턴을 지금 그대로 적용할 수는 없다. 현재 schema에는
`AddMember`·`RemoveMember`·`ApproveDevice`·`RevokeDevice`라는 **action**과 조회용
`DeviceRecord`는 있지만, signed member/device membership **record**는 없다. 더 결정적으로
action들은 canonical/domain까지만 구현돼 있고 `Signable`이 없어 `Verified<AddMember>`나
`Verified<ApproveDevice>`를 만들 수 없다. 메시지 안에는 일반 검증 경계가 요구하는
`schema_version`, canonical signer ID, lifetime 입력도 없고 `RevokeDevice`는 단일
`Verified<M>`가 표현하지 못하는 다중 서명이다.

규범도 이 공백을 의도적으로 막고 있다. `docs/protocol/signing.md` §5는 membership action의
서로 다른 domain tag만 등록하지만(`:262-265`), §9 lifetime 표에는 membership action이 없다
(`:562-575`). 코드도 "§9 표에 없는 메시지는 ADR 뒤에 구현"하라고 명시한다
(`crates/protocol/src/signable.rs:21-25`). `docs/protocol/state-machines.md`의 Node 표는 승인과
폐기를 `COMMITTED`로 요구하지만(`:49`, `:52`), member add/remove state machine은 없다.
따라서 raw action을 local SQLite에 넣은 뒤 authoritative directory라고 부를 수 없고,
검증되지 않은 action을 `Verified`처럼 포장해서도 안 된다.

membership의 첫 정직한 슬라이스는 **membership authorization/record 규범과 검증 타입을
먼저 확정하는 것**이다. Owner signer identity, team/revision, lifetime, action과 snapshot 중
무엇을 durable fact로 삼는지, multi-signature를 `Verified` 경계와 어떻게 결합하는지를 ADR과
schema에 정한 뒤 canonical/reference vectors와 `Signable` 또는 별도 threshold-verified 타입을
구현해야 한다. 이 기반만 약 **2일**, 그 다음 verified durable inbox가 **1일**이다. 즉 질문의
형태인 "`&Verified<...>`만 받는 signed membership record 저장"까지의 첫 기능 슬라이스는
약 **3일**이며 오늘 하루 조각이 아니다.

반면 `MIRRORED` 경로에는 이미 완성된 signed evidence 타입이 있다. `CheckpointManifest`는
proto, canonical, `Signable`, `Verified` framed ingress가 모두 존재한다. coordinator에는
그것을 묶을 durable `(job, attempt, node, fence)`와 reservation도 있다. 먼저 manifest를
해석하거나 checkpoint state를 바꾸지 않고 first-write durable fact로 보존하면, 다음
`ReplicaAck` 저장 시 `checkpoint_id`와 `root_digest`를 대조할 durable anchor가 생긴다.
이것이 고립된 ACK inbox를 먼저 만드는 것보다 아래층이며, 이후
`ReplicaAck` 저장 → membership/failure-domain 판정 → `MIRRORED` 충족 → final artifact guard →
Attempt/Job terminal 경로를 순서대로 연다.

## 1. membership / ControlStore 최우선 조사

### (a) 규범이 허용하는 범위

`docs/protocol/signing.md`:

- §5 domain 표는 `AddMember`, `RemoveMember`, `ApproveDevice`, `RevokeDevice`에 각각 독립
  domain을 부여한다(`:262-265`). 이것은 canonical signature context만 정한다.
- §8은 서명 검증과 membership/승인 확인을 통과하기 전에는 필드를 신뢰하지 말라고 한다
  (`:514-527`).
- §9 lifetime 표에는 네 membership action이 없다(`:562-575`). 추측으로
  `LongLived`/`Evidence`/`Perpetual`을 고를 수 없다.
- §13.2는 상위 코드가 `Verified<M>`만 받는 타입 게이트를 요구한다(`:849-864`).

`docs/protocol/state-machines.md`:

- Node `ENROLLING -> APPROVED`는 Owner 서명 유효를 guard로 하고 `COMMITTED`다(`:49`).
- Node `APPROVED -> REVOKED`도 Owner 서명과 `COMMITTED`를 요구한다(`:52`).
- member add/remove의 상태·revision·revocation 표는 없다. 규범 표에 없는 member lifecycle을
  coordinator가 임의로 만들 수 없다.

결론: 규범은 signed/committed membership **결과**를 요구하지만, 현재 일반
`Verified<M>` ingress와 member directory apply 계약을 완성할 만큼의 lifetime/signer/state
규칙은 제공하지 않는다. local record-only store가 committed authority를 대신하는 것은
허용되지 않는다.

### (b) 현재 타입의 실제 범위

`proto/control.proto`:

- `ControlAction`에 네 membership action이 있다(`:238-244`).
- `AddMember`/`RemoveMember`는 member ID와 Owner signature를, `ApproveDevice`는
  device/member/public key/peer/key protection/ephemeral과 Owner signature를 갖는다
  (`:288-308`).
- `RevokeDevice`는 Owner 또는 2-of-3 Coordinator의 `repeated bytes signatures`다
  (`:310-315`).
- 조회 결과에는 `DeviceRecord { device_id, member_id, approved, ... }`가 있다
  (`:524-535`). 그러나 `ControlState`에는 member 목록이 없고(`:468-474`), member query,
  public key, committed revision, removed/revoked revision을 함께 주는 complete membership
  view가 없다.

`crates/protocol`:

- 네 action의 canonical fields는 구현돼 있다
  (`crates/protocol/src/to_fields.rs:777-832`).
- coverage test 자체가 membership domain은 `Signable`이 없어 `verify()`를 통과하지
  못한다고 고정한다(`crates/protocol/tests/t1_signing_targets.rs:590-600`).
- 현재 `Verified<M>`는 일반 `Signable`의 단일 signer identity를 보존한다
  (`crates/protocol/src/signing.rs:624-647`). 현재 action에는 그 canonical signer ID가 없고,
  `RevokeDevice`의 threshold signatures는 단일 `signature_bytes()` 계약과 맞지 않는다.

`crates/coordinator`:

- membership store와 ControlStore trait/consumer가 없다
  (`crates/coordinator/src/lib.rs:30-36`의 공개 module 목록).
- `AgentRegistry.owner_member_id`는 존재하지만(`inventory_store.rs:20-31`), 모듈 계약상
  caller가 identity-checked fact를 주는 저장소일 뿐이다(`inventory_store.rs:1-6`). 이것은
  authoritative submitter membership source가 아니다.

### DoD-50/51 패턴 적용 판정

| 요구 패턴 | JobManifest / AttemptReport | 현재 membership action | 판정 |
|---|---|---|---|
| signed protobuf record | 있음 | action은 있으나 complete state record 없음 | 불충분 |
| `Signable` + `Verified<M>` | 있음 | 없음 | 차단 |
| canonical signer identity | message 안에 있음 | Owner ID 없음, revoke는 다중 서명 | 차단 |
| 규범 lifetime | §9에 있음 | §9에 없음 | 차단 |
| durable identity anchor | Job/Attempt/reservation 있음 | committed member/device revision 없음 | 차단 |
| raw load, 재검증 경계 | 구현 가능 | 앞의 타입/권위 결정 뒤 가능 | 후속 |

따라서 오늘 바로 membership store만 만드는 것은 같은 패턴이 아니다. 가장 먼저 필요한
슬라이스와 규모는 다음과 같다.

1. **membership authorization/record protocol foundation — 약 2일**
   - ADR: action log와 signed snapshot/record 중 durable fact 선택
   - team/signer/revision/lifetime 및 add/remove/approve/revoke 순서 정의
   - Owner 단일 서명과 threshold signature의 typed verification 결과 정의
   - proto/canonical/reference vector/coverage와 검증 타입 구현
2. **verified membership durable inbox — 1일**
   - public save API는 위 검증 타입만 수용
   - raw load, body hash, exact replay idempotency, typed corruption/rollback
   - validity/revocation/apply와 authoritative lookup은 여전히 후속
3. **committed apply + revision-consistent directory — 2~3일 이상**
   - 이것부터 `(team_id, device_id) -> active member_id`를 authoritative 결과로 소비 가능

첫 2일 슬라이스는 2번 저장을 열고, 3번은 `JobRequirements.submitter_member_id`, key
directory 동기화, ReplicaAck holder/failure-domain 판정을 함께 연다. 해제 효과는 모든 후보 중
가장 크지만 **오늘 하루 후보는 아니다**.

## 2. 다른 blocker의 첫 슬라이스 비교

| blocker | (a) 규범 허용 | (b) 선행 타입/코드 | 첫 정직한 슬라이스·규모 | (c) 여는 downstream | 판정 |
|---|---|---|---|---|---|
| membership directory | signing §5/§8과 Node 표는 서명·COMMITTED를 요구하지만 §9/member state 표가 없음 | action/canonical만 있고 `Signable`, member view, store 없음 | authorization/record protocol foundation **2일**, verified store까지 누적 **3일** | device→member, projection, key authority, replica validity, scope | 효과 최대지만 오늘 불가 |
| `JobRequirements` projection | Job 표는 Hard Filter를 전제하지만(`state-machines.md:108-115`) Manifest→domain mapping 표는 없음 | `JobRequirements`는 존재(`scheduler/src/model.rs:116-133`), Manifest body는 DoD-50으로 저장됨 | 규범 mapping table + strict converter **1~1.5일**; membership 입력은 capability로 남음 | projection unit 경계만; production admission은 안 열림 | 독립 효용이 작고 규범 보강 필요 |
| `MIRRORED` / checkpoint | Checkpoint 표가 signed ACK 수신과 DURABLE 갱신을 명시(`state-machines.md:215-223`); signing §9.1이 Manifest/ACK를 Evidence로 규정(`signing.md:595-625`) | `CheckpointManifest`/`ReplicaAck` proto·`Signable`·framed `Verified` 모두 있음 | **verified CheckpointManifest durable binding 1일** | ACK durable binding, root 대조, durability, artifact/terminal chain | **오늘 선택** |
| Job/Attempt terminal | Job `RUNNING -> COMPLETED`와 Attempt `RUNNING -> COMPLETED` guard가 명시됨(`state-machines.md:127`, `:170-186`) | Job은 STAGING/FAILED까지만(`job_store.rs:23-50`), Attempt는 CREATED만(`staging_store.rs:40-55`); terminal report 저장은 DoD-51 완료 | durable Grant publish + verified GrantAck binding 후 STARTING/RUNNING 전이 **2일 이상** | 정직한 terminal transition prerequisites | outbox norm/type gap 때문에 오늘 불가 |
| reservation release | Attempt 표는 process tree/VRAM 반환을 effect로 요구(`state-machines.md:173-175`), Lease 표는 revoke/supersede 시 자원 반납(`:271-278`) | reservation과 terminal report는 있으나 runtime-stop fact/message 없음 | signed runtime-stop/safe-fence evidence contract+record **2~3일** | atomic Lease/reservation release | 오늘 불가 |
| Grant/Lease scope | signing §13.2는 verified input 경계를 요구하지만 docs/protocol에 GPU provenance 표가 없음 | `ResourceScope.gpu_uuids`(`proto/common.proto:205-213`), selected IDs와 reservation은 있으나 wire UUID 증명 아님(`scheduler/src/model.rs:260-267`) | signed NVML inventory/provenance contract+durable record **2~3일** | Grant plan, Lease scope, Agent enforcement | 오늘 불가 |
| production ingress/session/outbox | signing §8/§10은 verify/replay 순서를 정하지만 durable publish/outbox 상태 표는 없음 | framed ingress와 Resume Hello는 있으나 general submit/session router/outbox 없음 | publish-state/outbox 규범 + durable signed Grant envelope **2~3일** | DB-send crash recovery, session-safe dispatch | 오늘 불가 |

### 왜 raw `ReplicaAck` inbox를 오늘 선택하지 않는가

`ReplicaAck` 자체는 즉시 `Verified`로 받을 수 있다
(`crates/protocol/src/signable.rs:384-409`,
`crates/crypto/src/framed_ingress.rs:183,298`). 그러나 현재 coordinator에는
`checkpoint_id/root_digest`의 durable authority가 없다. ACK만 먼저 저장하면 서명된
holder claim은 보존하지만 어느 producer checkpoint에 대한 것인지 안전하게 묶을 수 없다.
failure domain, ephemeral 여부, replica 생존성도 해석하지 못한다.

따라서 순서는 다음이어야 한다.

```text
Verified<CheckpointManifest> + durable Attempt/reservation binding
  -> Verified<ReplicaAck> + checkpoint_id/root_digest binding
  -> authoritative membership/failure-domain/ephemeral 판정
  -> effective replica count + MIRRORED/REPLICATED transition
```

## 3. 오늘 착수할 최소 조각

### 이름

**verified signed CheckpointManifest durable Attempt/reservation binding**

### 규범 근거

- `docs/protocol/signing.md` §5는 `CheckpointManifest`의 독립 domain을 등록한다(`:256`).
- §9.1은 `CheckpointManifest`를 만료되지 않는 `Lifetime::Evidence`로 명시하고, 관측 시각과
  fencing을 소비 측이 판단하도록 한다(`:595-630`).
- §13.2는 검증된 메시지만 상위 코드에 전달하는 `Verified<M>` 경계를 요구한다
  (`:849-864`).
- `docs/protocol/state-machines.md` §4는 local hash 검증과 signed ReplicaAck 이후의
  durability 전이를 분리한다(`:215-223`). 따라서 이번 저장은 state를 앞당기지 않고
  evidence만 보존해야 한다.

### 현재 존재하는 선행 타입

- signed protobuf `CheckpointManifest`: identity, root digest, created time, producer,
  fence, signature가 모두 있다(`proto/artifact.proto:46-69`).
- `Signable<Lifetime::Evidence>`와 canonical signer `producer_node_id`가 있다
  (`crates/protocol/src/signable.rs:349-374`).
- framed ingress가 `Verified<pb::CheckpointManifest>`를 반환한다
  (`crates/crypto/src/framed_ingress.rs:181,296`).
- coordinator durable Attempt와 reservation에 job/attempt/node/fence identity가 있다
  (`crates/coordinator/src/staging_store.rs:40-74`).
- 반대로 coordinator module 목록에는 checkpoint evidence store가 없다
  (`crates/coordinator/src/lib.rs:30-36`).

### In

1. `crates/coordinator/src/checkpoint_manifest_store.rs`에 public 저장 API를 추가한다.
   raw `pb::CheckpointManifest`가 아니라 오직
   `&Verified<pb::CheckpointManifest>`만 받는다.
2. `Verified::get()` 뒤에만 manifest 필드를 읽는다. 구조상 필수인 schema/checkpoint/job/
   attempt/producer/root/fence/signature가 비었거나 unspecified/undecodable이면 fail closed한다.
   파일 내용 해시가 실제로 맞는지, resume completeness가 참인지 해석하지 않는다.
3. 한 `BEGIN IMMEDIATE` transaction에서 현재 durable single-node Attempt와 reservation에
   다음을 대조한다.
   - manifest `job_id == StoredAttempt.job_id == reservation.job_id`
   - manifest `attempt_id == StoredAttempt.attempt_id == reservation.attempt_id`
   - manifest `producer_node_id == Verified::signer_id() == Attempt.node_id == reservation.node_id`
   - manifest `fence_epoch == StoredAttempt.fence_epoch`
4. signature를 포함한 complete protobuf semantic body, 저장소가 직접 계산한 BLAKE3 body
   hash, verified signer, checkpoint/job/attempt/producer/fence, root digest를 보존한다.
5. `checkpoint_id`의 exact semantic replay는 최초 binding을 그대로 반환하고 행을 늘리지
   않는다. 같은 ID의 body/signature/root/fence 변경은 overwrite하지 않고 typed conflict다.
6. reopen/load는 `Verified<CheckpointManifest>`를 만들지 않는다. raw
   `StoredCheckpointManifestBinding`을 반환하며 body/hash/row identity/signer/fence/root와
   현재 durable Attempt binding 손상은 typed corruption으로 fail closed한다.
7. INSERT 직후 fault injection으로 전체 rollback을 검증한다.
8. 저장 전후 Job/Attempt/Lease/reservation 값과 checkpoint durability state는 바뀌지 않는다.

### Out

- 실제 checkpoint writer의 protobuf/signature producer 연결
- checkpoint file BLAKE3/Merkle 검증과 `HASH_VERIFIED` 전이
- production `FrameType::Checkpoint` session routing
- `ReplicaAck` durable 저장, checkpoint/root binding, replica 생존성 확인
- holder membership, failure domain, ephemeral/submission-worker 중복 판정
- `MIRRORED`/`REPLICATED` effective count와 state transition
- artifact `COMMITTED`, canonical selection, Attempt/Job terminal transition
- runtime-stop proof, Lease revoke, reservation release
- Raft/ControlStore `COMMITTED`

### 완료 조건

1. raw protobuf로 production save API를 호출할 수 없고 `Verified<CheckpointManifest>`만
   저장할 수 있다.
2. job/attempt/producer/signer/fence/reservation owner 중 하나라도 다르면 행을 만들지 않는다.
3. exact replay는 1행과 최초 body를 유지하며 changed replay는 typed conflict다.
4. reopen 뒤 complete signed manifest와 binding을 복원하지만 `Verified`를 재구성하지 않는다.
5. empty/truncated/undecodable body, body hash, row↔body identity, signer, fence, root digest
   손상을 typed corruption으로 거부한다.
6. injected failure 뒤 partial row가 없고 durable Attempt/reservation은 불변이다.
7. storage success가 `HASH_VERIFIED`, `REPLICATING`, `REPLICATED`, `COMMITTED` 중 어느 상태도
   주장하지 않는다는 invariant를 테스트한다.
8. signer/producer 또는 fence 대조를 제거하는 두 mutation에서 지정 negative test가
   실패한다.
9. 기존 coordinator Job/Manifest/inventory/staging/Lease/AttemptReport 테스트가 회귀하지
   않는다.

### 예상 변경 소유권

- `crates/coordinator/src/checkpoint_manifest_store.rs`
- `crates/coordinator/src/lib.rs` module export
- coordinator unit/integration tests
- 구현 완료 뒤 evidence/history

`proto/`, `crates/protocol`, `crates/checkpoint`의 기존 custom JSON writer, production `run()`은
이 조각에서 바꾸지 않는다.

## 4. downstream 해제 효과 순위

| 순위 | 첫 슬라이스 | 직접 여는 다음 단계 | 최종 영향 | 오늘 가능 |
|---:|---|---|---|:---:|
| 1 | membership authorization/record foundation | verified membership store | projection, replica validity, scope, key authority | ❌ 2일 |
| 2 | verified CheckpointManifest durable binding | verified ReplicaAck binding | MIRRORED, artifact guard, terminal/release chain | ✅ 1일 |
| 3 | projection mapping + strict converter | membership-backed submit ingress | hard filter/admission | △ 1~1.5일, 규범 보강 필요 |
| 4 | durable Grant publish + GrantAck binding | STARTING/RUNNING | terminal transition | ❌ 2일+ |
| 5 | runtime-stop evidence contract | atomic release | capacity 회수 | ❌ 2~3일 |
| 6 | NVML provenance contract | Grant/Lease scope | runtime authorization | ❌ 2~3일 |

membership의 최종 해제 효과가 가장 크다는 판단은 바뀌지 않는다. 다만 그 첫 조각부터
규범·schema·검증 타입을 함께 정해야 해 하루를 넘는다. 오늘 가능한 조각 중에는
`CheckpointManifest` durable binding이 가장 아래에 있고, 다음 저장 조각이 참조할 durable
anchor를 만들며, durability에서 terminal/release까지 가장 긴 실제 경로를 연다.

## 다음 순서

```text
오늘: Verified<CheckpointManifest> durable Attempt/reservation binding
  -> Verified<ReplicaAck> durable checkpoint/root binding
  -> membership/failure-domain/ephemeral validity consumer
  -> MIRRORED/REPLICATED state transition
  -> final artifact guard + Attempt/Job terminal transaction
  -> runtime-stop/safe-fence proof + atomic reservation release

병행 상위 경로:
membership authorization/record protocol foundation (약 2일)
  -> verified membership durable inbox (1일)
  -> committed revision-consistent directory
  -> strict JobRequirements projection + submit admission
  -> Grant/Lease scope + production outbox/routing
```

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-24 | 최초 작성 |

## 구현 결과 (2026-08-24)

### 구현

- `crates/coordinator/src/checkpoint_manifest_store.rs`를 추가하고
  `CoordinatorCheckpointManifestStore`를 공개했다. production 저장 API
  `store_verified_manifest`는 `&Verified<pb::CheckpointManifest>`만 받는다.
- `coordinator_checkpoint_manifests` 테이블을 추가했다. primary key인
  `checkpoint_id`와 job/attempt/producer/verified signer, big-endian BLOB
  `fence_epoch`, protobuf `root_digest`, 저장소가 complete signed protobuf body에서 직접
  계산한 32-byte BLAKE3 `manifest_hash`, `manifest_body`를 보존한다.
- 저장 경로는 `Verified::get()` 뒤에만 manifest를 관찰한다. schema와 필수 identity,
  producer signature, BLAKE3-256 32-byte root digest의 구조 검사를 통과한 뒤
  `BEGIN IMMEDIATE`를 획득한다. 같은 transaction 안에서 Attempt와 reservation을 읽고
  job/attempt/producer/verified signer/fence를 모두 대조한 뒤 INSERT한다.
- exact semantic replay는 최초 durable binding을 `created=false`로 반환한다. 같은
  `checkpoint_id`의 body 또는 signature 변경은 `ManifestConflict`로 거부한다.
- load는 의도적으로 `StoredCheckpointManifestBinding`을 반환하며 `Verified`를 만들지 않는다.
  body hash를 재계산한 뒤 decode하고 row identity/signer/fence/root 및 현재 Attempt binding을
  재검사한다. 손상은 `CheckpointManifestCorruption`으로 fail closed한다.
- 이 저장은 checkpoint durability state를 저장하거나 바꾸지 않으며 Job, Attempt, Lease,
  reservation도 변경하지 않는다.

### negative 및 fault 테스트

- 빈 checkpoint identity, 누락/unspecified/SHA-256/잘못된 길이 root digest를 거부한다.
- 잘못된 job/attempt/producer/signer, stale fence, 누락되거나 다른 reservation owner를
  모두 무행 생성으로 거부한다.
- empty/truncated/undecodable body, hash encoding/hash mismatch, body↔row checkpoint/job/
  attempt/producer identity, signer, fence encoding/value, root encoding/value 및 현재 Attempt
  fence 손상을 typed corruption으로 거부한다.
- INSERT 직후 fault injection에서 manifest 행 전체가 rollback되고 Attempt/reservation이
  그대로 남는 것을 확인한다.
- exact replay, changed body/signature conflict, reopen 복원, complete signature 보존,
  storage-derived hash 및 무상태전이를 확인한다.

### 뮤테이션 확인

1. `bind_producer_and_signer`의 producer/signer/Attempt/reservation owner 대조를 제거하자
   `producer_signer_mismatch_with_durable_owner_creates_no_row`가 실패하며 잘못된 행이
   생성됐다. 가드 원복 뒤 재통과했다.
2. Attempt fence 대조를 제거하자 `wrong_job_attempt_and_stale_fence_create_no_row`가
   실패하며 `fence_epoch=0` 행이 생성됐다. 가드 원복 뒤 재통과했다.

### 자체 재검토와 수정

- 첫 구현은 structurally valid한 SHA-256 root digest도 허용했다. `common.proto`가 SHA-256을
  OCI 호환 목적으로만 허용하고 Checkpoint 상태 표는 BLAKE3 대조를 요구하므로,
  Checkpoint root를 BLAKE3-256/32-byte로 제한하고 SHA-256 negative case를 추가했다.
- load가 authoritative key directory 없이 `Verified`를 재구성하지 않는지, 모든 durable
  owner read가 INSERT와 같은 `BEGIN IMMEDIATE` 안에 있는지, replay가 overwrite하지 않는지,
  새 테이블에 durability state 열이 없는지를 다시 확인했다.

### 검증

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS
- 집중 테스트 `cargo test -p gputeer-coordinator checkpoint_manifest_store --no-fail-fast`:
  10 passed
- 환경 참고: Rust toolchain에 `rustfmt` component가 설치되어 있지 않아 `cargo fmt`는
  실행할 수 없었다. build와 전체 test에는 영향이 없었다.
