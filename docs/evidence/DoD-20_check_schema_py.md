---
schema_version: 2
id: DoD-20
claim: "tools/canonical/check_schema.py 를 신규 구현했다 — reference_canonical.py 의 SCHEMAS 딕셔너리(서명 대상 메시지 필드 표를 손으로 옮겨 적은 것)를 protoc --descriptor_set_out 으로 뽑은 FileDescriptorSet 과 구조적으로 대조해, field number/이름/타입/nested 참조 불일치를 잡아낸다. field 90(서명 필드)은 canonical_encode() 가 번호로만 무조건 건너뛰므로 타입 비교에서 명시적으로 제외하고(번호·이름은 그대로 검사), derived hash field(manifest_hash)는 INFO 로만 보고한다. 현재 저장소 상태에서 실행하면 오류 0건(경고 42건은 SCHEMAS 가 서명 대상만 다루는 설계라 정상). 실행 환경 오류(protoc 없음/컴파일 실패/임시 파일 I/O 실패/descriptor 디코드 실패)는 전부 exit(2) 로, schema mismatch 는 exit(1) 로 구분된다"
status: PASS
commit: 1a419f8ffa0675a89a7506044fc66abefb291081

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + python + protoc)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-20T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high (설계·1라운드) / medium (2라운드)"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "descriptor 생성/파싱 방식의 타당성, protoc 없음/컴파일 실패/임시 파일 I/O 실패/descriptor 디코드 실패가 전부 exit(2) 로 schema mismatch(exit 1) 와 구분되는지, nested_type 재귀 인덱싱과 map synthetic entry 처리, field 90 예외의 근거(canonical_encode() 가 번호로만 무조건 건너뛰는지 직접 코드 확인), DERIVED_HASH_FIELDS 재사용 여부, --json 출력 구조. 1라운드(p120) — CHANGES_REQUESTED(코덱스가 자신의 read-only 샌드박스에서 실제로 실행해보다가 임시 파일 생성 실패가 처리되지 않은 예외로 새어나가 exit(1)이 되는 것을 직접 재현). 수정(descriptor 생성·읽기·파싱 전 구간을 OSError/DecodeError 로 감싸 exit(2) 통일) 뒤 2라운드(p121) — ACCEPTED, 짧은 이름 인덱스 충돌 시 후자가 덮어쓰는 점은 이미 문서화된 알려진 한계로 판단해 blocking 사유로 삼지 않음"
review_artifact: "docs/evidence/_raw/DoD-20_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-20_check_schema_2026-08-20.txt"
raw_output_digest: "sha256:baa957ce7eb0934caf0d3707fed81081212ed5952f2fc4835917f2a6200d66bc"
raw_output_bytes: 55239

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14) / protoc 25.3 (anaconda) / python 3.12.7 / google.protobuf 7.35.1"
protocol_versions:
  schema_version: "해당 없음 — 이 조각은 도구 스크립트다. proto/*.proto·SCHEMAS·canonical 인코딩 로직 자체는 바꾸지 않는다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "해당 없음 — 로컬 protoc 컴파일과 순수 Python 비교만 수행한다"
command: |
  python tools/canonical/check_schema.py
  python tools/canonical/check_schema.py --json
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  cargo test --workspace
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 회귀 없음 확인, 24개 시나리오 5회
raw_output: |
  (docs/evidence/_raw/DoD-20_check_schema_2026-08-20.txt 전문 참조)

  schema 검사 — .../proto
  오류 0건, 경고 42건 (SCHEMAS 43개 메시지 / .proto 85개 메시지)
  schema 검사 통과

  reference_canonical.py --self-test: all checks passed
  reference_canonical.py --verify: 재생성 대조 45개 벡터 일치, vector cross-checks: OK
  cargo test --workspace: 전체 스위트 통과, 0 failed
  coordinator-agent-selftest: 24개 시나리오, 5회 연속 전부 exit=0
artifacts:
  - docs/plans/2026-08-20_0000_check_schema_py_v1.md
  - tools/canonical/check_schema.py
  - proto/README.md
  - docs/evidence/_raw/DoD-20_check_schema_2026-08-20.txt
  - docs/evidence/_raw/DoD-20_review.txt
negative_tests:
  - "뮤테이션 1(타입 불일치) — JobManifest.env_vars 의 SCHEMAS kind 를 map_ss -> bytes 로 바꾸자 정확히 field-type 오류로 잡힘. 원복 후 재통과"
  - "뮤테이션 2·3(번호·이름 불일치) — job_id 의 field number 를 2 -> 199 로, team_id 의 이름을 wrong_name_on_purpose 로 바꾸자 각각 missing-in-proto·field-name 오류로 잡히고, 부수적으로 .proto 의 실제 #2 필드가 SCHEMAS 에서 사라진 것도 missing-in-schema 로 잡힘(4건 동시 검출). 원복 후 재통과"
  - "★ 코덱스 1라운드(p120)가 실제로 실행해보다가 찾은 결함 — 임시 descriptor 파일 생성(NamedTemporaryFile)이 바깥 try 밖에 있었고 read_bytes()/MergeFromString() 의 오류도 안 잡혀서, 실행 환경이 실패하면(예: 샌드박스가 임시 디렉터리 쓰기를 막는 경우) 처리되지 않은 예외가 그대로 새어나가 exit(1)이 됐다 — schema mismatch(정상 exit 1)와 구분 불가능한 계약 위반이었다. 수정 전/후 버전을 직접 대조해 재현: 수정 전 버전은 NamedTemporaryFile 실패를 mock 하면 처리되지 않은 OSError 가 그대로 새는 것을 확인했고(실제 실행 시 traceback + exit 1), 수정 후에는 정확히 SystemExit(2)로 변환됨을 확인"
  - "field 90 예외의 정당성 검증 — SIGNATURE_FIELD_NUMBER 상수를 -1로 바꿔 예외를 무력화하면, 설계 단계(p119)에서 손으로 찾은 정확히 그 3건(RevokeDevice.signatures·UpdatePolicy.signatures·QuarantineDevice.verdict_signatures, 전부 SCHEMAS=bytes vs 실제 .proto=repeated bytes)가 field-type 오류로 재현됨 — 검사기의 핵심 비교 로직이 실제 .proto 파일에 대해 진짜로 동작한다는 것을 설계 단계의 수작업 발견과 독립적으로 재확인"
limitations:
  - "SCHEMAS 는 원래 서명 대상 메시지와 그 nested helper만 다루는 설계다 — .proto 의 나머지 42개 메시지(ControlAction·ProposeRequest 등 ControlStore RPC 메시지 다수)는 SCHEMAS 에 없는 것이 정상이라 경고로만 처리한다. 이 42개가 실제로 서명 대상이 돼야 하는지는 이 조각의 범위 밖이다"
  - "짧은 메시지 이름으로 인덱싱한다 — 이름이 겹치는 nested/top-level 메시지가 생기면 나중 것이 앞선 것을 덮어쓴다. 현재 저장소 규모(85개 메시지)에서는 발생하지 않으며, 코덱스도 두 라운드 모두 이를 확인하되 blocking 사유로 삼지 않았다"
  - "이 개발 기계의 anaconda protoc(25.3)를 쓴다 — crates/protocol/build.rs 는 protoc_bin_vendored 의 별도 vendored protoc 를 쓴다(설계 p119 가 지적). 두 컴파일러가 항상 동일한 결과를 낸다고 보장하지 않는다 — 검사 대상이 descriptor 의 메시지·필드 번호·이름·타입뿐이라 버전 차이의 영향은 작다고 판단했지만, 별도로 검증하지 않았다"
  - "CI 파이프라인에 실제로 연결하지 않았다 — 스크립트만 만들었다. 언제 CI 에 연결할지는 이 저장소의 CI 설정 자체가 범위 밖이다"
  - "코덱스는 read-only 샌드박스에서 실제 실행을 시도했으나 임시 디렉터리 쓰기 제한으로 정상 경로(오류 0건) 자체는 끝까지 실행하지 못했다 — mock 으로 오류 경로만 재현했다. 정상 경로의 실행 검증은 claude-code 세션이 직접 수행했다"
  - "Windows 단일 플랫폼에서만 실행했다"
decision: "proto/README.md 가 오래전부터 안내·전제해온 check_schema.py 를 신규 구현했다 — reference_canonical.py 자신이 이미 '표와 .proto 가 어긋나면 CI 가 잡도록 check_schema.py 를 둔다'고 전제하고 있었다. 설계 단계 실측이 실제 drift 3건(전부 field 90, 인코딩에는 무영향)을 찾았고, 구현 후 코덱스 1라운드가 실행 환경 오류 처리의 진짜 결함(exit 코드 계약 위반)을 실제 실행으로 찾아내 수정, 2라운드에서 ACCEPTED. 뮤테이션 테스트 4건(타입/번호/이름 불일치, field 90 예외 무력화)과 실행 전/후 대조 1건으로 비공허성을 확인했다. 이것으로 코덱스가 이 세션 동안 순차적으로 찾은 진행 가능한 후보(p104→p110→p116→p118)를 모두 소진했다."
---

# DoD-20 · tools/canonical/check_schema.py

## 무엇을 입증하려 했는가

`proto/README.md:70`이 `python tools/canonical/check_schema.py`
("스키마 표 정합성 — 참조 구현의 필드 표 ↔ .proto")를 실행하라고
안내했지만, 그 파일이 `tools/canonical/` 에 실제로 없었다.
`proto/README.md:97`도 "check_schema.py — 필드 표와 .proto 정합성
검사 (M1-02)"를 미완료(⬜)로 명시하고 있었다. `reference_canonical.py`
자신도 "표와 .proto 가 어긋나면 CI 가 잡도록
tools/canonical/check_schema.py 를 둔다"고 이미 전제하고 있었다
(`reference_canonical.py:83-84`) — 코덱스 최종 확인 감사(`p118`)가
이 저장소에서 남은 마지막 진행 가능한 후보로 찾았다.

## 설계(코덱스 `p119`)와 실측

`protoc --descriptor_set_out` 으로 `FileDescriptorSet` 을 뽑아
`google.protobuf.descriptor_pb2` 로 구조적으로 파싱하는 방식을
정규식 파싱보다 우선 채택하도록 설계했다(주석·문자열·nested
message·map synthetic entry·oneof 에 취약하지 않다).

설계 단계 실측이 실제 drift 3건을 찾았다: `RevokeDevice.signatures`·
`UpdatePolicy.signatures`·`QuarantineDevice.verdict_signatures` 가
`SCHEMAS` 에는 `bytes` 로 적혀 있지만 `.proto` 실제 선언은 `repeated
bytes` 다. 전부 field number 90(서명 필드)이고, `canonical_encode()`
가 field number 로만 무조건 이 필드를 건너뛰므로(`reference_canonical.py:546,877`)
인코딩에는 영향이 없다 — `SCHEMAS` 를 건드리지 않고 검사기 쪽에서
field 90 을 타입 비교에서 명시적으로 제외하도록 설계했다.

## 구현

`tools/canonical/check_schema.py` 신설. `nested_type` 을 재귀
인덱싱해 map synthetic entry(`JobManifest.EnvVarsEntry` 등)를
정확히 `map_ss` 로 판정하고, `DERIVED_HASH_FIELDS`(기존
`reference_canonical.py:512`)를 재사용해 `ExecutionGrant.manifest_hash`
같은 의도된 생략을 INFO 로만 보고한다. `.proto` 에만 있는 메시지는
경고로만 처리한다(`SCHEMAS` 는 원래 서명 대상만 다루는 설계).

## 코덱스 1라운드(`p120`) — 실행해보다가 실제 결함 발견

코덱스가 자신의 read-only 샌드박스에서 실제로 `python
check_schema.py` 를 실행해보려다 임시 디렉터리 쓰기 제한에
부딪혔는데, 그 과정에서 **처리되지 않은 `OSError` 가 그대로 새어나가
exit(1) 이 되는 것**을 직접 재현해냈다 — docstring 이 약속한
"0=오류없음, 1=schema mismatch, 2=실행 환경 오류" 계약이 임시 파일
생성 실패 경로에서 지켜지지 않았다.

## 수정

descriptor 생성(`NamedTemporaryFile`)부터 `protoc` 실행·파일 읽기·
`MergeFromString` 파싱까지 전 구간을 `OSError`/
`message.DecodeError` 를 잡는 코드로 감싸 전부 `exit(2)` 로
통일했다. 수정 전/후 버전을 직접 대조해 확인했다 — 수정 전 버전은
`NamedTemporaryFile` 실패를 `mock` 하면 처리되지 않은 `OSError` 가
그대로 새는 것을 재현했고(실제 실행 시 traceback + exit 1), 수정
후에는 정확히 `SystemExit(2)` 로 변환된다.

## 코덱스 2라운드(`p121`) — ACCEPTED

수정된 전 구간이 실제로 `OSError`/`DecodeError` 를 다 잡는지, 모든
경로에서 `finally` 로 임시 파일이 정리되는지 확인했다. 짧은 이름
인덱스 충돌 시 후자가 덮어쓰는 점은 이미 문서화된 알려진 한계로
판단해 이번 판정을 막을 사유로 삼지 않았다.

## 결과

```text
python tools/canonical/check_schema.py                오류 0건, 경고 42건, exit=0
reference_canonical.py --self-test                     all checks passed
reference_canonical.py --verify canonical_v1.json       45개 벡터 일치
cargo test --workspace                                  전체 스위트 통과, 0 failed
coordinator-agent-selftest                              24개 시나리오, 5회 연속 통과(회귀 없음)
```

### 뮤테이션·대조로 비공허성을 확인했다 (4건)

1. 타입 불일치(`map_ss` → `bytes`) → 정확히 잡힘.
2. 필드 번호 불일치(`job_id` #2 → #199) → `missing-in-proto` +
   부수적으로 `missing-in-schema` 도 동시 검출.
3. 필드 이름 불일치(`team_id` → `wrong_name_on_purpose`) → 정확히
   잡힘.
4. field 90 예외 무력화(`SIGNATURE_FIELD_NUMBER = -1`) → 설계
   단계에서 손으로 찾은 정확히 그 3건이 재현됨 — 검사기 로직이 실제
   `.proto` 파일에 대해 진짜로 동작함을 독립적으로 재확인.

## 이 실험이 증명하지 "않는" 것

- `SCHEMAS` 에 없는 42개 `.proto` 메시지가 실제로 서명 대상이 돼야
  하는지는 범위 밖이다.
- `crates/protocol/build.rs` 의 vendored protoc 와 이 도구가 쓰는
  anaconda protoc 가 항상 같은 결과를 낸다고 검증하지 않았다.
- CI 파이프라인 연결은 범위 밖이다 — 스크립트만 만들었다.
- Windows 단일 플랫폼.

## 결정

1. `check_schema.py` 를 계획대로 구현·검증했다.
2. 코덱스 1라운드가 실제 실행으로 진짜 결함(exit 코드 계약 위반)을
   찾아냈고, 수정 후 2라운드에서 `ACCEPTED`.
3. 코덱스가 이 세션 동안 순차 감사(`p104`→`p110`→`p116`→`p118`)로
   찾은 진행 가능한 후보를 모두 소진했다.

관련: `docs/evidence/DoD-19_오래된_테스트_공백_3건.md` ·
`docs/plans/2026-08-20_0000_check_schema_py_v1.md`
