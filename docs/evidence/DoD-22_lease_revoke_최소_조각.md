---
schema_version: 2
id: DoD-22
claim: "Coordinator 가 이미 발급한 Lease 를 대상으로 서명된 RevokeLeaseNotice 를 같은 TCP 연결로 보내고, Agent 가 서명·lease_id·fence_epoch·만료 여부를 검증한 뒤 보유 Lease 를 revoked 로 표시해 이후 갱신 요청을 만들지 않는 최소 경로를 구현했다 — RevokeLeaseNotice 자체는 이미 서명 대상 메시지·framed_ingress dispatch 로 존재했지만(DoD-17) Coordinator/Agent 업무 로직은 이번에 처음 생겼다. coordinator-agent-selftest 에 시나리오 25~29(정상 revoke+차단, 위조 서명, 잘못된 lease_id, 잘못된 fence_epoch, 만료된 Lease 에 대한 revoke 거부)를 추가했다"
status: PASS
commit: eeb7a85

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현·수정) / claude-code (cargo build/test 독립 재확인, 뮤테이션 재현)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-19T15:17:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high(1·2라운드)/medium(3라운드) — 매 라운드 이전 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "Coordinator 의 revoke 통지 생성·전송 지점 2곳(회차 0 직후·매 갱신 회차 뒤), Agent 의 서명 검증·lease_id/fence_epoch/만료 대조·revoke 후 갱신 차단 로직, coordinator-agent-selftest 시나리오 25~29 의 판정 조건, V-08(signer_id()==lease_id) 계약이 실제 protocol 코드에 있는지, 재접속/재시작 시 revoke 유실 처리(범위 밖 여부), revoke_after_round=0 조합에서의 교착 가능성, 유닛 테스트가 서명 payload 를 실제로 검증하는지. 3라운드 진행 — 1라운드(p134) CHANGES_REQUESTED(①--revoke-after-round 0 + 양쪽 do_renew=true 조합에서 Coordinator 가 오지 않을 RenewLeaseRequest 를 기다리는 교착 가능성 — 실제 코드 결함, 기존 selftest 시나리오들이 이 정확한 조합을 우연히 피해가서 안 드러났었다 ②서명 payload 를 실제로 검증하지 않는 유닛 테스트 ③selftest 시나리오 25 의 주장이 실제로 증명하는 것보다 강함 ④오래된 시나리오 개수 주석) -> 전부 수정 -> 2라운드(p136) CHANGES_REQUESTED(코드 4건은 전부 타당하게 처리됐으나 계획 문서 개정 이력에 서명 테스트 보강과 주석 갱신 2건이 누락됨 — 문서만) -> 문서 수정 -> 3라운드(p137) ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-22_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-22_lease_revoke_2026-08-19.txt"
raw_output_digest: "sha256:1f2e43239da4abab482267bcd1a51c67a11a927a6735568ce87b9fe2fa4f94eb"
raw_output_bytes: 969

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "RevokeLeaseNotice 자체(스키마·서명 대상 등록)는 바꾸지 않았다 — DoD-17 이 이미 정식화한 것을 그대로 쓴다. V-08(signer_id()==lease_id) 계약도 유지했다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 이 조각은 127.0.0.1 TCP 핸드셰이크 stub 이다"
network_profile: "127.0.0.1 루프백 TCP 만 사용(coordinator-agent-selftest 기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 5회+5회 반복, 각 15~20초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-22_lease_revoke_2026-08-19.txt 전문 참조)

  cargo test --workspace --exclude gputeer-runtime-windows: 42개 스위트 전부 test result: ok, FAILED/error[ 검색 결과 없음
  coordinator-agent-selftest: 1차(구현 직후) 5회 연속 exit=0, 2차(수정 직후, 하드 타임아웃 포함) 5회 연속 exit=0
  뮤테이션 재현(claude-code 독립 실행) — 교착 방지 가드 되돌리면 시나리오 25 에서 exit=1 로 정확히 재현, 원복 후 재통과
artifacts:
  - docs/plans/2026-08-19_1517_lease_revoke_최소_조각_v1.md
  - docs/reports/2026-08-19_1532_lease_revoke_최소_조각.md
  - crates/coordinator/src/lib.rs
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-22_lease_revoke_2026-08-19.txt
  - docs/evidence/_raw/DoD-22_review.txt
negative_tests:
  - "selftest 시나리오 26 — 위조된 RevokeLeaseNotice.coordinator_signature 는 Agent 의 프레임 검증 단계에서 즉시 거부된다"
  - "selftest 시나리오 27 — 통지의 lease_id 가 Agent 보유 Lease 와 다르면 REVOKE_REJECTED: lease_id 불일치 로 거부된다(V-08 제약으로 서명 검증용 alias 키를 별도 등록해, 서명 검증과 identity 검증이 분리돼 있는지까지 확인한다)"
  - "selftest 시나리오 28 — 통지의 fence_epoch 이 Agent 보유 Lease 와 다르면 REVOKE_REJECTED: fence_epoch 불일치 로 거부된다"
  - "selftest 시나리오 29 — Agent 가 보유한 Lease 가 이미 만료된 뒤 도착한 revoke 통지는 REVOKE_REJECTED: held Lease가 이미 만료됐다 로 거부된다(Coordinator 가 lease_ttl_ms=2000·revoke_delay_ms=2200 으로 실제 만료를 만들어낸다)"
  - "selftest 시나리오 25 — --revoke-after-round 0 + Coordinator/Agent 둘 다 --do-renew true + --renew-rounds 2 (독립 검수 1라운드가 지적한 교착 위험 조합 그 자체)로 정상 종료하고, 양쪽 stdout 어디에도 RENEW_RESULT ok=true 가 없음을 확인한다"
  - "뮤테이션 1(코덱스 자체 보고) — Agent 의 lease_id 검사를 임시 무력화하면 시나리오 27 이 예상대로 실패, 원복 후 재통과"
  - "뮤테이션 2(코덱스 자체 보고) — Coordinator 의 revoke 서명 변조 분기를 임시 무력화하면 시나리오 26 이 예상대로 실패, 원복 후 재통과"
  - "뮤테이션 3(claude-code 독립 재현, 코덱스 자체 보고와 별개) — 교착 방지 가드(revoked_after_grant 로 갱신 루프 전체를 건너뛰는 분기)를 되돌리면 시나리오 25 가 정확히 실패(exit=1, 스트림 끊김 오류) — 무한 행은 아니었지만 이 조합이 실질적으로 고장난 상태임을 재확인. 원복 후 5회 연속 재통과"
limitations:
  - "재접속(failover) 시 revoke 전달은 다루지 않는다 — Coordinator 가 revoke 를 보낸 순간 Agent 와의 연결이 끊겨 있으면 그 통지는 유실된다. 재전송·큐잉·영속화는 범위 밖이다"
  - "비동기 이벤트 multiplexing 이 없다 — revoke 통지는 이 stub 프로토콜이 이미 알고 있는 고정된 시점(회차 N 직후)에만 전송된다. 실제 Coordinator 가 임의 시점에 비동기로 revoke 를 보내는 경우는 다루지 않는다"
  - "왜 revoke 하는가(정책)는 범위 밖이다 — 트리거는 CLI 플래그로 시뮬레이션했을 뿐, 진짜 Coordinator 정책 엔진(SUPERSEDED/QUARANTINED 판단 등)은 없다. cause 는 항상 Quarantine 으로 하드코딩했다"
  - "selftest 시나리오 25 는 Agent 코드가 revoke 후 요청을 만들지 않는 분기를 탔다는 것과 양쪽 프로세스가 stdout 에 RENEW_RESULT 를 찍지 않았다는 것만 확인한다 — TCP wire 위에 실제로 추가 프레임이 전혀 오가지 않았음을 패킷 수준에서 직접 관찰하지는 않는다. 코덱스 2라운드 검수가 이 한계를 확인했고, 코드 주석과 계획 문서에 정직하게 적어뒀다"
  - "Job 실제 취소·체크포인트 정리 등 revoke 이후의 상위 작업 처리는 범위 밖이다 — Agent 는 Lease 를 revoked 로 표시하고 갱신만 멈출 뿐, 그 Lease 아래에서 실행 중이던 다른 작업(Job 실행 자체가 아직 미착수이므로 실제로는 없다)을 정리하는 로직은 없다"
  - "다중 Agent·HA·TLS 는 여전히 범위 밖이다"
  - "구현자와 독립 검수자가 이번에는 **같은 도구(codex CLI)의 서로 다른 프로세스 인스턴스**였다 — 이 세션의 다른 조각들처럼 서로 다른 두 종류의 AI(예: Claude 구현 + Codex 검수)가 아니다. 각 인스턴스는 이전 대화 기록을 공유하지 않는 완전히 새 컨텍스트에서 시작했고, claude-code(이 세션)가 매 라운드 사이에 독립적으로 빌드·테스트·뮤테이션 재현을 직접 실행해 코덱스의 자체 보고를 검증했다 — 그러나 '같은 모델 계열이 자기 코드를 리뷰한다'는 근본적 한계 자체는 남아 있다"
decision: "RevokeLeaseNotice 의 업무 로직(Coordinator 발신·Agent 검증/무효화)을 최소 조각으로 구현했다 — 이번 조각은 사용자 요청에 따라 구현 자체를 코덱스 CLI(workspace-write 샌드박스)에게 맡기고, claude-code(이 세션)는 감독·독립 재검증·evidence 기록 역할을 맡는 방식으로 진행했다. 독립 검수(매 라운드 이전 기록이 없는 새 코덱스 프로세스) 1라운드가 실제 교착 결함 1건을 포함해 4건을 찾아냈고 — 특히 --revoke-after-round 0 조합의 교착은 기존 selftest 시나리오들이 우연히 피해가서 드러나지 않았던 진짜 결함이었다 — 전부 코드로 고쳤다. claude-code 가 이 수정과 별개로 직접 뮤테이션을 재현해(교착 방지 가드를 되돌려 정확한 실패를 확인) 코덱스의 자체 보고에 의존하지 않고 독립적으로 결함의 실재를 재확인했다. 2라운드는 문서 완결성만 지적했고, 문서를 보강한 뒤 3라운드에서 ACCEPTED. 재접속 시 revoke 유실·비동기 전송·실제 정책 엔진은 의도적으로 범위 밖으로 남겼다."
---

# DoD-22 · Lease Revoke 최소 조각

## 무엇을 입증하려 했는가

`RevokeLeaseNotice` 는 `DoD-17`(2026-08-19)이 서명 대상 메시지로
정식화하고 `framed_ingress` dispatch 커버리지까지 갖췄지만, 실제
Coordinator/Agent 업무 로직은 없었다(`grep -r RevokeLeaseNotice
crates/coordinator` 결과 0건) — `CLAUDE.md` "다음에 할 일" 미착수
목록의 "Lease revoke" 가 그대로 남아 있었다.

## 이번 조각의 진행 방식 — 구현을 코덱스에 위임

사용자가 "코덱스 cli 에 5.6 솔로 작업" 을 명시적으로 요청했다.
이 세션(claude-code)이 직접 코드를 짜는 대신, **workspace-write**
샌드박스로 코덱스 CLI 인스턴스를 띄워 계획·구현·테스트·뮤테이션
검증까지 스스로 하도록 맡겼다(`p133`). 이 세션의 역할은 감독 —
결과물을 스스로 재빌드·재테스트하고, evidence 기록에 필요한
**독립 검수**(구현한 인스턴스와 대화 기록을 공유하지 않는 새
코덱스 프로세스, read-only)를 별도로 돌리는 것으로 좁혔다 — 이
저장소의 "구현자와 검수자가 달라야 한다"(ADR-030) 원칙을, 구현 자체를
외주 준 상황에서도 지키기 위해서다.

## 구현 (코덱스, `p133`)

- Coordinator: `--revoke-after-round` 트리거로 이미 발급한 Grant 의
  Lease 를 대상으로 서명된 `RevokeLeaseNotice` 를 같은 TCP 연결로
  전송(`send_revoke_notice`/`build_revoke_notice`).
- Agent: 서명 검증 뒤 `lease_id`·`fence_epoch`·만료 상태를 대조하고
  (`receive_and_validate_revoke`/`validate_revoke_notice`), 통과하면
  `revoked = true` 로 표시해 이후 갱신 회차에서 요청 생성 전에
  `RENEW_BLOCKED` 로 멈춘다.
- `coordinator-agent-selftest` 시나리오 25~29(정상 revoke+차단, 위조
  서명, 잘못된 lease_id, 잘못된 fence_epoch, 만료된 Lease revoke 거부).

claude-code 가 즉시 독립적으로 재빌드·재테스트(`cargo build`/`test`
--workspace) 하고 `coordinator-agent-selftest` 를 직접 5회 반복
실행해(29개 시나리오, 전부 exit=0) 코덱스의 자체 보고를 1차
검증했다.

## 독립 검수 1라운드(`p134`) — CHANGES_REQUESTED, 4건

**핵심 결함(#1)**: `--revoke-after-round 0` 이면서 Coordinator 의
`do_renew` 가 true 인 조합에서 진짜 결함이 있었다 — Coordinator 는
revoke 를 보낸 뒤에도 그대로 갱신 루프에 진입해 Agent 의
`RenewLeaseRequest` 를 기다리는데, Agent 는 `revoked == true` 라
갱신 루프 첫 반복에서 요청을 만들지도 않고 `RENEW_BLOCKED` 로
`break` 한다 — Coordinator 가 영원히 오지 않을 프레임을 기다리게
된다. 기존 selftest 시나리오들은 이 정확한 조합(시나리오 25 는
round=1 사용, 26~29 는 round=0 이지만 do_renew=false)을 우연히
피해가서 드러나지 않았을 뿐, 실제 코드 경로에 남아있던 진짜 결함
이었다.

나머지 3건: 서명 payload 를 실제로 검증하지 않는 유닛 테스트,
selftest 시나리오 25 의 주장이 실제 증명 범위보다 강함, 오래된
시나리오 개수 주석.

## 수정(코덱스, `p135`)

- Coordinator 도 `revoke_after_round == Some(0)` 이면 `revoked_after_grant`
  로 표시하고 갱신 루프 전체를 건너뛰도록 고쳐, Agent 와 대칭적으로
  동작하게 했다.
- 시나리오 25 를 정확히 그 위험 조합(round=0, 양쪽 `do_renew=true`,
  `renew-rounds 2`)으로 바꿔 재발을 잡도록 했다.
- 서명 payload 유닛 테스트가 override 적용 뒤의 **최종** payload 에
  대해 `sign(&key, &notice)` 를 다시 계산해 저장된 서명과 직접
  비교하도록 강화했다.
- 시나리오 25 의 한계(wire 상 추가 프레임 부재 자체를 직접 관찰하지
  않는다)를 보고 문구와 계획 문서에 정직하게 남겼다.
- 오래된 주석을 "29가지 시나리오" 로 갱신했다.

**claude-code 가 코덱스의 자체 뮤테이션 보고와 별개로 직접 재현했다**
— 교착 방지 가드(`if !revoked_after_grant { ... }`)를 `if true { ... }`
로 되돌려 재빌드한 뒤 `coordinator-agent-selftest` 를 실행하니 정확히
시나리오 25 에서 `exit=1` 로 실패했다(Agent 가 먼저 정상 종료해
연결을 닫아 Coordinator 의 블로킹 read 가 "스트림이 끊겼다" 오류로
끝난다 — 문자 그대로 무한 대기는 아니었지만, 이 조합이 실질적으로
고장난 상태임을 재확인했다). 원복 후 재빌드, 5회 연속 재통과 확인.

## 독립 검수 2라운드(`p136`) — CHANGES_REQUESTED, 문서 1건

코드 수정 4건은 전부 타당했다. 유일한 잔여 지적 — 계획 문서의
개정 이력에 서명 payload 테스트 보강과 시나리오 개수 주석 갱신
2건이 반영되지 않았다. claude-code 가 직접 계획 문서를 고쳤다(간단한
문서 편집이라 코덱스에 다시 위임하지 않았다).

## 독립 검수 3라운드(`p137`) — **ACCEPTED**

## 결과

```text
cargo build --workspace --exclude gputeer-runtime-windows      성공
cargo test --workspace --exclude gputeer-runtime-windows       42개 스위트 전부 통과, 0 failed
coordinator-agent-selftest                                      1차 5회 + 2차 5회(하드 타임아웃 포함) 연속 exit=0, 29/29 시나리오
```

### 뮤테이션 테스트 3건

| # | 무력화한 것 | 실행자 | 예측대로 실패한 시나리오 |
|---|---|---|---|
| 1 | Agent 의 `lease_id` 검사 | 코덱스(자체 보고) | 27 |
| 2 | Coordinator 의 revoke 서명 변조 분기 | 코덱스(자체 보고) | 26 |
| 3 | 교착 방지 가드(`revoked_after_grant`) | **claude-code(독립 재현)** | 25(exit=1) |

## 이 실험이 증명하지 "않는" 것

- 재접속(failover) 시 revoke 전달, 비동기 이벤트 multiplexing.
- 실제 Coordinator 정책 엔진(왜 revoke 하는가) — cause 는 항상
  `Quarantine` 으로 하드코딩했다.
- 시나리오 25 는 wire 상 추가 프레임 부재 자체를 패킷 수준에서
  관찰하지 않는다 — Agent 코드 경로와 양쪽 stdout 부재만 확인한다.
- Job 실제 취소, 다중 Agent, HA, TLS.
- "구현자와 검수자가 다르다" 는 이번엔 서로 다른 두 AI 가 아니라
  같은 도구(codex CLI)의 서로 다른 프로세스 인스턴스였다 — 컨텍스트는
  독립이지만 같은 모델 계열이 자기 코드를 리뷰한다는 한계는 남는다.

## 결정

1. Lease revoke 최소 경로(Coordinator 발신 · Agent 검증/무효화)를
   구현했다 — 구현 자체는 사용자 요청에 따라 코덱스 CLI(workspace-write)
   에 위임했다.
2. 독립 검수 1라운드가 실제 교착 결함 1건을 포함해 4건을 찾아냈고,
   전부 코드로 고쳤다. claude-code 가 그 중 핵심 결함(교착)을 코덱스의
   자체 보고에 의존하지 않고 직접 재현해 재확인했다.
3. 2라운드는 문서 완결성만 지적, 문서 보강 뒤 3라운드에서 `ACCEPTED`.
4. 재접속 시 revoke 유실·비동기 전송·실제 정책 엔진은 의도적으로
   범위 밖으로 남겼다.

관련: `docs/evidence/DoD-17_revoke_lease_notice_framed_ingress_커버리지.md` ·
`docs/plans/2026-08-19_1517_lease_revoke_최소_조각_v1.md` ·
`docs/reports/2026-08-19_1532_lease_revoke_최소_조각.md`
