# 2026-08-21_1253_scheduler_best_fit_v1

- **대상:** scheduler 로드맵 조각 4의 하루 규모 재범위
- **선행 완료:** DoD-41(조각 1), DoD-42(2a), DoD-43(2b-1),
  DoD-44(3a)
- **조사 방식:** 현재 저장소 read-only 조사. 이 문서 외 구현 변경 없음
- **판정:** 조각 4 원안 전체는 오늘 완료 불가. 오늘 착수 후보는
  `crates/scheduler` 안의 **순수·결정적 resource best-fit kernel**뿐이다.

## 결론

로드맵의 조각 4는 단순 winner 선택이 아니다. 남은 §13.2/13.3 admission,
Declared estimate, Exclusive allocation, checkpoint/durability, 자원 CAS reservation,
rationale까지 합쳐 production 750~1,200줄 + 테스트 850~1,350줄, 4~6일로
잡혀 있다. 조각 3의 live producer와 session owner/fencing도 아직 없다.

현재 세 구현을 기계적으로 이어

```text
pool_snapshot → evaluate_eligibility → best-fit → stage_queued_with_lease
```

로 호출하는 코드는 만들 수 있다. 그러나 그것은 원자적 **자원** reservation이
아니다. 선택과 staging 사이에 inventory가 바뀔 수 있고, 서로 다른 두 Job이 같은
node/GPU를 선택해 각각 STAGING에 성공할 수 있다. 따라서 이 배선을 오늘 조각 4
완료라고 부르면 안 된다.

오늘의 정직한 최소 조각은 합성 immutable 입력 위에서 복수 적격 후보의 자원
여유량을 결정적으로 정렬하는 순수 kernel이다. Coordinator 배선과
`stage_queued_with_lease()` 호출은 조각 5의 시작 부분으로 넘긴다. 단, 그때도
단순 호출을 “CAS resource reservation”이라고 부르지 말고 inventory version과
allocation row를 같은 admission transaction에서 검사·기록하는 계약을 먼저
추가해야 한다.

## 확인한 현재 경계

### 1. `crates/scheduler`

- `evaluate_eligibility()`는 고정 `PoolSnapshot`·`JobRequirements`·`Policy`로
  hard gate를 판정하고 결과를 정렬한다
  (`crates/scheduler/src/filter.rs:7-34`).
- 복수 적격이면 `RankingRequired`만 낸다. `EligibleCandidate`에는 `node_id`만
  남는다 (`crates/scheduler/src/model.rs:206-226`).
- `CandidateSnapshot`에는 관측된 GPU별 available VRAM과 node 단위 CPU/RAM/
  workspace가 있고 `JobRequirements`에는 각 요구량이 있다
  (`crates/scheduler/src/model.rs:94-138`).
- 현재 `Policy`는 `maximum_snapshot_age_ms` 하나뿐이다
  (`crates/scheduler/src/model.rs:140-143`). best-fit의 의미, 자원 축 우선순위,
  동점 규칙은 아직 계약이 아니다.
- total capacity, 기존 reservation, Exclusive GPU 점유, inventory revision,
  normalized performance/cost, Declared `T_est`, checkpoint/durability 입력은 없다.

따라서 제안된 정확한 형태인 **`EligibilityReport + Policy`만 받는 랭킹**은
불가능하다. report에는 순위를 계산할 자원 사실이 없고 policy에도 순위 규칙이
없다. `node_id` 정렬의 첫 항목을 뽑는 것은 결정적이지만 best-fit은 아니다.

### 2. `crates/coordinator/src/inventory_store.rs`

- `CoordinatorInventoryStore::pool_snapshot()`은 registry와 최신 normalized
  inventory를 한 SQLite read snapshot에서 읽어 node ID 순 `PoolSnapshot`으로
  투영한다 (`inventory_store.rs:349-372`).
- 저장 row에는 `inventory_revision`이 있지만 `PoolSnapshot`/`CandidateSnapshot`에
  투영되지 않는다 (`inventory_store.rs:42-56`, `model.rs:94-138`).
- 관측 자원은 읽을 수 있을 뿐 reservation을 검사·차감하는 allocation table이나
  CAS API가 없다.
- heartbeat/capability wire, freshness producer, 실제 concurrent Agent session
  owner/fencing은 조각 3 미완 범위다. 그러므로 저장소가 복수 row를 담는다는
  사실은 live 경쟁 후보가 준비됐다는 뜻이 아니다.

### 3. `crates/coordinator/src/staging_store.rs`

- `stage_queued_with_lease()`는 `BEGIN IMMEDIATE` 한 transaction에서 Attempt/node,
  Lease, fence epoch, `QUEUED→STAGING`, operation idempotency를 원자화한다
  (`staging_store.rs:162-230`). 같은 Job에 대한 두 staging 경쟁은 한쪽만 성공한다.
- 파일 자체가 이 kernel은 “does not reserve resources or dispatch a Grant”라고
  경계를 명시한다 (`staging_store.rs:1-7`).
- 요청에는 선택된 `node_id`가 들어갈 뿐 GPU ID·자원량·inventory revision이 없다
  (`staging_store.rs:19-31`). transaction은 inventory나 allocation row를 읽지
  않는다.
- 따라서 이것은 **Job/Attempt/Lease의 원자적 admission**이지 roadmap 조각 4의
  **resource CAS reservation**은 아니다.

## 하루 안에 세 구현을 배선할 수 있는가

**정상 동작을 주장할 수준으로는 불가능하다.** 단순 happy-path glue보다 먼저
다음 계약이 필요하다.

1. 선택에 사용한 inventory revision 또는 snapshot token
2. node/GPU별 Exclusive allocation row와 Job/Attempt 소유권
3. capacity 재검사 + allocation 기록 + QUEUED→STAGING의 원자성 경계
4. CAS 실패 시 새 snapshot으로 filter/rank를 다시 수행하는 bounded retry
5. 실패·Grant 거부·timeout 때 allocation과 Lease를 함께 해제하는 규칙
6. Job manifest를 `JobRequirements`, Declared estimate, checkpoint/durability
   admission 입력으로 만드는 producer
7. 선택된 node를 실제 active Agent session owner에 결합하는 fencing

특히 inventory store와 staging store는 각각 임의 path로 열 수 있는 독립
connection/API다. 두 호출을 순서대로 놓는 것만으로는 한 transaction이 되지 않는다.
로드맵이 경고한 selection/reservation TOCTOU를 그대로 남긴다.

## 순수 랭킹으로 좁히는 방안

순수 랭킹만 먼저 만드는 것은 가능하고, 조각 3의 wire blocker와 무관하게 합성
snapshot으로 검증할 수 있다. 다만 공개 입력은 최소한 다음처럼 고쳐야 한다.

```rust
rank_best_fit(
    pool: &PoolSnapshot,
    job: &JobRequirements,
    report: &EligibilityReport,
    policy: &BestFitPolicy,
) -> Result<BestFitRanking, RankingError>
```

`EligibilityReport`에 자원 snapshot을 복제해서 넣는 것보다 기존 immutable
`pool`과 `job`을 함께 받는 편이 작고 출처가 명확하다. 구현 시에는 report가 같은
입력에서 나온 것인지 보장할 방법도 정해야 한다. 가장 안전한 public API는
`place_v01(pool, job, policy)`가 내부에서 `evaluate_eligibility()`와 랭킹을 한 번에
호출하고, 별도 report를 caller에게 받지 않는 형태다. 내부 랭킹 helper만 report를
받을 수 있다.

### 오늘 허용할 best-fit 의미

현재 입력으로 정직하게 계산할 수 있는 것은 **resource-tight fit**뿐이다.
reliability, fairness, locality, cost/performance, deadline 확률을 점수에 몰래
섞지 않는다.

- hard-filter를 통과한 후보만 대상으로 한다.
- healthy/model/VRAM 조건을 만족하는 GPU 중 Job이 요구한 개수를 골랐을 때의
  VRAM 잔여량, GPU 개수 잔여, CPU/RAM/workspace 잔여를 계산한다.
- 여러 자원 축은 임의 가중합을 쓰지 않는다. `BestFitPolicy`가 명시한 축 순서의
  lexicographic key로 비교한다.
- 모든 정책 key가 같을 때만 `node_id` 오름차순을 최종 tie-breaker로 쓴다.
- 중복 `node_id`, report에만 있거나 pool에만 있는 적격 ID, 필요한 값의 누락,
  `RankingRequired`와 후보 수 불일치는 fail-closed error다.

정책 축 순서가 합의되지 않았다면 코드를 시작하지 않는다. 고정된 임의 가중치나
“가장 작은 node_id”를 best-fit이라고 이름 붙이는 것보다 이 하루 조각을 보류하는
편이 맞다.

## 오늘 착수할 최소 조각

### 이름

**DoD-45 후보: scheduler 순수 deterministic resource best-fit kernel**

### In

- 변경 범위는 `crates/scheduler`의 model/ranking module/export와 독립 테스트뿐.
- 별도 `BestFitPolicy`, 비교 가능한 `FitKey`, 순위 목록과 winner를 담는
  `BestFitRanking`, malformed input용 `RankingError`를 정의한다.
- 기존 `evaluate_eligibility()`가 만든 복수 적격 집합만 정렬한다.
- 위 resource-tight lexicographic 정책과 `node_id` 최종 tie-break를 구현한다.
- network/filesystem/clock/random 없이 완전 순수 함수로 둔다.

### Out

- `inventory_store`/`staging_store`/Coordinator entrypoint 배선
- `stage_queued_with_lease()` 호출과 ID·Lease lifetime 생성
- inventory revision/CAS, allocation table, resource 차감·해제·재시도
- heartbeat/session owner/fencing, 실제 다중 Agent 경쟁
- Declared estimate, CUDA/architecture, availability, checkpoint/durability admission
- `PlacementRationale` proto, Grant dispatch
- reliability/chance-constrained/fairness/exploration
- 조각 4 또는 v0.1 placement+reservation 완료 주장

### 완료 조건

```text
1. 같은 pool/job/report/policy는 byte-for-byte 동등한 ranking을 낸다.
2. pool과 report 후보 순서를 뒤집어도 ranking이 같다.
3. 각 resource 축에서 더 작은 non-negative 잔여량의 후보가 정책 순서대로 앞선다.
4. 완전 동점은 node_id로만 해소한다.
5. 탈락 후보는 순위에 들어오지 않는다.
6. duplicate/missing/mismatched candidate와 unknown rank fact는 fail-closed한다.
7. 0/1 후보를 복수 후보 ranking으로 가장하지 않는다.
8. scheduler crate 밖 파일 diff는 0이고 I/O·clock·random 의존성도 0이다.
9. 결과는 placement 계산일 뿐 reservation/Grant 성공을 주장하지 않는다.
```

### 정직한 규모

- production Rust 약 **120~220줄**
- 테스트 약 **250~400줄**
- 계획/evidence 별도, **1일**

GPU subset과 multi-resource ordering 계약이 커지면 하루 범위를 넘는다. 그 경우
오늘은 타입·정책·결정성/오류 테스트까지만 더 줄이고 winner 구현을 다음 조각으로
넘긴다.

## 후속 순서

```text
오늘 DoD-45 후보: 순수 resource best-fit
  ↓
조각 3 잔여: heartbeat/capability + concurrent session owner/fencing
  ↓
조각 4 잔여: admission fact + inventory version + allocation/CAS reservation
  ↓
조각 5: CAS 성공 결과를 stage_queued_with_lease와 active session Grant에 연결,
        거부/timeout release와 fresh-snapshot 재계산
```

wire-up을 조각 5로 넘기는 것은 타당하다. 다만 roadmap의 resource reservation
자체까지 이름만 바꿔 조각 5로 숨기지 않는다. 조각 5가 시작되기 전에 또는 그 첫
하위 조각으로 allocation/CAS 계약이 반드시 있어야 한다.

## 구현 결과 (2026-08-21)

### 구현한 경계

- `crates/scheduler/src/rank.rs`에 순수 함수
  `rank_best_fit(pool, job, report, policy)`를 추가했다.
- `model.rs`에 `FitAxis`, `BestFitPolicy`, `FitKey`, `RankedCandidate`,
  `BestFitRanking`, `RankingError`만 추가하고 기존 `PoolSnapshot`,
  `JobRequirements`, `EligibilityReport`, `MissingFact`를 재사용했다.
- 입력 report는 `RankingRequired`이면서 적격 후보가 2개 이상이어야 한다.
  pool/report의 node 집합은 rejected 후보까지 포함해 정확히 일치해야 하며 빈 ID,
  중복 ID, report-only/pool-only ID는 모두 오류로 닫힌다.
- hard-filter 적격 후보마다 healthy/model/minimum VRAM을 만족하는 GPU를 VRAM
  오름차순으로 놓고 요구 개수만큼 가장 tight한 subset을 선택한다. VRAM key는 그
  subset의 `available - required` 합, GPU count key는 조건을 만족한 GPU 수에서 요구
  개수를 뺀 값이다. CPU/RAM/workspace key도 각각 `available - required`다. 부족,
  누락, 합계 overflow는 순위에서 제외하는 대신 전체 호출을 오류로 닫는다.
- `BestFitPolicy.axis_order`는 다섯 축의 완전한 순열이다. 작은 non-negative
  잔여량을 축 순서대로 lexicographic 비교하고 모든 축이 같을 때만 `node_id`
  오름차순으로 해소한다.

### 순위 기준의 근거와 좁힌 부분

마스터 플랜 §13.2의 hard filter를 통과한 뒤 §33.2 v0.1의 “단순 best-fit”을
수행하되, §13.4 chance-constrained selection은 v0.2이므로 넣지 않았다. 구체적인
resource-tight 잔여량과 lexicographic 비교는 이 문서의 “오늘 허용할 best-fit
의미”를 그대로 구현했다. 마스터 플랜에는 v0.1 자원 축의 고정 우선순위가 없으므로
임의 기본 순서를 만들지 않았다. 대신 모든 호출이 `BestFitPolicy`에 다섯 축의
순서를 명시하도록 했고 중복 축은 fail-closed한다.

Coordinator 배선, `stage_queued_with_lease()`, inventory revision/CAS,
allocation, reservation/Grant는 추가하지 않았다. report가 같은 hard-filter
`Policy`와 동일 snapshot에서 생성됐다는 암호학적/버전 기반 증명도 현재 타입에는
없다. 이 kernel은 ID 집합과 순위 계산에 필요한 자원 사실의 정합성까지만 검사한다.

### 테스트와 뮤테이션

`crates/scheduler/tests/best_fit.rs`에 15개 테스트를 추가했다.

- 결정성: pool/report 순서 역전 결과와 debug byte 표현 동일성
- 정책: 다섯 자원 축 각각의 작은 잔여량 우선, 축 순서 변경에 따른 winner 변경
- GPU subset: 가장 tight한 적격 GPU 요구 개수 선택과 잔여량 합, 동일 후보의 GPU
  벡터를 뒤집어도 전체 `BestFitRanking`이 동일함
- 동점: 완전 동점은 `node_id` 오름차순만 사용
- negative: rejected 후보 배제, 0/1 후보 거부, pool/report 중복 ID,
  report-only/pool-only ID, 중복 정책 축, Job/candidate rank fact 누락,
  snapshot/report GPU·CPU 자원 불일치(underflow 방지), VRAM 합 overflow

수동 뮤테이션 3건은 각각 기대한 테스트 실패를 확인하고 원복했다.

1. CPU 잔여량 비교 방향을 반대로 변경:
   `policy_axis_order_changes_which_tradeoff_wins` 실패
   (`vram-tight`을 `cpu-tight` 대신 선택).
2. 최종 `node_id` 비교 방향을 반대로 변경:
   `complete_tie_is_broken_only_by_node_id_ascending` 실패
   (`node-z`를 `node-a` 대신 선택).
3. 적격 GPU VRAM 정렬을 생략:
   `gpu_inventory_permutations_produce_identical_ranking` 실패
   (정방향은 `node-b`, 역방향은 `node-a`를 winner로 선택).

원복 뒤 targeted test와 scheduler 전체 테스트가 다시 통과했다.

### 검증 결과

```text
cargo build --workspace --exclude gputeer-runtime-windows
  PASS (exit 0)

cargo test --workspace --exclude gputeer-runtime-windows
  PASS (exit 0; 기존 ignored 1건 유지)

cargo test -p gputeer-scheduler
  PASS (48 passed: best_fit 15 + hard_filter 33; 0 failed)

git diff --check
  PASS (공백 오류 0; Git의 LF→CRLF 경고만 있음)
```

현재 toolchain에 `rustfmt` component가 설치되어 있지 않아 `cargo fmt -p
gputeer-scheduler`는 실행하지 못했다. 새/변경 Rust 파일은 수동 형식 검토와
컴파일로 확인했다.

### 자체 재검토

검토 중 다음을 확인하고 유지했다.

- scalar VRAM key가 임의 GPU 입력 순서에 좌우되지 않도록 적격 VRAM을 먼저
  정렬하고 가장 tight한 subset만 합산한다.
- `u64` VRAM 합과 `usize→u32` GPU 잔여 개수 변환은 포화/랩어라운드 대신
  명시적 `FitOverflow`로 실패한다.
- stale report의 non-GPU 부족 subtraction 경로에 직접 negative test가 없음을
  발견해 CPU one-below 테스트를 추가했고, underflow 대신 mismatch 오류를 확인했다.
- 성공 결과의 comparator는 모든 정책 축 뒤에 고유 `node_id`를 비교하므로 total
  order이며 stable sort의 입력 순서에 기대지 않는다.
- 결과 타입과 crate 문서에 ranking이 reservation/Grant 성공을 뜻하지 않음을
  명시했다.

재검토 후 추가로 발견되어 남은 production 결함은 없다. 다만 위에서 밝힌 report
origin 증명 부재는 현 타입 경계의 한계이며 조각 5의 inventory version/CAS 계약
없이는 해결됐다고 주장하지 않는다.

물리 줄 수는 `rank.rs` 269줄 + `model.rs` 타입 추가 54줄로 사전 추정
120~220줄을 넘었다. 기능 범위를 넓힌 결과가 아니라 duplicate/missing/mismatched
입력과 overflow를 모두 명시적 오류로 닫는 검증 코드가 예상보다 컸기 때문이다.
테스트는 405줄로 추정 범위(250~400줄)를 5줄 넘었다.
