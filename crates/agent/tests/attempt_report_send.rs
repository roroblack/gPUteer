//! Agent 가 `AttemptReport` 를 **언제 보내고 언제 안 보내는가.**
//!
//! ★ 여기서는 Agent 의 production 진입점(`run()`)을 그대로 부르고,
//!   테스트가 Coordinator 역할을 한다 — Grant 를 보내고 ACK 를 받는다.
//!
//! # 여기서 재지 **못하는** 것 (정직하게 적는다)
//!
//! ```text
//! 실제 종료를 관측한 뒤의 발신    `run()` 이 종료를 관측하려면 자식
//!                                프로세스를 **실제로 띄워야** 한다
//!                                (`--i-understand-this-executes-untrusted-code`
//!                                + 서명된 nested Manifest + 실행 가능한
//!                                entrypoint). 이 테스트는 그것을 하지
//!                                않는다 — 남의 코드를 실행하는 경로를
//!                                단위 테스트에서 켜지 않는다.
//!
//!                                보고 **내용**은 `src/report.rs` 의 단위
//!                                테스트가, 보고가 **wire 를 건너 저장되는
//!                                것**은 `gputeer-coordinator` 의
//!                                `tests/attempt_report_ingress.rs` 가 잰다.
//!                                이 파일이 잰 것은 그 사이의 관문이다.
//! ```

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gputeer_agent::parse_config_from_args;
use gputeer_crypto::{sign, FrameType, SigningKey};
use gputeer_protocol::nonce::derive_replay_nonce;
use gputeer_protocol::pb;
use prost::Message;

const COORDINATOR_SEED: [u8; 32] = [0x31; 32];
const AGENT_SEED: [u8; 32] = [0x22; 32];
const COORDINATOR_ID: &str = "01JCOORDREPORTSEND000001";
const AGENT_ID: &str = "01JAGENTREPORTSEND000001";
const GRANT_ID: &str = "01JGRANTREPORTSEND000001";
const ATTEMPT_ID: &str = "01JATTEMPTREPORTSEND0001";
const LEASE_ID: &str = "01JLEASEREPORTSEND000001";
const JOB_ID: &str = "01JJOBREPORTSEND00000001";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("시계")
        .as_millis() as u64
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn signed_grant() -> pb::ExecutionGrant {
    let key = SigningKey::from_bytes(&COORDINATOR_SEED);
    let now = now_ms();
    let mut lease = pb::Lease {
        schema_version: 1,
        lease_id: LEASE_ID.into(),
        job_id: JOB_ID.into(),
        attempt_id: ATTEMPT_ID.into(),
        fence_epoch: 1,
        coordinator_term: 1,
        holder_node_id: AGENT_ID.into(),
        member_node_ids: vec![AGENT_ID.into()],
        issuing_coordinator_id: COORDINATOR_ID.into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 600_000,
        renew_after_unix_ms: now + 300_000,
        max_total_duration_seconds: 86_400,
        ..Default::default()
    };
    lease.coordinator_signature = sign(&key, &lease).to_vec();

    let mut grant = pb::ExecutionGrant {
        schema_version: 1,
        grant_id: GRANT_ID.into(),
        attempt_id: ATTEMPT_ID.into(),
        coordinator_device_id: COORDINATOR_ID.into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_replay_nonce("grant", GRANT_ID, 0),
        lease: Some(lease),
        ..Default::default()
    };
    grant.coordinator_signature = sign(&key, &grant).to_vec();
    grant
}

fn write_frame_body(stream: &mut TcpStream, frame_type: FrameType, body: &[u8]) {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(frame_type as u8);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    stream.write_all(&out).expect("프레임 전송");
    stream.flush().expect("flush");
}

/// 헤더 5바이트를 읽는다. `None` 이면 상대가 더 보낸 것이 없다.
fn read_frame_type(stream: &mut TcpStream) -> Option<u8> {
    let mut header = [0u8; 5];
    match stream.read_exact(&mut header) {
        Ok(()) => {
            let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
            let mut body = vec![0u8; len];
            stream.read_exact(&mut body).expect("프레임 본문");
            Some(header[0])
        }
        Err(_) => None,
    }
}

struct SessionOutcome {
    agent_result: Result<(), String>,
    /// Agent 가 ACK 뒤에 **무엇을 더 보냈는가.**
    frames_after_ack: Vec<u8>,
}

/// Agent 를 실제로 띄우고 Grant/ACK 한 판을 돈다.
fn run_agent_session(extra: &[&str]) -> SessionOutcome {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().expect("주소").to_string();

    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let fence_db = dir.path().join("fence.sqlite3");
    let checkpoints = dir.path().join("checkpoints");

    let coordinator_pubkey = hex(
        SigningKey::from_bytes(&COORDINATOR_SEED)
            .verifying_key()
            .as_bytes(),
    );
    let mut argv: Vec<String> = [
        "--connect",
        &address,
        "--own-seed",
        &hex(&AGENT_SEED),
        "--peer-pubkey",
        &coordinator_pubkey,
        "--coordinator-device-id",
        COORDINATOR_ID,
        "--agent-device-id",
        AGENT_ID,
        "--disable-reconnect",
        "true",
        "--fence-db",
        fence_db.to_str().expect("경로"),
        "--checkpoint-root",
        checkpoints.to_str().expect("경로"),
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    argv.extend(extra.iter().map(|s| s.to_string()));

    let agent = std::thread::spawn(move || {
        gputeer_agent::run(parse_config_from_args(&argv).expect("설정 파싱"))
    });

    let (mut stream, _peer) = listener.accept().expect("Agent 연결");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("읽기 타임아웃");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("쓰기 타임아웃");

    // D2 — Agent 가 먼저 Hello(FRESH) 를 보낸다.
    let hello_type = read_frame_type(&mut stream).expect("Agent 가 Hello 를 먼저 보내야 한다");
    assert_eq!(hello_type, FrameType::SessionHello as u8, "첫 프레임은 Hello 다(D2)");

    let grant = signed_grant();
    write_frame_body(&mut stream, FrameType::Grant, &grant.encode_to_vec());

    // ACK 를 먼저 받는다.
    let ack_type = read_frame_type(&mut stream).expect("Agent 가 ACK 를 보내야 한다");
    assert_eq!(ack_type, FrameType::GrantAck as u8, "첫 응답은 ACK 다");

    // 그 뒤에 오는 것을 전부 모은다.
    stream
        .set_read_timeout(Some(Duration::from_millis(1_500)))
        .expect("짧은 타임아웃");
    let mut frames_after_ack = Vec::new();
    while let Some(frame_type) = read_frame_type(&mut stream) {
        frames_after_ack.push(frame_type);
    }

    let agent_result = agent.join().expect("Agent 스레드가 panic 하지 않았다");
    SessionOutcome {
        agent_result,
        frames_after_ack,
    }
}

/// 기본값은 **끄기**다 — ACK 뒤에 아무것도 더 보내지 않는다.
///
/// ★ 이 대조가 없으면 "항상 보낸다" 로 바꿔도 아래 테스트가 통과한다.
#[test]
fn without_the_flag_nothing_extra_follows_the_ack() {
    let outcome = run_agent_session(&[]);
    assert!(
        outcome.frames_after_ack.is_empty(),
        "끄고 있는데 무언가 더 왔다: {:?}",
        outcome.frames_after_ack
    );
    assert!(
        outcome.agent_result.is_ok(),
        "기존 handshake 가 깨졌다: {:?}",
        outcome.agent_result
    );
}

/// ★ **관측된 종료가 없으면 보내지 않고, 조용히 넘기지도 않는다.**
///
/// 실행이 꺼져 있으면 종료를 관측한 적이 없다. 그 상태에서 빈 보고를
/// 만들어 보내면 그것이 곧 지어낸 값이다(`CLAUDE.md` §1). 그렇다고
/// 조용히 건너뛰면 운영자는 보고가 간 줄 안다 — 그래서 **오류로 끝난다.**
#[test]
fn the_flag_without_an_observed_exit_refuses_instead_of_inventing_one() {
    let outcome = run_agent_session(&["--send-attempt-report", "true"]);
    let error = outcome
        .agent_result
        .expect_err("관측 없이 보고를 보내면 안 된다");
    assert!(
        error.contains("ATTEMPT_REPORT_REFUSED"),
        "거부 사유가 이 계층의 것이라고 말해야 한다: {error}"
    );
    assert!(
        error.contains("관측된 워크로드 종료가 없다"),
        "무엇이 없어서 거부했는지 말해야 한다: {error}"
    );
    assert!(
        outcome.frames_after_ack.is_empty(),
        "거부했는데 프레임이 나갔다: {:?}",
        outcome.frames_after_ack
    );
}
