//! **신뢰망 파티 장애 이어받기 — 실행 중인 노드를 죽이면 다른 노드가 마지막 체크포인트에서 이어서 끝낸다.**
//!
//! ```text
//! A 가 작업을 받아 단계마다 체크포인트를 쓴다 -> Agent 가 공유 저장소로 게시(생산자 서명)
//! A 를 죽인다(작업도 Job Object 와 함께 죽는다) -> 갱신이 멈추고 Lease 가 만료된다
//! scheduler-loop 의 장애 판정 -> RUNNING -> INTERRUPTED -> REPLANNING -> QUEUED (마지막 체크포인트를 붙여서)
//! B 가 v3 Grant(재개 지점)를 받는다 -> 서명 · 해시 검증 뒤 되살려 그 다음 단계부터 -> 완료 · 보고 · 예약 해제
//! 운영자가 A 가 멈췄음을 확인하고 옛 예약을 푼다
//! ```
//!
//! ★ 2026-09-23 (신뢰망 남은 일 E · F · G). 전에는 체크포인트가 만든 기계 밖으로 나가지 않았고, 이어갈 지점을 알릴 칸도,
//!   끊긴 노드를 판정하는 곳도 없었다.
//!
//! ★ **Windows 전용이다**(실행 관문). 파이썬이 필요하다 — 없으면 시험이 **실패한다**(조용히 건너뛰지 않는다).
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use gputeer_coordinator::job_store::{CoordinatorJobStore, JobState};
use gputeer_crypto::SigningKey;

const NODE_A: &str = "01JFAILNODE000000000001";
const NODE_B: &str = "01JFAILNODE000000000002";
const SEED_A: &str = "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1";
const SEED_B: &str = "c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2";
const SUBMITTER: &str = "01JSUBMITTERFAIL00000001";
const SEED: &str = "99999999999999999999999999999999999999999999999999999999999999cf";
const OWNER: &str = "owner-fail";
const COORDINATOR: &str = "01JCOORDINATORFAIL000001";
const COORD_SEED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaf2";
const AXES: &str = "vram,gpu_count,cpu,ram,workspace";
const JOB: &str = "01JJOBFAIL0000000000001";
const STEPS: u64 = 6;

/// 단계마다 체크포인트를 쓰는 작업. 재개 폴더가 있으면 거기 적힌 단계 **다음부터** 한다.
/// 어느 시도가 몇 단계를 했는지 trace 폴더에 남긴다(시험이 본다).
const WORKLOAD: &str = r#"
import os, pathlib, sys, time
out = pathlib.Path(os.environ["GPUTEER_CHECKPOINT_DIR"])
trace = pathlib.Path(sys.argv[1])
attempt = os.environ["GPUTEER_ATTEMPT_ID"]
resume = os.environ.get("GPUTEER_RESUME_DIR")
start = int(pathlib.Path(resume, "counter").read_text()) if resume else 0
with open(trace / (attempt + ".txt"), "a") as log:
    log.write(f"start {start}\n")
for step in range(start + 1, int(sys.argv[2]) + 1):
    time.sleep(0.5)
    tmp = out / f"step-{step}.tmp"
    tmp.mkdir()
    (tmp / "counter").write_text(str(step))
    tmp.rename(out / f"step-{step}")
    with open(trace / (attempt + ".txt"), "a") as log:
        log.write(f"step {step}\n")
"#;

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

/// 파이썬의 **절대 경로** — 실행 관문은 PATH 를 찾지 않는다.
fn python() -> String {
    let out = Command::new("python")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .expect("이 시험은 파이썬이 필요하다 — PATH 에 python 이 없다");
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        Path::new(&path).is_file(),
        "파이썬 실행 파일을 찾지 못했다: {path:?}"
    );
    path
}

struct Pool {
    dir: tempfile::TempDir,
    db: PathBuf,
    keyring: PathBuf,
    shared: PathBuf,
    trace: PathBuf,
}

fn prepare() -> Pool {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    let shared = dir.path().join("shared-checkpoints");
    let trace = dir.path().join("trace");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::create_dir_all(&trace).unwrap();
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
    let bootstrap = dir.path().join("bootstrap.json");
    std::fs::write(
        &bootstrap,
        format!(
            "{{\n  \"schema_version\": 1,\n  \"agents\": [\n{},\n{}\n  ]\n}}",
            agent(NODE_A, SEED_A),
            agent(NODE_B, SEED_B)
        ),
    )
    .unwrap();
    let (ok, out) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory 실패: {out}");

    let keyring = dir.path().join("submitters.keyring");
    let mut ring = gputeer_crypto::PersistentKeyring::new(
        &keyring,
        gputeer_crypto::KeyProtection::K0Plaintext,
        gputeer_crypto::PlaintextPolicy::Allow,
    )
    .unwrap();
    ring.insert_public(
        SUBMITTER,
        SigningKey::from_bytes(&seed_bytes(SEED)).verifying_key(),
    )
    .unwrap();
    ring.save().unwrap();

    let script = dir.path().join("workload.py");
    std::fs::write(&script, WORKLOAD).unwrap();
    let manifest = dir.path().join("job.pb");
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
    let python = python();
    let args_csv = format!(
        "{},{},{STEPS}",
        script.to_str().unwrap(),
        trace.to_str().unwrap()
    );
    let (ok, out) = run_cli(&[
        "submit",
        "--job-id",
        JOB,
        "--entrypoint",
        &python,
        "--args",
        &args_csv,
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
    let (ok, out) = run_cli(&[
        "import-manifest",
        "--manifest",
        manifest.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--job-db",
        db.to_str().unwrap(),
        "--idempotency-key",
        "f10102030405060708090a0b0c0d0e0f",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "import-manifest 실패: {out}");
    let (ok, out) = run_cli(&[
        "plan-job",
        "--job-id",
        JOB,
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
    Pool {
        dir,
        db,
        keyring,
        shared,
        trace,
    }
}

fn pool_agents() -> String {
    format!("{NODE_A}={};{NODE_B}={}", pub_hex(SEED_A), pub_hex(SEED_B))
}

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

fn agent_args(pool: &Pool, addr: &str, node: &str, seed: &str) -> Vec<String> {
    let fence = pool.dir.path().join(format!("{node}-fence.sqlite3"));
    let checkpoints = pool.dir.path().join(format!("{node}-checkpoints"));
    [
        "--connect",
        addr,
        "--own-seed",
        seed,
        "--peer-pubkey",
        &pub_hex(COORD_SEED),
        "--coordinator-device-id",
        COORDINATOR,
        "--agent-device-id",
        node,
        "--fence-db",
        fence.to_str().unwrap(),
        "--checkpoint-root",
        checkpoints.to_str().unwrap(),
        "--submitter-pubkey",
        &pub_hex(SEED),
        "--i-understand-this-executes-untrusted-code",
        "true",
        "--report-over-session",
        "true",
        "--max-reconnect-attempts",
        "1",
        "--renew-during-execution-ms",
        "600",
        "--shared-checkpoint-root",
        pool.shared.to_str().unwrap(),
        "--checkpoint-publish-interval-ms",
        "200",
        "--pool-peer-keys",
        &pool_agents(),
        "--workload-commit-limit-bytes",
        "1073741824",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn collect(mut child: Child) -> String {
    let _ = child.kill();
    let output = child.wait_with_output().expect("출력 수집");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn job(db: &Path) -> Option<gputeer_coordinator::job_store::StoredJob> {
    CoordinatorJobStore::open(db).ok()?.get(JOB).ok().flatten()
}

fn signed_checkpoints(pool: &Pool) -> usize {
    gputeer_checkpoint::shared::list_signed_manifests(&pool.shared, JOB)
        .map(|listed| listed.len())
        .unwrap_or(0)
}

#[test]
fn a_killed_node_is_taken_over_from_its_last_checkpoint_on_another_node() {
    let pool = prepare();
    let db_s = pool.db.to_str().unwrap().to_string();
    let keyring_s = pool.keyring.to_str().unwrap().to_string();

    // ── 풀 Coordinator
    let coordinator_log = pool.dir.path().join("coordinator.log");
    let agents = pool_agents();
    let coordinator = Command::new(cli_bin())
        .args([
            "coordinator-stub",
            "--pool-mode",
            "true",
            "--pool-agents",
            &agents,
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
            std::fs::File::create(&coordinator_log).unwrap(),
        ))
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator spawn");
    let addr = wait_ready(&coordinator_log);

    // ── A 한 번만 붙는다(되풀이 루프가 아니라 프로세스 하나 — 이것을 죽인다). 먼저 인사해서 소식을 남긴다.
    let mut a_args = vec!["agent-stub".to_string()];
    a_args.extend(agent_args(&pool, &addr, NODE_A, SEED_A));
    // 스케줄러가 A 에게만 줄 수 있게, A 가 먼저 소식을 남기도록 한 번 붙였다 떨어진다(일이 아직 없다).
    let (_, _) = run_cli(&a_args.iter().map(String::as_str).collect::<Vec<_>>());

    // ── 스케줄러 루프 — 장애 판정 포함. Lease 는 짧게(3초), 실행 중 갱신이 늘린다.
    let scheduler_args: Vec<String> = [
        "scheduler-loop",
        "--interval-ms",
        "150",
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
        "3000",
        "--lease-renew-after-ms",
        "1000",
        "--lease-max-total-duration-seconds",
        "3600",
        "--silent-after-ms",
        "60000",
        "--failover-grace-ms",
        "500",
        "--shared-checkpoint-root",
        pool.shared.to_str().unwrap(),
        "--pool-agents",
        &agents,
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let scheduler = Command::new(cli_bin())
        .args(&scheduler_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("scheduler spawn");

    // 스케줄러가 A 에 예약할 때까지 기다린다(소식이 있는 노드는 A 뿐이다).
    let deadline = Instant::now() + Duration::from_secs(30);
    while job(&pool.db).map(|job| job.state) != Some(JobState::Staging) {
        if Instant::now() >= deadline {
            let scheduler_out = collect(scheduler);
            let coordinator_err = collect(coordinator);
            panic!(
                "A 에 예약되지 않았다
--- scheduler ---
{scheduler_out}
--- coordinator ---
{coordinator_err}"
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
    let agent_a = Command::new(cli_bin())
        .args(&a_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent A spawn");

    // A 가 체크포인트를 둘 이상 게시할 때까지 기다렸다가 **죽인다**.
    let deadline = Instant::now() + Duration::from_secs(40);
    while signed_checkpoints(&pool) < 2 {
        if Instant::now() >= deadline {
            let a = collect(agent_a);
            let scheduler_out = collect(scheduler);
            let coordinator_err = collect(coordinator);
            let log = std::fs::read_to_string(&coordinator_log).unwrap_or_default();
            panic!(
                "A 가 체크포인트를 게시하지 않았다
--- agent A ---
{a}
--- scheduler ---
{scheduler_out}
--- coordinator ---
{log}
{coordinator_err}"
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
    let agent_a_out = collect(agent_a);

    // ── B 가 되풀이해 붙는다 — 이어받기 전에는 일이 없다
    let mut b_args: Vec<String> = [
        "agent-loop",
        "--interval-ms",
        "200",
        "--max-rounds",
        "0",
        "--",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    b_args.extend(agent_args(&pool, &addr, NODE_B, SEED_B));
    let agent_b = Command::new(cli_bin())
        .args(&b_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent B spawn");

    let deadline = Instant::now() + Duration::from_secs(90);
    let finished = loop {
        let state = job(&pool.db).map(|job| job.state);
        if matches!(state, Some(JobState::Completed | JobState::Failed))
            || Instant::now() >= deadline
        {
            break state;
        }
        thread::sleep(Duration::from_millis(200));
    };

    let agent_b_out = collect(agent_b);
    let scheduler_out = collect(scheduler);
    let coordinator_err = collect(coordinator);
    let coordinator_out = std::fs::read_to_string(&coordinator_log).unwrap_or_default();
    let traces: Vec<(String, String)> = std::fs::read_dir(&pool.trace)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().to_string(),
                // 파이썬 텍스트 모드는 Windows 에서 CRLF 를 쓴다 — 비교 전에 LF 로 맞춘다.
                std::fs::read_to_string(entry.path())
                    .unwrap_or_default()
                    .replace("\r\n", "\n"),
            )
        })
        .collect();
    let everything = format!(
        "--- traces ---\n{traces:#?}\n--- agent A ---\n{agent_a_out}\n--- agent B ---\n{agent_b_out}\n--- scheduler ---\n{scheduler_out}\n--- coordinator ---\n{coordinator_out}\n{coordinator_err}"
    );

    assert_eq!(
        finished,
        Some(JobState::Completed),
        "작업이 끝나지 않았다\n{everything}"
    );
    let requeued = scheduler_out
        .lines()
        .find(|line| line.starts_with("FAILOVER_REQUEUED"))
        .unwrap_or_else(|| panic!("장애 판정이 작업을 되돌리지 않았다\n{everything}"));
    assert!(
        requeued.contains(&format!("lost_node={NODE_A}")),
        "{requeued}"
    );
    assert!(requeued.contains("was_running=true"), "{requeued}");
    let resume_step: u64 = requeued
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix("resume_step="))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("이어갈 체크포인트 없이 되돌렸다: {requeued}\n{everything}"));
    assert!(resume_step >= 2, "{requeued}");
    assert!(
        agent_b_out.contains("RESUME_PREPARED"),
        "B 가 재개 지점을 검증 · 복원하지 않았다\n{everything}"
    );

    // 두 시도의 trace — A 는 처음부터, B 는 **A 의 마지막 체크포인트 다음부터** 끝까지.
    assert_eq!(traces.len(), 2, "시도가 둘이어야 한다\n{everything}");
    let resumed = traces
        .iter()
        .find(|(_, text)| text.starts_with(&format!("start {resume_step}\n")))
        .unwrap_or_else(|| panic!("step {resume_step} 에서 이어간 시도가 없다\n{everything}"));
    assert!(
        !resumed.1.contains("step 1\n"),
        "이어간 시도가 처음부터 다시 했다\n{everything}"
    );
    assert!(
        resumed.1.contains(&format!("step {STEPS}\n")),
        "이어간 시도가 끝까지 가지 않았다\n{everything}"
    );

    // ── 운영자 — A 가 멈췄음을 확인하고 옛 예약을 푼다. 살아 있는 시도의 예약은 못 푼다(아래 음성).
    let (ok, out) = run_cli(&[
        "release-lost-node",
        "--control-db",
        &db_s,
        "--node",
        NODE_A,
        "--operator-statement",
        "시험: A 의 agent-stub 프로세스를 죽였고 Job Object 가 작업을 함께 끝냈다",
    ]);
    assert!(ok, "끊긴 노드의 예약을 풀지 못했다: {out}");
    assert!(out.contains("LOST_NODE_RELEASED"), "{out}");
}
