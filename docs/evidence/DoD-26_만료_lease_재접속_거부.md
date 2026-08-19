---
schema_version: 2
id: DoD-26
claim: "CoordinatorLeaseStore::get_or_issue() 가 저장된 Lease 의 expires_at_unix_ms 를 확인해, 이미 만료된 Lease 는 재접속(process-restart rehydration, DoD-24)해도 다시 발급하지 않고 LeaseStoreError::Expired 로 거부한다. 만료 경계는 expires_at_unix_ms <= now_unix_ms(경계 포함)로 판정하며, 이는 crates/protocol/src/signing.rs 의 Lease 서명 검증(now >= expires_at)과 crates/agent/src/lib.rs 의 revoke 검사(expires <= now)가 이미 쓰는 경계 규칙과 일치한다 — 1라운드 검수가 처음 구현(엄격한 < 비교)이 이 경계와 어긋난다는 진짜 결함을 찾아 수정했다"
status: PASS
commit: 4c13cd3

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현·수정) / claude-code (cargo build/test 독립 재확인)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T01:36:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 매 라운드 이전 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "get_or_issue() 의 만료 판정 경계가 crates/protocol/src/signing.rs 의 Lease 서명 검증·crates/agent/src/lib.rs 의 revoke 검사와 일관되는지, revoked 검사와 expired 검사의 순서, get_or_issue() 시그니처 변경에 따른 모든 호출부의 now_unix_ms 전달 정확성, selftest 시나리오 36 의 타이밍(250ms TTL + 400ms sleep) 이 느린 환경에서도 안정적인지, 저장소 단위 테스트가 실제로 저장값을 보존하는지. 2라운드 진행 — 1라운드(p149) CHANGES_REQUESTED(핵심 결함: 만료 판정이 엄격한 < 를 써서 정확히 expires_at_unix_ms 와 같은 순간을 아직 유효로 취급 — crates/protocol/src/signing.rs 와 crates/agent/src/lib.rs 는 이미 <=(경계 포함)를 쓰고 있어, 같은 Lease 가 Coordinator 에서는 재발급되는데 Agent 에서는 같은 순간 즉시 만료로 거부되는 계층 간 불일치가 생긴다. 기존 테스트도 경계값(정확히 1_000)은 검사하지 않고 1_001만 확인해 이 불일치를 놓치고 있었다) -> < 를 <= 로 수정, 경계값 테스트 2건(정확히 경계에서 Expired, 경계 바로 전은 정상) 추가 -> 2라운드(p151) ACCEPTED — 수정된 경계가 signing.rs·agent 양쪽과 실제로 일치함을 코드 대조로 재확인"
review_artifact: "docs/evidence/_raw/DoD-26_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-26_만료_lease_재접속_거부_2026-08-20.txt"
raw_output_digest: "sha256:71caf259d2b6f1d35914484a46c5d38d4b7af49a0eb65055fd6f8c43c89958c3"
raw_output_bytes: 540

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "새 proto 메시지·필드 없음 — 순수히 Coordinator 내부 판정 로직(get_or_issue() 의 만료 검사)에 한정된다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub + SQLite 파일 I/O"
network_profile: "127.0.0.1 루프백 TCP 만 사용, 별도 프로세스 쌍이 같은 SQLite lease-db 파일을 공유"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 2세트 x 5회, 각 60초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-26_만료_lease_재접속_거부_2026-08-20.txt 전문 참조)

  cargo test --workspace --exclude gputeer-runtime-windows: 42개 스위트 전부 test result: ok, FAILED/error[ 검색 결과 없음(coordinator 유닛 테스트 24→27개)
  coordinator-agent-selftest: 1차(구현 직후) 5회 + 2차(경계 수정 직후) 5회, 매번 36개 시나리오 전부 exit=0
artifacts:
  - docs/plans/2026-08-20_0136_만료_lease_재접속_거부_v1.md
  - docs/reports/2026-08-20_0136_만료_lease_재접속_거부.md
  - crates/coordinator/src/lease_store.rs
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-26_만료_lease_재접속_거부_2026-08-20.txt
  - docs/evidence/_raw/DoD-26_review.txt
negative_tests:
  - "selftest 시나리오 36 — 짧은 --lease-ttl-ms(250ms) 로 Lease 를 발급한 뒤 실제로 400ms sleep 해서 만료시키고, 완전히 새 프로세스 쌍이 같은 --lease-db 로 재접속을 시도하면 Coordinator 가 Expired 로 거부하고 Grant 를 전송하지 않는지 확인"
  - "단위 테스트 unexpired_existing_lease_is_reissued — expires_at_unix_ms 보다 이른 now 는 정상 재발급됨"
  - "단위 테스트 expired_existing_lease_is_rejected_without_overwrite — 만료된 레코드가 Expired 로 거부되고 저장값을 덮어쓰지 않음"
  - "경계값 테스트(2라운드 추가) — expires_at_unix_ms == now_unix_ms(정확히 경계) 는 Expired, now_unix_ms == expires_at_unix_ms - 1(경계 바로 전) 은 정상 재발급"
  - "뮤테이션(코덱스 자체 보고) — 만료 조건을 무력화하면 시나리오 36 이 예상대로 실패, 원복 후 재검증 통과"
limitations:
  - "만료된 Lease 의 자동 갱신·재발급 정책은 다루지 않는다 — 그냥 거부만 한다"
  - "새 signed outcome/proto 변경은 하지 않았다 — 기존 raw error 거부 패턴을 그대로 썼다(DoD-25 의 Revoked 오류와 같은 스타일)"
  - "revoked 검사가 expired 검사보다 먼저 실행된다 — 둘 다 해당하는 레코드는 Revoked 로만 보고되고 Expired 사실은 별도로 드러나지 않는다(2라운드 검수가 의미상 타당하다고 확인했다)"
  - "자동 재접속 루프, ResumeLeaseRequest, 다중 Agent 경쟁은 여전히 범위 밖"
  - "구현자와 독립 검수자가 이번에도 같은 도구(codex CLI)의 서로 다른 프로세스 인스턴스였다 — 컨텍스트는 독립이지만 같은 모델 계열이 자기 코드를 리뷰한다는 근본적 한계는 남는다"
decision: "DoD-24 가 명시적으로 이월한 '만료된 Lease 의 재접속 복원 거부' 시나리오를 구현했다. 구현·수정 모두 코덱스 CLI(workspace-write)에 위임하고 claude-code 는 독립 재검증·독립 검수 감독 역할을 맡았다. 독립 검수 1라운드가 실제 계층 간 불일치 결함을 찾아냈다 — Coordinator 의 만료 판정이 엄격한 부등호를 써서, 이미 이 저장소가 정착시킨(Lease 서명 검증·Agent 의 revoke 검사) '경계 포함' 만료 규칙과 어긋났다. 정확히 만료 시각과 같은 순간에는 Coordinator 는 재발급을 허용하는데 Agent 는 같은 Lease 를 즉시 만료로 거부하는 모순이 생길 뻔했다. 경계를 통일하고 경계값 테스트를 추가한 뒤 2라운드에서 ACCEPTED."
---

# DoD-26 · 만료된 Lease 재접속 거부

## 무엇을 입증하려 했는가

`docs/evidence/DoD-24_lease_재접속_최소_조각.md` 가 "만료 Lease 의
재접속 복원 거부 시나리오는 시간 관계상 다음 조각으로 미뤘다" 고
명시적으로 남겼다 — `get_or_issue()` 가 지금까지 identity 충돌과
(`DoD-25` 이후) revoked 여부만 확인하고 **만료 여부는 확인하지
않았다.**

## 구현 (코덱스, `p148`)

- `LeaseStoreError::Expired { expires_at_unix_ms: u64 }` 신설.
- `get_or_issue()` 시그니처에 `now_unix_ms` 추가, 저장된 레코드가
  만료됐으면(처음엔 `<` 로) 거부.
- `issue_lease()`(`crates/coordinator/src/lib.rs`) 가 이미 계산해둔
  `now` 를 정확히 전달하도록 호출부 수정.
- selftest 시나리오 36 — 250ms TTL 로 발급 후 실제 400ms sleep 으로
  만료시킨 뒤 재접속 거부 확인.
- 저장소 단위 테스트 2건(만료/미만료).

## 독립 검수 1라운드(`p149`) — CHANGES_REQUESTED, 진짜 결함 1건

만료 판정이 `stored.expires_at_unix_ms < now_unix_ms`(엄격한 `<`)
를 썼는데, 같은 저장소가 다루는 Lease 의 **서명 검증 계층**
(`crates/protocol/src/signing.rs`, `now >= expires_at` 거부)과
Agent 의 revoke 검사(`crates/agent/src/lib.rs`, `expires <= now`)
는 이미 **경계 포함**(`<=`) 규칙을 쓰고 있었다. 즉 정확히
`expires_at_unix_ms` 와 같은 순간에 같은 Lease 를 Coordinator 는
"아직 유효, 재발급 OK" 로 판단하는데 Agent 는 즉시 만료로 거부하는
— 두 계층의 계약이 어긋나는 상황이 생길 뻔했다. 기존 테스트도
`1_001` 만 확인해 정확한 경계(`1_000`)를 검사하지 않아 이 불일치를
놓치고 있었다.

## 수정(코덱스, `p150`)

`<` 를 `<=` 로 바꾸고, 경계값 테스트 2건(정확히 경계에서 `Expired`,
경계 바로 전은 정상 재발급)을 추가했다. claude-code 가 직접
재빌드·재테스트하고 `coordinator-agent-selftest` 를 5회 반복
실행해(하드 타임아웃 포함) 확인했다.

## 독립 검수 2라운드(`p151`) — **ACCEPTED**

수정된 경계가 `signing.rs`·`agent/src/lib.rs` 양쪽과 실제로
일치하는지 코드 대조로 재확인하고, 새 경계값 테스트 2건과 selftest
시나리오 36 이 여전히 정확한지 확인했다.

## 결과

```text
cargo build --workspace --exclude gputeer-runtime-windows      성공
cargo test --workspace --exclude gputeer-runtime-windows       42개 스위트 전부 통과, 0 failed
coordinator-agent-selftest                                      2세트 x 5회, 매번 36개 시나리오 전부 exit=0
```

## 이 실험이 증명하지 "않는" 것

- 만료된 Lease 의 자동 갱신/재발급 정책 — 거부만 한다.
- 새 signed outcome/proto 변경 없음.
- revoked 와 expired 를 동시에 만족하는 경우 `Revoked` 로만 보고됨
  (검수가 의미상 타당하다고 확인).
- 자동 재접속 루프, 다중 Agent 경쟁은 여전히 범위 밖.

## 결정

1. `DoD-24` 가 이월한 만료 Lease 재접속 거부를 구현했다.
2. 독립 검수 1라운드가 만료 판정 경계 불일치라는 진짜 계층 간
   결함을 찾아냈고, 수정 후 2라운드에서 `ACCEPTED`.

관련: `docs/evidence/DoD-24_lease_재접속_최소_조각.md` ·
`docs/evidence/DoD-25_coordinator_lease_revoke_영속화.md` ·
`docs/plans/2026-08-20_0136_만료_lease_재접속_거부_v1.md`
