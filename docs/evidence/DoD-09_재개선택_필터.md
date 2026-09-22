---
schema_version: 2
id: DoD-09
claim: "find_resume_point_for 는 job_id·attempt_id 가 다른 체크포인트, files 가 빈 매니페스트, checkpoint_id 가 디렉터리명과 다른 매니페스트를 재개 후보에서 제외한다."
status: PASS
commit: 7ae2b0b06e19c25a6e8a9672ea1c54ea670a8828

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-opus-5"
executed_at: "2026-08-17T11:05:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "writer.rs 재개 선택 · startup_gc · write_checkpoint 실패 경로 · DurabilityState 연결"
review_artifact: "docs/evidence/_raw/DoD-09_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-09_resume_selection.txt"
raw_output_digest: "sha256:d9acfb59d5ad4587d4f6311e49ab43b234b276ad8850325a4df62367f5703cee"
raw_output_bytes: 5138

binary_digests:
  gputeer-checkpoint: "cargo test 프로필 (unoptimized + debuginfo) — 배포 바이너리 아님"
protocol_versions:
  schema_version: 1
  canonical_rules: "a~j (i-2 · i-3 · c-2 포함)"
platform: "Windows 11 Pro 10.0.26200 · NTFS · Rust 1.97.1"
hardware: "개발 기계 (Intel Iris Xe · GPU 미사용 — 파일시스템 로직만 검증)"
network_profile: "해당 없음 (단일 프로세스 · 로컬 디스크)"
command: "cargo test -p gputeer-checkpoint --test resume_selection && cargo test --workspace"
raw_output: |
  resume_selection  7 passed / 0 failed
  workspace 전체   203 passed / 0 failed

  뮤테이션(M1 job/attempt 필터 제거 · M2 빈 매니페스트 검사 제거 ·
  M3 id/디렉터리명 검사 제거) 상태에서: 3 passed / 4 failed.
  -> 부정 테스트 4건이 결함을 실제로 잡는다.

  ★ 이것은 요약이다. 원문은 raw_output_artifact 에 있고 digest 로 묶여 있다.
artifacts:
  - crates/checkpoint/src/writer.rs
  - crates/checkpoint/tests/resume_selection.rs
  - docs/evidence/_raw/DoD-09_resume_selection.txt
  - docs/evidence/_raw/DoD-09_review.txt
negative_tests:
  - w1_resume_must_not_pick_another_job
  - w1b_resume_must_not_pick_another_attempt
  - w2_empty_manifest_must_not_be_a_resume_candidate
  - w3_manifest_id_must_match_directory_name
  - "뮤테이션 M1·M2·M3 동시 적용 시 위 4건이 모두 FAILED — 공허하지 않다"
limitations:
  - "★ 옛 find_resume_point(필터 없음)는 그대로 남아 있다. 호출부가 그것을 쓰면 결함은 그대로다. 삭제하지 않은 이유는 조용한 깨짐을 피하기 위함이고, unfiltered_api_documents_its_danger 가 그 동작을 고정한다."
  - "★ LATEST 포인터를 여전히 쓰지 않는다. 검수자 지적을 수용했으나 고치지 않았다 — 포인터 갱신 실패 시 잔여물 문제(아래)를 먼저 풀어야 LATEST 를 신뢰할 수 있다."
  - "★ write_checkpoint 가 포인터 갱신에 실패하면 호출자는 Err 를 받지만 디스크에는 완전한 체크포인트가 남는다. job/attempt 필터는 이 위험을 줄이지 않는다 — 같은 job 의 것이기 때문이다. 미수정."
  - "★ startup_gc 는 매니페스트에 등록된 .tmp 파일도 무조건 지운다. 두 프로세스 동시 실행 시 NotFound 경합이 있다. 미수정."
  - "★ DurabilityState 전이는 실제 파일 연산과 연결되어 있지 않다. state_table_parity.rs 는 표와 enum 의 일치만 검사한다. state-machines.md §6 에 이미 선언된 미검사 항목이다."
  - "단일 프로세스 · 단일 디스크에서만 측정했다. 네트워크 파일시스템 · 동시 접근은 측정하지 않았다."
  - "Linux 에서 한 번도 실행하지 않았다 (D-3)."
  - "표본은 각 시나리오 1회다. 이 테스트들은 결정적 로직 검사이므로 반복이 새 정보를 주지 않지만, 경합 조건은 이 방법으로 잡히지 않는다."
decision: "find_resume_point_for 를 재개 경로의 기본 API 로 삼는다. 옛 find_resume_point 는 '한 root 에 한 실행' 이 보장될 때만 쓴다. 미수정 4건은 다음 작업 목록으로 넘긴다."
---

# DoD-09 — 재개 지점 선택의 job/attempt 필터

## 왜 이 실험을 했나

독립 검수(2026-08-17, `agent:codex-cli`)가 `find_resume_point` 에 구체적 반례 4가지를
파일:줄과 함께 제출했다. 추측이 아니라 **읽고 지적한 것**이므로 먼저 재현했다.

```text
검수자 지적                                    재현       조치
──────────────────────────────────────────────────────────────────
job_id/attempt_id 를 거르지 않는다             확인됨     고침
files: [] 매니페스트가 통과한다                확인됨     고침
checkpoint_id != 디렉터리명이 통과한다         확인됨     고침
LATEST 포인터를 쓰지 않는다                    확인됨     ★ 안 고침
startup_gc 가 등록된 .tmp 도 지운다            확인됨     ★ 안 고침
write_checkpoint 실패 후 잔여물이 남는다       확인됨     ★ 안 고침
DurabilityState 가 파일 연산과 무관하다        확인됨     ★ 안 고침
```

## 가장 위험했던 것

**해시 검증을 통과하는데도 남의 체크포인트를 고른다.**

```text
root/
  ckpt-mine   job=A  step=10    <- 내 것
  ckpt-other  job=B  step=100   <- 남의 것. step 이 더 크다
```

옛 구현은 `step` 최대값을 골랐다. 해시는 전부 맞다 —
해시는 *그 파일이 그 매니페스트의 것*임을 보장할 뿐,
*그 매니페스트가 내 것*임은 보장하지 않는다.

재개하면 **완전히 다른 학습 상태를 로드한다.** 오류 없이.

## 이 실험이 증명하지 않는 것

- **옛 API 가 안전해졌다** — 아니다. 그대로 있다. 호출부가 새 API 를 쓰기로 한 것뿐이다.
- **재개 경로 전체가 안전하다** — 아니다. 위 `limitations` 의 미수정 4건이 남아 있다.
- **검수자가 모든 결함을 찾았다** — 알 수 없다. 검수자는 두 파일만 읽었다.
- **동시 접근에서 안전하다** — 측정하지 않았다.

## schema v2 최초 적용

이 evidence 는 `schema_version: 2` 를 쓰는 **첫 번째** 기록이다 (ADR-030).

```text
executor_id != reviewer_id                검사기가 강제한다
review_outcome == ACCEPTED                검사기가 강제한다 (PASS 인 경우)
review_artifact 존재 · artifacts 에 포함   검사기가 강제한다
review_artifact 에 실재하는 파일:줄 인용    검사기가 강제한다
raw_output_digest == 실제 sha256          검사기가 강제한다
raw_output_bytes  == 실제 크기            검사기가 강제한다
```

★ **이것이 보장하지 않는 것**: 명령이 실제로 실행됐는가, 검수자가 정직했는가,
작성자가 원문과 digest 를 **함께** 고쳤는가. 그건 기계가 볼 수 없다.

---

## ★ 이후 변경 (2026-08-17) — 미수정 4건 중 3건이 해결됐다

> 관측 기록은 고치지 않는다. 그러나 `limitations` 가 스테일해지면
> **없는 것보다 나쁜 기록**이 된다.

```text
당시 limitations                          지금 (커밋 ba4fe35)
──────────────────────────────────────────────────────────────────────
write_checkpoint 실패 후 잔여물이 남는다   해결 — .publication-failed 마커로 배제
startup_gc 가 등록된 .tmp 도 지운다        해결 — 매니페스트 등록분은 보존
startup_gc 동시 실행 NotFound 경합         해결 — ★ 단 두 번 걸렸다. 처음엔
                                           writer.rs 루프만 고쳤고 atomic.rs::
                                           gc_partial 내부는 그대로 둬서 8회
                                           반복 중 5회 재발했다. 두 파일이
                                           공유하는 재시도 헬퍼(atomic.rs::
                                           retry_tolerating_race)로 통합한
                                           뒤 8회 연속 통과로 확인했다
                                           (커밋 f1fedc4).
DurabilityState 가 파일 연산과 무관하다    해결 — 상태별 write-once 사이드카로 기록

LATEST 포인터를 쓰지 않는다                ★ 여전히 미수정 (의도적)
```

### `LATEST` 를 여전히 쓰지 않는 이유

포인터를 **완결 신호로 삼지 않기로** 했다.

기록 순서는 `데이터 → 매니페스트 → HASH_VERIFIED → LATEST 교체 → COMMITTED` 다.
포인터나 `COMMITTED` 마커를 완결 신호로 요구하면, 그 직전에 kill 된
**온전한 체크포인트**(데이터·매니페스트·해시 전부 정상)를 버리게 된다.

카오스 테스트가 실제로 이것을 잡았다 — 부하가 높을 때 재개 지점이 통째로 사라졌다.

완결 신호는 **매니페스트의 존재**로 남긴다 (`CLAUDE.md` §0.3 과 일치).
포인터는 힌트이고, 실패한 것은 `.publication-failed` 마커로만 배제한다.

관련: `crates/checkpoint/tests/write_failure.rs` · `crates/checkpoint/src/writer.rs`
