---
schema_version: 2
id: DoD-03
claim: "JobManifest 와 Lease 의 서명 필드를 제외한 전 필드가 canonical 서명 대상에 포함되며, 각 필드가 실제로 서명 결과에 영향을 준다. Rust 와 Python 참조 구현이 전 필드 메시지에서 바이트 단위로 일치한다"
status: PASS
commit: 13795c604c74c5c9bb5bd0104a5338407d03f3d7

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-18T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "claim 범위 · negative_tests 실재성과 domain 수치(24종 중 20개 구현·ToCanonicalFields 42개) 재확인 · coverage 테스트 자동성 한계 · cargo test 재실행 확인"
review_artifact: "docs/evidence/_raw/DoD-03_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-03_v2_promotion_2026-08-18.txt"
raw_output_digest: "sha256:bc914c401f7f54ba27d2552a5e134f994fc80f344964dda819c583ef604a42b1"
raw_output_bytes: 884

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 / protoc (protoc-bin-vendored) / python 3.12.7"
  note: "라이브러리 크레이트라 실행 바이너리 없음"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1"
  vectors: "tests/vectors/canonical_v1.json (20 벡터, 12 -> 20)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 (순수 인코딩 로직)"
network_profile: "해당 없음 - 로컬 단위 테스트"
command: |
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --emit-vectors > tests/vectors/canonical_v1.json
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  cargo test --workspace -- --nocapture
raw_output: |
  python reference_canonical.py --self-test
    all checks passed  (12/12)
  python reference_canonical.py --verify tests/vectors/canonical_v1.json
    vector cross-checks: OK

  Running tests\field_number_audit.rs
  Digest: proto 2개 필드 중 2개 서명 대상
  CudaRequirement: proto 3개 필드 중 3개 서명 대상
  GpuRequest: proto 5개 필드 중 5개 서명 대상
  ResourceRequest: proto 5개 필드 중 5개 서명 대상
  WorkloadHint: proto 7개 필드 중 7개 서명 대상
  TarballPolicy: proto 3개 필드 중 3개 서명 대상
  ExecutionEnvironment: proto 14개 필드 중 14개 서명 대상
  DatasetRef: proto 6개 필드 중 6개 서명 대상
  NetworkPolicy: proto 3개 필드 중 3개 서명 대상
  ArtifactScope: proto 2개 필드 중 2개 서명 대상
  ResourceScope: proto 5개 필드 중 5개 서명 대상
  Lease: proto 15개 필드 중 14개 서명 대상          <- 나머지 1개는 서명 필드(90)
  JobManifest: proto 29개 필드 중 28개 서명 대상    <- 나머지 1개는 서명 필드(90)
  test result: ok. 7 passed; 0 failed; 0 ignored

  Running tests\prost_canonical.rs
  v02 전 필드: 896 bytes 일치
  전 필드 27개 전부 서명에 반영됨 (+ schema_version 은 sig_input)
  500회 재구축 — prost 서로 다른 인코딩 498종 / canonical 1종
  test result: ok. 25 passed; 0 failed; 0 ignored

  Running tests\canonical_vectors.rs    15 passed
  Running tests\durability_chaos.rs     16 passed
  Running tests\kill_chaos.rs            7 passed

  전체: 70 passed / 0 failed
artifacts:
  - docs/evidence/_raw/DoD-03_test_output.txt
  - tools/canonical/reference_canonical.py
  - tests/vectors/canonical_v1.json
  - crates/protocol/src/to_fields.rs
  - crates/protocol/tests/prost_canonical.rs
  - crates/protocol/tests/field_number_audit.rs
  - docs/evidence/_raw/DoD-03_v2_promotion_2026-08-18.txt
  - docs/evidence/_raw/DoD-03_review.txt
negative_tests:
  - "★ every_field_in_full_manifest_affects_canonical: 27개 필드를 하나씩 기본값으로 되돌려 canonical 이 반드시 변하는지 확인. 변하지 않는 필드는 서명 밖이며 위조 가능하다. 벡터 대조만으로는 '두 구현이 사이좋게 같은 필드를 빠뜨린' 경우를 못 잡으므로 이 테스트가 별도로 필요하다"
  - "network_policy_is_signed / artifact_scope_is_signed / lease_scope_is_signed: 보안 필드 3건이 각각 canonical 을 바꾸는지 개별 확인"
  - "execution_environment_is_signed / dataset_ref_is_signed / input_artifacts_are_signed_and_order_is_preserved: 나머지 3건"
  - "input_artifacts 순서 뒤집기: repeated message 는 정렬하지 않는다 (규칙 d). 순서가 다르면 canonical 이 달라야 한다"
  - "every_impl_is_audited: ToCanonicalFields 를 구현했는데 AUDITED 목록에 없는 메시지가 있으면 실패. 감사망에 조용히 구멍이 생기는 것을 막는다"
  - "unimplemented_field_list_is_empty: UNIMPLEMENTED_FIELDS 가 비어 있지 않으면 실패"
  - "missing_from_full() (참조 구현): '전체 필드' 벡터가 정말 전 필드를 채웠는지 벡터 생성 시점에 검사. v02 는 오랫동안 '모든 필드'라고 적혀 있었으나 실제로는 부분집합이었다"
  - "서명 필드(90)는 반대로 변하면 안 된다: 0x11 64바이트로 바꿔도 canonical 불변"
  - "prost_encode_is_not_deterministic_for_maps: 비공허성 단언 포함 (498종 vs 1종)"
limitations:
  - "17종 서명 대상 메시지 중 13종만 ToCanonicalFields 를 구현했다. artifact.proto / control.proto 의 서명 대상 메시지는 여전히 미구현이다. 이 evidence 의 완전성 주장은 JobManifest 와 Lease 에만 적용된다"
  - "★ every_field_in_full_manifest_affects_canonical 은 JobManifest 최상위 27개 필드만 검사한다. 중첩 메시지(예: ExecutionEnvironment 의 14개 필드) 각각이 서명에 영향을 주는지는 개별 검사하지 않았다. field_number_audit 이 번호-이름 대조는 하므로 누락은 잡히지만, '넣었는데 값이 반영 안 되는' 결함은 중첩 안에서는 미검증이다"
  - "Ed25519 서명·검증을 여전히 하지 않았다. sig_input 바이트 생성까지만 확인했다"
  - "SCHEMA_TOO_NEW 경로(signing.md §7.2)는 미구현이다. prost 가 unknown field 를 버리는 동작이 검증을 어떻게 깨는지 미검증 — P0-08 로 등록"
  - "Windows 단일 플랫폼에서만 실행했다"
  - "field_number_audit 의 파서는 정규식이다. oneof / reserved / 중첩 message 선언을 다루지 않는다. 현 스키마에 해당 구문이 없어 지금은 무해하다"
  - "서명 대상에 '포함되었다'는 것과 '검증자가 실제로 그 필드를 정책 판단에 쓴다'는 것은 다르다. 예컨대 network(54) 가 서명에 들어갔어도 Agent 가 그 정책을 강제하지 않으면 의미가 없다. 강제 계층은 미구현이다"
decision: "signing.md §3 규칙을 변경하지 않는다. JobManifest 와 Lease 의 서명 완전성이 확보되었으므로 DoD-02 가 제기한 '위조 가능한 보안 필드 3건' 은 해소되었다. 다음: artifact/control 서명 대상 구현 -> Ed25519 -> P0-08"
---

# DoD-03 · 서명 대상 완전성

## 무엇을 입증하려 했는가

`DoD-02` 가 찾은 것이다.

> **서명 대상에서 빠진 필드가 6개 있다. 그 중 3개가 보안 필드다.**
> 54 `network` · 55 `artifact_scope` · Lease 40 `scope`.
> 서명 밖에 있으면 중간자가 고쳐도 검증이 통과한다.

`DoD-02` 는 그것들을 `UNIMPLEMENTED_FIELDS` 로 **선언**했다.
선언은 눈에 보이게 만들 뿐 안전하게 만들지 않는다. 이 검증은 그것들을 **없앤다.**

## 어떻게 했는가 — 계약이 먼저다

`RULE.md` §3.5 대로 **참조 구현을 먼저 확장하고, 거기서 벡터를 생성한 뒤, Rust 를 맞췄다.**

```text
1. tools/canonical/reference_canonical.py 의 SCHEMAS 확장
2. 새 벡터 8건 생성 -> tests/vectors/canonical_v1.json  (12 -> 20건)
3. crates/protocol/src/to_fields.rs 구현
4. Rust 가 벡터와 일치하는지 대조
```

★ **구현에 맞춰 벡터를 고치지 않았다.** 그 순서를 뒤집으면 교차검증이 자기 확인이 된다.

## 발견 — v02 는 "모든 필드" 가 아니었다

`v02_full_manifest` 벡터의 설명은 이랬다.

> "모든 필드. field number 오름차순으로 직렬화되어야 한다 (규칙 a)"

**실제로는 16개 필드 부분집합이었다.** 참조 구현의 `SCHEMAS["JobManifest"]` 자체가
주석으로 "벡터 생성에 필요한 JobManifest 부분집합" 이라고 적혀 있었고,
그 사실이 벡터 설명과 어긋난 채로 남아 있었다.

**주장과 실제가 어긋나면 그 차이만큼은 아무도 검증하지 않는다.**

→ `missing_from_full()` 을 만들어 **벡터 생성 시점에 코드가 검사**하게 했다.
전 필드를 채우지 않으면 벡터 생성이 실패한다.

```python
_gap = missing_from_full("JobManifest", _full_manifest())
assert not _gap, "v02 가 전 필드를 채우지 않았다: %s" % ", ".join(_gap)
```

## 결과

### 서명 대상 커버리지

| 메시지 | proto 필드 | 서명 대상 | 빠진 것 |
|---|---|---|---|
| JobManifest | 29 | **28** | 서명 필드(90) 뿐 |
| Lease | 15 | **14** | 서명 필드(90) 뿐 |
| ExecutionEnvironment | 14 | 14 | — |
| DatasetRef | 6 | 6 | — |
| NetworkPolicy | 3 | 3 | — |
| ArtifactScope | 2 | 2 | — |
| ResourceScope | 5 | 5 | — |
| ResourceRequest / GpuRequest | 5 / 5 | 5 / 5 | — |
| WorkloadHint / CudaRequirement | 7 / 3 | 7 / 3 | — |
| TarballPolicy / Digest | 3 / 2 | 3 / 2 | — |

`UNIMPLEMENTED_FIELDS` 가 **비었다.**

### 전 필드 교차검증

```text
v02_full_manifest    896 bytes    Rust == Python  (BLAKE3 다이제스트까지)
v02b_full_lease      Rust == Python
```

**896바이트가 우연히 일치할 확률은 없다.** 두 독립 구현이 JobManifest 의
28개 필드 · 4단 중첩 · repeated message · map 정렬을 전부 같은 바이트로 낸다.

### ★ 벡터 대조만으로는 부족하다

벡터 대조는 "두 구현이 같다" 를 증명하지, **"두 구현이 옳다"** 를 증명하지 않는다.
참조 구현도 같은 필드를 빠뜨렸다면 **둘이 사이좋게 틀린 채로 일치한다.**

그래서 별도 테스트를 뒀다.

```text
every_field_in_full_manifest_affects_canonical
  27개 필드를 하나씩 기본값으로 되돌린다
  -> canonical 이 반드시 달라져야 한다
  -> 달라지지 않는 필드 = 서명 밖 = 위조 가능
```

결과: **27/27 전부 서명에 반영됨.**

추가로 두 가지를 함께 확인한다.

```text
schema_version(1)      canonical 이 아니라 sig_input 에 들어간다 (§4)
                       -> canonical 만 보면 놓친다. 별도 확인
submitter_signature(90) 반대로 **변하면 안 된다** (규칙 i)
                       -> 0x11 로 채워도 canonical 불변
```

### 감사망에 구멍이 생기지 않게

`every_impl_is_audited` — `ToCanonicalFields` 를 구현했는데 `AUDITED` 목록에
없는 메시지가 있으면 실패한다. 새 메시지를 추가할 때 field number 대조를
조용히 빠져나가지 못한다.

### 전체

```text
cargo test --workspace
  canonical_vectors     15 passed
  field_number_audit     7 passed
  prost_canonical       25 passed
  durability_chaos      16 passed
  kill_chaos             7 passed
  전체                  70 passed / 0 failed      (55 -> 70)

python reference_canonical.py --self-test   12/12
python reference_canonical.py --verify      vector cross-checks: OK
```

## 이 실험이 증명하지 "않는" 것

- **`artifact.proto` · `control.proto` 의 서명 대상은 여전히 미구현이다.**
  이 evidence 의 완전성 주장은 **JobManifest 와 Lease 에만** 적용된다.
- **중첩 메시지 내부 필드의 개별 영향은 검사하지 않았다.**
  `every_field_in_full_manifest_affects_canonical` 은 최상위 27개만 본다.
  `ExecutionEnvironment` 의 14개 필드 각각이 서명에 반영되는지는
  `field_number_audit` 의 번호-이름 대조로 간접 보증할 뿐이다.
- **Ed25519 를 여전히 하지 않았다.**
- **`SCHEMA_TOO_NEW` 미구현** — P0-08.
- ★ **"서명에 들어갔다" 와 "검증자가 그 필드를 실제로 강제한다" 는 다르다.**
  `network(54)` 가 서명에 들어갔어도 Agent 가 그 정책을 강제하지 않으면
  위조를 막은 것이지 정책을 시행한 것이 아니다. **강제 계층은 미구현이다.**
- Windows 단일 플랫폼.

## 결정

1. **`signing.md` §3 을 변경하지 않는다.**
2. `DoD-02` 가 제기한 **"위조 가능한 보안 필드 3건" 은 해소되었다.**
3. 다음 순서: `artifact.proto`/`control.proto` 서명 대상 → Ed25519 → P0-08.
4. **중첩 메시지 필드별 영향 검사**를 다음 작업에 포함한다 (limitations 2번).

관련: `docs/evidence/DoD-02_prost_연동_계층.md` · `docs/protocol/signing.md` §13.1

---

## ★ 이후 변경 (2026-08-17 22:40) — claim 범위가 넓게 읽혔다

독립 검수(`agent:codex-cli`, read-only)가 이 evidence 를 재검수해
`CHANGES_REQUESTED` 로 판정했다. DoD-04·P0-03 과 달리 이번엔 코드
결함이 아니라 **claim 문장이 지금 검사 범위보다 넓게 읽힌다**는
지적이다. 직접 대조해 확인했다.

### 1. vectors 메타데이터가 stale — 재현해 확인

frontmatter(`DoD-03:12`)는 "20 벡터" 라고 적었다. 지금 파일을 직접
파싱해 세었다.

```text
$ python -c "import json; print(len(json.load(open('tests/vectors/canonical_v1.json'))['vectors']))"
40
```

`DoD-06`(20→36)과 다른 후속 작업들이 벡터를 계속 늘렸는데 이 문서만
20에 머물러 있었다. 20 → **40** 으로 고친다.

### 2. claim의 "각 필드가 실제로 서명 결과에 영향을 준다" 는 최상위 필드에만 확인됐다

`every_field_in_full_manifest_affects_canonical`(`crates/protocol/tests/prost_canonical.rs:745`)
은 `JobManifest` 최상위 27개 필드를 하나씩 지워 canonical 이 변하는지
본다. `Lease` 는 `lease_scope_is_signed`(`prost_canonical.rs:389`) 하나로
`scope` 필드만 개별 확인한다 — `Lease` 의 나머지 최상위 필드나,
두 메시지의 **중첩 메시지 내부 필드**(예: `ExecutionEnvironment` 의
14개) 각각이 서명에 영향을 주는지는 개별 mutation 검사가 없다. 이
한계는 이 문서의 limitations 2번(`DoD-03:73`)이 이미 정직하게
적어 두었다 — **새로 발견된 것이 아니라, claim 문장 자체가 그 한계를
반영할 만큼 좁지 않았다**는 지적이다.

### claim 을 이렇게 좁혀 읽는다

> `JobManifest` 최상위 27개 필드 전부와 `Lease.scope` 는 mutation 으로
> 직접 확인됐다. `Lease` 의 나머지 최상위 필드와 모든 중첩 메시지
> 내부 필드는 `field_number_audit`(번호-이름 대조, `crates/protocol/tests/field_number_audit.rs:249-274`)
> 으로 **필드 자체가 빠지지 않았음**만 확인됐다 — "canonical 에 들어는
> 갔는데 값이 반영 안 되는" 결함까지는 중첩 내부에서 검증되지 않는다.

### 그 외 확인

- negative_tests 이름은 실재를 확인했다(`prost_canonical.rs:307,321,334,351,366,745`,
  `every_impl_is_audited`(`field_number_audit.rs:282`) — ★ 이 인용은
  바로 위 "claim 을 이렇게 좁혀 읽는다" 절의 `field_number_audit.rs:249-274`
  (번호-이름 대조)와 **다른 함수를 가리킨다.** 헷갈리지 않도록 명시한다:
  `:249-274` 는 `common_message_field_numbers_match_proto` 류의
  번호-이름 대조 테스트, `:282` 는 `every_impl_is_audited`(감사망
  등록 누락 검사) 다. 둘 다 negative_tests 목록에 실재한다.
  `prost_canonical.rs:279,852`).
- limitation "17종 중 13종만 구현"(`DoD-03:72`)은 **틀린 숫자가 됐다**
  — 지금 `ToCanonicalFields` 구현은 41개, domain coverage 는 23종 중
  19종이다(`crates/protocol/src/to_fields.rs`,
  `crates/protocol/tests/t1_signing_targets.rs:397-445`).
- "Ed25519 를 하지 않았다"(`:74`) — ★ 여기서 "하지 않았다"는 두 가지로
  읽힐 수 있다: "구현이 없다"(거짓 — `crates/crypto/src/lib.rs:58-64`
  에 `sign()` 이 있다, `DoD-04` 가 그 검증 경로까지 확인했다) 와
  "**이 evidence(DoD-03) 가 Ed25519 를 실행하지 않았다**"(참 — 위
  `raw_output`(`DoD-03:16-25`)은 canonical/sig_input 생성까지만
  본다, Ed25519 서명·검증 호출은 없다). limitation 은 후자로 좁혀
  읽는다 — DoD-03 자체가 Ed25519 를 실행했다는 근거는 없다.
- "SCHEMA_TOO_NEW 미구현"(`:75`) — 구현 자체는 지금 존재한다
  (`crates/protocol/src/signing.rs:741-744`). 다만 이것도 위와 같은
  구분이 필요하다: DoD-03 자체가 이 경로를 실행해 확인한 것은 아니다
  (그 검증은 `P0-08`·`DoD-04` 범위다).
- "강제 계층 미구현"(`:78`) 은 **여전히 대체로 참이지만 절반만** —
  `runtime-policy` 크레이트가 판정 계층을 추가했다
  (`crates/runtime-policy/src/network.rs:22-25` `OsFirewallBackend`
  trait, `:85-99` `NetworkPolicyCheck::decide`), 그러나 실제 OS 방화벽
  호출·커널 경로 잠금은 여전히 없다(`docs/history/HISTORY.md`
  "2026-08-17 21:30" 항목). "미구현" 을 "**판정 계층은 있으나 실제
  강제는 없다**" 로 좁힌다 — "완전히 미구현" 은 이제 부정확하다.

### review_outcome

★ 2026-08-17 22:40 최초 정정에 이어, `agent:codex-cli` 의 4건 일괄
최종 재검수가 다음을 지적했다: (a) `field_number_audit.rs:282` 인용은
필드번호 대조 테스트가 아니라 감사망 등록 여부만 확인하는
`every_impl_is_audited` 라며, 실제 필드번호 대조는 `:249-274` 라고
지적 — 위 "claim 을 이렇게 좁혀 읽는다" 절의 인용을 고쳤다. (b)
Ed25519/SCHEMA_TOO_NEW/runtime-policy limitation 정정이 "구현이
존재한다" 와 "이 evidence 가 실행해 확인했다" 를 충분히 구분하지
않아 과장으로 읽힐 수 있다고 지적 — 위 세 항목을 그 구분을 명시하도록
다시 썼다. `CHANGES_REQUESTED` 는 유지됐다. 원본 YAML 은 당시
기록이므로 고치지 않는다.

★ 2026-08-17 23:35 세 번째(마지막) 재검수 — `agent:codex-cli` 가
`DoD-04`·`DoD-06` 은 이번 라운드에서 `ACCEPTED` 를 줬지만, `DoD-03`
은 여전히 `CHANGES_REQUESTED` 를 유지했다. 남은 지적: 위 "그 외
확인" 절의 `field_number_audit.rs:282` 인용이, 바로 앞 문단의
`:249-274` 인용과 나란히 있어 **같은 결함이 또 남은 것처럼 읽혔다**
— 실제로는 서로 다른, 둘 다 진짜인 함수를 가리키고 있었을 뿐이다
(`every_impl_is_audited` vs `common_message_field_numbers_match_proto`
류). 두 인용을 명시적으로 구분해 고쳤다(위 "negative_tests 이름은
실재를 확인했다" 절 참조).

★ 2026-08-17 23:45 네 번째(최종) 재검수 — `agent:codex-cli` 가
**`ACCEPTED`** 로 판정했다. `field_number_audit.rs:249,259,269` 가
번호-이름 대조 진입점, `:282` 가 `every_impl_is_audited` 라는 구분이
정확한지 소스를 직접 열어(`grep -n '^fn '`) 확인했고, claim 범위·
vectors 40건·Ed25519/SCHEMA_TOO_NEW/runtime-policy 구분까지 이
문서의 처음부터 끝까지 다시 훑어 더 남은 문제가 없다고 판정했다.
"확인 안 됨: 없음."

이로써 `DoD-03`·`DoD-04`·`DoD-06`·`P0-03` 4건 모두 addendum 이
독립 재검수 `ACCEPTED` 를 받았다 — `DoD-03` 은 4라운드, 나머지는
1~3라운드 만에. 문서 전체를 schema v2 로 승격하는 것은 여전히 별도
작업이다(frontmatter 를 v1 → v2 로 바꾸고 executor/reviewer 메타데이터
· raw_output digest 를 정식으로 채우는 일) — 아직 하지 않았다.

---

## ★ 이후 변경 (2026-08-18) — schema v2 승격 전 재확인, 수치 재정정

`DoD-01`·`DoD-02` 를 schema v1 → v2 로 승격하며 이미 두 번 겪은
패턴이 `DoD-03` 에도 그대로 나타났다. 새로운 독립 검수
(`agent:codex-cli`, read-only, v2 승격용 재검수)가 `CHANGES_REQUESTED`
로 판정했다 — 바로 위 2026-08-17 addendum이 정정한 "41개·23종·19종"
수치가 세션 중 `Domain::GrantAck` 추가로 다시 stale 해졌다는 지적이다.

### 현재(2026-08-18) 실측치

- `Domain` enum: **24종** (`crates/protocol/src/canonical.rs:282-315`)
- `domain_coverage_is_explicit`: **24종 중 20개 구현**
  (`crates/protocol/tests/t1_signing_targets.rs:397-448`)
- `ToCanonicalFields` impl: **42개 선언** (`crates/protocol/src/to_fields.rs`
  직접 grep 결과)

frontmatter limitations 1번(`DoD-03:72` 의 "17종 중 13종")과 2026-08-17
addendum(`DoD-03:279-282` 의 "41개·23종·19종")은 원문 그대로 두고
고치지 않는다(append-only 원칙, `P0-07` 선례) — **진짜 현재 값은
위 실측치**다. 이 문서를 읽는 사람은 항상 가장 최근 addendum 의
수치를 신뢰해야 한다.

### coverage 테스트의 자동성 한계 — 새로 명시

`domain_coverage_is_explicit` 은 `Domain` enum 을 순회하지 않고 손으로
쓴 `coverage` 배열을 쓴다(`t1_signing_targets.rs:397-428`). 새
enum variant 가 추가돼도 이 배열에 반영하는 것을 잊으면 테스트가
조용히 stale 해질 수 있다 — 실제로 `DoD-02` 재검수 때 이 결함으로
숫자가 어긋나 있었던 적이 있다(그때 고쳤다; 지금은 24/20 으로
맞다). 이 자동성 한계 자체는 이전까지 이 문서에 명시된 적이
없었으므로 여기 새로 기록한다. `canonical_vectors.rs` 의 동급
테스트는 실제 enum 값을 순회해 이 문제가 없다 — 구조적으로 다르다.

### Rust 50/50 재실행 — 이 세션에서 직접 확인, Codex 샌드박스에서는 못함

Codex read-only 샌드박스는 `.cargo-build-lock` 생성 권한이 없어
`cargo test` 를 실행하지 못했다(샌드박스 제약이지 코드 결함이
아니다). 이 세션은 이미 로컬에서 직접 실행해 확인했다 —
`docs/evidence/_raw/DoD-03_v2_promotion_2026-08-18.txt:1-5` 가 그
receipt 다: `canonical_vectors`/`field_number_audit`/`prost_canonical`
합계 50 passed / 0 failed, Python self-test 8/8, 벡터 40개 교차
일치.

### 그 외 재확인 결과 (요약, 전부 실재 확인됨)

- claim(JobManifest 28/29·Lease 14/15 필드 완전성, 두 구현 바이트
  일치): 실재 확인됨.
- negative_tests 아홉 항목: 전부 실재 확인됨(`prost_canonical.rs`,
  `field_number_audit.rs`, `reference_canonical.py` 내 정확한
  줄 인용까지 확인).
- limitations 나머지 3건(중첩 메시지 미검증 / Ed25519 미실행 /
  `SCHEMA_TOO_NEW` 미실행)의 2026-08-17 정밀화: 여전히 유효.
- vectors 40건: 2026-08-17 addendum 과 일치, 재확인됨.

### review_outcome

`CHANGES_REQUESTED` — 위 수치 재정정(이 addendum)으로 반영했다.
좁은 범위의 후속 확인을 별도로 요청해 `ACCEPTED` 를 받은 뒤에만
schema v2 로 승격한다.
