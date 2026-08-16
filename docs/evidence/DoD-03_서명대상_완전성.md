---
id: DoD-03
claim: "JobManifest 와 Lease 의 서명 필드를 제외한 전 필드가 canonical 서명 대상에 포함되며, 각 필드가 실제로 서명 결과에 영향을 준다. Rust 와 Python 참조 구현이 전 필드 메시지에서 바이트 단위로 일치한다"
status: PASS
commit: 13795c604c74c5c9bb5bd0104a5338407d03f3d7
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
