---
schema_version: 2
id: DoD-27
claim: "proto/lease.proto 의 RenewOutcome 에 RENEW_OUTCOME_REVOKED = 8 을 순수 추가하고, Coordinator 가 revoked Lease 에 대한 갱신(renew) 요청을 raw error 로 연결을 끊는 대신 서명된 RenewLeaseResult{ outcome: 8 } 로 응답하도록 고쳤다 — override 읽기 전용 경로와 renew_existing_within_duration() 을 쓰는 정상 경로 둘 다. Agent 는 outcome 8 을 만나면 RENEW_REFUSED:REVOKED 로 즉시 종료하고, Coordinator 도 결과를 전송한 뒤 갱신 루프를 break 한다(오늘 이미 두 번 나온 outcome-분기 교착 버그를 이번엔 처음부터 피했다). 초기 Grant 발급 시점의 revoked 거부는 범위 밖으로 남겨 기존 raw error 그대로다"
status: PASS
commit: 86aef4e

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현) / claude-code (canonical self-test·schema check·cargo build/test 독립 재확인)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T03:57:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "proto enum 값 추가가 기존 번호를 안 바꾸는 순수 추가인지, Coordinator 의 두 revoked 감지 경로(override 읽기 전용 분기 + 정상 renew_existing_within_duration 경로) 모두 signed outcome 8 로 변환하는지, 오늘 이미 두 번(DoD-22·DoD-23) 나온 outcome-분기 교착 버그가 세 번째로 있는지(특히 break 가 결과 전송·flush 이후에 실행되는지, Agent 쪽도 outcome 8 을 즉시 종료 처리하는지), 새 테스트 전용 플래그 revoke_before_renew 가 기존 revoke_after_round 경로와 독립적인지, selftest 시나리오 37 이 실제 서명·replay·nonce 검증을 통과한 뒤의 outcome 8 을 확인하는지(단순 exit/stdout 검사가 아닌지), lease_store.rs 가 이번 조각에서 전혀 안 바뀌었는지, 초기 Grant 발급 거부가 여전히 범위 밖(raw error)인지. 1라운드(p157) 만에 ACCEPTED — 수정 요청 없음"
review_artifact: "docs/evidence/_raw/DoD-27_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-27_revoked_signed_outcome_2026-08-20.txt"
raw_output_digest: "sha256:2f148fb05b9028f502833a29e74c3ddba165db591a0b477bbbc616d83b9e7299"
raw_output_bytes: 641

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto/lease.proto 의 RenewOutcome enum 에 값 1개(REVOKED=8) 순수 추가. 새 메시지·domain_tag 는 없다 — 기존 RenewLeaseResult·서명 경로를 그대로 재사용한다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음, enum 값 자체는 서명 대상 필드가 아니라 canonical_encode 흐름에 영향 없음 — check_schema.py·reference_canonical.py 로 확인)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(coordinator-agent-selftest 기존 하네스와 동일)"
command: |
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/check_schema.py
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 5회 연속, 각 60초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-27_revoked_signed_outcome_2026-08-20.txt 전문 참조)

  reference_canonical.py --self-test: all checks passed
  check_schema.py: 오류 0건, 경고 42건(SCHEMAS 43개/.proto 85개 — 기존과 동일 패턴)
  cargo test --workspace --exclude gputeer-runtime-windows: 42개 스위트 전부 test result: ok, FAILED/error[ 없음
  coordinator-agent-selftest: 5회 연속, 37개 시나리오 전부 exit=0
artifacts:
  - docs/plans/2026-08-20_0357_revoked_signed_outcome_v1.md
  - docs/reports/2026-08-20_0357_revoked_signed_outcome.md
  - proto/lease.proto
  - crates/coordinator/src/lib.rs
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-27_revoked_signed_outcome_2026-08-20.txt
  - docs/evidence/_raw/DoD-27_review.txt
negative_tests:
  - "selftest 시나리오 37 — 같은 연결에서 revoke 후(--revoke-before-renew, 저장소에만 확정하고 wire notice는 안 보냄) 갱신 요청을 보내면 서명된 RENEW_OUTCOME_REVOKED(8) 을 받고 Agent 가 RENEW_REFUSED:REVOKED 로 즉시 종료. 실제 Agent 의 wire 서명·replay·nonce 검증을 전부 통과한 뒤의 outcome 분기이므로 단순 exit/stdout 검사가 아니다"
  - "뮤테이션(코덱스 자체 보고) — outcome 8 의 break 를 제거하면 시나리오 37 이 실패하고 정확히 오늘 이미 두 번(DoD-22·DoD-23) 나온 것과 같은 부류의 Coordinator 교착이 재현됨. 원복 후 재검증 통과"
limitations:
  - "초기 Grant 발급 시점의 revoked 거부(issue_lease() -> get_or_issue())는 범위 밖 — 기존 raw error 그대로다. SUPERSEDED 도 초기 발급엔 적용된 적이 없다는 선례를 따랐다"
  - "crates/coordinator/src/lease_store.rs 는 이번 조각에서 전혀 바뀌지 않았다 — LeaseStoreError::Revoked 자체는 그대로 두고, lib.rs 쪽에서 이 오류를 받았을 때의 처리(raw error -> signed outcome)만 바꿨다"
  - "EXPIRED(DoD-26)에 대한 같은 종류의 signed outcome 은 이번 조각에서 만들지 않았다 — REVOKED 만 다뤘다"
  - "재접속(reconnect) 시나리오에서의 REVOKED 전달은 다루지 않는다 — 이건 여전히 완전히 새 프로세스의 초기 발급 거부(DoD-25가 이미 다룸, raw error)로 처리된다"
  - "구현자와 독립 검수자가 이번에도 같은 도구(codex CLI)의 서로 다른 프로세스 인스턴스였다 — 컨텍스트는 독립이지만 같은 모델 계열이 자기 코드를 리뷰한다는 근본적 한계는 남는다"
decision: "갱신 경로에서 revoked Lease 거부를 raw error 에서 서명된 RenewLeaseResult{outcome: REVOKED} 로 바꿔, Agent 가 이 거부를 다른 handshake 실패와 구분할 수 있게 했다 — DoD-25 가 남긴 관측성 공백을 닫았다. proto/lease.proto 에 enum 값 하나만 순수 추가했고 canonical/schema 검사로 부작용이 없음을 확인했다. 구현을 코덱스 CLI(workspace-write)에 위임하고, 이번엔 구현자가 스스로 오늘 이미 두 번 나온 outcome-분기 교착 패턴을 의식해 처음부터 올바르게(break 를 결과 전송 이후·Agent 즉시 종료와 함께) 구현했다 — 독립 검수 1라운드 만에 ACCEPTED."
---

# DoD-27 · REVOKED signed outcome

## 무엇을 입증하려 했는가

`docs/evidence/DoD-25_coordinator_lease_revoke_영속화.md` 가
limitations 절에 명시했다: "새 signed RENEW_OUTCOME(예: REVOKED)이나
proto 변경은 하지 않았다 — Coordinator 가 revoked Lease 에 대한
최초 발급/갱신 요청을 기존 raw error 로 거부한다. Agent 쪽에서 이
거부와 다른 종류의 handshake 실패(예: 위조 서명)를 구분할 신호가
없다." 오늘 백로그 정리 조사(`p152`)가 이 항목을 "현재 동작의
보안 상태는 맞지만 Agent 가 revoked 와 다른 handshake 실패를
구분하지 못하는 직접적인 계약 공백"이라며 최우선 후보로 꼽았다.

## 구현 (코덱스, `p155`)

- `proto/lease.proto` 의 `RenewOutcome` 에 `RENEW_OUTCOME_REVOKED = 8`
  순수 추가(기존 번호 변경 없음).
- `crates/coordinator/src/lib.rs` 의 `build_renew_result()` — 두
  revoked 감지 경로(override 읽기 전용 분기, `renew_existing_within_duration()`
  정상 경로) 모두 `revoked_result(...)` 헬퍼로 서명된
  `RenewLeaseResult{ outcome: 8 }` 를 만들어 반환하도록 바꿨다 —
  더 이상 연결을 raw error 로 끊지 않는다.
- **오늘 이미 두 번(`DoD-22`·`DoD-23`) "정책 거부 outcome 을 보낸
  뒤 한쪽만 종료하고 다른 쪽은 계속 기다리는" 교착 버그가 나왔다**
  — 이번엔 구현자가 처음부터 이걸 의식해서, `if matches!(result.outcome,
  2 | 3 | 6 | 8) { break; }` 로 새 outcome 8 을 포함시키고, Agent
  쪽(`crates/agent/src/lib.rs`)도 `8 => return Err("RENEW_REFUSED:REVOKED".into())`
  로 즉시 종료하도록 같은 커밋 안에서 함께 고쳤다.
- 새 테스트 전용 `--revoke-before-renew` — ACK 직후 저장소에만
  revoke 를 확정하고 wire 로 `RevokeLeaseNotice` 는 안 보내서, "Agent
  가 revoke 통지를 받기 전에 이미 revoke 된 Lease 로 갱신을 시도하는"
  현실적인 경쟁 시나리오를 재현한다.
- selftest 시나리오 37.

## 독립 검수(`p157`) — **1라운드 만에 ACCEPTED**

proto enum 값이 순수 추가인지, 두 revoked 감지 경로 모두 signed
outcome 으로 바뀌었는지, **오늘 이미 두 번 나온 교착 패턴이 세
번째로 있는지 특히 의심하며** break 의 정확한 위치(결과 전송·flush
이후)와 Agent 쪽 즉시 종료 처리를 코드로 추적했다 — 문제없음을
확인했다. `revoke_before_renew` 가 기존 `revoke_after_round` 와
독립적인지, `lease_store.rs` 가 전혀 안 바뀌었는지, 초기 Grant
발급 거부가 여전히 범위 밖인지도 전부 코드 대조로 확인했다. 잔여
지적 없음.

## 결과

```text
reference_canonical.py --self-test                             all checks passed
check_schema.py                                                 오류 0건, 경고 42건
cargo build --workspace --exclude gputeer-runtime-windows       성공
cargo test --workspace --exclude gputeer-runtime-windows        42개 스위트 전부 통과, 0 failed
coordinator-agent-selftest                                       5회 연속, 37개 시나리오 전부 exit=0
```

### 뮤테이션 테스트(코덱스 자체 보고)

outcome 8 의 `break` 를 제거하면 시나리오 37 이 실패하고, **정확히
오늘 이미 두 번 나온 것과 같은 부류의 Coordinator 교착이
재현됐다** — 이 뮤테이션 자체가 "왜 이 `break` 가 꼭 필요한지"를
직접 증명한다. 원복 후 재검증 통과.

## 이 실험이 증명하지 "않는" 것

- 초기 Grant 발급 시점의 revoked 거부는 여전히 raw error(범위 밖).
- `EXPIRED`(`DoD-26`)에 대한 같은 종류의 signed outcome은 만들지
  않았다.
- 재접속(reconnect) 시나리오의 REVOKED 전달은 다루지 않는다 —
  완전히 새 프로세스의 초기 발급 거부(`DoD-25`)로만 처리된다.

## 결정

1. 갱신 경로에서 revoked Lease 거부를 서명된 정책 결과로 바꿔
   `DoD-25` 가 남긴 관측성 공백을 닫았다 — 구현은 코덱스 CLI
   (workspace-write)에 위임했다.
2. 오늘 이미 두 번 나온 outcome-분기 교착 패턴을 구현자가 스스로
   의식해 처음부터 올바르게 구현했다 — 독립 검수 1라운드 만에
   `ACCEPTED`.

관련: `docs/evidence/DoD-25_coordinator_lease_revoke_영속화.md` ·
`docs/evidence/DoD-23_lease_재발급_정책_superseded.md`(같은 부류의
교착 버그가 처음 나왔던 조각) ·
`docs/plans/2026-08-20_0357_revoked_signed_outcome_v1.md`
