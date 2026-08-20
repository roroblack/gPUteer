---
schema_version: 2
id: DoD-38
claim: "자동 재접속 루프 로드맵 조각 6(Agent Resume 통합)을 완료했다. crates/agent/src/lib.rs 의 run_resume_connection() 은 이미 실제로 AgentSessionHello/ResumeLeaseRequest 를 보내고 ResumeLeaseResult 를 검증하며 재시도 여부(안전 동작)는 이미 올바르게 구현돼 있었다 — REVOKED·EXPIRED·SUPERSEDED·UNKNOWN_LEASE·IDENTITY_CONFLICT·EPOCH_AHEAD 는 전부 재시도 없이 즉시 종료, UNAVAILABLE 만 재시도 가능(RETRYABLE_RESUME). 이번 조각은 그 위에 outcome 별 명시적 오류 문자열(RESUME_REFUSED:REVOKED 등, DoD-27 의 RENEW_REFUSED:REVOKED 패턴을 재사용)을 추가하고, 기존 selftest 시나리오 53~59 가 이 문자열과 CONNECTION_ATTEMPT 1회·ReconnectExhausted 미발생을 실제로 assert 하도록 보강했다 — 재시도 안전 동작 자체는 조금도 안 바뀌었다. Coordinator·proto 는 전혀 안 건드렸다"
status: PASS
commit: c952a5b

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현) / claude-code (cargo build·test·coordinator-agent-selftest 5회 연속 독립 재실행 — 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T14:10:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "★ 최우선 — 재시도 안전 동작이 정말 하나도 안 바뀌었는지: agent/lib.rs:39-47 의 SessionError 분류에서 RETRYABLE_CONNECTION·RETRYABLE_RESUME 만 Retryable 이고 새로 추가된 6개 RESUME_REFUSED:* 는 전부 Fatal 로 남아있음을 코드로 확인(agent/lib.rs:411-416), UNAVAILABLE(outcome 7) 은 여전히 RETRYABLE_RESUME 경로(agent/lib.rs:391-394)를 유지함을 확인. selftest 시나리오 53~59 의 보강이 실제로 outcome 별 기대 문자열·연결 1회·ReconnectExhausted 금지를 assert 하는지(coordinator_agent_selftest.rs:590-613, 호출부 매핑 3135-3216) 확인, 뮤테이션 주장(REVOKED 문자열을 generic 으로 바꾸면 시나리오 56 의 refusal_matches 가 실패) 타당성 확인, 범위(agent/lib.rs·coordinator_agent_selftest.rs 두 파일 + 계획 문서만, Coordinator/proto 무변경) 확인. 1라운드 만에 ACCEPTED — 수정 요청 없음"
review_artifact: "docs/evidence/_raw/DoD-38_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-38_agent_resume_통합_2026-08-20.txt"
raw_output_digest: "sha256:af9fd55d190279e9841d864477424f1e4e29dcf7637d9cb6dd2c45f014194579"
raw_output_bytes: 2978

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — Agent 내부 오류 문자열 구분만 추가"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 5회 연속, 각 120초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-38_agent_resume_통합_2026-08-20.txt,
   docs/evidence/_raw/DoD-38_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공, 실패 0건
  coordinator-agent-selftest: 5회 연속 exit=0, 64개 시나리오, 약 30.5~31.4초/회
artifacts:
  - docs/plans/2026-08-20_1410_agent_resume_통합_v1.md
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-38_agent_resume_통합_2026-08-20.txt
  - docs/evidence/_raw/DoD-38_review.txt
negative_tests:
  - "selftest 시나리오 53~59(보강) — 각 Resume outcome(RESUMED 제외) 발생 시 Agent stdout/stderr 에 정확한 RESUME_REFUSED:<OUTCOME> 문자열이 나타나는지, CONNECTION_ATTEMPT 가 정확히 1회만 발생했는지(재시도 안 함), ReconnectExhausted 가 나타나지 않는지(retryable 로 오분류되지 않았는지) 확인"
  - "뮤테이션(코덱스 자체 보고, p199) — REVOKED 오류 문자열을 임시로 generic 오류로 바꾸면 시나리오 56 의 refusal_matches 검사가 실패, 원복 후 재검증 통과"
limitations:
  - "RESUMED 뒤 정상 renew 루프로 자동 이어붙이는 lifecycle 통합은 하지 않았다 — durable request ledger(로드맵 조각 5)와 얽혀 하루 작업 범위를 넘는다고 판단해 명시적으로 범위 밖으로 뒀다"
  - "Coordinator 쪽은 전혀 안 건드렸다 — Resume 은 one-shot 요청-응답이라 다회차 갱신과 달리 교착 위험이 낮다는 게 이미 확인돼 있었다"
  - "durable request ledger(조각 5)·다중 Agent 경쟁(조각 7)은 여전히 범위 밖이다"
decision: "로드맵 조각 6(Agent Resume 통합)을 완료했다 — 이미 안전하게 동작하던 재시도 로직 위에 outcome 별 명시적 오류 문자열과 그걸 검증하는 selftest 보강만 추가했다. 구현을 코덱스 CLI(workspace-write)에 위임했고, 독립 검수가 재시도 안전 동작 불변을 코드로 직접 추적해 확인하고 1라운드 만에 ACCEPTED. 로드맵 7조각 중 1·2·3·4·6 이 완료됐다 — 남은 5(durable request ledger)·7(다중 Agent selftest, 조각 5 이후 유의미)만 후속 조각으로 남는다."
---

# DoD-38 · Agent Resume 통합

## 무엇을 입증하려 했는가

설계 조사(`p198`, read-only)가 로드맵 조각 6 의 상태를 "부분
완료"로 정직하게 판정했다 — Agent 는 이미 실제로
`AgentSessionHello`/`ResumeLeaseRequest` 를 보내고 재시도 여부
(안전 동작)도 이미 올바르지만, outcome 별 **명시적 구분**(오류
문자열·selftest 검증)이 없었다.

## 구현 (코덱스, `p199`)

- `agent/lib.rs:411` 부근에 `RESUME_REFUSED:REVOKED`·
  `RESUME_REFUSED:EXPIRED`·`RESUME_REFUSED:SUPERSEDED`·
  `RESUME_REFUSED:UNKNOWN_LEASE`·`RESUME_REFUSED:IDENTITY_CONFLICT`·
  `RESUME_REFUSED:EPOCH_AHEAD` 추가(`DoD-27` 의 `RENEW_REFUSED:
  REVOKED` 패턴 재사용). `RESUMED`·`UNAVAILABLE`/
  `RETRYABLE_RESUME` 경로는 안 건드림.
- `coordinator_agent_selftest.rs` 의 기존 시나리오 53~59 를
  보강 — outcome 별 정확한 오류 문자열·`CONNECTION_ATTEMPT` 1회·
  `ReconnectExhausted` 미발생을 실제로 assert.

## 독립 검수(`p200`) — **1라운드 만에 ACCEPTED**

재시도 안전 동작이 조금도 안 바뀌었는지가 최우선 검증 대상이었다
— `SessionError` 분류에서 `RETRYABLE_CONNECTION`·
`RETRYABLE_RESUME` 만 재시도 가능이고 새 6개
`RESUME_REFUSED:*` 는 전부 `Fatal` 로 남아있음을, `UNAVAILABLE`
은 여전히 `RETRYABLE_RESUME` 경로를 유지함을 코드로 확인했다.
selftest 보강이 실제로 assert 하는지, 뮤테이션이 타당한지, 범위
(2개 소스 파일만)까지 전부 확인하고 잔여 지적 없이 `ACCEPTED`.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   성공, 실패 0건
coordinator-agent-selftest                                        5회 연속 exit=0, 64개 시나리오, 약 30.5~31.4초/회
```

## 이 실험이 증명하지 "않는" 것

- `RESUMED` 뒤 정상 renew 루프로 자동 이어지는 lifecycle 통합은
  없다.
- durable request ledger(조각 5)·다중 Agent 경쟁(조각 7)은 여전히
  범위 밖.

## 결정

1. 로드맵 조각 6을 완료했다 — 이미 안전한 재시도 로직에 명시적
   구분만 추가.
2. 독립 검수 1라운드 만에 `ACCEPTED`.
3. **로드맵 7조각 중 1·2·3·4·6 완료** — 남은 5(durable request
   ledger)·7(다중 Agent selftest)만 후속 조각으로 남는다.

관련: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`
(전체 로드맵) · `docs/evidence/DoD-27_revoked_signed_outcome.md`
(재사용한 `RENEW_REFUSED:` 패턴의 출처) · `docs/evidence/DoD-36_resume_프로토콜.md`
(Resume wire 경로의 원본 구현)
