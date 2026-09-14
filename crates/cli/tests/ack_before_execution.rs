//! 결함 ⑱ (설계 A, 2026-09-14) — 레거시 lane(`--manifest-file`)에서 본다.
//!
//! ```text
//! P1  10초보다 긴 워크로드 — ACK 가 **기동 전**에 가고(ACK_SENT 가 WORKLOAD_SPAWNED 보다 먼저),
//!     Coordinator 는 워크로드가 끝나기 한참 전에 ACK 를 받는다
//! P2  10초보다 긴 워크로드 + heartbeat — ACK 다음 첫 읽기가 Lease 만료까지 기다린다
//! N1  첫 사전 관문이 거부하면 ACK 가 가지 않는다
//! N2  ACK 다음 첫 읽기는 Lease 만료 무렵 포기한다 — 10초도 아니고 무한도 아니다
//! N3  워크로드를 띄운 연결은 끊김(peek)을 재접속 사유로 쓰지 않는다 — 워크로드가 한 번만 돈다
//! N4  워크로드를 띄운 연결은 Revoke 읽기 실패도 재접속 사유로 쓰지 않는다(결함 ㊺)
//! N5  첫 읽기 뒤에는 시한이 10초로 돌아온다(결함 ㊻)
//! ```
//!
//! ★ 시간은 **프로토콜 사건**(출력 줄이 도착한 시각)에 묶는다 — 프로세스 기동 시간이 섞이지 않게(결함 ㊻).
//! 실측과 설계는 `docs/plans/2026-09-10_2142_결함18_ACK_시한_설계_선택지.md` §7.
//! ★ Windows 전용이다 — 실행 관문이 리눅스에서는 cgroup 을 요구한다(`grant_over_wire.rs` 와 같다).
#![cfg(windows)]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use gputeer_crypto::SigningKey;

const COORDINATOR: &str = "01JACKCOORD000000000000001";
const AGENT: &str = "01JACKAGENT000000000000001";
const GRANT: &str = "01JACKGRANT000000000000001";
const ATTEMPT: &str = "01JACKATTEMPT0000000000001";
const LEASE: &str = "01JACKLEASE000000000000001";
const JOB: &str = "01JACKJOB00000000000000001";
const SUBMITTER: &str = "01JACKSUBMIT00000000000001";
const COORDINATOR_SEED: u8 = 0x11;
const AGENT_SEED: u8 = 0x22;
const SUBMITTER_SEED: u8 = 0x33;

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("gputeer.exe")
}

fn seed_hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn pub_hex(byte: u8) -> String {
    SigningKey::from_bytes(&[byte; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_millis() as u64
}

fn cmd_exe() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    format!(r"{root}\System32\cmd.exe")
}

/// `ping -n <count> 127.0.0.1` 은 약 `count - 1` 초 걸린다.
fn submit_ping_manifest(dir: &Path, ping_count: u32) -> PathBuf {
    let path = dir.join("job.manifest");
    let args = format!("/c,ping,-n,{ping_count},127.0.0.1");
    let out = Command::new(cli_bin())
        .args([
            "submit",
            "--job-id",
            JOB,
            "--entrypoint",
            &cmd_exe(),
            "--args",
            &args,
            "--submitter-device-id",
            SUBMITTER,
            "--submitter-seed",
            &seed_hex(SUBMITTER_SEED),
            "--issued-at-unix-ms",
            &now_unix_ms().to_string(),
            "--out",
            path.to_str().unwrap(),
        ])
        .output()
        .expect("gputeer submit");
    assert!(
        out.status.success(),
        "submit 실패: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    path
}

/// 출력 줄마다 **도착한 시각**을 붙인다.
type Lines = Vec<(Instant, String)>;

struct Proc {
    child: Child,
    stdout: JoinHandle<Lines>,
    stderr: JoinHandle<String>,
}

fn stamp_lines(pipe: ChildStdout, first_line: Option<mpsc::Sender<String>>) -> JoinHandle<Lines> {
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut lines = Vec::new();
        let mut first_line = first_line;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = line.trim_end().to_string();
                    if let Some(tx) = first_line.take() {
                        let _ = tx.send(line.clone());
                    }
                    lines.push((Instant::now(), line));
                }
            }
        }
        lines
    })
}

fn drain_stderr(child: &mut Child) -> JoinHandle<String> {
    let pipe = child.stderr.take().expect("piped stderr");
    thread::spawn(move || {
        let mut s = String::new();
        let _ = BufReader::new(pipe).read_to_string(&mut s);
        s
    })
}

fn spawn_coordinator(manifest: &Path, extra: &[&str]) -> (Proc, String) {
    let mut child = Command::new(cli_bin())
        .args([
            "coordinator-stub",
            "--listen",
            "127.0.0.1:0",
            "--own-seed",
            &seed_hex(COORDINATOR_SEED),
            "--peer-pubkey",
            &pub_hex(AGENT_SEED),
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            AGENT,
            "--grant-id",
            GRANT,
            "--attempt-id",
            ATTEMPT,
            "--lease-id",
            LEASE,
            "--job-id",
            JOB,
            "--manifest-file",
            manifest.to_str().unwrap(),
            "--i-understand-legacy-mode-is-unsafe",
            "true",
        ])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator-stub spawn");
    let (tx, rx) = mpsc::channel();
    let stdout = stamp_lines(child.stdout.take().expect("piped stdout"), Some(tx));
    let stderr = drain_stderr(&mut child);
    let ready = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("Coordinator 가 READY 를 안 냈다");
    let addr = ready
        .trim()
        .strip_prefix("READY ")
        .unwrap_or_else(|| panic!("READY 줄이 아니다: {ready:?}"))
        .to_string();
    (
        Proc {
            child,
            stdout,
            stderr,
        },
        addr,
    )
}

fn spawn_agent(addr: &str, dir: &Path, extra: &[&str]) -> Proc {
    let fence_db = dir.join("agent-fence.sqlite3");
    let checkpoint_root = dir.join("agent-checkpoints");
    let mut child = Command::new(cli_bin())
        .args([
            "agent-stub",
            "--connect",
            addr,
            "--own-seed",
            &seed_hex(AGENT_SEED),
            "--peer-pubkey",
            &pub_hex(COORDINATOR_SEED),
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            AGENT,
            "--submitter-pubkey",
            &pub_hex(SUBMITTER_SEED),
            "--i-understand-this-executes-untrusted-code",
            "true",
            "--fence-db",
            fence_db.to_str().unwrap(),
            "--checkpoint-root",
            checkpoint_root.to_str().unwrap(),
        ])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent-stub spawn");
    let stdout = stamp_lines(child.stdout.take().expect("piped stdout"), None);
    let stderr = drain_stderr(&mut child);
    Proc {
        child,
        stdout,
        stderr,
    }
}

struct Finished {
    success: bool,
    killed: bool,
    lines: Lines,
    stderr: String,
    /// 프로세스가 끝난 것을 본 시각.
    ended: Instant,
}

impl Finished {
    fn output(&self) -> String {
        let mut s: String = self.lines.iter().map(|(_, l)| format!("{l}\n")).collect();
        s.push_str(&self.stderr);
        s
    }
    fn first(&self, needle: &str) -> Option<(usize, Instant)> {
        self.lines
            .iter()
            .enumerate()
            .find(|(_, (_, l))| l.contains(needle))
            .map(|(i, (at, _))| (i, *at))
    }
    fn count(&self, needle: &str) -> usize {
        self.output().matches(needle).count()
    }
}

/// 끝날 때까지(최대 `limit`) 기다린다. 넘으면 죽이고 `killed=true`.
fn wait(mut p: Proc, limit: Duration) -> Finished {
    let deadline = Instant::now() + limit;
    let (success, killed) = loop {
        if let Some(status) = p.child.try_wait().expect("try_wait") {
            break (status.success(), false);
        }
        if Instant::now() >= deadline {
            let _ = p.child.kill();
            let _ = p.child.wait();
            break (false, true);
        }
        thread::sleep(Duration::from_millis(50));
    };
    let ended = Instant::now();
    Finished {
        success,
        killed,
        lines: p.stdout.join().unwrap_or_default(),
        stderr: p.stderr.join().unwrap_or_default(),
        ended,
    }
}

fn both(agent: &Finished, coordinator: &Finished) -> String {
    format!(
        "--- agent (success={}, killed={}) ---\n{}\n--- coordinator (success={}, killed={}) ---\n{}",
        agent.success,
        agent.killed,
        agent.output(),
        coordinator.success,
        coordinator.killed,
        coordinator.output()
    )
}

/// P1 — 약 15초 워크로드. ACK 가 **기동 전**에 가고, Coordinator 는 워크로드가 끝나기 한참 전에 ACK 를 받는다.
#[test]
fn a_workload_longer_than_the_io_timeout_is_acknowledged_before_it_runs() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_ping_manifest(dir.path(), 16);
    let (coordinator, addr) = spawn_coordinator(&manifest, &[]);
    let agent = spawn_agent(&addr, dir.path(), &["--disable-reconnect", "true"]);
    let coordinator = wait(coordinator, Duration::from_secs(90));
    let agent = wait(agent, Duration::from_secs(90));
    let all = both(&agent, &coordinator);

    assert!(
        coordinator.success && coordinator.output().contains("RESULT ok=true"),
        "Coordinator 가 ACK 를 받지 못했다\n{all}"
    );
    assert!(
        agent.success
            && agent.output().contains("WORKLOAD_EXITED")
            && agent.output().contains("exit_code=0"),
        "워크로드가 끝까지 돌지 않았다\n{all}"
    );
    // ★ 기동 **전** ACK 의 직접 관측 — 같은 프로세스의 표준 출력 순서다.
    let (ack_line, _) = agent.first("ACK_SENT").unwrap_or_else(|| panic!("ACK_SENT 가 없다\n{all}"));
    let (spawn_line, _) = agent
        .first("WORKLOAD_SPAWNED")
        .unwrap_or_else(|| panic!("WORKLOAD_SPAWNED 가 없다\n{all}"));
    assert!(ack_line < spawn_line, "자식을 띄운 뒤에 ACK 를 보냈다\n{all}");
    // Coordinator 가 ACK 를 받은 사건(RESULT)이 워크로드 종료 사건보다 한참 먼저다.
    let (_, acked_at) = coordinator.first("RESULT ok=true").expect("RESULT");
    let (_, exited_at) = agent.first("WORKLOAD_EXITED").expect("WORKLOAD_EXITED");
    assert!(
        acked_at + Duration::from_secs(5) < exited_at,
        "Coordinator 가 워크로드가 끝날 무렵에야 ACK 를 받았다\n{all}"
    );
}

/// P2 — 약 15초 워크로드 뒤에 오는 heartbeat 를 Coordinator 가 받는다(Lease 30초).
#[test]
fn the_first_read_after_ack_waits_for_the_workload_within_the_lease() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_ping_manifest(dir.path(), 16);
    let (coordinator, addr) = spawn_coordinator(
        &manifest,
        &["--expect-heartbeats", "1", "--lease-ttl-ms", "30000"],
    );
    let agent = spawn_agent(
        &addr,
        dir.path(),
        &["--disable-reconnect", "true", "--heartbeat-rounds", "1"],
    );
    let coordinator = wait(coordinator, Duration::from_secs(90));
    let agent = wait(agent, Duration::from_secs(90));
    let all = both(&agent, &coordinator);

    assert!(
        coordinator.success && coordinator.output().contains("HEARTBEAT_ACCEPTED"),
        "워크로드 뒤의 heartbeat 를 받지 못했다\n{all}"
    );
    assert!(agent.success, "Agent 가 실패했다\n{all}");
}

/// N1 — 첫 사전 관문(상한 0)이 거부하면 ACK 를 보내지 않고, 아무것도 띄우지 않는다.
#[test]
fn a_refused_preflight_sends_no_ack() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_ping_manifest(dir.path(), 3);
    let (coordinator, addr) = spawn_coordinator(&manifest, &[]);
    let agent = spawn_agent(
        &addr,
        dir.path(),
        &["--disable-reconnect", "true", "--workload-commit-limit-bytes", "0"],
    );
    let coordinator = wait(coordinator, Duration::from_secs(60));
    let agent = wait(agent, Duration::from_secs(60));
    let all = both(&agent, &coordinator);

    assert!(
        !agent.success && agent.output().contains("EXEC_REFUSED:LIMIT_NOT_APPLIED"),
        "사전 관문 거부가 보고되지 않았다\n{all}"
    );
    assert!(!agent.output().contains("ACK_SENT"), "거부했는데 ACK 를 보냈다\n{all}");
    assert!(!agent.output().contains("WORKLOAD_SPAWNED"), "거부됐는데 실행됐다\n{all}");
    assert!(
        !coordinator.output().contains("RESULT ok=true"),
        "받아들이지 못한 Grant 에 ACK 가 갔다\n{all}"
    );
}

/// N2 — Lease 20초 · 워크로드 약 30초. ACK 다음 첫 읽기(heartbeat)는 Lease 만료 무렵 포기한다 —
///   10초(`IO_TIMEOUT`)에 포기하지도, 워크로드가 끝날 때까지 기다리지도 않는다.
///   ★ 시간은 Coordinator 가 연결을 받은 사건(CONNECTION_ATTEMPT)부터 잰다.
#[test]
fn the_first_read_after_ack_gives_up_at_lease_expiry() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_ping_manifest(dir.path(), 31);
    let (coordinator, addr) = spawn_coordinator(
        &manifest,
        &["--expect-heartbeats", "1", "--lease-ttl-ms", "20000"],
    );
    let agent = spawn_agent(
        &addr,
        dir.path(),
        &["--disable-reconnect", "true", "--heartbeat-rounds", "1"],
    );
    let coordinator = wait(coordinator, Duration::from_secs(60));
    // Agent 는 30초를 다 돌 필요가 없다 — Coordinator 판정이 끝나면 죽인다.
    let agent = wait(agent, Duration::from_millis(1));
    let all = both(&agent, &coordinator);

    assert!(
        !coordinator.success && !coordinator.output().contains("HEARTBEAT_ACCEPTED"),
        "Lease 가 끝난 뒤에 온 heartbeat 를 받았다\n{all}"
    );
    let (_, accepted_at) = coordinator
        .first("CONNECTION_ATTEMPT")
        .unwrap_or_else(|| panic!("CONNECTION_ATTEMPT 가 없다\n{all}"));
    let waited = coordinator.ended.saturating_duration_since(accepted_at);
    assert!(
        waited >= Duration::from_secs(15),
        "Lease 만료가 아니라 10초 시한에 포기했다({waited:?})\n{all}"
    );
    assert!(
        waited < Duration::from_secs(28),
        "Lease 만료에서 멈추지 않고 더 기다렸다({waited:?})\n{all}"
    );
}

/// N3 — 재접속을 켠 Agent · 연결을 두 번 받는 Coordinator. 워크로드(약 2초)를 띄운 뒤 Coordinator 가
///   이미 닫혀 있어도 peek 로 재접속해 **다시 실행하지 않는다.**
#[test]
fn a_connection_that_ran_the_workload_does_not_reconnect_and_run_it_again() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_ping_manifest(dir.path(), 3);
    let (coordinator, addr) = spawn_coordinator(
        &manifest,
        &["--max-connections", "2", "--accept-timeout-ms", "5000"],
    );
    let agent = spawn_agent(&addr, dir.path(), &[]);
    let agent = wait(agent, Duration::from_secs(90));
    let coordinator = wait(coordinator, Duration::from_secs(30));
    let all = both(&agent, &coordinator);

    assert_eq!(
        agent.count("WORKLOAD_EXITED"),
        1,
        "워크로드가 한 번이 아니라 여러 번 돌았다\n{all}"
    );
    assert!(agent.success, "Agent 가 실패했다\n{all}");
    assert_eq!(
        coordinator.count("CONNECTION_ATTEMPT"),
        1,
        "Agent 가 다시 연결했다\n{all}"
    );
}

/// N4 — 결함 ㊺. Agent 가 Revoke 를 기다리는데 Coordinator 가 ACK 직후 끊는다. Revoke 읽기 실패는
///   원래 재접속 가능 오류지만, 워크로드를 띄운 연결이므로 재접속하지 않고 실패로 끝난다.
#[test]
fn a_revoke_read_failure_after_the_workload_does_not_reconnect_and_run_it_again() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_ping_manifest(dir.path(), 3);
    let (coordinator, addr) = spawn_coordinator(
        &manifest,
        &[
            "--drop-connection-after-ack-once",
            "true",
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
        ],
    );
    let agent = spawn_agent(&addr, dir.path(), &["--expect-revoke-after-round", "0"]);
    let agent = wait(agent, Duration::from_secs(90));
    let coordinator = wait(coordinator, Duration::from_secs(30));
    let all = both(&agent, &coordinator);

    assert_eq!(
        agent.count("WORKLOAD_EXITED"),
        1,
        "Revoke 읽기 실패 뒤 재접속해 워크로드를 다시 돌렸다\n{all}"
    );
    assert!(
        !agent.success && agent.output().contains("WORKLOAD_ALREADY_RAN"),
        "재접속을 억제한 사유가 보고되지 않았다\n{all}"
    );
    assert_eq!(
        coordinator.count("CONNECTION_ATTEMPT"),
        1,
        "Agent 가 다시 연결했다\n{all}"
    );
}

/// N5 — 결함 ㊻. 첫 heartbeat 는 곧바로 오고 두 번째는 13초 뒤에 온다. 첫 읽기 뒤 시한이 10초로
///   돌아왔으면 Coordinator 는 두 번째를 기다리다 포기한다(Lease 60초가 남아 있어도).
#[test]
fn the_read_timeout_returns_to_the_io_timeout_after_the_first_read() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_ping_manifest(dir.path(), 2);
    let (coordinator, addr) = spawn_coordinator(
        &manifest,
        &["--expect-heartbeats", "2", "--lease-ttl-ms", "60000"],
    );
    let agent = spawn_agent(
        &addr,
        dir.path(),
        &[
            "--disable-reconnect",
            "true",
            "--heartbeat-rounds",
            "2",
            "--heartbeat-interval-ms",
            "13000",
        ],
    );
    let coordinator = wait(coordinator, Duration::from_secs(60));
    let agent = wait(agent, Duration::from_secs(60));
    let all = both(&agent, &coordinator);

    assert_eq!(
        coordinator.count("HEARTBEAT_ACCEPTED"),
        1,
        "첫 heartbeat 만 받고 두 번째에서 포기해야 한다\n{all}"
    );
    assert!(!coordinator.success, "두 번째 읽기가 10초를 넘겨 기다렸다\n{all}");
}
