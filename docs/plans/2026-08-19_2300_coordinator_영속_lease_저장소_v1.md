# 2026-08-19_2300_coordinator_영속_lease_저장소_v1

- **기준선:** `docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md`
  (Agent 쪽 durable FenceWatermark 완료, `DoD-14`). 그 계획의 "Out"
  절이 명시적으로 남긴 항목 — Coordinator 의 영속 Lease 저장소.
- **대상 단계:** v0.1
- **선행 게이트:** 없음. `docs/plans/2026-08-19_2330_같은_연결_반복_lease_갱신_v1.md`
  (반복 갱신)과는 독립적이나, 구현 순서는 반복 갱신을 먼저 진행한다
  (범위가 더 작고 의존성이 적다).
- **상태:** 구현·뮤테이션 테스트 완료. 코덱스 독립 검수 대기 중.

★ 이 문서는 코덱스(`agent:codex-cli`, read-only 샌드박스)의 설계
응답(`p106` 프롬프트)을 정리한 것이다.

## 왜 지금 이 조각인가

Agent 쪽은 이미 `DurableFenceWatermark` 로 재시작을 넘는 방어를
갖췄지만(`DoD-14`), Coordinator 는 Lease 상태를 **전혀 저장하지
않는다** — 매 실행이 CLI 인자(`--fence-epoch`·`--lease-id`·
`--job-id`)로만 상태를 구성하고 끝나면 사라진다.
`crates/coordinator/src/lib.rs:56-58, 73-86, 445-466` 확인.
Coordinator Cargo 의존성에도 SQLite/`runtime-policy` 가 없다
(`Cargo.toml:8-11`).

## 핵심 결정 — `DurableFenceWatermark` 를 재사용하지 않는다

`DurableFenceWatermark` 는 `resource -> 최대 epoch` 하나만 저장하는
좁은 타입이다. Coordinator 는 **Lease 전체 신원**(lease_id·job_id·
attempt_id·holder_node_id·fence_epoch·expires_at 등, `proto/lease.proto:17-44`)
을 복원해야 하므로 별도의 `CoordinatorLeaseStore` 를 새로 만든다.
SQLite 연결·트랜잭션·PRAGMA·에러 매핑 **패턴만** 재사용한다
(`durable_lease_scope.rs:59-104, 124-163` 참조).

### 저장 스키마(제안)

```sql
CREATE TABLE coordinator_leases (
    lease_id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    holder_node_id TEXT NOT NULL,
    fence_epoch BLOB NOT NULL,              -- u64 big-endian (DurableFenceWatermark 와 같은 이유)
    expires_at_unix_ms BLOB NOT NULL,
    issuing_coordinator_id TEXT NOT NULL,
    coordinator_term BLOB NOT NULL,
    issued_at_unix_ms BLOB NOT NULL,
    renew_after_unix_ms BLOB NOT NULL,
    max_total_duration_seconds BLOB NOT NULL
);
```

`coordinator_signature` 는 저장하지 않는다 — 전송 시 재계산되는
서명 필드다. signing key 영속화는 범위 밖이다.

## 동작 변경(제안)

- **시작·최초 발급**: TCP bind 전에 `--lease-db` 를 연다(fail
  closed, 파일 기반 아니면 거부). `lease_id` 로 기존 레코드를 조회 —
  **없을 때만** CLI 값을 최초 발급 후보로 쓴다. 있으면 저장된 값이
  우선(`--fence-epoch` 는 그 `lease_id` 의 최초 발급값으로만 의미가
  있다). 기존 레코드와 CLI 의 `job_id`/`attempt_id`/holder 가
  다르면 **덮어쓰지 않고** configuration conflict 로 fail closed —
  이미 존재하는 Lease 사실을 훼손하지 않는다(재발급 정책이 아니다).
- **갱신**: `lease_id` 로 저장소 조회 → 없으면 거부(`LEASE_NOT_FOUND`
  성격) → `node_id == holder_node_id`, `request.fence_epoch ==
  stored.fence_epoch` 확인 → identity·epoch 는 그대로, `expires_at`
  등만 트랜잭션으로 갱신 → commit 성공 후에만 저장된 레코드로
  `RenewLeaseResult` 구성·서명. **갱신 결과 epoch 는
  `--renewed-fence-epoch` 가 아니라 저장된 `fence_epoch` 를 쓴다.**
- **저장소에 없는 `lease_id`**: 갱신 경로에서 새 Lease 를 발급하지
  않는다. 현재 프로토콜에는 "unknown lease" 전용 outcome 이 없으므로,
  최소 구현은 signed result 를 추가하지 않고 Coordinator 로컬 오류로
  연결을 종료한다(별도 protocol outcome 은 범위 밖).

## 범위

### In

- Coordinator 의 `--lease-db` 파일 기반 SQLite 저장소
- 발급 Lease 핵심 필드 저장, 갱신 시 저장소의 identity·epoch 조회
- 재시작 후 같은 `lease_id` 의 Lease 복원
- 트랜잭션·commit 실패 시 fail closed, 저장소 open 전에 handshake
  시작 안 함
- 저장소 단위 테스트 + 두 Coordinator 프로세스 재시작 테스트

### Out (명시하지 않으면 범위가 샌다)

- 새 `lease_id` 를 언제 발급할지 결정하는 정책 · `SUPERSEDED`/
  `QUARANTINED` 판정 · Lease revoke
- job 별 최대 epoch·active Lease 선택 정책
- retry/backoff/failover · 다중 Coordinator HA·합의 · 여러 Agent
  동시 처리 · scheduler · 원격 네트워크·TLS
- Lease 이력/audit event store · signing key 영속화
- 기존 `renew_outcome_override` 기반 test-only 플러밍은 남기되, 이
  저장소 조각의 DoD 에는 포함하지 않는다

## selftest 시나리오 설계(제안)

### 시나리오 1 — 발급 상태 복원

1차: `--lease-db <path> --fence-epoch 5 --do-renew true` — 정상 발급
+ 같은 epoch 갱신 성공. 2차(별도 프로세스, 같은 DB):
`--fence-epoch 3 --do-renew false` — 저장소를 쓰면 저장된 epoch=5 를
복원해 Agent 가 성공, CLI epoch=3 을 그냥 썼다면 Agent 의 durable
watermark(5)와 충돌해 실패. **저장소를 실제로 쓰는지 아닌지가 이
시나리오의 성패로 직접 드러난다.**

### 시나리오 2 — 재시작 후 갱신 대조

1차: `--fence-epoch 5 --do-renew false`(발급만). 2차: `--fence-epoch 6
--renewed-fence-epoch 6 --do-renew true` + Agent
`--renew-request-epoch-override 5`. 저장소를 쓰는 구현은 Grant
epoch=5 를 복원해 요청 epoch=5 조회가 성공하지만, 저장소 없는
현재 구현은 CLI epoch=6 과 달라 거부한다.

★ **주의** — Agent 쪽 durable FenceWatermark 설계 때 이미 한 번
"갱신 경로 전용" 시나리오가 최초 검증 뒤에 항상 실행되는 구조 때문에
거짓양성이었던 함정을 겪었다(`docs/plans/2026-08-19_2200_...v1.md`
"★ 구현 중 정정" 절). 여기서도 Grant epoch 와 갱신 epoch 를 **의도적으로
CLI 값과 충돌**시켜야 저장소 사용 여부가 실제로 판별된다 — 단순히
"두 번째 프로세스에서 갱신 성공"만 보면 안 된다.

### 저장소 단위 테스트

`open → insert → drop → open → get` 필드 보존, `renew_existing` 후
`expires_at_unix_ms` 갱신, 같은 `lease_id` 다른 identity 는 overwrite
아닌 conflict, 없는 `lease_id` 갱신은 `NotFound`, `:memory:`/open
실패는 fail closed, `BUSY`/`LOCKED` 와 일반 I/O 오류 구분.

## 단계

| # | 단계 | 완료 기준 | 상태 |
|---|---|---|---|
| 1 | `CoordinatorLeaseStore` 신설(`crates/coordinator/src/lease_store.rs`) + SQLite 스키마 | open/조회/삽입 단위 테스트 | ✅ (7개 단위 테스트) |
| 2 | 최초 발급 경로 전환(레코드 없으면 CLI 값, 있으면 저장값 우선 + conflict 거부) | 재발급 없이 기존 레코드 보존 확인 | ✅ |
| 3 | 갱신 경로 전환(저장소 조회 → identity·epoch 대조 → expires 갱신) | 저장된 epoch 로 결과 구성 | ✅ |
| 4 | fail closed(`:memory:`, open 실패, lock timeout) | 단위 테스트 | ✅ `is_durable()` 검사(Agent 와 같은 관례) — 단, Coordinator 는 `--lease-db` 자체가 optional 이라 아래 "설계와 다른 점" 참조 |
| 5 | selftest 시나리오 20·21 추가 | 5회 연속 통과 | ✅ (21개 시나리오 전체 5회 연속) |
| 6 | 뮤테이션 테스트 + 코덱스 독립 검수 + evidence 기록(DoD-16) | `ACCEPTED` | 🟡 뮤테이션 완료(2건), 검수 진행 중 |

## ★ 구현이 설계와 다른 점 — `--lease-db` 는 필수가 아니라 선택이다

설계(`p106`)는 Agent 의 `--fence-db`(필수, 안 주면 임시 파일 자동
생성)와 대칭으로 Coordinator 도 항상 저장소를 쓰는 모델을 암시했다.
구현 단계에서 **의도적으로 다르게** 했다 — `lease_db_path:
Option<PathBuf>` 로 두고, `None`(기본값, `--lease-db` 안 줌)이면
**이 조각 이전과 완전히 같은 레거시 경로**(`config.fence_epoch` 를
그 실행 동안만 신뢰)를 그대로 쓴다.

이유: Agent 의 `--fence-db` 는 필수로 만들어도 기존 시나리오들이
자동 임시 파일로 회귀 없이 동작했다(매 프로세스가 빈 watermark 에서
시작하는 것이 기존 동작 그 자체였으므로). 그러나 Coordinator 는
**이미 기존 시나리오 19개가 `config.fence_epoch` 정적 비교라는
레거시 계약에 의존**하고 있었다 — 강제로 항상 저장소를 쓰게 하면
이 계약 자체가 사라져 기존 negative test(11·15·16번 등)의 의미가
바뀌거나, 그 시나리오들도 전부 `--lease-db` 를 받도록 다시 손봐야
했다. `Option` 으로 두어 **기존 19개 시나리오는 단 한 줄도 안
건드리고**, 새 시나리오(20·21)만 명시적으로 `--lease-db` 를 켜는
쪽을 선택했다 — 범위를 좁게 유지한다는 이 저장소 전체의 원칙과
일치한다.

## 기준선과 다른 점

없음 — `docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md`
의 "Out" 절이 이미 예고한 다음 한 걸음을 그대로 이행한다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-19 23:00 | 코덱스 설계(`p106`) 정리 — 구현 착수 전, 반복 갱신(DoD-15) 다음 순번으로 대기 |
