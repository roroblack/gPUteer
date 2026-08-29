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
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use prost::Message;

const RESULT_OK_MARKER: &str = "RESULT ok=true";
const HANDSHAKE_HARD_TIMEOUT: Duration = Duration::from_secs(90);

trait ChildTimeoutExt {
    fn wait_with_output_until(self, deadline: Instant) -> std::io::Result<Output>;
    fn wait_until(&mut self, deadline: Instant) -> std::io::Result<ExitStatus>;
}

impl ChildTimeoutExt for Child {
    fn wait_with_output_until(mut self, deadline: Instant) -> std::io::Result<Output> {
        loop {
            if self.try_wait()?.is_some() {
                return self.wait_with_output();
            }
            if Instant::now() >= deadline {
                let _ = self.kill();
                let _ = self.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "child process exceeded hard timeout",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_until(&mut self, deadline: Instant) -> std::io::Result<ExitStatus> {
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                let _ = self.kill();
                let _ = self.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "child process exceeded hard timeout",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

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
    lease_id: &'static str,
    job_id: &'static str,
    /// nested `JobManifest` 를 서명할 제출자 키. Coordinator·Agent 키와
    /// **반드시 달라야** Agent 의 독립 검증이 의미가 있다.
    submitter_seed: [u8; 32],
    submitter_pub_hex: String,
    submitter_device_id: &'static str,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let exe =
            std::env::current_exe().map_err(|e| format!("현재 실행 파일 경로를 못 얻었다: {e}"))?;

        // ★ 실제 CSPRNG 대신 라벨을 해시해 시드를 결정적으로 만든다 —
        //   이 selftest 는 매번 같은 조건으로 재현 가능해야 하고, 키
        //   프로비저닝 자체를 증명하는 것이 이 단계의 목적이 아니다
        //   (계획서 "Out" 절 — 운영용 key protection 은 범위 밖).
        let coordinator_seed = seed_from_label("coordinator-agent-selftest/coordinator");
        let agent_seed = seed_from_label("coordinator-agent-selftest/agent");

        let coordinator_pub =
            gputeer_crypto::SigningKey::from_bytes(&coordinator_seed).verifying_key();
        let agent_pub = gputeer_crypto::SigningKey::from_bytes(&agent_seed).verifying_key();
        let submitter_seed = seed_from_label("coordinator-agent-selftest/submitter");
        let submitter_pub = gputeer_crypto::SigningKey::from_bytes(&submitter_seed).verifying_key();

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
            lease_id: "01JLEASESELFTEST000000001",
            job_id: "01JJOBSELFTEST00000000001",
            submitter_seed,
            submitter_pub_hex: to_hex(submitter_pub.as_bytes()),
            submitter_device_id: "01JSUBMITSELFTEST0000000001",
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
/// This selftest intentionally has two lanes: scenarios with `--lease-db`
/// exercise the persisted lease-store path, while scenarios without it
/// exercise storage-independent contracts. In the latter lane,
/// `run_handshake()` automatically adds
/// `--i-understand-legacy-mode-is-unsafe true`; that opt-in is deliberate when
/// the contract under test must work independently of a lease-store path.
/// `gputeer submit` 를 **실제 서브프로세스로** 호출해 서명된 Manifest
/// 파일을 만든다.
///
/// ★ 라이브러리 함수를 직접 부르지 않고 프로세스로 띄운다 — 이 selftest
///   의 목적은 "제출 -> 배치 -> 실행" 이 **실제 경계를 넘어** 이어지는지
///   보이는 것이다. 같은 프로세스 안에서 함수를 부르면 그 경계가 사라진다.
/// 지금 시각(밀리초).
///
/// ★ 고정값(예: 1000)을 쓰면 Agent 가 **실제 현재 시각**로
///   검증하므로 매니페스트가 이미 만료된 것으로 거부된다 —
///   이 selftest 를 처음 배선할 때 정확히 그 것으로 한 번 실패했다.
fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_millis() as u64
}

fn run_submit(
    fixture: &Fixture,
    job_id: &str,
    entrypoint: &str,
    args_csv: &str,
    submitter_seed_hex: &str,
    out_path: &str,
) -> Result<(), String> {
    let output = Command::new(&fixture.exe)
        .args([
            "submit",
            "--job-id",
            job_id,
            "--entrypoint",
            entrypoint,
            "--args",
            args_csv,
            "--submitter-device-id",
            fixture.submitter_device_id,
            "--submitter-seed",
            submitter_seed_hex,
            "--issued-at-unix-ms",
            &now_unix_ms().to_string(),
            "--out",
            out_path,
        ])
        .output()
        .map_err(|e| format!("gputeer submit 실행 실패: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "gputeer submit 실패(job_id={job_id}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("SUBMITTED") || !stdout.contains(job_id) {
        return Err(format!(
            "gputeer submit 출력이 기대와 다르다: {stdout}"
        ));
    }
    Ok(())
}

fn run_handshake(
    fixture: &Fixture,
    extra_coordinator_args: &[&str],
    extra_agent_args: &[&str],
) -> Result<HandshakeOutcome, String> {
    run_handshake_internal(
        fixture,
        extra_coordinator_args,
        extra_agent_args,
        HANDSHAKE_HARD_TIMEOUT,
        true,
    )
}

fn run_reconnect_case(
    fixture: &Fixture,
    extra_coordinator_args: &[&str],
    extra_agent_args: &[&str],
) -> Result<HandshakeOutcome, String> {
    run_handshake_internal(
        fixture,
        extra_coordinator_args,
        extra_agent_args,
        Duration::from_secs(120),
        false,
    )
}

fn run_handshake_internal(
    fixture: &Fixture,
    extra_coordinator_args: &[&str],
    extra_agent_args: &[&str],
    hard_timeout: Duration,
    disable_reconnect: bool,
) -> Result<HandshakeOutcome, String> {
    run_handshake_internal_with_ready_hook(
        fixture,
        extra_coordinator_args,
        extra_agent_args,
        hard_timeout,
        disable_reconnect,
        None,
    )
}

fn run_handshake_internal_with_ready_hook(
    fixture: &Fixture,
    extra_coordinator_args: &[&str],
    extra_agent_args: &[&str],
    hard_timeout: Duration,
    disable_reconnect: bool,
    ready_hook: Option<&dyn Fn(&str) -> Result<(), String>>,
) -> Result<HandshakeOutcome, String> {
    let handshake_deadline = Instant::now() + hard_timeout;
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
    coordinator_args.push("--lease-id");
    coordinator_args.push(fixture.lease_id);
    coordinator_args.push("--job-id");
    coordinator_args.push(fixture.job_id);
    coordinator_args.extend_from_slice(extra_coordinator_args);
    if !extra_coordinator_args.contains(&"--lease-db") {
        coordinator_args.extend_from_slice(&["--i-understand-legacy-mode-is-unsafe", "true"]);
    }

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
    let coordinator_stdout_pipe = coordinator.stdout.take().expect("piped stdout");
    let coordinator_stderr_reader = coordinator.stderr.take().expect("piped stderr");

    // Publish READY from a reader thread, then drain stdout independently so
    // the parent can enforce the process deadline before joining it.
    let (ready_sender, ready_receiver) = mpsc::channel();
    let coordinator_stdout_reader = thread::spawn(move || -> Result<String, std::io::Error> {
        let mut reader = BufReader::new(coordinator_stdout_pipe);
        let mut ready_line = String::new();
        reader.read_line(&mut ready_line)?;
        ready_sender.send(ready_line.clone()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "READY receiver dropped")
        })?;
        let mut rest = String::new();
        reader.read_to_string(&mut rest)?;
        Ok(format!("{ready_line}{rest}"))
    });

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

    let ready_line = match ready_receiver
        .recv_timeout(handshake_deadline.saturating_duration_since(Instant::now()))
    {
        Ok(line) => line,
        Err(error) => {
            let _ = coordinator.kill();
            let _ = coordinator.wait();
            let _ = coordinator_stdout_reader.join();
            let _ = stderr_drain.join();
            return Err(format!("coordinator READY deadline exceeded: {error}"));
        }
    };
    let address = ready_line
        .trim()
        .strip_prefix("READY ")
        .ok_or_else(|| format!("coordinator 가 READY 대신 이걸 찍었다: {ready_line:?}"))?
        .to_string();

    if let Some(ready_hook) = ready_hook {
        ready_hook(&address)?;
    }

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
    // Preserve the historical DoD-24 meaning: this hook is a deliberate
    // successful ACK-then-close test, not a reconnect scenario. The new
    // reconnect helper does not go through run_handshake().
    if disable_reconnect {
        agent_args.extend_from_slice(&["--disable-reconnect", "true"]);
    }

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

    let agent_output = match agent.wait_with_output_until(handshake_deadline) {
        Ok(output) => output,
        Err(error) => {
            let _ = coordinator.kill();
            let _ = coordinator.wait();
            return Err(format!("agent-stub wait failed: {error}"));
        }
    };

    let coordinator_status = coordinator
        .wait_until(handshake_deadline)
        .map_err(|e| format!("coordinator-stub 대기 실패: {e}"))?;

    // Enforce the process deadline before joining either full-output reader.
    // This remains bounded when Coordinator is stuck in accept().
    let coordinator_stdout = coordinator_stdout_reader
        .join()
        .map_err(|_| "coordinator stdout reader panicked".to_string())?
        .map_err(|e| format!("coordinator stdout read failed: {e}"))?;
    let coordinator_stderr = stderr_drain
        .join()
        .map_err(|_| "coordinator stderr reader panicked".to_string())?
        .map_err(|e| format!("coordinator stderr read failed: {e}"))?;

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

fn run_handshake_with_checkpoint_root(
    fixture: &Fixture,
    checkpoint_root: &Path,
    extra_coordinator_args: &[&str],
    extra_agent_args: &[&str],
) -> Result<HandshakeOutcome, String> {
    let root = checkpoint_root
        .to_str()
        .ok_or_else(|| format!("checkpoint root가 UTF-8이 아니다: {checkpoint_root:?}"))?;
    let mut agent_args = extra_agent_args.to_vec();
    agent_args.extend_from_slice(&["--checkpoint-root", root]);
    run_handshake(fixture, extra_coordinator_args, &agent_args)
}

fn checkpoint_entries(root: &Path) -> Result<Vec<std::fs::DirEntry>, String> {
    let entries = std::fs::read_dir(root)
        .map_err(|error| format!("checkpoint root 읽기 실패({root:?}): {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("checkpoint root 항목 읽기 실패({root:?}): {error}"))?;
    Ok(entries)
}

fn started_checkpoint_id(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        line.strip_prefix("JOB_STARTED ")?
            .split_whitespace()
            .find_map(|field| field.strip_prefix("checkpoint_id=").map(ToOwned::to_owned))
    })
}

fn output_field<'a>(output: &'a str, marker: &str, field: &str) -> Option<&'a str> {
    output.lines().find_map(|line| {
        if !line.contains(marker) {
            return None;
        }
        line.split_whitespace()
            .find_map(|part| part.strip_prefix(field))
    })
}

fn output_u64_field(output: &str, marker: &str, field: &str) -> Result<u64, String> {
    output_field(output, marker, field)
        .ok_or_else(|| format!("missing {field} on output line containing {marker:?}: {output}"))?
        .parse::<u64>()
        .map_err(|error| format!("invalid u64 {field} on {marker:?}: {error}"))
}

fn assert_no_agent_ack_or_marker(
    outcome: &HandshakeOutcome,
    root: &Path,
    label: &str,
) -> Result<(), String> {
    let entries = checkpoint_entries(root)?;
    if !entries.is_empty()
        || outcome.agent_success
        || outcome.agent_stdout.contains("JOB_STARTED ")
        || outcome.coordinator_success
        || outcome.coordinator_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "{label}: 거부 경로에서 marker 또는 ACK/성공 결과가 관측됐다. entries={} coordinator_success={} agent_success={} coordinator_stdout={} agent_stdout={} agent_stderr={}",
            entries.len(),
            outcome.coordinator_success,
            outcome.agent_success,
            outcome.coordinator_stdout,
            outcome.agent_stdout,
            outcome.agent_stderr
        ));
    }
    Ok(())
}

fn seed_resume_lease(
    fixture: &Fixture,
    lease_db: &Path,
    fence_db: &Path,
    fence_epoch: u64,
    lease_ttl_ms: u64,
) -> Result<(), String> {
    let lease_db = lease_db
        .to_str()
        .ok_or_else(|| format!("lease DB path is not UTF-8: {lease_db:?}"))?;
    let fence_db = fence_db
        .to_str()
        .ok_or_else(|| format!("fence DB path is not UTF-8: {fence_db:?}"))?;
    let fence_epoch = fence_epoch.to_string();
    let lease_ttl_ms = lease_ttl_ms.to_string();
    let seeded = run_handshake(
        fixture,
        &[
            "--lease-db",
            lease_db,
            "--fence-epoch",
            &fence_epoch,
            "--lease-ttl-ms",
            &lease_ttl_ms,
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db, "--do-renew", "false"],
    )?;
    if !seeded.coordinator_success || !seeded.agent_success {
        return Err(format!(
            "resume seed handshake failed: coordinator={} stdout={} stderr={}; agent={} stdout={} stderr={}",
            seeded.coordinator_success,
            seeded.coordinator_stdout,
            seeded.coordinator_stderr,
            seeded.agent_success,
            seeded.agent_stdout,
            seeded.agent_stderr
        ));
    }
    Ok(())
}

fn run_resume_case(
    fixture: &Fixture,
    lease_db: &Path,
    fence_db: &Path,
    session_id: &str,
    lease_id: &str,
    job_id: &str,
    attempt_id: &str,
    fence_epoch: u64,
    unavailable_without_store: bool,
    disable_reconnect: bool,
) -> Result<HandshakeOutcome, String> {
    let lease_db = lease_db
        .to_str()
        .ok_or_else(|| format!("lease DB path is not UTF-8: {lease_db:?}"))?;
    let fence_db = fence_db
        .to_str()
        .ok_or_else(|| format!("fence DB path is not UTF-8: {fence_db:?}"))?;
    let epoch = fence_epoch.to_string();
    let mut coordinator_args = vec![
        "--resume-protocol".to_string(),
        "true".to_string(),
        "--session-id".to_string(),
        session_id.to_string(),
        "--max-connections".to_string(),
        "1".to_string(),
        "--accept-timeout-ms".to_string(),
        "5000".to_string(),
    ];
    if !unavailable_without_store {
        coordinator_args.extend(["--lease-db".to_string(), lease_db.to_string()]);
    }
    let coordinator_refs: Vec<&str> = coordinator_args.iter().map(String::as_str).collect();
    let mut agent_args = vec![
        "--resume-protocol".to_string(),
        "true".to_string(),
        "--session-id".to_string(),
        session_id.to_string(),
        "--resume-lease-id".to_string(),
        lease_id.to_string(),
        "--resume-job-id".to_string(),
        job_id.to_string(),
        "--resume-attempt-id".to_string(),
        attempt_id.to_string(),
        "--resume-fence-epoch".to_string(),
        epoch,
        "--fence-db".to_string(),
        fence_db.to_string(),
    ];
    if disable_reconnect {
        agent_args.extend(["--disable-reconnect".to_string(), "true".to_string()]);
    } else {
        agent_args.extend([
            "--max-reconnect-attempts".to_string(),
            "2".to_string(),
            "--max-reconnect-duration-seconds".to_string(),
            "5".to_string(),
            "--retry-base-ms".to_string(),
            "1".to_string(),
            "--retry-cap-ms".to_string(),
            "1".to_string(),
        ]);
    }
    let agent_refs: Vec<&str> = agent_args.iter().map(String::as_str).collect();
    run_handshake_internal(
        fixture,
        &coordinator_refs,
        &agent_refs,
        HANDSHAKE_HARD_TIMEOUT,
        disable_reconnect,
    )
}

/// Start a healthy Resume Coordinator, wait until it has opened and bound the
/// lease DB, then corrupt the already-open SQLite file before the request is
/// sent. This exercises the dispatcher classification during Resume rather
/// than the startup-open failure covered by scenario 63.
fn run_resume_storage_failure_case(
    fixture: &Fixture,
    lease_db: &Path,
    fence_db: &Path,
) -> Result<HandshakeOutcome, String> {
    let lease_db = lease_db.to_str().ok_or_else(|| {
        format!("resume storage-failure lease DB path is not UTF-8: {lease_db:?}")
    })?;
    let fence_db = fence_db.to_str().ok_or_else(|| {
        format!("resume storage-failure fence DB path is not UTF-8: {fence_db:?}")
    })?;
    let session_id = "resume-session-64";
    let coordinator_args = [
        "--resume-protocol",
        "true",
        "--session-id",
        session_id,
        "--max-connections",
        "2",
        "--accept-timeout-ms",
        "5000",
        "--lease-db",
        lease_db,
    ];
    let agent_args = [
        "--resume-protocol",
        "true",
        "--session-id",
        session_id,
        "--resume-lease-id",
        fixture.lease_id,
        "--resume-job-id",
        fixture.job_id,
        "--resume-attempt-id",
        fixture.attempt_id,
        "--resume-fence-epoch",
        "7",
        "--fence-db",
        fence_db,
        "--disable-reconnect",
        "true",
    ];
    let lease_db_path = lease_db.to_owned();
    let corrupt_after_ready = move |_address: &str| {
        std::fs::write(&lease_db_path, b"not a sqlite database")
            .map_err(|error| format!("resume storage-failure DB corruption failed: {error}"))
    };
    run_handshake_internal_with_ready_hook(
        fixture,
        &coordinator_args,
        &agent_args,
        Duration::from_secs(120),
        true,
        Some(&corrupt_after_ready),
    )
}

fn assert_resume_outcome(
    outcome: &HandshakeOutcome,
    scenario: u32,
    expected_outcome: u32,
    agent_should_succeed: bool,
) -> Result<(), String> {
    let marker = format!("RESULT ok=true resume_outcome={expected_outcome}");
    let expected_refusal = match expected_outcome {
        2 => Some("RESUME_REFUSED:REVOKED"),
        3 => Some("RESUME_REFUSED:EXPIRED"),
        4 => Some("RESUME_REFUSED:SUPERSEDED"),
        5 => Some("RESUME_REFUSED:UNKNOWN_LEASE"),
        6 => Some("RESUME_REFUSED:IDENTITY_CONFLICT"),
        8 => Some("RESUME_REFUSED:EPOCH_AHEAD"),
        _ => None,
    };
    let agent_output = format!("{}\n{}", outcome.agent_stdout, outcome.agent_stderr);
    let refusal_matches = expected_refusal.is_none_or(|expected| agent_output.contains(expected));
    let retried_to_exhaustion = agent_output.contains("ReconnectExhausted:");
    let accepted_connection_count = outcome
        .coordinator_stdout
        .matches("CONNECTION_ATTEMPT")
        .count();
    if !outcome.coordinator_success
        || !outcome.coordinator_stdout.contains(&marker)
        || outcome.agent_success != agent_should_succeed
        || (agent_should_succeed && !outcome.agent_stdout.contains(&marker))
        || (!agent_should_succeed && outcome.agent_stdout.contains("RESULT ok=true"))
        || !refusal_matches
        || retried_to_exhaustion
        || accepted_connection_count != 1
    {
        return Err(format!(
            "{scenario}) Resume outcome mismatch: expected={} expected_refusal={:?} retried_to_exhaustion={} accepted_connection_count={} coordinator_success={} coordinator_stdout={} coordinator_stderr={} agent_success={} agent_stdout={} agent_stderr={}",
            expected_outcome,
            expected_refusal,
            retried_to_exhaustion,
            accepted_connection_count,
            outcome.coordinator_success,
            outcome.coordinator_stdout,
            outcome.coordinator_stderr,
            outcome.agent_success,
            outcome.agent_stdout,
            outcome.agent_stderr
        ));
    }
    Ok(())
}

/// 37가지 시나리오를 차례로 돌린다. 하나라도 기대와 다르면 그 자리에서
/// 이유를 담아 반환한다.
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
    if forged_grant.coordinator_success
        || forged_grant.coordinator_stdout.contains(RESULT_OK_MARKER)
    {
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
            forged_ack.coordinator_success,
            forged_ack.coordinator_stdout,
            forged_ack.coordinator_stderr
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

    // ── 5. 위조 nested Lease 서명 — outer Grant 는 정상인데 Agent 가
    //    Lease 를 독립적으로 검증해야만 잡힌다 ──────────────────────
    // ★ Coordinator 는 이 시나리오에서 outer Grant 를 정상 서명하므로
    //   자기 자신은 "성공"을 주장할 수 있다(outer 검증만 보면 다
    //   맞다) — 판정은 Agent 가 LEASE_REJECTED 로 거부했는지로만 한다.
    let forged_lease = run_handshake(&fixture, &["--corrupt-lease-signature", "true"], &[])?;
    if forged_lease.agent_success || !forged_lease.agent_stderr.contains("LEASE_REJECTED:") {
        return Err(format!(
            "위조된 nested Lease 서명이 거부되지 않았다 — outer Grant 검증만으로는 \
             이 결함을 잡지 못한다는 뜻이다.\n\
             agent exit={} stdout={} stderr={}",
            forged_lease.agent_success, forged_lease.agent_stdout, forged_lease.agent_stderr
        ));
    }
    report.push_str(
        "5) 위조 nested Lease 서명 거부 확인 (Agent 가 Lease 를 outer Grant 와 \
         독립적으로 검증한다)\n",
    );

    // ── 6. 만료된 Lease ────────────────────────────────────────────
    let expired_lease = run_handshake(&fixture, &["--expire-lease", "true"], &[])?;
    if expired_lease.agent_success || !expired_lease.agent_stderr.contains("LEASE_REJECTED:") {
        return Err(format!(
            "만료된 Lease 가 거부되지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            expired_lease.agent_success, expired_lease.agent_stdout, expired_lease.agent_stderr
        ));
    }
    report.push_str("6) 만료된 Lease 거부 확인 (Lease::LIFETIME == LongLived 의 만료 검사)\n");

    // ══════════════════════════════════════════════════════════════
    // Lease 갱신 (2026-08-19, `docs/plans/2026-08-19_0500_...`)
    // ══════════════════════════════════════════════════════════════

    // ── 7. 정상 Lease 갱신 (같은 epoch 유지) ─────────────────────────
    let renew_ok = run_handshake(
        &fixture,
        &["--do-renew", "true", "--renewed-fence-epoch", "0"],
        &["--do-renew", "true"],
    )?;
    if !renew_ok.coordinator_success || !renew_ok.agent_success {
        return Err(format!(
            "정상 Lease 갱신이 실패했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            renew_ok.coordinator_success,
            renew_ok.coordinator_stdout,
            renew_ok.coordinator_stderr,
            renew_ok.agent_success,
            renew_ok.agent_stdout,
            renew_ok.agent_stderr
        ));
    }
    if !renew_ok
        .agent_stdout
        .contains("RENEW_RESULT ok=true outcome=RENEWED")
    {
        return Err(format!(
            "정상 Lease 갱신인데 Agent 가 RENEW_RESULT 를 찍지 않았다.\nagent stdout: {}",
            renew_ok.agent_stdout
        ));
    }
    report.push_str("7) 정상 Lease 갱신 성공 (같은 epoch 유지, Agent 가 새 Lease 를 독립 검증)\n");

    // ── 8. 위조 RenewLeaseRequest.node_signature — Coordinator ingress 검증 실패 ──
    let forged_renew_req = run_handshake(
        &fixture,
        &["--do-renew", "true", "--renewed-fence-epoch", "0"],
        &[
            "--do-renew",
            "true",
            "--corrupt-renew-request-signature",
            "true",
        ],
    )?;
    if forged_renew_req.coordinator_success
        || forged_renew_req.coordinator_stdout.contains(RESULT_OK_MARKER)
        // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — 실패 여부만 보지
        //   말고 **왜** 실패했는지 특정한다. 그러지 않으면 이 시나리오가
        //   무관한 이유(예: 다른 버그로 인한 크래시)로도 우연히 통과할
        //   수 있다.
        || !forged_renew_req
            .coordinator_stderr
            .contains("RenewLeaseRequest 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "위조된 RenewLeaseRequest.node_signature 가 거부되지 않았다 — coordinator 가 성공을 주장했거나 \
             기대한 이유(RenewLeaseRequest 프레임 읽기/검증 실패)로 거부하지 않았다.\n\
             coordinator exit={} stdout={} stderr={}",
            forged_renew_req.coordinator_success,
            forged_renew_req.coordinator_stdout,
            forged_renew_req.coordinator_stderr
        ));
    }
    report.push_str(
        "8) 위조 RenewLeaseRequest.node_signature 거부 확인 (Coordinator ingress 검증 실패)\n",
    );

    // ── 9. 위조 RenewLeaseResult.coordinator_signature — Agent 결과 서명 검증 실패 ──
    let forged_renew_result = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--corrupt-renew-result-signature",
            "true",
        ],
        &["--do-renew", "true"],
    )?;
    if forged_renew_result.agent_success
        || !forged_renew_result
            .agent_stderr
            .contains("RenewLeaseResult 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "위조된 RenewLeaseResult.coordinator_signature 가 거부되지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            forged_renew_result.agent_success,
            forged_renew_result.agent_stdout,
            forged_renew_result.agent_stderr
        ));
    }
    report.push_str(
        "9) 위조 RenewLeaseResult.coordinator_signature 거부 확인 (Agent 결과 서명 검증 실패)\n",
    );

    // ── 10. 위조 nested 새 Lease 서명 — outer 결과 서명은 정상, Agent 독립 검증으로 거부 ──
    let forged_renewed_lease = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--corrupt-renewed-lease-signature",
            "true",
        ],
        &["--do-renew", "true"],
    )?;
    if forged_renewed_lease.agent_success
        || !forged_renewed_lease
            .agent_stderr
            .contains("RENEW_REJECTED: 갱신된 Lease 서명 검증 실패")
    {
        return Err(format!(
            "위조된 nested Lease 서명(갱신)이 거부되지 않았다 — outer 결과 검증만으로는 \
             이 결함을 잡지 못한다는 뜻이다.\n\
             agent exit={} stdout={} stderr={}",
            forged_renewed_lease.agent_success,
            forged_renewed_lease.agent_stdout,
            forged_renewed_lease.agent_stderr
        ));
    }
    report.push_str(
        "10) 위조 nested 새 Lease 서명 거부 확인 (Agent 가 갱신된 Lease 를 outer 결과와 \
         독립적으로 검증한다)\n",
    );

    // ── 11. epoch 강등 — Agent 의 FenceWatermark 가 거부 ────────────
    let downgrade = run_handshake(
        &fixture,
        &[
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "3",
        ],
        &["--do-renew", "true"],
    )?;
    if downgrade.agent_success
        || !downgrade
            .agent_stderr
            .contains("RENEW_REJECTED: fence_epoch 검사 실패")
    {
        return Err(format!(
            "낮은 fence_epoch 로 갱신했는데 Agent watermark 가 거부하지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            downgrade.agent_success, downgrade.agent_stdout, downgrade.agent_stderr
        ));
    }
    report.push_str(
        "11) epoch 강등 거부 확인 (FenceWatermark.check_and_advance 가 낮은 epoch 를 거부)\n",
    );

    // ── 12. RENEW_OUTCOME_SUPERSEDED — 서명된 정상 정책 거부로 분류 ──
    let superseded = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--renew-outcome-override",
            "2",
        ],
        &["--do-renew", "true"],
    )?;
    if superseded.agent_success || !superseded.agent_stderr.contains("RENEW_REFUSED:SUPERSEDED") {
        return Err(format!(
            "RENEW_OUTCOME_SUPERSEDED 가 서명된 정상 정책 거부로 분류되지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            superseded.agent_success, superseded.agent_stdout, superseded.agent_stderr
        ));
    }
    report.push_str(
        "12) RENEW_OUTCOME_SUPERSEDED 서명된 정상 정책 거부로 분류 확인 (Lease·watermark 불변)\n",
    );

    // ── 13. RENEW_OUTCOME_QUARANTINED — 서명된 정상 정책 거부로 분류 ──
    let quarantined = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--renew-outcome-override",
            "3",
        ],
        &["--do-renew", "true"],
    )?;
    if quarantined.agent_success
        || !quarantined
            .agent_stderr
            .contains("RENEW_REFUSED:QUARANTINED")
    {
        return Err(format!(
            "RENEW_OUTCOME_QUARANTINED 가 서명된 정상 정책 거부로 분류되지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            quarantined.agent_success, quarantined.agent_stdout, quarantined.agent_stderr
        ));
    }
    report.push_str(
        "13) RENEW_OUTCOME_QUARANTINED 서명된 정상 정책 거부로 분류 확인 (Lease·watermark 불변)\n",
    );

    // ── 14. RenewLeaseResult.request_nonce 를 echo 하지 않음 — Agent 거부 ──
    //
    // ★ 계획서 "6종" 표에는 없지만, "In" 절이 request_nonce 의 목적으로
    //   든 것과 정확히 같은 공격이다 — 서명은 정상인 결과를 **다른**
    //   갱신 요청에 재사용하는 시나리오. 구현 중 발견한 별도 게이트라
    //   여기 추가한다.
    let wrong_nonce = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--corrupt-renew-result-nonce",
            "true",
        ],
        &["--do-renew", "true"],
    )?;
    if wrong_nonce.agent_success
        || !wrong_nonce
            .agent_stderr
            .contains("RENEW_REJECTED: request_nonce 가 우리가 보낸 요청과 다르다")
    {
        return Err(format!(
            "RenewLeaseResult.request_nonce 가 echo 되지 않았는데 Agent 가 거부하지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            wrong_nonce.agent_success, wrong_nonce.agent_stdout, wrong_nonce.agent_stderr
        ));
    }
    report.push_str(
        "14) RenewLeaseResult.request_nonce 불일치 거부 확인 (응답이 다른 요청에 재사용되는 것을 방지)\n",
    );

    // ── 15. epoch 상승 — 계획서가 명시적으로 범위 밖(정책상 거부)이라고 정한 경우 ──
    //
    // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — `FenceWatermark` 는
    //   `<` 만 거부하고 `>` 는 정상적인 전진으로 통과시킨다. 계획서는
    //   "높으면 이 조각에서는 정책상 거부"라고 명시했으므로, Agent 가
    //   watermark 와 별개로 명시적으로 막아야 한다.
    let upgrade = run_handshake(
        &fixture,
        &["--do-renew", "true", "--renewed-fence-epoch", "5"],
        &["--do-renew", "true"],
    )?;
    if upgrade.agent_success
        || !upgrade
            .agent_stderr
            .contains("epoch 상승은 이 조각의 범위 밖이라 정책상 거부한다")
    {
        return Err(format!(
            "epoch 상승(0 -> 5) 갱신을 Agent 가 거부하지 않았다 — 계획서는 이를 범위 밖 정책 거부로 \
             명시했다.\n\
             agent exit={} stdout={} stderr={}",
            upgrade.agent_success, upgrade.agent_stdout, upgrade.agent_stderr
        ));
    }
    report.push_str(
        "15) epoch 상승 거부 확인 (계획서 범위 — FenceWatermark 만으로는 안 잡히고 명시적 검사가 필요하다)\n",
    );

    // ── 16. Coordinator 가 요청의 fence_epoch 을 자신이 기억하는 값과 대조 ──
    //
    // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — 이전에는
    //   `renew_req.fence_epoch` 를 아무것도와 비교하지 않았다.
    let epoch_mismatch = run_handshake(
        &fixture,
        &["--do-renew", "true", "--renewed-fence-epoch", "0"],
        &["--do-renew", "true", "--renew-request-epoch-override", "99"],
    )?;
    if epoch_mismatch.coordinator_success
        || epoch_mismatch.coordinator_stdout.contains(RESULT_OK_MARKER)
        || !epoch_mismatch
            .coordinator_stderr
            .contains("RenewLeaseRequest.fence_epoch 불일치")
    {
        return Err(format!(
            "요청의 fence_epoch(99) 이 Coordinator 가 기억하는 값(0)과 다른데 거부되지 않았다.\n\
             coordinator exit={} stdout={} stderr={}",
            epoch_mismatch.coordinator_success,
            epoch_mismatch.coordinator_stdout,
            epoch_mismatch.coordinator_stderr
        ));
    }
    report.push_str(
        "16) RenewLeaseRequest.fence_epoch 불일치 거부 확인 (Coordinator 가 요청 epoch 을 자신이 \
         기억하는 값과 대조한다)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // durable FenceWatermark (2026-08-19, `docs/plans/2026-08-19_2200_...`)
    //
    // 지금까지의 시나리오는 매번 새 `--fence-db` 임시 파일(기본값)을
    // 쓰므로 매 프로세스가 빈 watermark 에서 시작한다 — "재시작을
    // 넘는 방어"를 증명하지 않는다. 아래는 **같은 SQLite 파일**을
    // 서로 다른 `agent-stub` **프로세스**(별도 PID)가 순서대로 열어,
    // 앞선 프로세스가 기록한 watermark 가 실제로 남아 있는지 확인한다.
    //
    // ★ 설계 당시엔 "최초 Grant 경로"·"갱신 경로" 두 개를 따로
    //   시험하려 했다(`docs/plans/2026-08-19_2200_...v1.md` §재시작
    //   selftest 설계). **뮤테이션 테스트로 실제로 만들어보니 그 둘은
    //   분리되지 않는다** — Agent 의 제어 흐름상 갱신은 항상 같은
    //   프로세스의 최초 Grant 검증 **뒤에** 오고, 최초 Grant 검증
    //   자체가 이미 그 프로세스의 watermark 로컬 뷰를 durable 값으로
    //   채운다. 그래서 "최초 Grant 는 낮은 epoch 로 통과시키고 그
    //   프로세스의 갱신만 거부하는" 조합을 만들면, 그 거부는 진짜
    //   프로세스 경계를 넘는 영속성이 아니라 **그 프로세스 자신의
    //   최초 Grant 호출이 방금 쓴 값**만으로도 똑같이 재현된다 —
    //   `DurableFenceWatermark::open()` 이 주어진 경로를 무시하고 매번
    //   새 파일을 열도록 무력화해봤더니, "최초 Grant 경로" 거부는
    //   실패했지만 "갱신 경로" 거부는 **속아서 계속 통과했다**(그
    //   프로세스 자신의 최초 Grant 가 이미 watermark=5 를 로컬에
    //   써 놓았기 때문). 즉 진짜 재시작 방어의 유일한 관측 지점은
    //   **최초 Grant 검증 호출부 하나뿐**이다 — 그것이 durable 하면
    //   그 위에 올라타는 갱신도 자동으로 안전하고, 그것이 durable 하지
    //   않으면 갱신 검증이 아무리 정확해도 프로세스 경계를 넘는 방어는
    //   전혀 없다. 그래서 시나리오를 하나로 정리했다 — 존재하지 않는
    //   구분을 존재하는 것처럼 보고하지 않는다.

    let fence_dir =
        tempfile::tempdir().map_err(|e| format!("fence watermark 임시 디렉터리 생성 실패: {e}"))?;
    let fence_db_path = fence_dir.path().join("fence.sqlite3");
    let fence_db = fence_db_path
        .to_str()
        .ok_or_else(|| "fence watermark 경로가 UTF-8 이 아니다".to_string())?;

    // ── 17. durable FenceWatermark 재시작 방어 ───────────────────────
    //
    // 1차(별도 프로세스): epoch 5 로 정상 발급 — durable watermark 가
    //   5 로 기록된 채 프로세스가 종료된다.
    // 2차(★ 별도 프로세스, 같은 DB): 최초 Grant 자체가 epoch=3(durable
    //   watermark 보다 낮다)을 담아 발급된다 — `verify_and_record_lease()`
    //   의 최초 검증 호출부가 거부해야 한다. **1차 프로세스의 메모리가
    //   2차 프로세스에 전달되지 않았다는 것**은 PID 가 다르고 각 실행이
    //   독립적으로 종료된다는 사실이 보장한다 — DB 파일만 상태를
    //   전달한다.
    let restart_first = run_handshake(
        &fixture,
        &["--fence-epoch", "5", "--do-renew", "false"],
        &["--do-renew", "false", "--fence-db", fence_db],
    )?;
    if !restart_first.coordinator_success || !restart_first.agent_success {
        return Err(format!(
            "재시작 방어 시나리오 1차 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            restart_first.coordinator_success,
            restart_first.coordinator_stdout,
            restart_first.coordinator_stderr,
            restart_first.agent_success,
            restart_first.agent_stdout,
            restart_first.agent_stderr
        ));
    }

    let restart_second = run_handshake(
        &fixture,
        &["--fence-epoch", "3", "--do-renew", "false"],
        &["--do-renew", "false", "--fence-db", fence_db],
    )?;
    if restart_second.agent_pid == restart_first.agent_pid {
        return Err(format!(
            "재시작 방어 시나리오의 2차 agent PID 가 1차와 같다 — 별도 프로세스가 \
             아니다.\n1차 agent_pid={} 2차 agent_pid={}",
            restart_first.agent_pid, restart_second.agent_pid
        ));
    }
    // ★ 코덱스 독립 검수(2026-08-19, p102) 지적 — 접두사
    //   "LEASE_REJECTED: fence_epoch 검사 실패" 만으로는 진짜 정책
    //   거부(`Stale`)와 저장소 장애(`Io`/`LockTimeout`)를 구분하지
    //   못한다(둘 다 그 접두사를 낼 수 있었다 — 지금은 갈라졌지만,
    //   회귀를 대비해 여기서 구체적인 값까지 확인한다). "(정책 거부)"
    //   표시와 `StaleEpoch` 의 실제 수치(incoming=3, watermark=5)까지
    //   본다 — 저장소 장애였다면 이 문구가 나올 수 없다.
    if restart_second.agent_success
        || !restart_second
            .agent_stderr
            .contains("LEASE_REJECTED: fence_epoch 검사 실패(정책 거부)")
        || !restart_second
            .agent_stderr
            .contains("fence_epoch 3 은 기록된 watermark 5 보다 낮다")
    {
        return Err(format!(
            "durable watermark 가 재시작(별도 프로세스)을 넘어 낮은 epoch 의 최초 Grant 를 \
             거부하지 못했다(또는 거부 이유가 정책 거부가 아니다 — 저장소 장애와 혼동됐을 \
             수 있다).\n\
             1차 agent_pid={} 2차 agent_pid={}\n\
             2차 agent exit={} stdout={} stderr={}",
            restart_first.agent_pid,
            restart_second.agent_pid,
            restart_second.agent_success,
            restart_second.agent_stdout,
            restart_second.agent_stderr
        ));
    }
    if restart_second.coordinator_success
        || restart_second.coordinator_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "Agent 가 최초 Grant 를 거부해 ACK 를 보내지 않았는데 coordinator 가 성공을 \
             주장했다.\n\
             coordinator exit={} stdout={} stderr={}",
            restart_second.coordinator_success,
            restart_second.coordinator_stdout,
            restart_second.coordinator_stderr
        ));
    }
    report.push_str(
        "17) durable FenceWatermark 재시작 방어 확인 — 별도 프로세스가 SQLite 파일로 이전 \
         watermark 를 물려받아 낮은 epoch 의 최초 Grant 를 거부한다(뮤테이션 테스트로 확인한 \
         대로, 이 하나의 관측 지점이 그 위에 올라타는 갱신 경로까지 보호한다)\n",
    );

    // ── 18. `--fence-db :memory:` 는 fail closed — 코덱스 독립 검수(2026-08-19, p102) 지적 ──
    //
    // ★ 전에는 `is_durable() == false` 인 채로도 계속 진행했다 — 이
    //   조각 전체의 목적(재시작을 넘는 방어)이 겉으로는 durable
    //   타입을 쓰는 것처럼 보이면서 조용히 거짓이 될 수 있었다.
    //   Coordinator 조차 필요 없다 — Agent 의 fail-closed 검사가
    //   `TcpStream::connect()` 보다 먼저 실행되므로, 연결 대상 주소가
    //   실재하지 않아도 절대 도달하지 않는다.
    let (memory_ok, _memory_stdout, memory_stderr) =
        run_agent_alone(&fixture, &["--fence-db", ":memory:"])?;
    if memory_ok || !memory_stderr.contains("fence watermark 저장소가 영속이 아니다") {
        return Err(format!(
            "--fence-db :memory: 가 fail closed 되지 않았다.\nexit ok={memory_ok} stderr={memory_stderr}"
        ));
    }
    report.push_str(
        "18) --fence-db :memory: fail closed 확인 (비영속 경로로 재시작 방어를 흉내내지 못한다)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // 반복 Lease 갱신 (2026-08-19, `docs/plans/2026-08-19_2330_...`)
    //
    // 설계(`p105`)가 코드 경로로 확정한 핵심 위험 — 갱신 요청의
    // nonce 가 lease_id 에서만 결정적으로 유도되면, 회차마다 같은
    // nonce 가 나와 두 번째 요청부터 replay guard 가 Duplicate 로
    // 거부한다. round 를 nonce 입력에 섞지 않았다면 이 시나리오는
    // round 1(0-based)에서 즉시 실패한다 — **성공 자체가 그 결함이
    // 고쳐졌다는 증거**다.
    // ══════════════════════════════════════════════════════════════

    // ── 19. 같은 연결에서 3회 정상 갱신 ──────────────────────────────
    let repeated = run_handshake(
        &fixture,
        &[
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
            "--renew-rounds",
            "3",
        ],
        &["--do-renew", "true", "--renew-rounds", "3"],
    )?;
    if !repeated.coordinator_success || !repeated.agent_success {
        return Err(format!(
            "같은 연결에서 3회 반복 갱신이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            repeated.coordinator_success,
            repeated.coordinator_stdout,
            repeated.coordinator_stderr,
            repeated.agent_success,
            repeated.agent_stdout,
            repeated.agent_stderr
        ));
    }
    let agent_renewed_count = repeated
        .agent_stdout
        .matches("RENEW_RESULT ok=true outcome=RENEWED")
        .count();
    let coordinator_renewed_count = repeated
        .coordinator_stdout
        .matches("RENEW_RESULT ok=true outcome=")
        .count();
    // ★ "RESULT ok=true" 는 "RENEW_RESULT ok=true" 의 부분 문자열이다
    //   — 줄 단위로 정확히 그 접두사로 시작하는 줄만 센다.
    let agent_result_count = repeated
        .agent_stdout
        .lines()
        .filter(|line| line.starts_with(RESULT_OK_MARKER))
        .count();
    if agent_renewed_count != 3 || coordinator_renewed_count != 3 || agent_result_count != 1 {
        return Err(format!(
            "3회 반복 갱신의 출력 횟수가 기대와 다르다 — agent RENEWED={agent_renewed_count}(기대 3), \
             coordinator RENEW_RESULT={coordinator_renewed_count}(기대 3), agent 최종 RESULT={agent_result_count}(기대 1).\n\
             agent stdout={}\ncoordinator stdout={}",
            repeated.agent_stdout, repeated.coordinator_stdout
        ));
    }
    report.push_str(
        "19) 같은 연결에서 3회 정상 갱신 확인 (회차별 nonce 분리로 replay guard 의 \
         Duplicate 거부를 피한다 — 성공 자체가 증거다)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // Coordinator 영속 Lease 저장소 (2026-08-19, `docs/plans/2026-08-19_2300_...`)
    //
    // 지금까지의 시나리오는 `--lease-db` 를 안 주므로 Coordinator 가
    // 매번 CLI 인자만으로 상태를 구성하는 레거시 경로를 쓴다. 아래는
    // **같은 SQLite 파일**을 서로 다른 `coordinator-stub` 프로세스가
    // 순서대로 열어, 저장된 Lease 신원·epoch 가 CLI 인자보다
    // 우선하는지 확인한다. Agent 쪽 durable FenceWatermark 설계 때
    // 겪은 함정(갱신 전용 시나리오가 실제로는 최초 검증에 의해 이미
    // 결정돼 판별력이 없었던 문제)을 참고해, 두 시나리오 모두
    // **Coordinator 가 스토어를 무시했다면 관측 가능한 방식으로
    // 실패해야 한다**는 조건을 명시적으로 설계했다.
    // ══════════════════════════════════════════════════════════════

    // ── 20. 발급 상태 복원 ────────────────────────────────────────────
    //
    // 1차(별도 프로세스): epoch 5 로 정상 발급 + 같은 epoch 갱신 —
    //   lease store 에 epoch=5 가 저장되고, Agent 의 durable
    //   watermark 에도 5 가 남는다(같은 --fence-db 재사용).
    // 2차(★ 별도 프로세스, 같은 두 DB): Coordinator 를 **의도적으로
    //   틀린 epoch=3** 으로 실행한다. Coordinator 가 스토어를 쓰면
    //   `get_or_issue()` 가 저장된 값(5)을 그대로 반환해 Grant 도
    //   epoch=5 로 나간다 — Agent 의 watermark(5)와 일치해 성공한다.
    //   반대로 스토어를 무시하고 CLI 값(3)을 그대로 썼다면 Agent 의
    //   watermark(5)가 3 을 거부해 **관측 가능하게** 실패한다.
    let lease_dir_20 =
        tempfile::tempdir().map_err(|e| format!("lease store 임시 디렉터리 생성 실패(20): {e}"))?;
    let lease_db_path_20 = lease_dir_20.path().join("leases.sqlite3");
    let lease_db_20 = lease_db_path_20
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;
    let fence_dir_20 = tempfile::tempdir()
        .map_err(|e| format!("fence watermark 임시 디렉터리 생성 실패(20): {e}"))?;
    let fence_db_path_20 = fence_dir_20.path().join("fence.sqlite3");
    let fence_db_20 = fence_db_path_20
        .to_str()
        .ok_or_else(|| "fence watermark 경로가 UTF-8 이 아니다".to_string())?;

    let issue_first = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_20,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
        ],
        &["--fence-db", fence_db_20, "--do-renew", "true"],
    )?;
    if !issue_first.coordinator_success || !issue_first.agent_success {
        return Err(format!(
            "발급 상태 복원 시나리오 1차 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_first.coordinator_success,
            issue_first.coordinator_stdout,
            issue_first.coordinator_stderr,
            issue_first.agent_success,
            issue_first.agent_stdout,
            issue_first.agent_stderr
        ));
    }

    let issue_second = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_20,
            "--fence-epoch",
            "3",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_20, "--do-renew", "false"],
    )?;
    if issue_second.coordinator_pid == issue_first.coordinator_pid {
        return Err(format!(
            "발급 상태 복원 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 프로세스가 \
             아니다.\n1차 coordinator_pid={} 2차 coordinator_pid={}",
            issue_first.coordinator_pid, issue_second.coordinator_pid
        ));
    }
    if !issue_second.coordinator_success || !issue_second.agent_success {
        return Err(format!(
            "lease store 가 재시작(별도 프로세스)을 넘어 저장된 epoch(5)를 CLI 의 틀린 \
             값(3)보다 우선시키지 못했다 — Coordinator 가 스토어를 무시하고 CLI 값을 그대로 \
             썼다면 Agent 의 durable watermark(5) 가 epoch=3 을 거부해 이렇게 실패한다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_first.coordinator_pid,
            issue_second.coordinator_pid,
            issue_second.coordinator_success,
            issue_second.coordinator_stdout,
            issue_second.coordinator_stderr,
            issue_second.agent_success,
            issue_second.agent_stdout,
            issue_second.agent_stderr
        ));
    }
    report.push_str(
        "20) Coordinator 영속 Lease 저장소 — 발급 상태 복원 확인 (별도 프로세스가 SQLite \
         파일로 저장된 epoch 를 CLI 의 틀린 값보다 우선시킨다)\n",
    );

    // ── 21. 재시작 후 갱신 대조 ────────────────────────────────────────
    //
    // 1차(별도 프로세스): epoch 5 로 발급만 한다(갱신 없음) — lease
    //   store 에 epoch=5, Agent watermark 에도 5 가 남는다.
    // 2차(★ 별도 프로세스, 같은 두 DB): Coordinator 를 **의도적으로
    //   틀린 epoch=6** 으로 실행하고 갱신까지 수행한다.
    //   `get_or_issue()` 가 최초 Grant 를 저장된 epoch=5 로 자동
    //   교정하므로, Agent 는 held_lease.fence_epoch=5 로 갱신 요청을
    //   만든다. 이 요청을 Coordinator 가 **저장소의 epoch(5)와 대조**
    //   하면 일치해 갱신이 성공한다 — 만약 여전히 `config.fence_epoch`
    //   (틀린 값 6)과 비교했다면 5 != 6 으로 **정당한 갱신을 잘못
    //   거부**했을 것이다. 이 시나리오는 그 회귀를 정확히 잡는다.
    let lease_dir_21 =
        tempfile::tempdir().map_err(|e| format!("lease store 임시 디렉터리 생성 실패(21): {e}"))?;
    let lease_db_path_21 = lease_dir_21.path().join("leases.sqlite3");
    let lease_db_21 = lease_db_path_21
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;
    let fence_dir_21 = tempfile::tempdir()
        .map_err(|e| format!("fence watermark 임시 디렉터리 생성 실패(21): {e}"))?;
    let fence_db_path_21 = fence_dir_21.path().join("fence.sqlite3");
    let fence_db_21 = fence_db_path_21
        .to_str()
        .ok_or_else(|| "fence watermark 경로가 UTF-8 이 아니다".to_string())?;

    let renew_first = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_21,
            "--fence-epoch",
            "5",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_21, "--do-renew", "false"],
    )?;
    if !renew_first.coordinator_success || !renew_first.agent_success {
        return Err(format!(
            "재시작 후 갱신 대조 시나리오 1차 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            renew_first.coordinator_success,
            renew_first.coordinator_stdout,
            renew_first.coordinator_stderr,
            renew_first.agent_success,
            renew_first.agent_stdout,
            renew_first.agent_stderr
        ));
    }

    let renew_second = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_21,
            "--fence-epoch",
            "6",
            "--do-renew",
            "true",
        ],
        &["--fence-db", fence_db_21, "--do-renew", "true"],
    )?;
    if renew_second.coordinator_pid == renew_first.coordinator_pid {
        return Err(format!(
            "재시작 후 갱신 대조 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 \
             프로세스가 아니다.\n1차 coordinator_pid={} 2차 coordinator_pid={}",
            renew_first.coordinator_pid, renew_second.coordinator_pid
        ));
    }
    if !renew_second.coordinator_success || !renew_second.agent_success {
        return Err(format!(
            "lease store 가 재시작을 넘어 갱신 요청의 fence_epoch 대조에 저장된 값을 \
             쓰지 못했다 — 여전히 그 실행의 CLI 값(6)과 비교했다면 저장된 값(5)과 달라 \
             정당한 갱신을 잘못 거부했을 것이다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            renew_first.coordinator_pid,
            renew_second.coordinator_pid,
            renew_second.coordinator_success,
            renew_second.coordinator_stdout,
            renew_second.coordinator_stderr,
            renew_second.agent_success,
            renew_second.agent_stdout,
            renew_second.agent_stderr
        ));
    }
    report.push_str(
        "21) Coordinator 영속 Lease 저장소 — 재시작 후 갱신 대조 확인 (별도 프로세스가 \
         갱신 요청의 fence_epoch 을 저장된 값과 대조한다, 그 실행의 CLI 값이 아니라)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // max_total_duration_seconds 갱신 차단 (2026-08-19, `docs/plans/2026-08-19_2350_...`)
    //
    // 실제로 시간을 흘려보내야 판별력이 생긴다 — 짧은 한도
    // (`--max-total-duration-seconds 2`)를 CLI 로 주고, 이 selftest
    // 프로세스가 실제로 2.2초 넘게 sleep 한 뒤 **별도** Coordinator/
    // Agent 프로세스로 갱신을 시도한다. `--renew-rounds` 만 늘리는
    // 것으로는 판별할 수 없다 — 현재 반복 요청 사이에 sleep 이 없어
    // 모든 라운드가 한도 안에서 끝난다(설계 p111).
    // ══════════════════════════════════════════════════════════════

    // ── 22. 누적 시간 초과 — 서명된 MAX_DURATION_EXCEEDED 로 갱신 거부 ──
    let lease_dir_22 =
        tempfile::tempdir().map_err(|e| format!("lease store 임시 디렉터리 생성 실패(22): {e}"))?;
    let lease_db_path_22 = lease_dir_22.path().join("leases.sqlite3");
    let lease_db_22 = lease_db_path_22
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;

    let issue_22 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_22,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "2",
            "--do-renew",
            "false",
        ],
        &["--do-renew", "false"],
    )?;
    if !issue_22.coordinator_success || !issue_22.agent_success {
        return Err(format!(
            "누적 시간 초과 시나리오 1차(최초 발급) 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_22.coordinator_success,
            issue_22.coordinator_stdout,
            issue_22.coordinator_stderr,
            issue_22.agent_success,
            issue_22.agent_stdout,
            issue_22.agent_stderr
        ));
    }

    // 저장소에 실제로 기록된 만료시각을 미리 읽어둔다 — 초과 판정
    // 뒤에도 이 값이 그대로인지 나중에 직접 대조한다.
    let expires_before_22 = {
        let store =
            gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_22)
                .map_err(|e| format!("lease store 재조회 열기 실패(22, 사전): {e}"))?;
        store
            .get(fixture.lease_id)
            .map_err(|e| format!("lease store 재조회 실패(22, 사전): {e}"))?
            .ok_or_else(|| "lease store 에 22번 시나리오 lease 가 없다(사전)".to_string())?
            .expires_at_unix_ms
    };

    // 실제로 한도(2초)를 넘긴다 — 이 sleep 이 짧은 한도를 CLI 로 준
    // 것을 실측 가능하게 만든다(설계 p111 "두 프로세스 + sleep" 권장안).
    std::thread::sleep(std::time::Duration::from_millis(2_200));

    let renew_22 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_22,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "2",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
        ],
        &["--do-renew", "true"],
    )?;
    if renew_22.coordinator_pid == issue_22.coordinator_pid {
        return Err(format!(
            "누적 시간 초과 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 프로세스가 \
             아니다.\n1차 coordinator_pid={} 2차 coordinator_pid={}",
            issue_22.coordinator_pid, renew_22.coordinator_pid
        ));
    }
    if !renew_22.coordinator_success
        || renew_22.agent_success
        || !renew_22
            .agent_stderr
            .contains("RENEW_REFUSED:MAX_DURATION_EXCEEDED")
    {
        return Err(format!(
            "누적 시간(2초)을 넘겼는데 서명된 MAX_DURATION_EXCEEDED 로 거부되지 않았다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_22.coordinator_pid,
            renew_22.coordinator_pid,
            renew_22.coordinator_success,
            renew_22.coordinator_stdout,
            renew_22.coordinator_stderr,
            renew_22.agent_success,
            renew_22.agent_stdout,
            renew_22.agent_stderr
        ));
    }

    // 저장소의 만료시각이 연장되지 않았는지 직접 재조회로 확인한다 —
    // 초과 판정에서 UPDATE 를 아예 실행하지 않아야 다음 판정 시각도
    // 밀리지 않는다(연장해버리면 정책이 스스로 무력화된다).
    let expires_after_22 = {
        let store =
            gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_22)
                .map_err(|e| format!("lease store 재조회 열기 실패(22, 사후): {e}"))?;
        store
            .get(fixture.lease_id)
            .map_err(|e| format!("lease store 재조회 실패(22, 사후): {e}"))?
            .ok_or_else(|| "lease store 에 22번 시나리오 lease 가 없다(사후)".to_string())?
            .expires_at_unix_ms
    };
    if expires_after_22 != expires_before_22 {
        return Err(format!(
            "MAX_DURATION_EXCEEDED 로 거부했는데도 저장소의 expires_at_unix_ms 가 연장됐다 \
             — 초과 시 UPDATE 를 실행하지 않아야 한다.\n이전={expires_before_22} 이후={expires_after_22}"
        ));
    }
    report.push_str(
        "22) max_total_duration_seconds 초과 — 서명된 MAX_DURATION_EXCEEDED 로 갱신 거부 확인 \
         (저장소 만료시각 불변)\n",
    );

    // ── 23. 대조군 — 한도 안에서는 여전히 RENEWED (오탐 없음) ──────────
    let lease_dir_23 =
        tempfile::tempdir().map_err(|e| format!("lease store 임시 디렉터리 생성 실패(23): {e}"))?;
    let lease_db_path_23 = lease_dir_23.path().join("leases.sqlite3");
    let lease_db_23 = lease_db_path_23
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;

    let issue_23 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_23,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "3600",
            "--do-renew",
            "false",
        ],
        &["--do-renew", "false"],
    )?;
    if !issue_23.coordinator_success || !issue_23.agent_success {
        return Err(format!(
            "대조군 시나리오 1차(최초 발급) 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_23.coordinator_success,
            issue_23.coordinator_stdout,
            issue_23.coordinator_stderr,
            issue_23.agent_success,
            issue_23.agent_stdout,
            issue_23.agent_stderr
        ));
    }

    let renew_23 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_23,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "3600",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
        ],
        &["--do-renew", "true"],
    )?;
    if renew_23.coordinator_pid == issue_23.coordinator_pid {
        return Err(format!(
            "대조군 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 프로세스가 아니다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}",
            issue_23.coordinator_pid, renew_23.coordinator_pid
        ));
    }
    if !renew_23.coordinator_success || !renew_23.agent_success {
        return Err(format!(
            "한도(3600초) 안인데 갱신이 실패했다 — 오탐(false positive)이다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_23.coordinator_pid,
            renew_23.coordinator_pid,
            renew_23.coordinator_success,
            renew_23.coordinator_stdout,
            renew_23.coordinator_stderr,
            renew_23.agent_success,
            renew_23.agent_stdout,
            renew_23.agent_stderr
        ));
    }
    report.push_str(
        "23) max_total_duration_seconds 대조군 — 한도 안에서는 여전히 RENEWED 확인 (오탐 없음)\n",
    );

    // ── 24. lease_store=Some 에서 override — 저장소 불변 확인 ──────────
    //
    // 코덱스 독립 검수(2026-08-19, p114)가 찾은 결함의 회귀 방지 —
    // 처음 구현은 `lease_store=Some` 이고 `--renew-outcome-override`
    // 도 있을 때, 초과 여부와 무관하게 먼저 저장소를 갱신(만료시각
    // 연장)한 **뒤에** override 를 적용했다 — "거부 응답인데 저장소는
    // 갱신됨" 이라는 상태 불일치였다. override 는 저장소를 **전혀**
    // 건드리지 않아야 한다(레거시 `None` 경로와 같은 계약).
    let lease_dir_24 =
        tempfile::tempdir().map_err(|e| format!("lease store 임시 디렉터리 생성 실패(24): {e}"))?;
    let lease_db_path_24 = lease_dir_24.path().join("leases.sqlite3");
    let lease_db_24 = lease_db_path_24
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;

    let issue_24 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_24,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "3600",
            "--do-renew",
            "false",
        ],
        &["--do-renew", "false"],
    )?;
    if !issue_24.coordinator_success || !issue_24.agent_success {
        return Err(format!(
            "override·저장소 불변 시나리오 1차(최초 발급) 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_24.coordinator_success,
            issue_24.coordinator_stdout,
            issue_24.coordinator_stderr,
            issue_24.agent_success,
            issue_24.agent_stdout,
            issue_24.agent_stderr
        ));
    }

    let expires_before_24 = {
        let store =
            gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_24)
                .map_err(|e| format!("lease store 재조회 열기 실패(24, 사전): {e}"))?;
        store
            .get(fixture.lease_id)
            .map_err(|e| format!("lease store 재조회 실패(24, 사전): {e}"))?
            .ok_or_else(|| "lease store 에 24번 시나리오 lease 가 없다(사전)".to_string())?
            .expires_at_unix_ms
    };

    // 2차(별도 프로세스): 한도(3600초) 안인데 override=SUPERSEDED(2) 를
    // 강제 주입한다. 초과가 아니므로 override 값 그대로 나가야 하고,
    // 저장소는 전혀 갱신되지 않아야 한다.
    let override_24 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_24,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "3600",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--renew-outcome-override",
            "2",
        ],
        &["--do-renew", "true"],
    )?;
    if override_24.coordinator_pid == issue_24.coordinator_pid {
        return Err(format!(
            "override·저장소 불변 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 \
             프로세스가 아니다.\n1차 coordinator_pid={} 2차 coordinator_pid={}",
            issue_24.coordinator_pid, override_24.coordinator_pid
        ));
    }
    if override_24.agent_success
        || !override_24
            .agent_stderr
            .contains("RENEW_REFUSED:SUPERSEDED")
    {
        return Err(format!(
            "lease_store=Some 에서 override(SUPERSEDED) 가 서명된 정상 정책 거부로 \
             분류되지 않았다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            override_24.coordinator_success,
            override_24.coordinator_stdout,
            override_24.coordinator_stderr,
            override_24.agent_success,
            override_24.agent_stdout,
            override_24.agent_stderr
        ));
    }

    let expires_after_24 = {
        let store =
            gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_24)
                .map_err(|e| format!("lease store 재조회 열기 실패(24, 사후): {e}"))?;
        store
            .get(fixture.lease_id)
            .map_err(|e| format!("lease store 재조회 실패(24, 사후): {e}"))?
            .ok_or_else(|| "lease store 에 24번 시나리오 lease 가 없다(사후)".to_string())?
            .expires_at_unix_ms
    };
    if expires_after_24 != expires_before_24 {
        return Err(format!(
            "override(SUPERSEDED) 로 거부했는데도 저장소의 expires_at_unix_ms 가 갱신됐다 \
             — override 는 저장소를 전혀 건드리지 않아야 한다.\n\
             이전={expires_before_24} 이후={expires_after_24}"
        ));
    }
    report.push_str(
        "24) lease_store=Some + override — 서명된 정책 거부는 그대로이고 저장소는 \
         전혀 갱신되지 않음을 확인 (코덱스 p114 지적의 회귀 방지)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // Coordinator의 실제 SUPERSEDED 정책 (2026-08-19)
    // ══════════════════════════════════════════════════════════════

    // ── 25. 저장된 epoch보다 낮은 요청 — signed SUPERSEDED, 연결 유지 ──
    // 최초 Grant는 저장된 epoch=5로 기록하고, 별도 Coordinator 프로세스에서
    // 같은 DB를 다시 연다. Agent가 Grant를 먼저 같은 durable watermark에
    // 기록한 뒤 요청 epoch=4를 보내므로, Coordinator가 raw error로 연결을
    // 끊었다면 coordinator 성공/RENEW_RESULT가 남을 수 없다.
    let superseded_dir_25 = tempfile::tempdir()
        .map_err(|e| format!("SUPERSEDED 시나리오 임시 디렉터리 생성 실패(25): {e}"))?;
    let lease_db_25 = superseded_dir_25.path().join("lease.sqlite3");
    let fence_db_25 = superseded_dir_25.path().join("fence.sqlite3");
    let lease_db_25 = lease_db_25
        .to_str()
        .ok_or_else(|| "SUPERSEDED lease store 경로가 UTF-8이 아니다(25)".to_string())?;
    let fence_db_25 = fence_db_25
        .to_str()
        .ok_or_else(|| "SUPERSEDED fence watermark 경로가 UTF-8이 아니다(25)".to_string())?;

    let issue_25 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_25,
            "--fence-epoch",
            "5",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_25, "--do-renew", "false"],
    )?;
    if !issue_25.coordinator_success || !issue_25.agent_success {
        return Err(format!(
            "SUPERSEDED 시나리오 1차 발급이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_25.coordinator_success,
            issue_25.coordinator_stdout,
            issue_25.coordinator_stderr,
            issue_25.agent_success,
            issue_25.agent_stdout,
            issue_25.agent_stderr
        ));
    }

    let superseded_25 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_25,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
        ],
        &[
            "--fence-db",
            fence_db_25,
            "--do-renew",
            "true",
            "--renew-request-epoch-override",
            "4",
        ],
    )?;
    if !superseded_25.coordinator_success
        || !superseded_25
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=2")
        || !superseded_25.coordinator_stdout.contains(RESULT_OK_MARKER)
        || superseded_25.agent_success
        || !superseded_25
            .agent_stderr
            .contains("RENEW_REFUSED:SUPERSEDED")
        || superseded_25
            .agent_stderr
            .contains("RenewLeaseResult 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "영속 저장소의 낮은 epoch 요청이 signed SUPERSEDED로 정상 응답되지 않았다 — raw 연결 종료가 아니어야 한다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            superseded_25.coordinator_success,
            superseded_25.coordinator_stdout,
            superseded_25.coordinator_stderr,
            superseded_25.agent_success,
            superseded_25.agent_stdout,
            superseded_25.agent_stderr
        ));
    }
    report.push_str(
        "25) 영속 stored epoch(5)보다 낮은 renew 요청(4)을 연결 종료 없이 signed SUPERSEDED로 응답하고 Agent가 정상 정책 거부로 분류함\n",
    );

    // ── 26. 저장된 epoch와 같은 요청 — 기존 signed RENEWED 대조군 ──
    let same_epoch_dir_26 = tempfile::tempdir()
        .map_err(|e| format!("same epoch 시나리오 임시 디렉터리 생성 실패(26): {e}"))?;
    let lease_db_26 = same_epoch_dir_26.path().join("lease.sqlite3");
    let fence_db_26 = same_epoch_dir_26.path().join("fence.sqlite3");
    let lease_db_26 = lease_db_26
        .to_str()
        .ok_or_else(|| "same epoch lease store 경로가 UTF-8이 아니다(26)".to_string())?;
    let fence_db_26 = fence_db_26
        .to_str()
        .ok_or_else(|| "same epoch fence watermark 경로가 UTF-8이 아니다(26)".to_string())?;

    let issue_26 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_26,
            "--fence-epoch",
            "7",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_26, "--do-renew", "false"],
    )?;
    if !issue_26.coordinator_success || !issue_26.agent_success {
        return Err(format!(
            "same epoch 시나리오 1차 발급이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_26.coordinator_success,
            issue_26.coordinator_stdout,
            issue_26.coordinator_stderr,
            issue_26.agent_success,
            issue_26.agent_stdout,
            issue_26.agent_stderr
        ));
    }
    let same_epoch_26 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_26,
            "--fence-epoch",
            "0",
            "--do-renew",
            "true",
        ],
        &["--fence-db", fence_db_26, "--do-renew", "true"],
    )?;
    if !same_epoch_26.coordinator_success
        || !same_epoch_26
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=1")
        || !same_epoch_26.agent_success
        || !same_epoch_26
            .agent_stdout
            .contains("RENEW_RESULT ok=true outcome=RENEWED")
    {
        return Err(format!(
            "저장된 epoch와 같은 epoch의 기본 renew가 RENEWED가 아니었다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            same_epoch_26.coordinator_success,
            same_epoch_26.coordinator_stdout,
            same_epoch_26.coordinator_stderr,
            same_epoch_26.agent_success,
            same_epoch_26.agent_stdout,
            same_epoch_26.agent_stderr
        ));
    }
    report.push_str("26) 저장된 epoch와 같은 epoch의 기본 renew가 signed RENEWED임을 확인\n");

    // ══════════════════════════════════════════════════════════════
    // Lease revoke 최소 조각 (2026-08-19)
    // ══════════════════════════════════════════════════════════════

    // ── 27. ACK 직후 revoke + 양쪽 renew 활성화의 교착 방지 ─────────
    // Scenarios 27-31 intentionally omit `--lease-db`; run_handshake() adds
    // the legacy-mode opt-in above. This deliberately tests the
    // storage-independent, signature-based RevokeLeaseNotice contract itself.
    // 27: intentionally uses that no-`--lease-db` contract lane.
    let revoke_ok = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--revoke-after-round",
            "0",
            "--renew-rounds",
            "2",
        ],
        &[
            "--do-renew",
            "true",
            "--renew-rounds",
            "2",
            "--expect-revoke-after-round",
            "0",
        ],
    )?;
    if !revoke_ok.coordinator_success
        || !revoke_ok.agent_success
        || !revoke_ok.agent_stdout.contains("REVOKE_RESULT ok=true")
        || !revoke_ok
            .agent_stdout
            .contains("RENEW_BLOCKED: lease revoked")
        || revoke_ok
            .agent_stdout
            .matches("RENEW_RESULT ok=true")
            .count()
            != 0
        || revoke_ok
            .coordinator_stdout
            .matches("RENEW_RESULT ok=true")
            .count()
            != 0
    {
        return Err(format!(
            "ACK 직후 revoke 교착 방지 또는 revoke 뒤 renew 차단이 실패했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            revoke_ok.coordinator_success,
            revoke_ok.coordinator_stdout,
            revoke_ok.coordinator_stderr,
            revoke_ok.agent_success,
            revoke_ok.agent_stdout,
            revoke_ok.agent_stderr
        ));
    }
    report.push_str(
        "27) ACK 직후 revoke + 양쪽 renew=true 정상 종료 및 Agent의 revoke 후 renew 미생성 경로 확인 \
         (Coordinator/Agent stub이 즉시 종료하므로 wire상 추가 프레임 부재 자체는 이 selftest가 직접 증명하지 않음)\n",
    );

    // 공통으로 ACK 직후 revoke를 받는 단일 왕복 negative 시나리오다.
    // V-08 때문에 잘못된 target id도 selftest 전용 키 alias를 등록해
    // 서명 검증을 통과시킨 뒤 Agent identity 게이트를 실제로 밟게 한다.
    // 28: intentionally uses that no-`--lease-db` contract lane.
    let forged_revoke = run_handshake(
        &fixture,
        &[
            "--revoke-after-round",
            "0",
            "--corrupt-revoke-signature",
            "true",
        ],
        &["--do-renew", "false", "--expect-revoke-after-round", "0"],
    )?;
    if forged_revoke.agent_success
        || !forged_revoke
            .agent_stderr
            .contains("RevokeLeaseNotice 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "위조된 revoke 서명이 거부되지 않았다.\nagent exit={} stdout={} stderr={}",
            forged_revoke.agent_success, forged_revoke.agent_stdout, forged_revoke.agent_stderr
        ));
    }
    report.push_str("28) 위조 RevokeLeaseNotice 서명 거부 확인\n");

    // 29: intentionally uses that no-`--lease-db` contract lane.
    let wrong_revoke_lease_id = "01JWRONGREVOKELEASE00000001";
    let wrong_id_revoke = run_handshake(
        &fixture,
        &[
            "--revoke-after-round",
            "0",
            "--revoke-lease-id",
            wrong_revoke_lease_id,
        ],
        &[
            "--do-renew",
            "false",
            "--expect-revoke-after-round",
            "0",
            "--revoke-signer-id",
            wrong_revoke_lease_id,
        ],
    )?;
    if wrong_id_revoke.agent_success
        || !wrong_id_revoke
            .agent_stderr
            .contains("REVOKE_REJECTED: lease_id 불일치")
    {
        return Err(format!(
            "잘못된 revoke lease_id가 거부되지 않았다.\nagent exit={} stdout={} stderr={}",
            wrong_id_revoke.agent_success,
            wrong_id_revoke.agent_stdout,
            wrong_id_revoke.agent_stderr
        ));
    }
    report.push_str("29) 잘못된 revoke lease_id 거부 확인\n");

    // 30: intentionally uses that no-`--lease-db` contract lane.
    let wrong_epoch_revoke = run_handshake(
        &fixture,
        &["--revoke-after-round", "0", "--revoke-fence-epoch", "6"],
        &["--do-renew", "false", "--expect-revoke-after-round", "0"],
    )?;
    if wrong_epoch_revoke.agent_success
        || !wrong_epoch_revoke
            .agent_stderr
            .contains("REVOKE_REJECTED: fence_epoch 불일치")
    {
        return Err(format!(
            "잘못된 revoke fence_epoch이 거부되지 않았다.\nagent exit={} stdout={} stderr={}",
            wrong_epoch_revoke.agent_success,
            wrong_epoch_revoke.agent_stdout,
            wrong_epoch_revoke.agent_stderr
        ));
    }
    report.push_str("30) 잘못된 revoke fence_epoch 거부 확인\n");

    // Grant 시점에는 아직 유효하지만, Coordinator가 실제로 2.2초
    // 기다린 뒤 revoke를 보내므로 Agent의 보유 Lease는 이미 만료된다.
    // 31: intentionally uses that no-`--lease-db` contract lane.
    let expired_revoke = run_handshake(
        &fixture,
        &[
            "--revoke-after-round",
            "0",
            "--lease-ttl-ms",
            "2000",
            "--revoke-delay-ms",
            "2200",
        ],
        &["--do-renew", "false", "--expect-revoke-after-round", "0"],
    )?;
    if expired_revoke.agent_success
        || !expired_revoke
            .agent_stderr
            .contains("REVOKE_REJECTED: held Lease가 이미 만료됐다")
    {
        return Err(format!(
            "이미 만료된 Lease에 대한 revoke가 거부되지 않았다.\nagent exit={} stdout={} stderr={}",
            expired_revoke.agent_success, expired_revoke.agent_stdout, expired_revoke.agent_stderr
        ));
    }
    report.push_str("31) 이미 만료된 Lease에 대한 revoke 거부 확인\n");

    // ── 32. SUPERSEDED가 다회차 갱신의 첫 회차에 발생하면 즉시 종료 ──
    // Agent의 정책 거부는 함수 자체를 끝내므로 Coordinator도 다음
    // RenewLeaseRequest를 기다리지 않고 같은 회차에서 루프를 끝내야 한다.
    // 이 시나리오는 renew_rounds=2로 첫 회차에서만 SUPERSEDED를 유도하고,
    // 두 번째 회차의 RENEW_RESULT가 없으며 양쪽 프로세스가 정상적으로
    // 수렴하는지를 확인한다.
    let superseded_multi_round = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_25,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
            "--renew-rounds",
            "2",
        ],
        &[
            "--fence-db",
            fence_db_25,
            "--do-renew",
            "true",
            "--renew-request-epoch-override",
            "4",
            "--renew-rounds",
            "2",
        ],
    )?;
    if !superseded_multi_round.coordinator_success
        || !superseded_multi_round
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=2")
        || superseded_multi_round
            .coordinator_stdout
            .matches("RENEW_RESULT ok=true")
            .count()
            != 1
        || !superseded_multi_round
            .coordinator_stdout
            .contains(RESULT_OK_MARKER)
        || superseded_multi_round.agent_success
        || !superseded_multi_round
            .agent_stderr
            .contains("RENEW_REFUSED:SUPERSEDED")
        || superseded_multi_round
            .agent_stdout
            .contains("RENEW_RESULT ok=true")
    {
        return Err(format!(
            "다회차 SUPERSEDED가 첫 회차에서 정상 종료되지 않았거나 이후 RENEW_RESULT가 발생했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            superseded_multi_round.coordinator_success,
            superseded_multi_round.coordinator_stdout,
            superseded_multi_round.coordinator_stderr,
            superseded_multi_round.agent_success,
            superseded_multi_round.agent_stdout,
            superseded_multi_round.agent_stderr
        ));
    }
    report.push_str(
        "32) renew_rounds=2의 첫 회차 SUPERSEDED 후 양쪽이 교착 없이 종료되고 이후 RENEW_RESULT가 없음을 확인\n",
    );

    // ══════════════════════════════════════════════════════════════
    // Active Lease process-restart rehydration (2026-08-19)
    //
    // 기존 Grant/ACK handshake를 그대로 재사용한다. 첫 번째 프로세스
    // 쌍은 ACK를 실제로 받은 뒤 Coordinator가 의도적으로 연결을 닫고,
    // 두 번째 새 프로세스 쌍은 같은 lease/fence DB를 열어 저장된 Lease를
    // 복원한다. 새 proto 메시지나 자동 재접속은 이 조각에 없다.
    // ══════════════════════════════════════════════════════════════

    // ── 33. ACK 뒤 단절 후 새 프로세스 쌍의 active Lease 복원 ───────
    let reconnect_dir_33 = tempfile::tempdir()
        .map_err(|e| format!("재접속 시나리오 임시 디렉터리 생성 실패(33): {e}"))?;
    let lease_db_path_33 = reconnect_dir_33.path().join("lease.sqlite3");
    let fence_db_path_33 = reconnect_dir_33.path().join("fence.sqlite3");
    let lease_db_33 = lease_db_path_33
        .to_str()
        .ok_or_else(|| "재접속 lease store 경로가 UTF-8이 아니다(33)".to_string())?;
    let fence_db_33 = fence_db_path_33
        .to_str()
        .ok_or_else(|| "재접속 fence watermark 경로가 UTF-8이 아니다(33)".to_string())?;

    let disconnected_33 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_33,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--disconnect-after-ack",
            "true",
        ],
        &["--fence-db", fence_db_33, "--do-renew", "true"],
    )?;
    if !disconnected_33.coordinator_success
        || !disconnected_33
            .coordinator_stdout
            .contains("DISCONNECT_AFTER_ACK coordinator_acknowledged=true")
        || disconnected_33.agent_success
        || disconnected_33.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "ACK 뒤 의도적 연결 단절이 정상적으로 관측되지 않았다(33) — Coordinator는 ACK 후 종료하고 Agent는 기존 오류 전파로 종료해야 한다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            disconnected_33.coordinator_success,
            disconnected_33.coordinator_stdout,
            disconnected_33.coordinator_stderr,
            disconnected_33.agent_success,
            disconnected_33.agent_stdout,
            disconnected_33.agent_stderr
        ));
    }

    let rehydrated_33 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_33,
            "--fence-epoch",
            "3",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_33, "--do-renew", "false"],
    )?;
    if rehydrated_33.coordinator_pid == disconnected_33.coordinator_pid
        || rehydrated_33.agent_pid == disconnected_33.agent_pid
        || !rehydrated_33.coordinator_success
        || !rehydrated_33.agent_success
        || !rehydrated_33.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "연결 단절 뒤 새 Coordinator/Agent 프로세스 쌍이 저장된 Lease를 복원하지 못했다(33) — \
             새 PID, 같은 DB, CLI fence_epoch=3이어도 저장된 epoch=5 Grant/ACK가 성공해야 한다.\n\
             1차 coordinator_pid={} agent_pid={}\n\
             2차 coordinator_pid={} agent_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            disconnected_33.coordinator_pid,
            disconnected_33.agent_pid,
            rehydrated_33.coordinator_pid,
            rehydrated_33.agent_pid,
            rehydrated_33.coordinator_success,
            rehydrated_33.coordinator_stdout,
            rehydrated_33.coordinator_stderr,
            rehydrated_33.agent_success,
            rehydrated_33.agent_stdout,
            rehydrated_33.agent_stderr
        ));
    }
    report.push_str(
        "33) ACK 직후 의도적 연결 단절 후 새 프로세스 쌍이 같은 lease/fence DB에서 active Lease를 복원하고, 잘못된 CLI fence_epoch=3 대신 저장된 epoch=5로 Grant/ACK 성공\n",
    );

    // ── 34. 같은 lease_id를 다른 holder identity로 주장 ─────────────
    // `get_or_issue()`의 기존 holder_node_id IdentityConflict가 실제
    // Coordinator 발급 호출부에서 동작하는지 확인한다. 새 Agent도 같은
    // 잘못된 identity를 사용해 Grant의 holder 검증에서 먼저 가려지지
    // 않도록 한다 — 거부 주체가 Coordinator임을 확인하는 시나리오다.
    let wrong_holder_id = "01JOTHERAGENTSELFTEST00000001";
    let wrong_holder_34 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_33,
            "--fence-epoch",
            "5",
            "--agent-device-id",
            wrong_holder_id,
            "--do-renew",
            "false",
        ],
        &[
            "--fence-db",
            fence_db_33,
            "--agent-device-id",
            wrong_holder_id,
            "--do-renew",
            "false",
        ],
    )?;
    if wrong_holder_34.coordinator_success
        || !wrong_holder_34
            .coordinator_stderr
            .contains("holder_node_id")
        || !wrong_holder_34
            .coordinator_stderr
            .contains("lease store 최초 발급 실패")
        || wrong_holder_34.agent_success
    {
        return Err(format!(
            "같은 lease_id의 다른 holder_node_id가 Coordinator에서 거부되지 않았다(34).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            wrong_holder_34.coordinator_success,
            wrong_holder_34.coordinator_stdout,
            wrong_holder_34.coordinator_stderr,
            wrong_holder_34.agent_success,
            wrong_holder_34.agent_stdout,
            wrong_holder_34.agent_stderr
        ));
    }
    report.push_str(
        "34) 같은 lease_id를 다른 holder_node_id로 재접속 주장 시 Coordinator의 기존 IdentityConflict 거부 확인\n",
    );

    // ── 35. revoke 상태의 Coordinator 영속화와 재시작 거부 ─────────────
    let revoke_persistence_dir = tempfile::tempdir()
        .map_err(|e| format!("revoke 영속화 시나리오 임시 디렉터리 생성 실패(35): {e}"))?;
    let lease_db_35 = revoke_persistence_dir.path().join("lease.sqlite3");
    let fence_db_35 = revoke_persistence_dir.path().join("fence.sqlite3");
    let lease_db_35 = lease_db_35
        .to_str()
        .ok_or_else(|| "revoke 영속화 lease store 경로가 UTF-8이 아니다(35)".to_string())?;
    let fence_db_35 = fence_db_35
        .to_str()
        .ok_or_else(|| "revoke 영속화 fence watermark 경로가 UTF-8이 아니다(35)".to_string())?;

    let revoked_first_35 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_35,
            "--fence-epoch",
            "5",
            "--revoke-after-round",
            "0",
            "--do-renew",
            "false",
        ],
        &[
            "--fence-db",
            fence_db_35,
            "--do-renew",
            "false",
            "--expect-revoke-after-round",
            "0",
        ],
    )?;
    if !revoked_first_35.coordinator_success
        || !revoked_first_35.agent_success
        || !revoked_first_35
            .agent_stdout
            .contains("REVOKE_RESULT ok=true")
    {
        return Err(format!(
            "revoke 영속화 시나리오 1차 프로세스 쌍이 정상 종료하지 않았다(35).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            revoked_first_35.coordinator_success,
            revoked_first_35.coordinator_stdout,
            revoked_first_35.coordinator_stderr,
            revoked_first_35.agent_success,
            revoked_first_35.agent_stdout,
            revoked_first_35.agent_stderr
        ));
    }

    let stored_revoked_35 =
        gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(lease_db_35)
            .map_err(|e| format!("revoke 영속화 lease store 재조회 열기 실패(35): {e}"))?
            .get(fixture.lease_id)
            .map_err(|e| format!("revoke 영속화 lease store 재조회 실패(35): {e}"))?
            .ok_or_else(|| "revoke 영속화 lease store에 Lease가 없다(35)".to_string())?;
    if stored_revoked_35.revoked_at_unix_ms.is_none() {
        return Err(
            "revoke 통지 성공 뒤 lease store의 revoked_at_unix_ms가 NULL이다(35)".to_string(),
        );
    }

    let revoked_second_35 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_35,
            "--fence-epoch",
            "5",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_35, "--do-renew", "false"],
    )?;
    if revoked_second_35.coordinator_success
        || !revoked_second_35
            .coordinator_stderr
            .contains("lease store 최초 발급 실패")
        || !revoked_second_35.coordinator_stderr.contains("revoked")
        || revoked_second_35.agent_success
        || !revoked_second_35
            .agent_stderr
            .contains("Grant 프레임 읽기/검증 실패")
        || revoked_second_35
            .coordinator_stdout
            .contains(RESULT_OK_MARKER)
        || revoked_second_35.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "revoke 상태가 새 프로세스 쌍에서 거부되지 않았다(35).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            revoked_second_35.coordinator_success,
            revoked_second_35.coordinator_stdout,
            revoked_second_35.coordinator_stderr,
            revoked_second_35.agent_success,
            revoked_second_35.agent_stdout,
            revoked_second_35.agent_stderr
        ));
    }
    report.push_str(
        "35) revoke 통지 후 revoked_at_unix_ms 영속화 및 새 Coordinator의 revoked Lease 최초 발급 거부 확인\n",
    );

    // ── 36. 만료된 Lease의 재접속 복원 거부 ───────────────────────
    // 첫 번째 프로세스 쌍은 짧은 TTL 안에 정상 ACK를 끝내고 종료한다.
    // 실제 시간이 TTL을 지난 뒤 완전히 새 프로세스 쌍이 같은 저장소로
    // 재접속하면 CoordinatorLeaseStore가 저장된 expires_at을 확인해
    // 새 Grant를 만들지 않고 Expired raw error로 handshake를 끝내야 한다.
    let expired_reconnect_dir_36 = tempfile::tempdir()
        .map_err(|e| format!("만료 Lease 재접속 시나리오 임시 디렉터리 생성 실패(36): {e}"))?;
    let lease_db_path_36 = expired_reconnect_dir_36.path().join("lease.sqlite3");
    let fence_db_path_36 = expired_reconnect_dir_36.path().join("fence.sqlite3");
    let lease_db_36 = lease_db_path_36
        .to_str()
        .ok_or_else(|| "만료 Lease 재접속 lease store 경로가 UTF-8이 아니다(36)".to_string())?;
    let fence_db_36 = fence_db_path_36
        .to_str()
        .ok_or_else(|| "만료 Lease 재접속 fence watermark 경로가 UTF-8이 아니다(36)".to_string())?;

    let issued_short_36 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_36,
            "--fence-epoch",
            "5",
            "--lease-ttl-ms",
            "250",
            "--disconnect-after-ack",
            "true",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_36, "--do-renew", "false"],
    )?;
    if !issued_short_36.coordinator_success
        || !issued_short_36
            .coordinator_stdout
            .contains("DISCONNECT_AFTER_ACK coordinator_acknowledged=true")
        || !issued_short_36.agent_success
        || !issued_short_36.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "짧은 TTL Lease의 최초 발급/ACK가 정상 종료되지 않았다(36).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issued_short_36.coordinator_success,
            issued_short_36.coordinator_stdout,
            issued_short_36.coordinator_stderr,
            issued_short_36.agent_success,
            issued_short_36.agent_stdout,
            issued_short_36.agent_stderr
        ));
    }

    // 실제 벽시계가 저장된 250ms 만료시각을 지나도록 기다린다.
    thread::sleep(std::time::Duration::from_millis(400));

    let expired_reconnect_36 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_36,
            "--fence-epoch",
            "3",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_36, "--do-renew", "false"],
    )?;
    if expired_reconnect_36.coordinator_success
        || !expired_reconnect_36
            .coordinator_stderr
            .contains("lease store 최초 발급 실패")
        || !expired_reconnect_36.coordinator_stderr.contains("expired")
        || expired_reconnect_36.agent_success
        || expired_reconnect_36.agent_stdout.contains(RESULT_OK_MARKER)
        || expired_reconnect_36
            .coordinator_stdout
            .contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "만료된 Lease 재접속이 Coordinator에서 Expired로 거부되지 않았다(36).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            expired_reconnect_36.coordinator_success,
            expired_reconnect_36.coordinator_stdout,
            expired_reconnect_36.coordinator_stderr,
            expired_reconnect_36.agent_success,
            expired_reconnect_36.agent_stdout,
            expired_reconnect_36.agent_stderr
        ));
    }
    report.push_str(
        "36) 짧은 TTL Lease를 실제로 만료시킨 뒤 새 프로세스 쌍의 재접속 복원 거부(Expired) 확인\n",
    );

    // ── 37. revoke된 Lease 갱신의 signed REVOKED outcome ─────────────
    // ACK 뒤 Coordinator 저장소에 revoke를 먼저 확정하고, revoke notice는
    // 보내지 않는다. 따라서 같은 프로세스 쌍의 Agent가 다음 갱신 요청을
    // 실제로 만들고, 연결을 끊지 않은 signed outcome=8을 받아 즉시
    // RENEW_REFUSED:REVOKED로 종료한다. 이 경로는 초기 Grant 발급
    // 거부(DoD-25 시나리오 35)와 의도적으로 분리돼 있다.
    let revoked_renew_dir_37 = tempfile::tempdir()
        .map_err(|e| format!("revoked renew 시나리오 임시 디렉터리 생성 실패(37): {e}"))?;
    let lease_db_path_37 = revoked_renew_dir_37.path().join("lease.sqlite3");
    let fence_db_path_37 = revoked_renew_dir_37.path().join("fence.sqlite3");
    let lease_db_37 = lease_db_path_37
        .to_str()
        .ok_or_else(|| "revoked renew lease store 경로가 UTF-8이 아니다(37)".to_string())?;
    let fence_db_37 = fence_db_path_37
        .to_str()
        .ok_or_else(|| "revoked renew fence watermark 경로가 UTF-8이 아니다(37)".to_string())?;

    let revoked_renew_37 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_37,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renew-rounds",
            "2",
            "--revoke-before-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_37,
            "--do-renew",
            "true",
            "--renew-rounds",
            "2",
        ],
    )?;
    if !revoked_renew_37.coordinator_success
        || !revoked_renew_37
            .coordinator_stdout
            .contains("REVOKE_STORE ok=true")
        || !revoked_renew_37
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=8")
        || revoked_renew_37
            .coordinator_stderr
            .contains("lease store 갱신 거부")
        || revoked_renew_37.agent_success
        || !revoked_renew_37
            .agent_stderr
            .contains("RENEW_REFUSED:REVOKED")
        || revoked_renew_37
            .agent_stderr
            .contains("RenewLeaseResult 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "revoked Lease 갱신이 signed REVOKED outcome으로 종료되지 않았다(37).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            revoked_renew_37.coordinator_success,
            revoked_renew_37.coordinator_stdout,
            revoked_renew_37.coordinator_stderr,
            revoked_renew_37.agent_success,
            revoked_renew_37.agent_stdout,
            revoked_renew_37.agent_stderr
        ));
    }
    report.push_str(
        "37) 같은 연결에서 revoke 후 갱신 요청을 보내 signed RENEW_OUTCOME_REVOKED(8)를 받고 즉시 종료 확인\n",
    );

    // Scenario 38: a Coordinator without a lease DB must require explicit
    // opt-in and must terminate before binding or waiting in accept().
    let legacy_rejected = run_coordinator_without_legacy_opt_in(&fixture)?;
    if legacy_rejected.success
        || !legacy_rejected.stderr.contains("max-duration")
        || legacy_rejected.agent_success
        || legacy_rejected.agent_exit_code.unwrap_or(0) == 0
    {
        return Err(format!(
            "legacy opt-in 없는 Coordinator가 즉시 거부되지 않았거나 Agent가 비정상 종료하지 않았다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={:?} elapsed_ms={} stdout={} stderr={}",
            legacy_rejected.success,
            legacy_rejected.stdout,
            legacy_rejected.stderr,
            legacy_rejected.agent_exit_code,
            legacy_rejected.agent_elapsed_ms,
            legacy_rejected.agent_stdout,
            legacy_rejected.agent_stderr
        ));
    }
    report.push_str(&format!(
        "38) --lease-db 및 legacy opt-in 없는 Coordinator가 bind 전에 즉시 실패하고, Agent도 비리스닝 주소 연결 후 {}ms 안에 exit={}로 비정상 종료함\n",
        legacy_rejected.agent_elapsed_ms,
        legacy_rejected.agent_exit_code.unwrap_or(-1),
    ));

    // ── 39. 정상 Grant 뒤 실제 WRITING marker 생성 ──────────────────
    let checkpoint_root_39 =
        tempfile::tempdir().map_err(|e| format!("정상 시작 marker root 생성 실패(39): {e}"))?;
    let started_39 =
        run_handshake_with_checkpoint_root(&fixture, checkpoint_root_39.path(), &[], &[])?;
    if !started_39.coordinator_success
        || !started_39.agent_success
        || !started_39.agent_stdout.contains("state=WRITING")
    {
        return Err(format!(
            "정상 Grant의 WRITING 시작 경로가 실패했다(39). coordinator={} agent={} stdout={} stderr={}",
            started_39.coordinator_success,
            started_39.agent_success,
            started_39.agent_stdout,
            started_39.agent_stderr
        ));
    }
    let expected_checkpoint_id =
        gputeer_agent::start_checkpoint_id(fixture.job_id, fixture.attempt_id, fixture.grant_id);
    let started_id_39 = started_checkpoint_id(&started_39.agent_stdout)
        .ok_or_else(|| "JOB_STARTED checkpoint_id가 없다(39)".to_string())?;
    if started_id_39 != expected_checkpoint_id {
        return Err(format!(
            "checkpoint_id 생성 규칙이 다르다(39): expected={} actual={}",
            expected_checkpoint_id, started_id_39
        ));
    }
    let entries_39 = checkpoint_entries(checkpoint_root_39.path())?;
    if entries_39.len() != 1
        || entries_39[0].file_name().to_string_lossy() != expected_checkpoint_id
    {
        return Err(format!(
            "정상 Grant 뒤 checkpoint 디렉터리가 정확히 하나 생성되지 않았다(39): entries={:?}",
            entries_39
                .iter()
                .map(|entry| entry.file_name())
                .collect::<Vec<_>>()
        ));
    }
    let marker_39 = checkpoint_root_39
        .path()
        .join(&expected_checkpoint_id)
        .join(".durability.writing");
    if std::fs::read(&marker_39).map_err(|e| format!("WRITING marker 읽기 실패(39): {e}"))?
        != b"Writing\n"
        || marker_39.with_file_name("manifest.json").exists()
    {
        return Err("정상 시작 marker의 내용 또는 범위가 잘못됐다(39)".into());
    }
    report.push_str(
        "39) 정상 Grant 뒤 checkpoint_root/<digest>와 .durability.writing(WRITING) 생성 및 manifest 미생성 확인\n",
    );

    // ── 40. 위조 Lease는 marker/ACK 전에 거부 ────────────────────────
    let checkpoint_root_40 =
        tempfile::tempdir().map_err(|e| format!("위조 Lease marker root 생성 실패(40): {e}"))?;
    let forged_lease_40 = run_handshake_with_checkpoint_root(
        &fixture,
        checkpoint_root_40.path(),
        &["--corrupt-lease-signature", "true"],
        &[],
    )?;
    if forged_lease_40.agent_success || !forged_lease_40.agent_stderr.contains("LEASE_REJECTED:") {
        return Err(format!(
            "위조 Lease가 marker 이전에 거부되지 않았다(40): agent={} stderr={}",
            forged_lease_40.agent_success, forged_lease_40.agent_stderr
        ));
    }
    assert_no_agent_ack_or_marker(
        &forged_lease_40,
        checkpoint_root_40.path(),
        "위조 Lease(40)",
    )?;
    report.push_str("40) 위조 Lease 거부 시 WRITING marker 미생성 및 AgentGrantAck 미전송 확인\n");

    // ── 41. 만료 Lease는 marker/ACK 전에 거부 ────────────────────────
    let checkpoint_root_41 =
        tempfile::tempdir().map_err(|e| format!("만료 Lease marker root 생성 실패(41): {e}"))?;
    let expired_lease_41 = run_handshake_with_checkpoint_root(
        &fixture,
        checkpoint_root_41.path(),
        &["--expire-lease", "true"],
        &[],
    )?;
    if expired_lease_41.agent_success || !expired_lease_41.agent_stderr.contains("LEASE_REJECTED:")
    {
        return Err(format!(
            "만료 Lease가 marker 이전에 거부되지 않았다(41): agent={} stderr={}",
            expired_lease_41.agent_success, expired_lease_41.agent_stderr
        ));
    }
    assert_no_agent_ack_or_marker(
        &expired_lease_41,
        checkpoint_root_41.path(),
        "만료 Lease(41)",
    )?;
    report.push_str("41) 만료 Lease 거부 시 WRITING marker 미생성 및 AgentGrantAck 미전송 확인\n");

    // ── 42. 영속 store에 revoke된 Lease는 재발급되지 않으며 marker/ACK 없음 ──
    let revoked_dir_42 = tempfile::tempdir()
        .map_err(|e| format!("revoked Lease 시나리오 디렉터리 생성 실패(42): {e}"))?;
    let lease_db_42 = revoked_dir_42.path().join("lease.sqlite3");
    let lease_db_42_str = lease_db_42
        .to_str()
        .ok_or_else(|| "revoked Lease lease-db 경로가 UTF-8이 아니다(42)".to_string())?;
    let first_root_42 =
        tempfile::tempdir().map_err(|e| format!("revoke 준비 marker root 생성 실패(42): {e}"))?;
    let first_42 = run_handshake_with_checkpoint_root(
        &fixture,
        first_root_42.path(),
        &["--lease-db", lease_db_42_str, "--revoke-after-round", "0"],
        &["--expect-revoke-after-round", "0"],
    )?;
    if !first_42.coordinator_success || !first_42.agent_success {
        return Err(format!(
            "revoke 상태를 준비하는 첫 handshake가 실패했다(42): coordinator={} stdout={} stderr={} agent={} stdout={} stderr={}",
            first_42.coordinator_success,
            first_42.coordinator_stdout,
            first_42.coordinator_stderr,
            first_42.agent_success,
            first_42.agent_stdout,
            first_42.agent_stderr
        ));
    }
    let checkpoint_root_42 = tempfile::tempdir()
        .map_err(|e| format!("revoked Lease 거부 marker root 생성 실패(42): {e}"))?;
    let revoked_42 = run_handshake_with_checkpoint_root(
        &fixture,
        checkpoint_root_42.path(),
        &["--lease-db", lease_db_42_str],
        &[],
    )?;
    assert_no_agent_ack_or_marker(&revoked_42, checkpoint_root_42.path(), "revoked Lease(42)")?;
    report.push_str("42) 영속 store에서 revoked Lease 재발급 거부 시 WRITING marker 미생성 및 AgentGrantAck 미전송 확인\n");

    // ── 43. 동일 attempt 재시도는 같은 digest 디렉터리에 멱등 기록 ────
    let checkpoint_root_43 = tempfile::tempdir()
        .map_err(|e| format!("동일 attempt retry marker root 생성 실패(43): {e}"))?;
    let retry_first_43 =
        run_handshake_with_checkpoint_root(&fixture, checkpoint_root_43.path(), &[], &[])?;
    let retry_second_43 =
        run_handshake_with_checkpoint_root(&fixture, checkpoint_root_43.path(), &[], &[])?;
    let first_id_43 = started_checkpoint_id(&retry_first_43.agent_stdout)
        .ok_or_else(|| "첫 retry에서 JOB_STARTED가 없다(43)".to_string())?;
    let second_id_43 = started_checkpoint_id(&retry_second_43.agent_stdout)
        .ok_or_else(|| "두 번째 retry에서 JOB_STARTED가 없다(43)".to_string())?;
    let entries_43 = checkpoint_entries(checkpoint_root_43.path())?;
    let marker_43 = checkpoint_root_43
        .path()
        .join(&first_id_43)
        .join(".durability.writing");
    if !retry_first_43.agent_success
        || !retry_second_43.agent_success
        || !retry_first_43.coordinator_success
        || !retry_second_43.coordinator_success
        || first_id_43 != second_id_43
        || entries_43.len() != 1
        || std::fs::read(&marker_43).map_err(|e| format!("retry marker 읽기 실패(43): {e}"))?
            != b"Writing\n"
    {
        return Err(format!(
            "동일 attempt retry가 멱등 처리되지 않았다(43): first_id={} second_id={} entries={} first_agent={} second_agent={}",
            first_id_43,
            second_id_43,
            entries_43.len(),
            retry_first_43.agent_success,
            retry_second_43.agent_success
        ));
    }
    report.push_str("43) 동일 attempt 재시도에서 같은 checkpoint_id와 단일 WRITING marker만 유지되어 write_once 멱등 경로가 확인됨\n");

    // ── 44. marker 생성 실패는 fail-closed ───────────────────────────
    let failed_root_dir_44 = tempfile::tempdir()
        .map_err(|e| format!("fail-closed 시나리오 디렉터리 생성 실패(44): {e}"))?;
    let failed_root_44 = failed_root_dir_44.path().join("not-a-directory");
    std::fs::write(&failed_root_44, b"regular file")
        .map_err(|e| format!("fail-closed용 root 파일 생성 실패(44): {e}"))?;
    let marker_failure_44 =
        run_handshake_with_checkpoint_root(&fixture, &failed_root_44, &[], &[])?;
    if marker_failure_44.agent_success
        || !marker_failure_44
            .agent_stderr
            .contains("시작 checkpoint 디렉터리 생성 실패")
        || marker_failure_44.coordinator_success
        || marker_failure_44
            .coordinator_stdout
            .contains(RESULT_OK_MARKER)
        || marker_failure_44.agent_stdout.contains("JOB_STARTED ")
        || !failed_root_44.is_file()
    {
        return Err(format!(
            "marker 생성 실패가 fail-closed가 아니다(44): coordinator={} agent={} coordinator_stdout={} agent_stdout={} agent_stderr={}",
            marker_failure_44.coordinator_success,
            marker_failure_44.agent_success,
            marker_failure_44.coordinator_stdout,
            marker_failure_44.agent_stdout,
            marker_failure_44.agent_stderr
        ));
    }
    report.push_str("44) checkpoint root가 일반 파일인 디스크 오류에서 marker/AgentGrantAck/JOB_STARTED 없이 fail-closed 확인\n");

    // ── 45. QUARANTINED가 다회차 갱신의 첫 회차에 발생하면 즉시 종료 ──
    // 기존 단일 회차의 --renew-outcome-override 3 트리거를 그대로
    // 재사용한다. Agent는 signed QUARANTINED를 받은 즉시 갱신 함수를
    // 끝내므로 Coordinator도 두 번째 RenewLeaseRequest를 기다리지
    // 않아야 한다.
    let quarantine_multi_dir_45 = tempfile::tempdir()
        .map_err(|e| format!("QUARANTINED 다회차 시나리오 임시 디렉터리 생성 실패(45): {e}"))?;
    let quarantine_lease_db_45 = quarantine_multi_dir_45.path().join("lease.sqlite3");
    let quarantine_lease_db_45 = quarantine_lease_db_45
        .to_str()
        .ok_or_else(|| "QUARANTINED 다회차 lease store 경로가 UTF-8이 아니다(45)".to_string())?;
    let quarantined_multi_round = run_handshake(
        &fixture,
        &[
            "--lease-db",
            quarantine_lease_db_45,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
            "--renew-outcome-override",
            "3",
            "--renew-rounds",
            "2",
        ],
        &["--do-renew", "true", "--renew-rounds", "2"],
    )?;
    if !quarantined_multi_round.coordinator_success
        || !quarantined_multi_round
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=3")
        || quarantined_multi_round
            .coordinator_stdout
            .matches("RENEW_RESULT ok=true")
            .count()
            != 1
        || !quarantined_multi_round
            .coordinator_stdout
            .contains(RESULT_OK_MARKER)
        || quarantined_multi_round.agent_success
        || !quarantined_multi_round
            .agent_stderr
            .contains("RENEW_REFUSED:QUARANTINED")
        || quarantined_multi_round
            .agent_stdout
            .contains("RENEW_RESULT ok=true")
    {
        return Err(format!(
            "다회차 QUARANTINED가 첫 회차에서 정상 종료되지 않았거나 이후 RENEW_RESULT가 발생했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            quarantined_multi_round.coordinator_success,
            quarantined_multi_round.coordinator_stdout,
            quarantined_multi_round.coordinator_stderr,
            quarantined_multi_round.agent_success,
            quarantined_multi_round.agent_stdout,
            quarantined_multi_round.agent_stderr
        ));
    }
    report.push_str(
        "45) renew_rounds=2의 첫 회차 QUARANTINED(--renew-outcome-override 3) 후 양쪽이 교착 없이 종료되고 이후 RENEW_RESULT가 없음을 확인\n",
    );

    // ── 46. MAX_DURATION_EXCEEDED가 다회차 갱신의 첫 회차에 발생하면 즉시 종료 ──
    // 기존 22번의 --max-total-duration-seconds 2 + 실제 2.2초 경과
    // 트리거를 그대로 재사용한다. 먼저 Lease를 발급하고 한도를 넘긴
    // 뒤, renew_rounds=2 갱신을 시작해 첫 응답이 outcome=6이 되게 한다.
    let max_duration_multi_dir_46 = tempfile::tempdir().map_err(|e| {
        format!("MAX_DURATION_EXCEEDED 다회차 시나리오 임시 디렉터리 생성 실패(46): {e}")
    })?;
    let max_duration_lease_db_46 = max_duration_multi_dir_46.path().join("lease.sqlite3");
    let max_duration_lease_db_46 = max_duration_lease_db_46.to_str().ok_or_else(|| {
        "MAX_DURATION_EXCEEDED 다회차 lease store 경로가 UTF-8이 아니다(46)".to_string()
    })?;
    let issue_max_duration_46 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            max_duration_lease_db_46,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "2",
            "--do-renew",
            "false",
        ],
        &["--do-renew", "false"],
    )?;
    if !issue_max_duration_46.coordinator_success || !issue_max_duration_46.agent_success {
        return Err(format!(
            "MAX_DURATION_EXCEEDED 다회차 시나리오 최초 발급이 실패했다(46).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_max_duration_46.coordinator_success,
            issue_max_duration_46.coordinator_stdout,
            issue_max_duration_46.coordinator_stderr,
            issue_max_duration_46.agent_success,
            issue_max_duration_46.agent_stdout,
            issue_max_duration_46.agent_stderr
        ));
    }
    std::thread::sleep(std::time::Duration::from_millis(2_200));

    let max_duration_multi_round = run_handshake(
        &fixture,
        &[
            "--lease-db",
            max_duration_lease_db_46,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "2",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
            "--renew-rounds",
            "2",
        ],
        &["--do-renew", "true", "--renew-rounds", "2"],
    )?;
    if !max_duration_multi_round.coordinator_success
        || !max_duration_multi_round
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=6")
        || max_duration_multi_round
            .coordinator_stdout
            .matches("RENEW_RESULT ok=true")
            .count()
            != 1
        || !max_duration_multi_round
            .coordinator_stdout
            .contains(RESULT_OK_MARKER)
        || max_duration_multi_round.agent_success
        || !max_duration_multi_round
            .agent_stderr
            .contains("RENEW_REFUSED:MAX_DURATION_EXCEEDED")
        || max_duration_multi_round
            .agent_stdout
            .contains("RENEW_RESULT ok=true")
    {
        return Err(format!(
            "다회차 MAX_DURATION_EXCEEDED가 첫 회차에서 정상 종료되지 않았거나 이후 RENEW_RESULT가 발생했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            max_duration_multi_round.coordinator_success,
            max_duration_multi_round.coordinator_stdout,
            max_duration_multi_round.coordinator_stderr,
            max_duration_multi_round.agent_success,
            max_duration_multi_round.agent_stdout,
            max_duration_multi_round.agent_stderr
        ));
    }
    report.push_str(
        "46) renew_rounds=2의 첫 회차 MAX_DURATION_EXCEEDED(--max-total-duration-seconds 2, 2.2초 경과) 후 양쪽이 교착 없이 종료되고 이후 RENEW_RESULT가 없음을 확인\n",
    );

    // ── 47. 만료된 Lease의 renew 경로는 signed outcome 없이 raw error로 종료 ──
    // A short TTL plus the Coordinator's test-only post-ACK delay makes the
    // first renewal request arrive after the stored lease has expired. This
    // keeps the test on the actual renew path instead of the reissue path.
    let expired_renew_dir_47 = tempfile::tempdir()
        .map_err(|e| format!("expired renew scenario temp dir creation failed (47): {e}"))?;
    let expired_renew_lease_db_47 = expired_renew_dir_47.path().join("lease.sqlite3");
    let expired_renew_lease_db_47 = expired_renew_lease_db_47
        .to_str()
        .ok_or_else(|| "expired renew lease store path was not UTF-8 (47)".to_string())?;
    let expired_renew_47 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            expired_renew_lease_db_47,
            "--lease-ttl-ms",
            "500",
            "--renew-delay-ms",
            "1000",
            "--do-renew",
            "true",
        ],
        &["--do-renew", "true"],
    )?;
    if expired_renew_47.coordinator_success
        || !expired_renew_47.coordinator_stderr.contains("expired")
        || expired_renew_47
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true")
        || expired_renew_47.agent_success
    {
        return Err(format!(
            "expired Lease renewal was not rejected as a raw error (47).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            expired_renew_47.coordinator_success,
            expired_renew_47.coordinator_stdout,
            expired_renew_47.coordinator_stderr,
            expired_renew_47.agent_success,
            expired_renew_47.agent_stdout,
            expired_renew_47.agent_stderr
        ));
    }
    report.push_str(
        "47) 짧은 TTL로 실제 renew 경로에서 Lease 만료를 유도하고 signed outcome 없이 raw error로 연결 종료 확인\n",
    );

    // ── 48. Agent가 갱신 직전에 만료를 감지하면 요청을 만들지 않음 ──
    // Agent-only delay로 짧은 TTL을 갱신 요청 생성 직전에 만료시킨다.
    // Agent는 LOCAL_EXPIRED로 종료하고 소켓을 닫으므로 Coordinator는
    // 다음 RenewLeaseRequest를 기다리지 않고 EOF 오류로 즉시 종료해야 한다.
    let local_expired_renew_dir_48 = tempfile::tempdir()
        .map_err(|e| format!("local expired renew scenario temp dir creation failed (48): {e}"))?;
    let local_expired_renew_lease_db_48 = local_expired_renew_dir_48.path().join("lease.sqlite3");
    let local_expired_renew_lease_db_48 = local_expired_renew_lease_db_48
        .to_str()
        .ok_or_else(|| "local expired renew lease store path was not UTF-8 (48)".to_string())?;
    let local_expired_started_48 = Instant::now();
    let local_expired_renew_48 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            local_expired_renew_lease_db_48,
            "--lease-ttl-ms",
            "500",
            "--do-renew",
            "true",
        ],
        &["--do-renew", "true", "--renew-delay-ms", "1000"],
    )?;
    let local_expired_elapsed_48 = local_expired_started_48.elapsed();
    if local_expired_elapsed_48 >= HANDSHAKE_HARD_TIMEOUT
        || local_expired_renew_48.agent_success
        || !local_expired_renew_48
            .agent_stderr
            .contains("RENEW_REFUSED:LOCAL_EXPIRED")
        || local_expired_renew_48
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true")
        || !local_expired_renew_48
            .coordinator_stderr
            .contains("RenewLeaseRequest")
        || local_expired_renew_48.coordinator_success
    {
        return Err(format!(
            "Agent local expiry did not fail closed in scenario 48. elapsed={:?}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            local_expired_elapsed_48,
            local_expired_renew_48.coordinator_success,
            local_expired_renew_48.coordinator_stdout,
            local_expired_renew_48.coordinator_stderr,
            local_expired_renew_48.agent_success,
            local_expired_renew_48.agent_stdout,
            local_expired_renew_48.agent_stderr
        ));
    }
    report.push_str(&format!(
        "48) Agent가 expires_at_unix_ms <= now를 갱신 직전에 감지해 LOCAL_EXPIRED로 종료하고 RenewLeaseRequest 없이 Coordinator도 EOF 오류로 종료 (hard timeout=90s, elapsed={:?})\n",
        local_expired_elapsed_48
    ));

    // 49. Same-process reconnect: the first accepted connection is dropped
    // after ACK, then the same Agent/Coordinator PIDs complete attempt 1.
    let reconnect_dir_49 =
        tempfile::tempdir().map_err(|e| format!("reconnect success tempdir failed (49): {e}"))?;
    let lease_db_49 = reconnect_dir_49.path().join("lease.sqlite3");
    let fence_db_49 = reconnect_dir_49.path().join("fence.sqlite3");
    let lease_db_49 = lease_db_49
        .to_str()
        .ok_or_else(|| "lease db 49 is not UTF-8".to_string())?;
    let fence_db_49 = fence_db_49
        .to_str()
        .ok_or_else(|| "fence db 49 is not UTF-8".to_string())?;
    let reconnect_ok_49 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_49,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--drop-connection-after-ack-once",
            "true",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_49, "--do-renew", "false"],
    )?;
    if !reconnect_ok_49.coordinator_success
        || !reconnect_ok_49.agent_success
        || reconnect_ok_49
            .coordinator_stdout
            .matches("CONNECTION_ATTEMPT")
            .count()
            != 2
        || reconnect_ok_49
            .coordinator_stdout
            .matches(RESULT_OK_MARKER)
            .count()
            != 1
        || reconnect_ok_49
            .agent_stdout
            .matches(RESULT_OK_MARKER)
            .count()
            != 1
    {
        return Err(format!(
            "49) reconnect success failed: coordinator={:?} agent={:?}",
            reconnect_ok_49.coordinator_stdout, reconnect_ok_49.agent_stderr
        ));
    }
    report.push_str("49) 동일 Agent/Coordinator PID에서 ACK 후 1회 drop, connection_attempt=1의 새 Grant/ACK nonce로 bounded reconnect 성공; RESULT ok=true 각 1회 (hard timeout=120s, accept-timeout=5000ms)\n");

    // 50. The coordinator drops once and then reaches max-connections=1;
    // the Agent must exhaust its deliberately short retry budget.
    let exhausted_50 = run_reconnect_case(
        &fixture,
        &[
            "--max-connections",
            "1",
            "--accept-timeout-ms",
            "5000",
            "--disconnect-after-ack",
            "true",
            "--do-renew",
            "false",
        ],
        &[
            "--max-reconnect-attempts",
            "2",
            "--max-reconnect-duration-seconds",
            "3",
            "--retry-base-ms",
            "10",
            "--retry-cap-ms",
            "25",
            "--do-renew",
            "false",
        ],
    )?;
    if exhausted_50.agent_success
        || exhausted_50.agent_stdout.contains(RESULT_OK_MARKER)
        || exhausted_50.coordinator_stdout.contains(RESULT_OK_MARKER)
        || !exhausted_50.agent_stderr.contains("ReconnectExhausted")
    {
        return Err(format!(
            "50) reconnect exhaustion failed: coordinator={:?} agent={:?}",
            exhausted_50.coordinator_stderr, exhausted_50.agent_stderr
        ));
    }
    report.push_str("50) max-reconnect-attempts=2/max-duration=3s로 재접속 예산 소진; RESULT ok=true 없음 (hard timeout=120s, accept-timeout=5000ms)\n");

    // 51. Revoke is persisted after the first ACK and before the drop; the
    // second get_or_issue must fail closed.
    let revoke_51 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_49,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--drop-connection-after-ack-once",
            "true",
            "--revoke-before-drop",
            "true",
            "--do-renew",
            "false",
        ],
        &[
            "--max-reconnect-attempts",
            "2",
            "--max-reconnect-duration-seconds",
            "3",
            "--retry-base-ms",
            "10",
            "--retry-cap-ms",
            "25",
            "--fence-db",
            fence_db_49,
            "--do-renew",
            "false",
        ],
    )?;
    if revoke_51.coordinator_success
        || revoke_51.agent_success
        || revoke_51.coordinator_stdout.contains(RESULT_OK_MARKER)
        || revoke_51.agent_stdout.contains(RESULT_OK_MARKER)
        || !revoke_51.coordinator_stderr.contains("revoked")
    {
        return Err(format!(
            "51) reconnect revoke failed: coordinator={:?} agent={:?}",
            revoke_51.coordinator_stderr, revoke_51.agent_stderr
        ));
    }
    report.push_str("51) 첫 ACK 직후 durable revoke 후 재접속 get_or_issue가 revoked로 거부; RESULT ok=true 없음 (hard timeout=120s, accept-timeout=5000ms)\n");

    // 52. A short lease expires while the Coordinator deliberately pauses
    // before its second accept.
    let expire_dir_52 =
        tempfile::tempdir().map_err(|e| format!("reconnect expiry tempdir failed (52): {e}"))?;
    let lease_db_52 = expire_dir_52.path().join("lease.sqlite3");
    let fence_db_52 = expire_dir_52.path().join("fence.sqlite3");
    let lease_db_52 = lease_db_52
        .to_str()
        .ok_or_else(|| "lease db 52 is not UTF-8".to_string())?;
    let fence_db_52 = fence_db_52
        .to_str()
        .ok_or_else(|| "fence db 52 is not UTF-8".to_string())?;
    let expire_52 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_52,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--drop-connection-after-ack-once",
            "true",
            "--lease-ttl-ms",
            "2000",
            "--pause-before-next-accept-ms",
            "2500",
            "--do-renew",
            "false",
        ],
        &[
            "--max-reconnect-attempts",
            "2",
            "--max-reconnect-duration-seconds",
            "3",
            "--retry-base-ms",
            "10",
            "--retry-cap-ms",
            "25",
            "--fence-db",
            fence_db_52,
            "--do-renew",
            "false",
        ],
    )?;
    if expire_52.coordinator_success
        || expire_52.agent_success
        || expire_52.coordinator_stdout.contains(RESULT_OK_MARKER)
        || expire_52.agent_stdout.contains(RESULT_OK_MARKER)
        || !expire_52.coordinator_stderr.contains("expired")
    {
        return Err(format!(
            "52) reconnect expiry failed: coordinator={:?} agent={:?}",
            expire_52.coordinator_stderr, expire_52.agent_stderr
        ));
    }
    report.push_str("52) 2초 TTL과 2500ms next-accept 대기로 재접속 시 expired 거부; RESULT ok=true 없음 (hard timeout=120s, accept-timeout=5000ms)\n");

    // 53–60. Explicit opt-in Hello-first Resume lane. Every case owns a
    // separate temporary directory/database; the legacy 1–52 cases above do
    // not share this state.
    let resume_dir_53 =
        tempfile::tempdir().map_err(|e| format!("resume success tempdir failed (53): {e}"))?;
    let lease_db_53 = resume_dir_53.path().join("lease.sqlite3");
    let fence_db_53 = resume_dir_53.path().join("fence.sqlite3");
    seed_resume_lease(&fixture, &lease_db_53, &fence_db_53, 7, 60_000)?;
    let resumed_53 = run_resume_case(
        &fixture,
        &lease_db_53,
        &fence_db_53,
        "resume-session-53",
        fixture.lease_id,
        fixture.job_id,
        fixture.attempt_id,
        7,
        false,
        false,
    )?;
    assert_resume_outcome(&resumed_53, 53, 1, true)?;
    report.push_str(
        "53) 명시적 --resume-protocol Hello-first Resume 성공(RESUMED), 저장된 expires_at 유지\n",
    );

    let unknown_dir_54 =
        tempfile::tempdir().map_err(|e| format!("resume unknown tempdir failed (54): {e}"))?;
    let lease_db_54 = unknown_dir_54.path().join("lease.sqlite3");
    let fence_db_54 = unknown_dir_54.path().join("fence.sqlite3");
    seed_resume_lease(&fixture, &lease_db_54, &fence_db_54, 7, 60_000)?;
    let unknown_54 = run_resume_case(
        &fixture,
        &lease_db_54,
        &fence_db_54,
        "resume-session-54",
        "unknown-lease-id",
        fixture.job_id,
        fixture.attempt_id,
        7,
        false,
        false,
    )?;
    assert_resume_outcome(&unknown_54, 54, 5, false)?;
    report.push_str("54) 존재하지 않는 lease_id를 UNKNOWN_LEASE로 거부\n");

    let identity_dir_55 =
        tempfile::tempdir().map_err(|e| format!("resume identity tempdir failed (55): {e}"))?;
    let lease_db_55 = identity_dir_55.path().join("lease.sqlite3");
    let fence_db_55 = identity_dir_55.path().join("fence.sqlite3");
    seed_resume_lease(&fixture, &lease_db_55, &fence_db_55, 7, 60_000)?;
    let identity_55 = run_resume_case(
        &fixture,
        &lease_db_55,
        &fence_db_55,
        "resume-session-55",
        fixture.lease_id,
        "wrong-job-id",
        fixture.attempt_id,
        7,
        false,
        false,
    )?;
    assert_resume_outcome(&identity_55, 55, 6, false)?;
    report.push_str("55) job_id 불일치 identity conflict를 IDENTITY_CONFLICT로 거부\n");

    let revoked_dir_56 =
        tempfile::tempdir().map_err(|e| format!("resume revoked tempdir failed (56): {e}"))?;
    let lease_db_56 = revoked_dir_56.path().join("lease.sqlite3");
    let fence_db_56 = revoked_dir_56.path().join("fence.sqlite3");
    seed_resume_lease(&fixture, &lease_db_56, &fence_db_56, 7, 60_000)?;
    gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_56)
        .map_err(|e| format!("resume revoked store open failed (56): {e}"))?
        .mark_revoked(fixture.lease_id, 1)
        .map_err(|e| format!("resume revoke mutation failed (56): {e}"))?;
    let revoked_56 = run_resume_case(
        &fixture,
        &lease_db_56,
        &fence_db_56,
        "resume-session-56",
        fixture.lease_id,
        fixture.job_id,
        fixture.attempt_id,
        7,
        false,
        false,
    )?;
    assert_resume_outcome(&revoked_56, 56, 2, false)?;
    report.push_str("56) durable revoke 상태를 REVOKED로 판정\n");

    let expired_dir_57 =
        tempfile::tempdir().map_err(|e| format!("resume expired tempdir failed (57): {e}"))?;
    let lease_db_57 = expired_dir_57.path().join("lease.sqlite3");
    let fence_db_57 = expired_dir_57.path().join("fence.sqlite3");
    seed_resume_lease(&fixture, &lease_db_57, &fence_db_57, 7, 2_000)?;
    thread::sleep(Duration::from_millis(2_200));
    let expired_57 = run_resume_case(
        &fixture,
        &lease_db_57,
        &fence_db_57,
        "resume-session-57",
        fixture.lease_id,
        fixture.job_id,
        fixture.attempt_id,
        7,
        false,
        false,
    )?;
    assert_resume_outcome(&expired_57, 57, 3, false)?;
    report.push_str("57) expires_at_unix_ms <= now 경계로 EXPIRED 판정\n");

    let superseded_dir_58 =
        tempfile::tempdir().map_err(|e| format!("resume superseded tempdir failed (58): {e}"))?;
    let lease_db_58 = superseded_dir_58.path().join("lease.sqlite3");
    let fence_db_58 = superseded_dir_58.path().join("fence.sqlite3");
    seed_resume_lease(&fixture, &lease_db_58, &fence_db_58, 7, 60_000)?;
    let superseded_58 = run_resume_case(
        &fixture,
        &lease_db_58,
        &fence_db_58,
        "resume-session-58",
        fixture.lease_id,
        fixture.job_id,
        fixture.attempt_id,
        6,
        false,
        false,
    )?;
    assert_resume_outcome(&superseded_58, 58, 4, false)?;
    report.push_str("58) 요청 epoch < 저장 epoch 방향을 SUPERSEDED로 고정\n");

    let ahead_dir_59 =
        tempfile::tempdir().map_err(|e| format!("resume epoch-ahead tempdir failed (59): {e}"))?;
    let lease_db_59 = ahead_dir_59.path().join("lease.sqlite3");
    let fence_db_59 = ahead_dir_59.path().join("fence.sqlite3");
    seed_resume_lease(&fixture, &lease_db_59, &fence_db_59, 7, 60_000)?;
    let ahead_59 = run_resume_case(
        &fixture,
        &lease_db_59,
        &fence_db_59,
        "resume-session-59",
        fixture.lease_id,
        fixture.job_id,
        fixture.attempt_id,
        8,
        false,
        false,
    )?;
    assert_resume_outcome(&ahead_59, 59, 8, false)?;
    report.push_str("59) 요청 epoch > 저장 epoch을 EPOCH_AHEAD(enum 값 8)로 고정\n");

    let unavailable_dir_60 =
        tempfile::tempdir().map_err(|e| format!("resume unavailable tempdir failed (60): {e}"))?;
    let lease_db_60 = unavailable_dir_60.path().join("lease.sqlite3");
    let fence_db_60 = unavailable_dir_60.path().join("fence.sqlite3");
    let unavailable_60 = run_resume_case(
        &fixture,
        &lease_db_60,
        &fence_db_60,
        "resume-session-60",
        fixture.lease_id,
        fixture.job_id,
        fixture.attempt_id,
        7,
        true,
        true,
    )?;
    if unavailable_60.coordinator_success
        || !unavailable_60.coordinator_stderr.contains("kind=storage")
        || unavailable_60
            .coordinator_stdout
            .contains("resume_outcome=7")
        || unavailable_60.coordinator_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "60) Resume without durable store did not fail closed: coordinator_success={} stdout={:?} stderr={:?} agent_success={} agent_stderr={:?}",
            unavailable_60.coordinator_success,
            unavailable_60.coordinator_stdout,
            unavailable_60.coordinator_stderr,
            unavailable_60.agent_success,
            unavailable_60.agent_stderr,
        ));
    }
    report.push_str("60) durable store 없이 Resume을 요청한 구성 오류(Io)를 kind=storage로 분류하고 UNAVAILABLE 없이 fail-closed 종료\n");

    // 61. A truncated first connection is transport-scoped. The same
    // Coordinator must accept and complete a second, valid Agent session.
    let transport_dir_61 = tempfile::tempdir()
        .map_err(|e| format!("transport dispatcher tempdir failed (61): {e}"))?;
    let transport_lease_db_61 = transport_dir_61.path().join("lease.sqlite3");
    let transport_lease_db_61 = transport_lease_db_61
        .to_str()
        .ok_or_else(|| "transport lease db 61 is not UTF-8".to_string())?;
    let transport_61 = run_two_connection_case(
        &fixture,
        &[
            "--lease-db",
            transport_lease_db_61,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--do-renew",
            "false",
        ],
        true,
    )?;
    if !transport_61.coordinator_success
        || !transport_61.second_agent_success
        || transport_61
            .coordinator_stdout
            .matches("CONNECTION_ATTEMPT")
            .count()
            != 2
        || !transport_61.coordinator_stderr.contains("kind=transport")
        || transport_61
            .coordinator_stdout
            .matches(RESULT_OK_MARKER)
            .count()
            != 1
        || transport_61
            .second_agent_stdout
            .matches(RESULT_OK_MARKER)
            .count()
            != 1
    {
        return Err(format!(
            "61) transport isolation failed: coordinator={:?} stderr={:?} agent={:?}",
            transport_61.coordinator_stdout,
            transport_61.coordinator_stderr,
            transport_61.second_agent_stderr
        ));
    }
    report.push_str("61) 첫 연결의 truncated EOF를 transport 오류로 로그하고 connection_attempt=1의 두 번째 정상 Agent를 같은 Coordinator가 수락\n");

    // 62. A forged Agent ACK is a protocol error. It must not poison the
    // accept loop; a second valid Agent session still completes.
    let protocol_dir_62 =
        tempfile::tempdir().map_err(|e| format!("protocol dispatcher tempdir failed (62): {e}"))?;
    let protocol_lease_db_62 = protocol_dir_62.path().join("lease.sqlite3");
    let protocol_lease_db_62 = protocol_lease_db_62
        .to_str()
        .ok_or_else(|| "protocol lease db 62 is not UTF-8".to_string())?;
    let protocol_62 = run_two_connection_case(
        &fixture,
        &[
            "--lease-db",
            protocol_lease_db_62,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--do-renew",
            "false",
        ],
        false,
    )?;
    if !protocol_62.coordinator_success
        || protocol_62.first_agent_success
        || !protocol_62.second_agent_success
        || protocol_62
            .coordinator_stdout
            .matches("CONNECTION_ATTEMPT")
            .count()
            != 2
        || !protocol_62.coordinator_stderr.contains("kind=protocol")
        || protocol_62
            .coordinator_stdout
            .matches(RESULT_OK_MARKER)
            .count()
            != 1
        || protocol_62
            .second_agent_stdout
            .matches(RESULT_OK_MARKER)
            .count()
            != 1
    {
        return Err(format!(
            "62) protocol isolation failed: coordinator={:?} stderr={:?} first={:?} second={:?}",
            protocol_62.coordinator_stdout,
            protocol_62.coordinator_stderr,
            protocol_62.first_agent_stderr,
            protocol_62.second_agent_stderr
        ));
    }
    report.push_str("62) 첫 연결의 위조 Agent 서명을 protocol 오류로 로그하고 다음 정상 Agent 연결까지 accept 계속\n");

    // 63. Opening a lease DB below a missing parent is a storage failure before
    // bind. It must fail closed and must never enter the accept loop.
    let storage_dir_63 =
        tempfile::tempdir().map_err(|e| format!("storage dispatcher tempdir failed (63): {e}"))?;
    let missing_parent_63 = storage_dir_63.path().join("missing-parent");
    let missing_lease_db_63 = missing_parent_63.join("lease.sqlite3");
    let missing_lease_db_63 = missing_lease_db_63
        .to_str()
        .ok_or_else(|| "storage lease db 63 is not UTF-8".to_string())?;
    let storage_63 = run_coordinator_startup_case(
        &fixture,
        &["--lease-db", missing_lease_db_63, "--max-connections", "2"],
    )?;
    if storage_63.success
        || !storage_63.stderr.contains("kind=storage")
        || storage_63.stdout.contains("READY ")
    {
        return Err(format!(
            "63) storage fail-closed failed: stdout={:?} stderr={:?}",
            storage_63.stdout, storage_63.stderr
        ));
    }
    report.push_str("63) 존재하지 않는 --lease-db 부모 경로의 SQLite open 오류를 storage로 로그하고 bind/accept 전에 fail-closed 종료\n");

    // 64. Startup succeeds, then the open lease DB is corrupted before a
    // Resume request. The storage classification must terminate run()
    // immediately instead of signing UNAVAILABLE and accepting again.
    let resume_storage_dir_64 = tempfile::tempdir()
        .map_err(|e| format!("resume storage dispatcher tempdir failed (64): {e}"))?;
    let resume_storage_lease_db_64 = resume_storage_dir_64.path().join("lease.sqlite3");
    let resume_storage_fence_db_64 = resume_storage_dir_64.path().join("fence.sqlite3");
    seed_resume_lease(
        &fixture,
        &resume_storage_lease_db_64,
        &resume_storage_fence_db_64,
        7,
        60_000,
    )?;
    let resume_storage_64 = run_resume_storage_failure_case(
        &fixture,
        &resume_storage_lease_db_64,
        &resume_storage_fence_db_64,
    )?;
    if resume_storage_64.coordinator_success
        || !resume_storage_64
            .coordinator_stderr
            .contains("kind=storage")
        || resume_storage_64
            .coordinator_stdout
            .contains("resume_outcome=7")
        || resume_storage_64
            .coordinator_stdout
            .matches("CONNECTION_ATTEMPT")
            .count()
            != 1
        || resume_storage_64
            .coordinator_stdout
            .contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "64) Resume storage fail-closed failed: coordinator_success={} stdout={:?} stderr={:?} agent_success={} agent_stderr={:?}",
            resume_storage_64.coordinator_success,
            resume_storage_64.coordinator_stdout,
            resume_storage_64.coordinator_stderr,
            resume_storage_64.agent_success,
            resume_storage_64.agent_stderr,
        ));
    }
    report.push_str("64) 정상 startup 뒤 열린 lease DB를 Resume 요청 직전에 손상시켜 kind=storage 분류·UNAVAILABLE 미서명·다음 accept 없는 즉시 run 종료를 검증\n");

    // 65. The first renewal is committed to SQLite but its result is dropped.
    // The same Agent process reconnects through the ordinary Grant/ACK lane,
    // observes exactly that committed Lease, and sends a fresh renewal nonce.
    let ambiguous_dir_65 = tempfile::tempdir()
        .map_err(|e| format!("ambiguous renew recovery tempdir failed (65): {e}"))?;
    let lease_db_path_65 = ambiguous_dir_65.path().join("lease.sqlite3");
    let fence_db_path_65 = ambiguous_dir_65.path().join("fence.sqlite3");
    let lease_db_65 = lease_db_path_65
        .to_str()
        .ok_or_else(|| "lease db 65 is not UTF-8".to_string())?;
    let fence_db_65 = fence_db_path_65
        .to_str()
        .ok_or_else(|| "fence db 65 is not UTF-8".to_string())?;
    let recovered_65 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_65,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--drop-after-renew-commit-before-result-once",
            "true",
            "--do-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_65,
            "--do-renew",
            "true",
            "--recover-ambiguous-renew-from-durable-lease",
            "true",
            "--max-reconnect-attempts",
            "3",
            "--max-reconnect-duration-seconds",
            "5",
            "--retry-base-ms",
            "1",
            "--retry-cap-ms",
            "1",
        ],
    )?;
    let committed_expiry_65 = output_u64_field(
        &recovered_65.coordinator_stdout,
        "DROP_AFTER_RENEW_COMMIT_BEFORE_RESULT_ONCE",
        "expires_at_unix_ms=",
    )
    .map_err(|error| format!("65) committed expiry observation failed: {error}"))?;
    let regranted_expiry_65 = output_u64_field(
        &recovered_65.agent_stdout,
        "LEASE_ACCEPTED connection_attempt=1",
        "expires_at_unix_ms=",
    )
    .map_err(|error| format!("65) regranted expiry observation failed: {error}"))?;
    let first_nonce_65 = output_field(
        &recovered_65.coordinator_stdout,
        "DROP_AFTER_RENEW_COMMIT_BEFORE_RESULT_ONCE",
        "request_nonce=",
    )
    .ok_or_else(|| "65) missing first committed renewal nonce".to_string())?;
    let second_nonce_65 = output_field(
        &recovered_65.coordinator_stdout,
        "RENEW_RESULT ok=true outcome=1",
        "request_nonce=",
    )
    .ok_or_else(|| "65) missing second successful renewal nonce".to_string())?;
    if !recovered_65.coordinator_success
        || !recovered_65.agent_success
        || recovered_65
            .coordinator_stdout
            .matches("CONNECTION_ATTEMPT")
            .count()
            != 2
        || recovered_65.agent_stdout.matches("LEASE_ACCEPTED").count() != 2
        || recovered_65
            .agent_stdout
            .matches("RENEW_RESULT ok=true")
            .count()
            != 1
        || committed_expiry_65 != regranted_expiry_65
        || first_nonce_65 == second_nonce_65
    {
        return Err(format!(
            "65) ambiguous renew recovery failed: committed_expiry={} regranted_expiry={} first_nonce={} second_nonce={} coordinator_success={} stdout={:?} stderr={:?} agent_success={} stdout={:?} stderr={:?}",
            committed_expiry_65,
            regranted_expiry_65,
            first_nonce_65,
            second_nonce_65,
            recovered_65.coordinator_success,
            recovered_65.coordinator_stdout,
            recovered_65.coordinator_stderr,
            recovered_65.agent_success,
            recovered_65.agent_stdout,
            recovered_65.agent_stderr,
        ));
    }
    report.push_str("65) Renew SQLite commit 직후 결과 전송 전 drop; 동일 Agent가 bounded reconnect 후 기존 Grant/ACK get_or_issue 경로에서 정확히 committed expiry를 받고 새 nonce Renew 성공\n");

    // 66. Revocation committed after the ambiguous renewal must make the
    // second get_or_issue fail closed; reconnect cannot resurrect the Lease.
    let revoke_dir_66 = tempfile::tempdir()
        .map_err(|e| format!("ambiguous renew revoke tempdir failed (66): {e}"))?;
    let lease_db_path_66 = revoke_dir_66.path().join("lease.sqlite3");
    let fence_db_path_66 = revoke_dir_66.path().join("fence.sqlite3");
    let lease_db_66 = lease_db_path_66
        .to_str()
        .ok_or_else(|| "lease db 66 is not UTF-8".to_string())?;
    let fence_db_66 = fence_db_path_66
        .to_str()
        .ok_or_else(|| "fence db 66 is not UTF-8".to_string())?;
    let revoked_66 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_66,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--drop-after-renew-commit-before-result-once",
            "true",
            "--revoke-before-drop",
            "true",
            "--do-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_66,
            "--do-renew",
            "true",
            "--recover-ambiguous-renew-from-durable-lease",
            "true",
            "--max-reconnect-attempts",
            "2",
            "--max-reconnect-duration-seconds",
            "5",
            "--retry-base-ms",
            "1",
            "--retry-cap-ms",
            "1",
        ],
    )?;
    let revoked_record_66 =
        gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_66)
            .map_err(|e| format!("66) reopen lease store failed: {e}"))?
            .get(fixture.lease_id)
            .map_err(|e| format!("66) read revoked lease failed: {e}"))?
            .ok_or_else(|| "66) revoked lease disappeared".to_string())?;
    if revoked_66.agent_success
        || revoked_66.coordinator_success
        || revoked_record_66.revoked_at_unix_ms.is_none()
        || !revoked_66.coordinator_stderr.contains("revoked")
        || !revoked_66.agent_stderr.contains("ReconnectExhausted")
        || revoked_66.agent_stdout.contains("RENEW_RESULT ok=true")
    {
        return Err(format!(
            "66) revoke bypassed ambiguous recovery: record={:?} coordinator_stdout={:?} coordinator_stderr={:?} agent_stdout={:?} agent_stderr={:?}",
            revoked_record_66,
            revoked_66.coordinator_stdout,
            revoked_66.coordinator_stderr,
            revoked_66.agent_stdout,
            revoked_66.agent_stderr,
        ));
    }
    report.push_str("66) ambiguous commit 뒤 durable revoke를 확정한 경우 두 번째 get_or_issue가 revoked로 fail-closed; Agent는 bounded ReconnectExhausted로 종료\n");

    // 67. A deliberately short committed renewal expires while the
    // Coordinator pauses before accepting the reconnect.
    let expiry_dir_67 = tempfile::tempdir()
        .map_err(|e| format!("ambiguous renew expiry tempdir failed (67): {e}"))?;
    let lease_db_path_67 = expiry_dir_67.path().join("lease.sqlite3");
    let fence_db_path_67 = expiry_dir_67.path().join("fence.sqlite3");
    let lease_db_67 = lease_db_path_67
        .to_str()
        .ok_or_else(|| "lease db 67 is not UTF-8".to_string())?;
    let fence_db_67 = fence_db_path_67
        .to_str()
        .ok_or_else(|| "fence db 67 is not UTF-8".to_string())?;
    let expired_67 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_67,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--drop-after-renew-commit-before-result-once",
            "true",
            "--renew-extension-ms",
            "300",
            "--pause-before-next-accept-ms",
            "600",
            "--lease-ttl-ms",
            "5000",
            "--do-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_67,
            "--do-renew",
            "true",
            "--recover-ambiguous-renew-from-durable-lease",
            "true",
            "--max-reconnect-attempts",
            "2",
            "--max-reconnect-duration-seconds",
            "5",
            "--retry-base-ms",
            "1",
            "--retry-cap-ms",
            "1",
        ],
    )?;
    if expired_67.agent_success
        || expired_67.coordinator_success
        || !expired_67.coordinator_stderr.contains("expired")
        || !expired_67.agent_stderr.contains("ReconnectExhausted")
        || expired_67
            .agent_stdout
            .contains("LEASE_ACCEPTED connection_attempt=1")
    {
        return Err(format!(
            "67) expiry bypassed ambiguous recovery: coordinator_stdout={:?} coordinator_stderr={:?} agent_stdout={:?} agent_stderr={:?}",
            expired_67.coordinator_stdout,
            expired_67.coordinator_stderr,
            expired_67.agent_stdout,
            expired_67.agent_stderr,
        ));
    }
    report.push_str("67) 300ms로 연장된 committed Lease를 next-accept 600ms 지연 중 만료시켜 두 번째 get_or_issue의 <= expiry 거부 확인\n");

    // 68. The original issued_at remains authoritative after recovery.  Once
    // max duration elapses, the fresh heartbeat is signed as policy refusal.
    let max_dir_68 = tempfile::tempdir()
        .map_err(|e| format!("ambiguous renew max-duration tempdir failed (68): {e}"))?;
    let lease_db_path_68 = max_dir_68.path().join("lease.sqlite3");
    let fence_db_path_68 = max_dir_68.path().join("fence.sqlite3");
    let lease_db_68 = lease_db_path_68
        .to_str()
        .ok_or_else(|| "lease db 68 is not UTF-8".to_string())?;
    let fence_db_68 = fence_db_path_68
        .to_str()
        .ok_or_else(|| "fence db 68 is not UTF-8".to_string())?;
    let maxed_68 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_68,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--drop-after-renew-commit-before-result-once",
            "true",
            "--max-total-duration-seconds",
            "1",
            "--pause-before-next-accept-ms",
            "1200",
            "--do-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_68,
            "--do-renew",
            "true",
            "--recover-ambiguous-renew-from-durable-lease",
            "true",
            "--max-reconnect-attempts",
            "2",
            "--max-reconnect-duration-seconds",
            "5",
            "--retry-base-ms",
            "1",
            "--retry-cap-ms",
            "1",
        ],
    )?;
    if !maxed_68.coordinator_success
        || maxed_68.agent_success
        || !maxed_68
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=6")
        || !maxed_68
            .agent_stderr
            .contains("RENEW_REFUSED:MAX_DURATION_EXCEEDED")
        || maxed_68.agent_stderr.contains("ReconnectExhausted")
    {
        return Err(format!(
            "68) max-duration bypassed ambiguous recovery: coordinator_stdout={:?} coordinator_stderr={:?} agent_stdout={:?} agent_stderr={:?}",
            maxed_68.coordinator_stdout,
            maxed_68.coordinator_stderr,
            maxed_68.agent_stdout,
            maxed_68.agent_stderr,
        ));
    }
    report.push_str("68) ambiguous recovery 후에도 최초 issued_at 기준 1초 max-duration이 유지되어 새 heartbeat를 signed MAX_DURATION_EXCEEDED로 즉시 종료\n");

    // 69. Deliberately reusing the old nonce proves this is not response
    // replay/idempotency: the in-memory guard still rejects the duplicate.
    let replay_dir_69 = tempfile::tempdir()
        .map_err(|e| format!("ambiguous renew replay tempdir failed (69): {e}"))?;
    let lease_db_path_69 = replay_dir_69.path().join("lease.sqlite3");
    let fence_db_path_69 = replay_dir_69.path().join("fence.sqlite3");
    let lease_db_69 = lease_db_path_69
        .to_str()
        .ok_or_else(|| "lease db 69 is not UTF-8".to_string())?;
    let fence_db_69 = fence_db_path_69
        .to_str()
        .ok_or_else(|| "fence db 69 is not UTF-8".to_string())?;
    let replayed_69 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_69,
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--drop-after-renew-commit-before-result-once",
            "true",
            "--do-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_69,
            "--do-renew",
            "true",
            "--recover-ambiguous-renew-from-durable-lease",
            "true",
            "--reuse-renew-nonce-after-reconnect",
            "true",
            "--max-reconnect-attempts",
            "2",
            "--max-reconnect-duration-seconds",
            "5",
            "--retry-base-ms",
            "1",
            "--retry-cap-ms",
            "1",
        ],
    )?;
    let replay_error_69 = replayed_69.coordinator_stderr.to_ascii_lowercase();
    if replayed_69.coordinator_success
        || replayed_69.agent_success
        || !(replay_error_69.contains("duplicate") || replay_error_69.contains("replay"))
        || replayed_69
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=1")
        || !replayed_69.agent_stderr.contains("ReconnectExhausted")
    {
        return Err(format!(
            "69) same nonce was not rejected: coordinator_stdout={:?} coordinator_stderr={:?} agent_stdout={:?} agent_stderr={:?}",
            replayed_69.coordinator_stdout,
            replayed_69.coordinator_stderr,
            replayed_69.agent_stdout,
            replayed_69.agent_stderr,
        ));
    }
    report.push_str("69) 재접속 뒤 old Renew nonce를 강제로 재사용하면 Coordinator InMemoryReplayGuard가 Duplicate/replay로 거부; 이전 응답 재생이 아님을 확인\n");

    // 70. Legacy mode has no durable authority.  An Agent-side drop after the
    // flushed request creates the same ambiguity, but the recovery gate is
    // absent and therefore no second connection is attempted.
    let legacy_dir_70 = tempfile::tempdir()
        .map_err(|e| format!("legacy ambiguous gate tempdir failed (70): {e}"))?;
    let fence_db_path_70 = legacy_dir_70.path().join("fence.sqlite3");
    let fence_db_70 = fence_db_path_70
        .to_str()
        .ok_or_else(|| "fence db 70 is not UTF-8".to_string())?;
    let legacy_70 = run_reconnect_case(
        &fixture,
        &[
            "--max-connections",
            "1",
            "--accept-timeout-ms",
            "5000",
            "--do-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_70,
            "--do-renew",
            "true",
            "--drop-after-renew-request-once",
            "true",
            "--max-reconnect-attempts",
            "3",
            "--max-reconnect-duration-seconds",
            "5",
            "--retry-base-ms",
            "1",
            "--retry-cap-ms",
            "1",
        ],
    )?;
    if legacy_70.agent_success
        || !legacy_70
            .agent_stderr
            .contains("AMBIGUOUS_RENEW_RECOVERY_DISABLED")
        || legacy_70.agent_stderr.contains("ReconnectExhausted")
        || legacy_70
            .coordinator_stdout
            .matches("CONNECTION_ATTEMPT")
            .count()
            != 1
    {
        return Err(format!(
            "70) unsafe legacy mode entered ambiguous recovery: coordinator_stdout={:?} coordinator_stderr={:?} agent_stdout={:?} agent_stderr={:?}",
            legacy_70.coordinator_stdout,
            legacy_70.coordinator_stderr,
            legacy_70.agent_stdout,
            legacy_70.agent_stderr,
        ));
    }
    report.push_str("70) 명시적 unsafe legacy(--lease-db 없음)에서 request flush 뒤 결과 유실을 만들어도 durable recovery gate가 꺼져 connection_attempt=0 한 번만 실행\n");

    // 71. If the Coordinator consumed max-connections while dropping the
    // committed result, all later connects are refused.  The Agent must leave
    // through its bounded retry budget rather than waiting forever.
    let exhausted_dir_71 = tempfile::tempdir()
        .map_err(|e| format!("ambiguous max-connections tempdir failed (71): {e}"))?;
    let lease_db_path_71 = exhausted_dir_71.path().join("lease.sqlite3");
    let fence_db_path_71 = exhausted_dir_71.path().join("fence.sqlite3");
    let lease_db_71 = lease_db_path_71
        .to_str()
        .ok_or_else(|| "lease db 71 is not UTF-8".to_string())?;
    let fence_db_71 = fence_db_path_71
        .to_str()
        .ok_or_else(|| "fence db 71 is not UTF-8".to_string())?;
    let exhausted_started_71 = Instant::now();
    let exhausted_71 = run_reconnect_case(
        &fixture,
        &[
            "--lease-db",
            lease_db_71,
            "--max-connections",
            "1",
            "--accept-timeout-ms",
            "5000",
            "--drop-after-renew-commit-before-result-once",
            "true",
            "--do-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_71,
            "--do-renew",
            "true",
            "--recover-ambiguous-renew-from-durable-lease",
            "true",
            "--max-reconnect-attempts",
            "3",
            "--max-reconnect-duration-seconds",
            "3",
            "--retry-base-ms",
            "10",
            "--retry-cap-ms",
            "25",
        ],
    )?;
    let exhausted_elapsed_71 = exhausted_started_71.elapsed();
    if !exhausted_71.coordinator_success
        || exhausted_71.agent_success
        || !exhausted_71.agent_stderr.contains("ReconnectExhausted")
        || exhausted_elapsed_71 >= Duration::from_secs(10)
        || exhausted_71
            .coordinator_stdout
            .matches("CONNECTION_ATTEMPT")
            .count()
            != 1
    {
        return Err(format!(
            "71) max-connections exhaustion was not bounded: elapsed={:?} coordinator_success={} stdout={:?} stderr={:?} agent_success={} stdout={:?} stderr={:?}",
            exhausted_elapsed_71,
            exhausted_71.coordinator_success,
            exhausted_71.coordinator_stdout,
            exhausted_71.coordinator_stderr,
            exhausted_71.agent_success,
            exhausted_71.agent_stdout,
            exhausted_71.agent_stderr,
        ));
    }
    report.push_str(&format!(
        "71) commit/result 사이 drop과 함께 Coordinator max-connections=1 소진; Agent가 무한 대기 없이 ReconnectExhausted ({:?}, hard timeout=120s)\n",
        exhausted_elapsed_71
    ));

    // 72. The local opt-in must not turn a legacy Coordinator into a durable
    // authority.  The first request is deliberately made ambiguous, then the
    // recovery connection receives a correctly signed v2 Grant whose durable
    // bit is false.  The Agent must reject it before ACK or another renewal.
    let legacy_dir_72 = tempfile::tempdir()
        .map_err(|e| format!("legacy durable attestation tempdir failed (72): {e}"))?;
    let fence_db_path_72 = legacy_dir_72.path().join("fence.sqlite3");
    let fence_db_72 = fence_db_path_72
        .to_str()
        .ok_or_else(|| "fence db 72 is not UTF-8".to_string())?;
    let legacy_recovery_72 = run_reconnect_case(
        &fixture,
        &[
            "--max-connections",
            "2",
            "--accept-timeout-ms",
            "5000",
            "--do-renew",
            "true",
        ],
        &[
            "--fence-db",
            fence_db_72,
            "--do-renew",
            "true",
            "--drop-after-renew-request-once",
            "true",
            "--recover-ambiguous-renew-from-durable-lease",
            "true",
            "--max-reconnect-attempts",
            "2",
            "--max-reconnect-duration-seconds",
            "5",
            "--retry-base-ms",
            "1",
            "--retry-cap-ms",
            "1",
        ],
    )?;
    if legacy_recovery_72.agent_success
        || legacy_recovery_72.coordinator_success
        || !legacy_recovery_72
            .agent_stderr
            .contains("DURABLE_LEASE_RECOVERY_REFUSED")
        || legacy_recovery_72
            .agent_stderr
            .contains("ReconnectExhausted")
        || legacy_recovery_72
            .coordinator_stdout
            .matches("CONNECTION_ATTEMPT")
            .count()
            != 2
        || legacy_recovery_72
            .agent_stdout
            .contains("LEASE_ACCEPTED connection_attempt=1")
        || legacy_recovery_72
            .agent_stdout
            .matches("RENEW_RESULT ok=true")
            .count()
            != 0
    {
        return Err(format!(
            "72) legacy Coordinator was accepted as durable recovery authority: coordinator_success={} stdout={:?} stderr={:?} agent_success={} stdout={:?} stderr={:?}",
            legacy_recovery_72.coordinator_success,
            legacy_recovery_72.coordinator_stdout,
            legacy_recovery_72.coordinator_stderr,
            legacy_recovery_72.agent_success,
            legacy_recovery_72.agent_stdout,
            legacy_recovery_72.agent_stderr,
        ));
    }
    report.push_str("72) recovery=true 오조합에서도 legacy Coordinator의 signed durable=false Grant를 ACK·새 Renew 전에 fatal 거부\n");

    // 73) Resume 경로가 durable fence watermark 를 거친다.
    //
    //   Coordinator 가 정상적으로 서명해 `RESUMED` 를 돌려줘도, 그
    //   Lease 의 epoch 가 **이 Agent 가 이미 본 epoch 보다 낮으면**
    //   받아들이면 안 된다. Coordinator 저장소가 오래된 백업으로
    //   되돌려진 상황을 재현한다 — fence DB 는 epoch 9 를 기억하고,
    //   Coordinator 의 lease DB 는 epoch 7 만 가진 별도 파일이다.
    let resume_dir_73 = tempfile::tempdir()
        .map_err(|e| format!("resume fence regression tempdir failed (73): {e}"))?;
    let fence_db_73 = resume_dir_73.path().join("fence.sqlite3");
    let lease_db_high_73 = resume_dir_73.path().join("lease-high.sqlite3");
    let lease_db_low_73 = resume_dir_73.path().join("lease-low.sqlite3");
    let fence_db_seed_73 = resume_dir_73.path().join("fence-seed.sqlite3");

    // (1) 이 Agent 의 durable watermark 를 epoch 9 로 올린다.
    seed_resume_lease(&fixture, &lease_db_high_73, &fence_db_73, 9, 60_000)?;
    // (2) 따로 떨어진 fence DB 로 epoch 7 짜리 Lease 를 가진 Coordinator
    //     저장소를 별도로 만든다(오래된 백업 역할).
    seed_resume_lease(&fixture, &lease_db_low_73, &fence_db_seed_73, 7, 60_000)?;

    let stale_resume_73 = run_resume_case(
        &fixture,
        &lease_db_low_73,
        &fence_db_73,
        "resume-session-73",
        fixture.lease_id,
        fixture.job_id,
        fixture.attempt_id,
        7,
        false,
        true,
    )?;
    // Coordinator 는 RESUMED(outcome=1) 를 돌려주고 Agent 만 거부해야
    // 한다 — 즉 이 방어는 Coordinator 의 협조에 의존하지 않는다.
    assert_resume_outcome(&stale_resume_73, 73, 1, false)?;
    let agent_output_73 = format!(
        "{}\n{}",
        stale_resume_73.agent_stdout, stale_resume_73.agent_stderr
    );
    // 저장소 장애가 아니라 **정책 거부**임을 문자열로 가른다.
    if !agent_output_73.contains("RESUME_REJECTED: fence_epoch")
        || agent_output_73.contains("FENCE_STORAGE_ERROR")
    {
        return Err(format!(
            "73) Resume 이 durable fence watermark 를 거치지 않았다: agent_stdout={:?} agent_stderr={:?}",
            stale_resume_73.agent_stdout, stale_resume_73.agent_stderr
        ));
    }
    report.push_str("73) Coordinator가 RESUMED를 서명해도 durable fence watermark보다 낮은 epoch은 Agent가 거부\n");

    // ── 74~77) nested JobManifest 독립 검증 ─────────────────────────
    //
    //   Grant 에 실려 온 Manifest 는 **제출자가 서명한 별도 메시지**다.
    //   outer Grant 서명이 유효해도 nested 서명은 따로 위조될 수 있으므로
    //   Agent 가 독립 검증해야 한다 — nested Lease 와 같은 이유다.
    //
    //   제출자 키는 Coordinator·Agent 키와 다르다. 같은 키를 쓰면
    //   "독립 검증" 이 아무것도 증명하지 못한다.
    let submitter_seed_hex = to_hex(&fixture.submitter_seed);

    // ★ 제출자가 직접 서명한다. Coordinator 는 제출자 개인키를
    //   보지 못하고 서명된 파일을 실어 나를 뿐이다 — 그래야 Agent 의
    //   독립 검증이 의미를 갖는다.
    let manifest_dir = tempfile::tempdir()
        .map_err(|e| format!("manifest tempdir 생성 실패: {e}"))?;
    let manifest_path = manifest_dir.path().join("job.manifest");
    let manifest_path_str = manifest_path
        .to_str()
        .ok_or_else(|| format!("manifest 경로가 UTF-8 이 아니다: {manifest_path:?}"))?
        .to_string();
    run_submit(
        &fixture,
        fixture.job_id,
        "python",
        "train.py,--epochs,3",
        &submitter_seed_hex,
        &manifest_path_str,
    )?;

    let manifest_coordinator_args: Vec<String> = vec![
        "--manifest-file".to_string(),
        manifest_path_str.clone(),
    ];
    let manifest_coordinator_refs: Vec<&str> = manifest_coordinator_args
        .iter()
        .map(String::as_str)
        .collect();
    let manifest_agent_args = ["--submitter-pubkey", fixture.submitter_pub_hex.as_str()];

    // 74) 정상 — Agent 가 nested Manifest 를 검증하고 실행 지시를 뽑는다.
    let manifest_74 = run_handshake(&fixture, &manifest_coordinator_refs, &manifest_agent_args)?;
    if !manifest_74.agent_success
        || !manifest_74.agent_stdout.contains("MANIFEST_ACCEPTED")
        || !manifest_74
            .agent_stdout
            .contains("entrypoint=python args=3 env_vars=0")
        || !manifest_74.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "74) nested Manifest 정상 경로 실패: agent_success={} stdout={:?} stderr={:?}",
            manifest_74.agent_success, manifest_74.agent_stdout, manifest_74.agent_stderr
        ));
    }
    report.push_str(
        "74) Grant 에 실린 제출자 서명 Manifest 를 Agent 가 독립 검증하고 실행 지시(entrypoint·args)를 도출\n",
    );

    // 75) 제출자 서명 위조 — outer Grant 서명은 멀쩡한데 nested 만 망가뜨린다.
    let mut forged_args = manifest_coordinator_args.clone();
    forged_args.push("--corrupt-manifest-signature".to_string());
    forged_args.push("true".to_string());
    let forged_refs: Vec<&str> = forged_args.iter().map(String::as_str).collect();
    let manifest_75 = run_handshake(&fixture, &forged_refs, &manifest_agent_args)?;
    let out_75 = format!("{}\n{}", manifest_75.agent_stdout, manifest_75.agent_stderr);
    if manifest_75.agent_success
        || !out_75.contains("MANIFEST_REJECTED")
        || manifest_75.agent_stdout.contains("MANIFEST_ACCEPTED")
        || manifest_75.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "75) 위조된 nested Manifest 서명이 통과했다: agent_success={} stdout={:?} stderr={:?}",
            manifest_75.agent_success, manifest_75.agent_stdout, manifest_75.agent_stderr
        ));
    }
    report.push_str(
        "75) outer Grant 서명이 유효해도 nested Manifest 서명이 위조되면 Agent 가 거부\n",
    );

    // 76) manifest_hash 위조 — Coordinator 가 보낸 해시를 신뢰하지 않는다.
    //
    //     ★ 이 거부는 Agent 의 Manifest 처리 코드가 아니라 **프로토콜
    //       계층**이 한다 — crates/protocol/src/signable.rs 의
    //       ExecutionGrant::check_derived_consistency() 가 Grant 검증
    //       중에 BLAKE3_256(sig_input_of(JobManifest)) 를 재계산해
    //       대조하고 DerivedMismatch 로 Grant 자체를 거부한다.
    //       (CLAUDE.md §0.2 가 요구하는 그 검사다.)
    //
    //       처음엔 Agent 쪽에도 같은 대조를 넣었다가, 뮤테이션으로
    //       그 코드가 도달하지 않음을 확인하고 제거했다. 이 시나리오는
    //       그래서 "Agent 재계산" 이 아니라 "Grant 파생값 대조" 를
    //       증명한다 — 무엇이 실제로 막는지 정확히 적는다.
    let mut bad_hash_args = manifest_coordinator_args.clone();
    bad_hash_args.push("--corrupt-manifest-hash".to_string());
    bad_hash_args.push("true".to_string());
    let bad_hash_refs: Vec<&str> = bad_hash_args.iter().map(String::as_str).collect();
    let manifest_76 = run_handshake(&fixture, &bad_hash_refs, &manifest_agent_args)?;
    let out_76 = format!("{}\n{}", manifest_76.agent_stdout, manifest_76.agent_stderr);
    if manifest_76.agent_success
        || !out_76.contains("ExecutionGrant.manifest_hash")
        || !out_76.contains("DerivedMismatch")
        || manifest_76.agent_stdout.contains("MANIFEST_ACCEPTED")
    {
        return Err(format!(
            "76) Manifest 와 다른 manifest_hash 가 통과했다: agent_success={} stdout={:?} stderr={:?}",
            manifest_76.agent_success, manifest_76.agent_stdout, manifest_76.agent_stderr
        ));
    }
    report.push_str(
        "76) Coordinator 가 Manifest 와 다른 manifest_hash 를 보내면 프로토콜 계층의 파생값 대조(DerivedMismatch)가 Grant 자체를 거부\n",
    );

    // 77) 제출자 키 없이 Manifest 가 오면 fail closed.
    //     조용히 무시하면 "지시가 없다" 와 "검증 못 한 지시가 왔다" 가
    //     구분되지 않는다.
    let manifest_77 = run_handshake(&fixture, &manifest_coordinator_refs, &[])?;
    let out_77 = format!("{}\n{}", manifest_77.agent_stdout, manifest_77.agent_stderr);
    if manifest_77.agent_success
        || !out_77.contains("MANIFEST_REJECTED")
        || manifest_77.agent_stdout.contains("MANIFEST_ACCEPTED")
    {
        return Err(format!(
            "77) 제출자 공개키 없이 Manifest 를 받아들였다: agent_success={} stdout={:?} stderr={:?}",
            manifest_77.agent_success, manifest_77.agent_stdout, manifest_77.agent_stderr
        ));
    }
    report.push_str(
        "77) 제출자 공개키가 없으면 Manifest 가 실린 Grant 를 fail closed 로 거부(조용히 무시하지 않음)\n",
    );

    // 78) Manifest 의 job_id 가 Lease 와 다르면 거부.
    //
    //     Manifest 서명이 유효해도 **다른 Job 의 것**일 수 있다 —
    //     제출자가 예전에 서명한 Manifest 를 Coordinator 가 이 Grant 에
    //     끼워 넣는 경우다. 서명만으로는 "어느 Job 인가" 가 안 묶인다.
    let wrong_job_path = manifest_dir.path().join("other-job.manifest");
    let wrong_job_path_str = wrong_job_path
        .to_str()
        .ok_or_else(|| format!("경로가 UTF-8 이 아니다: {wrong_job_path:?}"))?
        .to_string();
    run_submit(
        &fixture,
        "01JOTHERJOBSELFTEST000001",
        "python",
        "train.py,--epochs,3",
        &submitter_seed_hex,
        &wrong_job_path_str,
    )?;
    let wrong_job_args: Vec<String> =
        vec!["--manifest-file".to_string(), wrong_job_path_str.clone()];
    let wrong_job_refs: Vec<&str> = wrong_job_args.iter().map(String::as_str).collect();
    let manifest_78 = run_handshake(&fixture, &wrong_job_refs, &manifest_agent_args)?;
    let out_78 = format!("{}\n{}", manifest_78.agent_stdout, manifest_78.agent_stderr);
    if manifest_78.agent_success
        || !out_78.contains("MANIFEST_REJECTED")
        || !out_78.contains("job_id")
        || manifest_78.agent_stdout.contains("MANIFEST_ACCEPTED")
    {
        return Err(format!(
            "78) Lease 와 다른 job_id 의 Manifest 가 통과했다: agent_success={} stdout={:?} stderr={:?}",
            manifest_78.agent_success, manifest_78.agent_stdout, manifest_78.agent_stderr
        ));
    }
    report.push_str(
        "78) 서명이 유효해도 Manifest 의 job_id 가 같은 Grant 의 Lease 와 다르면 Agent 가 거부\n",
    );

    // ── 79~82) 실제 프로세스 실행 ───────────────────────────────────
    //
    //   ★ 이 저장소가 **남의 코드를 실제로 돌리는 첫 지점**이다.
    //     그래서 selftest 도 무해한 대상만 쓴다 — 실행 대상은
    //     `cmd /c exit N` 이고, 검증하는 것은 "지시대로 떴고 종료
    //     코드를 관측했는가" 뿐이다.
    //
    //   `exec.rs` 의 세 게이트를 각각 증명한다.
    //     79  opt-in 없으면 실행 안 함
    //     80  opt-in 하면 실제로 뜨고 exit_code=0 관측
    //     81  0 이 아닌 종료 코드를 구분해 보고
    //     82  상한을 못 걸면 실행 자체를 안 함
    let cmd_exe =
        std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_string());
    let exec_ok_path = manifest_dir.path().join("exec-ok.manifest");
    let exec_ok_str = exec_ok_path
        .to_str()
        .ok_or_else(|| format!("경로가 UTF-8 이 아니다: {exec_ok_path:?}"))?
        .to_string();
    run_submit(
        &fixture,
        fixture.job_id,
        &cmd_exe,
        "/c,exit,0",
        &submitter_seed_hex,
        &exec_ok_str,
    )?;
    let exec_coordinator_base: Vec<String> =
        vec!["--manifest-file".to_string(), exec_ok_str.clone()];
    let exec_coordinator_refs: Vec<&str> =
        exec_coordinator_base.iter().map(String::as_str).collect();

    // 79) opt-in 을 안 켜면 지시만 뽑고 실행하지 않는다.
    //
    //     ★ 이게 **기본값**이다. 위험한 동작이 기본으로 켜져 있으면
    //       안 된다(DoD-29 와 같은 원칙).
    let exec_79 = run_handshake(&fixture, &exec_coordinator_refs, &manifest_agent_args)?;
    if !exec_79.agent_success
        || !exec_79.agent_stdout.contains("WORKLOAD_SKIPPED")
        || !exec_79.agent_stdout.contains("reason=not_opted_in")
        || exec_79.agent_stdout.contains("WORKLOAD_EXITED")
    {
        return Err(format!(
            "79) opt-in 없이 실행됐거나 건너뛰기가 보고되지 않았다: stdout={:?} stderr={:?}",
            exec_79.agent_stdout, exec_79.agent_stderr
        ));
    }
    report.push_str(
        "79) 실행 opt-in 이 꺼진 기본값에서는 실행 지시를 뽑되 프로세스를 띄우지 않고 WORKLOAD_SKIPPED 보고\n",
    );

    // 80) opt-in 하면 실제로 프로세스가 뜨고 종료 코드 0 을 관측한다.
    let exec_on: Vec<&str> = vec![
        "--submitter-pubkey",
        fixture.submitter_pub_hex.as_str(),
        "--i-understand-this-executes-untrusted-code",
        "true",
    ];
    let exec_80 = run_handshake(&fixture, &exec_coordinator_refs, &exec_on)?;
    if !exec_80.agent_success
        || !exec_80.agent_stdout.contains("WORKLOAD_EXITED")
        || !exec_80.agent_stdout.contains("exit_code=0")
        || !exec_80.agent_stdout.contains("WORKLOAD_RESULT ok=true")
    {
        return Err(format!(
            "80) 실제 프로세스 실행/종료 관측 실패: stdout={:?} stderr={:?}",
            exec_80.agent_stdout, exec_80.agent_stderr
        ));
    }
    // 상한이 실제로 걸렸는지도 값으로 확인한다 — 0 이면 안 걸린 것이다.
    if exec_80.agent_stdout.contains("commit_limit_bytes=0") {
        return Err(format!(
            "80) 커밋 상한이 0 으로 보고됐다 — 상한 없이 실행됐다: stdout={:?}",
            exec_80.agent_stdout
        ));
    }
    report
        .push_str("80) opt-in 시 실제 프로세스가 뜨고 exit_code=0 과 0 이 아닌 커밋 상한을 관측\n");

    // 81) 0 이 아닌 종료 코드를 **구분해서** 보고한다.
    //
    //     state-machines.md §3 이 WORKLOAD_EXITED_OK 와
    //     WORKLOAD_EXITED_ERROR 를 다른 전이로 두는 이유다.
    let exec_fail_path = manifest_dir.path().join("exec-fail.manifest");
    let exec_fail_str = exec_fail_path
        .to_str()
        .ok_or_else(|| format!("경로가 UTF-8 이 아니다: {exec_fail_path:?}"))?
        .to_string();
    run_submit(
        &fixture,
        fixture.job_id,
        &cmd_exe,
        "/c,exit,7",
        &submitter_seed_hex,
        &exec_fail_str,
    )?;
    let fail_args: Vec<String> = vec!["--manifest-file".to_string(), exec_fail_str.clone()];
    let fail_refs: Vec<&str> = fail_args.iter().map(String::as_str).collect();
    let exec_81 = run_handshake(&fixture, &fail_refs, &exec_on)?;
    if !exec_81.agent_stdout.contains("exit_code=7")
        || !exec_81.agent_stdout.contains("WORKLOAD_RESULT ok=false")
        || exec_81.agent_stdout.contains("WORKLOAD_RESULT ok=true")
    {
        return Err(format!(
            "81) 0 이 아닌 종료 코드가 성공과 구분되지 않았다: stdout={:?} stderr={:?}",
            exec_81.agent_stdout, exec_81.agent_stderr
        ));
    }
    report.push_str("81) 종료 코드 7 을 관측해 WORKLOAD_RESULT ok=false 로 성공과 구분\n");

    // 82) 상한을 걸 수 없으면 **실행하지 않는다.**
    //
    //     "일단 띄우고 상한은 나중에" 를 하지 않는다는 계약을 고정한다
    //     (CLAUDE.md §0.4).
    let no_limit: Vec<&str> = vec![
        "--submitter-pubkey",
        fixture.submitter_pub_hex.as_str(),
        "--i-understand-this-executes-untrusted-code",
        "true",
        "--workload-commit-limit-bytes",
        "0",
    ];
    let exec_82 = run_handshake(&fixture, &exec_coordinator_refs, &no_limit)?;
    let out_82 = format!("{}\n{}", exec_82.agent_stdout, exec_82.agent_stderr);
    if exec_82.agent_success
        || !out_82.contains("EXEC_REFUSED:LIMIT_NOT_APPLIED")
        // ★ 어느 계층이 막았는지까지 구분한다. runtime-windows 도 0 을
        //   거부하므로("커밋 상한이 0이다"), exec.rs 의 게이트가 실제로
        //   먼저 막는지 확인하려면 그쪽 메시지를 봐야 한다 —
        //   뮤테이션으로 이 구분이 없으면 게이트를 지워도 안 잡힘을 확인했다.
        || !out_82.contains("commit_limit_bytes")
        || exec_82.agent_stdout.contains("WORKLOAD_EXITED")
    {
        return Err(format!(
            "82) 상한 없이 실행됐거나 거부가 보고되지 않았다: agent_success={} stdout={:?} stderr={:?}",
            exec_82.agent_success, exec_82.agent_stdout, exec_82.agent_stderr
        ));
    }
    report.push_str(
        "82) 커밋 상한을 걸 수 없으면(상한 0) 프로세스를 띄우지 않고 EXEC_REFUSED:LIMIT_NOT_APPLIED 로 거부\n",
    );

    Ok(report)
}

struct TwoConnectionOutcome {
    coordinator_success: bool,
    coordinator_stdout: String,
    coordinator_stderr: String,
    first_agent_success: bool,
    first_agent_stderr: String,
    second_agent_success: bool,
    second_agent_stdout: String,
    second_agent_stderr: String,
}

/// Run two sequential clients against one Coordinator. The first client can
/// be a deliberately truncated raw TCP connection, which exercises the real
/// transport EOF path without changing the Agent crate.
fn run_two_connection_case(
    fixture: &Fixture,
    extra_coordinator_args: &[&str],
    first_is_truncated_transport: bool,
) -> Result<TwoConnectionOutcome, String> {
    let deadline = Instant::now() + Duration::from_secs(120);
    let coordinator_seed_hex = to_hex(&fixture.coordinator_seed);
    let mut coordinator_args: Vec<String> = vec![
        "coordinator-stub",
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        coordinator_seed_hex.as_str(),
        "--peer-pubkey",
        fixture.agent_pub_hex.as_str(),
        "--coordinator-device-id",
        fixture.coordinator_device_id,
        "--agent-device-id",
        fixture.agent_device_id,
        "--grant-id",
        fixture.grant_id,
        "--attempt-id",
        fixture.attempt_id,
        "--lease-id",
        fixture.lease_id,
        "--job-id",
        fixture.job_id,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    coordinator_args.extend(extra_coordinator_args.iter().map(|arg| (*arg).to_owned()));
    if !extra_coordinator_args.contains(&"--lease-db") {
        coordinator_args.extend([
            "--i-understand-legacy-mode-is-unsafe".to_owned(),
            "true".to_owned(),
        ]);
    }

    let mut coordinator = Command::new(&fixture.exe)
        .args(&coordinator_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("two-connection Coordinator spawn failed: {e}"))?;
    let stdout_pipe = coordinator.stdout.take().expect("piped coordinator stdout");
    let stderr_pipe = coordinator.stderr.take().expect("piped coordinator stderr");
    let (ready_sender, ready_receiver) = mpsc::channel();
    let stdout_reader = thread::spawn(move || -> Result<String, std::io::Error> {
        let mut reader = BufReader::new(stdout_pipe);
        let mut ready_line = String::new();
        reader.read_line(&mut ready_line)?;
        ready_sender.send(ready_line.clone()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "READY receiver dropped")
        })?;
        let mut rest = String::new();
        reader.read_to_string(&mut rest)?;
        Ok(format!("{ready_line}{rest}"))
    });
    let stderr_reader = thread::spawn(move || -> Result<String, std::io::Error> {
        let mut reader = BufReader::new(stderr_pipe);
        let mut output = String::new();
        reader.read_to_string(&mut output)?;
        Ok(output)
    });
    let ready_line =
        match ready_receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) => line,
            Err(error) => {
                let _ = coordinator.kill();
                let _ = coordinator.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("two-connection Coordinator READY timeout: {error}"));
            }
        };
    let address = ready_line
        .trim()
        .strip_prefix("READY ")
        .ok_or_else(|| format!("two-connection Coordinator READY invalid: {ready_line:?}"))?
        .to_owned();

    let first_wire = if first_is_truncated_transport {
        let socket = std::net::TcpStream::connect(&address)
            .map_err(|e| format!("truncated transport client connect failed: {e}"))?;
        drop(socket);
        WireClientOutcome {
            success: false,
            stdout: String::new(),
            stderr: String::new(),
        }
    } else {
        run_wire_agent_client(&address, fixture, 0, true)?
    };
    let second_wire = run_wire_agent_client(&address, fixture, 1, false)?;
    let coordinator_status = coordinator
        .wait_until(deadline)
        .map_err(|e| format!("two-connection Coordinator wait failed: {e}"))?;
    let coordinator_stdout = stdout_reader
        .join()
        .map_err(|_| "two-connection Coordinator stdout reader panicked".to_string())?
        .map_err(|e| format!("two-connection Coordinator stdout read failed: {e}"))?;
    let coordinator_stderr = stderr_reader
        .join()
        .map_err(|_| "two-connection Coordinator stderr reader panicked".to_string())?
        .map_err(|e| format!("two-connection Coordinator stderr read failed: {e}"))?;

    Ok(TwoConnectionOutcome {
        coordinator_success: coordinator_status.success(),
        coordinator_stdout,
        coordinator_stderr,
        first_agent_success: first_wire.success,
        first_agent_stderr: first_wire.stderr,
        second_agent_success: second_wire.success,
        second_agent_stdout: second_wire.stdout,
        second_agent_stderr: second_wire.stderr,
    })
}

struct WireClientOutcome {
    success: bool,
    stdout: String,
    stderr: String,
}

/// Minimal wire-level Agent used only by the dispatcher scenarios. It follows
/// the existing Grant/ACK fields and nonce/signature rules, but lets the test
/// deliberately corrupt only the first ACK while still sending attempt=1 on
/// the second connection. No Agent production code is changed for this fault
/// injection.
fn run_wire_agent_client(
    address: &str,
    fixture: &Fixture,
    connection_attempt: u32,
    corrupt_signature: bool,
) -> Result<WireClientOutcome, String> {
    use gputeer_crypto::Clock;
    use std::io::Write;
    use std::time::Duration;

    let mut stream = std::net::TcpStream::connect(address)
        .map_err(|e| format!("wire Agent connect failed: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("wire Agent read timeout setup failed: {e}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("wire Agent write timeout setup failed: {e}"))?;

    let coordinator_key =
        gputeer_crypto::SigningKey::from_bytes(&fixture.coordinator_seed).verifying_key();
    let mut coordinator_keys = gputeer_crypto::InMemoryKeyring::new();
    coordinator_keys.insert(fixture.coordinator_device_id.to_owned(), coordinator_key);
    let mut replay = gputeer_crypto::InMemoryReplayGuard::new();
    let clock = gputeer_crypto::SystemClock;
    let received = gputeer_crypto::read_frame(
        &mut stream,
        2,
        gputeer_crypto::KeyDirectorySource::Provided(&coordinator_keys),
        &mut replay,
        &clock,
    )
    .map_err(|e| format!("wire Agent Grant read failed: {e}"))?;
    let grant = match &received {
        gputeer_crypto::IngressMessage::Grant(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("wire Agent Grant replay failed: {e:?}"))?
            .clone(),
        other => return Err(format!("wire Agent expected Grant, got {other:?}")),
    };
    if grant.nonce != derive_selftest_nonce("grant", &grant.grant_id, connection_attempt) {
        return Err(format!(
            "wire Agent Grant nonce mismatch at attempt {connection_attempt}"
        ));
    }

    let now = clock.now_unix_ms();
    let mut ack = gputeer_protocol::pb::AgentGrantAck {
        schema_version: 1,
        grant_id: grant.grant_id.clone(),
        attempt_id: grant.attempt_id.clone(),
        agent_device_id: fixture.agent_device_id.to_owned(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_selftest_nonce("grant-ack", &grant.grant_id, connection_attempt),
        accepted: true,
        ..Default::default()
    };
    let agent_key = gputeer_crypto::SigningKey::from_bytes(&fixture.agent_seed);
    ack.agent_signature = gputeer_crypto::sign(&agent_key, &ack).to_vec();
    if corrupt_signature {
        let last = ack
            .agent_signature
            .last_mut()
            .ok_or_else(|| "wire Agent signature is empty".to_string())?;
        *last ^= 0x01;
    }
    let frame =
        gputeer_crypto::write_frame(gputeer_crypto::FrameType::GrantAck, &ack.encode_to_vec())
            .map_err(|e| format!("wire Agent ACK encode failed: {e}"))?;
    stream
        .write_all(&frame)
        .map_err(|e| format!("wire Agent ACK write failed: {e}"))?;
    stream
        .flush()
        .map_err(|e| format!("wire Agent ACK flush failed: {e}"))?;

    Ok(WireClientOutcome {
        success: !corrupt_signature,
        stdout: if corrupt_signature {
            String::new()
        } else {
            format!("{RESULT_OK_MARKER} wire_agent=true\n")
        },
        stderr: if corrupt_signature {
            "wire Agent intentionally sent a corrupt signature".to_string()
        } else {
            String::new()
        },
    })
}

fn derive_selftest_nonce(tag: &str, id: &str, connection_attempt: u32) -> Vec<u8> {
    let mut input = Vec::with_capacity(tag.len() + 1 + id.len() + 4);
    input.extend_from_slice(tag.as_bytes());
    input.push(0);
    input.extend_from_slice(id.as_bytes());
    if connection_attempt != 0 {
        input.extend_from_slice(&connection_attempt.to_be_bytes());
    }
    gputeer_protocol::canonical::blake3_256(&input)[..16].to_vec()
}

fn wait_child_with_drain(mut child: Child, deadline: Instant) -> Result<Output, String> {
    let stdout = child.stdout.take().expect("piped child stdout");
    let stderr = child.stderr.take().expect("piped child stderr");
    let stdout_reader = thread::spawn(move || -> Result<Vec<u8>, std::io::Error> {
        let mut output = Vec::new();
        let mut reader = stdout;
        reader.read_to_end(&mut output)?;
        Ok(output)
    });
    let stderr_reader = thread::spawn(move || -> Result<Vec<u8>, std::io::Error> {
        let mut output = Vec::new();
        let mut reader = stderr;
        reader.read_to_end(&mut output)?;
        Ok(output)
    });
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("child wait failed: {e}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("child exceeded 120-second hard deadline".to_string());
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "child stdout reader panicked".to_string())?
        .map_err(|e| format!("child stdout read failed: {e}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "child stderr reader panicked".to_string())?
        .map_err(|e| format!("child stderr read failed: {e}"))?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn run_coordinator_startup_case(
    fixture: &Fixture,
    extra_coordinator_args: &[&str],
) -> Result<CoordinatorOnlyOutcome, String> {
    let coordinator_seed_hex = to_hex(&fixture.coordinator_seed);
    let mut args: Vec<String> = vec![
        "coordinator-stub",
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        coordinator_seed_hex.as_str(),
        "--peer-pubkey",
        fixture.agent_pub_hex.as_str(),
        "--coordinator-device-id",
        fixture.coordinator_device_id,
        "--agent-device-id",
        fixture.agent_device_id,
        "--grant-id",
        fixture.grant_id,
        "--attempt-id",
        fixture.attempt_id,
        "--lease-id",
        fixture.lease_id,
        "--job-id",
        fixture.job_id,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    args.extend(extra_coordinator_args.iter().map(|arg| (*arg).to_owned()));
    let coordinator = Command::new(&fixture.exe)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("startup storage Coordinator spawn failed: {e}"))?;
    let output = wait_child_with_drain(coordinator, Instant::now() + Duration::from_secs(120))?;
    Ok(CoordinatorOnlyOutcome {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        agent_success: false,
        agent_exit_code: None,
        agent_elapsed_ms: 0,
        agent_stdout: String::new(),
        agent_stderr: String::new(),
    })
}

struct CoordinatorOnlyOutcome {
    success: bool,
    stdout: String,
    stderr: String,
    agent_success: bool,
    agent_exit_code: Option<i32>,
    agent_elapsed_ms: u128,
    agent_stdout: String,
    agent_stderr: String,
}

/// Negative test helper with independent hard timeouts so a regression that
/// binds, waits for accept, or makes Agent retry forever cannot hang the
/// complete selftest.
fn run_coordinator_without_legacy_opt_in(
    fixture: &Fixture,
) -> Result<CoordinatorOnlyOutcome, String> {
    let coordinator_own_seed_hex = to_hex(&fixture.coordinator_seed);
    let args = [
        "coordinator-stub",
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        coordinator_own_seed_hex.as_str(),
        "--peer-pubkey",
        fixture.agent_pub_hex.as_str(),
        "--coordinator-device-id",
        fixture.coordinator_device_id,
        "--agent-device-id",
        fixture.agent_device_id,
        "--grant-id",
        fixture.grant_id,
        "--attempt-id",
        fixture.attempt_id,
        "--lease-id",
        fixture.lease_id,
        "--job-id",
        fixture.job_id,
    ];
    let mut coordinator = Command::new(&fixture.exe)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("legacy negative test Coordinator spawn 실패: {e}"))?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if coordinator
            .try_wait()
            .map_err(|e| format!("legacy negative test Coordinator 상태 확인 실패: {e}"))?
            .is_some()
        {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = coordinator.kill();
            let _ = coordinator.wait();
            return Err(
                "legacy opt-in negative test가 5초 하드 타임아웃 안에 종료되지 않았다".to_string(),
            );
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }

    let output = coordinator
        .wait_with_output()
        .map_err(|e| format!("legacy negative test Coordinator 출력 수집 실패: {e}"))?;

    // The Coordinator rejected the legacy configuration before binding, so
    // this is deliberately an unbound address. Agent must fail closed rather
    // than retrying or waiting forever when connect() is refused.
    let mut agent_args: Vec<&str> = vec!["agent-stub", "--connect", "127.0.0.1:0", "--own-seed"];
    let agent_own_seed_hex = to_hex(&fixture.agent_seed);
    agent_args.push(&agent_own_seed_hex);
    agent_args.push("--peer-pubkey");
    agent_args.push(&fixture.coordinator_pub_hex);
    agent_args.push("--coordinator-device-id");
    agent_args.push(fixture.coordinator_device_id);
    agent_args.push("--agent-device-id");
    agent_args.push(fixture.agent_device_id);

    let mut agent = Command::new(&fixture.exe)
        .args(&agent_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("legacy negative test Agent spawn 실패: {e}"))?;
    let agent_started = std::time::Instant::now();
    let agent_deadline = agent_started + std::time::Duration::from_secs(5);
    loop {
        if agent
            .try_wait()
            .map_err(|e| format!("legacy negative test Agent 상태 확인 실패: {e}"))?
            .is_some()
        {
            break;
        }
        if std::time::Instant::now() >= agent_deadline {
            let _ = agent.kill();
            let _ = agent.wait();
            return Err(
                "legacy opt-in negative test Agent가 5초 하드 타임아웃 안에 종료되지 않았다"
                    .to_string(),
            );
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    let agent_elapsed_ms = agent_started.elapsed().as_millis();
    let agent_output = agent
        .wait_with_output()
        .map_err(|e| format!("legacy negative test Agent 출력 수집 실패: {e}"))?;

    Ok(CoordinatorOnlyOutcome {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        agent_success: agent_output.status.success(),
        agent_exit_code: agent_output.status.code(),
        agent_elapsed_ms,
        agent_stdout: String::from_utf8_lossy(&agent_output.stdout).into_owned(),
        agent_stderr: String::from_utf8_lossy(&agent_output.stderr).into_owned(),
    })
}

/// 시나리오 18 전용 — Agent 하나만 단독으로 띄워 `--fence-db :memory:`
/// 가 네트워크 연결조차 시도하기 전에 fail closed 하는지 확인한다.
/// Coordinator 는 필요 없다 — `DurableFenceWatermark::is_durable()`
/// 검사가 `TcpStream::connect()` 보다 먼저 실행되므로(`crates/agent/src/lib.rs`),
/// `--connect` 주소가 실제로 열려 있지 않아도 상관없다.
fn run_agent_alone(
    fixture: &Fixture,
    extra_agent_args: &[&str],
) -> Result<(bool, String, String), String> {
    let mut agent_args: Vec<&str> = vec!["agent-stub", "--connect", "127.0.0.1:1", "--own-seed"];
    let agent_own_seed_hex = to_hex(&fixture.agent_seed);
    agent_args.push(&agent_own_seed_hex);
    agent_args.push("--peer-pubkey");
    agent_args.push(&fixture.coordinator_pub_hex);
    agent_args.push("--coordinator-device-id");
    agent_args.push(fixture.coordinator_device_id);
    agent_args.push("--agent-device-id");
    agent_args.push(fixture.agent_device_id);
    agent_args.extend_from_slice(extra_agent_args);

    let output = Command::new(&fixture.exe)
        .args(&agent_args)
        .output()
        .map_err(|e| format!("agent-stub 단독 실행 실패: {e}"))?;

    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn seed_from_label(label: &str) -> [u8; 32] {
    gputeer_protocol::canonical::blake3_256(label.as_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
