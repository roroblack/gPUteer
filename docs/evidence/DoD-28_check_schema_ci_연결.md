---
schema_version: 2
id: DoD-28
claim: "docs/plans/2026-08-20_0000_check_schema_py_v1.md(DoD-20)이 limitations 절에 명시적으로 남긴 공백 — check_schema.py를 CI 파이프라인에 실제로 연결하지 않았다 — 를 닫았다. .github/workflows/canonical-schema-check.yml을 신설해 main 대상 push/pull_request에서 canonical 참조 self-test, canonical 벡터 대조, check_schema.py, 워크스페이스 build/test(gputeer-runtime-windows 제외), verify_evidence.py를 순서대로 실행한다. 이 저장소는 원격이 설정돼 있지 않아 워크플로 파일은 아직 어디서도 실제 실행된 적이 없다 — 로컬에서 동일 명령을 순서대로 실행해 성공을 확인하는 것으로 검증을 대신했다"
status: PASS
commit: bb93f2f

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현) / claude-code (git 상태·evidence 스키마 독립 재확인)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T04:24:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "워크플로 YAML이 실제로 존재하고 파싱 가능한지, push/pull_request 트리거가 main 대상인지, 각 스텝이 이 저장소에 실제로 존재하는 파일/명령을 가리키는지(특히 --exclude gputeer-runtime-windows 이름이 crates/runtime-windows/Cargo.toml의 package name과 정확히 일치하는지), Rust 툴체인 버전이 Cargo.toml의 rust-version(1.89)과 일치하는지, apt protobuf-compiler 설치가 실제로 필요한지(check_schema.py가 protoc-bin-vendored와 별개로 PATH의 protoc를 직접 subprocess 호출하는지 코드로 확인), permissions가 최소 권한(contents: read)인지, 워크플로가 이미 커밋/push되지 않았는지(git remote 자체가 없음 확인), docs/evidence·CLAUDE.md·HISTORY.md가 이번 변경에서 안 건드려졌는지. 핵심 명령(check_schema.py·verify_evidence.py)을 독립적으로 재실행해 재확인. 1라운드(p159) 만에 ACCEPTED — 수정 요청 없음"
review_artifact: "docs/evidence/_raw/DoD-28_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-28_check_schema_ci_연결_2026-08-20.txt"
raw_output_digest: "sha256:57501d532c353be0f41672e022d54a6c5a1fda28a102e0927b6d1f1fbb04f04c"
raw_output_bytes: 1902

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — 이번 조각은 순수 CI 설정 파일(.github/workflows/*.yml) 추가다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc (구현·1차 로컬 검증) — 워크플로 자체가 대상으로 하는 러너는 ubuntu-latest(GitHub 원격)"
hardware: "Intel Iris Xe Graphics / GPU 무관 — CI 워크플로는 GPU 를 쓰지 않는다"
network_profile: "GitHub API 호출 없음, 원격 push 없음 — 이 저장소는 origin 자체가 설정돼 있지 않다"
command: |
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  python tools/canonical/check_schema.py
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  python scripts/verify_evidence.py
  python -c "import yaml; yaml.safe_load(open('.github/workflows/canonical-schema-check.yml'))"
raw_output: |
  (docs/evidence/_raw/DoD-28_check_schema_ci_연결_2026-08-20.txt 전문 참조)

  reference_canonical.py --self-test: 성공, 12개 PASS
  reference_canonical.py --verify: 성공, 45개 벡터 일치
  check_schema.py: 성공, 오류 0건·경고 42건
  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공, 실패 0건
  verify_evidence.py: 종료 코드 0, PASS 35/36·FAIL-SCOPE 1
  YAML 파싱: 성공
artifacts:
  - .github/workflows/canonical-schema-check.yml
  - docs/plans/2026-08-20_0424_check_schema_ci_연결_v1.md
  - docs/evidence/_raw/DoD-28_check_schema_ci_연결_2026-08-20.txt
  - docs/evidence/_raw/DoD-28_review.txt
negative_tests:
  - "워크플로가 실제로 커밋/push된 적이 없음을 git log·git remote -v로 확인 — 이 조각의 검증은 '로컬에서 동일 명령이 순서대로 성공한다'는 것이지 'GitHub Actions에서 실제로 그린이 떴다'는 것이 아니다(이 저장소는 원격이 없어 후자를 이 세션에서 증명할 수 없다)"
  - "독립 검수가 apt protobuf-compiler 설치 스텝이 불필요한 중복이 아닌지 의심하고 check_schema.py 소스(72-76행, 102-110행)를 직접 읽어 PATH의 protoc를 subprocess로 직접 호출함을 확인 — Cargo의 protoc-bin-vendored와는 별개 경로임을 코드로 검증"
limitations:
  - "워크플로가 실제 GitHub Actions 러너(ubuntu-latest)에서 성공적으로 돈 적은 없다 — 원격이 없어 이 세션에서는 증명 불가능하다. 로컬 재현(cargo 1.97.1, Windows) 명령 성공만으로 대신했다"
  - "독립 검수 시 check_schema.py 재실행이 read-only 샌드박스의 임시 descriptor 파일 생성 제약으로 ENVIRONMENT-BLOCKED됐다 — 코드 결함이 아니라 검수 환경의 쓰기 제한이며, 구현 시점(workspace-write)에는 이미 정상 실행을 확인했다"
  - "Linux 에서 cargo test 전체가 통과하는지는 ENV-03(remote5090 원격 기계)의 과거 실측을 근거로 삼았을 뿐, 오늘 이 워크플로 자체로 재확인하지 않았다 — ENV-03 기록에는 k1c 테스트 1건이 Linux 에서 실패한 이력도 있어, 실제 GitHub Linux 러너에서의 전체 통과 여부는 여전히 미확인이다"
  - "act(로컬 GitHub Actions 실행기)는 설치돼 있지 않아 사용하지 않았다 — 워크플로 문법과 개별 명령의 로컬 성공만 확인했을 뿐, 워크플로 실행기 자체의 동작(캐싱, 환경변수 주입 등)은 검증하지 않았다"
decision: "check_schema.py(DoD-20)가 남긴 'CI에 실제로 연결하지 않았다'는 공백을 닫았다 — .github/workflows/canonical-schema-check.yml 하나로 canonical 참조 구현·schema 검사·워크스페이스 build/test·evidence 스키마 검사까지 한 파이프라인에 묶었다. 원격이 없어 실제 GitHub Actions 실행 자체는 증명 못 했지만, 로컬에서 동일 명령의 성공을 확인했고 구현을 코덱스 CLI(workspace-write)에 위임한 뒤 독립 검수(대화 기록 없는 새 인스턴스, read-only)가 YAML 문법·트리거·경로 정확성·최소 권한·apt 설치의 실제 필요성까지 코드로 추적해 1라운드 만에 ACCEPTED했다."
---

# DoD-28 · check_schema.py CI 연결

## 무엇을 입증하려 했는가

`docs/plans/2026-08-20_0000_check_schema_py_v1.md`(`DoD-20`,
`tools/canonical/check_schema.py` 신규 구현)의 limitations 절이
명시했다: "CI 파이프라인에 실제로 연결하지 않았다 — 스크립트만
만들었다." 오늘 백로그 정리 조사(`p152`)가 이 항목을 "코드 위험이
낮고 이후 proto 변경을 자동 차단하는 가장 싼 안전장치"라며 세
후보 중 하나로 꼽았다.

## 구현 (코덱스, `p156`)

- `.github/workflows/canonical-schema-check.yml` 신설 — `main` 대상
  `push`·`pull_request` 트리거, `ubuntu-latest` 러너, Rust 1.89
  (`Cargo.toml` 의 `rust-version` 과 일치), Python 3, `protobuf-compiler`
  (apt) + `protobuf`·`blake3`(pip) 설치.
- 순서대로 실행: `reference_canonical.py --self-test` →
  `reference_canonical.py --verify tests/vectors/canonical_v1.json`
  → `check_schema.py` → `cargo build --workspace --exclude
  gputeer-runtime-windows` → `cargo test --workspace --exclude
  gputeer-runtime-windows` → `scripts/verify_evidence.py`.
- `permissions: contents: read` 로 최소 권한.
- `docs/plans/2026-08-20_0424_check_schema_ci_연결_v1.md` 계획 문서
  작성 — `check_schema.py` 가 Cargo 의 `protoc-bin-vendored` 와
  별개로 PATH 의 `protoc` 를 직접 subprocess 호출하므로 apt 설치가
  중복이 아니라 실제로 필요함을 명시.
- 지침대로 `docs/evidence/`·`CLAUDE.md`·`docs/history/HISTORY.md`
  는 건드리지 않았다 — 이번 evidence 기록은 별도 세션(이 문서)이
  담당한다.

## 독립 검수(`p159`) — **1라운드 만에 ACCEPTED**

YAML 존재·파싱, 트리거 대상, 각 스텝의 파일/명령 경로 정확성
(`--exclude gputeer-runtime-windows` 이름이 `crates/runtime-windows/Cargo.toml`
의 실제 package name 과 일치하는지 포함), Rust 버전 일치, apt
`protobuf-compiler` 설치의 실제 필요성(`check_schema.py` 소스를
직접 읽어 확인), 최소 권한, 워크플로가 아직 커밋·push 된 적이
없는지(`git log`·`git remote -v`), evidence/CLAUDE.md/HISTORY 무변경
여부까지 전부 코드/명령으로 직접 재확인했다. `check_schema.py`
재실행은 read-only 샌드박스의 임시 파일 생성 제약으로
ENVIRONMENT-BLOCKED 됐으나(코드 결함 아님), `verify_evidence.py` ·
`reference_canonical.py --self-test` 는 독립적으로 재실행해 성공을
재확인했다. 잔여 지적 없음.

## 결과

```text
reference_canonical.py --self-test                              성공, 12개 PASS
reference_canonical.py --verify canonical_v1.json                성공, 45개 벡터 일치
check_schema.py                                                  성공, 오류 0건·경고 42건
cargo build/test --workspace --exclude gputeer-runtime-windows   성공, 실패 0건
verify_evidence.py                                                종료 코드 0, PASS 35/36
YAML 파싱(PyYAML)                                                  성공
```

## 이 실험이 증명하지 "않는" 것

- 워크플로가 실제 GitHub Actions(`ubuntu-latest`)에서 성공적으로
  돈 적은 없다 — 이 저장소는 원격이 없다. 로컬 재현 성공만으로
  대신했다.
- Linux 전체 워크스페이스 테스트 통과는 `ENV-03`(remote5090) 의 과거
  실측을 근거로 했을 뿐, 오늘 이 워크플로로 재확인하지 않았다.
- `act` 로컬 실행기로 워크플로 자체를 실제로 실행해보지 않았다.

## 결정

1. `check_schema.py`(`DoD-20`)가 남긴 CI 미연결 공백을 닫았다 —
   구현은 코덱스 CLI(workspace-write)에 위임했다.
2. 독립 검수(대화 기록 없는 새 인스턴스, read-only)가 YAML·트리거·
   경로·버전·권한·apt 설치 필요성까지 전부 코드로 추적해 1라운드
   만에 `ACCEPTED`.
3. 원격이 없어 실제 GitHub Actions 실행 자체는 이 세션 범위 밖으로
   남는다 — 사용자가 나중에 원격을 연결하면 첫 실제 실행 결과를
   별도로 확인해야 한다.

관련: `docs/evidence/DoD-20_check_schema_py.md`(선행 조각) ·
`docs/plans/2026-08-20_0424_check_schema_ci_연결_v1.md`
