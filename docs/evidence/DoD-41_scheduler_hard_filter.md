---
schema_version: 2
id: DoD-41
claim: "scheduler 로드맵 조각 1(순수 hard-filter kernel)을 완료했다. 9단계 로드맵 중 첫 조각이며, Coordinator 연결·실제 자동 매칭은 하지 않는다. 1차 독립 검수가 실제 보안 결함 2건 (isolation_class 축 오류·빈 identity MissingFact 우회)을 찾아 2라운드에서 근본 수정, 2차 검수 ACCEPTED — 고정 PoolSnapshot·JobRequirements·Policy만 받는 evaluate_eligibility()가 Node/Risk/freshness·보안 축·GPU/CPU/RAM/workspace·owner 정책을 fail-closed로 판정하고, 복수 적격이면 winner를 고르지 않고 RankingRequired를 반환함을 33개 테스트와 두 회귀 뮤테이션으로 확인했다"
status: PASS
commit: 6ec46a1c6d1a3cd6309787262ef1e6a8c57505bd

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — 2라운드"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-21T10:27:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 2라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(CHANGES_REQUESTED) — 제3자 opt-in·pure·sensitivity 제한을 security_tier==S1에 결합해 S2+Restricted 후보가 보안 gate를 우회하는 isolation_class 축 오류와, owner/submitter identity의 Some(\"\")가 MissingFact를 우회하는 결함을 발견했다. 현재 수정은 Restricted 분기(filter.rs:274)와 빈 identity 처리(filter.rs:253,260)에 있고, S2+Restricted 반례 3건(hard_filter.rs:370,381,394)·빈 identity 반례 2건(hard_filter.rs:281,292)이 회귀를 고정한다. stream_ownership.rs는 신규 크레이트 등록에 필요한 1줄(stream_ownership.rs:177)만 남기도록 요청했다. 2라운드(ACCEPTED) — S0 독립 gate와 Restricted 축 분리·빈 문자열 fail-closed·두 뮤테이션의 비공허성·ownership guard가 crates 전체를 실제 열거한다는 필수성(stream_ownership.rs:180,193)·범위가 scheduler+workspace 등록+문서+ownership guard 1줄뿐임을 확인했다. evaluate_eligibility()가 입력 snapshot만 사용하고 복수 적격을 RankingRequired로 남기는 순수 kernel임(filter.rs:7,29)까지 확인 후 최종 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-41_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-41_scheduler_hard_filter_2026-08-21.txt"
raw_output_digest: "sha256:66eea9e13159e55c3a7ffa90dc823f13e26203398c4654225f72f6a9d0f08eec"
raw_output_bytes: 3956

binary_digests:
  toolchain: "제공된 이력 요약에 toolchain version·binary digest 없음 — supervisor cargo test 결과만 기록"
protocol_versions:
  schema_version: "proto 변경 없음 — scheduler 내부 domain model 신설"
  canonical_spec: "docs/protocol/signing.md 변경 없음 — 서명 메시지·canonical 경로 범위 밖"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test"
hardware: "GPU 미사용 — 합성 snapshot을 평가하는 순수 함수 테스트"
network_profile: "네트워크 미사용 — Coordinator·Agent 연결 없는 in-process 단위 테스트"
command: |
  cargo test -p gputeer-scheduler
raw_output: |
  (docs/evidence/_raw/DoD-41_scheduler_hard_filter_2026-08-21.txt,
   docs/evidence/_raw/DoD-41_review.txt 전문 참조)

  감독자 직접 확인: PASS — 33 passed / 0 failed
  독립 검수 1라운드: CHANGES_REQUESTED — 보안 결함 2건과 범위 위반 지적
  독립 검수 2라운드: ACCEPTED — 잔여 수정 요청 없음
artifacts:
  - docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md
  - docs/plans/2026-08-21_1002_scheduler_hard_filter_v1.md
  - docs/reports/2026-08-21_1002_scheduler_hard_filter_v1.md
  - docs/contracts/proposals/2026-08-21_0955_scheduler_workspace_등록.md
  - docs/contracts/01_스트림_소유권.md
  - Cargo.toml
  - Cargo.lock
  - crates/scheduler/Cargo.toml
  - crates/scheduler/src/lib.rs
  - crates/scheduler/src/model.rs
  - crates/scheduler/src/filter.rs
  - crates/scheduler/tests/hard_filter.rs
  - crates/protocol/tests/stream_ownership.rs
  - docs/evidence/_raw/DoD-41_scheduler_hard_filter_2026-08-21.txt
  - docs/evidence/_raw/DoD-41_review.txt
negative_tests:
  - "Node/Risk/freshness·tier/isolation/key protection·GPU count/health/VRAM/model·CPU/RAM/workspace·workload allowlist의 정상 바로 밖 경계와 필요한 사실 누락을 fail-closed로 거부"
  - "S0 타인 Job 무조건 거부와 Restricted 타인 Job의 opt-in 없음·non-pure·SENSITIVE data 거부를 확인하고, S2+Restricted 반례 3건으로 판단 축이 security_tier==S1로 퇴행하지 않음을 고정"
  - "빈 owner/submitter identity Some(\"\") 각각이 MissingFact로 거부되는 회귀 테스트 2건"
  - "0/1/복수 적격, 후보 역순, 동일 node_id 중복 입력에서도 결정적 report와 RankingRequired 동작 확인"
  - "뮤테이션 2건 — Restricted 축을 S1로 되돌리면 회귀 테스트 3건 실패, 빈 identity 검사를 제거하면 회귀 테스트 2건 실패; 둘 다 원복 후 scheduler 33개 테스트 통과"
limitations:
  - "합성 PoolSnapshot에 대한 순수 hard gate만 증명한다 — live telemetry 생산·정확성·freshness 수집 경로는 검증하지 않는다"
  - "Coordinator와 연결하지 않았고 실제 자동 매칭·winner 선택·best-fit·reservation·Lease/Grant dispatch를 수행하지 않는다"
  - "durable Job/Attempt/Queue, 다중 Agent inventory, Agent entrypoint 실행, 실패 감지·복구, fairness·chance constraint, quarantine·E2E는 범위 밖이다"
  - "CUDA/architecture compatibility, availability window, T_est/deadline, durability/failure-domain, checkpoint budget, 실제 GPU 실측은 검증하지 않는다"
  - "감독자 직접 확인으로 기록된 실행은 cargo test -p gputeer-scheduler 33/33뿐이며, 하드웨어 성능이나 Linux 동작을 주장하지 않는다"
decision: "scheduler의 첫 구현 조각을 외부 상태를 읽지 않는 순수 hard-filter kernel로 제한해 완료했다. 1차 독립 검수가 SecurityTier와 IsolationClass를 혼동한 제3자 정책 우회와 빈 identity의 MissingFact 우회라는 실제 보안 결함 2건을 찾아 CHANGES_REQUESTED했고, 구현 2라운드가 판단 축을 Restricted isolation으로 고치고 빈 문자열을 fail-closed로 처리했다. S2+Restricted 3건과 빈 identity 2건의 회귀 테스트 및 두 뮤테이션으로 수정의 판별력을 확인했으며, 다른 스트림 파일은 신규 크레이트 ownership 등록 필수 1줄만 남겼다. 독립 검수 2라운드는 판단·테스트·변경 범위를 모두 확인해 ACCEPTED했고 감독자는 scheduler 테스트 33/33을 직접 확인했다. scheduler 로드맵 9단계 중 조각 1 완료 — 남은 8단계(durable Job/Attempt/Queue·다중 Agent inventory·v0.1 best-fit·실제 Grant dispatch·Agent entrypoint 실행·실패 감지/복구·chance-constrained/fairness·quarantine/E2E)는 후속 조각"
---

# DoD-41 · scheduler 순수 hard-filter kernel (로드맵 조각 1)

## 무엇을 입증하려 했는가

외부 상태를 읽지 않는 `evaluate_eligibility()`가 고정
`PoolSnapshot`·`JobRequirements`·`Policy`만으로 후보별 hard gate를
fail-closed 판정하고, 모든 확인된 탈락 사유와 결정적인 적격 목록을
반환하는지 검증했다. 적격 후보가 여러 개면 이 조각이 winner를
선택하지 않고 `RankingRequired`로 다음 단계에 넘기는 것까지가 범위다.

## 구현 1라운드 — 28개 테스트

`crates/scheduler`를 신설해 Node/Risk/freshness, 서로 독립적인
tier/isolation/key protection, GPU·CPU·RAM·workspace, owner workload
정책을 평가하는 순수 kernel과 28개 정상·경계·negative·결정성
테스트를 만들었다.

## 독립 검수 1라운드 — **CHANGES_REQUESTED**

실제 보안 결함 2건을 찾았다. 제3자 opt-in·pure·sensitivity 제한이
`isolation_class == Restricted`가 아니라 `security_tier == S1`에
결합돼 S2+Restricted 후보가 gate를 우회했고, 빈 owner/submitter
identity인 `Some("")`가 `MissingFact`를 우회했다. 또한 다른 스트림의
ownership guard는 신규 크레이트 등록 1줄 외 변경을 원복하라고
요청했다.

## 구현 2라운드 — 근본 수정과 33개 테스트

제3자 정책의 판단 축을 `IsolationClass::Restricted`로 바로잡고,
owner/submitter의 `None`과 빈 문자열을 모두 `MissingFact`로
fail-closed 처리했다. S2+Restricted 반례 3건과 빈 identity 반례
2건을 추가해 총 33개 테스트가 됐다. 판단 축을 다시 S1로 돌리는
뮤테이션과 빈 문자열 검사를 제거하는 뮤테이션은 각각 새 회귀
테스트를 실패시켰고, 원복 뒤 전체 scheduler 테스트가 통과했다.

## 독립 검수 2라운드 — **ACCEPTED**

Restricted 판단 축, 빈 identity 처리, ownership guard 등록 1줄의
필수성, 두 뮤테이션의 비공허성, 변경 범위가 scheduler·workspace
등록·문서·ownership guard 1줄뿐임을 확인하고 잔여 지적 없이
`ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-scheduler    PASS — 33 passed / 0 failed
```

위 결과는 감독자가 직접 확인했다.

## 이 실험이 증명하지 "않는" 것

- live PoolSnapshot 수집과 telemetry 정확성은 증명하지 않는다.
- 실제 winner 선택·자동 매칭·reservation·Lease/Grant dispatch는 없다.
- Coordinator·Agent 연결과 실제 entrypoint 실행은 없다.
- scheduler 로드맵의 남은 8단계는 완료되지 않았다.

## 결정

1. scheduler 9단계 로드맵 중 조각 1인 순수 hard-filter kernel을
   완료했다.
2. 1차 독립 검수가 실제 보안 결함 2건을 찾아 `CHANGES_REQUESTED`,
   근본 수정과 회귀 뮤테이션 뒤 2차 검수에서 `ACCEPTED`했다.
3. 남은 8단계는 후속 조각으로 명시적으로 남긴다.

관련: `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md` ·
`docs/plans/2026-08-21_1002_scheduler_hard_filter_v1.md` ·
`docs/reports/2026-08-21_1002_scheduler_hard_filter_v1.md`
