//! `gputeer coordinator-agent-selftest` — coordinator/agent 프로세스 경계 실측.
//!
//! `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md` 단계 4·5.
//!
//! `gputeer selftest` §5(2026-08-17)는 127.0.0.1 실제 TCP 소켓을 왕복하지만
//! **같은 프로세스 안 스레드 하나**가 서버를 연다(`crates/cli/src/selftest.rs:543`).
//! 이 서브커맨드는 그 다음 한 걸음이다 — `coordinator-stub`/`agent-stub`
//! 을 `Command::current_exe()` 로 **실제 별도 OS 프로세스**로 띄우고,
//! 서명된 `ExecutionGrant`/`AgentGrantAck` 가 그 프로세스 경계를 실제로
//! 넘어 검증되는지, 그리고 위조·replay 가 실제로 거부되는지 확인한다.
//!
//! # 위조를 어떻게 만드는가 (2026-08-18, 코덱스 설계 — `p56` 프롬프트)
//!
//! 정직하게 동작하는 coordinator/agent 프로세스는 자기 서명을 절대
//! 위조하지 않는다. 그래서 각 stub 에 테스트 전용 플래그
//! (`--corrupt-own-signature`, `--send-grant-twice`, `--expect-replay`)
//! 를 두어 "이 프로세스가 스스로 타락한 메시지를 보낸다" 를 흉내낸다
//! — `crates/coordinator/src/lib.rs`·`crates/agent/src/lib.rs` 참조.
//! TCP proxy 로 진짜 중간자 변조를 재현하는 대안도 검토했지만, 이
//! 계획의 DoD 는 TLS/MITM 방어가 아니라 서명 검증·replay guard 검증
//! 이므로 범위를 넘는다고 판단했다.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::thread;

const RESULT_OK_MARKER: &str = "RESULT ok=true";

struct Fixture {
    exe: std::path::PathBuf,
    coordinator_seed: [u8; 32],
    agent_seed: [u8; 32],
    coordinator_pub_hex: String,
    agent_pub_hex: String,
    coordinator_device_id: &'static str,
    agent_device_id: &'static str,
    grant_id: &'static str,
    attempt_id: &'static str,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let exe = std::env::current_exe()
            .map_err(|e| format!("현재 실행 파일 경로를 못 얻었다: {e}"))?;

        // ★ 실제 CSPRNG 대신 라벨을 해시해 시드를 결정적으로 만든다 —
        //   이 selftest 는 매번 같은 조건으로 재현 가능해야 하고, 키
        //   프로비저닝 자체를 증명하는 것이 이 단계의 목적이 아니다
        //   (계획서 "Out" 절 — 운영용 key protection 은 범위 밖).
        let coordinator_seed = seed_from_label("coordinator-agent-selftest/coordinator");
        let agent_seed = seed_from_label("coordinator-agent-selftest/agent");

        let coordinator_pub =
            gputeer_crypto::SigningKey::from_bytes(&coordinator_seed).verifying_key();
        let agent_pub = gputeer_crypto::SigningKey::from_bytes(&agent_seed).verifying_key();

        Ok(Self {
            exe,
            coordinator_seed,
            agent_seed,
            coordinator_pub_hex: to_hex(coordinator_pub.as_bytes()),
            agent_pub_hex: to_hex(agent_pub.as_bytes()),
            coordinator_device_id: "01JCOORDSELFTEST0000000001",
            agent_device_id: "01JAGENTSELFTEST00000000001",
            grant_id: "01JGRANTSELFTEST000000001",
            attempt_id: "01JATTEMPTSELFTEST00000001",
        })
    }

    fn expected_result_line(&self) -> String {
        format!(
            "{RESULT_OK_MARKER} grant_id={} attempt_id={} agent_device_id={}",
            self.grant_id, self.attempt_id, self.agent_device_id
        )
    }
}

/// 한 handshake 시도(정상 경로든 거부 경로 시나리오든)의 전체 결과.
/// 시나리오마다 "무엇이 성공/실패여야 하는가" 판정 기준이 다르므로,
/// 이 구조체는 판정하지 않고 관측한 사실만 담는다.
struct HandshakeOutcome {
    self_pid: u32,
    coordinator_pid: u32,
    agent_pid: u32,
    coordinator_success: bool,
    coordinator_stdout: String,
    coordinator_stderr: String,
    agent_success: bool,
    agent_stdout: String,
    agent_stderr: String,
}

/// coordinator-stub/agent-stub 을 별도 프로세스로 한 번 띄워 끝까지
/// 실행하고 결과를 모은다. `extra_coordinator_args`/`extra_agent_args`
/// 로 단계 5 의 거부 경로 플래그를 얹는다(정상 경로는 빈 슬라이스).
fn run_handshake(
    fixture: &Fixture,
    extra_coordinator_args: &[&str],
    extra_agent_args: &[&str],
) -> Result<HandshakeOutcome, String> {
    let mut coordinator_args: Vec<&str> = vec![
        "coordinator-stub",
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        // to_hex 결과를 빌려 쓰기 위해 아래에서 String 을 만들어 둔다.
    ];
    let coordinator_own_seed_hex = to_hex(&fixture.coordinator_seed);
    coordinator_args.push(&coordinator_own_seed_hex);
    coordinator_args.push("--peer-pubkey");
    coordinator_args.push(&fixture.agent_pub_hex);
    coordinator_args.push("--coordinator-device-id");
    coordinator_args.push(fixture.coordinator_device_id);
    coordinator_args.push("--agent-device-id");
    coordinator_args.push(fixture.agent_device_id);
    coordinator_args.push("--grant-id");
    coordinator_args.push(fixture.grant_id);
    coordinator_args.push("--attempt-id");
    coordinator_args.push(fixture.attempt_id);
    coordinator_args.extend_from_slice(extra_coordinator_args);

    let mut coordinator = Command::new(&fixture.exe)
        .args(&coordinator_args)
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
    let coordinator_stderr_reader = coordinator.stderr.take().expect("piped stderr");

    // ★ 코덱스 독립 검수(2026-08-18)가 지적한 결함 — stderr 를 메인
    //   흐름과 동시에 비우지 않으면, coordinator 가 OS 파이프 버퍼를
    //   채울 만큼 stderr 에 쓰다가 블로킹되고, agent 는 coordinator 의
    //   TCP 응답을 기다리느라 블로킹되어 이 selftest 전체가 교착할 수
    //   있다. stdout 은 READY/RESULT 줄을 순서대로 읽어야 하므로 메인
    //   스레드에 남기고, stderr 만 별도 스레드에서 끝까지 비운다.
    let stderr_drain = thread::spawn(move || -> Result<String, std::io::Error> {
        let mut buf = String::new();
        let mut reader = coordinator_stderr_reader;
        reader.read_to_string(&mut buf)?;
        Ok(buf)
    });

    let mut ready_line = String::new();
    coordinator_stdout_reader
        .read_line(&mut ready_line)
        .map_err(|e| format!("coordinator READY 줄 읽기 실패: {e}"))?;
    let address = ready_line
        .trim()
        .strip_prefix("READY ")
        .ok_or_else(|| format!("coordinator 가 READY 대신 이걸 찍었다: {ready_line:?}"))?
        .to_string();

    let mut agent_args: Vec<&str> = vec!["agent-stub", "--connect", &address, "--own-seed"];
    let agent_own_seed_hex = to_hex(&fixture.agent_seed);
    agent_args.push(&agent_own_seed_hex);
    agent_args.push("--peer-pubkey");
    agent_args.push(&fixture.coordinator_pub_hex);
    agent_args.push("--coordinator-device-id");
    agent_args.push(fixture.coordinator_device_id);
    agent_args.push("--agent-device-id");
    agent_args.push(fixture.agent_device_id);
    agent_args.extend_from_slice(extra_agent_args);

    let agent: Child = Command::new(&fixture.exe)
        .args(&agent_args)
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

    let coordinator_stderr = stderr_drain
        .join()
        .map_err(|_| "coordinator stderr 배수 스레드가 패닉했다".to_string())?
        .map_err(|e| format!("coordinator stderr 읽기 실패: {e}"))?;

    let coordinator_status = coordinator
        .wait()
        .map_err(|e| format!("coordinator-stub 대기 실패: {e}"))?;

    Ok(HandshakeOutcome {
        self_pid,
        coordinator_pid,
        agent_pid,
        coordinator_success: coordinator_status.success(),
        coordinator_stdout,
        coordinator_stderr,
        agent_success: agent_output.status.success(),
        agent_stdout: String::from_utf8_lossy(&agent_output.stdout).into_owned(),
        agent_stderr: String::from_utf8_lossy(&agent_output.stderr).into_owned(),
    })
}

/// 4가지 시나리오(정상 1 + 거부 경로 3)를 차례로 돌린다. 하나라도
/// 기대와 다르면 그 자리에서 이유를 담아 반환한다.
pub fn run() -> Result<String, String> {
    let fixture = Fixture::new()?;
    let mut report = String::new();

    // ── 1. 정상 경로 ──────────────────────────────────────────────
    let normal = run_handshake(&fixture, &[], &[])?;
    if !normal.coordinator_success || !normal.agent_success {
        return Err(format!(
            "정상 handshake 가 실패했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            normal.coordinator_success,
            normal.coordinator_stdout,
            normal.coordinator_stderr,
            normal.agent_success,
            normal.agent_stdout,
            normal.agent_stderr
        ));
    }
    let expected = fixture.expected_result_line();
    if !normal.coordinator_stdout.contains(&expected) || !normal.agent_stdout.contains(&expected) {
        return Err(format!(
            "RESULT 줄이 기대한 상관관계와 다르다.\n기대: {expected}\n\
             coordinator stdout: {}\nagent stdout: {}",
            normal.coordinator_stdout, normal.agent_stdout
        ));
    }
    report.push_str(&format!(
        "1) 정상 handshake 성공 (self={} coordinator={} agent={}, grant_id={})\n",
        normal.self_pid, normal.coordinator_pid, normal.agent_pid, fixture.grant_id
    ));

    // ── 2. 위조 coordinator_signature — Agent 가 ACK 를 발급하지 않는다 ──
    let forged_grant = run_handshake(&fixture, &["--corrupt-own-signature", "true"], &[])?;
    if forged_grant.agent_success || forged_grant.agent_stdout.contains(RESULT_OK_MARKER) {
        return Err(format!(
            "위조된 coordinator_signature 가 거부되지 않았다 — Agent 가 ACK 를 발급했다.\n\
             agent exit={} stdout={} stderr={}",
            forged_grant.agent_success, forged_grant.agent_stdout, forged_grant.agent_stderr
        ));
    }
    // ★ Coordinator 도 ACK 를 못 받으므로 성공으로 끝나면 안 된다 —
    //   그러지 않으면 "위조를 보냈는데 스스로는 성공을 주장하는" 앞뒤가
    //   안 맞는 상태가 된다.
    if forged_grant.coordinator_success || forged_grant.coordinator_stdout.contains(RESULT_OK_MARKER) {
        return Err(format!(
            "위조된 Grant 를 보냈는데 coordinator 가 스스로 성공을 주장했다.\n\
             coordinator exit={} stdout={} stderr={}",
            forged_grant.coordinator_success,
            forged_grant.coordinator_stdout,
            forged_grant.coordinator_stderr
        ));
    }
    report.push_str("2) 위조 coordinator_signature 거부 확인 (Agent 가 ACK 미발급)\n");

    // ── 3. 위조 agent_signature — Coordinator 가 성공 처리하지 않는다 ──
    // ★ Agent 는 자기가 만든 서명이 위조됐는지 스스로 검증하지 않는다
    //   (검증은 언제나 수신자 책임이다) — 그래서 Agent 자신은 정상
    //   종료할 수 있다. 판정은 Coordinator 쪽 실패 여부로만 한다.
    let forged_ack = run_handshake(&fixture, &[], &["--corrupt-own-signature", "true"])?;
    if forged_ack.coordinator_success || forged_ack.coordinator_stdout.contains(RESULT_OK_MARKER) {
        return Err(format!(
            "위조된 agent_signature 가 거부되지 않았다 — Coordinator 가 성공을 주장했다.\n\
             coordinator exit={} stdout={} stderr={}",
            forged_ack.coordinator_success, forged_ack.coordinator_stdout, forged_ack.coordinator_stderr
        ));
    }
    report.push_str("3) 위조 agent_signature 거부 확인 (Coordinator 가 성공 처리 안 함)\n");

    // ── 4. 동일 Grant wire bytes replay ──────────────────────────
    let replay = run_handshake(
        &fixture,
        &["--send-grant-twice", "true"],
        &["--expect-replay", "true"],
    )?;
    if replay.agent_success || !replay.agent_stderr.contains("REPLAY_REJECTED:") {
        return Err(format!(
            "동일 Grant 를 두 번 보냈는데 두 번째가 거부됐다는 증거(REPLAY_REJECTED:)가 없다.\n\
             agent exit={} stdout={} stderr={}",
            replay.agent_success, replay.agent_stdout, replay.agent_stderr
        ));
    }
    if replay.coordinator_success || replay.coordinator_stdout.contains(RESULT_OK_MARKER) {
        return Err(format!(
            "replay 시나리오인데 coordinator 가 성공을 주장했다.\n\
             coordinator exit={} stdout={} stderr={}",
            replay.coordinator_success, replay.coordinator_stdout, replay.coordinator_stderr
        ));
    }
    report.push_str("4) 동일 Grant wire bytes replay 거부 확인 (DurableReplayGuard 계약과 동일하게 InMemoryReplayGuard 도 Duplicate 를 거부)\n");

    Ok(report)
}

fn seed_from_label(label: &str) -> [u8; 32] {
    gputeer_protocol::canonical::blake3_256(label.as_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
