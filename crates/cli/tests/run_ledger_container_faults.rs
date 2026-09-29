//! **노드 실행 원장 — 컨테이너 행의 보고 보관 전 원장 쓰기 실패(계획 `docs/plans/2026-09-29_0212_노드_실행원장_기존노드_이관_구현계획.md` R8b).**
//!
//! ```text
//! coordinator-stub --pool-mode    실제 Coordinator(풀 · 보고 세션)
//! agent-loop -- --run-ledger true 실제 루프 바이너리 · 실제 자식 · 컨테이너 런타임은 이 시험 실행 파일 자신(가짜)
//! ```
//!
//! # 가짜 런타임은 이 시험 실행 파일 자신이다
//!
//! `harness = false` 라 `main` 을 직접 쓴다(`crates/agent/tests/container_lifecycle.rs` 와 같은 방식). `GPUTEER_FAKE_RUNTIME_STATE` 가 있으면
//! 런타임 흉내만 내고 끝난다 — 환경 변수는 agent-loop → Agent → 런타임으로 그대로 물려간다.
//!
//! # 실패는 코드에 주입 자리를 두지 않고 실제 파일시스템으로 만든다
//!
//! 가짜 런타임이 실행 뒤의 `rm` 에서 컨테이너를 지운 다음 원장의 저널 자리(`<원장>-journal`, journal_mode=DELETE)에 폴더를 만든다. 그 뒤 Agent 의
//! 첫 원장 쓰기(판정 사실 기록 — 보고 보관 **전**)가 저널을 만들지 못해 실패한다. R8c(보관 **뒤** 실패)는 `trusted_party_pool.rs` 에 있다.
//!
//! ★ **Windows 전용이다** — 풀 Agent 의 실행 관문이 리눅스에서는 cgroup 위임을 요구한다(`trusted_party_pool.rs` 와 같다).

use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::process::{Child, Command, Stdio};
#[cfg(windows)]
use std::time::{Duration, Instant};

const STATE_ENV: &str = "GPUTEER_FAKE_RUNTIME_STATE";
/// 있으면 실행 뒤 `rm` 이 컨테이너를 지운 다음 이 경로에 폴더를 만든다.
const BLOCK_ENV: &str = "GPUTEER_FAKE_RUNTIME_BLOCK_AFTER_RM";
/// 있으면 만든 컨테이너의 `rm` 이 지우지 못하고 실패로 답한다(컨테이너가 남는 갈래 — 지움 확인 없음).
const RM_FAILS_ENV: &str = "GPUTEER_FAKE_RUNTIME_RM_FAILS";
const FAKE_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn main() {
    if let Ok(state) = std::env::var(STATE_ENV) {
        std::process::exit(fake_runtime(Path::new(&state)));
    }
    #[cfg(windows)]
    for (label, rm_fails) in [("removed", false), ("left_behind", true)] {
        r8b_a_ledger_write_failure_before_the_report_is_kept_leaves_no_report_and_the_restart_blocks(rm_fails);
        println!("test r8b_a_ledger_write_failure_before_the_report_is_kept_leaves_no_report_and_the_restart_blocks[{label}] ... ok");
    }
    #[cfg(not(windows))]
    println!("run_ledger_container_faults: Windows 전용 — 건너뜀");
}

// ─── 가짜 런타임(정상 경로만) ─────────────────────────────────────────────────

fn fake_runtime(state: &Path) -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    append(state, "calls", &format!("{}\n", args.join(" ")));
    let created = state.join("create.args").exists();
    let gone = state.join("gone").exists();
    let exists = created && !gone;
    let target = args.last().cloned().unwrap_or_default();
    let running_only = args.get(1).map(String::as_str) == Some("--format={{.State.Running}}");
    let owner_only = args
        .get(1)
        .is_some_and(|a| a.starts_with("--format={{.Id}} {{index .Config.Labels"));
    let name_only = args.get(1).map(String::as_str) == Some("--format={{.Name}}");
    let create_args = || std::fs::read_to_string(state.join("create.args")).unwrap_or_default();
    let started = state.join("started").exists();
    match command {
        "create" => {
            std::fs::write(state.join("create.args"), args.join("\n")).unwrap();
            let _ = std::fs::remove_file(state.join("gone"));
            println!("{FAKE_ID}");
            0
        }
        "start" => {
            std::fs::write(state.join("started"), "").unwrap();
            0
        }
        "wait" => {
            println!("0");
            0
        }
        "inspect" => {
            if !exists {
                eprintln!("Error: No such container: {target}");
                return 1;
            }
            if owner_only {
                let owner = create_args()
                    .lines()
                    .find_map(|l| l.strip_prefix("--label=gputeer.owner=").map(str::to_string))
                    .unwrap_or_default();
                println!("{FAKE_ID} {owner}");
            } else if name_only {
                let name = create_args()
                    .lines()
                    .find_map(|l| l.strip_prefix("--name=").map(str::to_string))
                    .unwrap_or_default();
                println!("/{name}");
            } else if running_only {
                println!("false");
            } else if started {
                // 작업은 코드 0 으로 끝났다.
                println!("false 0 false 2026-09-29T00:00:00Z");
            } else {
                println!("false 0 false 0001-01-01T00:00:00Z");
            }
            0
        }
        "kill" => {
            eprintln!("fake: container is not running");
            1
        }
        "logs" => {
            println!("hello-out");
            0
        }
        "rm" => {
            if !exists {
                eprintln!("Error: No such container: {target}");
                return 1;
            }
            // 실행 뒤의 지우기 — 끝나면 원장 저널 자리를 막는다(보고 보관 전 원장 쓰기가 실패하게).
            if let Ok(block) = std::env::var(BLOCK_ENV) {
                std::fs::create_dir_all(block).unwrap();
            }
            if std::env::var_os(RM_FAILS_ENV).is_some() {
                eprintln!("fake: rm 실패를 흉내낸다(컨테이너는 남는다)");
                return 125;
            }
            std::fs::write(state.join("gone"), "").unwrap();
            0
        }
        "pull" => 0,
        "ps" => 0,
        other => {
            eprintln!("fake: 모르는 명령 {other}");
            2
        }
    }
}

fn append(state: &Path, name: &str, text: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join(name))
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

// ─── 풀 준비(`trusted_party_pool.rs` 의 한 회차 흐름을 줄여 옮김) ──────────────────

#[cfg(windows)]
mod pool {
    use super::*;
    use gputeer_coordinator::job_store::{CoordinatorJobStore, JobState};
    use gputeer_crypto::SigningKey;

    pub const NODE_1: &str = "01JLEDGERNODE00000000001";
    pub const AGENT_SEED_1: &str =
        "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1";
    const SUBMITTER: &str = "01JSUBMITTERLEDGER000001";
    const SEED: &str = "99999999999999999999999999999999999999999999999999999999999999cf";
    const OWNER: &str = "owner-ledger";
    const COORDINATOR: &str = "01JCOORDINATORLEDGER0001";
    const COORD_SEED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaf2";
    const AXES: &str = "vram,gpu_count,cpu,ram,workspace";
    pub const JOB: &str = "01JJOBLEDGER000000000001";

    pub fn cli_bin() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_gputeer"))
    }

    fn seed_bytes(hex: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    pub fn pub_hex(seed: &str) -> String {
        SigningKey::from_bytes(&seed_bytes(seed))
            .verifying_key()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn now_unix_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub fn run_cli(args: &[&str]) -> (bool, String) {
        let output = Command::new(cli_bin())
            .args(args)
            .output()
            .expect("gputeer 실행");
        (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    pub fn job_state(db: &Path, job: &str) -> Option<JobState> {
        CoordinatorJobStore::open(db)
            .ok()?
            .get(job)
            .ok()
            .flatten()
            .map(|job| job.state)
    }

    /// 노드 하나 · OCI 이미지 작업 하나를 넣고 예약까지 한다 — (control db · 키링).
    pub fn one_container_job(dir: &Path) -> (PathBuf, PathBuf) {
        let db = dir.join("control.sqlite3");
        let observed = now_unix_ms();
        let bootstrap = dir.join("bootstrap.json");
        std::fs::write(
            &bootstrap,
            format!(
                r#"{{
  "schema_version": 1,
  "agents": [{{
    "registry": {{
      "node_id": "{NODE_1}", "device_id": "{NODE_1}",
      "owner_member_id": "{OWNER}", "verifying_key_hex": "{key}",
      "node_state": "ONLINE", "risk_state": "NORMAL",
      "security_tier": "S2", "isolation_class": "CONTAINED",
      "key_protection": "K1"
    }},
    "inventory": {{
      "inventory_revision": 1, "observed_at_unix_ms": {observed},
      "gpus": [{{ "gpu_id": "{NODE_1}-gpu-0", "model": "RTX 4070 SUPER",
                  "healthy": true, "available_vram_bytes": 12884901888 }}],
      "available_cpu_cores": 16, "available_ram_bytes": 34359738368,
      "available_workspace_bytes": 107374182400,
      "allowed_workload_classes": ["TRAINING"],
      "third_party_workloads_opt_in": true
    }}
  }}]
}}"#,
                key = pub_hex(AGENT_SEED_1)
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
        let keyring = dir.join("submitters.keyring");
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
        let manifest = dir.join("job.pb");
        let issued = now_unix_ms().saturating_sub(60_000).to_string();
        let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
        let digest = "ab".repeat(32);
        let (ok, out) = run_cli(&[
            "submit",
            "--job-id",
            JOB,
            "--entrypoint",
            "exit-0",
            "--image-ref",
            "registry.example.test/gputeer/fake",
            "--image-sha256",
            &digest,
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
            "0f0102030405060708090a0b0c0d0e0f",
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
        let (ok, out) = run_cli(&[
            "scheduler-tick",
            "--control-db",
            db.to_str().unwrap(),
            "--submitter-keyring",
            keyring.to_str().unwrap(),
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
        (db, keyring)
    }

    /// 시험이 중간에 실패해도(panic) 끝없이 기다리는 Coordinator 를 남기지 않는다.
    pub struct KillOnDrop(pub Option<Child>);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                // 정리 단계다 — 오류가 시험의 진짜 실패 메시지를 가리지 않게 버린다.
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// 풀 Coordinator 를 띄우고 READY 주소를 기다린다.
    pub fn start_coordinator(
        dir: &Path,
        db: &Path,
        keyring: &Path,
    ) -> (KillOnDrop, PathBuf, String) {
        let log = dir.join("coordinator.log");
        let pool_agents = format!("{NODE_1}={}", pub_hex(AGENT_SEED_1));
        let child = Command::new(cli_bin())
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
                db.to_str().unwrap(),
                "--lease-db",
                db.to_str().unwrap(),
                "--liveness-db",
                db.to_str().unwrap(),
                "--submitter-keyring",
                keyring.to_str().unwrap(),
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
            .stdout(Stdio::from(std::fs::File::create(&log).unwrap()))
            .stderr(Stdio::null())
            .spawn()
            .expect("coordinator spawn");
        let guard = KillOnDrop(Some(child));
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            if let Some(addr) = text.lines().find_map(|l| l.strip_prefix("READY ")) {
                return (guard, log, addr.trim().to_string());
            }
            assert!(
                Instant::now() < deadline,
                "Coordinator 가 READY 를 찍지 않았다: {text}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// 원장을 켠 풀 Agent(agent-loop) — 컨테이너 런타임은 이 시험 실행 파일 자신.
    pub fn agent_loop(dir: &Path, addr: &str, max_rounds: &str) -> Vec<String> {
        let runtime = std::env::current_exe().unwrap();
        [
            "agent-loop",
            "--interval-ms",
            "100",
            "--max-rounds",
            max_rounds,
            "--",
            "--connect",
            addr,
            "--own-seed",
            AGENT_SEED_1,
            "--peer-pubkey",
            &pub_hex(COORD_SEED),
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            NODE_1,
            "--fence-db",
            dir.join("fence.sqlite3").to_str().unwrap(),
            "--checkpoint-root",
            dir.join("checkpoints").to_str().unwrap(),
            "--submitter-pubkey",
            &pub_hex(SEED),
            "--i-understand-this-executes-untrusted-code",
            "true",
            "--report-over-session",
            "true",
            "--max-reconnect-attempts",
            "1",
            "--require-ack-receipt",
            "true",
            "--renew-during-execution-ms",
            "1000",
            "--container-runtime",
            runtime.to_str().unwrap(),
            "--container-runtime-kind",
            "docker",
            "--run-ledger",
            "true",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// 명령을 끝까지 돌려 (성공 여부 · 표준 출력과 오류). 시한(120초)을 넘기면 죽이고 실패시킨다 — 자식이 멈춰도 시험이 끝없이 기다리지 않게
    ///   (코덱스 r1t). 출력은 파일로 받는다(기다리는 동안 파이프가 차서 자식이 멈추는 일이 없게).
    pub fn run_to_end(dir: &Path, args: &[String], envs: &[(&str, &Path)]) -> (bool, String) {
        let out_path = dir.join(format!("agent-loop-{}.out", now_unix_ms()));
        let out = std::fs::File::create(&out_path).unwrap();
        let mut command = Command::new(cli_bin());
        command
            .args(args)
            .stdout(Stdio::from(out.try_clone().unwrap()))
            .stderr(Stdio::from(out));
        for (key, value) in envs {
            command.env(key, value);
        }
        let mut child = KillOnDrop(Some(command.spawn().expect("agent-loop 실행")));
        let deadline = Instant::now() + Duration::from_secs(120);
        let status = loop {
            let running = child.0.as_mut().expect("자식");
            if let Some(status) = running.try_wait().expect("자식 상태") {
                child.0 = None;
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "agent-loop 가 120초 안에 끝나지 않았다\n{}",
                std::fs::read_to_string(&out_path).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        (
            status.success(),
            std::fs::read_to_string(&out_path).unwrap_or_default(),
        )
    }
}

/// 계획 R8b — 컨테이너 행에서 **보고 보관 전** 원장 쓰기(판정 사실 · 지움 확인)가 실패하면: 보고를 보관하지 않고 · 루프가 첫 회차 뒤 멈추고 · 작업은 끝나지
///   않는다. 저널 자리를 되살려 재기동하면 원장이 시도를 LOCAL_BLOCKED 로 풀고(보고가 없으니 끝났다고 볼 수 없다) 새 작업을 받지 않는다.
///   `rm_fails` — 컨테이너를 지우지 못한 갈래(지움 확인 없음). 두 갈래 모두 종료 단계의 **한 번의 쓰기**(판정 사실 + 지움 확인 또는 막힘 —
///   `RunLedger::record_container_exit`)가 실패한다. 사실만 적히고 결과가 빠지는 중간 상태는 그 쓰기가 한 트랜잭션이라 없다(계획 R8b 구현 메모).
#[cfg(windows)]
fn r8b_a_ledger_write_failure_before_the_report_is_kept_leaves_no_report_and_the_restart_blocks(
    rm_fails: bool,
) {
    use gputeer_agent::run_ledger::{self, RowState};
    use gputeer_coordinator::job_store::JobState;
    use pool::*;

    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("runtime-state");
    std::fs::create_dir_all(&state).unwrap();
    let checkpoints = dir.path().join("checkpoints");
    let mut journal = checkpoints.clone().into_os_string();
    journal.push(run_ledger::LEDGER_SUFFIX);
    journal.push("-journal");
    let journal = PathBuf::from(journal);
    let (db, keyring) = one_container_job(dir.path());
    let (coordinator, coordinator_log, addr) = start_coordinator(dir.path(), &db, &keyring);
    let context = |extra: &str| {
        format!(
            "{extra}\n--- runtime calls ---\n{}\n--- coordinator ---\n{}",
            std::fs::read_to_string(state.join("calls")).unwrap_or_default(),
            std::fs::read_to_string(&coordinator_log).unwrap_or_default()
        )
    };

    // ── 1회차: 컨테이너는 돌고 지워진다 · 지운 뒤 원장 쓰기가 실패한다. 회차를 셋까지 허락해도 첫 회차 뒤에 멈춰야 한다.
    let mut envs: Vec<(&str, &Path)> = vec![(STATE_ENV, &state), (BLOCK_ENV, &journal)];
    if rm_fails {
        envs.push((RM_FAILS_ENV, &state));
    }
    let (first_ok, first) = run_to_end(dir.path(), &agent_loop(dir.path(), &addr, "3"), &envs);
    // 실패 자리를 가른다(코덱스 r1t) — 컨테이너가 **시작해 끝났고 로그까지 거뒀다.** 시작 전 정리(NotStarted 닫기)의 실패가 아니다.
    let calls = std::fs::read_to_string(state.join("calls")).unwrap_or_default();
    assert!(
        state.join("create.args").exists()
            && state.join("started").exists()
            && calls.lines().any(|l| l.starts_with("logs")),
        "컨테이너를 만들고 · 시작하고 · 로그를 거두는 데까지 가지 않았다\n{}",
        context(&first)
    );
    assert_eq!(
        state.join("gone").exists(),
        !rm_fails,
        "지움 갈래가 뜻과 다르다(rm_fails={rm_fails})\n{}",
        context(&first)
    );
    assert!(
        !first_ok,
        "원장 치명 오류 뒤 루프가 성공으로 끝났다\n{}",
        context(&first)
    );
    assert!(
        first.contains("RUN_LEDGER_FATAL")
            && first.contains("종료 뒤 원장에 컨테이너 처리 결과를 적지 못했다"),
        "보관 전 원장 쓰기 실패가 치명 오류로 나오지 않았다\n{}",
        context(&first)
    );
    assert!(
        first.contains("AGENT_LOOP_STOPPED round=1") && !first.contains("AGENT_ROUND 2"),
        "치명 오류 뒤 다음 회차를 돌았다\n{}",
        context(&first)
    );
    let mut outbox = checkpoints.clone().into_os_string();
    outbox.push(".report-outbox");
    let kept = std::fs::read_dir(PathBuf::from(&outbox))
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0);
    assert_eq!(
        kept,
        0,
        "보관 전 실패인데 보고가 보관됐다({outbox:?})\n{}",
        context(&first)
    );
    assert_ne!(
        job_state(&db, JOB),
        Some(JobState::Completed),
        "{}",
        context(&first)
    );
    let paths = run_ledger::LedgerPaths::for_root(&checkpoints).unwrap();
    let rows = run_ledger::open_for_clear(&paths)
        .expect("원장 열기")
        .expect("원장")
        .rows()
        .unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        rows[0].state,
        RowState::Active,
        "쓰기가 실패했는데 행이 바뀌었다: {rows:?}"
    );
    // 컨테이너 행은 만들 때 "지움 확인 없음(0)" 으로 적힌다(R11e — 컨테이너 행의 NULL 은 받지 않는다). 판정 사실도 적히지 않았다.
    assert_eq!(rows[0].container_removed, Some(false), "{rows:?}");
    assert_eq!(
        rows[0].stopped, None,
        "판정 사실 쓰기가 실패했는데 적혔다: {rows:?}"
    );

    // ── 재기동: 저널 자리를 되살리고 같은 Agent 를 다시 띄운다 — 보고가 없으니 끝났다고 볼 수 없다. LOCAL_BLOCKED · 새 작업을 받지 않는다.
    std::fs::remove_dir(&journal).expect("막아 둔 저널 폴더 지우기");
    let creates_before = std::fs::read_to_string(state.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("create"))
        .count();
    let (_, second) = run_to_end(
        dir.path(),
        &agent_loop(dir.path(), &addr, "1"),
        &[(STATE_ENV, &state)],
    );
    assert!(
        second.contains("RUN_LEDGER_BLOCKED") || second.contains("CONTAINER_INCIDENT_OPEN"),
        "재기동이 막힌 시도를 막지 않았다\n{}",
        context(&second)
    );
    let creates_after = std::fs::read_to_string(state.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("create"))
        .count();
    assert_eq!(
        creates_before,
        creates_after,
        "막힌 노드가 새 컨테이너를 만들었다\n{}",
        context(&second)
    );
    let rows = run_ledger::open_for_clear(&paths)
        .expect("원장 열기")
        .expect("원장")
        .rows()
        .unwrap();
    assert_eq!(
        rows[0].state,
        RowState::LocalBlocked,
        "{rows:?}\n{}",
        context(&second)
    );
    assert_ne!(
        job_state(&db, JOB),
        Some(JobState::Completed),
        "{}",
        context(&second)
    );
    drop(coordinator);
}
