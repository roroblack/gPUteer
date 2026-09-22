---
schema_version: 2
id: DoD-58
claim: "`crates/coordinator/src/multi_agent.rs` 와 `crates/agent/src/multi_agent.rs` 가 여러 Agent 를 동시에 처리하는 별도 lane 을 제공하고, 그 **동시성이 구조적으로 증명된다** — 각 세션이 요구된 수의 세션이 동시에 열릴 때까지 기다리는 관문 때문에 순차 서버는 통과할 수 없다. 두 Agent 가 각자 다른 신원으로 Hello 를 보내고 각자 다른 Lease 를 받아 각자 ACK 까지 마치는 것을 별도 OS 프로세스 3개(coordinator + Agent 2)로 확인했다. `DoD-40` 이 '2~4일 아키텍처 변경' 으로 이월했던 것을 기존 순차 `run()` 을 건드리지 않고 별도 lane 으로 열어, 기존 91개 시나리오에 회귀가 없다"
status: PASS
commit: 7c5d768318273f6c2e1678cf087470eab2a1d27d

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — multi_agent lane 신설(coordinator·agent), 동시성 관문, 식별자 분리, device_id 검증"
executor_model: "claude-opus-5"
executed_at: "2026-08-30T17:20:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 3라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "파일:줄로 지목된 지점 — `crates/coordinator/src/multi_agent.rs:326`·`:338`·`:346`(32비트 short_tag 충돌, 실제 반례 `agent-47131`/`agent-71872` 둘 다 `e7525a3b`), `crates/cli/src/coordinator_agent_selftest.rs:5579`·`:5607`·`:5639`(시나리오 88 이 동시 진입 barrier 없이 성공만 확인 — 순차 서버로도 통과), `crates/agent/src/multi_agent.rs:58` 대 `crates/coordinator/src/multi_agent.rs:261`(Coordinator 가 `hello.mode` 를 대조하지 않아 Resume lane 용 Hello 도 통과), `crates/coordinator/src/lib.rs:675` 대 `crates/cli/src/coordinator_agent_selftest.rs:4950`(heartbeat `device_id` 대조 미검증), `crates/coordinator/src/multi_agent.rs:403`(`require_distinct_scoped_ids` 정의만 되고 미호출), `crates/coordinator/src/lib.rs:1776`·`:1832`·`:310`·`crates/coordinator/src/multi_agent.rs:67`(`validate_device_id` 가 CLI 파서에만 있어 라이브러리 진입점이 우회). 전부 코드로 수정 후 ACCEPTED. 검수는 별도 `lease_store`/`replay` Mutex 사이에 저장소 안전성을 깨는 TOCTOU 가 없음(`issue_grant` 전체가 lease-store 잠금 안, SQLite `BEGIN IMMEDIATE`)과 `declare_domains!` 매크로가 'variant 추가 시 ALL/tag 누락' 구멍을 실제로 닫았음을 코드로 확인했다"
review_artifact: "docs/evidence/_raw/DoD-58_review_rounds.txt"

decision: "기존 순차 `run()` 을 고치지 않고 **별도 lane** 을 연다 — `DoD-40` 설계 조사가 '순차 처리는 의도적이며 바꾸려면 2~4일' 이라고 판정했고, 그 판정을 뒤집는 대신 옆에 새 경로를 만들어 기존 91개 시나리오의 회귀 위험을 0 으로 두었다. 동시성 증명은 **확률이 아니라 관문**으로 한다 — 겹칠 때까지 붙잡아 두는 방법은 느린 기계에서 조용히 무의미해지지만, '2개 세션이 동시에 열릴 때까지 기다린다' 는 관문은 순차 서버가 구조적으로 통과할 수 없다. 식별자는 축약하지 않고 `device_id` 를 그대로 붙인다 — 32비트 해시는 생일 문제로 수만 개면 충돌하고, 충돌하면 저장소 없이 도는 경로에서 서로 다른 holder 앞으로 같은 식별자의 서명된 Lease 가 나간다. `device_id` 형태 검증은 CLI 파서가 아니라 **실행 진입점**(`run()`·`run_multi_agent()`)에 둔다 — CLI 는 이 저장소가 쓰는 한 가지 진입 방법일 뿐이다."
raw_output_artifact: "docs/evidence/_raw/DoD-58_multi_agent_concurrency_2026-08-30.txt"
raw_output_digest: "sha256:5c5b41f057c1e54074020020fa3652f65cc3b0afdfa68b8742ebc52066e08079"
raw_output_bytes: 27558

binary_digests:
  toolchain: "Windows 개발 기계 cargo 1.97.1 / x600 WSL2 cargo 1.89.0"
protocol_versions:
  schema_version: "proto 변경 없음 — 기존 `AgentSessionHello` 를 쓴다. `MODE_MULTI_AGENT_GRANT`·`MODE_RESUME` 를 `crates/protocol/src/constants.rs` 로 옮겼다(값 불변)"
  canonical_spec: "canonical 벡터 50건 불변 — 새 서명 대상 메시지 없음"
platform: "Windows 11 개발 기계(별도 OS 프로세스 3개로 실제 127.0.0.1 TCP 왕복) + x600 WSL2 회귀"
hardware: "GPU 무관 — 이 lane 은 handshake 만 하고 작업을 실행하지 않는다"
network_profile: "127.0.0.1 루프백 TCP 만. 외부 인터페이스 미사용"
command: |
  cargo run -q --bin gputeer -- coordinator-agent-selftest
  # 뮤테이션 A: accept 루프를 순차 처리로
  # 뮤테이션 B: hello.mode 대조 무력화
  cargo test --workspace
  # ★ x600 회귀도 돌렸으나 그 출력은 이 raw 에 **없다**(아래 한계 참조)
  ssh x600 "wsl -e bash /mnt/f/gputeer-work/lxv2.sh"
raw_output: |
  (docs/evidence/_raw/DoD-58_multi_agent_concurrency_2026-08-30.txt 전문 참조)

  88) 두 Agent 가 동시에 붙어(peak_concurrent=2) 각자 다른 Lease 를 받고
      각자 ACK 까지 완료
  91) 다른 lane(Resume) 용으로 서명된 Hello 를 mode 대조로 거부

  뮤테이션 A(순차 처리) -> 88 실패, 첫 Agent 가 os error 10060 으로 끊김
  뮤테이션 B(mode 대조 제거) -> 91 실패, served=1 peak_concurrent=1

artifacts:
  - crates/coordinator/src/multi_agent.rs
  - crates/agent/src/multi_agent.rs
  - crates/coordinator/src/lib.rs
  - crates/protocol/src/constants.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-58_multi_agent_concurrency_2026-08-30.txt
  - docs/evidence/_raw/DoD-58_review_rounds.txt
  - docs/evidence/_raw/DoD-58_review_round1_verbatim.txt
negative_tests:
  - "시나리오 91: 등록된 Agent 가 **Resume lane 용 mode 로 서명한** Hello 를 보내면 거부되고, 거부가 Grant 전송 **전에** 일어남을 확인한다. 서명은 자기 키로 유효하므로 서명 검증은 이것을 절대 못 잡는다 — 뮤테이션으로 대조를 무력화하니 정확히 이 시나리오가 실패했다"
  - "시나리오 88 의 동시성 관문: 각 세션이 2개 세션이 동시에 열릴 때까지 기다린다. **뮤테이션으로 accept 루프를 순차 처리로 바꾸니** 첫 세션의 대기가 안 풀려 Agent 가 타임아웃(os error 10060)으로 끊겼다 — 순차 서버는 구조적으로 통과할 수 없다"
  - "시나리오 88 의 식별자 검사: 각 Lease 에 자기 Agent 신원이 그대로 들어 있는지 확인한다. 축약을 다시 넣으면 여기서 걸린다"
  - "`require_multiple_identities`: Agent 신원이 하나뿐이면 이 lane 을 거부한다 — '다중 Agent 를 켰다' 고 믿는데 실제로는 하나만 도는 상태를 만들지 않는다"
  - "`require_distinct_scoped_ids`: 등록된 Agent 들이 서로 다른 식별자를 받는지 bind 전에 확인한다. `scoped_id()` 가 단사임을 코드로 알 수 있어도 여기서 한 번 더 강제한다 — 나중에 누가 축약을 다시 넣으면 그때 걸린다"
  - "`the_forbidden_shapes_are_actually_refused`: `device_id` 로 빈 값·`;`·`=`·개행·공백·`/`·`../escape`·비ASCII·NUL·64자 초과를 거부한다. `ordinary_identifiers_pass` 가 정상 값 통과를 같이 확인한다 — 없으면 '전부 거부' 로도 통과한다"
  - "시나리오 90: 남의 Agent 이름으로 보낸 heartbeat 가 통과하지 못함을 확인한다. ★ 막히는 지점이 예상과 달랐다 — `NodeHeartbeat::signer_id()` 가 `device_id` 라 서명자 조회(`UnknownSigner`)에서 먼저 막힌다. 명시적 `device_id` 대조는 이 경로에서 **도달 불가**이며, 없는 테스트를 지어내는 대신 실제로 막히는 지점을 고정했다"
limitations:
  - "★ **이 lane 은 작업을 실행하지 않는다** — Hello/Grant/ACK 한 왕복만 한다. GPU 배치·자원 경쟁·동일 GPU 중복 선택은 범위 밖이다"
  - "★ 식별자를 Agent 마다 갈라 놓으므로 **같은 자원을 두고 다투는 상황이 아니다.** 이 조각이 보는 것은 '여러 연결을 동시에 처리하는가' 이지 '경쟁하면 어떻게 되는가' 가 아니다"
  - "replay 잠금을 `read_frame()` 동안 잡아 모든 ingress 읽기를 직렬화한다 — 느린 Agent 하나가 최대 타임아웃 동안 다른 Agent 의 검증을 막을 수 있다. 안전성이 아니라 동시성·가용성 문제이며 검수도 그렇게 판정했다"
  - "저장소 없이(legacy) 도는 경로로 검증했다 — durable lease-store 경합은 `DoD-40` 이 별도로 다뤘고 이 lane 과 결합해 재검증하지 않았다"
  - "Coordinator HA·다중 Coordinator·동일 identity 복제 Agent 의 active-session owner 선정은 여전히 범위 밖이다"
  - "★ **x600 WSL 회귀 출력이 이 raw 에 없다**(2026-08-30 독립 검수 6라운드 지적). `command` 에는 있지만 저장하지 않았다 — 이 artifact 만으로는 Linux 회귀를 독립 확인할 수 없다"
  - "★ **뮤테이션 출력은 명령의 stdout/stderr 를 직접 저장한 원본이 아니라 수동 전사본**이다(같은 검수 지적). raw 파일 자체가 그렇게 밝히고 있다"
  - "★ `raw_output_artifact` 는 실제 출력이지만 **워크스페이스 전체 출력은 요약 줄만 남긴 것**이다(수천 줄이라 전량 보존하지 않았다). 남긴 줄 자체는 편집하지 않았다. 검수 원문도 판정 블록만 잘라 보존했다"
  - "`hello.mode` 대조는 두 lane 을 구분하지만, 같은 lane 안에서 등록된 Agent 가 남의 자리를 주장하는 것은 `parse_agent_directory` 의 중복 거부와 keyring 조회에 의존한다"
---

# DoD-58 — 다중 Agent 동시 처리 lane

## 왜 별도 lane 인가

`DoD-40` 설계 조사가 정직하게 판정했다 — 진짜 다중 Agent 동시 경쟁은
지금 Coordinator 아키텍처로는 **표현 자체가 안 된다.** `run()` 은
`accept()` → 동기 `serve_one_connection()` → 다음 `accept()` 로
의도적으로 순차 처리이고(코드 주석에 "deliberately sequential" 명시),
설정에 Agent identity/key 가 각각 하나뿐이다.

그 판정을 뒤집는 대신 **옆에 새 경로**를 만들었다. 기존 91개 시나리오에
회귀 위험이 없고, 순차 경로의 의도적 설계도 그대로 남는다.

## 동시성을 확률이 아니라 구조로 증명한다

★ 검수가 초안 시나리오의 공허성을 지적했다 — 두 Agent 를 연달아 띄우고
둘 다 성공했는지만 보면 **순차 서버로도 통과한다.**

겹칠 때까지 잠깐 붙잡아 두는 방법도 있지만 그건 확률이다. 느린 기계에서
첫 세션이 먼저 끝나면 조용히 무의미해진다.

관문은 다르다.

```text
각 세션은 2개 세션이 동시에 열릴 때까지 기다린다
  순차 서버는 두 번째 연결을 아예 받지 않는다
  -> 첫 세션의 대기가 절대 안 풀린다
  -> 시간 초과로 명확히 실패한다
```

통과하는 유일한 방법이 실제 동시 처리다. 뮤테이션으로 accept 루프를
순차 처리로 바꾸니 정확히 그렇게 실패했다.

## 검수가 실제 충돌 반례를 만들었다

식별자 분리가 Agent ID 의 BLAKE3 앞 8자(32비트)에만 의존했다.

```text
agent-47131 -> e7525a3b
agent-71872 -> e7525a3b
```

충돌하면 두 Agent 가 같은 `lease_id`·`job_id`·`attempt_id`·`grant_id` 를
받는다. 영속 저장소가 있으면 holder 충돌로 fail-closed(그래도 서비스
거부)지만, **저장소가 없으면 서로 다른 holder 앞으로 같은 식별자·같은
fence epoch 를 가진 서명된 Lease 두 개가 나간다.** 시나리오 88 이 정확히
그 경로를 탄다.

32비트는 생일 문제로 수만 개면 충돌한다. 축약을 없앴다.

## 보내는 쪽만 아는 상수는 있으나 마나다

`MODE_MULTI_AGENT_GRANT` 가 `crates/agent` 안에만 있어서 **Coordinator 는
mode 를 아예 안 봤다.** 등록된 Agent 가 Resume lane 값으로 서명한 Hello 를
보내도 다중 Agent Grant 를 받았다 — 서명은 자기 키로 유효하므로 서명
검증은 이것을 절대 못 잡는다.

상수를 `crates/protocol` 로 옮기고(`CLAUDE.md` §3 — 프로토콜 상수는 한
곳에만) 수신부에서 대조한다.

## 이 실험이 증명하지 않는 것

```text
자원 경쟁       식별자를 Agent 마다 갈라 놓으므로 같은 자원을 두고
                다투는 상황이 아니다
작업 실행       Hello/Grant/ACK 한 왕복만 한다
durable 결합    legacy(저장소 없음) 경로로 검증했다
ingress 병렬성  replay 잠금이 read_frame 동안 모든 ingress 를 직렬화한다
Coordinator HA  다중 Coordinator·active-session owner 선정은 범위 밖
```
