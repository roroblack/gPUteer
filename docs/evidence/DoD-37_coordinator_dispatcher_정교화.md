---
schema_version: 2
id: DoD-37
claim: "자동 재접속 루프 로드맵 조각 4(session dispatcher + transport 오류 격리)를 구현했다. DoD-35 가 만든 반복 accept 루프의 기반 위에, crates/coordinator/src/lib.rs 의 인라인 Grant 처리 로직을 serve_one_connection()/serve_one_connection_impl() 로 추출하고, 새 CoordinatorSessionError{Transport, Protocol, Storage} 로 오류를 분류했다 — transport/protocol 오류는 로그 후 다음 accept 로 진행(--max-connections 는 성공이 아니라 accept 성공한 연결 시도 수를 세어 무한 재시도를 막는다), storage 오류는 즉시 run() 을 Err 로 종료하는 fail-closed. 1차 구현이 Resume 처리 경로(classify_resume() 호출부)의 SQLite 조회 오류를 서명된 UNAVAILABLE 응답으로 흡수해 fail-closed 분기에 도달하지 못하는 진짜 안전 결함을 남겼으나, 독립 검수 1라운드가 이를 찾아 반려했고 2라운드 수정이 LeaseStoreError 를 정책 판정(NotFound/IdentityConflict/Revoked/Expired, 정상 서명 응답)과 진짜 저장소 장애(Io/LockTimeout, CoordinatorSessionError::Storage 로 fail-closed)로 명확히 구분해 닫았다. 이 수정 과정에서 DoD-36 이 기록한 시나리오 60(durable store 없이 Resume 시 서명된 UNAVAILABLE 반환)의 기대 동작이 '설정 오류를 fail-closed 로 처리'로 의도적으로 강화됐다 — 독립 검수가 이 변경 방향이 fail-closed 원칙과 일관됨을 확인했다. coordinator-agent-selftest 시나리오 61~64 신설, 기존 1~60개는 이 의미 변경 1건(시나리오 60)을 제외하고 전부 회귀 없음"
status: PASS
commit: 5a69ee3

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현 2라운드) / claude-code (cargo build/test·coordinator-agent-selftest 5회 연속 독립 재실행 — 매 라운드마다, 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T13:10:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스 2라운드"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(p195) — storage 오류가 정말 fail-closed 되는지·교착/무한루프 위험·selftest 신규 wire-level client 의 nonce 계산 일치·범위를 확인하다가 진짜 결함을 찾았다: classify_resume() 이 SQLite 조회 오류(LeaseStoreError)를 서명된 ResumeLeaseResult{outcome: UNAVAILABLE=7} 로 응답한 뒤 함수가 Ok(()) 로 종료해(lib.rs:840,858) dispatcher 의 Storage fail-closed 분기(lib.rs:333)에 절대 도달 못함 — CHANGES_REQUESTED. 그 외(startup storage fail-closed·transport/protocol 무한루프 없음·nonce 일치·범위)는 이미 문제없다고 확인. 감독자가 코드로 직접 재확인해 실재함을 확인. 2라운드(p197) — 수정된 LeaseStoreError 분류(Io/LockTimeout→Storage, lib.rs:238)가 서명·프레임 전송 전에 즉시 반환됨(lib.rs:933)과 dispatcher 의 Storage 분기(lib.rs:339)가 로그 후 즉시 run() 을 Err 로 종료하고 다음 accept 로 안 넘어감을 코드로 재확인, Grant 발급(lib.rs:1339)·renew 조회/갱신(lib.rs:1103)·revoke 저장(lib.rs:924)도 모두 lease store 문맥으로 storage fail-closed 됨을 확인. ★ 이 수정 과정에서 DoD-36 의 시나리오 60(durable store 없이 Resume 시 서명된 UNAVAILABLE)의 기대 동작이 fail-closed 종료로 바뀐 것에 대해 별도로 판단을 요청했다 — durable store 없이 Resume 은 복구할 권위 있는 상태 자체가 없는 구성 오류이므로 retryable UNAVAILABLE 을 반환하면 영구적 설정 오류를 일시 장애처럼 재시도하게 만든다는 점에서 즉시 종료가 fail-closed 원칙과 일관된다고 판단, 의도적이고 타당한 변경으로 확인. 신규 뮤테이션(Resume 오류 매핑을 예전 동작으로 되돌리면 시나리오 64 가 resume_outcome=7 로 실패)도 유효함을 확인. 2라운드 끝에 최종 ACCEPTED. 감독자(claude-code)가 두 라운드 모두 cargo build/test·coordinator-agent-selftest 5회 연속(전부 exit=0, 63→64개 시나리오, 약 30.4~34.3초/회, 120초 하드 타임아웃 대비 여유)으로 독립 재확인했다"
review_artifact: "docs/evidence/_raw/DoD-37_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-37_coordinator_dispatcher_정교화_2026-08-20.txt"
raw_output_digest: "sha256:6ef1926a0061249f1ff8f236e792b3632d4a84a4ca88e4644bcce15ce1ef39b6"
raw_output_bytes: 5726

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — 이번 조각은 Coordinator 내부 dispatcher/오류 처리 구조 리팩터링이다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(기존 하네스와 동일)"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 2라운드 각각 5회 연속, 각 120초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-37_coordinator_dispatcher_정교화_2026-08-20.txt,
   docs/evidence/_raw/DoD-37_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 2라운드 전부 성공, 실패 0건
  coordinator-agent-selftest(1라운드 5회): 5회 연속 exit=0, 63개 시나리오, 약 30.5초/회
  coordinator-agent-selftest(2라운드 5회): 5회 연속 exit=0, 64개 시나리오, 약 31.7초/회
artifacts:
  - docs/plans/2026-08-20_1310_coordinator_dispatcher_정교화_v1.md
  - crates/coordinator/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-37_coordinator_dispatcher_정교화_2026-08-20.txt
  - docs/evidence/_raw/DoD-37_review.txt
negative_tests:
  - "selftest 시나리오 61 — transport 오류(truncated 연결) 후 dispatcher 가 로그만 남기고 다음 accept 로 진행해 두 번째 연결이 정상 handshake 에 성공함을 확인"
  - "selftest 시나리오 62 — 위조 ACK 서명으로 protocol 오류를 유도한 뒤에도 dispatcher 가 다음 accept 로 진행해 두 번째 연결이 정상 성공함을 확인"
  - "selftest 시나리오 63 — 존재하지 않는 lease DB 부모 경로로 startup storage 오류를 유도해 Coordinator 가 즉시 fail-closed 종료함을 확인"
  - "selftest 시나리오 64(2라운드 신설) — 정상 시작 후 SQLite 파일을 손상시켜(not a sqlite database) Resume 요청 처리 중 진짜 storage 오류를 유도, dispatcher 가 kind=storage 로 분류해 signed UNAVAILABLE 없이 즉시 fail-closed 종료함을 확인 — CONNECTION_ATTEMPT 1회만 발생, 이후 accept 없음"
  - "★ selftest 시나리오 60(재정의, DoD-36 원본은 durable store 없이 Resume 시 서명된 UNAVAILABLE 을 기대) — 이번 조각 이후로는 같은 상황(durable store 미설정)이 구성 오류로 분류돼 fail-closed 종료함을 기대하도록 바뀌었다. 독립 검수 2라운드가 이 방향 전환이 fail-closed 원칙과 일관됨을 확인했다"
  - "뮤테이션(코덱스 자체 보고, p194) — Storage 를 임시로 Transport 로 잘못 분류하면 시나리오 63 이 실패, 원복 후 재검증 통과"
  - "뮤테이션(코덱스 자체 보고, p196) — Resume 오류 매핑을 예전 동작(UNAVAILABLE 서명 응답)으로 되돌리면 시나리오 64 가 resume_outcome=7 로 실패, 원복 후 재검증 통과"
limitations:
  - "★ DoD-36 이 evidence 로 기록한 시나리오 60 의 기대 동작이 이 조각에서 재정의됐다 — 'durable store 없이 Resume 시 서명된 UNAVAILABLE(재시도 가능)' 에서 'storage 로 분류해 fail-closed 즉시 종료' 로 바뀌었다. DoD-36 문서 자체는 소급 수정하지 않았다 — 그 문서는 작성 당시의 실제 동작을 정확히 기록한 것이고, 이 문서(DoD-37)가 그 이후의 의도적 정책 강화를 기록한다"
  - "durable request ledger(로드맵 조각 5)·다중 Agent 경쟁(조각 7)·Agent 쪽 Resume terminal outcome 명시적 처리(조각 6)는 여전히 범위 밖이다"
  - "storage 오류 판정은 `LeaseStoreError` 의 `Io`/`LockTimeout` variant 를 기준으로 한다 — 향후 새 variant 가 추가되는데 이 분류에 안 넣는 실수는 이번 조각의 테스트로 못 잡는다(같은 종류의 위험이 `DoD-31` 의 outcome 목록 누락 패턴과 유사하다)"
decision: "로드맵 조각 4(session dispatcher + transport 오류 격리)를 완료했다 — Coordinator 의 인라인 처리 로직을 함수로 분리하고, transport/protocol/storage 오류를 명확히 구분해 차등 처리(전자는 계속 accept, 후자는 fail-closed)한다. 구현을 코덱스 CLI(workspace-write)에 2라운드에 걸쳐 위임했고, 독립 검수 1라운드가 진짜 storage fail-closed 결함(Resume 경로에서 오류가 UNAVAILABLE 로 흡수됨)을 찾아 반려, 수정 뒤 2라운드가 완전성을 재확인하고 DoD-36 의 기존 시나리오 60 의미 변경까지 정당성을 검토해 최종 ACCEPTED. 감독자가 두 라운드 모두 build/test/selftest 를 독립 재확인했다."
---

# DoD-37 · Coordinator dispatcher 정교화

## 무엇을 입증하려 했는가

`DoD-35`(로드맵 조각 1+2)가 Coordinator 의 반복 accept 루프
기반을 만들었지만, 로드맵이 조각 4 에 요구한 "session dispatcher
+ transport 오류 격리" 계약(정상 처리 로직 분리, 오류 종류별
차등 처리)은 아직 없었다 — 후속 설계 조사(`p193`)가 이걸 확인하고
정직하게 "기반은 됐지만 조각 4 전체는 미완"이라고 판정했다.

## 구현 1라운드 (코덱스, `p194`)

`serve_one_connection()`/`serve_one_connection_impl()` 로 인라인
Grant 처리를 추출, `CoordinatorSessionError{Transport, Protocol,
Storage}` 신설, `--max-connections` 를 "accept 성공한 연결 시도
수"(실패도 카운트, 무한 재시도 방지)로 재정의. selftest 시나리오
61~63.

## 독립 검수 1라운드(`p195`) — `CHANGES_REQUESTED`(진짜 안전 결함)

storage 오류가 정말 fail-closed 되는지 확인하던 중, Resume 처리
경로(`classify_resume()` 호출부)에서 SQLite 조회 오류가 서명된
`UNAVAILABLE` 로 응답된 뒤 함수가 `Ok(())` 로 정상 종료해
dispatcher 의 fail-closed 분기에 절대 도달 못하는 진짜 결함을
찾았다. 감독자가 코드로 직접 재확인해 실재함을 확인했다.

## 구현 2라운드 (코덱스, `p196`)

`LeaseStoreError` 를 정책 판정(`NotFound`·`IdentityConflict`·
`Revoked`·`Expired` — 정상 서명 응답)과 진짜 저장소 장애(`Io`·
`LockTimeout` — `CoordinatorSessionError::Storage` 로 fail-closed)
로 명확히 구분했다. Grant 발급·renew·revoke 경로는 이미 올바르게
fail-closed 돼 있음을 확인만 하고 안 건드렸다. 새 시나리오 64
(실제 SQLite 파일 손상 → storage 분류 → 즉시 종료). **기존 시나리오
60 의 기대 동작을 재정의**했다 — durable store 미설정 상태의
Resume 요청을 "재시도 가능한 일시 장애"(`UNAVAILABLE`)가 아니라
"복구 불가능한 구성 오류"(fail-closed)로 재분류했다.

## 독립 검수 2라운드(`p197`) — **`ACCEPTED`**

storage fail-closed 가 새는 곳이 없는지(Resume·Grant·renew·revoke
전부)를 코드로 끝까지 추적해 확인했다. **시나리오 60 의 의미
변경에 대해 별도로 판단을 요청**했다 — durable store 없이는
Resume 이 복구할 권위 있는 상태 자체가 없는 구성 오류이므로,
`UNAVAILABLE`(재시도 가능)로 응답하면 영구적 설정 오류를 일시
장애처럼 재시도하게 만든다는 점에서 즉시 종료가 이 조각의
fail-closed 원칙과 일관된다고 판단, 의도적이고 타당한 변경으로
결론지었다. 뮤테이션도 유효함을 확인하고 최종 `ACCEPTED`.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   2라운드 전부 성공, 실패 0건
coordinator-agent-selftest(1라운드 5회)                            5회 연속 exit=0, 63개 시나리오, 약 30.5초/회
coordinator-agent-selftest(2라운드 5회)                            5회 연속 exit=0, 64개 시나리오, 약 31.7초/회
```

## 이 실험이 증명하지 "않는" 것

- durable request ledger·다중 Agent 경쟁·Agent 쪽 Resume terminal
  outcome 처리(조각 5~7)는 여전히 범위 밖.
- `LeaseStoreError` 에 새 variant 가 추가될 때 이 분류에 빠뜨리는
  실수는 이번 조각의 테스트로 못 잡는다.

## 결정

1. 로드맵 조각 4(session dispatcher + 오류 격리)를 완료했다.
2. 독립 검수 1라운드가 진짜 storage fail-closed 결함을 찾아
   고쳤고, 2라운드가 완전성과 시나리오 60 의미 변경의 정당성까지
   확인해 `ACCEPTED`.
3. `DoD-36` 이 기록한 시나리오 60 동작은 이 조각에서 의도적으로
   강화됐다 — 과거 evidence 문서는 소급 수정하지 않고, 이 변경
   사실을 여기(DoD-37)에 명시적으로 남긴다.

관련: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`
(전체 로드맵) · `docs/evidence/DoD-35_자동_재접속_최소_경로.md`
(조각 1+2, 반복 accept 기반) · `docs/evidence/DoD-36_resume_프로토콜.md`
(조각 3, 시나리오 60 원본 기록)
