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
    log.write(f"cuda {os.environ.get('CUDA_VISIBLE_DEVICES', '-')}\n")
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
        // ★ 결함 288 (2026-09-25) — 풀 Grant(v4)는 수신 확인 없는 Agent 가 ACK 전에 거부한다. 전에는 이 시험이 바로 그 설정 오류로 돌았다.
        "--require-ack-receipt",
        "true",
        "--shared-checkpoint-root",
        pool.shared.to_str().unwrap(),
        "--checkpoint-publish-interval-ms",
        "200",
        "--pool-peer-keys",
        &pool_agents(),
        "--workload-commit-limit-bytes",
        "1073741824",
        // ★ 신뢰망 남은 일 I — 한 기계의 GPU 마다 노드 하나. A 는 0 번, B 는 1 번만 보게 한다.
        "--gpu-pin",
        if node == NODE_A { "0" } else { "1" },
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
            // ★ 결함 288 (2026-09-25) — 수신 확인을 켠 풀 Agent 는 실행 중 갱신이 Lease 를 이 폭만큼 늘린다. 기본 60초면 A 를 죽인 뒤
            //   Lease 가 만료될 때까지 시험 시한을 넘긴다. 갱신 주기(600ms)의 두 배 + 여유보다 길고 "확인 뒤 5초" 보다 길게 둔다.
            "--renew-extension-ms",
            "7000",
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
        // ★ 결함 288 (2026-09-25) — 3000 이었다. 수신 확인을 켠 풀 Agent 는 "갱신 주기(600) + 5초" 보다 짧은 Lease 로 시작하지 않는다
        //   (LEASE_TOO_SHORT_TO_START) — 3초 Lease 는 수신 확인을 끈(풀 규칙을 어긴) Agent 에서만 돌았다.
        // ★ 2026-09-26 — 7000 으로도 흔들렸다: 스케줄러가 발급한 뒤 B 가 붙기까지 2초 넘게 걸리면 남은 4.8초가 시작 관문(갱신 주기 + 5초)에 걸렸다.
        //   A 를 죽인 뒤 기다리는 시간은 이 값이 아니라 갱신 연장 폭(7초)이 정한다.
        "--lease-ttl-ms",
        "15000",
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
    let mut agent_b = Command::new(cli_bin())
        .args(&b_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent B spawn");
    // ★ 2026-09-25 — agent-loop 는 한 회차의 출력을 **자식이 끝난 뒤에** 찍는다. Job 이 COMPLETED 가 되는 순간(종료 보고 저장)과 그 출력 사이에
    //   틈이 있어, 완료를 보자마자 죽이면 일한 회차의 줄(RESUME_PREPARED 등)이 사라졌다(수신 확인을 켠 뒤 간헐적으로 드러났다 — 고치기 전 14회 중 3회 실패 · 고친 뒤 10회 통과, 교대 측정 아님).
    //   출력을 따로 읽어 일한 회차 줄이 나올 때까지 기다린 뒤 죽인다.
    let b_lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let b_reader = {
        let stdout = agent_b.stdout.take().expect("B stdout");
        let lines = std::sync::Arc::clone(&b_lines);
        thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                let mut all = lines.lock().unwrap();
                all.push_str(&line);
                all.push_str("\n");
            }
        })
    };

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

    let worked_deadline = Instant::now() + Duration::from_secs(20);
    while !b_lines.lock().unwrap().contains("outcome=worked") && Instant::now() < worked_deadline {
        thread::sleep(Duration::from_millis(100));
    }
    let b_err = collect(agent_b);
    b_reader.join().ok();
    let agent_b_out = format!("{}{b_err}", b_lines.lock().unwrap());
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
    // ★ I — 각 시도는 자기 노드에 고정된 GPU 만 봤다(CUDA_VISIBLE_DEVICES).
    assert!(
        resumed.1.contains("cuda 1\n"),
        "B 의 작업이 B 에 고정된 GPU(1)를 보지 않았다\n{everything}"
    );
    assert!(
        traces
            .iter()
            .any(|(_, text)| text.starts_with("start 0\n") && text.contains("cuda 0\n")),
        "A 의 작업이 A 에 고정된 GPU(0)를 보지 않았다\n{everything}"
    );

    // ── 운영자 — A 가 멈췄음을 확인하고 옛 예약을 푼다. 살아 있는 시도의 예약은 못 푼다(음성은 소유자 선점 시험에 있다).
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

/// 로컬 HTTP 한 번 — Owner Panel 과 이야기한다(이 기계의 소유자가 브라우저로 하는 일).
fn http(port: u16, method: &str, path: &str, token: Option<&str>, body: &str) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("Owner Panel 연결");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("x-gputeer-owner-token: {token}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).expect("요청 쓰기");
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("빈 포트")
        .local_addr()
        .expect("주소")
        .port()
}

/// ★★ 2026-09-23 (신뢰망 남은 일 H) — **소유자가 GPU 를 되찾으면**, Lease 만료를 기다리지 않고 다른 노드가 마지막
/// 체크포인트에서 이어 끝낸다. 되찾은 노드는 소유자가 다시 켤 때까지 풀에 붙지 않는다.
///
/// ```text
/// A 에서 작업이 돈다 -> 소유자가 Owner Panel 로 멈춘다(토큰 · POST /api/stop)
/// A 가 INTERRUPTED 로 보고 -> Coordinator: RUNNING -> PAUSED (OWNER_PREEMPT) + A 되찾김 표시
/// B 가 PAUSED -> RUNNING (RESUMED) 로 받아 이어서 끝낸다   ★ Lease 는 60초 — 그 전에 끝나야 한다
/// A 는 다음 회차에 OWNER_RECLAIMED 로 붙지 않는다 -> owner-resume 뒤에는 다시 붙는다
/// ```
#[test]
fn an_owner_reclaiming_the_gpu_moves_the_job_without_waiting_for_the_lease() {
    let pool = prepare();
    let db_s = pool.db.to_str().unwrap().to_string();
    let keyring_s = pool.keyring.to_str().unwrap().to_string();
    let agents = pool_agents();
    let coordinator_log = pool.dir.path().join("coordinator.log");
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
            "--shared-checkpoint-root",
            pool.shared.to_str().unwrap(),
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

    let panel_port = free_port();
    let mut a_args = vec!["agent-stub".to_string()];
    a_args.extend(agent_args(&pool, &addr, NODE_A, SEED_A));
    a_args.extend(["--owner-panel-port".to_string(), panel_port.to_string()]);
    // A 가 먼저 소식을 남긴다(일은 아직 없다).
    let _ = run_cli(&a_args.iter().map(String::as_str).collect::<Vec<_>>());

    // Lease 60초 — 이 시험은 그 만료를 **기다리지 않고** 끝나야 한다.
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
        "60000",
        "--lease-renew-after-ms",
        "20000",
        "--lease-max-total-duration-seconds",
        "3600",
        "--silent-after-ms",
        "60000",
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
    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(30);
    while job(&pool.db).map(|job| job.state) != Some(JobState::Staging) {
        assert!(Instant::now() < deadline, "A 에 예약되지 않았다");
        thread::sleep(Duration::from_millis(100));
    }
    let agent_a = Command::new(cli_bin())
        .args(&a_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent A spawn");
    let deadline = Instant::now() + Duration::from_secs(40);
    while signed_checkpoints(&pool) < 2 {
        assert!(
            Instant::now() < deadline,
            "A 가 체크포인트를 게시하지 않았다"
        );
        thread::sleep(Duration::from_millis(50));
    }

    // 음성 — 살아 있는 시도의 예약은 운영자라도 못 푼다.
    let (ok, out) = run_cli(&[
        "release-lost-node",
        "--control-db",
        &db_s,
        "--node",
        NODE_A,
        "--operator-statement",
        "잘못 짚은 노드",
    ]);
    assert!(
        !ok && out.contains("살아 있는 시도"),
        "도는 작업의 예약을 풀었다: {out}"
    );

    // ── 소유자 — 자기 PC 의 패널에서 멈춘다(토큰은 로컬 페이지가 받는 값 그대로)
    let listing = http(panel_port, "GET", "/api/workloads", None, "");
    let json = &listing[listing.find('{').expect("JSON")..];
    let value: serde_json::Value = serde_json::from_str(json).expect("workloads JSON");
    let token = value["token"].as_str().expect("token").to_string();
    let attempt = value["workloads"][0]["attempt_id"]
        .as_str()
        .expect("도는 작업이 목록에 없다")
        .to_string();
    let stopped = http(panel_port, "POST", "/api/stop", Some(&token), &attempt);
    assert!(
        stopped.contains("STOP_REQUESTED"),
        "소유자 정지가 안 됐다: {stopped}"
    );
    let agent_a_out = {
        let output = agent_a.wait_with_output().expect("A 종료");
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    };

    // ── B 가 되풀이해 붙는다
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
    let deadline = Instant::now() + Duration::from_secs(45);
    let finished = loop {
        let state = job(&pool.db).map(|job| job.state);
        if matches!(state, Some(JobState::Completed | JobState::Failed))
            || Instant::now() >= deadline
        {
            break state;
        }
        thread::sleep(Duration::from_millis(200));
    };
    let elapsed = started.elapsed();
    // 되찾은 노드는 다음 회차에 붙지 않는다.
    let (again_ok, again) = run_cli(&a_args.iter().map(String::as_str).collect::<Vec<_>>());

    let agent_b_out = collect(agent_b);
    let scheduler_out = collect(scheduler);
    let coordinator_err = collect(coordinator);
    let coordinator_out = std::fs::read_to_string(&coordinator_log).unwrap_or_default();
    let traces: Vec<String> = std::fs::read_dir(&pool.trace)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| {
            std::fs::read_to_string(entry.path())
                .unwrap_or_default()
                .replace("\r\n", "\n")
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
    assert!(
        elapsed < Duration::from_secs(60),
        "Lease 만료(60초)를 기다린 뒤에야 끝났다({elapsed:?})\n{everything}"
    );
    assert!(
        agent_a_out.contains("OWNER_STOPPED"),
        "A 가 소유자 정지를 적지 않았다\n{everything}"
    );
    let preempted = coordinator_out
        .lines()
        .find(|line| line.starts_with("OWNER_PREEMPTED"))
        .unwrap_or_else(|| panic!("Coordinator 가 선점으로 멈추지 않았다\n{everything}"));
    let resume_step: u64 = preempted
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix("resume_step="))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("이어갈 체크포인트 없이 멈췄다: {preempted}\n{everything}"));
    assert!(
        traces
            .iter()
            .any(|trace| trace.starts_with(&format!("start {resume_step}\n"))
                && trace.contains(&format!("step {STEPS}\n"))),
        "다른 노드가 step {resume_step} 에서 이어 끝내지 않았다\n{everything}"
    );
    assert!(
        agent_b_out.contains("RESUME_PREPARED"),
        "B 가 재개 지점을 검증 · 복원하지 않았다\n{everything}"
    );
    assert!(
        !again_ok && again.contains("OWNER_RECLAIMED"),
        "되찾은 노드가 다시 풀에 붙었다: {again}"
    );
    let a_checkpoints = pool.dir.path().join(format!("{NODE_A}-checkpoints"));
    let (ok, out) = run_cli(&[
        "owner-resume",
        "--checkpoint-root",
        a_checkpoints.to_str().unwrap(),
    ]);
    assert!(ok && out.contains("OWNER_RESUMED"), "{out}");
    assert!(
        !gputeer_agent::owner_reclaim_marker(&a_checkpoints)
            .unwrap()
            .exists(),
        "공유를 다시 켰는데 표시 파일이 남았다"
    );
}
