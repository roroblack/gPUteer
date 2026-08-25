# 2026-08-24_1730 membership 규범 초안 v0

- **상태:** 검토용 초안. 확정 규범·구현 지시가 아님
- **조사 기반:** `docs/plans/2026-08-24_1700_membership_norm_skeleton_v1.md`
- **직접 대조한 선례:** Lease·Grant·`AttemptReport`·`CheckpointManifest`·`ReplicaAck`의
  signed 입력, `Verified<M>` 경계, durable binding, restart 재검증 패턴
- **중요한 제한:** 이 초안은 `crates/`, `proto/`, `docs/protocol/`의 기존 파일을
  수정하지 않는다. 아래의 schema/action/state 표는 사용자 검토 뒤 별도 변경 작업으로
  반영해야 한다.

## 결론

membership의 권위 있는 사실은 **독립적으로 서명된 현재 `MemberRecord` 하나가 아니라,
root anchor에서 시작해 순서대로 검증된 `COMMITTED` membership action log와 그 결정적
projection**으로 두는 것을 권고한다.

권고하는 신뢰 경로는 다음과 같다.

```text
out-of-band bootstrap trust bundle
  -> verified Genesis record
  -> committed owner/recovery/coordinator key registry
  -> verified membership actions
  -> committed Member/Device projection
  -> revision-pinned resolver result
```

이렇게 해야 첫 `AddMember`를 검증할 때 아직 존재하지 않는 membership directory를
사용하는 순환이 끊긴다. 최초 signer key는 membership으로 찾지 않고 bootstrap anchor에서
찾는다. 그 다음 action부터만 이전에 검증·commit된 key registry를 사용할 수 있다.

이 문서는 다음을 권고한다.

1. `AddMember`·`RemoveMember`·`ApproveDevice`·`RevokeDevice`를 서로 다른 domain의
   mutation으로 유지하고, 각 메시지에 `schema_version`, canonical `signer_id`/`key_id`,
   operation identity와 lifetime 입력을 추가한다.
2. action만 서명 대상이 되고, `MemberRecord`/`DeviceBinding`은 검증된 action log의
   결정적 projection이 된다. 별도 signed snapshot은 최적화로만 허용하며, snapshot
   protocol은 별도 결정 없이는 도입하지 않는다.
3. root는 **out-of-band로 고정된 Owner root + 별도 Recovery root**를 권고한다. self-signed
   Genesis는 anchor가 이미 고정한 key의 binding을 증명할 뿐, 그 자체로 trust를 만들지
   않는다.
4. member revoke/remove는 수신 시각이 아니라 `COMMITTED` index/term과 monotonic
   generation에서 효력을 얻는다. tombstone은 member/device ID 재사용을 막기 위해
   기본적으로 영구 보존한다.
5. resolver의 권한 판단은 기본적으로 linearizable `ControlStore` read를 사용한다.
   stale read는 명시적인 `as_of_index`가 요구되는 비권한 용도 외에는 fail-closed한다.
6. restart는 full replay 또는 검증된 snapshot+tail replay를 완료하기 전까지 directory를
   공개하지 않는다. durable row를 `Verified`로 복원하지 않는다.

아래 권고는 모두 최상위 규칙 `CLAUDE.md` §0.2, 즉 **“서명 검증 전에는 아무것도
신뢰하지 않는다”**와 정합한다. 단, `signer_id`를 키 조회 hint로 읽는 기존 §8.1 예외는
그대로 인정하며, 검증 전에는 그 값을 라우팅·권한·state 판단에 쓰지 않는다.

## 범위

### In

- member/device/node/coordinator/owner/recovery의 identity graph와 signer lookup 경계
- membership action과 projection 중 무엇을 signable/durable authority로 삼는지
- root of trust bootstrap, key rotation, signer authorization
- canonical, field 90, domain tag, lifetime, fail-closed 검증 순서
- Member 상태기계, revoke/remove, tombstone, device cascade
- `device_id -> member_id` authoritative resolver 계약
- ControlStore commit/read consistency, revision pinning
- durable membership의 restart revalidation과 replay/idempotency
- 현재 AddMember domain tag 불일치의 판정과 별도 수정 필요성

### Out

- `proto` field 번호·실제 schema migration·canonical reference vector 추가
- threshold signature의 최종 암호 스킴(FROST 등)과 Coordinator quorum의 운영 구성
- node state machine, Job/Attempt/Lease/Checkpoint 전이의 재정의
- 실제 ControlStore/Raft 구현, wire routing, CLI, key storage 구현
- device capability·policy·quarantine의 전체 규범
- GDPR/법적 보존기간에 따른 tombstone 삭제 정책
- 외부 부작용을 이미 일으킨 workload를 되돌리는 보장

## 1. 기존 규범과의 고정 골격

### 1.1 검증 순서

membership action도 기존 순서를 바꾸지 않는다.

```text
1. 정적 message type에서 domain_tag 결정
2. schema_version 확인; 초과면 SCHEMA_TOO_NEW로 종료
3. signature field 90을 제외한 canonical 재구성
4. domain_tag || schema_version || canonical 길이 || canonical 조립
5. Ed25519 검증; 실패면 INVALID_SIGNATURE로 종료
6. signer identity/key가 해당 시점의 trust graph에서 허가되었는지 확인
7. lifetime 확인
8. replay/operation identity 확인
9. 여기서부터 action field와 state transition guard를 신뢰
```

검증 전에 `member_id`, `role`, `public_key`, `device_id`, `effective_at`, `reason`,
`generation`을 읽어 routing·권한·state 판단에 사용하지 않는다. `signer_id`/`key_id`만
키 조회 키로 사용하는 것은 기존 규범 §8.1의 제한된 예외다. 어떤 단계든 실패하면
projection을 만들거나 바꾸지 않고 typed reject를 반환한다.

### 1.2 Signable 대상 선택

| 선택지 | 장점 | 위험·trade-off | §0.2 정합성 |
|---|---|---|---|
| A. action만 signable | 현재 `Add/Remove/Approve/Revoke`의 명령 의미가 분명하고, ordered log에서 projection을 재생할 수 있음 | snapshot 전달·빠른 조회에는 별도 proof가 필요함 | action을 `Verified`로 만든 뒤에만 apply하므로 정합 |
| B. 현재 `MemberRecord`/`DeviceBinding`만 signable | 조회 결과가 바로 self-contained이고 offline 검증이 쉬움 | 누가 어떤 순서로 state를 바꿨는지 잃고, stale snapshot·rollback·동시 update를 막기 어려움 | record 서명만으로 과거 authorization과 commit을 보장하지 못하면 위반 위험 |
| C. action과 record를 모두 독립 signable | log와 snapshot 모두 직접 검증 가능 | 두 서명 대상의 의미·revision·root가 어긋나는 이중 권위와 복잡한 migration 발생 | 두 개 모두 검증할 때만 안전하지만 일치 규칙을 빠뜨리면 우회 경로가 생김 |

**권고: A.** mutation의 signable 대상은 action으로 한정하고, `MemberRecord`와
`DeviceBinding`은 검증된 action log의 deterministic projection으로 정의한다. projection은
서명 검증과 commit proof를 통과한 action만으로 계산한다. 향후 signed snapshot이 필요하면
마지막 committed index, 이전 log hash, team/root identity를 포함하는 별도 message와
새 domain tag를 먼저 정한다.

현재 action에는 이 규범에 필요한 모든 field가 없으므로 이것은 현 proto가 이미 만족한다는
주장이 아니다. `Signable`/`Verified<M>` 구현과 schema 변경은 후속 계약 작업이다.

### 1.3 AddMember domain tag 불일치

현재 `proto/control.proto`의 `AddMember` 주석은 `gputeer/v1/membership`을 가리키지만,
`docs/protocol/signing.md` §5의 domain 표는 `gputeer/v1/member-add`를 등록한다. 이는
서명 검증에서 같은 canonical payload를 서로 다른 문맥으로 만들 수 있는 실제 불일치다.

| 선택지 | trade-off | 판단 |
|---|---|---|
| `membership` 유지 | 기존 주석과 맞지만, signing 규범의 action별 tag와 ADR-028의 tag 분리 원칙을 깨뜨림 | Add/Remove/Approve/Revoke가 다시 공유 tag로 합쳐질 위험 |
| `member-add` 채택 | signing 표와 action별 분리, cross-message replay 방지에 맞음 | proto 주석과 구현/벡터를 별도 작업에서 함께 갱신해야 함 |
| 두 tag를 버전별로 모두 허용 | migration은 쉬워 보임 | 같은 action에 두 서명 문맥이 생기고 downgrade/cross-version ambiguity가 발생 |

**권고: `gputeer/v1/member-add`.** 근거는 현재의 normative domain 표와 “새 signable
message마다 독립 tag”라는 규칙이다. `membership`은 주석의 오류로 취급하고, 기존에
생성된 `membership` 서명을 조용히 유효화하지 않는다. 다만 어떤 tag를 실제 wire 규범으로
확정할지는 **사용자 결정 필요**이며, 이 초안은 기존 파일을 고치지 않는다.

이 선택은 §0.2와도 직접 정합한다. 검증자가 message type에 따라 하나의 tag만 선택하게
하여, 검증 전에 payload의 의미를 추측하거나 다른 action의 유효 서명을 재사용하는 경로를
닫는다. 잘못된 tag는 경고가 아니라 `INVALID_SIGNATURE`/domain mismatch로 끝나야 한다.

## 2. Identity graph와 root of trust

### 2.1 Identity graph

```text
Team
 ├─ OwnerRootKey (root authority)
 ├─ RecoveryRootKey (separate recovery authority)
 ├─ CoordinatorKey(s) (only if committed policy permits)
 └─ Member(member_id, generation, role, state)
      └─ Device(device_id, generation, public_key, state)
```

`member_id`는 public key의 대체 이름이 아니다. `device_id -> member_id` binding은
`ApproveDevice` action과 그 committed projection이 제공한다. keyring에서
`signer_id -> public key` 조회가 성공했다고 active member·approved device라고 결론내리지
않는다.

모든 action에는 적어도 `team_id`, `action_id`/nonce, `schema_version`, `signer_id`,
`signer_key_id`, signature field 90이 필요하다. `public_key`와 `member_id` 등 subject
field는 canonical에 들어가야 한다. signer identity가 signature payload 밖에 있으면
다른 key의 서명을 잘못 귀속할 수 있으므로 허용하지 않는다.

### 2.2 순환을 끊는 root 선택

| 선택지 | 신뢰 가정 | 장점 | 위험·trade-off |
|---|---|---|---|
| A. out-of-band bootstrap bundle | 설치 시 운영자가 `team_id`, genesis digest, OwnerRootKey, RecoveryRootKey를 별도 채널로 pin했다는 가정 | 첫 action부터 membership 없이 검증 가능; recovery key를 owner가 자기 마음대로 추가하지 못함 | bundle 배포·교체 절차와 운영자 실수가 trust boundary가 됨 |
| B. self-signed Genesis만 신뢰 | 최초 Genesis의 public key가 자기 자신을 인증해도 된다는 가정 | proto가 단순하고 offline 생성이 쉬움 | self-signature는 key 소유만 증명하며 “이 팀의 root”라는 외부 authenticity를 만들지 못함. 공격자가 새 genesis/team을 만들 수 있음 |
| C. 운영자 고정 key만 신뢰 | 모든 설치가 동일하거나 사전 배포된 OperatorKey를 신뢰한다는 가정 | bootstrap이 단순하고 중앙 회수 가능 | 단일 운영자 key가 전 팀의 단일 장애·침해 지점; tenant별 독립성이 약함 |
| D. 초기 Coordinator quorum을 root로 신뢰 | 설치된 Coordinator 집합이 이미 정직하고 정확히 고정됐다는 가정 | 운영 중 quorum 모델과 자연스럽게 연결 | 그 Coordinator 집합을 누가 인증하는지 다시 필요하므로 순환을 제거하지 못함 |

**권고: A.** bundle은 서명된 Genesis를 포함할 수 있지만, Genesis의 Owner/Recovery
key가 신뢰되는 이유는 bundle pin 때문이다. 구체적인 최초 절차는 다음과 같다.

1. 설치 전에 운영자가 OOB 채널로 `team_id`, protocol/schema ceiling, OwnerRootKey,
   RecoveryRootKey, Genesis digest를 배포·pin한다.
2. 노드는 bundle의 digest와 서명된 Genesis를 비교하고, key·team·genesis_id가 일치할
   때만 Genesis를 Perpetual root fact로 받아들인다.
3. 최초 `AddMember`는 이 anchor의 OwnerRootKey로 검증한다. `AddMember`가 최초 member를
   만든다고 해서 그 signer가 새로 신뢰되는 것은 아니다.
4. Genesis 이후의 key registry와 member/device projection은 오직 앞서 검증되고
   committed된 action에서만 갱신한다.

이 가정은 “설치 시 OOB bundle의 진위와 무결성을 확인했다”는 하나의 명시적인 외부
신뢰를 요구한다. 그 절차를 제공할 수 없다면 이 시스템은 첫 membership action의
authenticity를 증명할 수 없으며, self-signed Genesis만으로 안전하다고 선언해서는 안 된다.

§0.2와의 정합성은 명확하다. 최초에도 signer key 조회만 OOB anchor를 통해 허용하고,
Genesis·action의 다른 field는 domain/schema/canonical/signature 검증이 끝날 때까지
사용하지 않는다. anchor가 없거나 bundle과 Genesis가 다르면 “첫 member”를 추측하지
않고 전체 bootstrap을 거부한다.

### 2.3 Owner·Recovery·Coordinator signer 선택

| 결정 | 선택지 | trade-off | 권고 및 §0.2 정합성 |
|---|---|---|---|
| member add/remove | Owner 단독 / Owner+Recovery / Coordinator quorum | 단독은 가용성이 높지만 key compromise에 약함; 다중 서명은 안전하지만 운영 복잡·가용성 저하 | v0는 Owner 단독 action을 허용하되 OOB OwnerRootKey로 시작한다. 다만 `RemoveMember`와 root rotation은 Recovery 승인 없이는 권고하지 않으며, 모든 signer 검증 전 state를 쓰지 않는다 |
| device approve | Owner 단독 / member 본인+Owner / Coordinator | 본인 승인은 분산되지만 member key lifecycle이 먼저 필요; Coordinator는 별도 quorum root 필요 | v0 권고는 Owner 승인이다. member self-service는 후속 action으로 분리한다 |
| device revoke | Owner 서명 / 2-of-3 Coordinator / 둘의 OR | OR는 가용성이 높고 정책이 넓어짐; threshold는 안전하지만 정확한 scheme·quorum registry가 필요 | 긴급 revoke는 Owner 또는 이미 committed된 Coordinator threshold policy 중 하나만 허용하되, threshold envelope가 확정되기 전에는 `RevokeDevice`를 `Verified`로 노출하지 않는다 |

Owner 단독을 권고하는 이유는 현재 root와 action schema가 가장 직접적으로 연결되기
때문이지, 단일 key compromise가 안전하다는 뜻이 아니다. Owner key는 오프라인 보관을
권고하고, root rotation에는 별도 Recovery 승인(아래)을 요구한다. threshold의 최종
암호 스킴과 Coordinator 집합은 **사용자 결정 필요**다.

### 2.4 Key rotation

| 선택지 | trade-off |
|---|---|
| 기존 key 단독으로 새 key 승인 | 단순하지만 기존 key가 탈취되면 영구 takeover 가능 |
| 기존 Owner + Recovery의 2인 승인 | root compromise를 한 key로 완성하기 어렵고 Recovery의 역할이 명확함; 두 key를 모두 잃으면 복구 불가 |
| Coordinator threshold가 Owner를 교체 | 운영 분산은 좋지만 Coordinator set 자체의 root·quorum·offline recovery가 추가됨 |

**권고: 기존 Owner + Recovery의 2인 승인.** `RotateOwnerKey`는 old owner signature와
recovery signature가 같은 action digest에 대해 검증되고 `COMMITTED`된 때만 새 key가
유효하다. 전환 index 이전의 action은 old key로, 이후 action은 new key로 검증한다.
기존 규범의 24시간 grace는 “전환 전에 발행된 유효 action의 늦은 전달”에만 적용하고,
전환 이후 새 action을 old key로 허용하는 우회로 사용하지 않는다. 이 규칙은 검증 전
key status를 판단하지 않고, ordered log의 predecessor state에서 signer 권한을 계산하므로
§0.2와 정합한다. Recovery 상실 시의 별도 social recovery는 **사용자 결정 필요**다.

## 3. Signable 대상과 lifetime

### 3.1 제안하는 signable 표

| 객체 | signable 여부 | signer identity | lifetime | durable 의미 |
|---|---|---|---|---|
| Genesis | 예, Perpetual | bootstrap OwnerRootKey | Perpetual | anchor가 허용한 최초 root fact |
| AddMember | 예 | OwnerRootKey 또는 predecessor state가 허용한 owner key | LongLived credential, 제안 기본 TTL 7일 | member generation 생성 |
| RemoveMember | 예 | Owner/Recovery policy | LongLived, 제안 기본 TTL 7일 | member tombstone 및 revoke effect |
| ApproveDevice | 예 | Owner policy | LongLived, 제안 기본 TTL 7일 | `(device_id, generation) -> member_id` binding |
| RevokeDevice | 예, threshold typed envelope 필요 | Owner 또는 committed Coordinator quorum | LongLived command; revoke fact 자체는 Perpetual history | device tombstone/revoked generation |
| RotateOwnerKey | 예 | old Owner + Recovery | LongLived | root key epoch 변경 |
| MemberRecord/DeviceBinding projection | 아니오 | 없음 | Perpetual projection, 현재 상태는 revision-bound | action log의 결정적 결과; 독립 권위 아님 |
| Revoke evidence/commit proof | 별도 결정 | action signer 또는 ControlStore proof signer | Evidence | “그 index에서 revoke가 commit됨”을 보존 |

LongLived는 `issued_at`의 60초 skew를 적용하지 않고 `now < expires_at`만 적용한다는
기존 규범을 따른다. 7일은 권고 기본값이지 현 proto에 이미 있는 값이 아니다. action이
만료되면 서명이 맞아도 새 commit을 만들지 않는다. 이미 commit된 revoke·remove의
효력은 action credential의 `expires_at`로 끝나지 않는다.

### 3.2 lifetime 선택

| 선택지 | trade-off | 권고 |
|---|---|---|
| Perpetual action | offline queue와 장기 장애에 강하지만, 탈취된 오래된 Add/Approve를 재사용할 위험이 큼 | mutation command에는 부적절 |
| LongLived action + operation id | 며칠의 장애를 견디고, expiry와 idempotency로 replay 범위를 제한함 | **권고**. 7일은 사용자 확인 후 확정 |
| ShortLived 60초 action | replay 창이 작음 | offline 운영과 clock skew에 취약하고 정상 queue가 불필요하게 거부될 수 있음 |
| Evidence로만 저장 | 과거 사실을 보존하기 좋음 | authorization command를 evidence로 취급하면 현재 state 변경 guard가 사라짐 |

§0.2와의 정합성은 “서명이 맞으면 곧바로 권한”이 아니라 schema→canonical→signature→
signer authority→lifetime→replay 순서를 모두 통과해야 action을 apply한다는 데 있다.
`MemberRecord`를 Evidence처럼 저장하더라도 현재 active 권한으로 사용하지 않는다.

### 3.3 Replay와 operation identity

각 mutation은 `(team_id, action_id, domain_tag, signer_key_id)`를 idempotency key로
갖는다. 같은 key와 byte-identical semantic payload가 다시 오면 기존 committed 결과를
반환한다. 같은 key의 다른 payload·signature·subject·generation이면 `REPLAY_CONFLICT`로
거부하며 기존 state를 바꾸지 않는다. 장수명 action이라고 replay 검사를 생략하지 않는다.

## 4. Member 상태기계 — 이 표가 계약이다

현재 `docs/protocol/state-machines.md`에는 Member 표가 없다. 아래는 v0 검토용 제안이며,
확정 전에는 구현자가 이 표 밖의 Member transition을 추가해서는 안 된다. 표의 형식은
기존 6열 `statetable` 계약을 따른다.

> ★ 아래 표는 **제안**이지 정본이 아니다. 정본 상태 전이표는
> `docs/protocol/state-machines.md` 하나뿐이므로, 여기서는 예약 마커인
> ```statetable``` 펜스를 의도적으로 쓰지 않는다(`scripts/check_docs.py`
> §4 상태 전이표 중복 검사).

```text
machine: Member
from | to | trigger | guard | effect | durability
(none) | PENDING | ADD_MEMBER_COMMITTED | action 서명·signer·lifetime·replay 검증 통과 AND bootstrap/owner policy 허용 AND tombstone 없음 | member generation 생성; device 없음 | COMMITTED
PENDING | ACTIVE | MEMBER_ACTIVATED | activation action이 predecessor state의 owner/recovery policy에 허용됨 AND same generation | member을 authorization 대상으로 공개 | COMMITTED
PENDING | REMOVED | MEMBER_REMOVE_COMMITTED | RemoveMember가 검증됨 AND generation 일치 | 영구 tombstone 기록; device binding 없음 | COMMITTED
ACTIVE | SUSPENDED | MEMBER_SUSPEND_COMMITTED | owner/recovery policy 검증됨 AND generation 일치 | 신규 device·grant·lease 발급 차단; 기존 권한 재검증 대상 | COMMITTED
SUSPENDED | ACTIVE | MEMBER_REINSTATED | reinstatement action 검증됨 AND 같은 generation AND tombstone 없음 | 신규 발급 허용 재개 | COMMITTED
ACTIVE | REVOKED | MEMBER_REVOKE_COMMITTED | revoke action 검증됨 AND generation 일치 | 모든 device binding revoke; grant/lease 재검증 실패; tombstone 유지 | COMMITTED
SUSPENDED | REVOKED | MEMBER_REVOKE_COMMITTED | revoke action 검증됨 AND generation 일치 | 모든 device binding revoke; tombstone 유지 | COMMITTED
PENDING | REVOKED | MEMBER_REVOKE_COMMITTED | revoke action 검증됨 AND generation 일치 | 발급된 적 없는 device도 future binding 금지; tombstone 유지 | COMMITTED
ACTIVE | REMOVED | MEMBER_REMOVE_COMMITTED | remove action 검증됨 AND generation 일치 AND revoke effect가 먼저 기록됨 | 영구 tombstone; historical evidence만 조회 허용 | COMMITTED
SUSPENDED | REMOVED | MEMBER_REMOVE_COMMITTED | remove action 검증됨 AND generation 일치 AND revoke effect가 먼저 기록됨 | 영구 tombstone; historical evidence만 조회 허용 | COMMITTED
REVOKED | REMOVED | MEMBER_REMOVE_COMMITTED | remove action 검증됨 AND same generation | tombstone을 삭제하지 않고 상태만 terminal로 표시 | COMMITTED
```

표에 없는 `REVOKED -> ACTIVE`, `REMOVED -> ACTIVE`, ID 재사용, local cache만으로의
activation은 금지한다. 어떤 guard라도 실패하면 `UNKNOWN_SIGNER`, `INVALID_SIGNATURE`,
`SCHEMA_TOO_NEW`, `EXPIRED`, `REPLAY_CONFLICT`, `STALE_REVISION`, `INVALID_TRANSITION`
중 구체 사유를 반환하고 action·projection·tombstone을 전혀 쓰지 않는다. `SUSPENDED`의
정확한 trigger와 긴급 복구 절차는 현재 proto에 없어 **사용자 결정 필요**다.

### 4.1 PENDING을 둘지 여부

| 선택지 | trade-off |
|---|---|
| AddMember 즉시 ACTIVE | action 수가 적고 운영이 단순함; add와 activation을 분리할 수 없어 잘못 발급된 member의 노출 시간이 늘어남 |
| AddMember -> PENDING -> ACTIVE | 승인·검증 단계를 분리하고 device 없이 identity를 준비할 수 있음; action/상태가 늘고 pending cleanup이 필요 |
| PENDING 없이 ACTIVE/REVOKED만 유지 | 표가 작지만 suspend/invite/activation을 표현하지 못함 |

**권고: PENDING을 유지한다.** AddMember commit은 identity namespace를 예약할 뿐
authorization subject를 ACTIVE로 만들지 않는다. 다만 현재 요구사항이 “Owner가 AddMember
즉시 사용 가능 member를 만든다”는 것이라면 이 선택은 바뀌어야 하며, **사용자 결정 필요**다.

어느 lifecycle을 택하든 §0.2의 경계는 같다. `ACTIVE` 공개는 검증된 activation action과
predecessor state guard를 통과한 뒤의 projection effect일 뿐이며, raw `AddMember`의
`role`/`member_id`를 검증 전에 읽어 ACTIVE로 만들 수 없다.

## 5. Revocation, remove, tombstone

### 5.1 효력 시점

revocation은 네 시점을 혼동하지 않는다.

```text
received      네트워크에 도착한 시각. 권한 변화 없음
verified      서명·signer·lifetime·replay를 통과한 시각. 아직 authority 아님
committed     ControlStore가 index/term을 확정한 시점. 여기서부터 효력 발생
observed      resolver/agent가 해당 commit을 읽은 시점. stale이면 권한 사용 금지
```

`effective_index`는 `COMMITTED` index이며 wall-clock `revoked_at`만으로 판단하지 않는다.
resolver가 `as_of_index >= effective_index`인 view를 확보하면 해당 member/device를
REVOKED로 처리한다. `as_of_index`가 그보다 낮거나 unknown이면 ACTIVE로 추정하지 않고
`STALE_REVISION`으로 거부한다.

member revoke의 권고 effect는 다음과 같다.

- 같은 transaction에서 member 상태, 모든 known device binding의 revoke generation,
  member/device tombstone을 commit한다.
- 새 Grant/Lease/ApproveDevice는 즉시 거부한다.
- 이미 발급된 signed Grant/Lease는 암호학적으로 “없었던 것”이 되지 않는다. 그러나
  authorization consumer와 agent가 current committed membership/revocation을 확인하는
  지점에서 fail-closed한다. 외부 시스템이 이미 수행한 side effect를 되돌린다고
  보장하지 않는다.
- revoke 통지를 놓친 cache는 resolver의 revision pinning으로 발견되어야 하며,
  cache를 ACTIVE로 계속 제공하지 않는다.

이것은 `CLAUDE.md` §0.2와 §0.4의 정합성도 지킨다. “서명이 있는 오래된 Grant”라는
필드만 보고 현재 권한을 부여하지 않고, 외부 side effect를 fencing으로 완전히 막을 수
있다고 과장하지 않는다.

### 5.2 REVOKED와 REMOVED

| 상태 | 의미 | 재활성화 | tombstone |
|---|---|---|---|
| `REVOKED` | credential과 현재 authorization을 폐기했으나 historical identity는 남음 | 금지; 새 generation/새 member 절차 필요 | 유지 |
| `REMOVED` | 관리상 directory에서 더 이상 active subject가 아님 | 금지; 같은 ID 재사용 금지 | 영구 유지 권고 |

Remove는 revoke를 우회하는 삭제가 아니다. ACTIVE/SUSPENDED에서 REMOVED로 가려면
revoke effect가 같은 commit에 포함되거나 선행 commit으로 존재해야 한다. historical
evidence는 `member_id`, generation, 상태 이력과 함께 읽을 수 있지만 현재 signer
authority로 읽을 수 없다.

### 5.3 tombstone 보존

| 선택지 | trade-off |
|---|---|
| tombstone 영구 보존 | ID replay·delayed action·old device resurrection을 막음; 저장 공간이 증가 |
| 시간 기반 GC | 저장 공간 절약; 만료된 old action이 다시 유효해지거나 ID가 재사용될 위험 |
| signed checkpoint 이후 GC | 공간을 줄일 수 있음; checkpoint anchor·retention·offline node 복구 규범이 추가됨 |

**권고: 영구 보존.** `member_id`, `device_id`, generation은 재사용하지 않는다. 법적/운영적
삭제 요구가 생기면 “ID tombstone의 최소 보존 proof”를 별도 규범으로 정한 뒤에만 GC한다.
그 전까지 GC된 ID를 다시 AddMember/ApproveDevice에 허용하지 않는다.

이것은 §0.2의 재생 방지 적용이다. 과거 서명이 다시 들어왔을 때 tombstone이 없으면
검증자는 서명만 맞는 지연 action을 새 identity로 오인할 수 있다. tombstone과 generation을
검증된 predecessor state로 확인하지 못하면 `REPLAY_CONFLICT`로 닫는다.

## 6. Authoritative device→member resolver

### 6.1 계약

raw `DeviceRecord.member_id`는 hint일 수 있지만, signed/committed provenance와 revision이
검증되기 전에는 권한 판단에 쓰지 않는다.

```text
입력:
  team_id
  device_id
  optional presented_public_key 또는 device certificate digest
  required_as_of_index (권한 판단이면 필수)
  consistency = LINEARIZABLE 또는 명시된 BOUNDED_STALE

성공:
  ResolvedDevice {
    team_id,
    device_id,
    device_generation,
    member_id,
    member_generation,
    member_state,
    device_state,
    public_key_digest,
    binding_revision,
    as_of_index
  }

실패:
  UNKNOWN_DEVICE | UNKNOWN_MEMBER | KEY_MISMATCH | REVOKED | REMOVED |
  SUSPENDED | STALE_REVISION | AMBIGUOUS_BINDING | CORRUPT_DIRECTORY
```

resolver는 다음을 보장해야 한다.

1. `as_of_index`가 requested index보다 작으면 결과 대신 `STALE_REVISION`을 반환한다.
2. 동일 `(team_id, device_id, generation)`에 서로 다른 active member가 보이면
   `AMBIGUOUS_BINDING`으로 전체 실패한다.
3. presented public key가 있으면 committed binding의 key digest와 대조하고, 불일치하면
   `KEY_MISMATCH`다.
4. member/device가 `REVOKED`, `REMOVED`, `SUSPENDED`면 ACTIVE로 정규화하지 않는다.
5. resolver 내부에서 action signature가 재검증되지 않은 local cache row를 권위로 쓰지
   않는다.

### 6.2 resolver 선택

| 선택지 | trade-off | 권고 |
|---|---|---|
| committed mapping resolver | log/projection과 같은 authority를 사용하고 index pinning이 쉬움 | **권고**. online authorization의 기본 경로 |
| certificate-chain resolver | offline 검증과 portable credential에 강함 | chain distribution·revocation·rotation이 추가되어 현재 공백이 큼 |
| signed snapshot + local cache | 빠른 read와 장애 내성이 좋음 | snapshot freshness와 rollback proof가 없으면 stale ACTIVE를 제공할 위험 |

권고안은 signed snapshot을 금지하는 것이 아니라, snapshot이 있더라도 committed index와
log/snapshot proof를 검증한 뒤 committed mapping으로 취급하라는 뜻이다. `as_of_index`
없이 “현재”를 반환하는 API는 권한용으로 만들지 않는다.

§0.2와의 정합성은 resolver가 raw `member_id`를 믿지 않고, 검증된 binding·key digest·state와
revision을 모두 확인한 뒤에만 결과를 내보낸다는 데 있다. 하나라도 확인할 수 없으면
`UNKNOWN`이나 `STALE`을 ACTIVE로 바꾸지 않는다.

## 7. ControlStore consistency

### 7.1 write 계약

membership mutation은 `Verified`가 되었다고 바로 local state를 바꾸지 않는다.
검증된 action을 ControlStore에 propose하고, 필요한 quorum이 정한 `COMMITTED` index/term,
prev hash, action idempotency를 원자적으로 확정한 뒤 projection을 갱신한다. `DURABLE`
SQLite row나 process memory cache만으로 `COMMITTED`를 가장하지 않는다.

| 읽기 방식 | 허용 용도 | fail-closed 조건 |
|---|---|---|
| Linearizable | grant/lease 발급, device admission, revoke 판정 | quorum/read-index를 얻지 못하면 `READ_UNAVAILABLE`; stale 값 반환 금지 |
| Bounded stale + `as_of_index` | UI, historical evidence, 비권한 감사 조회 | requested index 미달·watch gap·snapshot reset이면 `STALE_REVISION` |
| Local cache only | 표시·진단 | authorization/resolver 결과로 사용 금지 |

### 7.2 consistency 선택

| 선택지 | trade-off |
|---|---|
| 모든 resolver read linearizable | revoke race를 가장 잘 막음; quorum 장애 시 가용성 저하 |
| bounded-stale 기본 + fence index | 장애 시 읽기 가용성이 좋음; caller가 minimum index를 빠뜨리면 위험 |
| local cache 기본 | 빠르고 단순함; revoked member/device를 active로 잘못 허용할 수 있음 |

**권고: 권한용은 linearizable 기본, historical/UI만 명시적 bounded-stale.** 이는 stale
read를 성공으로 바꾸지 않고, `CLAUDE.md` §0.2의 “검증 전 신뢰 금지”를 현재 revision에도
적용하는 선택이다. watch cursor가 무효화되거나 gap/reset이면 전체 directory를 필요한
index까지 재동기화하고, 그 전에는 resolver를 닫는다.

## 8. Restart와 revalidation

### 8.1 재시작 선택

| 선택지 | trade-off | 권고 |
|---|---|---|
| full log replay | 의미가 가장 단순하고 누락 검출이 명확함; log가 길어질수록 startup 비용 증가 | 작은 팀/초기 구현의 기준선 |
| verified snapshot + tail replay | 빠른 restart; snapshot signer, last index/term, previous hash, schema/root binding이 필요 | **운영 권고**. full replay와 동일 결과를 byte/semantic 비교해야 함 |
| lazy revalidation | 빠른 boot; 첫 요청이 stale cache와 revoke race를 만남 | 권한 directory에는 부적합 |

**권고: snapshot+tail을 최적화로 허용하되 full replay를 normative reference로 둔다.**
snapshot은 `COMMITTED` index/term, action-log digest/prev hash, team/root identity, schema
version ceiling을 포함해야 하며, 그 proof를 검증하지 못하면 snapshot을 버리고 full
replay하거나 directory를 닫는다.

이 선택은 §0.2 및 DoD-50~53의 load 경계와 정합한다. snapshot cache가 “이미 검증됨”이라는
표시를 갖고 있어도 startup에서 다시 signature·signer-at-index·commit continuity를
확인하기 전에는 `Verified`나 권한 입력으로 노출하지 않는다.

### 8.2 재시작 절차

```text
1. OOB bootstrap anchor와 schema ceiling을 먼저 load
2. Genesis/root bundle을 검증
3. snapshot이 있으면 signature/proof/index/root/schema를 검증
4. snapshot 이후 tail action을 commit 순서로 decode
5. 각 action에 대해 domain -> schema -> canonical -> signature
   -> predecessor signer authority -> lifetime -> replay를 재검증
6. action을 deterministic state machine에 apply
7. generation monotonicity, tombstone, device uniqueness, prev hash를 검증
8. current ControlStore read-index와 snapshot/tail의 마지막 committed index를 대조
9. 전부 성공한 경우에만 RevalidatedDirectory를 resolver에 공개
```

durable data는 `Verified<M>`가 아니다. load된 raw bytes가 당시 write 시점에 검증되었다는
메타데이터가 있어도, key registry·schema·commit continuity·tombstone을 현재 startup에서
다시 확인해야 한다. 따라서 DoD-50~53의 durable binding과 동일하게 load 결과는 raw
binding/observation이며, 상위 authorization은 재검증된 타입만 받는다.

어느 한 action이라도 signature, signer-at-that-index, schema, lifetime, replay, sequence,
snapshot proof 검증에 실패하면 부분 projection을 공개하지 않는다. 마지막으로 성공한
member만 살려서 계속하는 것은 revoke 누락·split brain을 숨기므로 `CORRUPT_DIRECTORY`로
fail-closed한다. 복구는 trusted snapshot 재선택 또는 전체 log 재검증 후에만 가능하다.

### 8.3 historical signer와 key rotation

현재 key가 revoked되었다는 이유만으로 rotation 이전에 정상적으로 committed된 action을
무효화하지 않는다. 각 action의 signer authorization은 **그 action 직전 projection**에서
판정한다. 반대로 현재 action이 old key로 서명되었고 predecessor state에서 이미 old key가
폐기되었다면 signature 자체가 맞아도 `UNKNOWN_SIGNER`/`KEY_NOT_AUTHORIZED`로 reject한다.

이 규칙이 없으면 restart 시 현재 keyring만 보고 과거를 모두 무효화하거나, revoke된 old
key로 새 action을 허용하는 두 종류의 보안 오류가 생긴다.

## 9. Fail-closed 결과와 negative 계약

최소한 다음 경우는 모두 state 불변으로 거부해야 한다.

- `membership`/`member-add` domain 혼용, 다른 action domain으로 cross-message replay
- schema_version 초과, unknown required field, signature field가 90이 아님
- signer_id/key_id가 unknown이거나 predecessor state에서 권한 없음
- canonical 재구성·Ed25519 불일치, public key와 device binding 불일치
- expired action, duplicate action id의 payload conflict, generation rollback
- committed index gap, prev hash 불일치, snapshot root/team mismatch
- 같은 device가 두 active member에 binding, tombstone ID 재사용
- resolver의 stale read, watch gap/reset, quorum/read-index 미확보
- restart 중 일부 action만 검증된 projection, corrupt durable row

오류를 warning으로 낮추어 ACTIVE/authorized로 진행하지 않는다. 구체적인 `VerifyOutcome`
enum 값과 신규 membership error enum은 proto 설계 때 확정해야 하며, 지금 임의의 기존
값을 재사용하지 않는다.

## 10. 결정 목록

### 이 초안의 권고

| 항목 | 권고 |
|---|---|
| authority | OOB anchor → Genesis → committed action log → deterministic projection |
| signable | mutation action만; record는 projection |
| AddMember tag | `gputeer/v1/member-add` |
| root | OOB-pinned OwnerRootKey + 별도 RecoveryRootKey |
| first signer | bootstrap OwnerRootKey; AddMember가 자기 signer를 bootstrap하지 않음 |
| root rotation | old Owner + Recovery 2인 승인 |
| lifetime | mutation은 LongLived, 기본 7일 제안; revoke fact는 committed historical fact |
| member state | PENDING → ACTIVE를 포함한 표의 전이만 허용 |
| revoke | committed index/term부터 효력; known device cascade; grant/lease 재검증 실패 |
| tombstone | member/device ID와 generation 영구 보존, 재사용 금지 |
| resolver | committed mapping, 권한용 linearizable, minimum index pinning |
| restart | normative full replay, 운영 최적화 signed snapshot + tail replay |
| load type | `Verified` 복원 금지; 재검증 완료 후에만 directory 공개 |

### 사용자 결정 필요

1. 현재 proto 주석과 signing 표 중 AddMember tag를 실제로 `member-add`로 확정할지.
2. `PENDING -> ACTIVE` 2단계 lifecycle이 필요한지, AddMember 즉시 ACTIVE인지.
3. mutation 기본 TTL을 7일로 할지, action 종류별 TTL을 둘지.
4. Owner 단독 `AddMember`/`ApproveDevice`를 유지할지, member self-approval 또는 threshold를
   도입할지.
5. `RevokeDevice`의 최종 threshold 방식과 Coordinator set의 최초 trust anchor.
6. Recovery key를 잃었을 때의 social recovery/교체 절차.
7. tombstone의 법적 삭제 요구가 있을 때 사용할 cryptographic retention proof.
8. `MemberRecord` signed snapshot을 별도 signable message로 만들지, action projection만
   유지할지.
9. 실제 proto field 번호, schema version, `VerifyOutcome`/resolver error의 이름과 wire 형식.

## 11. 구현 전 게이트와 범위 경계

이 초안만으로는 membership 구현을 시작할 수 없다. 다음을 별도 계약 작업으로 확정해야
한다.

- schema_version·signer_id·key_id·action_id·lifetime field와 canonical field 90
- `Signable` 및 단일/threshold `Verified` 타입의 정확한 경계
- bootstrap bundle/Genesis의 배포·pin·rotation 형식
- ControlStore의 committed index/term, snapshot proof, read-index API
- 위 Member statetable의 `docs/protocol/state-machines.md` 반영
- Add/Remove/Approve/Revoke의 domain tag 표·reference vectors·negative vectors

그 전까지 현재 `AddMember`/`RemoveMember` action은 canonical/domain 조각으로만 취급하고,
`Verified<AddMember>`나 authoritative member resolver가 이미 있다고 쓰지 않는다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-24 | membership 규범 검토용 초안 v0 최초 작성 |
