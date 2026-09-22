---
schema_version: 2
id: DoD-30
claim: "crates/agent 가 Job 실행을 향한 가장 작은 첫 걸음을 구현했다 — 유효한 Grant/Lease 검증 성공 직후, AgentGrantAck 전송 전에 결정적 checkpoint_id(BLAKE3-256, domain + 길이-프리픽스된 job_id/attempt_id/grant_id)로 checkpoint 디렉터리를 만들고 WRITING 마커를 write_once()(DoD-21 의 동시 호출 거부 계약을 그대로 상속)로 기록한 뒤, JOB_STARTED state=WRITING 을 stdout 에 출력한다. 위조·만료·revoked Lease 는 마커를 만들지 않고 ACK 도 안 보낸다. 마커 생성 자체가 실패하면(checkpoint root 가 디렉터리가 아닌 경우 등) fail-closed 로 ACK 를 안 보낸다. 동일 attempt 재시도는 같은 checkpoint_id 로 write_once() 의 기존 idempotent 동작에 의존한다. 실제 entrypoint 프로세스 실행·manifest.json 작성·Coordinator 에 보고하는 wire 메시지·scheduler 는 전부 범위 밖이다 — RESULT ok=true 는 여전히 Job 완료를 뜻하지 않는다는 것을 코드 주석으로 명시했다"
status: PASS
commit: 230f89c

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현) / claude-code (cargo build·coordinator-agent-selftest 5회 연속 독립 재실행 — 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T05:30:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "마커 기록 로직이 정확히 Lease 검증·fence 검사 성공 뒤·AgentGrantAck 전송 전에 실행되는지(lib.rs:152-169 → ACK 전송 :186-212), checkpoint_id 규칙이 실제로 도메인 태그·세 필드 각각의 u64 big-endian 길이 프리픽스·BLAKE3-256 을 갖춰 canonical encoding 결함(길이 프리픽스 없는 문자열 접합 충돌)을 피했는지(:521-540), 거부 경로(위조/만료/revoked)에서 마커 미생성·ACK 미전송이 파일시스템 수준으로 확인되는지(:490-513, :552-563, selftest 2333-2415), fail-closed(checkpoint root 를 일반 파일로 만들어 디렉터리 생성 실패 유도, 시나리오 44), 멱등성(동일 root 에 동일 호출 2회, write_once() 의 기존 동일 내용 검사가 atomic.rs:233-243 에 있음을 확인, selftest 2410-2448), RESULT ok=true 가 Job 완료가 아니라는 주석(lib.rs:103-105), 이번 조각이 entrypoint 실행·manifest.json·wire 메시지·scheduler 를 안 건드렸는지(git diff --stat 예상 5개 파일과 일치), 기본 --checkpoint-root 값이 OS 임시 디렉터리 아래 PID+nanos 별 경로라 Agent 인스턴스 간 충돌이 없는지(:720-724). 코덱스 read-only 샌드박스 안에서 selftest 실행이 30초 제한에 걸렸으나 검수자가 이를 판정 근거로 쓰지 않았고, 감독자(claude-code)가 같은 바이너리를 샌드박스 밖에서 5회 연속 실행해(전부 exit=0, 44개 시나리오, 약 10초/회) 재확인 — DoD-28·DoD-29 에서 이미 관측된 것과 같은 종류의 샌드박스 프로세스 스폰 제약으로 결론지었다. 1라운드(p166) 만에 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-30_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-30_job_시작_writing_마커_2026-08-20.txt"
raw_output_digest: "sha256:b913dc70a632095d29a92b68694195f768ba7a57f469502f44a16b150163c4e6"
raw_output_bytes: 2559

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — 이번 조각은 기존 checkpoint API(record_initial_state())·기존 Grant/Lease handshake 를 재사용하는 순수 Agent 내부 로직이다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음). checkpoint_id 자체는 서명 대상이 아니다 — BLAKE3-256(domain + 길이-프리픽스 필드) 로 별도 정의된 내부 식별자다"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub, checkpoint 디렉터리는 로컬 파일시스템"
network_profile: "127.0.0.1 루프백 TCP 만 사용(coordinator-agent-selftest 기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 코덱스 구현 시 5회 + 감독자 재검증 5회, 각 90초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-30_job_시작_writing_마커_2026-08-20.txt,
   docs/evidence/_raw/DoD-30_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공, 실패 0건
  coordinator-agent-selftest(코덱스 구현 시 5회): 5회 연속 exit=0, 44개 시나리오
  coordinator-agent-selftest(감독자 재검증 5회, 코덱스 샌드박스 밖): 5회 연속 exit=0,
    매회 44개 시나리오, 약 10초/회
artifacts:
  - docs/plans/2026-08-20_0310_job_시작_마커_최소_조각_v1.md
  - crates/agent/Cargo.toml
  - Cargo.lock
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-30_job_시작_writing_마커_2026-08-20.txt
  - docs/evidence/_raw/DoD-30_review.txt
negative_tests:
  - "selftest 시나리오 40 — 위조 서명 Grant/Lease 에서는 checkpoint 마커가 생성되지 않고 AgentGrantAck 도 전송되지 않음을 파일시스템 상태로 확인"
  - "selftest 시나리오 41 — 만료된 Lease 에서도 동일하게 마커 미생성·ACK 미전송 확인"
  - "selftest 시나리오 42 — 영속 저장소의 revoked Lease 재발급 거부 상황에서도 마커 미생성·ACK 미전송 확인"
  - "selftest 시나리오 44 — checkpoint root 경로를 디렉터리가 아닌 일반 파일로 만들어 디렉터리 생성 자체를 실패시키고, Agent 가 fail-closed 로 AgentGrantAck 를 보내지 않고 종료하는지 확인"
  - "뮤테이션(코덱스 자체 보고, p165) — record_initial_state() 호출을 제거하면 시나리오 39(정상 Grant 의 마커 생성 확인)가 실패로 바뀜을 확인 후 원복, 재빌드·selftest 재검증 통과"
limitations:
  - "checkpoint_id 의 write_once() 내부 Ok(false)(동일 내용 재호출) 반환값은 record_initial_state() 의 공개 시그니처가 Result<()> 라 직접 관찰할 수 없다 — 시나리오 43 은 '동일 attempt 재시도 시 같은 ID·단일 마커 유지'라는 외부 관측으로 멱등성을 대신 확인했다. write_once() 자체의 Ok(false) 분기는 기존 crates/checkpoint 단위 테스트가 이미 커버한다"
  - "실제 entrypoint 프로세스 실행·GPU 확인·runtime 격리는 전혀 다루지 않는다 — 설계 문서(Candidate 2)가 명시적으로 범위 밖으로 뒀다"
  - "데이터 파일·manifest.json 작성·LOCAL_WRITTEN 이후 상태 전이는 없다 — WRITING 마커만 있는 디렉터리는 기존 gc_partial() 규칙상 PARTIAL 로 취급돼 GC 대상이다(이 조각이 새로 만든 규칙이 아니라 기존 규칙을 재사용한다)"
  - "Coordinator 에 시작 사실을 보고하는 wire 메시지(설계 문서의 후보 3)는 만들지 않았다 — Coordinator 는 여전히 Agent 가 실제로 시작했는지 알 방법이 없다"
  - "코덱스 read-only 샌드박스 안에서 coordinator-agent-selftest 가 30초 제한에 걸리는 현상이 이번에도 관측됐다(DoD-28·DoD-29 와 같은 패턴) — 감독자가 샌드박스 밖에서 5회 연속 재현해 문제없음을 확인했지만, 이 샌드박스 제약 자체의 근본 원인은 여전히 규명하지 않았다"
decision: "Job 실행을 향한 첫 걸음(WRITING 시작 마커)을 구현했다 — 기존 Grant/Lease handshake·checkpoint write_once() 인프라만 재사용하고 실제 워크로드 실행은 전혀 건드리지 않는, 설계 문서가 제안한 '핸드셰이크/stub 수준에 머문다'는 원칙을 그대로 지켰다. 구현을 코덱스 CLI(workspace-write)에 위임했고, 독립 검수(대화 기록 없는 새 인스턴스, read-only)가 순서·checkpoint_id 인코딩·거부 경로·fail-closed·멱등성·범위 제한까지 전부 코드로 확인하고 1라운드 만에 ACCEPTED. 검수 환경(read-only 샌드박스)의 selftest 실행 제한은 DoD-28·DoD-29 와 같은 종류의 샌드박스 한계로 판단, 감독자가 샌드박스 밖에서 재확인했다."
---

# DoD-30 · Job 시작 WRITING 마커

## 무엇을 입증하려 했는가

`crates/agent` 는 지금까지 Grant/Lease 검증 → `AgentGrantAck` 전송
→ 갱신/revoke stub 처리 → `RESULT ok=true` 출력 후 종료, 여기서
멈췄다. `JobManifest.entrypoint`/`args`/`env`/`grant.plan` 은 proto
에 있지만 아무 데도 쓰이지 않았다 — 즉 `RESULT ok=true` 는 "Job 을
완료했다" 는 뜻이 아니라 "handshake/stub 이 성공했다" 는 뜻일
뿐이었다. 설계 조사(`p154`, `docs/plans/2026-08-20_0310_job_시작_마커_최소_조각_v1.md`)
가 이 간극을 메우는 가장 작은 첫 걸음("후보 2 — WRITING 시작
마커")을 이미 설계해뒀다.

## 구현 (코덱스, `p165`)

- Lease 검증·fence 검사 성공 직후, `AgentGrantAck` 전송 전에
  `record_start_checkpoint()`(신설) 호출.
- `checkpoint_id` = `start-<BLAKE3-256(domain ||
  u64_be(len(job_id)) || job_id || u64_be(len(attempt_id)) ||
  attempt_id || u64_be(len(grant_id)) || grant_id)>`, domain
  `"gputeer/job-start-checkpoint/v1\0"` — 길이 프리픽스로 필드
  경계 충돌(canonical encoding 결함)을 방지.
- `crates/checkpoint::durability::record_initial_state()` 로
  `WRITING` 마커를 write-once 로 기록(`DoD-21` 의 동시 호출 거부
  계약을 그대로 상속).
- `JOB_STARTED ... state=WRITING` stdout 출력.
- 새 `--checkpoint-root` CLI 플래그(기본값: OS 임시 디렉터리 아래
  PID+nanos 별 경로 — Agent 인스턴스 간 충돌 방지).
- `RESULT ok=true` 가 Job 완료를 뜻하지 않는다는 것을 코드 주석으로
  명시.
- `coordinator-agent-selftest` 시나리오 39~44 신설(정상 마커 생성·
  위조/만료/revoked Lease 거부·재시도 멱등성·fail-closed).

## 독립 검수(`p166`) — **1라운드 만에 ACCEPTED**

실행 순서(검증 뒤·ACK 전)·`checkpoint_id` 의 길이 프리픽스 인코딩·
거부 경로의 파일시스템 수준 확인·fail-closed·멱등성·`RESULT
ok=true` 주석·범위 제한(entrypoint/manifest/wire 메시지/scheduler
미포함)·기본 `--checkpoint-root` 의 충돌 안전성까지 전부 코드로
확인했다. 검수 환경(read-only 샌드박스)에서 selftest 가 30초
제한에 걸렸으나 판정 근거로 쓰지 않았다 — 감독자(claude-code)가
같은 바이너리를 샌드박스 밖에서 5회 연속 실행해(전부 exit=0, 44개
시나리오, 약 10초/회) 문제없음을 재확인했다.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   성공, 실패 0건
coordinator-agent-selftest(코덱스 구현 시 5회)                     5회 연속 exit=0, 44개 시나리오
coordinator-agent-selftest(감독자 재검증 5회, 샌드박스 밖)           5회 연속 exit=0, 매회 약 10초
```

## 이 실험이 증명하지 "않는" 것

- 실제 entrypoint 프로세스 실행·GPU 확인·runtime 격리는 없다.
- 데이터 파일·`manifest.json` 작성·`LOCAL_WRITTEN` 이후 전이는 없다
  — `WRITING` 마커만 있는 디렉터리는 기존 `gc_partial()` 규칙상
  PARTIAL 로 GC 대상이다.
- Coordinator 에 시작 사실을 보고하는 wire 메시지(설계 문서의
  "후보 3")는 없다 — Coordinator 는 여전히 Agent 가 실제로
  시작했는지 알 방법이 없다.
- `write_once()` 의 `Ok(false)`(동일 내용 재호출) 분기는 이번
  조각에서 직접 관찰하지 않고 외부 관측(같은 ID·단일 마커)으로
  대신했다.

## 결정

1. Job 실행을 향한 가장 작은 첫 걸음을 구현했다 — 기존 인프라만
   재사용하고 실제 워크로드 실행은 전혀 건드리지 않았다.
2. 독립 검수 1라운드 만에 `ACCEPTED`. 검수 환경의 selftest 제한은
   `DoD-28`·`DoD-29` 와 같은 종류의 샌드박스 한계로 판단, 감독자가
   샌드박스 밖에서 재확인했다.

관련: `docs/plans/2026-08-20_0310_job_시작_마커_최소_조각_v1.md` ·
`docs/evidence/DoD-21_write_once_동시_호출_계약.md`(재사용한
write-once 인프라) · `docs/evidence/DoD-29_레거시_lease_경로_명시적_opt_in.md`
(같은 종류의 샌드박스 프로세스 스폰 제약이 먼저 관측된 조각)
