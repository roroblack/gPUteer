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
ExecutionGrant          derive_nonce("grant", config.grant_id, connection_attempt)     충돌(Agent 쪽 guard 도 같은 문제 · Agent 가 유도값을 검사한다)
RenewLeaseRequest       FRESH 연결 안 갱신   derive_renew_nonce(lease_id, connection_attempt, round)   충돌 ★
                        5b RENEW 연결        Hello · 요청 모두 fresh_nonce() (agent lib.rs RENEW 세션)   안전
NodeHeartbeat           derive_nonce("node-heartbeat", lease_id:round, connection_attempt)  충돌
NeighborUnreachableReport derive_nonce("neighbor-unreachable", lease_id:round, connection_attempt)  충돌   ★ 검수 67 전에는 표에 없었다
REPORT 세션 Hello       fresh_nonce()                                                  안전
multi_agent Hello · ACK derive_nonce("multi-agent-hello" · "multi-agent-ack", …, 0)     충돌 — Hello 유도값은 Coordinator 가 검사하지 않는다
```

★ 검수 67 (결함 142) 정정 — 전에는 RenewLeaseRequest 를 한 줄로 합치고 NeighborUnreachableReport 를 빠뜨렸다. 표는 **메시지 · 발신 경로별**이다.
  조사 방법: agent · coordinator 의 `derive_nonce(` · `derive_renew_nonce(` · `fresh_nonce()` 호출부 grep(2026-09-17). 매크로 · 테스트 전용 경로는 보지 않았다.

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
D  B + 단수명 nonce 전부 CSPRNG — **새 proto 칸 없이** ACK 를 발급에 묶는다(검수 67 · 결함 141 이 제안)
   ACK      nonce 무작위. 결합은 ACK 가 이미 서명하는 grant_id 와 Coordinator 가 이번에 보낸 Grant 의 id 대조(이미 있다) — grant_id 가 발급마다 다르면 충분하다
   얻음  §10 MUST · 새 칸 없음(proto · canonical 벡터 불변)
   잃음  ★ grant_id 는 `start_checkpoint_id(job_id, attempt_id, grant_id)` 의 입력이다 — 발급마다 바꾸면 **재접속마다 시작 체크포인트 자리가 바뀐다**
         (selftest 43 · 재개 경로). 그 의존을 먼저 끊어야 한다. 유도값 검사 교체는 wire 규칙 변경이라 옛 Agent 와의 호환 검토는 C 와 같이 필요하다
         (새 Agent 의 무작위 ACK 를 옛 Coordinator 가 거부한다)
```

★ 검수 67 (결함 141) 정정 — 전에는 "새 ACK 칸이 영속화의 필수 선행 조건" 이라고 적었다. **근거가 모자랐다** — D 가 새 칸 없이 같은 결합을 준다.
  C 와 D 중 무엇이 나은지는 grant_id 의 체크포인트 의존을 끊는 비용과 새 칸의 계약 비용을 비교해 정한다 — 이 계획은 아직 고르지 않는다.

## 4. 권장 — 조각 0 을 먼저, 그 뒤 C 또는 D 로

★★ **2026-09-21 결정 — C 로 간다**(사용자 지시로 코덱스와 논의 · 논의 71, 읽기 전용). 근거와 조건:

```text
왜 C 인가   D 의 "새 칸 없음" 은 싸 보이지만, grant_id 를 **안정된 식별자**에서 발급마다 바뀌는 값으로
            바꾸는 비용을 이 범위의 코드만으로는 다 셀 수 없다(체크포인트 의존 호출부가 이 파일들 밖에 있다).
            호환성은 C 와 D 가 같다 — 어느 쪽이든 새 Agent 의 무작위 ACK 를 옛 Coordinator 가 거부한다
            (`coordinator/src/lib.rs` 의 ACK 유도 nonce 대조). C 는 계약 변경이 **눈에 보이고**,
            D 는 기존 식별자의 의미를 조용히 바꾼다 — 뒤엣것이 나중에 찾기 어렵다
지금 안 한다 C 를 바로 전면 구현하지 않는다. 계약 제안(칸 추가) · 벡터 · 혼합 버전 규칙을 먼저 확정한다.
            D 는 체크포인트 호출부 전수 확인 전까지 보류다(기각이 아니다)
```

★★ **조각 0 의 범위를 좁힌다 — "guard 변수 하나 교체" 가 아니다**(같은 논의). 지금 `run()` 은 guard 를 하나만 만들어
   **Hello · ACK · Renew 가 같은 객체를 함께 쓴다.** 그 하나를 영속 guard 로 바꾸면 조각 0 이 아니라 전체 교체가 되고,
   계획이 경고한 DoD-24 충돌이 그대로 난다. 그래서 조각 0 은 이렇게 한다:

```text
바꾼다      순차 일반 경로의 read_session_hello 에만 **별도** durable guard 를 배선한다
안 바꾼다   ACK · Renew · Heartbeat 는 기존 in-memory guard · 명시적 Resume 경로도 이번에는 그대로 둔다
막는다      multi_agent lane 에서 --replay-db 가 **말없이 무시되지 않게** 한다(거부하거나 lane 관문을 둔다)
깨지지 않는다  DoD-24 재시작 복원 — 정상 Agent 는 fresh Hello 를 보내므로 통과한다.
            거부되는 것은 재시작 전과 **바이트가 같은** Hello 뿐이다
```

★ 검수 67 (결함 141) — **부분 개선과 전체 교체의 선행 조건을 가른다.** 조각 0 은 계약과 무관하다. 조각 1~3 은 C 로 적었고, D 를 고르면 1 이
  "grant_id 발급마다 · start_checkpoint_id 의존 끊기" 로 바뀐다.

| # | 조각 | 완료 기준 | 상태 |
|---|---|---|---|
| 0 | ✅ **2026-09-23 됐다** — `--hello-replay-db`. 순차 lane 의 일반 Hello 를 읽은 **직후** 영속 guard 로 한 번 더 본다(메모리 guard 는 그대로 둔다). 다중 Agent · Resume lane 과 함께 주면 시작을 거부한다(그 lane 의 Hello 는 이 방어를 거치지 않는다). 시험 셋 · 뮤테이션 1건. 아래는 계획 당시의 서술이다. **일반 경로 Hello 만** 영속 guard 로 검사(`--replay-db` 가 있을 때) — Hello 는 이미 fresh_nonce 이고 검사 진입점이 따로다. 나머지 메시지는 기존 in-memory guard. multi_agent lane 은 Hello 가 유도값이라 그 lane 의 Agent 를 fresh_nonce 로 바꾸거나 이 조각에서 빼야 한다 | 재시작 뒤 같은 Hello 바이트 거부 음성 테스트 · DoD-24 재시작 복원 유지 | ⬜ |
| 1 | 계약 제안 — AgentGrantAck.grant_nonce(가칭) 순수 추가 · ACK 결합 규칙 교체 · §10 에 "유도 nonce 금지" 명시 · 벡터 | 제안 승인 | ⬜ |
| 2 | Agent · Coordinator 가 단수명 nonce 를 CSPRNG 로 · ACK 결합을 새 칸으로 · selftest 전제 손질 | 기존 테스트 통과 · 재접속 시나리오 유지 | ⬜ |
| 3 | Coordinator `DurableReplayGuard`(경로: `--replay-db`, 영속 lease 저장소를 쓰는 구성에서는 필수) · 1분 GC · 음성 테스트(재시작 뒤 같은 Hello · ACK 바이트 거부) | 음성 테스트 · 뮤테이션 | ⬜ |

★ 3 만 먼저 하면(바꿔 끼우기) DoD-24 재시작 복원이 깨진다 — **순서를 바꾸지 않는다.** 조각 0 은 guard 를 **Hello 에만** 쓰므로 이 제약 밖이다.

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
| 2026-09-17 | 검수 67 반영 — 조사표를 메시지 · 발신 경로별로(이웃 신고 추가 · 결함 142) · 선택지 D · 조각 0(Hello 만 영속 guard)을 더하고 "새 칸이 필수 선행" 서술을 거뒀다(결함 141) |
| 2026-09-21 | 논의 71(코덱스 · 읽기 전용) 반영 — **C 채택** · 조각 0 의 범위를 "read_session_hello 에만 별도 durable guard" 로 좁히고 multi_agent 의 `--replay-db` 무시 금지를 더했다 |
