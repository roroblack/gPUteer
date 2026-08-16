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
| **i** | 서명 필드(field 90)와 계산으로 도출되는 해시 필드는 **제외**한다 |

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
| `ExecutionGrant` | `gputeer/v1/grant` |
| `Lease` | `gputeer/v1/lease` |
| `RenewLeaseRequest` | `gputeer/v1/lease-renew` |
| `RevokeLeaseNotice` | `gputeer/v1/lease-revoke` |
| `CheckpointManifest` | `gputeer/v1/checkpoint` |
| `ReplicaAck` | `gputeer/v1/replica-ack` |
| `ArtifactRef` | `gputeer/v1/artifact` |
| `AttemptReport` | `gputeer/v1/attempt-report` |
| `CanonicalDecision` | `gputeer/v1/canonical` |
| Genesis Manifest | `gputeer/v1/genesis` |
| 멤버십 action | `gputeer/v1/membership` |
| 정책 변경 | `gputeer/v1/policy` |
| Quarantine verdict | `gputeer/v1/quarantine` |
| 감사 로그 엔트리 | `gputeer/v1/audit` |
| Release Manifest | `gputeer/v1/release` |
| Invite Bundle | `gputeer/v1/invite` |

**한 문맥의 서명을 다른 문맥에서 검증하면 domain_tag가 달라 반드시 실패한다.**
이것이 없으면 예컨대 Lease 서명을 Manifest 서명으로 재사용하는 공격이 가능하다.

새 서명 대상 메시지를 추가할 때는 **반드시 새 domain_tag를 이 표에 등록해야 한다(MUST).**

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
proto/SCHEMA_FINGERPRINT.txt                      66개 메시지 · 389개 필드
crates/protocol/tests/schema_fingerprint.rs       대조. 다르면 실패

갱신: UPDATE_SCHEMA_FINGERPRINT=1 cargo test -p gputeer-protocol --test schema_fingerprint
```

스키마가 바뀌면 테스트가 **무엇이 추가/삭제됐는지 출력하며 실패**하고,
둘 중 하나를 강제한다.

```text
(a) 필드 추가 · 타입 변경 · 의미 변경   -> schema_version 을 올린다
(b) 그 외 (주석 · 서식 · 파서 무관 변경) -> 지문만 갱신하고 사유를 커밋에 적는다
```

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
| Genesis / Release Manifest | ❌ 미적용 | ❌ (무기한) | — |

```text
clock_skew_tolerance = 60초 (프로토콜 상수)

단수명:  |now − issued_at| <= clock_skew_tolerance  AND  now < expires_at
장수명:  now < expires_at                            (issued_at 은 검사하지 않음)
```

Job은 큐에서 수 시간 대기하는 것이 **정상 동작**이다(계획서 §13.6 aging queue).
따라서 Manifest에 skew 규칙을 걸면 안 된다.

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
