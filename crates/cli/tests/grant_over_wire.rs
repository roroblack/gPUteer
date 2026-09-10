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
    staged_control_db_running(dir, "python", None)
}

/// 같은 준비를 하되 Manifest 의 entrypoint·인자를 고른다.
///
/// ★ 기존 두 테스트는 실행하지 않으므로 `python` 이 무엇이든 상관없다.
///   실제로 실행하는 테스트는 **확실히 있고 바로 끝나는** 명령을 줘야 한다
///   — 인자 없는 `python` 은 표준입력을 기다릴 수 있다.
fn staged_control_db_running(dir: &Path, entrypoint: &str, args_csv: Option<&str>) -> PathBuf {
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
    let mut submit_args: Vec<&str> = vec![
        "submit", "--job-id", JOB, "--entrypoint", entrypoint,
        "--submitter-device-id", SUBMITTER, "--submitter-seed", SEED,
        "--issued-at-unix-ms", &issued, "--expires-at-unix-ms", &expires,
        "--out", manifest.to_str().unwrap(),
        "--workload-class", "TRAINING", "--side-effect-class", "PURE",
        "--dataset-sensitivity", "INTERNAL", "--minimum-security-tier", "S2",
        "--minimum-isolation-class", "CONTAINED", "--minimum-key-protection", "K1",
        "--gpu-count", "1", "--gpu-min-vram-bytes", "8589934592",
        // ★ 2026-09-10 — 이 셋을 안 주고 있었다. 그전에는 변환기가
        //   생략을 `Some(0)` 으로 채워 줘서 통과했다. 독립 검수가
        //   그 채움을 지적해 이제 거부한다 — 그래서 여기서 선언한다.
        "--cpu-cores", "4", "--ram-bytes", "8589934592",
        "--workspace-bytes", "10737418240",
    ];
    if let Some(args_csv) = args_csv {
        submit_args.extend(["--args", args_csv]);
    }
    let (ok, out) = run_cli(&submit_args);
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
fn spawn_coordinator(db: &Path, fence_dir: &Path, extra: &[&str]) -> (Spawned, String) {
    let lease_db = fence_dir.join("coordinator-lease.sqlite3");
    // ★ 저장된 예약 lane 은 제출자 keyring 을 요구한다(§A1 1.5 선행). 준비 코드가
    //   만든 것이 있으면 그것을, 없으면(빈 DB 대조군) 빈 keyring 을 쓴다 — 거부
    //   사유가 keyring 이 아니라 **예약 부재**로 남아야 대조군이 뜻을 가진다.
    let keyring = fence_dir.join("submitters.keyring");
    if !keyring.exists() {
        gputeer_crypto::PersistentKeyring::new(
            &keyring,
            gputeer_crypto::KeyProtection::K0Plaintext,
            gputeer_crypto::PlaintextPolicy::Allow,
        )
        .expect("빈 keyring 생성")
        .save()
        .expect("빈 keyring 저장");
    }
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
            "--submitter-keyring",
            keyring.to_str().unwrap(),
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ])
        .args(extra)
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
    let (coordinator, addr) = spawn_coordinator(&db, dir.path(), &[]);

    let fence_db = dir.path().join("agent-fence.sqlite3");
    let submitter_pub = pub_hex(SEED);
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
            // ★ 저장된 Grant 가 이제 제출자 서명 Manifest 를 싣는다(§A1 1.5 선행).
            //   Agent 는 검증할 키가 없으면 그 Manifest 를 거부한다.
            "--submitter-pubkey",
            &submitter_pub,
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

    let (coordinator, addr) = spawn_coordinator(&empty, dir.path(), &[]);
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

// ─────────────────────────────────────────────────────────────────────
// 종료 보고가 **별도 프로세스 둘 사이의 실제 소켓**을 건너 저장되는가
// (`docs/plans/_열린_작업.md` §A1 1.5)
// ─────────────────────────────────────────────────────────────────────

/// Windows 에 확실히 있는 실행 파일.
#[cfg(windows)]
fn cmd_exe() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    format!(r"{root}\System32\cmd.exe")
}

/// 실행을 켠 Agent 를 띄우고 끝날 때까지 기다린다. `send_report` 만 다르다.
#[cfg(windows)]
fn run_executing_agent(addr: &str, dir: &Path, send_report: bool) -> (bool, String) {
    let fence_db = dir.join("agent-fence.sqlite3");
    // ★ 기본값은 `%TEMP%` 아래 매번 새 이름이라 테스트가 끝나도 남는다.
    let checkpoint_root = dir.join("agent-checkpoints");
    let coordinator_pub = pub_hex(COORD_SEED);
    let submitter_pub = pub_hex(SEED);
    let mut args: Vec<&str> = vec![
        "agent-stub",
        "--connect",
        addr,
        "--own-seed",
        AGENT_SEED,
        "--peer-pubkey",
        &coordinator_pub,
        "--coordinator-device-id",
        COORDINATOR,
        "--agent-device-id",
        AGENT_DEVICE,
        "--fence-db",
        fence_db.to_str().unwrap(),
        "--checkpoint-root",
        checkpoint_root.to_str().unwrap(),
        "--submitter-pubkey",
        &submitter_pub,
        "--i-understand-this-executes-untrusted-code",
        "true",
    ];
    if send_report {
        args.extend(["--send-attempt-report", "true"]);
    }
    let mut agent = Command::new(cli_bin())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent-stub spawn");
    let (a_out, a_err) = drain(&mut agent);
    let status = agent.wait().expect("agent wait");
    (
        status.success(),
        format!(
            "{}{}",
            a_out.join().unwrap_or_default(),
            a_err.join().unwrap_or_default()
        ),
    )
}

/// ★★ **덫 — 오늘은 저장된 예약에서 시작하면 보고할 종료가 없다.**
///
/// §A1 1.5("종료 보고가 별도 프로세스 둘 사이의 실제 소켓을 건너 저장되는가")
/// 를 재려고 실행과 보고를 켰더니 Agent 가 **아무것도 실행하지 않았다.**
/// 저장된 예약에서 만든 Grant 에는 Manifest 가 없다 —
/// `grant_from_stored.rs` 가 "안 한다: Manifest 싣기 — 별도 조각" 이라고 적어
/// 둔 그대로다. 실행할 것이 없으니 관측한 종료도 없고, Agent 는 지어내지
/// 않고 거부한다(`ATTEMPT_REPORT_REFUSED`).
///
/// ★ 그래서 1.5 는 "정상 경로 시나리오 하나" 가 아니었다. **Manifest 싣기가
///   먼저다**(결함 리포트 ⑯).
///
/// ★★ **이 fixture 가 Manifest 를 받도록 바뀌면 이 테스트가 깨진다.** 그때
///   지우지 말고 뒤집어라 — 정상 경로 테스트가 볼 것:
///   ```text
///   Agent       WORKLOAD_RESULT ok=true · ATTEMPT_REPORT_SENT
///   Coordinator ATTEMPT_REPORT_STORED
///   DB          get_report_binding(ATTEMPT, NODE) 가 Some 이고, 저장된
///               outcome·fence_epoch·started/finished 가 **보낸 줄의 값과 같다**
///   대조군      보고만 끄면 행이 없다
///   ```
///
/// ★ 재검수 14 — 로그만 보면 판별력이 약하다. Manifest 와 무관한 이유(예: Grant
///   서명 손상)로도 깨지고, 미래 구현이 "`--manifest-file` 을 줄 때만 싣기" 면
///   안 깨질 수 있다. 그래서 **저장된 Grant 자체**도 본다 — Coordinator 가 쓰는
///   것과 같은 함수(`signed_grant_from_stored`)로 만드는 `issue-grant` 를 같은
///   DB 에 돌려 `manifest` 가 비었는지 직접 확인한다. 깨지면 **이유부터** 보라.
///   또 Manifest 가 실려도 보고까지 가려면 결함 ⑱⑲⑳ 이 남아 있다.
///
/// ★ **Windows 전용이다** — 실행 관문이 리눅스에서는 cgroup 을 요구한다.
///
/// ★★ 2026-09-10 — **뒤집었다**(§A1 1.5 선행). 위 문단은 뒤집기 전의 기록이다 — 저장된
///   Grant 가 이제 제출자 서명 Manifest 를 싣는다(발급 시각 기준 재검증 뒤). 아래가 그
///   문단이 적어 둔 정상 경로다. ★ 워크로드가 짧아(`cmd /c exit 0`) 결함 ⑱(ACK 전 실행 ·
///   10초 시한)을 밟지 않는다 — 이 테스트의 통과가 ⑱ 이 풀렸다는 뜻은 아니다.
#[cfg(windows)]
#[test]
fn a_stored_grant_carries_the_manifest_and_the_exit_report_crosses_the_wire() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = staged_control_db_running(dir.path(), &cmd_exe(), Some("/c,exit,0"));
    let (coordinator, addr) =
        spawn_coordinator(&db, dir.path(), &["--expect-attempt-reports", "1"]);
    let (agent_ok, agent_output) = run_executing_agent(&addr, dir.path(), true);
    let (_, coordinator_output) = finish(coordinator, "coordinator-stub");
    let both = format!("--- agent ---\n{agent_output}\n--- coordinator ---\n{coordinator_output}");

    assert!(agent_ok, "Agent 가 실패했다\n{both}");
    assert!(agent_output.contains("MANIFEST_ACCEPTED"), "Agent 가 Manifest 를 받지 않았다\n{both}");
    assert!(
        agent_output.contains("WORKLOAD_RESULT ok=true"),
        "워크로드가 성공하지 않았다\n{both}"
    );
    let sent = agent_output
        .lines()
        .find(|line| line.starts_with("ATTEMPT_REPORT_SENT "))
        .unwrap_or_else(|| panic!("보고를 보내지 않았다\n{both}"));
    assert!(
        coordinator_output.contains("ATTEMPT_REPORT_STORED"),
        "Coordinator 가 저장하지 않았다\n{both}"
    );

    // DB 의 행이 **보낸 줄의 값과 같다**.
    let field = |name: &str| -> String {
        let prefix = format!("{name}=");
        sent.split_whitespace()
            .find_map(|kv| kv.strip_prefix(prefix.as_str()).map(str::to_string))
            .unwrap_or_else(|| panic!("{name} 가 보낸 줄에 없다: {sent}"))
    };
    let store =
        gputeer_coordinator::attempt_report_store::CoordinatorAttemptReportStore::open(&db)
            .expect("저장소 열기");
    let binding = store
        .get_report_binding(ATTEMPT, NODE)
        .expect("조회")
        .unwrap_or_else(|| panic!("보고 행이 없다\n{both}"));
    let report = &binding.report;
    assert_eq!(report.job_id, field("job_id"));
    assert_eq!(report.attempt_id, field("attempt_id"));
    assert_eq!(report.node_id, field("node_id"));
    assert_eq!(report.fence_epoch.to_string(), field("fence_epoch"));
    assert_eq!(report.outcome.to_string(), field("outcome"));
    assert_eq!(report.started_at_unix_ms.to_string(), field("started_at_unix_ms"));
    assert_eq!(report.finished_at_unix_ms.to_string(), field("finished_at_unix_ms"));
    assert_eq!(binding.bound_fence_epoch.to_string(), field("fence_epoch"));

    // ★ 저장된 Grant 자체를 본다 — Agent 는 hash 없음을 통과시키므로
    //   (`verify_nested_manifest`) Agent 성공만으로는 hash 채우기가 빠진 것을 못 잡는다.
    let grant = issue_stored_grant(&db, dir.path());
    let stored = gputeer_coordinator::job_store::CoordinatorJobStore::open(&db)
        .expect("job store")
        .get_manifest_binding(JOB)
        .expect("binding 조회")
        .expect("binding 이 있다");
    assert_eq!(grant.manifest.as_ref(), Some(&stored.manifest), "저장된 Manifest 가 그대로 실리지 않았다");
    let hash = grant.manifest_hash.as_ref().expect("manifest_hash 가 없다");
    assert_eq!(hash.algo, 1, "hash 알고리즘이 BLAKE3-256(1) 이 아니다");
    assert_eq!(hash.value, stored.manifest_hash.to_vec(), "hash 가 저장된 값과 다르다");
}

/// ★ 대조군 — 보고만 끄면 워크로드는 **성공하는데** 행이 없다.
///   실행 자체가 실패해서 행이 없는 경우와 가르려고 `WORKLOAD_RESULT ok=true` 를 같이
///   본다(설계 논의 36).
#[cfg(windows)]
#[test]
fn with_the_report_off_the_workload_runs_but_no_report_row_is_stored() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = staged_control_db_running(dir.path(), &cmd_exe(), Some("/c,exit,0"));
    let (coordinator, addr) = spawn_coordinator(&db, dir.path(), &[]);
    let (agent_ok, agent_output) = run_executing_agent(&addr, dir.path(), false);
    let (_, coordinator_output) = finish(coordinator, "coordinator-stub");
    let both = format!("--- agent ---\n{agent_output}\n--- coordinator ---\n{coordinator_output}");

    assert!(agent_ok, "Agent 가 실패했다\n{both}");
    assert!(
        agent_output.contains("WORKLOAD_RESULT ok=true"),
        "워크로드가 성공하지 않았다 — 행이 없는 이유가 보고 끄기가 아닐 수 있다\n{both}"
    );
    assert!(!agent_output.contains("ATTEMPT_REPORT_SENT"), "\n{both}");
    assert!(!coordinator_output.contains("ATTEMPT_REPORT_STORED"), "\n{both}");
    let store =
        gputeer_coordinator::attempt_report_store::CoordinatorAttemptReportStore::open(&db)
            .expect("저장소 열기");
    assert!(store.get_report_binding(ATTEMPT, NODE).expect("조회").is_none());
}

/// 저장된 예약에서 `issue-grant` 로 Grant 를 만들어 읽는다 — Coordinator 가 쓰는 것과
/// 같은 함수(`signed_grant_from_stored`)다.
#[cfg(windows)]
fn issue_stored_grant(db: &Path, dir: &Path) -> gputeer_protocol::pb::ExecutionGrant {
    let key = dir.join("coordinator.key");
    std::fs::write(&key, COORD_SEED).expect("키 파일");
    let grant_file = dir.join("stored-grant.pb");
    let now = now_unix_ms();
    let issued = (now + 1_000).to_string();
    let expires = (now + 60_000).to_string();
    let keyring = dir.join("submitters.keyring");
    let (ok, out) = run_cli(&[
        "issue-grant",
        "--job-id", JOB,
        "--control-db", db.to_str().unwrap(),
        "--attempt-id", ATTEMPT,
        "--lease-id", LEASE,
        "--grant-id", GRANT,
        "--grant-issued-at-unix-ms", &issued,
        "--grant-expires-at-unix-ms", &expires,
        "--coordinator-key-file", key.to_str().unwrap(),
        "--out", grant_file.to_str().unwrap(),
        "--submitter-keyring", keyring.to_str().unwrap(),
        "--i-understand-plaintext-keyring-is-unsafe", "true",
    ]);
    assert!(ok, "저장된 예약에서 Grant 를 못 만들었다: {out}");
    <gputeer_protocol::pb::ExecutionGrant as prost::Message>::decode(
        std::fs::read(&grant_file).expect("Grant 읽기").as_slice(),
    )
    .expect("Grant 디코드")
}

/// ★ 결함 ⑯ — 저장된 예약 lane 에 `--manifest-file` 을 주면 **시작 전에** 거부한다.
///
/// 전에는 받아 두고 말없이 버렸다. 관문이 없으면 이 Coordinator 는 READY 를
/// 내고 연결을 기다리다 `--accept-timeout-ms` 뒤에 끝난다 — 그래서 시한을
/// 짧게 준다(관문을 지우는 뮤테이션에서 테스트가 멈추지 않게).
#[test]
fn a_manifest_file_on_the_stored_lane_is_refused_at_startup() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("empty.sqlite3");
    let lease_db = dir.path().join("coordinator-lease.sqlite3");
    let (ok, output) = run_cli(&[
        "coordinator-stub",
        "--listen", "127.0.0.1:0",
        "--own-seed", COORD_SEED,
        "--peer-pubkey", &pub_hex(AGENT_SEED),
        "--coordinator-device-id", COORDINATOR,
        "--agent-device-id", AGENT_DEVICE,
        "--grant-id", GRANT,
        "--attempt-id", ATTEMPT,
        "--lease-id", LEASE,
        "--job-id", JOB,
        "--lease-db", lease_db.to_str().unwrap(),
        "--grant-from-control-db", db.to_str().unwrap(),
        "--stored-grant-job-id", JOB,
        "--stored-grant-attempt-id", ATTEMPT,
        "--stored-grant-lease-id", LEASE,
        "--manifest-file", dir.path().join("m.pb").to_str().unwrap(),
        "--accept-timeout-ms", "2000",
    ]);
    assert!(!ok, "두 플래그를 같이 줬는데 시작했다: {output}");
    assert!(
        output.contains("STARTUP_REFUSED") && output.contains("--manifest-file"),
        "시작은 막았는데 이유가 이 관문이 아니다: {output}"
    );
    assert!(!output.contains("READY"), "소켓을 연 뒤에 거부했다: {output}");
}
