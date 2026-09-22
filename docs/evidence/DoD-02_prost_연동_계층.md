---
schema_version: 2
id: DoD-02
claim: "실제 prost 생성 메시지에서 canonical 규칙 a~i 가 유지되며, to_fields 변환 계층이 Python 참조 구현과 바이트 단위로 일치한다. 서명 대상에서 빠진 필드는 전부 명시적으로 선언되어 있다"
status: PASS
commit: 2c066e2421b6d40775eefc23d2249395171e6d2f

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-18T18:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "claim 범위 · negative_tests 실재성과 domain 수치(24/20) · limitations stale 여부 · ControlAction 9/21 재확인 · cargo test 재실행 확인"
review_artifact: "docs/evidence/_raw/DoD-02_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-02_v2_promotion_2026-08-18.txt"
raw_output_digest: "sha256:de28ab70469d3f6e17384f5c8d58b3bdca5ddab3e427e038d35b005d25642d84"
raw_output_bytes: 1646

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 / protoc (protoc-bin-vendored)"
  note: "라이브러리 크레이트라 실행 바이너리 없음. prost-build 가 OUT_DIR 에 gputeer.v1.rs 생성"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1"
  proto_files: "proto/{common,job,lease,artifact,control}.proto"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 (순수 인코딩 로직)"
network_profile: "해당 없음 - 로컬 단위 테스트"
command: |
  cargo build -p gputeer-protocol
  cargo test --workspace -- --nocapture
  cargo test -p gputeer-protocol --test prost_canonical
  cargo test -p gputeer-protocol --test field_number_audit
raw_output: |
  Running tests\canonical_vectors.rs
  test result: ok. 15 passed; 0 failed; 0 ignored

  Running tests\field_number_audit.rs
  Digest: proto 2개 필드 중 2개 서명 대상
  CudaRequirement: proto 3개 필드 중 3개 서명 대상
  GpuRequest: proto 5개 필드 중 5개 서명 대상
  ResourceRequest: proto 5개 필드 중 5개 서명 대상
  WorkloadHint: proto 7개 필드 중 7개 서명 대상
  Lease: proto 15개 필드 중 13개 서명 대상
  JobManifest: proto 29개 필드 중 23개 서명 대상
  test result: ok. 6 passed; 0 failed; 0 ignored

  Running tests\prost_canonical.rs
  map 없는 매니페스트: prost 113B, canonical 113B, 동일=true
  500회 재구축 — prost 서로 다른 인코딩 495종 / canonical 1종
  미구현 서명 필드: JobManifest field 10 — env (ExecutionEnvironment)
  미구현 서명 필드: JobManifest field 11 — input_artifacts (repeated Digest)
  미구현 서명 필드: JobManifest field 12 — dataset (DatasetRef)
  미구현 서명 필드: JobManifest field 54 — network (NetworkPolicy)
  미구현 서명 필드: JobManifest field 55 — artifact_scope (ArtifactScope)
  미구현 서명 필드: Lease field 40 — scope (ResourceScope)
  test result: ok. 11 passed; 0 failed; 0 ignored

  Running tests\durability_chaos.rs
  test result: ok. 16 passed; 0 failed; 0 ignored
  Running tests\kill_chaos.rs
  test result: ok. 7 passed; 0 failed; 0 ignored

  전체: 55 passed / 0 failed
artifacts:
  - docs/evidence/_raw/DoD-02_test_output.txt
  - crates/protocol/build.rs
  - crates/protocol/src/to_fields.rs
  - crates/protocol/tests/prost_canonical.rs
  - crates/protocol/tests/field_number_audit.rs
  - docs/evidence/_raw/DoD-02_v2_promotion_2026-08-18.txt
  - docs/evidence/_raw/DoD-02_review.txt
negative_tests:
  - "prost_encode_is_not_deterministic_for_maps: map 을 가진 같은 내용의 메시지를 500회 재구축했을 때 prost::encode 가 495종의 서로 다른 바이트를 냈다. canonical 은 1종. ★ 비공허성 단언 포함 — prost 가 1종만 냈다면 실패시킨다"
  - "hashmap_canonical_is_stable_across_many_rebuilds: 삽입 순서를 매번 회전시켜 200회 재구축, canonical 1종"
  - "signature_field_excluded_in_prost_path: 실제 prost 메시지의 submitter_signature 에 0xFF 64바이트를 채워도 canonical 불변"
  - "default_valued_fields_are_omitted_in_prost_path: 명시적 0/false/빈 vec/빈 map 을 넣어도 미설정과 같은 canonical"
  - "job_manifest_field_numbers_match_proto / lease_ / common_: to_fields.rs 소스와 .proto 소스를 파싱해 field number-이름 대조. 컴파일러가 잡지 못하는 번호 오타를 잡는다"
  - "every_unsigned_field_is_declared_in_unimplemented_list: 서명 대상에서 빠진 모든 필드가 UNIMPLEMENTED_FIELDS 에 선언되어 있는지 검사. 조용히 빠진 필드는 위조 가능하다"
  - "unimplemented_list_has_no_stale_entries: 반대 방향 - 목록에 있는데 이미 구현된 항목 검출"
  - "parsers_are_not_vacuous: 감사 테스트의 파서 자체가 빈 결과를 내면 위 검사가 전부 공허해지므로 파서를 먼저 검증"
limitations:
  - "★ 서명 대상 6개 필드가 아직 canonical 에 들어가지 않는다 (JobManifest 10/11/12/54/55, Lease 40). UNIMPLEMENTED_FIELDS 로 선언하고 테스트로 강제했으나 구현 자체는 미완이다. 이 필드들은 현재 위조 가능하다"
  - "17종 서명 대상 메시지 중 7종(JobManifest, Lease, Digest, CudaRequirement, GpuRequest, ResourceRequest, WorkloadHint)만 ToCanonicalFields 를 구현했다. artifact.proto / control.proto 의 서명 대상은 미구현이다"
  - "field_number_audit 는 정규식 파서다. oneof / reserved / 중첩 message 선언을 다루지 않는다. 현 스키마에 해당 구문이 없어 지금은 무해하나 스키마가 커지면 prost-reflect 로 교체해야 한다"
  - "Ed25519 서명·검증을 여전히 하지 않았다. sig_input 바이트 생성까지만 확인했다"
  - "SCHEMA_TOO_NEW 경로(signing.md §7.2)는 미구현이다. prost 는 알 수 없는 필드를 조용히 버리는데, 그 동작이 검증을 어떻게 깨는지 미검증이다"
  - "Windows 단일 플랫폼. Linux/macOS 에서 HashMap 순회 순서가 다를 수 있으나 canonical 은 순서 무관이므로 영향 없을 것으로 보인다 — 그러나 미검증이다"
  - "prost 생성 코드가 float 필드를 만들 수 있는 경로는 검사하지 않았다. 현재 proto 에 float/double 이 없어서 발생하지 않을 뿐, 누군가 추가하면 to_fields 컴파일 오류로 잡히는지는 미검증이다"
decision: "signing.md §3 규칙을 변경하지 않는다. prost 경로에서도 규칙이 유지됨이 실증되었다. §13.1 의 'prost 인코더를 서명에 쓰지 말라'는 경고는 근거가 확정되었다 - 단 근거는 '출력이 항상 다르다'가 아니라 'map 이 있으면 prost 가 결정론적이지 않다'이다. 다음: UNIMPLEMENTED_FIELDS 6건 구현 + artifact/control 서명 대상 + Ed25519"
---

# DoD-02 · prost 연동 계층

## 무엇을 입증하려 했는가

`DoD-01` 이 **자기 자신의 최대 공백**으로 지목한 항목이다.

> prost 연동을 하지 않았다. 지금은 손으로 만든 `Fields`/`Value` 를 쓴다.
> 실제 protobuf 메시지에서 `Fields` 로 변환하는 계층이 없다.
> **이 계층에서 규칙이 깨질 수 있으며 그것은 미검증이다.**

`DoD-01` 은 "규칙이 결정론적이다" 를 증명했다. 이 검증은 **"실제 메시지가 그 규칙을
실제로 통과한다"** 를 증명한다. 둘은 다른 주장이며, 후자가 없으면 전자는 종이 위의 사실이다.

## 발견 1 — proto 가 한 번도 컴파일된 적이 없었다

`prost-build` 를 붙이자마자 컴파일이 실패했다.

```text
"Lease" is not defined
```

`proto/job.proto` 에 `import "lease.proto";` 가 없었다.
**5개 proto 를 작성한 이래 한 번도 컴파일한 적이 없어서** 교차 파일 참조 오류가
드러나지 않았다. 스키마는 규범 문서인데 문법 검사조차 받지 않고 있었다.

→ 이제 `cargo build` 가 항상 `protoc` 를 돌린다. 회귀할 수 없다.

## 발견 2 — prost 출력과 canonical 이 같을 수 있다

처음 쓴 negative test 는 다음이었고, **실패했다.**

```rust
assert_ne!(prost_bytes, canon, "prost 인코딩과 canonical 이 같다면 ...");
```

```text
map 없는 매니페스트: prost 113B, canonical 113B, 동일=true
```

prost 역시 필드 번호 오름차순으로 쓰고, 기본값을 생략하고, 최소 varint 를 쓴다.
**map 이 없는 단순 메시지에서는 두 인코딩이 바이트 단위로 일치한다.**

이것은 내 구현의 결함이 아니라 **테스트의 주장이 틀린 것**이었다.
`signing.md` §13.1 의 근거를 정확히 다시 세워야 했다.

### 정확한 근거

```text
틀린 근거   "prost 출력은 canonical 과 다르다"
맞는 근거   "map 이 있으면 prost 는 실행마다 다른 바이트를 낸다"
```

같은 내용의 메시지를 500회 재구축해 측정했다.

| 인코더 | 서로 다른 바이트 |
|---|---|
| `prost::Message::encode` | **495 종** |
| `canonical_encode` | **1 종** |

prost 는 map 을 `HashMap` 으로 생성하고 순회 순서대로 쓴다.
`HashMap` 순회 순서는 인스턴스마다 다르므로, **같은 매니페스트에 서명해도
매번 다른 서명이 나오고 검증이 랜덤하게 실패한다.** 이것이 v5 검토가 지적한
바로 그 결함이며, `to_fields.rs::norm_map()` 의 `HashMap → BTreeMap` 변환이
그것을 막는 단 하나의 지점이다.

★ 이 테스트에는 **비공허성 단언**이 들어 있다. prost 가 500회 전부 같은 바이트를
냈다면(=이 러너에서 `HashMap` 이 안정적이라면) 테스트는 "canonical 이 안정적이다"만
보인 것이지 "prost 가 불안정하다"를 보인 게 아니다. 그 경우 조용히 통과시키지 않고
실패시켜 §13.1 의 근거를 다시 세우게 한다.

## 발견 3 — 컴파일러가 못 잡는 결함이 정확히 하나 있다

`to_fields.rs` 는 손으로 쓴다(`signing.md` §13.1: 어떤 필드가 서명에 들어가는지
사람이 눈으로 확인할 수 있어야 한다). 손으로 쓰면 실수가 3종류 나온다.

| 실수 | 컴파일러 | 비고 |
|---|---|---|
| 타입 불일치 (`put_bytes` 에 `String`) | **잡는다** | 실제로 이번에 잡혔다 |
| 없는 필드 (`self.nonexistent`) | **잡는다** | |
| **잘못된 번호** (`put_str(f, 61, &self.entrypoint)`) | ❌ **못 잡는다** | |

3번이 가장 위험하다. 코드는 돌고, 서명도 만들어지고, **자기 자신과는 검증도 통과한다.**
다른 구현체와 붙는 순간에만 깨지는데, 그때는 이미 서명된 매니페스트가 돌아다닌다.

그래서 `field_number_audit.rs` 가 `.proto` 소스와 `to_fields.rs` 소스를
**둘 다 파싱해 번호↔이름을 대조**한다.

### 감사 테스트의 실효성 확인

통과하는 테스트는 아무것도 증명하지 않는다. 뮤테이션 2종으로 확인했다.

```text
뮤테이션 A  issued_at_unix_ms 를 field 61 -> 62
            -> "field number 62 이 두 번 쓰였다" 로 검출 (중복 경로)

뮤테이션 B  entrypoint 를 field 13 -> 12
            -> "field 12 이 proto 에서는 `dataset` 인데
                to_fields 는 `self.entrypoint` 을 넣었다" 로 검출 (이름 불일치 경로)
```

두 경로 모두 동작한다. 뮤테이션은 검증 후 원복했다.

## 발견 4 — 서명에서 빠진 필드가 6개 있다

가장 중요한 결과다. **서명 대상에서 조용히 빠진 필드는 위조 가능한 필드다.**

| 메시지 | field | 이름 | 위조 시 영향 |
|---|---|---|---|
| JobManifest | 10 | `env` (ExecutionEnvironment) | 실행 환경(이미지·런타임) 교체 가능 |
| JobManifest | 11 | `input_artifacts` | 입력 산출물 바꿔치기 |
| JobManifest | 12 | `dataset` | 데이터셋 교체 |
| JobManifest | 54 | `network` (NetworkPolicy) | **네트워크 정책 우회** |
| JobManifest | 55 | `artifact_scope` | 산출물 접근 범위 확대 |
| Lease | 40 | `scope` (ResourceScope) | **자원 범위 확대** |

54·55·40 은 보안 필드다. 서명 밖에 있으면 중간자가 고쳐도 검증이 통과한다.

이것들을 `UNIMPLEMENTED_FIELDS` 로 선언하고, `every_unsigned_field_is_declared_in_unimplemented_list`
가 **선언되지 않은 누락을 실패시킨다.** 즉 지금 이 6건은 "알려진 미구현"이고,
앞으로 새로 생기는 누락은 **컴파일이 아니라 테스트가 막는다.**

★ 그러나 **선언했다고 안전해지는 것은 아니다.** 구현 전까지 이 필드들은 위조 가능하다.
v0.1 이전에 반드시 채워야 한다.

## 발견 5 — 계획서와 proto 의 드리프트

```text
gputeer_master_plan_FINAL.md §15.2   bytes  submitter_device_id = 60
proto/job.proto:88                   string submitter_device_id = 60
```

`docs/README.md` 의 우선순위에 따라 **규범 문서인 proto 가 이긴다.**
`common.proto` 의 "ID 는 ULID 26자 문자열" 규약과도 proto 쪽이 일치한다.
기준선은 읽기 전용이므로 수정 요청 목록(D-5)에 추가한다.

## 결과

```text
cargo test --workspace
  canonical_vectors     15 passed
  field_number_audit     6 passed   <- 신규
  prost_canonical       11 passed   <- 신규
  durability_chaos      16 passed
  kill_chaos             7 passed
  전체                  55 passed / 0 failed   (기존 38 -> 55)
```

### 규칙별 — prost 경로에서 재검증

| signing.md 규칙 | 테스트 | 결과 |
|---|---|---|
| a. field number 오름차순 | `prost_message_produces_same_canonical_as_reference` | 참조 구현과 일치 |
| b. 기본값 생략 | `default_valued_fields_are_omitted_in_prost_path` | 명시적 0 == 미설정 |
| **c. map key 정렬** | `hashmap_insertion_order_does_not_affect_canonical` | **prost 495종 → canonical 1종** |
| d. repeated 순서 유지 | `nested_messages_and_enums_round_through_prost` | 유지 |
| f. 재귀 적용 | 동상 (중첩 3단: ResourceRequest→GpuRequest→CudaRequirement) | 유지 |
| g. float 금지 | `Value` enum 에 variant 없음 | 타입 수준 강제 (proto 에 float 없음) |
| i. 서명 필드 제외 | `signature_field_excluded_in_prost_path` | 0xFF 채워도 불변 |
| §4 sig_input | `prost_message_produces_same_sig_input_and_digest` | BLAKE3 까지 일치 |

## 이 실험이 증명하지 "않는" 것

- **서명 대상 6개 필드가 여전히 빠져 있다.** 선언했을 뿐 구현하지 않았다.
- **17종 서명 대상 중 7종만** `ToCanonicalFields` 를 구현했다.
  `artifact.proto` · `control.proto` 의 서명 대상은 손도 대지 않았다.
- **Ed25519 를 여전히 하지 않았다.** `sig_input` 바이트까지다.
- **`SCHEMA_TOO_NEW` 미구현.** prost 는 모르는 필드를 조용히 버리는데,
  그것이 검증을 어떻게 깨는지 검사하지 않았다. `CLAUDE.md` §0.2 가 금지하는
  "모르는 필드를 조용히 통과" 가 **prost 기본 동작**이라는 점은 별도 스파이크가 필요하다.
- **Windows 단일 플랫폼.**
- `field_number_audit` 의 파서는 `oneof` · `reserved` 를 다루지 못한다.

## 결정

1. **`signing.md` §3 을 변경하지 않는다.** prost 경로에서도 규칙이 유지된다.
2. **§13.1 의 근거 문구를 정정한다** — "출력이 다르다" 가 아니라
   "map 이 있으면 결정론적이지 않다". 규범 자체는 그대로다.
3. `UNIMPLEMENTED_FIELDS` 6건을 **v0.1 게이트 항목**으로 올린다.
   보안 필드(54·55·Lease 40)가 서명 밖에 있는 상태로 v0.1 에 가지 않는다.
4. **`SCHEMA_TOO_NEW` × prost unknown-field 를 신규 스파이크로 등록한다** (P0-08).
   `CLAUDE.md` §0.2 와 prost 기본 동작이 정면으로 충돌한다.

관련: `docs/evidence/DoD-01_canonical_encode_교차검증.md` · `docs/protocol/signing.md` §13.1

---

## ★ 이후 변경 (2026-08-17 23:55) — claim 이 넓게 읽혔다, limitations 다수 stale

독립 검수(`agent:codex-cli`, read-only)가 재검수해 `CHANGES_REQUESTED`
로 판정했다. 핵심 구현 자체는 지금도 유효하지만, claim 문장과
limitations 여러 곳이 현재 코드를 정확히 반영하지 않았다.

### claim 을 이렇게 좁혀 읽는다

원래 claim("실제 prost 생성 메시지에서... to_fields 변환 계층이
Python 참조 구현과 **바이트 단위로 일치한다**")은 마치 전체 메시지에
대한 전수 참조 비교가 있는 것처럼 읽힌다. 실제로는:

- `JobManifest`/`Lease` 의 당시 누락 필드(10·11·12·54·55, Lease 40)는
  지금 구현되어 있고 `UNIMPLEMENTED_FIELDS` 는 빈 배열이다
  (`crates/protocol/src/to_fields.rs:830-869`,`893-915`,`934-944`).
- canonical bytes 를 Python 참조 구현과 **바이트 단위로 직접
  대조**하는 전 필드 테스트는 `JobManifest`(`prost_canonical.rs:55-85`
  최소 벡터, `:677-699` `v02_full_manifest_matches_reference` 전
  필드)와 `Lease`(`:700-735` `v02b_full_lease_matches_reference`
  전 필드) 둘 다 있다. ★ 2026-08-17 23:55 최초 정정 시 `Lease` 쪽
  전 필드 테스트 인용을 빠뜨렸었다 — 재검수가 지적해 추가했다.
  `field_number_audit` 는 나머지 메시지들의 필드 번호·이름 대조이지
  바이트 대조가 아니다(`field_number_audit.rs:161-185`).

> claim 은 "`JobManifest`(그리고 `Lease`) 는 참조 구현과 바이트
> 단위로 일치하고, 나머지 서명 대상 메시지는 필드 번호·이름 감사로
> 누락이 없음만 확인됐다" 로 좁혀 읽는다.

### negative_tests 이름 정정

`common_` 은 함수명이 아니다 — 실제로는
`common_message_field_numbers_match_proto`(`field_number_audit.rs:249`).
나머지 이름은 실재를 확인했다: `prost_encode_is_not_deterministic_for_maps`(`prost_canonical.rs:852`),
`hashmap_canonical_is_stable_across_many_rebuilds`(`:126`),
`signature_field_excluded_in_prost_path`(`:150`),
`default_valued_fields_are_omitted_in_prost_path`(`:220`),
`job_manifest_field_numbers_match_proto`(`field_number_audit.rs:189`),
`lease_field_numbers_match_proto`(`:194`),
`every_unsigned_field_is_declared_in_unimplemented_list`(`:324`),
`unimplemented_list_has_no_stale_entries`(`:383`),
`parsers_are_not_vacuous`(`:146`).

### stale limitations

| 원래 서술 | 지금 |
|---|---|
| "6개 필드가 아직 canonical 에 들어가지 않는다"(`:68`) | ★ 거짓이다. `docs/history/HISTORY.md` 의 "2026-08-16 10:40 — 서명 밖 필드 6건 제거" 항목에서 이미 해소됐다. ★ 이 항목의 **줄 번호는 세션마다 바뀐다** — `HISTORY.md` 는 최신 항목을 맨 위에 추가하는 append 방식이라, 이 정정 시점(2026-08-17 23:55)엔 `:629-645` 였지만 그 뒤 이 세션이 새 항목을 더 추가하면서 지금은 `:651-667` 로 밀렸다(재검수가 실제로 이 어긋남을 잡았다). 그래서 이 항목은 줄 번호 대신 **제목으로** 인용한다 — append-only 파일에서 줄 번호 인용은 그 자체로 불안정하다 |
| "17종 중 7종만 구현"(`:69`) | ★ 거짓이다. 지금 `ToCanonicalFields` 구현 41개, domain 23종 중 19종(`crates/protocol/tests/t1_signing_targets.rs:387-445`) |
| "현재 스키마에 oneof/reserved 가 없어 무해하다"(`:70`) | ★ 절반만 거짓이다. `control.proto` 에 지금 `ControlAction` oneof 가 실재하고, 21개 arm 을 갖는다(`proto/control.proto:238-269`). 정규식 감사기가 oneof 를 못 다루는 한계 자체는 남아 있으나, "지금 스키마에 없다"는 더 이상 참이 아니다. **★ 2026-08-17 23:55 최초 정정 시 "oneof 하위 메시지들은 각각 구현되어 있다"고 적었는데 틀렸다** — 재검수가 직접 세어 지적했다: `crates/protocol/src/to_fields.rs:694-818` 에는 21개 arm 중 **9개**(멤버십·정책 그룹 — `AddMember`·`RemoveMember`·`ApproveDevice`·`RevokeDevice`·`ChangeCoordinatorSet`·`RotateOwnerKey`·`UpdatePolicy`·`QuarantineDevice`·`ReleaseQuarantine`)만 구현되어 있다. 나머지 **12개**(Job 수명주기 5종·Lease 3종·관측 결과 4종 — `SubmitJob`·`TransitionJob`·`CreateAttempt`·`TransitionAttempt`·`SetCanonical`·`IssueLease`·`RenewLease`·`RevokeLease`·`TransitionNode`·`RecordBenchmark`·`RecordWorkloadProfile`·`RecordDurabilityStatus`)는 `ToCanonicalFields` 구현이 **없다**(grep 으로 0건 확인). `ControlAction` wrapper 자체도 미구현이다 |
| "SCHEMA_TOO_NEW 미구현"(`:72`) | ★ 거짓이다. 지금 `verify()` 는 스키마 버전 초과 시 `SchemaTooNew` 를 반환한다(`crates/protocol/src/signing.rs:741-744`). 다만 prost 가 unknown field 를 조용히 버리는 현상 자체는 여전히 사실이다(`crates/protocol/tests/schema_evolution.rs:76-124`) — "경로 미구현"이 아니라 "버전을 안 올린 unknown-field 추가는 여전히 탐지 못 한다"로 좁힌다 |
| "Ed25519 를 저장소 전체에서 아직 하지 않았다"(`:71`) | ★ 부분적으로 거짓이다. Ed25519 verifier·ingress 경로는 지금 존재한다(`crates/crypto/src/lib.rs:120-125`, `crates/crypto/src/ingress.rs:211-215`). "이 evidence(DoD-02) 자체가 Ed25519 를 실행하지 않았다"로 좁힌다 |

Windows 단일 플랫폼·float 경로 미검증 limitation 은 지금도 유효하다.

### 메타데이터

`raw_output` 의 "55 tests"·"15 벡터"는 당시(commit `2c066e2`) 실행
기록이다 — 지금 `tests/vectors/canonical_v1.json` 은 40건이고
`reference_canonical.py --verify` 도 40건 전부 통과한다. 과거 기록과
지금 상태를 혼동하지 않는다.

### review_outcome

`CHANGES_REQUESTED` → 위 정정으로 claim 범위·negative_tests 이름·
stale limitations 를 반영했다. 원본 YAML 은 당시 기록이므로 고치지
않는다.

★ 2026-08-18 00:10 두 번째 재검수 — `agent:codex-cli` 가 여전히
`CHANGES_REQUESTED` 를 냈다. 남은 지적 셋: (a) `Lease` 전 필드
바이트 대조 테스트 인용이 빠졌었다(`prost_canonical.rs:700-735`) —
추가했다. (b) **가장 중요한 지적** — "oneof 하위 메시지들은 각각
구현되어 있다"는 서술이 **과장이 아니라 틀린 사실**이었다. 재검수가
직접 세어 21개 arm 중 9개만 구현됨을 밝혔고, 이 세션도 grep 으로
재확인했다(나머지 12개는 0건) — 위 표를 정정했다. (c)
`HISTORY.md` 인용에 줄 번호가 없었다 — `:629-645` 로 추가했다.

★ 2026-08-18 00:20 세 번째 재검수 — `agent:codex-cli` 가
`ControlAction` 9/21 수치와 `Lease` 인용은 정확하다고 확인했지만,
방금 추가한 `HISTORY.md:629-645` 인용이 **이미 틀렸다는 것**을
잡았다 — 그 사이 이 세션이 새 `HISTORY.md` 항목을 여러 개 더
추가하면서(append-at-top 방식) 대상 항목이 `:651-667` 로 밀려나 있었다.
줄 번호를 다시 맞추는 대신, 위 표의 인용을 **제목 기반**으로
바꿨다 — `HISTORY.md` 처럼 세션 안에서 계속 자라는 append-only
파일은 애초에 줄 번호 인용이 위험하다는 것을 이번에 두 번째로
확인했다(같은 문제가 이 재검수 사이클 동안 두 번 발생했다). — `DoD-05` 는
이번 라운드에서 `ACCEPTED` 를 받았다.

★ 2026-08-18 00:30 네 번째(최종) 재검수 — `agent:codex-cli` 가
**`ACCEPTED`** 로 판정했다. `HISTORY.md` 에서 제목("2026-08-16 10:40
— 서명 밖 필드 6건 제거")으로 항목을 찾아 `JobManifest`
10/11/12/54/55 와 `Lease` 40 편입을 명시함을 확인했고, `ControlAction`
9/21 수치·`Lease` 전 필드 대조 인용·claim 범위 축소·negative_tests
실제 함수명까지 전부 재확인했다. "추가 수정 사항은 확인되지 않았다"
— 다만 테스트 재실행 자체는 하지 않아 현재 실행 결과는 확인 안 됨
으로 남겼다.

이로써 이번 v1-evidence 재검수 사이클(`DoD-02`·`DoD-03`·`DoD-04`·
`DoD-05`·`DoD-06`·`P0-03`) **6건 전부**가 addendum 독립 재검수
`ACCEPTED` 를 받았다 — 라운드 수는 `DoD-02` 4회, `DoD-03` 4회,
`P0-03` 2회, `DoD-04`·`DoD-05`·`DoD-06` 각 3회. 문서 전체를 schema
v2 로 승격하는 것(frontmatter 교체·정식 executor/reviewer 메타데이터
·raw_output digest)은 여전히 별도 작업으로 남아 있다.

## ★ 이후 변경 (2026-08-18 18:00) — domain 수치 재차 stale + 실제 코드 결함 발견, schema v2 승격을 위한 재검수

이 evidence 를 schema v1 -> v2(RULE.md §7.3, ADR-030)로 승격하기
위해 새 독립 검수를 받았다(`agent:codex-cli`, fresh-read-only,
`p66` 프롬프트) — `CHANGES_REQUESTED`.

### domain 수치가 또 stale — 이번엔 진짜 코드 결함이었다

바로 위 addendum(2026-08-18 00:10 무렵)이 "23종 중 19종"으로
정정했는데, `DoD-01` 의 같은 날 재검수와 똑같은 이유로 이미 또
stale 이었다 — 같은 세션 안에서 `Domain::GrantAck` 가 추가됐다.
지금은 **24종 중 20종**이다.

다만 이번엔 evidence 문서만의 문제가 아니었다 — **실제 회귀 방지
테스트 자체에 결함이 있었다.**
`crates/protocol/tests/t1_signing_targets.rs::domain_coverage_is_explicit`
가 domain 목록을 `Domain` enum 에서 자동으로 뽑지 않고 손으로 나열한
배열(`coverage`)을 쓰는데, `Domain::GrantAck` 를 추가했을 때 이
배열을 갱신하지 않았다 — 그런데도 `assert_eq!(coverage.len(), 23,
...)` 가 계속 통과했다. 왜냐하면 이 assert 는 **그 손으로 쓴 배열
자신의 길이**를 세지, `Domain` enum 의 실제 variant 개수를 세지
않기 때문이다 — enum 에 새 variant 가 추가돼도 이 테스트는 그
사실을 전혀 모른다. `canonical_vectors.rs::domain_tags_are_32_bytes_and_unique`
는 실제 `Domain` 값들을 순회하므로 새 variant 를 빠뜨리면 그 자체로
개수가 안 맞아 잡히지만, 이 테스트는 그런 자동 대조 장치가 없었다.

이 세션이 코드를 고쳤다 — `coverage` 배열에
`(Domain::GrantAck, Some("AgentGrantAck"), true)` 를 추가하고,
`assert_eq!(coverage.len(), 24, ...)` 와
`assert_eq!(implemented, 20, ...)` 로 갱신했다. `cargo test -p
gputeer-protocol --test t1_signing_targets domain_coverage_is_explicit`
로 직접 확인 — `domain 24종 — 구현 20 · proto 메시지 없음 4`.
`cargo test --workspace` 도 재실행해 회귀 없음을 확인했다(306/0/1
유지 — 이 테스트는 이미 있던 테스트라 개수가 늘지 않는다).

### negative_tests — 이름은 실재, 약칭 표기가 혼동을 줄 수 있다

frontmatter 의 `"job_manifest_field_numbers_match_proto /
lease_ / common_"`(`:63`) 는 세 함수를 `/` 로 묶은 약칭이다 —
`lease_field_numbers_match_proto`(`field_number_audit.rs:194`),
`common_message_field_numbers_match_proto`(`field_number_audit.rs:251`)
가 온전한 이름이다. 원문 약칭을 append-only 원칙에 따라 고치지
않는다 — 여기 명시하는 것으로 충분하다.

### limitations 4건 재확인 — 지금도 전부 stale(이전 addendum 대로)

- "6개 필드 미구현"(`:68`) — 지금도 거짓. `UNIMPLEMENTED_FIELDS` 는
  비어 있다(`to_fields.rs:953-963`).
- "17종 중 7종만 구현"(`:69`) — 지금은 **24종 중 20종**(위 절 참조,
  이전 addendum 의 "23종 중 19종"도 이미 stale 이었다).
- `ControlAction` oneof "9/21 구현"은 **지금도 정확하다** —
  `to_fields.rs:694,707,718,739,751,765,778,797,811` 9곳을 직접
  세어 재확인했다. `AgentGrantAck` 는 `ControlAction` oneof 의
  일부가 아니라 `control.proto` 의 독립 top-level 메시지다
  (`control.proto:377-390`) — 9/21 수치에 포함시키면 안 된다는
  점도 확인했다.
- Ed25519·`SCHEMA_TOO_NEW` 관련 stale 정정은 이전 addendum 그대로
  지금도 유효하다.

### cargo test — 확인 안 됨(검수자) → 이 세션이 직접 실행해 해소

검수자의 read-only 샌드박스는 `.cargo-build-lock` 접근 거부로
`cargo test` 를 직접 돌리지 못했다. 이 세션이 승격 직전
`cargo test -p gputeer-protocol --test prost_canonical --test
field_number_audit --test canonical_vectors` 를 직접 실행해
50/50 통과를 확인했다(`docs/evidence/_raw/DoD-02_v2_promotion_2026-08-18.txt`).

### review_outcome

`CHANGES_REQUESTED` → domain 24/20 갱신(evidence 본문 + 실제 코드
`t1_signing_targets.rs` 둘 다) + cargo test 재실행 결과 첨부로 해소.

## ★ 이후 변경 (2026-08-18 18:10) — schema v1 → v2 승격

좁은 후속 확인 재검수(`agent:codex-cli`, fresh-read-only)에서
**`ACCEPTED`** 를 받았다 — 두 지적(domain 24/20, cargo test 미확인)
모두 해소됐음을 확인했다. 전문은 `docs/evidence/_raw/DoD-02_review.txt`
참조.

이 검수를 근거로 이 문서를 `schema_version: 1`(유예 목록)에서
`schema_version: 2`(RULE.md §7.3, ADR-030)로 승격했다. frontmatter
에 `executor_*`·`reviewer_*`·`review_*`·`raw_output_artifact`/
`digest`/`bytes` 필드를 새로 추가했고(기존 필드는 위 "정정" 절과
같은 이유로 손대지 않았다), `docs/evidence/_schema_v1_grandfathered.txt`
에서 이 파일명을 지우고 `scripts/verify_evidence.py` 의
`GRANDFATHER_DIGEST` 상수를 갱신했다.
