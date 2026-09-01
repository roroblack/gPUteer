//! 다중 Agent lane 의 Agent 쪽 — Hello 를 먼저 보낸다.
//!
//! # 왜 Hello 가 먼저인가
//!
//! 기존 경로는 Coordinator 가 Grant 를 **먼저** 보낸다. 그러면
//! Coordinator 는 누가 붙었는지 모른 채 발급해야 하므로, Agent 신원이
//! 하나뿐일 때만 성립한다. 여러 Agent 를 받으려면 Coordinator 가
//! **누구인지 안 뒤에** 그 사람 몫의 Grant 를 만들어야 한다.
//!
//! Resume lane 이 이미 Hello-first 다(`DoD-36`). 그 프레임을 그대로
//! 쓰되 `mode` 만 다르게 해 두 lane 이 섞이지 않게 한다.
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 갱신·revoke·Resume   이 lane 은 Hello/Grant/ACK 한 왕복만 한다
//! 실제 작업 실행        Manifest 를 안 싣는다 — 이 조각은 동시 처리만 본다
//! 재접속               한 번 붙고 끝난다. bounded retry 는 기존 lane 에 있다
//! ```

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, InMemoryKeyring, InMemoryReplayGuard,
    IngressMessage, KeyDirectorySource, SigningKey, SystemClock,
};
use gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT;
use gputeer_protocol::pb;
use prost::Message;

use crate::{derive_nonce, AgentConfig};

// `AgentSessionHello.mode` 는 `gputeer_protocol::constants` 에 있다.
//
// ★ 원래 이 파일에 있었다. 2026-08-30 독립 검수가 "보내는 쪽만 알고
//   받는 쪽은 모르는 상수" 라는 결함을 짚어 protocol 로 옮겼다 —
//   Coordinator 가 이 값을 대조하지 않아, Resume lane 용으로 서명된
//   Hello 도 이 lane 이 받아들이고 있었다.
//
//   Resume lane 은 2(`MODE_RESUME`)를 쓴다. 같은 값을 쓰면 Coordinator
//   가 두 lane 을 구분하지 못하고 한쪽 프레임 순서를 다른 쪽으로 해석해
//   조용히 어긋난다 — `serve_resume_connection` 이 "두 wire ordering 을
//   한 포트에서 모호하게 만들지 말라" 고 이미 적어 둔 이유다.


/// Hello -> Grant -> ACK 한 왕복.
pub fn run_multi_agent_session(config: &AgentConfig) -> Result<(), String> {
    // ★ 라이브러리 호출자가 CLI 관문을 지나쳐 여기로 바로 올 수 있다
    //   (독립 검수 6라운드 지적) — 이 lane 이 실제로 시작하는 자리에서
    //   다시 본다.
    if let Some(message) = crate::unsupported_neighbor_report_lane(
        config,
        crate::NeighborReportLane::MultiAgent,
    ) {
        return Err(message);
    }
    let clock = SystemClock;
    let signing_key = SigningKey::from_bytes(&config.own_seed);

    let mut stream = TcpStream::connect(&config.coordinator_addr)
        .map_err(|e| format!("연결 실패({}): {e}", config.coordinator_addr))?;
    // ★ 타임아웃을 반드시 건다 — `framed_ingress` 자체엔 없다.
    let io_timeout = Duration::from_secs(10);
    stream
        .set_read_timeout(Some(io_timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(io_timeout))
        .map_err(|e| e.to_string())?;

    let now = clock.now_unix_ms();
    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        // ★ 테스트 전용으로 다른 lane 의 mode 를 실을 수 있다.
        //   Coordinator 가 이 값을 실제로 대조하는지 증명하려면,
        //   틀린 값을 보내 거부당하는 것을 봐야 한다.
        mode: if config.corrupt_hello_mode {
            gputeer_protocol::constants::MODE_RESUME
        } else {
            MODE_MULTI_AGENT_GRANT
        },
        session_id: config.session_id.clone(),
        node_id: config.agent_device_id.clone(),
        connection_attempt: 0,
        issued_at_unix_ms: now,
        nonce: derive_nonce("multi-agent-hello", &config.agent_device_id, 0),
        ..Default::default()
    };
    hello.node_signature = sign(&signing_key, &hello).to_vec();
    let frame = write_frame(FrameType::SessionHello, &hello.encode_to_vec())
        .map_err(|e| format!("Hello 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .map_err(|e| format!("Hello 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;
    println!("HELLO_SENT node_id={}", config.agent_device_id);

    let mut coordinator_keys = InMemoryKeyring::new();
    coordinator_keys.insert(
        config.coordinator_device_id.clone(),
        config.coordinator_verifying_key,
    );
    let mut replay = InMemoryReplayGuard::new();

    let message = read_frame(
        &mut stream,
        2,
        KeyDirectorySource::Provided(&coordinator_keys),
        &mut replay,
        &clock,
    )
    .map_err(|e| format!("Grant 프레임 읽기/검증 실패: {e}"))?;
    let grant = match &message {
        IngressMessage::Grant(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("Grant replay 검사 실패: {e:?}"))?
            .clone(),
        other => return Err(format!("GRANT_REJECTED: 예상하지 못한 타입: {other:?}")),
    };

    // ★ 받은 Grant 가 **내 것**인가. Coordinator 가 다른 Agent 몫의
    //   Grant 를 잘못 보냈다면 여기서 걸린다 — 서명이 유효해도
    //   내 것이 아니면 받아들이면 안 된다.
    let lease = grant
        .lease
        .as_ref()
        .ok_or_else(|| "GRANT_REJECTED: Lease 가 없다".to_string())?;
    if lease.holder_node_id != config.agent_device_id {
        return Err(format!(
            "GRANT_REJECTED: 이 Grant 의 holder 는 {} 인데 나는 {} 다 — 남의 Lease 다",
            lease.holder_node_id, config.agent_device_id
        ));
    }
    println!(
        "LEASE_ACCEPTED lease_id={} holder={} fence_epoch={}",
        lease.lease_id, lease.holder_node_id, lease.fence_epoch
    );

    let now = clock.now_unix_ms();
    let mut ack = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: grant.grant_id.clone(),
        attempt_id: grant.attempt_id.clone(),
        agent_device_id: config.agent_device_id.clone(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_nonce("multi-agent-ack", &grant.grant_id, 0),
        accepted: true,
        ..Default::default()
    };
    ack.agent_signature = sign(&signing_key, &ack).to_vec();
    let frame = write_frame(FrameType::GrantAck, &ack.encode_to_vec())
        .map_err(|e| format!("ACK 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .map_err(|e| format!("ACK 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;

    println!(
        "RESULT ok=true grant_id={} agent_device_id={}",
        grant.grant_id, config.agent_device_id
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 lane 의 mode 가 Resume lane(2)과 다른가.
    ///
    /// ★ 같으면 Coordinator 가 두 lane 을 구분하지 못하고 한쪽의 프레임
    ///   순서를 다른 쪽으로 해석한다 — 조용히 어긋나는 종류의 결함이다.
    #[test]
    fn this_lane_uses_a_distinct_session_mode() {
        assert_ne!(
            MODE_MULTI_AGENT_GRANT,
            gputeer_protocol::constants::MODE_RESUME,
            "Resume lane 과 같은 mode 를 쓰면 두 wire ordering 이 모호해진다"
        );
    }

    /// Agent 마다 Hello nonce 가 다른가.
    ///
    /// 같으면 두 번째 Agent 의 Hello 가 replay 로 거부된다.
    #[test]
    fn each_agent_derives_a_distinct_hello_nonce() {
        assert_ne!(
            derive_nonce("multi-agent-hello", "agent-a", 0),
            derive_nonce("multi-agent-hello", "agent-b", 0),
            "두 Agent 의 Hello nonce 가 같다 — 뒤엣것이 replay 로 거부된다"
        );
    }

    /// Hello 와 ACK 의 nonce 가 서로 다른가.
    ///
    /// 같은 연결 안에서 겹치면 두 번째가 replay 로 거부된다.
    #[test]
    fn hello_and_ack_nonces_do_not_collide() {
        assert_ne!(
            derive_nonce("multi-agent-hello", "x", 0),
            derive_nonce("multi-agent-ack", "x", 0),
            "같은 연결 안에서 nonce 가 겹친다"
        );
    }
}
