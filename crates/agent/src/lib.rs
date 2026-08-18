//! Agent 프로세스 stub — coordinator/agent 최소 핸드셰이크.
//!
//! `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
//! 단계 3·4. **"완전한 Agent" 가 아니다** — Coordinator 가 보낸
//! `ExecutionGrant` 를 검증하고, 서명된 `AgentGrantAck` 를 돌려주는
//! 것까지만 한다. Job 실행 · GPU 할당 · CUDA 실행은 범위 밖(계획서 "Out" 절).
//!
//! # 키 배분
//!
//! `crates/coordinator/src/lib.rs` 모듈 문서의 "키 배분" 절과 동일한
//! 이유로 `PersistentKeyring` 대신 호출자가 준 시드로 만든 [`SigningKey`]
//! 를 메모리에만 들고, Coordinator 공개키는 [`InMemoryKeyring`] 에 담아
//! 검증에만 쓴다.

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, InMemoryKeyring, InMemoryReplayGuard,
    IngressMessage, KeyDirectorySource, SigningKey, SystemClock, VerifyingKey,
};
use gputeer_protocol::pb;
use prost::Message;

const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AgentConfig {
    pub coordinator_addr: String,
    pub own_seed: [u8; 32],
    pub coordinator_verifying_key: VerifyingKey,
    pub coordinator_device_id: String,
    pub agent_device_id: String,
    /// ★ 테스트 전용 — `crates/coordinator/src/lib.rs::CoordinatorConfig::corrupt_own_signature`
    ///   와 대칭. 서명 직후 `agent_signature` 의 마지막 바이트를 뒤집어
    ///   전송한다 — "위조된 ACK 가 도착했을 때 Coordinator 가 실제로
    ///   거부하는가" 를 프로세스 경계에서 확인한다.
    pub corrupt_own_signature: bool,
    /// ★ 테스트 전용 — 첫 ACK 를 보낸 뒤 같은 연결에서 프레임을 하나 더
    ///   읽는다. Coordinator 가 같은 Grant wire bytes 를 두 번 보낸
    ///   경우(`CoordinatorConfig::send_grant_twice`), 이 두 번째 읽기는
    ///   그 replay 된 Grant 다 — `verify()` 가 `Duplicate` 를 만나면
    ///   `Err` 를 직접 반환한다(`crates/protocol/src/signing.rs:826`).
    ///   여기서 `Err` 가 나오는 것이 **기대한 결과**다.
    pub expect_replay: bool,
}

/// 정상 handshake 한 번을 실행한다.
///
/// Coordinator 에 연결해 `ExecutionGrant` 를 받아 검증하고, 서명된
/// `AgentGrantAck` 를 돌려준다. 성공하면 `stdout` 에
/// `RESULT ok=true ...` 를 찍고 `Ok(())`, 실패하면 그 이유를 담아
/// `Err` 를 반환한다(호출자가 exit code 로 매핑).
pub fn run(config: AgentConfig) -> Result<(), String> {
    let signing_key = SigningKey::from_bytes(&config.own_seed);
    let mut coordinator_keys = InMemoryKeyring::new();
    coordinator_keys.insert(
        config.coordinator_device_id.clone(),
        config.coordinator_verifying_key,
    );

    let mut replay = InMemoryReplayGuard::new();
    let clock = SystemClock;

    let mut stream = TcpStream::connect(&config.coordinator_addr)
        .map_err(|e| format!("Coordinator 연결 실패: {e}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;

    let received = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&coordinator_keys),
        &mut replay,
        &clock,
    )
    .map_err(|e| format!("Grant 프레임 읽기/검증 실패: {e}"))?;

    // ★ Grant 를 replay 검사까지 통과한 뒤에만 그 내용으로 ACK 를
    //   만든다 — 부작용(ACK 발급)이 검증되지 않은 값에서 나오지 않는다.
    let grant: pb::ExecutionGrant = match &received {
        IngressMessage::Grant(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("Grant replay 검사 실패: {e:?}"))?
            .clone(),
        other => return Err(format!("예상하지 못한 요청 타입: {other:?}")),
    };

    let now = clock.now_unix_ms();
    let mut ack = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: grant.grant_id.clone(),
        attempt_id: grant.attempt_id.clone(),
        agent_device_id: config.agent_device_id.clone(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_nonce("grant-ack", &grant.grant_id),
        accepted: true,
        ..Default::default()
    };
    ack.agent_signature = sign(&signing_key, &ack).to_vec();

    if config.corrupt_own_signature {
        let last = ack
            .agent_signature
            .last_mut()
            .ok_or_else(|| "agent_signature 가 비어 있다".to_string())?;
        *last ^= 0x01;
    }

    let frame = write_frame(FrameType::GrantAck, &ack.encode_to_vec())
        .map_err(|e| format!("ACK 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .map_err(|e| format!("ACK 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;

    // ★ replay 시나리오는 여기서 끝낸다 — **절대 `RESULT ok=true` 를
    //   찍지 않는다.** 두 번째로 도착하는 프레임은 Coordinator 가
    //   `send_grant_twice` 로 다시 보낸 같은 Grant wire bytes 다.
    //   `require_replay_checked()` 까지 갈 필요도 없다 — `verify()`
    //   자체가 `Duplicate` 를 만나면 `read_frame` 단계에서 이미 `Err`
    //   를 반환한다.
    if config.expect_replay {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        return match read_frame(
            &mut stream,
            1,
            KeyDirectorySource::Provided(&coordinator_keys),
            &mut replay,
            &clock,
        ) {
            Err(error) => Err(format!("REPLAY_REJECTED: {error}")),
            Ok(message) => Err(format!(
                "REPLAY_NOT_REJECTED: 두 번째 Grant 가 거부되지 않고 {message:?} 로 검증됐다"
            )),
        };
    }

    println!(
        "RESULT ok=true grant_id={} attempt_id={} agent_device_id={}",
        grant.grant_id, grant.attempt_id, ack.agent_device_id
    );
    Ok(())
}

/// `crates/coordinator/src/lib.rs::derive_nonce` 와 같은 방식 —
/// 결정적 유도로 재현 가능한 selftest 시나리오를 만든다. `tag` 로
/// Grant nonce 와 네임스페이스를 분리한다(같은 grant_id 라도
/// Coordinator->Agent 방향과 Agent->Coordinator 방향의 nonce 가 같아지면
/// `nonce_namespace_is_per_device` 가 보장하는 sender 별 분리에 기대게
/// 되어 이 stub 자체의 nonce 선택이 우연히 안전해 보일 수 있다).
///
/// ★ **운영 코드는 이 패턴을 쓰면 안 된다.** 같은 `grant_id` 로 다시
/// 부르면 같은 nonce 가 나온다 — CSPRNG 가 아니라 결정적 해시이기
/// 때문이다. 이 stub 이 안전한 이유는 매 selftest 실행이 새 OS
/// 프로세스·새 `InMemoryReplayGuard` 를 쓰기 때문이다(실행 간 replay
/// 상태가 없다). `DurableReplayGuard` 처럼 재시작을 견디는 저장소와
/// 이 nonce 선택을 같이 쓰면, 재시작 후 같은 `grant_id` 를 다시
/// 발급했을 때 정당한 새 Grant 가 예전 nonce 와 충돌해 `Duplicate`
/// 로 오판될 수 있다(코덱스 독립 검수 2026-08-18 지적).
fn derive_nonce(tag: &str, id: &str) -> Vec<u8> {
    let mut input = Vec::with_capacity(tag.len() + 1 + id.len());
    input.extend_from_slice(tag.as_bytes());
    input.push(0);
    input.extend_from_slice(id.as_bytes());
    gputeer_protocol::canonical::blake3_256(&input)[..16].to_vec()
}

/// `--flag value` 쌍으로 이루어진 CLI 인자를 [`AgentConfig`] 로
/// 파싱해 [`run`] 을 부른다. `crates/coordinator/src/lib.rs::run_from_args`
/// 와 같은 이유로 `crates/cli` 대신 여기(Agent 스트림)가 인자 의미를
/// 정의한다.
pub fn run_from_args(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;

    let config = AgentConfig {
        coordinator_addr: flags.require("--connect")?,
        own_seed: hex_to_seed(&flags.require("--own-seed")?)?,
        coordinator_verifying_key: hex_to_verifying_key(&flags.require("--peer-pubkey")?)?,
        coordinator_device_id: flags.require("--coordinator-device-id")?,
        agent_device_id: flags.require("--agent-device-id")?,
        corrupt_own_signature: flags.bool_flag("--corrupt-own-signature"),
        expect_replay: flags.bool_flag("--expect-replay"),
    };

    run(config)
}

struct Flags(std::collections::HashMap<String, String>);

impl Flags {
    fn require(&self, key: &str) -> Result<String, String> {
        self.0
            .get(key)
            .cloned()
            .ok_or_else(|| format!("필수 인자 누락: {key}"))
    }

    /// `crates/coordinator/src/lib.rs::Flags::bool_flag` 와 동일 — 값이
    /// 있는 boolean 플래그(`--flag true`). 안 주면 `false`.
    fn bool_flag(&self, key: &str) -> bool {
        self.0.get(key).map(|v| v == "true").unwrap_or(false)
    }
}

fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut map = std::collections::HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = &args[i];
        if !key.starts_with("--") {
            return Err(format!("플래그가 아닌 인자: {key}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} 뒤에 값이 없다"))?;
        map.insert(key.clone(), value.clone());
        i += 2;
    }
    Ok(Flags(map))
}

fn hex_to_seed(hex: &str) -> Result<[u8; 32], String> {
    let bytes = hex_decode(hex)?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| "seed 는 정확히 32바이트여야 한다".into())
}

fn hex_to_verifying_key(hex: &str) -> Result<VerifyingKey, String> {
    let seed = hex_to_seed(hex)?;
    VerifyingKey::from_bytes(&seed).map_err(|e| format!("유효하지 않은 공개키: {e}"))
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("hex 문자열 길이가 홀수다".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}
