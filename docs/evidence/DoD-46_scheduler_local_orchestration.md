---
schema_version: 2
id: DoD-46
claim: "scheduler 로드맵 조각 5를 '로컬 orchestration kernel'로 완료했다. crate-internal orchestrate_placement_to_staging()이 pool_snapshot()→evaluate_eligibility()→0/1/N 후보 분기→N에서만 rank_best_fit()→stage_queued_with_lease()를 조합하고 caller-supplied ID·coordinator term·시각·Lease 수명을 사용함을 구현·독립 검수 1라운드 ACCEPTED·감독자 coordinator 테스트 78/78로 확인했다. 단 inventory revision/CAS reservation이 없어 서로 다른 Job의 같은 GPU 중복 선택을 막지 못하므로 이 함수는 private module의 test fixture 외 production 경로에는 연결되지 않았다"
status: PASS
commit: 21ef993d6643f2c89d61f3b3ab6d838c2be42bc8

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — private orchestration module과 실제 파일 DB 테스트 7건, 뮤테이션 2건"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-21T15:24:31+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "private module·pub(crate) 함수와 test-only 유일 호출, production run()/accept-loop의 별도 issue_grant() 유지, inventory read와 staging write 사이 revision/CAS reservation 부재로 순차 호출도 같은 GPU를 중복 선택할 수 있다는 정직한 위험 보고, 0/1/N 분기, 뮤테이션 2건 판별력, unchanged-inventory에 한정된 replay 의미, orchestrate.rs+lib.rs 2줄+문서 범위를 확인해 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-46_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-46_scheduler_local_orchestration_2026-08-21.txt"
raw_output_digest: "sha256:37fc64d9ffff848fa2405f139681fc63cbcb312bed60507de04d92428dc4243d"
raw_output_bytes: 7645

binary_digests:
  toolchain: "C:\\Users\\playdata2\\.cargo\\bin\\cargo.exe 사용 — 제공된 이력과 이번 실행에 rustc/cargo version·binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — 기존 scheduler·Coordinator store의 process-local crate-internal 조합 API"
  canonical_spec: "wire canonical/signature 계약 미사용 — Grant 생성·서명·전송 경로에 연결되지 않음"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test"
hardware: "GPU 미사용 — 합성 inventory와 임시 SQLite control DB로 placement-to-staging 경계 검증"
network_profile: "네트워크 미사용 — private module의 단위 테스트만 실행하고 accept-loop/wire 경로는 미변경"
command: |
  C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-46_scheduler_local_orchestration_2026-08-21.txt,
   docs/evidence/_raw/DoD-46_review.txt 전문 참조)

  감독자 직접 확인: PASS — coordinator unit 74 + integration 4 = 78 passed, 0 failed
  독립 검수 1라운드: ACCEPTED — private/test-only 격리, 0/1/N 분기, CAS 부재 위험,
  뮤테이션 2건, replay 의미와 전체 변경 범위를 확인해 잔여 요청 없이 수용
artifacts:
  - docs/plans/2026-08-21_1502_scheduler_grant_dispatch_v1.md
  - docs/reports/2026-08-21_1514_scheduler_local_orchestration.md
  - crates/coordinator/src/lib.rs
  - crates/coordinator/src/orchestrate.rs
  - docs/evidence/_raw/DoD-46_scheduler_local_orchestration_2026-08-21.txt
  - docs/evidence/_raw/DoD-46_review.txt
negative_tests:
  - "zero_candidates_preserves_queued_job_and_creates_no_attempt: 0 후보에서 Job을 QUEUED로 유지하고 Attempt·epoch·staging side effect를 만들지 않음"
  - "one_candidate_bypasses_ranking_and_stages_that_node: 1 후보는 rank_best_fit()을 호출하지 않고 유일 node를 stage하며, 단일 후보 ranking 호출 뮤테이션은 ResolutionNotRankingRequired로 실패"
  - "multiple_candidates_stage_the_best_fit_winner: N 후보에서만 best-fit winner를 stage하며 first-eligible 뮤테이션은 기대 node-b 대신 node-a를 골라 실패"
  - "invalid_ranking_policy_fails_closed_before_staging: malformed ranking policy를 staging 전에 typed Ranking error로 거부"
  - "staging_validation_error_does_not_claim_success_or_consume_epoch: storage validation 실패를 success로 가장하지 않고 epoch를 소비하지 않음"
  - "corrupt_inventory_fails_closed_before_staging: 손상 inventory payload를 선택·stage하지 않고 typed Inventory error로 거부"
  - "operation_replay_returns_the_original_durable_staging_result: 동일 scheduler 입력과 unchanged inventory에서 exact-request replay가 최초 Attempt·Lease를 반환하고 epoch를 재소비하지 않음"
limitations:
  - "inventory revision token, allocation row와 CAS reservation이 없어 서로 다른 Job의 순차·동시 호출이 unchanged inventory에서 같은 node/GPU를 중복 선택할 수 있다"
  - "이 위험 때문에 orchestrate module은 private이고 유일한 함수 호출은 #[cfg(test)] fixture다. production run()/accept-loop는 기존 별도 issue_grant()를 계속 사용한다"
  - "rank_best_fit()이 선택 GPU UUID를 반환하지 않아 Lease GPU scope, GrantedExecutionPlan과 PlacementRationale을 만들지 않는다"
  - "원본 Manifest 보관과 JobRequirements adapter가 없어 검증·정규화된 JobRequirements를 caller에게 받고 Grant를 만들지 않는다"
  - "node_id/device_id/session owner를 결합하는 routing과 live socket registry가 없어 특정 Agent session이나 ACK identity를 선택하지 않는다"
  - "wire Grant 생성·서명·전송, ACK/거부/timeout, rollback/requeue와 Lease/allocation 정리는 구현하지 않았다"
  - "replay는 동일 scheduler 입력과 unchanged inventory의 staging-store exact-request 의미만 검증한다. changed-inventory retry가 기존 operation을 선택보다 먼저 찾는 orchestration-level replay는 보장하지 않는다"
  - "inventory read와 staging write는 별도 SQLite transaction이라 snapshot과 stage 사이 TOCTOU를 닫지 않는다"
decision: "scheduler 로드맵 조각 5의 전체 Grant dispatch를 완료했다고 과장하지 않고, 기존 inventory projection·hard-filter·best-fit·durable staging을 순서대로 묶는 crate-internal 로컬 placement-to-staging orchestration kernel로 제한했다. 설계 조사는 구현 전에 실제 계약 불일치 7건을 찾아 0/1/N resolution 분기와 caller-supplied ID·coordinator term·시각·Lease 수명 경계를 구현으로 해결하고, selected GPU UUID/Grant scope, Manifest adapter, node/device/session routing, inventory revision/CAS reservation, wire/ACK/rollback은 명시적으로 범위 밖에 남겼다. 특히 inventory CAS reservation이 없어 unchanged inventory에서 서로 다른 Job을 순차 호출해도 같은 GPU를 다시 선택할 수 있음을 인정하고, private module·pub(crate) 함수·test-only 유일 호출로 production에서 격리했다. 독립 검수는 이 격리와 기존 run()/accept-loop의 issue_grant() 유지, 0/1/N 분기, 자원 중복 위험, 두 뮤테이션의 판별력, unchanged-inventory에 한정된 replay 의미, 제한된 변경 범위를 확인해 1라운드 만에 ACCEPTED했다. 감독자는 coordinator 테스트 78건을 직접 재확인했다. scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4·5 완료(5는 production 미연결 kernel 만) — 남은 조각 3 나머지·inventory CAS reservation(6번 불일치 해결)·실제 wire 연결·조각 6~9 는 후속"
---

# DoD-46 · scheduler 로컬 placement-to-staging orchestration kernel (로드맵 조각 5)

## 무엇을 입증하려 했는가

기존 `pool_snapshot()`·`evaluate_eligibility()`·`rank_best_fit()`·
`stage_queued_with_lease()` 계약을 한 crate-internal application seam에서 정확한 순서와
0/1/N 분기로 조합하고, 실패를 stage 성공으로 가장하지 않는지를 검증했다. 동시에
inventory revision/CAS reservation이 없는 현재 계약으로는 자원 중복 선택을 막을 수
없으므로 production에 연결하지 않았다는 제한도 코드와 검수로 확인했다.

## 범위 결정 — Grant dispatch가 아닌 로컬 orchestration kernel

설계 조사는 구현 전에 실제 계약 불일치 7건을 찾았다. 이 조각은 0/1/N resolution
분기와 caller-supplied operation/attempt/lease/coordinator identity·term·시각·Lease
수명 경계를 해결했다. selected GPU UUID·Grant scope, 원본 Manifest 변환,
node/device/session routing, inventory revision/CAS reservation, wire/ACK/rollback은 현재
계약만으로 안전하게 만들 수 없어 범위 밖에 뒀다.

가장 중요한 제한은 resource reservation 부재다. inventory read와 staging write가
별도 transaction이고 allocation row/CAS가 없어서, inventory가 갱신되지 않으면 서로
다른 Job의 순차 호출도 같은 GPU를 중복 선택할 수 있다. 따라서 module은 비공개이고
유일한 호출은 `#[cfg(test)]` fixture이며 production `run()`/accept-loop는 여전히 별도
`issue_grant()`를 사용한다.

## 구현 — `orchestrate_placement_to_staging()`

`crates/coordinator/src/orchestrate.rs`를 신설하고 `pub(crate)` 함수가 한 번의
`pool_snapshot()` 결과를 hard-filter와 ranking에 함께 사용하도록 했다.
`NoEligibleCandidates`는 Job을 QUEUED로 둔 채 반환하고, `SingleEligible`은 ranking을
우회하며, `RankingRequired`에서만 `rank_best_fit()`을 호출한다. 선택된 node는 호출자가
공급한 issuance input과 합쳐 `stage_queued_with_lease()`에 정확히 한 번 전달된다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 private module·`pub(crate)` 가시성과 저장소 전체 test-only 유일 호출을
확인하고 production loop가 기존 `issue_grant()`를 유지함을 추적했다. inventory와
staging transaction 사이 CAS가 없어 순차 호출도 같은 GPU를 재선택할 수 있다는 위험
보고, 0/1/N 분기, typed fail-closed 오류와 두 뮤테이션의 판별력을 모두 확인했다.

replay 테스트는 동일 scheduler 입력과 unchanged inventory에 한정된 staging store의
exact-request replay만 증명한다. changed-inventory retry를 선택 전에 기존 operation으로
해결하지 못한다는 자기 보고도 정확하다고 판정했다. 변경 범위가 `orchestrate.rs`,
`lib.rs`의 private module 선언 2줄과 문서뿐임을 확인해 잔여 요청 없이 `ACCEPTED`했다.

## 결과

```text
C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-coordinator
  PASS — unit 74 + integration 4 = 78 passed, 0 failed
```

위 결과는 이 evidence 작성 중 감독자가 직접 재확인했다.

## 이 실험이 증명하지 "않는" 것

- inventory revision/allocation CAS reservation과 multi-Job 자원 중복 방지는 없다.
- selected GPU UUID, Lease GPU scope, Grant plan/rationale을 만들지 않는다.
- 원본 Manifest 보관·검증과 `JobRequirements` 변환기를 제공하지 않는다.
- node/device/session routing, active session registry와 ACK identity 결합은 없다.
- wire Grant 전송, ACK/거부/timeout, rollback/requeue와 Lease/allocation 정리는 없다.
- changed-inventory orchestration replay나 snapshot-to-stage TOCTOU 안전성을 보장하지 않는다.

## 결정

1. scheduler 로드맵 조각 5를 전체 Grant dispatch가 아니라 crate-internal 로컬
   placement-to-staging orchestration kernel로 완료했다.
2. 0/1/N 분기, N에서만 best-fit 호출, caller-supplied issuance input, typed
   fail-closed와 snapshot 1회 사용을 실제 파일 DB 테스트로 검증했다.
3. inventory revision/CAS reservation이 없어 같은 GPU 중복 선택을 막지 못하므로
   private module의 test fixture 외 production 경로에는 연결하지 않았다.
4. 독립 검수는 격리·분기·뮤테이션·replay 의미·범위를 확인해 1라운드 만에
   `ACCEPTED`했고 감독자가 coordinator 테스트 78/78을 직접 재확인했다.
5. scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4·5 완료(5는 production 미연결 kernel 만) — 남은 조각 3 나머지·inventory CAS reservation(6번 불일치 해결)·실제 wire 연결·조각 6~9 는 후속.

관련: `docs/plans/2026-08-21_1502_scheduler_grant_dispatch_v1.md`
