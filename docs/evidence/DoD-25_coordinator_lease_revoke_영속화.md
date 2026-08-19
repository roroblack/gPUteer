---
schema_version: 2
id: DoD-25
claim: "CoordinatorLeaseStore 에 revoked_at_unix_ms 필드를 추가해 Lease revoke 상태를 재시작을 넘어 영속화했다. send_revoke_notice() 가 wire 로 통지를 만들어 보내기 전에 mark_revoked()(idempotent — 최초 revoke 시각 유지)로 SQLite 커밋을 먼저 확정한다. get_or_issue() 와 두 갱신 경로(정상 경로·renew_outcome_override 경로) 모두 저장된 레코드가 revoked 면 거부한다 — revoke 된 뒤 프로세스가 재시작해도 새 Agent 가 같은 Lease 를 다시 받거나 갱신할 수 없다. 기존 SQLite 파일과의 호환을 위해 open() 시점에 PRAGMA table_info 로 컬럼 존재를 확인하고 없으면 ALTER TABLE 로 보정하는 마이그레이션을 추가했다. 새 proto 메시지·signed denial 은 추가하지 않았다 — Coordinator 가 Grant 발급 자체를 raw error 로 거부하는 fail-closed 방식을 그대로 썼다"
status: PASS
commit: 4491c13

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (설계 조사·구현) / claude-code (cargo build/test 독립 재확인)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-19T19:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "migrate_revoked_at_column() 이 트랜잭션 안에서 안전하게 동작하는지와 listener bind 보다 먼저 실행되는지, mark_revoked() 의 idempotent 동작과 트랜잭션 범위, get_or_issue()·갱신 경로(정상 경로 + renew_outcome_override 읽기 전용 경로) 양쪽 다 revoked 검사가 실제로 들어갔는지, send_revoke_notice() 의 mark_revoked() 호출이 wire 전송보다 먼저이고 revoke_delay_ms 순서가 기존 시나리오(만료된 Lease revoke 거부)의 의미를 깨지 않는지, override 가 있어도 저장소에는 Grant 의 실제 lease_id 가 기록되는지, 마이그레이션 단위 테스트가 실제로 컬럼 없는 옛 스키마를 수동 생성해서 검증하는지, 기존 34개 시나리오 회귀 여부. 1라운드(p147) 만에 ACCEPTED — 수정 요청 없음, 프롬프트의 시나리오 번호 오기(29→실제 31) 지적만 있었음(코드 문제 아님)"
review_artifact: "docs/evidence/_raw/DoD-25_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-25_coordinator_lease_revoke_영속화_2026-08-19.txt"
raw_output_digest: "sha256:775b2c0364c5d8987ac624a8bbd1f9c36b509826307b467c63cb1422c6e88877"
raw_output_bytes: 628

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "새 proto 메시지·필드 없음 — RevokeLeaseNotice 자체는 바꾸지 않았다. 변경은 순수히 Coordinator 내부 SQLite 스키마(coordinator_leases 테이블에 컬럼 1개 추가)에 한정된다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub + SQLite 파일 I/O"
network_profile: "127.0.0.1 루프백 TCP 만 사용, 별도 프로세스 쌍이 같은 SQLite lease-db 파일을 공유"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 5회 연속, 각 90초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-25_coordinator_lease_revoke_영속화_2026-08-19.txt 전문 참조)

  cargo test --workspace --exclude gputeer-runtime-windows: 42개 스위트 전부 test result: ok, FAILED/error[ 검색 결과 없음(coordinator 크레이트 유닛 테스트 20→24개로 증가)
  coordinator-agent-selftest: 5회 연속, 35개 시나리오 전부 exit=0
  뮤테이션 2건 — mark_revoked() 무력화, get_or_issue() revoked 검사 무력화 — 둘 다 시나리오 35 를 정확히 실패시킴, 원복 후 재통과
artifacts:
  - docs/plans/2026-08-19_0110_coordinator_lease_revoke_영속화_v1.md
  - crates/coordinator/src/lease_store.rs
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-25_coordinator_lease_revoke_영속화_2026-08-19.txt
  - docs/evidence/_raw/DoD-25_review.txt
negative_tests:
  - "selftest 시나리오 35 — 1차 프로세스 쌍이 revoke 통지를 성공적으로 주고받은 뒤(REVOKE_RESULT ok=true), 저장소를 재조회해 revoked_at_unix_ms.is_some() 확인. 이어서 같은 lease-db 로 완전히 새로운 프로세스 쌍이 같은 lease_id/holder_node_id 로 재접속을 시도하면 Coordinator 가 최초 발급 자체를 거부(stderr 에 'lease store 최초 발급 실패'·'revoked' 포함)하고 Grant 를 전송하지 않으며, Agent 는 기존 'Grant 프레임 읽기/검증 실패' 경로로 실패, 양쪽 다 RESULT ok=true 없음"
  - "단위 테스트 opens_old_schema_and_preserves_existing_lease_as_active — 컬럼 없는 옛 스키마 테이블을 수동으로 만들어 open() 이 마이그레이션한 뒤 기존 Lease 가 revoked_at_unix_ms == None 으로 보존되는지 확인"
  - "단위 테스트 — mark_revoked() 후 재조회 시 timestamp 보존, 이미 revoked 인 Lease 를 다시 mark_revoked() 해도 최초 timestamp 유지(idempotent), revoked Lease 의 get_or_issue() 가 Revoked 로 거부되고 저장값을 덮어쓰지 않음, revoked Lease 의 갱신이 거부됨"
  - "뮤테이션 1(코덱스 자체 보고) — mark_revoked() 호출 제거 시 시나리오 35 가 revoked_at_unix_ms NULL 로 실패"
  - "뮤테이션 2(코덱스 자체 보고) — get_or_issue() 의 revoked 검사 제거 시 2차 프로세스가 성공해버려 시나리오 35 실패"
limitations:
  - "새 signed RENEW_OUTCOME(예: REVOKED)이나 proto 변경은 하지 않았다 — Coordinator 가 revoked Lease 에 대한 최초 발급/갱신 요청을 기존 raw error 로 거부한다. Agent 쪽에서 이 거부와 다른 종류의 handshake 실패(예: 위조 서명)를 구분할 신호가 없다"
  - "Agent 의 revoke 상태(인메모리 bool)는 이번 조각에서 영속화하지 않았다 — Coordinator 가 Grant 발급 자체를 거부하므로 Agent 쪽 영속화가 굳이 필요하지 않다고 판단했다"
  - "revoke 해제(un-revoke) API 는 만들지 않았다 — 한번 revoked 면 영구적이다"
  - "--lease-db 없는 레거시 경로(lease_store=None)는 이 영속화의 보호를 받지 않는다 — 원래 재시작 영속성을 약속한 적이 없는 경로다"
  - "만료된 Lease 의 재접속 복원 거부(DoD-24 가 이월한 시나리오)는 여전히 다음 조각이다"
  - "자동 재접속 루프, ResumeLeaseRequest, 다중 Agent 경쟁, 다중 Coordinator HA 는 여전히 범위 밖"
  - "구현자와 독립 검수자가 이번에도 같은 도구(codex CLI)의 서로 다른 프로세스 인스턴스였다 — 컨텍스트는 독립이지만 같은 모델 계열이 자기 코드를 리뷰한다는 근본적 한계는 남는다"
decision: "Lease revoke 상태를 CoordinatorLeaseStore 에 영속화해, DoD-24 가 남긴 안전 공백(revoke 된 Lease 가 재접속으로 되살아나는 문제)을 닫았다. 설계 조사(코덱스, read-only)가 먼저 기존 스키마·get_or_issue()·send_revoke_notice() 를 실측해 SQLite ALTER TABLE 마이그레이션이 필요하다는 걸 정확히 짚었고, 그 설계를 그대로 구현에 넘겼다. 사용자 요청에 따라 구현을 코덱스 CLI(workspace-write)에 위임하고 claude-code 는 독립 재검증·독립 검수 감독 역할을 맡았다. 이번엔 구현자가 evidence 문서·CLAUDE.md·HISTORY 를 건드리지 않고 코드·계획 문서만 남겨 '구현자와 검수자가 다르다'는 경계를 이전 조각들보다 더 깔끔하게 지켰다. 독립 검수가 1라운드 만에 ACCEPTED — 마이그레이션 로직·두 갱신 경로 전부의 revoked 검사·override 시에도 실제 lease_id 로 기록되는지까지 전부 확인했다."
---

# DoD-25 · Coordinator Lease Revoke 영속화

## 무엇을 입증하려 했는가

`docs/evidence/DoD-24_lease_재접속_최소_조각.md` 가 명시적으로 남긴
안전 공백 — "revoke 상태는 재접속에서 보존되지 않는다. `revoked`
는 Agent 메모리 상태이고 `CoordinatorLeaseStore` 에는 그 필드가
없어서, revoke 된 뒤 프로세스가 끊기면 새 Agent 프로세스가 같은
Lease 를 다시 받을 수 있다" — 를 닫았다.

## 설계 조사(코덱스, `p145`)

`StoredLease` 스키마(11개 필드, `revoked` 관련 필드 없음)와
`CREATE TABLE IF NOT EXISTS` 만 있고 마이그레이션 헬퍼가 없다는 걸
실측으로 확인해, 기존 DB 파일 호환을 위해 `PRAGMA table_info` +
`ALTER TABLE ADD COLUMN` 보정이 필요하다고 정확히 짚었다.
`get_or_issue()` 의 정확한 검사 삽입 지점, `send_revoke_notice()`
가 지금 `lease_store` 를 아예 받지 않는다는 사실(revoke 발생 시
저장소에 아무것도 안 남는다는 걸 코드로 확인)도 실측했다.

## 구현 (코덱스, `p146`)

- `StoredLease`·SQLite 테이블에 `revoked_at_unix_ms Option<u64>`
  추가. `open()` 시점에 `migrate_revoked_at_column()` 으로 기존
  DB 파일을 트랜잭션 안에서 안전하게 보정.
- `mark_revoked()` 신설 — idempotent(이미 revoked 면 최초 timestamp
  유지).
- `send_revoke_notice()` 가 `lease_store` 를 받아, **wire 전송보다
  먼저** `mark_revoked()` 로 커밋을 확정한다. 테스트용
  `revoke_lease_id_override` 가 있어도 저장소에는 override 가 아닌
  Grant 의 **실제** `lease_id` 를 기록한다.
- `get_or_issue()` 와 갱신 경로 **양쪽 다**(정상 경로의
  `renew_existing_within_duration()`류, 그리고 `build_renew_result()`
  안 `renew_outcome_override` 읽기 전용 분기까지) revoked 검사를
  추가해, revoke 통지 전송이 끊긴 기존 Agent 가 갱신을 재시도해도
  Coordinator 가 다시 갱신해주지 않게 했다.
- selftest 시나리오 35, 저장소 단위 테스트 5건(마이그레이션·
  idempotency·발급 거부·갱신 거부 등).

## 독립 검수(`p147`) — **1라운드 만에 ACCEPTED**

마이그레이션이 트랜잭션 안에서 listener bind 보다 먼저 실행되는지,
두 갱신 경로 모두 검사가 있는지, `mark_revoked()` 가 wire 전송보다
먼저이고 기존 "만료된 Lease revoke 거부" 시나리오의 의미를 깨지
않는지, 마이그레이션 단위 테스트가 진짜로 컬럼 없는 옛 스키마를
수동 생성해서 검증하는지 — 전부 코드를 직접 열어 확인하고
`ACCEPTED`. 유일한 지적은 코드가 아니라 검수 프롬프트 자체의
시나리오 번호 오기(29 대신 실제로는 31번)였다.

## 결과

```text
cargo build --workspace --exclude gputeer-runtime-windows      성공
cargo test --workspace --exclude gputeer-runtime-windows       42개 스위트 전부 통과, 0 failed (coordinator 유닛 테스트 20→24)
coordinator-agent-selftest                                      5회 연속, 35개 시나리오 전부 exit=0
```

### 뮤테이션 테스트 2건(코덱스 자체 보고)

| # | 무력화한 것 | 예측대로 실패한 시나리오 |
|---|---|---|
| 1 | `mark_revoked()` 호출 | 35 |
| 2 | `get_or_issue()` 의 revoked 검사 | 35 |

## 이 실험이 증명하지 "않는" 것

- 새 signed outcome/proto 변경 없음 — raw error 거부만.
- Agent 쪽 revoke 상태 영속화 없음(불필요하다고 판단).
- revoke 해제(un-revoke) API 없음.
- 레거시 `lease_store=None` 경로는 이 보호를 안 받음.
- 만료 Lease 재접속 복원 거부, 자동 재접속 루프, 다중 Agent 경쟁은
  여전히 범위 밖.

## 결정

1. Lease revoke 상태를 영속화해 `DoD-24` 가 남긴 안전 공백을 닫았다
   — 구현은 코덱스 CLI(workspace-write)에 위임했다.
2. 이번엔 구현자가 evidence·CLAUDE.md·HISTORY 를 건드리지 않아
   구현자/검수자 경계가 이전 조각들보다 깔끔했다.
3. 독립 검수가 마이그레이션·두 갱신 경로·override 시 실제
   lease_id 기록까지 전부 확인하고 1라운드 만에 `ACCEPTED`.

관련: `docs/evidence/DoD-22_lease_revoke_최소_조각.md` ·
`docs/evidence/DoD-24_lease_재접속_최소_조각.md` ·
`docs/plans/2026-08-19_0110_coordinator_lease_revoke_영속화_v1.md`
