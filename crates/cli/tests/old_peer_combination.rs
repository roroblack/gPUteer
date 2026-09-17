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
    /// 결함 185 — 자식이 끝난 뒤에도 출력 수집이 시한 안에 끝나지 않았다(후손이 파이프를 물고 있는 경우).
    output_timed_out: bool,
}

/// 결함 185 — 자식이 끝난 뒤 출력 수집에 주는 시한. 후손이 파이프를 물고 있으면 join 이 끝나지 않으므로 채널로 받는다.
/// ★ 결함 193 (재검수 69c) — 전에는 수집할 때 스트림마다 **새로** 10초를 줘 stdout 9초 · stderr 18초에 끝나도 통과했고, 먼저 끝난 자식의 출력이
///   다른 자식을 기다리는 동안 늦게 완성돼도 못 잡았다. 이제 **그 자식의 종료 관측 시각 + 10초** 를 고정 마감으로 두고, 읽기 스레드가 보낸
///   **출력 완료 시각**을 그 마감과 비교한다. ★ 채널 시한은 읽기 스레드나 파이프를 문 후손을 끝내지 않는다(판정만 한다).
const OUTPUT_DEADLINE: Duration = Duration::from_secs(10);

/// 출력 전체와 **읽기가 끝난 시각**(EOF 를 본 시각)을 함께 보낸다(결함 193).
fn drain(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<(String, Instant)> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut out = Vec::new();
        let text = match pipe.read_to_end(&mut out) {
            Ok(_) => String::from_utf8_lossy(&out).to_string(),
            Err(error) => format!("{}<출력 읽기 실패: {error}>", String::from_utf8_lossy(&out)),
        };
        let _receiver_may_be_gone = sender.send((text, Instant::now()));
    });
    receiver
}

/// 결함 193 — 고정 마감까지만 기다리고, 받은 출력도 완료 시각이 마감 뒤면 시한 초과로 센다. (출력, 시한 초과 여부)
fn collect_by(receiver: mpsc::Receiver<(String, Instant)>, deadline: Instant) -> (String, bool) {
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok((text, completed)) => {
            let late = completed > deadline;
            (text, late)
        }
        Err(_) => ("<출력 수집 시한 초과 — 후손이 파이프를 물고 있을 수 있다>".to_string(), true),
    }
}

/// 결함 176 — 어느 경로로 끝나든(READY 시한 panic 포함) 살아 있는 자식을 끝내고 회수한다.
struct Guarded {
    child: Child,
    started: Instant,
    /// (종료 상태 · 매달림 · 경과 · **종료를 관측한 시각** — 결함 193 의 출력 마감 기준)
    outcome: Option<(Option<ExitStatus>, bool, Duration, Instant)>,
}

impl Guarded {
    /// 끝났으면 기록한다. 자기 시작 시각 기준 시한을 넘기면 죽이고 매달림으로 기록한다.
    fn poll(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        if let Some(status) = self.child.try_wait().expect("상태 조회") {
            // ★ 결함 185 — 스스로 끝났어도 자기 시작 시각 기준 시한을 넘겼으면 매달림으로 센다(감시 주기 사이에 끝난 경우).
            let elapsed = self.started.elapsed();
            self.outcome = Some((Some(status), elapsed > DEADLINE, elapsed, Instant::now()));
        } else if self.started.elapsed() > DEADLINE {
            self.child.kill().expect("시한 초과 프로세스 종료");
            self.child.wait().expect("종료 대기");
            self.outcome = Some((None, true, self.started.elapsed(), Instant::now()));
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
    run_pair_with(coordinator_exe, &[], agent_exe, label)
}

fn run_pair_with(coordinator_exe: &Path, coordinator_extra: &[&str], agent_exe: &Path, label: &str) -> (Finished, Finished) {
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
            .args(coordinator_extra)
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
    let (stdout_sender, coordinator_stdout) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut line = String::new();
        let _ready_read = reader.read_line(&mut line);
        let _receiver_may_be_gone = ready_sender.send(line.clone());
        let mut rest = String::new();
        let tail = match reader.read_to_string(&mut rest) {
            Ok(_) => rest,
            Err(error) => format!("{rest}<나머지 stdout 읽기 실패: {error}>"),
        };
        let _receiver_may_be_gone = stdout_sender.send((format!("{line}{tail}"), Instant::now()));
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
    let finish = |guarded: &Guarded, stdout: mpsc::Receiver<(String, Instant)>, stderr: mpsc::Receiver<(String, Instant)>| {
        let (status, hung, elapsed, observed_at) = guarded.outcome.expect("끝났다");
        // ★ 결함 193 — 마감은 이 자식의 종료 관측 시각에 고정한다(스트림마다 새로 주지 않는다)
        let deadline = observed_at + OUTPUT_DEADLINE;
        let (stdout, stdout_late) = collect_by(stdout, deadline);
        let (stderr, stderr_late) = collect_by(stderr, deadline);
        Finished { success: status.is_some_and(|s| s.success()), hung, elapsed, stdout, stderr, output_timed_out: stdout_late || stderr_late }
    };
    let agent_run = finish(&agent, agent_stdout, agent_stderr);
    let coordinator_run = finish(&coordinator, coordinator_stdout, coordinator_stderr);
    for (who, run) in [("agent", &agent_run), ("coordinator", &coordinator_run)] {
        eprintln!(
            "=== {label} · {who}: success={} hung={} output_timed_out={} elapsed_ms={}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            run.success,
            run.hung,
            run.output_timed_out,
            run.elapsed.as_millis(),
            run.stdout,
            run.stderr
        );
    }
    (agent_run, coordinator_run)
}

/// **결정적 덫**(결함 131 · 182) — 새 Coordinator 가 ACK 를 **검증한 뒤**(`DISCONNECT_AFTER_ACK coordinator_acknowledged=true`) 수신 확인 없이 끊는다.
/// ACK 는 확실히 도달했으므로 ACK 전송 실패의 경쟁이 없다. 오늘의 새 Agent 는 **성공으로 끝난다** — "ACK 를 받아들였다" 는 서명된 응답이 계약에 없어서다.
/// 결함 131 을 고치면(ACK 뒤 수신 확인을 기다림) Agent 는 여기서 **실패해야 한다** — 그때 이 시험을 뒤집는다. 옛 바이너리가 필요 없어 늘 돈다.
#[test]
fn today_a_new_agent_succeeds_without_an_ack_receipt_after_the_coordinator_verified_the_ack() {
    let exe = new_binary();
    let (agent, coordinator) = run_pair_with(&exe, &["--disconnect-after-ack", "true"], &exe, "새 · 새(ACK 검증 뒤 수신 확인 없이 끊음)");
    assert!(!agent.hung && !coordinator.hung && !agent.output_timed_out && !coordinator.output_timed_out, "덫이 매달렸다");
    assert!(
        coordinator.success && coordinator.stdout.contains("DISCONNECT_AFTER_ACK coordinator_acknowledged=true"),
        "Coordinator 가 ACK 를 검증한 뒤 끊은 흔적이 없다 — 덫의 전제가 깨졌다: stdout={} stderr={}",
        coordinator.stdout,
        coordinator.stderr
    );
    assert!(
        agent.success,
        "새 Agent 가 수신 확인 없는 연결에서 실패했다 — 결함 131 이 고쳐졌으면 이 덫을 뒤집어라: {}",
        agent.stderr
    );
}

/// 새 Agent · 옛 Coordinator — **관측 기록**(결함 131 · 174 · 182). 옛 Coordinator 는 Hello 를 **검증한 뒤** ACK 자리에서 받았다고 거부한다.
/// 새 Agent 는 성공하거나, 옛 쪽이 먼저 닫으면 **ACK 전송 실패**로 끝난다(어느 쪽이었는지 출력한다).
/// ★ 결함 182 — 이 조합은 결함 131 을 고친 Agent 를 **구별하지 못한다**(고친 Agent 도 수신 확인을 기다리기 전의 ACK 쓰기에서 같은 문구로 실패할 수 있다).
///   수정 전 · 후를 가르는 것은 위의 결정적 덫이다.
#[test]
#[ignore = "GPUTEER_OLD_BINARY(D2 이전 gputeer) 가 필요하다 — cargo test -p gputeer-cli --test old_peer_combination -- --include-ignored --nocapture"]
fn today_a_new_agent_reports_success_against_an_old_coordinator_that_failed() {
    let (agent, coordinator) = run_pair(&old_binary(), &new_binary(), "새 Agent · 옛 Coordinator");
    assert!(!agent.hung && !coordinator.hung && !agent.output_timed_out && !coordinator.output_timed_out, "조합이 시한({DEADLINE:?})까지 매달렸다");
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
            "새 Agent 가 ACK 전송 실패가 아닌 이유로 실패했다 — 관측을 다시 해석하라(결정적 덫과 함께 본다): {}",
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
    assert!(!agent.hung && !coordinator.hung && !agent.output_timed_out && !coordinator.output_timed_out, "조합이 시한({DEADLINE:?})까지 매달렸다");
    assert!(!agent.success, "옛 Agent 가 새 Coordinator 와 성공으로 끝났다");
    assert!(!coordinator.success, "새 Coordinator 가 옛 Agent 와 성공으로 끝났다");
    assert!(coordinator.stdout.contains("CONNECTION_ATTEMPT 0"), "새 Coordinator 가 연결을 받지 않았다: {}", coordinator.stdout);
    // ★ 결함 183 · 191 — 아래 판정 함수의 주석 참조. 서로 상대 프레임을 기다리다 **한쪽 이상이 읽기 시한 초과**로 끝났음을 오류 종류로 고정한다.
    assert!(
        both_waited_for_the_peer_until_a_read_timeout(&agent.stderr, agent.elapsed, &coordinator.stderr, coordinator.elapsed),
        "서로 상대 프레임을 기다리다 읽기 시한으로 끝난 흔적이 없다(reset 등 다른 연결 장애일 수 있다): agent_elapsed_ms={} agent_stderr={} coordinator_elapsed_ms={} coordinator_stderr={}",
        agent.elapsed.as_millis(),
        agent.stderr,
        coordinator.elapsed.as_millis(),
        coordinator.stderr
    );
}

/// 읽기 시한 초과의 OS 오류 번호(`FramingError::Io` 표시 끝의 `(os error N)`) — Windows `WSAETIMEDOUT` · Linux `EAGAIN` · 그 밖의 unix(BSD 계열) `EAGAIN`.
/// ★ Windows 번호만 실행으로 봤다(2026-09-17 원본의 옛 Agent 줄). unix 번호는 set_read_timeout 문서 · errno 표 근거이고 리눅스에서 돌리지 않았다.
#[cfg(windows)]
const READ_TIMEOUT_OS_ERROR: &str = "(os error 10060)";
#[cfg(target_os = "linux")]
const READ_TIMEOUT_OS_ERROR: &str = "(os error 11)";
#[cfg(all(unix, not(target_os = "linux")))]
const READ_TIMEOUT_OS_ERROR: &str = "(os error 35)";

/// 두 쪽 IO_TIMEOUT(10초)에서 여유를 뺀 하한 — 시한 초과로 끝난 쪽은 적어도 이만큼 돌았어야 한다.
const READ_TIMEOUT_FLOOR: Duration = Duration::from_secs(9);

/// 옛 Agent · 새 Coordinator 가 **서로 상대 프레임을 기다리다** 끝났는지(결함 183 · 191).
/// ★ 결함 191 (재검수 69c) — 전에는 "스트림 읽기 실패" 와 "프레임이 완결되기 전에 스트림이 끊겼다" 중 하나면 받았다. 앞의 것은 `FramingError::Io` 전체의
///   표시라 **reset 같은 별개의 연결 장애**도 통과했다. 이제 오류 **종류**를 본다 —
///   (a) 적어도 한쪽은 해당 줄에 읽기 시한 오류 번호가 있고 그 자식이 READ_TIMEOUT_FLOOR 이상 돌았다(자기 시한이 먼저 걸림)
///   (b) 다른 쪽은 역시 (a) 이거나, **완결 전 끊김(EOF)** 이다 — 먼저 시한에 걸린 쪽이 연결을 닫아 생기는 끝(2026-09-17 첫 실행에서 Coordinator 가 이쪽이었다)
///   그 밖의 I/O 오류(reset 10054 등) · 둘 다 EOF 는 떨어진다. 검증 실패(HELLO_REJECTED) · 검증된 다른 첫 프레임은 protocol 분류라 Coordinator 쪽 조건에서 떨어진다.
fn both_waited_for_the_peer_until_a_read_timeout(agent_stderr: &str, agent_elapsed: Duration, coordinator_stderr: &str, coordinator_elapsed: Duration) -> bool {
    const AGENT: &str = "RETRYABLE_CONNECTION: Grant 프레임 읽기/검증 실패: ";
    let coordinator_line = |reason: &str, need_timeout: bool| {
        coordinator_stderr.lines().any(|line| {
            line.contains("kind=transport") && line.contains("HELLO_MISSING") && line.contains(reason) && (!need_timeout || line.contains(READ_TIMEOUT_OS_ERROR))
        })
    };
    let agent_line = |reason: &str, need_timeout: bool| {
        agent_stderr
            .lines()
            .any(|line| line.contains(&format!("{AGENT}{reason}")) && (!need_timeout || line.contains(READ_TIMEOUT_OS_ERROR)))
    };
    let coordinator_timed_out = coordinator_line("스트림 읽기 실패", true) && coordinator_elapsed >= READ_TIMEOUT_FLOOR;
    let agent_timed_out = agent_line("스트림 읽기 실패", true) && agent_elapsed >= READ_TIMEOUT_FLOOR;
    let coordinator_saw_close = coordinator_line("프레임이 완결되기 전에 스트림이 끊겼다", false);
    let agent_saw_close = agent_line("프레임이 완결되기 전에 스트림이 끊겼다", false);
    (coordinator_timed_out && (agent_timed_out || agent_saw_close)) || (agent_timed_out && coordinator_saw_close)
}

/// 결함 191 — 판정 함수가 관측한 끝은 받고 reset · 짧은 경과 · 둘 다 EOF 는 떨어뜨리는지(옛 바이너리 없이 늘 돈다).
#[test]
fn the_wait_classifier_accepts_read_timeouts_and_rejects_other_connection_failures() {
    let t = READ_TIMEOUT_OS_ERROR;
    let long = Duration::from_secs(10);
    let agent_timeout = format!("RETRYABLE_CONNECTION: Grant 프레임 읽기/검증 실패: 스트림 읽기 실패: 응답이 없다 {t}");
    let agent_close = "RETRYABLE_CONNECTION: Grant 프레임 읽기/검증 실패: 프레임이 완결되기 전에 스트림이 끊겼다".to_string();
    let coordinator_timeout = format!("SESSION_ERROR attempt=0 kind=transport error=transport: session: HELLO_MISSING: 스트림 읽기 실패: 응답이 없다 {t}");
    let coordinator_close = "SESSION_ERROR attempt=0 kind=transport error=transport: session: HELLO_MISSING: 프레임이 완결되기 전에 스트림이 끊겼다".to_string();
    // 2026-09-17 첫 실행의 모양(옛 Agent 시한 · 새 Coordinator 끊김)과 그 반대 · 둘 다 시한
    assert!(both_waited_for_the_peer_until_a_read_timeout(&agent_timeout, long, &coordinator_close, long));
    assert!(both_waited_for_the_peer_until_a_read_timeout(&agent_close, long, &coordinator_timeout, long));
    assert!(both_waited_for_the_peer_until_a_read_timeout(&agent_timeout, long, &coordinator_timeout, long));
    // reset(시한 번호 아님) — 검수 69c 의 반례
    let agent_reset = "RETRYABLE_CONNECTION: Grant 프레임 읽기/검증 실패: 스트림 읽기 실패: 원격 호스트가 연결을 끊었다 (os error 10054)";
    let coordinator_reset = "SESSION_ERROR attempt=0 kind=transport error=transport: session: HELLO_MISSING: 스트림 읽기 실패: 원격 호스트가 연결을 끊었다 (os error 10054)";
    assert!(!both_waited_for_the_peer_until_a_read_timeout(agent_reset, long, coordinator_reset, long));
    assert!(!both_waited_for_the_peer_until_a_read_timeout(agent_reset, long, &coordinator_close, long));
    // 둘 다 EOF — 누구도 시한에 걸리지 않았다
    assert!(!both_waited_for_the_peer_until_a_read_timeout(&agent_close, long, &coordinator_close, long));
    // 시한 번호는 있지만 너무 일찍 끝났다
    assert!(!both_waited_for_the_peer_until_a_read_timeout(&agent_timeout, Duration::from_secs(1), &coordinator_close, long));
    // Coordinator 가 protocol 분류(검증 실패)면 떨어진다
    let coordinator_rejected = format!("SESSION_ERROR attempt=0 kind=protocol error=protocol: HELLO_REJECTED: 스트림 읽기 실패 {t}");
    assert!(!both_waited_for_the_peer_until_a_read_timeout(&agent_close, long, &coordinator_rejected, long));
}

/// 결함 193 — 출력 마감이 고정이고, 받은 출력의 완료 시각도 마감과 비교되는지(늘 돈다).
#[test]
fn output_collection_uses_a_fixed_deadline_and_the_completion_time() {
    let base = Instant::now();
    let deadline = base + Duration::from_secs(1);
    // 마감 전에 완성된 출력 — 통과
    let (sender, receiver) = mpsc::channel();
    sender.send(("ok".to_string(), base)).expect("보냄");
    assert_eq!(collect_by(receiver, deadline), ("ok".to_string(), false));
    // 채널에는 있지만 완성 시각이 마감 뒤 — 시한 초과(전에는 통과했다)
    let (sender, receiver) = mpsc::channel();
    sender.send(("late".to_string(), deadline + Duration::from_millis(1))).expect("보냄");
    assert_eq!(collect_by(receiver, deadline), ("late".to_string(), true));
    // 아무것도 안 온다 — 마감까지만 기다리고 시한 초과
    let (_sender, receiver) = mpsc::channel::<(String, Instant)>();
    let short = Instant::now() + Duration::from_millis(50);
    let started = Instant::now();
    assert!(collect_by(receiver, short).1);
    assert!(started.elapsed() < Duration::from_secs(5), "고정 마감을 넘겨 기다렸다");
    // 이미 지난 마감 — 기다리지 않고 판정
    let (_sender, receiver) = mpsc::channel::<(String, Instant)>();
    assert!(collect_by(receiver, base).1);
}

/// 대조 — 옛 바이너리끼리는 **이 인자 · 키로** 성공한다(결함 175 · 183). 옛 바이너리의 인자 · 키 구성이 유효함을 보인다 — 조합 시험 실행 자체의
/// 별개 장애까지 배제하지는 않는다(그 실행의 실패 원인은 위 시험의 문구 단언이 본다).
#[test]
#[ignore = "GPUTEER_OLD_BINARY(D2 이전 gputeer) 가 필요하다 — cargo test -p gputeer-cli --test old_peer_combination -- --include-ignored --nocapture"]
fn the_same_old_binary_on_both_sides_succeeds() {
    let exe = old_binary();
    let (agent, coordinator) = run_pair(&exe, &exe, "옛 · 옛(대조)");
    assert!(!agent.hung && !coordinator.hung && !agent.output_timed_out && !coordinator.output_timed_out, "대조가 매달렸다");
    assert!(agent.success && coordinator.success, "대조(옛 · 옛)가 실패했다 — 옛 바이너리의 인자 · 키 구성을 먼저 의심하라");
}

/// 대조 — 같은 새 바이너리끼리는 성공한다.
#[test]
fn the_same_new_binary_on_both_sides_succeeds() {
    let exe = new_binary();
    let (agent, coordinator) = run_pair(&exe, &exe, "새 · 새(대조)");
    assert!(!agent.hung && !coordinator.hung && !agent.output_timed_out && !coordinator.output_timed_out, "대조가 매달렸다");
    assert!(agent.success && coordinator.success, "대조(새 · 새)가 실패했다 — 인자 · 키 구성을 먼저 의심하라");
}
