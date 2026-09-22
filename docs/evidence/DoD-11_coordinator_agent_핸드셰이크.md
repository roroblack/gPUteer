---
schema_version: 2
id: DoD-11
claim: "gputeer coordinator-stub 와 gputeer agent-stub 가 실제 별도 OS 프로세스(PID)로 TCP 를 통해 서명된 ExecutionGrant/AgentGrantAck 를 주고받는 정상 handshake 를 증명하고, 거부 경로 3종(위조 coordinator_signature, 위조 agent_signature, 동일 Grant wire bytes replay)을 gputeer coordinator-agent-selftest 가 자동으로 검증한다"
status: PASS
commit: bbbc0589e6eb4c55610bc69a26d7462f15042d73

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-19T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "coordinator-agent-selftest 6개 시나리오 중 handshake 관련 4개(정상·위조 Grant·위조 ACK·replay), 별도 OS 프로세스 검증, 뮤테이션 비공허성"
review_artifact: "docs/evidence/_raw/DoD-11_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt"
raw_output_digest: "sha256:914903814b3ed6df4aac30779d9948a89423422e1b75e0212a8beb21c3b6a944"
raw_output_bytes: 3533

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1"
  cli_bin: "target/debug/gputeer.exe (dev profile, 2026-08-19 오늘 HEAD — Lease 최소 조각(commit 595bf0f, DoD-12)까지 반영된 소스에서 빌드. commit 필드(bbbc058)는 핸드셰이크 자체가 완성된 시점을 가리킬 뿐, 이 바이너리의 빌드 시점이 아니다 — 혼동하지 않는다)"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1"
  new_message: "AgentGrantAck (proto/control.proto, domain_tag gputeer/v1/grant-ack)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 (프로세스·네트워크 계층 검증)"
network_profile: "127.0.0.1 실제 TCP 소켓, 두 개의 별도 OS 프로세스 사이"
command: |
  cargo build -p gputeer-cli
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 5회 연속
  cargo test --workspace
raw_output: |
  (docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt 전문 참조)

  5회 연속 실행 전부 exit=0, 매번 서로 다른 PID 3개(self/coordinator/
  agent). cargo test --workspace: 308 passed / 0 failed.
artifacts:
  - docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md
  - crates/coordinator/src/lib.rs
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - crates/crypto/src/framed_ingress.rs
  - proto/control.proto
  - docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt
  - docs/evidence/_raw/DoD-11_review.txt
negative_tests:
  - "위조 coordinator_signature: Coordinator 가 ExecutionGrant 를 서명한 직후 서명 마지막 바이트를 뒤집어 전송한다(corrupt_own_signature). Agent 는 read_frame() 검증에서 거부해 AgentGrantAck 를 발급하지 않는다 — coordinator_agent_selftest.rs 가 Agent 의 비정상 종료로 판정한다"
  - "위조 agent_signature: Agent 가 AgentGrantAck 를 서명한 직후 서명 마지막 바이트를 뒤집어 전송한다. Coordinator 는 ACK 검증에서 거부해 성공 처리하지 않는다 — Agent 자신은 스스로의 서명을 검증하지 않으므로 정상 종료할 수 있다는 점을 selftest 판정 기준(Coordinator 쪽 실패만 본다)이 명시한다"
  - "동일 Grant wire bytes replay: Coordinator 가 같은 프레임(재인코딩하지 않은 동일 바이트)을 같은 TCP 연결에 두 번 쓴다. verify() 가 InMemoryReplayGuard 에서 Duplicate 를 만나 read_frame() 단계에서 즉시 Err 를 반환하므로 require_replay_checked() 까지 갈 필요도 없이 거부된다"
  - "★ 뮤테이션 비공허성(2026-08-19 오늘 재현): corrupt_own_signature 처리를 if false && ... 로 무력화하면 Agent 가 위조된 Grant 에도 정상 ACK/RESULT ok=true 를 내어 selftest 가 실패로 뒤집힌다 — 원복 후 6개 시나리오 전부 재통과·cargo test --workspace 회귀 없음 확인(docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt 의 '뮤테이션 재현' 절 참조)"
  - "별도 OS 프로세스 확인: coordinator-agent-selftest 자기 자신·coordinator-stub·agent-stub 세 PID 를 상호 비교해 전부 다름을 assert 한다(Command::current_exe() 로 재실행한 실제 자식 프로세스)"
limitations:
  - "★ replay 방어는 InMemoryReplayGuard 다 — 각 stub 프로세스가 매 실행마다 새로 만드는 메모리 상태이며, 재시작을 넘는 replay 방어(DurableReplayGuard)는 이 evidence 가 증명하지 않는다. 그 성질은 별도로 crates/crypto/tests/durable_replay_process.rs 가 프로세스 경계에서 증명했다"
  - "키 배분이 테스트 전용이다 — PersistentKeyring 을 쓰지 않고 selftest 가 두 시드를 결정적으로(라벨 문자열의 BLAKE3-256) 생성해 --own-seed/--peer-pubkey 로 각 stub 에 넘긴다. 실제 키 프로비저닝·회전·폐기는 검증하지 않았다"
  - "127.0.0.1 로컬 루프백만 검증했다. 원격 네트워크·TLS·relay·인증서 교환은 범위 밖이다"
  - "Job 실행·GPU 할당·CUDA 실행·스케줄링은 범위 밖이다 — 이 evidence 는 '서명된 메시지가 프로세스 경계를 넘어 오가고 위조·replay 가 거부된다' 만 증명한다"
  - "여러 Agent 동시 처리, coordinator 고가용성/control-store, checkpoint 작성·재개 연동, runtime-policy 의 OS 강제 연동(방화벽·Job Object 등)은 전부 범위 밖이다"
  - "프로세스 crash recovery 를 검증하지 않았다 — coordinator·agent 둘 다 정상 실행 경로만 다룬다(강제 kill 은 이 evidence 범위가 아니다, checkpoint 계열 P0-03/DoD-09 가 별도로 다룬다)"
  - "Windows 단일 플랫폼에서만 실행했다"
decision: "핸드셰이크 stages 1~6(proto 메시지 추가부터 거부 경로 3종까지)이 계획대로 구현·검증됐다. docs/plans/2026-08-18_0800 의 DoD 체크박스 중 'docs/evidence/ 에 schema v2 기록'을 이 문서로 충족한다. 다음 조각(Lease 최소 조각)은 DoD-12 로 별도 기록한다."
---

# DoD-11 · coordinator/agent 최소 핸드셰이크

## 무엇을 입증하려 했는가

`CLAUDE.md` 가 오랫동안 반복해서 적어 둔 가장 큰 공백이다.

> 서비스가 아니다 — 네트워크 수신도, 데몬도, 스케줄러도 없다.
> coordinator · agent · scheduler 는 여전히 미착수다.

`gputeer selftest` §5 가 이미 127.0.0.1 실제 TCP 소켓으로 signed
Grant 를 왕복시켰지만, **같은 프로세스 안 스레드 하나**가 여는
소켓이었다 — 프로세스 경계는 증명하지 않았다. 이 스파이크는 그
다음 한 걸음이다: **실제 별도 PID 두 개**가 서명된 메시지를 주고
받고, 위조·replay 를 거부하는 것.

## 왜 `ReplicaAck` 를 재사용하지 않았는가

`ReplicaAck` 는 checkpoint durability 증거용이고
(`proto/artifact.proto`) `Lifetime::Evidence` 라 replay nonce 를
검사하지 않는다. 빌려 쓰면 "ACK 가 실제 이 Grant 에 대응하는가"
와 "ACK replay 를 거부하는가" 둘 다 증명할 수 없다. 그래서
전용 메시지 `AgentGrantAck`(`ShortLived`, nonce 필드 7)를 새로
만들었다 — domain_tag `"gputeer/v1/grant-ack"`.

## 구현 개요

```text
crates/coordinator/    listener 소유 · Grant 발급 · Agent ACK 검증
crates/agent/          TCP client 소유 · Grant 검증 · ACK 발급
crates/cli/            coordinator-stub · agent-stub · coordinator-agent-selftest 서브커맨드
```

정상 경로: Coordinator 가 `ExecutionGrant` 를 서명해 발급 → Agent
가 `read_frame()` 으로 검증 → Agent 가 서명된 `AgentGrantAck` 발급
→ Coordinator 가 ACK 를 검증하고 `grant_id`/`attempt_id`/
`agent_device_id` 를 대조한다.

키 배분은 `PersistentKeyring` 대신 `InMemoryKeyring`(검증 전용)과
호출자가 직접 쥔 `SigningKey` 를 쓴다 — `crates/cli/src/selftest.rs`
가 이미 쓰는 패턴을 그대로 따랐다. Coordinator/Agent 가 서로 다른
OS 프로세스라 키를 공유할 방법이 필요했는데, 파일 기반 keyring
저장/로드를 새로 검증하는 대신 `coordinator-agent-selftest` 가
두 시드를 결정적으로(라벨 문자열의 BLAKE3-256) 만들어 각
stub 에 `--own-seed`/`--peer-pubkey` hex 인자로 넘긴다.

## 결과

```text
gputeer coordinator-agent-selftest   5회 연속 실행 — 전부 exit=0
                                      매번 서로 다른 PID 3개(self/coordinator/agent)
cargo test --workspace               308 passed / 0 failed
cargo build --workspace              경고 0건
```

### 거부 경로 3종이 실제로 거부한다

| 시나리오 | 메커니즘 | 판정 |
|---|---|---|
| 위조 `coordinator_signature` | 서명 **후** 마지막 바이트 반전 | Agent 가 ACK 미발급 |
| 위조 `agent_signature` | 서명 **후** 마지막 바이트 반전 | Coordinator 가 성공 처리 안 함 |
| 동일 Grant wire bytes replay | 재인코딩 없이 같은 `frame` 재전송 | `read_frame()` 이 `Duplicate` 로 즉시 거부 |

**"오염된 데이터에 서명"이 아니라 "서명 후 서명 필드만 변조"다**
— 정직한 프로세스는 자기 서명을 위조하지 않으므로, 위조를
시뮬레이션하려면 테스트 전용 self-corruption 플래그가 필요했다.
TCP proxy 로 실제 중간자 변조를 만드는 대안도 검토했으나, 이
계획의 DoD 범위(서명·replay 검증)를 더 증명하지 않으면서
아키텍처만 복잡해져 기각했다.

### 뮤테이션으로 비공허성을 확인했다(2026-08-19 오늘 재현)

`corrupt_own_signature` 처리(위조 Grant 시나리오)를
`if false && config.corrupt_own_signature` 로 일시 무력화하자,
selftest 가 정확히 예상대로 실패했다 — "위조된
`coordinator_signature` 가 거부되지 않았다, Agent 가 ACK 를
발급했다"(Agent 가 실제로 `RESULT ok=true` 를 냈다). 원복 후
재빌드해 6개 시나리오 전부 재통과와 `cargo test --workspace` 회귀
없음을 확인했다 — 2026-08-18 최초 구현 때 이미 한 번 확인된
것이지만, 이 evidence 문서를 위해 오늘 직접 다시 재현했다
(`docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt` 참조).

### 실제로 실행해서 잡은 결함(설계 문서에는 없던 것)

첫 구현은 `coordinator.stdout.take()` 로 `READY` 줄을 읽은 뒤,
나중에 `wait_with_output()` 을 또 불러 나머지 출력(`RESULT` 줄)을
얻으려 했다 — 그러나 stdout 핸들은 이미 `take()` 로 소비된 뒤라
`wait_with_output()` 은 빈 stdout 을 돌려줬다. 실행해 보고서야
발견했다. `wait()` 만 쓰고 stdout/stderr 를 처음부터 끝까지 직접
읽는 것으로 고쳤다.

### 코덱스 독립 검수가 잡은 결함(stderr 파이프 교착 위험)

`coordinator_agent_selftest.rs` 가 coordinator 의 stderr 를 메인
흐름과 동시에 비우지 않았다 — coordinator 가 OS 파이프 버퍼를
채울 만큼 stderr 에 쓰면(에러 메시지가 길어지는 경우 등)
coordinator 가 쓰기에서 블로킹되고, agent 는 coordinator 의 TCP
응답을 기다리느라 블로킹되어 selftest 전체가 교착할 수 있었다.
정상 경로에서는 coordinator 가 stderr 에 아무것도 안 써서 5회
연속 실행에서는 드러나지 않았던 잠재적 결함이다. coordinator 의
stderr 를 별도 스레드가 `read_to_string` 으로 처음부터 끝까지
비우고 메인 흐름은 `.join()` 으로 그 결과를 나중에 받도록 고쳤다.

## 안전망 5종이 새 메시지 추가를 놓치지 않았다

`AgentGrantAck` 를 추가하는 과정에서 계획서에 없던 안전망들이
전부 걸렸다 — `field_number_audit.rs::AUDITED`,
`lifetime_consistency.rs`(`declared_lifetime_matches_message_capability`/
`every_signable_is_covered`), `schema_fingerprint.rs`(P0-08 가
만든 스키마 진화 가드), `canonical_vectors.rs` 의
domain 개수 하드코딩, `stream_ownership.rs::every_crate_is_covered_by_ownership_rules`
(새 크레이트 `coordinator`/`agent` 감지). 다섯 곳 모두 순서대로
채웠다 — "강제할 수 없는 규칙은 규범이 아니라 희망이다"(P0-08)
의 실증이다.

## 이 실험이 증명하지 "않는" 것

- **재시작을 넘는 replay 방어.** 각 stub 은 `InMemoryReplayGuard`
  를 쓴다 — 프로세스가 매번 새로 뜨므로 실행 간 replay 상태가
  없다. `DurableReplayGuard` 특정 성질은
  `crates/crypto/tests/durable_replay_process.rs` 가 별도로
  증명했다.
- **실제 키 프로비저닝.** 테스트 전용 결정적 시드로만 검증했다.
- **원격 네트워크·TLS.** 127.0.0.1 로컬 루프백만 검증했다.
- **Job 실행·GPU 할당·스케줄링·다중 Agent·고가용성.** 전부 범위
  밖이다 — "완전한 coordinator/agent" 가 아니라 프로세스 경계 +
  서명 전달 + 거부 경로만 증명하는 최소 조각이다.
- **프로세스 crash recovery.**
- Windows 단일 플랫폼.

## 결정

1. 핸드셰이크 단계 1~6 이 계획대로 완료됐다 — `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
   의 DoD 체크박스 중 "docs/evidence/ 에 schema v2 기록"을 이
   문서로 충족한다.
2. lease 발급·갱신·다중 Agent·스케줄링 등은 후속 조각으로
   남는다 — Lease 최소 조각은 `DoD-12` 로 별도 기록한다.

관련: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md` ·
`docs/evidence/DoD-12_coordinator_agent_lease_최소_조각.md` ·
`docs/contracts/01_스트림_소유권.md`
