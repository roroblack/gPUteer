# ADR-028 · 메시지별 domain_tag 분리

- **상태:** 채택
- **날짜:** 2026-08-16
- **관련:** `docs/protocol/signing.md` §5 · `proto/control.proto` ·
  `docs/evidence/DoD-06_domain_tag_충돌.md` ·
  `crates/protocol/tests/t1b_grant_and_control.rs`

## 배경

`signing.md` §5 는 domain_tag 17종을 등록하면서 **세 개의 tag 를 여러 메시지가
공유**하게 두었다.

```text
gputeer/v1/membership   AddMember · RemoveMember · ApproveDevice
                        · RevokeDevice · ChangeCoordinatorSet · RotateOwnerKey
gputeer/v1/policy       UpdatePolicy
gputeer/v1/quarantine   QuarantineDevice · ReleaseQuarantine
```

같은 절의 규범은 이렇게 적혀 있다.

> 새 서명 대상 메시지를 추가할 때는 **반드시 새 domain_tag를 이 표에 등록해야 한다(MUST).**

**표가 자기 규범을 어기고 있었다.**

### 왜 문제인가 — 실측

`domain_tag` 의 존재 이유는 §5 가 스스로 밝힌다.

> 한 문맥의 서명을 다른 문맥에서 검증하면 domain_tag가 달라 **반드시 실패한다.**

tag 를 공유하면 그 방어가 사라지고, **canonical 차이만이 유일한 방어**가 된다.
그런데 canonical 은 규칙 b(기본값 생략) 때문에 **필드를 비우면 짧아진다.**
공격자는 어느 필드를 비울지 고를 수 있다.

2026-08-16 실측 (`tools/canonical` 참조 구현으로 전수 대조):

| 충돌 쌍 | 공통 필드 | canonical |
|---|---|---|
| `AddMember` ↔ `RemoveMember` | `[1]` | **28바이트 동일** |
| `ApproveDevice` ↔ `RemoveMember` | `[1]` | **28바이트 동일** |
| `ApproveDevice` ↔ `RevokeDevice` | `[1,2]` | **56바이트 동일** |
| `RemoveMember` ↔ `RevokeDevice` | `[1]` | **28바이트 동일** |
| `QuarantineDevice` ↔ `ReleaseQuarantine` | `[1]` | **동일** |

`sig_input = domain_tag ‖ schema_version ‖ len ‖ canonical` 이므로,
**tag 가 같고 canonical 이 같으면 sig_input 이 같고, 곧 서명이 그대로 통과한다.**

### 구체적 공격

```text
소유자가 RemoveMember{member_id: X} 에 서명한다
  -> 그 서명 바이트를 RevokeDevice{device_id: X} 에 그대로 붙인다
  -> 검증 통과. 멤버 탈퇴가 **기기 폐기**로 바뀐다

Coordinator 들이 QuarantineDevice{device_id: X} 에 m-of-n 서명한다
  -> 그 서명들을 ReleaseQuarantine{device_id: X} 에 붙인다
  -> ★ **격리 판정이 격리 해제로 바뀐다**
```

두 번째가 특히 위험하다. 격리는 위험 신호에 대한 대응인데,
그 판정 서명이 **정확히 반대 동작**의 승인으로 재사용된다.

## 결정

**서명 대상 메시지마다 고유한 `domain_tag` 를 갖는다.**

`membership` · `policy` · `quarantine` 세 tag 를 9개로 분리한다.

```text
gputeer/v1/member-add            AddMember
gputeer/v1/member-remove         RemoveMember
gputeer/v1/device-approve        ApproveDevice
gputeer/v1/device-revoke         RevokeDevice
gputeer/v1/coordinator-set       ChangeCoordinatorSet
gputeer/v1/owner-key-rotate      RotateOwnerKey
gputeer/v1/policy-update         UpdatePolicy
gputeer/v1/quarantine-device     QuarantineDevice
gputeer/v1/quarantine-release    ReleaseQuarantine
```

domain_tag 총수: **17 → 23.**

## 근거

- **§5 의 기존 MUST 를 지키는 최소 변경이다.** 새 규칙을 만드는 것이 아니라
  이미 있는 규칙을 표에 적용하는 것이다.
- **canonical 충돌에 의존하지 않는다.** 필드 구성이 우연히 같아져도 안전하다.
  앞으로 필드가 추가·삭제되어도 이 방어는 흔들리지 않는다.
- **`schema_version` 을 올리지 않아도 된다.** `.proto` 를 바꾸지 않으므로
  §7.3 의 대상이 아니다. `SCHEMA_FINGERPRINT` 도 변하지 않는다.
- **배포 전이다.** 실행 중인 노드가 없어 호환성 부담이 없다.
  지금이 가장 싼 시점이다.

### 왜 지금 발견되었나

`ToCanonicalFields` 를 구현하며 **"같은 tag 를 쓰는 메시지들이 서로 다른
canonical 을 내는가"** 를 테스트로 물었기 때문이다.
그 질문을 하지 않았다면 코드는 정상 동작했고, 배포 후에야 드러났을 것이다.

## 대안과 기각 사유

| 대안 | 기각 사유 |
|---|---|
| **`sig_input` 에 메시지 타입 판별자를 추가** (`domain_tag ‖ type_name ‖ …`) | 더 일반적인 해법이고 tag 를 늘리지 않아도 되지만, **`sig_input` 레이아웃 변경**이라 §4 를 고쳐야 하고 기존 벡터 36건이 전부 바뀐다. 회귀 범위가 훨씬 크다. 문제는 tag 공유이지 레이아웃이 아니다 |
| **canonical 에 메시지 타입 필드를 넣는다** | `.proto` 변경 → `schema_version` 상향 필요(§7.3). 스키마를 건드리지 않고 해결할 수 있는데 건드릴 이유가 없다 |
| **필드 구성이 겹치지 않도록 field number 를 재배치** | 우연에 기대는 방어다. 필드가 추가되면 다시 깨진다. 그리고 field number 재사용 금지(§7.3)와 충돌한다 |
| **그대로 두고 "검증자가 문맥을 안다"고 가정** | ★ `CLAUDE.md` §0.4 위반 — 강제할 수 없는 것을 보장으로 선언하는 것이다. 검증자가 어느 `ControlAction` 필드에서 왔는지 안다는 보장이 없다 |

## 결과

### 가능해지는 것

- 서명 재사용이 **tag 수준에서** 막힌다. canonical 우연에 기대지 않는다.
- 새 제어 동작을 추가할 때 "tag 를 새로 만들어야 한다" 가 자명해진다.

### 포기하는 것

- domain_tag 표가 길어진다 (17 → 23). 관리 대상이 늘어난다.
- tag 이름이 메시지 이름과 1:1 이 되어 **메시지 이름을 바꾸면 tag 도 함께 봐야 한다.**
  (tag 는 와이어 상수이므로 실제로는 이름이 바뀌어도 tag 는 고정한다.)

### 바꿔야 하는 것

```text
docs/protocol/signing.md §5           표 갱신 + §5.1 정정
crates/protocol/src/canonical.rs      Domain enum 6종 추가
tools/canonical/reference_canonical.py DOMAIN_TAGS + MESSAGE_DOMAIN 갱신
tests/vectors/canonical_v1.json        sig_input 재생성 (canonical 은 불변)
crates/protocol/tests/                 회귀 방지 테스트 추가
```

★ **canonical bytes 는 하나도 바뀌지 않는다.** tag 는 `sig_input` 에만 들어간다.

## 되돌리는 조건

domain_tag 가 50종을 넘어 관리가 실질적 부담이 되고,
그때까지 `sig_input` 레이아웃 변경(대안 1)의 회귀 비용이 감당 가능해진 경우.

그 경우에도 **"tag 를 다시 공유한다" 로 되돌리지는 않는다** — 판별자를 도입하는
방향으로만 간다. 공유는 이 ADR 이 막으려는 바로 그 상태다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-16 | 최초 작성. T1b 구현 중 canonical 충돌 5쌍 실측으로 발견 |
