//! `gputeer coordinator-agent-selftest` — coordinator/agent 프로세스 경계 실측.
//!
//! `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md` 단계 4.
//!
//! `gputeer selftest` §5(2026-08-17)는 127.0.0.1 실제 TCP 소켓을 왕복하지만
//! **같은 프로세스 안 스레드 하나**가 서버를 연다(`crates/cli/src/selftest.rs:543`).
//! 이 서브커맨드는 그 다음 한 걸음이다 — `coordinator-stub`/`agent-stub`
//! 을 `Command::current_exe()` 로 **실제 별도 OS 프로세스**로 띄우고,
//! 서명된 `ExecutionGrant`/`AgentGrantAck` 가 그 프로세스 경계를 실제로
//! 넘어 검증되는지 확인한다.
//!
//! ★ 이 첫 버전은 **정상 경로 한 번**만 증명한다. 계획서 단계 5(거부
//! 경로 3종 — 위조 Grant·위조 ACK·replay)는 아직 이 서브커맨드에 없다
//! — CLAUDE.md "다음에 할 일" 에 남겨 뒀다.

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

/// 두 stub 을 별도 프로세스로 띄우고 정상 handshake 가 성립하는지
/// 확인한다. 실패하면 사람이 읽을 이유를 담아 반환한다.
pub fn run() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("현재 실행 파일 경로를 못 얻었다: {e}"))?;

    // ★ 실제 CSPRNG 대신 라벨을 해시해 시드를 결정적으로 만든다 —
    //   이 selftest 는 매번 같은 조건으로 재현 가능해야 하고, 키
    //   프로비저닝 자체를 증명하는 것이 이 단계의 목적이 아니다
    //   (계획서 "Out" 절 — 운영용 key protection 은 범위 밖).
    let coordinator_seed = seed_from_label("coordinator-agent-selftest/coordinator");
    let agent_seed = seed_from_label("coordinator-agent-selftest/agent");

    let coordinator_pub = gputeer_crypto::SigningKey::from_bytes(&coordinator_seed).verifying_key();
    let agent_pub = gputeer_crypto::SigningKey::from_bytes(&agent_seed).verifying_key();

    let coordinator_device_id = "01JCOORDSELFTEST0000000001";
    let agent_device_id = "01JAGENTSELFTEST00000000001";
    let grant_id = "01JGRANTSELFTEST000000001";
    let attempt_id = "01JATTEMPTSELFTEST00000001";

    let mut coordinator = Command::new(&exe)
        .args([
            "coordinator-stub",
            "--listen",
            "127.0.0.1:0",
            "--own-seed",
            &to_hex(&coordinator_seed),
            "--peer-pubkey",
            &to_hex(agent_pub.as_bytes()),
            "--coordinator-device-id",
            coordinator_device_id,
            "--agent-device-id",
            agent_device_id,
            "--grant-id",
            grant_id,
            "--attempt-id",
            attempt_id,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("coordinator-stub 스폰 실패: {e}"))?;

    // ★ stdout 을 여기서 take() 하면 이후 `wait_with_output()` 은 이
    //   출력을 더 이상 얻지 못한다(핸들이 이미 소비됐다) — 그래서
    //   coordinator 는 `wait()` 만 쓰고, stdout·stderr 는 끝까지 직접
    //   읽는다. 처음엔 이걸 놓쳐서 coordinator 의 RESULT 줄이 조용히
    //   사라지는 결함이 있었다 — 실제로 이 서브커맨드를 실행해서 잡았다.
    let mut coordinator_stdout_reader =
        BufReader::new(coordinator.stdout.take().expect("piped stdout"));
    let mut coordinator_stderr_reader = coordinator.stderr.take().expect("piped stderr");

    let mut ready_line = String::new();
    coordinator_stdout_reader
        .read_line(&mut ready_line)
        .map_err(|e| format!("coordinator READY 줄 읽기 실패: {e}"))?;
    let address = ready_line
        .trim()
        .strip_prefix("READY ")
        .ok_or_else(|| format!("coordinator 가 READY 대신 이걸 찍었다: {ready_line:?}"))?
        .to_string();

    let agent = Command::new(&exe)
        .args([
            "agent-stub",
            "--connect",
            &address,
            "--own-seed",
            &to_hex(&agent_seed),
            "--peer-pubkey",
            &to_hex(coordinator_pub.as_bytes()),
            "--coordinator-device-id",
            coordinator_device_id,
            "--agent-device-id",
            agent_device_id,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("agent-stub 스폰 실패: {e}"))?;

    // ★ 비공허성의 핵심 — 이 값이 서로 다르지 않으면 "별도 프로세스가
    //   핸드셰이크했다" 는 이 서브커맨드 전체의 전제가 무너진다.
    let self_pid = std::process::id();
    let coordinator_pid = coordinator.id();
    let agent_pid = agent.id();
    if coordinator_pid == agent_pid || coordinator_pid == self_pid || agent_pid == self_pid {
        return Err(format!(
            "PID 가 구분되지 않는다: self={self_pid} coordinator={coordinator_pid} agent={agent_pid}"
        ));
    }

    let agent_output = agent
        .wait_with_output()
        .map_err(|e| format!("agent-stub 대기 실패: {e}"))?;

    let mut coordinator_stdout_rest = String::new();
    coordinator_stdout_reader
        .read_to_string(&mut coordinator_stdout_rest)
        .map_err(|e| format!("coordinator 나머지 stdout 읽기 실패: {e}"))?;
    let coordinator_stdout = format!("{ready_line}{coordinator_stdout_rest}");

    let mut coordinator_stderr = String::new();
    coordinator_stderr_reader
        .read_to_string(&mut coordinator_stderr)
        .map_err(|e| format!("coordinator stderr 읽기 실패: {e}"))?;

    let coordinator_status = coordinator
        .wait()
        .map_err(|e| format!("coordinator-stub 대기 실패: {e}"))?;

    let agent_stdout = String::from_utf8_lossy(&agent_output.stdout).into_owned();

    if !coordinator_status.success() {
        return Err(format!(
            "coordinator-stub 이 실패했다(exit={:?}):\nstdout={coordinator_stdout}\nstderr={coordinator_stderr}",
            coordinator_status.code(),
        ));
    }
    if !agent_output.status.success() {
        return Err(format!(
            "agent-stub 이 실패했다(exit={:?}):\nstdout={agent_stdout}\nstderr={}",
            agent_output.status.code(),
            String::from_utf8_lossy(&agent_output.stderr)
        ));
    }

    let expected = format!(
        "RESULT ok=true grant_id={grant_id} attempt_id={attempt_id} agent_device_id={agent_device_id}"
    );
    if !coordinator_stdout.contains(&expected) || !agent_stdout.contains(&expected) {
        return Err(format!(
            "RESULT 줄이 기대한 상관관계와 다르다.\n기대: {expected}\ncoordinator stdout: {coordinator_stdout}\nagent stdout: {agent_stdout}"
        ));
    }

    Ok(format!(
        "coordinator/agent 별도 프로세스 handshake 성공 \
         (self={self_pid} coordinator={coordinator_pid} agent={agent_pid}, \
         grant_id={grant_id})\n"
    ))
}

fn seed_from_label(label: &str) -> [u8; 32] {
    gputeer_protocol::canonical::blake3_256(label.as_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
