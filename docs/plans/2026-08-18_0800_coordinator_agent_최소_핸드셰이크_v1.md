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
| 1 | `AgentGrantAck` proto 추가 + `ToCanonicalFields`/`Signable` 구현 | Protocol | `cargo build -p gputeer-protocol` 통과, field_number_audit 에 등록됨 | ⬜ |
| 2 | `framed_ingress` 에 `FrameType::GrantAck`/`IngressMessage::GrantAck` 추가 | Crypto | 기존 framed_ingress 테스트 전부 green + 새 타입 round-trip 테스트 | ⬜ |
| 3 | `crates/coordinator`·`crates/agent` 신설, 최소 handshake 구현 | Coordinator/Agent(신규 스트림) | 정상 경로 1회 성공 | ⬜ |
| 4 | `gputeer coordinator-stub`/`agent-stub`/`coordinator-agent-selftest` CLI 배선 | CLI | 별도 PID 확인(`assert_ne!` on process id), exit code 0 | ⬜ |
| 5 | 거부 경로 3종(위조 Grant·위조 ACK·replay) 을 selftest 에 추가 | CLI/Coordinator/Agent | 셋 다 명시적으로 거부됨을 자동 검증 | ⬜ |
| 6 | 코덱스 독립 검수 1라운드 이상 | — | `ACCEPTED` | ⬜ |

## 완료 기준 (DoD)

- [ ] `gputeer coordinator-agent-selftest` 가 exit code 0 로 정상 handshake 를 증명한다
- [ ] **negative test**: 위조된 `coordinator_signature` 1바이트 변조 시 Agent 가 ACK 를 발급하지 않는다
- [ ] **negative test**: 위조된 `agent_signature` 1바이트 변조 시 Coordinator 가 성공 처리하지 않는다
- [ ] **negative test**: 동일 Grant wire bytes 를 두 번 보내면 두 번째는 `DurableReplayGuard` 가 거부한다
- [ ] Coordinator·Agent 가 실제 별도 OS 프로세스(PID)임을 자동 검증에서 확인한다
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
