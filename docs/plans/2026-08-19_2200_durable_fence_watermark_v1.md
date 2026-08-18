# 2026-08-19_2200_durable_fence_watermark_v1

- **기준선:** `docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md`
  (Lease 갱신 최소 조각 — 구현·독립 검수·evidence 기록(`DoD-13`) 완료).
  그 계획의 "Out" 절이 명시적으로 남겨 둔 항목 중 하나.
- **대상 단계:** v0.1
- **선행 게이트:** 없음(기존 coordinator/agent 핸드셰이크 + Lease
  최소 조각 + Lease 갱신 최소 조각 위에 얹는다)

★ 이 문서는 코덱스(`agent:codex-cli`, read-only 샌드박스)의 설계
응답(`p101` 프롬프트)을 정리한 것이다. **구현은 아직 착수하지 않았다.**

## 왜 지금 이 조각인가

`FenceWatermark`(`crates/runtime-policy/src/lease_scope.rs:57-98`)는
`HashMap<String, u64>` 메모리 상태다. `is_durable() == false` 로
정직하게 표현돼 있지만, 이 한계가 매 evidence 문서(`DoD-12`·`DoD-13`)
의 limitations 절에 "재시작 후 stale epoch 차단은 증명하지 않는다"
로 반복 등장한다.

실제 공격/장애 시나리오:

```text
Agent 프로세스가 fence_epoch=5 로 Lease 를 보유한 채 재시작한다
  (크래시 · 배포 · OS 재부팅 등)
재시작한 Agent 의 FenceWatermark::new() 는 빈 상태로 시작한다
공격자(또는 stale Coordinator)가 fence_epoch=3(이미 폐기된 epoch)
  짜리 예전 Lease 를 재전송한다
check_and_advance("job-x", 3) 는 watermark 가 비어 있으므로
  통과시킨다(0 < 3 이 아니므로 거부되지 않는다)
  -> 강등 방어가 재시작 한 번으로 완전히 무력화된다
```

이 저장소는 같은 문제를 **replay guard** 에서 이미 한 번 풀었다 —
`InMemoryReplayGuard`(재시작 못 넘음)와 `DurableReplayGuard`
(`crates/crypto/src/durable_replay.rs`, SQLite 기반, 재시작을 넘음)
가 나란히 존재하고, `crates/crypto/tests/durable_replay_process.rs`
+ `crates/crypto/src/bin/durable_replay_process_fixture.rs` 가 별도
OS 프로세스 여러 개로 재시작을 넘는 방어를 실측했다.

## 핵심 결정 — `DurableReplayGuard` 를 재사용하지 않는다

**새 `DurableFenceWatermark` 를 `crates/runtime-policy` 에 만들고,
`DurableReplayGuard` 의 SQLite 운용 패턴(연결·트랜잭션·에러 매핑)만
재사용한다.** `DurableReplayGuard` 자체를 억지로 쓰지 않는 이유:

- epoch 를 nonce 처럼 취급하게 된다 — 의미가 다르다.
- fence watermark 는 **같은 값 재사용을 허용**하는데
  (`same_epoch_reuse_is_allowed_by_design`), replay guard 의
  `Duplicate` 의미와 충돌한다.
- replay guard 의 GC 가 watermark 항목을 지우면 stale epoch 가 다시
  통과할 위험이 생긴다 — fence watermark 는 **GC 하면 안 되는** 값이다.
- 저장소 장애(`Io`/`LockTimeout`)와 정책 거부(`Stale`)가 같은
  `ReplayStoreError` 로 섞이면, "공격을 막았다"와 "우리 저장소가
  고장났다"를 구분할 수 없다.

재사용할 것: `Connection::open` · `busy_timeout(1초)` ·
`PRAGMA journal_mode = DELETE`(Windows rollback journal 이유) ·
`PRAGMA synchronous = FULL` · SQLite `BUSY`/`LOCKED` ->
`LockTimeout` 매핑 · `BEGIN IMMEDIATE` 트랜잭션 · commit 성공 뒤에만
`Ok` 반환 · SQLite INTEGER ↔ Rust 정수 변환 시 명시적 오류 처리.
근거: `crates/crypto/src/durable_replay.rs:11-27`(원칙),
`:49-79`(에러 매핑), `:206-246`(연결·PRAGMA), `:416-485`(트랜잭션·commit).

재사용하지 않을 것: nonce 16바이트 검증 · `sender_device_id +
domain_tag + nonce` 복합 키 · `retain_until_ms` · capacity/quota ·
GC 및 clock rollback/jump metadata · `ReplayGuard` trait·
`ReplayDecision`.

## 현재 `FenceWatermark` 계약(그대로 유지해야 한다)

`check_and_advance(resource, epoch)` (`lease_scope.rs:79-94`):

| 현재 watermark | incoming epoch | 결과 |
|---:|---:|---|
| 없음 | 5 | 통과, 5 저장 |
| 5 | 5 | 통과(같은 epoch 갱신 허용) |
| 5 | 3 | `StaleEpoch` 거부 |
| 5 | 6 | 통과, 6 저장 |
| job-a=100 | job-b=1 | 통과(resource 별 독립) |

`<=` 로 바꾸면 정상적인 Lease 갱신까지 거부하게 된다 — durable
버전도 이 표를 정확히 지켜야 한다.

## 제안 API

```rust
// crates/runtime-policy/src/durable_lease_scope.rs
pub struct DurableFenceWatermark {
    connection: rusqlite::Connection,
}

impl DurableFenceWatermark {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DurableFenceError>;
    pub fn is_durable(&self) -> bool;   // true
    pub fn check_and_advance(&mut self, resource: &str, epoch: u64)
        -> Result<(), DurableFenceError>;
}

pub enum DurableFenceError {
    Stale(LeaseScopeViolation),  // 정책상 정상 거부
    Io(String),                  // 저장소 장애 — 공격과 구분한다
    LockTimeout,
}
```

스키마(GC 불필요, 테이블 하나):

```sql
CREATE TABLE IF NOT EXISTS fence_watermarks (
    resource  TEXT PRIMARY KEY,
    watermark BLOB NOT NULL     -- u64 를 8바이트 big-endian 으로 저장
                                 -- (SQLite INTEGER 의 i64::MAX 제한을 피한다)
) WITHOUT ROWID;
```

`check_and_advance()` 는 매 호출 `BEGIN IMMEDIATE` -> 조회 -> 비교 ->
(낮으면 rollback+`Stale`, 아니면 upsert) -> commit 순서로 **비교와
기록을 같은 트랜잭션 안**에서 한다 — 그러지 않으면 두 프로세스가
동시에 오래된 값을 읽는 창이 생긴다.

## Agent 호출부 변경

현재(`crates/agent/src/lib.rs`): `FenceWatermark::new()` 를 한 번
생성(`:116`)해 최초 Grant 의 Lease 검증(`:325-359` 근방)과 Lease
갱신 검증(`:182-296` 근방) 두 곳에서 재사용한다.

변경 후: `AgentConfig` 에 `fence_db_path: PathBuf` 추가, CLI 에
`--fence-db <path>` 추가, `run()` 초기에
`DurableFenceWatermark::open()` — **DB open 실패 시 네트워크 연결이나
ACK 전에 fail closed**. 두 검증 함수의 인자 타입을
`&mut DurableFenceWatermark` 로 변경. resource key 는 기존과 동일하게
`job_id` 유지(`lease_id` 로 바꾸면 갱신마다 다른 watermark 가 생겨
강등 방어가 깨진다는 기존 주석과 같은 이유,
`docs/plans/2026-08-19_0500_...v1.md` §FenceWatermark 재사용 절).

서명 대상 메시지는 변경하지 않는다 — `fence_db_path` 는 로컬 Agent
설정이지 Protocol payload 가 아니다.

## 재시작 selftest 설계

기존 `coordinator-agent-selftest` 는 `Command::new()` 로 별도 OS
프로세스를 띄우고 PID 를 대조한다(`coordinator_agent_selftest.rs:122-194`).
같은 방식을 SQLite 파일 공유에 적용한다 — **`agent-stub` 을 같은
`--fence-db` 로 두 번(별도 프로세스로) 실행**한다.

### 시나리오 A — 갱신 경로의 재시작 방어

1차 실행: `--fence-epoch 5 --do-renew true --renewed-fence-epoch 5`
(Coordinator) / `--do-renew true --fence-db <temp>/fence.sqlite3`
(Agent) → 정상 갱신 성공, 프로세스 종료.

2차 실행(**새 프로세스**, 같은 DB 파일): `--fence-epoch 5 --do-renew
true --renewed-fence-epoch 3` (Coordinator) / 같은 `--fence-db`
(Agent) → 최초 Grant epoch=5 는 durable watermark=5 와 같아 통과,
Lease 갱신의 epoch=3 은 durable watermark=5 보다 낮아 `RENEW_REJECTED:
fence_epoch` 로 거부. **1차 프로세스의 메모리 객체가 2차 프로세스에
전달되지 않는다는 것을 PID·프로세스 종료로 보장하면서, DB 파일만
상태를 전달한다** — `durable_replay_process.rs:108-205` 와 같은 원리.

### 시나리오 B — 최초 Grant 경로의 재시작 방어

시나리오 A 직후 같은 DB 로: `--fence-epoch 3 --do-renew false`
(Coordinator) / 같은 `--fence-db`(Agent) → 최초 Grant 의 Lease
epoch=3 이 durable watermark=5 보다 낮아 `LEASE_REJECTED: fence_epoch`
로 거부, ACK 미전송. 최초 Grant 검증과 Lease 갱신 검증 **두 호출부
모두**를 재시작 경계에서 입증한다.

### ★ 구현 중 정정(2026-08-19) — 시나리오 A 는 실제로 성립하지 않는다

구현 후 뮤테이션 테스트로 검증하는 과정에서(`DurableFenceWatermark::open()`
이 주어진 경로를 무시하고 매번 새 파일을 열도록 무력화) **시나리오 A
가 설계 의도와 달리 진짜 프로세스 경계를 넘는 영속성을 시험하지
못한다는 것이 드러났다.**

이유: Agent 의 제어 흐름상 Lease 갱신은 항상 같은 프로세스의 최초
Grant 검증(`verify_and_record_lease()`) **뒤**에만 일어난다. 시나리오
A 의 2차 프로세스는 최초 Grant(epoch 5)와 갱신(epoch 3) 을 **같은
프로세스** 안에서 순서대로 검증한다 — 최초 Grant 검증이 그 프로세스의
`DurableFenceWatermark` 인스턴스에 watermark=5 를 (다시) 써 넣고,
그 값이 durable 저장소에서 왔든 이 프로세스 자신이 방금 썼든
상관없이 뒤이은 갱신(epoch 3)의 거부를 설명하기에 충분하다. 실제로
`open()` 을 무력화해 2차 프로세스가 완전히 빈 파일에서 시작하게
만들어도, 그 프로세스 **자신의** 최초 Grant 호출이 로컬 watermark 를
5 로 만들어 놓으므로 뒤이은 갱신 거부는 계속 "통과"했다 —
**거짓양성(가짜 통과)** 이었다.

반면 시나리오 B(최초 Grant 검증 호출이 그 프로세스에서 **유일한**
`check_and_advance` 호출)는 같은 무력화에서 정확히 예상대로 실패했다
— 진짜 판별력이 있는 시험은 이것 하나뿐이었다.

**결론:** 이 Agent 의 구조(최초 Grant 검증이 항상 갱신 검증보다
먼저 실행되고, 같은 resource key 를 쓴다)에서는 "갱신 경로 전용"
재시작 방어를 최초 Grant 경로와 분리해서 증명할 방법이 없다 —
최초 Grant 검증이 durable 하면 그 위에 올라타는 갱신도 자동으로
안전하고, durable 하지 않으면 갱신 검증이 아무리 정확해도 프로세스
경계를 넘는 방어는 전혀 없다. 그래서 구현 단계에서 시나리오 A 를
버리고 시나리오 B 하나로 단계 7 을 재구성했다 — 존재하지 않는
구분을 존재하는 것처럼 evidence 에 남기지 않는다.

## 범위

### In

- `resource -> 최대 epoch` 의 SQLite 영속 저장
- 여러 `job_id` 동시 추적(기존 `HashMap` 과 동등, `resource PRIMARY
  KEY` 로 자연스럽게 보존 — 서로 다른 job 의 epoch 가 서로 차단하지
  않는 단위 테스트 포함)
- same epoch 허용 · 낮은 epoch 거부 · 높은 epoch 전진(기존 계약 그대로)
- 프로세스 종료 후 같은 DB 재오픈
- SQLite 트랜잭션·commit 실패의 fail-closed 처리
- `is_durable() == true` 인 파일 기반 구현
- 최초 Grant 경로 + Lease 갱신 경로 **둘 다** durable 로 전환

### Out (명시하지 않으면 범위가 샌다)

- GC·오래된 resource 항목 정리 — watermark 는 절대 만료시켜 지우면
  안 된다(지우면 그 resource 의 stale epoch 가 다시 통과한다).
  실제 resource lifecycle 이 생긴 뒤 별도 설계.
- 여러 Agent 가 같은 DB 파일을 공유하는 운영 모델 — 이 조각은 Agent
  하나가 DB 파일 하나를 소유하는 모델이다. `BEGIN IMMEDIATE`·
  `busy_timeout` 은 손상 방지·동시 접근의 기반일 뿐, 다중 Agent 의
  소유권·리더십·quota 정책은 정의하지 않는다(별도 계획 필요).
- Coordinator 의 영속 Lease 저장소
- Lease revoke · 실제 `SUPERSEDED`/`QUARANTINED` 결정 정책
- retry/backoff/failover
- Protocol schema·canonical field·domain tag·서명 대상 메시지 변경
  (이번 조각은 필요 없다 — Agent 로컬 설정일 뿐이다)

## 단계

| # | 단계 | 스트림 | 완료 기준 | 상태 |
|---|---|---|---|---|
| 1 | 기존 `FenceWatermark` 계약을 테스트로 고정(same/lower/higher/per-resource) | runtime-policy | 단위 테스트 통과(회귀 방지용 — 이미 있으면 확인만) | ✅(기존 테스트로 이미 고정돼 있어 확인만 함) |
| 2 | `crates/runtime-policy/src/durable_lease_scope.rs` 신설 + `DurableFenceWatermark` API | runtime-policy | `rusqlite` 의존성 추가, `open()`/`is_durable()` 동작 | ✅ |
| 3 | SQLite 스키마 + Windows 용 PRAGMA(journal_mode=DELETE·synchronous=FULL·busy_timeout) | runtime-policy | 파일 DB 생성·재오픈 확인 | ✅ |
| 4 | `BEGIN IMMEDIATE` 안에서 조회·비교·갱신·commit 구현 | runtime-policy | same epoch 통과·lower epoch 거부·higher epoch 전진 단위 테스트 | ✅ |
| 5 | `DurableFenceError` 추가(Stale/Io/LockTimeout 분리) | runtime-policy | 오류별 fail-closed 테스트 | ✅ |
| 6 | Agent 에 `--fence-db`/`fence_db_path` 추가, `FenceWatermark::new()` -> durable open 으로 교체(두 호출부 모두) | Agent · CLI | `cargo build -p gputeer-agent` 통과 | ✅ |
| 7 | `coordinator-agent-selftest` 에 재시작 시나리오 추가(별도 `agent-stub` 재실행) | CLI | 시나리오 통과, 5회 연속 확인 | ✅(시나리오 A 는 뮤테이션 테스트로 거짓양성임이 드러나 폐기, 시나리오 B 하나로 재구성 — 위 "구현 중 정정" 절 참조) |
| 8 | 뮤테이션 테스트 + 코덱스 독립 검수 + `docs/evidence/` schema v2 기록(DoD-14) | — | `ACCEPTED`, evidence PASS | 🟡 1라운드 검수(`p102`) `CHANGES_REQUESTED` — `:memory:` 미검증·오류 메시지 미분리 2건 지적, 코드로 수정(fail-closed `is_durable()` 검사 추가, `Stale`/저장소 장애 오류 메시지 분리, 시나리오 17 강화 + 시나리오 18 신규) + 뮤테이션 테스트로 비공허성 확인. 좁은 후속 검수 진행 중 |

## 완료 기준 (DoD)

- [x] `DurableFenceWatermark` 가 SQLite 파일에 watermark 를 영속한다.
- [x] **negative test**: 별도 프로세스로 재시작한 뒤, 재시작 전
      watermark 보다 낮은 epoch 의 최초 Grant 가 거부된다(시나리오 17,
      원래 시나리오 B). ★ "갱신 경로 전용" negative test(원래 시나리오
      A)는 구현 중 뮤테이션 테스트로 거짓양성임이 드러나 폐기했다 —
      최초 Grant 검증 호출 하나가 유일한 판별력 있는 재시작 경계다.
- [x] same epoch 재사용 여전히 허용(회귀 없음, 기존 단위 테스트로 확인).
- [x] 서로 다른 `job_id` 가 서로의 watermark 를 침범하지 않는다(단위 테스트).
- [x] `coordinator-agent-selftest` 확장 시나리오가 5회 연속 통과한다(17개
      시나리오 전체).
- [ ] `docs/evidence/` 에 schema v2 형식(DoD-14)으로 기록.

## 기준선과 다른 점

없음 — 이 계획은 `docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md`
의 "Out" 절이 이미 예고한 다음 한 걸음을 그대로 이행한다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-19 22:00 | 코덱스 설계(`p101`) 정리 — 구현 착수 전 |
