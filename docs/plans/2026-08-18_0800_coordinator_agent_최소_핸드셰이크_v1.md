# 2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1

- **기준선:** `../gputeer_master_plan_FINAL.md` — coordinator/agent 미착수
- **대상 단계:** v0.1
- **선행 게이트:** 없음(이미 있는 protocol·crypto·checkpoint·runtime-policy 계층 위에 얹는다)

★ 이 문서는 코덱스(`agent:codex-cli`, read-only 샌드박스)의 설계
응답을 거의 그대로 옮긴 것이다. **코드는 저장소에 적용되지 않았고
컴파일도 확인되지 않았다.** 구현 세션이 이 계획을 실행하기 전에
아래 "확인 안 됨" 항목부터 검증해야 한다.

## 왜 지금 이 계획인가

`CLAUDE.md` 가 반복해서 적어 둔 가장 큰 공백:

> 서비스가 아니다 — 네트워크 수신도, 데몬도, 스케줄러도 없다.
> coordinator · agent · scheduler 는 여전히 미착수다.

`gputeer selftest` §5(2026-08-17 추가)가 127.0.0.1 실제 TCP 소켓으로
signed Grant 를 왕복시키지만, **같은 프로세스 안 스레드 하나**가
여는 소켓이다(`crates/cli/src/selftest.rs:493-623`, 특히 스레드
스폰은 `543-577`). 소켓 왕복은 증명하지만 **프로세스 경계**는 아직
증명하지 않는다. 이 계획은 그 다음 한 걸음이다 — 완전한
coordinator/agent 가 아니라 **실제 별도 PID 두 개가 서명된 메시지를
주고받고, 위조·replay 를 거부하는 것**까지만 증명한다.

## 범위

### In
- `gputeer coordinator-stub` / `gputeer agent-stub` 서브커맨드 —
  같은 `gputeer` 바이너리를 `Command::current_exe()` 로 재실행해
  **실제 별도 OS 프로세스**로 띄운다
- `gputeer coordinator-agent-selftest` — 두 stub 을 자식 프로세스로
  띄우고 실제 handshake 가 되는지/거부되는지 자동 검증
- 새 서명 대상 메시지 `AgentGrantAck`(Coordinator 가 발급한
  `ExecutionGrant` 에 대한 Agent 의 서명된 응답) — `ReplicaAck` 를
  재사용하지 않는다(아래 이유 참조)
- 정상 경로: Coordinator 가 Grant 를 서명해 발급 -> Agent 가
  `read_frame` 으로 검증 -> Agent 가 서명된 ACK 발급 -> Coordinator
  가 ACK 를 검증하고 `grant_id`/`attempt_id` 대조
- 거부 경로 3종: 위조 Grant, 위조 ACK, replay 된 Grant

### Out (명시하지 않으면 범위가 샌다)
- Job 실행 · GPU 할당·CUDA 실행 · 스케줄링
- lease 발급·갱신
- 여러 Agent 동시 처리
- coordinator 고가용성·control-store
- checkpoint 작성·재개 연동
- `runtime-policy` 의 OS 강제 연동(방화벽·Job Object 등)
- 원격 네트워크·relay·TLS·인증서 교환(여전히 127.0.0.1 만)
- 운영용 key protection(K0 평문, 테스트 전용 그대로)
- 프로세스 crash recovery

## 왜 `ReplicaAck` 를 재사용하지 않는가

`ReplicaAck` 는 checkpoint durability 증거용이고
(`proto/artifact.proto:101-102`) `Lifetime::Evidence` 라 replay
nonce 를 검사하지 않는다(`crates/protocol/src/signable.rs:262-285`).
빌려 쓰면 "ACK 가 실제 이 Grant 에 대응하는가" 와 "ACK replay 를
`DurableReplayGuard` 가 거부하는가" 둘 다 증명할 수 없다. 그래서
전용 메시지가 필요하다.

## 새 proto 메시지 (제안, 컴파일 확인 안 됨)

```protobuf
// proto/control.proto 에 추가
message AgentGrantAck {
  uint32 schema_version = 1;
  string grant_id = 2;
  string attempt_id = 3;
  string agent_device_id = 4;

  uint64 issued_at_unix_ms = 5;
  uint64 expires_at_unix_ms = 6;
  bytes nonce = 7;
  bool accepted = 8;

  // domain_tag = "gputeer/v1/grant-ack"
  bytes agent_signature = 90;
}
```

`crates/protocol/build.rs:13-25` 가 이미 `control.proto` 를
protobuf 입력으로 쓰므로 생성 파일은 직접 건드리지 않는다.

## 파일 구조

스트림 소유권상(`docs/contracts/01_스트림_소유권.md:27-38` —
Coordinator·Agent 는 이미 별도 스트림으로 정의되어 있고, 두
디렉터리는 아직 존재하지 않는다) TCP 서버를 `cli` 에 직접 넣지
않는다.

```text
crates/
  coordinator/          (신규 crate)
    Cargo.toml
    src/lib.rs           listener 소유 · Grant 발급 · Agent ACK 검증
  agent/                 (신규 crate)
    Cargo.toml
    src/lib.rs           TCP client 소유 · Grant 검증 · ACK 발급
  cli/
    src/main.rs           서브커맨드 3개 추가
    src/coordinator_agent_selftest.rs  (신규)
  protocol/               AgentGrantAck 의 Domain·Signable·ToCanonicalFields
  crypto/                 framed_ingress 에 FrameType::GrantAck·IngressMessage::GrantAck 추가, 그 외 전부 재사용
```

새 공용 transport crate 는 만들지 않는다 — `framed_ingress` 가
이미 `std::io::Read` 만 요구하고 transport 는 호출자 책임이다
(`crates/crypto/src/framed_ingress.rs:223-266`).

## 단계

| # | 단계 | 스트림 | 완료 기준 | 상태 |
|---|---|---|---|---|
| 1 | `AgentGrantAck` proto 추가 + `ToCanonicalFields`/`Signable` 구현 | Protocol | `cargo build -p gputeer-protocol` 통과, field_number_audit 에 등록됨 | ✅ 2026-08-18 |
| 2 | `framed_ingress` 에 `FrameType::GrantAck`/`IngressMessage::GrantAck` 추가 | Crypto | 기존 framed_ingress 테스트 전부 green + 새 타입 round-trip 테스트 | ✅ 2026-08-18 |
| 3 | `crates/coordinator`·`crates/agent` 신설, 최소 handshake 구현 | Coordinator/Agent(신규 스트림) | 정상 경로 1회 성공 | ✅ 2026-08-18 |
| 4 | `gputeer coordinator-stub`/`agent-stub`/`coordinator-agent-selftest` CLI 배선 | CLI | 별도 PID 확인(`assert_ne!` on process id), exit code 0 | ✅ 2026-08-18 |
| 5 | 거부 경로 3종(위조 Grant·위조 ACK·replay) 을 selftest 에 추가 | CLI/Coordinator/Agent | 셋 다 명시적으로 거부됨을 자동 검증 | ✅ 2026-08-18 |
| 6 | 코덱스 독립 검수 1라운드 이상 | — | `ACCEPTED` | ⬜ |

### 단계 1·2 수행 메모 (2026-08-18)

"확인 안 됨" 3건을 구현 착수 전에 실측으로 검증했다(계획서 §"확인 안 됨" 참조):

1. **`PersistentKeyring` 서명키 재획득 API 없음 — 확인됨, 그러나 막지 않는다.**
   `keys: BTreeMap<...>` 와 `KeyVersion.private` 필드는 비공개이고, `insert_private`
   이후 `SecretSigningKey` 를 다시 꺼내는 공개 API가 없다(`crates/crypto/src/keyring.rs`).
   그러나 기존 `crates/cli/src/selftest.rs:198` 가 이미 같은 상황을 쓰고 있다 —
   `SigningKey` 를 호출자 스코프에 **별도로** 보관해 서명에 쓰고, `PersistentKeyring`
   에는 검증/키 디렉터리 부기용으로만 등록한다. coordinator/agent 구현도 이 패턴을
   그대로 따르면 새 keyring API가 필요 없다.
2. **`AgentGrantAck` 컴파일 — 확인됨, 충돌 없음.** 새 message 이므로 필드 번호는
   message-scope 라 다른 메시지와 충돌하지 않는다. 실제로 `proto/control.proto` 에
   추가하고 `cargo build -p gputeer-protocol` 로 확인했다 — 통과.
3. **domain_tag 23→24종, `canonical_vectors.rs` 하드코딩 — 확인됨, 정확한 위치.**
   `crates/protocol/tests/canonical_vectors.rs:304-324` 의
   `domain_tags_are_32_bytes_and_unique` 가 배열과 `23` 을 하드코딩하고 있었다.
   `Domain::GrantAck` 를 배열에 추가하고 `24` 로 갱신했다.

구현 중 계획서에 없던 **4번째 안전망**도 걸렸다 — `field_number_audit.rs::AUDITED`,
`lifetime_consistency.rs` 의 `declared_lifetime_matches_message_capability`/
`every_signable_is_covered`, `schema_fingerprint.rs`(P0-08 스키마 진화 가드) 전부
새 메시지를 빠뜨리면 실패하도록 설계되어 있었다 — 넷 다 걸렸고, 순서대로 채웠다.
`schema_fingerprint.rs` 는 `UPDATE_SCHEMA_FINGERPRINT=1` 로 갱신했다(새 message
추가는 필드 추가와 달리 schema_version 상향이 필요 없다 — 그 메시지는 태어날 때부터
schema_version=1 이므로 "구버전 검증자가 새 필드를 모른다" 는 위험이 없다).

`cargo test --workspace` 297/0/1(ignored) — 이전 296 + 신규 GrantAck round-trip 1건.
(`gputeer-checkpoint::write_failure::concurrent_startup_gc_treats_not_found_as_normal_race`
가 병렬 실행에서 1회 우연히 실패했다 — `--test-threads=1` 단독 재실행 시 통과. 이
계획과 무관한 기존 테스트의 타이밍 취약성으로 보이며 별도 조사가 필요하다.)

### 단계 3·4 수행 메모 (2026-08-18)

설계 초안(`coordinator_agent_design_clean.md`, 코덱스 `p49` 프롬프트 응답)은
`PersistentKeyring::load`/`save` 로 키를 영속화하는 것을 전제했지만,
단계 1·2 수행 메모의 "확인 안 됨" 1번 검증 결과를 그대로 따라 **더 단순한
경로**를 택했다 — `InMemoryKeyring`(검증 전용, `crates/cli/src/selftest.rs:540`
이 이미 쓰는 패턴)과 호출자가 직접 쥔 `SigningKey` 만 쓰고 `PersistentKeyring`
은 아예 쓰지 않는다. Coordinator/Agent 는 서로 다른 OS 프로세스라 키를
공유할 방법이 필요했는데, 파일 기반 keyring 저장/로드를 새로 검증하는 대신
`coordinator-agent-selftest` 가 두 시드를 결정적으로(라벨 문자열의
BLAKE3-256) 만들어 각 stub 에 `--own-seed`/`--peer-pubkey` hex 인자로
넘긴다. 실제 키 프로비저닝은 이 단계의 범위가 아니다(계획 "Out" 절).

구현 결과:
- `crates/coordinator/src/lib.rs` — `run()`: 리스닝 시작 시 `stdout` 에
  `READY <addr>` 를 찍고, accept 후 `ExecutionGrant` 를 서명해 전송,
  `AgentGrantAck` 를 `read_frame` + `require_replay_checked()` 로 검증해
  `grant_id`/`attempt_id`/`agent_device_id` 를 대조한다.
- `crates/agent/src/lib.rs` — `run()`: connect 후 `read_frame` +
  `require_replay_checked()` 로 Grant 를 검증한 뒤에만 그 값으로
  `AgentGrantAck` 를 만들어 서명해 돌려준다 — 검증되지 않은 값으로
  부작용(ACK 발급)을 만들지 않는다.
- `crates/cli/src/coordinator_agent_selftest.rs` — 두 stub 을
  `Command::current_exe()` 로 별도 프로세스로 띄우고, PID 3개(자기 자신·
  coordinator·agent)가 서로 다른지 확인하고, 양쪽 `RESULT` 줄이 같은
  `grant_id`/`attempt_id`/`agent_device_id` 를 가리키는지 대조한다.
- `crates/cli/src/main.rs` — `coordinator-stub`/`agent-stub`/
  `coordinator-agent-selftest` 세 서브커맨드 배선.

**실제로 실행해서 잡은 결함 (설계 문서에는 없던 것).** 처음 구현에서
`coordinator.stdout.take()` 로 `READY` 줄을 읽은 뒤, 나중에
`coordinator.wait_with_output()` 을 또 불러 나머지 출력(RESULT 줄)을
얻으려 했다 — 그러나 stdout 핸들은 이미 `take()` 로 소비된 뒤라
`wait_with_output()` 은 빈 stdout 을 돌려준다. 실제로 `gputeer
coordinator-agent-selftest` 를 실행해 보고서야 발견했다("RESULT 줄이
기대한 상관관계와 다르다 ... coordinator stdout: " 빈 문자열로 실패).
고친 방법: coordinator 는 `wait_with_output()` 대신 `wait()` 만 쓰고,
stdout·stderr 를 처음부터 끝까지 직접 읽는다. 고친 뒤 5회 연속 실행해
매번 서로 다른 PID 3개로 성공함을 확인했다(타이밍 경합으로 인한
플레이키니스 없음).

구현 중 계획서에 없던 **5번째 안전망**도 걸렸다 —
`crates/protocol/tests/stream_ownership.rs::every_crate_is_covered_by_ownership_rules`
가 새 크레이트 `coordinator`/`agent` 를 감지하고 실패했다(소유권 표
`docs/contracts/01_스트림_소유권.md`·`RULE.md` §4.1 에는 두 스트림이
이미 있었지만, 이 테스트의 `KNOWN` 목록에는 없었다). `KNOWN` 에 추가해
해소했다.

`cargo test --workspace` 297/0/1(ignored) — 프로토콜/크립토 테스트 개수는
변하지 않았다(coordinator/agent 크레이트에는 아직 자체 단위 테스트가
없다 — 검증은 `coordinator-agent-selftest` 실행으로 했다). `cargo build
--workspace` 경고 0.

**남은 것(단계 5)**: 거부 경로 3종(위조 Grant·위조 ACK·replay)은
`coordinator-agent-selftest` 에 아직 없다 — 현재는 정상 경로 1회만
증명한다.

### 단계 6 — 코덱스 독립 검수 (2026-08-18) ✅ 단계 3·4 라운드: `ACCEPTED`(수정 후) · 단계 5 라운드: `ACCEPTED`(1라운드)

**단계 5 커밋(`d7aacdc`) 검수** — `p57` 프롬프트, 1라운드 만에 `ACCEPTED`.
위조 시점(서명 후 변조인지 확인), replay wire bytes 동일성(재인코딩
없이 `frame` 재사용), replay 판정 근거(`signing.rs:826` 인용의
정확성), 위조 ACK 판정 기준의 정직성, 뮤테이션 주장의 논리적 타당성,
기존 정상 경로 판정 보존 여부를 전부 코드 대조로 확인받았다 —
결함 없음. (코덱스는 `cargo test`/`selftest` 재실행이나 뮤테이션
재현 자체는 하지 않았다 — 코드만으로 논리 검증했다고 명시했다.)

### 단계 3·4 코덱스 검수 — 아래는 그 당시 기록

첫 라운드는 `CHANGES_REQUESTED` — `crates/cli/src/coordinator_agent_selftest.rs`
가 coordinator 의 stderr 를 메인 흐름과 동시에 비우지 않아, coordinator 가
OS 파이프 버퍼를 채울 만큼 stderr 에 쓰면(예: 에러 메시지가 커지는 경우)
coordinator 가 쓰기에서 블로킹되고 agent 는 coordinator 의 TCP 응답을
기다리느라 블로킹되어 이 selftest 전체가 교착할 수 있다고 지적했다.
그 외(검증 순서·키 배분·PID 검사·`InMemoryReplayGuard` 사용·
`derive_nonce` 안전성)는 전부 문제없음을 확인받았다.

수정: coordinator 의 stderr 를 `thread::spawn` 으로 만든 별도 스레드가
`read_to_string` 으로 처음부터 끝까지 비우고, 메인 흐름은 `.join()` 으로
그 결과를 나중에 받는다 — stdout(READY/RESULT 줄을 순서대로 읽어야
하므로 메인 스레드에 남긴다)과 stderr(순서 무관, 그냥 다 비우면 된다)를
분리했다. 또한 `derive_nonce` 의 "운영 코드에서 이 패턴을 쓰면 안 되는
이유" 경고를 `coordinator`·`agent` 양쪽에 대칭적으로 명시했다(전에는
coordinator 만 있고 agent 는 coordinator 를 참조하는 수준이었다).

수정 후 재검증: `cargo build --workspace` 경고 0. 5회 연속
`coordinator-agent-selftest` 실행 — 매번 성공, 서로 다른 PID 3개.
`cargo test --workspace` 297/0/1(ignored) 유지.

### 단계 5 수행 메모 (2026-08-18)

코덱스에게 설계를 다시 시켰다(`p56` 프롬프트) — "정직한 프로세스는
자기 서명을 위조하지 않는다" 는 문제를 어떻게 풀지 물었더니, TCP
proxy 로 진짜 중간자 변조를 만드는 안(아키텍처가 복잡해지고 이
계획의 DoD 범위인 서명·replay 검증을 더 증명하지도 않는다)보다
**stub 에 테스트 전용 플래그를 추가해 self-corruption 을 주입하는
안**을 권장받았다. 그대로 채택했다.

- `CoordinatorConfig`/`AgentConfig` 에 `corrupt_own_signature: bool`
  추가 — 서명 직후 마지막 바이트를 `^= 0x01` 로 뒤집는다.
- `CoordinatorConfig::send_grant_twice` / `AgentConfig::expect_replay`
  — 같은 `frame`(재인코딩하지 않은 동일 wire bytes) 을 같은 TCP
  연결에 두 번 쓰고, Agent 는 두 번째 프레임을 다시 `read_frame` 으로
  읽어 거부되는지 확인한다.
  `crates/protocol/src/signing.rs:826`(`Ok(ReplayDecision::Duplicate)
  => return Err(...)`) 을 실제로 읽고 확인했다 — `verify()` 자체가
  중복 nonce 를 만나면 `Err` 를 직접 반환하므로, `require_replay_checked()`
  까지 갈 필요 없이 `read_frame` 단계에서 이미 거부된다.
- `crates/cli/src/coordinator_agent_selftest.rs` 를 `Fixture`/
  `HandshakeOutcome`/`run_handshake()` 로 리팩터링해 4개 시나리오
  (정상 · 위조 Grant · 위조 ACK · replay) 를 공통 오케스트레이션
  위에서 돌린다. 위조 ACK 시나리오는 **Coordinator 쪽 실패만
  판정 기준**으로 삼는다 — Agent 는 자기가 만든 서명이 위조됐는지
  스스로 검증하지 않으므로(검증은 언제나 수신자 책임) Agent 자신은
  정상 종료할 수 있다.

**replay DoD 범위의 정직한 재정의.** 원래 DoD 문구는
"`DurableReplayGuard` 가 거부한다" 였다. 두 stub 은 각각
`InMemoryReplayGuard` 를 쓰고(코덱스가 확인 — 실행 간 replay 상태를
공유하지 않는다), 이 selftest 는 프로세스를 매번 새로 띄우므로
"재시작을 넘는 replay 방어"를 증명하지 않는다. 실제로 증명하는 것은
"같은 프로세스·같은 guard 수명 안에서 동일 wire bytes 두 번째가
거부되는가" 다 — `DurableReplayGuard` 도 같은 `check_and_record`
계약(`Ok(ReplayDecision::Duplicate)`)을 따르므로 검증한 성질 자체는
guard 구현과 무관하지만, "재시작을 견딘다" 는 별개 성질까지 증명한다고
과장하지 않는다. DoD 문구를 "replay guard 가 거부한다"로 정정했다
(위 체크박스 참조) — `DurableReplayGuard` 특정 성질은 이미
`crates/crypto/tests/durable_replay_process.rs`(2026-08-18 앞선 작업)
가 별도 프로세스 경계에서 증명했다.

**뮤테이션 테스트로 비공허성 증명(대표 1건).** 위조 Grant 시나리오의
`corrupt_own_signature` 처리를 `if false && config.corrupt_own_signature`
로 일시 무력화 → 예상대로 selftest 가 "위조된 coordinator_signature
가 거부되지 않았다 — Agent 가 ACK 를 발급했다"로 실패(Agent 가
실제로 `RESULT ok=true` 를 냈다) → 원복 후 재빌드해 4개 시나리오
전부 재통과 확인. 이 세션의 확립된 패턴(백업 → 뮤테이션 → 실패 확인
→ 원복)을 그대로 따랐다.

실제로 5회 연속 실행해 4개 시나리오 전부 매번 통과함을 확인했다
(총 실행 시간 ~1.2초 — 위조 Grant 시나리오에서 Agent 프로세스가
검증 실패로 즉시 종료하면 그 TCP 연결도 즉시 닫히므로, Coordinator
의 ACK 대기가 10초 `IO_TIMEOUT` 을 다 기다리지 않고 연결 종료를
즉시 감지해 빠르게 끝난다 — 당초 예상한 "느릴 수 있다"는 우려는
실측으로 기각됐다). `cargo test --workspace` 297/0/1(ignored) 유지,
`cargo build --workspace` 경고 0.

## 완료 기준 (DoD)

- [x] `gputeer coordinator-agent-selftest` 가 exit code 0 로 정상 handshake 를 증명한다 (2026-08-18, 5회 연속 확인)
- [x] **negative test**: 위조된 `coordinator_signature` 1바이트 변조 시 Agent 가 ACK 를 발급하지 않는다 (2026-08-18, 뮤테이션 테스트로 비공허성 확인)
- [x] **negative test**: 위조된 `agent_signature` 1바이트 변조 시 Coordinator 가 성공 처리하지 않는다 (2026-08-18)
- [x] **negative test**: 동일 Grant wire bytes 를 두 번 보내면 두 번째는 replay guard 가 거부한다 (2026-08-18, `InMemoryReplayGuard` — 아래 "단계 5 수행 메모" 의 범위 설명 참조)
- [x] Coordinator·Agent 가 실제 별도 OS 프로세스(PID)임을 자동 검증에서 확인한다 (2026-08-18, `assert` 3개 PID 상호 비교)
- [ ] `docs/evidence/` 에 schema v2 형식으로 기록(이 계획 자체가 이미 독립 검수 설계이므로, 구현 후 실행자/검수자를 분리한 재검수를 거친다)

## 확인 안 됨 — 구현 전에 반드시 검증할 것

이 설계는 read-only 샌드박스에서 소스만 읽고 작성됐다. 다음은
설계자 스스로 "확인 안 됨" 이라고 표시한 항목이다:

1. **`PersistentKeyring` 에서 등록된 private key 를 서명용으로
   꺼내는 public API가 있는지 확인되지 않았다** — `insert_private`
   (`crates/crypto/src/keyring.rs:398-422`) 는 있지만, 그 뒤 서명에
   쓰려면 별도 `SecretSigningKey` 세션 핸들을 프로세스 메모리에
   따로 유지해야 할 수 있다(설계 문서의 "Keyring과 replay 상태
   소유권" 절 참조). 구현 시작 전에 `crates/crypto/src/keyring.rs`
   전체를 다시 읽고 실제 API를 확인해야 한다.
2. **제안된 `AgentGrantAck` 가 실제로 컴파일되는지** — proto 필드
   번호·타입이 기존 규칙(서명 필드는 90, canonical 규칙 a~i)과
   충돌하지 않는지 `cargo build -p gputeer-protocol` 로 직접
   확인해야 한다.
3. **`Domain::GrantAck` 를 추가하면 domain_tag 총 개수가 23→24 로
   늘어난다** — `canonical_vectors.rs` 의
   `domain_tags_are_32_bytes_and_unique` 류 테스트가 이 숫자를
   하드코딩하고 있을 수 있으므로 갱신이 필요하다.

## 코덱스 원본 설계 전문

전체 코드 스니펫(Coordinator/Agent 핵심 함수, selftest 오케스트레이션
전문)은 이 계획을 실행하는 세션이 다시 코덱스에게 같은 질문을
던지거나, 아래 프롬프트를 재사용해 받을 수 있다 — 원본을 이 문서에
그대로 박아 넣으면 실제로 검증되지 않은 코드가 "계획"이 아니라
"완성된 설계"처럼 보일 위험이 있어, 여기서는 구조와 근거 파일:줄만
남긴다.

## 기준선과 다른 점

기준선(`gputeer_master_plan_FINAL.md`)은 coordinator/agent 의 완전한
설계를 전제하지만, 이 계획은 그 중 "프로세스 경계 + 서명된 Grant
전달 + 거부 경로" 하나만 증명하는 최소 조각이다. 스케줄링·복수
Agent·고가용성 등은 이 문서 이후 별도 계획으로 다룬다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-18 | 최초 작성 — 코덱스 설계 응답(`p49` 프롬프트) 정리 |
