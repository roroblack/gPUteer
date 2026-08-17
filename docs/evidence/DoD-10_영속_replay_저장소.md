---
schema_version: 2
id: DoD-10
claim: "DurableReplayGuard 는 프로세스 재시작 후에도 이미 본 nonce 를 Duplicate 으로 거부하고, 미커밋 트랜잭션을 rollback 하며, InMemoryReplayGuard 와 replay_contract.rs 의 9개 시나리오에서 같은 답을 낸다."
status: PASS
commit: e188cce41bd7b223812804d6bac8f69ffaa95377

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-opus-5"
executed_at: "2026-08-17T13:40:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "§10 MUST/MUST NOT · 트랜잭션 원자성 · 시계 방어 · busy_timeout · journal 모드 · 테스트 공허성 · 오류 분류 · 두 구현의 계약 일치"
review_artifact: "docs/evidence/_raw/DoD-10_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-10_durable_replay.txt"
raw_output_digest: "sha256:1b91a428ca31d8978109cc94831b1777582de1e59c00ac18d9364d9293efd1c1"
raw_output_bytes: 11136

binary_digests:
  gputeer-crypto: "cargo test 프로필 (unoptimized + debuginfo) — 배포 바이너리 아님"
  sqlite: "3.46.0 (rusqlite 0.32.1 bundled · libsqlite3-sys 0.30.1)"
protocol_versions:
  schema_version: 1
  signing_md: "§10 (replay) · §11 (키 보관 K0/K1)"
platform: "Windows 11 Pro 10.0.26200 · NTFS · Rust 1.97.1"
hardware: "개발 기계 (Intel Iris Xe · GPU 미사용 — 저장소 로직만 검증)"
network_profile: "해당 없음 (단일 기계 · 로컬 디스크)"
command: "cargo test --workspace && cargo test -p gputeer-crypto --test replay_contract"
raw_output: |
  workspace 전체            229 passed / 0 failed / 빌드 경고 0
  durable_replay            11 passed (1 ignored — 자식 프로세스 진입점)
  replay_contract            9 passed
  keyring                    6 passed

  뮤테이션 D1(영속성 제거) 상태에서 durable_replay 4건 FAILED.
  뮤테이션 K1(Debug 유출) 상태에서 개인키 테스트 FAILED.

  rusqlite bundled 빌드 실측: 14.55초, SQLite 3.46.0.

  ★ 이것은 요약이다. 원문은 raw_output_artifact 에 있고 digest 로 묶여 있다.
artifacts:
  - crates/crypto/src/durable_replay.rs
  - crates/crypto/tests/durable_replay.rs
  - crates/crypto/tests/replay_contract.rs
  - crates/crypto/src/replay.rs
  - docs/evidence/_raw/DoD-10_durable_replay.txt
  - docs/evidence/_raw/DoD-10_review.txt
negative_tests:
  - first_use_is_fresh_and_restart_is_duplicate
  - crash_recovery_discards_uncommitted_partial_record
  - clock_state_survives_restart_and_blocks_bad_gc
  - cache_full_does_not_evict_unexpired_entries
  - invalid_nonce_does_not_consume_a_valid_nonce_slot
  - short_nonce_is_rejected_by_both
  - extreme_gc_time_agrees
  - zero_capacity_is_rejected_by_both
  - "뮤테이션 D1(Connection::open -> open_in_memory) 에서 4건 FAILED — 공허하지 않다"
  - "뮤테이션 K1(Debug 가 바이트 유출) 에서 개인키 테스트 FAILED"
limitations:
  - "★ 아무도 이것을 아직 쓰지 않는다. 검증 경로(coordinator·agent)가 미착수라 DurableReplayGuard 는 어디에도 wiring 되어 있지 않다. '영속 저장소가 있다' 는 '재시작 후 replay 창이 닫혔다' 가 아니다."
  - "★ two_connections_share_state_sequentially (옛 이름 two_connections_race_on_the_same_nonce)는 **경쟁 테스트가 아니다.** 이름은 고쳤다. 두 연결을 만들지만 호출은 순차적이다. 실제 락 경쟁·busy_timeout·LockTimeout 은 검증하지 않았다. 검수자가 지적했고, 이름은 고쳤으나 실제 경쟁 검증은 미수정이다."
  - "★ crash 테스트는 '부분 기록' 이 아니다. 자식이 완전한 INSERT 뒤 commit 없이 죽는다 — 찢어진 row 가 아니다. 미커밋 트랜잭션 rollback 자체는 검사하지만, INSERT 와 commit 사이의 전원 차단이나 torn write 는 검증하지 않았다."
  - "★ LockTimeout 재시도 정책이 없다. 호출자가 재시도할지 거부할지 정하지 않았다. 현재 구현에 fail-open 경로는 없다(Fresh 는 commit 성공 뒤에만 반환). 소비 측이 없어 정책을 정할 근거가 없다."
  - "★ journal_mode=DELETE 를 골랐으나 WAL 과 비교한 근거가 문서에 없다. §10 은 WAL 을 금지하지 않는다. 측정 없이 바꾸지 않는다."
  - "★ 검수자가 이 코드의 **초안을 작성했다.** 프롬프트에 그 사실을 명시하고 '초안을 옹호하지 말라' 고 지시했으며 실제로 결함을 찾았지만, 완전한 독립 검수는 아니다. 다른 모델을 쓸 수 있게 되면 다시 받아야 한다."
  - "성능을 측정하지 않았다. synchronous=FULL 의 쓰기 비용, 대량 GC 의 지연을 모른다."
  - "Linux 에서 한 번도 실행하지 않았다 (D-3). K1(DPAPI)은 Windows 전용이며 Linux 는 UnsupportedPlatform 으로 실패한다."
  - "다중 **프로세스** 동시 접근을 실측하지 않았다. SQLite 가 직렬화한다는 것은 문헌 지식이지 이 환경의 측정이 아니다."
decision: "DurableReplayGuard 를 §10 3단계의 구현으로 채택한다. 소비 측이 생기면 InMemoryReplayGuard 대신 이것을 wiring 한다. 위 미수정 4건은 소비 측 착수와 함께 다룬다."
---

# DoD-10 — 영속 replay 저장소와 키 관리

## 왜 이 실험을 했나

`InMemoryReplayGuard` 는 프로세스가 죽으면 캐시가 빈다.
**재시작 직후 replay 창이 열린다.** `is_durable()` 이 `false` 인 것이 그 신호였다.

`signing.md` §10 은 로컬 SQLite 와 원자적 트랜잭션을 요구한다.

## 의존성을 추정으로 고르지 않았다

`rusqlite` 는 bundled SQLite 를 C 로 컴파일한다. 이 환경(Windows 11 · Rust 1.97.1)에서
C 컴파일러 문제로 못 쓸 수도 있었다.

**먼저 측정했다** — 14.55초, SQLite 3.46.0 동작 확인.
그 뒤에 채택했다.

## 검수가 찾은 가장 중요한 것

**두 구현이 같은 계약을 만족하지 않았다.**

```text
같은 입력                 InMemoryReplayGuard   DurableReplayGuard
──────────────────────────────────────────────────────────────────
15바이트 nonce            Fresh                 Io
retain_until = u64::MAX   Fresh                 Io
gc(u64::MAX)              clamp                 Io
capacity = 0              생성 성공             생성 실패
```

★ **같은 입력에 다른 답을 내는 guard 는 계약이 아니다.**
호출자가 어느 구현을 쓰는지에 따라 replay 방어가 달라진다는 뜻이다.

각 구현을 따로 시험하면 이것은 **영원히 안 보인다.**
`crates/crypto/tests/replay_contract.rs` — 시나리오 한 벌을 두 구현에 똑같이 먹인다.

★ 그 계약 테스트가 **내 기댓값의 오류도 잡았다.**
`retain_until` 2분짜리 항목은 5분 clamp 안에서 정당하게 만료된다.
"극단적 미래 시각이 캐시를 비우면 안 된다" 를 검사하려면
보존 시한이 clamp 상한보다 길어야 한다.

## 거짓말하던 플래그

`is_durable()` 을 처음에 `true` 로 하드코딩했다.

`Connection::open_in_memory()` 로 바꾸는 뮤테이션에서 다른 테스트 4건이 실패했는데
**`is_durable()` 은 계속 `true` 를 반환했다.** 주장이지 사실이 아니었다.

지금은 `Connection::path()` 에서 도출한다. 뮤테이션에서 뒤집힌다.

## 개인키 유출 테스트가 공허했다

초안은 소문자 hex 한 가지만 검사했다.

```rust
// Debug 파생은 이렇게 찍는다
SecretSigningKey(REDACTED)[165, 165, 165, ...]
```

`"a5a5..."` 를 찾는 검사는 이것을 **통과시킨다.** `contains("REDACTED")` 도 만족한다.
4가지 표기를 전부 검사하도록 고쳤고, 뮤테이션이 잡힌다.

## 이 실험이 증명하지 않는 것

- **replay 창이 닫혔다** — 아니다. **아무도 이 저장소를 아직 쓰지 않는다.**
  검증 경로가 미착수라 `DurableReplayGuard` 는 어디에도 연결되어 있지 않다.
- **다중 프로세스에서 안전하다** — 측정하지 않았다. 두 연결을 쓰는 테스트는
  순차 호출이며, 이름도 그렇게 고쳤다.
- **전원 차단에서 안전하다** — 미커밋 rollback 만 검사했다. torn write 는 아니다.
- **키 저장이 안전하다** — K0 는 파일 권한뿐이고, K1(DPAPI)은 Windows 전용이며
  프로세스 메모리·크래시 덤프는 보호하지 않는다. K2(TPM)는 미구현이다.
- **검수가 독립적이었다** — 같은 도구가 초안을 썼다. 한계를 receipt 에 적었다.
