---
schema_version: 2
id: DoD-14
claim: "FenceWatermark(Lease epoch 강등 방어)가 SQLite 파일에 영속되어 Agent 프로세스 재시작을 넘어 유지된다. Agent 의 최초 Grant 검증·Lease 갱신 검증 두 호출부 모두 DurableFenceWatermark 를 쓰며, 열 수 없거나(:memory: 등) 비영속인 저장소는 fail closed 로 거부한다. 정책 거부(낮은 epoch)와 저장소 장애(I/O·락 타임아웃)는 서로 다른 오류로 구분된다"
status: PASS
commit: 47ba0c019e2146907def5063117bec31d82ac5cf

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
review_scope: "DurableFenceWatermark(SQLite 영속·트랜잭션·is_durable), Agent 의 두 검증 호출부 전환, fail-closed(:memory: 거부), 오류 메시지 분리(Stale vs 저장소 장애), coordinator-agent-selftest 시나리오 17·18, 뮤테이션 비공허성. 2라운드 — 1라운드 CHANGES_REQUESTED(:memory: 미검증·오류 메시지 미분리 2건 지적), 코드 수정 후 2라운드 좁은 후속 검수 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-14_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-14_selftest_2026-08-19.txt"
raw_output_digest: "sha256:e2801dbbba7a1f783a55c9fff06fa2b91934a8cdfe0ee48442ecd30df882d45b"
raw_output_bytes: 22650

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1"
  cli_bin: "target/debug/gputeer.exe (dev profile, commit 47ba0c0 에서 빌드)"
protocol_versions:
  schema_version: "해당 없음 — 이 조각은 Protocol 서명 대상 메시지를 바꾸지 않는다. fence_db_path 는 Agent 로컬 설정이지 서명 필드가 아니다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "127.0.0.1 실제 TCP 소켓, 두 개의 별도 OS 프로세스 사이 — DoD-11~13 과 같은 handshake 위에 이어 붙인 같은 연결. 재시작 시나리오는 세 번째(scenario 17)·네 번째(scenario 18, 단독) 별도 프로세스를 추가로 띄운다"
command: |
  cargo build --workspace
  cargo test --workspace
  cargo test -p gputeer-runtime-policy --lib durable_lease_scope
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 18개 시나리오, 5회 연속
raw_output: |
  (docs/evidence/_raw/DoD-14_selftest_2026-08-19.txt 전문 참조)

  17) durable FenceWatermark 재시작 방어 확인 — 별도 프로세스가 SQLite 파일로 이전
      watermark 를 물려받아 낮은 epoch 의 최초 Grant 를 거부한다(뮤테이션 테스트로
      확인한 대로, 이 하나의 관측 지점이 그 위에 올라타는 갱신 경로까지 보호한다)
  18) --fence-db :memory: fail closed 확인 (비영속 경로로 재시작 방어를 흉내내지 못한다)

  5회 연속 전부 exit=0 (시나리오 1~18 전부). cargo test --workspace: 47개 테스트
  스위트 전체 통과, 0 failed. durable_lease_scope 단위 테스트 7/7 통과.

  ★ raw 파일에는 이 evidence 의 claim 과 무관한 flaky 관측 1건(crates/checkpoint
  crate, 이번 작업에서 손대지 않은 코드)도 투명하게 기록해 뒀다 — 별도 조사
  태스크로 분리했다.
artifacts:
  - docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md
  - crates/runtime-policy/Cargo.toml
  - crates/runtime-policy/src/lib.rs
  - crates/runtime-policy/src/durable_lease_scope.rs
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-14_selftest_2026-08-19.txt
  - docs/evidence/_raw/DoD-14_review.txt
negative_tests:
  - "재시작을 넘는 낮은 epoch 의 최초 Grant 거부(시나리오 17): 1차 별도 프로세스가 epoch=5 로 정상 발급해 durable watermark=5 를 SQLite 파일에 남기고 종료한다. 2차 별도 프로세스(다른 PID)가 같은 파일로 epoch=3 의 최초 Grant 를 시도하면 verify_and_record_lease() 가 거부한다(LEASE_REJECTED: fence_epoch 검사 실패(정책 거부), incoming=3/watermark=5 수치까지 확인). ★ 뮤테이션으로 비공허성 확인 — open() 이 주어진 경로를 무시하도록 무력화하면 이 시나리오가 정확히 예상대로 실패한다(원복 후 재통과)"
  - "★ 구현 중 발견한 설계 결함: 원래 계획한 '갱신 경로 전용' 재시작 시나리오는 거짓양성이었다. Agent 의 제어 흐름상 Lease 갱신은 항상 같은 프로세스의 최초 Grant 검증 뒤에만 실행되므로, 그 프로세스 자신의 최초 Grant 검증이 이미 로컬 watermark 를 채워 놓아 갱신 거부를 durable 저장소와 무관하게 설명할 수 있었다. 실제로 open() 을 무력화해도 이 시나리오는 계속 통과했다(거짓양성 확인). 존재하지 않는 구분을 존재하는 것처럼 보고하지 않기 위해 시나리오를 하나(최초 Grant 경로)로 재구성했다 — 계획 문서 '★ 구현 중 정정' 절 참조"
  - "--fence-db :memory: fail closed(시나리오 18): Coordinator 없이 Agent 단독 프로세스로 실행 — is_durable() 검사가 TcpStream::connect() 보다 먼저이므로 네트워크 연결 시도 자체가 없다(약 200ms 내 종료). ★ 뮤테이션으로 비공허성 확인 — is_durable() 검사를 무력화하면 실제로 연결을 시도해 '기대와 다른 이유'(Coordinator 연결 실패)로 실패한다는 것까지 시나리오가 잡아낸다(단순 성공/실패가 아니라 실패 이유까지 검증)"
  - "정책 거부와 저장소 장애의 오류 메시지 분리: DurableFenceError::Stale 은 '...(정책 거부) — {violation}', Io/LockTimeout 은 'FENCE_STORAGE_ERROR: ...'로 완전히 다른 접두사를 낸다(fence_error_message() 헬퍼) — negative test 가 우연한 저장소 장애를 정책 거부로 오인해 거짓으로 통과하지 않도록 한다. 코덱스 1라운드 검수가 이 공백을 지적해 추가됐다"
  - "u64::MAX 근접 epoch 값이 BLOB 인코딩(8바이트 big-endian)에서 잘리지 않는다(u64_max_epoch_round_trips_without_truncation 단위 테스트) — SQLite INTEGER 의 i64::MAX 제한을 BLOB 저장으로 우회한다"
  - "재오픈 시 watermark 보존(reopening_the_same_file_preserves_the_watermark 단위 테스트): 같은 프로세스 안에서 인스턴스를 드롭하고 같은 파일 경로로 재오픈해도 이전에 기록한 값이 그대로 남는다 — 프로세스 간 실측(시나리오 17)보다 가벼운 단위 테스트 수준의 회귀 방지"
  - "서로 다른 job_id 는 서로의 watermark 를 침범하지 않는다(watermark_is_per_resource 단위 테스트, 기존 FenceWatermark 와 동일 계약)"
limitations:
  - "★ Coordinator 는 영속 Lease 저장소가 없다 — config.fence_epoch(최초 CLI 인자)를 '기억하는 현재 epoch'로 쓴다. 재시작하면 그 값도 사라진다. 이 조각은 Agent 쪽 방어만 durable 하게 만들었다 — Coordinator 쪽 durable 저장소는 다음 후보로 남는다(docs/plans/2026-08-19_2200_...v1.md §Out)"
  - "GC 없음 — watermark 항목은 절대 삭제하지 않는다(삭제하면 그 resource 의 stale epoch 가 다시 통과할 위험이 있다). resource lifecycle 이 생기기 전까지는 의도적으로 무기한 보존한다"
  - "여러 Agent 가 같은 SQLite 파일을 공유하는 운영 모델은 범위 밖이다 — 이 조각은 Agent 하나가 DB 파일 하나를 소유하는 모델이다. BEGIN IMMEDIATE·busy_timeout 은 손상 방지·동시 접근의 기반일 뿐, 다중 Agent 의 소유권·리더십·quota 정책은 정의하지 않는다"
  - "반복 갱신·재접속(failover)·실제 Lease 재발급 정책(SUPERSEDED/QUARANTINED 를 언제 내리는지)·Lease revoke 는 여전히 범위 밖이다 — DoD-13 의 한계가 그대로 이어진다"
  - "Windows 단일 플랫폼에서만 실행했다"
  - "코덱스 검수 2라운드 모두 read-only 샌드박스에 cargo 가 없어 cargo build/test 를 직접 실행하지 못했다 — 2라운드는 기존 바이너리를 직접 실행해 :memory: 시나리오(~200ms)는 확인했으나 18개 시나리오 전체 순차 실행은 60초 내 끝나지 않아 중단했다고 명시했다. 실행 검증은 claude-code 세션이 직접 수행했다(docs/evidence/_raw/DoD-14_selftest_2026-08-19.txt)"
  - "이 raw 캡처 과정에서 crates/checkpoint/tests/write_failure.rs 가 한 차례 flaky 하게 실패했다(직후 3회 재실행 전부 통과) — 이번 작업에서 손대지 않은 무관한 코드이며, 별도 조사 태스크로 분리했다. 이 evidence 의 claim 에는 영향이 없다"
decision: "durable FenceWatermark(SQLite 기반 영속 Lease epoch 강등 방어)가 계획대로 구현·검증됐다. docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md 의 단계 1~8, DoD 체크박스 전부 충족. 코덱스 1라운드 검수가 완료 보고 전에 실제 결함 2건(:memory: 미검증, 오류 메시지 미분리)을 찾아냈고 둘 다 코드로 고쳤다. 구현 과정에서 계획 자체의 설계 결함(갱신 경로 전용 재시작 시나리오가 거짓양성)도 뮤테이션 테스트로 발견해 정직하게 정정했다 — 이는 '계획을 세웠다'와 '계획이 실제로 판별력이 있다'가 다르다는 것을 다시 확인한 사례다. 다음 후보(Coordinator 영속 Lease 저장소, 반복 갱신, 실제 재발급 정책)는 이 계획 문서와 DoD-13 의 Out 절이 이미 명시했다 — 각각 새 계획 문서가 필요하다."
---

# DoD-14 · durable FenceWatermark

## 무엇을 입증하려 했는가

`DoD-12`·`DoD-13` 의 limitations 절에 매번 "재시작 후 stale epoch
차단은 증명하지 않는다"가 반복 등장했다 — `FenceWatermark`
(`crates/runtime-policy/src/lease_scope.rs`)가 `HashMap` 기반
메모리 전용 상태였기 때문이다. Agent 프로세스가 재시작하면(크래시·
배포·OS 재부팅 등) watermark 를 잃고, 그 순간부터 재시작 전 epoch
보다 낮은 Lease 도 다시 통과할 수 있었다.

이 저장소는 같은 문제를 replay guard 에서 이미 한 번 풀었다 —
`InMemoryReplayGuard`/`DurableReplayGuard` 가 나란히 있다. 이 조각은
같은 패턴을 fence watermark 에 적용한다.

## 구현 개요

1. **`crates/runtime-policy/src/durable_lease_scope.rs`(신규)** —
   `DurableFenceWatermark`. `DurableReplayGuard` 를 재사용하지 않고
   별도 타입으로 만들었다 — fence watermark 의 "낮은 epoch 만 거부,
   같은 epoch 재사용 허용" 계약이 replay guard 의 "nonce 중복 거부"
   의미·GC 정책과 충돌하기 때문이다(replay guard 의 GC 가 오래된
   항목을 지우면, fence watermark 에서는 그 resource 의 stale epoch
   가 다시 통과하는 정반대 결과가 된다). SQLite 연결·`BEGIN IMMEDIATE`
   트랜잭션·에러 매핑 **패턴만** 재사용했다. `epoch` 는 8바이트
   big-endian `BLOB` 으로 저장해 SQLite `INTEGER` 의 `i64::MAX` 제한을
   우회한다.
2. **`crates/agent/src/lib.rs`** — 최초 Grant 검증
   (`verify_and_record_lease()`)과 Lease 갱신 검증(`do_renew` 블록)
   **두 호출부 모두** `DurableFenceWatermark` 를 쓰도록 바꿨다.
   `DurableFenceWatermark::open()` 을 **네트워크 연결보다 먼저** 열고,
   `is_durable() == false` 면 즉시 실패한다(fail closed) — `:memory:`
   같은 비영속 경로로 이 조각의 목적을 조용히 무력화하지 못하게 한다.
   `fence_error_message()` 헬퍼가 정책 거부(`Stale`)와 저장소 장애
   (`Io`/`LockTimeout`)를 서로 다른 오류 문자열로 낸다.
3. **`crates/cli/src/coordinator_agent_selftest.rs`** — 시나리오
   17(재시작 방어)·18(`:memory:` fail closed) 추가.

## ★ 구현 중 발견한 설계 결함 — 뮤테이션 테스트가 잡았다

원래 계획(`docs/plans/2026-08-19_2200_...v1.md` §재시작 selftest
설계)은 "최초 Grant 경로"·"갱신 경로" 재시작 방어를 각각 따로
시험하려 했다. 구현 후 `DurableFenceWatermark::open()` 을 "주어진
경로를 무시하고 매번 새 파일을 연다"로 무력화해 뮤테이션 테스트를
해보니:

- "최초 Grant 경로" 시나리오는 예상대로 실패했다(진짜 판별력이 있었다).
- "갱신 경로" 시나리오는 **계속 통과했다** — 거짓양성이었다.

이유: Agent 의 제어 흐름상 Lease 갱신은 항상 같은 프로세스의 최초
Grant 검증 **뒤**에만 일어난다. "갱신 경로" 시나리오의 2차 프로세스는
최초 Grant(epoch 5)와 갱신(epoch 3)을 **같은 프로세스** 안에서
순서대로 검증하는데, 최초 Grant 검증이 그 프로세스의 로컬
`DurableFenceWatermark` 인스턴스에 watermark=5 를 (다시) 써 넣고,
그 값이 durable 저장소에서 왔든 이 프로세스 자신이 방금 썼든
상관없이 뒤이은 갱신(epoch 3)의 거부를 설명하기에 충분했다.

**결론:** 이 Agent 의 구조(최초 Grant 검증이 항상 갱신 검증보다
먼저 실행되고, 같은 resource key 를 쓴다)에서는 "갱신 경로 전용"
재시작 방어를 최초 Grant 경로와 분리해서 증명할 방법이 없다 — 최초
Grant 검증이 durable 하면 그 위에 올라타는 갱신도 자동으로 안전하고,
durable 하지 않으면 갱신 검증이 아무리 정확해도 프로세스 경계를
넘는 방어는 전혀 없다. 시나리오를 하나로 정리했다 — 존재하지 않는
구분을 존재하는 것처럼 evidence 에 남기지 않는다.

## 코덱스 1라운드 검수가 잡은 결함 2건

1. `--fence-db :memory:` 를 검사 없이 받아들였다 — `is_durable()`
   이 `false` 를 반환할 수 있지만 Agent 가 그 값을 확인하지 않았다.
2. `DurableFenceError::Stale`(정책 거부)과 `Io`/`LockTimeout`(저장소
   장애)이 같은 오류 문자열로 뭉뚱그려져, negative test 가 진짜
   정책 거부를 확인하는지 우연한 저장소 장애를 확인하는지 구분할 수
   없었다.

둘 다 코드로 고치고(`is_durable()` fail-closed 검사,
`fence_error_message()` 헬퍼, 시나리오 17 강화, 시나리오 18 신규),
각각 뮤테이션 테스트로 비공허성을 확인한 뒤 좁은 후속 검수에서
`ACCEPTED` 를 받았다.

## 결과

```text
gputeer coordinator-agent-selftest   18개 시나리오(DoD-11~13 의 16 + 이 조각의 2) — 5회 연속 전부 통과
cargo test --workspace               47개 테스트 스위트 전체 통과, 0 failed
durable_lease_scope 단위 테스트       7/7 통과
```

## 이 실험이 증명하지 "않는" 것

- **Coordinator 의 영속 Lease 저장소.** `config.fence_epoch` 를
  "기억하는 현재 epoch"으로 쓰는 것은 이 stub 이 CLI 인자 하나로
  단일 실행만 처리하기 때문이다 — 재시작하면 그 값도 사라진다.
- **watermark GC.** resource lifecycle 이 생기기 전까지는 절대
  삭제하지 않는다.
- **다중 Agent 가 같은 DB 파일을 공유하는 운영 모델.**
- **반복 갱신·재접속(failover)·실제 Lease 재발급 정책·revoke.**
- Windows 단일 플랫폼.

## 결정

1. durable FenceWatermark 가 계획대로 완료됐다 —
   `docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md` 의
   단계 1~8, DoD 체크박스 전부 충족.
2. 독립 검수가 완료 보고 전에 실제 결함 2건을 잡아낸 사례로,
   그리고 뮤테이션 테스트가 계획 자체의 설계 결함을 잡아낸 사례로
   기록한다.
3. 다음 후보(Coordinator 영속 Lease 저장소, 반복 갱신, 실제 재발급
   정책)는 이 계획 문서와 `DoD-13` 의 "Out" 절이 이미 명시했다 —
   각각 새 계획 문서가 필요하다.

관련: `docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md` ·
`docs/evidence/DoD-12_coordinator_agent_lease_최소_조각.md` ·
`docs/evidence/DoD-13_coordinator_agent_lease_갱신_최소_조각.md` ·
`crates/runtime-policy/src/durable_lease_scope.rs`
