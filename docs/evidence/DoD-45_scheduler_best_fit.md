---
schema_version: 2
id: DoD-45
claim: "scheduler 로드맵 조각 4를 '순수 resource best-fit kernel'로 완료했다. rank_best_fit()이 hard-filter 적격 후보 중 가장 tight한 GPU 요구 개수를 고른 뒤 BestFitPolicy의 VRAM 잔여 합→GPU 수 잔여→CPU/RAM/workspace 잔여 축 순서로 lexicographic 비교하고 완전 동점은 node_id 오름차순으로 해소하며, GPU 벡터·pool·report 입력 순서 독립성, malformed 입력 fail-closed와 뮤테이션 판별력을 구현·검수 3라운드 끝의 ACCEPTED 및 감독자 cargo test 48/48로 확인했다"
status: PASS
commit: 187776033ec5bdb99c8ca1fa5607d643d22987f5

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — 검수 1라운드 지적 후 GPU permutation test 보강"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-21T13:26:28+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 3라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(CHANGES_REQUESTED) — GPU 벡터 자체의 순열 동등성 테스트와 VRAM 정렬 제거 뮤테이션 요구. 2라운드(CHANGES_REQUESTED) — 조각 4 전체가 미커밋인데 lib.rs/model.rs 1차 구현 산출물을 후속 수정분으로 오인한 git-diff-scope 오탐. 3라운드(ACCEPTED) — HEAD 1877760이 DoD-44임과 두 production 파일이 module 연결·신규 타입인 순수 1차 산출물임을 확인하고, 새 GPU reverse 전체-ranking 동등성 테스트와 정렬 제거 시 node-b/node-a winner 분기의 판별력을 재확인해 최종 수용"
review_artifact: "docs/evidence/_raw/DoD-45_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-45_scheduler_best_fit_2026-08-21.txt"
raw_output_digest: "sha256:03acdf1bd9f7e377733499f9374156249bc753dd6822f9e75ac42732858311bd"
raw_output_bytes: 6557

binary_digests:
  toolchain: "C:\\Users\\playdata2\\.cargo\\bin\\cargo.exe 사용 — 제공된 이력과 이번 실행에 rustc/cargo version·binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — 기존 scheduler domain model을 확장한 process-local 순수 ranking API"
  canonical_spec: "wire canonical/signature 계약 미사용 — immutable PoolSnapshot·JobRequirements·EligibilityReport·BestFitPolicy만 입력"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test"
hardware: "GPU 미사용 — 합성 CandidateSnapshot/GpuSnapshot으로 순위·결정성·오류 경계 검증"
network_profile: "네트워크 미사용 — filesystem/clock/random도 읽지 않는 in-process 순수 함수"
command: |
  C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-scheduler
raw_output: |
  (docs/evidence/_raw/DoD-45_scheduler_best_fit_2026-08-21.txt,
   docs/evidence/_raw/DoD-45_review.txt 전문 참조)

  감독자 직접 확인: PASS — best-fit 15 + hard-filter 33 = 48 passed, 0 failed
  독립 검수 1라운드: CHANGES_REQUESTED — GPU 벡터 순열 직접 검증 요구
  독립 검수 2라운드: CHANGES_REQUESTED — 미커밋 전체 diff를 후속 수정으로 오인한 scope 오탐
  독립 검수 3라운드: ACCEPTED — HEAD·1차 산출물 범위 명확화와 새 테스트·뮤테이션 재확인
artifacts:
  - docs/plans/2026-08-21_1253_scheduler_best_fit_v1.md
  - docs/reports/2026-08-21_1308_scheduler_best_fit_kernel.md
  - crates/scheduler/src/lib.rs
  - crates/scheduler/src/model.rs
  - crates/scheduler/src/rank.rs
  - crates/scheduler/tests/best_fit.rs
  - docs/evidence/_raw/DoD-45_scheduler_best_fit_2026-08-21.txt
  - docs/evidence/_raw/DoD-45_review.txt
negative_tests:
  - "gpu_inventory_permutations_produce_identical_ranking: 각 후보 GPU 벡터를 reverse한 입력과 정방향 입력의 BestFitRanking 전체가 같으며 VRAM 정렬 제거 뮤테이션은 node-b/node-a로 winner가 갈려 실패"
  - "pool_and_report_permutations_produce_byte_equal_debug_output: pool/report 후보 역순에도 구조체와 debug byte 표현이 동일"
  - "zero_or_one_candidate_report_is_not_misrepresented_as_a_ranking: 복수 후보가 아닌 report와 조작된 RankingRequired를 명시적 오류로 거부"
  - "duplicate_pool_and_report_ids_fail_closed: pool/report의 중복 node ID를 결정적 오류로 거부"
  - "report_only_and_pool_only_candidate_ids_fail_closed: pool/report 후보 집합 불일치를 양방향으로 거부"
  - "missing_job_and_candidate_rank_facts_fail_closed: Job·후보의 ranking 필수 사실 누락을 기본값 없이 거부"
  - "report_from_resource_incompatible_snapshot_fails_closed: stale report가 현재 GPU 부족 후보를 적격으로 들고 와도 underflow·순위 제외 대신 mismatch 오류"
  - "stale_report_cannot_underflow_a_non_gpu_resource_remainder: stale report와 CPU one-below snapshot 조합을 mismatch 오류로 거부"
  - "vram_remainder_overflow_fails_closed: tight GPU subset의 VRAM 잔여 합 overflow를 wrap하지 않고 FitOverflow로 거부"
limitations:
  - "고정 immutable 입력에 대한 순수 ranking 계산만 증명하며 Coordinator 호출 경로에 배선되지 않았다"
  - "inventory revision/snapshot token이 없어 report가 같은 snapshot과 hard-filter Policy에서 생성됐음을 완전히 증명하지 못하고 ID 집합과 ranking fact 정합성까지만 검사한다"
  - "allocation table, resource 차감·해제, CAS reservation, 동시 Job 경쟁과 bounded retry는 없다"
  - "stage_queued_with_lease 결합, Lease lifetime 생성, active Agent session 선택과 Grant dispatch는 없다"
  - "Declared estimate, CUDA/architecture, availability, checkpoint/durability admission과 performance/cost/reliability/fairness 비교는 없다"
  - "실제 GPU hardware·telemetry·network·filesystem을 사용하지 않아 운영 환경의 resource 변화나 TOCTOU를 입증하지 않는다"
  - "rank.rs 269줄과 model.rs 타입 추가 54줄, 테스트 405줄로 계획 추정 production 120~220줄·테스트 250~400줄을 넘었다 — malformed/overflow fail-closed 검증과 GPU permutation 보강 때문이며 범위 밖 배선 기능을 넣은 결과는 아니다"
decision: "scheduler 로드맵 조각 4의 전체 placement/reservation을 완료했다고 과장하지 않고, 기존 hard-filter가 만든 복수 적격 후보를 고정 snapshot의 자원 잔여량으로 결정 정렬하는 순수 kernel로 제한했다. GPU는 healthy·model·최소 VRAM을 만족하는 값들을 먼저 정렬해 요구 개수만큼 가장 tight한 subset만 사용하고, 호출자가 명시한 다섯 FitAxis 완전 순열로 작은 잔여량을 lexicographic 비교한 뒤 완전 동점에서만 node_id 오름차순을 사용한다. 구현 1라운드 뒤 독립 검수는 GPU vector 자체의 순열 검증이 비어 있음을 찾아 CHANGES_REQUESTED했고, reverse된 GPU 벡터의 BestFitRanking 전체 동등성 테스트와 VRAM 정렬 제거 시 node-b/node-a로 실제 winner가 갈리는 뮤테이션을 추가했다. 2라운드의 CHANGES_REQUESTED는 조각 전체 미커밋 상태에서 lib.rs/model.rs의 1차 module·타입 산출물을 이번 수정으로 오인한 git-diff-scope 오탐이었고, 감독자가 HEAD 1877760과 diff를 명확히 한 3라운드에서 새 테스트·뮤테이션·범위를 재확인해 ACCEPTED했다. 감독자는 scheduler 테스트 48건을 직접 재확인했다. scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4 완료 — 남은 조각 3 나머지·5~9는 후속"
---

# DoD-45 · scheduler 순수 deterministic resource best-fit kernel (로드맵 조각 4)

## 무엇을 입증하려 했는가

기존 `evaluate_eligibility()`가 고정 snapshot에서 만든 복수 적격 후보를 외부 상태,
시각, 난수 없이 resource-tight 순서로 정렬하고, 동일 입력의 후보·GPU 벡터 순서가
달라도 byte-for-byte 동등한 ranking을 내며 malformed 입력을 fail closed하는지를
검증했다.

## 범위 결정 — placement가 아닌 순수 ranking kernel

로드맵 조각 4 전체에는 admission fact, inventory version, allocation row, CAS
reservation까지 필요하다. 현재 domain model에는 inventory revision이나 allocation
계약이 없으므로 이번 완료 범위는 **순수 deterministic resource best-fit kernel**로
제한했다. 계산된 winner는 reservation이나 Grant 성공을 뜻하지 않는다.

## 구현 — `rank_best_fit()`

`crates/scheduler/src/rank.rs`를 신설하고 `rank_best_fit(pool, job, report, policy)`를
구현했다. `model.rs`에는 `FitAxis`·`BestFitPolicy`·`FitKey`·`RankedCandidate`·
`BestFitRanking`·`RankingError`를 추가하고 `lib.rs`에서 공개했다.

각 적격 후보에서 healthy·허용 model·최소 VRAM을 만족하는 GPU의 available VRAM을
오름차순 정렬하고 요구 개수만큼 가장 tight한 subset을 고른다. `FitKey`는 이 subset의
VRAM 잔여 합과 전체 적격 GPU 수의 잔여, CPU/RAM/workspace 잔여를 담는다. 정책은
다섯 축을 중복 없이 모두 명시하며, 각 잔여량을 작은 값 우선으로 lexicographic
비교하고 모든 축이 같을 때만 `node_id` 오름차순으로 동점을 해소한다.

## 독립 검수 3라운드 — **ACCEPTED**

1라운드는 기존 결정성 테스트가 pool/report 후보 순서만 뒤집고 GPU 벡터 자체를
순열화하지 않아 `matching_vram.sort_unstable()`의 필요성을 직접 검증하지 못한다고
판정했다. 구현 2라운드는 각 후보 GPU 벡터를 reverse한 뒤 정방향/역방향의
`BestFitRanking` 전체를 비교하는 테스트를 추가했다. 정렬 제거 뮤테이션에서 실제로
정방향 `node-b`, 역방향 `node-a`가 winner가 되어 새 테스트의 판별력도 확인했다.

2라운드는 `git diff`에 보인 `lib.rs`·`model.rs`를 후속 수정의 범위 확장으로 오인해
`CHANGES_REQUESTED`했다. 조각 4 전체가 미커밋이었고 기준 HEAD가 DoD-44의
`1877760`임을 감독자가 직접 명확히 한 뒤, 3라운드 검수는 두 파일이 1차 구현의
module 연결과 신규 타입일 뿐임을 확인했다. 새 GPU permutation 테스트와 뮤테이션,
범위 밖 Coordinator·reservation·Grant 무변경을 재확인해 최종 `ACCEPTED`했다.

## 결과

```text
C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-scheduler
  PASS — best-fit 15 + hard-filter 33 = 48 passed, 0 failed
```

위 결과는 이 evidence 작성 중 감독자가 직접 재확인했다.

## 이 실험이 증명하지 "않는" 것

- inventory revision/snapshot origin과 allocation CAS reservation은 없다.
- 동시 Job의 자원 경쟁, 차감·해제·retry와 TOCTOU 안전성을 증명하지 않는다.
- Coordinator staging, active session, Grant dispatch에 연결되지 않았다.
- 실제 GPU telemetry나 hardware의 정확성과 가변성을 검증하지 않는다.
- estimate·checkpoint/durability·성능·비용·신뢰성·공정성 정책은 범위 밖이다.

## 결정

1. scheduler 로드맵 조각 4를 전체 placement/reservation이 아니라 순수 deterministic
   resource best-fit kernel로 완료했다.
2. GPU tight subset, 정책 지정 lexicographic 비교, `node_id` 최종 tie-break,
   malformed 입력 fail-closed와 입력 순서 결정성을 검증했다.
3. GPU 순열 직접 검증 공백을 1라운드가 발견했고, 구현 보강 뒤 2라운드 scope 오탐을
   HEAD·전체 미커밋 상태로 명확히 해 3라운드에서 `ACCEPTED`했으며 감독자가 scheduler
   테스트 48/48을 직접 재확인했다.
4. scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4 완료 — 남은 조각 3 나머지·5~9는 후속.

관련: `docs/plans/2026-08-21_1253_scheduler_best_fit_v1.md`
