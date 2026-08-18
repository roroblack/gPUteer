# 2026-08-18_1800_coordinator_agent_lease_최소_조각_v1

- **기준선:** `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  (단계 1~6 전부 완료·검수됨). 그 계획의 "Out" 절이 명시적으로 남겨 둔
  lease 발급 쪽 첫 걸음.
- **대상 단계:** v0.1
- **선행 게이트:** 없음(기존 coordinator/agent 핸드셰이크 위에 얹는다)

★ 이 문서는 코덱스(`agent:codex-cli`, read-only 샌드박스)의 설계
응답(`p67` 프롬프트)을 거의 그대로 옮긴 뒤, claude-code 세션이 실제
구현·실측까지 마친 결과를 기록한다.

## 왜 지금 이 조각인가

기존 계획은 별도 PID 두 개가 서명된 `ExecutionGrant`/`AgentGrantAck`
를 주고받고 위조·replay 를 거부하는 것까지만 증명했다 — lease 발급·
갱신·다중 Agent·스케줄링은 전부 범위 밖으로 남겼다. `ExecutionGrant.lease`
필드는 proto 에 이미 있었지만 이 stub 이 채우지 않았고, `Lease`/
`RenewLeaseRequest` 는 이미 `Signable` 이지만 아무도 이 경로에서
서명·검증하지 않았다.

## 범위

### In
- Coordinator 가 `ExecutionGrant.lease` 에 서명된 `Lease` 를 채워
  보낸다.
- Agent 가 nested `Lease` 를 outer `Grant` 와 **독립적으로** 검증하고
  (§6 규칙 i — 중첩 메시지는 각자 서명된다), `fence_epoch` 를
  `crates/runtime-policy::FenceWatermark` 에 기록한다.
- 거부 경로 2종: 위조된 nested Lease 서명, 만료된 Lease.

### Out (명시하지 않으면 범위가 샌다)
- `RenewLeaseRequest` 왕복 및 만료 연장
- `RenewLeaseResult` 정책 처리, Lease revoke
- 여러 Agent 동시 처리
- scheduler · queue · capacity allocation
- Coordinator control store, term/epoch 영속화, HA
- Job 실행 · GPU 할당 · checkpoint
- durable `FenceWatermark`(재시작 후 stale Lease 차단은 증명하지 않는다)
- TLS · 원격 네트워크 · crash recovery
- 운영용 key protection(여전히 테스트 전용 K0)

## 단계

| # | 단계 | 스트림 | 완료 기준 | 상태 |
|---|---|---|---|---|
| 1 | `CoordinatorConfig` 에 lease 관련 필드 추가, `issue_lease()`/`issue_grant()` 확장 | Coordinator | `cargo build -p gputeer-coordinator` 통과 | ✅ 2026-08-18 |
| 2 | Agent 가 nested Lease 를 `verify()` 로 독립 검증 + `FenceWatermark` 기록 | Agent | `cargo build -p gputeer-agent` 통과, `gputeer-runtime-policy` 의존성 추가 | ✅ 2026-08-18 |
| 3 | `coordinator-agent-selftest` 에 정상 경로 lease 포함 검증 + 거부 경로 2종 추가 | CLI | 6개 시나리오 전부 통과, 5회 연속 확인 | ✅ 2026-08-18 |
| 4 | 뮤테이션 테스트로 비공허성 증명 | — | Lease 검증을 무력화하면 시나리오 5·6 이 실제로 실패 | ✅ 2026-08-18 |

## 완료 기준 (DoD)

- [x] `gputeer coordinator-agent-selftest` 가 6개 시나리오(정상 1 +
      거부 경로 5) 전부 통과한다 — 5회 연속 확인.
- [x] **negative test**: 위조된 nested `Lease` 서명 시 Agent 가
      `LEASE_REJECTED:` 로 거부한다 — outer Grant 검증만으로는 못
      잡는다는 것을 뮤테이션 테스트로 증명했다.
- [x] **negative test**: 만료된 `Lease` 를 Agent 가 거부한다
      (`Lease::LIFETIME == LongLived` 의 만료 검사).
- [x] `docs/evidence/` 에 schema v2 형식으로 기록 — `docs/evidence/DoD-12_coordinator_agent_lease_최소_조각.md`(2026-08-19, 독립 검수 `ACCEPTED`)

## 실제 구현 메모 (2026-08-18)

- `crates/coordinator/src/lib.rs::issue_lease()` — `Lease` 를 만들어
  **독립적으로 서명**한다. `corrupt_lease_signature` 는 서명 **후**
  마지막 바이트를 뒤집는다 — outer `Grant` 서명 계산에는 nested
  `Lease.coordinator_signature` 가 들어가지 않으므로(규칙 i), 이
  위조는 outer 서명을 깨지 않는다.
- `crates/agent/src/lib.rs::verify_and_record_lease()` — Grant 의
  replay 검사를 통과한 뒤에만 호출된다. `gputeer_protocol::verify()`
  로 nested Lease 를 독립 검증하고, **서명 검증이 끝난 뒤에만**
  attempt_id/issuing_coordinator_id/holder_node_id/job_id 상관관계를
  검사한다 — 서명 안 된 필드를 먼저 믿고 분기하지 않는다.
- `Ed25519Verifier::new(coordinator_keys)` 와 outer Grant 검증에 쓰던
  것과 같은 `InMemoryReplayGuard` 를 그대로 재사용한다 — `Lease` 는
  `LongLived` 라 replay 검사 대상이 아니므로(§10) 안전하게 공유할 수
  있다.
- `crates/agent/Cargo.toml` 에 `gputeer-runtime-policy` 의존성 추가
  — `FenceWatermark` 를 쓰기 위해서다. 계획 설계 시점엔 "이번
  구현자의 의도와 맞는지 확인 안 됨"으로 남겨 뒀는데, 실제로
  추가해 보니 자연스러웠다(Agent 프로세스 로컬 상태로만 쓴다 —
  durable 하지 않다는 한계는 `FenceWatermark::is_durable() == false`
  로 이미 정직하게 표현되어 있다).
- `crates/cli/src/coordinator_agent_selftest.rs` — 기존 4개 시나리오
  (정상·위조 Grant·위조 ACK·replay) 뒤에 5·6번을 추가했다. 위조
  Lease 시나리오는 **Agent 쪽 실패만** 판정 기준으로 삼는다 —
  Coordinator 는 outer Grant 를 정상 서명하므로 자기 자신은 성공을
  주장할 수 있다(그것이 바로 이 시나리오가 증명하려는 것 —
  "outer 검증만으로는 부족하다").
- 뮤테이션 테스트: `verify_and_record_lease()` 호출을 `if false { }`
  로 무력화 → 시나리오 5 가 정확히 예상대로 실패("위조된 nested
  Lease 서명이 거부되지 않았다 — outer Grant 검증만으로는 이 결함을
  잡지 못한다는 뜻이다") → 원복 후 6개 시나리오 전부 재통과 확인.

## 기준선과 다른 점

없음 — 이 계획은 기존 계획(`2026-08-18_0800`)의 "Out" 절이 이미
예고한 다음 한 걸음을 그대로 이행했다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-18 18:00 | 코덱스 설계(`p67`) 정리 |
| 2026-08-18 19:00 | claude-code 세션이 구현·실측·뮤테이션 테스트까지 완료. "실제 구현 메모" 절 추가 |
