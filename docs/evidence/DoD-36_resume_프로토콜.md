---
schema_version: 2
id: DoD-36
claim: "자동 재접속 루프 로드맵(docs/plans/2026-08-20_0300_..., 7조각)의 조각 3 — 명시적 Resume 프로토콜을 구현했다. proto/lease.proto 에 SessionMode·AgentSessionHello·ResumeLeaseRequest·ResumeOutcome·ResumeLeaseResult 를 순수 추가했다(기존 필드 번호 불변). 세 메시지 모두 Signable 서명 대상으로 만들고 canonical/signing 체인 전체(docs/protocol/signing.md 의 domain_tag 3개 신규 등록, crates/protocol 의 canonical.rs/to_fields.rs/signable.rs, 5개 protocol 테스트 파일, tools/canonical/reference_canonical.py, tests/vectors/canonical_v1.json 45→48개 벡터, proto/SCHEMA_FINGERPRINT.txt, crates/crypto/src/framed_ingress.rs 의 FrameType 3종)를 갱신했다 — domain count 25→28. Coordinator 에 읽기 전용 classify_resume()(crates/coordinator/src/lease_store.rs, get_or_issue()/renew_existing_within_duration() 재사용 안 함, 판정 순서 identity→revoke→만료(<=)→epoch)을 신설하고 wire dispatch·서명은 crates/coordinator/src/lib.rs 에 뒀다. Agent 에는 --resume-protocol opt-in 플래그를 추가했고, 기본값(플래그 없음)은 기존 server-first Grant/ACK handshake 그대로다 — 기존 48개 레거시 시나리오와 DoD-35 의 49~52 재접속 시나리오는 전혀 안 바뀐다. coordinator-agent-selftest 시나리오 53~60 신설(RESUMED·UNKNOWN_LEASE·IDENTITY_CONFLICT·REVOKED·EXPIRED·SUPERSEDED·EPOCH_AHEAD·UNAVAILABLE). durable request ledger(조각 5)·다중 Agent 경쟁(조각 7)·session_id 의 durable 소유권 검증은 의도적으로 범위 밖 — 이번 조각의 request_nonce 는 서명·상관관계·기존 replay guard 용도까지만 다룬다"
status: PASS
commit: afda055

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현 2라운드) / claude-code (canonical self-test·verify·check_schema·cargo build/test·coordinator-agent-selftest 5~8회 연속 독립 재실행 — 매 라운드마다, 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T12:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스 3라운드"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(p189) — proto 순수 추가 여부(기존 필드 번호 불변, lease.proto:122)·기존 handshake 완전 보존(agent/lib.rs:463, coordinator/lib.rs:270 에서 --resume-protocol 조건문 뒤로 완전히 분리)·domain 28개 고유성(t1b_grant_and_control.rs:418)·ShortLived/field_number_audit 등록(lifetime_consistency.rs:203, field_number_audit.rs:250)·classify_resume() 이 get_or_issue()/renew_existing_within_duration() 을 재사용 안 하고 identity→revoke→만료(<=)→epoch 순서·만료시각 불변을 지키는지(lease_store.rs:236)·EPOCH_AHEAD enum 존재·범위(durable ledger/다중 Agent 코드 없음) 를 전부 확인해 대부분 통과시켰으나, 새 canonical 벡터 v34/v35/v36 이 Rust 쪽에서 실제로 to_fields.rs 결과와 대조되는 테스트가 없다는 진짜 공백 1건(과거 DoD-05 와 같은 종류)을 찾아 CHANGES_REQUESTED. 2라운드(p191) — t1_signing_targets.rs:460-537 의 신규 테스트가 세 메시지를 벡터와 동일한 필드값으로 구성해 canonical_hex 와 직접 비교함·필드값이 reference_canonical.py:1614-1640 과 일치함·뮤테이션 논리 타당함(to_fields.rs:559-572 매핑을 바꾸면 실패)까지 전부 코드로 확인했으나, git diff --stat 이 t1_signing_targets.rs 외 21개 파일을 더 보고한다는 이유로 CHANGES_REQUESTED — 그러나 이 조각 전체가 아직 커밋 전이라 1라운드 변경분이 diff 에 그대로 남아있는 당연한 결과였다(DoD-31 과 같은 종류의 오해, 감독자가 git diff --stat -- . ':!t1_signing_targets.rs' 로 21개 파일의 변경 줄 수가 1라운드 직후와 정확히 동일함을 직접 확인). 3라운드(p192) — 이 경위를 설명받고 재확인해 21개 파일 불변·신규 테스트 함수 1개만 깔끔하게 추가됨(domain_coverage_is_explicit 의 domain 목록·count 갱신 포함)을 확인하고 최종 ACCEPTED. 감독자(claude-code)가 매 라운드 canonical self-test/verify·check_schema·cargo build/test·coordinator-agent-selftest 5~8회 연속(전부 exit=0, 60개 시나리오, 약 30초/회, 120초 하드 타임아웃 대비 여유)으로 독립 재확인했다"
review_artifact: "docs/evidence/_raw/DoD-36_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-36_resume_프로토콜_2026-08-20.txt"
raw_output_digest: "sha256:d413a1f23672adfa4d7b47f386845d06e2d10b88ad9a684e7e4ae404369f7547"
raw_output_bytes: 4298

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto/lease.proto 에 SessionMode·AgentSessionHello·ResumeLeaseRequest·ResumeOutcome·ResumeLeaseResult 순수 추가(기존 메시지 필드 번호 불변). domain_tag 3개 신규 등록(gputeer/v1/session-hello·gputeer/v1/lease-resume·gputeer/v1/lease-resume-result), domain count 25→28"
  canonical_spec: "docs/protocol/signing.md v1 + §5 신규 domain_tag 3건. tests/vectors/canonical_v1.json 45→48개 벡터(v34/v35/v36), Rust(to_fields.rs)·Python(reference_canonical.py) 양쪽에서 바이트 단위 대조 테스트(t1_signing_targets.rs:459) 확인됨"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(기존 하네스와 동일)"
command: |
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  python tools/canonical/check_schema.py
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  cargo test -p gputeer-protocol resume_protocol_messages_match_reference
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 3라운드 각각 5~8회 연속, 각 120초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-36_resume_프로토콜_2026-08-20.txt,
   docs/evidence/_raw/DoD-36_review.txt 전문 참조)

  reference_canonical.py --self-test/--verify: 3라운드 전부 PASS, 48개 벡터 일치
  check_schema.py: 오류 0건
  cargo build/test --workspace --exclude gputeer-runtime-windows: 3라운드 전부 성공, 실패 0건
  cargo test -p gputeer-protocol resume_protocol_messages_match_reference: 통과
  coordinator-agent-selftest: 1차 8회 연속(exit=0, 60개 시나리오, 약 30초/회) +
    2차 5회 연속(exit=0, 60개 시나리오, 약 30.5초/회)
artifacts:
  - docs/plans/2026-08-20_1200_resume_프로토콜_v1.md
  - proto/lease.proto
  - proto/SCHEMA_FINGERPRINT.txt
  - docs/protocol/signing.md
  - crates/protocol/src/canonical.rs
  - crates/protocol/src/signable.rs
  - crates/protocol/src/to_fields.rs
  - crates/protocol/tests/canonical_vectors.rs
  - crates/protocol/tests/field_number_audit.rs
  - crates/protocol/tests/lifetime_consistency.rs
  - crates/protocol/tests/t1_signing_targets.rs
  - crates/protocol/tests/t1b_grant_and_control.rs
  - crates/crypto/src/framed_ingress.rs
  - crates/crypto/tests/framed_ingress.rs
  - crates/coordinator/src/lease_store.rs
  - crates/coordinator/src/lib.rs
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - tools/canonical/reference_canonical.py
  - tests/vectors/canonical_v1.json
  - docs/evidence/_raw/DoD-36_resume_프로토콜_2026-08-20.txt
  - docs/evidence/_raw/DoD-36_review.txt
negative_tests:
  - "selftest 시나리오 54 — 존재하지 않는 lease_id 로 Resume 시도 시 UNKNOWN_LEASE 로 거부됨을 확인"
  - "selftest 시나리오 55 — node_id/job_id/attempt_id 불일치 시 IDENTITY_CONFLICT 로 거부됨을 확인"
  - "selftest 시나리오 56 — revoked Lease 에 대한 Resume 시도가 REVOKED 로 거부됨을 확인"
  - "selftest 시나리오 57 — 만료된(<=) Lease 에 대한 Resume 시도가 EXPIRED 로 거부됨을 확인"
  - "selftest 시나리오 58 — 저장된 값보다 낮은 epoch 요청이 SUPERSEDED 로 거부됨을 확인"
  - "selftest 시나리오 59 — 저장된 값보다 높은 epoch 요청이 EPOCH_AHEAD 로 거부됨을 확인(설계 문서 의사코드-enum 불일치를 고정하는 회귀 테스트)"
  - "뮤테이션(코덱스 자체 보고, p188) — classify_resume() 의 revoke 판정을 만료 판정 뒤로 이동시키자 대응 시나리오 실패, epoch 비교 방향을 반전시키자 대응 시나리오 실패. 둘 다 원복 후 재검증 통과"
  - "뮤테이션(코덱스 자체 보고, p190) — ResumeLeaseRequest 의 canonical 필드 번호를 10→11 로 바꾸자 신규 교차검증 테스트(resume_protocol_messages_match_reference)가 실제로 실패, 원복 후 재검증 통과"
limitations:
  - "durable request ledger(로드맵 조각 5)가 없다 — request_nonce 는 서명·상관관계·기존 InMemoryReplayGuard 용도까지만 다룬다. 동일 nonce+동일 digest 재전송, 동일 nonce+다른 digest 탐지, 응답 유실 후 idempotent 재적용 보장은 이번 조각 범위 밖이다"
  - "다중 Agent 동시 경쟁(로드맵 조각 7)은 다루지 않는다 — Coordinator 는 여전히 순차적으로 연결을 처리한다"
  - "session_id 의 장기(durable) 소유권 검증은 없다 — 이번 조각에서는 순수 상관관계 값으로만 취급한다"
  - "UNAVAILABLE outcome(시나리오 60)은 실제 SQLite lock 장애가 아니라 durable store 미제공 경로로 흉내냈다 — 실제 일시적 저장소 장애 상황의 검증은 아니다"
  - "SessionMode.FRESH 는 proto 에 정의만 됐고 이번 조각에서 실제 사용 경로로 구현되지 않았다 — 기존 handshake 가 여전히 '묵시적 FRESH' 다"
  - "Coordinator 가 Hello-first 인지 Grant-first 인지 자동 탐지하지 않는다 — 호출자(CLI 플래그)가 명시적으로 Resume lane 을 선택해야 한다. 같은 포트에서의 투명한 자동 전환은 의도적으로 범위 밖이다(지연·부분 프레임·legacy agent 와의 교착 위험 때문)"
decision: "자동 재접속 루프 로드맵의 조각 3(Resume 프로토콜)을 구현했다 — proto 4종을 순수 추가하고 canonical/signing 체인 전체를 갱신했으며, 기존 handshake 와 DoD-35 의 재접속 경로는 완전히 보존했다. 구현을 코덱스 CLI(workspace-write)에 2라운드에 걸쳐 위임했고, 독립 검수가 1라운드에서 canonical 교차검증 테스트 부재(과거 DoD-05 와 같은 종류의 공백)를 찾아 반려, 2라운드에서 그 수정 내용은 이미 문제없다고 확인하면서도 아직 커밋되지 않은 조각 전체의 git diff 범위를 오해해 다시 반려, 3라운드에서 그 오해를 해소하고 최종 ACCEPTED. 감독자가 매 라운드 canonical/build/test/selftest 를 독립 재확인했다. 이로써 로드맵 7조각 중 1~3 이 완료됐다 — 남은 4(dispatcher 정교화)·5(durable ledger)·6(Agent Resume 통합)·7(다중 Agent selftest)은 후속 조각으로 남는다."
---

# DoD-36 · Resume 프로토콜

## 무엇을 입증하려 했는가

사용자가 "코덱스 쿼터만 써서 다 진행 최대한 시켜봐" 라고 지시했다
— `DoD-35`(자동 재접속 최소 경로, 로드맵 조각 1+2)에 이어 로드맵
조각 3 을 시작했다. 설계 조사(`p187`, read-only)가 정확한 범위를
확정했다: proto 4종 신설·canonical/signing 체인 전체 갱신·
Coordinator 판정 로직(`classify_resume()`)·selftest 확장(최소
6~8개) — 단 durable request ledger(조각 5)·다중 Agent 경쟁(조각
7)은 명시적으로 범위 밖.

## 구현 1라운드 (코덱스, `p188`)

22개 파일, 1156줄 — proto·canonical 체인 전체·Coordinator/Agent
로직·selftest 시나리오 53~60. `EPOCH_AHEAD` 를 enum 에 추가해
설계 문서의 의사코드-enum 불일치를 해소했다. Windows 용
`getrandom` CSPRNG 의존성을 추가했다.

## 독립 검수 1라운드(`p189`) — `CHANGES_REQUESTED`(진짜 공백 1건)

proto 순수성·기존 handshake 완전 보존·domain 28개 고유성·
`ShortLived`/`field_number_audit` 등록·`classify_resume()` 의
판정 순서와 만료시각 불변·범위 제한까지 대부분 통과시켰으나, 새
canonical 벡터 `v34`/`v35`/`v36` 이 Rust 쪽에서 실제로
`to_fields.rs` 결과와 대조되는 테스트가 없다는 진짜 공백을 찾았다
— 이 저장소가 `DoD-05`(`AgentGrantAck` 승격)에서 겪은 것과 같은
종류의 결함이다.

## 구현 2라운드 (코덱스, `p190`)

`crates/protocol/tests/t1_signing_targets.rs` 에 새 테스트
`resume_protocol_messages_match_reference` 를 추가 — 세 메시지를
벡터와 동일한 필드값으로 구성해 `canonical_hex` 와 직접 비교한다.
뮤테이션(필드 번호 10→11)으로 실제 실패를 확인한 뒤 원복.

## 독립 검수 2·3라운드(`p191`·`p192`) — 오해 끝에 **`ACCEPTED`**

2라운드가 새 테스트 내용은 이미 문제없다고 코드로 확인하면서도,
아직 커밋 전인 조각 전체의 `git diff --stat` 이 여러 파일을
보고한다는 이유로 반려했다 — `DoD-31` 에서 겪은 것과 같은 종류의
범위 오해였다. 감독자가 `git diff --stat -- . ':!t1_signing_targets.rs'`
로 나머지 21개 파일의 변경 줄 수가 1라운드 직후와 정확히 동일함을
확인한 뒤 3라운드를 요청했고, 이 경위를 설명받은 3라운드가
21개 파일 불변·신규 테스트 함수 1개만 깔끔하게 추가됨을 재확인하고
최종 `ACCEPTED`.

## 결과

```text
reference_canonical.py --self-test/--verify   3라운드 전부 PASS, 48개 벡터 일치
check_schema.py                                오류 0건
cargo build/test --workspace                    3라운드 전부 성공, 실패 0건
coordinator-agent-selftest                       1차 8회+2차 5회 연속 exit=0, 60개 시나리오, 약 30초/회
```

## 이 실험이 증명하지 "않는" 것

- durable request ledger 없이는 응답 유실 후 idempotent 재적용을
  보장 못 한다.
- 다중 Agent 동시 경쟁은 다루지 않는다.
- `UNAVAILABLE` 은 실제 SQLite lock 장애가 아니라 근사로 재현했다.
- `SessionMode.FRESH` 는 정의만 됐고 실제 사용 경로는 없다.

## 결정

1. 로드맵 조각 3(Resume 프로토콜)을 완료했다 — 기존 경로 완전
   보존, canonical 체인 전체 갱신.
2. 독립 검수 1라운드가 진짜 교차검증 공백을 찾아 고쳤고, 2라운드의
   반려는 오해였음이 3라운드에서 확인돼 최종 `ACCEPTED`.
3. 로드맵 7조각 중 1~3 완료 — 남은 4~7 은 후속 조각.

관련: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`
(전체 로드맵) · `docs/evidence/DoD-35_자동_재접속_최소_경로.md`
(조각 1+2) · `docs/evidence/DoD-13_...md`(과거 유사 신규 서명
메시지 추가 사례) · `docs/evidence/DoD-05` 계열(과거 같은 종류의
교차검증 공백이 처음 발견된 사례)
