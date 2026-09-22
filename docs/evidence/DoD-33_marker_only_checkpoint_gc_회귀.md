---
schema_version: 2
id: DoD-33
claim: "DoD-30(Job 시작 WRITING 마커) evidence 문서가 주장했던 'WRITING 마커만 있는 디렉터리는 기존 gc_partial() 규칙상 PARTIAL 로 취급돼 GC 대상' 이라는 주장을 실제로 검증하는 회귀 테스트를 추가했다 — crates/checkpoint/tests/durability_chaos.rs 에 신설한 startup_gc_removes_marker_only_checkpoint_but_preserves_manifest_checkpoint 가 .durability.writing 마커만 있는 디렉터리는 startup_gc()/gc_partial() 실행 후 실제로 삭제되고, manifest.json+데이터 파일까지 있는 완결된 디렉터리는 보존됨을 파일시스템 상태(exists/is_dir/is_file)로 직접 확인한다. GC 알고리즘 자체(crates/checkpoint/src/atomic.rs·writer.rs)와 crates/agent/src/lib.rs 는 전혀 바꾸지 않은 순수 테스트 추가다"
status: PASS
commit: 6b3213d

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현) / claude-code (cargo build·cargo test -p gputeer-checkpoint --test durability_chaos·워크스페이스 전체 테스트 독립 재실행 — 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T07:09:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "gc_partial() 의 실제 판정식(atomic.rs:589 의 !manifest_exists || (is_tmp && !is_registered))이 구현자 보고와 정확히 일치하는지, startup_gc() 가 이를 호출하는지(writer.rs:407), Agent 가 실제로 만드는 디렉터리 구조(agent/lib.rs:582-602, record_initial_state() 가 .durability.writing 만 생성, 마커명이 durability.rs:115·141 과 일치)가 새 테스트가 흉내낸 구조와 정확히 일치하는지, 신규 테스트(durability_chaos.rs:179-197)가 실제 파일시스템 상태로 두 assert(marker-only 삭제·완결 디렉터리 보존)를 확인하는지, 뮤테이션 주장(판정 조건 반전 시 두 assert 모두 실패)의 논리적 타당성, 범위 확인(git diff --stat 정확히 durability_chaos.rs 한 파일, writer.rs·agent/lib.rs 무변경). 1라운드(p175) 만에 ACCEPTED — 수정 요청 없음. 감독자(claude-code)가 검수 완료 후 cargo build·cargo test -p gputeer-checkpoint --test durability_chaos(17개 전부 통과)·워크스페이스 전체 테스트로 독립 재확인"
review_artifact: "docs/evidence/_raw/DoD-33_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-33_marker_only_checkpoint_gc_회귀_2026-08-20.txt"
raw_output_digest: "sha256:82bc12362aa7670dc28e36ac36d213201509c1c128a396547f033ef06dfa44d2"
raw_output_bytes: 1879

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — 이번 조각은 crates/checkpoint 의 기존 테스트 파일에 순수 테스트 함수 1건만 추가한다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 로컬 파일시스템 checkpoint 디렉터리 조작"
network_profile: "네트워크 없음 — 순수 로컬 파일시스템 테스트"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test -p gputeer-checkpoint --test durability_chaos
  cargo test --workspace --exclude gputeer-runtime-windows
raw_output: |
  (docs/evidence/_raw/DoD-33_marker_only_checkpoint_gc_회귀_2026-08-20.txt,
   docs/evidence/_raw/DoD-33_review.txt 전문 참조)

  cargo build --workspace --exclude gputeer-runtime-windows: 성공
  cargo test -p gputeer-checkpoint --test durability_chaos: 17개 전부 통과
    (신규 startup_gc_removes_marker_only_checkpoint_but_preserves_manifest_checkpoint 포함)
  cargo test --workspace --exclude gputeer-runtime-windows: 성공, 실패 0건
artifacts:
  - crates/checkpoint/tests/durability_chaos.rs
  - docs/evidence/_raw/DoD-33_marker_only_checkpoint_gc_회귀_2026-08-20.txt
  - docs/evidence/_raw/DoD-33_review.txt
negative_tests:
  - "startup_gc_removes_marker_only_checkpoint_but_preserves_manifest_checkpoint — .durability.writing 마커만 있는 checkpoint 디렉터리가 startup_gc() 실행 후 실제로 삭제됨을 확인(Path::exists()==false)"
  - "같은 테스트 — manifest.json 과 데이터 파일까지 있는 완결된 checkpoint 디렉터리는 같은 GC 호출 뒤에도 보존됨을 확인(디렉터리·파일 전부 exists()==true)"
  - "뮤테이션(코덱스 자체 보고, p174) — gc_partial() 의 판정 조건을 임시로 반전하면 테스트가 exit 101 로 실패함을 확인, 원복 후 durability_chaos 17개 테스트 재검증 통과"
limitations:
  - "다중 Agent 가 동시에 GC 와 활성 writer 를 경합하는 시나리오는 다루지 않는다 — DoD-21 이 이미 이걸 의도적으로 범위 밖(다중 Agent 실행 시작이 트리거)으로 남겨뒀고, 이번 조각도 그 경계를 유지한다"
  - "이 테스트는 Agent 의 실제 checkpoint_id 생성 규칙(BLAKE3-256 digest, DoD-30)까지 재현하지는 않는다 — 디렉터리 구조(마커 파일 이름·위치)만 Agent 의 실제 레이아웃과 일치시켰다. 독립 검수가 이 레이아웃 일치 자체는 코드로 확인했다"
  - "GC 가 실제로 언제 트리거되는지(주기적 실행·Coordinator 요청 등)는 이번 조각의 범위 밖 — startup_gc() 를 직접 호출하는 경로만 검증했다"
decision: "DoD-30 이 문서로만 주장했던 'marker-only 디렉터리는 GC 대상' 이라는 안전 불변식을 실제 회귀 테스트로 고정했다 — GC 알고리즘 자체는 바꾸지 않고 기존 동작이 맞다는 것을 증명만 했다. 구현을 코덱스 CLI(workspace-write)에 위임했고, 독립 검수(대화 기록 없는 새 인스턴스, read-only)가 판정 조건·디렉터리 레이아웃 일치·assert 의 실질성·뮤테이션 논리까지 전부 코드로 확인하고 1라운드 만에 ACCEPTED. 감독자가 직접 재현해 재확인했다."
---

# DoD-33 · marker-only checkpoint GC 회귀 테스트

## 무엇을 입증하려 했는가

백로그 재조사(`p167`) 3순위 — `docs/evidence/DoD-30_job_시작_writing_마커.md`
가 limitations 절에서 "`WRITING` 마커만 있는 디렉터리는 기존
`gc_partial()` 규칙상 PARTIAL 로 취급돼 GC 대상이다(이 조각이
새로 만든 규칙이 아니라 기존 규칙을 재사용한다)"고 주장했지만,
이 주장 자체를 직접 검증하는 테스트는 없었다 — `DoD-30` 의 Agent
selftest 는 마커가 생성되는 것만 확인했지, 그 상태의 디렉터리가
실제로 GC 되는지는 확인하지 않았다.

## 구현 (코덱스, `p174`)

- `crates/checkpoint/src/writer.rs` 의 `startup_gc()`(407행)와
  `crates/checkpoint/src/atomic.rs` 의 `gc_partial()`(471행, 판정식
  589행 `!manifest_exists || (is_tmp && !is_registered)`)을 먼저
  읽어 Agent 의 실제 디렉터리 레이아웃(`crates/agent/src/lib.rs:582-602`,
  `.durability.writing` 마커만 생성)과 일치함을 확인.
- `crates/checkpoint/tests/durability_chaos.rs:179` 에 새 테스트
  `startup_gc_removes_marker_only_checkpoint_but_preserves_manifest_checkpoint`
  신설 — marker-only 디렉터리가 GC 후 삭제되는지, 완결된 디렉터리
  (manifest+데이터 파일)는 보존되는지 실제 파일시스템 상태로 확인.

## 독립 검수(`p175`) — **1라운드 만에 ACCEPTED**

`gc_partial()` 의 판정식·`startup_gc()` 의 호출 관계·Agent 의
실제 마커 생성 코드와 마커명 일치 여부·새 테스트의 assert 가
실제 파일시스템 상태(`exists()`/`is_dir()`/`is_file()`)를 확인
하는지·뮤테이션 논리 타당성·범위(GC 알고리즘·Agent 무변경, 단일
파일만 변경)까지 전부 코드로 확인했다. `cargo test` 는 환경에
`cargo` 가 없어 실행 못 했으나 판정 근거로 쓰지 않았다.

감독자(claude-code)가 검수 완료 후 `cargo build`·`cargo test -p
gputeer-checkpoint --test durability_chaos`(17개 전부 통과, 신규
테스트 포함)·워크스페이스 전체 테스트로 독립 재확인했다.

## 결과

```text
cargo build --workspace --exclude gputeer-runtime-windows        성공
cargo test -p gputeer-checkpoint --test durability_chaos          17개 전부 통과
cargo test --workspace --exclude gputeer-runtime-windows           성공, 실패 0건
```

## 이 실험이 증명하지 "않는" 것

- 다중 Agent 동시 GC 경쟁은 다루지 않는다 — `DoD-21` 이 이미 범위
  밖으로 남긴 것과 같은 경계.
- GC 가 실제로 언제 트리거되는지(주기·Coordinator 요청 등)는
  범위 밖 — `startup_gc()` 직접 호출 경로만 검증했다.

## 결정

1. `DoD-30` 이 문서로만 주장했던 안전 불변식을 실제 회귀 테스트로
   고정했다 — GC 알고리즘 자체는 바꾸지 않았다.
2. 독립 검수 1라운드 만에 `ACCEPTED`, 감독자가 직접 재확인했다.

관련: `docs/evidence/DoD-30_job_시작_writing_마커.md`(이 조각이
검증하는 주장의 출처) · `docs/evidence/DoD-21_write_once_동시_호출_계약.md`
(같은 GC 인프라를 다룬 선행 조각)
