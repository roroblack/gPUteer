---
id: DoD-06
claim: "signing.md §5 가 세 domain_tag 를 여러 메시지에 공유시켜 서명 재사용이 가능했음을 실측으로 확인했다. ADR-028 로 tag 를 분리해 시정했고, canonical bytes 는 하나도 바뀌지 않았다. domain 커버리지가 19/23 이 되었다"
status: PASS
commit: e5ac1ff319a3c130c13a29c2dc10beeddb6ffa4c
binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 / prost 0.14 / python 3.12.7"
  note: "라이브러리 크레이트라 실행 바이너리 없음"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1 (§5 표 17 -> 23종, §5.1 재작성)"
  vectors: "tests/vectors/canonical_v1.json (36건, 28 -> 36)"
  schema_fingerprint: "blake3-256 0a34709f5599658f071ed8ac6df7c0ccca441cba89ce640db9ce061aa7866bcf (변경 없음 — .proto 를 건드리지 않았다)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "해당 없음 - 로컬 단위 테스트"
command: |
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  cargo test --workspace -- --nocapture
  cargo build --workspace --all-targets
raw_output: |
  === 취약점 실측 (참조 구현 전수 대조, ADR-028 이전) ===
  같은 domain_tag 를 공유하는 메시지 쌍의 canonical 충돌:
    [Membership] AddMember      == RemoveMember     공통필드[1]    28바이트 동일
    [Membership] ApproveDevice  == RemoveMember     공통필드[1]    28바이트 동일
    [Membership] ApproveDevice  == RevokeDevice     공통필드[1,2]  56바이트 동일
    [Membership] RemoveMember   == RevokeDevice     공통필드[1]    28바이트 동일
    [Quarantine] QuarantineDevice == ReleaseQuarantine 공통필드[1] 동일
  총 충돌 쌍: 5

  === ADR-028 적용 후 ===
  domain 수: 23
  AddMember != RemoveMember tag: True
  QuarantineDevice != ReleaseQuarantine tag: True

  기존 벡터 canonical 변경: none   <- tag 는 sig_input 에만 들어간다
  기존 벡터 sig_input 변경: none

  Running tests\t1_signing_targets.rs
  domain 23종 — 구현 19 · proto 메시지 없음 4
    미구현 gputeer/v1/genesis   ★ proto 메시지 없음 — 스펙 공백
    미구현 gputeer/v1/audit     ★ proto 메시지 없음 — 스펙 공백
    미구현 gputeer/v1/release   ★ proto 메시지 없음 — 스펙 공백
    미구현 gputeer/v1/invite    ★ proto 메시지 없음 — 스펙 공백
  test result: ok. 11 passed; 0 failed; 0 ignored

  Running tests\t1b_grant_and_control.rs
  test result: ok. 10 passed; 0 failed; 0 ignored

  Running tests\field_number_audit.rs   10 passed
  Running tests\canonical_vectors.rs    15 passed
  Running tests\prost_canonical.rs      25 passed
  Running tests\schema_evolution.rs      6 passed
  Running tests\schema_fingerprint.rs    2 passed
  Running tests\ed25519_verify.rs       20 passed
  Running tests\stream_ownership.rs      5 passed
  Running tests\durability_chaos.rs     16 passed
  Running tests\kill_chaos.rs            7 passed
  Doc-tests                              2 passed

  전체: 129 passed / 0 failed
  cargo build --all-targets  경고 0건
artifacts:
  - docs/evidence/_raw/DoD-06_test_output.txt
  - docs/decisions/ADR-028_메시지별_domain_tag_분리.md
  - crates/protocol/tests/t1b_grant_and_control.rs
  - crates/protocol/src/to_fields.rs
  - crates/protocol/src/canonical.rs
negative_tests:
  - "★ signatures_do_not_transfer_between_control_messages: 같은 domain 을 공유하던 메시지들의 sig_input 이 서로 다른지 확인. ★ canonical 이 아니라 sig_input 을 본다 — canonical 은 **여전히 같기 때문이다**. canonical 을 검사하면 '우연히 필드가 달라서 통과'하는 약한 보증밖에 못 얻는다. 전제(canonical 동일)를 assert_eq 로 함께 고정해, 전제가 바뀌면 ADR-028 근거를 재확인하도록 했다"
  - "all_domain_tags_are_distinct: 23종 tag 가 전부 서로 다른지 확인. 오타로 두 tag 가 같아지면 그 두 메시지 사이의 방어가 조용히 사라진다"
  - "membership_messages_have_distinct_canonicals: 5종 membership 메시지가 실제 사용 형태에서 서로 다른 canonical 을 내는지 (이중 방어)"
  - "★ derived_hash_and_nested_signature_are_both_excluded: ExecutionGrant 의 manifest_hash(도출 해시)와 중첩 manifest 서명을 **둘 다** 바꿔도 canonical 이 같은지 확인. 규칙 i 의 필연적 결과이며, Agent 의 독립 검증·재계산이 필수인 이유다"
  - "nested_manifest_content_does_affect_grant_canonical: 위 테스트만 있으면 '중첩 manifest 전체가 무시되는' 결함과 구분되지 않는다. entrypoint 변경 · manifest 제거 · lease fence_epoch 변경을 각각 확인"
  - "every_grant_field_affects_canonical: Grant 의 10개 최상위 필드 + creds.token + assigned_gpu_uuids + PlacementRationale + rejected 후보를 하나씩 지워 canonical 이 변하는지 확인"
  - "multi_signature_field_is_excluded: repeated bytes signatures = 90 도 규칙 i 로 제외되는지. 서명 3개를 넣어도 canonical 불변. is_relaxation 변경은 canonical 을 바꾸는지 함께 확인 (완화를 강화로 위장 방지)"
  - "quarantine_verdict_matches_reference: target_is_coordinator 와 근거 signals 가 서명 대상인지 확인. 근거 없는 격리와 Coordinator 격리 위장을 막는다"
  - "change_coordinator_set: new_set 순서(규칙 d)와 failure_domain 이 서명 대상인지. failure_domain 이 밖이면 quorum 이 같은 랙에 몰려도 알 수 없다"
  - "derived_hash_fields_are_actually_excluded: DERIVED_HASH_FIELDS 에 선언한 필드가 실제로 to_fields 에 없는지 (반대 방향 검사). 선언해 놓고 넣으면 '서명이 보증한다'는 잘못된 인상을 주어 재계산을 건너뛰게 만든다"
limitations:
  - "★ 이 evidence 는 **canonical 바이트 수준의 재사용**만 다룬다. Ed25519 실제 서명으로 재사용 공격을 실행해 보지 않았다. sig_input 이 다르면 서명이 통과할 수 없다는 것은 Ed25519 의 성질이며 별도 검증하지 않았다"
  - "★ 19/23 domain 이 ToCanonicalFields 를 갖지만, 그 중 **Signable 을 가진 것은 JobManifest 와 Lease 둘뿐**이다. 나머지 17종은 §9 시각 정책이 정의되지 않아 verify() 를 통과할 수 없다 (DoD-05 limitations 참조)"
  - "genesis · audit · release · invite 4종은 proto 메시지 자체가 없다"
  - "다중 서명(m-of-n) 메시지 3종(RevokeDevice · UpdatePolicy · QuarantineDevice)의 **검증 절차를 구현하지 않았다.** canonical 에서 제외되는 것만 확인했다. '서로 다른 승인자 m명 이상'을 세는 로직이 없으며, 같은 키의 서명 2개를 2표로 세지 않는지도 미검증이다"
  - "ADR-028 은 tag 를 늘리는 방식이다. 대안이었던 'sig_input 에 메시지 타입 판별자 추가'는 더 일반적이나 §4 레이아웃 변경이라 기각했다. 앞으로 tag 가 계속 늘면 재검토가 필요하다"
  - "충돌 탐지 프로브는 string/bytes 필드만 채우는 방식이라 message/repeated 필드가 관여하는 충돌은 놓칠 수 있다. QuarantineDevice ↔ ReleaseQuarantine 충돌은 프로브가 아니라 Rust 테스트가 잡았다"
  - "Windows 단일 플랫폼"
decision: "ADR-028 채택 — 서명 대상 메시지마다 고유한 domain_tag 를 갖는다 (17 -> 23종). signing.md §5 표와 §5.1 을 갱신했다. canonical bytes 는 불변이므로 기존 벡터에 회귀가 없고 schema_version 도 올리지 않는다. 다음: T2(§9 시각 정책 결정) -> 다중 서명 검증 절차"
---

# DoD-06 · domain_tag 충돌과 서명 재사용

## 무엇을 입증하려 했는가

T1b 를 구현하며 `membership` · `policy` · `quarantine` 세 domain 이
**여러 메시지에 공유**된다는 것을 `DoD-05` 에서 이미 기록했다.

그때는 이렇게 적었다.

> 한 tag 를 여러 메시지가 공유하면 그 사이에서는 domain 분리가 없다.
> `AddMember` 서명을 `RemoveMember` 로 재사용하는 것은 canonical 차이로
> **실질적으로 막히지만**, domain 분리가 아니라 필드 차이에 의존하는 방어다.

**"실질적으로 막힌다" 를 확인하지 않고 적었다.** 이 검증이 그것을 확인한다.

## ★ 결과 — 막히지 않았다

테스트를 쓰자마자 실패했다.

```rust
#[test]
fn quarantine_messages_have_distinct_canonicals() { … }   // FAILED
```

```text
★ QuarantineDevice 와 ReleaseQuarantine 의 canonical 이 같다.
```

### 왜 같아지는가

`canonical_encode` 는 규칙 b 로 **기본값을 생략**한다.
공격자는 어느 필드를 비울지 고를 수 있으므로, **최소 형태가 공격면**이다.

```text
QuarantineDevice { device_id: X }              -> {1: X}
ReleaseQuarantine { device_id: X, reason: "" } -> {1: X}
                                                  ^^^^^^ 같다
```

`sig_input = domain_tag ‖ schema_version ‖ len ‖ canonical` 인데
tag 도 같고 canonical 도 같으므로 **sig_input 이 완전히 같다.**
→ 서명이 그대로 통과한다.

### 전수 대조

참조 구현으로 같은 domain 을 공유하는 모든 쌍을 조사했다.

| 충돌 쌍 | 공통 필드 | canonical |
|---|---|---|
| `AddMember` ↔ `RemoveMember` | `[1]` | **28바이트 동일** |
| `ApproveDevice` ↔ `RemoveMember` | `[1]` | **28바이트 동일** |
| `ApproveDevice` ↔ `RevokeDevice` | `[1,2]` | **56바이트 동일** |
| `RemoveMember` ↔ `RevokeDevice` | `[1]` | **28바이트 동일** |
| `QuarantineDevice` ↔ `ReleaseQuarantine` | `[1]` | 동일 |

**5쌍.**

### 구체적 공격

```text
소유자가 RemoveMember{member_id: X} 에 서명한다
  -> 서명 바이트를 RevokeDevice{device_id: X} 에 붙인다
  -> 검증 통과. 멤버 탈퇴가 **기기 폐기**로 바뀐다

Coordinator 들이 QuarantineDevice{device_id: X} 에 m-of-n 서명한다
  -> 그 서명들을 ReleaseQuarantine{device_id: X} 에 붙인다
  -> ★ **격리 판정이 격리 해제로 바뀐다**
```

두 번째가 특히 나쁘다. 격리는 위험 신호에 대한 대응인데,
그 판정 서명이 **정확히 반대 동작**의 승인으로 재사용된다.

## ★ 규범이 자기 규범을 어기고 있었다

`signing.md` §5 는 표 바로 아래에 이렇게 적혀 있다.

> 새 서명 대상 메시지를 추가할 때는 **반드시 새 domain_tag를 이 표에 등록해야 한다(MUST).**

그리고 tag 의 존재 이유도 같은 절이 밝힌다.

> 한 문맥의 서명을 다른 문맥에서 검증하면 domain_tag가 달라 **반드시 실패한다.**

**표가 두 문장을 모두 어기고 있었다.** 새 규칙을 만들 필요가 없었다 —
이미 있는 규칙을 표에 적용하기만 하면 됐다.

## 조치 — ADR-028

`membership`(6) · `policy`(1) · `quarantine`(2) → **9개 tag 로 분리.**
domain_tag 총수 **17 → 23.**

### ★ canonical 은 하나도 바뀌지 않았다

```text
기존 벡터 canonical 변경: 없음
기존 벡터 sig_input 변경: 없음
SCHEMA_FINGERPRINT 변경:  없음 (.proto 를 건드리지 않았다)
schema_version 상향:      불필요 (§7.3 대상이 아니다)
```

tag 는 `sig_input` 에만 들어가고 canonical 에는 들어가지 않기 때문이다.
**배포 전이므로 지금이 가장 싼 시점이었다.**

## ★ 회귀 방지 테스트는 canonical 이 아니라 sig_input 을 본다

처음 쓴 테스트는 canonical 비교였고, **그것이 취약점을 찾아 주었다.**
그러나 **회귀 방지용으로는 틀린 대상**이다.

```text
ADR-028 이후에도 canonical 은 여전히 같다.
막힌 것은 domain_tag 가 달라졌기 때문이다.

canonical 을 검사하면
  -> "우연히 필드 구성이 달라서 통과" 하는 약한 보증
sig_input 을 검사하면
  -> "서명이 전이되지 않는다" 는 실제 불변식
```

그래서 테스트를 다시 썼다. 전제(`canonical` 이 같다)도 `assert_eq!` 로
함께 고정해, **전제가 바뀌면 ADR-028 의 근거를 재확인**하게 한다.

## 부수 결과

### `DERIVED_HASH_FIELDS` 분리

`ExecutionGrant.manifest_hash` 는 규칙 i 의 **도출 해시 필드**다.
`UNIMPLEMENTED_FIELDS` 와 같은 목록에 두면 안 된다 — 뜻이 정반대다.

```text
UNIMPLEMENTED_FIELDS   실수로 빠졌다 -> 위조 가능 -> 채워야 한다
DERIVED_HASH_FIELDS    규칙 i 로 뺐다 -> 검증자가 재계산한다 -> 채우면 안 된다
```

반대 방향 테스트(`derived_hash_fields_are_actually_excluded`)도 넣었다.
선언해 놓고 실수로 넣으면 **"서명이 이 값을 보증한다" 는 잘못된 인상**을 주어
재계산을 건너뛰게 만든다.

### `ExecutionGrant` — 규칙 i 가 두 번 적용되는 유일한 메시지

```text
(a) manifest_hash(4)                도출 해시 필드      -> 제외
(b) manifest/lease 의 서명(90)      규칙 i 재귀        -> 제외

=> Agent 는 manifest 와 lease 를 **각각 독립 검증**하고
   manifest_hash 를 **재계산**해야 한다(MUST).
   하지 않으면 서명이 벗겨진 매니페스트로 Job 을 실행한다.
```

### `PlacementRationale` 을 서명 대상에 넣었다

처음에는 "설명용 값" 이라며 `UNIMPLEMENTED_FIELDS` 에 두었는데,
가드가 "위조 가능한 필드가 있다"고 실패시켰다. **약화하지 않고 구현했다.**

서명 밖이면 Coordinator 가 **"왜 이 노드를 골랐는가" 를 사후에 조작**할 수 있다.
분쟁 시 유일한 기록이다.

### 가드가 제 역할을 했다

`every_impl_is_audited` 가 T1b 의 신규 `impl` 15종을 잡아
`AUDITED` 등록을 강제했다. 그 가드가 없었으면 15종의 field number 는
**아무도 대조하지 않은 채** 통과했을 것이다.

## 이 실험이 증명하지 "않는" 것

- **Ed25519 실제 서명으로 재사용 공격을 실행해 보지 않았다.**
  canonical/sig_input 바이트 수준까지만 확인했다.
- ★ **19/23 이 `ToCanonicalFields` 를 갖지만 `Signable` 은 2종뿐이다.**
  나머지는 §9 시각 정책이 없어 `verify()` 를 통과할 수 없다.
- **다중 서명(m-of-n) 검증 절차가 없다.** canonical 에서 제외되는 것만 확인했다.
  "서로 다른 승인자 m명" 을 세는 로직도, 같은 키의 서명 2개를 2표로 세지
  않는지도 **미검증**이다.
- **충돌 탐지 프로브는 string/bytes 필드만 채운다.** message/repeated 가 관여하는
  충돌은 놓칠 수 있다 — 실제로 `QuarantineDevice` ↔ `ReleaseQuarantine` 은
  프로브가 아니라 Rust 테스트가 잡았다.
- Windows 단일 플랫폼.

## 결정

1. **ADR-028 채택** — 서명 대상 메시지마다 고유한 `domain_tag`.
2. `signing.md` §5 표(17→23) 와 §5.1 갱신.
3. **회귀 방지는 `sig_input` 으로 검사한다.** canonical 은 여전히 겹친다.
4. 다음: **T2**(§9 시각 정책 결정) → **다중 서명 검증 절차**.

관련: `docs/decisions/ADR-028_메시지별_domain_tag_분리.md` ·
`docs/evidence/DoD-05_T1_서명대상_확장.md` · `docs/protocol/signing.md` §5 · §5.1
