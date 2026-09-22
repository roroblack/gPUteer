---
schema_version: 2
id: DoD-32
claim: "crates/coordinator/src/lease_store.rs 의 renew_existing_within_duration() 이 revoke 여부·max_total_duration_seconds 만 검사하고 저장된 expires_at_unix_ms 가 이미 지났는지는 검사하지 않은 채 즉시 새 만료시각으로 UPDATE 하던 안전 공백을 닫았다 — revoke 검사 뒤·max-duration 검사 전에 expires_at_unix_ms <= now_unix_ms(DoD-26 과 동일한 경계 포함 규칙) 를 추가하고, 기존 LeaseStoreError::Expired(DoD-26) 를 재사용해 트랜잭션을 commit 한 뒤 UPDATE 없이 반환한다. crates/coordinator/src/lib.rs 의 build_renew_result() override 읽기 경로·일반 갱신 경로 양쪽 다 이 검사를 적용하고, 만료 시 signed outcome 없이 raw error 로 연결을 끊는다(proto 변경 없음). 기존 정상 갱신·MaxDurationExceeded·경계 단위 테스트 fixture 의 만료시각을 미래로 보정하면서도 각 테스트의 원래 판정 조건은 유지했고, 새 경계 단위 테스트 2건(expires_at==now → Expired·DB 레코드 완전 불변, expires_at==now+1 → 정상 갱신·UPDATE)과 신규 selftest 시나리오 47(짧은 TTL 로 실제 만료시킨 뒤 갱신 시도가 raw error 로 거부됨)을 추가했다"
status: PASS
commit: e2f2016

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현) / claude-code (cargo build·test·coordinator-agent-selftest 5회 연속 독립 재실행 — 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T06:47:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "새 만료 검사가 정확히 <= 인지·revoke 검사 뒤 max-duration 검사 전 순서인지(lease_store.rs:394-400), Expired 반환 시 트랜잭션 commit 후 UPDATE 가 실행되지 않는지(:409-422), 새 경계 단위 테스트 2건이 DB 레코드 불변·실제 갱신을 각각 검증하는지(:937-974), lib.rs 의 override·일반 갱신 경로 양쪽 다 서명·전송 경로(:731 이후) 도달 전에 raw error 로 탈출하는지(:664-706), 기존 fixture 보정이 원래 판정 조건을 유지했는지, selftest 시나리오 47(coordinator_agent_selftest.rs:2632-2676)이 --lease-ttl-ms 500·--renew-delay-ms 1000 으로 실제 만료 뒤 renew 요청이 도달하도록 하고 coordinator stderr 의 expired·실패 exit·RENEW_RESULT 부재를 확인하는지, renew_delay_ms 가 이 목적으로만 쓰이는지, renew_existing()(별도 미사용 함수)·crates/agent/src/lib.rs·proto 는 전혀 안 바뀌었는지(git diff --stat 정확히 3개 파일). 1라운드(p173) 만에 ACCEPTED — 수정 요청 없음. 감독자(claude-code)가 검수 완료 후(동시 파일 조작 방지) cargo build/test·coordinator-agent-selftest 5회 연속으로 독립 재확인(전부 exit=0, 47개 시나리오, 약 14초/회, coordinator 유닛 테스트 27→29건)"
review_artifact: "docs/evidence/_raw/DoD-32_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-32_만료_lease_갱신_fail_closed_2026-08-20.txt"
raw_output_digest: "sha256:4a863ae35b27f27e4c9b3d89505445fbdf8584e7842ef2772dbae584f64a406d"
raw_output_bytes: 2812

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — 이번 조각은 Coordinator 저장소·응답 조립 로직 레벨의 fail-closed 정책이다(raw error, signed outcome 없음)"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(coordinator-agent-selftest 기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 코덱스 구현 시 5회 + 감독자 재검증 5회, 각 90초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-32_만료_lease_갱신_fail_closed_2026-08-20.txt,
   docs/evidence/_raw/DoD-32_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공, 실패 0건
    (coordinator 유닛 테스트 27→29건)
  coordinator-agent-selftest(코덱스 구현 시 5회): 5회 연속 exit=0, 47개 시나리오
  coordinator-agent-selftest(감독자 재검증 5회): 5회 연속 exit=0, 매회 47개 시나리오, 약 14초/회
artifacts:
  - crates/coordinator/src/lease_store.rs
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-32_만료_lease_갱신_fail_closed_2026-08-20.txt
  - docs/evidence/_raw/DoD-32_review.txt
negative_tests:
  - "단위 테스트 — expires_at_unix_ms == now_unix_ms 는 Expired 를 반환하고 저장 레코드가 완전히 불변임을 확인(DB 상태까지)"
  - "단위 테스트 — expires_at_unix_ms == now_unix_ms + 1 은 다른 정책에 안 걸리면 정상 갱신·UPDATE 됨을 확인"
  - "selftest 시나리오 47 — 짧은 TTL(--lease-ttl-ms 500)로 Lease 를 실제 만료시킨 뒤 지연된 갱신 요청(--renew-delay-ms 1000)이 raw error(coordinator stderr 에 expired 포함)로 거부되고 RENEW_RESULT 가 없음을 확인"
  - "뮤테이션(코덱스 자체 보고, p172) — 만료 검사를 제거하면 새 경계 단위 테스트와 selftest 시나리오 47 모두 실패(만료된 Lease 가 RENEWED 로 통과)로 바뀜을 확인, 원복 후 재검증 통과"
limitations:
  - "이번 조각은 raw error 거부만 한다 — 새 signed RENEW_OUTCOME_EXPIRED 는 만들지 않았다(DoD-27 이 REVOKED 만 다뤘던 것과 같은 범위 판단). Agent 는 이 거부를 다른 handshake 실패와 구분할 신호가 아직 없다"
  - "renew_existing()(326행 부근, 현재 실제 Coordinator 운영 경로에서 호출되지 않는 별도 함수)은 손대지 않았다 — 향후 이 함수가 실제로 쓰이게 되면 같은 검사를 추가해야 한다"
  - "Agent 쪽(crates/agent/src/lib.rs)은 여전히 갱신 루프 진입 전 보유 Lease 의 만료를 스스로 재확인하지 않는다(설계 조사 p171 이 확인) — 이번 조각은 Coordinator 쪽 방어만 닫았다"
  - "재접속(get_or_issue()) 경로의 만료 거부(DoD-26)와 이번 갱신 경로의 만료 거부는 별도 코드 경로다 — 공통 헬퍼로 통합하지 않았다"
decision: "갱신 경로로 만료된 Lease 가 되살아날 수 있던 안전 공백을 닫았다 — DoD-26 이 재접속 경로에 이미 적용한 것과 동일한 <= 경계 규칙을 갱신 경로에도 적용했다. 구현을 코덱스 CLI(workspace-write)에 위임했고, 독립 검수(대화 기록 없는 새 인스턴스, read-only)가 경계 규칙·검사 순서·fixture 보정의 타당성·범위 제한까지 전부 코드로 확인하고 1라운드 만에 ACCEPTED. 감독자가 검수 완료 후(DoD-31 의 동시 조작 오탐 교훈을 반영해 순차로) 직접 재현해 재확인했다."
---

# DoD-32 · 만료된 Lease 갱신 경로 fail-closed

## 무엇을 입증하려 했는가

백로그 재조사(`p167`) 2순위 — 설계 조사(`p171`, read-only)가
실제 안전 공백을 확인했다: `renew_existing_within_duration()`
(`crates/coordinator/src/lease_store.rs`)이 revoke 여부·
`max_total_duration_seconds` 만 검사하고 저장된
`expires_at_unix_ms` 가 이미 지났는지는 검사하지 않은 채 즉시 새
만료시각으로 `UPDATE` 했다. 반면 `get_or_issue()`(최초 발급/재접속
경로, `DoD-26`)는 이미 이 검사를 한다. 즉 **정상 Agent 도**
checkpoint/ACK 처리 지연이나 시계 어긋남으로 이미 만료된 Lease 를
갱신 경로로 계속 살릴 수 있는 상태였다 — 악의적 Agent 만의
문제가 아니었다.

## 구현 (코덱스, `p172`)

- `renew_existing_within_duration()` 에 revoke 검사 뒤·max-duration
  검사 전 `expires_at_unix_ms <= now_unix_ms` 검사 추가, 기존
  `LeaseStoreError::Expired`(`DoD-26`) 재사용, 트랜잭션 commit 후
  `UPDATE` 없이 반환.
- `build_renew_result()` 의 override 읽기 경로·일반 갱신 경로
  양쪽 다 이 검사 적용, raw error 로 연결 종료(signed outcome
  새로 안 만듦).
- 기존 정상 갱신·`MaxDurationExceeded`·경계 단위 테스트 fixture
  의 만료시각을 미래로 보정(원래 판정 조건은 유지).
- 새 경계 단위 테스트 2건, 신규 selftest 시나리오 47.

## 독립 검수(`p173`) — **1라운드 만에 ACCEPTED**

경계 규칙(`<=`)·검사 순서(revoke 우선)·`Expired` 반환 시
`UPDATE` 미실행·새 단위 테스트의 DB 상태 검증·override/일반 경로
양쪽의 raw error 탈출·기존 fixture 보정의 타당성·시나리오 47 의
실질 검증(stderr 내용까지)·범위 제한(`renew_existing()`·Agent·
proto 무변경)까지 전부 코드로 확인했다. `cargo test` 는 환경에
`cargo` 가 없어 실행 못 했으나 판정 근거로 쓰지 않았다.

감독자(claude-code)가 검수 완료 **후**(`DoD-31` 에서 검수 진행
중 감독자의 동시 파일 조작이 오탐을 냈던 교훈을 반영해 순차로
진행) `cargo build`/`test`·`coordinator-agent-selftest` 5회
연속으로 독립 재확인 — 전부 exit=0, 47개 시나리오, 약 14초/회,
coordinator 유닛 테스트 27→29건.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   성공, 실패 0건(유닛 테스트 27→29건)
coordinator-agent-selftest(코덱스 구현 시 5회)                     5회 연속 exit=0, 47개 시나리오
coordinator-agent-selftest(감독자 재검증 5회)                       5회 연속 exit=0, 매회 약 14초
```

## 이 실험이 증명하지 "않는" 것

- 새 signed `RENEW_OUTCOME_EXPIRED` 는 없다 — raw error 거부만
  한다.
- `renew_existing()`(미사용 별도 함수)은 안 건드렸다.
- Agent 쪽이 스스로 보유 Lease 의 만료를 갱신 요청 전에 재확인
  하지는 않는다 — Coordinator 쪽 방어만 닫았다.

## 결정

1. 갱신 경로로 만료된 Lease 가 되살아날 수 있던 공백을 닫았다 —
   `DoD-26` 과 동일한 `<=` 경계 규칙을 재사용했다.
2. 독립 검수 1라운드 만에 `ACCEPTED`, 감독자가 순차로 재확인했다.

관련: `docs/evidence/DoD-26_만료_lease_재접속_거부.md`(같은 경계
규칙의 최초 적용) · `docs/evidence/DoD-27_revoked_signed_outcome.md`
(같은 방식으로 signed outcome 을 다음 조각으로 미룬 선례)
