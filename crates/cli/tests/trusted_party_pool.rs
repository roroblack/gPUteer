//! **신뢰망 파티 무인 운영 — Coordinator 한 번 · Agent 둘 · 작업 셋이 사람 손 없이 끝난다.**
//!
//! ```text
//! coordinator-stub --pool-mode      계속 떠 있다. Hello 로 누구인지 알고 그 노드의 배정으로 Grant 를 만든다
//! scheduler-loop --silent-after-ms   큐를 돌며 **소식이 있는** 빈 노드에 작업을 예약한다
//! agent-loop × 2                     되풀이해 붙는다 — 일이 있으면 실행 · 보고, 없으면 기다렸다 다시
//! ```
//!
//! ★ 2026-09-23 (신뢰망 남은 일 K · C). 전에는 작업마다 Coordinator 를 새로 띄워야 했다
//!   (`trusted_party_loop.rs` 가 그렇게 돈다) — 사람이 지켜보지 않는 운영이 아니었다.
//!
//! ★ **Windows 전용이다** — 실행 관문이 리눅스에서는 cgroup 위임을 요구한다(`grant_over_wire.rs` 와 같다).
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use gputeer_coordinator::job_store::{CoordinatorJobStore, JobState};
use gputeer_crypto::SigningKey;

const NODE_1: &str = "01JPOOLNODE000000000001";
const NODE_2: &str = "01JPOOLNODE000000000002";
const AGENT_SEED_1: &str = "b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1";
const AGENT_SEED_2: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const SUBMITTER: &str = "01JSUBMITTERPOOL00000001";
const SEED: &str = "99999999999999999999999999999999999999999999999999999999999999ce";
const OWNER: &str = "owner-pool";
const COORDINATOR: &str = "01JCOORDINATORPOOL000001";
const COORD_SEED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaf1";
const AXES: &str = "vram,gpu_count,cpu,ram,workspace";
const JOBS: [&str; 3] = [
    "01JJOBPOOL0000000000001",
    "01JJOBPOOL0000000000002",
    "01JJOBPOOL0000000000003",
];

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("gputeer.exe")
}

fn seed_bytes(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("seed hex");
    }
    out
}

fn pub_hex(seed: &str) -> String {
    SigningKey::from_bytes(&seed_bytes(seed))
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

fn run_cli(args: &[&str]) -> (bool, String) {
    let out = Command::new(cli_bin())
        .args(args)
        .output()
        .expect("gputeer 실행");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// 노드 둘을 등록하고 작업 셋을 LOCAL 내구성으로 큐에 올린다.
fn pool(dir: &Path) -> (PathBuf, PathBuf) {
    let db = dir.join("control.sqlite3");
    let observed = now_unix_ms();
    let agent = |node: &str, seed: &str| {
        format!(
            r#"{{
      "registry": {{
        "node_id": "{node}", "device_id": "{node}",
        "owner_member_id": "{OWNER}", "verifying_key_hex": "{key}",
        "node_state": "ONLINE", "risk_state": "NORMAL",
        "security_tier": "S2", "isolation_class": "CONTAINED",
        "key_protection": "K1"
      }},
      "inventory": {{
        "inventory_revision": 1, "observed_at_unix_ms": {observed},
        "gpus": [{{ "gpu_id": "{node}-gpu-0", "model": "RTX 4070 SUPER",
                    "healthy": true, "available_vram_bytes": 12884901888 }}],
        "available_cpu_cores": 16, "available_ram_bytes": 34359738368,
        "available_workspace_bytes": 107374182400,
        "allowed_workload_classes": ["TRAINING"],
        "third_party_workloads_opt_in": true
      }}
    }}"#,
            key = pub_hex(seed)
        )
    };
    let bootstrap = dir.join("bootstrap.json");
    std::fs::write(
        &bootstrap,
        format!(
            "{{\n  \"schema_version\": 1,\n  \"agents\": [\n{},\n{}\n  ]\n}}",
            agent(NODE_1, AGENT_SEED_1),
            agent(NODE_2, AGENT_SEED_2)
        ),
    )
    .expect("문서 쓰기");
    let (ok, out) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory 실패: {out}");

    let keyring = dir.join("submitters.keyring");
    let mut ring = gputeer_crypto::PersistentKeyring::new(
        &keyring,
        gputeer_crypto::KeyProtection::K0Plaintext,
        gputeer_crypto::PlaintextPolicy::Allow,
    )
    .expect("keyring 생성");
    ring.insert_public(
        SUBMITTER,
        SigningKey::from_bytes(&seed_bytes(SEED)).verifying_key(),
    )
    .expect("공개키 등록");
    ring.save().expect("keyring 저장");

    let entrypoint = cmd_exe();
    for (index, job) in JOBS.iter().enumerate() {
        let manifest = dir.join(format!("{job}.pb"));
        let issued = now_unix_ms().saturating_sub(60_000).to_string();
        let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
        let (ok, out) = run_cli(&[
            "submit",
            "--job-id",
            job,
            "--entrypoint",
            &entrypoint,
            "--args",
            "/c,exit,0",
            "--submitter-device-id",
            SUBMITTER,
            "--submitter-seed",
            SEED,
            "--issued-at-unix-ms",
            &issued,
            "--expires-at-unix-ms",
            &expires,
            "--out",
            manifest.to_str().unwrap(),
            "--workload-class",
            "TRAINING",
            "--side-effect-class",
            "PURE",
            "--durability",
            "LOCAL",
            "--dataset-sensitivity",
            "INTERNAL",
            "--minimum-security-tier",
            "S2",
            "--minimum-isolation-class",
            "CONTAINED",
            "--minimum-key-protection",
            "K1",
            "--gpu-count",
            "1",
            "--gpu-min-vram-bytes",
            "8589934592",
            "--cpu-cores",
            "4",
            "--ram-bytes",
            "8589934592",
            "--workspace-bytes",
            "10737418240",
        ]);
        assert!(ok, "submit 실패: {out}");
        let idempotency = format!("{:02x}{}", index + 1, "0102030405060708090a0b0c0d0e0f");
        let (ok, out) = run_cli(&[
            "import-manifest",
            "--manifest",
            manifest.to_str().unwrap(),
            "--submitter-keyring",
            keyring.to_str().unwrap(),
            "--job-db",
            db.to_str().unwrap(),
            "--idempotency-key",
            &idempotency,
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ]);
        assert!(ok, "import-manifest 실패: {out}");
        let (ok, out) = run_cli(&[
            "plan-job",
            "--job-id",
            job,
            "--control-db",
            db.to_str().unwrap(),
            "--submitter-keyring",
            keyring.to_str().unwrap(),
            "--submitter-member",
            OWNER,
            "--max-snapshot-age-ms",
            "86400000",
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ]);
        assert!(ok, "plan-job 실패: {out}");
    }
    (db, keyring)
}

fn spawn(args: &[String]) -> Child {
    Command::new(cli_bin())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn")
}

/// 자식을 끝내고 출력을 모은다.
fn collect(mut child: Child) -> String {
    let _ = child.kill();
    let output = child.wait_with_output().expect("출력 수집");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn job_state(db: &Path, job: &str) -> Option<JobState> {
    CoordinatorJobStore::open(db)
        .ok()?
        .get(job)
        .ok()
        .flatten()
        .map(|job| job.state)
}

/// Coordinator 의 주소를 알아낸다 — READY 줄을 쓰는 파일에서 읽는다(파이프는 끝까지 모아야 하므로).
fn wait_ready(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Some(line) = text.lines().find(|line| line.starts_with("READY ")) {
                return line.trim_start_matches("READY ").trim().to_string();
            }
        }
        assert!(Instant::now() < deadline, "Coordinator 가 READY 를 안 냈다");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn one_long_lived_coordinator_and_two_agents_finish_three_jobs_unattended() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, keyring) = pool(dir.path());
    let db_s = db.to_str().unwrap().to_string();
    let keyring_s = keyring.to_str().unwrap().to_string();

    // ── 풀 Coordinator — 한 번만 띄운다. 출력은 파일로(READY 를 읽으려고)
    let coordinator_log = dir.path().join("coordinator.log");
    let pool_agents = format!(
        "{NODE_1}={};{NODE_2}={}",
        pub_hex(AGENT_SEED_1),
        pub_hex(AGENT_SEED_2)
    );
    // ★ J — 운영은 시드를 명령줄이 아니라 파일로 준다(프로세스 목록에 비밀이 드러나지 않게).
    let coordinator_seed_file = dir.path().join("coordinator.seed");
    std::fs::write(&coordinator_seed_file, COORD_SEED).expect("시드 파일");
    let coordinator = Command::new(cli_bin())
        .args([
            "coordinator-stub",
            "--pool-mode",
            "true",
            "--pool-agents",
            &pool_agents,
            "--listen",
            "127.0.0.1:0",
            "--own-seed-file",
            coordinator_seed_file.to_str().unwrap(),
            "--coordinator-device-id",
            COORDINATOR,
            "--grant-from-control-db",
            &db_s,
            "--lease-db",
            &db_s,
            "--liveness-db",
            &db_s,
            "--submitter-keyring",
            &keyring_s,
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
            "--accept-report-sessions",
            "true",
            "--release-on-exit-report",
            "true",
            "--max-connections",
            "0",
            "--accept-timeout-ms",
            "0",
        ])
        .stdout(Stdio::from(
            std::fs::File::create(&coordinator_log).expect("log 파일"),
        ))
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator spawn");
    let addr = wait_ready(&coordinator_log);

    // ── 스케줄러 루프 — 소식이 있는 노드에만 준다
    let scheduler: Vec<String> = [
        "scheduler-loop",
        "--interval-ms",
        "100",
        "--max-ticks",
        "0",
        "--control-db",
        &db_s,
        "--submitter-keyring",
        &keyring_s,
        "--submitter-member",
        OWNER,
        "--max-snapshot-age-ms",
        "86400000",
        "--best-fit-axes",
        AXES,
        "--coordinator-id",
        COORDINATOR,
        "--coordinator-term",
        "3",
        "--lease-ttl-ms",
        "600000",
        "--lease-renew-after-ms",
        "300000",
        "--lease-max-total-duration-seconds",
        "86400",
        "--silent-after-ms",
        "60000",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let scheduler = spawn(&scheduler);

    // ── Agent 둘 — 되풀이해 붙는다
    let coordinator_pub = pub_hex(COORD_SEED);
    let submitter_pub = pub_hex(SEED);
    let agent_loop = |node: &str, seed: &str| -> Child {
        let fence = dir.path().join(format!("{node}-fence.sqlite3"));
        let checkpoints = dir.path().join(format!("{node}-checkpoints"));
        let args: Vec<String> = [
            "agent-loop",
            "--interval-ms",
            "100",
            "--max-rounds",
            "0",
            "--",
            "--connect",
            &addr,
            "--own-seed",
            seed,
            "--peer-pubkey",
            &coordinator_pub,
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            node,
            "--fence-db",
            fence.to_str().unwrap(),
            "--checkpoint-root",
            checkpoints.to_str().unwrap(),
            "--submitter-pubkey",
            &submitter_pub,
            "--i-understand-this-executes-untrusted-code",
            "true",
            "--report-over-session",
            "true",
            "--max-reconnect-attempts",
            "1",
            // ★ 결함 131 — 서명된 수신 확인을 받아야만 실행한다.
            "--require-ack-receipt",
            "true",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        spawn(&args)
    };
    let agent_1 = agent_loop(NODE_1, AGENT_SEED_1);
    let agent_2 = agent_loop(NODE_2, AGENT_SEED_2);

    // ── 사람 손 없이 셋 다 끝날 때까지 기다린다
    let deadline = Instant::now() + Duration::from_secs(120);
    let all_done = loop {
        let done = JOBS
            .iter()
            .all(|job| job_state(&db, job) == Some(JobState::Completed));
        if done || Instant::now() >= deadline {
            break done;
        }
        thread::sleep(Duration::from_millis(200));
    };

    let agent_1_out = collect(agent_1);
    let agent_2_out = collect(agent_2);
    let scheduler_out = collect(scheduler);
    let coordinator_err = collect(coordinator);
    let coordinator_out = std::fs::read_to_string(&coordinator_log).unwrap_or_default();
    let everything = format!(
        "--- agent 1 ---\n{agent_1_out}\n--- agent 2 ---\n{agent_2_out}\n--- scheduler ---\n{scheduler_out}\n--- coordinator ---\n{coordinator_out}\n{coordinator_err}"
    );
    let states: Vec<_> = JOBS.iter().map(|job| job_state(&db, job)).collect();
    assert!(
        all_done,
        "120초 안에 셋 다 끝나지 않았다: {states:?}\n{everything}"
    );

    // Coordinator 는 **한 번만** 떴고 여러 노드의 Hello 를 받았다.
    for node in [NODE_1, NODE_2] {
        assert!(
            coordinator_out.contains(&format!("SESSION_SEEN node_id={node}")),
            "{node} 의 Hello 를 생존 관측으로 적지 않았다\n{everything}"
        );
    }
    assert!(
        coordinator_out.contains("NO_WORK_FOR_NODE"),
        "일이 없는 노드에게 일을 지어내지 않는다는 기록이 없다(기다리는 회차가 한 번도 없었다)\n{everything}"
    );
    assert_eq!(
        coordinator_out.matches("RESERVATION_RELEASED").count(),
        3,
        "작업 셋의 보고가 예약을 셋 풀어야 한다\n{everything}"
    );
    // 두 노드가 **모두** 일했다 — 한 노드만 도는 것은 다중 Agent 운영이 아니다.
    for (label, out) in [("agent 1", &agent_1_out), ("agent 2", &agent_2_out)] {
        assert!(
            out.contains("outcome=worked"),
            "{label} 가 한 번도 일하지 않았다\n{everything}"
        );
    }
    // ★ 결함 131 — 두 Agent 모두 서명된 수신 확인을 검증한 뒤에 실행했다.
    for (label, out) in [("agent 1", &agent_1_out), ("agent 2", &agent_2_out)] {
        assert!(
            out.contains("ACK_RECEIPT_VERIFIED"),
            "{label} 가 수신 확인을 검증하지 않았다\n{everything}"
        );
    }
    // ★ J — 운영자가 보는 한 줄 요약이 사실과 같다.
    let (ok, status) = run_cli(&["status", "--control-db", &db_s]);
    assert!(ok, "status 실패: {status}");
    assert!(status.contains("SUMMARY completed=3"), "{status}");
    for node in [NODE_1, NODE_2] {
        assert!(
            status.contains(&format!("{node} reserved_by=-")),
            "끝났는데 {node} 의 예약이 보인다: {status}"
        );
    }
}

/// 음성 — **풀에 없는 노드의 Hello 는 거부되고**, Coordinator 는 계속 산다(다음 정상 노드를 받는다).
///
/// ★ 풀 목록에 없는 키로 서명한 Hello 는 검증에서 떨어진다 — "누구든 붙으면 일을 받는다" 가 아니다.
#[test]
fn a_node_outside_the_pool_is_refused_and_the_coordinator_keeps_serving() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, keyring) = pool(dir.path());
    let db_s = db.to_str().unwrap().to_string();
    let keyring_s = keyring.to_str().unwrap().to_string();
    let coordinator_log = dir.path().join("coordinator.log");
    // NODE_1 만 풀에 있다.
    let pool_agents = format!("{NODE_1}={}", pub_hex(AGENT_SEED_1));
    let coordinator = Command::new(cli_bin())
        .args([
            "coordinator-stub",
            "--pool-mode",
            "true",
            "--pool-agents",
            &pool_agents,
            "--listen",
            "127.0.0.1:0",
            "--own-seed",
            COORD_SEED,
            "--coordinator-device-id",
            COORDINATOR,
            "--grant-from-control-db",
            &db_s,
            "--lease-db",
            &db_s,
            "--liveness-db",
            &db_s,
            "--submitter-keyring",
            &keyring_s,
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
            "--accept-report-sessions",
            "true",
            "--max-connections",
            "0",
            "--accept-timeout-ms",
            "0",
        ])
        .stdout(Stdio::from(
            std::fs::File::create(&coordinator_log).expect("log 파일"),
        ))
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator spawn");
    let addr = wait_ready(&coordinator_log);
    let coordinator_pub = pub_hex(COORD_SEED);
    let submitter_pub = pub_hex(SEED);
    let one_round = |node: &str, seed: &str| -> String {
        let fence = dir.path().join(format!("{node}-fence.sqlite3"));
        let checkpoints = dir.path().join(format!("{node}-checkpoints"));
        let (_, out) = run_cli(&[
            "agent-loop",
            "--interval-ms",
            "0",
            "--max-rounds",
            "1",
            "--",
            "--connect",
            &addr,
            "--own-seed",
            seed,
            "--peer-pubkey",
            &coordinator_pub,
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            node,
            "--fence-db",
            fence.to_str().unwrap(),
            "--checkpoint-root",
            checkpoints.to_str().unwrap(),
            "--submitter-pubkey",
            &submitter_pub,
            "--i-understand-this-executes-untrusted-code",
            "true",
            "--report-over-session",
            "true",
            "--max-reconnect-attempts",
            "1",
        ]);
        out
    };
    let stranger = one_round(NODE_2, AGENT_SEED_2);
    let member = one_round(NODE_1, AGENT_SEED_1);
    thread::sleep(Duration::from_millis(300));
    let coordinator_err = collect(coordinator);
    let log = std::fs::read_to_string(&coordinator_log).unwrap_or_default();
    let everything = format!(
        "--- stranger ---\n{stranger}\n--- member ---\n{member}\n--- coordinator ---\n{log}\n{coordinator_err}"
    );
    assert!(
        !log.contains(&format!("SESSION_SEEN node_id={NODE_2}")),
        "풀에 없는 노드를 생존 관측으로 적었다\n{everything}"
    );
    assert!(
        coordinator_err.contains("HELLO_REJECTED") || coordinator_err.contains("HELLO_MISSING"),
        "풀에 없는 노드의 Hello 를 거부했다는 기록이 없다\n{everything}"
    );
    assert!(
        log.contains(&format!("SESSION_SEEN node_id={NODE_1}")),
        "거부 뒤에 Coordinator 가 다음 노드를 받지 않았다\n{everything}"
    );
}

/// 풀 Coordinator 를 띄우고, 서명된 Hello 바이트 하나를 보내고, 끈다. Coordinator 의 전체 출력을 돌려준다.
fn one_hello_against_a_pool_coordinator(
    dir: &Path,
    db: &Path,
    keyring: &Path,
    replay_db: Option<&Path>,
    hello_frame: &[u8],
    log_name: &str,
) -> String {
    use std::io::Write;
    let db_s = db.to_str().unwrap().to_string();
    let log = dir.join(log_name);
    let pool_agents = format!("{NODE_1}={}", pub_hex(AGENT_SEED_1));
    let mut args: Vec<String> = [
        "coordinator-stub",
        "--pool-mode",
        "true",
        "--pool-agents",
        &pool_agents,
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        COORD_SEED,
        "--coordinator-device-id",
        COORDINATOR,
        "--grant-from-control-db",
        &db_s,
        "--lease-db",
        &db_s,
        "--liveness-db",
        &db_s,
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
        "--accept-report-sessions",
        "true",
        "--max-connections",
        "1",
        "--accept-timeout-ms",
        "30000",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(replay_db) = replay_db {
        args.extend([
            "--replay-db".to_string(),
            replay_db.to_str().unwrap().to_string(),
        ]);
    }
    let coordinator = Command::new(cli_bin())
        .args(&args)
        .stdout(Stdio::from(std::fs::File::create(&log).unwrap()))
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator spawn");
    let addr = wait_ready(&log);
    let mut stream = std::net::TcpStream::connect(&addr).expect("연결");
    stream.write_all(hello_frame).expect("Hello 쓰기");
    stream.flush().ok();
    let output = coordinator.wait_with_output().expect("Coordinator 종료");
    format!(
        "{}{}",
        std::fs::read_to_string(&log).unwrap_or_default(),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// ★ 2026-09-23 (결함 88 조각 3, 신뢰망 L) — 풀 모드 `--replay-db` 는 **Coordinator 를 다시 띄워도** 같은 Hello 바이트를 거부한다.
///
/// 대조군이 같은 시험 안에 있다 — `--replay-db` 없이는 재시작 뒤 같은 바이트가 받아들여진다(메모리 방어는 재시작하면 잊는다).
#[test]
fn a_replayed_hello_is_refused_across_a_pool_coordinator_restart() {
    use prost::Message;
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, keyring) = pool(dir.path());
    let key = SigningKey::from_bytes(&seed_bytes(AGENT_SEED_1));
    let mut hello = gputeer_protocol::pb::AgentSessionHello {
        schema_version: 1,
        mode: gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT,
        session_id: "replay-probe".into(),
        node_id: NODE_1.into(),
        connection_attempt: 0,
        issued_at_unix_ms: now_unix_ms(),
        nonce: vec![0x5A; 16],
        ..Default::default()
    };
    hello.node_signature = gputeer_crypto::sign(&key, &hello).to_vec();
    let frame = gputeer_crypto::write_frame(
        gputeer_crypto::FrameType::SessionHello,
        &hello.encode_to_vec(),
    )
    .expect("프레임");

    // 대조 — 영속 방어 없이는 재시작 뒤 같은 바이트를 받는다.
    let first =
        one_hello_against_a_pool_coordinator(dir.path(), &db, &keyring, None, &frame, "c1.log");
    let second =
        one_hello_against_a_pool_coordinator(dir.path(), &db, &keyring, None, &frame, "c2.log");
    assert!(first.contains("SESSION_HELLO_ACCEPTED"), "{first}");
    assert!(
        second.contains("SESSION_HELLO_ACCEPTED"),
        "대조군이 성립하지 않는다 — 메모리 방어는 재시작 뒤 같은 Hello 를 받아야 한다: {second}"
    );

    // 영속 방어 — 재시작 뒤 같은 바이트를 거부한다.
    let replay_db = dir.path().join("replay.sqlite3");
    let third = one_hello_against_a_pool_coordinator(
        dir.path(),
        &db,
        &keyring,
        Some(&replay_db),
        &frame,
        "c3.log",
    );
    let fourth = one_hello_against_a_pool_coordinator(
        dir.path(),
        &db,
        &keyring,
        Some(&replay_db),
        &frame,
        "c4.log",
    );
    assert!(third.contains("SESSION_HELLO_ACCEPTED"), "{third}");
    assert!(
        !fourth.contains("SESSION_HELLO_ACCEPTED") && fourth.contains("HELLO_REJECTED"),
        "재시작 뒤 같은 Hello 를 받아들였다: {fourth}"
    );

    // 풀 모드가 아니면 --replay-db 를 거부한다(유도 nonce 가 재시작 뒤 겹친다).
    let (ok, out) = run_cli(&[
        "coordinator-stub",
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        COORD_SEED,
        "--peer-pubkey",
        &pub_hex(AGENT_SEED_1),
        "--coordinator-device-id",
        COORDINATOR,
        "--agent-device-id",
        NODE_1,
        "--grant-id",
        "g",
        "--attempt-id",
        "a",
        "--lease-id",
        "l",
        "--job-id",
        "j",
        "--lease-db",
        db.to_str().unwrap(),
        "--replay-db",
        replay_db.to_str().unwrap(),
    ]);
    assert!(!ok && out.contains("REPLAY_DB_NEEDS_POOL_MODE"), "{out}");
}
