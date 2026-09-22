---
schema_version: 2
id: DoD-01
claim: "Rust 구현 canonical_encode 가 Python 참조 구현과 바이트 단위로 일치하며, signing.md §3 의 규칙 a~i 를 모두 만족한다"
status: PASS
commit: 74d7c278f2ee287cbb977aeea8fe339a6bb5a42e

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-18T16:40:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "claim 범위 · negative_tests 실재성과 domain 수치 · limitations 4건 stale 여부 · decision 절 정합성 · cargo test 재실행 확인"
review_artifact: "docs/evidence/_raw/DoD-01_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-01_v2_promotion_2026-08-18.txt"
raw_output_digest: "sha256:294248cc7f10757ff205c3993b8ae07105ce55d318e61380074a46b2cecb8959"
raw_output_bytes: 1694

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
  - docs/evidence/_raw/DoD-01_v2_promotion_2026-08-18.txt
  - docs/evidence/_raw/DoD-01_review.txt
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

---

## 후속 정정 (2026-08-16 · `DoD-08`)

★ **이 문서의 `raw_output` 에 있는 `vector cross-checks: OK` 는
당시 실제보다 약한 검사였다.**

독립 검수가 지적했다 — 그때의 `--verify` 는 저장된 `canonical_hex` 끼리
`MUST_EQUAL`/`MUST_DIFFER` 관계만 확인했고, **저장본이 구현과 어긋나도 통과**했다.

```text
당시 --verify 가 실제로 한 것    벡터 사이의 관계 검사
당시 --verify 가 하지 않은 것    build_vectors() 재실행 후 바이트 대조
```

### 그럼에도 이 evidence 의 주장은 유효하다

교차검증은 `--verify` 가 아니라 **Rust 테스트**가 했다.
`crates/protocol/tests/canonical_vectors.rs` 가 Python 이 생성해 저장한
`canonical_hex` 를 읽어 Rust 출력과 직접 대조한다.
그 경로는 처음부터 지금까지 유효하다.

`--verify` 는 2026-08-16 에 재생성 대조를 하도록 고쳤다 (`DoD-08`).

### 이 문서가 놓쳤던 것

같은 검수에서 **Rust 와 Python 이 동일하게 규범을 어기던 3건**이 발견됐다
(규칙 i 의 중첩 누출 · map 엔트리 기본값 · 도출 해시 재귀 제외).
**벡터 대조는 "두 구현이 같은가" 를 증명하지 "옳은가" 를 증명하지 않는다** —
이 문서가 그 한계를 충분히 적지 않았다. `DoD-08` 참조.

관련: `docs/protocol/signing.md` · `docs/decisions/ADR-026_체크포인트_확정_절차_플랫폼_차이.md`

---

## ★ 이후 변경 (2026-08-18 01:00) — claim 범위 초과, domain 수치·limitation 4건 stale

독립 검수(`agent:codex-cli`, read-only)가 재검수해 `CHANGES_REQUESTED`
로 판정했다. 이 저장소에서 가장 오래된 evidence 라 stale 이 가장
많이 쌓여 있었다.

### claim 을 이렇게 좁혀 읽는다

원래 claim은 "두 구현이 모든 범위에서 바이트 단위로 일치하고
a~i 를 모두 만족한다"로 읽힌다. 실제로는:

- canonical 인코더 자체는 지금도 규칙 a~i 를 구현한다(`crates/protocol/src/canonical.rs:6,185,204`).
- Python 참조 구현엔 지금 `JobManifest` 전 필드 표가 있다(`tools/canonical/reference_canonical.py:175`).
- 벡터는 지금 **40건**이다 — `tests/vectors/canonical_v1.json:7`
  부터 시작하는 `vectors` 배열을 직접 파싱해 확인했다(`:2` 는
  `spec` 선언일 뿐 개수를 보여주지 않는다 — 재검수가 지적했다).
  이 evidence 가 쓰인 시점의 **12건**은 초기 스냅샷이었다. `HISTORY.md`
  의 "2026-08-16 10:40 — 서명 밖 필드 6건 제거" 항목(제목 기반 인용
  — 재검수가 원래 인용한 "09:30 — prost 연동 계층" 항목이 아니라
  이 항목에 12→20 증가 기록이 있음을 잡았다)에서 그 증가가
  확인된다.
- 그러나 `crates/protocol/tests/canonical_vectors.rs` 는 40개 벡터
  **전체를 순회**하지 않고 수동 구성한 `JobManifest` 부분집합을
  쓴다(`:35,40`). prost 참조 대조도 최소 메시지 중심이다(`prost_canonical.rs:55`).

> claim 은 "canonical 인코더가 규칙 a~i 를 구현하고, 대표 벡터들에서
> Python 참조 구현과 바이트 단위로 일치한다 — 그러나 40개 벡터
> 전체와 `Signable` 10종 전체를 순회하는 전수 대조는 아니다"로
> 좁혀 읽는다.

### negative_tests 이름은 실재 확인됨, 단 설명 문구 하나가 stale

`negative_non_minimal_varint_is_rejected`(`canonical_vectors.rs:259`),
`negative_truncated_varint_is_rejected`(`:271`),
`negative_cross_domain_signature_input_differs`(`:289`),
`v08_signature_field_is_excluded`(`:173`),
`v04_repeated_order_is_preserved`(`:125`),
`domain_tags_are_32_bytes_and_unique`(`:304`) 전부 실재한다. 다만
`domain_tags_are_32_bytes_and_unique` 의 원래 서술("17종 domain_tag")
은 stale — 지금 이 테스트는 **23종**을 검사하고 `assert_eq!(seen.len(), 23, ...)`
로 고정한다(`:321`). `Domain` enum 도 지금 23종이다(`canonical.rs:282`).

### stale limitations

| 원래 서술 | 지금 |
|---|---|
| "JobManifest 16개 필드 부분집합"(`:59`) | ★ 거짓이다. 지금 전 필드가 구현되어 있고 `UNIMPLEMENTED_FIELDS` 는 비어 있다(`to_fields.rs:830,944`) |
| "prost 연동 미구현"(`:60`) | ★ 거짓이다. `ToCanonicalFields` 와 prost 경로 테스트가 지금 존재한다(`to_fields.rs:27`, `prost_canonical.rs:55`) |
| "Ed25519 서명 자체 미검증"(`:61`) | ★ 거짓이다. Crypto 스트림에 지금 구현되어 있다 — `HISTORY.md` 의 "2026-08-16 12:40 — Ed25519 서명·검증 + Verified<M>" 항목(제목 기반 인용) |
| "SCHEMA_TOO_NEW 미구현"(`:62`) | ★ 거짓이다. 지금 반환 경로가 있다(`crates/protocol/src/signing.rs:741-743`) |

float/prost 우회 가능성(`:63`, 직접적인 float 우회 negative test 는
확인 안 됨) 과 Windows 단일 플랫폼(`:64`) limitation 은 지금도
유효하다.

### DoD-08 이 이후에 canonical 결함 3건을 더 찾았다

이 문서 자체가 위쪽 "이후 변경" 절에서 이미 "벡터 대조는 '같은가'
를 증명하지 '옳은가' 를 증명하지 않는다"고 적어 뒀다 — `DoD-08`
이 실제로 Rust/Python 이 **똑같이** 규범을 어기던 3건(규칙 i 중첩
누출·map 엔트리 기본값·도출 해시 재귀 제외)을 찾아 시정했다. 이
사실은 이미 이 문서에 반영되어 있다.

### review_outcome

`CHANGES_REQUESTED` → 위 정정으로 claim 범위·domain 수치·stale
limitations 를 반영했다. 원본 YAML 은 당시 기록이므로 고치지
않는다.

★ 2026-08-18 01:10 두 번째 재검수 — `agent:codex-cli` 가 claim 범위
축소·domain 17→23·stale limitation 4건 정정은 원문과 대조해 확인했지만,
벡터 40건 인용이 `:2`(spec 선언)를 가리켜 실제로 개수를 입증하지
못했고, `HISTORY.md` 제목 인용도 틀렸다("09:30 — prost 연동 계층"
에는 12→20 기록이 없다 — 실제로는 "10:40 — 서명 밖 필드 6건 제거")고
지적했다. 둘 다 위에서 고쳤다.

★ 2026-08-18 01:20 세 번째(최종) 재검수 — `agent:codex-cli` 가
**`ACCEPTED`** 로 판정했다. `canonical_v1.json:7` 이 실제
`vectors` 배열 시작임을 확인했고 40건도 재확인했다. `HISTORY.md`
의 "2026-08-16 10:40 — 서명 밖 필드 6건 제거" 제목에 "벡터 12 ->
20건" 기록이 실제로 있음도 확인했다. "잔여 사항 없음."

## ★ 이후 변경 (2026-08-18 16:40) — domain 수치 재차 stale, schema v2 승격을 위한 재검수

이 evidence 를 schema v1 -> v2(RULE.md §7.3, ADR-030)로 승격하기
위해 새 독립 검수를 받았다(`agent:codex-cli`, fresh-read-only,
`p64` 프롬프트) — `CHANGES_REQUESTED`.

### domain 수치가 또 stale — 17 -> 23 -> 24

바로 위 addendum(2026-08-18 01:00)이 "17종" 원본 서술을 "23종"으로
정정했는데, **그 정정 자체가 이 재검수 시점에는 이미 stale 이었다.**
같은 세션(2026-08-18) 안에서 coordinator/agent 핸드셰이크 작업의
일부로 `Domain::GrantAck` 를 새로 추가했기 때문이다
(`crates/protocol/src/canonical.rs`). 지금
`domain_tags_are_32_bytes_and_unique` 는 **24종**을 검사하고
`assert_eq!(seen.len(), 24, ...)` 로 고정한다
(`crates/protocol/tests/canonical_vectors.rs:305,315,323`).

이 stale 은 검수 지적이 틀려서가 아니라, **원본 평가와 재검수
사이에 이 evidence 가 다루는 코드 자체가 실제로 바뀌었기 때문**
이다 — canonical evidence 의 근본적 한계(살아 있는 코드베이스를
한 시점의 스냅샷으로 기록한다)를 그대로 보여준다.

### 나머지 항목은 전부 재확인됨

- claim(대표 벡터 대조이며 40개 전수 대조는 아니라는 좁힌 해석)은
  지금도 정확함 — `canonical.rs:6,185,204`,
  `reference_canonical.py:504,511,564`,
  `canonical_vectors.rs:26,40` 대조 확인.
- negative_tests 6개 이름 전부 실재(`canonical_vectors.rs:259,271,289,173,125,304`).
- stale limitations 4건(JobManifest 부분집합·prost 미구현·Ed25519
  미구현·SCHEMA_TOO_NEW 미구현) 정정은 지금도 유효함을
  `to_fields.rs:849,899,963`, `prost_canonical.rs:56`,
  `crypto/src/lib.rs:62,120`, `signing.rs:741,743` 로 재확인.
- `cargo test -p gputeer-protocol --test canonical_vectors` 는
  검수자의 read-only 샌드박스가 `.cargo-build-lock` 접근 거부로
  직접 실행하지 못했다 — 이 세션이 이 문서의 v2 승격 직전에
  **직접 실행해 15/15 통과를 확인**했다(아래 v2 frontmatter 의
  `raw_output_artifact` 참조).

### 정정 — frontmatter 원문은 손대지 않는다

★ 처음에는 frontmatter `negative_tests` 의
`domain_tags_are_32_bytes_and_unique` 설명 문구("17종")를 직접
"24종"으로 고쳤는데, 이 저장소의 append-only 원칙("관측 기록은
고치지 않는다")과 이 문서 자신의 앞선 addendum 들이 이미 세운
관례(원본 frontmatter 프로즈는 그대로 두고 addendum 본문에서만
정정한다 — `P0-07` 이 그 관례의 유일한 예외였고, 그 이유(status 는
관측이 아니라 분류 필드)를 명시했다)를 어긴 것임을 깨닫고 원복했다.
`negative_tests`/`limitations`/`decision` 의 원문 문자열은 v1 그대로
"17종"으로 **남긴다** — 정정은 여기, addendum 본문에만 있다. 실제
숫자는 24종이다(위 절 참조). schema v2 승격 시 frontmatter 에 새로
추가한 필드(`schema_version`·`executor_*`·`reviewer_*`·`review_*`·
`raw_output_artifact`/`digest`/`bytes`)와 `artifacts:` 리스트에 새
raw 파일 2개를 추가한 것은 예외다 — v2 스키마 자체가 요구하는
**새 필드 추가**이지 기존 프로즈의 **수정**이 아니기 때문이다.

### review_outcome

★ 첫 라운드 `CHANGES_REQUESTED` → 이 addendum 에 domain 24종 정정과
직접 실행한 `cargo test` 결과(`DoD-01_v2_promotion_2026-08-18.txt`)
첨부로 대응 → 좁은 후속 확인 재검수(`agent:codex-cli`,
fresh-read-only, `p65` 프롬프트)에서 **`ACCEPTED`**. 두 지적(domain
수치, cargo test 미확인) 모두 해소됐음을 확인했다. 전문은
`docs/evidence/_raw/DoD-01_review.txt` 참조.

### schema v1 → v2 승격

이 검수를 근거로 이 문서를 `schema_version: 1`(유예 목록,
`docs/evidence/_schema_v1_grandfathered.txt`)에서 `schema_version: 2`
(RULE.md §7.3, ADR-030)로 승격했다. frontmatter 에 `executor_*`·
`reviewer_*`·`review_*`·`raw_output_artifact`/`digest`/`bytes` 필드를
새로 추가했고(기존 필드는 위 "정정" 절에서 설명한 대로 손대지
않았다), `docs/evidence/_schema_v1_grandfathered.txt` 에서 이 파일명을
지우고 `scripts/verify_evidence.py` 의 `GRANDFATHER_DIGEST` 상수를
갱신했다(그 diff 자체가 검토 대상이라는 것이 RULE.md §7.3 의 설계
의도다).
