---
schema_version: 2
id: DoD-40
claim: "자동 재접속 루프 로드맵 조각 7(원안 '다중 Agent selftest, 같은 프로세스 안 시나리오·fault injection·다중 Agent 경쟁')을 오늘의 설계 조사가 정직하게 재범위했다 — 진짜 '다중 Agent 동시 경쟁'은 지금 Coordinator 아키텍처(의도적 순차 처리, Agent identity/key 1개만 등록, connection_attempt 가 Coordinator 전체 accept 순번)로는 표현 자체가 안 되고, 이를 가능하게 하려면 최소 2~4일짜리 아키텍처 변경이 필요하다. 대신 실제로 검증되지 않았던 위험 — CoordinatorLeaseStore::get_or_issue() 가 BEGIN IMMEDIATE 로 check-then-insert TOCTOU 를 막는다고 코드는 주장하지만 실제 동시 호출로 측정된 적이 없었던 것 — 을 새 통합 테스트로 증명했다. 1차 독립 검수가 테스트의 실제 결함(경쟁 후보가 holder_node_id 외 필드는 전부 같아 부분 덮어쓰기를 못 잡음)을 찾아 필드를 전부 구별되게 만들고 self-check 로 assert 의 판별력을 직접 증명하도록 고쳤다. 프로덕션 코드는 전혀 안 건드렸다(순수 테스트 추가). 로드맵 조각 7 원안 전체(진짜 다중 Agent 병렬 처리·wire-level 경쟁·active-session owner 선정)는 scheduler/다중 Agent 아키텍처 도입 단계로 명시적으로 이월한다 — 이 조각은 '다중 Agent selftest 완료'가 아니라 'Lease store 동시 최초 발급 안전성'이라는 훨씬 좁은 이름으로 기록한다"
status: PASS
commit: e904bbd4e534cced2e5c619e4356c1043e2181a7

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현 2라운드) / claude-code (cargo build·test·신규 테스트 반복(10+80회 등)·coordinator-agent-selftest 재실행 — 코덱스 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T18:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스, 2라운드"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(CHANGES_REQUESTED) — 경쟁 후보가 holder_node_id 외 모든 필드(fence_epoch·시각 필드·coordinator_term·max_total_duration_seconds)가 같아 최종 assert_eq!(stored, winner) 가 패자 값의 부분 덮어쓰기를 실제로 구별해내지 못하는 진짜 결함 발견(lease_store_concurrent_issue.rs:19,211). Barrier 유효성(실제 경쟁 조건 생성, 어느 후보가 먼저 lock 을 얻을지 미리 정하지 않음)·뮤테이션 실질성(TransactionBehavior::Immediate 가 실제 BEGIN IMMEDIATE 로 변환됨을 rusqlite 0.32.1 기준 확인)·프로덕션 무변경(git diff --stat HEAD 비어있음, lease_store.rs/coordinator/lib.rs working-tree hash 가 HEAD 와 일치)·문서가 로드맵 조각 7 완료를 과대 주장하지 않고 scheduler 단계 이월을 명확히 밝힘은 전부 확인. 2라운드(ACCEPTED) — 수정된 후보 A/B 가 lease_id·job_id·attempt_id 는 동일(identity 비교 순서 job_id→attempt_id→holder_node_id→issuing_coordinator_id 를 lease_store.rs:538 로 확인, holder_node_id 충돌이 의도대로 먼저 발생)하되 요청된 8개 필드는 전부 서로 다름을 확인, self-check(승자 필드를 하나씩 패자 값으로 바꿔 8번 모두 assert_eq!(stored, winner) 에서 panic 하는지 실제 실행해 확인)가 #[test] 실행 경로에 포함되고 dead code 가 아님을 확인, 최종 raw SQLite 검증의 assert_eq!+8개 assert_ne! 가 전부 순차 실행됨을 확인, 추적 파일 diff 0건(lease_store.rs 객체 해시가 HEAD 와 df9c9716... 로 동일)까지 확인 후 최종 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-40_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-40_lease_store_동시_발급_안전성_2026-08-20.txt"
raw_output_digest: "sha256:936e3ae534f9bd0e038e8c2ddf4f45964550e1e24023b70013930a367cbc7d6d"
raw_output_bytes: 6510

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — 순수 테스트 추가만"
  canonical_spec: "docs/protocol/signing.md v1(변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 파일 기반 SQLite 동시성 테스트"
network_profile: "네트워크 미사용(같은 프로세스 내 스레드 두 개가 별도 SQLite 연결로 경쟁)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  cargo test -p gputeer-coordinator --test lease_store_concurrent_issue   # 여러 차례 반복
  .\target\debug\gputeer.exe coordinator-agent-selftest
raw_output: |
  (docs/evidence/_raw/DoD-40_lease_store_동시_발급_안전성_2026-08-20.txt,
   docs/evidence/_raw/DoD-40_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공(2라운드 각각), 실패 0건
  신규 테스트(수정 후): 5회 연속 test result: ok, 각 1.5~1.7초, flake 없음
  coordinator-agent-selftest(수정 후): exit=0, 72개 시나리오, 회귀 없음
artifacts:
  - docs/plans/2026-08-20_1732_lease_store_동시_발급_안전성_v1.md
  - docs/reports/2026-08-20_1732_lease_store_동시_발급_안전성.md
  - crates/coordinator/tests/lease_store_concurrent_issue.rs
  - docs/evidence/_raw/DoD-40_lease_store_동시_발급_안전성_2026-08-20.txt
  - docs/evidence/_raw/DoD-40_review.txt
negative_tests:
  - "lease_store_concurrent_issue.rs — 16개 서로 다른 lease_id 라운드, 각 라운드마다 identity(job_id·attempt_id) 는 동일하되 holder_node_id·fence_epoch·issuing_coordinator_id·coordinator_term·issued/renew/expires 시각·max_total_duration_seconds 전부 다른 두 후보를 Barrier 로 동시 출발, 정확히 하나만 Ok 이고 저장된 행이 승자와 완전 일치·패자와는 8개 필드 전부 불일치함을 확인"
  - "self-check — 승자 복제본의 각 필드를 하나씩 패자 값으로 바꿔가며 최종 assert 헬퍼가 8번 모두 실제 panic 하는지 확인(테스트 자체의 판별력 증명, 자기 검증)"
  - "뮤테이션 — get_or_issue() 의 TransactionBehavior::Immediate 를 Deferred 로 임시 변경하면 '정확히 한 최초 발급 성공' 불변식이 깨짐(첫 라운드 양쪽 모두 LockTimeout) 확인, 원복 후 재검증"
limitations:
  - "이 테스트는 **같은 프로세스 안 스레드 두 개**가 별도 SQLite 연결로 경쟁하는 storage-level 안전성만 증명한다 — 별도 OS 프로세스 두 쌍(Coordinator/Agent)이 같은 DB 파일을 공유하는 실제 wire-level 경쟁은 검증하지 않는다"
  - "Coordinator 의 진짜 다중 Agent 병렬 처리(현재는 의도적으로 순차 처리)·서로 다른 Agent identity/key 등록·Agent 별 connection_attempt·동일 identity 복제 Agent 의 active-session owner 선정·다중 Coordinator HA 는 전부 범위 밖 — 설계 조사가 이걸 가능하게 하려면 최소 2~4일 규모의 아키텍처 변경(production 500~900줄+테스트 350~600줄)이 필요하다고 판단했다"
  - "**로드맵 조각 7 원안('다중 Agent selftest')은 완료된 것이 아니라 scheduler/다중 Agent 아키텍처 도입 단계로 명시적으로 이월된다** — 이 문서는 그 훨씬 좁은 하위 조각(storage-level 동시 최초 발급 안전성)만 완료를 주장한다"
  - "Linux 환경에서의 동일 검증은 없음(Windows 개발 기계 한정)"
decision: "로드맵 조각 7 원안('다중 Agent selftest')을 오늘 안전하게 끝낼 수 있는 정직한 하위 조각인 'Lease store 동시 최초 발급 안전성'으로 재범위해 완료했다. 구현을 코덱스 CLI(workspace-write)에 위임했고, 독립 검수 1라운드가 테스트 자체의 판별력 결함(부분 덮어쓰기를 못 잡음)을 찾아 CHANGES_REQUESTED, 경쟁 후보 필드를 전부 구별되게 만들고 self-check 로 판별력을 직접 증명한 2라운드에서 ACCEPTED. 진짜 다중 Agent 병렬 처리는 scheduler/다중 Agent 아키텍처 도입 시점까지 명시적으로 미룬다. **이로써 2026-08-20 자동 재접속 루프 로드맵(7조각) 작업을 마무리한다** — 조각 1~6 은 원안 그대로 완료(DoD-35~39), 조각 7 은 정직하게 재범위된 하위 조각만 완료(DoD-40), 원안의 다중 Agent 아키텍처 부분은 후속 로드맵(scheduler 단계)으로 이월."
---

# DoD-40 · Lease store 동시 최초 발급 안전성 (로드맵 조각 7 재정의)

## 무엇을 입증하려 했는가

로드맵 조각 7 원안("다중 Agent selftest")의 설계 조사(`p206`,
read-only)가 정직한 결론을 냈다 — 진짜 "다중 Agent 동시 경쟁"은
지금 Coordinator 아키텍처로는 표현 자체가 안 된다. `run()` 은
의도적으로 순차 처리이고, Coordinator 설정에는 Agent identity/key
가 하나뿐이라 두 번째 독립 Agent 는 그 계약에서 먼저 막힌다.

대신 조사가 찾은 진짜 검증 안 된 위험은 —
`CoordinatorLeaseStore::get_or_issue()` 가 `BEGIN IMMEDIATE` 로
TOCTOU 를 막는다고 코드는 주장하는데, **실제 동시 호출로 측정된
적이 한 번도 없었다**는 것이었다.

## 구현 1라운드 (`p207`, 코덱스 workspace-write)

새 통합 테스트 `lease_store_concurrent_issue.rs` 신설 —
`durable_replay_race.rs` 의 "스레드마다 별도 SQLite 연결 +
`Barrier`" 패턴을 그대로 따라, 16개 라운드에서 같은 `lease_id`
를 다른 `holder_node_id` 로 동시에 최초 발급 시도해 정확히
하나만 성공함을 확인했다. 프로덕션 코드는 전혀 안 건드렸다.

## 독립 검수 1라운드 (`p208`) — **CHANGES_REQUESTED**

진짜 결함을 찾았다 — 두 경쟁 후보가 `holder_node_id` 외 다른
모든 필드가 같아서, 최종 `assert_eq!` 가 패자 값의 부분 덮어쓰기
를 실제로 구별해내지 못했다. "부분 덮어쓰기 없음" 이라는 핵심
주장이 증명되지 않은 상태였다.

## 구현 2라운드 (`p209`, 코덱스 workspace-write) — 근본 수정

경쟁 후보 A/B 를 identity 필드(`lease_id`·`job_id`·`attempt_id`)
는 동일하게 유지하되, 나머지 8개 필드(`holder_node_id`·
`fence_epoch`·`issuing_coordinator_id`·`coordinator_term`·시각
3종·`max_total_duration_seconds`) 는 전부 서로 다른 값으로
구별했다. **self-check** 를 추가해 — 승자 필드를 하나씩 패자
값으로 바꿔가며 최종 assert 헬퍼가 8번 모두 실제로 panic 하는지
직접 실행해 증명했다.

## 독립 검수 2라운드 (`p210`) — **ACCEPTED**

identity 비교 순서가 실제 코드와 일치하는지, self-check 가
dead code 없이 실행 경로에 포함돼 있는지, 프로덕션 코드가 여전히
안 바뀌었는지(객체 해시 동일)까지 전부 확인하고 잔여 지적 없이
`ACCEPTED`.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   성공(2라운드 각각), 실패 0건
신규 테스트(수정 후)                                                5회 연속 test result: ok, flake 없음
coordinator-agent-selftest(수정 후)                                exit=0, 72개 시나리오, 회귀 없음
```

## 이 실험이 증명하지 "않는" 것

- 별도 OS 프로세스 두 쌍이 같은 DB 를 공유하는 실제 wire-level
  경쟁은 검증하지 않는다.
- Coordinator 의 진짜 다중 Agent 병렬 처리·active-session owner
  선정·다중 Coordinator HA 는 전부 범위 밖(최소 2~4일 아키텍처
  변경 필요).
- **로드맵 조각 7 원안은 완료가 아니라 scheduler 단계로 이월**
  됐다.

## 결정

1. 로드맵 조각 7 을 "Lease store 동시 최초 발급 안전성"이라는
   훨씬 좁은 하위 조각으로 재범위해 완료했다.
2. 독립 검수 1라운드가 테스트 판별력 결함을 찾아
   `CHANGES_REQUESTED`, self-check 로 판별력을 직접 증명한
   2라운드에서 `ACCEPTED`.
3. **이로써 2026-08-20 자동 재접속 루프 로드맵(7조각) 작업을
   마무리한다** — 조각 1~6 원안 완료, 조각 7 은 재범위된 하위
   조각만 완료, 원안의 다중 Agent 아키텍처는 scheduler 단계로
   이월.

관련: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`
(전체 로드맵) · `docs/evidence/DoD-24_lease_재접속_최소_조각.md`
(`IdentityConflict` 최초 도입) · `crates/crypto/tests/durable_replay_race.rs`
(동시성 테스트 패턴의 선례)
