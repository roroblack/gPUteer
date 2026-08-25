# Membership 규범 초안 뼈대 조사

**조사일:** 2026-08-24  
**성격:** 규범 초안이 다뤄야 할 범위와 미결정 사항을 고정하는 조사 문서. 이 문서 자체는
membership 동작을 결정하지 않는다.

## 0. 핵심 발견

1. 현재 proto에는 `AddMember`, `RemoveMember`, `ApproveDevice`, `RevokeDevice`와
   `DeviceRecord.member_id`가 있지만, 서명 가능한 member 상태 레코드나
   device→member authoritative resolver의 입력·출력 계약은 없다. `ControlQuery`에도
   member 조회가 없고 `ControlState`에도 member 목록이 없다
   (`proto/control.proto:455-474`, `proto/control.proto:524-534`).
2. 서명 규범의 일반 골격은 이미 강하게 정해져 있다. 그러나 membership의 기존
   `AddMember.owner_signature` 주석은 `gputeer/v1/membership`을 가리키고
   (`proto/control.proto:288-293`), 규범 표는 `gputeer/v1/member-add`를 요구한다
   (`docs/protocol/signing.md:249-265`). 이 불일치는 구현 전에 해소해야 한다.
3. `owner_signature`에는 signer identity field가 없고, 검증 순서의 “팀 멤버십/승인으로
   signer identity 확인” 규칙만 있다 (`docs/protocol/signing.md:514-528`). Member를
   추가하는 서명 자체를 기존 member 조회에 의존시키면 순환한다. Owner/Recovery key의
   존재는 선언돼 있지만 (`docs/protocol/signing.md:729-736`), 그 최초 trust anchor인
   Genesis/초대 bundle의 proto 메시지는 없다 (`docs/protocol/signing.md:336-348`).
4. Node 상태 기계는 있지만 Member 상태 기계는 없다. 따라서 `RemoveMember`가 현재
   조회에서 언제부터 효력을 갖는지, device revoke를 member revoke와 어떻게 연쇄할지,
   재시작 후 어떤 레코드를 다시 검증할지가 모두 설계 결정이다
   (`docs/protocol/state-machines.md:40-81`, `docs/protocol/state-machines.md:335-341`).

## 1. 현황: 이미 정해진 것과 비어 있는 것

### 1.1 공통 규범으로 이미 정해진 것

| 항목 | 현재 규범 | 근거 |
|---|---|---|
| 직렬화/서명 | canonical protobuf, BLAKE3-256, Ed25519; signature field는 90 | `docs/protocol/signing.md:34-44`, `docs/protocol/signing.md:48-65`; `proto/README.md:38-60` |
| 서명 입력 | `domain_tag || schema_version || canonical 길이 || canonical` | `docs/protocol/signing.md:224-241` |
| domain 분리 | 메시지마다 독립 tag를 등록하며 다른 문맥의 서명 재사용을 금지 | `docs/protocol/signing.md:245-298`; 과거 membership tag 충돌 사례는 `docs/protocol/signing.md:300-334` |
| 검증 순서 | schema → canonical → signature → signer identity → time → replay 후에만 payload를 신뢰 | `docs/protocol/signing.md:514-530` |
| 사전 검증 예외 | `signer_id`는 키 조회 hint로만 먼저 읽을 수 있고, 틀리면 `UnknownSigner`/`InvalidSignature`로 실패 | `docs/protocol/signing.md:532-555` |
| schema 진화 | 서명 대상은 schema_version을 가져야 하며, 모르는 버전은 `SCHEMA_TOO_NEW`로 fail-closed | `docs/protocol/signing.md:415-438`; `proto/common.proto:389-402` |
| lifetime 분리 | 단수명은 issued/expires, 60초 skew와 replay; 장수명은 issued_at skew 없이 expires; Evidence는 만료 없음 | `docs/protocol/signing.md:562-582` |
| Evidence | 과거 사실은 만료시키지 않으며 소비자가 신선도를 판단; `observed_at` 노출 요구 | `docs/protocol/signing.md:595-625` |
| ControlStore 보증 | membership 관련 action은 COMMITTED가 필요하고 SingleNodeStore는 그 보증을 제공하지 않음 | `proto/control.proto:238-286`; `docs/protocol/state-machines.md:18-35` |
| 읽기/구독 | linearizable/bounded/stale read, read index, cursor와 snapshot reset이 정의돼 있음 | `proto/control.proto:128-188` |

### 1.2 membership에 대해 현재 있는 것

| 현재 조각 | 이미 말하는 것 | 아직 말하지 않는 것 |
|---|---|---|
| `AddMember` | `member_id`, `public_key`, `role`, Owner 서명 | schema_version, owner key의 식별자, member 상태, 생성 시각/수명, revocation 정보 (`proto/control.proto:288-293`) |
| `RemoveMember` | `member_id`, Owner 서명 | 제거 시각, 사유, 순서/epoch, tombstone 보존, 대상 member의 서명된 현재 레코드 (`proto/control.proto:295-298`) |
| `ApproveDevice` | `device_id → member_id`, device public key, peer/key protection/ephemeral 표시, Owner 서명 | device binding의 유효 기간, binding version, 이전 binding 폐기, signer identity (`proto/control.proto:300-308`) |
| `RevokeDevice` | device id, reason, Owner 또는 다중 서명 배열이라는 방향 | signer별 identity, threshold의 구체 방식, 효력 순서/시각 (`proto/control.proto:310-315`; `docs/protocol/signing.md:880-889`) |
| `DeviceRecord` | 조회 결과에 `member_id`, `approved`, `risk_state`가 존재 | 이 값이 어떤 검증된 binding에서 파생됐는지, 어느 committed index 기준인지 (`proto/control.proto:468-474`, `proto/control.proto:524-534`) |
| Control action | membership/policy/quarantine 변경은 COMMITTED action으로 분류 | member 상태 snapshot, member query, resolver API, restart 시 재검증 계약 (`proto/control.proto:238-286`, `proto/control.proto:455-474`) |
| Node 상태 | Owner 서명에 의한 APPROVED/REVOKED, quarantine, key rotation 등 | Member 상태와의 관계 및 device revoke가 member revoke에 미치는 효과 (`docs/protocol/state-machines.md:40-99`) |

추가로, `proto/README.md:47-57`은 모든 서명 대상에 schema_version과 새 domain 등록을
요구하지만, 위 네 membership action에는 schema_version field가 없다. 따라서 현재
proto 조각은 그 자체로 완성된 Signable member 규범이 아니다.

## 2. 기존 Signable 규범의 공통 골격

기존 메시지를 비교하면 새 membership 규범은 다음 순서를 따라야 한다.

1. **무엇을 서명하는지 분리한다.** action(`AddMember`)과 현재 상태 레코드 또는
   certificate를 같은 메시지로 볼지 먼저 정한다. 기존에는 JobManifest와
   ExecutionGrant를 분리해 장수명 요청과 단수명 실행 권한을 구분했다
   (`proto/job.proto:7-20`, `proto/job.proto:41-136`).
2. **schema_version을 canonical의 일부로 둔다.** 서명 field는 90이고 canonical에서
   제외되며, 모든 서명 대상은 지원 버전 밖이면 통과시키지 않는다
   (`proto/README.md:47-60`, `docs/protocol/signing.md:422-438`).
3. **서명자 identity를 payload에 명시한다.** `submitter_device_id`,
   `coordinator_device_id`, `issuing_coordinator_id`, `producer_node_id`,
   `holder_device_id`, `node_id`가 각각 서명자/주체를 가리킨다
   (`proto/job.proto:87-96`, `proto/job.proto:121-136`, `proto/lease.proto:46-66`,
   `proto/artifact.proto:60-68`, `proto/artifact.proto:101-123`,
   `proto/artifact.proto:196-219`).
4. **lifetime class를 고른다.** 단수명 메시지는 `issued_at`, `expires_at`, nonce와
   replay guard를 갖고, 장수명 메시지는 큐 대기 때문에 60초 skew를 적용하지 않는다
   (`proto/job.proto:87-96`, `proto/job.proto:121-136`,
   `docs/protocol/signing.md:562-593`). Evidence는 expiry 대신 관측 시점과 소비 측
   freshness/fencing 판단을 사용한다 (`docs/protocol/signing.md:595-654`).
5. **domain tag를 메시지별로 고정한다.** membership 과거 충돌은 payload가 비어
   있으면 서로 다른 action의 canonical이 같아질 수 있음을 보여줬으므로, canonical이
   우연히 달라 보인다는 이유로 tag를 공유하면 안 된다
   (`docs/protocol/signing.md:300-334`).
6. **검증과 업무 판단을 분리한다.** domain/schema/canonical/서명 검증 후에야
   signer authority, 시간, replay, member 상태 같은 payload 기반 판단을 한다
   (`docs/protocol/signing.md:514-528`). 단, 서명 검증을 위한 공개키 조회 hint만
   예외다 (`docs/protocol/signing.md:532-555`).
7. **현재 권한과 역사적 사실을 분리한다.** `ReplicaAck`는 만료되지 않는 Evidence지만
   “지금 durable”이 아니라 “acked_at 시점에 durable”이라고만 읽어야 한다
   (`proto/artifact.proto:101-123`, `docs/protocol/signing.md:632-654`). Member도
   역사적 생성/바인딩 증거와 현재 ACTIVE/REVOKED 판정을 한 레코드에 섞을지 결정해야 한다.
8. **검증 실패는 조용히 강등하지 않는다.** `UNKNOWN_SIGNER`, `SCHEMA_TOO_NEW`,
   `EXPIRED`, `REPLAY` 등을 명시적 outcome으로 반환한다
   (`proto/common.proto:389-402`, `docs/protocol/signing.md:870-875`).

## 3. membership 규범이 답해야 할 질문

아래에서 **시사됨**은 기존 규범이 방향만 제공한다는 뜻이며, 그 자체로 membership
결정을 확정하지 않는다.

### 3.1 Signable 대상과 domain tag

**현재 근거:** 규범 표는 `AddMember`에 `gputeer/v1/member-add`, `RemoveMember`에
`gputeer/v1/member-remove`, `ApproveDevice`에 `gputeer/v1/device-approve`,
`RevokeDevice`에 `gputeer/v1/device-revoke`를 배정한다
(`docs/protocol/signing.md:249-270`). 반면 `AddMember` proto 주석은 공통
`gputeer/v1/membership`이라고 쓴다 (`proto/control.proto:288-293`).

**설계 결정이 필요함:** “member record”를 무엇으로 서명할지.

- **기존 action을 authoritative record로 사용:** `AddMember`/`RemoveMember`를
  committed log의 사실로 삼고 현재 `MemberRecord`를 projection한다. 기존 tag를
  활용하는 대신 action과 snapshot의 의미가 섞이고, key rotation·role 변경을
  표현하기 어렵다.
- **새 `MemberRecord` 또는 `MemberCertificate`를 추가:** immutable identity와
  상태/발급 정보를 명시하고 별도의 새 tag를 등록한다. 의미가 명확하지만 새
  message, schema fingerprint, root-of-trust, migration이 필요하다.
- **action과 record를 분리:** action은 상태 변경 명령, record/certificate는 검증된
  binding 증거로 각각 독립 tag를 둔다. 재생·projection이 명확하지만 두 객체의
  순서/commit index와 일관성을 규범에 추가해야 한다.

최소한 tag 불일치를 먼저 어느 한쪽으로 통일해야 한다. domain은 메시지마다 독립이어야
한다는 원칙 자체는 이미 정해져 있다 (`docs/protocol/signing.md:295-298`).

### 3.2 누가 서명하는가: signer identity와 root of trust

**현재 근거:** Add/Remove/Approve는 Owner 서명으로 표시되고 RevokeDevice는 Owner
또는 2-of-3 Coordinator 방향이다 (`proto/control.proto:288-315`). 키 종류로 Owner,
Owner Recovery, Device, Coordinator가 정의돼 있으며 Recovery key는 Genesis에
등록한다고 되어 있다 (`docs/protocol/signing.md:729-736`). 검증 후 signer identity를
팀 membership/approval로 확인한다는 일반 규칙도 있다
(`docs/protocol/signing.md:514-528`).

**핵심 공백:** Owner 서명에는 `owner_id`/key id가 없고, Genesis message와 Invite
Bundle message도 proto에 없다 (`docs/protocol/signing.md:336-348`). 따라서 “AddMember를
검증하려면 기존 member/owner registry가 필요하고, 그 registry를 AddMember가 만든다”는
순환을 현재 규범만으로 끊을 수 없다. `signer_id`를 먼저 읽는 예외는 키를 찾기 위한
라우팅 예외일 뿐, trust anchor를 만들어 주지 않는다
(`docs/protocol/signing.md:532-555`).

**설계 결정이 필요함:** 다음 중 root를 선택하고 bootstrap·rotation·recovery를 함께
정해야 한다.

- **오프라인 Owner/Recovery root:** 미리 배포된 Genesis 또는 동등한 trust bundle이
  Owner/Recovery public key를 고정하고, 그 key가 최초 member/device binding을
  서명한다. 순환을 끊지만 Genesis 형식과 key rotation/recovery protocol이 필요하다.
- **서명된 invite/bootstrap bundle:** 설치 시 받은 Owner-signed bundle을 trust
  anchor로 삼고 committed log에 편입한다. 오프라인 bootstrap에는 적합하지만 bundle
  replay·폐기·재발급과 현재 proto에 없는 메시지를 정해야 한다.
- **초기 Coordinator quorum:** 이미 신뢰된 Coordinator 집합이 member를 승인한다.
  운영 중 threshold authority는 가능하지만, Coordinator 집합 자체를 누가 최초로
  승인하는지 다시 Owner/Genesis에 의존한다. 최초 trust anchor로 쓰면 순환이 남는다.

또한 Owner key가 하나인지 여러 key id가 있는지, key rotation 중 구·신 key 병존
기간을 member 서명에도 적용할지 결정해야 한다. 기존 문서는 key rotation의 24시간
병존만 말하고 있다 (`docs/protocol/signing.md:729-741`).

### 3.3 lifetime은 Evidence인가, 별도 credential인가

**현재 근거:** Evidence는 expiry가 없고 관측 시점이 필요하며, freshness는 소비 측이
판단한다 (`docs/protocol/signing.md:595-625`). 장수명 manifest는 `issued_at`/`expires_at`
를 갖되 skew를 적용하지 않는다 (`proto/job.proto:87-96`, `docs/protocol/signing.md:566-585`).
단수명 grant는 60초 TTL과 nonce/replay를 갖는다 (`proto/job.proto:121-136`).

**설계 결정이 필요함:**

- **Member identity를 Evidence/Perpetual로 취급:** 생성·public key·role은 역사적
  사실로 남기고 현재 권한은 별도 state/revocation으로 판정한다. 장기 재검증과
  감사에는 유리하지만, 소비자가 현재 state를 확인하지 않으면 오래된 identity를
  권한으로 오인할 수 있다.
- **장수명 credential:** `issued_at`/`expires_at`를 두고 갱신을 요구한다. 탈취 key의
  유효 창을 줄이지만 offline node, renewal 실패, queue 대기 및 key rotation 정책이
  추가된다.
- **분리형:** member identity는 장수명 Evidence, device binding 또는 authorization은
  단수명 credential로 둔다. 현재 권한을 명확히 할 수 있지만 두 서명의 결합 검증과
  저장/갱신 순서를 정의해야 한다.

어떤 선택이든 `revoked_at`만료와 credential `expires_at`를 같은 의미로 쓰지 말고,
“과거의 서명된 사실”과 “현재 사용 가능”을 구분해야 한다.

### 3.4 member 상태 기계

**현재 근거:** Node에는 `DISCOVERED → ENROLLING → APPROVED`, `APPROVED → REVOKED`,
quarantine/release 등의 표가 있다 (`docs/protocol/state-machines.md:40-81`). 표는
6열(`from`, `to`, `trigger`, `guard`, `effect`, `durability`)과 `COMMITTED`/`DURABLE`/
`LOCAL` 의미를 규정한다 (`docs/protocol/state-machines.md:18-35`). 그러나 문서의
미해결 표에는 Node/Job/Attempt/Lease만 있고 Member machine이 없다
(`docs/protocol/state-machines.md:316-341`).

**설계 결정이 필요함:** 최소한 다음을 표로 고정해야 한다.

- 후보 상태: `PENDING/ACTIVE/SUSPENDED/REVOKED/REMOVED`처럼 승인 전·사용 가능·일시
  중지·폐기·역사 보존을 분리할지, `ACTIVE/REVOKED`만 둘지.
- trigger/guard: Owner 승인, committed bootstrap, device binding, quarantine,
  key rotation, explicit remove, expiry 중 무엇이 전이인지.
- terminal semantics: `REVOKED`와 `REMOVED`를 구분할지, 삭제 대신 tombstone을
  영구 보존할지.
- cascade: member revoke가 모든 device를 즉시 revoke하는지, device별 별도 전이를
  남기는지.
- durability: 각 전이를 `COMMITTED`로 요구할지, 관찰성/위험 신호만 `DURABLE`로
  둘지. 기존 membership action 분류는 COMMITTED 방향이지만 세부 전이표는 없다
  (`proto/control.proto:238-286`).

### 3.5 revocation 표현과 효력

**현재 근거:** `RemoveMember`는 member id만, `RevokeDevice`는 device id와 reason만
담는다 (`proto/control.proto:295-315`). Node의 Owner revoke는 모든 상태에서
`REVOKED`로 가며 모든 lease 무효·artifact 재검증 대상이다
(`docs/protocol/state-machines.md:76-81`). Lease revoke는 별도의 서명된 notice와
fence epoch을 사용하는 선례가 있다 (`proto/lease.proto:198-205`,
`docs/protocol/signing.md:648-654`).

**설계 결정이 필요함:**

- **Committed tombstone/log event:** member/device id, reason, effective committed
  index/epoch를 ControlStore에 남긴다. 현재 판정이 선형화되지만 log/snapshot 보존과
  cache invalidation이 필요하다.
- **서명된 revocation evidence:** 별도 revoke message가 사실을 증명하고 소비자가
  freshness를 판단한다. offline 전달에 강하지만, 최신 revoke 목록을 어디서 얻고
  중복/순서를 어떻게 판정할지 필요하다.
- **record 안의 monotonic generation:** binding/member generation을 올려 낮은
  세대 사용을 거부한다. fence와 잘 맞지만 최초 generation, 다중 writer 합의,
  tombstone 보존 규칙을 정해야 한다.

공통으로 정해야 하는 세부는 효력 시점, 이미 발급된 lease/grant의 처리, member revoke
후 device→member 조회 결과, 복구된 이전 device key의 처리, revoke 철회 가능 여부다.

### 3.6 device→member authoritative resolver

**현재 근거:** `ApproveDevice`가 유일하게 명시적으로 `device_id → member_id`를
연결하고, `DeviceRecord`가 같은 값을 조회 결과로 노출한다
(`proto/control.proto:300-308`, `proto/control.proto:524-534`). Job/Lease/ReplicaAck는
각각 submitter/coordinator/holder/node 같은 device 주체를 담지만 member resolver를
호출하는 필드는 없다 (`proto/job.proto:87-96`, `proto/lease.proto:46-52`,
`proto/artifact.proto:101-122`).

**설계 결정이 필요함:** 규범은 최소한 아래 계약을 명시해야 한다.

```text
입력 후보: device_id, 제시된 device public key 또는 서명, member/device
           binding의 committed snapshot과 read_index, 필요 시 node_id와 시각
출력 후보: Resolved(member_id, binding_generation, member_state, device_state,
                    as_of_index) 또는 명시적 reject reason
```

위 형식은 계약에 필요한 정보의 목록이지 최종 API가 아니다. 선택지는 다음과 같다.

- **Committed mapping resolver:** committed `ApproveDevice`/revoke log를 재생한
  state에서만 mapping을 반환한다. ControlStore와 자연스럽게 결합되고 선형화가
  가능하지만, 서명된 binding record와 store 자체의 trust boundary를 별도로 정해야 한다.
- **Certificate-chain resolver:** device certificate가 member certificate를 가리키고
  각 서명을 검증한 뒤 현재 revocation set을 적용한다. offline 검증에 강하지만
  chain bootstrap, revocation distribution, key rotation이 복잡하다.
- **혼합형:** committed store는 current status/index를, signed binding은 identity와
  public key를 증명한다. 가장 많은 보장을 표현하지만 두 source의 index 불일치와
  fail-closed 규칙을 정의해야 한다.

`DeviceRecord.member_id`는 signed/committed provenance가 정해지기 전까지는
authoritative 입력으로 사용할 수 없다는 점을 명시해야 한다. 서명 검증 전 field를
업무 로직에 사용하지 않는 원칙도 그대로 적용된다 (`docs/protocol/signing.md:514-528`).

### 3.7 재시작 후 재검증

**현재 근거:** ControlStore read에는 `read_index`가 있고, watch cursor가 무효화되면
전체 재동기화하도록 되어 있다 (`proto/control.proto:148-155`, `proto/control.proto:174-188`).
서명 검증은 모르는 schema/signature/signer/time/replay를 모두 fail-closed로 처리한다
(`docs/protocol/signing.md:514-528`). Fence watermark는 재시작 후 stale lease를
받지 않도록 영속화해야 한다는 선례도 있다 (`proto/lease.proto:217-230`). 반면 현재
replay guard는 영속되지 않아 재시작 직후 replay 창이 열린다
(`docs/protocol/signing.md:711-725`).

**설계 결정이 필요함:**

- **부팅 시 full replay/reverify:** committed log 또는 snapshot부터 모든 member/device
  signature와 transition을 다시 검증하고 resolver를 연다. 가장 명확하지만 startup
  비용과 snapshot format이 필요하다.
- **서명된 snapshot + tail replay:** snapshot을 검증한 뒤 이후 log만 재생한다. 빠르지만
  snapshot signing/root와 snapshot이 어느 committed index까지 포함하는지 정해야 한다.
- **lazy reverify:** 요청 시 해당 chain만 확인한다. 부팅은 빠르지만 첫 요청이 느리고,
  cache stale/revocation race를 막는 index pinning이 필수다.

어느 방식을 택해도 다음은 MUST 후보로 검토해야 한다: 검증 완료 전 scheduler에
resolver를 공개하지 않음, `SCHEMA_TOO_NEW`/unknown signer/불완전 snapshot이면 거부,
cache entry에 committed index/term을 붙임, revoke event가 해당 device/member cache를
즉시 무효화함, watch reset이면 전체 재동기화 전까지 새 결정을 내리지 않음.

## 4. 초안 문서에 들어갈 규범 장 구성

1. **용어와 identity graph:** owner key, recovery key, member, device, node,
   coordinator의 구분과 각각의 public key/ID 관계.
2. **Signable 객체 목록:** action, member identity/state record, device binding,
   revocation evidence 중 실제 채택한 객체; 각 schema_version, field 90, domain tag,
   canonical 제외 필드.
3. **Root of trust와 bootstrap:** 최초 trust anchor, signer key lookup, key rotation,
   recovery, coordinator quorum이 개입할 수 있는 시점을 명시. “기존 membership으로
   최초 membership을 검증”하는 순환을 금지/해소하는 규칙을 글과 예제로 고정.
4. **Lifetime/freshness:** 각 객체의 `Evidence`/Perpetual/long-lived/short-lived
   분류, issued/observed/effective/revoked 시각, skew, expiry, replay 여부.
5. **Member/device 상태 기계:** `state-machines.md`의 6열 표 형식과 durability를
   그대로 사용하고 허용되지 않은 전이를 금지.
6. **Revocation:** event/record 표현, committed 효력, cascade, lease/grant 영향,
   tombstone과 rollback 불가성.
7. **Authoritative resolver:** 입력, 검증 순서, snapshot/index 요구, 성공 결과,
   reject outcome, stale/unknown/revoked/ambiguous 상태를 정의.
8. **ControlStore 계약:** write는 어떤 경우 COMMITTED인지, read consistency와
   `as_of_index`를 어떻게 선택하는지, watch gap/reset 시 동작.
9. **Restart/recovery:** snapshot/log/replay guard 복구, cache invalidation, fail-closed
   조건과 “resolver 공개 시점”.
10. **projection 소비 규칙:** `JobRequirements` projection과 `MIRRORED` 판정은 resolver가
    반환한 검증된 member/device 상태와 index만 입력으로 쓰며, raw `member_id` hint를
    권한 판단에 직접 사용하지 않도록 연결.
11. **벡터와 negative cases:** wrong domain, missing/unknown signer, schema too new,
    signature over uncommitted/revoked device, stale snapshot, restart 중 revoke,
    동일 device의 member 재바인딩, cross-message replay.

## 5. 규범 확정 2일 제안

이 분할은 배경에서 제시된 “규범 확정 약 2일”을 하루 단위 산출물로 나눈 제안이다.

### 1일차 — identity, trust, signing, lifetime

- 오전: member/device/node/coordinator/owner 용어와 identity graph를 확정하고,
  action과 상태 record/certificate의 경계를 결정.
- 오후 전반: root-of-trust bootstrap, Owner/Recovery rotation, signer identity field,
  key lookup과 최초 AddMember 검증의 순환 해소를 결정.
- 오후 후반: Signable 대상, schema_version, domain tag, canonical field, lifetime
  class, Evidence/credential 분리를 표로 확정. `AddMember` proto 주석과 signing 표의
  tag 불일치를 이 시점의 blocker로 해소.
- **1일차 종료 기준:** “누가 무엇에 서명하고, 어떤 독립 root로 최초 서명을 검증하며,
  그 서명의 수명이 무엇인가”에 대해 예외 없이 한 문장으로 답할 수 있음.

### 2일차 — state, revocation, resolver, durability/restart

- 오전: Member 상태 표를 작성하고 Add/Approve/Remove/Revoke/Quarantine/key rotation의
  모든 guard/effect/cascade와 COMMITTED·DURABLE 요구를 확정.
- 오후 전반: device→member resolver의 입력·출력·reject reason, ControlStore
  consistency/index, cache invalidation, `JobRequirements`/`MIRRORED` 소비 경로를 확정.
- 오후 후반: restart snapshot/log/reverify 방식, watch reset, revoke race, replay/cache
  복구와 negative test vector 목록을 확정.
- **2일차 종료 기준:** 임의의 device와 임의의 committed index에 대해 resolver가
  같은 결과 또는 명시적 거부를 내고, 재시작·quorum 상실·revocation race에서도
  검증 전 신뢰가 발생하지 않음을 설명할 수 있음.

## 6. 현재 “설계 결정이 필요함” 목록

1. member의 Signable 대상이 기존 action인지, 새 record/certificate인지, 둘 다인지.
2. membership domain tag를 `member-add` 계열로 확정할지와 proto의 `membership` 주석을
   어떻게 제거/정합화할지.
3. Owner/Recovery bootstrap root와 최초 AddMember 검증 경로; coordinator가 root가
   될 수 있는 시점.
4. signer identity field/key id와 Owner key rotation/recovery의 정확한 규칙.
5. member identity/state/binding의 `Evidence`, `Perpetual`, 장수명 credential,
   단수명 credential 분류와 expiry/freshness 규칙.
6. Member 상태·전이·terminal/tombstone semantics 및 device revoke cascade.
7. revocation의 표현, committed 효력 index/epoch, 이미 발급된 lease/grant 처리,
   revoke 철회 가능 여부.
8. resolver의 authoritative 입력, 성공 결과의 index/version, ambiguous/stale/unknown
   reject semantics.
9. ControlStore read consistency와 snapshot/watch reset/cache invalidation 규칙.
10. 재시작 시 full replay, signed snapshot+tail, lazy reverify 중 선택과 resolver 공개
    시점.
11. 위 결정에 따른 proto schema_version/domain tag/message 추가와 migration/negative
    vector 범위.

