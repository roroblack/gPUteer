# 2026-08-21_1036 scheduler selected GPU assignment v1

- **조사 대상:** `DoD-46` 로컬 orchestration kernel의 production 연결 가능성
- **선행 완료:** `DoD-41`~`DoD-47` 중 사용자 요약에 적힌 조각
- **오늘 착수 결론:** production `run()` 연결이 아니라 **조각 4b / selected GPU
  assignment 순수 kernel**
- **예상 규모:** production Rust 80~160줄, scheduler/coordinator test 140~260줄,
  문서/evidence 별도 — 1일
- **proto 변경:** 없음. 기존 `ResourceScope.gpu_uuids`와
  `GrantedExecutionPlan.assigned_gpu_uuids`를 아직 만들거나 전송하지 않는다.

## 결론

`DoD-47`은 `DoD-46`이 production 미연결의 직접 이유로 든 “서로 다른 Job이
같은 node/GPU를 중복 선택”하는 위험을 **node-exclusive CAS admission**으로 닫았다.
그러나 이것만으로 현재 `run()`에 orchestration을 연결할 수는 없다.

`DoD-46`의 나머지 범위 밖 3건은 모두 실제 dispatch 경로에 필요하다.

| 남은 항목 | 실제 연결에 필수인가 | 근거와 정확한 경계 |
|---|---|---|
| 선택 GPU 식별자, Grant plan/scope | **필수** | ranking은 node와 aggregate `FitKey`만 반환한다(`crates/scheduler/src/model.rs:260-270`, `crates/scheduler/src/rank.rs:125-186`). 따라서 Agent가 사용할 GPU와 Lease가 허가하는 GPU를 같은 값으로 만들 수 없다. 완전한 확률 기반 `PlacementRationale`은 첫 안전 dispatch의 선행 조건이 아니지만, 선택 GPU 집합과 명시적 execution/allocation mode·durability 값은 빈 protobuf 기본값으로 보내면 안 된다. |
| 원본 Manifest 저장과 `JobRequirements` 변환 | **필수** | durable Job row에는 원본이 아니라 hash만 있다(`crates/coordinator/src/job_store.rs:101-112`). `ExecutionGrant.manifest`는 submitter 서명을 포함한 원본을 전달해야 한다(`proto/job.proto:103-119`). 또한 scheduler가 요구하는 submitter member ID, enum의 unspecified 처리와 명시 기본값을 만드는 변환 계약이 없다. |
| node/device/session routing | **필수** | scheduler/Lease는 `node_id`, ACK 검증은 `agent_device_id`를 사용하며 둘이 같다는 invariant가 없다. 선택 node를 registry의 device/key와 현재 session/socket owner로 연결하지 않으면 올바른 Agent에게 보내거나 ACK identity를 검증할 수 없다. |

따라서 “orchestration 함수 한 번 호출 후 기존 `issue_grant()`로 전송”은 안전한
연결이 아니다. 현재 stub의 고정 ID를 scheduler 결과로 덮어쓰는 방식도 금지한다.

## 현재 `run()`/accept-loop 판독

현재 production entrypoint는 Job 수신·queue 처리 loop가 아니다.

1. `CoordinatorConfig`가 agent key/device와 grant/attempt/job/lease ID를 각각 한
   개씩 CLI 입력으로 받는다(`crates/coordinator/src/lib.rs:43-87`).
2. `run()`은 단일 key를 `InMemoryKeyring`에 넣고 TCP 연결을 순차 accept한다
   (`lib.rs:320-360`). 복수 active session registry나 per-session owner는 없다.
3. 기본 session은 상대의 역할이나 Job을 먼저 읽지 않고, 연결 직후
   `issue_grant()`로 server-first stub Grant를 만든 뒤 전송한다
   (`lib.rs:464-527`).
4. ACK는 wire 상관관계만 대조한다(`lib.rs:532-560`). durable Job/Attempt 상태를
   다음 상태로 옮기지 않는다.
5. `Manifest` frame decoder와 proto `SubmitJob`/`SubmitJobResult` 모양은 이미
   존재한다. 그러나 coordinator session handler에는 Manifest submit 분기, submitter
   key lookup, 제출 응답 frame/lane, Job store 연결이 없다. 그러므로 “proto가 전혀
   없다”는 표현은 부정확하지만, **사용 가능한 Job submit wire 경로는 없다.**

실제 연결 전에 최소한 다음 application/wire 경계가 선행돼야 한다.

```text
signed Manifest ingress + submitter identity/key lookup
  -> 원본 Manifest/hash durable 저장 + JobRequirements 변환
  -> queue consumer + orchestration/CAS admission
  -> 완성 Grant bytes와 target session을 같은 transaction에 기록하는 outbox
  -> selected node -> device/key -> fenced active session/socket routing
  -> send/ACK/거부/timeout 상태 전이
  -> reservation/Lease release + requeue 또는 terminal failure
```

이 가운데 SQLite commit과 TCP send 사이 crash window는 callback 하나로 해결할 수
없다. `DoD-47` reservation에는 release API도 없으므로 지금 연결하면 send 실패나
ACK timeout 한 번으로 node가 영구 점유될 수 있다. CAS mismatch/점유 충돌의 bounded
rerank도 아직 없다.

## 규모 판단

**실제 production 연결은 하루 규모를 넘는다.** 기존 로드맵의 조각 5 추정인
3~5일보다 작다고 볼 새 근거가 없다. 오히려 현재 조사에서 다음이 그대로 확인됐다.

- Job submit용 protobuf 구성요소는 일부 있지만 실제 ingress/response wire 경로와
  제품 CLI가 없다.
- `DoD-44`는 durable inventory repository이지 heartbeat/capability wire producer나
  active session registry가 아니다.
- durable outbox, ACK state machine, release/requeue가 없다.
- Agent는 Grant/Lease 검증과 WRITING marker까지만 수행하고 entrypoint는 실행하지
  않는다(`crates/agent/src/lib.rs:192-201`). 이는 dispatch 연결 뒤의 로드맵 조각 6
  범위이며, 빈 Grant를 먼저 연결할 이유가 되지 않는다.

따라서 오늘 `run()`을 고치는 것은 범위 축소가 아니라 미완성 ingress·routing·복구
정책을 임의로 발명하는 일이 된다.

## 후보 비교

| 후보 | 오늘 조각으로 적합한가 | 판단 |
|---|---|---|
| Agent entrypoint 실행 하위 조각 | 아니오 | 유효한 Manifest/plan/scope와 V-06 정책 강제 경계가 아직 소비자에게 도달하지 않는다. isolated command runner 같은 더 작은 kernel은 만들 수 있으나 지금 scheduler의 가장 가까운 blocker를 닫지 않는다. |
| Manifest → `JobRequirements` 변환기 | 아직 아님 | proto 필드 매핑 자체는 작지만 submitter device→member lookup, unspecified/default 규칙, 원본 Manifest 저장·재해시 경계를 먼저 계약해야 한다. 이를 생략한 변환기는 hard-filter에 잘못된 사실을 공급한다. |
| selected GPU 식별자 반환 | **예** | inventory와 deterministic best-fit kernel이 이미 있고, 현재 ranking이 내부에서 버리는 식별자만 명시적으로 보존하면 된다. filesystem/network 없이 판별 가능한 하루 조각이다. |
| production accept-loop 연결 | 아니오 | ingress, session routing, outbox, ACK/cleanup이 함께 필요해 1일을 넘는다. |

## 오늘 착수할 최소 조각

### 이름

**조각 4b / deterministic selected GPU assignment kernel**

이 조각은 “GPU allocation”, “Grant scope 완성”, “조각 5 production 연결”이라고
부르지 않는다. scheduler snapshot 안의 `gpu_id`를 선택 결과로 잃지 않게 만드는
순수 계산 조각이다. live inventory producer가 이 값을 실제 NVML GPU UUID로
검증·공급하는 계약은 별도이므로, wire 수준 UUID provenance까지 주장하지 않는다.

### In

1. scheduler에 순수 resource-fit 결과를 추가한다.

   ```text
   ResourceFit {
     fit_key,
     selected_gpu_ids,
   }
   ```

2. GPU 후보는 기존 hard-filter/ranking과 같은 조건을 사용한다.

   - `healthy == true`
   - allowed model 일치(목록이 비면 제약 없음)
   - `available_vram_bytes >= minimum_vram_bytes_per_gpu`
   - `gpu_id`는 비어 있지 않고 같은 candidate 안에서 유일해야 하며, 위반은
     typed error로 fail closed

3. 필요한 개수만큼 GPU를 결정적으로 선택한다.

   ```text
   (available_vram_bytes 오름차순, gpu_id 오름차순)
   ```

   첫 축은 기존 tight-VRAM 의미를 보존하고, 같은 VRAM에서는 `gpu_id`로 완전한
   순서를 만든다. 선택된 ID 목록 자체는 `gpu_id` 오름차순으로 정규화해 downstream
   plan/scope의 canonical 입력으로 쓸 수 있게 한다.

4. `rank_best_fit()`의 각 `RankedCandidate`가 node/`FitKey`와 그 node에서 선택한
   GPU ID 집합을 함께 반환한다. GPU ID를 제거해도 node ranking 결과가 우연히 같은
   테스트만으로 통과하지 않게 한다.
5. 단일 후보와 복수 후보가 서로 다른 GPU 선택 알고리즘을 갖지 않도록 같은 순수
   helper를 사용한다. orchestration outcome은 선택 node의 GPU ID 집합을 보존한다.
6. 기존 node-exclusive CAS request/schema는 바꾸지 않는다. 오늘 결과는 다음
   GPU별 reservation/Grant-scope 조각의 입력일 뿐이다.

### Out

- protobuf `GrantedExecutionPlan`, `ResourceScope`, `ExecutionGrant` 생성/서명
- GPU별/CPU/RAM/workspace allocation row와 accounting
- node-exclusive reservation을 GPU-exclusive로 변경
- active reservation을 뺀 snapshot projection, CAS 실패 rerank/retry
- reservation release/requeue
- Manifest 저장/adapter와 membership lookup
- session registry/routing, outbox, send/ACK
- Agent entrypoint 실행
- public `run()`/accept-loop 변경

### 완료 조건

1. 두 GPU가 모두 적격이면 더 작은 VRAM 잔여를 만드는 GPU ID가 선택된다.
2. VRAM이 같으면 입력 vector 순서와 무관하게 작은 `gpu_id`가 선택된다.
3. `minimum_gpu_count > 1`이면 정확한 개수의 서로 다른 ID를 반환한다.
4. GPU vector를 뒤집어도 `FitKey`, node 순위, 선택 ID 집합이 모두 같다.
5. single-candidate와 ranking winner가 동일 snapshot/job에 대해 동일한 resource-fit
   helper 결과를 사용한다.
6. missing health/model/VRAM, 중복 또는 빈 GPU ID는 기존 fail-closed 경계를
   약화하지 않는다. coordinator inventory의 빈/중복 ID 거부도 회귀하지 않는다.
7. 선택 ID 수가 요구 개수와 다르면 orchestration은 STAGING 전에 typed error로
   실패하고 Job을 QUEUED로 유지한다.
8. 기존 scheduler/coordinator 테스트가 통과하며 wire behavior는 바뀌지 않는다.
9. 뮤테이션 검증에서 `(VRAM, gpu_id)` 정렬의 `gpu_id` tie-break 제거 또는 선택 ID
   반환 제거가 최소 한 negative test를 실패시킨다.

### 예상 변경 소유권

- `crates/scheduler/src/model.rs`
- `crates/scheduler/src/rank.rs`
- `crates/scheduler/src/lib.rs`
- `crates/scheduler/tests/best_fit.rs`
- `crates/coordinator/src/orchestrate.rs`와 해당 module test
- 구현 완료 시 계획 규칙에 따른 evidence/report/history

오늘 설계 조사 산출물은 이 계획서 하나뿐이며 production 코드는 변경하지 않는다.

## 이 조각 뒤의 순서

```text
selected GPU assignment
  -> GPU IDs + resource amounts를 CAS reservation/Lease scope에 원자 반영
  -> release/requeue + durable Grant outbox
  -> 원본 Manifest 저장/검증 + JobRequirements/plan adapter
  -> active session registry와 node/device/session routing
  -> send/ACK/timeout state machine
  -> 그 뒤에만 run()/wire E2E 연결
```

Manifest와 session 작업은 일부 병행할 수 있지만, outbox와 release 없이 production
send를 먼저 노출하지 않는다.

## 구현 결과 (2026-08-24)

### 구현

- `crates/scheduler/src/model.rs`에 `ResourceFit { fit_key, selected_gpu_ids }`를
  추가하고 `RankedCandidate`가 선택 GPU ID를 보존하도록 확장했다. 빈/중복 GPU ID와
  선택 개수 불일치를 `RankingError`의 typed variant로 fail closed 한다.
- `crates/scheduler/src/rank.rs`의 순수 `resource_fit()`이 기존 hard-filter/ranking과
  같은 health/model/VRAM 조건을 적용한다. 적격 GPU를
  `(available_vram_bytes, gpu_id)` 오름차순으로 정렬해 요구 개수만 선택하고,
  반환 ID 목록은 `gpu_id` 오름차순으로 정규화한다. 기존 `FitKey`의 VRAM 잔여도
  바로 이 선택 집합에서 계산한다.
- `rank_best_fit()`은 모든 적격 candidate에 `resource_fit()`을 호출하고 각
  `RankedCandidate`에 `fit_key`와 `selected_gpu_ids`를 함께 보존한다.
- `crates/coordinator/src/orchestrate.rs`의 단일 후보 분기는 `resource_fit()`을 직접
  호출하고, 복수 후보 분기는 내부에서 같은 helper를 호출한 ranking winner의 ID를
  사용한다. `PlacementToStagingOutcome::Staged`가 `selected_gpu_ids`를 보존하며,
  요구 개수와 다르면 `SelectedGpuCountMismatch`로 STAGING 전에 실패한다.
- protobuf, Grant/scope 생성, GPU별 reservation/accounting, production `run()`/wire는
  변경하지 않았다.

### 테스트와 뮤테이션

- scheduler best-fit 테스트를 15건에서 20건으로 늘렸다. tight VRAM 선택,
  동일 VRAM의 `gpu_id` tie-break, 복수 GPU의 정확한 개수/서로 다른 ID/canonical 순서,
  GPU vector reverse 시 `ResourceFit`과 전체 ranking 동등성, 부적격 GPU 제외,
  빈/중복 ID typed error, missing health/model/VRAM fail-closed를 검사한다.
- coordinator unit test는 단일 후보와 복수 후보 winner가 같은 node snapshot/job에서
  모두 `gpu-a`를 선택하고 outcome/ranking이 같은 ID를 보존함을 검사한다. 기존
  inventory test의 빈/중복 ID 무부작용 거부도 전체 coordinator/workspace 테스트에서
  재통과했다.
- 뮤테이션 1: GPU 정렬을 VRAM-only stable sort로 바꾸자
  `equal_vram_gpu_tie_break_and_assignment_ignore_input_order`가 실패했다. forward는
  `gpu-z`, reverse는 `gpu-a`를 선택해 순서 의존성을 실제 포착했다.
- 뮤테이션 2: 반환 `selected_gpu_ids`를 빈 목록으로 바꾸자 scheduler의 canonical ID
  테스트가 실패했고, coordinator 단일 후보 테스트도
  `SelectedGpuCountMismatch { required: 1, actual: 0 }`로 실패했다.
- 두 뮤테이션은 검증 직후 원복했고 원복 후 전체 테스트를 다시 통과했다.

### 자체 재검토

전체 diff를 다시 읽어 I/O/clock/randomness 추가 없음, node-exclusive CAS 및 wire 경계
무변경, single/N 공용 helper 사용, selected ID canonicalization과 typed error 경계를
확인했다. 재검토 중 `healthy=false`·허용 모델 불일치·VRAM 부족 GPU가 선택에서
제외되는지를 assignment 테스트가 직접 증명하지 않던 공백을 발견해
`assignment_excludes_unhealthy_disallowed_and_too_small_gpus`를 추가했다.

### 검증 결과

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS.
- `cargo test -p gputeer-scheduler`: PASS — best-fit 20 + hard-filter 33 = 53 passed.
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS — 모든 실행 suite
  실패 0, 기존 durable replay test 1건 ignored 유지.
- `cargo test -p gputeer-coordinator`: PASS — unit 83 + integration 4 = 87 passed.
- `git diff --check`: PASS. `cargo fmt`는 설치된 stable toolchain에 `rustfmt` component가
  없어 실행하지 못했다. 빌드/테스트에는 영향이 없었고 변경 코드는 기존 형식에 맞춰
  수동 검토했다.

### 제한

`gpu_id`는 scheduler snapshot의 불투명 식별자다. live inventory producer가 실제
NVML UUID를 공급했다는 provenance, Grant/Lease scope 반영, GPU별 reservation과
release는 이 조각이 증명하거나 구현하지 않는다. 사용자 지시에 따라
`docs/evidence/DoD-NN_*`, `CLAUDE.md`, `docs/history/HISTORY.md`는 수정하지 않았다.
