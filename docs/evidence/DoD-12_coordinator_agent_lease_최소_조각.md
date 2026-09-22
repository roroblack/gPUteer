---
schema_version: 2
id: DoD-12
claim: "Coordinator 가 ExecutionGrant.lease 에 서명된 Lease 를 채워 보내고, Agent 가 그 nested Lease 를 outer Grant 와 독립적으로 검증해 fence_epoch 를 FenceWatermark 에 기록한다. 위조된 nested Lease 서명과 만료된 Lease 는 각각 거부된다"
status: PASS
commit: 595bf0f8a2403a7f1d66c4831c20158e5d5e75b4

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
review_scope: "coordinator-agent-selftest 시나리오 5·6(위조 nested Lease 서명·만료된 Lease), FenceWatermark 기록, 뮤테이션 비공허성"
review_artifact: "docs/evidence/_raw/DoD-12_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt"
raw_output_digest: "sha256:914903814b3ed6df4aac30779d9948a89423422e1b75e0212a8beb21c3b6a944"
raw_output_bytes: 3533

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1"
  cli_bin: "target/debug/gputeer.exe (dev profile, DoD-11 과 같은 빌드)"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1 (§6 규칙 i — 중첩 메시지는 각자 서명된다)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "127.0.0.1 실제 TCP 소켓, 두 개의 별도 OS 프로세스 사이 — DoD-11 과 같은 handshake 위에 얹는다"
command: |
  cargo build -p gputeer-cli
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 6개 시나리오, 5회 연속
  cargo test --workspace
raw_output: |
  (docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt 전문 참조 —
  같은 실행이 시나리오 5·6 을 포함한 6개 전부를 검증한다)

  5) 위조 nested Lease 서명 거부 확인 (Agent 가 Lease 를 outer Grant 와
     독립적으로 검증한다)
  6) 만료된 Lease 거부 확인 (Lease::LIFETIME == LongLived 의 만료 검사)

  5회 연속 전부 exit=0. cargo test --workspace: 308 passed / 0 failed.
artifacts:
  - docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md
  - crates/coordinator/src/lib.rs
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - crates/runtime-policy/src/lease_scope.rs
  - proto/lease.proto
  - docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt
  - docs/evidence/_raw/DoD-12_review.txt
negative_tests:
  - "위조 nested Lease 서명: Coordinator 가 Lease 를 독립적으로 서명한 뒤(issue_lease) 그 서명 마지막 바이트를 뒤집어 outer Grant 에 실어 보낸다(corrupt_lease_signature). 규칙 i 에 따라 nested Lease.coordinator_signature 는 outer Grant.coordinator_signature 계산에 들어가지 않으므로, outer Grant 서명은 여전히 유효한 채로 Agent 에 도달한다 — Agent 가 verify_and_record_lease() 로 nested Lease 를 outer Grant 와 별도로 검증해야만 이 위조를 잡는다"
  - "만료된 Lease: Lease.expires_at_unix_ms 를 발급 시각보다 과거로 만든다(expire_lease). Lease::LIFETIME == LongLived 이므로 verify() 가 만료 검사를 수행해 거부한다"
  - "★ 뮤테이션 비공허성: verify_and_record_lease() 호출을 if false { } 로 무력화하면 시나리오 5 가 정확히 예상대로 실패한다(\"위조된 nested Lease 서명이 거부되지 않았다 — outer Grant 검증만으로는 이 결함을 잡지 못한다는 뜻이다\") — 원복 후 6개 시나리오 전부 재통과 확인"
  - "서명 검증 순서: verify_and_record_lease() 는 gputeer_protocol::verify() 로 서명을 먼저 검증하고, 서명이 유효하다고 확인된 뒤에만 attempt_id·issuing_coordinator_id 는 Grant 의 값과, holder_node_id 는 Agent 자신의 config.agent_device_id 와 일치하는지(상관관계 검사 3건, 비교 대상이 서로 다르다)와 job_id 비공백 검사(watermark 키로 쓰이므로 비어 있으면 거부, 다른 값과의 비교가 아니다)를 한다 — 서명 안 된 필드값을 먼저 믿고 분기하지 않는다. ★ 처음에 넷 다 '상관관계'로 뭉뚱그려 적었던 것을 이번 재검수에서 정정했다"
  - "위조 Lease 시나리오의 판정 기준: Agent 쪽 실패만 본다. Coordinator 는 outer Grant 를 정상 서명하므로 자기 자신은 성공을 주장할 수 있다 — 이 시나리오가 증명하려는 것은 정확히 'outer 검증만으로는 부족하다' 는 것이다"
limitations:
  - "★ FenceWatermark 는 Agent 프로세스 로컬 메모리 상태다(is_durable() == false) — 재시작 후 stale Lease 차단은 이 evidence 가 증명하지 않는다. FenceWatermark::new() 가 handshake 매 실행마다 새로 만들어진다"
  - "RenewLeaseRequest 왕복·만료 연장은 범위 밖이다 — Coordinator 가 Lease 를 발급하는 것까지만 검증했고, 갱신 프로토콜은 아직 없다"
  - "RenewLeaseResult 정책 처리·Lease revoke 는 범위 밖이다"
  - "여러 Agent 동시 처리, scheduler·queue·capacity allocation 은 범위 밖이다"
  - "Coordinator control store·term/epoch 영속화·HA 는 범위 밖이다 — coordinator_term 은 이 stub 에서 항상 1로 고정된다"
  - "Job 실행·GPU 할당·checkpoint 연동은 범위 밖이다"
  - "TLS·원격 네트워크·crash recovery 는 DoD-11 과 같은 이유로 범위 밖이다. 운영용 key protection 도 여전히 테스트 전용 K0 그대로다"
  - "Windows 단일 플랫폼에서만 실행했다"
decision: "Lease 최소 조각(Coordinator 가 서명된 Lease 를 실어 보내고 Agent 가 독립 검증)이 계획대로 구현·검증됐다. docs/plans/2026-08-18_1800 의 DoD 체크박스 중 'docs/evidence/ 에 schema v2 기록'을 이 문서로 충족한다. 다음 후보(RenewLeaseRequest 왕복, 다중 Agent, coordinator control store)는 이 계획 문서의 Out 절이 이미 명시했다 — 각각 새 계획 문서가 필요하다."
---

# DoD-12 · coordinator/agent Lease 최소 조각

## 무엇을 입증하려 했는가

`DoD-11`(coordinator/agent 최소 핸드셰이크)이 남긴 것이다 — 별도
PID 두 개가 서명된 `ExecutionGrant`/`AgentGrantAck` 를 주고받고
위조·replay 를 거부하는 것까지만 증명했고, **lease 발급·갱신·
다중 Agent·스케줄링은 전부 범위 밖으로 남겼다.**
`ExecutionGrant.lease` 필드는 proto 에 이미 있었지만 그 stub 이
채우지 않았고, `Lease`/`RenewLeaseRequest` 는 이미 `Signable` 이지만
아무도 이 경로에서 서명·검증하지 않았다.

이 스파이크는 그 중 **가장 작은 다음 한 걸음**만 증명한다: Grant
가 유효한 서명된 Lease 를 운반하고, Agent 가 그것을 독립적으로
검증한다.

## 구현 개요

- `crates/coordinator/src/lib.rs::issue_lease()` — `Lease` 를 만들어
  **독립적으로 서명**한다(`crates/coordinator/src/lib.rs:234-268`
  근방). `corrupt_lease_signature` 는 서명 **후** 마지막 바이트를
  뒤집는다 — 규칙 i 에 따라 outer `Grant` 서명 계산에는 nested
  `Lease.coordinator_signature` 가 들어가지 않으므로, 이 위조는
  outer 서명을 깨지 않는다.
- `crates/agent/src/lib.rs::verify_and_record_lease()` — Grant 의
  replay 검사를 통과한 뒤에만 호출된다(`crates/agent/src/lib.rs:101`
  근방에서 `FenceWatermark::new()` 를 만든 뒤 이어서 호출).
  `gputeer_protocol::verify()` 로 nested Lease 를 독립 검증하고,
  **서명 검증이 끝난 뒤에만** `attempt_id`/`issuing_coordinator_id`
  가 Grant 의 값과 일치하는지, `holder_node_id` 가 이 Agent
  자신의 `config.agent_device_id` 와 일치하는지(상관관계 검사 3건,
  비교 대상이 서로 다르다 — 앞 둘은 Grant 값과, `holder_node_id`
  는 Agent 자기 설정값과), `job_id` 가 비어 있지 않은지(watermark
  키이므로 단순 비공백 검사, 다른 값과의 비교가 아니다)를 확인한다.
- `Ed25519Verifier::new(coordinator_keys)` 와 outer Grant 검증에
  쓰던 것과 같은 `InMemoryReplayGuard` 를 그대로 재사용한다 —
  `Lease` 는 `LongLived` 라 replay 검사 대상이 아니므로(§10) 안전
  하게 공유할 수 있다.
- `crates/agent/Cargo.toml` 에 `gputeer-runtime-policy` 의존성을
  추가했다 — `FenceWatermark` 를 쓰기 위해서다. Agent 프로세스
  로컬 상태로만 쓴다 — durable 하지 않다는 한계는
  `FenceWatermark::is_durable() == false` 로 이미 정직하게
  표현되어 있다(`crates/runtime-policy/src/lease_scope.rs`).

## 결과

```text
gputeer coordinator-agent-selftest   6개 시나리오(정상 + 거부 경로 5) — 5회 연속 전부 통과
cargo test --workspace               308 passed / 0 failed
```

DoD-11 이 이미 증명한 4개 시나리오(정상·위조 Grant·위조 ACK·
replay) 위에 이번 조각이 시나리오 5·6(위조 nested Lease·만료된
Lease)을 추가했다.

### outer 검증만으로는 부족하다는 것을 실제로 보였다

Lease 는 outer `ExecutionGrant` 안에 중첩되지만 §6 규칙 i 에 따라
**각자 독립적으로 서명**된다. Coordinator 가 nested Lease 서명만
위조해도 outer Grant 서명은 여전히 유효하다 — Agent 가 nested
Lease 를 별도로 `verify()` 하지 않으면 위조를 놓친다. 이것이
`verify_and_record_lease()` 가 존재하는 이유이고, 시나리오 5가
정확히 이 성질을 시험한다.

### 뮤테이션으로 비공허성을 확인했다

`verify_and_record_lease()` 호출을 `if false { }` 로 무력화하자,
시나리오 5가 정확히 예상대로 실패했다 — "위조된 nested Lease
서명이 거부되지 않았다, outer Grant 검증만으로는 이 결함을 잡지
못한다는 뜻이다". 원복 후 6개 시나리오 전부 재통과를 확인했다.

## 이 실험이 증명하지 "않는" 것

- **durable fence watermark.** `FenceWatermark` 는 Agent 프로세스
  로컬 메모리 상태다. 재시작 후 stale Lease 차단은 증명하지
  않는다.
- **RenewLeaseRequest 왕복·만료 연장.** Coordinator 가 Lease 를
  발급하는 것까지만 검증했다.
- **RenewLeaseResult 정책 처리·Lease revoke.**
- **여러 Agent 동시 처리·scheduler·queue·capacity allocation.**
- **Coordinator control store·term/epoch 영속화·HA.**
  `coordinator_term` 은 이 stub 에서 항상 1이다.
- **Job 실행·GPU 할당·checkpoint 연동.**
- **TLS·원격 네트워크·crash recovery·운영용 key protection.**
- Windows 단일 플랫폼.

## 결정

1. Lease 최소 조각이 계획대로 완료됐다 —
   `docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md`
   의 DoD 체크박스 중 "docs/evidence/ 에 schema v2 기록"을 이
   문서로 충족한다.
2. 다음 후보(`RenewLeaseRequest` 왕복, 다중 Agent, coordinator
   control store)는 이 계획 문서의 "Out" 절이 이미 명시했다 — 각각
   새 계획 문서가 필요하다.

관련: `docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md` ·
`docs/evidence/DoD-11_coordinator_agent_핸드셰이크.md` ·
`crates/runtime-policy/src/lease_scope.rs`
