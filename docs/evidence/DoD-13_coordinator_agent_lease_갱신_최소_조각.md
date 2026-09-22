---
schema_version: 2
id: DoD-13
claim: "Coordinator 와 Agent 가 같은 TCP 연결에 이어서 서명된 RenewLeaseRequest/RenewLeaseResult 왕복으로 Lease 를 갱신한다. RenewLeaseResult 는 새로 서명 대상 메시지로 승격됐다(이전에는 서명 필드가 없어 SUPERSEDED/QUARANTINED 같은 정책 거부를 누구나 위조할 수 있었다). Agent 는 결과 서명·request_nonce echo·nested 새 Lease 독립 서명·epoch 단조성(낮은 epoch 거부 + 높은 epoch 도 이 조각 범위에서는 정책상 거부)을 전부 확인한 뒤에만 보유 Lease 를 교체한다. Coordinator 도 요청의 fence_epoch 을 자신이 기억하는 값과 대조해 거부한다"
status: PASS
commit: 666cf10cb2acd7be79da9e4cc053d29d03b6f26e

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-19T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "RenewLeaseResult 서명화(Protocol)·framed_ingress dispatch(Crypto)·Coordinator/Agent 갱신 왕복·coordinator-agent-selftest 시나리오 7~16·epoch 계약(낮은 epoch 거부·높은 epoch 거부·Coordinator 요청 epoch 대조). 2라운드 — 1라운드 CHANGES_REQUESTED(epoch 계약 결함 2건 + negative test 서술 부정확), 코드 수정 후 2라운드 좁은 후속 검수 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-13_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-13_selftest_2026-08-19.txt"
raw_output_digest: "sha256:5d30605cb508ed43ad6d1a0114f0c1a28bbf80f9ec472b9614a3e5e9c4e8ffb7"
raw_output_bytes: 18853

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1"
  cli_bin: "target/debug/gputeer.exe (dev profile, commit 666cf10 에서 빌드)"
protocol_versions:
  schema_version: "1 (RenewLeaseResult 의 인증 필드는 이번에 처음 추가됐다 — 이전 버전과의 마이그레이션 대상이 없어 schema_version=1 로 시작)"
  canonical_spec: "docs/protocol/signing.md v1 (§5 domain_tag 25종째 gputeer/v1/lease-renew-result, §6 규칙 i)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "127.0.0.1 실제 TCP 소켓, 두 개의 별도 OS 프로세스 사이 — DoD-11·DoD-12 와 같은 handshake 위에 이어 붙인 같은 연결"
command: |
  cargo build --workspace
  cargo test --workspace
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 16개 시나리오, 5회 연속
raw_output: |
  (docs/evidence/_raw/DoD-13_selftest_2026-08-19.txt 전문 참조)

  7) 정상 Lease 갱신 성공 (같은 epoch 유지, Agent 가 새 Lease 를 독립 검증)
  8) 위조 RenewLeaseRequest.node_signature 거부 확인 (Coordinator ingress 검증 실패)
  9) 위조 RenewLeaseResult.coordinator_signature 거부 확인 (Agent 결과 서명 검증 실패)
  10) 위조 nested 새 Lease 서명 거부 확인 (Agent 가 갱신된 Lease 를 outer 결과와 독립적으로 검증한다)
  11) epoch 강등 거부 확인 (FenceWatermark.check_and_advance 가 낮은 epoch 를 거부)
  12) RENEW_OUTCOME_SUPERSEDED 서명된 정상 정책 거부로 분류 확인 (Lease·watermark 불변)
  13) RENEW_OUTCOME_QUARANTINED 서명된 정상 정책 거부로 분류 확인 (Lease·watermark 불변)
  14) RenewLeaseResult.request_nonce 불일치 거부 확인 (응답이 다른 요청에 재사용되는 것을 방지)
  15) epoch 상승 거부 확인 (계획서 범위 — FenceWatermark 만으로는 안 잡히고 명시적 검사가 필요하다)
  16) RenewLeaseRequest.fence_epoch 불일치 거부 확인 (Coordinator 가 요청 epoch 을 자신이 기억하는 값과 대조한다)

  5회 연속 전부 exit=0 (시나리오 1~16 전부). cargo test --workspace: 전체 통과, 0 failed.
artifacts:
  - docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md
  - proto/lease.proto
  - crates/protocol/src/canonical.rs
  - crates/protocol/src/to_fields.rs
  - crates/protocol/src/signable.rs
  - crates/protocol/tests/t1_signing_targets.rs
  - crates/protocol/tests/t1b_grant_and_control.rs
  - crates/protocol/tests/field_number_audit.rs
  - crates/protocol/tests/lifetime_consistency.rs
  - tools/canonical/reference_canonical.py
  - tests/vectors/canonical_v1.json
  - crates/crypto/src/framed_ingress.rs
  - crates/crypto/tests/framed_ingress.rs
  - crates/coordinator/src/lib.rs
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/protocol/signing.md
  - docs/evidence/_raw/DoD-13_selftest_2026-08-19.txt
  - docs/evidence/_raw/DoD-13_review.txt
negative_tests:
  - "위조 RenewLeaseRequest.node_signature: Agent 가 서명 직후 마지막 바이트를 뒤집어 보낸다(corrupt_renew_request_signature). Coordinator 의 read_frame/decode_and_verify 단계에서 거부되고, 그 이유(\"RenewLeaseRequest 프레임 읽기/검증 실패\")가 stderr 에 남는다 — 성공 여부뿐 아니라 이유까지 검사한다(시나리오 8)"
  - "위조 RenewLeaseResult.coordinator_signature: Coordinator 가 서명 직후 마지막 바이트를 뒤집어 보낸다(corrupt_renew_result_signature). Agent 의 read_frame 단계에서 거부된다(시나리오 9)"
  - "위조 nested 새 Lease 서명: Coordinator 가 새 Lease 를 독립적으로 서명한 뒤 그 서명만 뒤집어 outer RenewLeaseResult 에 실어 보낸다(corrupt_renewed_lease_signature). 규칙 i 에 따라 nested Lease.coordinator_signature 는 outer RenewLeaseResult.coordinator_signature 계산에 들어가지 않으므로 outer 결과 서명은 여전히 유효한 채로 Agent 에 도달한다 — Agent 가 새 Lease 를 outer 결과와 독립적으로 verify() 해야만 이 위조를 잡는다(시나리오 10). ★ 뮤테이션으로 비공허성 확인 — 독립 검증 호출을 무력화하면 시나리오 10 이 정확히 예상대로 실패한다(원복 후 재통과)"
  - "epoch 강등: Coordinator 가 요청보다 낮은 fence_epoch 로 새 Lease 를 발급한다(renewed_fence_epoch < 보유 epoch). Agent 의 FenceWatermark.check_and_advance() 가 <  를 거부한다(시나리오 11, DoD-12 의 것과 같은 계약 재사용)"
  - "epoch 상승: Coordinator 가 요청보다 높은 fence_epoch 로 새 Lease 를 발급한다(renewed_fence_epoch > 보유 epoch). FenceWatermark 자체는 >  를 정상 전진으로 통과시키므로(계획서가 명시한 \"이 조각에서는 정책상 거부\"를 강제하지 못한다), Agent 가 watermark 호출 앞에 별도로 new_lease.fence_epoch > held_lease.fence_epoch 검사를 두어 명시적으로 거부한다(시나리오 15). ★ 이 게이트는 코덱스 1라운드 검수(p99)가 실제 결함으로 지적해 추가됐다 — 원래 구현은 이를 놓치고 있었다. 뮤테이션으로 비공허성 확인"
  - "RenewLeaseRequest.fence_epoch 불일치: Agent 가 보유 중인 실제 epoch 대신 임의의 값(99)을 요청에 채운다(renew_request_epoch_override). Coordinator 가 자신이 기억하는 값(config.fence_epoch, 최초 발급 시 준 값)과 대조해 거부한다(시나리오 16). ★ 이 게이트도 코덱스 1라운드 검수가 지적해 추가됐다 — 원래 구현은 요청의 epoch 을 아무것도와 비교하지 않았다. 뮤테이션으로 비공허성 확인"
  - "RenewLeaseResult.request_nonce 불일치: Coordinator 가 요청의 nonce 를 echo 하지 않고 다른 값으로 채운다(corrupt_renew_result_nonce, 서명은 정상). Agent 가 자신이 보낸 요청의 nonce 와 대조해 거부한다(시나리오 14) — 안 그러면 다른 갱신 요청에 대한 결과가 재사용될 수 있다. ★ 계획서의 6종 negative test 표에는 없었지만 구현 중 발견해 추가했다. 뮤테이션으로 비공허성 확인"
  - "RENEW_OUTCOME_SUPERSEDED/QUARANTINED: Coordinator 가 test-only 플래그(renew_outcome_override)로 값을 강제 주입한다 — 실제 정책 결정 로직은 없다(범위 밖). Agent 는 이를 Err(\"RENEW_REFUSED:...\")로 분류하고 Lease·watermark 를 바꾸지 않는다(시나리오 12·13)"
  - "서명 검증 순서: 위 negative test 전부 read_frame/decode_and_verify(서명 검증) 뒤에만 request_nonce·상관관계·epoch 비교를 한다 — 검증 전 필드를 신뢰하지 않는다(CLAUDE.md §0.2)"
limitations:
  - "★ FenceWatermark 는 Agent 프로세스 로컬 메모리 상태다(DoD-12 와 같은 한계) — 재시작 후 stale epoch 차단은 이 evidence 가 증명하지 않는다"
  - "여러 번 반복 갱신은 범위 밖이다 — 이 조각은 1회 왕복만 증명한다(같은 연결에 두 번째 RenewLeaseRequest 를 보내는 시나리오는 없다)"
  - "갱신 주기 타이머·자동 retry/backoff·별도 재접속(failover)은 범위 밖이다 — 같은 TCP 연결만 다룬다"
  - "Coordinator 의 실제 Lease 재발급 정책(어떤 조건에서 SUPERSEDED/QUARANTINED 를 내리는지)은 범위 밖이다 — renew_outcome_override 는 test-only 주입일 뿐이다"
  - "max_total_duration_seconds 정책 집행, 모든 RenewOutcome 케이스의 운영 의미 정의, Lease revoke 는 범위 밖이다"
  - "durable FenceWatermark 는 범위 밖이다 — 재시작을 넘는 epoch 방어는 여전히 증명되지 않는다"
  - "다중 Agent·scheduler·ControlStore·HA·TLS 는 범위 밖이다"
  - "Coordinator 의 '기억하는 epoch'(config.fence_epoch)은 이 stub 이 별도 Lease 저장소를 갖지 않기 때문에 최초 CLI 인자 그대로다 — 실제 Coordinator 는 영속 저장소에서 현재 epoch 을 읽어야 하고, 이 조각은 그 저장소 자체를 구현하지 않는다"
  - "Windows 단일 플랫폼에서만 실행했다"
  - "코덱스 검수 2라운드 모두 read-only 샌드박스에 cargo 가 없어 cargo build/test/coordinator-agent-selftest 를 직접 실행하지 못했다(\"확인 안 됨\") — 실행 검증은 claude-code 세션이 직접 수행했다(docs/evidence/_raw/DoD-13_selftest_2026-08-19.txt)"
decision: "Lease 갱신 최소 조각(RenewLeaseRequest/RenewLeaseResult 왕복, RenewLeaseResult 서명화 포함)이 계획대로 구현·검증됐다. docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md 의 단계 1~8 전부 완료, DoD 체크박스 전부 충족. 코덱스 1라운드 검수가 실제 설계 결함 2건(Coordinator 의 요청 epoch 미검증, Agent 의 epoch 상승 미거부)을 찾아냈고 둘 다 코드로 고쳤다 — 완료로 보고하기 전에 독립 검수가 실제로 결함을 잡아낸 사례다. 다음 후보(반복 갱신, 재접속, durable FenceWatermark, Coordinator 의 실제 Lease 저장소·정책 엔진, 다중 Agent)는 이 계획 문서의 Out 절이 이미 명시했다 — 각각 새 계획 문서가 필요하다."
---

# DoD-13 · coordinator/agent Lease 갱신 최소 조각

## 무엇을 입증하려 했는가

`DoD-12`(Lease 최소 조각)는 Coordinator 가 서명된 `Lease` 를 `ExecutionGrant`
에 실어 발급하고 Agent 가 독립 검증하는 것까지만 증명했다 — lease **갱신**
(`RenewLeaseRequest` 왕복)은 명시적으로 범위 밖에 남겨졌다.

설계 단계(`docs/plans/2026-08-19_0500_...v1.md`, 코덱스 설계 `p98b`)에서
가장 중요한 발견은 **`RenewLeaseResult` 가 서명 대상이 아니었다**는
것이다 — `RenewLeaseRequest`(agent 서명)는 이미 `Signable` 이었지만,
응답인 `RenewLeaseResult` 에는 서명 필드 자체가 없었다. `SUPERSEDED`/
`QUARANTINED` 같은 정책 거부를 누구나 위조해 정당한 Agent 의 작업을
강제 중단시킬 수 있는 상태였다. 이 조각의 In 범위에 `RenewLeaseResult`
를 서명 대상으로 만드는 작업을 포함시켰다.

## 구현 개요

1. **Protocol** — `RenewLeaseResult` 에 인증 필드
   (`schema_version`(5)·`coordinator_id`(6)·`issued_at_unix_ms`(7)·
   `request_nonce`(8, `RenewLeaseRequest.nonce` 를 echo)·
   `coordinator_signature`(90))를 추가하고 새 domain_tag
   `gputeer/v1/lease-renew-result` 를 등록했다(`RenewLeaseRequest` 와
   tag 를 공유하지 않는다 — 공유하면 요청 서명이 응답 검증도 통과해
   교차 재생이 가능해진다). `ToCanonicalFields`/`Signable` 을 구현하고,
   Python 참조 구현(`tools/canonical/reference_canonical.py`)에도
   같은 스키마·domain_tag 를 추가해 벡터 3건(전 필드·nonce 변형·
   nested Lease 서명 제외 확인)을 생성, Rust 인코딩과 바이트 단위로
   대조했다(`renew_lease_result_matches_reference`) — `AgentGrantAck`
   때 발견했던 "Python 참조 구현과 한 번도 대조된 적 없는" 공백을
   이번에는 미리 메웠다.
2. **Crypto** — `framed_ingress.rs` 에 `FrameType::LeaseRenewResult`
   (11)·`IngressMessage::LeaseRenewResult` 를 추가했다.
3. **Coordinator** — ACK 처리 뒤 같은 TCP 연결에 이어서
   `RenewLeaseRequest` 를 읽어(서명은 `read_frame` 이 검증) `node_id`·
   `lease_id`·`fence_epoch` 상관관계를 확인한 뒤, 정상이면 같은
   epoch 의 새 `Lease` 를 독립적으로 서명해 `RenewLeaseResult` 에
   담아 서명 후 돌려준다. test-only 플래그로 SUPERSEDED/QUARANTINED
   강제 주입, 각 서명 위조, `request_nonce` 오염을 재현한다.
4. **Agent** — `RenewLeaseRequest` 를 서명해 보내고
   `RenewLeaseResult` 를 받아: 서명 검증(`read_frame`) → `replay_nonce`
   확인 → `request_nonce` 가 자신이 보낸 요청과 일치하는지 확인 →
   `outcome==RENEWED` 면 nested 새 `Lease` 를 **outer 결과 서명과
   독립적으로** `verify()` → `lease_id`/`job_id`/`attempt_id`/
   `issuing_coordinator_id`/`holder_node_id` 상관관계 확인 → **epoch
   상승 명시적 거부** → `FenceWatermark.check_and_advance()`(같은
   `job_id` 를 resource key 로 재사용) → 전부 통과해야만 보유 Lease
   교체.
5. **CLI** — `coordinator-agent-selftest` 에 시나리오 7~16 추가(기존
   1~6 은 DoD-11/12 의 핸드셰이크·Lease 최소 조각 그대로 유지).

## 코덱스 1라운드 검수가 실제로 잡은 결함 2건

정적 검수(`p99`)가 지적했다:

- Coordinator 가 `renew_req.fence_epoch` 을 **아무것도와 비교하지
  않았다** — 서버가 요청의 현재 epoch 을 확인하지 않은 채 갱신을
  진행했다.
- `FenceWatermark.check_and_advance()` 는 `<` 만 거부하고 `>` 는 정상
  전진으로 통과시킨다. 계획서는 "epoch **상승**은 이 조각에서는
  정책상 거부"라고 명시했는데, Agent 는 watermark 호출 하나에만
  기대고 있어서 실제로는 epoch 상승 갱신이 **통과했다**.

둘 다 코드 결함이었다 — 계획 문서가 요구한 계약과 구현이 어긋나
있었다. Coordinator 에 `renew_req.fence_epoch != config.fence_epoch`
검사를, Agent 에 `new_lease.fence_epoch > held_lease.fence_epoch`
검사(watermark 호출 **앞**)를 각각 추가해 고쳤다. 시나리오 15·16 을
추가해 각각 뮤테이션 테스트로 비공허성을 확인한 뒤, 좁은 후속
검수(`p100`)에서 `ACCEPTED` 를 받았다.

## 결과

```text
gputeer coordinator-agent-selftest   16개 시나리오(DoD-11/12 의 6 + 이 조각의 10) — 5회 연속 전부 통과
cargo test --workspace               전체 통과, 0 failed
```

### outer 검증만으로는 부족하다는 것을 다시 확인했다

`RenewLeaseResult.lease` 는 규칙 i 에 따라 **독립적으로 서명**된다.
Coordinator 가 nested Lease 서명만 위조해도 outer 결과 서명은 여전히
유효하다 — Agent 가 nested Lease 를 별도로 `verify()` 하지 않으면
위조를 놓친다. DoD-12 가 첫 서명(Grant→Lease)에서 증명한 것과 같은
성질을, 두 번째 서명 경로(RenewLeaseResult→Lease)에서도 다시
확인했다.

### 뮤테이션으로 비공허성을 확인했다 — 4건

- nested Lease 독립 검증 무력화 → 시나리오 10 실패
- `request_nonce` 대조 무력화 → 시나리오 14 실패
- epoch 상승 거부 무력화 → 시나리오 15 실패
- Coordinator 의 요청 epoch 대조 무력화 → 시나리오 16 실패

넷 다 무력화 시 정확히 예상한 시나리오만 실패하고, 원복 후 16개
시나리오 전부 재통과했다(`docs/evidence/_raw/DoD-13_selftest_2026-08-19.txt`
말미의 뮤테이션 로그 참조).

## 이 실험이 증명하지 "않는" 것

- **반복 갱신.** 이 조각은 1회 왕복만 증명한다.
- **갱신 주기 타이머·자동 retry/backoff·재접속(failover).**
- **Coordinator 의 실제 Lease 재발급 정책.** `SUPERSEDED`/
  `QUARANTINED` 는 test-only 주입일 뿐 실제 판단 로직이 없다.
- **durable FenceWatermark.** 재시작을 넘는 epoch 방어는 여전히
  증명되지 않는다(DoD-12 와 같은 한계).
- **Coordinator 의 영속 Lease 저장소.** `config.fence_epoch` 를
  "기억하는 현재 epoch"으로 쓰는 것은 이 stub 이 CLI 인자 하나로
  단일 갱신만 처리하기 때문이다 — 실제 운영에서는 저장소에서 읽어야
  한다.
- **다중 Agent·scheduler·ControlStore·HA·TLS.**
- Windows 단일 플랫폼.

## 결정

1. Lease 갱신 최소 조각이 계획대로 완료됐다 —
   `docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md`
   의 단계 1~8, DoD 체크박스 전부 충족.
2. 독립 검수가 완료 보고 전에 실제 설계 결함 2건을 잡아낸 사례로
   기록한다 — "구현했다"와 "계획서가 요구한 계약을 실제로 지킨다"는
   다르다는 것을 다시 확인했다.
3. 다음 후보(반복 갱신, durable FenceWatermark, Coordinator 의 실제
   Lease 저장소·정책 엔진, 다중 Agent)는 이 계획 문서의 "Out" 절이
   이미 명시했다 — 각각 새 계획 문서가 필요하다.

관련: `docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md` ·
`docs/evidence/DoD-11_coordinator_agent_핸드셰이크.md` ·
`docs/evidence/DoD-12_coordinator_agent_lease_최소_조각.md` ·
`crates/runtime-policy/src/lease_scope.rs`
