---
schema_version: 2
id: DoD-39
claim: "자동 재접속 루프 로드맵 조각 5(원래 'durable request ledger')를 오늘의 설계 조사가 정직하게 재범위했다 — 진짜 공백은 '정확히 한 번 처리'가 아니라 가용성 공백이었다: Agent 가 RenewLeaseRequest 전송 뒤 결과를 받기 전에 연결이 끊기면 SessionError::AmbiguousRenew 로 분류돼 지금까지는 즉시 fatal 종료했다. 이제 명시적 게이트(--recover-ambiguous-renew-from-durable-lease)가 켜졌을 때만 bounded reconnect 후 기존 Grant/ACK 경로(get_or_issue())로 최신 저장 Lease 를 재조회하고 새 nonce 로 새 Renew 를 보낸다. 1차 독립 검수가 legacy Coordinator + 이 게이트 오조합 시 자동 복구가 실제로는 '재조회'가 아니라 '그 순간 새로 조작된 Lease 발급'이 되는 진짜 안전 결함을 찾았다 — proto/job.proto 의 ExecutionGrant 를 schema v2 로 승격해 서명 대상 필드 lease_from_durable_store 를 순수 추가하고, Coordinator 는 lease_store.is_some() 일 때만 true 로 서명하며, Agent 는 이 서명된 비트가 true 가 아니면 ACK·checkpoint·새 Renew 전에 fatal 거부하도록 고쳐 닫았다. lease_store.rs·SQLite 스키마·lease_requests 테이블 같은 범용 request ledger 는 만들지 않았다 — 로드맵 원안보다 훨씬 작은 범위다"
status: PASS
commit: 3a76a107147e985836b537479cc5da1093b2d313

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현 2라운드) / claude-code (cargo build·test·canonical self-test/verify·coordinator-agent-selftest 5회+5회+10회 독립 재실행 — 코덱스 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T16:40:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스, 2라운드"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(CHANGES_REQUESTED) — durable 복구 게이트가 Coordinator 의 실제 durable 상태와 검증 가능하게 결합되지 않아 legacy Coordinator + recovery=true 오조합 시 재접속이 저장된 상태 재조회가 아니라 그 순간 새로 조작된 Lease 발급이 되는 진짜 안전 결함 발견(agent/lib.rs:301,1213 — 로컬 boolean만 검사; coordinator/lib.rs:1408,1417 — None => StoredLease 경로로 현재 시각 기준 재구성). 그 외 교착 안전성(재접속이 동일 run_one_connection() 재사용, ReconnectExhausted 로 상한 확인agent/lib.rs:262,277,308)·outcome 2|3|6|8 즉시 종료 유지(agent/lib.rs:823, coordinator/lib.rs:792)·durable commit 순서(lease_store.rs:514, coordinator/lib.rs:746)·flush() 통일 정확성·범위(lease_store.rs·proto·SQLite 스키마 무변경)는 전부 통과 확인. 2라운드(ACCEPTED) — 수정된 proto v2 필드 lease_from_durable_store 가 실제로 canonical 서명 입력에 포함되는지(job.proto:133, to_fields.rs:732, t1b_grant_and_control.rs:199 새 교차검증 테스트), Coordinator 가 lease_store.is_some() 판정을 SQLite transaction commit 성공 후에만 정직하게 채우는지(coordinator/lib.rs:288,1354, lease_store.rs:287 — 'store는 Some 이지만 특정 Lease 는 미커밋'인 경로 없음), Agent 의 거부가 ACK·checkpoint·Renew 전송 전(agent/lib.rs:530, checkpoint 550행, ACK 572·595행, Renew 651행 이후)이고 Fatal 분류로 재시도 안 되는지, durable/legacy 정상 경로(시나리오 65~69·71)에 회귀가 없는지, v2 승격이 배포 전 변경(원격 없음, ADR-028:93)이라 v1 호환성 문제가 없는지, 시나리오 72 뮤테이션 타당성, 범위(lease_store.rs 무변경) 전부 확인 후 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-39_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-39_ambiguous_renew_복구_2026-08-20.txt"
raw_output_digest: "sha256:63e57cccf9f48c4edb76019214c0b385f6960592f1568a241a84f77ec1980534"
raw_output_bytes: 8321

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto/job.proto ExecutionGrant v1 -> v2, 서명 대상 필드 25 lease_from_durable_store(bool) 순수 추가, domain_tag gputeer/v2/grant 신설(기존 v1 필드 번호 불변)"
  canonical_spec: "docs/protocol/signing.md v1(문서 구조 불변) — canonical_v1.json 45+3=48개 벡터, SCHEMA_FINGERPRINT.txt 갱신"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  cargo test --workspace --exclude gputeer-runtime-windows
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 여러 차례 연속, 각 120초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-39_ambiguous_renew_복구_2026-08-20.txt,
   docs/evidence/_raw/DoD-39_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공(2라운드 각각), 실패 0건
  canonical self-test/verify: all checks passed, 48개 벡터 일치
  coordinator-agent-selftest(2라운드 수정 후): 5회 연속 exit=0, 72개 시나리오, 약 39.0~39.9초/회
artifacts:
  - docs/plans/2026-08-20_1615_ambiguous_renew_복구_v1.md
  - crates/agent/src/lib.rs
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - proto/job.proto
  - crates/protocol/src/to_fields.rs
  - crates/protocol/src/canonical.rs
  - crates/protocol/src/constants.rs
  - crates/protocol/tests/t1b_grant_and_control.rs
  - tools/canonical/reference_canonical.py
  - tests/vectors/canonical_v1.json
  - proto/SCHEMA_FINGERPRINT.txt
  - docs/evidence/_raw/DoD-39_ambiguous_renew_복구_2026-08-20.txt
  - docs/evidence/_raw/DoD-39_review.txt
negative_tests:
  - "selftest 시나리오 65 — Renew SQLite commit 직후·결과 전송 전 drop, 동일 Agent 가 bounded reconnect 후 기존 Grant/ACK get_or_issue 경로에서 정확히 committed expiry 를 받고 새 nonce Renew 성공"
  - "selftest 시나리오 66~69·71 — ambiguous 복구 경로에서도 durable revoke·만료·max-duration·replay guard·재접속 예산 소진이 전부 그대로 강제됨을 확인"
  - "selftest 시나리오 70 — 명시적 unsafe legacy(--lease-db 없음)에서 recovery 게이트가 꺼져 있으면 connection_attempt=0 한 번만 실행(1차 구현)"
  - "selftest 시나리오 72(2라운드 신설) — legacy Coordinator + recovery=true 오조합에서도 signed durable=false Grant 를 ACK·새 Renew 전에 fatal 거부. 방어 조건을 임시로 제거하는 뮤테이션으로 원래 결함(legacy 가 조작 Lease 발급, 두 번째 Renew 까지 성공)이 재현됨을 확인, 원복 후 72/72 재검증"
  - "t1b_grant_and_control.rs 신규 테스트 — lease_from_durable_store 비트가 canonical 서명 입력에 실제로 영향을 주는지(true/false 변조 시 서명 검증 실패) 확인"
limitations:
  - "범용 durable request ledger(요청 digest·signed response 재전송)는 만들지 않았다 — 설계 조사가 현재 실제 공백은 '정확히 한 번 처리'가 아니라 가용성 공백임을 코드로 확인했다. 같은 nonce 재전송에 이전 응답을 그대로 재생하는 정확한 idempotency 는 여전히 없다(새 heartbeat 로 취급, 정책 재검사로 안전성만 확보)"
  - "lease_from_durable_store 비트는 'Coordinator 가 이 Grant 를 durable store 에서 발급했다'는 서명된 사실만 증명한다 — durable request ledger 나 Coordinator HA(여러 Coordinator 인스턴스 간 일관성)까지 증명하지 않는다"
  - "Coordinator 재시작/HA 를 넘는 자동 복구, Hello FRESH/RESUME 통합 상태 머신, 다중 Agent 경쟁(로드맵 조각 7)은 여전히 범위 밖"
  - "proto ExecutionGrant v1->v2 승격은 기존 v1 바이너리와 서명 수준에서 호환되지 않는다 — 이 저장소는 원격이 없는 배포 전 상태라 승격을 문서화만 하고 마이그레이션 경로는 만들지 않았다(ADR-028 과 일관)"
decision: "로드맵 조각 5를 '범용 durable request ledger' 대신 정직하게 재범위된 'Ambiguous Renew 의 durable Lease 상태 기반 복구'로 완료했다. 구현을 코덱스 CLI(workspace-write)에 위임했고, 독립 검수 1라운드가 진짜 안전 결함(legacy 오조합 시 자동 복구가 상태 재조회가 아니라 새 발급이 되는 문제)을 찾아 CHANGES_REQUESTED, 서명된 durable 비트(proto 순수 추가)로 근본 수정한 2라운드에서 ACCEPTED. 감독자가 매 라운드 빌드·테스트·canonical 검증·selftest 를 직접 재실행해 재확인했다. 로드맵 7조각 중 1·2·3·4·5·6 완료 — 남은 7(다중 Agent selftest, 조각 5 이후 유의미)만 후속 조각으로 남는다."
---

# DoD-39 · Ambiguous Renew 복구 (로드맵 조각 5 재정의)

## 무엇을 입증하려 했는가

로드맵 원안의 조각 5("durable request ledger")를 오늘의 설계
조사(`p201`, read-only)가 정직하게 재범위했다 — 지금 실제 공백은
"정확히 한 번 처리"가 아니라 **가용성** 공백이다. Agent 가
`RenewLeaseRequest` 전송 뒤 결과를 받기 전에 연결이 끊기면
`SessionError::AmbiguousRenew` 로 분류돼 지금까지는 재접속 루프가
즉시 `Err` 로 끝났다. Coordinator 는 실제로는 SQLite 커밋을 결과
전송 **전에** 이미 확정하므로, Agent 가 무조건 종료하는 건 안전
하되 과도했다. Resume 프로토콜(`DoD-36`~`38`)이 "현재 권위 있는
상태"를 이미 제공하므로, 별도 request ledger 없이도 기존
Grant/ACK 경로의 `get_or_issue()` 로 안전하게 복구할 수 있다는
게 조사의 결론이었다.

## 구현 1라운드 (`p202`, 코덱스 workspace-write)

- `AmbiguousRenew` 를 새 게이트
  `--recover-ambiguous-renew-from-durable-lease`(기본 false) 가
  켜졌을 때만 bounded reconnect 대상으로 바꿨다.
- 재접속 성공 후 **기존 Grant/ACK 경로**(`get_or_issue()`)를 그대로
  재실행해 최신 저장 Lease 를 재조회·검증한 뒤 새 nonce 로 새
  Renew 를 보낸다.
- Coordinator 에 테스트 전용
  `--drop-after-renew-commit-before-result-once` hook 추가.
- selftest 시나리오 65~71 신설, 71/71 시나리오 5회 연속 통과.

## 독립 검수 1라운드 (`p203`) — **CHANGES_REQUESTED**

진짜 안전 결함을 찾았다 — durable 복구 게이트가 Coordinator 의
실제 `--lease-db` 설정과 **검증 가능하게 결합되지 않았다**. 사용자가
legacy Coordinator 와 이 Agent 게이트를 잘못 조합하면, 재접속
Coordinator 는 저장된 상태를 재조회하는 게 아니라 **그 순간 새로
조작된 Lease** 를 돌려주는데, Agent 는 이걸 "커밋된 상태 재조회"로
착각하고 그대로 진행했다.

## 구현 2라운드 (`p204`, 코덱스 workspace-write) — 근본 수정

`proto/job.proto` 의 `ExecutionGrant` 를 schema v2 로 승격해 서명
대상 필드 25 `lease_from_durable_store`(bool) 를 순수 추가했다.
Coordinator 는 `lease_store.is_some()` 이고 실제 SQLite transaction
commit 이 성공했을 때만 이 필드를 true 로 서명한다. Agent 는
Ambiguous Renew 복구 시 받은 Grant 가 서명된 durable=true 가
**아니면** ACK·checkpoint·새 Renew 전송 **전에**
`DURABLE_LEASE_RECOVERY_REFUSED` 로 fatal 종료한다. 새 selftest
시나리오 72 가 legacy+recovery=true 오조합을 검증하며, 뮤테이션
(방어 조건 제거)으로 원래 결함(legacy 가 조작 Lease 발급, 두 번째
Renew 까지 성공)이 재현됨을 확인한 뒤 원복했다.

## 독립 검수 2라운드 (`p205`) — **ACCEPTED**

새 필드가 canonical 서명 입력에 실제로 포함되는지(값 변조 시 서명
실패), Coordinator 가 이 필드를 정직하게 채우는지("store 는 Some
이지만 특정 Lease 는 미커밋"인 경로 없음), Agent 의 거부가
ACK·checkpoint·Renew 전송 **전**에 있고 재시도 안 되는지, durable/
legacy 정상 경로에 회귀가 없는지, v2 승격이 배포 전 변경이라
호환성 문제가 없는지까지 전부 코드로 확인하고 잔여 지적 없이
`ACCEPTED`.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   성공(2라운드 각각), 실패 0건
canonical self-test/verify                                        all checks passed, 48개 벡터 일치
coordinator-agent-selftest(수정 후)                                5회 연속 exit=0, 72개 시나리오, 약 39.0~39.9초/회
```

## 이 실험이 증명하지 "않는" 것

- 범용 durable request ledger(요청 digest·응답 재생)는 없다 — 같은
  nonce 재전송의 정확한 idempotency 는 여전히 새 heartbeat 로
  취급된다(정책 재검사로만 안전성 확보).
- `lease_from_durable_store` 는 "이 Grant 가 durable store 에서
  왔다"는 사실만 증명한다 — Coordinator HA 나 durable ledger 전체
  계약을 증명하지 않는다.
- Coordinator 재시작/HA 를 넘는 자동 복구, 다중 Agent 경쟁(로드맵
  조각 7)은 여전히 범위 밖.

## 결정

1. 로드맵 조각 5 를 원안(범용 durable request ledger)보다 훨씬
   작은 "Ambiguous Renew 의 durable Lease 상태 기반 복구"로
   완료했다.
2. 독립 검수 1라운드가 legacy 오조합 안전 결함을 찾아
   `CHANGES_REQUESTED`, proto 순수 추가로 근본 수정한 2라운드에서
   `ACCEPTED`.
3. **로드맵 7조각 중 1·2·3·4·5·6 완료** — 남은 7(다중 Agent
   selftest, 조각 5 이후 유의미)만 후속 조각으로 남는다.

관련: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`
(전체 로드맵) · `docs/evidence/DoD-35_자동_재접속_최소_경로.md`
(`AmbiguousRenew` 최초 정의) · `docs/evidence/DoD-27_revoked_signed_outcome.md`·
`DoD-36_resume_프로토콜.md`(proto 순수 추가 패턴의 선례)
