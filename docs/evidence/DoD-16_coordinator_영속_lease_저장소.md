---
schema_version: 2
id: DoD-16
claim: "Coordinator 가 SQLite 파일에 발급한 Lease 의 신원(lease_id/job_id/attempt_id/holder_node_id)과 epoch/만료 시각을 영속해, 프로세스 재시작 후에도 자신이 무엇을 발급했는지 기억한다. --lease-db 를 안 주면(기존 시나리오 전부) 이 조각 이전과 완전히 같은 레거시 경로(그 실행의 CLI 인자만 신뢰)를 그대로 쓴다 — 회귀가 없다. 최초 발급은 저장된 값이 CLI 인자보다 우선하고, 갱신 요청의 fence_epoch 대조도 저장된 값 기준이다"
status: PASS
commit: 334cc52819e119816e2cfbec7518d82d1cb0c32b

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
review_scope: "CoordinatorLeaseStore(SQLite 영속·get_or_issue/renew_existing 계약), Coordinator 최초 발급·갱신 경로의 store 분기(Option 기반, 레거시 경로 회귀 없음), fail-closed(is_durable), selftest 시나리오 20·21 판별력, 뮤테이션 비공허성, u32/u64 경계 변환. 2라운드 — 1라운드 CHANGES_REQUESTED(max_total_duration_seconds 무검사 u32 캐스팅 지적), 코드 수정 후 2라운드 좁은 후속 검수 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-16_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-16_selftest_2026-08-19.txt"
raw_output_digest: "sha256:22a161995ca3ca4160832e5de67f26a2c72144f2d72e91bb82638282c1e33208"
raw_output_bytes: 24669

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1"
  cli_bin: "target/debug/gputeer.exe (dev profile, commit 334cc52 에서 빌드)"
protocol_versions:
  schema_version: "해당 없음 — 이 조각은 Protocol 서명 대상 메시지를 바꾸지 않는다. lease_db_path 는 Coordinator 로컬 설정이지 서명 필드가 아니다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "127.0.0.1 실제 TCP 소켓, 두 개의 별도 OS 프로세스 사이 — DoD-11~15 와 같은 handshake 위에 얹는다. 재시작 시나리오는 Coordinator·Agent 양쪽 모두 별도 프로세스 두 개가 같은 SQLite 파일(각자의 lease-db/fence-db)을 공유한다"
command: |
  cargo build --workspace
  cargo test --workspace
  cargo test -p gputeer-coordinator --lib
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 21개 시나리오, 5회 연속
raw_output: |
  (docs/evidence/_raw/DoD-16_selftest_2026-08-19.txt 전문 참조)

  20) Coordinator 영속 Lease 저장소 — 발급 상태 복원 확인 (별도 프로세스가 SQLite
      파일로 저장된 epoch 를 CLI 의 틀린 값보다 우선시킨다)
  21) Coordinator 영속 Lease 저장소 — 재시작 후 갱신 대조 확인 (별도 프로세스가
      갱신 요청의 fence_epoch 을 저장된 값과 대조한다, 그 실행의 CLI 값이 아니라)

  5회 연속 전부 exit=0 (시나리오 1~21 전부). cargo test --workspace: 47개 스위트
  전체 통과. gputeer-coordinator --lib: 10/10 통과(lease_store 7건 +
  u32_from_stored 경계값 3건).
artifacts:
  - docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md
  - crates/coordinator/Cargo.toml
  - crates/coordinator/src/lease_store.rs
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-16_selftest_2026-08-19.txt
  - docs/evidence/_raw/DoD-16_review.txt
negative_tests:
  - "발급 상태 복원(시나리오 20): 1차 별도 프로세스가 epoch=5 로 정상 발급 + 같은 epoch 갱신 — lease store 에 epoch=5, Agent 의 durable watermark 에도 5 가 남는다(같은 --fence-db 재사용). 2차 별도 프로세스가 의도적으로 틀린 CLI epoch=3 으로 실행돼도, get_or_issue() 가 저장된 값(5)을 그대로 반환해 Grant 도 epoch=5 로 나가 성공한다. ★ 뮤테이션으로 비공허성 확인 — get_or_issue() 반환값을 버리고 candidate 를 그대로 쓰도록 무력화하면, 2차 Grant 가 CLI 의 틀린 epoch=3 을 그대로 실어 보내 Agent 의 durable watermark(5)가 이를 정확히 거부한다(원복 후 재통과)"
  - "재시작 후 갱신 대조(시나리오 21): 1차는 epoch=5 로 발급만 한다. 2차는 의도적으로 틀린 CLI epoch=6 으로 실행되지만, 최초 Grant 는 get_or_issue() 가 저장된 값(5)으로 자동 교정하므로 Agent 는 held_lease.fence_epoch=5 로 갱신 요청을 만든다. Coordinator 가 이 요청을 저장소의 epoch(5)와 대조하면 일치해 갱신이 성공한다. ★ 뮤테이션으로 비공허성 확인 — expected_epoch 를 항상 config.fence_epoch(그 실행의 CLI 값)로 고정하면, 저장된 값(5)과 그 실행의 CLI 값(6)이 달라 정당한 갱신 요청을 잘못 거부한다(원복 후 재통과)"
  - "두 시나리오 모두 Agent 쪽 durable FenceWatermark(DoD-14)의 설계 함정(갱신 전용 시나리오가 실제로는 판별력이 없었던 문제)을 참고해, 'Coordinator 가 스토어를 무시했다면 Agent 쪽에서 관측 가능하게 실패한다'는 조건으로 설계했다 — 존재하지 않는 판별력을 존재하는 것처럼 보고하지 않는다"
  - "u32/u64 경계 변환: max_total_duration_seconds 가 저장소(u64)에서 pb::Lease(u32)로 넘어갈 때 u32_from_stored() 가 fail-closed 로 변환한다 — u32::MAX 는 통과, u32::MAX+1 은 잘리지 않고 명시적으로 거부(단위 테스트 3건)"
  - "identity 충돌 방지(단위 테스트): 같은 lease_id 에 다른 job_id/attempt_id/holder_node_id/issuing_coordinator_id 를 재발급하면 저장된 레코드를 덮어쓰지 않고 IdentityConflict 로 거부한다 — 재발급 정책이 아니라 이미 존재하는 Lease 사실의 훼손 방지"
  - "존재하지 않는 lease_id 의 갱신 요청은 NotFound 로 거부하고 아무것도 쓰지 않는다(단위 테스트) — 갱신 경로에서 새 Lease 를 암묵적으로 발급하지 않는다"
limitations:
  - "이 조각은 새 lease_id 를 언제 발급할지 결정하는 정책, SUPERSEDED/QUARANTINED 판정, Lease revoke 를 결정하지 않는다 — 이미 존재하는 Lease 사실을 훼손 없이 기억만 한다"
  - "여러 Agent 가 같은 Coordinator 를 공유하는 운영 모델, 다중 Coordinator HA·합의는 범위 밖이다"
  - "--lease-db 는 설계(p106)와 달리 의도적으로 optional 로 만들었다 — 기존 19개 시나리오가 config.fence_epoch 정적 비교 레거시 계약에 의존하므로, None(기본값)이면 이 조각 이전과 완전히 같은 레거시 경로를 그대로 쓴다. 저장소를 실제로 쓰는 것은 새 시나리오(20·21)뿐이다 — 프로덕션에서 항상 켜는 것을 이 evidence 가 증명하지는 않는다"
  - "retry/backoff/failover, max_total_duration_seconds 정책 집행, 모든 RenewOutcome 의 운영 의미는 여전히 범위 밖이다"
  - "Windows 단일 플랫폼에서만 실행했다"
  - "코덱스 검수 2라운드 모두 read-only 샌드박스에 cargo 가 없어 cargo build/test/coordinator-agent-selftest 를 직접 실행하지 못했다 — 기존 target/debug/gputeer.exe 도 HEAD 보다 이전 산출물이라 검증에 쓰지 않았다. 실행 검증은 claude-code 세션이 직접 수행했다(docs/evidence/_raw/DoD-16_selftest_2026-08-19.txt)"
decision: "Coordinator 영속 Lease 저장소가 계획대로 구현·검증됐다. docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md 의 단계 1~6, DoD 체크박스 전부 충족. 코덱스 1라운드 검수가 완료 보고 전에 실제 데이터 무결성 결함(max_total_duration_seconds 의 u64→u32 무검사 캐스팅)을 찾아냈고 코드로 고쳤다. Agent 쪽(DoD-14)에서 얻은 교훈(갱신 전용 재시작 시나리오는 판별력이 없다)을 Coordinator 쪽 설계에 미리 반영해 같은 함정을 피했다. 이로써 Coordinator·Agent 양쪽 모두 재시작을 넘는 Lease/epoch 방어를 갖췄다 — 다음 후보(실제 재발급 정책, 다중 Agent, HA)는 이 계획 문서의 Out 절이 이미 명시했다."
---

# DoD-16 · Coordinator 영속 Lease 저장소

## 무엇을 입증하려 했는가

`DoD-14`(durable FenceWatermark)가 Agent 쪽 재시작 방어를 증명한
뒤, Coordinator 쪽은 여전히 대칭적인 방어가 없었다 — 매 프로세스
실행이 CLI 인자로만 Lease 상태를 구성하고 끝나면 사라졌다
(`crates/coordinator/src/lib.rs` 의 `CoordinatorConfig` 가
`lease_id`/`job_id`/`fence_epoch` 등을 직접 보유, 별도 저장소 없음).

## 구현 개요

1. **`crates/coordinator/src/lease_store.rs`(신규)** —
   `CoordinatorLeaseStore`. `DurableFenceWatermark`
   (`crates/runtime-policy/src/durable_lease_scope.rs`)를 재사용하지
   않고 별도 타입으로 만들었다 — watermark 는 `resource -> 최대
   epoch` 하나만 저장하지만 Coordinator 는 Lease 전체 신원을
   복원해야 하기 때문이다. SQLite 연결·`BEGIN IMMEDIATE` 트랜잭션·
   PRAGMA·에러 매핑 **패턴만** 재사용했다. `get_or_issue()` 는
   레코드가 없으면 후보를 그대로 삽입하고, 있으면 identity
   (`job_id`·`attempt_id`·`holder_node_id`·`issuing_coordinator_id`)
   를 대조해 다르면 `IdentityConflict` 로 거부하며, 같으면 **저장된
   값을 반환**한다(CLI 값이 아니라). `renew_existing()` 은 레코드가
   없으면 `NotFound`, 있으면 identity·epoch 는 그대로 두고
   `expires_at`/`renew_after` 만 갱신한다.
2. **`crates/coordinator/src/lib.rs`** — `lease_db_path:
   Option<PathBuf>` 를 추가했다. **설계(`p106`)와 달리 의도적으로
   `Option`** 으로 만들었다 — 기존 19개 시나리오가 이미
   `config.fence_epoch` 정적 비교 레거시 계약에 의존하므로,
   `None`(기본값)이면 이 조각 이전과 완전히 같은 경로를 그대로
   쓴다(회귀 없음). `Some` 이면 `TcpListener::bind()` 보다 먼저
   저장소를 열고(fail closed), 최초 발급·갱신 양쪽 경로가 저장소를
   쓴다.
3. **`coordinator-agent-selftest`** 시나리오 20(발급 상태 복원)·
   21(재시작 후 갱신 대조) — 둘 다 Coordinator 와 Agent **양쪽**
   프로세스가 같은 SQLite 파일(각자 `--lease-db`/`--fence-db`)을
   공유하고, 2차 프로세스에 **의도적으로 틀린 CLI epoch** 을 줘서
   "Coordinator 가 스토어를 무시했다면 Agent 쪽에서 관측 가능하게
   실패한다"는 조건으로 설계했다.

## 코덱스 1라운드 검수가 잡은 결함

`StoredLease.max_total_duration_seconds` 는 `u64` 로 저장되지만
`pb::Lease` 의 같은 필드는 `uint32` 다. 복원 시 `as u32` 로
무검사 캐스팅해, `u32::MAX` 를 넘는 값이 저장소에 들어오면 조용히
잘릴 수 있었다 — 지금 발급 경로는 항상 `86_400` 을 쓰므로 당장
재현되지는 않지만, 저장소 API 는 임의의 `u64` 를 저장할 수 있으므로
진짜 데이터 무결성 결함이었다. `u32_from_stored()` fail-closed
헬퍼로 고치고 경계값 단위 테스트 3건(정상 최대값·`u32::MAX`·
`u32::MAX+1` 거부)을 추가한 뒤 2라운드 검수에서 `ACCEPTED` 를
받았다.

## 결과

```text
gputeer coordinator-agent-selftest   21개 시나리오(DoD-11~15 의 19 + 이 조각의 2) — 5회 연속 전부 통과
cargo test --workspace               47개 테스트 스위트 전체 통과, 0 failed
gputeer-coordinator --lib            10/10 통과(lease_store 7건 + u32_from_stored 경계값 3건)
```

### 뮤테이션으로 비공허성을 확인했다 — 2건

- 최초 발급 경로에서 `get_or_issue()` 의 반환값을 버리고 candidate
  를 그대로 쓰도록 무력화하자, 시나리오 20 이 정확히 예상대로
  실패했다(2차 프로세스가 틀린 CLI epoch 을 그대로 Grant 에 실어
  보내 Agent 의 durable watermark 가 거부).
- 갱신 대조에서 `expected_epoch` 를 항상 `config.fence_epoch` 로
  고정하자, 시나리오 21 이 정확히 예상대로 실패했다(저장된 값과
  그 실행의 CLI 값이 달라 정당한 갱신을 잘못 거부).

두 뮤테이션 모두 원복 후 5회 연속 재통과했다.

### Agent 쪽 교훈을 미리 반영했다

`DoD-14`(durable FenceWatermark) 구현 중 "갱신 경로 전용" 재시작
시나리오가 실제로는 판별력이 없는 거짓양성이었다는 것을 뮤테이션
테스트로 뒤늦게 발견한 적이 있다. 이번 조각은 그 교훈을 설계
단계에서 미리 반영했다 — 시나리오 20·21 을 "Coordinator 가
스토어를 무시했다면 Agent 쪽에서 관측 가능하게 실패한다"는 조건으로
처음부터 설계해, 같은 함정에 빠지지 않았다(뮤테이션 테스트가 실제로
그 판별력을 확인했다).

## 이 실험이 증명하지 "않는" 것

- **새 `lease_id` 발급 정책, `SUPERSEDED`/`QUARANTINED` 판정, Lease
  revoke.** 이 저장소는 이미 존재하는 Lease 사실을 훼손 없이
  기억만 한다.
- **여러 Agent 공유, 다중 Coordinator HA·합의.**
- **`--lease-db` 상시 사용.** `Option` 으로 남겨 뒀다 — 저장소를
  실제로 쓰는 것은 새 시나리오뿐이다.
- **retry/backoff/failover, `max_total_duration_seconds` 정책
  집행.**
- Windows 단일 플랫폼.

## 결정

1. Coordinator 영속 Lease 저장소가 계획대로 완료됐다 —
   `docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md`
   의 단계 1~6, DoD 체크박스 전부 충족.
2. 코덱스 1라운드 검수가 완료 보고 전에 실제 데이터 무결성 결함을
   잡아낸 사례로, 그리고 이전 조각(DoD-14)의 설계 교훈을 미리
   반영해 같은 함정을 피한 사례로 기록한다.
3. Coordinator·Agent 양쪽 모두 재시작을 넘는 Lease/epoch 방어를
   갖췄다 — 다음 후보(실제 재발급 정책, 다중 Agent, HA)는 이 계획
   문서의 "Out" 절이 이미 명시했다.

관련: `docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md` ·
`docs/evidence/DoD-14_durable_fence_watermark.md` ·
`docs/evidence/DoD-15_같은_연결_반복_lease_갱신.md` ·
`crates/coordinator/src/lease_store.rs`
