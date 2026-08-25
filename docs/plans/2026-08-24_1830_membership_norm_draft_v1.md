# gPUteer membership 규범 초안 v1

- **작성 시각:** 2026-08-24 18:30 (KST)
- **성격:** 적대적 검토를 반영한 승인 전 규범 초안
- **선행 문서:**
  - `docs/plans/2026-08-24_1730_membership_norm_draft_v0.md`
  - `docs/plans/2026-08-24_1700_membership_norm_skeleton_v1.md`
- **중요한 경계:** 이 문서는 `crates/`, `proto/`, `docs/protocol/`의 기존 파일을
  수정하지 않는다. 아래 wire/action/state 표는 사용자가 고른 뒤 별도 schema·규범
  변경으로 반영해야 한다.

## 0. 이번 v1의 판정

v0는 승인용 규범으로 부적합했다. 특히 정상적으로 commit된 장수명 action을 재시작
때문에 만료된 것으로 되돌리는 규칙, 설치 전 anchor 교체를 탐지할 수 없는 bootstrap,
그리고 SQLite의 `DURABLE`을 membership 권위로 오인할 수 있는 표현이 치명적이었다.

v1의 기준 권위 경로는 다음과 같다.

```text
독립적으로 고정된 bootstrap pin
  -> 검증된 Genesis/anchor
  -> quorum이 서명한 COMMITTED action log
  -> revision/index가 있는 deterministic Member/Device projection
  -> linearizable, revision-pinned authorization view
```

여기서 반드시 구분한다.

| 개념 | 의미 | 권위로 사용할 수 있는가 |
|---|---|---|
| action signature | Owner/member/coordinator가 명령의 의미를 승인했다는 증거 | 단독으로 현재 권위가 아님 |
| `COMMITTED` | 과반 합의가 action log 위치와 내용을 확정했다는 증거 | membership authorization의 필수 조건 |
| `DURABLE` | 한 로컬 저장소가 fsync했다는 증거 | membership authorization 불가 |
| `Evidence` | 특정 시점에 관측·검증된 역사적 사실 | 현재 권한으로 승격 불가 |
| projection | 검증된 log에서 재계산한 상태 | commit proof와 revision이 있을 때만 권위 |

`COMMITTED`는 기존 규범대로 과반 합의이며 SingleNodeStore에서는 제공할 수 없다. 이는
이 초안의 사용자 선택 항목이 아니다.

## 1. 기존 규범에서 이미 고정된 사실

다음은 이 초안에서 다시 선택하지 않는다.

1. canonical protobuf, domain별 서명 입력, schema version, signature field 90,
   Ed25519 검증과 fail-closed 검증 순서는 기존 signing 규범을 따른다.
2. `COMMITTED`는 과반 합의이고 SingleNodeStore에서는 불가능하다.
3. Evidence는 현재 권한이 아니라 과거 관측이다.
4. `AddMember`의 정본 domain tag는 **`gputeer/v1/member-add`**다. 이는
   `docs/protocol/signing.md`의 규범 표, `crates/protocol/src/canonical.rs`,
   cross-domain 검증과 일치한다. `proto/control.proto` 주석과
   `tests/vectors/canonical_v1.json` 설명은 stale한 기존 파일이며 이 초안에서는
   고치지 않는다.

`membership` tag로 외부에서 이미 만들어진 서명을 받아야 하는지, 받는다면 legacy
version/migration을 둘지는 사용자 결정 항목이다(§12).

## 2. 치명적 결함 1: commit된 LongLived action의 안전한 replay

### 2.1 문제와 원칙

현재 `crates/protocol/src/signing.rs`의 `Lifetime::LongLived` 검사는
`now >= expires_at`이면 거부한다. 이 검사는 **신규 제출**에는 맞지만, commit된
역사 action을 현재 시각으로 다시 제출하는 검사로 사용하면 안 된다. 예를 들어 7일
TTL `AddMember`가 day 1에 정상 commit되고 day 8에 노드가 재시작해도, 그 action은
정상 commit log의 일부이지 새 Add 요청이 아니다.

따라서 lifetime 검증을 두 API와 두 타입으로 분리한다.

```text
verify_new_membership_submission(action, verifier, now, replay_guard)
    -> Verified<NewMembershipAction>

replay_committed_membership_action(log_record, commit_certificate)
    -> RevalidatedCommittedMembershipAction
```

`verify_new_membership_submission`만 현재 시각을 사용한다. `replay_committed...`는
현재 시각을 lifetime 입력으로 사용하지 않는다.

### 2.2 보존해야 하는 구체적 증거

각 membership action log entry는 action bytes만 보존해서는 안 된다. 다음의 불변
`MembershipCommitRecord`를 같은 log entry 또는 검증 가능한 commit-proof record로
보존해야 한다.

```text
MembershipCommitRecord {
  team_id
  log_id
  index
  term
  prev_entry_hash
  action_id
  domain_tag
  schema_version
  action_digest                 // signature field 90을 포함한 complete action bytes의 BLAKE3-256
  signer_id
  signer_key_id
  member_generation             // action이 요구하면 필수, 아니면 명시적 0/none
  issued_at_unix_ms
  expires_at_unix_ms
  lifetime_class = LONG_LIVED
  quorum_config_id
  voter_checks[]
}

VoterCheck {
  coordinator_id
  coordinator_key_id
  checked_at_unix_ms
  signature
}
```

각 `VoterCheck`의 서명 입력은 다음을 모두 포함하는 canonical
`MembershipCommitAttestation`이다.

```text
domain = gputeer/v1/membership-commit
team_id || log_id || quorum_config_id || index || term || prev_entry_hash
|| action_id || action_digest || signer_key_id || issued_at_unix_ms
|| expires_at_unix_ms || checked_at_unix_ms
```

서명자는 action signer가 아니라 해당 시점의 committed coordinator quorum의
coordinator key다. 각 quorum voter는 자신이 검증한 action digest, predecessor
index/hash, signer authorization, lifetime 및 log position을 함께 서명한다. proof는
다음 조건을 모두 만족해야 한다.

- `voter_checks`의 key는 anchor 또는 이전에 commit된 coordinator-set에서 조회된다.
- 서로 다른 voter가 현재 quorum config의 과반을 이룬다. 동일 key의 중복 서명은 한 표다.
- `index`는 연속적이고 `prev_entry_hash`가 predecessor와 일치한다.
- `action_digest`가 complete signed action의 재계산 digest와 일치한다.
- 각 voter의 `checked_at_unix_ms < expires_at_unix_ms`다. `issued_at`은 기존
  `LongLived` 의미처럼 현재시각 skew 검사의 근거로 쓰지 않으며, 필요하면
  `issued_at <= checked_at` 구조 조건만 둔다.
- `MembershipCommitRecord`와 proof 자체가 canonical·signature·schema 검사를
  통과한다.

이 proof는 “실제 시간이 틀림없이 이랬다”는 물리적 증명이 아니다. 분산시계가 정직하고
quorum voter의 시계가 운영 정책의 허용 범위 안이라는 신뢰가 필요하다. 서명은 그
검사를 누가 했는지와 어떤 값에 대해 했는지를 위조 방지로 고정한다. 공격자가 action
bytes와 proof를 함께 바꾸려면 quorum key 과반과 predecessor chain을 동시에 위조해야
하므로 단순 log row 교체로는 통과할 수 없다. quorum key 자체가 장악된 경우는
membership safety가 아니라 root/quorum compromise이며 §7의 anchor 절차로 다룬다.

### 2.3 신규 제출과 역사 replay의 정확한 순서

신규 제출:

```text
1. 정적 message type에서 domain tag 선택
2. schema_version 확인
3. field 90을 제외한 canonical 재구성
4. domain || schema_version || canonical length || canonical 조립
5. action signature 검증
6. signer_id/key_id를 anchor 또는 predecessor trust graph에서 조회
7. 현재 now에 대해 기존 LongLived 검사(now < expires_at)
8. action_id/replay 검사
9. predecessor state guard 검사
10. quorum에 propose
11. quorum voter가 위 lifetime와 log continuity를 다시 확인하고 proof 서명
12. proof가 과반을 만족할 때만 COMMITTED projection에 apply
```

역사 replay:

```text
1. action bytes와 MembershipCommitRecord/proof의 digest 대조
2. proof signature, quorum membership, index/term/prev hash 대조
3. proof에 보존된 각 checked_at이 expires_at 전인지 확인
4. action signer를 현재 keyring이 아니라 그 action 직전 projection에서 조회
5. action_id/generation/transition sequence를 log 순서로 확인
6. deterministic projection에 apply
```

역사 replay 3단계는 `now`를 읽지 않는다. 현재 시각이 day 8이라는 이유로 day 1의
정상 commit을 `EXPIRED`로 만들지 않는다. 반대로 proof가 없거나 lifetime 통과가
보존되지 않은 legacy row는 추측으로 살리지 않고 `MISSING_COMMIT_PROVENANCE`로
directory 전체를 공개하지 않는다.

### 2.4 기존 `Lifetime` 구현과의 공존

기존 `Lifetime::LongLived`와 `verify()`의 의미를 바꾸지 않는다. 그 구현은 신규
제출 API에서 계속 `now >= expires_at`을 거부한다. 추가로 필요한 것은 다음의 새
wire/domain 타입과 검증 함수다.

- `MembershipCommitRecord` / `MembershipCommitAttestation`
- `RevalidatedCommittedMembershipAction` 또는 동등한 역사 전용 wrapper
- `verify_new_membership_submission(...)`
- `replay_committed_membership_action(...)`

기존 일반 `Verified<M>`를 임의로 역사 권위로 재사용하지 않는다. durable load는
기존 DoD-50~53처럼 raw binding으로 복원하고, proof와 predecessor state를 다시
검증한 뒤에만 `RevalidatedCommittedMembershipAction`을 만든다. 그 타입도 현재
authorization을 뜻하지 않으며, 별도 `CommittedMembershipView`로 projection한
경우에만 권한 resolver가 사용할 수 있다.

이 설계는 기존 `LongLived` 코드를 수정하지 않아도 개념적으로 공존한다. 실제
구현에서는 새 타입·API·wire field·commit proof 저장소를 추가해야 하며, 기존
`verify()`를 replay에 호출하는 구현은 규범 위반이다.

## 3. 치명적 결함 2: OOB anchor의 보안 경계와 fencing

### 3.1 외부 신뢰 경계

Self-signed Genesis는 public key 소유와 내부 일관성만 증명한다. “이 key가 이
domain의 root다”라는 사실은 자체 서명으로 생기지 않는다. 따라서 제품의 보안 경계를
다음처럼 명시한다.

| 대상 | 신뢰하는 것 | 신뢰하지 않는 것 |
|---|---|---|
| 독립 bootstrap pin | 설치 전에 별도 채널로 고정된 bundle digest/anchor public key와 그 배포 인증 | 설치 디렉터리의 bundle 파일 자체 |
| anchor bundle | 고정된 배포 key로 검증된 `team_id`, `anchor_id`, genesis digest, root key, protocol ceiling | bundle 안의 self-assertion만으로 만든 authority |
| Genesis | pin과 digest가 일치하고 Genesis signature/canonical이 올바른 것 | Genesis가 자기 자신을 서명했다는 사실만으로 만든 새 team/root |
| network peer | 이미 pinned team/anchor와 일치하는 log/proof | 같은 `team_id`만 주장하는 peer |
| local SQLite | bytes와 fsync를 보존하는 저장소 | quorum commit 또는 현재 membership 권위 |

여기서 “별도 채널”은 반드시 서로 독립된 trust root를 뜻한다. 예를 들면 관리자가
설치 전에 고정한 fingerprint, 서명된 release manifest의 배포 key, secure-boot/TPM
provisioning 중 하나 이상이다. 어떤 제품 배포에도 이런 독립 pin이 없다면
`ANCHOR_UNVERIFIED`를 반환하고 membership authorization을 비활성화해야 한다.

### 3.2 bundle 교체 탐지의 한계

한 파일시스템에 bundle과 Genesis가 함께 있고 공격자가 설치 전에 둘 다 교체할 수
있으며 별도 pin·secure boot·transparency log가 없다면, 그 교체는 **탐지 불가**다.
bundle과 Genesis의 내부 digest 비교는 공격자가 새 bundle에 새 Genesis digest를
넣을 수 있으므로 탐지 수단이 아니다. 이 리스크를 “digest 비교로 안전하다”고
표현하지 않는다.

독립 pin이 있으면 노드는 다음을 검사한다.

```text
pinned_anchor_id / pinned_bundle_digest
    == 배포 채널이 고정한 값
bundle.genesis_digest
    == 실제 Genesis complete bytes의 BLAKE3-256
bundle.root keys / team_id / protocol ceiling
    == pinned bundle의 값
```

어느 하나라도 다르면 `ANCHOR_MISMATCH`로 bootstrap과 resolver를 fence한다. 설치
후 운영 로그에만 남기고 계속 실행하는 것은 탐지로 보지 않는다. 두 개의 서로 독립된
운영자가 같은 fingerprint를 비교하거나 transparency log를 조회하는 절차는 제품
배포 선택 사항이지만, 그런 외부 검증이 없을 때의 탐지 능력은 없다.

### 3.3 다른 Genesis를 가진 노드의 domain 합류

`team_id`만 같은 노드는 같은 domain으로 취급하지 않는다. join handshake와
membership log header에는 적어도 다음을 포함한다.

```text
team_id
anchor_id
anchor_epoch
genesis_id
genesis_digest
quorum_config_id
last_committed_index / term
```

coordinator는 자신의 committed anchor와 `anchor_id`, `anchor_epoch`,
`genesis_digest`가 모두 정확히 같은 node만 join candidate로 만든다. 다른 Genesis를
가진 node는 다음을 수행한다.

- `ANCHOR_MISMATCH`로 join을 거부한다.
- 해당 node의 membership action, lease, grant, artifact release, resolver 결과를
  domain 밖으로 공개하지 않는다.
- 그 node가 로컬에 이미 만든 projection을 같은 domain의 state로 merge하지 않는다.
- 운영자는 pinned bundle로 재설치/rebootstrap하거나, 유효한 anchor-rotation proof가
  있는 경우에만 §3.4 절차를 수행한다.

동일 `team_id`에 서로 다른 Genesis를 가진 두 log를 자동 병합하지 않는 것이 fencing
규칙이다. 그렇지 않으면 각자 Owner key가 맞는 것처럼 보이는 split-brain이 된다.

### 3.4 anchor rotation과 폐기

anchor rotation은 파일 교체가 아니라 별도 versioned fact다. 제안하는 절차는 다음과
같다. 다만 실제 rotation 승인자(Owner+Recovery인지, 다른 quorum인지)는 §12의
사용자 결정 항목이다.

1. 현재 anchor가 `new_anchor_id`, `new_bundle_digest`, `new_genesis_digest`,
   `new_root_keys`, `effective_index`를 가리키는 rotation action을 만든다.
2. 현재 정책이 허용한 root/quorum 서명과 `MembershipCommitRecord`를 받아
   `COMMITTED`한다.
3. 새 bundle은 기존 pinned 배포 경로와 독립 pin으로 각 노드에 배포한다. bundle과
   committed rotation fact가 모두 없으면 새 anchor를 신뢰하지 않는다.
4. `effective_index` 이전 action은 old anchor/proof로 역사 replay하고, 이후 신규
   action은 new anchor만 사용한다. old anchor는 history 검증용으로만 보존한다.
5. old anchor를 새 action의 signer lookup에서 폐기하고, old bundle을 쓰는 node는
   `ANCHOR_REVOKED`로 fence한다. 이미 commit된 사실과 historical evidence를
   소급 삭제하지 않는다.
6. old root를 잃었거나 rotation proof가 없는 경우 자동 복구하지 않는다. 독립적인
   operator recovery 또는 재설치가 필요하며, 이를 wire signature 하나로 봉합할 수
   없다.

anchor revocation은 key revocation과 다르다. anchor가 폐기돼도 그 anchor로 검증된
과거 commit proof를 현재 log의 predecessor chain이 보존하는 한 역사 사실은
무효화하지 않는다.

## 4. 치명적 결함 3: `COMMITTED`와 membership authorization의 결속

### 4.1 Raft/과반 경로

membership mutation의 권위 있는 결과는 다음 typed 경계로만 공개한다.

```text
ProposeMembershipOutcome::Committed {
    action,
    commit_record,
    quorum_certificate,
    committed_index,
    term,
}
```

`CommitCertificate`는 §2.2의 `MembershipCommitAttestation` 배열이다. action signer의
서명은 “누가 무엇을 요청했는가”를, quorum certificate는 “과반이 이 log entry를
같은 순서로 확정했는가”를 각각 증명한다. 둘을 하나의 `owner_signature`로
대체하지 않는다.

membership authorization view는 다음을 모두 만족할 때만 생성한다.

- certificate가 anchor 또는 이전 committed coordinator-set으로 검증된다.
- 과반 voter와 연속 index/term/prev hash가 검증된다.
- action signature·historical lifetime·predecessor signer authority·transition이
  검증된다.
- resolver read index가 요청된 revision 이상이며 read-index가 linearizable하다.

### 4.2 SingleNodeStore의 명시적 비활성화

**SQLite 단독 배포에서는 membership authorization을 제공하지 않는다.**

- `AddMember`, `RemoveMember`, `ApproveDevice`, `RevokeDevice`, member state
  transition, owner-key rotation은 `COMMITTED` 요구로 제출한다.
- SingleNodeStore가 반환할 수 있는 `DURABLE` row는 로컬 보존 사실일 뿐이며,
  `CommittedMembershipView`, `AuthorizedMember`, `ResolvedActiveDevice`, lease/grant
  admission 입력으로 변환할 수 없다.
- SingleNodeStore가 `requires = COMMITTED` 요청에 `Unsupported`를 반환하는 것은
  정상이다. quorum/read-index를 잃은 RaftStore는 `Unavailable`/`READ_UNAVAILABLE`로
  닫는다. `DURABLE`을 `COMMITTED`로 이름만 바꾸는 adapter는 금지한다.
- 로컬 SQLite는 서명된 raw action/evidence와 commit proof를 저장할 수는 있지만,
  proof가 없는 자체 row를 commit authority로 만들 수 없다.

bounded-stale 또는 local-cache 경로는 다음 **비권한** 용도로만 허용한다.

```text
HistoricalMembershipObservation {
  as_of_index
  commit_proof_digest
  member/device state at that revision
}
```

UI, 감사, historical evidence 설명에는 사용할 수 있지만 job 제출, device admission,
grant/lease 발급, revoke 판정, canonical 선택, replica count, artifact release에는
사용할 수 없다. requested `as_of_index` 미달·watch gap·snapshot reset이면
`STALE_REVISION`이며 ACTIVE로 추정하지 않는다.

이렇게 해야 기존 `DurabilityRequirement.COMMITTED`, `ProposeOutcome::Durable`,
`SingleNodeStore`의 최대 보증 의미와 membership 계약이 충돌하지 않는다.

## 5. action, field, provenance wire 설계 목록

현재 membership proto는 완성된 Signable schema가 아니다. `AddMember` 등에
`schema_version`, 명시적 `signer_id`/`key_id`, `action_id`, lifetime,
generation, commit provenance가 없고, `RotateOwnerKey`는 단일
`authorizing_signature`만 갖는다.

실제 field 번호는 별도 schema migration에서 배정해야 하지만, 다음 의미는 누락할 수
없다.

### 5.1 모든 membership action

```text
schema_version
team_id
action_id                    // stable idempotency identity
signer_id
signer_key_id
issued_at_unix_ms
expires_at_unix_ms
lifetime_class               // LONG_LIVED 등, action policy와 일치해야 함
expected_membership_revision // predecessor pin, 선택이 아니라 replay guard에 필요
generation                   // subject/device의 단조 세대
signature = field 90
```

`domain_tag`는 wire payload에 임의 문자열로 맡기지 않고 정적 message type으로
결정한다. `AddMember`는 `gputeer/v1/member-add`를 사용한다.

### 5.2 action별 추가

- `AddMember`: member public key, role, requested generation, initial lifecycle data.
- `RemoveMember`: member generation, reason, expected revision.
- `ApproveDevice`: device generation, member generation, member/device key digest,
  binding provenance와 expected revision.
- `RevokeDevice`: device generation, reason, revocation effective revision. Owner
  단독인지 threshold인지에 따라 signer envelope가 달라진다.
- `RotateOwnerKey`: key epoch, old/new key IDs, new keys, migration/cutover index,
  각 authorization signer의 `signer_id`/`key_id`/signature.

모든 action은 committed log가 부여한 `index`, `term`, `prev_entry_hash`,
`action_digest`, `quorum_config_id`, `quorum_certificate`를 `CommitRecord`로
보존해야 한다. 이 commit provenance는 action signer가 자기 payload에 써서 자기
서명만 하는 field가 아니다. coordinator quorum proof로 별도 보호된다.

### 5.3 member/device view

projection에는 적어도 다음이 있어야 한다.

```text
MemberView {
  team_id, member_id, generation, state,
  created_revision, last_transition_revision,
  transition_action_id, transition_proof_digest,
  tombstone / revoked_revision / removed_revision
}

DeviceBindingView {
  team_id, device_id, device_generation, member_id, member_generation,
  public_key_digest, state,
  approved_revision, revoked_revision,
  binding_action_id, binding_proof_digest
}
```

raw `DeviceRecord.member_id`만으로 authoritative mapping을 만들지 않는다.

## 6. member state machine과 기존 state machine의 연결

현재 proto에 `MEMBER_ACTIVATED`, `MEMBER_SUSPEND_COMMITTED`,
`MEMBER_REINSTATED`를 발생시키는 action이 없다. 또한 v0 표의 `ACTIVE -> REMOVED`
중복은 제거한다. 아래는 구현자가 임의 전이를 추가하지 못하게 하는 **제안 계약**이며,
선택 항목인 lifecycle/SUSPENDED 채택은 §12에서 사용자에게 남긴다.

### 6.1 action과 trigger의 대응

| trigger | 필요한 명시 action | 현재 proto 상태 |
|---|---|---|
| `MEMBER_ADD_COMMITTED` | `AddMember` | 존재하지만 schema 부족 |
| `MEMBER_ACTIVATED` | `ActivateMember` | **없음; 새 wire 필요** |
| `MEMBER_SUSPEND_COMMITTED` | `SuspendMember` | **없음; 새 wire 필요** |
| `MEMBER_REINSTATED` | `ReinstateMember` | **없음; 새 wire 필요** |
| `MEMBER_REVOKE_COMMITTED` | `RevokeMember` | **없음; 새 wire 필요** |
| `MEMBER_REMOVE_COMMITTED` | `RemoveMember` | 존재하지만 generation/revision 부족 |

`RemoveMember`를 member revoke action으로 재사용하지 않는다. revoke와 remove는
현재 권한 폐기와 directory/tombstone lifecycle이 다르다.

### 6.2 제안 state table

> ★ 아래 표는 **제안**이지 정본이 아니다. 정본 상태 전이표는
> `docs/protocol/state-machines.md` 하나뿐이므로, 여기서는 예약 마커인
> ```statetable``` 펜스를 의도적으로 쓰지 않는다(`scripts/check_docs.py`
> §4 상태 전이표 중복 검사).

```text
machine: Member
from | to | trigger | guard | effect | durability
(none) | PENDING | MEMBER_ADD_COMMITTED | AddMember 검증 + owner/root policy + tombstone 없음 | generation namespace 예약, device 없음 | COMMITTED
PENDING | ACTIVE | MEMBER_ACTIVATED | ActivateMember 검증 + 같은 generation + predecessor revision 일치 | authorization subject 공개 | COMMITTED
PENDING | REMOVED | MEMBER_REMOVE_COMMITTED | RemoveMember 검증 + 같은 generation | tombstone 보존, authorization 없음 | COMMITTED
ACTIVE | SUSPENDED | MEMBER_SUSPEND_COMMITTED | SuspendMember 검증 + 같은 generation | 신규 admission/lease/grant 차단 | COMMITTED
SUSPENDED | ACTIVE | MEMBER_REINSTATED | ReinstateMember 검증 + 같은 generation + revoked/removed 아님 | 신규 admission 재개 | COMMITTED
ACTIVE | REVOKED | MEMBER_REVOKE_COMMITTED | RevokeMember 검증 + 같은 generation | device/lease/attempt fence cascade | COMMITTED
SUSPENDED | REVOKED | MEMBER_REVOKE_COMMITTED | RevokeMember 검증 + 같은 generation | device/lease/attempt fence cascade | COMMITTED
PENDING | REVOKED | MEMBER_REVOKE_COMMITTED | RevokeMember 검증 + 같은 generation | future binding 금지, tombstone 유지 | COMMITTED
REVOKED | REMOVED | MEMBER_REMOVE_COMMITTED | RemoveMember 검증 + 같은 generation | terminal tombstone 표시, 삭제하지 않음 | COMMITTED
```

다음은 항상 금지한다: `REVOKED -> ACTIVE`, `REMOVED -> ACTIVE`, tombstone ID 재사용,
local cache만으로 activation, `ACTIVE -> REMOVED`의 revoke 없는 직접 전이.

사용자가 AddMember 즉시 ACTIVE를 선택하면 `PENDING -> ACTIVE`를 별도 action 없이
허용하는 것이 아니라, 그 선택에 맞춘 새 action/state schema와 audit 의미를 먼저
확정해야 한다. 그 전까지 `AddMember` commit은 PENDING namespace 예약으로만
취급한다. 사용자가 PENDING/SUSPENDED를 채택하지 않기로 할 때의 표 변경은 §12의
결정 뒤 별도 개정한다.

### 6.3 Node·Lease·Attempt·Checkpoint·Artifact cascade

member revoke가 commit index `r`에서 확정되면 다음 효과를 같은 membership
revision에 묶는다.

1. 알려진 해당 member의 모든 `(device_id, device_generation)` binding을
   `REVOKED`로 projection한다. 늦게 도착한 lower generation binding은 만들지 않는다.
2. 해당 device가 가진 active lease는 `MembershipRevoked(r)` fence를 받고 기존
   Lease 표의 `ACTIVE -> REVOKED`/`REVOKE_RECEIVED` 의미로 폐기한다. coordinator에
   도달하지 못한 agent는 다음 lease renew/heartbeat/data side effect 전에
   `fence_epoch`와 membership revision을 확인하고 fail-closed해야 한다. 이미 발생한
   외부 side effect를 되돌린다고 보장하지 않는다.
3. running Attempt는 즉시 외부 프로세스가 죽었다고 주장하지 않는다. coordinator는
   다음 linearizable observation에서 attempt를 `INTERRUPTED`/reconciliation 경로로
   fence하고, revoke 이후 제출된 새 실행 권한을 거부한다. revoke 이전 시점에
   관측된 report는 §8의 역사 evidence policy에 따라 별도로 심사한다.
4. CheckpointManifest와 ReplicaAck는 현재 권한을 자동으로 유지하지 않는다. 각
   observation의 membership revision/proof를 확인한 뒤 historical input으로만 보존한다.
5. Artifact release와 canonical 선택은 revoked member의 current authorization을
   raw signature나 durable row에서 추정하지 않는다. revoke 후 처리 정책은 §12에서
   사용자가 선택하기 전까지 `UNSUPPORTED`/fail-closed로 둔다.

기존 Node의 `* -> REVOKED`는 node identity revoke이고, Lease의 `ACTIVE -> REVOKED`는
lease fence다. member revoke는 둘을 대체하는 하나의 trigger가 아니라, committed
membership revision에서 해당 node/device에 revoke/fence 사실을 공급하는 상위 원인이다.
Attempt의 `VALIDITY_FILTER_REJECTED`는 revoke 후 device 제출을 제외하는 소비자
규칙으로 연결한다.

## 7. revoke와 DoD-50~DoD-53의 provenance 계약

### 7.1 공통 observation envelope

DoD-50~53의 durable row는 현재 구현 방향대로 load 시 `Verified`로 복원하지
않는다. 대신 각 signed observation의 canonical payload와 함께 다음을 저장해야 한다.

```text
MembershipObservation {
  membership_revision       // observation이 유효했다고 주장한 committed index
  membership_term
  member_id
  member_generation
  device_id
  device_generation
  binding_revision          // ApproveDevice가 commit된 index
  binding_proof_digest
  membership_view_proof_digest
  observed_at_unix_ms
}
```

이 envelope는 AttemptReport/CheckpointManifest/ReplicaAck/필요한 ArtifactRef의
canonical signed body에 포함하거나, 그 body digest에 암호학적으로 결합된 별도
signed wrapper여야 한다. DB column만 추가하고 signed bytes와 결합하지 않는 것은
충분하지 않다. consumer는 `membership_view_proof_digest`에 해당하는
`CommittedMembershipView`를 log/proof에서 다시 만들고, observation revision에서
device와 member가 ACTIVE/승인 상태였는지 확인한다.

`observed_at`과 `membership_revision`은 서로 다른 값이다. 시계가 늦거나 빠르다는
이유로 revision을 발명하지 않으며, revision이 proof와 맞지 않으면 typed corruption
또는 invalid evidence다.

### 7.2 소비자별 계약

| 소비자 | 역사적으로 인정할 최소 조건 | 현재 권한으로 쓸 수 있는가 |
|---|---|---|
| DoD-50 JobManifest binding | signed body, raw binding integrity, submitter/device와 observation revision의 committed proof 일치 | 아니오. resolver 재검증 후에만 scheduler가 별도 view 생성 |
| DoD-51 AttemptReport | report signer/device와 historical member/device binding, attempt/job/fence, observation revision 일치 | 아니오. canonical 선택 전 validity filter 필요 |
| DoD-52 CheckpointManifest | Attempt anchor/root와 historical membership proof가 같은 revision scope | 아니오. revoke 후 새 복구 권한으로 자동 승격 금지 |
| DoD-53 ReplicaAck | holder signature, checkpoint/root binding, holder의 historical membership proof와 device generation 일치 | 아니오. immutable observation을 그대로 replica 수로 세지 않음 |

DoD-50~53의 load 결과는 `Stored...Binding` 같은 raw 타입이어야 한다. 서명을
재검증하고 historical proof까지 대조한 결과는 `RevalidatedHistoricalObservation`
같은 별도 타입으로 반환하며, 이것을 `Verified<CurrentAuthorization>`로 이름
바꾸지 않는다.

### 7.3 revoke 전후 처리의 미결정과 안전한 임시 계약

member가 revoke되기 전에 유효했던 evidence, AttemptReport, CheckpointManifest,
ReplicaAck를 historical evidence로 인정할지, revoke 후 canonical 선택·effective
replica count·artifact release에서 제외할지는 보안 정책과 데이터 보존 정책의
결합점이다. v1은 이를 조용히 확정하지 않는다.

가능한 선택지는 다음과 같다.

| 선택지 | 의미 | trade-off |
|---|---|---|
| A. historical-only | revoke 전 revision에서 유효했던 관측은 감사/재현에는 인정하되 revoke 후 current canonical/count/release에는 제외 | 안전하고 단순하지만 정상 완료 artifact의 후속 release가 영향을 받음 |
| B. committed-outcome 보존 | revoke 전에 이미 canonical/replica/release가 COMMITTED된 결과는 유지하고, revoke 후 새 선택·증분만 제외 | 가용성과 역사 안정성이 좋지만 compromised member의 과거 결과를 유지할 위험 |
| C. 재심사 | 기존 evidence를 보존하되 독립 member/device 또는 coordinator 검증을 통과한 경우만 현재 소비에 재사용 | 가장 유연하지만 재검증 protocol과 운영 비용이 큼 |

사용자가 고르기 전의 임시 안전 계약은 다음과 같다.

- raw evidence의 역사 보존과 감사 조회는 허용한다.
- current authorization, 새 lease/grant, canonical 선택, effective replica count,
  artifact release에 revoked member observation이 영향을 주면 `UNSUPPORTED` 또는
  명시적 `REVOKED_EVIDENCE_POLICY_UNSET`로 닫는다.
- DoD-54 순수 count kernel은 정책을 발명하지 않는다. resolver가 선택한
  `ResolvedHolderObservation`에 historical/current eligibility를 명시해서 넘겨야
  하며, kernel은 membership을 직접 조회하지 않는다.

이것이 “revoke 전 제출물을 인정할지”와 “revoke 후 소비에서 제외할지”를 서로 다른
질문으로 보존하는 계약이다.

## 8. resolver와 durability 경계

권한용 resolver API와 비권한 historical API를 분리한다.

```text
resolve_current_authorization(device_id, min_index)
  -> Authorized(member_id, generations, revision, commit_proof)
  -> READ_UNAVAILABLE | STALE_REVISION | UNKNOWN | REVOKED | AMBIGUOUS

read_historical_membership(device_id, as_of_index)
  -> HistoricalMembershipObservation
  -> STALE_REVISION | PROOF_MISSING | AMBIGUOUS
```

첫 번째 API는 linearizable read-index와 `COMMITTED` proof를 요구한다. quorum이
없으면 가용성을 위해 stale ACTIVE를 내보내지 않고 `READ_UNAVAILABLE`이다. 두 번째
API는 UI/audit/evidence만을 위한 것이며 어떤 caller도 그 반환값을 lease/grant
authorization으로 넘길 수 없다.

resolver는 다음을 반드시 검사한다.

- 같은 `(team_id, device_id, device_generation)`에 서로 다른 active member가 있으면
  `AMBIGUOUS_BINDING`으로 전체 실패한다.
- 제시된 public key digest와 committed binding이 다르면 `KEY_MISMATCH`다.
- `as_of_index`가 revoke/removed revision보다 낮거나 proof가 없으면 ACTIVE로
  추정하지 않는다.
- local raw row, durable flag, cached `member_id`를 signer authority의 대체물로
  사용하지 않는다.
- watch gap/reset이면 해당 cache는 invalidate하고 필요한 log/proof까지 재동기화한다.

## 9. RotateOwnerKey: 현재 schema와 2-of-2 제안의 불일치

현재 `proto/control.proto`의 `RotateOwnerKey`는
`authorizing_signature` 하나만 가진다. v0가 제안한 Owner+Recovery 2-of-2는 정책
문구만 추가해서 구현할 수 없다. 그것은 schema, canonical field set, signature
envelope, migration의 변경이다.

가능한 wire 경로는 다음과 같다.

| 경로 | 장점 | 위험 |
|---|---|---|
| 기존 message에 두 번째 서명 field 추가 | message 이름 유지 | 구 schema가 새 canonical을 해석하지 못하고, field 90 의미를 잘못 재사용할 위험 |
| `RotateOwnerKeyV2` additive message | old bytes를 재해석하지 않음; signer/key/action/lifetime을 명시하기 쉬움 | 새 domain/schema, cutover와 migration 필요 |
| threshold envelope 공통 type | 향후 coordinator 정책과 공유 가능 | quorum/weight/정렬/canonical 규칙이 추가로 고정되어야 함 |

2-of-2를 사용자가 선택하는 경우의 권고 migration은 다음과 같다.

1. 기존 `RotateOwnerKey`를 legacy decoder로 보존하고, 새 생성 경로에서는
   `RotateOwnerKeyV2`와 명시적 schema version, action_id, key IDs, lifetime,
   key epoch, new keys, `repeated AuthorizationSignature`를 사용한다.
2. 각 authorization signature는 signer_id/key_id와 같은 action digest를 서명하며,
   canonical에서 제외되는 실제 signature bytes와 canonical에 포함되는 signer
   identity를 분리한다. Owner와 Recovery 두 distinct key가 모두 있어야 한다.
3. V2 cutover action 자체를 old policy로 허용할지, 별도 externally pinned recovery로
   허용할지는 migration policy로 정한다. V2를 commit한 뒤 effective index 이후에는
   single-signature V1을 신규 action으로 받지 않는다.
4. cutover 이전에 이미 `COMMITTED`된 V1 action은 그 당시의 provenance로 역사
   replay한다. V1 bytes를 V2의 두 서명으로 소급 해석하지 않는다.
5. V1 legacy action을 cutover 이후에도 받을지는 명시적인 grace/legacy 정책과
   end-index가 없는 한 금지한다.

2-of-2 자체, Recovery 분실 시 social recovery, V1 legacy 수용 기간은 확정하지
않는다. 사용자가 단일 signer 정책을 선택하면 V2 migration은 필요 없지만, 그 경우
단일 key compromise trade-off를 승인 문서에 남겨야 한다.

## 10. AddMember tag와 legacy migration

정본은 `gputeer/v1/member-add`로 확정된 사실이다. v1은 stale proto 주석이나
vector 설명을 근거로 `gputeer/v1/membership`을 새 AddMember에 허용하지 않는다.

다만 외부에서 이미 `membership` tag로 생성된 서명을 처리해야 하는지는 별도
사용자 결정이다.

| 선택지 | trade-off | 권고 |
|---|---|---|
| legacy 수용 안 함 | domain ambiguity와 downgrade 경로가 없음; 기존 서명 폐기 필요 | 보안상 가장 단순 |
| 제한된 legacy migration | cutover 전 action_id/index 범위에 한해 별도 legacy verifier로 역사 import 가능 | 운영 호환성은 좋지만 migration proof·end date 필요 |
| 두 tag를 계속 신규 허용 | 호환성은 가장 좋음 | 동일 semantic action의 두 서명 문맥과 replay ambiguity가 생겨 비권고 |

legacy를 수용하더라도 `membership` 서명을 새 `member-add` 서명으로 byte 변환하거나
새 action으로 재서명한 것처럼 만들지 않는다. 원래 tag, schema, action digest,
import provenance를 별도로 보존해야 한다.

## 11. restart, snapshot, durable load

### 11.1 normative reference

full committed log replay가 의미론적 기준선이다. restart는 다음을 완료하기 전까지
membership directory를 resolver에 공개하지 않는다.

```text
anchor pin / Genesis 검증
-> commit proof chain 검증
-> action signature 검증
-> action 직전 signer authority 검증
-> proof에 보존된 historical lifetime 검증
-> action_id/generation/transition 검증
-> deterministic projection 재생
-> device uniqueness/tombstone/prev hash 검증
-> linearizable current read-index와 마지막 committed index 대조
```

어느 하나라도 실패하면 부분 projection을 공개하지 않고
`CORRUPT_DIRECTORY` 또는 원인별 typed error로 닫는다. 단, 정상 commit action의
현재 lifetime이 지났다는 이유만으로 실패해서는 안 된다. 그 경우 §2의 historical
proof를 사용한다.

### 11.2 signed snapshot + tail

signed snapshot + tail은 성능 최적화로만 허용할 수 있다. snapshot을 도입하려면
다음이 필요하다.

```text
MembershipSnapshot {
  team_id, anchor_id, schema_version
  last_committed_index, term
  last_action_digest, previous_log_hash
  projection_digest
  quorum_config_id
  snapshot_certificate
}
```

snapshot certificate는 snapshot digest, 마지막 index/term, root/anchor identity,
schema ceiling을 coordinator quorum이 서명한 proof다. tail의 첫 entry가 snapshot의
last digest와 이어져야 하며, snapshot projection digest를 재계산한 결과가 맞아야
한다. proof가 없거나 key가 revoked되었다는 이유만으로 과거 snapshot을 지우지
않지만, 현재 anchor가 인정하지 않는 snapshot을 current authorization으로 쓰지
않는다.

사용자가 signed snapshot + tail을 채택하지 않으면 full replay만 사용한다. 채택
여부와 proof 형식의 최종 선택은 §12에 남긴다.

## 12. 사용자 결정 필요 항목

아래 항목은 이 초안이 임의로 확정하지 않는다. 각 항목에 선택지, trade-off, 권고를
함께 제시한다.

### 12.1 Owner+Recovery 2-of-2 root rotation

- **선택지:** (A) Owner+Recovery 2-of-2, (B) Owner 단독, (C) 별도 coordinator
  threshold/사회적 복구.
- **trade-off:** A는 단일 key 탈취에 강하지만 두 key 동시 가용성이 필요하고,
  schema/canonical/migration이 복잡하다. B는 가용성과 구현 단순성이 좋지만 Owner
  compromise가 곧 root takeover다. C는 운영 분산성이 좋지만 coordinator set 자체의
  bootstrap과 threshold scheme이 추가된다.
- **권고:** A를 선호한다. 그러나 사용자가 고르기 전에는 `RotateOwnerKeyV2`나
  2-of-2를 확정된 규범으로 부르지 않는다.

### 12.2 AddMember/ApproveDevice와 RevokeDevice 권한 주체

- **선택지:** Owner 단독, Owner+Recovery, member 본인+Owner, committed coordinator
  threshold, 긴급 revoke 전용 OR 정책.
- **trade-off:** Owner 단독은 가용성이 좋고 현재 proto와 가깝지만 key compromise에
  약하다. 다중 서명은 안전하지만 장애·운영 지연이 커진다. member self-approval은
  분산되지만 member key lifecycle이 먼저 필요하다. threshold는 정확한 quorum
  registry와 signature envelope가 필요하다.
- **권고:** AddMember/ApproveDevice는 초기에는 Owner policy를 단순 기준으로 삼고,
  RevokeDevice는 긴급 가용성과 threshold 안전성 중 하나를 별도로 결정한다. 이는
  확정이 아니다.

### 12.3 `PENDING -> ACTIVE`, SUSPENDED/REINSTATED 채택

- **선택지:** AddMember 즉시 ACTIVE, PENDING 후 별도 ActivateMember, PENDING만 두고
  activation은 외부 policy, SUSPENDED/REINSTATED 포함 또는 제외.
- **trade-off:** 즉시 ACTIVE는 단순하지만 승인 전 노출이 생긴다. 2단계는 안전한
  준비/승인을 가능하게 하지만 action·cleanup이 늘어난다. SUSPENDED는 긴급 차단을
  표현하지만 복구 권한과 audit semantics가 필요하다.
- **권고:** PENDING과 명시적 ActivateMember, SUSPENDED/REINSTATED를 유지하는 안을
  권고한다. 다만 현재 proto action이 없으므로 사용자가 고르기 전에는 어느 전이도
  구현 가능하다고 쓰지 않는다.

### 12.4 mutation 기본 TTL 7일

- **선택지:** 7일 공통, action별 TTL, 짧은 TTL + online renewal, Perpetual.
- **trade-off:** 7일은 offline queue에 강하지만 탈취 action 창이 길다. action별
  TTL은 위험에 맞지만 policy 복잡성이 커진다. 짧은 TTL은 보안성이 좋지만 장애와
  clock/renewal에 취약하다. Perpetual은 만료 replay 위험이 커서 mutation에는
  부적합하다.
- **권고:** LongLived mutation의 제안 기본값은 7일이다. 이미 commit된 action의
  역사 replay에는 current now를 쓰지 않는다는 §2를 TTL 선택과 별개로 유지한다.

### 12.5 tombstone 영구 보존과 법적 예외

- **선택지:** 영구 보존·ID 재사용 금지, 최소 cryptographic retention proof 후 GC,
  법적 요청에 따른 제한 삭제.
- **trade-off:** 영구 보존은 delayed action/resurrection 방어가 강하지만 저장·개인정보
  부담이 있다. GC는 비용을 줄이지만 ID 재사용과 과거 proof 연결을 어렵게 한다.
- **권고:** 기본은 영구 tombstone과 ID 재사용 금지다. 법적 예외를 허용하려면 먼저
  최소 tombstone proof, 재사용 금지 registry, historical verification 정책을 별도
  확정해야 하며, 이 초안이 예외를 자동 허용하지 않는다.

### 12.6 revoke member의 기존 evidence/Attempt/Checkpoint/Replica 정책

- **선택지:** §7.3의 A historical-only, B committed-outcome 보존, C 재심사.
- **trade-off:** A는 fail-closed와 현재 안전성이 강하지만 기존 결과의 release가
  영향을 받는다. B는 운영 연속성이 좋지만 과거 compromised member 결과를 유지할
  수 있다. C는 균형이 가능하지만 새 검증 protocol과 비용이 든다.
- **권고:** 최소 안전선은 historical 보존과 current authorization 분리이며, 선택 전
  revoke-tainted current canonical/count/release를 `UNSUPPORTED`로 닫는다.

### 12.7 resolver linearizable 강제와 가용성 희생

- **선택지:** 모든 권한 read linearizable, bounded-stale + caller minimum index,
  local cache.
- **trade-off:** linearizable은 revoke race를 줄이지만 quorum 장애 시
  `READ_UNAVAILABLE`이다. bounded-stale은 가용성이 좋지만 caller 실수가 stale
  ACTIVE를 만들 수 있다. local cache는 빠르지만 권한 경로로는 안전하지 않다.
- **권고:** current authorization은 linearizable, historical/UI만 명시적 bounded-stale,
  local cache는 표시/진단 전용으로 한다. 사용자가 가용성 우선 정책을 고르면 그
  경로는 권한이 아닌 비권한 경로로만 남겨야 한다.

### 12.8 signed snapshot + tail과 snapshot proof

- **선택지:** full replay만, quorum-signed snapshot + tail, trusted local snapshot.
- **trade-off:** full replay는 단순하지만 느리다. signed snapshot은 빠르지만 proof,
  root binding, rollback 방어가 필요하다. trusted local snapshot은 빠르지만 외부
  권위가 없어 current authorization에 부적합하다.
- **권고:** full replay를 normative reference로 두고 §11.2 형식의 signed snapshot을
  최적화로 허용한다. proof 형식은 사용자 선택과 별도 schema review 없이는 확정하지
  않는다.

### 12.9 legacy `membership` 서명 migration

- **선택지:** 즉시 거부, cutover 전 범위의 legacy import, 기간 제한 grace, 계속 신규
  이중 허용.
- **trade-off:** 즉시 거부는 안전하지만 호환성이 없다. 제한 import는 운영 부담과
  provenance 관리가 필요하다. 계속 이중 허용은 downgrade/cross-domain replay
  위험이 크다.
- **권고:** 새 action은 `gputeer/v1/member-add`만 허용하고, 필요할 때만 명시적
  end-index가 있는 legacy import를 추가한다. 기존 `membership`을 silently accept하는
  것은 권고하지 않는다.

## 13. 미해결·정직한 한계

다음은 이 문서만으로 해결되지 않는다.

1. 실제 protobuf field 번호, 새 message의 정확한 canonical field set, error enum,
   quorum signature encoding은 아직 schema review가 필요하다.
2. quorum signer가 과반 장악되거나 OOB pin 배포 key가 장악되면 이 규범은 그 사실을
   암호학적으로 복구하지 못한다. 별도 key ceremony, HSM, transparency/audit 운영이
   필요하다.
3. wall-clock lifetime은 quorum voter의 시계 정직성을 필요로 한다. commit proof는
   검사를 누가 했는지 위조 방지하지만 물리적 시간을 증명하지 않는다.
4. member revoke가 이미 실행된 외부 workload를 되돌리거나 이미 방출된 artifact를
   회수한다고 보장할 수 없다. fence는 이후 권한 사용을 닫는 계약이다.
5. DoD-50~53의 기존 durable row를 새 membership observation field로 안전하게
   migration하는 정확한 backfill 정책은 아직 없다. provenance가 없는 legacy row를
   현재 권한으로 살리는 것은 금지하며, 역사 evidence로도 인정할지는 §12.6 정책이
   필요하다.
6. signed snapshot을 실제로 채택하지 않는다면 full replay 외의 restart 최적화는
   권위 경로가 아니다.
7. SingleNodeStore가 membership authorization을 제공하지 않는다는 선택 때문에,
   단독 설치에서 offline admission을 계속 제공하려면 별도의 비권한/운영 절차가
   필요하다. 그것을 local durable row로 위장할 수 없다.

## 14. v1이 세 치명적 결함을 어떻게 고쳤는가

- **결함 1:** 신규 제출의 `LongLived(now)`와 역사 replay를 분리하고,
  action digest·lifetime 값·per-voter checked time·index/term/prev hash를 포함한
  quorum-signed `MembershipCommitRecord`를 보존한다. replay는 현재 now가 아니라
  commit proof의 lifetime 검사를 검증하며, 기존 `Lifetime::LongLived` 구현은 신규
  제출에 그대로 공존한다.
- **결함 2:** OOB bundle의 진위를 제품 외부의 독립 pin으로 명시하고, 독립 pin이
  없으면 교체 탐지 불가임을 명시했다. `anchor_id/genesis_digest`가 다른 node의
  join을 `ANCHOR_MISMATCH`로 fence하고, rotation/revocation을 versioned committed
  fact와 외부 배포로 분리했다.
- **결함 3:** `COMMITTED`를 action signer와 별도의 quorum commit certificate로
  membership view에 결속했다. SQLite 단독 배포의 membership authorization을
  명시적으로 비활성화하고, bounded-stale/local 결과는 historical/UI 비권한 경로로
  한정했다.

그 밖에 revoke provenance, DoD-50~53 consumer 경계, state-machine의 action 공백과
cascade, RotateOwnerKey schema migration, membership wire field 누락을 문서화했다.
아직 미해결인 것은 §12의 사용자 선택과 §13의 암호·schema·운영 한계다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-24 18:30 | v0 적대적 검토의 치명적 결함 3건과 revoke/state/wire 공백을 반영한 v1 초안 작성 |
