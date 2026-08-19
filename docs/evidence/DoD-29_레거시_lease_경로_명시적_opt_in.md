---
schema_version: 2
id: DoD-29
claim: "Coordinator 를 --lease-db 없이(레거시 경로, lease_store=None) 시작하면 revoke·만료·max_total_duration_seconds 보호 장치가 전부 조용히 우회되던 위험한 기본값을, 새 CLI 플래그 --i-understand-legacy-mode-is-unsafe(기본값 false)로 명시적 opt-in 을 요구하도록 바꿨다 — --lease-db 도 없고 이 플래그도 없으면 TCP bind 전에 즉시 종료한다. opt-in 시에는 기존과 완전히 동일하게 동작하되 시작 시 경고 로그를 남긴다. 기존 레거시 selftest 시나리오(1~17·19, 그리고 DoD-22 의 저장소 무관 revoke notice 계약 시나리오 27~31)는 run_handshake() 헬퍼가 --lease-db 부재 시 자동으로 opt-in 플래그를 붙여 원래 검증 목적을 그대로 유지한다. 신규 시나리오 38 은 opt-in 없는 Coordinator 의 즉시 거부와, 그 상황에서 연결을 시도하는 Agent 가 하드 타임아웃 안에 깔끔하게 실패 종료하는지까지 확인한다"
status: PASS
commit: 53e6836

executor_id: "agent:codex-cli+agent:claude-code"
executor_tool: "codex exec --sandbox workspace-write -c model_reasoning_effort=high (구현 2라운드) / claude-code (cargo build·coordinator-agent-selftest 16회 연속 독립 재실행 — 코덱스 read-only 샌드박스 밖 실제 환경)"
executor_model: "gpt-5.6-luna (OpenAI Codex v0.144.1) + claude-sonnet-5"
executed_at: "2026-08-20T04:37:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high — 대화 기록이 없는 새 프로세스 인스턴스 3라운드(claude-code 의 감독자 직접 재검증은 샌드박스 밖 보조 확인으로, executor_tool 쪽에 별도 기록)"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(p161) — 새 플래그의 조건(--lease-db None 이고 플래그 false 일 때만 거부)·TCP bind 이전 종료·opt-in 시 동작 불변·기존 레거시 시나리오 유지 여부를 확인해 CHANGES_REQUESTED(시나리오 27~31 의 legacy opt-in 자동 적용 의도가 주석에 없음, 시나리오 38 이 Agent 쪽 종료를 검증하지 않음). 2라운드(p163) — p161 지적 두 가지가 실제로 코드로 해소됐음을 확인했으나(coordinator_agent_selftest.rs:100-105 주석 추가, :1563~1683 각 시나리오 주석, :2192-2268 시나리오 38 Agent 스폰+5초 하드 타임아웃) 검수 환경(read-only 샌드박스)에서 cargo build 락 실패·selftest 60초 미완료를 추가로 보고해 CHANGES_REQUESTED. 3라운드(p164) — cargo build 락 실패는 read-only 샌드박스에서 당연한 결과임을 확인·git diff 3파일은 조각 전체가 아직 미커밋이라 정상임을 확인했으나, selftest 미완료(Coordinator만 생성되고 Agent 서브프로세스가 스폰되지 않은 채 시나리오 1 READY 읽기에서 정지)는 재현돼 CHANGES_REQUESTED. 감독자(claude-code)가 코덱스 read-only 샌드박스 밖(이 세션의 실제 PowerShell 환경)에서 같은 바이너리를 16회 연속(1+5+10, 10회차는 30초 하드 타임아웃 포함) 실행해 매회 exit=0·38개 시나리오 전부 완료(약 9초)를 확인 — 단 한 번도 재현되지 않아, 이 미완료를 코덱스 read-only 샌드박스의 프로세스 스폰 제약(DoD-28 의 check_schema.py 임시 파일 쓰기 제약과 같은 종류의 환경 한계)으로 결론지었다. 코드 자체의 정확성(주석·Agent 스폰·하드 타임아웃 로직)은 2·3라운드 양쪽이 이미 소스로 확인했으므로 최종 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-29_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-29_레거시_lease_경로_명시적_opt_in_2026-08-20.txt"
raw_output_digest: "sha256:6674d1af1501cbf18eb803ab88333ca4368a839bd644c063bb44c08558eb5ee7"
raw_output_bytes: 3067

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "proto 변경 없음 — 이번 조각은 순수 CLI/Coordinator 설정 레벨 정책(새 플래그·시작 시 거부)이다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 127.0.0.1 TCP 핸드셰이크 stub"
network_profile: "127.0.0.1 루프백 TCP 만 사용(coordinator-agent-selftest 기존 하네스와 동일), 신규 시나리오 38 은 Coordinator 가 bind 조차 안 하는 상황에서 Agent 의 연결 거부 처리까지 확인"
command: |
  cargo build --workspace --exclude gputeer-runtime-windows
  cargo test --workspace --exclude gputeer-runtime-windows
  .\target\debug\gputeer.exe coordinator-agent-selftest   # 16회 연속(코덱스 구현 시 5회 + 감독자 재검증 16회), 각 30~90초 하드 타임아웃
raw_output: |
  (docs/evidence/_raw/DoD-29_레거시_lease_경로_명시적_opt_in_2026-08-20.txt,
   docs/evidence/_raw/DoD-29_review.txt 전문 참조)

  cargo build/test --workspace --exclude gputeer-runtime-windows: 성공, 실패 0건
  coordinator-agent-selftest(코덱스 구현 시 5회): 5회 연속 exit=0, 38개 시나리오
  coordinator-agent-selftest(감독자 재검증 16회, 코덱스 샌드박스 밖): 16회 연속 exit=0,
    매회 38개 시나리오, 약 9초/회, 10회차는 30초 하드 타임아웃 내 전부 완료
artifacts:
  - docs/plans/2026-08-20_0437_레거시_lease_경로_명시적_opt_in_v1.md
  - crates/coordinator/src/lib.rs
  - crates/cli/src/main.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-29_레거시_lease_경로_명시적_opt_in_2026-08-20.txt
  - docs/evidence/_raw/DoD-29_review.txt
negative_tests:
  - "selftest 시나리오 38 — --lease-db 및 --i-understand-legacy-mode-is-unsafe 둘 다 없이 Coordinator 를 스폰하면 TCP bind 전에 위험 메시지와 함께 즉시 0이 아닌 exit code 로 종료(coordinator_agent_selftest.rs:2192 부근). 같은 시나리오 안에서 Agent 도 스폰해 연결을 시도시키면 5초 하드 타임아웃 안에 exit=1 로 종료함을 확인(실측 30~41ms, coordinator_agent_selftest.rs:2203-2268)"
  - "뮤테이션(코덱스 자체 보고, p160) — opt-in 검사를 비활성화하면 신규 negative test 가 실패로 바뀌고 5초 하드 타임아웃이 감지됨. 원복 후 재검증 통과"
  - "뮤테이션(코덱스 자체 보고, p162) — 시나리오 38 의 Agent 쪽 연결 거부 검사를 임시로 exit=0 처럼 통과하도록 바꾸면 selftest 전체가 exit=1 로 실패해 검출됨. 원복 후 재검증 통과"
limitations:
  - "코덱스 read-only 샌드박스 안에서는 coordinator-agent-selftest 가 신뢰성 있게 완주하지 못했다(3라운드 검수 중 2회가 미완료/타임아웃을 보고) — Coordinator 는 스폰되지만 Agent 서브프로세스가 스폰되지 않는 패턴으로 관측됐다. 감독자가 같은 바이너리를 샌드박스 밖에서 16회 연속 재현해 단 한 번도 재현되지 않았으므로 코드 결함이 아니라 샌드박스의 프로세스 스폰 제약으로 결론지었지만, 이 결론 자체가 코덱스 프로세스 내부를 완전히 들여다본 것은 아니다 — 정황 증거(Coordinator 는 뜨는데 Agent 는 안 뜬다·샌드박스 밖에서는 100% 재현)에 의존한다"
  - "레거시 경로 자체(--lease-db 없이 opt-in 만 한 상태)의 실제 동작은 이번 조각에서 바뀌지 않았다 — revoke/만료/max_total_duration_seconds 보호가 여전히 없다는 근본적 한계는 유지된다. 이번 조각은 그 위험한 상태로 빠지는 것을 '실수로' 하지 못하게 막을 뿐이다"
  - "opt-in 경고 로그의 실제 운영 관측성(로그 수집·알림 연동)은 다루지 않는다 — stderr/stdout 에 남기는 것으로 그친다"
  - "구현자와 독립 검수자가 이번에도 같은 도구(codex CLI)의 서로 다른 프로세스 인스턴스였고, 최종 판정은 감독자(claude-code)의 샌드박스 밖 직접 재검증에 의존했다 — 완전히 독립적인 제3의 실행 환경(예: 별도 CI 러너)에서의 재확인은 아직 없다"
decision: "레거시 --lease-db 없는 경로가 운영에서 실수로 켜지는 것을 명시적 opt-in 요구로 막았다 — 구현을 코덱스 CLI(workspace-write)에 위임했고, 독립 검수가 두 라운드에 걸쳐 실제 코드 결함 2건(시나리오 27~31 의도 불명확, 시나리오 38 Agent 쪽 미검증)을 찾아 전부 고쳤다. 3라운드째 검수가 코덱스 read-only 샌드박스에서만 재현되는 selftest 미완료를 보고했으나, 감독자가 같은 바이너리를 샌드박스 밖에서 16회 연속 실행해 재현되지 않음을 확인하고 이를 샌드박스 환경 제약(DoD-28 에서 이미 관측된 것과 같은 종류)으로 결론지어 최종 ACCEPTED 로 판단했다."
---

# DoD-29 · 레거시 Lease 경로 명시적 opt-in

## 무엇을 입증하려 했는가

백로그 조사(`p152`)가 2순위로 꼽은 항목 — `max_total_duration_seconds`
갱신 차단·revoke·만료 검사는 전부 `CoordinatorLeaseStore`(`--lease-db`
로 활성화되는 영속 저장소)가 있을 때만 유효하다. `--lease-db` 를
안 주면(레거시 경로, `lease_store=None`) 이 보호 장치들이 전부
조용히 우회된다. 설계 조사(`p158`, read-only)가 이걸 "의도된 레거시
호환 모드지만, 운영에서 실수로 켜질 수 있는 위험한 기본값"이라고
결론짓고, 명시적 opt-in 플래그를 권장안으로 제시했다.

## 구현 1라운드 (코덱스, `p160`)

- 새 CLI 플래그 `--i-understand-legacy-mode-is-unsafe`(기본값
  `false`) 추가.
- `--lease-db` 가 없고 이 플래그도 `false` 면 TCP bind 전에 위험
  메시지와 함께 즉시 종료.
- opt-in 시 경고 로그 출력, 그 외 동작은 기존과 동일.
- `run_handshake()` 헬퍼가 `--lease-db` 없는 기존 레거시 시나리오에
  자동으로 opt-in 플래그를 붙임.
- 신규 negative test 시나리오 38(opt-in 없는 Coordinator 즉시 거부).

## 독립 검수 1라운드(`p161`) — `CHANGES_REQUESTED`

시나리오 27~31(`DoD-22` 의 저장소 무관 revoke notice 계약 시나리오)
도 `--lease-db` 없이 실행돼 자동으로 opt-in 플래그를 받는데, 이게
"저장소 경로" 인지 "레거시 opt-in 경로" 인지 코드 주석에 명시가
없었다. 시나리오 38 이 Coordinator 만 스폰하고 Agent 쪽 종료는
검증하지 않았다.

## 구현 2라운드 (코덱스, `p162`) — `coordinator_agent_selftest.rs` 만 수정

- 시나리오 27~31 이 의도적으로 저장소 무관 revoke notice 계약을
  검증하는 것임을 주석으로 명시(`--lease-db` 는 추가하지 않음 —
  추가하면 검증 대상이 DoD-25/27 의 영속화 경로로 바뀌어버린다).
- 시나리오 38 에 Agent 스폰과 5초 하드 타임아웃을 추가해, 연결
  거부 상황에서 Agent 가 실제로(실측 30~41ms) 깔끔하게 실패
  종료하는지까지 확인.

## 독립 검수 2·3라운드(`p163`·`p164`) — 코드는 통과, 환경 문제로 재반려

2라운드가 두 지적이 코드로 해소됐음을 확인하면서도, 검수 환경
(read-only 샌드박스)에서 `cargo build` 락 실패와 selftest 60초
미완료를 추가로 지적했다. 3라운드가 `cargo build` 락 실패는
read-only 샌드박스의 당연한 제약임을, `git diff` 3파일은 조각
전체가 미커밋 상태라 정상임을 확인했지만, selftest 미완료(Coordinator
만 스폰되고 Agent 서브프로세스는 안 뜬 채 정지)는 재현했다.

## 감독자 직접 재검증(claude-code, 코덱스 샌드박스 밖)

같은 바이너리를 이 세션의 실제 PowerShell 환경에서 16회 연속
(1회 단독 + 5회 연속 + 10회 연속, 마지막 10회는 프로세스별 30초
하드 타임아웃 포함) 실행 — **16회 전부 exit=0, 38개 시나리오
전부, 약 9초/회**. 단 한 번도 미완료가 재현되지 않았다. 코드 자체는
2·3라운드 검수가 이미 소스로 확인했으므로, 이 미완료를 코덱스
read-only 샌드박스의 프로세스 스폰 제약(`DoD-28` 의 `check_schema.py`
임시 파일 쓰기 제약과 같은 종류의 한계)으로 결론짓고 최종
`ACCEPTED` 로 판단했다.

## 결과

```text
cargo build/test --workspace --exclude gputeer-runtime-windows   성공, 실패 0건
coordinator-agent-selftest(코덱스 구현 시 5회)                     5회 연속 exit=0, 38개 시나리오
coordinator-agent-selftest(감독자 재검증 16회, 샌드박스 밖)          16회 연속 exit=0, 매회 약 9초
```

## 이 실험이 증명하지 "않는" 것

- 레거시 경로 자체(opt-in 뒤)의 동작은 바뀌지 않았다 — revoke/만료/
  max_total_duration_seconds 보호는 여전히 없다.
- 코덱스 read-only 샌드박스 안에서 왜 정확히 Agent 서브프로세스가
  스폰되지 않는지는 내부까지 밝히지 못했다 — 정황 증거로 결론지었다.
- 완전히 독립된 제3의 실행 환경(예: 실제 CI 러너)에서의 재확인은
  아직 없다.

## 결정

1. 운영에서 `--lease-db` 누락으로 조용히 레거시 위험 모드에 빠지는
   경로를 명시적 opt-in 요구로 막았다.
2. 독립 검수 2라운드가 실제 코드 결함 2건을 찾아 고쳤고, 3라운드
   째의 반려 사유는 감독자의 직접 재현(16회 연속 성공)으로 코덱스
   read-only 샌드박스의 환경 제약임을 확인해 최종 `ACCEPTED`.

관련: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`(같은
백로그 조사에서 함께 나온 더 큰 항목) ·
`docs/evidence/DoD-28_check_schema_ci_연결.md`(같은 종류의 read-only
샌드박스 제약이 먼저 관측된 조각)
