---
schema_version: 2
id: DoD-23
claim: "Coordinator 가 RenewLeaseRequest.fence_epoch 이 영속 저장소의 저장된 값보다 낮게 도착했을 때, 연결을 raw error 로 끊는 대신 서명된 RenewLeaseResult{outcome: RENEW_OUTCOME_SUPERSEDED}로 응답하는 실제 정책을 구현했다(proto 주석이 이미 '정상적인 failover 경합'이라고 선언한 상황을 이제 정말 그렇게 처리한다). lease_store 가 없는 레거시 경로와 요청 epoch 이 더 높은 경우는 기존 hard error 를 유지한다. QUARANTINED 는 위험도/신뢰도 판정 인프라가 아직 없어 실제 트리거는 구현하지 않고 TODO_VISION V-11 로 등록했다"
status: PASS
commit: 0cdd263

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현·수정) / claude-code (cargo build/test 독립 재확인, 뮤테이션 재현)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-19T17:25:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 매 라운드 이전 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "SUPERSEDED 판정이 발생하는 지점 이후 Coordinator 의 갱신 회차 루프 제어 흐름 전체, Agent 가 outcome 2/3/6 을 받았을 때의 실제 처리, selftest 시나리오 25·26 이 우연히 이 경로의 위험 조합(다회차)을 피해가고 있는지, TODO_VISION 신규 항목의 등록 규칙 충족 여부. 2라운드 진행 — 1라운드(p139) CHANGES_REQUESTED(핵심 결함: SUPERSEDED 응답 후 Coordinator 가 `continue` 로 갱신 루프를 계속 도는데 Agent 는 outcome=2 를 받으면 즉시 Err 로 종료해 다음 요청을 보내지 않는다 — renew_rounds>1 이고 SUPERSEDED 가 마지막이 아닌 회차에서 발생하면 Coordinator 가 오지 않을 프레임을 기다린다. selftest 시나리오 25·26 이 우연히 renew_rounds=1(기본값)만 써서 이 조합을 피해가 안 드러났었다) -> `continue` 를 `break` 로 수정, 부수적으로 Agent 가 즉시 종료하는 다른 outcome(QUARANTINED=3, MAX_DURATION_EXCEEDED=6)도 같은 위험이 있음을 확인해 공통으로 처리하도록 일반화, renew_rounds=2 로 이 정확한 조합을 재현하는 시나리오 32 신설 -> 2라운드(p141) ACCEPTED, 부수적으로 QUARANTINED/MAX_DURATION_EXCEEDED 도 이전 조각들부터 있었을 잠재적 위험이 이번 공통 분기로 함께 닫혔음을 확인"
review_artifact: "docs/evidence/_raw/DoD-23_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-23_lease_재발급_정책_superseded_2026-08-19.txt"
raw_output_digest: "sha256:33dc033ea4d980c461ae55f8df15f94f2163e5b896055c317b7df282d8114cb5"
raw_output_bytes: 921

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "RenewOutcome/RenewLeaseResult 스키마는 바꾸지 않았다 — 이미 있던 SUPERSEDED/QUARANTINED enum 값을 실제로 판정하는 로직만 추가했다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(coordinator-agent-selftest 기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 3세트(1차+수정후+최종), 각 5회, 15~30초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-23_lease_재발급_정책_superseded_2026-08-19.txt 전문 참조)

  cargo test --workspace --exclude gputeer-runtime-windows: 42개 스위트 전부 test result: ok, FAILED/error[ 검색 결과 없음
  coordinator-agent-selftest: 3세트(1차 5회 31개 시나리오, 수정 후 5회 32개 시나리오, 최종 재확인 5회 32개 시나리오) 전부 exit=0
  뮤테이션 재현(claude-code 독립 실행) — break 를 continue 로 되돌리면 시나리오 32 에서 정확히 exit=1(스트림 끊김), 원복 후 재통과
artifacts:
  - docs/plans/2026-08-19_1725_lease_재발급_정책_superseded_v1.md
  - docs/reports/2026-08-19_1725_lease_재발급_정책_superseded.md
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/vision/TODO_VISION.md
  - docs/evidence/_raw/DoD-23_lease_재발급_정책_superseded_2026-08-19.txt
  - docs/evidence/_raw/DoD-23_review.txt
negative_tests:
  - "selftest 시나리오 25 — 영속 저장소(lease_store=Some)의 stored epoch(5)보다 낮은 renew 요청(4)이 연결 종료 없이 signed SUPERSEDED 로 응답되고, Agent 가 이를 정상 정책 거부(RENEW_REFUSED:SUPERSEDED)로 분류하는지 확인"
  - "selftest 시나리오 26 — 저장된 epoch 와 같은 epoch 의 기본 renew 는 여전히 signed RENEWED 인지 확인(대조군, 오탐 없음)"
  - "selftest 시나리오 32(2라운드에서 신설) — renew_rounds=2, SUPERSEDED 가 첫 회차에서 발생하는 정확히 위험했던 조합. 교착·오류 없이 정상 종료하고 이후 회차의 RENEW_RESULT 가 없는지 확인"
  - "뮤테이션 1(코덱스 자체 보고) — SUPERSEDED 전용 break 를 무력화하면 다회차 시나리오가 실패, 원복 후 재통과"
  - "뮤테이션 2(claude-code 독립 재현, 코덱스 자체 보고와 별개) — 같은 지점을 다시 되돌려 exit=1(정확한 오류 메시지까지 일치) 확인, 원복 후 재통과"
limitations:
  - "QUARANTINED(RENEW_OUTCOME_QUARANTINED)의 실제 트리거 조건은 구현하지 않았다 — 이 저장소에 기기 위험도/신뢰도를 판정하는 인프라가 아직 없다(다중 Agent 도 미착수). docs/vision/TODO_VISION.md 의 V-11 로 등록했다(관측 가능한 트리거·이유·비용·폐기조건 갖춤, 2라운드 검수가 등록 규칙 충족을 확인했다)"
  - "요청 epoch 이 저장된 값보다 **높은** 경우의 정책은 다루지 않았다 — 기존 hard error 를 유지한다. 새 lease_id 재발급 정책(SUPERSEDED 이후 실제로 어떻게 새 Lease 를 받는지)도 범위 밖이다"
  - "lease_store 가 None(레거시 경로)이면 이 판정 자체를 하지 않는다 — 재시작을 넘는 epoch 확인이 불가능한 경로이기 때문이다(max_total_duration_seconds 판정과 같은 원칙)"
  - "부수적으로 발견한 QUARANTINED(override 로만 도달 가능)·MAX_DURATION_EXCEEDED 의 다회차 교착 위험은 공통 break 분기로 함께 닫혔지만, 이 두 outcome 각각을 다회차로 직접 재현하는 전용 selftest 시나리오는 추가하지 않았다 — 2라운드 검수가 공통 제어 흐름으로 충분히 커버됨을 확인하고 필수 사항으로 보지 않았다"
  - "구현자와 독립 검수자가 이번에도 같은 도구(codex CLI)의 서로 다른 프로세스 인스턴스였다 — 컨텍스트는 독립이지만 같은 모델 계열이 자기 코드를 리뷰한다는 근본적 한계는 남는다. claude-code(이 세션)가 매 라운드 사이에 독립적으로 빌드·테스트·뮤테이션 재현을 직접 실행해 검증했다"
decision: "Coordinator 의 실제 Lease 재발급 정책 중 SUPERSEDED 부분을 구현했다 — 사용자 요청에 따라 구현 자체를 코덱스 CLI(workspace-write)에 위임하고, claude-code 는 독립 재검증과 독립 검수(대화 기록 없는 새 코덱스 인스턴스) 감독 역할을 맡았다. 독립 검수 1라운드가 오늘 두 번째로(Lease revoke 조각에 이어) 같은 부류의 진짜 교착 결함을 찾아냈다 — 정책 거부 응답 후 Coordinator 와 Agent 의 루프 종료 시점이 맞물리지 않는 문제. 고치는 과정에서 QUARANTINED·MAX_DURATION_EXCEEDED 에도 같은 잠재적 위험이 이전 조각들부터 있었다는 것까지 부수적으로 확인해 함께 닫았다. claude-code 가 코덱스의 자체 뮤테이션 보고에 의존하지 않고 직접 재현해 결함의 실재를 재확인했다. 2라운드에서 ACCEPTED. QUARANTINED 의 진짜 트리거는 인프라 부재로 TODO_VISION 에 등록만 하고 미룬다."
---

# DoD-23 · Coordinator 실제 Lease 재발급 정책 (SUPERSEDED)

## 무엇을 입증하려 했는가

`proto/lease.proto` 의 `RENEW_OUTCOME_SUPERSEDED` 는 "더 높은
fence_epoch 의 lease 가 이미 존재한다. 이 노드는 stale이다. 정상적인
failover 경합이므로 risk signal 을 발화하지 않는다"고 이미 선언하고
있었지만, `crates/coordinator/src/lib.rs` 는 `renew_req.fence_epoch
!= expected_epoch` 인 모든 경우를 **어떤 방향이든** raw string `Err`
로 연결을 끊었다 — proto 가 "정상" 이라고 부르는 상황을 코드는
"오류" 로 다뤘다. `CLAUDE.md` "다음에 할 일" 의 "Coordinator 의 실제
Lease 재발급 정책(SUPERSEDED/QUARANTINED 를 언제 내릴지)" 미착수
항목을 그 첫 걸음(SUPERSEDED)만큼 좁혀서 구현했다.

## 진행 방식 — 이번에도 구현을 코덱스에 위임

`write_once` 동시 호출 계약(`DoD-21`)·Lease revoke(`DoD-22`)에 이어
세 번째로, 사용자 요청("코덱스 쿼터 태워서 계속 진행")에 따라
구현·계획·테스트·뮤테이션 검증을 workspace-write 코덱스 인스턴스에
맡기고, claude-code(이 세션)는 독립 재검증과 독립 검수(구현한
인스턴스와 대화 기록을 공유하지 않는 새 코덱스 프로세스, read-only)
감독으로 역할을 좁혔다.

## 구현 (코덱스, `p138`)

- `lease_store.is_some()` 이고 요청 epoch 이 저장된 값보다 **낮으면**
  연결을 끊는 대신 `build_signed_policy_renew_result` 로 서명된
  `RenewLeaseResult{ outcome: SUPERSEDED, lease: None }` 를 만들어
  정상 응답한다.
- `lease_store` 가 `None`(레거시)이거나 요청 epoch 이 더 **높으면**
  기존 hard error 를 유지한다.
- selftest 시나리오 25(정상 SUPERSEDED 왕복)·26(대조군, 같은 epoch
  는 여전히 RENEWED).
- QUARANTINED 는 실제 트리거 인프라가 없어 구현하지 않고
  `docs/vision/TODO_VISION.md` 에 V-11 로 등록.

claude-code 가 즉시 독립적으로 재빌드·재테스트하고
`coordinator-agent-selftest` 를 5회 반복 실행해(31개 시나리오, 전부
exit=0) 1차 검증했다.

## 독립 검수 1라운드(`p139`) — CHANGES_REQUESTED

이 조각의 코드를 읽던 중 claude-code 자신도 의심스러운 지점을
먼저 발견했다(SUPERSEDED 응답 후 `continue` — 오늘 이미 한 번
Lease revoke 조각에서 정확히 같은 부류의 결함이 나왔었다) — 미리
알리지 않고 블라인드로 독립 검수를 돌려 교차 확인했다.

독립 검수가 정확히 같은 지점을 지적했다: Coordinator 는 SUPERSEDED
응답 후 `continue` 로 갱신 루프를 계속 도는데, Agent 는 outcome=2
를 받으면 즉시 `Err`(`RENEW_REFUSED:SUPERSEDED`)로 함수를 끝내
다음 요청을 만들지 않는다 — `renew_rounds > 1` 이고 SUPERSEDED 가
마지막이 아닌 회차에서 발생하면 Coordinator 가 오지 않을 프레임을
기다린다. 기존 시나리오 25·26 은 `--renew-rounds` 를 안 줘서 양쪽
다 기본값 1 이라 이 조합을 우연히 피해가 안 드러났었다.

## 수정(코덱스, `p140`)

- `continue` 를 `break` 로 수정.
- Agent 코드를 직접 확인해 outcome 3(QUARANTINED)·6(MAX_DURATION_EXCEEDED)
  도 똑같이 즉시 종료한다는 걸 확인하고, 정상 갱신 결과 경로에
  `if matches!(result.outcome, 2 | 3 | 6) { break; }` 를 추가해
  세 outcome 전부를 공통으로 처리하도록 일반화했다 — 이는 오늘
  새로 만든 결함이 아니라 **이전 조각들(DoD-13·DoD-18 등)부터
  잠재해 있었을 수 있는 위험**을 부수적으로 닫은 것이다.
- `renew_rounds=2` 로 SUPERSEDED 가 첫 회차에서 발생하는 정확한
  위험 조합을 재현하는 시나리오 32 신설.

**claude-code 가 코덱스의 자체 뮤테이션 보고와 별개로 직접
재현했다** — `break` 를 `continue` 로 되돌려 재빌드한 뒤 실행하니
정확히 시나리오 32 에서 `exit=1` 로 실패했다(코디네이터가 "프레임이
완결되기 전에 스트림이 끊겼다" 오류를 내고, Agent 는 이미
`RENEW_REFUSED:SUPERSEDED` 로 종료해 있었다). 원복 후 재빌드, 5회
연속 재통과 확인.

## 독립 검수 2라운드(`p141`) — **ACCEPTED**

새 공통 `break` 분기가 기존 SUPERSEDED 전용 분기·`revoke_after_round`
분기와 충돌 없이 맞물리는지, QUARANTINED/MAX_DURATION_EXCEEDED 도
같은 위험이 이전부터 있었는지(있었고, 이번 공통 분기가 함께 덮는지)
확인했다. 각 outcome 별 전용 다회차 시나리오는 공통 제어 흐름으로
충분히 커버되므로 필수는 아니라고 판단했다.

## 결과

```text
cargo build --workspace --exclude gputeer-runtime-windows      성공
cargo test --workspace --exclude gputeer-runtime-windows       42개 스위트 전부 통과, 0 failed
coordinator-agent-selftest   1차 5회(31개) + 수정 후 5회(32개) + 최종 5회(32개), 전부 exit=0
```

### 뮤테이션 테스트 2건

| # | 무력화한 것 | 실행자 | 예측대로 실패한 시나리오 |
|---|---|---|---|
| 1 | SUPERSEDED 전용 `break` | 코덱스(자체 보고) | 32 |
| 2 | 같은 지점 | **claude-code(독립 재현)** | 32(동일 오류 메시지까지 일치) |

## 이 실험이 증명하지 "않는" 것

- QUARANTINED 의 실제 트리거(위험도/신뢰도 판정) — 인프라 부재로
  `TODO_VISION` V-11 로만 등록.
- 요청 epoch 이 더 높은 경우의 정책, 새 lease_id 재발급.
- `lease_store=None`(레거시) 경로의 SUPERSEDED 판정.
- 구현자와 검수자는 서로 다른 두 AI 가 아니라 같은 도구의 서로 다른
  프로세스 인스턴스였다.

## 결정

1. Coordinator 의 SUPERSEDED 정책을 실제로 구현했다 — 구현 자체는
   코덱스 CLI(workspace-write)에 위임했다.
2. 독립 검수 1라운드가 오늘 두 번째(Lease revoke 에 이어)로 정책
   거부 후 Coordinator/Agent 루프 종료 시점이 맞물리지 않는 진짜
   교착 결함을 찾아냈고, 수정 과정에서 이전 조각들부터 있었을
   같은 부류의 위험(QUARANTINED·MAX_DURATION_EXCEEDED)까지 부수적
   으로 닫았다. claude-code 가 독립적으로 재현해 재확인했다.
3. 2라운드에서 `ACCEPTED`.

관련: `docs/evidence/DoD-22_lease_revoke_최소_조각.md`(오늘 처음
같은 부류의 교착 결함이 나왔던 조각) ·
`docs/plans/2026-08-19_1725_lease_재발급_정책_superseded_v1.md`
