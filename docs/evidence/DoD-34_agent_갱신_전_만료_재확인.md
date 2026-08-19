---
schema_version: 2
id: DoD-34
claim: "Agent(crates/agent/src/lib.rs)가 갱신 루프에서 지금까지 revoked 여부만 확인하고 만료는 재확인 안 해 이미 만료된 Lease로 RenewLeaseRequest를 계속 보낼 수 있던 공백(DoD-32가 Coordinator 쪽에서 이미 raw error로 방어해 Lease 부활 결함은 아니었으나 Agent 쪽 낭비·관측 공백)을 닫았다. Agent가 RenewLeaseRequest 를 만들어 보내기 직전에 새 lease_is_expired() 헬퍼(DoD-26/DoD-32 와 동일한 <= 경계 규칙, 최초 Grant 검증·revoke 경로와 일관)로 보유 Lease의 만료를 재확인하고, 만료됐으면 요청 자체를 보내지 않은 채 RENEW_REFUSED:LOCAL_EXPIRED 로 종료한다. Coordinator 코드는 전혀 바뀌지 않았다 — read_frame() 이 연결 EOF를 즉시 Truncated 오류로 전파하고 살아있는 연결에서 프레임이 없을 때만 기존 10초 read timeout이 적용되므로, Agent가 요청을 보내지 않아도 Coordinator가 무한 대기에 빠지지 않는다(오늘 밤 세 번 나온 것과 반대 방향의 교착을 만들지 않는다는 것을 독립 검수와 감독자가 각각 확인했다)"
status: PASS
commit: 1b5513c

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현) / claude-code (cargo build·test·coordinator-agent-selftest 5회 연속 독립 재실행 — 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T07:26:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "★ 최우선 — 오늘 밤 세 번(DoD-22·DoD-23·DoD-27) 나온 교착 패턴의 반대 방향(Agent가 요청을 안 보내 Coordinator가 무한 대기)이 재현되는지: Coordinator의 read_frame() 호출부(coordinator/lib.rs:343-351)가 TCP EOF를 framed_ingress.rs:277-282 의 Truncated 오류로 즉시 받는지, 살아있는 연결의 read timeout(coordinator/lib.rs:202-208, 10초)과 별개인지, Agent가 만료 확인 후에만 RenewLeaseRequest 를 생성하는지(agent/lib.rs:270-277) 코드 순서 추적. 신규 selftest 시나리오 48(coordinator_agent_selftest.rs:2656-2682, TTL 500ms+지연 1000ms)이 이 상황을 실제로 재현하는지, 90초 하드 타임아웃이 형식적 검사가 아니라 실제 kill()/wait() 를 수행하는지(coordinator_agent_selftest.rs:25-64) 확인. lease_is_expired() 의 <= 경계가 revoke 경로·최초 Grant 검증(signing.rs:763-775, now >= expires_at)과 일관되는지, 뮤테이션 주장(검사 무력화 시 Coordinator의 DoD-32 방어가 대신 거부)의 타당성, 범위 확인(git diff --stat 정확히 agent/src/lib.rs·coordinator_agent_selftest.rs 2개 파일, Coordinator/proto 무변경). 1라운드(p178) 만에 ACCEPTED — 교착 위험 없음 확인, 수정 요청 없음. 감독자(claude-code)가 cargo build/test·coordinator-agent-selftest 5회 연속(전부 exit=0, 48개 시나리오, 약 16초/회, 90초 타임아웃 근처 근접 없음)으로 독립 재확인"
review_artifact: "docs/evidence/_raw/DoD-34_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-34_agent_갱신_전_만료_재확인_2026-08-20.txt"
raw_output_digest: "sha256:08055ce1e2dd84d24d10c204ea4c3268a4e985b1750ad0a2e4cd8504cf433a98"
raw_output_bytes: 2950

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — Agent 내부 로직(요청 생성 전 자기 검사)만 추가"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(coordinator-agent-selftest 기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 코덱스 구현 시 5회 + 감독자 재검증 5회, 각 90초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-34_agent_갱신_전_만료_재확인_2026-08-20.txt,
   docs/evidence/_raw/DoD-34_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공, 실패 0건
  coordinator-agent-selftest(코덱스 구현 시 5회): 5회 연속 exit=0, 48개 시나리오, 약 15.8초/회
  coordinator-agent-selftest(감독자 재검증 5회): 5회 연속 exit=0, 매회 48개 시나리오, 약 15.7초/회
artifacts:
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-34_agent_갱신_전_만료_재확인_2026-08-20.txt
  - docs/evidence/_raw/DoD-34_review.txt
negative_tests:
  - "selftest 시나리오 48 — 짧은 TTL(500ms)로 Lease 를 실제 만료시킨 뒤 지연된 갱신 루프 진입(--renew-delay-ms 1000)에서 Agent 가 RenewLeaseRequest 를 아예 보내지 않고 RENEW_REFUSED:LOCAL_EXPIRED 로 종료함을 확인, Coordinator 는 RENEW_RESULT 없이 EOF 오류로 정상 종료함을 확인 — 90초 하드 타임아웃 안에 완료(실측 시나리오 자체는 약 1.1~1.2초)"
  - "뮤테이션(코덱스 자체 보고, p177) — 만료 검사를 임시로 항상 거짓으로 만들면 Agent 가 실제 RenewLeaseRequest 를 전송하고, Coordinator 의 DoD-32 방어(lease store expired during renewal raw error)가 대신 거부함을 확인, selftest exit=1 로 실패 재현. 원복 후 재검증 통과"
limitations:
  - "이번 조각은 Agent 쪽 방어만 닫았다 — Coordinator 쪽(DoD-32)이 이미 최종 방어선이므로, 이 조각이 없어도 Lease 부활 결함은 없었다. 이번 조각은 순수하게 '쓸데없는 왕복을 줄이는' 최적화·관측성 개선이다"
  - "새 signed outcome 이나 Agent→Coordinator 통지는 없다 — Agent 는 그냥 조용히 요청을 안 보내고 자기 프로세스만 종료한다. Coordinator 로그에는 'RenewLeaseRequest 를 못 받았다'는 사실만 EOF 형태로 남지, '왜 안 왔는지'(Agent 가 스스로 만료를 감지했는지, 다른 이유로 끊겼는지)는 구분되지 않는다"
  - "이 검사는 Agent 프로세스 자신의 시계에 의존한다 — Coordinator 와 시계가 어긋나 있으면(예: Agent 시계가 느리면) Agent 는 아직 유효하다고 판단해 요청을 보내는데 Coordinator 는 이미 만료로 거부할 수 있다(반대 방향도 마찬가지) — 이건 DoD-32 가 이미 Coordinator 쪽 fail-closed 로 커버하므로 안전하지만, 이번 조각의 최적화 효과가 시계 어긋남 상황에서는 줄어든다"
decision: "Agent 가 이미 만료된 Lease 로 쓸데없이 갱신 요청을 보내는 낭비·관측 공백을 닫았다 — DoD-26/DoD-32 와 동일한 <= 경계 규칙을 Agent 쪽에도 적용했다. 오늘 밤 세 번 나온 것과 반대 방향의 교착 위험(Agent 가 요청을 안 보내 Coordinator 가 무한 대기)을 구현자·독립 검수·감독자 세 단계 모두 명시적으로 확인해 배제했다. 구현을 코덱스 CLI(workspace-write)에 위임했고, 독립 검수가 1라운드 만에 ACCEPTED. 이로써 오늘 밤 백로그 재조사가 찾은 마지막 후보까지 마쳤다 — 남은 항목은 트리거가 없거나 하루 규모를 넘는 것들뿐이다."
---

# DoD-34 · Agent 쪽 갱신 직전 만료 재확인

## 무엇을 입증하려 했는가

마지막 백로그 재조사(`p176`)가 확인했다 — Agent 는 최초 Lease
검증 때는 만료를 확인하지만, 갱신 루프에서는 `revoked` 여부만
확인한 뒤 이미 만료된 Lease 로 갱신 요청을 만들 수 있었다.
`DoD-32` 가 Coordinator 쪽에서 이미 이런 요청을 raw error 로
거부하도록 막아뒀으므로 Lease 부활 결함은 아니었다 — 이건 Agent
쪽의 "쓸데없이 요청을 보내고 거부당하는" 방어/관측 공백이었다.

## 구현 (코덱스, `p177`)

- Agent 의 갱신 루프에서 `RenewLeaseRequest` 를 만들기 **직전**에
  새 `lease_is_expired()` 헬퍼(`DoD-26`/`DoD-32` 와 동일한 `<=`
  경계 규칙)로 보유 Lease 의 만료를 재확인.
- 만료됐으면 요청을 만들지도 보내지도 않고
  `RENEW_REFUSED:LOCAL_EXPIRED` 로 즉시 종료.
- 같은 헬퍼를 기존 revoke 검사 경로에도 적용해 로직을 통일.
- 신규 selftest 시나리오 48(짧은 TTL 로 실제 만료시킨 뒤 이
  상황을 재현, 90초 하드 타임아웃).
- Coordinator 코드는 전혀 안 바꿨다.

## ★ 교착 위험 검증 — 3단계 모두 명시적으로 확인

오늘 밤 이미 세 번(`DoD-22`·`DoD-23`·`DoD-27`) 나온 교착 버그의
반대 방향("Agent 가 요청을 안 보내면 Coordinator 가 기다린다")이
재현되는지가 이번 조각의 최우선 검증 대상이었다.

1. **구현자(`p177`)**: `read_frame()` 이 TCP EOF 를 즉시
   `Truncated` 오류로 반환하고, 살아있는 연결에서만 기존 10초
   read timeout 이 적용됨을 코드로 확인.
2. **독립 검수(`p178`)**: `coordinator/lib.rs:202-208`(10초
   read timeout 설정)·`:343-351`(다음 요청을 `read_frame()` 으로
   읽는 지점)·`crypto/framed_ingress.rs:277-282`(EOF →`Truncated`
   변환)를 직접 추적해 무한 대기 경로가 없음을 확인. 신규
   시나리오 48 의 90초 하드 타임아웃이 형식적이지 않고 실제
   `kill()`/`wait()` 를 수행함(`coordinator_agent_selftest.rs:25-64`)
   도 확인. **1라운드 만에 `ACCEPTED`**.
3. **감독자(claude-code)**: `coordinator-agent-selftest` 를 5회
   연속 실행해 전부 exit=0, 매회 약 16초로 완료(90초 하드
   타임아웃 근처에도 못 갔다) — 교착이 실제로 없음을 실측으로
   재확인.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   성공, 실패 0건
coordinator-agent-selftest(코덱스 구현 시 5회)                     5회 연속 exit=0, 48개 시나리오, 약 15.8초/회
coordinator-agent-selftest(감독자 재검증 5회)                       5회 연속 exit=0, 매회 48개 시나리오, 약 15.7초/회
```

## 이 실험이 증명하지 "않는" 것

- Coordinator 쪽 최종 방어선(`DoD-32`)이 없어도 이 조각만으로
  안전하다는 게 아니다 — 이건 순수 최적화/관측성 조각이다.
- Agent 와 Coordinator 시계가 크게 어긋난 상황에서의 정확한
  동작(둘 중 하나만 만료로 판단하는 경우)은 별도로 다루지 않는다
  — `DoD-32` 의 Coordinator fail-closed 가 안전망으로 남는다.

## 결정

1. Agent 쪽 낭비/관측 공백을 닫았다 — `DoD-26`/`DoD-32` 와 동일한
   경계 규칙 재사용.
2. 오늘 밤 세 번 나온 것과 반대 방향의 교착 위험을 구현자·독립
   검수·감독자 세 단계 모두 명시적으로 확인해 배제했다.
3. 이로써 오늘 밤 백로그 재조사(`p167`·`p176`)가 찾은 모든 하루
   규모 후보를 마쳤다.

관련: `docs/evidence/DoD-32_만료_lease_갱신_fail_closed.md`(이
조각이 보완하는 Coordinator 쪽 방어) · `docs/evidence/DoD-26_만료_lease_재접속_거부.md`
(동일 경계 규칙의 최초 출처)
