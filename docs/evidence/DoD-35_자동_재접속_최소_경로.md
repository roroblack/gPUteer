---
schema_version: 2
id: DoD-35
claim: "자동 재접속 루프 전체 로드맵(docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md, 7조각·6~8일 규모)의 '하루 조각으로 가능한 부분'(1+2 축소판)을 구현했다 — proto 변경 없이, 같은 Agent 프로세스가 TCP 연결이 끊겼을 때 bounded retry(최대 8회, 총 60초, exponential backoff+full jitter, 연결별 3초 timeout)로 재연결해 기존 Grant/ACK handshake 를 처음부터 재수행한다. Coordinator 는 listener.accept() 를 반복하는 루프로 바뀌었고(--max-connections·--accept-timeout-ms·--drop-connection-after-ack-once 신규 플래그, 기존 --disconnect-after-ack 의미는 불변), Grant/ACK/Renew nonce 계산에 connection_attempt 를 반영해 재접속 시 nonce 충돌(replay 오인)을 피한다. Windows 플랫폼에서 nonblocking listener 설정이 accept 된 stream 에도 전파돼 WSAEWOULDBLOCK(10035) 이 발생하던 버그도 발견해 accepted stream 을 blocking 모드로 되돌려 고쳤다. coordinator-agent-selftest 시나리오 49~52 신설(정상 재접속 성공·bounded retry 소진·재접속 중 revoke·재접속 중 만료), 기존 48개 시나리오는 전부 회귀 없이 그대로 통과한다(nonce 값이 connection_attempt=0 일 때 기존과 바이트 단위로 동일). Resume proto·durable request ledger·다중 Agent 경쟁은 의도적으로 범위 밖 — 이건 완전한 Resume 프로토콜이 아니라 로드맵 7조각 중 1+2 만 구현한 최소 경로다"
status: PASS
commit: 3de6cb5

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현 3라운드) / claude-code (cargo build·test·coordinator-agent-selftest 5회 연속 독립 재실행 — 매 라운드마다, 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T10:01:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스 3라운드"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(p182) — nonce 회귀·교착 위험을 최우선으로 검토해 진짜 결함 2건을 찾았다: (a) Agent 의 재접속 루프가 TCP connect() 레벨 실패(ConnectionRefused/TimedOut)마다 nonce 계산용 카운터를 증가시키는데 Coordinator 는 실제 accept() 성공 횟수만 증가시켜, connect() 실패가 한 번이라도 있으면 두 값이 어긋나 GRANT_REJECTED 로 재접속이 실패함(agent/lib.rs:207-231, coordinator/lib.rs:218-226). (b) selftest 헬퍼가 Agent 종료 후 Coordinator stdout 을 무제한 blocking read_to_string() 으로 읽어(coordinator_agent_selftest.rs:286-289) Coordinator 타임아웃 감시(wait_until())가 그 뒤에 와서, Coordinator 가 두 번째 accept 에서 멈추면 120초 하드 타임아웃이 보장 안 됨. CHANGES_REQUESTED — 감독자가 코드로 직접 재확인해 두 지적 모두 실재함을 확인. 2라운드(p184) — 수정 결과 프로덕션 로직 자체는 코드로 직접 추적해 올바름을 확인(agent/lib.rs:226-258 의 connection_attempt 가 재시도 변수와 분리돼 TCP connect() 성공 시에만 증가, coordinator/lib.rs:245-254 도 동일 원칙, 양쪽 nonce 유도 규칙과 attempt=0 바이트 생략 일치, selftest 의 reader thread+wait_until 순서 수정도 문제없음) — 단 새 회귀 테스트 nonce_attempt_counter_ignores_failed_connects(agent/lib.rs:1148-1161)가 run() 의 실제 재접속 루프나 derive_nonce() 를 안 타고 헬퍼를 손으로 두 번 호출할 뿐이라 증명력이 없다고 CHANGES_REQUESTED. 3라운드(p186) — 테스트를 실제 TCP connect() 거부 후 실제 run() 을 스레드로 실행해 재시도시키고 다시 listener 를 열어 성공 연결을 받는 통합 테스트로 재작성(agent/lib.rs:1151, 실제 ACK nonce 가 connection_attempt=0 유도값인지 assert, 1299행), 뮤테이션(원래 버그 상태로 되돌리면 실제로 실패)까지 확인. tempfile dev-dependency 추가가 순수 테스트 전용인지, 프로덕션 로직이 이번 라운드에서 안 바뀌었는지, git diff --stat 범위까지 전부 확인하고 ACCEPTED. 3라운드 전체에 걸쳐 감독자(claude-code)가 매 라운드마다 cargo build/test·coordinator-agent-selftest 5회 연속(전부 exit=0, 52개 시나리오, 약 25.4~26.7초/회, 90~120초 하드 타임아웃 대비 여유)으로 독립 재확인했다"
review_artifact: "docs/evidence/_raw/DoD-35_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-35_자동_재접속_최소_경로_2026-08-20.txt"
raw_output_digest: "sha256:0e1464e1e414e0d7871ec68ec6e4f649e298c2b59fca37f761fbb5fe988fbf27"
raw_output_bytes: 7422

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — 이번 조각은 기존 Grant/ACK handshake 를 재사용하며 새 메시지(AgentSessionHello/ResumeLeaseRequest/ResumeLeaseResult)는 만들지 않았다(로드맵 조각 3 이후로 명시적으로 미룸)"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음). Grant/ACK/Renew nonce 유도 함수에 connection_attempt 입력을 추가했으나 attempt=0(기존 단일 연결 경로) 에서는 바이트 생략으로 기존 nonce 값과 동일 — 3라운드 검수가 코드로 확인"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc — Windows 전용 WSAEWOULDBLOCK(10035) 버그를 실측으로 발견·수정(accepted stream 을 blocking 모드로 복귀, listener 자체는 nonblocking 유지)"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub, 신규 통합 테스트도 실제 loopback TCP 연결 거부/재개 사용"
network_profile: "127.0.0.1 루프백 TCP 만 사용(기존 하네스와 동일). 신규 통합 테스트(agent/lib.rs:1151)는 실제 포트를 닫았다 다시 여는 방식으로 진짜 connect() 거부를 재현"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  cargo test -p gputeer-agent
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 3라운드 각각 5회 연속, 각 120초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-35_자동_재접속_최소_경로_2026-08-20.txt,
   docs/evidence/_raw/DoD-35_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 3라운드 전부 성공, 실패 0건
  cargo test -p gputeer-agent: 5개 전부 통과(신규 통합 테스트 5.36초 — 실제 TCP 타이밍 포함)
  coordinator-agent-selftest: 3라운드 각각 5회 연속 exit=0, 매회 52개 시나리오,
    약 25.4~26.7초/회(90~120초 하드 타임아웃 대비 여유)
artifacts:
  - docs/plans/2026-08-20_1001_자동_재접속_최소_경로_v1.md
  - crates/agent/src/lib.rs
  - crates/agent/Cargo.toml
  - Cargo.lock
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-35_자동_재접속_최소_경로_2026-08-20.txt
  - docs/evidence/_raw/DoD-35_review.txt
negative_tests:
  - "selftest 시나리오 49 — 같은 Agent/Coordinator PID 에서 ACK 후 1회 연결을 끊고, Agent 가 connection_attempt=1 의 새 Grant/ACK nonce 로 bounded reconnect 에 성공하며 RESULT ok=true 가 정확히 한 번만 발생함을 확인"
  - "selftest 시나리오 50 — max-reconnect-attempts=2/max-duration=3s 로 재접속 예산을 소진시켜 RESULT ok=true 가 발생하지 않음을 확인"
  - "selftest 시나리오 51 — 첫 ACK 직후 durable revoke 를 기록한 뒤 재접속 시 get_or_issue() 가 revoked 로 거부해 RESULT ok=true 가 없음을 확인"
  - "selftest 시나리오 52 — 2초 TTL + 2500ms next-accept 지연으로 재접속 시 Expired 로 거부됨을 확인"
  - "단위 테스트(agent/lib.rs:1151, 3라운드 재작성) — 실제 loopback listener 를 닫아 TCP connect() 를 실제로 거부시킨 뒤, 실제 Agent::run() 을 스레드로 실행해 재시도 루프를 태우고, 몇 초 뒤 같은 포트에 실제 테스트 Coordinator 리스너를 열어 연결을 성공시킨다 — 이때 오간 실제 Grant nonce·Agent ACK nonce 가 connection_attempt=0 유도값과 일치함을 assert"
  - "뮤테이션(코덱스 자체 보고, p185) — production 코드를 attempt_config.connection_attempt = attempt(원래 버그 상태, 재시도 루프 반복 횟수를 그대로 씀)로 되돌리면 위 통합 테스트가 실제 TCP connect 실패 2회 후 성공(attempt=2)에서 GRANT_REJECTED: nonce does not match connection attempt 로 실패함을 확인, 원복 후 재검증 통과"
limitations:
  - "이건 완전한 Resume 프로토콜이 아니다 — 새 proto 메시지(AgentSessionHello·ResumeLeaseRequest·ResumeLeaseResult)가 없어, 재접속마다 기존 Grant/ACK handshake 전체를 처음부터 재수행한다(같은 lease_id 로 CoordinatorLeaseStore::get_or_issue() 가 저장된 레코드를 그대로 돌려주는 기존 DoD-24 인프라에 의존)"
  - "RenewLeaseRequest 를 보낸 뒤 결과를 받기 전에 연결이 끊기는 애매한 경우(AmbiguousRenew)는 재시도하지 않고 즉시 종료한다 — durable request ledger 가 없어 Coordinator 가 이미 갱신했을 가능성을 배제할 수 없기 때문이다. 이 경로의 실제 해소는 로드맵 조각 5(durable request ledger)로 미뤄졌다"
  - "Coordinator 는 순차적으로 연결을 처리한다(accept → 처리 → 종료 → 다시 accept) — 다중 Agent 가 동시에 경쟁하는 시나리오는 다루지 않는다. 이건 로드맵 조각 7(다중 Agent 경쟁)로 미뤄졌다"
  - "revoke/만료 거부는 여전히 Coordinator store 단계에서 raw 실패로만 Agent 에 전달된다 — signed REVOKED/EXPIRED wire 결과를 재접속 경로에서 Agent 가 명시적으로 받는 것은 Resume proto(로드맵 조각 3)가 생겨야 가능하다"
  - "grant_id 자체는 CoordinatorLeaseStore 의 identity 에 포함되지 않는다 — 이번 조각은 재접속 시나리오 동안 Coordinator 설정이 같은 grant_id 를 계속 쓴다는 전제에 의존한다(selftest 가 이 전제를 지킨다)"
  - "async runtime 전환은 하지 않았다 — Coordinator 의 반복 accept 는 여전히 동기 블로킹 방식이다"
decision: "자동 재접속 루프 전체 로드맵(7조각, 6~8일)의 가장 작은 실행 가능한 부분집합을 구현했다 — 같은 Agent 프로세스가 연결만 끊긴 경우에 한해 bounded retry 로 기존 handshake 를 재수행한다. 구현을 코덱스 CLI(workspace-write)에 3라운드에 걸쳐 위임했고, 독립 검수가 1라운드에서 진짜 nonce 정합성 결함과 selftest 교착 위험을 찾아 반려, 2라운드에서 그 수정 자체는 올바르다고 확인하되 회귀 테스트의 증명력 부족을 지적, 3라운드에서 테스트를 실제 프로덕션 경로를 타는 통합 테스트로 재작성한 뒤 ACCEPTED. 감독자가 매 라운드 cargo build/test·selftest 5회 연속으로 독립 재확인했다."
---

# DoD-35 · 자동 재접속 최소 경로

## 무엇을 입증하려 했는가

사용자가 기상 후 "코덱스로 더 할 거 뭐가 남았는지 체크해서 작업
이어가"라고 지시했다. 재조사(`p179`) 결과 최우선 후보는 이미
설계까지 끝나 있던 자동 재접속 루프 전체 로드맵
(`docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`,
7조각·6~8일 규모)의 "하루 조각으로 가능한 부분" — 조각 1+2 의
축소판이었다. 정확한 범위를 확정하는 후속 설계 조사(`p180`)가
"proto 변경 없이 Agent bounded retry + Coordinator 반복 accept"
로 목표를 좁히고, 정직한 규모를 1.5~2일로 재산정했다 — 사용자가
직접 "그대로 한 번에 진행" 을 선택했다.

## 구현 1라운드 (코덱스, `p181`)

- `crates/agent/src/lib.rs` — `SessionError`·`RetryPolicy`·
  `RetryBudget`·`run_one_connection`·`connect_with_timeout`·
  `classify_framing_error` 신설, Grant/ACK/Renew nonce 에
  `connection_attempt` 반영, replay guard/fence watermark 를
  연결 간 유지.
- `crates/coordinator/src/lib.rs` — 반복 accept 루프, `--max-connections`·
  `--accept-timeout-ms`·`--drop-connection-after-ack-once` 신설.
- `crates/cli/src/coordinator_agent_selftest.rs` — `run_reconnect_case`
  헬퍼, 신규 시나리오 49~52.
- Windows `WSAEWOULDBLOCK`(10035) 플랫폼 버그 발견·수정.

## 독립 검수 1라운드(`p182`) — `CHANGES_REQUESTED`(진짜 결함 2건)

감독자가 먼저 기존 48개 시나리오 회귀 없음을 직접 확인한 뒤 검수를
요청했다. 검수가 **진짜 결함 2건**을 찾았다 — (1) Agent 의 재접속
루프가 TCP `connect()` 레벨 실패마다 nonce 계산용 카운터를
증가시키는데 Coordinator 는 실제 `accept()` 성공 횟수만 증가시켜,
연결 실패가 한 번이라도 있으면 두 값이 어긋나 재접속 자체가
`GRANT_REJECTED` 로 실패하는 결함. (2) selftest 헬퍼가 Coordinator
stdout 을 무제한 blocking read 로 읽어 Coordinator 가 반복 accept
로 멈추면 120초 하드 타임아웃이 보장 안 되는 결함. 감독자가 코드로
직접 재확인해 둘 다 실재함을 확인했다.

## 구현 2라운드 (코덱스, `p183`)

- nonce 계산용 카운터를 TCP `connect()` 실제 성공 이후에만
  증가하도록, 재시도 루프 반복 횟수와 분리.
- Coordinator stdout 을 별도 reader thread 로 옮기고, `wait_until(deadline)`
  을 blocking read 보다 먼저 실행하도록 순서 변경.
- 기존 `run_handshake()` 의 `--disable-reconnect` 주입은 유지
  (attempt=0 에서 nonce 값이 기존과 바이트 단위로 동일함을 근거로).

## 독립 검수 2라운드(`p184`) — `CHANGES_REQUESTED`(로직은 정상,
테스트 증명력 부족)

검수가 이번엔 **프로덕션 로직 자체는 코드로 직접 추적해 올바르다**
고 확인했다 — nonce-attempt 분리·Coordinator 카운터·nonce 유도
규칙 일치·selftest 타임아웃 순서 전부 문제없음. 다만 새로 추가한
회귀 테스트 `nonce_attempt_counter_ignores_failed_connects` 가
`run()` 의 실제 재접속 루프나 `derive_nonce()` 를 안 타고 헬퍼를
손으로 두 번 호출할 뿐이라 "프로덕션이 다시 회귀해도 이 테스트는
계속 통과할 수 있다" 는 증명력 부족을 지적했다.

## 구현 3라운드 (코덱스, `p185`)

프로덕션 로직은 건드리지 않고, 회귀 테스트를 완전히 다시 썼다 —
실제 loopback 포트를 닫아 진짜 `connect()` 거부를 만들고, 실제
`Agent::run()` 을 스레드로 실행해 재시도를 태운 뒤, 같은 포트에
실제 테스트 Coordinator 리스너를 열어 연결을 성공시키고, 오간
실제 Grant/ACK nonce 가 `connection_attempt=0` 유도값과 일치하는지
assert 한다. 뮤테이션으로 원래 버그 상태를 재현해 이 테스트가
실제로 실패함을 확인한 뒤 원복했다.

## 독립 검수 3라운드(`p186`) — **`ACCEPTED`**

새 테스트가 실제 프로덕션 경로(`run()`)를 타는지, nonce 값을
실제로 assert 하는지, 뮤테이션 논리가 타당한지, `tempfile`
dev-dependency 추가가 순수 테스트 전용인지, 프로덕션 로직이 이번
라운드에서 다시 바뀌지 않았는지까지 전부 코드로 확인하고 최종
`ACCEPTED`.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   3라운드 전부 성공, 실패 0건
cargo test -p gputeer-agent                                       5개 전부 통과(신규 통합 테스트 5.36초)
coordinator-agent-selftest                                        3라운드 각각 5회 연속 exit=0, 52개 시나리오, 약 25.4~26.7초/회
```

## 이 실험이 증명하지 "않는" 것

- 완전한 Resume 프로토콜이 아니다 — 새 proto 메시지 없음.
- `AmbiguousRenew`(갱신 결과 유실) 는 재시도 안 하고 즉시 종료 —
  durable request ledger 가 없다.
- 다중 Agent 동시 경쟁은 다루지 않는다 — Coordinator 는 순차
  처리만 한다.
- signed `REVOKED`/`EXPIRED` wire 결과는 재접속 경로에서 여전히
  없다.

## 결정

1. 자동 재접속 루프 로드맵 7조각 중 가장 작은 실행 가능한
   부분집합(1+2 축소판)을 구현했다 — proto 변경 없음.
2. 독립 검수 3라운드 끝에 `ACCEPTED` — 1라운드가 찾은 진짜 nonce
   정합성 결함과 selftest 교착 위험을 전부 고쳤고, 2라운드가
   지적한 회귀 테스트 증명력 부족도 실제 프로덕션 경로를 타는
   통합 테스트로 재작성해 해소했다.
3. Resume proto·durable request ledger·다중 Agent 경쟁·Coordinator
   HA 는 로드맵의 후속 조각(3~7)으로 명시적으로 남는다.

관련: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`
(전체 7조각 로드맵) · `docs/plans/2026-08-20_1001_자동_재접속_최소_경로_v1.md`
(이번 조각의 계획 문서) · `docs/evidence/DoD-24_lease_재접속_최소_조각.md`
(재사용한 `get_or_issue()` 인프라)
