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
//! ★ 옛 바이너리는 저장소에 없다 — `GPUTEER_OLD_BINARY` 에 D2 이전 커밋을 빌드한 `gputeer` 경로를 준다. 그래서 옛 바이너리를 쓰는 시험은
//!   `#[ignore]` 다. 조용히 통과로 세지 않도록 **변수가 없으면 실패한다.** 대조까지 함께 돌리려면 `-- --include-ignored` 를 쓴다
//!   (`--ignored` 는 ignored 만 돈다 — 검수 69).
//! ★ 이 시험이 재는 구성은 **레거시 FRESH lane · 워크로드 실행 없음 · `--disable-reconnect true`** 다(결함 174 · 178). 저장된 예약 · 실행 · REPORT 세션 lane 으로
//!   일반화하지 않는다.
//! ★ 대조가 둘이다(결함 175): 새 · 새 는 새 바이너리의 인자 · 키 구성을, 옛 · 옛 은 **옛 바이너리의 인자 · 키 구성**을 보인다 — 조합 실패가 구성 실수가 아님은
//!   옛 · 옛 대조까지 있어야 말할 수 있다.
//! ★ 두 자식은 **한 루프에서 각자 시작 시각 기준으로 동시에** 감시하고, 어느 경로로 끝나든 살아 있는 자식을 끝낸다(결함 176).
//! ★ 두 실행 파일의 경로 · 크기 · BLAKE3 를 출력한다(결함 177) — 원본이 어느 파일로 잰 것인지 남기게.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
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

/// 결함 177 — 어느 파일로 쟀는지 원본에 남긴다.
fn describe(label: &str, exe: &Path) {
    let bytes = std::fs::read(exe).unwrap_or_else(|e| panic!("{label} 실행 파일을 읽지 못했다({exe:?}): {e}"));
    eprintln!(
        "=== 실행 파일 {label}: path={} size={} blake3={}",
        exe.display(),
        bytes.len(),
        hex(&gputeer_protocol::canonical::blake3_256(&bytes))
    );
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

/// 결함 176 — 어느 경로로 끝나든(READY 시한 panic 포함) 살아 있는 자식을 끝내고 회수한다.
struct Guarded {
    child: Child,
    started: Instant,
    outcome: Option<(Option<ExitStatus>, bool, Duration)>,
}

impl Guarded {
    /// 끝났으면 기록한다. 자기 시작 시각 기준 시한을 넘기면 죽이고 매달림으로 기록한다.
    fn poll(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        if let Some(status) = self.child.try_wait().expect("상태 조회") {
            self.outcome = Some((Some(status), false, self.started.elapsed()));
        } else if self.started.elapsed() > DEADLINE {
            self.child.kill().expect("시한 초과 프로세스 종료");
            self.child.wait().expect("종료 대기");
            self.outcome = Some((None, true, self.started.elapsed()));
        }
    }
}

impl Drop for Guarded {
    fn drop(&mut self) {
        if self.outcome.is_none() {
            let _already_gone = self.child.kill();
            let _reaped = self.child.wait();
        }
    }
}

fn run_pair(coordinator_exe: &Path, agent_exe: &Path, label: &str) -> (Finished, Finished) {
    describe("coordinator", coordinator_exe);
    describe("agent", agent_exe);
    let keys = keys();
    let mut coordinator = Guarded {
        child: Command::new(coordinator_exe)
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
            .expect("coordinator-stub 스폰"),
        started: Instant::now(),
        outcome: None,
    };
    // READY 줄을 먼저 받고, 나머지 stdout 은 같은 스레드가 끝까지 읽어 돌려준다.
    let pipe = coordinator.child.stdout.take().expect("stdout");
    let coordinator_stderr = drain(coordinator.child.stderr.take().expect("stderr"));
    let (ready_sender, ready_receiver) = mpsc::channel();
    let coordinator_stdout = thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut line = String::new();
        reader.read_line(&mut line).expect("READY 줄 읽기");
        let _receiver_may_be_gone = ready_sender.send(line.clone());
        let mut rest = String::new();
        reader.read_to_string(&mut rest).expect("나머지 stdout");
        format!("{line}{rest}")
    });
    // ★ 결함 176 — 여기서 panic 해도 `coordinator` 가드가 자식을 끝낸다.
    let ready = ready_receiver.recv_timeout(Duration::from_secs(20)).expect("coordinator READY 시한");
    let address = ready
        .trim()
        .strip_prefix("READY ")
        .unwrap_or_else(|| panic!("READY 대신: {ready:?}"))
        .to_string();

    let mut agent = Guarded {
        child: Command::new(agent_exe)
            .args([
                "agent-stub", "--connect", &address,
                "--own-seed", &keys.agent_seed, "--peer-pubkey", &keys.coordinator_pub,
                "--coordinator-device-id", COORDINATOR_ID, "--agent-device-id", AGENT_ID,
                "--disable-reconnect", "true",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("agent-stub 스폰"),
        started: Instant::now(),
        outcome: None,
    };
    let agent_stdout = drain(agent.child.stdout.take().expect("stdout"));
    let agent_stderr = drain(agent.child.stderr.take().expect("stderr"));

    // ★ 결함 176 — 두 자식을 한 루프에서 동시에 감시한다(전에는 Agent 를 끝까지 기다린 뒤 Coordinator 를 봐서 Coordinator 의 시한이 흐려졌다).
    while coordinator.outcome.is_none() || agent.outcome.is_none() {
        coordinator.poll();
        agent.poll();
        thread::sleep(Duration::from_millis(20));
    }
    let finish = |guarded: &Guarded, stdout: thread::JoinHandle<String>, stderr: thread::JoinHandle<String>| {
        let (status, hung, elapsed) = guarded.outcome.expect("끝났다");
        Finished {
            success: status.is_some_and(|s| s.success()),
            hung,
            elapsed,
            stdout: stdout.join().expect("stdout 스레드"),
            stderr: stderr.join().expect("stderr 스레드"),
        }
    };
    let agent_run = finish(&agent, agent_stdout, agent_stderr);
    let coordinator_run = finish(&coordinator, coordinator_stdout, coordinator_stderr);
    for (who, run) in [("agent", &agent_run), ("coordinator", &coordinator_run)] {
        eprintln!(
            "=== {label} · {who}: success={} hung={} elapsed_ms={}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            run.success,
            run.hung,
            run.elapsed.as_millis(),
            run.stdout,
            run.stderr
        );
    }
    (agent_run, coordinator_run)
}

/// 새 Agent · 옛 Coordinator — **오늘의 사실을 고정하는 덫**(결함 131 · 174). 옛 Coordinator 는 Hello 를 **검증한 뒤** ACK 자리에서 받았다고 거부한다.
/// 새 Agent 는 Grant 를 받고 ACK 를 쓴 뒤 — 재접속을 끈 이 구성에서는 — **성공으로 끝난다.** "ACK 를 받아들였다" 는 서명된 응답이 계약에 없어서다.
/// ★ 결함 174 — ACK 쓰기와 옛 쪽의 연결 닫기는 동기화되지 않는다. 그래서 새 Agent 가 **ACK 전송 실패**로 끝나는 실행도 오늘의 사실로 받는다(어느 쪽이었는지 출력한다).
///   그 밖의 실패는 덫이 뒤집힌 것이다 — 결함 131 을 고쳤는지(계약 변경) 확인하고 이 시험을 바꾼다.
#[test]
#[ignore = "GPUTEER_OLD_BINARY(D2 이전 gputeer) 가 필요하다 — cargo test -p gputeer-cli --test old_peer_combination -- --include-ignored --nocapture"]
fn today_a_new_agent_reports_success_against_an_old_coordinator_that_failed() {
    let (agent, coordinator) = run_pair(&old_binary(), &new_binary(), "새 Agent · 옛 Coordinator");
    assert!(!agent.hung && !coordinator.hung, "조합이 시한({DEADLINE:?})까지 매달렸다");
    assert!(!coordinator.success, "옛 Coordinator 가 새 Agent 와 성공으로 끝났다 — 조합이 실제로 통하게 됐다");
    assert!(
        coordinator.stderr.contains("예상하지 못한 응답 타입: SessionHello"),
        "옛 Coordinator 가 기대한 이유(ACK 자리에서 Hello)로 실패하지 않았다 — 구성 실수일 수 있다: {}",
        coordinator.stderr
    );
    if agent.success {
        eprintln!("=== 덫 판정: 새 Agent 성공(결함 131 의 원래 관측)");
    } else {
        assert!(
            agent.stderr.contains("ACK 전송 실패"),
            "새 Agent 가 ACK 전송 실패가 아닌 이유로 실패했다 — 결함 131 이 고쳐졌으면 이 덫을 뒤집어라: {}",
            agent.stderr
        );
        eprintln!("=== 덫 판정: 새 Agent 가 ACK 전송 실패로 끝남(옛 쪽이 먼저 닫은 경쟁 — 결함 174)");
    }
}

/// 옛 Agent · 새 Coordinator — 둘 다 성공으로 끝나지 않고, 시한 안에, **기대한 이유로** 끝난다(결함 175).
/// ★ 새 Coordinator 의 실패는 소켓 읽기 시한(약 10초)에 기댄 HELLO_MISSING 이다 — 버전 불일치를 즉시 알아챈 것이 아니다.
#[test]
#[ignore = "GPUTEER_OLD_BINARY(D2 이전 gputeer) 가 필요하다 — cargo test -p gputeer-cli --test old_peer_combination -- --include-ignored --nocapture"]
fn an_old_agent_and_a_new_coordinator_both_fail_explicitly() {
    let (agent, coordinator) = run_pair(&new_binary(), &old_binary(), "옛 Agent · 새 Coordinator");
    assert!(!agent.hung && !coordinator.hung, "조합이 시한({DEADLINE:?})까지 매달렸다");
    assert!(!agent.success, "옛 Agent 가 새 Coordinator 와 성공으로 끝났다");
    assert!(!coordinator.success, "새 Coordinator 가 옛 Agent 와 성공으로 끝났다");
    assert!(
        coordinator.stdout.contains("CONNECTION_ATTEMPT 0") && coordinator.stderr.contains("HELLO_MISSING"),
        "새 Coordinator 가 연결을 받아 Hello 를 기다리다 끝난 흔적이 없다(연결 전 실패 · accept 시한일 수 있다): stdout={} stderr={}",
        coordinator.stdout,
        coordinator.stderr
    );
    assert!(
        agent.stderr.contains("Grant 프레임 읽기/검증 실패"),
        "옛 Agent 가 Grant 를 기다리다 실패한 흔적이 없다(인자 · 저장소 오류일 수 있다): {}",
        agent.stderr
    );
}

/// 대조 — 옛 바이너리끼리는 성공한다(결함 175). 조합 실패가 옛 바이너리의 인자 · 키 구성 실수가 아님을 보인다.
#[test]
#[ignore = "GPUTEER_OLD_BINARY(D2 이전 gputeer) 가 필요하다 — cargo test -p gputeer-cli --test old_peer_combination -- --include-ignored --nocapture"]
fn the_same_old_binary_on_both_sides_succeeds() {
    let exe = old_binary();
    let (agent, coordinator) = run_pair(&exe, &exe, "옛 · 옛(대조)");
    assert!(!agent.hung && !coordinator.hung, "대조가 매달렸다");
    assert!(agent.success && coordinator.success, "대조(옛 · 옛)가 실패했다 — 옛 바이너리의 인자 · 키 구성을 먼저 의심하라");
}

/// 대조 — 같은 새 바이너리끼리는 성공한다.
#[test]
fn the_same_new_binary_on_both_sides_succeeds() {
    let exe = new_binary();
    let (agent, coordinator) = run_pair(&exe, &exe, "새 · 새(대조)");
    assert!(!agent.hung && !coordinator.hung, "대조가 매달렸다");
    assert!(agent.success && coordinator.success, "대조(새 · 새)가 실패했다 — 인자 · 키 구성을 먼저 의심하라");
}
