# 2026-08-21_1502 scheduler Grant dispatch v1

- **대상:** scheduler 로드맵 조각 5를 하루 규모로 재범위
- **선행 완료:** DoD-41, DoD-42, DoD-43, DoD-44, DoD-45
- **조사 방식:** 현재 코드와 기존 계획의 계약을 read-only로 대조
- **판정:** 네 API를 순서대로 부르는 로컬 application-service는 하루 안에
  가능하다. 그러나 이것은 순수 함수도, 실제 Grant dispatch도 아니다. 실제
  accept-loop 연결은 이번 하루 조각에서 제외한다.

## 결론

로드맵 조각 5의 원래 범위는 다음 전부다.

```text
선택 결과 + durable Attempt/Lease + 특정 live session
  -> Manifest/hash/plan/rationale/Lease가 채워진 Grant
  -> 전송
  -> ACK/거부/timeout
  -> 성공 확정 또는 reservation/Lease 정리
```

로드맵 자체도 production Rust 700~1,150줄, 테스트 700~1,150줄, 3~5일로
잡았다. 현재 구현으로 하루 안에 가능한 것은 아래 로컬 경로뿐이다.

```text
InventoryStore::pool_snapshot(now)              // SQLite read
  -> evaluate_eligibility(pool, job, hard_policy) // pure
  -> 0 / 1 / multiple 분기
       0: QUEUED 유지, NoEligible 반환
       1: 해당 node_id 직접 선택
       N: rank_best_fit(...).winner.node_id       // pure
  -> StageQueuedRequest의 node_id 채움
  -> StagingStore::stage_queued_with_lease(...)   // SQLite write
```

따라서 이것은 “순수 함수 여러 개를 순서대로 호출하는 조합 함수”가 아니다.
앞뒤가 서로 다른 SQLite transaction이고 마지막 호출은 Job을 `STAGING`으로
바꾼다. 정확한 이름은 **로컬 placement-to-staging orchestration**이다.

또한 현재 조각 4에서 끝난 것은 순수 best-fit뿐이다. 로드맵 조각 4 원안의
inventory revision/CAS resource reservation은 아직 없다. 위 orchestration은 같은
node/GPU를 두 Job이 동시에 선택하는 것을 막지 못하므로 실제 admission 또는
Grant dispatch 완료로 부르면 안 된다.

## 코드로 대조한 실제 계약

### 1. `pool_snapshot()` -> `evaluate_eligibility()`

이 경계는 현재 타입상 직접 맞는다.

- 실제 함수명은 가칭 `project_pool_snapshot()`이 아니라
  `CoordinatorInventoryStore::pool_snapshot(evaluated_at_unix_ms)`다.
- registry와 inventory를 한 SQLite read snapshot에서 읽고 scheduler의
  `PoolSnapshot`을 바로 반환한다.
- `project_candidate()`는 node/owner/state/risk/security/isolation/key,
  observation time, GPU health/model/available VRAM, CPU/RAM/workspace,
  workload allowlist와 third-party opt-in을 `CandidateSnapshot`의 대응 필드로
  투영한다.
- inventory가 없으면 telemetry 필드를 `None`으로 두므로 hard filter가
  `MissingFact`로 fail-closed한다. 미래 timestamp도 filter가 stale로 거부한다.
- coordinator의 inventory 테스트가 이미 투영 결과를
  `evaluate_eligibility()`에 직접 넣어 이 경계의 기본 호환성을 증명한다.

다만 snapshot에는 저장돼 있는 `inventory_revision`, `device_id`, verifying key,
session owner가 없다. eligibility에는 충분하지만 reservation과 실제 session
routing에는 부족하다.

### 2. `evaluate_eligibility()` -> `rank_best_fit()`

직접 맞지만 **복수 적격일 때만** 맞는다.

- `rank_best_fit()`는 `EligibilityResolution::RankingRequired`와 적격 후보 2개
  이상을 강제한다.
- `NoEligibleCandidates`에서 호출하면 오류이고 Job을 영구 실패시켜서도 안 된다.
  빈 pool이나 모두 stale인 상황은 일시적일 수 있으므로 QUEUED를 유지해야 한다.
- `SingleEligible`에서는 rank를 호출하지 않고 report의 `node_id`를 직접 써야 한다.
- 복수 후보에서는 같은 `pool`, `job`, 방금 산출한 `report`를 넘기면 후보 집합과
  자원 사실 검증을 통과한다. 오래 보관한 report나 다른 snapshot을 섞으면
  `RankingError`로 닫힌다.

즉 네 함수를 무조건 일렬 호출하는 구현은 틀리고, resolution 분기가 공개 계약에
들어가야 한다.

### 3. 선택 결과 -> `stage_queued_with_lease()`

선택 결과의 `node_id: String`은 `StageQueuedRequest.node_id`와 직접 맞는다.
그러나 나머지는 scheduler가 생산하지 않는다.

`StageQueuedRequest`에는 추가로 다음 값이 필요하다.

- 16바이트 operation key
- job/attempt/lease ID
- issuing coordinator ID와 coordinator term
- issued/renew-after/expires 시각
- max total duration

clock, ID와 lease lifetime 정책 producer는 현재 이 파이프라인에 없다. 하루 조합
함수는 이 값을 임의로 생성하면 안 되며, typed issuance input으로 호출자에게 받아야
한다. `stage_queued_with_lease()`가 반환한 fence epoch만이 저장소가 원자적으로
할당하는 값이다.

더 중요한 차이는 staging 요청에 GPU ID, 자원량, inventory revision이 전혀 없다는
점이다. 이 transaction은 inventory/allocation row를 읽지 않는다. 그러므로
`QUEUED -> STAGING`, Attempt, fence epoch, Lease는 서로 원자적이지만 선택한 자원은
예약되지 않는다.

### 4. `StageQueuedResult` -> 실제 `ExecutionGrant`

여기에는 실제 계약 구멍이 있다.

- `StageQueuedResult`의 `StoredLease`는 Lease identity와 fence epoch를 제공한다.
  기존 coordinator에는 이를 protobuf Lease로 옮기는 유사 코드와
  `u64 -> u32 max_total_duration_seconds` checked conversion도 있다.
- 그러나 `StoredJob`은 manifest 본문이 아니라 32바이트 manifest hash만 저장한다.
  `ExecutionGrant`는 submitter 서명을 포함한 원본 `JobManifest`가 반드시 필요하다.
- manifest에서 scheduler `JobRequirements`를 만드는 adapter가 없다. 특히
  submitter device를 member ID로 해석할 membership producer가 없고, proto3의
  unspecified/zero와 scheduler의 `Option`/명시 기본값을 정하는 규칙도 코드에 없다.
- `[u8; 32]` manifest hash를 `Digest { algo: BLAKE3_256, value }`로 만드는 adapter와
  전달받은 manifest를 재해시해 저장 hash와 대조하는 경계가 없다.
- `rank_best_fit()` 결과는 node와 aggregate `FitKey`뿐이다. 내부에서 tight GPU
  subset을 계산하지만 선택된 GPU UUID를 반환하지 않는다. 따라서
  `GrantedExecutionPlan.assigned_gpu_uuids`와 `Lease.scope.gpu_uuids`를 만들 수 없다.
- checkpoint/durability admission과 확정 execution mode producer가 없으므로
  `GrantedExecutionPlan`의 나머지 필드도 만들 수 없다.
- scheduler의 rejection reason과 proto `PlacementRationale`/`RejectedCandidate`를
  잇는 adapter가 없고, estimate/survival/confidence 값의 producer도 없다.
- 현재 `issue_grant()`는 CLI config에서 ID를 받아 stub Lease를 만들며,
  `ExecutionGrant`의 manifest, manifest_hash, plan을 `Default`로 비워 둔다. 현재
  scheduler/job/inventory/staging store를 전혀 사용하지 않는다.

따라서 stage 결과만 얻었다고 “내용이 찬 Grant”를 만들 수 없다.

### 5. 선택 node -> 특정 Agent session

이 경계에는 단순 누락을 넘어 실제 identity 불일치가 있다.

- inventory registry는 `node_id`와 `device_id`를 별도 필드로 저장하며 둘이 같다는
  invariant가 없다.
- scheduler와 staged Lease holder는 `node_id`를 쓴다.
- 현재 CoordinatorConfig는 `agent_device_id` 하나를 ACK의
  `AgentGrantAck.agent_device_id` 대조와 Lease의 `holder_node_id` 양쪽에 사용한다.
- 따라서 `node_id != device_id`인 정상 registry row를 고르면, 현재 stub에 그대로
  연결할 때 staged Lease와 wire Lease의 holder가 달라지거나 ACK identity 대조가
  틀어진다.

실제 dispatch는 `selected node_id -> registry(device_id, verifying_key) -> 현재
session owner/session_id/socket`의 명시적 lookup이 필요하다. 현재는 active session
registry나 socket owner fencing이 없다.

## 원자성과 실패 복구의 남은 구멍

SQLite commit과 TCP write/ACK는 하나의 원자 transaction이 될 수 없다. 현재
staging row에는 `grant_id`, 완성 Grant bytes/hash, target session ID, dispatch
state, send attempt가 없다. 따라서 다음 crash window를 복구할 수 없다.

```text
STAGING commit 성공
  -> process crash 또는 session 부재
  -> Grant가 보내졌는지 알 수 없음
```

또한 현재 API에는 다음 전이가 없다.

- Grant ACK 성공을 durable하게 기록하고 Attempt를 다음 상태로 전이
- `accepted=false`, write failure, ACK timeout 때 Attempt/Job을 정리하거나 재큐잉
- 실패한 allocation release
- dispatch 재시도의 same-Grant idempotency와 새-Grant 발급 구분

`AgentGrantAck`의 `accepted`는 bool뿐이고 거부 reason도 없다. lease revoke API는
있지만 Job은 이미 STAGING이며 Attempt state는 `Created` 하나뿐이다. revoke만으로
일관된 rollback이 되지 않는다.

정직한 actual dispatch에는 최소 transactional outbox 또는 그와 동등한 durable
dispatch record가 필요하다. stage transaction 안에 대상 node/session identity와
Grant identity/payload를 기록하고, sender가 이를 재개 가능하게 소비해야 한다.

## Coordinator accept-loop를 이번에 제외하는 이유

현재 loop는 “수신한 Job 요청을 scheduling”하는 loop가 아니다.

- listener가 받는 상대는 Agent이고, 기본 경로는 연결 직후 server-first stub Grant를
  보낸다. `Manifest` frame decoder는 있지만 coordinator session handler는 Job
  submission을 받지 않는다.
- config에 agent verifying key와 agent/grant/attempt/job/lease ID가 하나씩 고정돼
  있다. inventory에서 winner를 찾거나 복수 active session을 routing하지 않는다.
- accept는 순차적이며 connection-local socket을 장기 session registry에 등록하지
  않는다.
- ACK 검증은 wire 수준 상관관계만 확인하고 job/attempt durable state를 바꾸지 않는다.

이를 scheduler에 직접 연결하는 것은 작은 callback 추가가 아니라 session registry,
job ingress, durable outbox, ACK state machine과 release 정책을 함께 만드는 작업이다.
로드맵의 3~5일 추정 쪽에 가깝고 하루 범위가 아니다.

## 오늘 착수할 최소 조각

### 이름

**DoD-46 후보: local placement-to-staging orchestration kernel**

이 이름에는 의도적으로 “Grant dispatch”를 쓰지 않는다. 조각 5의 첫 접착점이지만
wire dispatch는 아니다.

### In

- coordinator 내부 application-service 하나에서 현재 네 API를 실제로 호출한다.
- 입력은 다음을 명시적으로 받는다.
  - `job_id`와 이미 검증·정규화된 `JobRequirements`
  - hard-filter `Policy`, `BestFitPolicy`
  - `evaluated_at_unix_ms`
  - operation/attempt/lease ID, coordinator identity/term, Lease lifetime을 담은
    caller-supplied issuance input
- `pool_snapshot(now)`을 정확히 한 번 읽고 그 동일 값으로 filter/rank한다.
- 0/1/N resolution을 위 규칙대로 분기한다.
- 선택된 node만 issuance input에 결합해 `StageQueuedRequest`를 만들고
  `stage_queued_with_lease()`를 정확히 한 번 호출한다.
- 결과에는 eligibility report, optional ranking, selected node와
  `StageQueuedResult`를 보존한다. storage/ranking 오류를 문자열로 뭉개지 않는다.
- 실제 파일 기반 동일 control DB를 사용하는 integration test에서 projection부터
  durable STAGING/Attempt/Lease까지 검증한다.

### 안전 표기

이 함수는 현재 resource reservation proof를 받을 수 없다. 따라서 첫 구현은
crate-internal pre-dispatch seam으로 두고 accept-loop에서 호출하지 않는다. 주석과
타입 문서에 다음을 명시한다.

```text
NOT resource reservation
NOT Grant construction
NOT network dispatch
NOT safe for concurrent multi-Job admission
```

조각 4 잔여인 revision/CAS allocation이 생기면 이 application-service의 staging
직전에 붙이거나 같은 control-DB transaction으로 합쳐야 한다. 그 전에는 production
entrypoint에서 공개 호출하지 않는다.

### Out

- JobManifest 수신·검증·보관과 `JobRequirements` adapter
- inventory revision token, GPU allocation/CAS, resource release/retry
- selected GPU UUID와 `GrantedExecutionPlan`/`PlacementRationale` 생성
- full `ExecutionGrant` 생성·서명
- active session registry, node/device/session routing
- TCP send, ACK/거부/timeout, durable dispatch outbox/state machine
- STAGING rollback/requeue와 Lease/allocation 정리
- Coordinator `run()`/accept-loop 연결
- “실제 Grant dispatch 완료” 또는 roadmap 조각 5 완료 주장

### 완료 조건

```text
1. inventory projection 결과를 별도 재구성 없이 hard filter에 넣는다.
2. 0 후보는 stage하지 않고 QUEUED를 유지한다.
3. 1 후보는 rank를 호출하지 않고 그 node를 stage한다.
4. 복수 후보는 rank winner와 Attempt node/Lease holder가 정확히 같다.
5. 같은 operation key 재호출은 staging store의 기존 durable 결과를 돌려준다.
6. malformed policy/ranking/storage 오류는 fail-closed하고 stage 성공으로 가장하지 않는다.
7. snapshot은 호출당 한 번만 읽으며 report와 rank에 같은 값을 쓴다.
8. public accept-loop와 wire behavior는 바뀌지 않는다.
9. 문서와 API가 reservation/dispatch 보장을 주장하지 않는다.
```

### 정직한 규모

- production Rust 약 100~180줄
- integration/unit test 약 180~300줄
- 계획/evidence 별도, 1일

이 규모를 넘기기 시작하면 오늘은 resolver와 real-store contract test까지만 남기고
production staging wrapper는 다음으로 넘긴다. 특히 CAS reservation이나 active
session routing을 같은 DoD에 끼워 넣지 않는다.

## 후속 순서

```text
오늘 DoD-46: local placement -> STAGING contract seam
  -> 조각 4 잔여: inventory revision + GPU/resource allocation CAS
  -> manifest 보관/JobRequirements + selected GPU plan/rationale adapter
  -> durable Grant outbox + node/device/session owner routing
  -> send/ACK/거부/timeout + release/requeue
  -> accept-loop/E2E 연결
```

로드맵의 원래 조각 5 완료 판정은 마지막 두 단계까지 끝난 뒤에만 가능하다.

## 구현 결과 (2026-08-21)

`crates/coordinator/src/orchestrate.rs`에 crate-internal
`orchestrate_placement_to_staging()`을 추가했다. 이 함수는 호출자가 제공한
`PlacementToStagingInput`/`StagingIssuanceInput`으로 다음 경로만 조합한다.

```text
pool_snapshot(evaluated_at_unix_ms) 1회
  -> evaluate_eligibility()
  -> 0: NoEligible, stage 없음
     1: report의 node_id 직접 선택, rank 없음
     N: rank_best_fit() winner 선택
  -> StageQueuedRequest
  -> stage_queued_with_lease() 1회
```

결과는 `PlacementToStagingOutcome` enum으로 `NoEligible`와 `Staged`를 분리해
0 후보가 staging 성공처럼 표현되지 못하게 했다. `Staged`는 eligibility report,
optional ranking, selected node와 원본 `StageQueuedResult`를 보존한다. 오류도
`PlacementToStagingError::{Inventory, Ranking, Staging}`으로 원래 typed error를
보존한다.

### 조사된 계약 불일치 7건의 처리

1. **해결:** `EligibilityResolution`의 0/1/N을 명시적으로 분기한다.
   `rank_best_fit()`은 `RankingRequired`에서만 호출한다.
2. **해결:** scheduler가 만들지 않는 operation/attempt/lease/coordinator ID와
   term, 발급·갱신·만료 시각, 최대 수명을 caller-supplied
   `StagingIssuanceInput`으로 받는다. kernel은 ID/clock/policy를 만들지 않는다.
3. **범위 밖:** ranking이 selected GPU UUID를 반환하지 않으므로 node만 stage한다.
   GPU scope, `GrantedExecutionPlan`, `PlacementRationale`은 만들지 않는다.
4. **범위 밖:** 원본 Manifest 보관과 `JobRequirements` adapter가 없으므로 이미
   검증·정규화된 `JobRequirements`를 입력으로 받으며 Grant를 만들지 않는다.
5. **범위 밖:** node/device/session routing을 추가하지 않았다. 테스트 registry도
   `node_id != device_id`인 정상 행을 사용하며 stage의 Attempt/Lease에는 선택한
   `node_id`만 들어간다. ACK identity나 socket을 추론하지 않는다.
6. **범위 밖, production 미노출로만 격리:** inventory revision/allocation CAS를
   추가하지 않았다. 모듈은 private이고 accept-loop/production entrypoint에 호출부가
   없다. 이것은 단순히 “현재 loop가 순차적”이라 안전한 것이 아니다. inventory가
   갱신되지 않으면 서로 다른 Job을 순차 호출해도 같은 node/GPU를 다시 고를 수 있다.
   따라서 resource reservation이나 multi-Job admission 안전성을 주장하지 않는다.
7. **범위 밖:** accept-loop, wire Grant, ACK/거부/timeout, rollback/requeue,
   Lease/allocation 정리를 전혀 바꾸지 않았다. `lib.rs` 변경은 private module 선언뿐이다.

### 테스트와 뮤테이션

실제 임시 파일의 같은 control DB를 Job/Inventory/Staging store가 함께 열어 다음 7건을
검증했다.

- 0 후보: QUEUED 유지, Attempt/epoch 없음
- 1 후보: ranking 없이 해당 node stage
- 복수 후보: best-fit winner가 selected node/Attempt node/Lease holder와 일치
- 동일 입력·동일 inventory의 operation replay: 기존 durable 결과, epoch 추가 소비 없음
- 중복 ranking axis: typed `RankingError`, staging 없음
- 손상 inventory payload: typed `InventoryStoreError`, staging 없음
- 잘못된 Lease lifetime: typed `StagingStoreError`, 부분 상태와 epoch 소비 없음

수동 뮤테이션은 각각 적용 후 대응 테스트가 실패하는 것을 확인하고 원복했다.

1. 단일 후보에서도 `rank_best_fit()` 호출: `ResolutionNotRankingRequired`로 단일 후보
   테스트 실패.
2. 복수 후보에서 ranking winner 대신 첫 eligible node 선택: 기대 `node-b`, 실제
   `node-a` 불일치로 best-fit 테스트 실패.

### 검증과 제한

- `cargo build --workspace --exclude gputeer-runtime-windows`: 성공.
- `cargo test --workspace --exclude gputeer-runtime-windows`: 449 passed,
  0 failed, 1 ignored.
- 새 orchestration 단위 테스트: 7 passed, 0 failed.
- 최종 원복 뒤 위 workspace 검증을 다시 통과했다.
- 이 환경의 stable toolchain에는 `rustfmt` component가 없어 `cargo fmt`는
  `cargo-fmt.exe is not installed`로 실행 불가했다. 빌드·테스트는 정상 수행했다.
- operation replay 검증은 계획의 동일 호출 조건대로 동일 scheduler 입력과 unchanged
  inventory에서 수행했다. retry 전에 inventory가 달라졌을 때 기존 operation을 선택보다
  먼저 조회하는 API는 현재 없으므로, 그 경우의 orchestration-level replay 의미는 이
  조각이 추가로 보장하지 않는다. staging store 자체의 exact-request replay 계약은 유지된다.
- 이 절은 구현·로컬 검증 결과이며 독립 검수 evidence가 아니다. 실제 Grant dispatch나
  roadmap 조각 5 완료를 주장하지 않는다.
