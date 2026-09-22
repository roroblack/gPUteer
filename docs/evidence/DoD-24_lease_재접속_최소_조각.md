---
schema_version: 2
id: DoD-24
claim: "재접속(failover)의 첫 최소 조각 — Active Lease process-restart rehydration — 을 구현했다. 새 proto 메시지·필드는 추가하지 않는다. 연결이 끊긴 뒤(테스트 전용 --disconnect-after-ack 로 시뮬레이션) 완전히 새로운 Coordinator/Agent 프로세스 쌍이 같은 --lease-db·--fence-db 로 시작하면, 기존 ExecutionGrant/AgentGrantAck handshake 만으로 CoordinatorLeaseStore::get_or_issue() 가 저장된 활성 Lease(identity·epoch·시각)를 복원해 Grant 에 실어 보낸다 — Coordinator CLI 의 틀린 fence_epoch 인자보다 저장된 값이 우선한다. 같은 lease_id 를 다른 holder_node_id 로 재접속 주장하면 기존 holder identity 검사(이번 조각에서 새로 만든 게 아니라 이미 있던 코드)가 거부한다. 자동 재접속 루프·ResumeLeaseRequest·revoke 상태 보존·다중 Agent 경쟁은 명시적으로 범위 밖"
status: PASS
commit: ffdf262

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (설계 조사·구현) / claude-code (cargo build/test 독립 재확인)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-19T18:14:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "disconnect_after_ack 반환 지점이 do_renew/renew_rounds 와 무관하게 안전한지와 자원 정리, 시나리오 33 에서 Agent 가 이미 닫힌 연결에 쓰기/읽기를 시도할 때 무한 대기 없이 EOF 로 빠르게 종료하는지(오늘 이미 두 번 나온 같은 부류의 교착 버그가 세 번째로 있는지 의심), get_or_issue() 가 CLI 의 틀린 fence_epoch 보다 저장된 값을 우선하는지, holder identity 충돌 검사가 이번 조각에서 새로 만든 게 아니라 실제로 기존 코드였는지, 계획 문서가 revoke 상태 미보존 한계(DoD-22)를 정직하게 재확인하는지. 1라운드(p144) 만에 ACCEPTED — 수정 요청 없음"
review_artifact: "docs/evidence/_raw/DoD-24_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-24_lease_재접속_최소_조각_2026-08-19.txt"
raw_output_digest: "sha256:28756d1dda322a3b044fb46c9558a51180b101f3ecd4092fb335496972984f20"
raw_output_bytes: 662

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "새 proto 메시지·필드 없음 — 기존 ExecutionGrant/AgentGrantAck 만 재사용한다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용, 별도 프로세스 쌍(coordinator-stub·agent-stub) 두 세트가 같은 SQLite 파일을 공유"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 5회 연속, 각 60초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-24_lease_재접속_최소_조각_2026-08-19.txt 전문 참조)

  cargo test --workspace --exclude gputeer-runtime-windows: 42개 스위트 전부 test result: ok, FAILED/error[ 검색 결과 없음
  coordinator-agent-selftest: 5회 연속, 34개 시나리오 전부 exit=0, 매번 34줄 출력, 타임아웃 근처까지 간 적 없음
artifacts:
  - docs/plans/2026-08-19_1814_lease_재접속_최소_조각_v1.md
  - docs/reports/2026-08-19_1814_lease_재접속_최소_조각.md
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-24_lease_재접속_최소_조각_2026-08-19.txt
  - docs/evidence/_raw/DoD-24_review.txt
negative_tests:
  - "selftest 시나리오 33 — Coordinator 가 ACK 검증 직후 의도적으로 연결을 끊는다(--disconnect-after-ack). 첫 번째 프로세스 쌍은 Coordinator 는 정상 종료(DISCONNECT_AFTER_ACK 로그)하고 Agent 는 이미 닫힌 연결에 갱신 요청을 쓰려다 실패해 종료함을 확인. 이어서 완전히 새로운 프로세스 쌍이 같은 lease-db/fence-db 로 시작해, 일부러 틀리게 준 CLI --fence-epoch 3 대신 저장된 epoch=5 로 Grant/ACK 가 성공하는지 확인"
  - "selftest 시나리오 34 — 같은 lease_id 를 다른 holder_node_id 로 재접속 주장하면 기존 holder identity 검사가 거부하는지 확인(이번 조각에서 새로 만든 검사가 아니라 이미 있던 코드임을 독립 검수가 확인)"
  - "뮤테이션(코덱스 자체 보고) — disconnect_after_ack 처리 분기를 `if false` 로 무력화하면 시나리오 33 이 예상대로 exit=1 로 실패, 원복 후 재빌드·34/34 재통과"
limitations:
  - "revoke 상태는 재접속에서 보존되지 않는다 — DoD-22 가 이미 명시한 한계를 그대로 재확인한다. `revoked` 는 Agent 메모리 상태이고 `CoordinatorLeaseStore` 에는 그 필드가 없어, revoke 된 뒤 프로세스가 끊기면 새 Agent 프로세스가 같은 Lease 를 다시 받을 수 있다"
  - "자동 재접속 루프(backoff, retry, jitter)는 구현하지 않았다 — Agent 는 연결이 끊기면 그냥 종료한다. 실제 운영에서 필요한 재시도 로직은 다음 조각이다"
  - "명시적 ResumeLeaseRequest·reconnect token·session ID 는 없다 — Coordinator 가 재접속을 '식별'하는 게 아니라, 그냥 같은 CLI 설정(같은 lease_id·같은 DB 경로)으로 다시 발급을 시도했을 때 저장소가 우연히 같은 레코드를 돌려주는 것뿐이다. Agent 가 '나는 lease_id X 를 복구하러 왔다'고 wire 에서 주장하는 프로토콜은 아니다"
  - "만료된 Lease 의 재접속 복원 거부 시나리오(설계 단계가 제안한 35번)는 시간 관계상 다음 조각으로 미뤘다"
  - "여러 Agent 가 같은 lease_id 로 동시에 재접속을 주장하는 경쟁, 다중 Coordinator HA, TLS 는 여전히 범위 밖이다(다중 Agent 자체가 미착수)"
  - "구현자와 독립 검수자가 이번에도 같은 도구(codex CLI)의 서로 다른 프로세스 인스턴스였다 — 컨텍스트는 독립이지만 같은 모델 계열이 자기 코드를 리뷰한다는 근본적 한계는 남는다"
decision: "재접속(failover)의 첫 최소 조각을 구현했다 — 전체 재연결 프로토콜은 이 세션 범위를 넘는다는 설계 단계 조사(read-only)의 정직한 판단에 따라, '활성 Lease 의 프로세스 재시작 복원' 만으로 범위를 좁혔다. 기존 CoordinatorLeaseStore::get_or_issue() 인프라가 이미 이 경로의 핵심(같은 lease_id 로 재발급 요청이 오면 저장된 레코드를 그대로 돌려준다)을 갖추고 있어서, 이번 조각은 주로 그 경로가 실제로 의도대로 동작함을 증명하고 의도적 연결 단절 시뮬레이션을 추가하는 데 집중했다. 사용자 요청에 따라 구현을 코덱스 CLI(workspace-write)에 위임하고 claude-code 는 독립 재검증·독립 검수 감독 역할을 맡았다. 오늘 이미 두 번(Lease revoke·SUPERSEDED 조각) 나온 '한쪽은 끝났는데 다른 쪽은 계속 기다리는' 교착 버그가 이번에도 있을까 특별히 의심하고 검수를 요청했으나, 이번 설계는 애초에 그 위험을 피했다(Coordinator 가 연결을 끊으면 Agent 의 쓰기/읽기가 즉시 EOF 오류로 끝나지, 서로 다른 쪽이 무한정 기다리는 구조가 아니다) — 1라운드 만에 ACCEPTED."
---

# DoD-24 · Lease 재접속(failover) 최소 조각

## 무엇을 입증하려 했는가

`docs/evidence/DoD-22_lease_revoke_최소_조각.md`·`DoD-23_lease_재발급_정책_superseded.md`
가 매번 "재접속 시 revoke 유실" 을 범위 밖으로 남겼다 — Coordinator
와 Agent 사이의 TCP 연결이 끊기면, 지금 이 stub 은 처음부터 다시
시작하는 것 외에 아무 것도 하지 않는다. `CLAUDE.md` "다음에 할 일"
의 "재접속(failover)" 미착수 항목의 첫 걸음을 구현했다.

## 설계 조사(코덱스, `p142`) — 정직한 범위 판단

전체 재연결 프로토콜(자동 재접속 루프, `ResumeLeaseRequest`,
revoke/만료/다중 Agent 경쟁까지 포함한 완전한 failover)은 오늘
조각들보다 크다고 먼저 솔직하게 판단했다. 대신 "프로세스 재시작 후
활성 Lease 복원" 하나로 좁히면 하루 규모라고 추천했다 —
`CoordinatorLeaseStore::get_or_issue()` 가 이미 같은 `lease_id`
로 재발급 요청이 오면 저장된 레코드를 그대로 돌려주는 걸 실측으로
확인했고, `DurableFenceWatermark`(Agent 쪽)도 이미 재시작을 넘는
fencing 방어를 하고 있어 "절반은 이미 풀려 있다"고 판단했다.

## 구현 (코덱스, `p143`)

- 새 `disconnect_after_ack` 설정 — Coordinator 가 ACK 검증 직후
  (`renew_rounds` 루프 진입 전) 로그를 찍고 `Ok(())` 로 즉시
  반환한다. `do_renew`/`renew_rounds` 설정과 무관하게 항상 이
  지점에서 끊는다.
- selftest 시나리오 33 — 첫 프로세스 쌍이 ACK 후 끊기고, 완전히
  새로운 프로세스 쌍이 같은 `--lease-db`·`--fence-db` 로 시작해
  일부러 틀린 CLI `--fence-epoch 3` 대신 저장된 epoch=5 로 Grant/ACK
  가 성공하는지 확인.
- selftest 시나리오 34 — 같은 `lease_id` 를 다른 `holder_node_id`
  로 재접속 주장하면 거부되는지 확인(코드 조사 결과 이 검사는 이번
  조각에서 새로 만든 게 아니라 이미 있었다 — 테스트만 추가했다).

## 독립 검수(`p144`) — **1라운드 만에 ACCEPTED**

이 조각의 코드를 검토하면서, 오늘 이미 두 번(Lease revoke·
SUPERSEDED 조각) "한쪽은 끝났는데 다른 쪽은 계속 기다리는" 교착
버그가 나왔던 걸 감안해 이번에도 같은 패턴이 있는지 특히 의심하며
검수를 요청했다. 독립 검수가 확인한 바로는, 이번 설계는 애초에 그
위험 구조 자체를 피했다 — `disconnect_after_ack` 는 Coordinator 가
**먼저 정상 종료**하는 시나리오이고, Agent 가 그 뒤 갱신 요청을
쓰려고 시도하면 이미 닫힌 소켓에 대한 쓰기/읽기가 무한 대기가
아니라 **즉시 EOF/`Truncated` 오류로 끝난다**(소켓 타임아웃도 걸려
있다) — "서로 다른 쪽이 서로를 무한정 기다리는" 구조가 아니라
"한쪽이 죽으면 다른 쪽이 빠르게 실패로 끝나는" 구조라 근본적으로
같은 부류의 결함이 성립할 수 없었다.

## 결과

```text
cargo build --workspace --exclude gputeer-runtime-windows      성공
cargo test --workspace --exclude gputeer-runtime-windows       42개 스위트 전부 통과, 0 failed
coordinator-agent-selftest                                      5회 연속, 34개 시나리오 전부 exit=0
```

## 이 실험이 증명하지 "않는" 것

- revoke 상태는 재접속에서 여전히 보존되지 않는다(`DoD-22` 한계
  그대로).
- 자동 재접속 루프, `ResumeLeaseRequest`, reconnect token.
- 만료된 Lease 의 재접속 복원 거부(다음 조각으로 이월).
- 다중 Agent 경쟁, 다중 Coordinator HA, TLS.

## 결정

1. 재접속(failover)의 첫 최소 조각("활성 Lease 프로세스 재시작
   복원")을 구현했다 — 전체 재연결 프로토콜은 범위를 넘는다는
   설계 단계의 정직한 판단에 따라 의도적으로 좁혔다.
2. 오늘 이미 두 번 나온 교착 버그 패턴을 특별히 의심하며 독립
   검수를 요청했으나, 이번 설계 자체가 그 위험을 구조적으로
   피했음을 확인했다 — 1라운드 만에 `ACCEPTED`.
3. revoke 상태 미보존을 포함한 한계를 계획 문서에 정직하게
   남겼다.

관련: `docs/evidence/DoD-22_lease_revoke_최소_조각.md` ·
`docs/evidence/DoD-23_lease_재발급_정책_superseded.md` ·
`docs/plans/2026-08-19_1814_lease_재접속_최소_조각_v1.md`
