---
id: DoD-01
claim: "Rust 구현 canonical_encode 가 Python 참조 구현과 바이트 단위로 일치하며, signing.md §3 의 규칙 a~i 를 모두 만족한다"
status: PASS
commit: f2ec00e25313bf56ea4782616af19e118999f5a0
binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1"
  note: "라이브러리 크레이트라 실행 바이너리 없음. 테스트 바이너리는 cargo 가 생성"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 (순수 인코딩 로직)"
network_profile: "해당 없음 - 로컬 단위 테스트"
command: |
  cargo test --workspace
  cargo test -p gputeer-protocol
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
raw_output: |
  Running tests\canonical_vectors.rs
  running 15 tests
  test minimal_varint_roundtrip ... ok
  test domain_tags_are_32_bytes_and_unique ... ok
  test negative_cross_domain_signature_input_differs ... ok
  test negative_non_minimal_varint_is_rejected ... ok
  test negative_truncated_varint_is_rejected ... ok
  test determinism_100_iterations ... ok
  test v08_signature_field_is_excluded ... ok
  test v06_explicit_defaults_are_omitted ... ok
  test v11_schema_version_separation ... ok
  test v03_map_insertion_order_is_irrelevant ... ok
  test v01_sig_input_and_digest_match_reference ... ok
  test v01_minimal_manifest_matches_reference ... ok
  test v04_repeated_order_is_preserved ... ok
  test v10_domain_separation ... ok
  test v13_merkle_promotion_matches_reference ... ok
  test result: ok. 15 passed; 0 failed; 0 ignored

  Running tests\durability_chaos.rs
  running 16 tests
  test result: ok. 16 passed; 0 failed; 0 ignored; finished in 4.86s

  전체: 31 passed / 0 failed

  python tools/canonical/reference_canonical.py --verify ...
  vector cross-checks: OK
artifacts:
  - docs/evidence/_raw/M1-02_test_output.txt
  - crates/protocol/tests/canonical_vectors.rs
  - tests/vectors/canonical_v1.json
negative_tests:
  - "negative_non_minimal_varint_is_rejected: 0 을 2바이트(0x80 0x00)로 인코딩한 non-minimal varint 를 거부. 같은 값의 복수 표현은 서명 우회 여지를 만든다"
  - "negative_truncated_varint_is_rejected: 연속 비트가 켜진 채 끝난 varint 를 거부"
  - "negative_cross_domain_signature_input_differs: 같은 canonical 에 8개 domain_tag 를 적용해 sig_input 이 전부 다름을 확인. 이것이 없으면 Lease 서명을 Manifest 서명으로 재사용 가능"
  - "v08_signature_field_is_excluded: 서명 필드(90)에 0xFF 64바이트를 채워도 canonical 이 변하지 않음을 확인 (자기참조 순환 방지)"
  - "v04_repeated_order_is_preserved: repeated 순서가 다르면 canonical 이 달라야 함을 확인 (정렬하면 안 되는 필드)"
  - "domain_tags_are_32_bytes_and_unique: 17종 domain_tag 가 전부 32바이트이고 중복 없음"
limitations:
  - "JobManifest 의 부분집합(16개 필드)만 검증했다. proto/job.proto 전체 필드를 Rust 로 옮기지 않았다"
  - "prost 연동을 하지 않았다. 현재는 손으로 만든 Fields/Value 자료구조를 쓴다. 실제 proto 메시지에서 Fields 로 변환하는 계층은 미구현이다"
  - "Ed25519 서명 자체를 검증하지 않았다. sig_input 바이트 생성까지만 확인했고 서명·검증은 crypto 스트림 범위다"
  - "SCHEMA_TOO_NEW 반환 경로를 구현하지 않았다. signing.md §7.2 의 버전 협상은 미구현이다"
  - "float/double 금지는 타입 수준에서 강제된다(Value enum 에 해당 variant 없음). 그러나 prost 연동 시 우회 가능성은 미검증이다"
  - "Windows 단일 플랫폼에서만 실행했다. Linux/macOS 에서의 동일성은 미검증이다"
decision: "signing.md §3 의 canonical 규칙이 두 독립 구현에서 동일한 바이트를 낸다는 것이 실증되었다. 규범 문서를 변경하지 않는다. 다음: prost 연동(Fields 변환 계층)과 Ed25519 서명 검증"
---

# DoD-01 · canonical_encode 교차 검증

## 무엇을 입증하려 했는가

v5 검토에서 **가장 심각한 결함**으로 지목된 항목이다.

> `manifest_hash` 를 "필드 전체의 해시" 라고만 적었다. proto3 는 deterministic
> serialization 을 보장하지 않고, map 에는 순서가 없다. **같은 메시지가 다른 해시를
> 갖고 서명 검증이 랜덤하게 실패한다.**

`signing.md` 로 규칙을 정했으나, **규칙이 실제로 결정론적인지**는 구현 두 개가
같은 바이트를 낼 때에만 증명된다. 이 검증이 그것이다.

## 어떻게 측정했는가

**독립적으로 작성된 두 구현을 대조**했다.

```text
Python  tools/canonical/reference_canonical.py   손으로 만든 wire format 인코더
Rust    crates/protocol/src/canonical.rs         BTreeMap 기반 별도 구현
```

두 구현은 자료구조가 다르다. Python 은 필드 표를 dict 로 두고 정렬하며,
Rust 는 `BTreeMap<u32, Value>` 를 써서 순회 자체가 정렬이다.
**같은 규칙을 다른 방식으로 구현했는데 바이트가 같다면** 규칙이 결정론적이라는 뜻이다.

벡터는 Python 이 생성해 `tests/vectors/canonical_v1.json` 에 고정했고,
Rust 테스트가 그 파일을 읽어 대조한다. **벡터를 손으로 쓰지 않았다.**

## 결과

```text
cargo test --workspace
  canonical_vectors    15 passed / 0 failed
  durability_chaos     16 passed / 0 failed
  전체                 31 passed / 0 failed
```

### 규칙별 검증

| signing.md 규칙 | 테스트 | 결과 |
|---|---|---|
| a. field number 오름차순 | `v01_minimal_manifest_matches_reference` | 일치 |
| b. 기본값 생략 | `v06_explicit_defaults_are_omitted` | 명시적 0 == 미설정 |
| c. map key 정렬 | `v03_map_insertion_order_is_irrelevant` | 삽입 순서 무관 |
| d. repeated 순서 유지 | `v04_repeated_order_is_preserved` | 순서 다르면 canonical 다름 |
| e. 최단 varint | `negative_non_minimal_varint_is_rejected` | non-minimal 거부 |
| f. 재귀 적용 | 중첩 메시지 포함 벡터 | 일치 |
| g. float 금지 | `Value` enum 에 variant 없음 | **타입 수준 강제** |
| i. 서명 필드 제외 | `v08_signature_field_is_excluded` | 0xFF 채워도 불변 |
| §4 sig_input | `v01_sig_input_and_digest_match_reference` | BLAKE3 다이제스트까지 일치 |
| §5 domain 분리 | `negative_cross_domain_signature_input_differs` | 17종 전부 상이 |
| §6.3 Merkle 승격 | `v13_merkle_promotion_matches_reference` | 1·2·3청크 루트 일치 |

### 부가 검증 — checkpoint (ADR-026)

같은 실행에서 체크포인트 durability 16건도 통과했다.
특히 `adr026_write_once_succeeds_while_readers_hold_files_open` 이 중요하다.

```text
P0-03a 실측 (Python, rename-over-existing)   313 / 3000 성공
ADR-026 적용 (Rust, write-once 고유 이름)    500 / 500 성공
```

**같은 최악 조건(독자 3스레드가 파일을 계속 열어둠)에서 ADR-026 의 회피 전략이
실제로 동작함이 실증되었다.**

## 이 실험이 증명하지 "않는" 것

- **`proto/job.proto` 전체 필드를 옮기지 않았다.** 16개 필드 부분집합만 검증했다.
- **prost 연동을 하지 않았다.** 지금은 손으로 만든 `Fields`/`Value` 를 쓴다.
  실제 protobuf 메시지에서 `Fields` 로 변환하는 계층이 없다.
  **이 계층에서 규칙이 깨질 수 있으며 그것은 미검증이다.**
- **Ed25519 서명을 검증하지 않았다.** `sig_input` 바이트 생성까지만 확인했다.
- **`SCHEMA_TOO_NEW` 경로가 미구현이다.** signing.md §7.2 버전 협상은 없다.
- **Windows 한 플랫폼**에서만 실행했다. Linux/macOS 동일성은 미검증이다.
- float 금지는 `Value` enum 에 variant 를 두지 않아 강제되지만,
  **prost 연동 시 우회 가능성은 확인하지 않았다.**

## 결정

1. **`signing.md` §3 규칙을 변경하지 않는다.** 두 독립 구현이 같은 바이트를 낸다.
2. 다음 작업: **prost 연동 계층**(proto 메시지 → `Fields`)과 **Ed25519 서명 검증**.
   전자가 이 검증의 가장 큰 공백이다.
3. `tests/vectors/canonical_v1.json` 을 **QA 스트림 소유**로 유지한다.
   구현자가 자기 구현에 맞춰 고치면 검증이 무의미해진다.

관련: `docs/protocol/signing.md` · `docs/decisions/ADR-026_체크포인트_확정_절차_플랫폼_차이.md`
