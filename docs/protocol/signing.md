# gPUteer Canonical Signing — 규범 명세 v1

**상태:** NORMATIVE. 이 문서와 구현이 충돌하면 이 문서가 옳다.
**적용 대상:** `proto/*.proto`의 모든 서명 대상 메시지
**설계 근거:** `gputeer_master_implementation_plan_v5.md` §25.2 / ADR-020
**최종 수정:** 2026-08-14

> **이 문서가 존재하는 이유**
>
> v5 초안은 `manifest_hash`를 "필드 전체의 해시"라고만 적었다. 이것으로는 구현할 수 없다.
> proto3는 deterministic serialization을 보장하지 않고, map에는 순서가 없으며,
> 해시 필드가 메시지 안에 있으면 자기참조 순환이 생긴다.
> 그 결과 **같은 메시지가 다른 해시를 갖고, 서명 검증이 랜덤하게 실패한다.**
> Agent의 17단계 검증(계획서 §15.4)이 통째로 무의미해진다.
>
> 규범 명세를 산문이 아니라 **실행 가능한 참조 구현**(`tools/canonical/`)과 함께 둔다.

---

## 1. 용어

| 용어 | 정의 |
|---|---|
| **서명 대상 메시지** | `bytes ..._signature = 90` 필드를 가진 메시지 |
| **canonical bytes** | §3의 알고리즘으로 생성된, 구현과 무관하게 결정론적인 바이트열 |
| **sig_input** | 실제로 Ed25519에 입력되는 바이트열 (§4) |
| **domain_tag** | 서명의 용도를 고정하는 32바이트 접두사 (§5) |
| **schema_version** | 서명자가 사용한 필드 집합의 버전 (§7) |

MUST / MUST NOT / SHOULD 는 RFC 2119의 의미로 쓴다.

---

## 2. 알고리즘 고정

```text
Serialization   Protobuf proto3 + 본 문서 §3의 canonical 제약
Hash            BLAKE3-256  (32바이트 출력)
Signature       Ed25519     (RFC 8032, 64바이트)
Encoding        네트워크 전송은 표준 proto3, 서명 계산만 canonical
```

`HASH_ALGORITHM_SHA256`은 OCI 레지스트리 호환 목적으로만 존재한다.
**서명 계산에 SHA-256을 사용하면 안 된다(MUST NOT).**

---

## 3. canonical_encode

### 3.1 규칙

서명 대상 메시지 `M`에 대해 `canonical_encode(M)`은 다음을 만족해야 한다(MUST).

| # | 규칙 |
|---|---|
| **a** | 필드는 **field number 오름차순**으로 직렬화한다 |
| **b** | **기본값 필드는 출력하지 않는다** — 0, `""`, 빈 bytes, 빈 repeated, 빈 map, UNSPECIFIED enum(=0), 미설정 메시지 |
| **c** | **map은 key를 바이트열로 보고 오름차순 정렬**한 뒤 repeated 엔트리로 인코딩한다 |
| **d** | **repeated는 선언 순서를 유지한다** (정렬하지 않는다) |
| **e** | varint는 **최단 인코딩**만 허용한다. non-minimal varint를 만나면 파싱을 거부한다 |
| **f** | 중첩 메시지에도 같은 규칙을 **재귀 적용**한다 |
| **g** | **부동소수점 타입을 사용하지 않는다.** 비율은 ppm 정수, 시각은 밀리초 정수 |
| **h** | **알 수 없는 필드는 포함하지 않는다** (§7 참조) |
| **i** | 서명 필드(field 90)와 계산으로 도출되는 해시 필드는 **제외**한다. ★ 제외 후 **빈 메시지가 되면 그 필드 자체를 생략**한다 (아래 i-2) |
| **j** | 부호 있는 정수(`int32`/`int64`)는 **2의 보수 u64 로 재해석해** varint 인코딩한다. `sint*`/`sfixed*`/`fixed*` 는 **사용하지 않는다(MUST NOT)** |

#### ★ 규칙 i-2 — 제외 후 빈 메시지는 생략한다 (2026-08-16 추가)

**독립 검수에서 발견됐다. 두 구현이 똑같이 틀리고 있었다.**

규칙 i 의 뜻은 "서명 필드가 canonical 에 **영향을 주지 않는다**" 이다.
그런데 제외 판정 순서를 잘못 두면 그 뜻이 새어나간다.

```text
잘못된 순서   기본값 검사 -> 서명 필드 제외
              Message({})        -> 기본값 -> 생략
              Message({90: sig}) -> 기본값 아님 -> **빈 중첩 메시지로 출력**
                                                    0a 00   (2바이트)

              두 값은 서명 필드를 빼면 내용이 같은데 canonical 이 다르다.
```

**올바른 순서:** 중첩 메시지는 **먼저 재귀 인코딩하고, 결과가 비면 필드를 생략한다.**

```text
inner = canonical_encode(nested)     # 규칙 i 가 여기서 90 을 제외한다
if len(inner) == 0: continue         # ★ 그 결과가 비면 필드 자체를 생략
```

##### repeated 메시지는 다르다

`repeated`는 **원소를 버리지 않는다**(규칙 d — 순서가 의미를 갖는다).
빈 메시지 원소도 길이 0으로 인코딩해 자리를 지킨다.
필드 전체가 비었을 때만 규칙 b 로 생략된다.

##### 왜 벡터 대조로 못 잡았나

Rust 와 Python 참조 구현이 **같은 순서로 틀렸다.**
벡터 대조는 "두 구현이 같은가" 를 증명하지 "두 구현이 옳은가" 를 증명하지 않는다.
고정 테스트: `crates/protocol/tests/codex_findings.rs`

#### ★ 규칙 i-3 — 도출 해시 제외는 **최상위에만** 적용한다 (2026-08-16 추가)

같은 검수에서 나온 반례다.

```text
ExecutionGrant.manifest_hash            = field 4    -> 제외해야 한다
ExecutionGrant.manifest.dataset.retention
                     (DatasetRef 의)     = field 4    -> 제외하면 **안 된다**
```

**field number 는 메시지마다 의미가 다르다.** 번호만으로 재귀 제외하면
우연히 같은 번호를 쓰는 중첩 필드가 함께 사라진다.
위 예에서는 **데이터셋 삭제 정책이 서명에서 조용히 빠진다.**

도출 해시 필드는 정의상 **최상위 메시지의 것**이므로(§6.1 —
`ExecutionGrant.manifest_hash`), 최상위에만 적용한다.

★ 서명 필드(90)는 다르다 — **모든 메시지에서 같은 의미**이므로 재귀 적용된다.

중첩 메시지가 자기 도출 해시를 갖게 되면 그때 `(메시지 타입, field number)`
쌍으로 확장한다. 지금은 그런 메시지가 없다.

#### ★ 규칙 c-2 — map 엔트리 안에서도 규칙 b 를 적용한다 (2026-08-16 추가)

같은 검수에서 발견된 모호성이다. `{"k": ""}` 를 어떻게 인코딩하는가?

```text
(1) 12 00 을 출력한다        엔트리 안의 빈 값을 명시
(2) 생략한다                 엔트리는 key(1) 만 남는다
```

proto3 map 시맨틱에서 **"값 없음" 과 "빈 값" 은 같다.** 규칙 b 의 논리
("설정된 0 과 미설정을 구분하지 않으므로 둘 다 생략해 일치시킨다")가 그대로 적용된다.

→ **(2) 를 택한다.** 엔트리 안에서도 규칙 b 를 적용한다.

★ **키의 존재 자체는 정보다.** `{"k": ""}` 와 `{}` 는 여전히 다르다 —
전자는 엔트리가 있고 후자는 없다.

#### 규칙 j — 부호 있는 정수 (2026-08-16 추가)

★ **이 규칙은 원래 없었다.** T1 구현 중 발견했다.

스키마 전체에서 부호 있는 필드는 **단 하나**인데, 하필 서명 대상 안에 있다.

```text
proto/artifact.proto:234   int64 value_micro = 2;      (ReportedMetric)
                           -> AttemptReport.metrics 안 -> domain "gputeer/v1/attempt-report"
```

규칙이 없으면 구현자마다 다르게 인코딩한다. 특히 세 갈래로 갈릴 수 있다.

```text
(1) 2의 보수 u64 재해석      -1 -> 0xFFFFFFFFFFFFFFFF -> 10바이트 varint   <- proto3 표준
(2) zigzag (sint 방식)       -1 -> 1 -> 1바이트 varint
(3) 음수 거부
```

**(1) 을 택한다.** canonical 인코딩은 **유효한 protobuf 인코딩의 부분집합**이어야 한다는
설계 원칙 때문이다(§13.1 — 별도 인코더를 쓰되 wire format 은 벗어나지 않는다).
`int64` 의 proto3 wire format 이 (1)이므로 다른 선택은 wire format 을 벗어난다.

```text
음수 int64  항상 정확히 10바이트다. 그것이 u64 재해석 값의 최단 varint 이므로
            규칙 e(최단 varint)와 모순되지 않는다.

★ int32 의 음수는 **먼저 64비트로 부호 확장한 뒤** 인코딩한다.
  protobuf 의 유명한 함정이다 — int32 -1 도 10바이트다. 5바이트가 아니다.

기본값 0    규칙 b 로 생략된다. -0 은 존재하지 않으므로 §g 의 -0.0 문제는 없다.
```

`sint*`(zigzag) · `fixed*` · `sfixed*` 를 **쓰지 않는 이유**는 규칙 e·g 와 같다 —
같은 값의 표현이 둘 이상 생기거나 바이트 순서가 개입하면 서명 우회 여지가 생긴다.

### 3.2 왜 이 규칙인가

- **a, f** — proto3 구현마다 필드 출력 순서가 다르다. Rust `prost`와 Python `protobuf`가 다른 바이트를 낸다.
- **b** — "설정된 0"과 "미설정"을 proto3는 구분하지 않는다. 둘 다 생략해 일치시킨다.
- **c** — `map<string,string> env_vars`가 매 인코딩마다 다른 순서로 나오면 해시가 매번 달라진다. 실제로 v5 초안이 이 함정에 빠져 있었다.
- **e** — 같은 값을 여러 바이트열로 표현할 수 있으면 서명 우회 여지가 생긴다.
- **g** — IEEE-754는 `-0.0`, `NaN` 페이로드, 비정규수 표현이 플랫폼마다 다르다. 애초에 쓰지 않는 것이 유일하게 안전하다.
- **i** — 서명을 계산하려면 서명 필드가 비어 있어야 한다. 순환 제거.

### 3.3 의사코드

```text
canonical_encode(msg) -> bytes:
    out = []
    for field in sorted(msg.descriptor.fields, key = f.number):
        if field.number == 90: continue          # 규칙 i
        if field.is_derived_hash: continue       # 규칙 i
        value = msg.get(field)
        if is_default(value): continue           # 규칙 b

        if field.is_map:                                    # 규칙 c
            entries = [(canonical_key_bytes(k), v) for k,v in value]
            entries.sort(by = key_bytes)
            for k, v in entries:
                out += encode_tag(field.number, WIRETYPE_LEN)
                inner = encode_field(1, k) + encode_field(2, v)
                out += encode_varint(len(inner)) + inner

        elif field.is_repeated:                             # 규칙 d
            for item in value:                              # 순서 유지
                out += encode_field(field.number, item)

        elif field.is_message:                              # 규칙 f
            inner = canonical_encode(value)
            out += encode_tag(field.number, WIRETYPE_LEN)
            out += encode_varint(len(inner)) + inner

        else:
            out += encode_field(field.number, value)        # 규칙 e (최단 varint)
    return out
```

**packed repeated를 사용하지 않는다(MUST NOT).** 각 원소를 개별 태그로 인코딩한다.
packed 여부가 구현마다 달라 바이트가 갈리기 때문이다.

---

## 4. sig_input

```text
sig_input = domain_tag                    (32 bytes, 고정)
          || uint32_be(schema_version)    (4 bytes)
          || uint32_be(len(canonical))    (4 bytes)
          || canonical                    (가변)
```

```text
signature = Ed25519_sign(private_key, sig_input)
verify    = Ed25519_verify(public_key, sig_input, signature)
```

`schema_version`을 길이 앞에 넣는 이유: 버전이 다르면 같은 canonical이라도 다른 서명이 된다.
구버전 필드 집합으로 재구성한 서명이 신버전으로 통과하는 것을 막는다.

`len(canonical)`을 넣는 이유: 길이 확장 공격과 연접 모호성(concatenation ambiguity) 차단.

---

## 5. domain_tag

용도별 고정 문자열. **ASCII, 32바이트, 우측 `0x00` 패딩.**

| 메시지 | domain_tag (패딩 전) |
|---|---|
| `JobManifest` | `gputeer/v1/manifest` |
| `ExecutionGrant` | `gputeer/v2/grant` |
| `Lease` | `gputeer/v1/lease` |
| `RenewLeaseRequest` | `gputeer/v1/lease-renew` |
| `RevokeLeaseNotice` | `gputeer/v1/lease-revoke` |
| `CheckpointManifest` | `gputeer/v1/checkpoint` |
| `ReplicaAck` | `gputeer/v1/replica-ack` |
| `ArtifactRef` | `gputeer/v1/artifact` |
| `AttemptReport` | `gputeer/v1/attempt-report` |
| `CanonicalDecision` | `gputeer/v1/canonical` |
| Genesis Manifest | `gputeer/v1/genesis` |
| `AddMember` | `gputeer/v1/member-add` |
| `RemoveMember` | `gputeer/v1/member-remove` |
| `ApproveDevice` | `gputeer/v1/device-approve` |
| `RevokeDevice` | `gputeer/v1/device-revoke` |
| `ChangeCoordinatorSet` | `gputeer/v1/coordinator-set` |
| `RotateOwnerKey` | `gputeer/v1/owner-key-rotate` |
| `UpdatePolicy` | `gputeer/v1/policy-update` |
| `QuarantineDevice` | `gputeer/v1/quarantine-device` |
| `ReleaseQuarantine` | `gputeer/v1/quarantine-release` |
| 감사 로그 엔트리 | `gputeer/v1/audit` |
| Release Manifest | `gputeer/v1/release` |
| Invite Bundle | `gputeer/v1/invite` |
| `AgentGrantAck` | `gputeer/v1/grant-ack` |
| `RenewLeaseResult` | `gputeer/v1/lease-renew-result` |
| `AgentSessionHello` | `gputeer/v1/session-hello` |
| `NodeHeartbeat` | `gputeer/v1/node-heartbeat` |
| `NeighborUnreachableReport` | `gputeer/v1/neighbor-unreachable` |
| `ResumeLeaseRequest` | `gputeer/v1/lease-resume` |
| `ResumeLeaseResult` | `gputeer/v1/lease-resume-result` |

**총 30종.** ★ 2026-08-16 이전에는 17종이었고 `membership`(6개 메시지) ·
`policy` · `quarantine`(2개 메시지)이 tag 를 공유했다. **ADR-028 로 분리했다** —
사유는 §5.1. `AgentGrantAck` 는 coordinator/agent 최소 핸드셰이크
(docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md, 2026-08-18)로
23→24 종째 추가됐다 — `ReplicaAck` 를 재사용하지 않는다. `ReplicaAck` 는
`Lifetime::Evidence` 라 replay nonce 를 검사하지 않으므로, 공유하면
"ACK replay 를 `DurableReplayGuard` 가 거부하는가" 를 증명할 수 없다.
`RenewLeaseResult` 는 Lease 갱신 최소 조각
(docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md, 2026-08-19)으로
24→25 종째 추가됐다 — `RenewLeaseRequest`(agent 가 서명하는 요청)와
tag 를 공유하지 않는다. 공유하면 요청 서명이 응답 검증도 통과해
교차 재생(cross-message replay)이 가능해진다. Resume 프로토콜의
`AgentSessionHello`·`ResumeLeaseRequest`·`ResumeLeaseResult`가 조각 3에서
각각 독립 tag를 추가해 29종이 됐고, 2026-08-30 이웃 신고
(`gputeer/v1/neighbor-unreachable`)가 더해져 현재 총 30종이다.

**한 문맥의 서명을 다른 문맥에서 검증하면 domain_tag가 달라 반드시 실패한다.**
이것이 없으면 예컨대 Lease 서명을 Manifest 서명으로 재사용하는 공격이 가능하다.

새 서명 대상 메시지를 추가할 때는 **반드시 새 domain_tag를 이 표에 등록해야 한다(MUST).**

### ★ 5.1 tag 공유가 서명 재사용을 허용했다 — ADR-028 로 시정 (2026-08-16)

2026-08-16 이전 표는 세 tag 를 여러 메시지가 공유했다.
**§5 자신의 MUST("메시지마다 새 domain_tag 를 등록한다")를 표가 어기고 있었다.**

tag 를 공유하면 §5 의 방어("tag 가 달라 반드시 실패한다")가 사라지고
**canonical 차이만이 유일한 방어**가 된다. 그런데 규칙 b(기본값 생략) 때문에
공격자가 필드를 비우면 canonical 이 짧아지고, 서로 다른 메시지가 **같은 바이트**가 된다.

실측 (참조 구현 전수 대조):

| 충돌 쌍 | 공통 필드 | canonical |
|---|---|---|
| `AddMember` ↔ `RemoveMember` | `[1]` | **28바이트 동일** |
| `ApproveDevice` ↔ `RemoveMember` | `[1]` | **28바이트 동일** |
| `ApproveDevice` ↔ `RevokeDevice` | `[1,2]` | **56바이트 동일** |
| `RemoveMember` ↔ `RevokeDevice` | `[1]` | **28바이트 동일** |
| `QuarantineDevice` ↔ `ReleaseQuarantine` | `[1]` | **동일** |

```text
소유자가 RemoveMember{member_id: X} 에 서명한다
  -> 그 서명 바이트가 RevokeDevice{device_id: X} 로도 검증된다
  -> 멤버 탈퇴가 기기 폐기로 바뀐다

Coordinator 들이 QuarantineDevice 에 m-of-n 서명한다
  -> 그 서명들이 ReleaseQuarantine 으로 검증된다
  -> ★ 격리 판정이 **격리 해제**로 바뀐다
```

→ **ADR-028** 로 9개 tag 를 분리했다. canonical bytes 는 하나도 바뀌지 않았다
(tag 는 `sig_input` 에만 들어간다).

회귀 방지: `crates/protocol/tests/t1b_grant_and_control.rs` 가 같은 domain 을
공유하는 메시지 쌍의 canonical 이 서로 다른지 검사한다.
`crates/protocol/tests/canonical_vectors.rs::domain_tags_are_32_bytes_and_unique` 와
`t1b_grant_and_control.rs::all_domain_tags_are_distinct` 가 tag 중복을 검사한다
(둘 다 `Domain::ALL` 을 순회하므로 새 domain 이 자동으로 포함된다).

★ 위 표와 `tools/canonical/reference_canonical.py` 의 `DOMAIN_TAGS` 가 코드의
`Domain::ALL` 과 정확히 같은지는
`canonical_vectors.rs::domain_tags_match_the_norm_document_and_the_python_reference`
가 세 목록을 실제로 파싱해 **메시지 → tag 대응까지** 대조한다 — 이 저장소가
다섯 번 겪은 "손으로 쓴 목록이 낡는" 결함을 잡기 위해서다(2026-08-30).

★ 이것은 **낡음을 잡는 장치이지 위조를 막는 장치가 아니다.** 관련된 자리를
**동시에 같은 방향으로** 고치면 여전히 통과한다. 어떤 파서도 일부러 속이려는
편집을 전부 막지는 못한다 — 이 테스트가 올리는 것은 **우회 비용**이고, 그
비용은 메시지에 따라 다르다.

```text
Signable 구현 메시지    네 자리 — canonical.rs 또는 signable.rs +
                        t1_signing_targets.rs 의 coverage 이름 + 이 표 +
                        reference_canonical.py
그 외(membership 계열)  세 자리 — coverage 이름 + 이 표 + reference_canonical.py
                        (Rust 쪽 메시지→domain 대응이 없어 한 자리가 빠진다)
```

### ★ 5.2 4종은 proto 메시지가 없다

이 표는 **아직 존재하지 않는 메시지의 domain_tag 를 등록해 두고 있다.**

| domain_tag | proto 메시지 |
|---|---|
| `gputeer/v1/genesis` | **없음** |
| `gputeer/v1/audit` | **없음** |
| `gputeer/v1/release` | **없음** |
| `gputeer/v1/invite` | **없음** |

현재 커버리지는 `crates/protocol/tests/t1_signing_targets.rs::domain_coverage_is_explicit`
가 고정한다.

---

## 6. 도출 해시

### 6.1 manifest_hash

```text
manifest_hash = BLAKE3_256( sig_input_of(JobManifest) )
```

- **`JobManifest` 메시지 안에 저장하지 않는다(MUST NOT).**
- `ExecutionGrant.manifest_hash`는 참조용 사본이다.
  **Agent는 이 값을 신뢰하지 않고 반드시 재계산해 대조한다(MUST)** — 계획서 §15.4 검증 13단계.
- Job 식별, `WorkloadProfile` 키, 실행 이력 조회의 키로 사용한다.

#### ★ 이 MUST 를 `verify()` 가 수행한다 (2026-08-16 추가)

독립 검수가 지적했다 — **규범은 MUST 라고 적었는데 그 코드가 어디에도 없었다.**
`verify()` 성공만으로는 Grant 가 올바른 manifest 를 가리킨다는 보장이 없었다.

```text
Signable::check_derived_consistency()   §8 흐름의 6.5 단계로 호출된다
                                        (서명 검증 뒤 · 시각 검사 앞)
```

판정 규칙:

```text
manifest 있음 + hash 있음   대조. 다르면 거부
manifest 있음 + hash 없음   통과 — 주장을 안 했다
manifest 없음 + hash 있음   거부 — 없는 것의 해시를 주장한다
둘 다 없음                  통과
```

★ **실패는 `VerifyOutcome` 이 아니다.** `common.proto` 에 대응하는 값이 없고,
`INVALID_SIGNATURE` 로 보고하면 **"서명이 위조됐다" 로 읽히는데 실제로는
서명은 정상이고 참조 해시가 틀린 것**이다(`CLAUDE.md` §3).
`VerifyError::Derived` 로 분리했다. proto 값 추가는 `schema_version` 상향이
필요하므로 `TODO_VISION` V-09 로 등록했다.

★ **기본 구현은 no-op 이다** — 타입이 강제하지 않는다.
`DERIVED_HASH_FIELDS` 에 있는 메시지가 실제로 덮어썼는지는
`derived_hash_messages_override_consistency_check` 테스트가 대조한다.

### 6.2 operation_id

```text
operation_id = BLAKE3_256( job_id_utf8 || attempt_id_utf8 || uint64_be(operation_seq) )
```

`fence_epoch`만으로는 같은 attempt의 서로 다른 두 쓰기를 구분할 수 없다.
Hub/CAS는 `(fence_epoch, operation_id)`로 중복을 판정한다.

### 6.3 Merkle root (체크포인트 / 데이터셋)

```text
leaf(i)   = BLAKE3_256( 0x00 || chunk_i )
inner(a,b)= BLAKE3_256( 0x01 || a || b )
```

- 홀수 노드는 **승격**한다(복제하지 않는다). 두 번째 원상 공격 방어를 위해 leaf/inner 도메인을 분리한다.
- 청크 크기 기본 **4 MiB**. 마지막 청크만 작을 수 있다.

---

## 7. schema_version 과 알 수 없는 필드

### 7.1 v5 초안의 오류

초안 §25.5는 **"알 수 없는 필드는 무시하되 서명 검증 대상에는 포함"** 이었다.
이러면 구버전은 신버전 메시지의 canonical을 재구성할 수 없어 **영원히 검증에 실패한다.**

### 7.2 규칙

```text
- canonical_encode는 "검증자가 아는 필드"만 포함한다 (§3 규칙 h)
- 모든 서명 대상 메시지는 schema_version 을 갖는다
- 검증자는 자신이 지원하는 최대 schema_version 을 안다

if  msg.schema_version <= verifier.max_supported:
        canonical 재구성 후 정상 검증
else:
        VERIFY_OUTCOME_SCHEMA_TOO_NEW 를 반환한다
        → 절대로 VALID로 취급하지 않는다 (MUST NOT)
        → 계획서 §24.5의 UPGRADE_REQUIRED 경로로 처리한다
```

**"모르는 필드는 무시하고 통과"가 아니라 "모르면 검증 불가를 선언"한다.**
보안 필드가 추가됐는데 구버전이 그것을 무시한 채 통과시키는 상황을 막기 위해서다.

#### 실측 근거 (P0-08 · 2026-08-16 · `docs/evidence/P0-08_스키마_진화.md`)

이 절차가 **실제로 구현 가능한지** 의심할 이유가 있었다.
prost 는 미지 필드를 조용히 버리므로, "모르는 필드가 있다" 를 구현이 알 수 없을 수 있다.

실측 결과:

```text
prost 는 미지 필드에 오류를 내지 않고 버린다      118B -> 148B(주입) -> 118B(재인코딩)
미지 필드는 canonical 에 아무 흔적도 남기지 않는다
-> 구버전 검증자는 **메시지 본문만 보고는 새 필드의 존재를 알 수 없다**
-> 유일한 신호는 schema_version 이다
```

그러므로 §7.2 는 **구현 가능하다.** 단 그 전제는 발신자가 §7.3 을 지키는 것이다.

`schema_version` 은 canonical(필드 1)과 `sig_input` **양쪽**에 들어가므로
버전 강등은 반드시 서명을 깬다. 버전 검사를 빠뜨려도 안전성은 유지된다.

★ 그럼에도 **버전 검사를 서명 검사보다 먼저** 해야 하는 이유는 안전성이 아니라
**진단 정확성**이다. 순서를 뒤집으면 "업그레이드가 필요하다" 를
**"서명이 위조됐다"** 로 보고하게 된다 (`CLAUDE.md` §3).

### 7.3 스키마 진화 규칙

```text
허용   새 필드 추가 + schema_version 증가
허용   새 enum 값 추가 (단 소비 측이 UNSPECIFIED 처리를 해야 함)
금지   기존 field number 재사용
금지   기존 필드의 타입 변경
금지   기존 필드의 의미 변경
금지   schema_version 증가 없는 필드 추가
```

제거된 필드 번호는 `reserved` 로 표시한다(MUST).

#### ★ 이 금지는 프로토콜이 강제하지 못한다 — 빌드 시점 검사로 강제한다 (MUST)

P0-08 이 확인한 가장 중요한 사실이다.

> `schema_version` 을 올리지 않고 필드를 추가하면
> **구버전이 새 보안 제약을 무시한 채 서명 검증을 통과시킨다.**
> 프로토콜 수준에서 이것을 탐지할 방법은 없다.

`.proto` 에 필드를 하나 추가하고 버전 상수를 그대로 두는 것은 한 줄짜리 실수인데,
그 결과는 조용한 보안 우회다. **강제할 수 없는 규칙을 규범으로 두지 않는다**
(`CLAUDE.md` §0.4 와 같은 정신).

그래서 스키마 지문을 저장소에 고정한다.

```text
proto/SCHEMA_FINGERPRINT.txt                      72개 메시지 · 449개 필드
crates/protocol/tests/schema_fingerprint.rs       대조. 다르면 실패

갱신: UPDATE_SCHEMA_FINGERPRINT=1 cargo test -p gputeer-protocol --test schema_fingerprint
```

스키마가 바뀌면 테스트가 **무엇이 추가/삭제됐는지 출력하며 실패**하고,
둘 중 하나를 강제한다.

```text
(a) 필드 추가 · 타입 변경 · 의미 변경   -> schema_version 을 올린다
(b) 그 외 (주석 · 서식 · 파서 무관 변경) -> 지문만 갱신하고 사유를 커밋에 적는다
```

★ **중첩 `message` 선언 · `reserved` 를 도입하기 전에 `docs/vision/TODO_VISION.md` V-05 를
반드시 읽는다.** 현재 지문 파서는 정규식이며 그 두 구문을 처리하지 못한다.
도입 시점이 곧 그 필드에 대해 이 강제 장치가 **조용히 무력해지는 시점**이다.

(`oneof` 는 이미 `control.proto` 에 5개 있으며 **정상 처리된다** — 실측 확인.
기존 필드를 `oneof` 안팎으로 **이동**하는 경우만 탐지되지 않는다.)

---

## 8. 검증 순서

**서명 검증 전에는 어떤 필드 값도 로직에 사용하지 않는다(MUST NOT).**

```text
1. domain_tag 결정 (메시지 타입에서 정적으로)
2. schema_version 확인               → 초과 시 SCHEMA_TOO_NEW, 종료
3. canonical 재구성
4. sig_input 조립
5. Ed25519 검증                      → 실패 시 INVALID_SIGNATURE, 종료
6. 서명자 신원 확인 (팀 멤버십/승인) → 실패 시 UNKNOWN_SIGNER, 종료
7. 시각 검증 (§9)                    → 실패 시 EXPIRED / CLOCK_SKEW, 종료
8. replay 검증 (§10)                 → 실패 시 REPLAY, 종료
9. ── 여기서부터 필드 값을 신뢰한다 ──
```

`VerifyOutcome`(common.proto)의 각 값이 위 단계에 1:1 대응한다.

### ★ 8.1 예외 — 키 조회용 `signer_id` 는 "필드 값을 로직에 쓰는 것" 이 아니다

독립 검수(2026-08-16)가 지적했다.

> `verify()` 는 서명 검증 **전에** `msg.signer_id()` 를 읽어 키를 찾는다.
> §8 의 "검증 전에 어떤 필드 값도 로직에 사용하지 않는다(MUST NOT)" 와 충돌한다.

**불가피하다** — 키를 찾아야 서명을 검증할 수 있다.
대안은 "후보 공개키 **전부**로 검증을 시도" 인데, 키 수에 비례해 비용이 늘고
타이밍 부채널이 생긴다.

그래서 예외를 **명시**한다.

```text
허용   signer_id 를 **키 조회 키**로 쓰는 것.
       신뢰하지 않는 라우팅이며, 틀린 값이면 UnknownSigner 로 끝난다.

금지   그 밖의 모든 필드를 검증 전에 쓰는 것.
       job_id · entrypoint · network 정책 등을 미리 읽어 준비하는 것도 금지다.
```

★ **`signer_id` 자체는 canonical 에 들어가므로 서명이 보증한다.**
검증이 통과한 뒤에는 그 값이 진짜다. 검증 전에는 "이 키로 시도해 보라" 는
힌트일 뿐이며, 틀리면 `UnknownSigner` 또는 `InvalidSignature` 로 거부된다.

이 예외를 적어 두지 않으면 다음 사람이 §8 위반으로 오해하거나,
반대로 **다른 필드도 미리 읽어도 된다고 오해한다.**

---

## 9. 시각 검증 — 장수명/단수명 분리

**계획서 §15.1 / ADR-021.** 이 구분을 지키지 않으면 큐를 통과한 정상 Job이 100% 거부된다.

| 메시지 | 60초 skew 규칙 | `expires_at` 검사 | 기본 TTL |
|---|:---:|:---:|---|
| `ExecutionGrant` | ✅ 적용 | ✅ | 60초 |
| `RenewLeaseRequest` | ✅ 적용 | ✅ | 60초 |
| `Lease` | ❌ | ✅ | 10분 |
| Heartbeat / RPC | ✅ 적용 | ✅ | 60초 |
| **`JobManifest`** | **❌ 미적용** | ✅ | **7일** |
| Invite Bundle | ❌ 미적용 | ✅ | 팀 설정 |
| **증거 6종** (아래 §9.1) | ❌ 미적용 | **❌ (만료 없음)** | — |
| Genesis / Release Manifest | ❌ 미적용 | ❌ (무기한) | — |

```text
clock_skew_tolerance = 60초 (프로토콜 상수)

단수명:  |now − issued_at| <= clock_skew_tolerance  AND  now < expires_at
장수명:  now < expires_at                            (issued_at 은 검사하지 않음)
```

Job은 큐에서 수 시간 대기하는 것이 **정상 동작**이다(계획서 §13.6 aging queue).
따라서 Manifest에 skew 규칙을 걸면 안 된다.

★ **기본 TTL(60초) == skew 허용치(60초)이므로 미래 방향 skew 검사는
만료 검사에 가려진다** (`DoD-04` 실측). 안전성 문제는 아니다 — 둘 다 거부한다.
**과거 방향**(검증자 시계가 빠른 경우)은 여전히 도달 가능하며 그것이 skew 검사의
실질적 역할이다.

TTL 을 늘려 미래 방향을 "살리는" 것은 **하지 않는다** — 단수명 메시지의 수명을
늘리면 replay 창이 커진다. 보안 매개변수를 코드 경로 도달성 때문에 바꾸지 않는다.

### ★ 9.1 증거 메시지 — `Lifetime::Evidence` (ADR-029)

**서명 대상 6종은 "권한" 이 아니라 "증거" 다. 시각으로 만료시키지 않는다.**

```text
CheckpointManifest    ReplicaAck    ArtifactRef
AttemptReport         CanonicalDecision    RevokeLeaseNotice
```

이들은 `expires_at` 필드조차 없다. 처음에는 표의 누락으로 보였으나,
**만료 개념 자체가 없는 것이 옳다.**

#### 과거의 사실은 만료되지 않는다

`CheckpointManifest` 는 "이 체크포인트의 내용이 이것이다" 라는 증거다.
3년 뒤에도 참이다. 만료시키면 **오래된 체크포인트에서 재개할 수 없게 되고**,
그것은 이 시스템의 존재 이유를 정면으로 부순다.

#### ★ 그러나 "그 시점의 사실" ≠ "지금의 사실"

`Perpetual` 과 구분하는 이유다.

```text
Perpetual   Genesis · Release Manifest. 시스템 상수에 가깝다.
            "지금도 참인가" 를 물을 필요가 없다.

Evidence    관측 시점의 사실이다. **소비 측이 신선도를 판단해야 한다.**
```

그래서 `Evidence` 메시지는 **`observed_at` 을 반드시 노출한다(MUST).**
"언제인지 모르는 증거" 는 증거가 아니다.

#### 신선도는 시각이 아니라 fencing 이 판단한다

6종 중 5종이 `fence_epoch` 을 갖는다. stale 한 증거는 epoch 이 낮아 거부된다.
**시계는 어긋나지만 epoch 은 어긋나지 않는다.**

#### ★ `ReplicaAck` 만 `fence_epoch` 이 없다

```text
ReplicaAck 는 REPLICATED(n) 을 세는 근거 = durability 주장의 뿌리인데
신선도 판단 근거가 acked_at 하나뿐이다.

**복제본이 삭제되어도 ACK 는 영원히 유효하다.**
```

소비 측은 이것을 "지금 durable 하다" 가 아니라
**"`acked_at` 시점에 durable 했다"** 로만 읽어야 한다(MUST).
`CLAUDE.md` §0.3 의 `COMMITTED_DEGRADED` 가 같은 인식이다.

→ `fence_epoch` 추가는 `schema_version` 상향(§7.3)이 필요해 별도 결정이다.
`TODO_VISION` V-07.

#### `Evidence` 는 replay 대상이 아니다

§10 은 단수명 메시지만 대상으로 한다. `RevokeLeaseNotice` 는 명령이지만
`fence_epoch` 이 stale 회수를 막는다. 다만 **같은 epoch 의 회수 통지를
반복 전송하는 것**은 막지 못한다 — 회수는 멱등하므로 무해하다.

근거와 대안 검토는 **ADR-029**.

---

## 10. Replay 캐시

```text
대상    단수명 메시지의 nonce만
        JobManifest 는 대상이 아니다
        — 하나의 Manifest로 여러 Attempt를 만드는 것이 정상이므로

키      (sender_device_id, domain_tag, nonce)      ← device별 namespace 필수
저장    로컬 SQLite
원자성  검사 → 삽입 → commit → 그 다음에 실행 시작 (단일 트랜잭션)
보존    해당 메시지의 expires_at + clock_skew_tolerance
GC      1분 주기. 만료 시각 기준으로만 삭제
```

### 축출 정책

```text
미만료 nonce는 절대 축출하지 않는다 (MUST NOT)

상한(기본 100,000)에 도달하면
  → 새 Grant 수락을 거부한다
  → 과부하를 risk signal로 보고한다
```

v5 초안은 "오래된 것부터 제거"였다. **아직 유효한 nonce가 밀려나면 replay 창이 열린다.**
축출이 아니라 **거부**가 안전한 실패 방향이다.

`nonce`는 CSPRNG로 생성한 **16바이트**여야 한다(MUST).

### ★ 10.1 시계 되감김 중에는 GC 하지 않는다 (2026-08-16 추가)

독립 검수 설계 검토에서 나온 것이다. 원 지적은 "이 부분은 문서에 규정되어 있지
않아 확신 없음" 이었다.

```text
1. 시계가 앞으로 튄다 (NTP 보정 등)
2. GC 가 "만료됐다" 며 항목을 지운다
3. 시계가 다시 뒤로 돌아온다
4. 지워진 nonce 가 "처음 보는 것" 이 된다   -> replay 창이 열린다
```

**마지막으로 관측한 시각보다 `now` 가 작으면 아무것도 지우지 않는다(MUST).**

★ 안전한 실패 방향이 어느 쪽인지가 근거다.

```text
지우지 않으면   캐시가 커진다 -> 상한 도달 -> CacheFull 로 **드러난다**
지우면          replay 가 **조용히** 통과한다
```

되감김 횟수는 **운영 신호로 노출한다** — 0이 아니면 시계 동기화 문제다.
되감김이 끝나면 GC 는 다시 동작해야 한다(영구 정지하면 캐시가 무한히 커진다).

### 10.2 현재 구현 상태

```text
crates/crypto/src/replay.rs   InMemoryReplayGuard
```

★ **영속되지 않는다.** 프로세스가 재시작하면 캐시가 비고
**재시작 직후 replay 창이 열린다.** `is_durable()` 이 `false` 를 반환하는 것이 그 신호다.

★ **전역 상한**을 쓴다 — device 별 quota 가 없어
한 device 가 다른 device 의 nonce 공간을 소진시킬 수 있다.
`global_capacity_lets_one_device_starve_others` 테스트가 그 사실을 고정한다
(통과가 곧 "아직 못 막는다" 는 뜻이다).

영속 저장소는 미구현이다 — 실행계획 v2 T4 3단계.

---

## 11. 키

| 키 | 알고리즘 | 보관 |
|---|---|---|
| Owner Key | Ed25519 | 오프라인 권장 |
| Owner Recovery Key | Ed25519 | Owner Key와 **다른 매체**. Genesis에 필수 등록 |
| Device Key | Ed25519 | K1 이상 (§계획서 7.3.1) |
| Coordinator Key | Ed25519 | K1 이상 필수 |
| Release Signing Key | Ed25519 | 오프라인 / HSM |

- 개인키를 로그·에러 메시지·텔레메트리에 출력하면 안 된다(MUST NOT).
- `K0`(평문 파일)는 기본 거부한다. 사용자가 명시적으로 허용해야만 동작한다.
- 키 회전 중에는 구 키와 신 키가 **24시간 grace period** 동안 병존한다.

---

## 12. 테스트 벡터

### 12.1 생성 방법

테스트 벡터는 **손으로 쓰지 않는다.** 참조 구현으로 생성해 저장소에 고정한다.

```bash
python tools/canonical/reference_canonical.py --emit-vectors > tests/vectors/canonical_v1.json
```

**이 문서에 해시 값을 문자열로 적지 않는다.** 지어낸 값이 규범이 되면 최악이기 때문이다.
값의 원본은 `tests/vectors/canonical_v1.json` 하나뿐이며, Rust 구현은 이 파일에 대해 검증한다.

### 12.2 필수 벡터 (모두 포함해야 한다)

| # | 벡터 | 검증하는 것 |
|---|---|---|
| 1 | 최소 `JobManifest` (필수 필드만) | 기본값 생략 (규칙 b) |
| 2 | 모든 필드가 채워진 `JobManifest` | 필드 순서 (규칙 a) |
| 3 | `env_vars` 3개를 **서로 다른 삽입 순서**로 만든 2개 메시지 | map 정렬 (규칙 c) — 두 canonical이 **동일해야** 한다 |
| 4 | `args` 순서를 바꾼 2개 메시지 | repeated 순서 유지 (규칙 d) — 두 canonical이 **달라야** 한다 |
| 5 | 중첩 3단계 메시지 | 재귀 적용 (규칙 f) |
| 6 | 빈 repeated / 빈 map / 0값 / UNSPECIFIED enum | 기본값 생략 (규칙 b) |
| 7 | 같은 메시지 100회 인코딩 | 결정론성 — 100개 바이트열이 전부 동일 |
| 8 | `submitter_signature`가 채워진 메시지 | 서명 필드 제외 (규칙 i) — 비어 있을 때와 canonical이 **동일해야** 한다 |
| 9 | non-minimal varint를 포함한 바이트열 | 파싱 거부 (규칙 e) |
| 10 | 같은 canonical, 다른 domain_tag | 서명이 **교차 검증되지 않아야** 한다 |
| 11 | 같은 canonical, 다른 schema_version | 서명이 **달라야** 한다 |
| 12 | `schema_version = 999` 메시지를 v1 검증자에 입력 | `SCHEMA_TOO_NEW` 반환 |
| 13 | Merkle root: 청크 1개 / 2개 / 3개(홀수) | 홀수 노드 승격 (§6.3) |

### 12.3 교차 구현 검증

Rust(`crates/protocol`)와 Python 참조 구현(`tools/canonical/`)이 **같은 입력에 같은 canonical bytes**를 내야 한다. CI에서 매 커밋 검사한다.

```bash
cargo test -p gputeer-protocol canonical_vectors
python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
```

---

## 13. 구현 지침

### 13.1 prost 기본 인코더를 서명에 쓰면 안 된다

`prost::Message::encode()`는 §3의 규칙을 보장하지 않는다.
`canonical_encode`를 **별도로 구현한다(MUST).**

#### 근거 (2026-08-16 실측 · `docs/evidence/DoD-02_prost_연동_계층.md`)

★ **근거를 정확히 적는다. "prost 출력은 canonical 과 다르다"는 틀린 설명이다.**

map 이 없는 단순 메시지에서는 prost 도 필드 번호 오름차순 · 기본값 생략 ·
최소 varint 를 쓰므로 **두 인코딩이 바이트 단위로 일치한다** (실측: 113B / 113B / 동일).

문제는 **map** 이다. prost 는 map 을 `HashMap` 으로 생성하고 순회 순서대로 쓴다.
같은 내용의 메시지를 500회 재구축한 실측:

```text
prost::Message::encode      495 종의 서로 다른 바이트
canonical_encode              1 종
```

→ prost 로 서명하면 **같은 매니페스트가 매번 다른 서명을 갖고 검증이 랜덤하게 실패한다.**
이것이 §3 규칙 c(map key 정렬)가 존재하는 이유이며, 구현에서는
prost 의 `HashMap` 을 `BTreeMap` 으로 정규화하는 지점이 그 규칙을 보장하는 **유일한 곳**이다.

권장 구조:

```text
crates/protocol/src/canonical.rs
    trait CanonicalEncode { fn canonical(&self) -> Vec<u8>; }
    → prost-build 플러그인으로 자동 생성하거나
    → 서명 대상 메시지에만 수동 구현 (17개뿐이므로 현실적)

crates/protocol/src/signing.rs
    fn sig_input(domain: DomainTag, schema_version: u32, canonical: &[u8]) -> Vec<u8>
    fn sign<M: CanonicalEncode>(key: &SigningKey, msg: &M) -> Signature
    fn verify<M: CanonicalEncode>(key: &VerifyingKey, msg: &M) -> VerifyOutcome
```

#### 채택한 방식 — 수동 구현 (2026-08-16)

위 두 선택지 중 **수동 구현**을 택했다. 구현은 `crates/protocol/src/to_fields.rs`.

```text
자동 생성   새 필드가 조용히 서명 대상에 들어가거나 빠지는 것을 숨긴다
수동 구현   어떤 필드가 서명에 들어가는지 사람이 눈으로 확인할 수 있다   <- 채택
```

수동 구현의 위험은 **field number 오타**다. 타입 불일치와 없는 필드는 컴파일러가
잡지만 번호 오타는 못 잡는다 — 코드는 돌고, 서명도 만들어지고, 자기 자신과는
검증도 통과하며, **다른 구현체와 붙는 순간에만 깨진다.**

→ `crates/protocol/tests/field_number_audit.rs` 가 `.proto` 소스와 구현 소스를
둘 다 파싱해 번호↔이름을 대조한다. **이 테스트 없이 수동 구현을 하면 안 된다(MUST).**

#### 서명 대상에서 빠진 필드는 명시적으로 선언한다 (MUST)

서명 대상에서 조용히 빠진 필드는 **위조 가능한 필드**다.
아직 canonical 에 넣지 못한 필드는 `UNIMPLEMENTED_FIELDS` 에 반드시 등록하고,
테스트가 **선언되지 않은 누락을 실패시킨다.**

### 13.2 타입 수준 강제

서명되지 않은 메시지가 로직에 흘러들지 않도록 래퍼 타입을 쓴다.

```rust
/// 서명 검증을 통과한 메시지만 이 타입이 될 수 있다.
pub struct Verified<M>(M);

impl<M: CanonicalEncode> Verified<M> {
    pub fn check(msg: M, key: &VerifyingKey) -> Result<Self, VerifyOutcome> { … }
    pub fn get(&self) -> &M { &self.0 }
}
```

**스케줄러·실행기 등 상위 코드는 `Verified<JobManifest>`만 받는다.**
`JobManifest`를 직접 받는 함수를 만들지 않는다. §8의 "검증 전 필드 사용 금지"를 컴파일러가 강제하게 한다.

### 13.3 금지 사항

```text
MUST NOT  float / double 필드 추가
MUST NOT  packed repeated 사용
MUST NOT  서명 필드 번호로 90 이외의 값 사용
MUST NOT  메시지 안에 자신의 해시를 저장
MUST NOT  domain_tag 없이 서명
MUST NOT  검증 실패를 경고로 강등하고 진행
MUST NOT  SCHEMA_TOO_NEW를 VALID로 취급
```

---

## 14. 미해결 / 후속

| # | 항목 | 상태 |
|---|---|---|
| 1 | 임계 서명(2-of-3 Coordinator verdict)의 구체 방식 | **미정.** 단순 서명 배열 vs FROST 등 임계 서명 스킴. M16 전까지 결정 |
| 2 | 감사 로그 엔트리의 Merkle 체인 형식 | M17에서 확정 |
| 3 | Post-quantum 전환 경로 | 장기. `SignatureAlgorithm` enum이 확장 지점 |
| 4 | 서명 대상 메시지 17개의 canonical 구현 자동 생성 | prost-build 플러그인 검토 중. 수동 구현으로 시작 가능 |

**1번이 열려 있는 동안 `QuarantineDevice.verdict_signatures`는 "서명 배열 + 각각 독립 검증"으로 구현한다.**
임계 서명으로 바꿀 때 `schema_version`을 올린다.
