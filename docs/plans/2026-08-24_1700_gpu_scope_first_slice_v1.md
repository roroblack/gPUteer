# 2026-08-24_1700 GPU scope first slice v1

- **조사 결론:** full Grant/Lease scope 발급은 아직 착수 불가
- **하루 규모 결론:** 가능. `ScopeCandidate`를 계산하는 순수 scheduler kernel과
  provenance gate 계약만 먼저 만들 수 있다.
- **추정 규모:** production Rust 120~220줄, 테스트 180~320줄, **1일**
- **proto 변경:** 이 첫 슬라이스에서는 없음
- **정직한 경계:** 이 조각은 NVML을 읽거나 서명하지 않고, Grant/Lease를 만들거나
  전송하지 않는다. 결과는 wire-level UUID가 아닌 검증 전 assignment candidate다.

## 착수 여부

실측으로 차단 사유가 **부분적으로** 줄었다. 이제 다음 하드웨어 값은 실제 호스트에서
관측되었다.

```text
GPU UUID       GPU-09a269a7-50a8-f5be-2a00-d20a1c281c93
model          NVIDIA GeForce RTX 4070 SUPER
PCI identity   00000000:70:00.0 / 0x278310DE
memory         total 12282 MiB, reserved 283 MiB
compute cap    8.9
driver         595.79
VBIOS          95.04.69.00.01
power limit    220.00 W
```

그러나 이것은 NVML 출력의 실측 증거이지, Coordinator가 검증할 수 있는 authoritative
inventory는 아니다. UUID를 `ResourceScope.gpu_uuids`에 복사해 넣는 것만으로는
provenance blocker가 해소되지 않는다. 따라서 **실측은 “하드웨어가 없다”는 차단은
해소했지만, “누가 이 값을 책임지고 서명했는가”라는 차단은 그대로 남겼다.**

## 1. 규범상 GPU scope의 정확한 요구

### Scope와 Grant가 직접 요구하는 값

| 대상 | 규범상 필드 | 근거 | 이번 실측으로 충족? |
|---|---|---|---|
| Lease resource scope | `gpu_uuids`, `cpu_cores`, `ram_bytes`, `workspace_bytes`, `writable_prefixes` | `proto/common.proto:175-183`; Lease가 이를 `scope`로 포함: `proto/lease.proto:63-66` | GPU 식별자만 부분 충족. 나머지는 없음 |
| Grant execution plan | `gpu_allocation`, `assigned_gpu_uuids`, durability/rationale | `proto/job.proto:139-154` | UUID 문자열은 있으나 provenance 없음. allocation/durability/rationale 입력 없음 |
| Grant binding | embedded `manifest`, its `manifest_hash`, `lease`, `plan` | `proto/job.proto:103-119` | Job/Attempt/Lease 값 없음 |
| Lease authority | `fence_epoch`, issuer, lifetime, scope, `coordinator_signature` | `proto/lease.proto:34-66` | 실측에는 없음 |

`ResourceScope`는 GPU UUID 문자열을 요구하지만 UUID의 출처, 관측자, 관측시각, NVML
원문, inventory revision을 필드로 갖지 않는다(`proto/common.proto:176-182`). 따라서
현재 wire 타입의 `_uuids` 이름만으로 NVML UUID provenance가 생기지 않는다.

### 요구값을 만드는 입력과 서명 규범

GPU admission의 요구 조건은 `min_vram_bytes`, `min_count`, CUDA driver/runtime/
compute capability, allocation mode, model allowlist이다(`proto/common.proto:148-164`).
노드 자원 요구에는 CPU/RAM/workspace도 있다(`proto/common.proto:166-173`).

Allocation mode는 `EXCLUSIVE`, `SHARED`, `PARTITIONED`로 구분되고, `PARTITIONED`는
MIG 지원 GPU에만 해당한다(`proto/common.proto:137-146`). 현재 측정의 `mig.mode.current=
[N/A]`는 PARTITIONED를 입증하지 못한다. 반대로 v1 기본 allocation이 EXCLUSIVE라는
주석이 있지만(`proto/common.proto:157-163`), 그것은 NVML이 실제 독점을 강제한다는
증명이 아니다.

Lease와 Grant의 서명은 각각 별도 domain을 가져야 한다. canonical 서명은
`domain_tag || schema_version || canonical length || canonical`이고 Ed25519로 계산한다
(`docs/protocol/signing.md:224-235`). `ExecutionGrant`와 `Lease`의 domain도 각각
등록되어 있다(`docs/protocol/signing.md:245-257`). 검증은 domain, schema, canonical,
서명, signer identity/membership, lifetime, replay 순서다(`docs/protocol/signing.md:512-526`).

하지만 GPU 관측을 위한 서명 대상 메시지나 domain은 현재 `proto/`와
`docs/protocol/signing.md`에 없다. `ApproveDevice`는 device ID, member ID, public key,
peer ID, key protection, owner signature를 승인할 뿐이다(`proto/control.proto:270-277`).
그것은 “이 장치의 공개키를 승인했다”는 사실이지 “이 장치가 보고한 GPU UUID가
실재한다”는 관측 attestation이 아니다.

Node 규범은 preflight에서 capability를 확인하고, Owner 서명으로 APPROVED가 되면
Device Certificate를 발급하며, heartbeat 서명 검증 후 ONLINE으로 전이한다
(`docs/protocol/state-machines.md:45-52`). 이 lifecycle은 signer identity의 기반이 될
수 있지만 GPU observation record 자체를 정의하지 않는다.

## 2. 실측값 대조

### 채울 수 있는 값

- `gpu_id`의 원시 입력: 정확한 NVML UUID를 넣을 수 있다.
- GPU model: allowlist 비교용 `NVIDIA GeForce RTX 4070 SUPER`를 넣을 수 있다.
- PCI bus/device, VBIOS, power limit, compute capability, driver version: 관측 레코드의
  raw/static evidence로 보존할 수 있다. 다만 현재 `ResourceScope`에는 이 필드들이 없다.
- `memory.total - memory.reserved = 11,999 MiB = 12,581,863,424 bytes`라는 파생값은
  계산할 수 있다. 이것을 곧바로 `available_vram_bytes`라고 부르면 안 된다. reserved와
  free/available의 의미가 규범에 정의되어 있지 않기 때문이다.
- `EXCLUSIVE` 정책 후보: shared 또는 MIG partition을 선택하지 않는 입력으로는
  계산할 수 있다. 단, 실제 runtime enforcement가 생겼다는 뜻은 아니다.

### 여전히 채울 수 없는 값

| 누락 사실 | 왜 필요한가 | 이번 데이터 |
|---|---|---|
| `healthy` | 현재 hard-filter는 GPU health를 필수 관측으로 취급하고 모르면 `MissingFact`로 거부한다 (`crates/scheduler/src/model.rs:83-90,147-160`; `crates/scheduler/src/filter.rs:128-177`) | 없음 |
| authoritative available VRAM | hard-filter/best-fit은 GPU별 available VRAM을 비교한다 (`crates/scheduler/src/filter.rs:212-230`; `crates/scheduler/src/rank.rs:155-180`) | total/reserved만 있음 |
| `observed_at_unix_ms` | snapshot freshness gate에 필요하다 (`crates/scheduler/src/model.rs:94-106`; `crates/scheduler/src/filter.rs:58-70`) | 없음 |
| `inventory_revision` | staging CAS의 admission token이다 (`crates/scheduler/src/model.rs:94-102`) | 없음 |
| CPU/RAM/workspace | `ResourceScope`의 GPU 외 자원 필드다 (`proto/common.proto:176-182`) | 없음 |
| `writable_prefixes` | Lease가 허용하는 CAS 쓰기 범위다 (`proto/common.proto:181-182`) | Job/Attempt policy 입력 없음 |
| signed observation / signer identity | UUID를 wire 권한으로 승격할 provenance다 | 현재 NVML 텍스트에는 없음 |
| Grant/Lease identity and fence | `grant_id`, attempt, lease lifetime, fence, coordinator signature에 필요 (`proto/job.proto:103-136`; `proto/lease.proto:34-66`) | 없음 |

`persistence_mode=[N/A]`, `ecc.mode.current=[N/A]`, `mig.mode.current=[N/A]`는 현재
ResourceScope/GrantedExecutionPlan의 필수 필드가 아니다. 그러므로 이 세 값 자체는
새 규범 차단 사유가 아니다. 다만 MIG가 N/A인 상태에서는 `PARTITIONED` allocation을
선택할 근거가 없고, ECC/persistence 정책을 향후 hard gate로 추가한다면 그때는 별도
관측 타입과 “N/A를 unknown으로 처리”하는 규칙이 필요하다. N/A를 `false`, `healthy`,
또는 “지원 안 함”으로 조용히 치환해서는 안 된다.

## 3. provenance의 본질과 runtime 의존성

NVML은 호스트에서 읽는 관측 API이지 authority가 아니다. 최소한 다음 경로가 필요하다.

```text
Agent의 NVML collector
  -> observation canonical bytes + observed_at + inventory_revision
  -> Agent device key 서명
  -> Coordinator가 승인된 device key/membership와 서명 검증
  -> 검증된 observation을 durable inventory revision에 binding
  -> 그때에만 UUID를 Grant plan / Lease scope의 wire 권한으로 사용
```

서명은 “승인된 Agent가 이 바이트를 보고했다”는 provenance를 제공한다. Agent가 거짓
값을 보고하지 않았다는 하드웨어 진실성까지 보장하지는 않는다. 그 stronger claim에는
별도 privileged probe, independent attestor 또는 runtime trust boundary가 필요하다.

현재 `CoordinatorInventoryStore`는 caller가 identity-checked normalized facts를
제공한다는 전제의 durable repository이며, signature verification이나 heartbeat
수집을 하지 않는다(`crates/coordinator/src/inventory_store.rs:1-6,34-56`). 따라서
그 저장소에 현재 값을 넣는 것만으로는 provenance blocker가 줄지 않는다.

실제 NVML collector와 device-key signing/wire ingress는 Agent runtime 로드맵에
의존한다. 그러므로 **authoritative signed record의 생산자까지 오늘 만들 수 있다고
주장할 수 없다.** 다만 runtime과 무관하게 아래 두 조각은 먼저 만들 수 있다.

1. 관측값을 `unknown`과 명시적 값으로 보존하고, UUID provenance가 없으면 wire scope로
   승격하지 않는 타입/검증 계약.
2. 서명된 관측이 나중에 도착할 자리를 위해 `observation_id`, signer/device,
   `observed_at`, inventory revision, canonical bytes/hash, verification outcome을
   durable key로 삼는 저장 경계의 설계. 단, 서명 전 raw cache를 authoritative inventory로
   부르지 않는다.

DoD-50~53의 패턴도 같다. 검증된 signed evidence를 current durable anchor에 먼저
   묶고, 그 뒤 consumer가 pure resolver를 실행해야 한다. 서명된 `CheckpointManifest`와
   `ReplicaAck`를 먼저 durable binding한 뒤 effective count를 계산하는 구조가 그
   선례다(`docs/plans/2026-08-24_1513_verified_checkpoint_manifest_durable_binding_v1.md:24-32`;
   `docs/plans/2026-08-24_1634_effective_replica_count_kernel_v1.md:16-26`). GPU도
   unverified caller payload를 먼저 authority로 저장하는 순서는 재사용하면 안 된다.

## 4. 하루 규모의 순수 kernel

### 제안: `gpu_scope_candidate`

기존 DoD-41 hard-filter와 DoD-45 best-fit처럼 고정 snapshot을 입력으로 받고 외부
상태를 읽지 않는 함수로 한정한다. DoD-41의 kernel 경계는 `PoolSnapshot`,
`JobRequirements`, `Policy` 입력과 pure evaluation이다(`docs/plans/2026-08-21_1002_scheduler_hard_filter_v1.md:9-14,58-68`).
DoD-45도 선택 GPU ID를 `(available_vram_bytes, gpu_id)`로 결정하고 wire UUID
provenance는 주장하지 않는 경계를 이미 제시했다
(`docs/plans/2026-08-21_1036_scheduler_selected_gpu_assignment_v1.md:90-145`).

```text
GpuObservationSnapshot
  + JobGpuRequirements
  + explicit ResourceAmounts(cpu, ram, workspace, prefixes)
  + ProvenanceGate (verified / unverified)
  -> Result<ScopeCandidate, ScopeError>
```

In:

- UUID/opaque GPU ID의 blank·duplicate·non-canonical 검사
- `healthy == Some(true)`, model, authoritative available VRAM, count를 fail-closed로
  검사
- required GPU 수를 deterministic하게 선택하고 ID를 canonical order로 반환
- compute capability/driver/allocation mode가 요구된 경우에만 검사하고, 값이 없으면
  `MissingFact`로 거부
- CPU/RAM/workspace/prefix는 별도 명시 입력으로 받아 scope candidate에 복사하되,
  누락 시 기본값을 만들지 않음
- `ProvenanceGate::Unverified`이면 결과를 `ScopeCandidate`로만 반환하고
  `ResourceScope.gpu_uuids` materialization은 거부

Out:

- NVML call, system clock, random ID, network, crypto verification
- Agent/Coordinator session, Lease/Grant creation or transition
- SQLite/ControlStore write, reservation, release, outbox
- `ScopeCandidate`를 authoritative `ResourceScope`라고 부르는 것

완료 조건:

1. 이번 RTX 4070 SUPER fixture의 UUID/model은 보존되지만 health, available VRAM,
   observed time, revision이 없는 입력은 hard admission 결과가 되지 않는다.
2. `total - reserved`를 available로 가정하는 fixture는 명시적 conversion policy가
   없으면 거부한다.
3. GPU 입력 순서를 바꿔도 선택 집합과 canonical output이 같다.
4. duplicate/blank UUID, missing health/VRAM, missing required CPU/RAM/workspace,
   unverified provenance는 각각 typed error다.
5. verified gate가 없는 candidate는 `ResourceScope.gpu_uuids`로 변환되지 않는다.
6. 함수는 signature, storage, state transition을 전혀 수행하지 않는다.

이 조각은 실측 데이터로 deterministic scope 계산의 fixture를 만들 수 있게 하지만,
실측 한 번만으로 production dispatch가 가능해졌다고 주장하지 않는다. DoD-54의
effective-replica처럼 판단에 필요한 사실을 모두 입력으로 받는 순수 resolver 패턴을
GPU assignment에 적용하는 것이다(`docs/plans/2026-08-24_1634_effective_replica_count_kernel_v1.md:16-26`).

## 5. 다음 순서와 차단 판정

```text
오늘(1일): ScopeCandidate pure kernel + strict missing/provenance gate
  -> Agent runtime: NVML observation collector + device-key signed record
  -> protocol: GPU observation message/domain/schema_version/canonical rules
  -> coordinator: verified observation + revision durable binding
  -> full ResourceScope / GrantedExecutionPlan builder
  -> Lease/Grant signature, outbox, routing, ACK and runtime enforcement
```

최종 판정:

- **순수 kernel 첫 슬라이스:** 착수 가능, 1일.
- **서명된 관측 record의 authoritative producer:** Agent runtime 의존, 오늘 완료
  주장 불가.
- **durable unverified NVML 저장:** 가능하더라도 차단을 해소하지 않으므로 첫 조각으로
  선택하지 않음.
- **full Grant/Lease scope:** 아직 불가. UUID provenance, health/available VRAM,
  freshness/revision, CPU/RAM/workspace/prefix와 lease identity가 남아 있다.
- **실측으로 해소된 범위:** “실제 GPU가 없어서 UUID/모델을 못 채운다”는 부분은 해소.
  **그 외 authoritative provenance와 scope completeness 차단은 그대로다.**

## 구현 결과 (2026-08-24)

### 구현 범위

- `crates/scheduler/src/scope.rs`에 `GpuObservationSnapshot`,
  `ScopeGpuObservation`, `JobGpuRequirements`, `ScopeResourceInput`,
  `ProvenanceGate`, `ScopeCandidate`, `ScopeError`와 순수
  `gpu_scope_candidate()`를 추가했다. `crates/scheduler/src/lib.rs`는 이 API만
  공개한다.
- kernel은 관측 시각/revision, GPU 관측, GPU 요구, CPU/RAM/workspace/prefix,
  caller가 이미 판정한 provenance gate만 입력받는다. I/O, 시스템 시계, TTL,
  DB/network, 난수, crypto 검증, 저장, reservation, Grant/Lease 생성·전이를 하지 않는다.
- 적격 GPU는 기존 DoD-45와 같이 `(authoritative available VRAM, opaque gpu_id)`
  오름차순으로 필요한 수만 선택하고, 결과 ID는 `gpu_id` 오름차순으로 정규화한다.
  writable prefix와 model/compute allowlist도 순서와 무관하게 해석한다.
- `ScopeCandidate`는 관측 binding, 선택 opaque GPU ID, allocation mode와 명시 자원값만
  보존한다. `ResourceScope`, UUID provenance, Grant/Lease 또는 runtime enforcement로
  materialize하는 API는 만들지 않았다.

### fail-closed와 한계

- unverified provenance, 누락 observed time/revision/GPU inventory/요구량/자원값,
  blank·공백 비정규·중복 GPU ID, 누락 health/model/driver/compute/allocation/available
  VRAM을 typed error로 닫는다. unhealthy 또는 요구와 불일치하는 GPU는 후보에서 제외한다.
- `AvailableVramObservation::DerivedFromTotalAndReserved`를 별도 표현하고 무조건
  `NonAuthoritativeAvailableVram`으로 거부한다. RTX fixture의 `12,282 MiB - 283 MiB`를
  available VRAM으로 추정하지 않는다.
- CUDA runtime 요구는 proto에 있지만 이 조각에는 runtime/driver 호환 규칙이 없다.
  요구가 있으면 `CudaRuntimeCompatibilityUnresolved`로 닫고 임의 exact-match 또는
  driver mapping을 만들지 않는다.
- `PARTITIONED`는 input이 MIG 지원을 주장해도 `PartitionedAllocationUnproven`으로
  거부한다. `[N/A]`를 false/unsupported/healthy로 해석하지 않으며 MIG partitioned
  allocation을 입증한다고 주장하지 않는다.
- `ProvenanceGate::Verified`는 caller가 공급한 판정값이다. kernel은 서명, signer,
  membership, canonical observation 또는 하드웨어 진실성을 검증하지 않는다.

### negative·결정성·뮤테이션 테스트

`crates/scheduler/tests/gpu_scope_candidate.rs`에 14건을 추가했다.

- 4개 GPU의 모든 24개 순열에서 `ScopeCandidate` 전체를 `assert_eq!`로 비교한다.
  model/compute allowlist와 writable-prefix 역순도 전체 결과가 같음을 비교한다.
- RTX 실측 fixture의 time/revision/health 누락과 total-minus-reserved 파생 VRAM,
  unverified provenance, missing/zero 요구, missing CPU/RAM/workspace/prefix,
  blank/non-canonical/duplicate ID와 prefix, 각 optional constraint의 필요한 사실 누락,
  unhealthy/model/driver/compute/allocation/VRAM 불일치, CUDA runtime 미해소,
  `PARTITIONED`를 모두 negative 경로로 검사한다.
- 뮤테이션 1: production provenance gate 분기를 제거하자
  `unverified_provenance_is_an_explicit_gate_not_a_kernel_inference`가 `Err` 대신
  `Ok(ScopeCandidate)`를 받아 실패했다. 원복 후 PASS.
- 뮤테이션 2: production `PARTITIONED` 거부 분기를 제거하자
  `partitioned_allocation_is_rejected_even_when_input_claims_mig_support`가
  `Ok(... allocation_mode: Partitioned ...)`를 받아 실패했다. 원복 후 PASS.

### 자체 재검토

전체 source/test diff와 금지된 외부 접근 문자열을 다시 검사했다. I/O/clock/randomness,
wire materialization, 상태 전이, storage/crypto/proto 변경은 없었다. 재검토 중 초안의
`JobGpuRequirements`가 proto의 `cuda_runtime_version`을 표현하지 않아 caller adapter가
그 요구를 조용히 버릴 수 있는 실제 결함을 발견했다. 필드를 추가하되 호환 규칙은
발명하지 않고 typed unresolved error와 negative test를 추가했다.

또한 unrelated GPU fact를 과잉 요구하지 않는지 확인했다. model/driver/compute는 해당
constraint가 있을 때만 필요하고, health=false 또는 앞선 compatibility gate에서 제외된
GPU의 뒤쪽 사실은 요구하지 않는다. Shared mode는 caller가 해소한 mode 입력을 소비할
뿐 실제 MPS/동일-owner enforcement를 증명하지 않는다.

### 검증 결과

```text
cargo test -p gputeer-scheduler --test gpu_scope_candidate --no-fail-fast
PASS — 14 passed, 0 failed

cargo test -p gputeer-scheduler --no-fail-fast
PASS — 67 passed, 0 failed

cargo build --workspace --exclude gputeer-runtime-windows
PASS — exit 0

cargo test --workspace --exclude gputeer-runtime-windows
PASS — 모든 실행 suite 0 failed, 기존 durable replay test 1건 ignored 유지
```

`cargo`가 PATH에 없어 설치된
`C:\Users\playdata2\.cargo\bin\cargo.exe` 절대 경로로 동일 명령을 실행했다.
`cargo fmt -p gputeer-scheduler -- --check`는 stable toolchain에 `cargo-fmt.exe`가
없어 실행되지 않았다. compiler와 수동 diff 검토는 통과했지만 formatter 검증은
환경 한계로 남는다. 사용자 지시에 따라 `docs/evidence/`, `CLAUDE.md`,
`docs/history/HISTORY.md`, `docs/vision/TODO_VISION.md`, elastic-admission 리포트와
membership 조사 문서는 수정하지 않았다.
