---
schema_version: 2
id: DoD-31
claim: "오늘 밤 세 번(DoD-22·DoD-23·DoD-27) 나온 교착 버그 패턴 — Coordinator 가 다회차 갱신 루프 중 정책 거부 outcome(SUPERSEDED=2·QUARANTINED=3·MAX_DURATION_EXCEEDED=6·REVOKED=8)을 마지막이 아닌 회차에서 보내면 Agent 는 즉시 종료하는데 Coordinator 는 다음 프레임을 계속 기다리는 교착 — 을 막는 crates/coordinator/src/lib.rs:486 의 matches!(result.outcome, 2 | 3 | 6 | 8) { break; } 가드에 대해, SUPERSEDED·REVOKED 만 다회차(renew_rounds>=2) 회귀 시나리오가 있고 QUARANTINED·MAX_DURATION_EXCEEDED 는 단일 회차 시나리오뿐이던 테스트 공백을 닫았다. 새 selftest 시나리오 45(QUARANTINED, renew_rounds=2 의 첫 회차)·46(MAX_DURATION_EXCEEDED, 같은 패턴, 기존 DoD-18 트리거 재사용)을 추가했다 — 프로덕션 코드는 전혀 바꾸지 않은 순수 테스트 커버리지 조각이다. 감독자가 matches! 에서 3·6 을 각각 제거하는 뮤테이션으로 직접 재현해, 두 경우 모두 정확히 예측된 교착(Coordinator 가 오지 않을 RenewLeaseRequest 를 기다리다 스트림이 끊김)으로 실패함을 확인했다"
status: PASS
commit: ac9bd31

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현) / claude-code (cargo build·coordinator-agent-selftest 5회 연속 독립 재실행 + 2건의 뮤테이션 직접 재현·원복 — 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T06:11:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스 2라운드(1라운드는 감독자 자신의 뮤테이션 재현 작업과 시점이 겹쳐 생긴 오탐, 2라운드가 안정 상태에서 최종 판정)"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(p169) — git diff --stat 이 selftest 한 파일이 아니라 coordinator/lib.rs 도 포함한다고 지적하며 CHANGES_REQUESTED. 그러나 이는 검수가 진행되던 바로 그 시간대에 감독자(claude-code)가 코덱스의 뮤테이션 보고를 독립 재확인하려고 matches!(result.outcome, 2|3|6|8) 를 순차적으로(3 제거→재현→원복→6 제거→재현→원복) 뮤테이션하던 중간 상태를 읽은 것이었다 — 신규 테스트 로직 자체(시나리오 45·46 의 assert·outcome 트리거·RENEW_RESULT 개수 제한)는 1라운드도 이미 '요구사항에 부합'으로 확인했다. 감독자가 두 뮤테이션 모두 정확히 예측된 교착(Coordinator가 다음 RenewLeaseRequest 프레임을 기다리다 스트림이 끊김)으로 재현한 뒤 완전히 원복(git diff --stat 무변경)했음을 확인하고, 안정된 현재 상태로 2라운드(p170) 재검수를 요청. 2라운드가 git diff --stat 단일 파일(coordinator_agent_selftest.rs, +150줄)·lib.rs:486 의 matches! 에 2|3|6|8 전부 존재·시나리오 45(:2504, outcome=3, RENEW_RESULT 정확히 1개 assert)·시나리오 46(:2580, 2.2초 대기 후 기존 DoD-18 트리거 재사용, outcome=6)·신규 CLI 플래그 없음·뮤테이션 논리 타당성까지 전부 코드로 확인하고 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-31_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-31_terminal_outcome_다회차_교착_회귀_2026-08-20.txt"
raw_output_digest: "sha256:61d3b7dbebefe9b66933c3724884dc5a19a157edf12a10f40a916c51cace3af2"
raw_output_bytes: 3206

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음, 코드 로직 변경도 없음 — 순수 테스트 커버리지 추가"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(coordinator-agent-selftest 기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 코덱스 구현 시 5회 + 감독자 재검증 5회, 각 90초 하드 타임아웃
  # 뮤테이션: matches!(result.outcome, 2|3|6|8) 에서 3·6 을 각각 제거,
  # 재빌드 후 selftest 1회씩 실행해 정확한 실패 재현 확인, 원복 후 재검증
raw_output: |
  (docs/evidence/_raw/DoD-31_terminal_outcome_다회차_교착_회귀_2026-08-20.txt,
   docs/evidence/_raw/DoD-31_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공, 실패 0건
  coordinator-agent-selftest(코덱스 구현 시 5회): 5회 연속 exit=0, 46개 시나리오
  coordinator-agent-selftest(감독자 재검증 5회, 원복 후): 5회 연속 exit=0,
    매회 46개 시나리오, 약 13초/회
  뮤테이션 1(outcome=3 제거): exit=1, "다회차 QUARANTINED가 첫 회차에서
    정상 종료되지 않았거나 이후 RENEW_RESULT가 발생했다" — 정확히 예측된
    교착(RenewLeaseRequest 프레임 대기 중 스트림 끊김) 재현
  뮤테이션 2(outcome=6 제거): exit=1, 같은 패턴으로 MAX_DURATION_EXCEEDED 재현
artifacts:
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-31_terminal_outcome_다회차_교착_회귀_2026-08-20.txt
  - docs/evidence/_raw/DoD-31_review.txt
negative_tests:
  - "selftest 시나리오 45 — QUARANTINED(--renew-outcome-override 3)가 renew_rounds=2 의 첫 회차에서 발생, RENEW_RESULT 가 정확히 1개만 발생하고 양쪽 프로세스가 교착 없이 정상 종료함을 확인"
  - "selftest 시나리오 46 — MAX_DURATION_EXCEEDED(기존 DoD-18 트리거인 --max-total-duration-seconds 2 + 2.2초 대기)가 renew_rounds=2 의 첫 회차에서 발생, 같은 방식으로 확인"
  - "뮤테이션(감독자 직접 재현, 코덱스 자체 보고와 독립적으로 재확인) — matches!(result.outcome, 2|3|6|8) 에서 3 을 빼면 시나리오 45 가, 6 을 빼면 시나리오 46 이 각각 정확히 예측된 교착으로 실패. 두 경우 모두 원복 후 재검증 통과"
limitations:
  - "SUPERSEDED(2)·REVOKED(8) 의 다회차 회귀 시나리오는 이번 조각 이전(DoD-23·DoD-27)에 이미 있었다 — 이번 조각은 QUARANTINED·MAX_DURATION_EXCEEDED 두 outcome 만 새로 커버한다"
  - "이 회귀 테스트는 현재의 matches!(2|3|6|8) 목록이 올바르다는 것만 증명한다 — 미래에 새 terminal outcome(예: EXPIRED)이 추가되는데 이 목록에 안 넣는 실수는 여전히 이 테스트로 못 잡는다. 그런 실수를 구조적으로 막으려면(예: outcome enum 전체를 순회하며 Agent 즉시 종료 여부와 Coordinator break 여부를 자동 대조) 별도 설계가 필요하다 — 이번 조각 범위 밖"
  - "1라운드 검수(p169)의 CHANGES_REQUESTED 는 실제 코드 결함이 아니라 감독자 자신의 검증 작업과 시점이 겹쳐 생긴 오탐이었다 — 이 사실 자체가 '독립 검수 도중 감독자가 같은 파일을 동시에 조작하면 안 된다'는 프로세스 교훈을 남긴다(이후 조각부터는 순차 진행하기로 함)"
decision: "오늘 밤 세 번 나온 교착 버그 패턴에 대한 회귀 방지망을 QUARANTINED·MAX_DURATION_EXCEEDED 까지 확장했다 — 프로덕션 코드는 바꾸지 않고 순수 테스트만 추가했다. 구현을 코덱스 CLI(workspace-write)에 위임했고, 감독자가 코덱스의 뮤테이션 보고를 독립적으로 두 번 다 재현해 신뢰도를 직접 높였다. 독립 검수 1라운드는 감독자 자신의 검증 작업과 시점이 겹쳐 오탐을 냈으나, 안정된 상태에서의 2라운드가 코드로 전부 확인하고 ACCEPTED."
---

# DoD-31 · terminal outcome 다회차 교착 회귀 테스트

## 무엇을 입증하려 했는가

오늘 밤 사이 같은 종류의 교착 버그가 세 번(`DoD-22`·`DoD-23`·
`DoD-27`) 나왔다 — Coordinator 가 정책 거부 outcome(`SUPERSEDED=2`·
`QUARANTINED=3`·`MAX_DURATION_EXCEEDED=6`·`REVOKED=8`)을 다회차
갱신 루프의 마지막이 아닌 회차에서 보내면, Agent 는 즉시 종료해
다음 요청을 안 보내는데 Coordinator 가 계속 기다리는 교착이었다.
지금은 `if matches!(result.outcome, 2 | 3 | 6 | 8) { break; }`
로 전부 고쳐져 있지만, `SUPERSEDED`·`REVOKED` 만 다회차 회귀
시나리오가 있었고 `QUARANTINED`·`MAX_DURATION_EXCEEDED` 는 단일
회차 시나리오뿐이었다 — 백로그 재조사(`p167`)가 이 공백을 1순위
후보로 꼽았다.

## 구현 (코덱스, `p168`)

- `crates/cli/src/coordinator_agent_selftest.rs` 에 시나리오 45
  (`QUARANTINED`, `renew_rounds=2` 의 첫 회차)·46
  (`MAX_DURATION_EXCEEDED`, 기존 `DoD-18` 트리거 재사용, 같은
  패턴) 신설. 프로덕션 코드는 전혀 바꾸지 않았다.
- 각 시나리오가 `RENEW_RESULT` 개수를 정확히 1개로 제한해, 두
  번째 회차 결과가 없음을 assert.

## 독립 검수 — 2라운드 끝에 `ACCEPTED`

1라운드(`p169`)가 `crates/coordinator/src/lib.rs` 가 HEAD 와
다르다며 `CHANGES_REQUESTED` 를 냈다. 조사 결과, 이건 검수가
진행되던 바로 그 시간대에 감독자(claude-code)가 코덱스의 뮤테이션
보고를 독립 재확인하려고 `matches!` 를 순차적으로(3 제거 → 재현
확인 → 원복 → 6 제거 → 재현 확인 → 원복) 뮤테이션하던 중간
상태를 검수가 읽은 것이었다 — 감독자 자신의 검증 작업과 시점이
겹쳐 생긴 오탐이지 실제 결함이 아니었다. 신규 테스트 로직 자체는
1라운드도 이미 "요구사항에 부합" 으로 확인했었다.

감독자가 두 뮤테이션 모두 정확히 예측된 교착(Coordinator 가 오지
않을 `RenewLeaseRequest` 프레임을 기다리다 "스트림이 끊겼다")으로
재현했고, 완전히 원복(`git diff --stat` 무변경)됐음을 확인한 뒤
안정된 상태로 2라운드(`p170`)를 요청했다 — `git diff --stat` 단일
파일(+150줄)·`matches!` 에 4개 값 전부 존재·시나리오 45·46 의
정확한 assert·신규 CLI 플래그 없음·뮤테이션 논리 타당성까지 전부
코드로 확인하고 `ACCEPTED`.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   성공, 실패 0건
coordinator-agent-selftest(코덱스 구현 시 5회)                     5회 연속 exit=0, 46개 시나리오
coordinator-agent-selftest(감독자 재검증 5회, 원복 후)               5회 연속 exit=0, 매회 약 13초
뮤테이션(outcome=3 제거)                                           exit=1, 예측된 교착 재현
뮤테이션(outcome=6 제거)                                           exit=1, 예측된 교착 재현
```

## 이 실험이 증명하지 "않는" 것

- 미래에 새 terminal outcome 이 추가되는데 `matches!` 목록에
  실수로 안 넣는 것은 이 테스트로 못 잡는다 — 구조적 방지(예:
  enum 전체 순회 자동 대조)는 범위 밖.

## 결정

1. 교착 버그 회귀 방지망을 `QUARANTINED`·`MAX_DURATION_EXCEEDED`
   까지 확장했다 — 프로덕션 코드 변경 없음.
2. 1라운드 검수의 반려는 감독자 자신의 검증 작업과 겹친 오탐이었음을
   규명했고, 안정 상태의 2라운드가 `ACCEPTED`.
3. 프로세스 교훈: 독립 검수가 진행 중일 때 감독자가 같은 파일을
   직접 조작하는 뮤테이션 재현은 순차적으로 진행한다.

관련: `docs/evidence/DoD-22_lease_revoke_최소_조각.md` ·
`docs/evidence/DoD-23_lease_재발급_정책_superseded.md` ·
`docs/evidence/DoD-27_revoked_signed_outcome.md`(같은 교착 패턴이
반복 발견된 조각들)
