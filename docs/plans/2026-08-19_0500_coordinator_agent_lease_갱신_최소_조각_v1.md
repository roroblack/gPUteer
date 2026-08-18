# 2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1

- **기준선:** `docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md`
  (Grant 안에 서명된 Lease 를 실어 보내고 Agent 가 독립 검증하는
  조각 — 구현·독립 검수·evidence 기록(`DoD-12`) 완료). 그 계획의
  "Out" 절이 명시적으로 남겨 둔 lease **갱신** 쪽 첫 걸음.
- **대상 단계:** v0.1
- **선행 게이트:** 없음(기존 coordinator/agent 핸드셰이크 + Lease
  최소 조각 위에 얹는다)

★ 이 문서는 코덱스(`agent:codex-cli`, read-only 샌드박스)의 설계
응답(`p98` 프롬프트, 재시도 성공)을 정리한 것이다. **구현은 아직
착수하지 않았다.**

## 왜 지금 이 조각인가

`DoD-12`(Lease 최소 조각)는 Coordinator 가 서명된 `Lease` 를 발급
하고 Agent 가 독립 검증하는 것까지만 증명했다 — `RenewLeaseRequest`
왕복은 범위 밖으로 남겼다. `proto/lease.proto` 에
`RenewLeaseRequest`/`RenewLeaseResult`/`RenewOutcome` 이 이미
정의돼 있고 `RenewLeaseRequest` 는 이미 `Signable`(`ShortLived`,
domain_tag `gputeer/v1/lease-renew`)이지만, `crates/coordinator`/
`crates/agent` 는 이것을 전혀 다루지 않는다.

## ★ 핵심 발견 — `RenewLeaseResult` 는 서명 대상이 아니다

설계 실측에서 가장 중요한 결과다.

```text
RenewLeaseRequest   schema_version·lease_id·fence_epoch·node_id·
                     progress·issued_at_unix_ms·nonce·
                     node_signature(90)   <- 이미 Signable
                     (proto/lease.proto:54-68,
                      crates/protocol/src/signable.rs:211-245,
                      crates/protocol/src/to_fields.rs:495-511)

RenewLeaseResult     outcome·lease·detail·retry_after_ms
                     서명 필드 없음, Signable 없음
                     (proto/lease.proto:80-102)
```

프레이밍도 요청만 지원한다 — `FrameType::LeaseRenew`(요청)만
있고 결과용 frame type/`IngressMessage` variant 는 없다
(`crates/crypto/src/framed_ingress.rs:74-85,155-165,259-270`).

**결과 메시지(`RENEWED`/`SUPERSEDED`/`QUARANTINED` 같은 정책 판정
과 새 Lease)가 인증되지 않는다면, Agent 가 그 판정을 신뢰할
근거가 없다.** 새 `Lease` 자체는 nested 서명으로 독립 검증할 수
있지만("`RENEWED`" 성공 경로만), `SUPERSEDED`/`QUARANTINED` 같은
**서명된 정책 거부**는 `RenewLeaseResult` 자체가 서명되지 않으면
누구나 위조할 수 있다 — 공격자가 정상 갱신 요청에 가짜
`QUARANTINED` 응답을 끼워 넣어 정당한 Agent 의 작업을 강제
중단시킬 수 있다.

→ **이 조각의 In 범위에 `RenewLeaseResult` 를 `Signable` 로
만드는 작업을 포함한다.**

## 범위

### In

- `RenewLeaseResult` 에 인증 메타데이터 추가 —
  `schema_version`(5)·`coordinator_id`(6)·`issued_at_unix_ms`(7)·
  `request_nonce`(8, 요청 nonce 를 echo — 결과가 다른 갱신 요청에
  재사용되는 것을 막는다)·`coordinator_signature`(90). 새
  domain_tag(예: `gputeer/v1/lease-renew-result`).
- `crates/crypto/src/framed_ingress.rs` 에 `FrameType::LeaseRenewResult`/
  `IngressMessage::LeaseRenewResult` 추가.
- 같은 TCP 연결에 이어서(별도 연결을 새로 열지 않는다) 왕복 추가:

  ```text
  Coordinator -> ExecutionGrant
  Agent       -> AgentGrantAck
  Agent       -> RenewLeaseRequest   (서명됨)
  Coordinator -> RenewLeaseResult    (서명됨, 새 Lease 를 담을 수 있다)
  ```

- Agent 가 결과를 검증하는 절차(전부 서명 확인 **후**):
  1. `RenewLeaseResult` 서명 검증 + `request_nonce` 가 자신이 보낸
     요청의 nonce 와 일치하는지 확인(replay 응답 거부)
  2. `outcome == RENEWED` 면 nested 새 `Lease` 를 **독립적으로**
     재검증(규칙 i — 결과 서명이 유효해도 nested Lease 서명은
     따로 위조될 수 있다)
  3. `lease_id`/`job_id`/`attempt_id`/holder·coordinator 상관관계
     재확인(기존 Lease 최소 조각과 같은 패턴)
  4. 새 `fence_epoch` 가 현재 보유 epoch 보다 **낮으면 거부**,
     **같으면 허용**(갱신은 같은 epoch 유지 —
     `same_epoch_reuse_is_allowed_by_design` 계약과 일관), **높으면
     이 조각에서는 정책상 거부**(epoch 상승은 범위 밖)
  5. 모든 검증을 통과한 뒤에만 `FenceWatermark.check_and_advance()`
     호출 + 보유 Lease 교체
- `outcome == SUPERSEDED`/`QUARANTINED` — **서명된 정상 정책
  거부**로 분류해 처리(Lease·watermark 변경 없이
  `RENEW_REFUSED:<outcome>` 로 종료) — 서명/epoch 오류로 인한
  거부(`RENEW_REJECTED:<reason>`)와 구분한다.
- 거부·공격 경로 6종을 selftest 에 추가:

  | 시나리오 | 판정 |
  |---|---|
  | 위조된 `RenewLeaseRequest.node_signature` | Coordinator ingress 검증 실패 |
  | 위조된 `RenewLeaseResult.coordinator_signature` | Agent 결과 검증 실패 |
  | 새 Lease 의 nested 서명만 위조 | 결과 서명은 통과하지만 Lease 독립 검증 실패 |
  | 낮은 `fence_epoch` | Agent watermark 거부 |
  | `RENEW_OUTCOME_SUPERSEDED` | 서명된 정상 정책 거부로 분류 |
  | `RENEW_OUTCOME_QUARANTINED` | 서명된 정상 정책 거부로 분류 |

  `SUPERSEDED`/`QUARANTINED` 를 실제로 **결정하는** Coordinator
  정책은 Out 이다 — selftest 는 test-only 플래그로 결과를
  주입해 **전송·검증·분류**만 증명한다.

### Out (명시하지 않으면 범위가 샌다)

- 여러 번 반복 갱신(이 조각은 1회 왕복만 증명한다)
- 갱신 주기 타이머 · 자동 retry/backoff
- 별도 재접속·failover(같은 TCP 연결만 다룬다)
- Coordinator 의 실제 Lease 재발급 정책(어떤 조건에서
  `SUPERSEDED`/`QUARANTINED` 를 내리는지)
- `max_total_duration_seconds` 정책 집행
- 모든 `RenewOutcome` 케이스의 운영 의미 정의
- Lease revoke
- durable `FenceWatermark`(재시작 후 stale epoch 차단은 여전히
  증명하지 않는다)
- 다중 Agent · scheduler · ControlStore · HA · TLS
- epoch **상승**(fence_epoch 증가) 처리 — 이번엔 같은 epoch 갱신만

## 왜 같은 TCP 연결에 이어 붙이는가

| | 같은 연결에 이어 붙임(채택) | 별도 연결 |
|---|---|---|
| 장점 | 새 listener/port/accept 불필요. Grant 가 승인된 같은 세션에서만 갱신 요청을 받는다. 기존 selftest 프로세스 구조·결정적 오케스트레이션을 그대로 재사용. 연결 단위 read/write timeout 재사용 | 실제 RPC 서비스에 더 가깝다. 재접속·부하분산에 유리 |
| 단점 | 연결이 갱신 왕복까지 살아 있어야 한다. 재접속·failover 를 검증하지 못한다 | 새 연결의 인증·세션 바인딩·listener 수명·재시도 정책까지 추가돼 이 최소 조각의 범위를 크게 늘린다 |

기존 계획도 갱신·다중 Agent·운영 정책을 Out 으로 남겼다
(`docs/plans/2026-08-18_1800_...md:32-41`) — 이 조각도 같은
최소주의를 유지한다.

## FenceWatermark 재사용

현재 Agent 는 초기 Grant 의 Lease 를 검증한 뒤
`watermark.check_and_advance(&lease.job_id, lease.fence_epoch)`
를 부른다(`crates/agent/src/lib.rs:164-200` 근방). 갱신된 Lease
도 **같은 `job_id` 를 resource key 로 재사용**한다 — `lease_id`
를 새 키로 쓰면 기존 watermark 와 분리되어 강등 방어가 깨진다.

```text
기존 watermark = 5, 기존 Lease = epoch 5
갱신 Lease = epoch 5  -> 허용 (same_epoch_reuse_is_allowed_by_design)
갱신 Lease = epoch 4  -> 거부 (강등 시도)
갱신 Lease = epoch 6  -> 이 조각에서는 정책상 거부(epoch 상승은 범위 밖)
```

`crates/runtime-policy/src/lease_scope.rs` 의 `check_and_advance()`
는 `<` 만 거부하므로(`:79-94`) 같은 epoch 는 이미 허용된다 —
`same_epoch_reuse_is_allowed_by_design` 테스트(`:198-206`)가 이
계약을 이미 고정하고 있다. 새 코드가 필요 없다.

## 키·시드

기존 패턴을 그대로 재사용한다 — 새 키가 필요 없다.

- selftest 의 시드는 label hash 로 결정적 생성(`crates/cli/src/coordinator_agent_selftest.rs:44-57`)
- 양쪽은 peer public key 를 `InMemoryKeyring` 에 등록
  (`coordinator/src/lib.rs:92-96`, `agent/src/lib.rs:50-62`)
- Coordinator 는 `Lease` 와 새 `RenewLeaseResult` 를 서명하고,
  Agent 는 `RenewLeaseRequest` 를 서명한다.

새로 필요한 것: 갱신 Request 용 별도 nonce namespace, Result 검증
용 새 domain, Result 의 request nonce echo. 결정적 nonce 는
selftest 전용이다 — `derive_nonce()` 의 기존 경고(같은 `grant_id`
재사용 시 nonce 재현 가능, `crates/coordinator/src/lib.rs:275-288`
근방)가 여기도 적용된다.

## 단계

| # | 단계 | 스트림 | 완료 기준 | 상태 |
|---|---|---|---|---|
| 1 | `RenewLeaseResult` 인증 메타데이터 + `ToCanonicalFields`/`Signable`/새 domain 구현 | Protocol | `cargo build -p gputeer-protocol` 통과, field_number_audit·lifetime_consistency·schema_fingerprint 전부 등록됨, canonical vector 추가 | ⬜ |
| 2 | `framed_ingress` 에 `FrameType::LeaseRenewResult`/`IngressMessage::LeaseRenewResult` 추가 | Crypto | 기존 framed_ingress 테스트 전부 green + 새 타입 round-trip 테스트 | ⬜ |
| 3 | Coordinator 에 같은 연결 위 갱신 왕복 추가(Request 검증 → 동일 epoch 새 Lease 발급 → Result 서명·송신) | Coordinator | `cargo build -p gputeer-coordinator` 통과 | ⬜ |
| 4 | Agent 에 갱신 처리 추가(Result 서명·nonce echo 검증 → nested Lease 독립 검증 → watermark 적용 → 보유 Lease 교체) | Agent | `cargo build -p gputeer-agent` 통과, `FenceWatermark` 재사용 확인 | ⬜ |
| 5 | `coordinator-agent-selftest` 시나리오 확장(정상 갱신·Request 위조·Result 위조·nested Lease 위조·epoch 강등·SUPERSEDED·QUARANTINED) | CLI | 시나리오 전부 자동 판정, 5회 연속 통과 | ⬜ |
| 6 | 뮤테이션 테스트로 비공허성 증명(각 검증 gate 최소 1건) | — | 무력화 시 대응 시나리오가 실제로 실패, 원복 후 재통과 | ⬜ |
| 7 | 코덱스 독립 검수 1라운드 이상 | — | `ACCEPTED` | ⬜ |
| 8 | `docs/evidence/` 에 schema v2 형식으로 기록 | — | 독립 검수 `ACCEPTED` (DoD-11/12 와 같은 절차 — 신규 작성이니 v1 단계 없이 바로 v2) | ⬜ |

## 완료 기준 (DoD)

- [ ] `RenewLeaseResult` 가 서명되고, 위조 시 Agent 가 거부한다.
- [ ] **negative test**: 위조된 `RenewLeaseRequest.node_signature` 를
      Coordinator 가 거부한다.
- [ ] **negative test**: 위조된 `RenewLeaseResult.coordinator_signature`
      를 Agent 가 거부한다.
- [ ] **negative test**: 결과 서명은 유효하나 nested 새 `Lease` 서명만
      위조된 경우 Agent 가 독립 검증으로 거부한다(outer 검증만으로는
      안 잡힌다는 것을 뮤테이션으로 증명).
- [ ] **negative test**: 낮은 `fence_epoch` 갱신을 `FenceWatermark`
      가 거부한다.
- [ ] `SUPERSEDED`/`QUARANTINED` 가 서명된 정상 정책 거부로
      올바르게 분류된다(Lease·watermark 불변, `RENEW_REFUSED:` 로
      종료).
- [ ] `gputeer coordinator-agent-selftest` 확장 시나리오가 5회
      연속 통과한다.
- [ ] `docs/evidence/` 에 schema v2 형식으로 기록.

## 이 조각이 결정하지 않는 것 — 최종 권고 재확인

★ 코덱스 설계의 마지막 권고: **`RenewLeaseResult` 를 unsigned
payload 로 억지로 TCP 에 실어 보내지 않는다.** 만약 구현 단계에서
결과 서명 추가가 예상보다 크다고 판단되면(예: 다른 안전망과의
충돌), 이 조각을 다시 좁혀 **`RENEWED` 성공과 nested Lease
검증만** 구현하고 `SUPERSEDED`/`QUARANTINED` 를 "인증된 정책
결과"라고 주장하지 않은 채 다음 조각으로 미룬다 — 정직하지 않은
채로 서명 안 된 정책 거부를 신뢰하는 코드를 만들지 않는다.

## 기준선과 다른 점

없음 — 이 계획은 기존 계획(`2026-08-18_1800`)의 "Out" 절이 이미
예고한 다음 한 걸음을 그대로 이행한다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-19 05:00 | 코덱스 설계(`p98`, 1차 시도 crash 후 재시도 성공) 정리 — 구현 착수 전 |
