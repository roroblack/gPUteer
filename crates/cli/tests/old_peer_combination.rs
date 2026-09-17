//! B+E — **옛 상대와의 조합**에서 두 프로세스가 명시적으로 실패하고 매달리지 않는지 잰다.
//!
//! 계획서(`docs/plans/2026-09-14_1238_B+E_갱신연결_보고연결_받았다응답_구현계획.md`) §8 · §13 이 "옛 Coordinator 와의 조합 시험은
//! 옛 바이너리가 없어 하지 않았다" 고 적었다. D2(모든 연결은 Agent 의 Hello 로 시작한다) 뒤로 wire 순서가 바뀌었다 —
//!
//! ```text
//! 새 Agent  · 옛 Coordinator   옛 쪽은 accept 뒤 곧바로 Grant 를 보내고 ACK 를 읽는다. 새 쪽은 Hello 를 먼저 보낸다
//! 옛 Agent  · 새 Coordinator   새 쪽은 Hello 를 기다린다. 옛 쪽은 Grant 를 기다린다
//! ```
//!
//! ★ 옛 바이너리는 저장소에 없다 — `GPUTEER_OLD_BINARY` 에 D2 이전 커밋을 빌드한 `gputeer` 경로를 준다. 그래서 두 조합 테스트는
//!   `#[ignore]` 다(`cargo test ... -- --ignored`). 조용히 통과로 세지 않도록 **변수가 없으면 실패한다.**
//! ★ 판정은 "시한(60초) 안에 둘 다 끝난다" 와 조합별 성공 · 실패다 — 새 Agent · 옛 Coordinator 는 결함 131 의 덫이다. 실패 문구는 버전마다 달라 고정하지 않고 출력을 그대로 남긴다.
//! ★ 대조(새 · 새)는 늘 돈다 — 두 실패가 인자 · 키 구성 실수 때문이 아님을 보인다.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(60);
const COORDINATOR_ID: &str = "01JCOORDOLDPEER0000000001";
const AGENT_ID: &str = "01JAGENTOLDPEER00000000001";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Keys {
    coordinator_seed: String,
    agent_seed: String,
    coordinator_pub: String,
    agent_pub: String,
}

fn keys() -> Keys {
    let coordinator = gputeer_protocol::canonical::blake3_256(b"old-peer-combination/coordinator");
    let agent = gputeer_protocol::canonical::blake3_256(b"old-peer-combination/agent");
    Keys {
        coordinator_seed: hex(&coordinator),
        agent_seed: hex(&agent),
        coordinator_pub: hex(gputeer_crypto::SigningKey::from_bytes(&coordinator).verifying_key().as_bytes()),
        agent_pub: hex(gputeer_crypto::SigningKey::from_bytes(&agent).verifying_key().as_bytes()),
    }
}

fn old_binary() -> PathBuf {
    let path = std::env::var_os("GPUTEER_OLD_BINARY")
        .map(PathBuf::from)
        .expect("GPUTEER_OLD_BINARY 가 없다 — D2 이전 커밋을 빌드한 gputeer 경로를 줘야 이 조합을 잴 수 있다");
    assert!(path.is_file(), "GPUTEER_OLD_BINARY 가 파일이 아니다: {path:?}");
    path
}

fn new_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gputeer"))
}

struct Finished {
    success: bool,
    hung: bool,
    elapsed: Duration,
    stdout: String,
    stderr: String,
}

fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut out = Vec::new();
        pipe.read_to_end(&mut out).expect("출력 읽기");
        String::from_utf8_lossy(&out).to_string()
    })
}

/// 끝날 때까지 기다리되 시한을 넘으면 죽이고 `hung=true` 로 돌려준다.
fn finish(mut child: Child, stdout: thread::JoinHandle<String>, started: Instant) -> Finished {
    let stderr = drain(child.stderr.take().expect("stderr"));
    let (status, hung) = loop {
        if let Some(status) = child.try_wait().expect("상태 조회") {
            break (Some(status), false);
        }
        if started.elapsed() > DEADLINE {
            child.kill().expect("시한 초과 프로세스 종료");
            child.wait().expect("종료 대기");
            break (None, true);
        }
        thread::sleep(Duration::from_millis(50));
    };
    Finished {
        success: status.is_some_and(|s| s.success()),
        hung,
        elapsed: started.elapsed(),
        stdout: stdout.join().expect("stdout 스레드"),
        stderr: stderr.join().expect("stderr 스레드"),
    }
}

fn run_pair(coordinator_exe: &Path, agent_exe: &Path, label: &str) -> (Finished, Finished) {
    let keys = keys();
    let coordinator_started = Instant::now();
    let mut coordinator = Command::new(coordinator_exe)
        .args([
            "coordinator-stub", "--listen", "127.0.0.1:0",
            "--own-seed", &keys.coordinator_seed, "--peer-pubkey", &keys.agent_pub,
            "--coordinator-device-id", COORDINATOR_ID, "--agent-device-id", AGENT_ID,
            "--grant-id", "01JGRANTOLDPEER000000001", "--attempt-id", "01JATTEMPTOLDPEER00000001",
            "--lease-id", "01JLEASEOLDPEER000000001", "--job-id", "01JJOBOLDPEER00000000001",
            "--i-understand-legacy-mode-is-unsafe", "true",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator-stub 스폰");
    // READY 줄을 먼저 받고, 나머지 stdout 은 같은 스레드가 끝까지 읽어 돌려준다.
    let pipe = coordinator.stdout.take().expect("stdout");
    let (ready_sender, ready_receiver) = mpsc::channel();
    let coordinator_stdout = thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut line = String::new();
        reader.read_line(&mut line).expect("READY 줄 읽기");
        ready_sender.send(line.clone()).expect("READY 전달");
        let mut rest = String::new();
        reader.read_to_string(&mut rest).expect("나머지 stdout");
        format!("{line}{rest}")
    });
    let ready = ready_receiver.recv_timeout(Duration::from_secs(20)).expect("coordinator READY 시한");
    let address = ready
        .trim()
        .strip_prefix("READY ")
        .unwrap_or_else(|| panic!("READY 대신: {ready:?}"))
        .to_string();

    let agent_started = Instant::now();
    let mut agent = Command::new(agent_exe)
        .args([
            "agent-stub", "--connect", &address,
            "--own-seed", &keys.agent_seed, "--peer-pubkey", &keys.coordinator_pub,
            "--coordinator-device-id", COORDINATOR_ID, "--agent-device-id", AGENT_ID,
            "--disable-reconnect", "true",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent-stub 스폰");
    let agent_stdout = drain(agent.stdout.take().expect("stdout"));
    let agent = finish(agent, agent_stdout, agent_started);
    let coordinator = finish(coordinator, coordinator_stdout, coordinator_started);
    for (who, run) in [("agent", &agent), ("coordinator", &coordinator)] {
        eprintln!(
            "=== {label} · {who}: success={} hung={} elapsed_ms={}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            run.success,
            run.hung,
            run.elapsed.as_millis(),
            run.stdout,
            run.stderr
        );
    }
    (agent, coordinator)
}

/// 새 Agent · 옛 Coordinator — **오늘의 사실을 고정하는 덫**(결함 131). 옛 Coordinator 는 ACK 자리에서 Hello 를 받아 실패하는데, 새 Agent 는 Grant 를 받고
/// ACK 를 쓴 뒤 **성공으로 끝난다** — "ACK 를 받아들였다" 는 서명된 응답이 계약에 없어 알아챌 수 없다. 고치면(계약 변경) 이 테스트를 뒤집는다:
/// Agent 도 실패해야 한다.
#[test]
#[ignore = "GPUTEER_OLD_BINARY(D2 이전 gputeer) 가 필요하다 — cargo test -p gputeer-cli --test old_peer_combination -- --ignored --nocapture"]
fn today_a_new_agent_reports_success_against_an_old_coordinator_that_failed() {
    let (agent, coordinator) = run_pair(&old_binary(), &new_binary(), "새 Agent · 옛 Coordinator");
    assert!(!agent.hung && !coordinator.hung, "조합이 시한({DEADLINE:?})까지 매달렸다");
    assert!(!coordinator.success, "옛 Coordinator 가 새 Agent 와 성공으로 끝났다 — 조합이 실제로 통하게 됐다");
    assert!(
        agent.success,
        "새 Agent 가 옛 Coordinator 와 실패로 끝났다 — 결함 131 이 고쳐졌으면 이 덫을 뒤집어라"
    );
}

/// 옛 Agent · 새 Coordinator — 둘 다 성공으로 끝나지 않고, 시한 안에 끝난다.
#[test]
#[ignore = "GPUTEER_OLD_BINARY(D2 이전 gputeer) 가 필요하다 — cargo test -p gputeer-cli --test old_peer_combination -- --ignored --nocapture"]
fn an_old_agent_and_a_new_coordinator_both_fail_explicitly() {
    let (agent, coordinator) = run_pair(&new_binary(), &old_binary(), "옛 Agent · 새 Coordinator");
    assert!(!agent.hung && !coordinator.hung, "조합이 시한({DEADLINE:?})까지 매달렸다");
    assert!(!agent.success, "옛 Agent 가 새 Coordinator 와 성공으로 끝났다");
    assert!(!coordinator.success, "새 Coordinator 가 옛 Agent 와 성공으로 끝났다");
}

/// 대조 — 같은 새 바이너리끼리는 성공한다.
#[test]
fn the_same_new_binary_on_both_sides_succeeds() {
    let exe = new_binary();
    let (agent, coordinator) = run_pair(&exe, &exe, "새 · 새(대조)");
    assert!(!agent.hung && !coordinator.hung, "대조가 매달렸다");
    assert!(agent.success && coordinator.success, "대조(새 · 새)가 실패했다 — 인자 · 키 구성을 먼저 의심하라");
}
