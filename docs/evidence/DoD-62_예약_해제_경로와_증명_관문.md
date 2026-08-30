---
schema_version: 2
id: DoD-62
claim: "`crates/coordinator/src/reservation_release.rs` 의 `CoordinatorReservationReleaseStore` 가 노드 예약 해제를 하나의 `BEGIN IMMEDIATE` 안에서 원자적으로 수행하되, **오늘 정직한 호출은 아무 예약도 풀지 못한다**. `DoD-49` 가 미뤄 둔 release 를 만들면서 초안은 '검증된 terminal `AttemptReport` 가 실행 종료의 증명' 이라고 전제했는데 **그 전제가 틀렸다** — `DoD-51` evidence 가 '저장 성공은 프로세스 종료를 증명하지 않으며 reservation release 의 충분조건이 아니다' 라고 직접 적어 뒀고 독립 검수가 이를 반박했다. 그래서 계획서의 '안전한 release proof 최소 형태' 4조건 중 1번만 코드로 확인하고, 2·4번은 `ReleaseAuthorization` 의 진술로 요구하며, 3번(전이 결합)은 **이 API 로 만족시킬 수 없음을 명시**한다. 저장된 증거 행 하나로는 풀 수 없고(재검증한 `Verified` 와 바이트 단위로 같아야 한다), 다른 attempt 가 잡고 있는 예약은 절대 지우지 않는다"
status: PASS
commit: 65efdc1ec142b7556132c06a5228685547ef4b5c

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — reservation_release 신설, 통합 테스트 20건, 뮤테이션 14건"
executor_model: "claude-opus-5"
executed_at: "2026-08-30T23:10:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 3라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1R: **핵심 전제 반박** — `crates/coordinator/src/reservation_release.rs:237-277`(검증된 terminal report 만으로 삭제. runtime-stop·process-tree 종료·VRAM 반환·Lease 종료·Attempt 전이를 확인 안 함. 정상 키로 '종료' 를 서명한 뒤 계속 실행되면 위조 없이 중복 실행), `:205-215`(`Verified` 는 임의 keyring 으로도 만들 수 있어 authoritative directory provenance 를 강제 못 함), `crates/coordinator/tests/reservation_release.rs:627-658`(부분 커밋·경쟁 검증 없음), `docs/evidence/DoD-51_...md:66-70` 및 `docs/plans/2026-08-24_1142_...md:131-148` 과의 정면 모순. 2R: `:33`(조건 4 에 대응하는 진술 없음), `:118`/`:301`(`TerminalTransitionBinding::BoundByCaller` 를 **정직하게 만족시킬 API 경로가 없다** — 메서드가 자기 트랜잭션을 소유), `tests/...:1`(파일 머리가 아직 '검증된 실행 종료 증거' 라고 씀). 3R: `:133-148`·`:474-491`(artifact guard 가 `Completed` 에만 걸리고 outcome 위장이 불가능함을 확인)·`:39-55`·`:308-309`(조건 3 미충족 명시가 정확하고 안전 저하 없음)·`tests:3-16`(서술 정정 확인)·`tests:314-1009`(20건 비공허 확인) — `ACCEPTED`"
review_artifact: "docs/evidence/_raw/DoD-62_review_all_rounds_verbatim.txt"

decision: "release 를 만들되 **오늘 쓸 수 없게** 만든다. `DoD-49` 가 미뤄 둔 이유(실행 종료 증명 부재)가 아직 해소되지 않았기 때문이다. 계획서 4조건 중 서명·identity·(job, attempt, node, fence) 일치만 코드로 확인하고, '실제 workload exit 뒤 생성됐다'(2번)와 '완료 Job 의 artifact durability guard'(4번)는 `RuntimeStopProof`·`ArtifactDurabilityGuard` 진술로 요구한다 — 기본값이 없으므로 호출부가 반드시 쓰고, 오늘 정직한 값은 전부 '아직 증명 못 함' 이다. ★ 조건 3(전이 결합)은 **진술로도 요구하지 않는다** — 이 메서드가 자기 트랜잭션을 소유하므로 호출부가 Attempt/Lease 전이를 끼워 넣을 방법이 없고, 만족시킬 수 없는 것을 요구하면 통과하려는 사람은 거짓말밖에 할 수 없다. 대신 그 사실을 문서에 적는다. artifact guard 는 `Completed` 에만 요구한다 — 실패·취소에까지 요구하면 없는 조건으로 정직한 호출을 막는다. 저장된 증거 행만으로는 풀 수 없게 재검증한 `Verified` 와의 바이트 일치를 요구한다(`attempt_report_store` 가 'raw 는 terminal decision 에 쓰지 말라' 고 적어 둔 계약). 예약이 다른 attempt 의 것이면 **지우지 않고 거부한다** — `runtime-linux` cgroup 회수에서 이미 밟았던 함정이다. Attempt fence 재대조는 따로 하지 않는다 — `fetch_report_binding` 이 이미 하므로 도달 불가능한 죽은 코드가 된다(내 뮤테이션이 잡았다)."
raw_output_artifact: "docs/evidence/_raw/DoD-62_reservation_release_2026-08-30.txt"
raw_output_digest: "sha256:9f2473ac7c630d7330f5d3204679f2c1063cc84c5915723d0381f097eb8d37c9"
raw_output_bytes: 4342

artifacts:
  - "docs/evidence/_raw/DoD-62_reservation_release_2026-08-30.txt"
  - "docs/evidence/_raw/DoD-62_review_all_rounds_verbatim.txt"

binary_digests:
  toolchain: "Windows 개발 기계 cargo 1.97.1"
protocol_versions:
  schema_version: "proto 변경 없음 — 기존 `AttemptReport`(schema_version 1)를 소비한다"
  canonical_spec: "canonical 벡터 50건 불변"
platform: "Windows 11 개발 기계. Linux 회귀는 x600 WSL2 에서 크레이트 단위로 별도 확인"
hardware: "GPU 무관 — SQLite 저장소다"
network_profile: "네트워크를 쓰지 않는다"
command: |
  cargo test -p gputeer-coordinator --test reservation_release
  cargo test -p gputeer-coordinator
  cargo test --workspace
  # 뮤테이션 14건 — scratchpad/mut_release.py (raw 4절에 실제 stdout)
raw_output: |
  === 1. cargo test -p gputeer-coordinator --test reservation_release ===
  test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

  === 2. cargo test -p gputeer-coordinator (전체) ===
  running 130 tests  test result: ok. 130 passed
  running 2 tests    test result: ok. 2 passed
  running 2 tests    test result: ok. 2 passed
  running 16 tests   test result: ok. 16 passed
  running 20 tests   test result: ok. 20 passed
  running 1 tests    test result: ok. 1 passed

  === 3. cargo test --workspace 합계 ===
  771 passed

  === 4. 뮤테이션 14건 — 전부 지정 테스트를 **동작 수준에서** 실패시켰다 ===
  R1  증거 없이 해제 허용              -> without_stored_terminal_evidence_the_reservation_is_not_released
  R2  저장된 행만으로 해제 허용         -> a_report_that_does_not_match_the_stored_evidence_cannot_release
  R3  남의 예약 삭제 허용              -> a_reservation_held_by_another_attempt_is_never_deleted
  R4  비-terminal outcome 허용         -> a_non_terminal_outcome_cannot_release
  R5  증거 조회의 Attempt fence 재대조 제거 -> a_moved_attempt_fence_blocks_the_release
  R6  재시도가 최초 기록을 덮어씀        -> releasing_twice_with_the_same_evidence_is_idempotent
  R7  예약 GPU 자식 행을 안 지움         -> releasing_removes_the_reserved_gpu_rows_too
  R8  해제 GPU 목록을 기록 안 함         -> a_verified_terminal_report_releases_the_reservation
  R9  예약 부재를 조용히 성공 처리        -> a_missing_reservation_without_a_release_record_fails_closed
  R10 실행 종료 관문 제거               -> todays_honest_caller_cannot_release_anything
  R11 키 디렉터리 관문 제거              -> each_missing_declaration_blocks_on_its_own
  R12 artifact durability 관문 제거      -> each_missing_declaration_blocks_on_its_own
  R13 허가를 증거보다 나중에 확인         -> the_authorization_gate_is_checked_before_the_evidence
  R14 완료 아닌 것에도 artifact 요구      -> a_non_completed_outcome_does_not_need_the_artifact_guard

  원복 후 전체 재실행: 통과 / 판정: 전부 비공허

  ★ 위는 발췌·재구성이다. **필터링·수동 결합한 실제 출력 발췌**이며
    `raw_output_artifact` 파일이 원본이다(뮤테이션 절은 스크립트 stdout 그대로).

negative_tests:
  - "★ 오늘의 정직한 호출(세 진술 전부 '아직 증명 못 함')은 `RuntimeStopNotProven` 으로 거부된다"
  - "세 진술을 하나씩 열어도 나머지가 각각 자기 오류로 막는다"
  - "허가 관문이 **증거보다 먼저** 확인된다 — 증거를 만들면 풀린다고 오해하지 않게"
  - "durable terminal 증거가 없으면 `NoTerminalEvidence` 로 거부"
  - "저장된 증거와 재검증 보고서가 다르면(다른 서명자) `EvidenceMismatch` 로 거부"
  - "terminal 이 아닌 outcome 은 `NotTerminalOutcome` 으로 거부"
  - "★ 예약이 다른 attempt 의 것이면 `ReservationBelongsToAnotherAttempt` — **지우지 않는다**"
  - "Attempt 의 fence 가 움직였으면 증거 조회가 `Corrupt { FenceEpochMismatch }` 로 막는다"
  - "예약이 없는데 해제 기록도 없으면 `ReservationNotFound` 로 fail closed — 조용히 성공하지 않는다"
  - "완료 보고서에 `NotApplicableNonCompleted` 를 쓰면 거부 / 완료가 아니면 artifact guard 를 요구하지 않는다"
  - "거부된 해제는 예약·자식 행·해제 기록 어느 것도 남기거나 지우지 않는다"
  - "삭제와 삽입 사이에서 실패하면 전체 rollback — 예약이 살아남는다"
  - "두 연결이 `Barrier` 로 동시에 해제하면 정확히 하나만 `Released`, 하나는 `AlreadyReleased`"
  - "재시도는 멱등 — 늦은 시각이 최초 기록을 덮어쓰지 않는다"

limitations:
  - "★ **이 API 는 오늘 아무 예약도 풀지 못한다.** 실행 종료를 증명하는 producer 가 이 저장소에 없기 때문이고, 그건 미완성이 아니라 이 조각이 하는 일이다"
  - "★ **진술이 참인지 확인하지 못한다.** `RuntimeStopProof::ProvenByCaller`·`KeyDirectoryProvenance::AuthoritativeDirectoryVerifiedByCaller`·`ArtifactDurabilityGuard::SatisfiedByCaller` 는 누구나 쓸 수 있는 값이다. 순수 저장소는 프로세스 종료도 키 디렉터리 권위도 확인할 수 없다(`CLAUDE.md` §0.4). 값어치는 '막는다' 가 아니라 '거짓말 없이는 통과 못 하고 그 거짓말이 호출 지점에 남는다' 다"
  - "★ **계획서 조건 3(Attempt terminal 전이·Lease 종료를 예약 삭제와 같은 durable transaction 에 결합)을 만족하지 못한다.** 이 메서드가 자기 connection 으로 자기 트랜잭션을 열기 때문이다. 진술로도 요구하지 않는다 — 만족시킬 수 없는 것을 요구하면 거짓말밖에 할 수 없다. 만족시키려면 저장소들이 트랜잭션을 공유하는 API 가 먼저 필요하고, 그건 이 조각 밖이다"
  - "★ **terminal `AttemptReport` 는 노드 자기보고다**(`CLAUDE.md` §1 의 `WORKER_REPORTED`). 정상 키를 가진 노드가 '끝났다' 고 서명해 놓고 계속 돌면 위조도 DB 조작도 없이 그 GPU 가 남에게 넘어간다 — 그래서 실행 종료 관문이 있다"
  - "Lease revoke·Job/Attempt 상태 전이·artifact durability 판정을 하지 않는다"
  - "production 소비자가 **없다**. `evaluate_reassignment` 와 마찬가지로, '오늘 아무도 못 푼다' 를 보장하는 것은 이 코드가 아니라 부르는 곳이 없다는 사실이다"
  - "local SQLite 한 파일 안의 원자성만 증명한다 — 다중 Coordinator 합의나 Raft `COMMITTED` 는 범위 밖이다"
  - "★ `raw_output` 절은 **필터링·수동 결합한 실제 출력 발췌**다. 원본은 `raw_output_artifact` 파일이다"
---

# DoD-62 · 예약 해제 경로와 증명 관문

## 왜 지금 만들고, 왜 못 쓰게 만드는가

`DoD-49` 가 예약을 만들면서 해제를 미뤘다.

> reservation release 경로는 여전히 없다 — **실행 종료 증명 없이 구현하면
> 중복 실행 위험이 생긴다**는 설계 판단으로 후순위

예약을 푸는 것은 그 GPU 를 **다른 Job 에게 내주는 일**이다. 원래 노드가
아직 돌고 있는데 풀면 같은 작업이 두 대에서 돈다.

## ★ 내 전제가 틀렸고, 검수가 그걸 반박했다

초안은 "`DoD-51` 의 검증된 terminal `AttemptReport` 가 그 증명이다" 로
시작했다. **틀렸다.** `DoD-51` evidence 가 스스로 정반대를 적어 뒀다.

> 저장 성공은 **프로세스 종료나 artifact/checkpoint durability 를 증명하지
> 않으며 reservation release 의 충분조건이 아니다.**

terminal 보고서는 노드 **자기보고**다. 정상 키를 가진 노드가 "끝났다" 고
서명해 놓고 계속 돌면, 위조도 DB 조작도 없이 GPU 가 넘어간다 — 검수가
내 테스트(`after_release_another_job_can_reserve_the_same_node`)를 그
경로의 증명으로 인용했다.

## 그래서 관문을 뒀다

계획서가 "안전한 release proof 의 최소 형태" 로 정한 네 조건 중 이
모듈은 **1번만** 코드로 확인한다.

```text
1 서명·identity 가 검증된 report 가 예약의 (job, attempt, node, fence) 와 일치
                                                    <- 코드로 확인한다
2 outcome 이 실제 workload exit 뒤 생성됐다          <- 값으로 요구한다
3 전이·Lease 종료가 예약 삭제와 같은 트랜잭션         <- ★ 만족시킬 수 없다
4 완료 Job 이 artifact durability guard 를 만족       <- 값으로 요구한다
```

2·4 는 기본값 없는 값으로 요구하므로 호출부가 반드시 쓴다. 오늘 정직하게
쓸 수 있는 값은 전부 "아직 증명 못 함" 이라 **이 API 는 아무것도 풀지
못한다.**

## ★ 만족시킬 수 없는 것은 요구하지 않는다

초안은 조건 3 도 `TerminalTransitionBinding::BoundByCaller` 로 요구했다.
검수 2라운드가 짚었다 — 이 메서드는 자기 connection 으로 자기 트랜잭션을
열고, 호출부가 거기에 전이를 끼워 넣을 방법이 **없다.**

만족시킬 수 없는 것을 요구하면 통과하려는 사람은 거짓말밖에 할 수 없다.
그래서 그 진술을 **없애고** 사실을 문서에 적었다. 지금 안전한 이유는
조건 3 이 충족돼서가 아니라 조건 2 관문이 모든 호출을 막고 있어서다.

## 내 뮤테이션이 내 테스트 2건과 죽은 코드 1건을 잡았다

```text
공허 1  released_gpu_ids 를 **반환값**으로만 확인 -> 자식 행을 안 써도 통과
        고침: 저장된 것을 다시 읽어 대조

공허 2  옛 fence 테스트가 matches! 로 두 오류를 다 받음 -> 어느 관문이
        막았는지 모름
        고침: 정확한 오류로 고정

죽은 코드  내가 넣은 Attempt fence 재대조가 도달 불가능
           (fetch_report_binding 이 이미 한다) -> 지웠다
```

## 이 실험이 증명하지 않는 것

```text
실행이 실제로 멈췄는지     확인 못 한다. 진술을 요구할 뿐이다
키 디렉터리의 권위        확인 못 한다. `Verified` 는 아무 keyring 으로도
                        만들 수 있다
전이 결합(조건 3)         만족하지 못한다. 트랜잭션을 공유할 API 가 없다
artifact durability      판정하지 않는다. 진술을 요구할 뿐이다
"오늘 아무도 못 푼다"      이건 이 코드가 아니라 **production 호출부가
                        없다는 사실**이 보장한다
다중 Coordinator 합의     local SQLite 한 파일의 원자성만 잰다
```
