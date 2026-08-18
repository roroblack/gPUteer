---
schema_version: 2
id: DoD-15
claim: "Coordinator 와 Agent 가 같은 TCP 연결에서 RenewLeaseRequest/RenewLeaseResult 왕복을 N 회(N=3 으로 실측) 반복할 수 있다. 각 회차는 독립적으로 서명·검증되고, 회차별로 분리된 nonce(lease_id + round 유도)를 써서 InMemoryReplayGuard 의 Duplicate 거부를 피한다 — 설계 단계에서 코드 경로로 확정한 대로, round 를 nonce 입력에 섞지 않으면 두 번째 왕복부터 반드시 실패한다"
status: PASS
commit: bc58623db007988aab401eb56fcc1255ef80b8ac

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
review_scope: "derive_renew_nonce(lease_id, round) 회차 분리, Coordinator/Agent 갱신 블록 반복화(off-by-one·do_renew=false 경계 포함), selftest 시나리오 19 판정 로직(부분 문자열 버그 수정 포함), 뮤테이션 비공허성, 기존 시나리오 1~18 회귀. 1라운드 — ACCEPTED, 추가 후속 검수 불필요"
review_artifact: "docs/evidence/_raw/DoD-15_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-15_selftest_2026-08-19.txt"
raw_output_digest: "sha256:4fb98028b91134107f704a43508866686701fc5bd8c05bb2544fbd13dd84fce3"
raw_output_bytes: 21097

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1"
  cli_bin: "target/debug/gputeer.exe (dev profile, commit bc58623 에서 빌드)"
protocol_versions:
  schema_version: "해당 없음 — 이 조각은 Protocol 서명 대상 메시지를 바꾸지 않는다. renew_rounds 는 CLI/설정값이지 서명 필드가 아니다. round 는 nonce 유도 입력일 뿐 RenewLeaseRequest.nonce 필드 자체의 구조는 그대로다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "127.0.0.1 실제 TCP 소켓, 두 개의 별도 OS 프로세스 사이 — DoD-11~14 와 같은 handshake 위에 이어 붙인 같은 연결에서 왕복을 반복한다(별도 재접속 없음)"
command: |
  cargo build --workspace
  cargo test --workspace
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 19개 시나리오, 5회 연속
raw_output: |
  (docs/evidence/_raw/DoD-15_selftest_2026-08-19.txt 전문 참조)

  19) 같은 연결에서 3회 정상 갱신 확인 (회차별 nonce 분리로 replay guard 의
      Duplicate 거부를 피한다 — 성공 자체가 증거다)

  5회 연속 전부 exit=0 (시나리오 1~19 전부). cargo test --workspace: 47개 테스트
  스위트 전체 통과, 0 failed.
artifacts:
  - docs/plans/2026-08-19_2330_같은_연결_반복_lease_갱신_v1.md
  - crates/agent/src/lib.rs
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-15_selftest_2026-08-19.txt
  - docs/evidence/_raw/DoD-15_review.txt
negative_tests:
  - "★ 설계 단계에서 실측으로 확정한 핵심 위험: RenewLeaseRequest.nonce 가 lease_id 에서만 결정적으로 유도되면(기존 derive_nonce(tag, id)), 정상 갱신 중 lease_id 가 절대 바뀌지 않으므로 매 회차 nonce 가 동일하다. InMemoryReplayGuard 의 키는 (signer_id, domain, nonce) 라서 두 번째 요청은 read_frame() 내부 검증에서 Duplicate 로 거부된다 — 추측이 아니라 코드 경로상 확정되는 동작이었다(설계 p105)"
  - "뮤테이션으로 비공허성 확인: derive_renew_nonce(lease_id, round) 를 derive_renew_nonce(lease_id, 0)(round 고정)으로 무력화하자 시나리오 19 가 정확히 예상대로 실패했다 — 1회차는 정상 처리됐지만 2회차 요청이 1회차와 같은 nonce 를 써서 Coordinator 가 'RenewLeaseRequest 프레임 읽기/검증 실패: 메시지 검증 실패: Outcome(Replay)' 로 거부했다. 원복 후 5회 연속 재통과"
  - "회차별 독립 검증: 매 회차 nested 새 Lease 를 outer 결과 서명과 무관하게 독립 검증하고(기존 규칙 i 계약 재사용), request_nonce echo 대조, epoch 단조성 확인을 반복한다 — 반복 갱신이 기존 단일 왕복의 검증 게이트를 우회하지 않는다"
  - "기존 시나리오 1~18 회귀 없음: renew_rounds 기본값 1일 때 기존 단일 왕복 시나리오들이 코드 경로상 정확히 이전과 동일하게 동작한다(반복문 도입 = for round in 0..1, 사실상 무변화). do_renew==false 인 시나리오는 range 가 0..0 이 되어 갱신 블록 자체를 건너뛴다 — 기존과 동일"
  - "경계 동작(코덱스 검수가 확인): renew_rounds=0 은 정확히 0회 왕복 후 최종 RESULT 로 종료. u32::MAX 근처 값도 range 계산·u64 변환에서 overflow 없음"
limitations:
  - "이 조각은 고정 횟수(N)를 CLI 인자로 미리 정한 반복만 증명한다 — 갱신 주기 타이머·조건부 반복(만료 임박 시에만 갱신 등)은 범위 밖이다"
  - "자동 retry/backoff, 별도 재접속(failover)은 범위 밖이다 — 같은 TCP 연결이 반복 전체를 유지해야 한다"
  - "시나리오 20(중간 회차에서 epoch 강등)은 의도적으로 생략했다 — Coordinator 에 회차별 epoch 배열을 추가하면 범위가 커진다고 판단했고, 계획서 자체가 이 선택지를 미리 허용했다. 낮은/높은 epoch 거부 자체는 기존 시나리오 11·15가 단일 회차로 이미 증명한다 — 이 조각은 '반복 중에도 그 게이트가 살아있는가'를 회차 3 전부 같은 epoch 로만 확인했다"
  - "max_total_duration_seconds 정책 집행, 모든 RenewOutcome 의 운영 의미, Lease revoke, 실제 Coordinator 재발급 정책은 여전히 범위 밖이다"
  - "Coordinator 의 영속 Lease 저장소는 범위 밖이다 — 이 조각은 반복 왕복 메커니즘만 다루고, config.fence_epoch 정적 비교라는 기존 한계는 그대로 남아 있다(다음 후보로 이미 설계 완료, docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md)"
  - "다중 Agent·scheduler·HA·TLS 는 범위 밖이다"
  - "Windows 단일 플랫폼에서만 실행했다"
  - "코덱스 검수는 read-only 샌드박스에 cargo 가 없어 cargo build/test/coordinator-agent-selftest 를 직접 실행하지 못했다 — 기존 target/debug/gputeer.exe 도 현재 커밋보다 이전 산출물이라 검증에 쓰지 않았다고 명시했다. 실행 검증은 claude-code 세션이 직접 수행했다(docs/evidence/_raw/DoD-15_selftest_2026-08-19.txt)"
decision: "같은 연결에서 반복 Lease 갱신이 계획대로 구현·검증됐다. docs/plans/2026-08-19_2330_같은_연결_반복_lease_갱신_v1.md 의 단계 1~8, DoD 체크박스 전부 충족(시나리오 20은 계획이 허용한 대로 의도적으로 생략). 설계 단계(코덱스 p105)가 구현 착수 전에 코드 경로로 확정한 nonce 충돌 위험을 실제로 고쳤고, 코덱스 독립 검수가 1라운드 만에 ACCEPTED — 이번 조각은 코덱스 1라운드 검수에서 추가 결함 없이 통과한 사례로 기록한다(이전 DoD-13·14 는 각 2라운드가 필요했다). 다음 후보(Coordinator 영속 Lease 저장소)는 이미 설계까지 완료된 상태로 대기 중이다(docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md)."
---

# DoD-15 · 같은 연결에서 반복 Lease 갱신

## 무엇을 입증하려 했는가

`DoD-13`(Lease 갱신 최소 조각)과 `DoD-14`(durable FenceWatermark)
둘 다 "Out" 절에 "여러 번 반복 갱신"을 공통으로 남겼다 — 지금까지
Coordinator/Agent 는 Grant/ACK 뒤 같은 TCP 연결에서
`RenewLeaseRequest`/`RenewLeaseResult` **1회 왕복만** 처리하고
연결을 끝냈다.

## ★ 설계 단계에서 확정한 핵심 위험

코덱스 설계(`p105`)가 코드를 직접 읽어 확정한 결과다 — **지금
구조에 반복문만 감싸면 두 번째 왕복이 100% 실패한다.**

```text
Agent 의 RenewLeaseRequest.nonce = derive_nonce("lease-renew", &held_lease.lease_id)

lease_id 는 정상 갱신 중 절대 바뀌지 않는다
  (Coordinator 가 항상 config.lease_id 그대로 발급, Agent 도
   lease_id 가 바뀌면 거부)

-> 매 회차의 요청 nonce 가 전부 동일하다
-> InMemoryReplayGuard 의 키는 (signer_id, domain, nonce)
-> 두 번째 RenewLeaseRequest 는 read_frame() 내부 검증에서
   Duplicate 로 거부된다(추측이 아니라 코드 경로상 확정되는 동작)
```

`RenewLeaseResult` 도 같은 문제 — `request_nonce` 를 그대로 echo
하므로 요청 nonce 를 안 바꾸면 결과 nonce 도 회차마다 같아 Agent
의 결과 replay 검사에서도 `Duplicate` 가 된다.

## 구현 개요

1. `derive_renew_nonce(lease_id, round)` — `"lease-renew" || 0x00 ||
   lease_id || 0x00 || round_u64_be` 를 해시해 회차별로 분리된 16
   바이트 nonce 를 만든다. 기존 `derive_nonce(tag, id)` 는 건드리지
   않는다(다른 호출부에 영향 없음).
2. `AgentConfig`/`CoordinatorConfig` 에 `renew_rounds: u32` 추가,
   양쪽 CLI 에 `--renew-rounds`(기본값 1). `do_renew` 블록 전체를
   `for round in 0..renew_rounds`(단, `do_renew == false` 면 `0..0`)
   로 감쌌다 — 기본값 1이면 기존 단일 왕복과 완전히 동일하게
   동작한다(회귀 없음).
3. Coordinator 는 요청의 nonce 를 그대로 echo 할 뿐 스스로 회차를
   유도하지 않는다 — 회차별 nonce 분리는 Agent 가 요청을 만들 때만
   책임진다.
4. `coordinator-agent-selftest` 시나리오 19 — 같은 연결에서 3회
   정상 갱신. Agent/Coordinator 의 `RENEW_RESULT` 출력이 정확히 3회,
   최종 `RESULT ok=true` 가 정확히 1회임을 확인한다(줄 단위 정확한
   매칭 — `"RESULT ok=true"` 가 `"RENEW_RESULT ok=true"` 의 부분
   문자열이라는 점을 구현 중 직접 발견해 `starts_with()` 로 고쳤다).

## 결과

```text
gputeer coordinator-agent-selftest   19개 시나리오(DoD-11~14 의 18 + 이 조각의 1) — 5회 연속 전부 통과
cargo test --workspace               47개 테스트 스위트 전체 통과, 0 failed
```

### 뮤테이션으로 비공허성을 확인했다

`derive_renew_nonce(lease_id, round)` 를 `derive_renew_nonce(lease_id,
0)`(round 고정)으로 무력화하자 시나리오 19 가 정확히 예상대로
실패했다 — 1회차는 정상 처리됐지만 2회차 요청이 1회차와 같은
nonce 를 써서 Coordinator 가 `RenewLeaseRequest 프레임 읽기/검증
실패: 메시지 검증 실패: Outcome(Replay)` 로 거부했다. 설계 단계가
예측한 정확히 그 실패 양상이다. 원복 후 5회 연속 재통과했다.

## 코덱스 검수 — 1라운드 만에 ACCEPTED

이전 두 조각(`DoD-13`·`DoD-14`)은 각각 코덱스 1라운드에서
`CHANGES_REQUESTED` 를 받아 2라운드가 필요했다. 이번 조각은 설계
단계에서 핵심 위험(nonce 충돌)을 미리 코드 경로로 확정해 뒀고,
구현이 그 설계를 정확히 따랐기 때문인지 1라운드 검수에서 추가
결함 없이 `ACCEPTED` 를 받았다 — 경계 동작(0회·`u32::MAX` 근처)과
nonce 직렬화 구분자까지 검토했지만 지적 사항이 없었다.

## 이 실험이 증명하지 "않는" 것

- **조건부/무기한 반복.** 고정 횟수 `N` 을 CLI 인자로 미리 정한
  반복만 증명한다 — 갱신 주기 타이머는 범위 밖이다.
- **자동 retry/backoff·재접속(failover).** 같은 연결이 반복 전체를
  유지해야 한다.
- **시나리오 20(중간 회차 강등)은 의도적으로 생략했다** — 계획서가
  이 선택지를 미리 허용했다. 낮은/높은 epoch 거부 자체는 기존
  시나리오 11·15가 단일 회차로 이미 증명한다.
- **Coordinator 의 영속 Lease 저장소.** `config.fence_epoch` 정적
  비교라는 기존 한계는 그대로 남아 있다.
- **다중 Agent·scheduler·HA·TLS.**
- Windows 단일 플랫폼.

## 결정

1. 같은 연결에서 반복 Lease 갱신이 계획대로 완료됐다 —
   `docs/plans/2026-08-19_2330_같은_연결_반복_lease_갱신_v1.md` 의
   단계 1~8, DoD 체크박스 전부 충족.
2. 코덱스 1라운드 검수에서 추가 결함 없이 통과한 사례로 기록한다 —
   설계 단계의 실측이 구현 단계의 재작업을 줄인 경우다.
3. 다음 후보(Coordinator 영속 Lease 저장소)는 이미 설계까지 완료된
   상태로 대기 중이다.

관련: `docs/plans/2026-08-19_2330_같은_연결_반복_lease_갱신_v1.md` ·
`docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md` ·
`docs/evidence/DoD-13_coordinator_agent_lease_갱신_최소_조각.md` ·
`docs/evidence/DoD-14_durable_fence_watermark.md`
