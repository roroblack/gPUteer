# 2026-08-24_1556 verified ReplicaAck durable checkpoint/root binding v1

- **조사 질문:** `MIRRORED` 전체가 아니라 그 선행 조건의 첫 저장 슬라이스가 1일인가
- **오늘 착수 결론:** **검증된 signed `ReplicaAck`의 durable checkpoint/root binding**
- **예상 규모:** coordinator production Rust 약 450~600줄, 재시작·손상·replay·rollback
  테스트 약 600~850줄, 문서/evidence 별도 — 기존 DoD-51/52 패턴을 재사용하는 **1일**
- **proto/protocol 변경:** 없음
- **production producer/routing:** 없음

## 결론

이 조각은 오늘 하루 규모로 성립한다.

직전 DoD-52 전에는 `ReplicaAck` 자체를 `Verified`로 받을 수 있어도, 그 ACK의
`checkpoint_id`와 `root_digest`를 대조할 coordinator durable authority가 없었다. 이제
`coordinator_checkpoint_manifests`가 complete signed `CheckpointManifest`, 검증 당시 signer,
Attempt/reservation identity, fence와 BLAKE3-256 root를 보존한다
(`crates/coordinator/src/checkpoint_manifest_store.rs:147-157,176-260`). 따라서 ACK를 해석하거나
checkpoint 상태를 바꾸기 전에 다음 사실만 원자적으로 기록할 수 있다.

```text
Verified<ReplicaAck>
  + current durable CheckpointManifest checkpoint_id/root_digest anchor
  -> immutable raw signed ACK observation
```

holder membership, failure-domain 중복 제거, ephemeral 여부, submitter/worker 동일-device,
복제본의 현재 생존성, effective replica count와 `MIRRORED`/`REPLICATED` 전이는 전부 후속이다.
이번 성공은 “유효 replica가 생겼다”가 아니라 “어느 시각에 어느 키가 서명한 ACK claim을
어느 durable checkpoint/root에 묶어 잃지 않게 기록했다”만 뜻한다.

## 1. 규범이 이 저장 슬라이스를 허용하는가

### 서명·검증 규범

- `docs/protocol/signing.md` §5는 `ReplicaAck` 전용 domain
  `gputeer/v1/replica-ack`를 등록한다(`:245-257`).
- §8은 서명 검증 전 필드 사용을 금지하고, 검증 뒤에만 필드를 신뢰하도록 순서를 고정한다
  (`:514-527`).
- §9.1은 `CheckpointManifest`와 `ReplicaAck`를 만료되지 않는
  `Lifetime::Evidence`로 함께 분류하고, 관측 시각을 반드시 노출하도록 한다(`:595-625`).
- 같은 절은 `ReplicaAck`에 fence가 없고 ACK가 과거 시점의 사실일 뿐임을 명시한다.
  소비자는 이를 “지금 durable”로 읽어서는 안 된다(`:632-645`).
- §13.2는 검증되지 않은 protobuf가 상위 로직으로 흐르지 않도록 `Verified<M>` 타입 게이트를
  요구한다(`:849-864`).

### checkpoint 상태 규범

`docs/protocol/state-machines.md` §4는 다음 두 단계를 분리한다.

- signed `ReplicaAck` 수신과 유효 replica 수 갱신은
  `REPLICATING -> REPLICATED`의 DURABLE 전이다(`:206-220`).
- 요구 replica 수 충족 뒤에야 `REPLICATED -> COMMITTED`가 허용된다(`:223`).

따라서 검증된 ACK body를 durable inbox에 먼저 보존하는 것은 다음 DURABLE 전이의 선행
기록으로 허용된다. 반대로 이번 저장 성공만으로 유효 replica 수를 올리거나 상태를 전이하면
membership/failure-domain/ephemeral 규칙을 건너뛰므로 규범 위반이다.

### durability 판정 규칙과 이번 경계

`proto/artifact.proto`는 유효한 holder 서명만 계산하고, 같은 failure domain을 하나로 세며,
ephemeral local copy와 submitter/worker 동일-device 중복을 제외하라고 정한다(`:135-144`).
이 네 규칙은 **판정 소비자**의 책임이다. 저장소는 `kind`, `failure_domain`, `fsynced`,
`hash_verified`, `stored_bytes`, `acked_at`을 signature 포함 complete body로 보존하되, 그 값이
replica count에 들어가는지는 결정하지 않는다.

판정: 규범은 signed ACK와 DURABLE 전이를 명시하고 둘 사이의 해석 guard도 명시한다. 그래서
“검증된 claim의 durable binding만 먼저 저장하고 상태는 불변”인 슬라이스가 정확히 성립한다.

## 2. 선행 타입과 검증 경로가 실제로 있는가

### protobuf와 canonical body

- `proto/artifact.proto:101-123`에 `ReplicaAck`가 실제 정의돼 있다.
  `checkpoint_id`, `root_digest`, `holder_device_id`, `kind`, `failure_domain`, fsync/hash flags,
  `stored_bytes`, `acked_at_unix_ms`, `holder_signature`를 갖는다.
- `crates/protocol/src/lib.rs:16-21`이 proto 생성 타입을 `pb`로 포함한다.
- `crates/protocol/src/to_fields.rs:376-399`가 위 필드를 canonical fields로 옮긴다.
  특히 `failure_domain`, fsync/hash flags와 관측 시각도 서명 입력에 포함된다.

### `Verified<ReplicaAck>` 생성 가능성

- `crates/protocol/src/signable.rs:384-409`에
  `impl Signable for pb::ReplicaAck`가 있다. domain은 `Domain::ReplicaAck`, lifetime은
  `Lifetime::Evidence`, signature는 `holder_signature`, signer identity는
  `holder_device_id`, observation time은 `acked_at_unix_ms`다.
- 공용 `verify()`는 schema/canonical/signature/key identity를 검사한 뒤
  `Verified<M>`를 반환한다(`crates/protocol/src/signing.rs:719-750`).
- raw bytes ingress도 제네릭 `decode_and_verify<M>() -> Verified<M>`를 제공한다
  (`crates/crypto/src/ingress.rs:164-200`).
- framed ingress에는 `FrameType::ReplicaAck = 9`,
  `IngressMessage::ReplicaAck(Verified<pb::ReplicaAck>)`, 실제 dispatch가 모두 연결돼 있다
  (`crates/crypto/src/framed_ingress.rs:84-99,174-188,289-303`).

판정: proto 추가, schema bump, canonical/vector 변경, 신규 `Signable` 작업 없이 production
저장 API가 곧바로 `&Verified<pb::ReplicaAck>`만 받을 수 있다.

## 3. DoD-50/51/52와 같은 패턴으로 저장 가능한가

### 이미 고정된 패턴

| 조각 | verified-only 입력 | 같은 transaction 대조 | load 결과 |
|---|---|---|---|
| DoD-50 JobManifest | `submit_verified_manifest(&Verified<JobManifest>)` (`job_store.rs:528-543`) | accepted Job/idempotency/body (`:559-635`) | raw `StoredManifestBinding`, 재검증 필요 (`:149-158,398-408`) |
| DoD-51 AttemptReport | `store_verified_terminal_report(&Verified<AttemptReport>)` (`attempt_report_store.rs:182-188`) | Attempt/reservation/signer/fence (`:204-244`) | raw binding, terminal 판단 전 재검증 (`:171-179`) |
| DoD-52 CheckpointManifest | `store_verified_manifest(&Verified<CheckpointManifest>)` (`checkpoint_manifest_store.rs:176-182`) | Attempt/reservation/signer/fence/root (`:203-260`) | raw binding, durability 판단 전 재검증 (`:20-34,168-174`) |

`ReplicaAck`도 같은 구조를 쓸 수 있다. 차이는 ACK가 한 번뿐인 manifest가 아니라
`acked_at` 시점의 관측 증거라는 점이다. 메시지에 `ack_id`와 fence가 없으므로 저장소가 새
ID를 발명하거나 `(checkpoint, holder)` 하나로 최신 관측을 overwrite하지 않는다. durable
observation identity는 메시지 안의 서명된 필드만으로 다음과 같이 잡는다.

```text
(checkpoint_id, holder_device_id, acked_at_unix_ms)
```

같은 observation key와 complete semantic body/signer의 replay는 최초 행을 반환한다. 같은 key의
body/signature/root 변경은 typed conflict다. 더 늦은 `acked_at`의 재관측은 별도 immutable
행으로 저장한다. 이것은 ACK 신선도를 판정하지 않고 과거 관측을 보존한다는 §9.1 의미와
맞으며, 후속 소비자가 어떤 관측을 유효하게 볼지 결정할 여지를 남긴다.

### 구현할 production 경계

신규 `CoordinatorReplicaAckStore`와 `coordinator_replica_acks`를 만든다.

1. public save API의 ACK 인자는 오직 `&Verified<pb::ReplicaAck>`다. ACK 필드는
   `Verified::get()` 뒤에 처음 읽는다.
2. 구조 입력은 현재 schema, 비어 있지 않은 checkpoint/holder, 존재하고 올바르게 인코딩된
   root, 0이 아닌 `acked_at_unix_ms`까지만 검사한다. `kind`, `failure_domain`, fsync/hash flags,
   bytes의 **replica eligibility**는 해석하지 않는다.
3. 하나의 `BEGIN IMMEDIATE` transaction에서 DoD-52 anchor를 raw SQL로 흉내 내지 않고
   crate-private schema/fetch helper로 읽어 anchor body/hash/Attempt binding까지 먼저 검증한다.
4. ACK `checkpoint_id`가 anchor ID와 같고 ACK `root_digest`가 anchor의
   `bound_root_digest`와 semantic exact match인지 대조한다.
5. `holder_device_id == Verified::signer_id()`를 대조하고 둘 다 별도 column에 보존한다.
6. signature를 포함한 complete protobuf body, 저장소 계산 BLAKE3 body hash,
   checkpoint/holder/acked-at/root/signer binding을 저장한다.
7. exact replay/changed replay는 위 observation key로 처리하고, insert 뒤 fault injection에서
   전체 rollback을 검증한다.
8. reopen/load는 `Verified<ReplicaAck>`를 만들지 않는다. raw
   `StoredReplicaAckBinding`을 반환하고 body hash, decode, row↔body identity/root/signer/time,
   현재 checkpoint anchor/root 손상을 fail closed한다.
9. checkpoint별 raw observation 목록은 결정적 `(acked_at, holder_device_id)` 순서로 읽는다.
   이 API는 count, dedup, membership, freshness 결론을 반환하지 않는다.

예상 schema:

```sql
CREATE TABLE coordinator_replica_acks (
    checkpoint_id TEXT NOT NULL
        REFERENCES coordinator_checkpoint_manifests(checkpoint_id),
    holder_device_id TEXT NOT NULL,
    acked_at_unix_ms BLOB NOT NULL,
    verified_signer_id TEXT NOT NULL,
    root_digest BLOB NOT NULL,
    ack_hash BLOB NOT NULL CHECK(length(ack_hash) = 32),
    ack_body BLOB NOT NULL,
    PRIMARY KEY(checkpoint_id, holder_device_id, acked_at_unix_ms)
);
```

DoD-52 schema 초기화와 validated anchor fetch를 공유할 수 있도록
`checkpoint_manifest_store.rs`의 해당 helper만 `pub(crate)`로 좁게 추출한다. 외부 public API나
proto 계약은 바꾸지 않는다.

## 4. 저장과 판정의 명시적 분리

### In — 이번 1일

- verified-only API와 verified signer/body 보존
- durable checkpoint 존재 및 exact root binding
- observation-key exact replay/changed replay/later observation
- raw reopen/list load와 corruption fail-closed
- 한 transaction의 anchor 대조 + insert + rollback
- checkpoint/Job/Attempt/Lease/reservation 및 durability state 불변 검증

### Out — 후속

- holder가 현재 active member/approved device인지 판정
- stored signer key가 현재 authoritative directory에서 여전히 유효한지 재검증
- `failure_domain`의 authoritative 값 대조와 domain 중복 제거
- ephemeral local replica 제외와 submitter/worker 동일-device 중복 제외
- `fsynced`, `hash_verified`, `kind`, `stored_bytes`의 eligibility 해석
- ACK freshness, replica liveness/challenge-response, missing `fence_epoch` 보완
- effective replica count와 `MIRRORED`/`REPLICATED`/`COMMITTED` 전이
- production `FrameType::ReplicaAck` session routing과 실제 replica producer
- artifact/canonical/terminal/release chain, Raft/ControlStore `COMMITTED`

이 분리 때문에 authoritative membership이 아직 없어도 저장 자체는 정직하다. 저장된 row는
“유효 replica”가 아니라 “검증 당시 key directory가 holder key로 검증한 signed observation”이며,
load 뒤에는 현재 authoritative key directory로 다시 검증해야 한다.

## 5. 완료 조건과 negative tests

1. raw `pb::ReplicaAck`로 production save API를 호출할 수 없고
   `Verified<pb::ReplicaAck>`만 저장할 수 있다.
2. 존재하지 않는 checkpoint 또는 다른 root의 ACK는 행을 만들지 않는다.
3. anchor fetch와 root 대조가 insert와 같은 `BEGIN IMMEDIATE` transaction 안에 있다.
4. exact observation replay는 최초 1행/body를 유지하고, 같은 key의 changed body/signature는
   typed conflict, 더 늦은 signed observation은 별도 행이다.
5. reopen 뒤 signature 포함 complete ACK와 binding을 복원하지만 `Verified`를 재구성하지 않는다.
6. empty/truncated/undecodable body, hash, row↔body checkpoint/holder/time/root/signer 및
   checkpoint anchor/root 손상을 typed corruption으로 거부한다.
7. insert 직후 injected failure는 ACK row를 남기지 않고 manifest/Job/Attempt/Lease/reservation을
   바꾸지 않는다.
8. ACK 저장 전후 checkpoint durability 상태를 새로 만들거나 바꾸지 않으며,
   `MIRRORED`, `REPLICATED`, `COMMITTED`, effective count를 반환하지 않는다.
9. `fsynced=false`, `hash_verified=false` 같은 signed body도 저장 단계에서는 claim으로 보존하되
   유효 replica로 해석하지 않음을 고정한다.
10. anchor root comparison을 제거하는 mutation과 load를 `Verified`로 승격시키는 우회가 지정
    negative test/type audit에서 실패한다.
11. 기존 coordinator 118 tests와 protocol/crypto의 `ReplicaAck` lifetime·signature 경로가
    회귀하지 않는다.

권장 검증 명령:

```powershell
cargo test -p gputeer-coordinator replica_ack_store --no-fail-fast
cargo test -p gputeer-coordinator --no-fail-fast
cargo test -p gputeer-protocol --test lifetime_consistency --no-fail-fast
cargo test -p gputeer-crypto replica_ack_stays_valid_forever_even_if_replica_is_gone --no-fail-fast
python scripts/verify_evidence.py
```

## 6. 이 조각이 여는 다음 단계

```text
DoD-52 durable CheckpointManifest anchor
  -> 이번 조각: durable Verified<ReplicaAck> observations
  -> authoritative membership/failure-domain/ephemeral consumer
  -> effective replica count + MIRRORED/REPLICATED transition
  -> final artifact COMMITTED guard
  -> Attempt/Job terminal transaction
  -> runtime-stop/safe-fence proof + reservation release
```

직접 여는 것은 membership-backed ACK consumer다. 그 consumer는 더 이상 transient frame을
즉석에서 세지 않고, 재시작 뒤에도 checkpoint/root에 묶인 raw signed observations를 다시
검증해 판정할 수 있다. `MIRRORED` 자체는 이번 조각의 완료 주장이 아니다.

## 7. 예상 변경 소유권

- 신규 `crates/coordinator/src/replica_ack_store.rs`
- `crates/coordinator/src/checkpoint_manifest_store.rs`의 crate-private schema/anchor helper 추출
- `crates/coordinator/src/lib.rs` module export
- coordinator unit/integration tests
- 구현 완료 뒤 evidence/history

`proto/`, `crates/protocol`, `crates/crypto`, checkpoint writer, production `run()`과
`docs/vision/TODO_VISION.md`는 이 조각에서 바꾸지 않는다.

## 8. 구현 결과 (2026-08-24 16:10 KST)

계획한 저장 슬라이스만 구현했다. `MIRRORED`/`REPLICATED`/`COMMITTED` 전이,
effective replica count, membership/failure-domain/ephemeral 판정, production frame routing은
추가하지 않았다.

### production

- `crates/coordinator/src/replica_ack_store.rs`
  - 공개 저장 경계 `CoordinatorReplicaAckStore::store_verified_ack()`는
    `&Verified<pb::ReplicaAck>`만 받는다. raw protobuf 호출은 compile-fail doctest로 고정했다.
  - `Verified::get()` 뒤에만 ACK body를 읽고, signature를 포함한 complete protobuf body에서
    저장소가 BLAKE3-256 `ack_hash`를 직접 계산한다.
  - `BEGIN IMMEDIATE`를 먼저 얻은 뒤 구조 검증, verified signer↔holder 대조,
    DoD-52 manifest anchor 조회/손상 검증, exact BLAKE3-256 root 대조, replay 대조와 INSERT를
    모두 같은 transaction에서 수행한다.
  - `(checkpoint_id, holder_device_id, acked_at_unix_ms)`를 immutable observation key로 쓰며
    시각은 u64 big-endian BLOB으로 저장한다. exact semantic replay만 최초 행을 반환하고,
    같은 key의 body/signature 변경은 typed conflict, 다른 시각은 별도 행이다.
  - load/list는 의도적으로 raw `StoredReplicaAckBinding`을 반환한다. body/hash/decode,
    row↔body identity/signer/time/root, 현재 DoD-52 anchor/root를 fail-closed로 재검사한다.
  - 신규 `coordinator_replica_acks` 테이블은 complete body, store-derived hash, verified signer,
    root와 observation key를 보존한다. state/count/eligibility column은 없다.
- `crates/coordinator/src/checkpoint_manifest_store.rs`
  - 기존 anchor schema와 완전 검증 fetch를 복제하지 않도록 `initialize_schema()`와
    `fetch_manifest_binding()`만 `pub(crate)` helper로 추출했다. 공개 API는 바꾸지 않았다.
- `crates/coordinator/src/lib.rs`
  - `replica_ack_store` 모듈을 공개했다.

### negative tests

- missing checkpoint, different root, SHA-256 root, 31-byte root는 행을 만들지 않는다.
- signature 포함 body/hash reopen, exact replay, changed body/signature conflict, later observation,
  deterministic list order를 검사한다.
- `fsynced=false`, `hash_verified=false`, unspecified kind claim도 저장만 하고 필터링하지 않는다.
- empty/truncated/undecodable body, hash encoding/mismatch, invalid body, checkpoint/holder/signer/time/root
  row binding, missing/corrupt/changed CheckpointManifest anchor를 fail-closed로 검사한다.
- insert 직후 주입 실패가 ACK row를 rollback하고 Job/Attempt/Lease/reservation/manifest row를
  바꾸지 않음을 검사한다. schema에 state/count/mirrored column이 없음을 함께 검사한다.

### mutation tests

1. production의 `root_digest != anchor.bound_root_digest` guard를 실제로 제거했다.
   `missing_anchor_wrong_root_sha256_and_wrong_length_create_no_row`가 다른 root ACK의
   `created=true` 삽입을 관측하며 실패했다. guard 원복 뒤 재통과했다.
2. load의 `blake3_256(&raw.body) != ack_hash` guard를 실제로 제거했다.
   `corrupt_body_hash_identity_signer_time_and_root_fail_closed`가 변조 hash 행을
   `Ok(Some(...))`로 읽으며 실패했다. guard 원복 뒤 재통과했다.

### 검증

```text
cargo test -p gputeer-coordinator replica_ack_store --no-fail-fast
  7 passed / 0 failed
cargo test -p gputeer-coordinator --no-fail-fast
  121 unit + 4 integration + 1 compile-fail doctest passed / 0 failed
cargo build --workspace --exclude gputeer-runtime-windows
  success
cargo test --workspace --exclude gputeer-runtime-windows
  success / 0 failed (1 existing ignored test)
python scripts/verify_evidence.py
  schema violations 0
python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  48 vectors matched
```

실행 환경의 Rust toolchain에는 `rustfmt` component가 설치돼 있지 않아
`cargo fmt --package gputeer-coordinator`는 실행하지 못했다. `cargo check`, build와 전체 test는
통과했고 `git diff --check`도 whitespace error 없이 통과했다. 사용자 지시 때문에
`docs/evidence/`, `CLAUDE.md`, `docs/history/HISTORY.md`는 수정하지 않았다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-24 | 최초 작성 |
| 2026-08-24 | verified ReplicaAck durable binding 구현·negative/mutation/workspace 검증 결과 추가 |
