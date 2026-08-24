# 2026-08-24_1142 scheduler verified Manifest durable binding v1

- **조사 대상:** `DoD-49` 뒤 scheduler 로드맵의 다음 정직한 하루 조각
- **선행 완료:** `DoD-41`~`DoD-49`
- **오늘 착수 결론:** **검증된 signed `JobManifest`의 Job 제출·hash와의 durable binding**
- **예상 규모:** coordinator production Rust 100~180줄, migration/단위·재시작·손상 테스트
  160~280줄, 문서/evidence 별도 — **1일**
- **proto 변경:** 없음
- **production wire 연결:** 없음

## 결론

`Manifest -> JobRequirements`의 필드 복사는 작다. 그러나 직전 조사에서 말한
“identity/default 정책 부족”은 하나가 아니라 다음 세 종류다.

1. Manifest에는 `submitter_member_id`가 없다. 서명 검증으로 확인되는 것은
   `submitter_device_id`의 키 소유뿐이며, 이 device가 어느 team의 어느 active member에
   속하는지는 별도 authoritative membership view가 필요하다.
2. proto3의 0은 `UNSPECIFIED`와 scalar 0을 함께 나타낸다. 일부 0에는 주석으로 기본값이
   있지만(`GpuRequest.min_count = 1`, allocation `EXCLUSIVE`, preference `BALANCED`,
   durability `MIRRORED`), VRAM/CPU/RAM/workspace와 dataset sensitivity에는 안전한
   기본값이 없다.
3. Manifest가 표현하지만 `JobRequirements`가 표현하지 않는 제약이 있다. 특히 effective
   기본값 자체가 `MIRRORED`인 durability는 현재 hard-filter/local orchestration이 검사하지
   않는다. 이 필드들을 버린 `JobRequirements`만 production scheduling에 넣으면 엄격한
   변환이 아니다.

따라서 아래와 같은 **순수 supported-subset projection**은 오늘 만들 수 있다.

```text
Verified<JobManifest> + authoritative ResolvedSubmitter
  -> Result<JobRequirements, ManifestProjectionError>
```

unknown/unspecified/ambiguous/unsupported 값은 모두 typed error로 거부하면 된다. 다만 현재
저장소에는 `ResolvedSubmitter`를 만드는 membership store/lookup이 없고, 원본 Manifest도
Job row에 남지 않으며, durability 등 projection 밖의 gate도 없다. 그래서 이 함수만 만든 뒤
현재 orchestration에 연결하는 것은 안전하지 않고, 성공 가능한 end-to-end adapter라고 부를
수도 없다.

반면 검증된 signed Manifest를 hash 및 Job identity와 원자적으로 영속화하는 일은 정책을
발명하지 않고 지금 할 수 있다. 현재 `StoredJob`은 hash만 보존하므로 재시작 뒤 adapter를
다시 실행하거나 `ExecutionGrant.manifest`를 복원할 수 없다. 이 durable fact는 strict adapter와
Grant builder 양쪽의 공통 선행 조건이다. 그래서 오늘은 converter 자체보다 한 단계 앞의
**verified Manifest durable binding**을 고른다.

## 후보 판정

| 후보 | (a) 지금 가능한가 | (b) 하루인가 | (c) 닫는 실제 위험/공백 | 판정 |
|---|---|---:|---|---|
| fail-closed Manifest projection | **순수 함수로는 가능.** `Verified<JobManifest>`와 별도 resolved member identity를 받고 애매하거나 미지원인 값은 거부할 수 있다. authoritative identity producer와 projection 밖 constraint gate까지 포함한 ingress는 불가능하다. | 순수 함수만 1일, 안전한 ingress는 초과 | unspecified/unknown/0을 scheduler의 확정 사실로 오인하는 위험 | **후순위.** 단독으로는 재시작 가능한 입력도, identity proof도, durability 소비도 없다. |
| reservation release | **안전하게는 불가능.** local Job은 `STAGING`, Attempt는 `Created`까지만 있고 terminal report consumer/runtime-stop proof가 없다. | 안전한 정상 종료·취소·만료 release는 1일 초과 | 영구 점유/capacity 고갈. 그러나 지금 삭제하면 실행 중 Job과 새 Job을 겹칠 수 있다. | **진짜 후속.** proof producer와 terminal transaction이 먼저다. |
| Grant/Lease full scope 생성 | **아직 불가능.** proto 필드는 이미 있지만 selected `gpu_id`는 NVML UUID임이 증명되지 않았고 CPU/RAM/workspace/writable prefix, mode/allocation/durability/rationale의 확정 producer가 없다. | GPU 문자열 복사만 1일 미만, 정직한 full builder는 초과 | 빈/default plan-scope 또는 잘못된 GPU 권한이 wire로 서명되는 위험 | **후순위.** proto 변경은 필요 없지만 입력 provenance가 필요하다. |
| 검증된 signed Manifest durable binding | **가능.** protocol/crypto/coordinator 의존성과 Job transaction이 이미 있다. membership/default를 결정하지 않고 verified message와 derived hash를 보존할 수 있다. | **1일** | restart 뒤 원본 Manifest를 잃어 adapter 재실행과 Grant 복원이 불가능한 공백, Job identity/hash/body 불일치 | **오늘 선택.** 두 다음 consumer가 공유할 durable source를 만든다. |
| NVML UUID provenance | 현재 inventory는 caller가 준 opaque `gpu_id`만 검증·저장한다. Agent telemetry producer/wire가 없다. | production provenance까지 1일 초과 | opaque ID를 `_gpu_uuids` 권한으로 오인하는 위험 | Grant scope 선행 후속 |
| production 연결/outbox | ingress, active session routing, 완성 Grant, ACK/timeout state와 cleanup이 모두 없다. | 1일 초과 | DB commit/TCP send crash window와 오배송/중복 dispatch | 더 뒤의 통합 조각 |

## `JobManifest`와 `JobRequirements`의 정확한 대조

`JobRequirements`는 `crates/scheduler/src/model.rs:118-133`의 15개 필드다.
Manifest source는 `proto/job.proto:41-91`, resource/dataset 하위 필드는
`proto/common.proto:187-213`, `:223-237`, `:326-349`에 있다.

| `JobRequirements` 필드 | Manifest source | 직접 변환 여부 | fail-closed 규칙과 아직 부족한 정책 |
|---|---|---|---|
| `submitter_member_id` | **없음.** Manifest에는 `team_id`, `submitter_device_id`만 있음 | 불가 | 서명 검증은 device key만 확인한다. `(team_id, device_id) -> active member_id`를 어느 committed revision/시각에서 해석하는지, device 승인·member/device revoke를 어떻게 검사하는지가 필요하다. team ID나 device ID를 member ID로 복사하면 안 된다. resolved evidence가 없으면 오류다. |
| `workload_class` | `workload.class` | 조건부 가능 | `workload` 부재, unknown enum, `UNSPECIFIED`는 오류. class의 암묵 기본값은 없다. |
| `side_effect_class` | `side_effect_class` | 조건부 가능 | unknown/`UNSPECIFIED`는 오류. `SIDE_EFFECTING`이면 `acknowledge_duplicate_risk`와 제출 policy gate도 함께 확인되어야 한다. |
| `sensitivity` | `dataset.sensitivity` | 조건부 가능 | dataset 부재를 `PUBLIC`으로 볼 근거가 없다. dataset 부재, unknown/`UNSPECIFIED`는 오류로 닫아야 한다. |
| `minimum_security_tier` | `minimum_security_tier` | 조건부 가능 | unknown/`UNSPECIFIED`는 오류. 숫자 0을 S0로 내리면 안 된다. |
| `minimum_isolation_class` | `minimum_isolation_class` | 조건부 가능 | unknown/`UNSPECIFIED`는 오류. |
| `minimum_key_protection` | `minimum_key_protection` | 조건부 가능 | unknown/`UNSPECIFIED`는 오류. |
| `minimum_gpu_count` | `resources.gpu.min_count` | 가능 | `resources`/`gpu` 부재는 오류. `0 -> 1`은 proto 주석에 명시된 유일한 안전한 정규화다. |
| `minimum_vram_bytes_per_gpu` | `resources.gpu.min_vram_bytes` | 조건부 가능 | 0의 의미가 문서화되지 않았다. “제약 없음”이나 “미상”을 추측하지 말고 0은 오류로 둔다. |
| `allowed_gpu_models` | `resources.gpu.allowed_gpu_models` | 가능 | 빈 목록은 명시적으로 제약 없음이다. 공백 모델은 오류로 둔다. 중복/순서의 정규화 정책을 발명하지 않고 set-equivalent 검증 또는 원순서 보존 계약을 명시해야 한다. |
| `cpu_cores` | `resources.cpu_cores` | 조건부 가능 | 0의 의미가 없다. scheduler에서는 `Some(0)`이 확정 요구량이 되므로 0은 오류다. |
| `ram_bytes` | `resources.ram_bytes` | 조건부 가능 | 위와 같이 0은 오류다. |
| `workspace_bytes` | `resources.workspace_bytes` | 조건부 가능 | 위와 같이 0은 오류다. |

### 변환 결과에 들어가지 않는 Manifest 제약

아래를 단순히 무시한 뒤 “Manifest를 scheduler input으로 변환했다”고 부르면 안 된다.

- `GpuRequest.cuda`, `ExecutionEnvironment.cuda`, OS/arch/libc/runtime 환경 제약
- `GpuRequest.allocation_mode`와 `ResourceRequest.max_egress_bps`
- `deadline_minutes`, `preference`, `max_queue_minutes`
- checkpoint interval, `durability`, `on_partition`, `max_data_loss_minutes`
- network/artifact scope와 실행 환경 안전 정책
- workload estimate와 checkpoint/VRAM 추정 입력

일부는 별도 queue/runtime/checkpoint gate가 소비할 필드이지 `JobRequirements`에 꼭 추가해야
하는 필드는 아니다. 문제는 현재 그 소비 경로가 연결되어 있지 않다는 점이다. 특히 다음
명시 기본값은 0을 “제약 없음”으로 해석할 수 없게 한다.

```text
preference == 0       -> BALANCED
durability == 0       -> MIRRORED
allocation_mode == 0  -> EXCLUSIVE
```

node-exclusive reservation은 EXCLUSIVE를 보수적으로 만족하지만, 현재 local scheduler는
MIRRORED durability나 BALANCED confidence를 보장하지 않는다. 따라서 안전한 adapter의
계약은 다음 둘 중 하나여야 한다.

1. projection 밖 제약을 별도 typed output으로 보존하고, 모든 consumer proof가 있어야만
   orchestration 호출을 허용한다.
2. 현재 consumer가 없는 effective constraint는 명시적 `Unsupported...` 오류로 거부한다.

2번을 지금 적용하면 정상 기본 durability를 가진 Manifest도 거부한다. fail-closed이기는
하지만 실사용 성공 경로가 없는 adapter다. 이것이 “코드 몇 줄이면 된다”와 “오늘 pipeline
조각으로 유용하다”가 다른 이유다.

## reservation release가 요구하는 실제 증명

### 현재 코드 상태

- `JobState`는 `Submitted/Planning/Queued/Staging/Failed`뿐이다
  (`crates/coordinator/src/job_store.rs:18-24`). `Failed`도
  `QUEUED -> FAILED` queue terminal reason 전용이며 STAGING 이후 terminal 상태가 아니다
  (`job_store.rs:522-589`, row shape `:715-765`).
- `AttemptState`는 `Created` 하나뿐이다
  (`crates/coordinator/src/staging_store.rs:40-55`).
- proto에는 signed `AttemptReport`와 terminal outcome이 이미 있다
  (`proto/artifact.proto:196-229`). generic framed ingress도 이를 검증할 수 있지만
  (`crates/crypto/src/framed_ingress.rs`), Agent producer, Coordinator session 분기,
  durable report/Attempt/Job reconciliation consumer는 없다.
- stub의 `AgentGrantAck`는 wire 상관관계만 확인하며 local staging Job/Attempt를 전이하지
  않는다. `mark_revoked()` 후 revoke notice를 보내는 경로도 Agent process 종료 ACK나
  GPU 반환 증명이 아니다 (`crates/coordinator/src/lib.rs:995-1038`).

### 안전한 release proof의 최소 형태

정상 완료/실패 release에는 적어도 다음 사실이 같은 reservation owner와 결합되어야 한다.

1. 서명·identity가 검증된 terminal report가 reservation의 정확한
   `(job_id, attempt_id, node_id, fence_epoch)`와 일치한다.
2. outcome이 실제 workload exit 뒤 생성됐고 stale/superseded attempt 보고가 현재
   reservation을 해제하지 못한다.
3. Attempt terminal 전이, 필요하면 Job terminal/reconciliation 전이, Lease 종료 상태,
   reservation 삭제가 한 durable transaction에서 결합된다.
4. 완료 Job은 최종 artifact/durability guard를 별도로 만족한다. artifact가 Job 완료를
   막더라도 process가 확실히 종료됐다는 사실은 자원 release와 구분해 보존해야 한다.

취소/revoke/timeout은 terminal report만으로 충분하지 않을 수 있다. process tree 종료와
VRAM 반환을 확인하는 signed stop ACK, 또는 lease 만료+grace+fencing 및 partition behavior가
중복 실행을 막는다는 계약이 필요하다. dispatch 전에 안전하게 해제하려면 durable outbox가
`never-published`를 증명해야 한다. 현재는 어느 proof producer도 없으므로 release는 진짜
후속이다.

## Grant/Lease scope 판정

proto 변경은 필요 없다.

```text
GrantedExecutionPlan.assigned_gpu_uuids  // proto/job.proto:141-160
Lease.scope.gpu_uuids                    // proto/common.proto:206-213,
                                         // proto/lease.proto:63
```

`DoD-49`의 reservation에서 같은 selected ID 집합을 재시작 후 복원할 수 있으므로
“plan과 scope가 서로 다른 GPU 집합을 가리키는” 문제의 durable source는 생겼다. 하지만
지금 바로 `_uuids`에 복사할 수는 없다.

- inventory store는 nonblank/unique opaque `gpu_id`만 검사한다
  (`crates/coordinator/src/inventory_store.rs:34-55`, `:441-459`). live NVML UUID producer,
  attestation, wire ingress가 없다.
- `StoredLease`는 resource scope를 보존하지 않고, selected IDs는 reservation child row에
  따로 있다. full Lease 재발급 시 동일 scope를 원자적으로 복원하는 계약이 없다.
- CPU/RAM/workspace는 요구량과 candidate availability만 있을 뿐 확정 allocation/reservation
  row가 없다. `writable_prefixes`는 signed Manifest의 artifact scope가 Job row에 없다.
- plan의 mode, allocation, effective durability, rationale producer도 없다. 기존 stub
  `issue_grant()`/`issue_lease()`는 이 필드들을 `Default`로 비워 둔다
  (`crates/coordinator/src/lib.rs:1359-1384`, `:1453-1468`).

따라서 GPU 문자열만 복사하는 builder는 작지만 정직한 Grant scope 조각은 아니다. 먼저
NVML UUID provenance, signed Manifest persistence, normalized plan/resource assignment가
필요하다.

## 오늘 착수할 최소 조각

### 이름

**조각 5d / verified signed Manifest durable binding**

### In

1. raw `pb::JobManifest`가 아니라 `Verified<pb::JobManifest>`를 받는 새 Job submission
   storage entrypoint를 둔다. 기존 hash-only API의 의미를 조용히 강화하지 않는다.
2. 기존 accepted submission의 다음 값과 verified Manifest를 대조한다.
   - `job_id`
   - `submitter_device_id`
   - `Verified::signer_id()`
   - `manifest_hash = BLAKE3_256(signing_input(JobManifest))`
3. caller가 준 hash를 신뢰하지 않는다. protocol의 기존 `signing_input()`과
   `blake3_256()`으로 derived hash를 계산하고, supplied hash가 있으면 exact match만
   허용한다.
4. submitter signature를 포함한 complete protobuf message를 복구 가능한 bytes로
   저장한다. wire byte-for-byte 동일성을 보장한다고 주장하지 않는다. canonical semantic
   message와 signature를 decode/re-encode할 수 있으면 충분하다.
5. Job row, Manifest body, submission idempotency row를 현재와 같은
   `BEGIN IMMEDIATE` transaction에서 한 번에 commit한다. Manifest insert 직후 fault를
   주입할 수 있게 한다.
6. reopen/load 시 decode 후 다음을 fail closed로 재검사한다.
   - body의 job/device identity가 Job row와 일치
   - body에서 재계산한 hash가 durable hash와 일치
   - 빈/손상/undecodable body가 success로 나오지 않음
7. storage load는 `Verified`를 새로 만들어내지 않는다. 재시작 뒤 scheduler adapter에
   넣기 전에는 당시 authoritative key directory로 서명을 다시 검증해야 한다. 이 조각은
   membership validity를 영속적으로 보증한다고 주장하지 않는다.
8. migration 전 hash-only row는 body가 있는 것처럼 보이지 않게 typed
   `LegacyManifestMissing` 또는 동등한 오류로 fail closed한다. 기존 Job 조회/queue 테스트를
   불필요하게 깨지 않되 새 adapter/Grant consumer는 legacy row를 사용할 수 없어야 한다.

### Out

- device -> member membership lookup과 membership revision binding
- Manifest semantic/default normalization과 `JobRequirements` projection
- deadline/max queue 산출 정책 변경
- durability/checkpoint/preference/allocation admission
- Job submit network ingress와 quorum/ControlStore `COMMITTED`
- Grant/Lease plan/scope 생성·서명·outbox·전송
- NVML UUID provenance
- Attempt terminal state/report ingestion과 reservation release
- production `run()`/accept-loop 연결

### 완료 조건

1. verified Manifest submission 후 store를 닫고 다시 열어도 signature를 포함한 semantic
   `JobManifest`와 동일 derived hash를 복원한다.
2. Manifest `job_id`, Manifest `submitter_device_id`, verified signer, accepted submission
   identity 중 하나라도 다르면 Job/Manifest/idempotency row를 하나도 만들지 않는다.
3. caller-supplied hash가 body의 derived hash와 다르면 fail closed한다.
4. 같은 idempotency key와 동일 Manifest replay는 최초 durable body/hash를 반환하고 새 row를
   만들지 않는다. 같은 key 또는 같은 job ID의 다른 Manifest는 conflict다.
5. Manifest body insert 뒤 injected fault는 Job, body, idempotency를 모두 rollback한다.
6. truncated/undecodable body, body/hash mismatch, body/Job identity mismatch, missing body를
   각각 typed corruption/legacy error로 거부한다.
7. `u64` queue/deadline 기존 경계와 기존 `SUBMITTED -> PLANNING -> QUEUED`, staging,
   concurrency 테스트가 회귀하지 않는다.
8. mutation으로 body/hash 대조 또는 identity 대조를 제거하면 해당 negative test가 반드시
   실패한다.
9. API/문서가 이 결과를 verified membership, normalized `JobRequirements`, full Grant,
   dispatch 또는 `COMMITTED` submission이라고 과장하지 않는다.

### 예상 변경 소유권

- `crates/coordinator/src/job_store.rs`
- 필요한 coordinator unit/integration tests
- 구현 완료 후 별도 evidence/report/history

`proto`, `crates/scheduler`, production `run()`은 이 조각에서 바꾸지 않는다.

## 다음 순서

```text
오늘: verified signed Manifest + derived hash + Job identity durable binding
  -> authoritative device/team/member resolution seam
  -> projection 밖 constraint를 보존하는 strict Manifest normalization
  -> fail-closed JobRequirements projection
  -> NVML UUID-proven inventory + full durable resource assignment
  -> Grant/Lease plan-scope builder + durable outbox/routing
  -> publish/ACK 또는 never-published proof
  -> terminal report/runtime-stop proof와 atomic reservation release
  -> production wire 연결
```

Manifest projection의 typed error 정의는 durable storage와 병행 설계할 수 있다. 하지만
identity proof나 MIRRORED durability를 임의의 기본값으로 꾸며 성공시키지 않는다. release와
GPU `_uuids` scope도 각각 종료 proof와 UUID provenance가 생기기 전에는 구현하지 않는다.

## 구현 결과 (2026-08-24)

### 구현

- `crates/coordinator/src/job_store.rs`에 raw `pb::JobManifest`가 아니라
  `Verified<pb::JobManifest>`만 받는 `submit_verified_manifest()`를 추가했다. 기존
  `submit_accepted()`의 hash-only 계약은 변경하지 않았다.
- 새 경로는 `Verified::get()` 이후에만 Manifest의 `job_id`와
  `submitter_device_id`를 읽고, `Verified::signer_id()`까지 accepted submission identity와
  대조한다. 그 뒤 protocol의 기존 `signing_input()`과 `blake3_256()`으로 hash를 직접
  계산하며 caller가 `AcceptedJobSubmission.manifest_hash`로 준 값과 exact match를 요구한다.
- `coordinator_job_manifests(job_id PRIMARY KEY/FK, verified_signer_id TEXT NOT NULL,
  manifest_body BLOB NOT NULL)`를 migration으로 추가했다. `coordinator_jobs.manifest_hash`,
  Job row, complete signed protobuf body, idempotency row를 한 `BEGIN IMMEDIATE` transaction에서
  commit한다. Manifest insert 직후 `AfterManifestInsert` fault 지점을 두었다.
- `StoredManifestBinding`과 `get_manifest_binding()`을 추가했다. 반환값은 의도적으로
  `Verified<JobManifest>`가 아니다. reopen/load는 빈 body와 decode 실패를 거부하고,
  body의 Job/device identity, 제출 시 signer identity, body에서 재계산한 hash를 durable Job
  row와 다시 대조한다. 사용 시점의 authoritative key directory를 이용한 재검증은 후속
  consumer의 의무다.
- migration 전 hash-only Job은 기존 `get()`과 queue/state transition에서는 계속 보이지만,
  새 Manifest consumer에서는 `LegacyManifestMissing`으로 fail closed한다.
- 동일 idempotency key와 동일 Manifest replay는 최초 timestamp/body/hash를 반환한다.
  동일 key 또는 동일 Job ID의 다른 Manifest는 각각 `IdempotencyConflict` 또는
  `JobIdConflict`로 거부한다.

### negative 및 원자성 테스트

- accepted/Manifest `job_id`, device identity, supplied hash 불일치가 세 테이블에 어떤 row도
  만들지 않음을 확인했다.
- reopen 뒤 signature를 포함한 semantic Manifest와 derived hash가 복원됨을 확인했다.
- empty body, truncated/undecodable body, body/hash mismatch, body/Job ID mismatch,
  body/device mismatch, stored signer mismatch를 각각 typed `ManifestCorrupt` 원인으로
  거부함을 확인했다.
- pre-existing schema의 hash-only Job과 현 스키마에서 기존 API로 만든 hash-only Job을 모두
  `LegacyManifestMissing`으로 거부하면서 기존 `get()`은 유지함을 확인했다.
- Manifest insert 직후 injected failure에서 Job/body/idempotency 세 row가 모두 rollback됨을
  확인했다.
- exact replay가 최초 durable result를 반환하고 changed replay/동일 Job의 다른 key가 원본을
  덮어쓰지 못함을 확인했다.

### 뮤테이션 테스트

1. load의 `derived_hash != job.manifest_hash` 대조를 제거하자
   `body_hash_and_job_identity_corruption_fail_closed_independently`가 실패했다. 원복 후 통과했다.
2. submission의 `manifest.job_id != submission.job_id` 대조를 제거하자
   `identity_and_supplied_hash_mismatches_create_no_rows`가 실패했다. 원복 후 통과했다.

### 검증

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS.
- `cargo test -p gputeer-coordinator`: unit 95/95, concurrent integration 4/4, doc 0/0 PASS.
- `cargo test --workspace --exclude gputeer-runtime-windows`: 최종 전체 PASS.
- 첫 workspace 실행에서는 기존
  `gputeer-crypto::separate_process_lock_timeout_is_not_duplicate`가 timing상 `Fresh`를 받아
  1회 실패했다. 단독 재실행은 PASS였고 두 번째 workspace 전체 실행도 PASS였다. 단독
  재실행 전에는 Windows linker `LNK1318` PDB 오류가 1회 있었으며 동일 명령 재시도로
  해소됐다.
- `git diff --check`: whitespace error 없음. checkout에 `rustfmt`와 `clippy` component가
  설치되어 있지 않아 `cargo fmt`/`cargo clippy`는 실행할 수 없었다.

### 자체 재검토와 한계

- 자체 재검토에서 pre-existing schema migration 뒤 legacy Job의 새 load 경로 assertion과
  body의 device identity 손상 케이스가 빠진 것을 발견해 둘 다 추가했다.
- 이 결과는 “제출 시 signature verification을 통과한 body와 signer/hash/Job identity가 한
  local SQLite transaction에 묶였다”는 사실만 보존한다. 현재 membership validity,
  normalized `JobRequirements`, full Grant/Lease, ControlStore `COMMITTED`, dispatch, production
  wire 연결은 증명하거나 구현하지 않았다.
