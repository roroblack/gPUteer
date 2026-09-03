//! **저장된 예약에서 만든 Grant 를 Agent 가 실제로 받아들이는가.**
//!
//! ```text
//! submit → import-manifest → import-inventory → plan-job → stage-job
//!   → coordinator-stub --grant-from-control-db  ──TCP──▶  agent-stub
//! ```
//!
//! ★ **무게중심은 서명이 아니라 수용이다.** `issue_grant.rs` 는 우리가
//!   만든 Grant 를 우리가 검증했다 — 같은 코드의 두 방향일 뿐이다. 여기서는
//!   **별도 OS 프로세스인 Agent** 가 자기 규칙(서명·nested Lease 독립
//!   검증·fence watermark·만료·`lease_from_durable_store`)으로 판정한다.
//!
//! ★ Lease 시각을 **지금 기준**으로 잡는다. `stage-job`/`issue-grant`
//!   테스트는 고정 상수를 썼는데(멱등 확인이 목적), 여기서는 Agent 가
//!   자기 시계로 만료를 보므로 미래 상수를 쓰면 잴 수 없다.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use gputeer_crypto::SigningKey;

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) { "gputeer.exe" } else { "gputeer" })
}

/// ★★ **scheduler 의 `node_id` 와 Agent 의 device id 는 다른 이름
///   공간인데, 둘을 잇는 것이 아직 없다.**
///
///   Agent 는 받은 Lease 의 `holder_node_id` 를 자기 `--agent-device-id`
///   와 대조한다. 그런데 그 값은 `stage-job` 이 고른 **inventory 의
///   node_id** 다. 처음에 `node-wire-a` 로 두었더니 Agent 가 거부했다:
///   `LEASE_REJECTED: holder_node_id 가 이 Agent 가 아니다`.
///
///   그래서 **오늘은 운영자가 inventory 의 `node_id` 를 그 노드에서
///   도는 Agent 의 device id 와 같게 선언해야** 이 경로가 이어진다.
///   그건 제약이지 설계가 아니다 — 여러 evidence 가 "authoritative
///   device→member projection" 과 "node/device/session routing" 이
///   없다고 적어 둔 것의 구체적인 얼굴이다.
///
///   이 테스트는 그 제약을 **지키는 값으로** 돌아간다. 숨기려는 게
///   아니라, 지금 이어지는 유일한 방법이 이것임을 코드로 남긴다.
const NODE: &str = "01JAGENTWIRE00000000001";
const GPU: &str = "01JAGENTWIRE00000000001-gpu-0";
const JOB: &str = "01JJOBWIRE0000000000001";
const SUBMITTER: &str = "01JSUBMITTERWIRE00000001";
const SEED: &str = "99999999999999999999999999999999999999999999999999999999999999cc";
const OWNER: &str = "owner-wire";
/// Coordinator 의 **장치 식별자**. Agent 가 Grant 의
/// `coordinator_device_id` 를 이 값과 대조하므로 `stage-job` 의
/// `--coordinator-id` 와 반드시 같아야 한다.
const COORDINATOR: &str = "01JCOORDINATORWIRE000001";
/// ★ `NODE` 와 **같은 값**이어야 한다 — 위 주석 참조.
const AGENT_DEVICE: &str = NODE;
const COORD_SEED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaadd";
const AGENT_SEED: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbee";
const ATTEMPT: &str = "01JATTEMPTWIRE00000000001";
const LEASE: &str = "01JLEASEWIRE0000000000001";
const GRANT: &str = "01JGRANTWIRE0000000000001";
const AXES: &str = "vram,gpu_count,cpu,ram,workspace";

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
    let out = Command::new(cli_bin()).args(args).output().expect("gputeer 실행");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// 다섯 명령으로 예약까지 만든다. Lease 시각은 **지금 기준**이다.
fn staged_control_db(dir: &Path) -> PathBuf {
    let db = dir.join("control.sqlite3");

    let node_key = pub_hex("2121212121212121212121212121212121212121212121212121212121212121");
    let observed = now_unix_ms();
    let bootstrap = dir.join("bootstrap.json");
    std::fs::write(
        &bootstrap,
        format!(
            r#"{{
  "schema_version": 1,
  "agents": [
    {{
      "registry": {{
        "node_id": "{NODE}", "device_id": "device-wire",
        "owner_member_id": "{OWNER}", "verifying_key_hex": "{node_key}",
        "node_state": "ONLINE", "risk_state": "NORMAL",
        "security_tier": "S2", "isolation_class": "CONTAINED",
        "key_protection": "K1"
      }},
      "inventory": {{
        "inventory_revision": 1, "observed_at_unix_ms": {observed},
        "gpus": [{{ "gpu_id": "{GPU}", "model": "RTX 4070 SUPER",
                    "healthy": true, "available_vram_bytes": 12884901888 }}],
        "available_cpu_cores": 16, "available_ram_bytes": 34359738368,
        "available_workspace_bytes": 107374182400,
        "allowed_workload_classes": ["TRAINING"],
        "third_party_workloads_opt_in": true
      }}
    }}
  ]
}}"#
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

    let manifest = dir.join("manifest.pb");
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
    let (ok, out) = run_cli(&[
        "submit", "--job-id", JOB, "--entrypoint", "python",
        "--submitter-device-id", SUBMITTER, "--submitter-seed", SEED,
        "--issued-at-unix-ms", &issued, "--expires-at-unix-ms", &expires,
        "--out", manifest.to_str().unwrap(),
        "--workload-class", "TRAINING", "--side-effect-class", "PURE",
        "--dataset-sensitivity", "INTERNAL", "--minimum-security-tier", "S2",
        "--minimum-isolation-class", "CONTAINED", "--minimum-key-protection", "K1",
        "--gpu-count", "1", "--gpu-min-vram-bytes", "8589934592",
    ]);
    assert!(ok, "submit 실패: {out}");

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

    let (ok, out) = run_cli(&[
        "import-manifest", "--manifest", manifest.to_str().unwrap(),
        "--submitter-keyring", keyring.to_str().unwrap(),
        "--job-db", db.to_str().unwrap(),
        "--idempotency-key", "0102030405060708090a0b0c0d0e0f10",
        "--i-understand-plaintext-keyring-is-unsafe", "true",
    ]);
    assert!(ok, "import-manifest 실패: {out}");

    let (ok, out) = run_cli(&[
        "plan-job", "--job-id", JOB, "--control-db", db.to_str().unwrap(),
        "--submitter-keyring", keyring.to_str().unwrap(),
        "--submitter-member", OWNER, "--max-snapshot-age-ms", "86400000",
        "--i-understand-plaintext-keyring-is-unsafe", "true",
    ]);
    assert!(ok, "plan-job 실패: {out}");

    // ★ 지금 기준 — Agent 가 자기 시계로 만료를 본다.
    //
    //   ★ 처음에는 `now - 1000` 을 썼다가 저장소가 거부했다:
    //     `staging clock moved backwards: issued=..., queued=...`.
    //     **Lease 발급 시각은 Job 이 큐에 들어간 시각보다 앞설 수 없다** —
    //     방금 `plan-job` 이 큐에 넣었으므로 과거를 주면 안 된다.
    //     몰랐던 계약이고 이 테스트가 알려 줬다.
    let now = now_unix_ms();
    let lease_issued = now.to_string();
    let lease_renew = (now + 300_000).to_string();
    let lease_expires = (now + 600_000).to_string();
    let (ok, out) = run_cli(&[
        "stage-job", "--job-id", JOB, "--control-db", db.to_str().unwrap(),
        "--submitter-keyring", keyring.to_str().unwrap(),
        "--submitter-member", OWNER, "--max-snapshot-age-ms", "86400000",
        "--best-fit-axes", AXES,
        "--coordinator-id", COORDINATOR, "--coordinator-term", "7",
        "--attempt-id", ATTEMPT, "--lease-id", LEASE,
        "--operation-key", "aa0102030405060708090a0b0c0d0e0f",
        "--lease-issued-at-unix-ms", &lease_issued,
        "--lease-renew-after-unix-ms", &lease_renew,
        "--lease-expires-at-unix-ms", &lease_expires,
        "--lease-max-total-duration-seconds", "86400",
        "--i-understand-plaintext-keyring-is-unsafe", "true",
    ]);
    assert!(ok, "stage-job 실패: {out}");
    db
}

struct Spawned {
    child: Child,
    stdout: thread::JoinHandle<String>,
    stderr: thread::JoinHandle<String>,
}

fn drain(child: &mut Child) -> (thread::JoinHandle<String>, thread::JoinHandle<String>) {
    let out = child.stdout.take().expect("piped stdout");
    let err = child.stderr.take().expect("piped stderr");
    (
        thread::spawn(move || {
            let mut s = String::new();
            let _ = BufReader::new(out).read_to_string(&mut s);
            s
        }),
        thread::spawn(move || {
            let mut s = String::new();
            let _ = BufReader::new(err).read_to_string(&mut s);
            s
        }),
    )
}

/// Coordinator 를 띄우고 `READY <addr>` 를 기다린다.
fn spawn_coordinator(db: &Path, fence_dir: &Path) -> (Spawned, String) {
    let lease_db = fence_dir.join("coordinator-lease.sqlite3");
    let mut child = Command::new(cli_bin())
        .args([
            "coordinator-stub",
            "--listen",
            "127.0.0.1:0",
            "--own-seed",
            COORD_SEED,
            "--peer-pubkey",
            &pub_hex(AGENT_SEED),
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            AGENT_DEVICE,
            "--grant-id",
            GRANT,
            "--attempt-id",
            ATTEMPT,
            "--lease-id",
            LEASE,
            "--job-id",
            JOB,
            "--lease-db",
            lease_db.to_str().unwrap(),
            // ★ 이 조각의 대상 — 저장된 예약에서 조립한다.
            "--grant-from-control-db",
            db.to_str().unwrap(),
            "--stored-grant-job-id",
            JOB,
            "--stored-grant-attempt-id",
            ATTEMPT,
            "--stored-grant-lease-id",
            LEASE,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator-stub spawn");

    // READY 한 줄만 먼저 읽는다.
    let stdout_pipe = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    let stdout = thread::spawn(move || {
        let mut reader = BufReader::new(stdout_pipe);
        let mut ready = String::new();
        let _ = reader.read_line(&mut ready);
        let _ = tx.send(ready.clone());
        let mut rest = String::new();
        let _ = reader.read_to_string(&mut rest);
        format!("{ready}{rest}")
    });
    let err_pipe = child.stderr.take().expect("piped stderr");
    let stderr = thread::spawn(move || {
        let mut s = String::new();
        let _ = BufReader::new(err_pipe).read_to_string(&mut s);
        s
    });

    let ready = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("Coordinator 가 READY 를 안 냈다");
    let addr = ready
        .trim()
        .strip_prefix("READY ")
        .unwrap_or_else(|| panic!("READY 줄이 아니다: {ready:?}"))
        .to_string();
    (
        Spawned {
            child,
            stdout,
            stderr,
        },
        addr,
    )
}

fn finish(mut s: Spawned, label: &str) -> (bool, String) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match s.child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = s.stdout.join().unwrap_or_default();
                let err = s.stderr.join().unwrap_or_default();
                return (status.success(), format!("{out}{err}"));
            }
            None if Instant::now() >= deadline => {
                let _ = s.child.kill();
                panic!("{label} 가 60초 안에 끝나지 않았다");
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────

/// ★★ **저장된 예약으로 만든 Grant 를 별도 프로세스인 Agent 가 받아들인다.**
#[test]
fn an_agent_process_accepts_a_grant_built_from_the_stored_reservation() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = staged_control_db(dir.path());
    let (coordinator, addr) = spawn_coordinator(&db, dir.path());

    let fence_db = dir.path().join("agent-fence.sqlite3");
    let mut agent = Command::new(cli_bin())
        .args([
            "agent-stub",
            "--connect",
            &addr,
            "--own-seed",
            AGENT_SEED,
            "--peer-pubkey",
            &pub_hex(COORD_SEED),
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            AGENT_DEVICE,
            "--fence-db",
            fence_db.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent-stub spawn");
    let (a_out, a_err) = drain(&mut agent);
    let agent_status = agent.wait().expect("agent wait");
    let agent_output = format!(
        "{}{}",
        a_out.join().unwrap_or_default(),
        a_err.join().unwrap_or_default()
    );
    let (_, coordinator_output) = finish(coordinator, "coordinator-stub");

    assert!(
        agent_status.success(),
        "Agent 가 저장된 예약의 Grant 를 거부했다\n--- agent ---\n{agent_output}\n--- coordinator ---\n{coordinator_output}"
    );
    assert!(
        agent_output.contains("RESULT ok=true"),
        "Agent 가 성공을 보고하지 않았다: {agent_output}"
    );
}

/// ★ **대조 — 예약이 없으면 Coordinator 가 Grant 를 못 만든다.**
///
/// 이게 없으면 위 테스트는 "어떤 Grant 든 Agent 가 받는다" 로도 통과한다.
/// 저장된 예약을 안 만든 빈 DB 를 주면 발급 자체가 거부돼야 한다.
#[test]
fn without_a_stored_reservation_the_coordinator_refuses_to_build_a_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    // 스키마만 있고 예약은 없는 DB.
    let empty = dir.path().join("empty.sqlite3");
    drop(
        gputeer_coordinator::job_store::CoordinatorJobStore::open(&empty)
            .expect("job store 생성"),
    );

    let (coordinator, addr) = spawn_coordinator(&empty, dir.path());
    let fence_db = dir.path().join("agent-fence.sqlite3");
    let mut agent = Command::new(cli_bin())
        .args([
            "agent-stub",
            "--connect",
            &addr,
            "--own-seed",
            AGENT_SEED,
            "--peer-pubkey",
            &pub_hex(COORD_SEED),
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            AGENT_DEVICE,
            "--fence-db",
            fence_db.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent-stub spawn");
    let (a_out, a_err) = drain(&mut agent);
    let agent_status = agent.wait().expect("agent wait");
    let agent_output = format!(
        "{}{}",
        a_out.join().unwrap_or_default(),
        a_err.join().unwrap_or_default()
    );
    let (_, coordinator_output) = finish(coordinator, "coordinator-stub");

    assert!(
        !agent_status.success(),
        "예약이 없는데 Agent 가 성공했다\n--- agent ---\n{agent_output}\n--- coordinator ---\n{coordinator_output}"
    );
    assert!(
        coordinator_output.contains("GRANT_REFUSED") || coordinator_output.contains("를 모른다"),
        "Coordinator 가 발급 거부 사유를 안 남겼다: {coordinator_output}"
    );
}
