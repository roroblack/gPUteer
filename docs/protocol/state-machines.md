# gPUteer State Machines — 규범 전이표 v1

**상태:** NORMATIVE. 이 표에 없는 전이는 구현하면 안 된다(MUST NOT).
**설계 근거:** `gputeer_master_implementation_plan_v5.md` §27
**최종 수정:** 2026-08-14

> **이 문서가 표 형식인 이유**
>
> v4와 v5 초안 모두 상태 기계를 **산문 화살표 나열**로 적었고, 두 번의 검토에서
> 매번 누락 전이가 발견됐다. 산문으로는 빠진 것을 찾을 수 없다.
>
> 이 문서의 표는 **테스트가 직접 파싱한다.** `crates/checkpoint/tests/state_table_parity.rs` 가
> 이 파일을 읽어, 표의 모든 행에 대응하는 테스트가 존재하는지 검사한다.
> 표에 행을 추가하면 테스트가 하나 늘고, 구현하지 않으면 CI가 실패한다.

---

## 0. 파싱 계약

각 전이표는 다음 6열을 정확히 이 순서로 갖는다.

| 열 | 의미 |
|---|---|
| `from` | 출발 상태. `*` = 명시된 모든 상태 |
| `to` | 도착 상태 |
| `trigger` | 전이를 일으키는 사건 (SCREAMING_SNAKE) |
| `guard` | 만족해야 하는 조건. `-` = 없음 |
| `effect` | 반드시 수행되는 부수효과. `-` = 없음 |
| `durability` | 이 전이를 기록할 때 필요한 ControlStore 보증. `COMMITTED` / `DURABLE` / `LOCAL` |

`durability` 열의 의미 (ADR-024):
- `COMMITTED` — 과반 합의 필요. **SingleNodeStore에서는 불가능**
- `DURABLE` — 로컬 fsync로 충분
- `LOCAL` — ControlStore에 기록하지 않음 (Agent 내부 상태)

표 블록은 ` ```statetable ` 펜스로 감싼다. 테스트 파서는 이 펜스만 읽는다.

---

## 1. Node

```statetable
machine: Node
from | to | trigger | guard | effect | durability
DISCOVERED | ENROLLING | JOIN_REQUESTED | invite 서명 유효 | preflight 시작 | DURABLE
DISCOVERED | DISCOVERED | REDISCOVERED | - | - | LOCAL
ENROLLING | ENROLL_REJECTED | PREFLIGHT_FAILED | clock skew 초과 또는 capability 미달 또는 agent 서명 불일치 | 사유를 사용자에게 표시 | DURABLE
ENROLLING | ENROLL_REJECTED | OWNER_DENIED | - | 감사 로그 기록 | COMMITTED
ENROLLING | APPROVED | OWNER_APPROVED | Owner 서명 유효 | Device Certificate 발급 | COMMITTED
ENROLL_REJECTED | ENROLLING | JOIN_RETRIED | 거부 사유 해소 확인 | preflight 재실행 | DURABLE
APPROVED | ONLINE | FIRST_HEARTBEAT | 서명 검증 통과 | 벤치마크 예약 | DURABLE
APPROVED | REVOKED | OWNER_REVOKED | Owner 서명 | Device Certificate 무효화 | COMMITTED
ONLINE | SUSPECT | HEARTBEAT_MISSED | 3회 연속 누락 (15초) | 신규 Job 배치 제외 | DURABLE
ONLINE | DRAINING | DRAIN_REQUESTED | - | 신규 Job 거부 시작 | DURABLE
ONLINE | QUARANTINED | QUARANTINE_VERDICT | Worker는 2-of-3, Coordinator는 Owner 서명 | §22.4 격리 절차 실행 | COMMITTED
ONLINE | ROTATING_KEY | KEY_ROTATION_STARTED | 실행 중 Job 없음 또는 grace 병존 가능 | 신규 Job 배치 일시 중단 | DURABLE
ONLINE | UPDATING | AGENT_UPDATE_STARTED | 실행 중 Job 없음 | 신규 Job 배치 일시 중단 | DURABLE
SUSPECT | ONLINE | HEARTBEAT_RESUMED | 연속 2회 정상 수신 | 배치 제외 해제 | DURABLE
SUSPECT | UNREACHABLE | HEARTBEAT_MISSED | 12회 연속 누락 (60초) | Job watchdog 경보 | DURABLE
SUSPECT | QUARANTINED | QUARANTINE_VERDICT | - | §22.4 절차 | COMMITTED
UNREACHABLE | RECOVERING | HEARTBEAT_RESUMED | lease TTL 미경과 | attempt 진행 상황 대조 | DURABLE
UNREACHABLE | LOST | LEASE_EXPIRED | lease.expires_at + grace 경과 | failover 트리거 | COMMITTED
LOST | RECOVERING | HEARTBEAT_RESUMED | - | AttemptReport 수집, reconciliation 대기 | DURABLE
RECOVERING | ONLINE | RECONCILE_DONE | 진행 상황 대조 완료 | - | DURABLE
RECOVERING | LOST | HEARTBEAT_MISSED | 복구 중 재차 단절 | - | DURABLE
DRAINING | ONLINE | DRAIN_CANCELLED | 사용자 취소 | 신규 Job 거부 해제 | DURABLE
DRAINING | OFFLINE | DRAIN_COMPLETED | 모든 Job 이전 또는 checkpoint 확정 | lease 반납 | DURABLE
DRAINING | LOST | POWER_LOST | drain 도중 전원 차단 | failover 트리거 | COMMITTED
DRAINING | OFFLINE | DRAIN_TIMEOUT | timeout 초과 시 다음 단계로 강등 | 손실 step 수 사용자에게 보고 | DURABLE
OFFLINE | ONLINE | HEARTBEAT_RESUMED | Device Certificate 유효 | 백그라운드 복제 재개 | DURABLE
OFFLINE | ENROLLING | REINSTALLED | 새 device key 생성됨 | 기존 device revoke 후 신규 등록 | COMMITTED
ROTATING_KEY | ONLINE | KEY_ROTATION_DONE | 신규 키 Raft 커밋 완료 | grace period 24h 시작 | COMMITTED
ROTATING_KEY | ONLINE | KEY_ROTATION_FAILED | - | 구 키 유지, 경고 | DURABLE
UPDATING | ONLINE | UPDATE_APPLIED | 신규 바이너리 서명 검증 통과 | agent_binary_digest 갱신 | DURABLE
UPDATING | ONLINE | UPDATE_ROLLED_BACK | 3회 연속 기동 실패 | 직전 버전 복원, risk signal | DURABLE
QUARANTINED | ONLINE | QUARANTINE_RELEASED | Owner 서명 | ACL 복원, 이력 보존 | COMMITTED
QUARANTINED | REVOKED | OWNER_REVOKED | Owner 서명 | - | COMMITTED
ONLINE | TERMINATED | EPHEMERAL_SHUTDOWN | is_ephemeral == true | replica 계수에서 제외, LOST로 취급하지 않음 | DURABLE
DRAINING | TERMINATED | EPHEMERAL_SHUTDOWN | is_ephemeral == true | - | DURABLE
TERMINATED | ENROLLING | EPHEMERAL_RELAUNCHED | enrollment token 유효 | 새 device_id 발급 | DURABLE
* | REVOKED | OWNER_REVOKED | Owner 서명 | 모든 lease 무효, artifact 재검증 대상 | COMMITTED
```

### risk_state 와 node_state 의 관계

v4는 두 축을 분리해두고 상호작용을 정의하지 않았다. v5 규칙:

```text
QUARANTINED / REVOKED / TERMINATED 는 node_state 에 통합한다.
SUSPECT 는 risk_state 로도 존재하지만 node_state 를 바꾸지 않는다.

Scheduler 후보 조건:
    node_state == ONLINE  AND  risk_state == NORMAL

"ONLINE + risk SUSPECT" 인 노드는
    - 실행 중 Job 은 계속한다
    - 신규 배치에서만 제외된다
    - 감시 주기를 2배로 높인다
```

---

## 2. Job

```statetable
machine: Job
from | to | trigger | guard | effect | durability
(none) | SUBMITTED | SUBMIT_ACCEPTED | manifest 서명 유효 AND quorum 정상 | job_id 발급 | COMMITTED
(none) | (none) | SUBMIT_UNAVAILABLE | quorum 상실 | Job 생성하지 않음, 예상 복구 시각 표시 | LOCAL
(none) | (none) | SUBMIT_INFEASIBLE | Hard Filter 만족 노드가 팀에 없음 | 사유 표시 | LOCAL
(none) | (none) | SUBMIT_POLICY_VIOLATION | side_effecting인데 acknowledge_duplicate_risk 없음 | 사유 표시 | LOCAL
SUBMITTED | PLANNING | PLANNING_STARTED | - | 후보 노드 수집 | DURABLE
SUBMITTED | CANCELLED | USER_CANCELLED | - | - | COMMITTED
PLANNING | QUEUED | PLAN_READY | 실행 가능한 plan 1개 이상 | PlacementRationale 기록 | COMMITTED
PLANNING | FAILED | NO_FEASIBLE_PLAN | 모든 후보가 Hard Filter 탈락 | 탈락 사유 목록 보고 | COMMITTED
PLANNING | CANCELLED | USER_CANCELLED | - | - | COMMITTED
QUEUED | STAGING | RESOURCE_AVAILABLE | 선택 노드 lease 발급 성공 | fence_epoch 증가 | COMMITTED
QUEUED | PLANNING | REPLAN_REQUIRED | 후보 노드 상태 변화 | - | DURABLE
QUEUED | FAILED | DEADLINE_PASSED | now > deadline | - | COMMITTED
QUEUED | FAILED | QUEUE_TIMEOUT | 대기 시간 > max_queue_minutes | - | COMMITTED
QUEUED | FAILED | PERMANENTLY_INFEASIBLE | Hard Filter 만족 노드가 팀에서 사라짐 | 사유 표시 | COMMITTED
QUEUED | CANCELLED | USER_CANCELLED | - | lease 미발급이므로 정리 불필요 | COMMITTED
STAGING | RUNNING | STAGING_COMPLETE | 환경 준비 + 데이터 스테이징 완료 | - | DURABLE
STAGING | FAILED | STAGING_FAILED | 재시도 3회 초과 | 부분 다운로드 정리 | COMMITTED
STAGING | QUEUED | STAGING_NODE_LOST | 노드가 STAGING 중 이탈 | lease 회수, 재배치 | COMMITTED
STAGING | CANCELLED | USER_CANCELLED | - | workspace 정리, lease 반납 | COMMITTED
RUNNING | COMPLETED | ATTEMPT_COMPLETED | canonical attempt 확정 | 최종 artifact COMMITTED 확인 | COMMITTED
RUNNING | INTERRUPTED | NODE_LOST | lease 만료 + grace 경과 | - | COMMITTED
RUNNING | FAILED | UNRECOVERABLE_ERROR | 재시도 정책 소진 | 사유와 마지막 step 보고 | COMMITTED
RUNNING | PAUSED | PARTITION_PAUSE | on_partition == PAUSE AND lease 만료 | checkpoint 후 정지 | DURABLE
RUNNING | PAUSED | OWNER_PREEMPT | 노드 소유자가 일시정지 요청 | checkpoint 후 정지 | DURABLE
RUNNING | PAUSED | USER_PAUSED | - | checkpoint 후 정지 | COMMITTED
RUNNING | RECONCILING | DUPLICATE_COMPLETION | 2개 이상 attempt가 완료 보고 | §20.3 알고리즘 실행 | COMMITTED
RUNNING | CANCELLED | USER_CANCELLED | - | process tree 종료, lease 반납 | COMMITTED
INTERRUPTED | REPLANNING | FAILOVER_STARTED | 마지막 COMMITTED checkpoint 존재 | - | DURABLE
INTERRUPTED | FAILED | NO_COMMITTED_CHECKPOINT | 복구 가능한 checkpoint 없음 | 손실 범위 보고 | COMMITTED
INTERRUPTED | CANCELLED | USER_CANCELLED | - | - | COMMITTED
REPLANNING | QUEUED | REPLAN_READY | 새 후보 확보 | - | COMMITTED
REPLANNING | STAGING | REPLAN_DIRECT | 후보 노드가 이미 checkpoint 보유 | 데이터 스테이징 생략 | COMMITTED
REPLANNING | FAILED | NO_FEASIBLE_PLAN | 재배치 가능한 노드 없음 | - | COMMITTED
REPLANNING | CANCELLED | USER_CANCELLED | - | - | COMMITTED
PAUSED | RUNNING | RESUMED | 새 lease 발급 | fence_epoch 증가 | COMMITTED
PAUSED | FAILED | PAUSE_TIMEOUT | 일시정지 상한 초과 | - | COMMITTED
PAUSED | CANCELLED | USER_CANCELLED | - | - | COMMITTED
RECONCILING | COMPLETED | CANONICAL_CHOSEN | 유효 attempt 1개 이상 | CanonicalDecision 기록 | COMMITTED
RECONCILING | FAILED | ALL_ATTEMPTS_INVALID | 모든 attempt가 유효성 필터 탈락 | 탈락 사유 보고 | COMMITTED
RECONCILING | RECONCILING | TIE_UNRESOLVED | 순위 동점 | canonical 변경 없이 사용자 확인 요청 | COMMITTED
COMPLETED | ARCHIVED | RETENTION_EXPIRED | artifact 보존 기간 경과 | CAS GC 대상 등록 | DURABLE
```

### Quorum 상실 중의 Job 제출

```text
클라이언트에 SUBMIT_UNAVAILABLE 을 즉시 반환한다. Job 은 생성되지 않는다.
UI 는 quorum 상태와 estimated_recovery_unix_ms 를 함께 표시한다.
로컬 큐잉을 하지 않는다 — 나중에 조용히 실행되는 것이 더 나쁘기 때문이다.
```

---

## 3. Attempt

```statetable
machine: Attempt
from | to | trigger | guard | effect | durability
(none) | CREATED | ATTEMPT_CREATED | Job이 STAGING 진입 | fence_epoch 증가, lease 발급 | COMMITTED
CREATED | STARTING | GRANT_ACCEPTED | Agent 17단계 검증 통과 | workspace 생성 | DURABLE
CREATED | CANCELLED | GRANT_REJECTED | Agent 검증 실패 | 실패 단계 번호 감사 로그 기록 | COMMITTED
CREATED | CANCELLED | JOB_CANCELLED | - | lease 반납 | COMMITTED
STARTING | RUNNING | PROCESS_STARTED | 프로세스 기동 + 첫 progress 수신 | - | DURABLE
STARTING | FAILED | START_FAILED | 환경 준비 실패 또는 프로세스 기동 실패 | workspace 정리 | COMMITTED
RUNNING | COMPLETED | WORKLOAD_EXITED_OK | exit code 0 AND 최종 artifact HASH_VERIFIED | AttemptReport 제출 | COMMITTED
RUNNING | FAILED | WORKLOAD_EXITED_ERROR | exit code != 0 | 로그 보존 | COMMITTED
RUNNING | FAILED | WATCHDOG_KILLED | no-progress 판정 | process tree 종료, VRAM 반환 확인 | COMMITTED
RUNNING | PAUSED | PAUSE_REQUESTED | - | checkpoint 생성 | DURABLE
RUNNING | STALE | LEASE_EXPIRED | lease 만료 AND Coordinator 도달 불가 | side_effect_class 에 따라 계속 또는 정지 | LOCAL
PAUSED | RUNNING | RESUME_REQUESTED | 새 lease 발급 | - | COMMITTED
PAUSED | CANCELLED | JOB_CANCELLED | - | - | COMMITTED
STALE | RUNNING | LEASE_RENEWED | 재연결 성공 AND RENEW_OUTCOME_RENEWED | - | COMMITTED
STALE | COMPLETED | STALE_WORKLOAD_FINISHED | 단절 중 실제로 완주 | AttemptReport 를 STALE_COMPLETED 로 제출 | DURABLE
STALE | FAILED | STALE_WORKLOAD_FAILED | 단절 중 실패 | - | DURABLE
STALE | RECONCILING | RECONNECTED_WITH_RESULT | 재연결 + 결과 제출 | §20.3 유효성 필터 | COMMITTED
COMPLETED | RECONCILING | DUPLICATE_DETECTED | 같은 Job의 다른 attempt도 완료 | §20.3 순위 결정 | COMMITTED
RECONCILING | CANONICAL | SELECTED | §20.3 순위 1위 | Job canonical_attempt_id 갱신 | COMMITTED
RECONCILING | SUPERSEDED | NOT_SELECTED | - | artifact 는 보존, canonical 아님 | COMMITTED
RECONCILING | FAILED | VALIDITY_FILTER_REJECTED | 해시 불일치 또는 revoke 된 device 제출 | risk signal 발화 | COMMITTED
```

### STALE 의 의미

```text
STALE 은 "죽었다"가 아니라 "lease 를 잃었고 Coordinator 와 통신이 안 된다"이다.
워크로드는 side_effect_class 에 따라 계속 돌 수 있다 (계획서 §5.4.1).

  PURE            계속 실행. 결과는 나중에 attempt artifact 로 제출
  IDEMPOTENT      계속 실행. 외부 쓰기에 operation_id 사용
  SIDE_EFFECTING  즉시 checkpoint 후 정지. 외부 부작용 금지

STALE 전이의 durability 가 LOCAL 인 이유:
  Coordinator 에 도달할 수 없는 상태이므로 기록할 수 없다.
  재연결 시 RECONCILING 으로 올라가면서 그때 COMMITTED 된다.
```

---

## 4. Checkpoint

```statetable
machine: Checkpoint
from | to | trigger | guard | effect | durability
(none) | WRITING | CHECKPOINT_STARTED | - | <name>.tmp 생성 | LOCAL
WRITING | LOCAL_WRITTEN | WRITE_COMPLETE | fsync(file) + rename + fsync(dir) 완료 | - | LOCAL
WRITING | PARTIAL | PROCESS_KILLED | 매니페스트 미기록 상태에서 중단 | - | LOCAL
PARTIAL | (deleted) | STARTUP_GC | 부팅 시 매니페스트 없는 데이터 파일 발견 | 삭제 후 로그 기록 | LOCAL
LOCAL_WRITTEN | HASH_VERIFIED | HASH_MATCHED | BLAKE3 재계산 == 매니페스트 | - | LOCAL
LOCAL_WRITTEN | PARTIAL | HASH_MISMATCH | 재계산 불일치 | 삭제, risk signal 발화 | DURABLE
HASH_VERIFIED | REPLICATING | REPLICATION_STARTED | durability != LOCAL | Hub/peer 전송 시작 | LOCAL
HASH_VERIFIED | COMMITTED | DURABILITY_MET | durability == LOCAL | canonical 후보 자격 획득 | DURABLE
HASH_VERIFIED | (deleted) | RETENTION_GC | 더 최신 COMMITTED 존재 AND 보존 개수 초과 | 삭제 | LOCAL
REPLICATING | REPLICATED | ACK_RECEIVED | 서명된 ReplicaAck 수신 | 유효 replica 수 갱신 | DURABLE
REPLICATING | HASH_VERIFIED | REPLICATION_FAILED | 전송 실패, 재시도 예정 | 백로그에 유지 | LOCAL
REPLICATING | HASH_VERIFIED | ACK_TIMEOUT | ACK 미수신 타임아웃 | 다른 replica 대상으로 재시도 | LOCAL
REPLICATED | COMMITTED | DURABILITY_MET | 유효 replica 수 >= 요구치 | canonical 후보 자격 획득 | DURABLE
REPLICATED | REPLICATING | REPLICA_LOST | 요구치 미달로 하락 | 추가 복제 시도 | DURABLE
COMMITTED | COMMITTED_DEGRADED | REPLICA_LOST | COMMITTED 이후 replica 유실 | 복구 큐 등록 + 경고. canonical 자격은 유지 | DURABLE
COMMITTED_DEGRADED | COMMITTED | REPLICA_RESTORED | 유효 replica 수 회복 | 경고 해제 | DURABLE
COMMITTED | (deleted) | RETENTION_GC | 더 최신 COMMITTED 존재 AND 보존 개수 초과 | 삭제 | DURABLE
```

### 원자성 규칙 (MUST)

★ **2026-08-16 — 아래 절차는 Windows 에서 그대로 성립하지 않는다 (ADR-026).**

`P0-03a` 실측: Windows `MoveFileEx` 는 **열린 파일 위로 rename 하지 못한다**
(313/3000 성공). 아래 절차 3번(`rename`)이 데이터 파일에 대해 실패한다.

```text
현재 구현 (ADR-026 반영)
  데이터 파일   write-once — 고유 이름으로만 쓴다. rename-over-existing 회피
  포인터 파일   유일한 replace 대상. bounded retry 후 명시적 오류
  sync_dir      Windows 는 쓰기 권한 필요 (FILE_FLAG_BACKUP_SEMANTICS)
```

**ADR-026 은 아직 `제안` 상태다** — 기준선 §18.2 수정 승인 대기(D-5).
그때까지 이 절의 원문을 남겨 두되, **구현은 ADR-026 을 따른다.**
독립 검수가 이 불일치를 지적했다.

```text
1. <name>.tmp 에 기록 → fsync(file) → rename(name) → fsync(dir)
2. 매니페스트는 모든 데이터 파일이 확정된 뒤 "마지막에" 쓴다
3. 매니페스트 없는 데이터 파일은 PARTIAL 로 간주하고 부팅 시 GC 한다
4. 같은 step 의 체크포인트가 중복 생성되면
   digest 가 같으면 병합, 다르면 나중 것을 PARTIAL 로 처리하고 경고한다
```

### COMMITTED 이후 replica 유실

```text
상태를 되돌리지 않는다. COMMITTED_DEGRADED 로 표시하고 canonical 후보 자격은 유지한다.
이미 이 체크포인트를 근거로 다른 결정이 내려졌을 수 있기 때문이다.
대신 복구 작업을 큐에 넣고 사용자에게 경고한다.
```

---

## 5. Lease

```statetable
machine: Lease
from | to | trigger | guard | effect | durability
(none) | ACTIVE | LEASE_ISSUED | fence_epoch 증가 커밋 완료 | 자원 예약 | COMMITTED
ACTIVE | ACTIVE | RENEWED | now >= renew_after AND 누적 < max_total_duration | expires_at 연장, epoch 불변 | COMMITTED
ACTIVE | EXPIRED | TTL_REACHED | now > expires_at | grace period 시작 | LOCAL
ACTIVE | REVOKED | REVOKE_RECEIVED | Coordinator 서명 유효 | 즉시 자원 반납 | COMMITTED
ACTIVE | SUPERSEDED | HIGHER_EPOCH_SEEN | 더 높은 fence_epoch 의 lease 관측 | 자원 반납, stale 선언 | COMMITTED
ACTIVE | EXPIRED | MAX_DURATION_EXCEEDED | 누적 >= max_total_duration_seconds | 새 lease_id 재발급 필요 | COMMITTED
EXPIRED | ACTIVE | LATE_RENEW_ACCEPTED | grace period 내 AND 더 높은 epoch 미발급 | - | COMMITTED
EXPIRED | SUPERSEDED | GRACE_ELAPSED | grace period 경과 | 새 attempt 생성 허용 | COMMITTED
```

### grace period

```text
Coordinator 는 lease 만료를 관측해도 "즉시" 재배치하지 않는다.
    expires_at + grace_period (기본 60초) 경과 후에 새 attempt 를 만든다.

이유: 네트워크 순단으로 인한 불필요한 중복 실행을 억제한다.
      §13.4 의 P(성공) 계산에 이 지연이 포함되어야 한다.
```

---

## 6. 테스트 계약

`crates/checkpoint/tests/state_table_parity.rs` 는 이 파일을 파싱해 다음을 검사한다.

★ **2026-08-16 정정.** 이 절은 원래 `tests/unit/state_machine_test.rs` 를 가리켰는데
**그 파일이 존재하지 않았다.** 독립 검수가 찾았다 —
`durability.rs` 가 전이를 하드코딩하고 있었고, 이 표가 바뀌어도 코드는 그대로였다.
**문서와 코드가 서로 다른 상태기계를 말해도 테스트는 초록색이었다.**

```text
1. 표의 모든 행에 대응하는 전이 함수가 구현되어 있다
2. 구현에 존재하는 전이가 표에 전부 있다 (표에 없는 전이 = 실패)
3. durability == COMMITTED 인 전이를 SingleNodeStore 로 시도하면
   Unsupported 가 반환된다 (ADR-024)
4. 각 상태에서 도달 불가능한 상태로의 전이 시도가 거부된다
5. `*` 행은 명시된 모든 from 상태에 대해 개별 검증된다
6. 모든 terminal 상태(REVOKED, ARCHIVED, CANCELLED, FAILED)에서
   나가는 전이가 표에 없음을 확인한다
```

**표를 고치지 않고 전이를 추가하면 CI 가 실패한다.** 이것이 이 문서의 목적이다.

### 현재 검사 범위

★ **위 6개 중 3개만 검사된다.** 나머지는 해당 계층이 미구현이다.

| # | 검사 | 상태 |
|---|---|---|
| 1 | 표의 모든 전이가 구현에서 허용된다 | ✅ Checkpoint |
| 2 | 구현의 모든 전이가 표에 있다 | ✅ Checkpoint |
| 3 | `COMMITTED` 전이 → SingleNodeStore 는 `Unsupported` | ❌ ControlStore 미구현 |
| 4 | 도달 불가 상태로의 전이 거부 | 🟡 2번이 부분적으로 덮는다 |
| 5 | `*` 행을 모든 from 상태에 대해 개별 검증 | ❌ 해당 표(Node/Job/Attempt/Lease) 미구현 |
| 6 | terminal 상태에서 나가는 전이 없음 | ✅ Checkpoint (`PARTIAL`) |

**Node · Job · Attempt · Lease 상태기계는 구현 자체가 없다.**
표만 있고 그것을 강제하는 코드가 없으므로, **그 표들은 아직 규범이 아니라 설계 메모다.**
`unchecked_contract_items_are_declared` 테스트가 이 사실을 고정한다.

---

## 7. 미해결

| # | 항목 | 상태 |
|---|---|---|
| 1 | 분산 Job(Mode B/C)에서 참여 노드 일부만 STALE 이 된 경우의 Attempt 상태 | **미정.** M13/M15 설계 시 확정. 현재는 "하나라도 STALE 이면 Attempt 전체 STALE" 로 보수적 처리 |
| 2 | `COMMITTED_DEGRADED` 상태가 canonical 선택 순위에 영향을 주는지 | 미정. 현재는 영향 없음 |
| 3 | Node `ROTATING_KEY` / `UPDATING` 중 들어온 ExecutionGrant 처리 | 현재는 거부. 유예 정책(계획서 §7.3.2, §24.4)과 함께 재검토 |
