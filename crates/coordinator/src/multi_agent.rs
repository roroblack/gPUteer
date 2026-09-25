//! 다중 Agent 동시 처리 — 별도 lane.
//!
//! # 왜 기존 `run()` 을 안 고치는가
//!
//! `run()` 은 **의도적으로 순차**다. `accept()` → 동기
//! `serve_one_connection()` → 완료 후 다음 `accept()`. 그 위에 selftest
//! 시나리오가 잔뜩 얹혀 있고, **그중 다수**는 "이 순서로 이 프레임이 온다"
//! 를 정확히 검사한다 — 그 함수를 동시 처리로 바꾸면 그것들이 흔들린다.
//!
//! ★ 전에 여기 개수를 적어 뒀는데 시나리오가 늘면서 낡았다(독립 검수
//!   11라운드). 개수를 손으로 적지 않는다 — 정확한 수는 selftest 출력이
//!   말한다. 그리고 "전부" 도 과장이었다: 프레임 순서를 안 보는 시나리오도
//!   있다.
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
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, InMemoryKeyring, InMemoryReplayGuard,
    IngressMessage, KeyDirectorySource, SigningKey, SystemClock, VerifyingKey,
};
use gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT;
use gputeer_protocol::pb;
use prost::Message;

use crate::{issue_grant, validate_device_id, CoordinatorConfig, CoordinatorLeaseStore};

/// 여러 Agent 를 동시에 받는다.
///
/// 각 연결은 자기 스레드에서 처리된다. `Shared` 가 모든 세션에 걸쳐
/// 공유하는 것은 Lease 저장소·replay 방어·설정·keyring·처리 수·동시
/// 세션 관문(`Condvar` 포함)·최대 동시치다 — 상태를 가진 것은 잠금 뒤에 있다.
///
/// ★ 전에 "Lease 저장소와 replay 방어뿐" 이라 적었는데 사실이 아니었다
///   (독립 검수 11라운드).
pub fn run_multi_agent(config: CoordinatorConfig) -> Result<(), String> {
    // ★ 2026-09-25 (결함 408 · 재검수 96) — 풀은 순차 lane 이다. 이 진입점을 바로 부르는 라이브러리 호출자도 막는다.
    if config.pool_mode {
        return Err(
            "STARTUP_REFUSED: POOL_LANE_CONFLICT — 풀 모드는 순차 lane 이다. multi-agent lane 으로 풀 예약을 내주지 않는다"
                .to_string(),
        );
    }
    // ★ 라이브러리 호출자가 CLI 관문을 지나쳐 여기로 바로 올 수 있다
    //   (독립 검수 6라운드 지적) — 이 lane 이 실제로 시작하는 자리에서
    //   다시 본다.
    if let Some(message) =
        crate::unsupported_neighbor_report_lane(&config, crate::NeighborReportLane::MultiAgent)
    {
        return Err(message);
    }
    // ★ 종료 보고도 같은 자리에서 막는다 — 이 lane 의 `serve()` 는
    //   자기 세션 루프를 따로 갖고 있어 `serve_one_connection()` 을
    //   부르지 않는다. 즉 `AttemptReport` 수신 구간이 **없다.**
    if let Some(message) =
        crate::unsupported_attempt_report_lane(&config, crate::NeighborReportLane::MultiAgent)
    {
        return Err(message);
    }
    // ★ 결함 ㉟ — heartbeat 수신 구간도 이 lane 에는 없다.
    if let Some(message) =
        crate::unsupported_heartbeat_lane(&config, crate::NeighborReportLane::MultiAgent)
    {
        return Err(message);
    }
    let agents = parse_agent_directory(
        &config.agent_device_id,
        config.agent_verifying_key,
        config.extra_agents.as_deref(),
    )?;
    require_multiple_identities(agents.len())?;
    // ★ Coordinator 자신의 신원도 검사한다(2026-08-30 독립 검수 2라운드).
    //   초안은 Agent 쪽만 봤다 — 이 값도 로그와 대조에 쓰인다.
    validate_device_id(&config.coordinator_device_id)?;
    // ★ 정의만 해 두고 안 부르면 아무것도 막지 못한다(2026-08-30 독립
    //   검수 지적 — 직전 수정이 정확히 그 상태였다). bind 전에 부른다.
    require_distinct_scoped_ids(&config, &agents)?;

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
        overlap: Mutex::new(0),
        overlap_changed: Condvar::new(),
        peak_concurrent: AtomicU32::new(0),
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
        return Err(format!(
            "세션 {}건 실패: {}",
            failures.len(),
            failures.join(" / ")
        ));
    }

    println!(
        "MULTI_AGENT_DONE served={} peak_concurrent={}",
        shared.served.load(Ordering::SeqCst),
        shared.peak_concurrent.load(Ordering::SeqCst)
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
    /// 지금 **동시에** 열려 있는 세션 수.
    ///
    /// ★ 2026-08-30 독립 검수 지적으로 생겼다. 시나리오 88 은 두
    ///   Agent 를 연달아 띄우고 둘 다 성공했는지만 봤는데, 그건
    ///   **순차 서버로도 통과한다** — 동시 처리를 전혀 증명하지 못했다.
    overlap: Mutex<u32>,
    overlap_changed: Condvar,
    /// 관측된 최대 동시 세션 수. 보고용이다.
    peak_concurrent: AtomicU32,
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

    // 이 세션이 열려 있는 동안 동시 개수를 센다. `_open` 이 drop 될 때
    // 자동으로 줄어든다 — 아래 어느 `?` 에서 빠져나가도 새지 않는다.
    let _open = OpenSession::enter(shared);
    await_required_overlap(shared)?;

    // ★ 이 Agent 몫의 설정을 만든다. 식별자를 Agent 마다 갈라 놓지
    //   않으면 두 Agent 가 같은 `lease_id` 를 두고 다투게 되고, 그건
    //   이 조각이 다루려는 문제(동시 처리)가 아니라 전혀 다른
    //   문제(자원 경쟁)가 된다.
    let per_agent = scope_config_to_agent(&shared.config, &agent_device_id);

    let signing_key = SigningKey::from_bytes(&shared.config.own_seed);
    let now = clock.now_unix_ms();
    let grant = match &shared.config.grant_from_control_db {
        // ★★ 2026-09-23 (신뢰망 P2) — **저장된 배정에서 조립한다.**
        //
        //   전에는 이 lane 이 설정에 적힌 식별자로 Agent 마다 Grant 를 **지어냈다**
        //   (`scope_config_to_agent`). 그러면 여러 대가 붙어도 각자 자기 몫의
        //   "가짜 일" 을 받을 뿐이고, 스케줄러가 정한 배치와는 아무 상관이 없었다.
        //
        //   이제 **그 노드에 배정된 일**을 찾아 그것으로 Grant 를 만든다. 조립은
        //   순차 lane · CLI 와 **같은 함수**(`grant_from_stored`)가 한다 — 두 벌이 생기지 않는다.
        Some(control_db) => {
            let jobs = crate::job_store::CoordinatorJobStore::open(control_db)
                .map_err(|e| format!("job store: {e}"))?;
            let staging = crate::staging_store::CoordinatorStagingStore::open(control_db)
                .map_err(|e| format!("staging store: {e}"))?;
            let leases =
                CoordinatorLeaseStore::open(control_db).map_err(|e| format!("lease store: {e}"))?;
            let Some((job_id, attempt_id, lease_id)) = staging
                .work_assigned_to_node(&agent_device_id)
                .map_err(|e| format!("배정 조회: {e}"))?
            else {
                // ★ 일이 없는 것은 **오류가 아니다.** 이유를 말하고 이 연결을 끝낸다 —
                //   없는 일을 지어내 Grant 를 만들지 않는다.
                return Err(format!(
                    "NO_WORK_FOR_NODE: {agent_device_id} 에 배정된 예약이 없다 — 스케줄러가 아직 고르지 않았다"
                ));
            };
            let keyring_path = shared
                .config
                .stored_grant_submitter_keyring
                .as_ref()
                .ok_or_else(|| {
                    "--submitter-keyring 이 없다 — 시작 관문이 막았어야 한다".to_string()
                })?;
            let policy = if shared.config.stored_grant_allow_plaintext_keyring {
                gputeer_crypto::PlaintextPolicy::Allow
            } else {
                gputeer_crypto::PlaintextPolicy::Reject
            };
            let submitters = gputeer_crypto::PersistentKeyring::load(keyring_path, policy)
                .map_err(|e| format!("제출자 keyring({}): {e:?}", keyring_path.display()))?;
            crate::grant_from_stored::signed_grant_from_stored(
                &jobs,
                &staging,
                &leases,
                &crate::grant_from_stored::StoredGrantRequest {
                    // 풀 모드는 이 lane 을 금지한다(`run_multi_agent` 맨 앞 · 결함 408). 그래도 박아 넣지 않고 설정을 넘긴다 — 두 겹.
                    pool_mode: shared.config.pool_mode,
                    job_id,
                    attempt_id,
                    lease_id,
                    grant_id: per_agent.grant_id.clone(),
                    issued_at_unix_ms: now,
                    expires_at_unix_ms: now.saturating_add(shared.config.stored_grant_ttl_ms),
                    // ★ 이 lane 의 연결 시도 번호는 0 이다(연결마다 새로 센다) —
                    //   Agent 가 같은 유도식으로 다시 만들어 대조한다.
                    nonce: crate::derive_nonce("grant", &per_agent.grant_id, 0),
                },
                &signing_key,
                &submitters,
            )
            .map_err(|e| format!("저장된 배정으로 Grant 를 만들지 못했다: {e:?}"))?
        }
        // 제어 DB 를 안 주면 기존 동작 그대로다 — 설정에 적힌 식별자로 발급한다.
        None => {
            let mut store = lock(&shared.lease_store);
            issue_grant(&per_agent, &mut store, &signing_key, now, 0)?
        }
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
            //   수신부와 같은 이유(§10). 검사했다는 사실을 **실행 중에**
            //   확인해 `Result` 로 돌려준다.
            //
            //   ★ 이건 typestate 강제가 아니다(독립 검수 11라운드 정정 —
            //     전에 "타입으로 확인" 이라 썼다). 부르지 않으면 아무도
            //     막지 않는다. 값어치는 "부르면 조용히 통과하지 않는다" 다.
            let hello = verified
                .require_replay_checked()
                .map_err(|e| format!("Hello replay 검사 실패: {e:?}"))?;
            if hello.node_id.is_empty() {
                return Err("HELLO_REJECTED: node_id 가 비었다".into());
            }
            // ★ mode 를 실제로 대조한다(2026-08-30 독립 검수 지적).
            //
            //   이 검사가 없으면 등록된 Agent 가 Resume lane 용
            //   (`MODE_RESUME`)으로 서명한 Hello 를 보내도 이 lane 이
            //   받아들여 다중 Agent Grant 를 내준다 — 서명은 유효하므로
            //   서명 검증은 이것을 절대 못 잡는다. 한 lane 용으로 서명한
            //   메시지를 다른 lane 이 쓰는 것은 `signing.md` 가
            //   domain_tag 로 막으려는 것과 같은 부류의 혼동이다.
            if hello.mode != MODE_MULTI_AGENT_GRANT {
                return Err(format!(
                    "HELLO_REJECTED: mode 불일치 — 이 lane 은 {MODE_MULTI_AGENT_GRANT} 만 받는다, 받은 값 {}",
                    hello.mode
                ));
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
///
/// # 왜 해시가 아니라 Agent ID 를 그대로 붙이는가
///
/// ★ 2026-08-30 독립 검수가 실제 충돌 반례를 만들어냈다. 초안은
///   BLAKE3 해시 **앞 8자(32비트)** 만 썼는데, 32비트는 생일 문제로
///   수만 개만 시도해도 충돌한다.
///
/// ```text
/// agent-47131  ->  e7525a3b
/// agent-71872  ->  e7525a3b
/// ```
///
/// 충돌하면 두 Agent 가 같은 `lease_id`·`job_id`·`attempt_id`·
/// `grant_id` 를 받는다. 영속 저장소가 있으면 `holder_node_id` 충돌로
/// 거부돼 fail-closed 지만(그래도 서비스 거부다), **저장소가 없으면
/// 서로 다른 holder 앞으로 같은 식별자·같은 fence epoch 를 가진
/// 서명된 Lease 두 개가 나간다.** 이 lane 의 전제 자체가 깨진다.
///
/// 그래서 축약하지 않는다. `device_id` 는 이미 사람이 정한 짧은
/// 이름이고, 식별자가 조금 길어지는 것보다 충돌이 훨씬 비싸다.
/// 해시로 짧게 만들고 싶으면 128비트 이상을 써야 하는데, 그러면
/// 어차피 원본보다 길다.
fn scoped_id(base_id: &str, agent_device_id: &str) -> String {
    format!("{base_id}-{agent_device_id}")
}

/// 등록된 Agent 들이 서로 다른 식별자를 받는지 확인한다.
///
/// ★ `scoped_id()` 가 단사임을 코드로 확인할 수 있어도, 그 성질을
///   **여기서 한 번 더 강제한다.** 나중에 누가 다시 축약을 넣으면
///   그때는 이 검사가 걸린다 — 검수가 든 결함이 조용히 되살아나는
///   것을 막는 유일한 방법이다.
fn require_distinct_scoped_ids(
    base: &CoordinatorConfig,
    agents: &BTreeMap<String, VerifyingKey>,
) -> Result<(), String> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for device_id in agents.keys() {
        let derived = scoped_id(&base.lease_id, device_id);
        if let Some(previous) = seen.insert(derived.clone(), device_id.clone()) {
            return Err(format!(
                "SCOPED_ID_COLLISION: {previous} 와 {device_id} 가 같은 식별자 {derived} 를 \
                    만든다 — 저장소 없이 실행하면 서로 다른 Agent 에게 같은 Lease 가 서명돼 나간다"
            ));
        }
    }
    Ok(())
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
    validate_device_id(primary_device_id)?;
    let mut agents = BTreeMap::new();
    agents.insert(primary_device_id.to_string(), primary_key);

    let Some(raw) = extra else {
        return Ok(agents);
    };
    for entry in raw.split(';').filter(|e| !e.is_empty()) {
        let (device_id, key_hex) = entry
            .split_once('=')
            .ok_or_else(|| format!("--extra-agents 항목에 '=' 가 없다: {entry:?}"))?;
        validate_device_id(device_id)?;
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

/// 열려 있는 세션 하나. RAII 로 동시 개수를 센다.
///
/// ★ 수동으로 증감하면 중간의 `?` 하나가 감소를 건너뛰고, 그러면
///   개수가 영영 안 줄어 다음 관문이 잘못 통과한다. 세는 코드가
///   틀리면 그 위에서 내린 판정이 전부 거짓이 된다.
struct OpenSession<'a> {
    shared: &'a Shared,
}

impl<'a> OpenSession<'a> {
    fn enter(shared: &'a Shared) -> Self {
        let mut open = lock(&shared.overlap);
        *open += 1;
        let now = *open;
        shared.peak_concurrent.fetch_max(now, Ordering::SeqCst);
        println!("SESSION_OPEN concurrent={now}");
        shared.overlap_changed.notify_all();
        drop(open);
        Self { shared }
    }
}

impl Drop for OpenSession<'_> {
    fn drop(&mut self) {
        let mut open = lock(&self.shared.overlap);
        *open = open.saturating_sub(1);
        self.shared.overlap_changed.notify_all();
    }
}

/// 요구된 만큼의 세션이 **동시에** 열릴 때까지 기다린다.
///
/// # 왜 이런 관문이 필요한가
///
/// ★ 2026-08-30 독립 검수가 시나리오 88 의 공허성을 지적했다 — 두
///   Agent 를 연달아 띄우고 둘 다 성공했는지만 보면, 서버가 하나씩
///   순차 처리해도 똑같이 통과한다. "동시에 처리한다" 를 전혀
///   증명하지 못했다.
///
/// 겹칠 때까지 잠깐 붙잡아 두는 방법도 있지만 그건 확률이다 —
/// 느린 기계에서 첫 세션이 먼저 끝나면 조용히 무의미해진다.
/// 관문은 다르다. 순차 서버는 두 번째 연결을 **아예 받지 않으므로**
/// 첫 세션의 대기가 절대 안 풀리고, 시간 초과로 명확히 실패한다.
/// 통과하는 유일한 방법이 실제 동시 처리다.
///
/// 기본값(`0`)이면 이 함수는 아무것도 안 한다 — 운영 경로는 대기하지
/// 않는다.
fn await_required_overlap(shared: &Shared) -> Result<(), String> {
    let required = shared.config.require_concurrent_sessions;
    if required == 0 {
        return Ok(());
    }
    let deadline = Duration::from_millis(shared.config.accept_timeout_ms.max(5_000));
    let started = std::time::Instant::now();
    let mut open = lock(&shared.overlap);
    while *open < required {
        let remaining = deadline.checked_sub(started.elapsed()).ok_or_else(|| {
            format!(
                "CONCURRENCY_NOT_OBSERVED: {required}개 세션이 동시에 열리기를 {deadline:?} 기다렸으나 최대 {}개까지만 열렸다 — 서버의 순차 처리일 수도, 클라이언트가 덜 붙었거나 연결/Hello 가 실패했을 수도, required 설정이 틀렸을 수도 있다",
                shared.peak_concurrent.load(Ordering::SeqCst)
            )
        })?;
        let (guard, timeout) = shared
            .overlap_changed
            .wait_timeout(open, remaining)
            .map_err(|poisoned| {
                // poisoned 여도 값 자체는 읽을 수 있다. 여기서 죽이면
                // 관문이 원인 불명으로 실패해 진단이 더 어려워진다.
                let _ = poisoned;
                "overlap 대기 중 잠금이 poisoned 됐다".to_string()
            })?;
        open = guard;
        if timeout.timed_out() && *open < required {
            return Err(format!(
                "CONCURRENCY_NOT_OBSERVED: {required}개 세션이 동시에 열리기를 기다렸으나 최대 {}개까지만 열렸다 — 서버의 순차 처리일 수도, 클라이언트가 덜 붙었거나 연결/Hello 가 실패했을 수도, required 설정이 틀렸을 수도 있다",
                shared.peak_concurrent.load(Ordering::SeqCst)
            ));
        }
    }
    Ok(())
}
