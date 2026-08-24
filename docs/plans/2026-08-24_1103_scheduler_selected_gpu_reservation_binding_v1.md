# 2026-08-24_1103 scheduler selected GPU reservation binding v1

- **조사 대상:** `DoD-48` 다음의 하루 규모 scheduler 조각
- **선행 완료:** `DoD-41`~`DoD-48`
- **오늘 착수 결론:** **선택 GPU 집합의 durable CAS reservation binding**
- **예상 규모:** production Rust 100~190줄, coordinator test 170~300줄,
  문서/evidence 별도 — 1일
- **proto 변경:** 없음
- **정직한 경계:** Grant/scope 생성, GPU별 가용량 차감, release, production wire가
  아니라 `selected_gpu_ids`를 STAGING과 같은 durable transaction에 보존하는 조각이다.

## 결론

다음 조각은 Manifest adapter, Grant 전송, reservation release가 아니다.
`DoD-48`이 계산한 `selected_gpu_ids`는 현재
`PlacementToStagingOutcome::Staged`의 메모리 반환값에만 남고
(`crates/coordinator/src/orchestrate.rs:191-221`), durable reservation에는
`node_id/job_id/attempt_id/inventory_revision/reserved_at`만 저장된다
(`crates/coordinator/src/staging_store.rs:62-68`, `:221-231`).

따라서 STAGING commit 직후 Coordinator가 재시작하면 다음 사실을 복원할 수 없다.

```text
이 Attempt가 CAS를 통과할 때 정확히 어느 GPU ID 집합을 선택했는가
```

나중에 현재 inventory를 다시 읽어 재계산하면 revision이 달라졌을 수 있고, 그 결과를
`GrantedExecutionPlan.assigned_gpu_uuids`나 `ResourceScope.gpu_uuids`에 넣으면
durable admission과 wire 권한이 서로 다른 GPU를 가리킬 수 있다. 이는 Grant builder를
먼저 만드는 것으로 해결되지 않는다.

오늘 닫을 최소 조각은 선택 ID 집합을 reservation request의 멱등 payload에 포함하고,
inventory revision/ID 존재 검증, node-exclusive reservation, Attempt/Lease 생성,
`QUEUED -> STAGING`과 **같은 `BEGIN IMMEDIATE` transaction**에 보존하는 것이다.
이 조각은 opaque scheduler `gpu_id`를 wire-level GPU UUID라고 주장하지 않는다.

## 후보별 판정

| 후보 | 지금 실제 가능성 | 하루 규모 | 닫는 진짜 위험 | 우선순위 판정 |
|---|---|---:|---|---|
| Manifest -> `JobRequirements` | **제한적으로 가능.** `JobManifest`는 이미 `proto/job.proto:41-91`에 있고 generated type은 `gputeer_protocol::pb::JobManifest`다. 다만 안전한 함수는 raw Manifest가 아니라 `Verified<JobManifest>`를 받아야 한다(`docs/protocol/signing.md` 13.2, `crates/protocol/src/signing.rs:624-643`). Manifest에는 scheduler가 요구하는 `submitter_member_id`가 없고 `team_id`/`submitter_device_id`만 있으므로 membership lookup 결과도 별도 입력이어야 한다. CUDA, allocation mode, deadline/durability 같은 아직 `JobRequirements`에 없는 제약은 조용히 버릴 수 없다. | 엄격한 supported-subset adapter만 1일. 원본 저장/ingress까지 합치면 아님 | unspecified/unknown enum과 protobuf 기본값이 hard-filter 사실로 오인되는 위험 | **후순위.** 순수 adapter는 coordinator crate에서 만들 수 있지만 durable Job row에는 여전히 hash만 남고(`job_store.rs:97-112`), lookup/저장/재해시 blocker를 닫지 못한다. |
| Grant plan/scope 생성 | **구조는 이미 충분.** `GrantedExecutionPlan.assigned_gpu_uuids`와 `Lease.scope.gpu_uuids`가 있어 proto 추가는 필요 없다(`proto/job.proto:141-160`, `proto/common.proto:206-213`, `proto/lease.proto:63`). 그러나 현재 ID는 UUID provenance가 없는 snapshot 식별자이고, 선택 집합은 durable하지 않다. plan의 mode/allocation/durability/replication/rationale와 scope의 CPU/RAM/workspace/writable prefixes producer도 없다. Agent도 아직 이 자원 plan/scope를 runtime에 강제하지 않는다. | 좁은 pure builder는 1일, 실제 Grant 연결은 1일 초과 | 빈 plan/scope 또는 plan-scope 불일치 | **지금 아님.** 기존 필드면 충분하지만 durable assignment와 UUID provenance보다 먼저 `_uuids` 필드를 채우면 이름보다 약한 보장을 만들게 된다. |
| reservation release | **위험은 실제다.** row에는 release API가 없고 node reservation은 영구히 남는다. 다만 orchestration이 production 미연결이라 현재 배포 경로의 누수는 아직 아니다. 더 중요한 점은 현재 Job 상태가 `STAGING`까지만, Attempt 상태가 `Created` 하나뿐이고(`job_store.rs:18-24`, `staging_store.rs:36-38`), Grant outbox/ACK/완료 보고가 없다는 것이다. Lease row를 revoke하거나 reservation을 삭제하는 것만으로 Agent가 멈췄다는 증명이 되지 않는다. | 안전한 완료/실패 release는 1일 초과. outbox가 증명하는 pre-send abort만 별도 1일 후보 | 영구 node 점유와 capacity 고갈 | **지금 단독 구현 금지.** naive delete는 아직 실행 중인 Lease와 새 Job을 겹치게 해 liveness 문제를 safety 문제로 바꾼다. release는 `never-published`, verified terminal report, 또는 중단/만료 정책이 증명된 경로와 원자 결합해야 한다. |
| 선택 GPU durable reservation binding | **가능.** `DoD-48`이 canonical ID 집합을 생산하고, `DoD-47` transaction이 inventory revision CAS와 reservation/STAGING을 이미 결합한다. 필요한 새 입력과 linearization point가 모두 있다. | **1일** | commit 뒤 crash/replay에서 GPU assignment를 잃거나 최신 inventory로 잘못 재구성해 admission과 Grant가 어긋나는 위험 | **오늘 선택.** 새 정책이나 wire를 발명하지 않고 바로 다음 consumer가 신뢰할 durable fact를 만든다. |

## Manifest adapter의 정확한 판정

### 있는 것

- `JobManifest.resources`와 `workload`가 있고(`proto/job.proto:58-59`),
  `GpuRequest`/`ResourceRequest`도 이미 있다(`proto/common.proto:187-203`).
- minimum isolation/security/key, side-effect class와 dataset sensitivity를 scheduler
  domain enum으로 일대일 변환할 수 있다.
- protobuf의 알려지지 않은 enum 값은 `try_from(i32)` 실패로, `*_UNSPECIFIED`는
  별도 typed error로 fail closed할 수 있다.
- `GpuRequest.min_count == 0`은 proto 주석의 명시 기본값 1로 정규화할 수 있다.
- scheduler crate가 protocol에 의존하게 만들 필요는 없다. 두 crate에 이미 의존하는
  coordinator에 pure adapter module을 두는 편이 현재 계층을 보존한다.

### 아직 없는 것

- `JobRequirements.submitter_member_id`의 source. `team_id`는 member ID가 아니며,
  `submitter_device_id -> member_id`의 검증된 membership lookup이 필요하다.
- 원본 signed Manifest의 durable bytes. `AcceptedJobSubmission`/`StoredJob`은
  `manifest_hash`만 보존한다(`crates/coordinator/src/job_store.rs:97-112`).
- CUDA/compute capability, allocation mode, max egress, deadline, durability/checkpoint
  가능성처럼 Manifest에는 있으나 현재 `JobRequirements`에는 없는 hard constraint의
  지원 계약.
- dataset이 없는 Job의 sensitivity 기본값 등 명시되지 않은 normalization 정책.

그러므로 오늘 만들 수 있는 정직한 함수의 최대 범위는 다음과 같다.

```text
Verified<JobManifest> + externally resolved submitter_member_id
  -> Result<JobRequirements, UnsupportedOrInvalidManifest>
```

이 함수는 지원하지 않는 제약을 오류로 거부해야 하며, 이것만으로 original Manifest
저장이나 submit ingress가 생겼다고 말할 수 없다. 좋은 후속 조각이지만 `DoD-48`의
새 결과가 durable 경계에서 사라지는 문제보다 먼저일 이유는 없다.

## Grant plan/scope의 proto 판정

**proto 변경은 필요 없다.** 필요한 GPU 필드는 이미 둘 다 repeated string으로 있다.

```text
GrantedExecutionPlan.assigned_gpu_uuids
Lease.scope.gpu_uuids
```

다만 생성기의 안전 계약은 최소한 다음을 강제해야 한다.

1. plan과 scope의 GPU 집합이 같은 durable assignment에서 왔다.
2. 둘 다 canonical 순서이고 정확히 같은 집합이다.
3. 그 ID가 실제 UUID라고 attested inventory producer가 증명했다.
4. scope의 CPU/RAM/workspace와 writable prefixes도 accepted Manifest/plan에서 왔다.
5. inner Lease 서명 후 outer Grant를 서명하며, recovery 시에도 같은 durable 값을 쓴다.

현재는 1번의 durable source부터 없다. 오늘 선택한 조각은 1번을 위한 저장 경계를
만들지만 3번을 증명하지 않으므로 proto `_uuids` 필드를 아직 채우지 않는다.

## reservation release의 안전 경계

release 부재는 production 연결 전에 반드시 닫아야 한다. 그러나 안전한 release는
단순히 다음 SQL이 아니다.

```sql
DELETE FROM coordinator_node_reservations WHERE node_id = ?;
```

최소한 아래 중 하나가 durable하게 증명되어야 같은 node를 새 Job에 줄 수 있다.

- Grant bytes가 어떤 session에도 publish되지 않았다.
- Agent의 서명된 terminal Attempt report를 검증하고 canonical completion/failure를
  commit했다.
- revoke/stop handshake가 완료되어 runtime과 GPU 반환까지 확인됐다.
- 정책상 계속 실행할 수 없는 workload이고 Lease 만료+grace+fencing 조건이
  충족됐다.

현재 durable outbox, publish state, Attempt `STARTING/RUNNING/terminal` 상태,
completion ingress가 모두 없다. `CoordinatorLeaseStore::mark_revoked()`
(`lease_store.rs:322-359`)은 Coordinator의 판단을 저장할 뿐 이미 받은 Agent process를
정지시키지 않는다. 따라서 full release를 오늘 조각으로 택하는 것은 정직하지 않다.

## 오늘 착수할 최소 조각

### 이름

**조각 5c / selected GPU durable reservation binding**

### In

1. reservation-aware staging request에 `selected_gpu_ids`를 추가한다.
   `DoD-48`의 canonical 결과를 orchestration이 그대로 전달한다.
2. storage boundary에서 다음을 fail closed로 검증한다.
   - ID 목록이 비어 있지 않음(현재 v0.1 GPU Job profile)
   - 각 ID가 non-blank이고 유일함
   - 목록이 `gpu_id` 오름차순 canonical form임
   - 같은 transaction에서 CAS로 확인한 node의 현재
     `coordinator_agent_gpus`에 모든 ID가 존재함
3. `selected_gpu_ids`를 reservation의 durable child rows 또는 동등하게 검증 가능한
   canonical encoding으로 저장한다. 기존 reservation row와 구분되지 않는 암묵적
   빈 목록은 허용하지 않는다. migration 전 row는 unbound legacy reservation으로
   fail closed한다.
4. GPU ID 집합을 `staging_operation_idempotency.request_payload`에 포함한다.
   같은 operation key와 다른 GPU 집합은 `OperationConflict`다.
5. inventory revision 비교, 선택 GPU 존재 확인, reservation+GPU binding 삽입,
   Attempt/Lease/fence 생성, Job STAGING, operation 기록은 현재와 같은 하나의
   `BEGIN IMMEDIATE` transaction에서 일어난다.
6. `StoredNodeReservation`/`get_node_reservation()`이 재시작 후 canonical
   `selected_gpu_ids`를 복원한다.
7. ID binding 삽입 뒤 fault injection을 두어 transaction rollback 시 reservation,
   binding, Attempt, Lease, fence, Job, operation이 전부 남지 않음을 검증한다.

### Out

- snapshot `gpu_id`가 NVML UUID라는 provenance/attestation
- `GrantedExecutionPlan`, `ResourceScope`, `ExecutionGrant` 생성·서명·전송
- Lease store에 full resource scope 저장
- CPU/RAM/workspace 또는 GPU별 가용량 차감/accounting
- node-exclusive reservation을 GPU-exclusive/shared/MIG reservation으로 변경
- reservation-aware inventory projection과 CAS conflict rerank/retry
- reservation release/requeue/terminal cleanup
- Manifest ingress, 원본 저장, membership lookup, `JobRequirements` adapter
- session routing, durable outbox, ACK state machine, production `run()` 연결

### 완료 조건

1. single/N orchestration 모두 `DoD-48`의 exact canonical GPU ID 집합을 reservation에
   저장하고 재조회한다.
2. STAGING commit 뒤 store를 닫고 다시 열어도 node/job/attempt/revision과 같은
   `selected_gpu_ids`를 복원한다.
3. selected ID가 현재 CAS revision의 node inventory에 없으면 typed error이고
   Job은 QUEUED, reservation/Attempt/Lease/operation은 0건이다.
4. blank/duplicate/non-canonical ID 목록은 STAGING 전에 거부된다.
5. 같은 operation key + 같은 payload replay는 최초 GPU 집합을 반환한다.
6. 같은 operation key + 다른 GPU 집합은 `OperationConflict`이며 최초 결과를
   바꾸지 않는다.
7. 두 Job의 같은 node 경쟁은 기존처럼 정확히 하나만 성공하고, 성공한 row에만
   GPU binding이 있다.
8. binding insert 직후 fault는 reservation을 포함한 모든 staging side effect를
   rollback한다.
9. inventory revision 0과 `u64::MAX`, 복수 GPU ID에서 encoding/ordering이 보존된다.
10. 기존 reservation 없는 DoD-43 API와 기존 scheduler/coordinator 테스트는
    회귀하지 않으며 wire behavior는 바뀌지 않는다.
11. 뮤테이션에서 GPU ID를 operation payload에서 빼거나 inventory 존재 검사를
    우회하면 각각 conflict/negative test가 실패한다.

### 예상 변경 소유권

- `crates/coordinator/src/orchestrate.rs`
- `crates/coordinator/src/staging_store.rs`
- 두 module의 기존 unit/concurrency/fault-injection tests
- 구현 완료 시 별도 evidence/report/history

`crates/scheduler`, `proto`, production `run()`은 이 조각에서 바꾸지 않는다.

## 이 조각 뒤의 정직한 순서

```text
오늘: selected GPU IDs + inventory revision + reservation + STAGING의 durable binding
  -> attested live inventory의 GPU UUID provenance
  -> verified original Manifest durable 저장 + strict JobRequirements adapter
  -> full resource assignment/Lease scope + Grant plan builder
  -> durable Grant outbox + node/device/session routing
  -> publish/ACK/timeout 상태와 안전한 pre-send abort release
  -> terminal report/runtime-stop 사실에 결합한 completion/failure release
  -> 그 뒤에만 production run()/wire 연결
```

Manifest 작업과 UUID provenance는 병행할 수 있다. 그러나 durable assignment 없이
Grant를 만들거나, runtime 종료 사실 없이 reservation을 지우는 순서는 허용하지 않는다.

## 구현 결과 (2026-08-24 11:18)

### 구현

- `StageQueuedRequest.selected_gpu_ids`와
  `StoredNodeReservation.selected_gpu_ids`를 추가했다. 예약 없는
  `stage_queued_with_lease()`는 이 필드를 의도적으로 무시하며 빈 목록으로도 기존
  staging 계약이 통과한다.
- `coordinator_node_reservation_gpus(node_id, gpu_id, ordinal)` child table을 추가했다.
  `node_id`별 ordinal과 GPU ID는 각각 유일하며, ordinal 순서가 canonical ID 순서를
  보존한다. 기존 reservation row에 child row가 없으면 legacy unbound row로 간주해
  `CorruptData`로 fail closed한다.
- reservation-aware 경로는 비어 있음·공백 ID·중복·비정렬을 거부한다. 선택 ID는
  operation payload에 count와 length-delimited bytes로 포함하며, inventory revision CAS와
  같은 `BEGIN IMMEDIATE` transaction에서 해당 node의 GPU child row 존재를 확인한다.
- 같은 transaction 안에서 node reservation → GPU binding → Attempt/Lease/fence → Job
  STAGING → operation row를 쓴 뒤 한 번만 commit한다. GPU binding 직후 fault injection은
  reservation/binding/Attempt/Lease/fence/Job/operation 전부가 rollback됨을 확인한다.
- orchestration의 single/N 후보 경로 모두 DoD-48의 canonical `selected_gpu_ids`를
  `StageQueuedRequest`로 그대로 전달하고, 저장된 reservation에서 같은 목록을 재조회한다.

### 검증

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS.
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS
  (실행된 모든 workspace unit/integration/doc test, 0 failed; 기존 ignored 1건 유지).
- `cargo test -p gputeer-coordinator`: PASS
  (unit 88 + integration 4 = 92 passed, 0 failed).
- negative/fault/concurrency:
  - empty/blank/duplicate/non-canonical selected IDs와 다른 node에만 존재하는 GPU ID 거부
  - missing/blank/non-canonical/gapped durable binding 손상 fail-closed
  - changed GPU payload operation conflict와 exact replay/reopen 복원
  - binding 직후 injected fault의 전체 rollback
  - 두 connection `Barrier` 경쟁에서 reservation과 GPU binding이 정확히 한 winner에만 존재
  - revision 0, `u64::MAX`, 복수 GPU canonical ordering round-trip
  - reservation-free API가 빈 selected GPU 목록으로 기존 staging 성공
- mutation 1: reserved operation payload에서 GPU count/IDs를 제거하면
  `selected_gpu_binding_is_in_operation_payload_and_replay_returns_original`이 실패했다.
- mutation 2: inventory GPU 존재 검사를 우회하면
  `invalid_or_missing_selected_gpu_ids_fail_before_staging`이 실패하고 존재하지 않는 GPU가
  STAGING되는 반례를 검출했다. 두 mutation은 즉시 원복했다.

### 자체 재검토

inventory 존재 쿼리가 단순 전역 GPU ID 존재가 아니라 `(node_id, gpu_id)` 쌍을 검사함을
직접 증명할 필요가 있음을 발견했다. negative fixture에 동일 GPU ID를 다른 node에만
등록하는 경우를 추가했고, 선택 node에서는 `SelectedGpuMissing`으로 거부됨을 재검증했다.
release, Grant/scope, GPU별 capacity accounting, UUID provenance, production wire는 계획의
Out 경계를 지켜 변경하지 않았다.

### 제한

이 환경의 stable toolchain에는 `cargo-fmt`/`rustfmt` component가 설치되어 있지 않아
`cargo fmt`는 실행되지 않았다(`cargo-fmt.exe is not installed`). `git diff --check`와 모든
요청 build/test는 통과했다. 이 결과는 local SQLite transaction의 durable binding만
증명하며 Raft `COMMITTED`, GPU UUID provenance, Grant 전송 또는 안전한 release를
증명하지 않는다.
