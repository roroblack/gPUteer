# 2026-08-24_1634 effective replica count 순수 kernel v1

- **조사 질문:** durable `CheckpointManifest`/`ReplicaAck` 다음에 `MIRRORED` 전체가
  아니라 판정 선행 조건의 첫 순수 슬라이스가 1일인가
- **선행 게이트:** DoD-52 `coordinator_checkpoint_manifests`, DoD-53
  `coordinator_replica_acks`
- **오늘 착수 결론:** **holder/freshness/membership 해석을 명시적 입력으로 받는 순수·결정적
  effective replica count kernel**
- **예상 규모:** production Rust 약 180~300줄, 독립·mutation-oriented 테스트 약
  300~450줄, 문서/evidence 별도 — 기존 `ReplicaSet`을 정직한 계약으로 교체하는 **0.5~1일**
- **proto/protocol/schema 변경:** 없음
- **durable store/상태 전이/production routing:** 없음

## 결론

이 조각은 하루 규모로 성립한다. 단, durable ACK 목록을 바로 받아 “현재 `MIRRORED`”라고
판정하는 consumer는 아니다.

```text
raw durable ACK observations
  -> 후속 resolver: current signature/membership + holder별 observation 선택
                     + ephemeral/device/failure-domain facts
  -> 이번 조각: resolved holder 집합을 holder/domain별로 중복 제거하고
                effective count와 요구치 충족 여부를 결정적으로 계산
  -> 후속 durable consumer와 checkpoint 상태 전이
```

규범에 없는 ACK TTL, liveness challenge, ONLINE-only 조건을 kernel에 발명하지 않는다. 대신
caller가 “이 holder에서 계산에 사용할 관측은 이것”이라고 해소한 결과만 받고, 같은 holder가
둘 이상 선택되거나 필요한 사실이 빠지면 fail closed한다. 따라서 DoD-53의
`(checkpoint_id, holder_device_id, acked_at_unix_ms)` 복수 행을 우연히 모두 세는 경로가 없다.

## 1. `MIRRORED` 규범과 정확한 남은 조건

### `docs/protocol/`에 실제로 있는 것과 없는 것

`docs/protocol/` 두 문서에는 `MIRRORED`라는 상태나 정책명의 직접 정의가 **없다**.
Checkpoint 상태기계는 다음 일반 전이만 규정한다.

- signed `ReplicaAck` 수신 시 `REPLICATING -> REPLICATED`, effect는 “유효 replica 수
  갱신”, durability는 `DURABLE`이다
  (`docs/protocol/state-machines.md:206-220`).
- `REPLICATED -> COMMITTED`는 “유효 replica 수 >= 요구치”일 때만 허용된다
  (`docs/protocol/state-machines.md:223`).
- 요구치 미달은 `REPLICATED -> REPLICATING`, COMMITTED 뒤 유실은 되돌리지 않고
  `COMMITTED_DEGRADED`로 간다 (`:224-226`).

`MIRRORED`의 수치 의미는 protocol 문서가 아니라 protobuf 계약에 있다.

- `DURABILITY_MIRRORED = REPLICATED(1)`이며 기본값, 즉 요구치는 1이다
  (`proto/common.proto:148-153`).
- `DURABILITY_REPLICATED = REPLICATED(2)`이고 서로 다른 failure domain 두 곳이 필요하다
  (`proto/common.proto:153`).
- `ReplicaAck.failure_domain`은 `REPLICATED(n)`의 n을 서로 다른 domain 수로 세며 같은
  박스의 두 복사본은 1로 센다 (`proto/artifact.proto:101-111`).
- 유효 count 규칙은 (1) 유효한 holder 서명만, (2) 같은 failure domain은 하나,
  (3) ephemeral node의 local copy 제외, (4) submitter/worker가 같은 device면 별도
  replica로 세지 않음이다 (`proto/artifact.proto:135-152`).

따라서 `MIRRORED`는 단순히 ACK row가 하나 존재한다는 뜻이 아니다. 규칙 1~4를 적용한
`effective_replica_count >= 1`이어야 한다. failure-domain “분산”이 두 곳 이상 필요한 것은
`REPLICATED(2)`지만, `MIRRORED`도 count의 단위가 distinct domain이라는 동일 규칙을 쓴다.

### holder가 유효 member여야 하는가

서명 검증 순서는 Ed25519 검증 뒤 서명자의 팀 membership/승인을 확인하고, 그것이 실패하면
`UNKNOWN_SIGNER`로 종료하도록 규정한다
(`docs/protocol/signing.md:514-527`). Node가 revoke되면 Device Certificate를 무효화하고
artifact를 재검증 대상으로 만든다
(`docs/protocol/state-machines.md:49-52,77-81`). 따라서 durable ACK를 결정에 다시 쓸 때는
현재 authoritative directory에 대한 signature/signer 승인 결과가 입력되어야 한다.

반면 replica holder가 반드시 `ONLINE`이어야 한다는 규범은 없다. `ONLINE + NORMAL`은 신규
스케줄링 후보 조건이다 (`state-machines.md:84-98`). replica count에 대해 표가 직접 명시하는
node 조건은 ephemeral 종료를 제외한다는 것뿐이다 (`:78-80`). 따라서 이 kernel은
SUSPECT/OFFLINE 같은 상태를 임의로 제외하지 않는다. 그런 현재 liveness 정책은 별도 규범이
생긴 뒤 resolver가 제공해야 한다.

### freshness의 한계

`ReplicaAck`는 `Lifetime::Evidence`라 시각으로 만료시키지 않으며, 소비자가 freshness를
판단해야 한다 (`docs/protocol/signing.md:595-625`). 그러나 ACK만 `fence_epoch`이 없고
`acked_at`밖에 없어서, 규범은 ACK를 “지금 durable”이 아니라 “`acked_at` 시점에 durable”로만
읽으라고 한다 (`:627-645`). TTL이나 “N분 이내” 기준은 없다.

따라서 이번 kernel은 다음을 구분한다.

1. **holder dedup은 강제한다.** 같은 device의 여러 kind/관측은 절대 여러 replica가 아니다.
2. **어느 관측이 최신·사용 가능인지는 입력으로 받는다.** raw 행 중 최대
   `acked_at_unix_ms`를 무조건 current로 간주하거나 자체 TTL을 두지 않는다.
3. holder마다 계산 대상으로 선택된 관측이 0개면 제외/미해소, 2개 이상이면 ambiguous input
   오류다. 단 하나만 effective-count 후보가 될 수 있다.
4. 결과는 “resolved inputs가 표현한 평가 시점의 count”이지 현재 liveness 증명이 아니다.

이 경계가 DoD-53 PK에 `acked_at_unix_ms`가 들어간 사실을 숨기지 않으면서도 규범에 없는
freshness 정책을 만들지 않는 최소 형태다.

## 2. 지금 durable 데이터로 가능한 부분과 membership 의존 부분

| 판정 요소 | 현재 durable 데이터만으로 가능 | membership/resolver 필요 | 이유 |
|---|---:|---:|---|
| checkpoint/root 일치 | 가능 | 아니오 | DoD-52 anchor와 DoD-53 저장 transaction에서 이미 대조 |
| 검증 당시 signer=holder | 가능 | 아니오 | DoD-53이 `Verified` signer와 holder를 묶어 보존 |
| holder별 관측 그룹화, timestamp/본문 보존 | 가능 | 아니오 | PK가 checkpoint/holder/time이고 list가 전 행 복원 |
| holder별 단일 선택 강제 | kernel에서 가능 | 실제 선택 정책은 필요 | TTL/fence 규범이 없어 kernel이 current를 발명할 수 없음 |
| 현재 signature/key/승인 유효성 | 불가 | 필요 | load는 의도적으로 raw이고 revoke/key rotation 뒤 재검증 필요 |
| ephemeral 여부 | 불가 | 필요 | ACK에는 없고 approved device fact임 |
| submitter/worker same-device 여부 | 일부 ID는 있음 | authoritative device 해석 필요 | Job/Attempt binding은 있으나 member/device authority가 없음 |
| authoritative failure domain | ACK claim만 있음 | 필요 | signed claim은 보존되지만 current device/node authority가 없음 |
| distinct holder/domain count | resolved input이면 가능 | raw durable row만으로는 불가 | 위 사실을 먼저 해소해야 함 |
| 요구치 (`MIRRORED=1`) | durable JobManifest에 값은 있음 | strict projection/재검증 필요 | raw manifest load는 current `Verified`가 아님 |
| checkpoint 상태 전이 | 불가 | 별도 durable state/transaction 필요 | DoD-53 schema에는 state/count column이 없고 이번 범위 밖 |

현재 `StoredCheckpointManifestBinding`은 checkpoint/job/attempt/producer/fence/root를 보존한다
(`crates/coordinator/src/checkpoint_manifest_store.rs:20-34`). `StoredManifestBinding`은 complete
`JobManifest`를 보존하지만 재시작 뒤 재검증을 요구한다
(`crates/coordinator/src/job_store.rs:149-158,398-409`). 즉 policy 값까지 데이터는 있으나,
membership-backed decision input으로 안전하게 조립하는 consumer는 아직 없다.

## 3. 선행 타입과 현재 kernel의 실제 상태

### 타입은 존재한다

- `ReplicaAck`, `ReplicaKind`, `DurabilityStatus.effective_replica_count`가 존재한다
  (`proto/artifact.proto:101-152`).
- `Durability::{LOCAL,MIRRORED,REPLICATED}`가 존재한다
  (`proto/common.proto:148-154`).
- membership 쪽 protobuf에는 `ApproveDevice.is_ephemeral`, `RevokeDevice`,
  `NodeRecord.failure_domain`, `DeviceRecord.approved/is_ephemeral`이 존재한다
  (`proto/control.proto:288-315,504-530`).
- production `AgentRegistry`에는 device/key/node state는 있으나 `is_ephemeral`과
  `failure_domain`은 없고 (`crates/coordinator/src/inventory_store.rs:20-31`), durable schema도
  두 필드를 저장하지 않는다 (`:383-394`). authoritative membership consumer로는 부족하다.

### 이미 있는 `ReplicaSet`은 후속 consumer 계약으로 쓰기 어렵다

`crates/checkpoint/src/durability.rs:260-298`에 순수 `ReplicaSet`이 이미 있다. 그러나 현재
형태는 다음 이유로 이번 문제를 해결하지 못한다.

- `failure_domain -> 한 entry` BTreeMap이라 같은 domain의 마지막 add가 앞선 entry를
  덮어쓴다. 입력 순서에 따라 valid/non-ephemeral 결과가 달라질 수 있다 (`:262-293`).
- holder별 dedup과 `acked_at` 복수 관측 모델이 없다.
- 규칙 4의 submitter/worker same-device 판단을 입력받지 않는다.
- ephemeral **local copy**가 아니라 ephemeral holder 전체를 제외한다.
- 현재 signature/membership resolution과 missing/ambiguous fact를 표현하지 못한다.
- 그런데 주석은 규칙 1~4를 모두 적용한다고 주장한다 (`:286`).

따라서 새 duplicate counter를 만드는 것이 아니라 이 기존 순수 kernel을 fail-closed resolved
input/report 계약으로 교체·보강하는 것이 가장 작은 정직한 변경이다. repository 내
DoD-41 `evaluate_eligibility()`와 DoD-45 `rank_best_fit()`처럼 filesystem/network/clock/random을
읽지 않고 immutable snapshot과 명시적 policy만 받는 패턴을 따른다
(`crates/scheduler/src/filter.rs:7-35`, `crates/scheduler/src/rank.rs:11-47`).

## 4. 오늘 구현할 순수 kernel 계약

정확한 공개 이름은 구현 때 기존 export와 맞추되 의미는 다음과 같다.

```rust
evaluate_effective_replicas(
    observations: &[ResolvedHolderObservation],
    required: Durability,
) -> Result<EffectiveReplicaReport, ReplicaEvaluationError>
```

`ResolvedHolderObservation`은 raw protobuf의 대체 authority가 아니다. 후속 adapter가 검증한
결과를 전달하는 값 객체다. 최소한 다음 사실을 명시적으로 담는다.

- checkpoint/root 평가 범위 식별자
- `holder_device_id`, signed `acked_at_unix_ms`, `ReplicaKind`
- resolver가 선택한 관측인지 여부; 선택 이유 자체는 opaque policy result
- current signature/signer approval resolution
- `is_ephemeral`, submitter/worker same-device resolution
- count에 사용할 resolved failure domain
- 필요한 사실이 unknown인지

kernel 규칙:

1. 빈 checkpoint/holder/domain, 서로 다른 checkpoint/root 혼합, unknown membership/domain,
   한 holder에서 선택 관측 2개 이상은 typed error로 닫는다.
2. selected 관측은 holder당 최대 하나다. superseded 관측은 보고서에 남기되 count하지 않는다.
3. current signature/signer approval이 유효하지 않으면 제외한다.
4. ephemeral holder의 `WORKER_LOCAL` copy만 제외한다. 다른 kind까지 임의로 제외하지 않는다.
5. 같은 holder/device는 kind가 달라도 하나만 후보가 된다. 따라서 submitter와 worker가 같은
   device인 경우 별개 replica가 되지 않는다.
6. 후보를 resolved failure domain별로 한 번만 세며 입력 순서와 무관한 결정적 counted/excluded
   목록을 반환한다.
7. `effective_replica_count >= required_replicas`를 계산한다. `MIRRORED=1`,
   `REPLICATED=2`, `LOCAL=0` 매핑만 기존 protobuf 계약대로 사용한다.
8. system clock, ACK TTL, liveness, DB, signature verification, membership lookup은 하지 않는다.

`fsynced`/`hash_verified`는 signed body에 존재하지만, protocol의 count 규칙 표는 false 값을
어떻게 처리할지 명시적으로 적지 않는다 (`proto/artifact.proto:113-116,135-144`). 이 조각은
이를 몰래 새 guard로 만들지 않는다. 후속 resolver가 eligibility를 해소하도록 opaque resolution
입력으로 두거나, 별도 protocol 결정 뒤 typed exclusion reason으로 고정한다. 테스트가 이 경계를
“ACK body가 존재하면 무조건 count”로 오해하지 않게 한다.

## 5. 범위

### In — 0.5~1일

- `crates/checkpoint/src/durability.rs`의 기존 `ReplicaSet`을 holder/freshness-resolved
  immutable input과 결정적 report 기반 kernel로 보강 또는 교체
- required replica mapping 재사용
- holder/domain 중복 제거, missing/ambiguous fact fail-closed, 입력 순서 독립성
- counted/excluded/superseded 근거를 테스트 가능한 report로 반환
- `crates/checkpoint/tests`의 독립 synthetic snapshot 테스트
- 기존 checkpoint 상태기계/atomic writer 회귀 테스트

### Out

- `coordinator_replica_acks` 직접 load/join 또는 schema 변경
- raw durable body를 current `Verified<ReplicaAck>`로 승격
- membership/device/key/failure-domain durable repository
- ACK TTL, liveness probe/challenge, `fence_epoch` schema 추가
- JobManifest strict durability projection과 effective plan 선택
- `REPLICATING -> REPLICATED`, `REPLICATED -> COMMITTED` 또는 degraded 전이
- `DurabilityStatus` 저장, ControlStore `RecordDurabilityStatus`, canonical/Attempt/Job 전이
- production frame routing/replica producer

이번 조각은 상태를 바꾸지 않으므로 새 전이를 만들지 않는다. 후속 전이는 반드시
`state-machines.md:217-226`의 기존 행과 정확히 일치하는 transaction으로 별도 계획한다.

## 6. 완료 기준과 negative tests

1. 같은 logical input을 모든 대표 순서로 섞어도 report와 count가 동일하다.
2. 같은 holder의 서로 다른 `acked_at` 행을 둘 다 selected로 주면 오류이며 2로 세지 않는다.
3. 한 holder의 superseded 여러 행과 selected 한 행은 정확히 하나만 후보가 된다.
4. 서로 다른 holder라도 같은 failure domain이면 하나만 센다.
5. invalid/current-unknown signer approval은 count하지 않거나 typed missing error로 닫힌다.
6. ephemeral `WORKER_LOCAL`은 제외하고, 규범에 없는 “모든 ephemeral kind 제외” mutation은
   실패한다.
7. 같은 device의 WORKER_LOCAL/SUBMITTER_MIRROR는 둘로 세지 않는다.
8. 하나의 eligible distinct domain은 `MIRRORED`를 만족하지만 `REPLICATED`는 만족하지 않는다.
9. 두 eligible distinct domain은 `REPLICATED`를 만족한다.
10. 빈 holder/domain, mixed checkpoint/root, ambiguous freshness resolution은 fail closed한다.
11. kernel은 clock/network/filesystem/DB를 읽지 않으며 coordinator state/table을 변경하지 않는다.
12. 기존 checkpoint 전체 테스트와 state-table parity가 회귀하지 않는다.

권장 검증:

```powershell
cargo test -p gputeer-checkpoint replica --no-fail-fast
cargo test -p gputeer-checkpoint --no-fail-fast
cargo test -p gputeer-coordinator replica_ack_store --no-fail-fast
python scripts/check_docs.py
git diff --check
```

## 7. 이 조각이 여는 것과 전체 규모

| 단계 | 첫 슬라이스/잔여 | 예상 |
|---|---|---:|
| 오늘 | resolved-input effective replica pure kernel | 0.5~1일 |
| 다음 | authoritative device membership/key/ephemeral/failure-domain snapshot + ACK reverify/selection adapter | 2~3일 |
| 다음 | durable ACK/Job/Checkpoint join과 effective count snapshot 저장 | 1~2일 |
| 다음 | 표에 있는 `ACK_RECEIVED`/`DURABILITY_MET` 전이를 한 transaction으로 연결 | 2~3일 |
| 전체 | production routing, liveness/degraded 복구까지 포함한 `MIRRORED` lifecycle | 약 6~9일 |

직접 여는 것은 “membership adapter가 만들어야 하는 정확한 출력 계약”과 “그 출력이 주어졌을
때 count가 맞다는 독립 증명”이다. `MIRRORED` lifecycle 전체 완료 주장은 아니다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-24 | 최초 작성 |

## 구현 결과 (2026-08-24)

### 구현 범위

- `crates/checkpoint/src/durability.rs`에 `ResolvedHolderObservation`,
  `FactResolution`, `HolderValidation`, `ReplicaEvaluationError`,
  `EffectiveReplicaReport`와 `evaluate_effective_replicas()`를 추가했다.
- resolver가 선택한 observation과 현재 signature/membership, canonical device ID,
  ephemeral, failure-domain 해석만 입력으로 받는다. 시계, ACK TTL, liveness,
  filesystem/network/DB, signature 검증 또는 membership 조회는 하지 않는다.
- selected observation은 holder당 최대 하나만 허용한다. 복수 selected는 typed error로
  계산 전체를 닫고, superseded observation은 정렬된 report에 남기되 세지 않는다.
- 유효 후보는 canonical holder 순서로 정렬한 뒤 `BTreeMap`의 failure-domain별 첫 후보만
  센다. counted/excluded/superseded와 mixed-scope error의 scope 목록도 모두 canonical
  정렬하므로 입력 순열과 무관한 동일 report/error를 반환한다.
- current signature/승인이 invalid이거나 membership/failure-domain이 unresolved 또는
  ambiguous이면 typed exclusion으로 남기고 세지 않는다. ephemeral 해석은 규범상 필요한
  `WORKER_LOCAL`에만 요구하며, ephemeral의 non-local kind는 임의로 제외하지 않는다.
- 기존 `ReplicaSet`의 타입과 동작은 보존했다. 다만 이 legacy 타입이 freshness/kind/current
  membership을 표현하지 못한다는 주석을 바로잡고 새 consumer는 순수 kernel을 사용하도록
  명시했다.
- checkpoint 상태 전이, durable store, coordinator routing, proto/schema는 변경하지 않았다.

### negative 및 결정성 테스트

`crates/checkpoint/tests/effective_replica_kernel.rs`에 독립 테스트 13건을 추가했다.

- 4개 observation의 모든 24개 순열에서 report 전체가 동일함을 비교한다.
- 같은 holder의 복수 selected, mixed checkpoint/root, 빈 checkpoint/root/holder/domain을
  typed error로 검증한다.
- invalid signature, 미승인 holder, unresolved/ambiguous membership·failure-domain과
  `WORKER_LOCAL`의 unresolved/ambiguous ephemeral을 모두 non-count로 검증한다.
- 동일 failure-domain 중복, 같은 device의 서로 다른 kind, superseded observation,
  `MIRRORED=1`/`REPLICATED=2` 경계를 검증한다.
- resolver가 더 오래된 observation을 selected로 주고 더 최신 observation을 superseded로
  주어도 kernel이 timestamp 최대값이나 TTL 정책을 발명하지 않음을 검증한다.
- ephemeral `WORKER_LOCAL`만 제외하고 ephemeral 또는 ephemeral-미해석 non-local kind는
  세는 규범 경계를 검증한다.

### 뮤테이션 검증

production 분기를 실제 변경하고 지정 테스트 실패를 확인한 뒤 원복했다.

1. holder별 복수 selected 가드를 제거하자
   `duplicate_selected_holder_fails_closed_instead_of_counting`이 실패했고, 같은 holder가 서로
   다른 두 domain에서 count 2가 되는 생존 mutant를 정확히 드러냈다. 원복 후 PASS.
2. ephemeral 제외의 `kind == WORKER_LOCAL` 가드를 제거하자
   `only_ephemeral_worker_local_is_excluded`가 count 기대값 1, 실제 0으로 실패했다.
   원복 후 PASS.

### 자체 재검토

초안은 `is_ephemeral`이 unresolved/ambiguous이면 kind와 무관하게 제외했다. 그러나
`artifact.proto:135-152`의 규칙은 ephemeral node의 **local copy**만 제외하므로, non-local
kind에 이 해석을 요구하는 것은 규범에 없는 과잉 조건이었다. ephemeral fact를
`WORKER_LOCAL`일 때만 요구하도록 수정하고
`non_local_kind_does_not_require_irrelevant_ephemeral_resolution`을 추가했다.

또한 기존 `ReplicaSet`의 “규칙 1~4 적용” 주석이 실제 표현력과 맞지 않는 점을 바로잡았다.
동작은 바꾸지 않아 기존 API와 테스트 계약은 유지했다.

### 검증 결과

```text
cargo test -p gputeer-checkpoint --test effective_replica_kernel --no-fail-fast
PASS — 13 passed, 0 failed

cargo test -p gputeer-checkpoint --no-fail-fast
PASS — 67 passed, 0 failed

cargo build --workspace --exclude gputeer-runtime-windows
PASS — exit 0

cargo test --workspace --exclude gputeer-runtime-windows
PASS — exit 0, 0 failed, 기존 ignored 1건 유지
```

환경의 stable toolchain에 `rustfmt` component가 없어 `cargo fmt --all -- --check`는
실행하지 못했다. 소스는 수동 형식 검토와 Rust compiler로 검증했다.
