---
schema_version: 2
id: DoD-18
claim: "Coordinator 영속 Lease 저장소(lease_store=Some)에서, 저장된 issued_at_unix_ms 기준 누적 시간이 max_total_duration_seconds 를 초과하면 갱신 요청에 서명된 RENEW_OUTCOME_MAX_DURATION_EXCEEDED(=6) 를 반환하고 저장소의 만료시각을 연장하지 않는다(실측: 2초 한도를 실제로 2.2초 초과시켜 확인). 한도 안에서는 여전히 RENEWED 가 나온다(오탐 없음). --renew-outcome-override(테스트 전용) 는 이제 lease_store=Some 에서도 저장소를 전혀 건드리지 않는다 — 실제 초과는 여전히 override 보다 우선한다. lease_store=None(레거시) 경로는 이 판정을 하지 않는다(경과시간을 추적할 수 없기 때문)"
status: PASS
commit: d3a74b5918ed0e7e1c98385d7b8a4aa1a89c00f0

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-19T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high (설계·1라운드) / medium (2라운드)"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "판정 위치·원자성(트랜잭션 하나로 조회→판정→조건부 UPDATE), 비교식과 clock rollback fail-closed 처리, renew_outcome_override 와의 우선순위, lease_store=None 레거시 경로가 판정을 건너뛰는지, Agent 의 outcome=6 처리, selftest 시나리오 22·23 구조, 기존 안전장치(u32_from_stored) 우회 여부, 설계 자체의 맹점. 1라운드(p114) — CHANGES_REQUESTED(lease_store=Some+override 조합에서 초과 여부와 무관하게 먼저 저장소를 갱신한 뒤 override 를 적용해 '거부 응답인데 저장소는 갱신됨' 상태 불일치 발견). 수정(StoredLease::is_max_duration_exceeded() 순수 함수 분리 + override 경로는 읽기 전용 get() 만 사용) 및 시나리오 24 추가 뒤 2라운드(p115) — ACCEPTED, TOCTOU 가능성은 테스트 전용 override 경로라 실질적 결함 아니라고 판단"
review_artifact: "docs/evidence/_raw/DoD-18_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-18_max_total_duration_2026-08-19.txt"
raw_output_digest: "sha256:64b86f2032f1e8ad46336c4f40a281b0fb4ac93f1c4e7b56c7897af3928886f2"
raw_output_bytes: 44220

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
  cli_bin: "target/debug/gputeer.exe (dev profile, commit d3a74b5 에서 빌드)"
protocol_versions:
  schema_version: "해당 없음 — max_total_duration_seconds(Lease.33)와 RENEW_OUTCOME_MAX_DURATION_EXCEEDED(=6)는 proto/lease.proto 에 이미 있던 필드다. 이 조각은 wire schema 를 바꾸지 않고, 지금까지 판정되지 않던 필드를 실제로 판정하게 만들었을 뿐이다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "127.0.0.1 실제 TCP 소켓, 별도 OS 프로세스 간(시나리오 22·23·24 는 각각 최초 발급/갱신을 서로 다른 Coordinator·Agent 프로세스로 나눠 실제 프로세스 경계를 넘는다). 시나리오 22 는 selftest 프로세스가 실제로 2.2초 sleep 해 2초 한도를 초과시킨다"
command: |
  cargo build --workspace
  cargo test --workspace
  ./target/debug/gputeer.exe coordinator-agent-selftest   # 24개 시나리오, 5회 연속
raw_output: |
  (docs/evidence/_raw/DoD-18_max_total_duration_2026-08-19.txt 전문 참조)

  22) max_total_duration_seconds 초과 — 서명된 MAX_DURATION_EXCEEDED 로 갱신 거부 확인
      (저장소 만료시각 불변)
  23) max_total_duration_seconds 대조군 — 한도 안에서는 여전히 RENEWED 확인 (오탐 없음)
  24) lease_store=Some + override — 서명된 정책 거부는 그대로이고 저장소는 전혀
      갱신되지 않음을 확인 (코덱스 p114 지적의 회귀 방지)

  5회 연속 전부 exit=0 (시나리오 1~24 전부). cargo test --workspace: 전체 스위트
  통과, 0 failed.
artifacts:
  - docs/plans/2026-08-19_2350_max_total_duration_seconds_갱신_차단_v1.md
  - crates/coordinator/src/lease_store.rs
  - crates/coordinator/src/lib.rs
  - crates/agent/src/lib.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-18_max_total_duration_2026-08-19.txt
  - docs/evidence/_raw/DoD-18_review.txt
negative_tests:
  - "시나리오 22 — 2초 한도를 CLI(`--max-total-duration-seconds 2`)로 주고, selftest 프로세스가 실제로 2.2초 sleep 한 뒤 별도 프로세스로 갱신을 시도한다. 서명된 outcome=6 + Agent 의 RENEW_REFUSED:MAX_DURATION_EXCEEDED 거부를 확인하고, 저장소를 직접 재조회해 expires_at_unix_ms 가 초과 판정 전후로 정확히 동일함을 확인한다(연장되지 않았다) — `--renew-rounds` 만 늘리는 것으로는 판별 불가능하다는 설계(p111) 경고를 반영해 두 프로세스 + 실제 sleep 방식을 썼다"
  - "시나리오 23(대조군) — 한도를 3600초로 크게 주고 즉시 갱신해도 여전히 RENEWED 가 나오는지 확인한다. 이 대조군이 없으면 22번이 '항상 거부'라는 다른 결함으로 우연히 통과하는 거짓양성을 배제하지 못한다"
  - "★ 코덱스 1라운드(p114)가 찾은 실제 결함 — lease_store=Some 이고 override 도 있을 때, 초과 여부와 무관하게 먼저 renew_existing_within_duration() 을 호출해 저장소를 갱신(만료시각 연장)한 뒤에야 override 를 적용했다. 즉 outcome=SUPERSEDED/QUARANTINED 같은 '서명된 정책 거부' 응답을 보내면서도 저장소에는 실제로 갱신이 기록되는 상태 불일치였다(레거시 lease_store=None 경로가 override 시 저장소를 전혀 안 건드리는 것과 비대칭). StoredLease::is_max_duration_exceeded() 를 순수 함수로 분리해, override 가 있으면 읽기 전용 get() 으로 초과 여부만 먼저 확인하고(초과 시엔 여전히 outcome=6 이 override 를 이긴다), 저장소를 바꾸는 쓰기 경로는 override 가 없을 때만 타도록 재구성했다"
  - "시나리오 24(신규) — 이 결함의 회귀 방지 테스트. lease_store=Some 상태에서 한도 안(3600초)에 override=SUPERSEDED(2) 를 강제 주입하고, Agent 가 서명된 정책 거부로 정확히 분류하는지 + 저장소의 expires_at_unix_ms 가 override 전후로 정확히 동일한지 직접 재조회로 확인한다"
  - "뮤테이션으로 비공허성 확인 4건: (1) 초과 판정을 `false && ...` 로 무력화 → 시나리오 22 가 정확히 예상대로 실패(outcome=1 로 정상 갱신됨), (2) 초과해도 UPDATE 를 강행하도록 되돌림 → 22 번의 저장소 불변 확인이 정확히 예상대로 실패(expires_at 이 실제로 증가함), (3) Agent 의 outcome=6 분기를 제거 → 시나리오 22 가 정확히 예상대로 실패(Coordinator 는 outcome=6 을 정상 전송했지만 Agent 가 '알 수 없는 outcome'으로 처리), (4) override-우선 수정을 예전(결함 있는) 순서로 되돌림 → 시나리오 24 가 정확히 예상대로 실패(expires_at 이 override 인데도 증가함). 4건 전부 원복 후 재통과"
limitations:
  - "lease_store=None(레거시) 경로는 이 판정 자체를 하지 않는다 — 그 경로는 매 갱신 요청마다 issued_at_unix_ms 를 즉석에서 재구성해 실제 경과시간을 프로세스 경계를 넘어 추적하지 못하기 때문이다. 이는 설계 단계(p111)에서 명시적으로 인정한 legacy enforcement bypass 다"
  - "새 lease_id 발급, 초과 후 자동 재시도/전환은 범위 밖이다 — MAX_DURATION_EXCEEDED 수신 후 Agent 는 명시적으로 거부만 하고 새 Lease 를 요청하지 않는다"
  - "SUPERSEDED/QUARANTINED 판정 정책, revoke, job cancel/completion/quorum unavailable 정책은 범위 밖이다"
  - "scheduler·watchdog·다중 Agent·다중 Coordinator HA·TLS 는 범위 밖이다"
  - "override 경로의 get() 과 이후 무엇도 쓰지 않는 구조 사이에 이론적 TOCTOU 가 있다(코덱스 p115 지적) — override 는 테스트 전용이고 이 경로 자체가 저장소를 쓰지 않으므로 실질적 결함은 아니라고 판단했다. 실제 운영에서 override 를 쓸 일이 없다는 전제에 의존한다"
  - "Windows 단일 플랫폼에서만 실행했다"
  - "코덱스는 read-only 샌드박스에 cargo 가 없어 cargo build/test/coordinator-agent-selftest 를 직접 실행하지 못했다 — 두 라운드 모두 정적 코드 검토만 했다고 명시했다. 실행 검증은 claude-code 세션이 직접 수행했다"
decision: "max_total_duration_seconds 갱신 차단 정책이 계획대로 구현·검증됐다 — docs/plans/2026-08-19_2350_max_total_duration_seconds_갱신_차단_v1.md 의 단계 1~6, DoD 체크박스 전부 충족. 코덱스 1라운드 검수가 실제 상태 불일치 결함(override + 저장소 갱신 순서)을 찾아냈고, 이를 순수 함수 분리 + 조건 재구성으로 고친 뒤 회귀 방지 시나리오 24 를 추가해 2라운드에서 ACCEPTED. 뮤테이션 테스트 4건(판정 무력화·초과시 UPDATE 강행·Agent 분기 제거·override 순서 되돌림) 전부 정확히 예상된 이유로 실패해 비공허성을 확인했다. 이 세션에서 코덱스 감사(p110)가 추천한 두 후보(RevokeLeaseNotice 커버리지=DoD-17, max_total_duration_seconds=이 조각) 모두 완료했다."
---

# DoD-18 · max_total_duration_seconds 갱신 차단 정책

## 무엇을 입증하려 했는가

`Lease.max_total_duration_seconds`(`proto/lease.proto:61`, 기본
24시간)와 `RenewOutcome.RENEW_OUTCOME_MAX_DURATION_EXCEEDED = 6`
(`proto/lease.proto:132`)은 스키마에 오래전부터 있었지만, 어디서도
실제로 판정되지 않았다 — 저장·전달만 될 뿐이고, outcome=6 은
`--renew-outcome-override`(테스트 전용 강제 주입)로만 나올 수
있었다. 코덱스 감사(`p110`)가 이 공백을 난이도 "중간"으로 재평가해
추천했다. `DoD-16`(Coordinator 영속 Lease 저장소)이 `issued_at_unix_ms`
를 재시작을 넘어 정확히 영속 추적하는 것이 이 조각의 선행 기반이었다.

## 설계(코덱스 `p111`)

Coordinator 갱신 경로에서 `now - stored.issued_at_unix_ms >
max_total_duration_seconds * 1000` 을 판정하되, 조회→판정→조건부
UPDATE 를 트랜잭션 하나로 묶어 TOCTOU 를 없애고, 초과 시 만료시각을
연장하지 않도록(연장하면 다음 판정 시각이 밀려 정책이 무력화된다)
설계했다. `lease_store=None`(레거시) 경로는 실제 경과시간을 추적할
수 없어 판정 자체를 하지 않기로 했다. Agent 는 outcome=6 을 기존
2/3(SUPERSEDED/QUARANTINED) 과 같은 패턴으로 명시 거부한다.

## 구현

- `CoordinatorLeaseStore::renew_existing_within_duration()` 신설 —
  `TransactionBehavior::Immediate` 트랜잭션 안에서 조회→판정→조건부
  UPDATE. clock rollback(`now < issued_at`)은 fail closed 로 초과
  취급.
- `build_renew_result()` 를 이 메서드 기반으로 재작성, 실제 초과가
  `--renew-outcome-override` 보다 우선하도록 구성.
- Agent: `RenewOutcome` 매치에 `6 => RENEW_REFUSED:MAX_DURATION_EXCEEDED`.
- `--max-total-duration-seconds` CLI 플래그(Coordinator, 기본값
  86,400 = 기존과 동일).
- selftest 시나리오 22(실제 2초 초과 후 별도 프로세스로 갱신 시도)·
  23(대조군, 한도 안에서는 여전히 RENEWED).

## 코덱스 1라운드(`p114`) — 실제 결함 발견

`lease_store=Some` 이고 override 도 있을 때, 초과 여부와 무관하게
먼저 저장소를 갱신(만료시각 연장)한 **뒤에** override 를 적용하고
있었다 — "거부 응답인데 저장소는 갱신됨" 이라는 상태 불일치였다.
레거시 `lease_store=None` 경로가 override 시 저장소를 전혀 안
건드리는 것과 비대칭이었다.

## 수정

`StoredLease::is_max_duration_exceeded()` 를 순수 함수로 분리했다.
override 가 있으면 읽기 전용 `get()` 으로 초과 여부만 먼저 확인하고
(초과 시엔 여전히 outcome=6 이 override 를 이긴다), 저장소를 바꾸는
쓰기 경로(`renew_existing_within_duration()`)는 override 가 없을 때
하나뿐이다. 회귀 방지 시나리오 24(lease_store=Some + override, 저장소
만료시각 불변 확인)를 추가했다.

## 코덱스 2라운드(`p115`) — ACCEPTED

수정된 흐름을 파일:줄로 직접 확인하고 시나리오 24 의 구조도 검토해
`ACCEPTED`. `get()` 이후 다른 정상 갱신이 끼어드는 이론적 TOCTOU
가능성을 스스로 지적했지만, override 가 테스트 전용이고 그 경로가
저장소를 쓰지 않으므로 이번 변경을 거부할 정도의 실질적 결함은
아니라고 판단했다.

## 결과

```text
cargo test --workspace                                전체 스위트 통과, 0 failed
gputeer coordinator-agent-selftest                     24개 시나리오 — 5회 연속 전부 통과
```

### 뮤테이션으로 비공허성을 확인했다 (4건)

1. 초과 판정을 `false && ...` 로 무력화 → 시나리오 22 가 정확히
   예상대로 실패(outcome=1 로 정상 갱신됨).
2. 초과해도 UPDATE 를 강행하도록 변경 → 22 번의 저장소 불변 확인이
   예상대로 실패(expires_at 이 실제로 증가함).
3. Agent 의 outcome=6 분기를 제거 → 22 번이 예상대로 실패(Coordinator
   는 outcome=6 을 정상 전송했지만 Agent 가 "알 수 없는 outcome"으로
   처리).
4. override-우선 수정을 예전(결함 있는) 순서로 되돌림 → 시나리오 24
   가 예상대로 실패(expires_at 이 override 인데도 증가함).

4건 전부 원복 후 재통과했다.

## 이 실험이 증명하지 "않는" 것

- `lease_store=None`(레거시) 경로는 이 판정을 하지 않는다 — 매
  요청마다 `issued_at_unix_ms` 가 즉석에서 재구성돼 실제 경과시간을
  추적하지 못하기 때문이다.
- 새 `lease_id` 발급, 초과 후 자동 재시도/전환.
- `SUPERSEDED`/`QUARANTINED` 판정 정책·revoke·job cancel/completion.
- scheduler·다중 Agent·다중 Coordinator HA·TLS.
- override 경로의 이론적 TOCTOU(테스트 전용이라 실질적 위험은 아님).
- Windows 단일 플랫폼.

## 결정

1. `max_total_duration_seconds` 갱신 차단이 계획대로 완료됐다.
2. 코덱스 1라운드가 실제 상태 불일치 결함을 찾아냈고, 수정 + 회귀
   방지 시나리오로 2라운드에서 `ACCEPTED`.
3. 코덱스 감사(`p110`)가 추천한 두 후보(`DoD-17`·이 조각) 모두 완료.

관련: `docs/plans/2026-08-19_2350_max_total_duration_seconds_갱신_차단_v1.md` ·
`docs/evidence/DoD-16_coordinator_영속_lease_저장소.md` ·
`docs/evidence/DoD-17_revoke_lease_notice_framed_ingress_커버리지.md`
