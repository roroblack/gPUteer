//! 다중 Agent 동시 처리 — 별도 lane.
//!
//! # 왜 기존 `run()` 을 안 고치는가
//!
//! `run()` 은 **의도적으로 순차**다. `accept()` → 동기
//! `serve_one_connection()` → 완료 후 다음 `accept()`. 그 위에 87개
//! selftest 시나리오가 얹혀 있고, 그중 다수는 "이 순서로 이 프레임이
//! 온다" 를 정확히 검사한다. 그 함수를 동시 처리로 바꾸면 87개가 전부
//! 흔들린다.
//!
//! 그래서 **새 lane** 을 만든다. 기존 경로는 바이트 하나 안 바뀌고,
//! `--multi-agent true` 를 준 경우에만 이쪽으로 온다.
//!
//! # `DoD-40` 이 남긴 것을 여기서 연다
//!
//! `DoD-40` 설계 조사는 진짜 다중 Agent 경쟁이 지금 아키텍처로는
//! 표현 자체가 안 된다고 정직하게 판정했다.
//!
//! ```text
//! 1  Coordinator 설정에 Agent identity/key 가 하나뿐이다
//! 2  Grant 를 먼저 보내므로 누가 붙었는지 모른 채 발급한다
//! 3  connection_attempt 가 Agent 별이 아니라 전체 accept 순번이다
//! ```
//!
//! 이 모듈이 그 셋을 각각 연다.
//!
//! ```text
//! 1  --extra-agents 로 여러 신원을 등록한다
//! 2  Hello 를 먼저 받아 **누구인지 안 뒤에** 그 사람 몫의 Grant 를 만든다
//! 3  Agent 별로 자기 nonce 순번을 쓴다(연결마다 0 에서 시작)
//! ```
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 스케줄링       누구에게 무엇을 줄지 정하지 않는다. 붙은 Agent 에게
//!                그 Agent 몫의 Lease 를 줄 뿐이다. 실제 배치는
//!                crates/scheduler 의 몫이고 아직 production 미연결이다
//! 같은 GPU 경쟁   두 Agent 가 같은 자원을 두고 다투는 상황을 만들지
//!                않는다 — inventory/reservation 계층이 그것을 다룬다
//! 갱신·revoke·Resume  이 lane 은 Grant/ACK 한 왕복만 한다. 나머지는
//!                기존 순차 lane 에 있고, 동시 처리로 옮기는 것은
//!                별도 조각이다
//! Coordinator HA  여전히 Coordinator 는 하나다
//! ```

use std::collections::BTreeMap;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, InMemoryKeyring, InMemoryReplayGuard,
    IngressMessage, KeyDirectorySource, SigningKey, SystemClock, VerifyingKey,
};
use gputeer_protocol::pb;
use prost::Message;

use crate::{issue_grant, CoordinatorConfig, CoordinatorLeaseStore};

/// 여러 Agent 를 동시에 받는다.
///
/// 각 연결은 자기 스레드에서 처리된다. 공유되는 것은 Lease 저장소와
/// replay 방어뿐이며 둘 다 잠금 뒤에 있다.
pub fn run_multi_agent(config: CoordinatorConfig) -> Result<(), String> {
    let agents = parse_agent_directory(
        &config.agent_device_id,
        config.agent_verifying_key,
        config.extra_agents.as_deref(),
    )?;
    require_multiple_identities(agents.len())?;

    let mut keyring = InMemoryKeyring::new();
    for (device_id, key) in &agents {
        keyring.insert(device_id.clone(), *key);
    }

    let lease_store = match &config.lease_db_path {
        Some(path) => {
            let store = CoordinatorLeaseStore::open(path)
                .map_err(|error| format!("lease store 저장소 열기 실패: {error}"))?;
            if !store.is_durable() {
                return Err(format!(
                    "lease store 저장소가 영속이 아니다(lease_db_path={path:?})"
                ));
            }
            Some(store)
        }
        None => None,
    };

    let listener = TcpListener::bind(&config.listen).map_err(|e| format!("bind 실패: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    println!("READY {address}");
    println!("MULTI_AGENT agents={}", agents.len());
    use std::io::Write as _;
    std::io::stdout().flush().map_err(|e| e.to_string())?;

    let shared = Arc::new(Shared {
        config,
        keyring,
        // ★ 두 자원을 **하나의 잠금**으로 묶지 않는다. 서로 다른 것을
        //   지키므로 따로 잠근다 — 함께 잠그면 한 Agent 의 저장소
        //   작업이 다른 Agent 의 replay 검사를 막는다.
        lease_store: Mutex::new(lease_store),
        replay: Mutex::new(InMemoryReplayGuard::new()),
        served: AtomicU32::new(0),
    });

    let expected = shared.config.max_connections;
    let mut handles = Vec::new();
    let mut accepted = 0u32;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("listener nonblocking failed: {e}"))?;

    while accepted < expected {
        let (stream, peer) = accept_with_deadline(
            &listener,
            Duration::from_millis(shared.config.accept_timeout_ms),
        )?;
        accepted += 1;
        println!("CONNECTION_ACCEPTED peer={peer}");
        let shared = Arc::clone(&shared);
        handles.push(
            std::thread::Builder::new()
                .name(format!("gputeer-session-{accepted}"))
                .spawn(move || serve(&shared, stream, peer))
                .map_err(|e| format!("세션 스레드 기동 실패: {e}"))?,
        );
    }

    // ★ 모든 세션을 기다린다. 여기서 안 기다리면 프로세스가 먼저 끝나
    //   진행 중인 세션이 끊긴다 — 그러면 Agent 쪽은 원인을 알 수 없는
    //   연결 종료를 본다.
    let mut failures = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(message)) => failures.push(message),
            // 스레드가 panic 했다. 조용히 넘기면 세션 하나가 통째로
            // 사라진 것을 아무도 모른다.
            Err(_) => failures.push("세션 스레드가 panic 했다".to_string()),
        }
    }
    if !failures.is_empty() {
        return Err(format!("세션 {}건 실패: {}", failures.len(), failures.join(" / ")));
    }

    println!(
        "MULTI_AGENT_DONE served={}",
        shared.served.load(Ordering::SeqCst)
    );
    Ok(())
}

/// 신원이 둘 이상인가.
///
/// ★ 하나뿐이면 이 lane 을 쓸 이유가 없다. 조용히 도는 대신 거부한다 —
///   "다중 Agent 를 켰다" 고 믿는데 실제로는 하나만 도는 상태를
///   만들지 않는다.
fn require_multiple_identities(count: usize) -> Result<(), String> {
    if count < 2 {
        return Err(format!(
            "MULTI_AGENT_REFUSED: Agent 신원이 {count}개다 — 이 lane 은 2개 이상일 때만 \
             의미가 있다. --extra-agents 로 더 등록하거나 기존 순차 경로를 쓰라"
        ));
    }
    Ok(())
}

struct Shared {
    config: CoordinatorConfig,
    keyring: InMemoryKeyring,
    lease_store: Mutex<Option<CoordinatorLeaseStore>>,
    replay: Mutex<InMemoryReplayGuard>,
    served: AtomicU32,
}

/// 연결 하나를 끝까지 처리한다.
///
/// ```text
/// 1  Hello 를 받아 **누구인지** 안다
/// 2  그 Agent 몫의 Grant 를 만들어 보낸다
/// 3  ACK 를 받아 검증한다
/// ```
fn serve(shared: &Shared, mut stream: TcpStream, peer: std::net::SocketAddr) -> Result<(), String> {
    // ★ 타임아웃을 반드시 건다. 없으면 헤더만 보내고 멈춘 상대 하나가
    //   그 스레드를 영원히 붙잡는다 — `framed_ingress` 가 타임아웃을
    //   갖지 않는다는 계약을 이미 실측으로 확인했다.
    // ★ accept 된 스트림을 **명시적으로** blocking 으로 되돌린다.
    //
    //   Windows 에서는 listener 의 nonblocking 설정이 accept 된
    //   소켓에 전파되어, 그대로 읽으면
    //   `WSAEWOULDBLOCK`(10035) 이 난다. `DoD-35` 가 이미 같은
    //   버그를 겪었고, 이번에도 정확히 그 오류로 재현됐다 —
    //   두 Agent 가 Hello 까지는 갔는데 ACK 읽기에서 전부 실패.
    stream
        .set_nonblocking(false)
        .map_err(|e| format!("blocking 복원 실패: {e}"))?;
    let io_timeout = Duration::from_secs(10);
    stream
        .set_read_timeout(Some(io_timeout))
        .map_err(|e| format!("read timeout 설정 실패: {e}"))?;
    stream
        .set_write_timeout(Some(io_timeout))
        .map_err(|e| format!("write timeout 설정 실패: {e}"))?;

    let clock = SystemClock;
    let agent_device_id = read_hello(shared, &mut stream, &clock)?;
    println!("SESSION_HELLO peer={peer} agent_device_id={agent_device_id}");

    // ★ 이 Agent 몫의 설정을 만든다. 식별자를 Agent 마다 갈라 놓지
    //   않으면 두 Agent 가 같은 `lease_id` 를 두고 다투게 되고, 그건
    //   이 조각이 다루려는 문제(동시 처리)가 아니라 전혀 다른
    //   문제(자원 경쟁)가 된다.
    let per_agent = scope_config_to_agent(&shared.config, &agent_device_id);

    let signing_key = SigningKey::from_bytes(&shared.config.own_seed);
    let now = clock.now_unix_ms();
    let grant = {
        let mut store = lock(&shared.lease_store);
        issue_grant(&per_agent, &mut store, &signing_key, now, 0)?
    };

    let frame = write_frame(FrameType::Grant, &grant.encode_to_vec())
        .map_err(|e| format!("Grant 프레임 인코딩 실패: {e}"))?;
    use std::io::Write as _;
    stream
        .write_all(&frame)
        .map_err(|e| format!("Grant 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;

    let ack = read_ack(shared, &mut stream, &clock)?;
    if ack.agent_device_id != agent_device_id {
        return Err(format!(
            "ACK_REJECTED: Hello 는 {agent_device_id} 인데 ACK 는 {} 다 — \
             한 연결 안에서 신원이 바뀌었다",
            ack.agent_device_id
        ));
    }
    if ack.grant_id != grant.grant_id {
        return Err(format!(
            "ACK_REJECTED: grant_id 불일치 — 발급 {} != ACK {}",
            grant.grant_id, ack.grant_id
        ));
    }
    if !ack.accepted {
        return Err(format!("ACK_REJECTED: {agent_device_id} 가 거절했다"));
    }

    shared.served.fetch_add(1, Ordering::SeqCst);
    println!(
        "RESULT ok=true grant_id={} attempt_id={} agent_device_id={}",
        grant.grant_id, grant.attempt_id, agent_device_id
    );
    Ok(())
}

fn read_hello(
    shared: &Shared,
    stream: &mut TcpStream,
    clock: &SystemClock,
) -> Result<String, String> {
    let message = {
        let mut replay = lock(&shared.replay);
        read_frame(
            stream,
            1,
            KeyDirectorySource::Provided(&shared.keyring),
            &mut *replay,
            clock,
        )
        .map_err(|e| format!("Hello 프레임 읽기/검증 실패: {e}"))?
    };
    match &message {
        IngressMessage::SessionHello(verified) => {
            // ★ `require_replay_checked()` — 이 저장소의 다른 ShortLived
            //   수신부와 같은 이유(§10). 검사했다는 사실을 타입으로
            //   확인하지 않으면 조용히 안 한 채로 지나갈 수 있다.
            let hello = verified
                .require_replay_checked()
                .map_err(|e| format!("Hello replay 검사 실패: {e:?}"))?;
            if hello.node_id.is_empty() {
                return Err("HELLO_REJECTED: node_id 가 비었다".into());
            }
            Ok(hello.node_id.clone())
        }
        other => Err(format!("HELLO_REJECTED: 예상하지 못한 타입: {other:?}")),
    }
}

fn read_ack(
    shared: &Shared,
    stream: &mut TcpStream,
    clock: &SystemClock,
) -> Result<pb::AgentGrantAck, String> {
    let message = {
        let mut replay = lock(&shared.replay);
        read_frame(
            stream,
            1,
            KeyDirectorySource::Provided(&shared.keyring),
            &mut *replay,
            clock,
        )
        .map_err(|e| format!("ACK 프레임 읽기/검증 실패: {e}"))?
    };
    match &message {
        IngressMessage::GrantAck(verified) => Ok(verified
            .require_replay_checked()
            .map_err(|e| format!("ACK replay 검사 실패: {e:?}"))?
            .clone()),
        other => Err(format!("ACK_REJECTED: 예상하지 못한 타입: {other:?}")),
    }
}

/// 이 Agent 몫으로 식별자를 갈라 놓은 설정.
///
/// ★ 두 Agent 가 같은 `lease_id`·`attempt_id`·`grant_id` 를 쓰면
///   저장소가 그것을 같은 것으로 보고 하나가 다른 하나를 덮거나
///   identity 충돌로 거부한다. 이 조각이 보려는 것은 "여러 연결을
///   동시에 처리하는가" 이지 "같은 자원을 두고 다투면 어떻게 되는가"
///   가 아니다 — 후자를 우연히 섞으면 무엇을 검증했는지 알 수 없다.
fn scope_config_to_agent(base: &CoordinatorConfig, agent_device_id: &str) -> CoordinatorConfig {
    let mut scoped = base.clone();
    scoped.agent_device_id = agent_device_id.to_string();
    scoped.lease_id = scoped_id(&base.lease_id, agent_device_id);
    scoped.job_id = scoped_id(&base.job_id, agent_device_id);
    scoped.attempt_id = scoped_id(&base.attempt_id, agent_device_id);
    scoped.grant_id = scoped_id(&base.grant_id, agent_device_id);
    scoped
}

/// 식별자 하나를 이 Agent 몫으로 갈라 놓는다. 순수 함수다 —
/// 그래서 테스트가 `CoordinatorConfig` 48개 필드를 채우지 않아도 된다.
fn scoped_id(base_id: &str, agent_device_id: &str) -> String {
    format!("{base_id}-{}", short_tag(agent_device_id))
}

/// Agent 식별자에서 짧고 안정적인 꼬리표를 만든다.
///
/// 이름을 그대로 이어 붙이면 식별자가 길어지고 읽기 어렵다. 해시를
/// 쓰면 같은 Agent 는 항상 같은 꼬리표를 받아 재접속에도 안정적이다.
fn short_tag(agent_device_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gputeer/v1/multi-agent-scope");
    hasher.update(agent_device_id.as_bytes());
    hasher.finalize().to_hex()[..8].to_string()
}

/// `--extra-agents` 를 포함한 전체 Agent 신원 목록.
///
/// 형식: `id=pubkeyhex;id2=pubkeyhex2`
///
/// ★ 중복된 `device_id` 는 거부한다. 같은 이름에 다른 키를 등록하면
///   어느 키로 검증할지가 등록 순서에 달리게 되고, 그건 조용히 잘못된
///   서명을 통과시키는 길이다.
fn parse_agent_directory(
    primary_device_id: &str,
    primary_key: VerifyingKey,
    extra: Option<&str>,
) -> Result<BTreeMap<String, VerifyingKey>, String> {
    let mut agents = BTreeMap::new();
    agents.insert(primary_device_id.to_string(), primary_key);

    let Some(raw) = extra else {
        return Ok(agents);
    };
    for entry in raw.split(';').filter(|e| !e.is_empty()) {
        let (device_id, key_hex) = entry
            .split_once('=')
            .ok_or_else(|| format!("--extra-agents 항목에 '=' 가 없다: {entry:?}"))?;
        if device_id.is_empty() {
            return Err(format!("--extra-agents 항목의 device_id 가 비었다: {entry:?}"));
        }
        let key = crate::hex_to_verifying_key(key_hex)
            .map_err(|e| format!("--extra-agents 의 공개키를 읽지 못했다({device_id}): {e}"))?;
        if agents.insert(device_id.to_string(), key).is_some() {
            return Err(format!(
                "--extra-agents 에 device_id 가 중복됐다: {device_id} — \
                 어느 키로 검증할지가 등록 순서에 달리게 된다"
            ));
        }
    }
    Ok(agents)
}

/// ★ 잠금이 poisoned 여도 계속 간다.
///
///   어떤 세션 스레드가 이 상태를 들고 panic 했다는 뜻인데, 그렇다고
///   나머지 세션을 전부 죽이면 한 Agent 의 실패가 다른 Agent 들의
///   작업까지 끊는다. 실패한 세션은 이미 `join()` 에서 보고된다.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn accept_with_deadline(
    listener: &TcpListener,
    timeout: Duration,
) -> Result<(TcpStream, std::net::SocketAddr), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok(connection) => return Ok(connection),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err("accept timeout exceeded".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(format!("accept failed: {error}")),
        }
    }
}

/// Grant 서명에 쓰는 키를 밖으로 내보내지 않는다 — `sign` 이 여기서만
/// 쓰이는지 확인하기 위한 표시.
#[allow(dead_code)]
fn _sign_is_used_here_only(key: &SigningKey, ack: &pb::AgentGrantAck) -> Vec<u8> {
    sign(key, ack).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 서로 다른 Agent 는 서로 다른 식별자를 받는다.
    ///
    /// ★ 같으면 두 Agent 가 같은 Lease 를 두고 다투게 되고, 이 조각이
    ///   보려는 "동시 처리" 대신 전혀 다른 문제(자원 경쟁)를 검증하게 된다.
    #[test]
    fn each_agent_gets_a_distinct_id() {
        assert_ne!(
            scoped_id("L", "agent-a"),
            scoped_id("L", "agent-b"),
            "두 Agent 가 같은 식별자를 받았다"
        );
    }

    /// 같은 Agent 는 항상 같은 식별자를 받는다.
    ///
    /// 재접속 때 달라지면 저장된 Lease 를 못 찾는다.
    #[test]
    fn the_same_agent_always_gets_the_same_id() {
        assert_eq!(scoped_id("L", "agent-a"), scoped_id("L", "agent-a"));
    }

    /// 기본 식별자가 다르면 결과도 다르다.
    ///
    /// 한 Agent 의 lease/job/attempt/grant 가 모두 같은 값이 되면
    /// 저장소가 그것들을 구분하지 못한다.
    #[test]
    fn different_base_ids_stay_different() {
        assert_ne!(scoped_id("L", "agent-a"), scoped_id("J", "agent-a"));
    }

    /// 신원이 하나뿐이면 이 lane 을 거부한다.
    #[test]
    fn a_single_identity_is_refused() {
        let error = require_multiple_identities(1).expect_err("하나뿐인데 통과했다");
        assert!(
            error.contains("MULTI_AGENT_REFUSED"),
            "거부 사유가 식별되지 않는다: {error}"
        );
        require_multiple_identities(2).expect("둘이면 통과해야 한다");
    }

    fn zero_key() -> VerifyingKey {
        crate::hex_to_verifying_key(&"0".repeat(64)).expect("key")
    }

    /// 같은 device_id 가 두 번 오면 거부한다.
    ///
    /// ★ 어느 키로 검증할지가 등록 순서에 달리면, 조용히 잘못된 서명을
    ///   통과시킬 수 있다.
    #[test]
    fn a_duplicate_device_id_is_refused() {
        let key_hex = "0".repeat(64);
        let error = parse_agent_directory(
            "agent-a",
            zero_key(),
            Some(&format!("agent-b={key_hex};agent-b={key_hex}")),
        )
        .expect_err("중복이 통과했다");
        assert!(error.contains("중복"), "{error}");
    }

    /// 기본 신원과 같은 이름을 추가로 등록해도 거부한다.
    #[test]
    fn shadowing_the_primary_identity_is_refused() {
        let error = parse_agent_directory(
            "agent-a",
            zero_key(),
            Some(&format!("agent-a={}", "0".repeat(64))),
        )
        .expect_err("기본 신원 덮어쓰기가 통과했다");
        assert!(error.contains("중복"), "{error}");
    }

    /// 목록에 적은 신원이 전부 등록된다.
    #[test]
    fn every_listed_agent_is_registered() {
        let key_hex = "0".repeat(64);
        let agents = parse_agent_directory(
            "agent-a",
            zero_key(),
            Some(&format!("agent-b={key_hex};agent-c={key_hex}")),
        )
        .expect("directory");
        assert_eq!(agents.len(), 3, "등록된 신원 수가 다르다");
        for expected in ["agent-a", "agent-b", "agent-c"] {
            assert!(agents.contains_key(expected), "{expected} 가 없다");
        }
    }
}
