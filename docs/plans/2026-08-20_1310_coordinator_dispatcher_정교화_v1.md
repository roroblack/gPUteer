# Coordinator dispatcher 정교화 v1

## 목적

로드맵 조각 4의 session dispatcher 계약을 Coordinator에 적용한다. 기존
Grant/ACK, Lease renew, revoke, reconnect, Resume 분기의 wire 동작은
그대로 유지하고, accepted connection 하나의 수명과 오류 경계를 명시한다.

## 구현 범위

- `crates/coordinator/src/lib.rs`
  - `CoordinatorSessionError`를 신설한다.
  - `serve_one_connection()`이 accepted stream의 blocking/timeout 설정과
    기존 Grant-first/Resume 처리를 한 세션 단위로 실행한다.
  - `run()`은 accept 성공 직후 `connection_attempt`를 증가시키고 peer 주소,
    attempt, 오류 종류를 `SESSION_ERROR` 형식으로 기록한다.
  - `Transport`와 `Protocol`은 해당 연결만 버리고 다음 accept로 진행한다.
    `Storage`는 로그 후 즉시 fail-closed 한다.
  - lease DB open 실패도 bind 전에 storage 오류로 기록하고 종료한다.
- `crates/cli/src/coordinator_agent_selftest.rs`
  - 61: truncated TCP 연결 뒤 다음 정상 wire handshake
  - 62: 위조 ACK 서명 뒤 다음 정상 wire handshake
  - 63: 존재하지 않는 lease DB 부모 경로의 startup storage fail-closed
  - 각 다중 연결 하네스는 stdout/stderr reader thread와 120초 hard deadline을
    사용한다. attempt=1 nonce를 검증해야 하므로 두 신규 연결 시나리오의
    최소 wire client는 Agent의 기존 필드·서명·nonce 규칙을 직접 재현한다.

## 오류 분류 근거

`FramingError`의 truncated/stream I/O와 socket 전송·flush·timeout 메시지는
`Transport`다. frame size/type, protobuf/decode, signature/replay,
identity/correlation/nonce 정책 오류는 `Protocol`이다. SQLite/lease-store
open, 발급, revoke, 조회, renew 실패는 `Storage`다. 기존 helper가 문자열
오류를 반환하므로 추출 경계에서 기존 메시지의 안정적인 context와 lease-store
표시를 사용해 분류한다.

## max-connections 결정

`max-connections`는 성공 세션 수가 아니라 `accept()`에 성공한 연결 시도 수를
센다. 따라서 transport/protocol 실패도 카운트된다. 이 값은 실패한 peer가
무한히 재시도해 Coordinator가 종료되지 않는 상황을 막고, 기존 기본값
`1`에서 정상 세션이 끝나는 의미도 보존한다.

## 검증 계획

1. `cargo build --workspace --exclude gputeer-runtime-windows`
2. `cargo test --workspace --exclude gputeer-runtime-windows`
3. `coordinator-agent-selftest`를 120초 hard deadline으로 5회 연속 실행하고
   63개 시나리오의 exit=0과 기존 1~60 회귀를 확인한다.
4. Storage 분류를 임시로 Transport로 바꾸면 시나리오 63이 실패하는지 확인한
   뒤 즉시 원복한다.

## 범위 밖

Resume proto, durable request ledger, Agent 구현, 다중 Agent 경쟁 및 commit/push는
이번 조각에서 변경하지 않는다.
