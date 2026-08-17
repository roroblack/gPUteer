---
id: DoD-05
claim: "signing.md §5 domain_tag 17종 중 9종이 ToCanonicalFields 로 구현되었고 참조 구현과 바이트 단위로 일치한다. 규칙 j(부호 있는 정수)가 신설되어 기존 벡터를 바꾸지 않고 int64 를 결정론적으로 인코딩한다. 중첩 서명 메시지의 규칙 i 재귀 적용 결과가 테스트로 고정되었다"
status: PASS
commit: 8ca27992e840b8aa64221615ecb5faf3524f435e
binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 / prost 0.14 / python 3.12.7"
  note: "라이브러리 크레이트라 실행 바이너리 없음"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1 (규칙 j 추가 · §5.1 · §9.1 신설)"
  vectors: "tests/vectors/canonical_v1.json (28건, 20 -> 28)"
  schema_fingerprint: "blake3-256 0a34709f5599658f071ed8ac6df7c0ccca441cba89ce640db9ce061aa7866bcf (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "해당 없음 - 로컬 단위 테스트"
command: |
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --emit-vectors > tests/vectors/canonical_v1.json
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  cargo test --workspace -- --nocapture
  cargo build --workspace --all-targets
raw_output: |
  python reference_canonical.py --self-test     all checks passed (12/12)
  python reference_canonical.py --verify        vector cross-checks: OK

  기존 벡터 20건 중 canonical 이 바뀐 것: 없음     <- 규칙 j 추가가 회귀를 만들지 않았다
  신규 벡터: 8건 (v20 · v20a · v20b · v21 · v22 · v22b · v23 · v24)

  Running tests\t1_signing_targets.rs
  domain 17종 — 구현 9 · proto 메시지 없음 4
    미구현 gputeer/v1/grant             ExecutionGrant
    미구현 gputeer/v1/membership        AddMember 외 5종
    미구현 gputeer/v1/policy            UpdatePolicy
    미구현 gputeer/v1/quarantine        QuarantineDevice 외 1종
    미구현 gputeer/v1/genesis           ★ proto 메시지 없음 — 스펙 공백
    미구현 gputeer/v1/audit             ★ proto 메시지 없음 — 스펙 공백
    미구현 gputeer/v1/release           ★ proto 메시지 없음 — 스펙 공백
    미구현 gputeer/v1/invite            ★ proto 메시지 없음 — 스펙 공백
  test result: ok. 11 passed; 0 failed; 0 ignored

  Running tests\field_number_audit.rs
  ReportedMetric: proto 3개 필드 중 3개 서명 대상
  CheckpointFile: proto 5개 필드 중 5개 서명 대상
  ResumeCompleteness: proto 8개 필드 중 8개 서명 대상
  CheckpointManifest: proto 14개 필드 중 13개 서명 대상
  ReplicaAck: proto 11개 필드 중 10개 서명 대상
  ArtifactRef: proto 12개 필드 중 11개 서명 대상
  AttemptReport: proto 14개 필드 중 13개 서명 대상
  CanonicalDecision: proto 9개 필드 중 8개 서명 대상
  ProgressReport: proto 5개 필드 중 5개 서명 대상
  RenewLeaseRequest: proto 8개 필드 중 7개 서명 대상
  RevokeLeaseNotice: proto 6개 필드 중 5개 서명 대상
  test result: ok. 8 passed; 0 failed; 0 ignored

  cargo test --workspace     전체 117 passed / 0 failed
  cargo build --all-targets  경고 0건
artifacts:
  - docs/evidence/_raw/DoD-05_test_output.txt
  - crates/protocol/tests/t1_signing_targets.rs
  - crates/protocol/src/to_fields.rs
  - tools/canonical/reference_canonical.py
  - tests/vectors/canonical_v1.json
negative_tests:
  - "★ rule_j_sign_affects_canonical: 같은 절대값의 +1000 / -1000 이 서로 다른 canonical 을 내는지 확인. 반영되지 않으면 지표의 부호를 뒤집어도 서명이 통과한다. 추가로 음수가 정확히 8바이트 더 긴지 검사 — zigzag 로 인코딩됐다면 길이가 같았을 것이다"
  - "rule_j_handles_i64_min_without_panic: i64::MIN 은 절대값을 취할 수 없다. 순진한 구현이 여기서 패닉한다. MIN/MIN+1/-1/1/MAX 전부 확인"
  - "rule_j_zero_is_omitted: 명시적 0 이 규칙 b 로 생략되는지 확인"
  - "★ nested_signature_is_excluded_from_outer_canonical: 중첩 ReplicaAck 의 서명만 바꿔도 바깥 canonical 이 같음을 확인. 규칙 i 재귀의 필연적 결과이며, 검증자가 중첩 서명을 독립 검증해야 하는 이유다"
  - "★ nested_message_content_does_affect_outer_canonical: 위 테스트만 있으면 '중첩 전체가 무시되는' 결함과 구분되지 않는다. failure_domain 변경 · fsynced 끄기 · replica 제거가 각각 canonical 을 바꾸는지 확인"
  - "checkpoint_file_order_is_preserved: 파일 순서를 뒤집으면 canonical 이 달라야 한다 (규칙 d — proto 주석이 '정렬하지 않는다'고 명시)"
  - "renew_lease_request: nonce 를 바꾸면 canonical 이 달라야 한다. 서명 밖이면 재전송 시 nonce 만 갈아끼워 replay 캐시를 우회할 수 있다"
  - "revoke_lease_notice: cause 를 바꾸면 canonical 이 달라야 한다. OWNER_PREEMPT(소유자 주권)를 다른 사유로 바꿔치기하는 것을 막는다"
  - "domain_coverage_is_explicit: 구현 9종 · proto 메시지 없음 4종을 숫자로 고정. 늘어나도 줄어들어도 실패하므로 후퇴를 잡는다"
  - "field_number_audit 11개 메시지 확장: T1 메시지 전부가 서명 필드를 뺀 전 필드를 서명하는지 proto 소스와 대조"
limitations:
  - "★ 17종 중 9종만 구현했다. ExecutionGrant(grant) · AddMember 외 5종(membership) · UpdatePolicy(policy) · QuarantineDevice 외 1종(quarantine) 은 메시지가 있으나 미구현이다 — 실행계획 v2 T1b"
  - "★ genesis · audit · release · invite 4종은 proto 메시지 자체가 없다. signing.md §5 가 존재하지 않는 메시지의 domain_tag 를 등록해 두고 있다. 스펙 공백이며 §5.1 에 기록했다"
  - "★ 이번에 구현한 9종 중 6종(CheckpointManifest · ReplicaAck · ArtifactRef · AttemptReport · CanonicalDecision · RevokeLeaseNotice)은 signing.md §9 시각 정책 표에 없고 expires_at 필드조차 없다. Lifetime 을 추측으로 정하지 않기 위해 Signable 을 구현하지 않았다 — 따라서 이들은 아직 verify() 를 통과할 수 없다. §9.1 에 결정할 질문을 적었다"
  - "membership · policy · quarantine 은 여러 메시지가 하나의 domain_tag 를 공유한다. 그 사이의 서명 재사용은 canonical 차이로 실질 차단되나, domain 분리가 아니라 필드 차이에 의존하는 방어다. T1b 에서 재검토가 필요하다"
  - "규칙 j 는 int64 만 실측 검증했다. int32 의 부호 확장 경로는 현 스키마에 int32 필드가 없어 실행되지 않는다 — 코드 주석과 규범에만 적혀 있고 테스트가 없다"
  - "ControlAction 의 oneof 하위 메시지들은 ToCanonicalFields 대상이 아니다. 각 하위 메시지가 자기 서명(90)을 갖는 구조라 개별 구현이 필요하다"
  - "Ed25519 실제 서명·검증을 이 9종에 대해 수행하지 않았다. canonical 바이트 생성까지만 확인했다 (Signable 미구현 때문)"
  - "Windows 단일 플랫폼"
decision: "signing.md 에 규칙 j 를 신설했다 — 규범 추가이나 기존 벡터 20건이 하나도 바뀌지 않아 회귀가 없다. §5.1 · §9.1 로 스펙 공백 2건을 기록했다. §9 표가 완성되기 전에는 6종의 Signable 을 구현하지 않는다 (추측 금지). 다음: T1b(나머지 4종) -> T2(§9 결정)"
---

# DoD-05 · T1 서명 대상 확장

## 무엇을 입증하려 했는가

`DoD-03` · `DoD-04` 가 모두 limitations 에 남긴 것이다.

> 17종 서명 대상 메시지 중 13종만 `ToCanonicalFields` 를 구현했다.
> `artifact.proto` / `control.proto` 의 서명 대상은 손도 대지 않았다.

★ 정확히는 **메시지 13종**이지만 **domain 은 2종**뿐이었다
(나머지 11종은 `JobManifest`·`Lease` 의 중첩 부품이다).
이 검증이 domain 커버리지를 **2 → 9** 로 올린다.

## ★ 발견 1 — 부호 있는 정수 규칙이 없었다

`AttemptReport` 를 구현하려다 막혔다.

```text
proto/artifact.proto:234   int64 value_micro = 2;   (ReportedMetric)
```

**스키마 전체(66 메시지 · 389 필드)에서 유일한 부호 있는 필드인데,
하필 서명 대상 안에 있었다.**

`signing.md` §3 의 규칙 a~i 어디에도 부호 있는 정수 이야기가 없다.
규칙이 없으면 구현자마다 셋 중 하나를 고른다.

| 방식 | `-1` | 비고 |
|---|---|---|
| 2의 보수 u64 재해석 | 10바이트 | **proto3 표준** |
| zigzag (`sint` 방식) | 1바이트 | wire format 이탈 |
| 음수 거부 | — | 정상 값을 거부 |

### 선택 근거

**2의 보수를 택했다.** canonical 인코딩은 **유효한 protobuf 인코딩의 부분집합**이어야
한다는 설계 원칙 때문이다(§13.1 — 별도 인코더를 쓰되 wire format 은 벗어나지 않는다).
`int64` 의 proto3 wire format 이 2의 보수이므로 다른 선택은 wire format 을 벗어난다.

```text
음수 int64   항상 정확히 10바이트. u64 재해석 값의 최단 varint 이므로 규칙 e 와 무모순
int32 음수   ★ 먼저 64비트로 부호 확장한다. protobuf 의 유명한 함정 — int32 -1 도 10바이트
기본값 0     규칙 b 로 생략. -0 이 없으므로 §g 의 -0.0 문제 없음
```

`sint*`(zigzag) · `fixed*` · `sfixed*` 는 **금지**했다 — 같은 값의 표현이 둘 이상 생기거나
바이트 순서가 개입하면 서명 우회 여지가 생긴다(규칙 e·g 와 같은 이유).

### ★ 회귀가 없다

```text
기존 벡터 20건 중 canonical 이 바뀐 것: 없음
```

규칙 추가가 이미 서명된 메시지의 바이트를 바꾸지 않는다.
**바꿨다면 `schema_version` 을 올려야 했을 것이다** (§7.3).

## ★ 발견 2 — domain_tag 4종은 메시지가 없다

`signing.md` §5 는 **존재하지 않는 메시지의 domain_tag 를 등록해 두고 있다.**

```text
gputeer/v1/genesis · gputeer/v1/audit · gputeer/v1/release · gputeer/v1/invite
```

또한 세 tag 는 **여러 메시지가 공유**한다.

```text
membership   AddMember · RemoveMember · ApproveDevice · RevokeDevice
             · ChangeCoordinatorSet · RotateOwnerKey
policy       UpdatePolicy
quarantine   QuarantineDevice · ReleaseQuarantine
```

★ 한 tag 를 여러 메시지가 공유하면 **그 사이에서는 domain 분리가 없다.**
`AddMember` 서명을 `RemoveMember` 로 재사용하는 것은 canonical 차이로 실질 차단되지만,
**domain 분리가 아니라 필드 차이에 의존하는 방어**다. T1b 에서 재검토한다.

→ `signing.md` §5.1 에 기록.

## ★ 발견 3 — §9 시각 정책 표에 6종이 없다

이번에 구현한 9종 중 **6종이 §9 표에 없고, `expires_at` 필드조차 없다.**

```text
CheckpointManifest    created_at_unix_ms(30) 만
ReplicaAck            acked_at_unix_ms(30) 만
ArtifactRef           created_at_unix_ms(21) 만
AttemptReport         issued_at_unix_ms(40) 만
CanonicalDecision     decided_at_unix_ms(13) 만
RevokeLeaseNotice     issued_at_unix_ms(5) 만
```

**만료 개념이 정의되지 않았다.**

### 추측으로 채우지 않았다

`CLAUDE.md` §1 — "값을 모르면 **비워 둔다.** 추정으로 채우면 그 오류가 조용히
스케줄링 결정까지 간다."

`Lifetime` 을 잘못 고르면:

```text
ShortLived 로 잘못 고르면    정상 메시지가 전부 거부된다 (§9 의 경고)
Perpetual 로 잘못 고르면     만료된 증거가 영원히 유효해진다
```

→ 이 6종은 `ToCanonicalFields` 만 구현하고 **`Signable` 을 구현하지 않았다.**
따라서 **이들은 아직 `verify()` 를 통과할 수 없다.** 그것이 정확한 현재 상태다.

→ `signing.md` §9.1 에 결정해야 할 질문 3개를 적고 **T2 로 미뤘다.**

## ★ 발견 4 — 중첩 서명 메시지의 귀결

`ArtifactRef.replicas` 는 **각자 서명된** `ReplicaAck` 들이다.
규칙 i 는 재귀 적용되므로 **중첩 서명(90)은 바깥 canonical 에 들어가지 않는다.**

```text
ReplicaAck 의 holder_signature 를 0xAA -> 0xBB 로 바꾼다
  -> ArtifactRef 의 canonical 은 **완전히 동일하다**
  -> ArtifactRef 의 producer_signature 검증은 **통과한다**
```

**결함이 아니라 규칙 i 의 필연적 결과다.** 그러나 결과를 알아야 한다.

```text
=> 검증자는 중첩 서명 메시지를 **독립적으로 검증해야 한다(MUST)**
=> 하지 않으면 ReplicaAck 가 위조된 채로 세어지고,
   "REPLICATED(n)" 이 거짓이 된다 — 즉 durability 주장이 무너진다
```

벡터 `v22` / `v22b` 와 테스트 2건이 이 사실을 고정한다.

★ 두 번째 테스트(`nested_message_content_does_affect_outer_canonical`)가 없으면
**"중첩 전체가 무시되는" 결함과 구분되지 않는다.** `failure_domain` 변경 ·
`fsynced` 끄기 · replica 제거가 각각 canonical 을 바꾸는지 함께 확인했다.

## 결과

```text
domain 커버리지     2 -> 9 / 17
벡터                20 -> 28건
테스트              105 -> 117 passed / 0 failed
빌드 경고           0
스키마 지문         변경 없음 (proto 를 건드리지 않았다)
```

## 이 실험이 증명하지 "않는" 것

- **17종 중 9종만 구현했다.** grant · membership · policy · quarantine 은 메시지가
  있으나 미구현이다 (T1b).
- **4종은 proto 메시지 자체가 없다.**
- ★ **6종은 `Signable` 이 없어 `verify()` 를 통과할 수 없다.**
  canonical 바이트를 만들 수 있을 뿐이다.
- **규칙 j 는 `int64` 만 실측했다.** `int32` 부호 확장 경로는 현 스키마에
  `int32` 필드가 없어 **실행되지 않는다** — 규범과 주석에만 있고 테스트가 없다.
- **Ed25519 실제 서명·검증을 이 9종에 대해 하지 않았다.**
- Windows 단일 플랫폼.

## 결정

1. **`signing.md` 에 규칙 j 를 신설한다.** 규범 추가이나 기존 벡터 20건이
   하나도 바뀌지 않아 **회귀가 없다.**
2. **§5.1 · §9.1 로 스펙 공백 2건을 기록한다.**
3. **§9 표가 완성되기 전에는 6종의 `Signable` 을 구현하지 않는다.** 추측 금지.
4. 다음: **T1b**(나머지 4종) → **T2**(§9 결정 + 단수명 메시지).

관련: `docs/evidence/DoD-03_서명대상_완전성.md` · `docs/evidence/DoD-04_ed25519_검증순서.md` ·
`docs/protocol/signing.md` §3.1(j) · §5.1 · §9.1 ·
`docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` §7

---

## ★ 이후 변경 (2026-08-17 23:55) — claim 의 수치가 stale, 이름·limitations 다수 정정

독립 검수(`agent:codex-cli`, read-only)가 재검수해 `CHANGES_REQUESTED`
로 판정했다. claim 첫 문장의 수치 자체가 지금 코드와 어긋난다.

### claim 을 이렇게 좁혀 읽는다

원래 claim: "signing.md §5 domain_tag **17종 중 9종**이 구현됐다."
지금은:

- §5 domain tag 는 17종이 아니라 **23종**이다(`docs/protocol/signing.md:245-276`,
  ADR-028 이 17→23 으로 늘렸다).
- 그 중 proto 메시지가 있고 `ToCanonicalFields` 가 구현된 domain 은
  **19종**이다(`crates/protocol/tests/t1_signing_targets.rs:387-445`).
- `Signable` 구현은 **10종**이다(`crates/protocol/src/signable.rs:40-407`).

claim 첫 문장을 "23종 중 19종이 구현되었다"로 정정해 읽는다. 규칙 j
(부호 있는 정수)와 중첩 서명 재귀 부분은 지금도 코드로 확인된다
(`crates/protocol/src/canonical.rs:95-106`,`210-213`,
`crates/protocol/tests/t1_signing_targets.rs:84-154`,`198-238`).

### negative_tests 이름 정정

| 원래 표기 | 실제 함수명 |
|---|---|
| `renew_lease_request` | `renew_lease_request_matches_reference`(`t1_signing_targets.rs:326`) |
| `revoke_lease_notice` | `revoke_lease_notice_matches_reference`(`:359`) |
| "field_number_audit 11개 메시지 확장" | 함수명이 아니다 — 지금은 `artifact_and_lease_field_numbers_match_proto` 와 `grant_and_control_field_numbers_match_proto` 로 분리되어 있다(`field_number_audit.rs:257-275`) |

나머지는 실재를 확인했다: `rule_j_sign_affects_canonical`(`:102`),
`rule_j_handles_i64_min_without_panic`(`:146`),
`rule_j_zero_is_omitted`(`:127`),
`nested_signature_is_excluded_from_outer_canonical`(`:198`),
`nested_message_content_does_affect_outer_canonical`(`:217`),
`checkpoint_file_order_is_preserved`(`:303`),
`domain_coverage_is_explicit`(`:393`).

### stale limitations

| 원래 서술 | 지금 |
|---|---|
| "17종 중 9종만 구현"(`:76`) | ★ 거짓이다. 23종 중 19종 |
| "6종이 Signable 미구현이라 verify() 를 통과할 수 없다"(`:78`) | ★ 거짓이다. 6종 모두 `Lifetime::Evidence` 로 `Signable` 구현되어 있다(`crates/protocol/src/signable.rs:229,264,291,322,349,383` — 정확히 6개 `Lifetime::Evidence` 선언 확인). `verify()` 도 Evidence 타입은 만료 검사를 생략하는 경로를 갖는다(`crates/protocol/src/signing.rs:763-767`) |
| "membership/policy/quarantine 이 domain tag 를 공유한다"(`:79`) | ★ 거짓이다. ADR-028 이후 domain 이 분리됐고 canonical.rs 의 enum 에도 별도 variant 가 있다(`crates/protocol/src/canonical.rs:297-305`) |
| "ControlAction 의 oneof 하위 메시지들은 대상이 아니다"(`:81`) | ★ 2026-08-17 최초 정정 시 "하위 메시지들은 각자 구현됨"이라고 썼는데 **틀렸다** — 재검수가 직접 세어 지적했다. `ControlAction` 은 21개 oneof arm 을 갖고(`proto/control.proto:238-269`), 그 중 **9개**(멤버십·정책 그룹)만 `ToCanonicalFields` 가 구현되어 있다(`crates/protocol/src/to_fields.rs:694-818`). 나머지 **12개**(Job 수명주기·Lease·관측 결과)는 미구현이다(grep 0건 확인). 원래 limitation("개별 구현이 필요하다")이 실제로는 더 정확했다 — 지금은 "9/21 구현, 12개 미구현"으로 정정한다 |
| "Ed25519 를 이 9종에 대해 실행하지 않았다"(`:82`) | "이 evidence(DoD-05) 자체가 Ed25519 를 직접 실행하지 않았다"로 좁힌다 — Ed25519 verifier 자체는 지금 존재한다(`crates/crypto/src/lib.rs:120-125`) |

genesis/audit/release/invite proto 부재(`:77`), int32 경로 미실행(`:80`,
지금도 유효 — 실제 signed 정수 필드는 `ReportedMetric.value_micro`
하나뿐이다, `crates/protocol/src/to_fields.rs:284-312`), Windows
단일 플랫폼(`:83`) limitation 은 지금도 유효하다.

결정문의 "다음 T1b(나머지 4종) → T2"·"6종 Signable 미구현" 서술도
위 정정에 맞춰 stale 로 읽는다.

### 메타데이터

`vectors: 28건`(`:12`)은 지금과 다르다 — `tests/vectors/canonical_v1.json`
을 직접 파싱하면 지금 **40건**이다(`:7` 부터 시작하는 `vectors`
배열, `python -c "import json; print(len(json.load(open(...))['vectors']))"`
로 재확인). 이 세션에서 파일을 늘린 정확한 커밋 이력(어느 라운드가
몇 건씩 늘렸는지)까지는 추적하지 않았다 — "지금 40건" 이라는 사실만
확인된 것이고, 그 증가 과정의 파일:줄 근거는 **확인 안 됨**이다.
schema fingerprint(`:13`)는 지금 `proto/SCHEMA_FINGERPRINT.txt` 와
일치해 유효하다. `raw_output` 의 117 tests 는 당시(commit `8ca2799`)
실행 기록이다.

### review_outcome

`CHANGES_REQUESTED` → 위 정정으로 claim 수치·negative_tests 이름·
stale limitations 를 반영했다. 원본 YAML 은 당시 기록이므로 고치지
않는다.

★ 2026-08-18 00:10 두 번째 재검수 — `agent:codex-cli` 가 여전히
`CHANGES_REQUESTED` 를 냈다. **가장 중요한 지적**: "ControlAction
의 oneof 하위 메시지들은 각각 개별 구현되어 있다"는 서술이 틀린
사실이었다 — 재검수가 21개 arm 중 9개만 구현됨을 직접 세어 밝혔고,
이 세션도 grep 으로 재확인했다. 원래 evidence 의 limitation(`:81`,
"개별 구현이 필요하다")이 오히려 더 정확했다 — "9/21 구현, 12개
미구현"으로 다시 정정했다. vectors 메타데이터도 "40건" 이라는
사실만 재확인 가능하고 그 증가 이력은 확인 안 됨으로 명시했다. 이
세 번째 수정 자체는 아직 재검수를 거치지 않았다.
