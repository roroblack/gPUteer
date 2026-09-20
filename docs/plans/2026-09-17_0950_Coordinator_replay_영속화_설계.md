# 2026-09-17_0950_Coordinator replay 영속화 설계 (결함 88)

- **기준선:** `docs/protocol/signing.md` §10(Replay 캐시 — 로컬 SQLite · 재시작을 넘는다 · nonce 는 CSPRNG 16바이트 MUST)
- **대상 단계:** v0.1
- **선행 게이트:** 계약 제안(아래 "계약 변경") 승인 — wire 규칙이 바뀐다
- **결함 기록:** `docs/reports/debugs/2026-09-10_0900_검수가_찾은_결함_5건.md` 결함 88(재검수 59) — Hello replay 가 Coordinator 재시작을 넘지 못한다
- **성격:** 설계 조사 · 계획. **코드는 아직 바꾸지 않았다**

## 1. 무엇이 문제인가

Coordinator `run()` 이 `InMemoryReplayGuard` 를 쓴다(`crates/coordinator/src/lib.rs` run). 유효시간 안에 Coordinator 를 재시작하면 같은 Hello
바이트가 다시 수락된다. Hello 만의 문제가 아니다 — ACK · RenewLeaseRequest · NodeHeartbeat · NeighborUnreachableReport 등 **모든 단수명 메시지**가 같다.

`crates/crypto/src/durable_replay.rs` 에 `DurableReplayGuard`(SQLite · 재시작을 넘는다 · 서명자별 quota · 되감김 중 GC 정지)가 **이미 있다.**
그래서 "저장소를 만든다" 는 일이 아니다. **바꿔 끼우면 무엇이 깨지는가** 가 이 계획의 본체다.

## 2. 조사 — 그대로 바꿔 끼우면 깨지는 것 (2026-09-17 코드 대조)

```text
메시지                  nonce 생성 (파일)                                           재시작 뒤
AgentSessionHello       fresh_nonce() CSPRNG (agent lib.rs)                           안전
AgentGrantAck           derive_nonce("grant-ack", grant_id, connection_attempt)        충돌 ★
                        Coordinator 가 그 유도값과 같은지 **검사한다**(coordinator lib.rs "ACK nonce does not match connection attempt")
ExecutionGrant          derive_nonce("grant", config.grant_id, connection_attempt)     충돌(Agent 쪽 guard 도 같은 문제)
RenewLeaseRequest       derive_renew_nonce(lease_id, connection_attempt, round)        충돌 ★
NodeHeartbeat           derive_nonce("node-heartbeat", lease_id:round, connection_attempt)  충돌
multi_agent Hello · ACK derive_nonce("multi-agent-hello" · "multi-agent-ack", …, 0)     충돌
```

- `grant_id` 는 Coordinator 설정값(`--grant-id`)이라 **연결마다 같다**. `connection_attempt` 는 프로세스 안 카운터라 **재시작하면 0 으로 돌아간다.**
  그래서 DoD-24(프로세스 재시작 뒤 같은 lease-db 로 Lease 복원)의 **정상 경로**가 영속 guard 에서 Duplicate 로 거부된다.
- ★ 이 유도 nonce 들은 **§10 의 "CSPRNG 16바이트 MUST" 와 이미 어긋난다.** 유도는 DoD-35 가 재접속마다 nonce 를 가르려고 들였다 —
  replay 를 막는 도구(무작위)와 "이 ACK 가 이 연결의 것" 을 묶는 도구(유도)를 한 칸에 겹쳐 쓴 것이다.

## 3. 선택지

```text
A  연결 카운터를 영속화한다(재시작 뒤에도 connection_attempt 가 이어진다)
   얻음  유도 규칙 · 기존 테스트가 그대로다
   잃음  Agent 도 재시작하면 카운터가 0 — 두 쪽 카운터를 맞출 수 없다. §10 MUST 위반도 그대로다
B  Grant 마다 grant_id 를 새로 만든다(설정값 대신 영속 순번 · 무작위)
   얻음  ACK · Grant 충돌이 사라진다
   잃음  Renew · Heartbeat 충돌은 남는다(lease_id · round 기반). §10 MUST 위반도 그대로다
C  단수명 메시지의 nonce 를 전부 CSPRNG 로 · "이 연결의 것" 결합은 **서명된 다른 칸**으로 한다    ← 권장
   ACK      nonce 무작위. 결합은 ACK 의 grant_id(이미 서명 대상) + Grant 의 nonce 를 ACK 에 되돌리는 새 칸(예: grant_nonce)
   Renew · Heartbeat  nonce 무작위. 회차 구분은 replay guard 가 이미 한다(무작위라 회차마다 다르다)
   Grant    nonce 무작위(Coordinator). Agent 쪽 guard 도 영속화할 수 있게 된다
   얻음  §10 MUST 를 지킨다 · 재시작을 넘는 replay 방어를 양쪽에 켤 수 있다 · 유도 충돌 부류가 사라진다
   잃음  **계약 변경**(AgentGrantAck 에 칸 추가 · 검증 규칙 교체 · canonical 벡터) · selftest 의 유도 nonce 전제(재접속 · replay 시나리오) 손질
```

## 4. 권장 — C, 세 조각으로

| # | 조각 | 완료 기준 | 상태 |
|---|---|---|---|
| 1 | 계약 제안 — AgentGrantAck.grant_nonce(가칭) 순수 추가 · ACK 결합 규칙 교체 · §10 에 "유도 nonce 금지" 명시 · 벡터 | 제안 승인 | ⬜ |
| 2 | Agent · Coordinator 가 단수명 nonce 를 CSPRNG 로 · ACK 결합을 새 칸으로 · selftest 전제 손질 | 기존 테스트 통과 · 재접속 시나리오 유지 | ⬜ |
| 3 | Coordinator `DurableReplayGuard`(경로: `--replay-db`, 영속 lease 저장소를 쓰는 구성에서는 필수) · 1분 GC · 음성 테스트(재시작 뒤 같은 Hello · ACK 바이트 거부) | 음성 테스트 · 뮤테이션 | ⬜ |

★ 3 만 먼저 하면(바꿔 끼우기) DoD-24 재시작 복원이 깨진다 — **순서를 바꾸지 않는다.**

## 5. 하지 않는 것

- 증거 · 장수명 메시지(AttemptReport · JobManifest)의 replay — §10 대상이 아니다(`ReplayStatus::NotApplicable`). 소비 측 멱등성으로 다룬다
- DB 파일 삭제 · 바꿔치기 방어 — `DurableReplayGuard` 도 못 막는다고 §10.2 가 적었다
- Agent 쪽 replay guard 영속화 — 조각 2 뒤에 가능해진다. 이 계획에서는 가능성만 적는다

## 6. 확인하지 못한 것

- selftest 97 시나리오 중 유도 nonce 를 **값으로** 전제하는 곳의 전수 목록(grep 으로 `derive_nonce` 호출부만 봤다)
- canonical 벡터 · 참조 구현(`tools/canonical`)에 필요한 변경 범위

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-09-17 | 최초 작성(설계 조사 — 코드 무변경) |
