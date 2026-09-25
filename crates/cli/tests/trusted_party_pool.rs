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
    pool_with(dir, ["LOCAL"; 3])
}

fn pool_with(dir: &Path, durabilities: [&str; 3]) -> (PathBuf, PathBuf) {
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
            durabilities[index],
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
            // 결함 218 — 풀 Agent 는 실행 중 갱신을 켜야 시작한다(첫 갱신이 "실행을 시작했다" 신호다).
            "--renew-during-execution-ms",
            "1000",
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
    // ★ 2026-09-25 — 대시보드(읽기 전용 · 127.0.0.1)가 같은 사실을 JSON 으로 낸다.
    let json = dashboard_get(&db_s, "/api/status", "127.0.0.1");
    assert!(json.starts_with("HTTP/1.1 200"), "{json}");
    let body: serde_json::Value =
        serde_json::from_str(json.split("\r\n\r\n").nth(1).unwrap_or("")).expect("JSON");
    assert_eq!(body["summary"]["completed"], 3, "{body}");
    let nodes = body["nodes"].as_array().expect("노드 목록");
    assert_eq!(nodes.len(), 2, "{body}");
    assert!(nodes.iter().all(|n| n["reserved_by"].is_null()), "{body}");
    assert_eq!(body["jobs"].as_array().map(Vec::len), Some(3), "{body}");
    // 음성 — Host 가 loopback 이 아니면(DNS 리바인딩) 거부 · 쓰기 요청 거부.
    let rebound = dashboard_get(&db_s, "/api/status", "attacker.example");
    assert!(rebound.starts_with("HTTP/1.1 403"), "{rebound}");
}

/// 대시보드를 포트 0 으로 띄워 요청 하나를 보내고 응답 전체를 돌려준다(`--max-requests 1`).
fn dashboard_get(db: &str, path: &str, host: &str) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut child = Command::new(cli_bin())
        .args([
            "dashboard",
            "--control-db",
            db,
            "--port",
            "0",
            "--max-requests",
            "1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("dashboard");
    let mut first = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut first)
        .unwrap();
    let address = first
        .trim()
        .strip_prefix("DASHBOARD_LISTENING http://")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap_or_else(|| panic!("주소 줄이 아니다: {first:?}"))
        .to_string();
    let mut stream = std::net::TcpStream::connect(&address).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "dashboard 가 요청 하나 뒤 정상으로 끝나지 않았다"
    );
    response
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
            // 풀에 붙는 Agent 는 실행 중 갱신을 켠다(런북 §5).
            "--renew-during-execution-ms",
            "1000",
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
        // ★ 결함 417 (2026-09-25) — 풀 DB 를 주면 이제 풀 표식 검사가 먼저 거부한다(POOL_DB_WITHOUT_POOL_MODE). 이 줄이 재는 것은
        //   "비풀 + --replay-db" 이므로 풀이 아닌 Lease DB 를 준다.
        "--lease-db",
        dir.path().join("unpooled-lease.sqlite3").to_str().unwrap(),
        "--replay-db",
        replay_db.to_str().unwrap(),
    ]);
    assert!(!ok && out.contains("REPLAY_DB_NEEDS_POOL_MODE"), "{out}");
}

/// 결함 217 · 235 · 236 (검수 73 · 재검수 79) — 풀 스케줄러는 풀이 채울 수 없는 내구성(LOCAL 아님)을 요구한 작업을 **배치 전에** 내리고,
/// 그 뒤의 작업을 계속 본다(맨 앞이 뒤를 인질로 잡지 않는다). 풀 여부는 **풀 Coordinator 가 control DB 에 적은 표식**으로 안다.
/// 대조군 — 표식이 없는 DB 는 `--pool-agents` 를 줘도 풀로 보지 않는다(이어받기 키로 쓰는 풀 밖 운영을 바꾸지 않는다).
#[test]
fn a_pool_scheduler_drops_a_job_whose_durability_the_pool_cannot_meet() {
    let tick = |db: &Path, keyring: &Path, pool_agents: Option<&str>| -> (bool, String) {
        let db_s = db.to_str().unwrap().to_string();
        let keyring_s = keyring.to_str().unwrap().to_string();
        let mut args = vec![
            "scheduler-tick",
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
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ];
        if let Some(agents) = pool_agents {
            args.push("--pool-agents");
            args.push(agents);
        }
        run_cli(&args)
    };
    let agents = format!(
        "{NODE_1}={};{NODE_2}={}",
        pub_hex(AGENT_SEED_1),
        pub_hex(AGENT_SEED_2)
    );

    // 풀 — Coordinator 가 적는 표식을 시험이 직접 적는다. 인자에는 --pool-agents 가 **없다**(235 A).
    let dir = tempfile::tempdir().expect("임시 폴더");
    let (db, keyring) = pool_with(dir.path(), ["MIRRORED", "LOCAL", "LOCAL"]);
    gputeer_coordinator::job_store::declare_pool_mode(&db, now_unix_ms()).expect("풀 표식");
    let (ok, out) = tick(&db, &keyring, None);
    assert!(ok, "{out}");
    assert!(
        out.contains(&format!(
            "TICK_JOB_FAILED_PERMANENTLY_INFEASIBLE {}",
            JOBS[0]
        )) && out.contains("DURABILITY_MIRRORED_NOT_SUPPORTED_BY_POOL"),
        "{out}"
    );
    assert_eq!(job_state(&db, JOBS[0]), Some(JobState::Failed));
    assert_eq!(
        job_state(&db, JOBS[1]),
        Some(JobState::Staging),
        "맨 앞을 내린 뒤 같은 tick 이 다음 작업을 배치하지 않았다(236)\n{out}"
    );

    // 결함 249 — 표식이 아직 없어도(Coordinator 가 먼저 안 떴다) 스케줄러가 `--pool-mode true` 를 명시하면 풀로 본다.
    let early = tempfile::tempdir().expect("임시 폴더");
    let (db, keyring) = pool_with(early.path(), ["MIRRORED", "LOCAL", "LOCAL"]);
    let db_s = db.to_str().unwrap().to_string();
    let keyring_s = keyring.to_str().unwrap().to_string();
    let (ok, out) = run_cli(&[
        "scheduler-tick",
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
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
        "--pool-mode",
        "true",
    ]);
    assert!(ok, "{out}");
    assert_eq!(
        job_state(&db, JOBS[0]),
        Some(JobState::Failed),
        "표식 전 스케줄러가 비-LOCAL 을 배치했다(249)\n{out}"
    );

    // 대조군 — 표식 없는 DB 에 --pool-agents 를 줘도 거부하지 않는다(235 B).
    let control = tempfile::tempdir().expect("임시 폴더");
    let (db, keyring) = pool_with(control.path(), ["MIRRORED"; 3]);
    let (ok, out) = tick(&db, &keyring, Some(&agents));
    assert!(ok, "풀 밖 tick 이 거부했다: {out}");
    assert_eq!(job_state(&db, JOBS[0]), Some(JobState::Staging));
}

/// 결함 221 (검수 74) — 풀이 받지 않는 mode 의 Hello 는 **생존 관측으로 적기 전에** 거부한다. 대조군: FRESH 는 적힌다.
#[test]
fn a_hello_the_pool_cannot_serve_is_not_recorded_as_alive() {
    use prost::Message;
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, keyring) = pool(dir.path());
    let key = SigningKey::from_bytes(&seed_bytes(AGENT_SEED_1));
    let frame_for = |mode: i32, nonce: u8| {
        let mut hello = gputeer_protocol::pb::AgentSessionHello {
            schema_version: 1,
            mode,
            session_id: format!("mode-probe-{mode}"),
            node_id: NODE_1.into(),
            connection_attempt: 0,
            issued_at_unix_ms: now_unix_ms(),
            nonce: vec![nonce; 16],
            ..Default::default()
        };
        hello.node_signature = gputeer_crypto::sign(&key, &hello).to_vec();
        gputeer_crypto::write_frame(
            gputeer_crypto::FrameType::SessionHello,
            &hello.encode_to_vec(),
        )
        .expect("프레임")
    };
    let resume = one_hello_against_a_pool_coordinator(
        dir.path(),
        &db,
        &keyring,
        None,
        &frame_for(gputeer_protocol::constants::MODE_RESUME, 0x61),
        "resume.log",
    );
    assert!(resume.contains("HELLO_REJECTED"), "{resume}");
    assert!(
        !resume.contains("SESSION_SEEN"),
        "받지 못할 Hello 를 생존 관측으로 적었다\n{resume}"
    );
    // 결함 230 (검수 77) — 보고 연결(REPORT)도 생존 관측이 아니다. 되찾은 노드가 보고를 다시 보내도 후보로 돌아오지 않는다.
    let report = one_hello_against_a_pool_coordinator(
        dir.path(),
        &db,
        &keyring,
        None,
        &frame_for(gputeer_protocol::constants::MODE_REPORT, 0x63),
        "report.log",
    );
    assert!(
        !report.contains("SESSION_SEEN"),
        "보고 연결을 생존 관측으로 적었다\n{report}"
    );
    let fresh = one_hello_against_a_pool_coordinator(
        dir.path(),
        &db,
        &keyring,
        None,
        &frame_for(gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT, 0x62),
        "fresh.log",
    );
    assert!(fresh.contains("SESSION_SEEN"), "{fresh}");
}

/// 결함 219 · 220 (검수 74) — 같은 공개키를 두 노드에 주거나, 생존 DB 가 control DB 와 다르면 풀 Coordinator 가 시작하지 않는다.
#[test]
fn a_pool_coordinator_refuses_shared_keys_and_a_separate_liveness_db() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, keyring) = pool(dir.path());
    let db_s = db.to_str().unwrap().to_string();
    let other_db = dir.path().join("liveness-elsewhere.sqlite3");
    let other_s = other_db.to_str().unwrap().to_string();
    let start = |pool_agents: &str, liveness: &str| {
        run_cli(&[
            "coordinator-stub",
            "--pool-mode",
            "true",
            "--pool-agents",
            pool_agents,
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
            liveness,
            "--submitter-keyring",
            keyring.to_str().unwrap(),
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
            "--accept-report-sessions",
            "true",
            "--max-connections",
            "1",
            "--accept-timeout-ms",
            "1",
        ])
    };
    let shared = format!(
        "{NODE_1}={};{NODE_2}={}",
        pub_hex(AGENT_SEED_1),
        pub_hex(AGENT_SEED_1)
    );
    let (ok, out) = start(&shared, &db_s);
    assert!(!ok && out.contains("POOL_AGENTS_DUPLICATE_KEY"), "{out}");
    let distinct = format!(
        "{NODE_1}={};{NODE_2}={}",
        pub_hex(AGENT_SEED_1),
        pub_hex(AGENT_SEED_2)
    );
    let (ok, out) = start(&distinct, &other_s);
    assert!(!ok && out.contains("POOL_LIVENESS_DB_MISMATCH"), "{out}");
    // 대조군 — 둘 다 맞으면 시작 관문을 지난다(연결이 없어 accept 시한으로 끝난다).
    let (_, out) = start(&distinct, &db_s);
    assert!(
        !out.contains("STARTUP_REFUSED"),
        "맞는 설정인데 시작을 거부했다\n{out}"
    );
}

/// 수신 확인 유실 상황을 만들고 풀 Coordinator 에 같은 노드의 Agent 한 회차를 붙인다.
///
/// 유실 상황은 시도를 미리 STARTING 으로 적어 흉내 낸다(ACK 기록 직후 끊긴 것과 저장 상태가 같다).
/// ★ 같은 노드가 **이미 시작한** 시도를 다시 받지 않는 쪽(Agent 의 시작 기록)은 단위 시험이 본다
///   (`owner_reclaim_marker_tests::the_start_journal_remembers_exactly_the_attempts_started_here`).
fn lost_receipt_round() -> (String, String, Option<JobState>) {
    let dir = tempfile::tempdir().expect("임시 폴더");
    let (db, keyring) = pool(dir.path());
    let db_s = db.to_str().unwrap().to_string();
    let keyring_s = keyring.to_str().unwrap().to_string();
    let (ok, out) = run_cli(&[
        "scheduler-tick",
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
        "1000",
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "예약 실패: {out}");
    let staging = gputeer_coordinator::staging_store::CoordinatorStagingStore::open(&db).unwrap();
    let (node, seed, attempt_id) = [(NODE_1, AGENT_SEED_1), (NODE_2, AGENT_SEED_2)]
        .into_iter()
        .find_map(|(node, seed)| {
            staging
                .work_assigned_to_node(node)
                .unwrap()
                .map(|(_, attempt, _)| (node, seed, attempt))
        })
        .expect("어느 노드에도 배정이 없다");
    drop(staging);
    // ACK 는 기록됐고(시도 STARTING · Job RUNNING) 수신 확인만 유실됐다.
    let recorded = gputeer_coordinator::staging_store::CoordinatorStagingStore::open(&db)
        .unwrap()
        .record_grant_accepted(&attempt_id, now_unix_ms(), false)
        .unwrap();
    assert_eq!(
        recorded,
        gputeer_coordinator::staging_store::GrantAcceptedRecord::Recorded
    );
    // 결함 218 — ACK 는 Job 을 옮기지 않는다(첫 진행 신호가 옮긴다). 한 번도 안 돈 이 시도의 Job 은 STAGING 이다.
    assert_eq!(job_state(&db, JOBS[0]), Some(JobState::Staging));

    let pool_agents = format!(
        "{NODE_1}={};{NODE_2}={}",
        pub_hex(AGENT_SEED_1),
        pub_hex(AGENT_SEED_2)
    );
    let coordinator_log = dir.path().join("coordinator.log");
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
    let fence = dir.path().join(format!("{node}-fence.sqlite3"));
    let checkpoints = dir.path().join(format!("{node}-checkpoints"));
    let coordinator_pub = pub_hex(COORD_SEED);
    let submitter_pub = pub_hex(SEED);
    let agent: Vec<String> = [
        "agent-loop",
        "--interval-ms",
        "100",
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
        "--require-ack-receipt",
        "true",
        // 결함 218 — 풀 Agent 는 실행 중 갱신을 켜야 시작한다(첫 갱신이 "실행을 시작했다" 신호다).
        "--renew-during-execution-ms",
        "1000",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // 한 회차가 끝날 때까지 기다린다(collect 는 곧바로 죽인다).
    let agent_output = spawn(&agent).wait_with_output().expect("agent 출력");
    let agent_out = format!(
        "{}{}",
        String::from_utf8_lossy(&agent_output.stdout),
        String::from_utf8_lossy(&agent_output.stderr)
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while job_state(&db, JOBS[0]) != Some(JobState::Completed) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(200));
    }
    let finished = job_state(&db, JOBS[0]);
    let coordinator_err = collect(coordinator);
    let coordinator_out = std::fs::read_to_string(&coordinator_log).unwrap_or_default();
    (agent_out, coordinator_out + &coordinator_err, finished)
}

fn unconfirmed_start_round(coordinator_extra: &[&str]) -> (String, String, Option<JobState>) {
    pool_round(coordinator_extra, true)
}

/// `require_ack_receipt` 가 false 면 Agent 에서 `--require-ack-receipt true` 를 뺀다(결함 288 — 풀에 잘못 붙인 Agent).
fn pool_round(
    coordinator_extra: &[&str],
    require_ack_receipt: bool,
) -> (String, String, Option<JobState>) {
    let dir = tempfile::tempdir().expect("임시 폴더");
    let (db, keyring) = pool(dir.path());
    let db_s = db.to_str().unwrap().to_string();
    let keyring_s = keyring.to_str().unwrap().to_string();
    let (ok, out) = run_cli(&[
        "scheduler-tick",
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
        "1000",
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "예약 실패: {out}");
    let staging = gputeer_coordinator::staging_store::CoordinatorStagingStore::open(&db).unwrap();
    let (node, seed, _attempt_id) = [(NODE_1, AGENT_SEED_1), (NODE_2, AGENT_SEED_2)]
        .into_iter()
        .find_map(|(node, seed)| {
            staging
                .work_assigned_to_node(node)
                .unwrap()
                .map(|(_, attempt, _)| (node, seed, attempt))
        })
        .expect("어느 노드에도 배정이 없다");
    drop(staging);
    let pool_agents = format!(
        "{NODE_1}={};{NODE_2}={}",
        pub_hex(AGENT_SEED_1),
        pub_hex(AGENT_SEED_2)
    );
    let coordinator_log = dir.path().join("coordinator.log");
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
            "--release-on-exit-report",
            "true",
        ])
        .args(coordinator_extra)
        .args(["--accept-timeout-ms", "0"])
        .stdout(Stdio::from(
            std::fs::File::create(&coordinator_log).expect("log 파일"),
        ))
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator spawn");
    let addr = wait_ready(&coordinator_log);
    let fence = dir.path().join(format!("{node}-fence.sqlite3"));
    let checkpoints = dir.path().join(format!("{node}-checkpoints"));
    let coordinator_pub = pub_hex(COORD_SEED);
    let submitter_pub = pub_hex(SEED);
    let agent: Vec<String> = [
        "agent-loop",
        "--interval-ms",
        "100",
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
        "--require-ack-receipt",
        "true",
        // 결함 218 — 풀 Agent 는 실행 중 갱신을 켜야 시작한다(첫 갱신이 "실행을 시작했다" 신호다).
        "--renew-during-execution-ms",
        "1000",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let agent: Vec<String> = if require_ack_receipt {
        agent
    } else {
        let at = agent
            .iter()
            .position(|a| a == "--require-ack-receipt")
            .expect("플래그 자리");
        let mut agent = agent;
        agent.drain(at..at + 2);
        agent
    };
    // 한 회차가 끝날 때까지 기다린다(collect 는 곧바로 죽인다).
    let agent_output = spawn(&agent).wait_with_output().expect("agent 출력");
    let agent_out = format!(
        "{}{}",
        String::from_utf8_lossy(&agent_output.stdout),
        String::from_utf8_lossy(&agent_output.stderr)
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while job_state(&db, JOBS[0]) == Some(JobState::Queued) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(200));
    }
    let finished = job_state(&db, JOBS[0]);
    let coordinator_err = collect(coordinator);
    let coordinator_out = std::fs::read_to_string(&coordinator_log).unwrap_or_default();
    (agent_out, coordinator_out + &coordinator_err, finished)
}

/// ★ 2026-09-25 (결함 268 · 재검수 87) — 실행 전 갱신(= "시작한다" 확인)을 받지 못하면 **띄우지 않는다.** 전에는 갱신을 기다리지 않고
///   띄워, 실제로 도는 작업이 STAGING 으로 남아 Lease 만료 뒤 다른 노드에서 한 번 더 돌았다. 대조군은 무인 운영 시험(갱신이 되면 돈다).
#[test]
fn a_start_that_the_coordinator_did_not_confirm_does_not_run() {
    // FRESH 연결 하나(Grant · ACK · 수신 확인)만 받고 끝난다 — 실행 전 갱신 연결은 거부된다.
    let (agent_out, coordinator_out, state) = unconfirmed_start_round(&["--max-connections", "1"]);
    let everything = format!("--- agent ---\n{agent_out}\n--- coordinator ---\n{coordinator_out}");
    assert!(
        agent_out.contains("ACK_RECEIPT_VERIFIED"),
        "수신 확인까지 가지 못했다 — 시험의 전제가 깨졌다\n{everything}"
    );
    assert!(
        agent_out.contains("PROCESS_START_NOT_CONFIRMED"),
        "확인 없이 시작을 거부하지 않았다\n{everything}"
    );
    assert!(
        !agent_out.contains("WORKLOAD_SPAWNED") && !agent_out.contains("WORKLOAD_RESULT"),
        "확인 없이 워크로드를 띄웠다\n{everything}"
    );
    // 정말로 안 돈 시도다 — Job 은 STAGING 에 남아 Lease 만료 뒤 큐로 돌아간다.
    assert_eq!(state, Some(JobState::Staging), "{everything}");
}

/// ★ 2026-09-24 (결함 218 · 257 · 258 — 재발급 철회) — 수신 확인이 유실돼 STARTING 에 멈춘 시도를 **다시 내주지 않는다.**
///   재발급을 하면 "한 번도 안 돈 시도" 와 "돌고 있는 시도" 를 계약 없이 가를 수 없어 두 번 실행 경로가 생겼다(재검수 84).
///   ★ 2026-09-25 (결함 218 닫힘) — ACK 는 Job 을 옮기지 않으므로 이 Job 은 STAGING 에 남고, Lease 만료 뒤 큐로 돌아가 처음부터 돈다(FAILED 가 아니다).
#[test]
fn a_start_whose_ack_receipt_was_lost_is_not_handed_out_again() {
    let (agent_out, coordinator_out, finished) = lost_receipt_round();
    let everything = format!("--- agent ---\n{agent_out}\n--- coordinator ---\n{coordinator_out}");
    assert!(
        !agent_out.contains("ACK_RECEIPT_VERIFIED") && !agent_out.contains("WORKLOAD_RESULT"),
        "STARTING 시도를 다시 내줬다\n{everything}"
    );
    assert!(
        coordinator_out.contains("이미 받아들여졌다"),
        "거부 사유가 보이지 않는다\n{everything}"
    );
    // Lease 만료 뒤 이어받기가 STAGING_NODE_LOST 로 큐에 되돌린다(단위 시험 an_acknowledged_start_that_never_ran_goes_back_to_the_queue).
    assert_eq!(finished, Some(JobState::Staging), "{everything}");
}

/// 결함 218 (2026-09-25) — 풀 방식 Agent(보고 세션 + 수신 확인 요구)는 실행 중 갱신 없이 시작하지 않는다. 대조군: 갱신을 켜면 이 관문을 지난다.
#[test]
fn a_pool_agent_without_renewal_refuses_to_start() {
    let dir = tempfile::tempdir().expect("임시 폴더");
    let fence = dir.path().join("fence.sqlite3");
    let checkpoints = dir.path().join("checkpoints");
    let coordinator_pub = pub_hex(COORD_SEED);
    let base = |extra: &[&str]| {
        let mut args = vec![
            "agent-stub",
            "--connect",
            "127.0.0.1:9",
            "--own-seed",
            AGENT_SEED_1,
            "--peer-pubkey",
            &coordinator_pub,
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            NODE_1,
            "--fence-db",
            fence.to_str().unwrap(),
            "--checkpoint-root",
            checkpoints.to_str().unwrap(),
            "--i-understand-this-executes-untrusted-code",
            "true",
            "--report-over-session",
            "true",
            "--require-ack-receipt",
            "true",
            "--max-reconnect-attempts",
            "1",
        ];
        args.extend_from_slice(extra);
        run_cli(&args)
    };
    let (ok, out) = base(&[]);
    assert!(!ok && out.contains("POOL_AGENT_NEEDS_RENEW"), "{out}");
    let (_, out) = base(&["--renew-during-execution-ms", "1000"]);
    assert!(
        !out.contains("POOL_AGENT_NEEDS_RENEW"),
        "갱신을 켰는데 거부했다\n{out}"
    );
}

/// ★ 2026-09-25 (결함 289 · 293 · 재검수 90 · 91) — 실행 직전 갱신으로 **받은** Lease 가 5초도 안 남았으면 띄우지 않는다(다음 갱신 전에
///   만료돼 이어받기와 겹친다). ★ 그 대가를 사실로 고정한다 — Coordinator 는 그 갱신에서 이미 시작을 기록해 Job 이 RUNNING 이다. 체크포인트가
///   없으면 이어받기에서 FAILED 가 된다(두 번 도는 대신 가용성을 잃는 쪽을 골랐다).
#[test]
fn a_confirmed_lease_too_short_to_keep_alive_does_not_start() {
    let (agent_out, coordinator_out, state) =
        unconfirmed_start_round(&["--renew-extension-ms", "3000", "--max-connections", "0"]);
    let everything = format!("--- agent ---\n{agent_out}\n--- coordinator ---\n{coordinator_out}");
    assert!(
        agent_out.contains("LEASE_TOO_SHORT_AFTER_CONFIRM"),
        "받은 Lease 가 짧은데 시작을 거부하지 않았다\n{everything}"
    );
    assert!(
        !agent_out.contains("WORKLOAD_SPAWNED"),
        "짧은 Lease 로 워크로드를 띄웠다\n{everything}"
    );
    assert_eq!(state, Some(JobState::Running), "{everything}");
}

/// ★ 2026-09-25 (결함 288 · signing.md §6.5) — 수신 확인 없이 풀에 붙은 Agent 는 풀 Grant(v4 · pool_mode)를 **ACK 전에** 거부한다.
///   전에는 Agent 가 상대가 풀인지 몰라 ACK 하고 바로 실행했다 — 실행 전 확인 관문이 서지 않아 같은 시도가 두 번 돌 수 있었다.
///   대조군은 같은 조립에서 수신 확인을 켠 `a_start_that_the_coordinator_did_not_confirm_does_not_run`(Grant 를 받아 ACK 까지 간다).
#[test]
fn an_agent_without_ack_receipt_refuses_a_pool_grant_before_ack() {
    let (agent_out, coordinator_out, state) = pool_round(&["--max-connections", "1"], false);
    let everything = format!("--- agent ---\n{agent_out}\n--- coordinator ---\n{coordinator_out}");
    assert!(
        agent_out.contains("POOL_GRANT_NEEDS_ACK_RECEIPT"),
        "풀 Grant 를 수신 확인 없이 받았다\n{everything}"
    );
    assert!(
        !agent_out.contains("ACK_SENT") && !agent_out.contains("WORKLOAD_SPAWNED"),
        "거부하기 전에 ACK 하거나 워크로드를 띄웠다\n{everything}"
    );
    // 받아들여지지 않은 시도다 — Job 은 STAGING 에 남아 Lease 만료 뒤 큐로 돌아간다.
    assert_eq!(state, Some(JobState::Staging), "{everything}");
}

/// ★ 2026-09-25 (결함 301 · signing.md §6.6) — FRESH Hello 의 GPU 관측이 등록된 선언과 **맞을 때만** 그 노드가 신선해진다.
///   전에는 운영자가 다시 선언하지 않으면 하루 뒤 모든 노드가 SnapshotNotFresh 로 빠졌다. 선언과 다르면(다른 GPU) 기록하지 않고,
///   구조 규칙을 어긴 관측(v1 · FRESH 가 아닌 Hello)은 Hello 째 거부한다.
#[test]
fn a_matching_gpu_observation_keeps_a_node_fresh_and_a_different_gpu_does_not() {
    use prost::Message;
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, keyring) = pool(dir.path());
    let key = SigningKey::from_bytes(&seed_bytes(AGENT_SEED_1));
    let declared_at = || {
        let mut store =
            gputeer_coordinator::inventory_store::CoordinatorInventoryStore::open(&db).unwrap();
        store
            .pool_snapshot(now_unix_ms())
            .unwrap()
            .candidates
            .into_iter()
            .find(|c| c.node_id == NODE_1)
            .unwrap()
            .observed_at_unix_ms
            .unwrap()
    };
    let frame_for = |schema: u32, mode: i32, model: &str, nonce: u8| {
        let now = now_unix_ms();
        let mut hello = gputeer_protocol::pb::AgentSessionHello {
            schema_version: schema,
            mode,
            session_id: format!("gpu-attest-{nonce}"),
            node_id: NODE_1.into(),
            connection_attempt: 0,
            issued_at_unix_ms: now,
            nonce: vec![nonce; 16],
            gpu_observation: Some(gputeer_protocol::pb::NodeGpuObservation {
                observed_at_unix_ms: now - 1_000,
                gpus: vec![gputeer_protocol::pb::ObservedGpu {
                    uuid: "GPU-00000000-0000-0000-0000-000000000001".into(),
                    model: model.into(),
                    total_vram_bytes: 12_884_901_888,
                }],
            }),
            ..Default::default()
        };
        hello.node_signature = gputeer_crypto::sign(&key, &hello).to_vec();
        gputeer_crypto::write_frame(
            gputeer_crypto::FrameType::SessionHello,
            &hello.encode_to_vec(),
        )
        .expect("프레임")
    };
    let fresh = gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT;
    let before = declared_at();
    std::thread::sleep(Duration::from_millis(1_100));

    // 다른 GPU — 기록하지 않는다
    let other = one_hello_against_a_pool_coordinator(
        dir.path(),
        &db,
        &keyring,
        None,
        &frame_for(2, fresh, "RTX 3060", 0x71),
        "other.log",
    );
    assert!(other.contains("GPU_ATTESTATION_MISMATCH"), "{other}");
    assert!(!other.contains("GPU_ATTESTATION_RECORDED"), "{other}");
    assert_eq!(declared_at(), before, "맞지 않는 관측이 신선도를 늘렸다");

    // 구조 규칙 — v1 Hello 에 관측 · RENEW Hello 에 관측은 Hello 째 거부한다(생존 관측으로도 적지 않는다)
    for (schema, mode, label) in [
        (1, fresh, "v1.log"),
        (2, gputeer_protocol::constants::MODE_RENEW, "renew.log"),
    ] {
        let out = one_hello_against_a_pool_coordinator(
            dir.path(),
            &db,
            &keyring,
            None,
            &frame_for(
                schema,
                mode,
                "RTX 4070 SUPER",
                0x72 + mode as u8 + schema as u8,
            ),
            label,
        );
        assert!(out.contains("HELLO_REJECTED"), "{label}\n{out}");
        assert!(
            !out.contains("SESSION_SEEN") && !out.contains("GPU_ATTESTATION_RECORDED"),
            "{label}\n{out}"
        );
    }
    assert_eq!(declared_at(), before);

    // 맞는 GPU — 그 선언 판을 확인하고 스냅샷의 관측 시각이 앞으로 간다(선언 자체는 그대로다)
    let matching = one_hello_against_a_pool_coordinator(
        dir.path(),
        &db,
        &keyring,
        None,
        &frame_for(2, fresh, "RTX 4070 SUPER", 0x79),
        "matching.log",
    );
    assert!(matching.contains("GPU_ATTESTATION_RECORDED"), "{matching}");
    let after = declared_at();
    assert!(
        after > before,
        "맞는 관측이 신선도를 늘리지 않았다({before} -> {after})\n{matching}"
    );
}
