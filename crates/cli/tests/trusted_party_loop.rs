//! **신뢰망 파티 — 노드 하나에 작업 둘이 차례로 끝까지 간다.**
//!
//! ```text
//! 작업 A · B 를 큐에 올린다 -> A 를 예약 -> Coordinator ─TCP─ Agent (실행 · 종료 보고)
//!   -> 보고 저장 · 시도 종료 · Job 완료 · 예약 해제가 한 커밋
//!   -> B 를 **같은 노드에** 예약(A 가 풀렸기 때문에 가능) -> 다시 실행 · 보고 -> 둘 다 COMPLETED
//! ```
//!
//! ★ 2026-09-23 (신뢰망 남은 일 B). 전에는 A 가 끝나도 예약이 남아 B 가 영영 그 노드에 못 들어갔다 —
//!   대조군(`without_the_release_switch_the_second_job_cannot_take_the_node`)이 그 상태를 고정한다.
//!
//! ★ **Windows 전용이다** — 실행 관문이 리눅스에서는 cgroup 위임을 요구한다(`grant_over_wire.rs` 와 같다).
#![cfg(windows)]

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use gputeer_coordinator::job_store::{CoordinatorJobStore, JobState, RunTerminal};
use gputeer_crypto::SigningKey;

const NODE: &str = "01JAGENTLOOP00000000001";
const GPU: &str = "01JAGENTLOOP00000000001-gpu-0";
const SUBMITTER: &str = "01JSUBMITTERLOOP00000001";
const SEED: &str = "99999999999999999999999999999999999999999999999999999999999999cd";
const OWNER: &str = "owner-loop";
const COORDINATOR: &str = "01JCOORDINATORLOOP000001";
const COORD_SEED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaee";
const AGENT_SEED: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbff";
const AXES: &str = "vram,gpu_count,cpu,ram,workspace";

struct Round {
    job: &'static str,
    attempt: &'static str,
    lease: &'static str,
    grant: &'static str,
    operation_key: &'static str,
    idempotency_key: &'static str,
}

const A: Round = Round {
    job: "01JJOBLOOPA000000000001",
    attempt: "01JATTEMPTLOOPA0000000001",
    lease: "01JLEASELOOPA000000000001",
    grant: "01JGRANTLOOPA000000000001",
    operation_key: "a10102030405060708090a0b0c0d0e0f",
    idempotency_key: "a1a2030405060708090a0b0c0d0e0f10",
};
const B: Round = Round {
    job: "01JJOBLOOPB000000000001",
    attempt: "01JATTEMPTLOOPB0000000001",
    lease: "01JLEASELOOPB000000000001",
    grant: "01JGRANTLOOPB000000000001",
    operation_key: "b10102030405060708090a0b0c0d0e0f",
    idempotency_key: "b1b2030405060708090a0b0c0d0e0f10",
};

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

struct Party {
    dir: tempfile::TempDir,
    db: PathBuf,
    keyring: PathBuf,
}

/// 노드 하나를 등록하고, 작업 둘을 **LOCAL 내구성으로** 제출해 큐에 올린다.
fn party_with_two_queued_jobs() -> Party {
    party_with_two_queued_jobs_at("LOCAL")
}

fn party_with_two_queued_jobs_at(durability: &str) -> Party {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    let node_key = pub_hex("2121212121212121212121212121212121212121212121212121212121212121");
    let bootstrap = dir.path().join("bootstrap.json");
    std::fs::write(
        &bootstrap,
        format!(
            r#"{{
  "schema_version": 1,
  "agents": [
    {{
      "registry": {{
        "node_id": "{NODE}", "device_id": "device-loop",
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
}}"#,
            observed = now_unix_ms()
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

    let keyring = dir.path().join("submitters.keyring");
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

    let party = Party { dir, db, keyring };
    for round in [&A, &B] {
        submit_and_plan(&party, round, durability);
    }
    party
}

fn submit_and_plan(party: &Party, round: &Round, durability: &str) {
    let manifest = party.dir.path().join(format!("{}.pb", round.job));
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
    let entrypoint = cmd_exe();
    let (ok, out) = run_cli(&[
        "submit",
        "--job-id",
        round.job,
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
        durability,
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
        party.keyring.to_str().unwrap(),
        "--job-db",
        party.db.to_str().unwrap(),
        "--idempotency-key",
        round.idempotency_key,
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "import-manifest 실패: {out}");
    let (ok, out) = run_cli(&[
        "plan-job",
        "--job-id",
        round.job,
        "--control-db",
        party.db.to_str().unwrap(),
        "--submitter-keyring",
        party.keyring.to_str().unwrap(),
        "--submitter-member",
        OWNER,
        "--max-snapshot-age-ms",
        "86400000",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "plan-job 실패: {out}");
}

fn stage(party: &Party, round: &Round) -> (bool, String) {
    let now = now_unix_ms();
    let issued = now.to_string();
    let renew = (now + 300_000).to_string();
    let expires = (now + 600_000).to_string();
    run_cli(&[
        "stage-job",
        "--job-id",
        round.job,
        "--control-db",
        party.db.to_str().unwrap(),
        "--submitter-keyring",
        party.keyring.to_str().unwrap(),
        "--submitter-member",
        OWNER,
        "--max-snapshot-age-ms",
        "86400000",
        "--best-fit-axes",
        AXES,
        "--coordinator-id",
        COORDINATOR,
        "--coordinator-term",
        "7",
        "--attempt-id",
        round.attempt,
        "--lease-id",
        round.lease,
        "--operation-key",
        round.operation_key,
        "--lease-issued-at-unix-ms",
        &issued,
        "--lease-renew-after-unix-ms",
        &renew,
        "--lease-expires-at-unix-ms",
        &expires,
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ])
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

/// 한 작업을 실제 프로세스 둘로 돌린다 — Coordinator(저장된 예약 lane) + 실행하는 Agent.
fn run_round(party: &Party, round: &Round, release_switch: bool) -> (String, String) {
    run_round_with(party, round, release_switch, &[])
}

fn run_round_with(
    party: &Party,
    round: &Round,
    release_switch: bool,
    agent_extra: &[&str],
) -> (String, String) {
    let lease_db = party.dir.path().join("coordinator-lease.sqlite3");
    let mut args: Vec<&str> = vec![
        "coordinator-stub",
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        COORD_SEED,
        "--coordinator-device-id",
        COORDINATOR,
        "--agent-device-id",
        NODE,
        "--grant-id",
        round.grant,
        "--attempt-id",
        round.attempt,
        "--lease-id",
        round.lease,
        "--job-id",
        round.job,
        "--lease-db",
        lease_db.to_str().unwrap(),
        "--grant-from-control-db",
        party.db.to_str().unwrap(),
        "--stored-grant-job-id",
        round.job,
        "--stored-grant-attempt-id",
        round.attempt,
        "--stored-grant-lease-id",
        round.lease,
        "--submitter-keyring",
        party.keyring.to_str().unwrap(),
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
        "--expect-attempt-reports",
        "1",
    ];
    if release_switch {
        args.extend(["--release-on-exit-report", "true"]);
    }
    let agent_pub = pub_hex(AGENT_SEED);
    args.extend(["--peer-pubkey", &agent_pub]);
    let mut coordinator = Command::new(cli_bin())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("coordinator-stub spawn");
    let stdout_pipe = coordinator.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    let c_out = thread::spawn(move || {
        let mut reader = BufReader::new(stdout_pipe);
        let mut ready = String::new();
        let _ = reader.read_line(&mut ready);
        let _ = tx.send(ready.clone());
        let mut rest = String::new();
        let _ = reader.read_to_string(&mut rest);
        format!("{ready}{rest}")
    });
    let err_pipe = coordinator.stderr.take().expect("piped stderr");
    let c_err = thread::spawn(move || {
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

    let fence_db = party.dir.path().join("agent-fence.sqlite3");
    let checkpoint_root = party.dir.path().join("agent-checkpoints");
    let coordinator_pub = pub_hex(COORD_SEED);
    let submitter_pub = pub_hex(SEED);
    let mut agent = Command::new(cli_bin())
        .args([
            "agent-stub",
            "--connect",
            &addr,
            "--own-seed",
            AGENT_SEED,
            "--peer-pubkey",
            &coordinator_pub,
            "--coordinator-device-id",
            COORDINATOR,
            "--agent-device-id",
            NODE,
            "--fence-db",
            fence_db.to_str().unwrap(),
            "--checkpoint-root",
            checkpoint_root.to_str().unwrap(),
            "--submitter-pubkey",
            &submitter_pub,
            "--i-understand-this-executes-untrusted-code",
            "true",
            "--send-attempt-report",
            "true",
        ])
        .args(agent_extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent-stub spawn");
    let (a_out, a_err) = drain(&mut agent);
    agent.wait().expect("agent wait");
    let agent_output = format!(
        "{}{}",
        a_out.join().unwrap_or_default(),
        a_err.join().unwrap_or_default()
    );

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match coordinator.try_wait().expect("try_wait") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = coordinator.kill();
                panic!("coordinator-stub 가 60초 안에 끝나지 않았다");
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    }
    let coordinator_output = format!(
        "{}{}",
        c_out.join().unwrap_or_default(),
        c_err.join().unwrap_or_default()
    );
    (agent_output, coordinator_output)
}

fn job(party: &Party, job_id: &str) -> gputeer_coordinator::job_store::StoredJob {
    CoordinatorJobStore::open(&party.db)
        .expect("job store")
        .get(job_id)
        .expect("조회")
        .expect("Job 이 있다")
}

/// ★★ 노드 하나 · 작업 둘 — 둘 다 끝까지 간다.
#[test]
fn one_node_runs_two_jobs_back_to_back_when_the_exit_report_releases_it() {
    let party = party_with_two_queued_jobs();

    let (ok, out) = stage(&party, &A);
    assert!(ok, "A 예약 실패: {out}");
    let (agent, coordinator) = run_round(&party, &A, true);
    let both = format!("--- agent ---\n{agent}\n--- coordinator ---\n{coordinator}");
    assert!(
        agent.contains("WORKLOAD_RESULT ok=true"),
        "A 가 돌지 않았다\n{both}"
    );
    assert!(
        coordinator.contains(&format!(
            "RESERVATION_RELEASED node_id={NODE} attempt_id={}",
            A.attempt
        )),
        "A 의 보고로 예약이 풀리지 않았다\n{both}"
    );
    assert!(
        coordinator.contains(&format!(
            "GRANT_ACCEPTED_RECORDED attempt_id={} outcome=recorded",
            A.attempt
        )),
        "ACK 를 관측으로 적지 않았다
{both}"
    );
    let a = job(&party, A.job);
    assert_eq!(
        a.state,
        JobState::Completed,
        "A 가 완료로 적히지 않았다\n{both}"
    );
    assert_eq!(a.run_terminal, Some(RunTerminal::AttemptCompleted));
    assert!(
        a.running_at_unix_ms.is_some(),
        "ACK 를 받은 시각(RUNNING 진입)이 없다
{both}"
    );

    // ★ A 가 풀렸으므로 B 가 **같은 노드에** 들어간다.
    let (ok, out) = stage(&party, &B);
    assert!(ok, "A 가 끝났는데 B 가 그 노드를 못 잡았다: {out}");
    let (agent, coordinator) = run_round(&party, &B, true);
    let both = format!("--- agent ---\n{agent}\n--- coordinator ---\n{coordinator}");
    assert!(
        agent.contains("WORKLOAD_RESULT ok=true"),
        "B 가 돌지 않았다\n{both}"
    );
    assert!(
        coordinator.contains("RESERVATION_RELEASED"),
        "B 의 보고로 예약이 풀리지 않았다\n{both}"
    );
    assert_eq!(job(&party, B.job).state, JobState::Completed, "{both}");
    assert!(
        gputeer_coordinator::staging_store::CoordinatorStagingStore::open(&party.db)
            .unwrap()
            .get_node_reservation(NODE)
            .unwrap()
            .is_none(),
        "둘 다 끝났는데 예약이 남았다"
    );
}

/// 대조군 — 스위치를 끄면 A 가 끝나도(Job 은 COMPLETED) 예약이 남아 B 가 그 노드를 못 잡는다.
///
/// ★ 전에는 이것이 **유일한 동작**이었다. 스위치가 해제를 만든다는 것을 이 대조가 보인다.
#[test]
fn without_the_release_switch_the_second_job_cannot_take_the_node() {
    let party = party_with_two_queued_jobs();
    let (ok, out) = stage(&party, &A);
    assert!(ok, "A 예약 실패: {out}");
    let (agent, coordinator) = run_round(&party, &A, false);
    let both = format!("--- agent ---\n{agent}\n--- coordinator ---\n{coordinator}");
    assert!(
        agent.contains("WORKLOAD_RESULT ok=true"),
        "A 가 돌지 않았다\n{both}"
    );
    assert!(
        coordinator.contains("RESERVATION_KEPT reason=RELEASE_SWITCH_OFF"),
        "스위치가 꺼졌다는 기록이 없다\n{both}"
    );
    assert_eq!(job(&party, A.job).state, JobState::Completed, "{both}");
    let (ok, out) = stage(&party, &B);
    assert!(!ok, "예약이 남았는데 B 가 그 노드를 잡았다: {out}");
}

/// 음성 — 복제(MIRRORED)를 요구한 작업은 이 풀이 그 요구를 못 채우므로 **완료해도 예약을 풀지 않는다.**
///
/// ★ 보고는 저장되고 Job 도 COMPLETED 로 적힌다 — 풀지 않는 것은 예약뿐이고, 그 사유를 한 줄로 남긴다.
#[test]
fn a_job_that_asked_for_replication_keeps_its_reservation() {
    let party = party_with_two_queued_jobs_at("MIRRORED");
    let (ok, out) = stage(&party, &A);
    assert!(ok, "A 예약 실패: {out}");
    let (agent, coordinator) = run_round(&party, &A, true);
    let both = format!(
        "--- agent ---
{agent}
--- coordinator ---
{coordinator}"
    );
    assert!(
        agent.contains("WORKLOAD_RESULT ok=true"),
        "A 가 돌지 않았다
{both}"
    );
    assert!(
        coordinator.contains("RESERVATION_KEPT reason=ARTIFACT_DURABILITY_DURABILITY_MIRRORED"),
        "복제를 요구한 작업의 예약을 풀었다
{both}"
    );
    assert!(
        gputeer_coordinator::staging_store::CoordinatorStagingStore::open(&party.db)
            .unwrap()
            .get_node_reservation(NODE)
            .unwrap()
            .is_some(),
        "예약이 없어졌다"
    );
}

/// ★ 결함 131 (2026-09-23) — 수신 확인을 **요구하는** Agent 는 그것을 보내지 않는 Coordinator(풀 모드가 아님 · 옛 버전)
/// 앞에서 **실행하지 않는다**(fail-closed). ACK 가 받아들여졌는지 모르는 채 작업을 돌리지 않는다.
#[test]
fn an_agent_that_requires_an_ack_receipt_does_not_run_without_one() {
    let party = party_with_two_queued_jobs();
    let (ok, out) = stage(&party, &A);
    assert!(ok, "A 예약 실패: {out}");
    let (agent, coordinator) =
        run_round_with(&party, &A, false, &["--require-ack-receipt", "true"]);
    let both = format!(
        "--- agent ---
{agent}
--- coordinator ---
{coordinator}"
    );
    assert!(
        agent.contains("ACK_RECEIPT_MISSING"),
        "수신 확인 없이 넘어갔다
{both}"
    );
    assert!(
        !agent.contains("WORKLOAD_SPAWNED") && !agent.contains("WORKLOAD_RESULT"),
        "수신 확인 없이 작업을 돌렸다
{both}"
    );
}
