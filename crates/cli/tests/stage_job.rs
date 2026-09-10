//! `gputeer stage-job` — **다섯 단계가 실제로 이어지는가.**
//!
//! ```text
//! submit           서명한 Manifest 를 낸다
//! import-manifest  durable job store 에 넣는다              (SUBMITTED)
//! import-inventory 노드 목록을 같은 DB 에 넣는다
//! plan-job         재검증 -> 요구 -> 적격 판정              (QUEUED)
//! stage-job        best-fit -> CAS 예약 + Attempt/Lease     (STAGING)
//! ```
//!
//! ★ `DoD-46` 이 "production 미연결" 로 남긴 `orchestrate` 의 **첫
//!   production 호출자**가 이 명령이다. 그 차단 이유(같은 GPU 중복
//!   선택)는 `DoD-47` 의 revision CAS 가 이미 풀었다.
//!
//! ★ **무게중심은 두 번째 예약이 막히는가**다. 노드 하나짜리 풀에서
//!   두 Job 을 예약하면 뒤엣것은 반드시 실패해야 한다 — 안 그러면 남의
//!   GPU 를 두 Job 이 동시에 잡는다.

use std::path::{Path, PathBuf};
use std::process::Command;

use gputeer_coordinator::job_store::{CoordinatorJobStore, JobState};

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) { "gputeer.exe" } else { "gputeer" })
}

const NODE: &str = "node-stage-a";
const GPU: &str = "node-stage-a-gpu-0";
const SUBMITTER: &str = "01JSUBMITTERSTAGE0000001";
const SEED: &str = "66666666666666666666666666666666666666666666666666666666666666ff";
const OWNER: &str = "owner-stage";
const COORDINATOR: &str = "01JCOORDINATORSTAGE00001";
/// ULID 는 26자다 — `fenced_operation.rs` 가 그 길이를 요구한다.
const ATTEMPT: &str = "01JATTEMPTSTAGE00000000001";
const LEASE: &str = "01JLEASESTAGE000000000001";
const AXES: &str = "vram,gpu_count,cpu,ram,workspace";
/// ★ Lease 시각은 **고정 상수**다. 시계를 읽으면 재시도마다 값이 달라져
///   operation key 의 멱등 약속이 깨진다(통합 테스트가 그걸 잡았다).
const LEASE_ISSUED: &str = "1800000000000";
const LEASE_RENEW: &str = "1800000300000";
const LEASE_EXPIRES: &str = "1800000600000";

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

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_millis() as u64
}

fn write_bootstrap(dir: &Path) -> PathBuf {
    let key: String = gputeer_crypto::SigningKey::from_bytes(&[13u8; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let observed = now_unix_ms();
    let body = format!(
        r#"{{
  "schema_version": 1,
  "agents": [
    {{
      "registry": {{
        "node_id": "{NODE}",
        "device_id": "device-stage",
        "owner_member_id": "{OWNER}",
        "verifying_key_hex": "{key}",
        "node_state": "ONLINE",
        "risk_state": "NORMAL",
        "security_tier": "S2",
        "isolation_class": "CONTAINED",
        "key_protection": "K1"
      }},
      "inventory": {{
        "inventory_revision": 1,
        "observed_at_unix_ms": {observed},
        "gpus": [
          {{
            "gpu_id": "{GPU}",
            "model": "RTX 4070 SUPER",
            "healthy": true,
            "available_vram_bytes": 12884901888
          }}
        ],
        "available_cpu_cores": 16,
        "available_ram_bytes": 34359738368,
        "available_workspace_bytes": 107374182400,
        "allowed_workload_classes": ["TRAINING"],
        "third_party_workloads_opt_in": true
      }}
    }}
  ]
}}"#
    );
    let path = dir.join("bootstrap.json");
    std::fs::write(&path, body).expect("문서 쓰기");
    path
}

fn write_keyring(dir: &Path) -> PathBuf {
    let keyring = dir.join("submitters.keyring");
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&SEED[i * 2..i * 2 + 2], 16).expect("seed hex");
    }
    let mut ring = gputeer_crypto::PersistentKeyring::new(
        &keyring,
        gputeer_crypto::KeyProtection::K0Plaintext,
        gputeer_crypto::PlaintextPolicy::Allow,
    )
    .expect("keyring 생성");
    ring.insert_public(
        SUBMITTER,
        gputeer_crypto::SigningKey::from_bytes(&seed).verifying_key(),
    )
    .expect("공개키 등록");
    ring.save().expect("keyring 저장");
    keyring
}

const DECLARATIONS: [&str; 22] = [
    "--workload-class",
    "TRAINING",
    "--side-effect-class",
    "PURE",
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
    // ★ 2026-09-10 — 변환기가 생략된 자원을 더 이상 0 으로 채우지 않는다.
    "--cpu-cores",
    "4",
    "--ram-bytes",
    "8589934592",
    "--workspace-bytes",
    "10737418240",
];

/// 한 Job 을 `submit` -> `import-manifest` -> `plan-job` 까지 올린다.
fn queue_job(dir: &Path, keyring: &Path, db: &Path, job_id: &str, idem: &str) {
    let manifest = dir.join(format!("{job_id}.pb"));
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
    let mut args: Vec<&str> = vec![
        "submit",
        "--job-id",
        job_id,
        "--entrypoint",
        "python",
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
    ];
    args.extend_from_slice(&DECLARATIONS);
    let (ok, output) = run_cli(&args);
    assert!(ok, "submit 실패: {output}");

    let (ok, output) = run_cli(&[
        "import-manifest",
        "--manifest",
        manifest.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--job-db",
        db.to_str().unwrap(),
        "--idempotency-key",
        idem,
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "import-manifest 실패: {output}");

    let (ok, output) = run_cli(&[
        "plan-job",
        "--job-id",
        job_id,
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
    assert!(ok, "plan-job 실패: {output}");
}

#[allow(clippy::too_many_arguments)]
fn stage(
    keyring: &Path,
    db: &Path,
    job_id: &str,
    attempt: &str,
    lease: &str,
    operation_key: &str,
) -> (bool, String) {
    run_cli(&[
        "stage-job",
        "--job-id",
        job_id,
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
        "1",
        "--attempt-id",
        attempt,
        "--lease-id",
        lease,
        "--operation-key",
        operation_key,
        "--lease-issued-at-unix-ms",
        LEASE_ISSUED,
        "--lease-renew-after-unix-ms",
        LEASE_RENEW,
        "--lease-expires-at-unix-ms",
        LEASE_EXPIRES,
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ])
}

fn job_state(db: &Path, job_id: &str) -> Option<JobState> {
    let store = CoordinatorJobStore::open(db).expect("job store");
    store.get(job_id).expect("조회").map(|job| job.state)
}

/// 한 Job 을 준비된 풀에 올린다.
fn prepared(dir: &Path, job_id: &str) -> (PathBuf, PathBuf) {
    let keyring = write_keyring(dir);
    let db = dir.join("control.sqlite3");
    let bootstrap = write_bootstrap(dir);
    // inventory 를 먼저 넣어야 plan-job 이 후보를 본다.
    let (ok, output) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory 실패: {output}");
    queue_job(dir, &keyring, &db, job_id, "0102030405060708090a0b0c0d0e0f10");
    (keyring, db)
}

/// 노드가 **둘**인 풀을 만든다.
///
/// ★★ 2026-09-10 독립 검수 지적 때문에 생겼다. 노드가 하나면
///   "이미 예약된 노드" 관문이 늘 먼저 걸려서, **`QUEUED` 에서만
///   예약한다는 관문이 한 번도 안 돈다.** 그 관문을 재려면 빈 노드가
///   남아 있어야 한다.
///
/// ★ 문자열을 잘라 붙이지 않는다 — 처음에 그렇게 했다가 JSON 이 깨졌다.
///   `agent_json()` 으로 두 개를 만들어 합친다.
fn prepared_two_nodes(dir: &Path, job_id: &str) -> (PathBuf, PathBuf) {
    let keyring = write_keyring(dir);
    let db = dir.join("control.sqlite3");
    let bootstrap = dir.join("bootstrap_two.json");
    let body = format!(
        "{{
  \"schema_version\": 1,
  \"agents\": [
{},
{}
  ]
}}",
        agent_json(NODE, GPU, "device-stage", 13),
        // ★ 키가 달라야 한다 — inventory 가 신원 충돌을 거부한다(DoD-44).
        //   처음에 같은 키를 썼다가 BOOTSTRAP_REJECTED 를 받았다.
        agent_json("node-stage-b", "node-stage-b-gpu-0", "device-stage-b", 14),
    );
    std::fs::write(&bootstrap, body).expect("두 노드 문서 쓰기");

    let (ok, output) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory 실패(노드 둘): {output}");
    queue_job(dir, &keyring, &db, job_id, "0102030405060708090a0b0c0d0e0f10");
    (keyring, db)
}

/// agent 하나의 JSON. `write_bootstrap` 과 같은 값을 쓴다.
fn agent_json(node_id: &str, gpu_id: &str, device_id: &str, seed_byte: u8) -> String {
    let key: String = gputeer_crypto::SigningKey::from_bytes(&[seed_byte; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let observed = now_unix_ms();
    format!(
        r#"    {{
      "registry": {{
        "node_id": "{node_id}",
        "device_id": "{device_id}",
        "owner_member_id": "{OWNER}",
        "verifying_key_hex": "{key}",
        "node_state": "ONLINE",
        "risk_state": "NORMAL",
        "security_tier": "S2",
        "isolation_class": "CONTAINED",
        "key_protection": "K1"
      }},
      "inventory": {{
        "inventory_revision": 1,
        "observed_at_unix_ms": {observed},
        "gpus": [
          {{
            "gpu_id": "{gpu_id}",
            "model": "RTX 4070 SUPER",
            "healthy": true,
            "available_vram_bytes": 12884901888
          }}
        ],
        "available_cpu_cores": 16,
        "available_ram_bytes": 34359738368,
        "available_workspace_bytes": 107374182400,
        "allowed_workload_classes": ["TRAINING"],
        "third_party_workloads_opt_in": true
      }}
    }}"#
    )
}

const JOB_A: &str = "01JJOBSTAGEA00000000001";
const JOB_B: &str = "01JJOBSTAGEB00000000001";

// ─────────────────────────────────────────────────────────────────────

/// ★★ **다섯 단계가 이어져 Job 이 실제로 노드에 예약된다.**
#[test]
fn a_queued_job_is_reserved_on_a_real_node() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), JOB_A);
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));

    let (ok, output) = stage(
        &keyring,
        &db,
        JOB_A,
        ATTEMPT,
        LEASE,
        "aa0102030405060708090a0b0c0d0e0f",
    );
    assert!(ok, "stage-job 실패: {output}");
    assert!(output.contains("STAGED"), "출력이 STAGED 가 아니다: {output}");
    assert!(
        output.contains(&format!("node={NODE}")),
        "어느 노드에 잡았는지 말하지 않는다: {output}"
    );
    assert!(
        output.contains(GPU),
        "선택한 GPU 를 말하지 않는다: {output}"
    );
    assert_eq!(
        job_state(&db, JOB_A),
        Some(JobState::Staging),
        "저장된 상태가 STAGING 으로 안 갔다"
    );

    // 큐에서 빠졌는가 — 안 빠지면 다른 Coordinator 가 또 고른다.
    let store = CoordinatorJobStore::open(&db).expect("job store");
    assert!(
        store.list_queued().expect("큐 조회").is_empty(),
        "예약했는데 아직 큐에 남아 있다"
    );
}

/// ★★ **노드 하나짜리 풀에서 두 번째 예약은 막힌다.**
///
/// 이게 `DoD-46` 이 production 연결을 미룬 바로 그 위험이고,
/// `DoD-47` 의 CAS 가 푼 것이다. 안 막히면 두 Job 이 같은 GPU 를 잡는다.
///
/// ★ **단일 지점 뮤테이션으로 이 테스트를 뒤집지 못했다.** 방어가 세
///   겹이고 어느 하나만 지워도 다음 겹이 잡는다:
///
/// ```text
/// 1  staging_store 의 명시적 점유 검사 -> NodeAlreadyReserved
/// 2  coordinator_node_reservations 의 node_id PRIMARY KEY
/// 3  또 다른 UNIQUE 제약(둘을 다 지우자 2067 로 걸렸다)
/// ```
///
///   그래서 이 테스트가 고정하는 것은 **어느 한 관문**이 아니라
///   **"두 번째가 못 잡는다" 는 동작**이다. 그 구분을 적어 두는 이유는,
///   "뮤테이션으로 검증했다" 가 여기서는 정확한 말이 아니기 때문이다.
#[test]
fn a_second_job_cannot_reserve_the_only_node() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), JOB_A);
    // 두 번째 Job 도 큐에 올린다.
    queue_job(
        dir.path(),
        &keyring,
        &db,
        JOB_B,
        "1102030405060708090a0b0c0d0e0f10",
    );

    let (ok, output) = stage(
        &keyring,
        &db,
        JOB_A,
        ATTEMPT,
        LEASE,
        "aa0102030405060708090a0b0c0d0e0f",
    );
    assert!(ok, "첫 예약이 실패했다: {output}");

    let (ok, output) = stage(
        &keyring,
        &db,
        JOB_B,
        "01JATTEMPTSTAGE00000000002",
        "01JLEASESTAGE000000000002",
        "bb0102030405060708090a0b0c0d0e0f",
    );
    eprintln!("DIAG ok={ok} out={output}");
    assert!(
        !ok,
        "노드가 하나뿐인데 두 Job 을 예약했다 — 같은 GPU 를 둘이 잡는다: {output}"
    );
    // ★★ **이유까지 본다** (2026-09-07 독립 검수 지적).
    //
    //   전에는 `!ok` 와 상태만 봤다. 그러면 `stage-job` 이 **아무 이유로든**
    //   실패하도록 바뀌어도 통과한다 — 이 테스트가 재려던 것은
    //   "같은 노드를 둘이 못 잡는다" 이지 "두 번째가 어떻게든 실패한다"
    //   가 아니다.
    assert!(
        output.contains("node is already reserved"),
        "예약 충돌이 아니라 다른 관문에 걸렸다 — 이 테스트가 재려던 것이 아니다: {output}"
    );
    eprintln!("DIAG 두 번째 실패 이유: {output}");
    assert_eq!(
        job_state(&db, JOB_B),
        Some(JobState::Queued),
        "실패했는데 상태가 바뀌었다 — 두 번째 Job 은 큐에 남아야 한다"
    );
    // 첫 Job 은 그대로다.
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));
}

/// 같은 입력 재시도는 같은 결과다 — operation key 가 멱등을 보장한다.
#[test]
fn staging_the_same_job_twice_is_idempotent() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), JOB_A);

    let (ok, first) = stage(
        &keyring,
        &db,
        JOB_A,
        ATTEMPT,
        LEASE,
        "aa0102030405060708090a0b0c0d0e0f",
    );
    assert!(ok, "1회차 실패: {first}");
    let (ok, second) = stage(
        &keyring,
        &db,
        JOB_A,
        ATTEMPT,
        LEASE,
        "aa0102030405060708090a0b0c0d0e0f",
    );
    assert!(ok, "재시도 실패: {second}");

    let fence_of = |line: &str| -> String {
        line.split_whitespace()
            .find_map(|t| t.strip_prefix("fence=").map(str::to_string))
            .expect("fence 가 출력에 없다")
    };
    assert_eq!(
        fence_of(&first),
        fence_of(&second),
        "재시도가 fence epoch 를 새로 썼다 — 멱등이 아니다"
    );
}

/// ★ **자원 축 우선순위에 기본값을 두지 않는다.**
///
/// 기준 계획서에 v0.1 의 고정 순서가 없다(`model.rs` 가 명시). 여기서
/// 하나를 골라 기본으로 두면 규범에 없는 정책을 발명하는 것이다.
#[test]
fn the_best_fit_axis_order_must_be_stated_in_full() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), JOB_A);

    // 넷만 준다.
    let (ok, output) = run_cli(&[
        "stage-job",
        "--job-id",
        JOB_A,
        "--control-db",
        db.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--submitter-member",
        OWNER,
        "--max-snapshot-age-ms",
        "86400000",
        "--best-fit-axes",
        "vram,gpu_count,cpu,ram",
        "--coordinator-id",
        COORDINATOR,
        "--coordinator-term",
        "1",
        "--attempt-id",
        ATTEMPT,
        "--lease-id",
        LEASE,
        "--operation-key",
        "aa0102030405060708090a0b0c0d0e0f",
        "--lease-issued-at-unix-ms",
        LEASE_ISSUED,
        "--lease-renew-after-unix-ms",
        LEASE_RENEW,
        "--lease-expires-at-unix-ms",
        LEASE_EXPIRES,
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(!ok, "넷만 줬는데 받아들였다: {output}");
    assert!(output.contains("다섯 축"), "이유를 안 말한다: {output}");
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));

    // 중복도 거부한다 — 다섯 개지만 한 축이 빠진다.
    let (ok, output) = run_cli(&[
        "stage-job",
        "--job-id",
        JOB_A,
        "--control-db",
        db.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--submitter-member",
        OWNER,
        "--max-snapshot-age-ms",
        "86400000",
        "--best-fit-axes",
        "vram,vram,cpu,ram,workspace",
        "--coordinator-id",
        COORDINATOR,
        "--coordinator-term",
        "1",
        "--attempt-id",
        ATTEMPT,
        "--lease-id",
        LEASE,
        "--operation-key",
        "aa0102030405060708090a0b0c0d0e0f",
        "--lease-issued-at-unix-ms",
        LEASE_ISSUED,
        "--lease-renew-after-unix-ms",
        LEASE_RENEW,
        "--lease-expires-at-unix-ms",
        LEASE_EXPIRES,
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(!ok, "중복 축을 받아들였다: {output}");
    assert!(output.contains("두 번"), "이유를 안 말한다: {output}");

    // 대조 — 다섯 축을 제대로 주면 통과한다. 없으면 "항상 거부" 로도 통과한다.
    let (ok, output) = stage(
        &keyring,
        &db,
        JOB_A,
        ATTEMPT,
        LEASE,
        "aa0102030405060708090a0b0c0d0e0f",
    );
    assert!(ok, "제대로 준 축도 거부했다: {output}");
}

/// 갱신 시점이 만료 뒤면 갱신할 기회가 없다 — 받아 주지 않는다.
#[test]
fn a_renew_point_after_expiry_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), JOB_A);

    let (ok, output) = run_cli(&[
        "stage-job",
        "--job-id",
        JOB_A,
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
        "1",
        "--attempt-id",
        ATTEMPT,
        "--lease-id",
        LEASE,
        "--operation-key",
        "aa0102030405060708090a0b0c0d0e0f",
        "--lease-issued-at-unix-ms",
        LEASE_ISSUED,
        "--lease-renew-after-unix-ms",
        LEASE_EXPIRES, // 만료와 같거나 뒤
        "--lease-expires-at-unix-ms",
        LEASE_RENEW,
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(!ok, "만료 뒤 갱신 시점을 받아들였다: {output}");
    assert!(output.contains("순서가 아니다"), "이유를 안 말한다: {output}");
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));
}

/// 큐에 없는 Job 은 예약하지 않는다 — `QUEUED` 에서만 시작한다.
#[test]
fn a_second_stage_of_the_same_job_is_blocked_by_the_node_reservation_not_by_the_state() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), JOB_A);
    // 한 번 예약해 STAGING 으로 보낸다.
    let (ok, _) = stage(
        &keyring,
        &db,
        JOB_A,
        ATTEMPT,
        LEASE,
        "aa0102030405060708090a0b0c0d0e0f",
    );
    assert!(ok);

    // 다른 operation key 로 다시 — 이미 STAGING 이라 전이할 수 없다.
    let (ok, output) = stage(
        &keyring,
        &db,
        JOB_A,
        "01JATTEMPTSTAGE00000000003",
        "01JLEASESTAGE000000000003",
        "cc0102030405060708090a0b0c0d0e0f",
    );
    assert!(!ok, "STAGING 인 Job 을 또 예약했다: {output}");
    // ★★ **이유까지 보게 하니 이 테스트가 재려던 관문이 아니었다**
    //   (2026-09-07 독립 검수 지적을 따라 고치다 발견).
    //
    //   이름은 "QUEUED 가 아닌 Job 은 예약 안 한다" 인데, 실제로는
    //   **노드 예약 관문**에 먼저 걸린다:
    //
    //     STAGE_REFUSED: durable staging failed:
    //       node is already reserved: node=node-stage-a, job=...
    //
    //   첫 예약이 그 노드를 잡고 있으므로, Job 상태를 보기도 전에
    //   거기서 끝난다. `scheduler_tick.rs` 에서 겪은 것과 **같은 함정**
    //   이다 — 뒤 관문을 재려는데 앞 관문이 먼저 걸린다.
    //
    //   ★ 지금은 그 사실을 그대로 고정한다. 상태 관문을 따로 재려면
    //     **노드를 하나 더 준 fixture** 가 필요하고, 그건 이 조각 밖이다.
    //     "재려던 것을 잰다" 고 거짓으로 적지 않는다.
    assert!(
        output.contains("node is already reserved"),
        "예약 관문이 아닌 다른 이유로 막혔다: {output}"
    );
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));
}

/// 비영속 DB 는 거부한다.
#[test]
fn a_non_durable_control_db_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let keyring = write_keyring(dir.path());
    for label in [":memory:", ""] {
        let (ok, output) = run_cli(&[
            "stage-job",
            "--job-id",
            JOB_A,
            "--control-db",
            label,
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
            "1",
            "--attempt-id",
            ATTEMPT,
            "--lease-id",
            LEASE,
            "--operation-key",
            "aa0102030405060708090a0b0c0d0e0f",
            "--lease-issued-at-unix-ms",
            LEASE_ISSUED,
            "--lease-renew-after-unix-ms",
            LEASE_RENEW,
            "--lease-expires-at-unix-ms",
            LEASE_EXPIRES,
            "--lease-max-total-duration-seconds",
            "86400",
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ]);
        assert!(!ok, "{label:?} 를 받아들였다: {output}");
        assert!(
            output.contains("영속이 아니다"),
            "{label:?}: 이유를 안 말한다: {output}"
        );
    }
}

/// `--operation-key` 는 **32자리 hex** 여야 한다 (뮤테이션 H6).
///
/// 길이 검사를 지워도 아무 테스트가 안 깨졌다 — 잘못된 키를 준 적이
/// 없어서다. 짧은 것·긴 것·hex 가 아닌 것을 각각 준다.
#[test]
fn a_malformed_operation_key_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), JOB_A);

    for bad in [
        "aa0102030405060708090a0b0c0d0e",     // 30자 — 짧다
        "aa0102030405060708090a0b0c0d0e0f00", // 34자 — 길다
        "",                                   // 빈 값
    ] {
        let (ok, output) = stage(&keyring, &db, JOB_A, ATTEMPT, LEASE, bad);
        assert!(!ok, "{bad:?} 를 받아들였다: {output}");
        assert!(
            output.contains("32자리 hex"),
            "{bad:?}: 길이 관문이 아니라 다른 곳에서 막혔다: {output}"
        );
    }

    // 대조 — 올바른 길이는 통과한다. 없으면 "항상 거부" 로도 통과한다.
    let (ok, output) = stage(
        &keyring,
        &db,
        JOB_A,
        ATTEMPT,
        LEASE,
        "aa0102030405060708090a0b0c0d0e0f",
    );
    assert!(ok, "올바른 키인데 거부했다: {output}");
}

/// ★★ 오늘 CLI 로는 **상태 관문에 도달할 수 없다** — 그 사실을 고정한다.
///
/// 2026-09-10 독립 검수가 짚었다: 기존 테스트
/// (`a_second_stage_of_the_same_job_is_blocked_by_the_node_reservation_not_by_the_state`)
/// 는 이름 그대로 **노드 점유**로 막히는 것을 잰다. 그래서 "`QUEUED` 에서만
/// 예약한다" 는 관문이 CLI 테스트에서는 한 번도 안 돈다.
///
/// 확인해 보니 그 관문 자체는 **저장소 계층에서 재고 있다** —
/// `crates/coordinator/src/staging_store.rs` 의 테스트가
/// `JobNotQueued(Staging)` 과 `JobNotQueued(Submitted)` 를 둘 다 보고,
/// 동시 경쟁에서 정확히 하나만 성공하는 것까지 확인한다. 공백은
/// "관문이 없다" 가 아니라 "CLI 경로로 그 관문에 못 닿는다" 다.
///
/// **왜 못 닿나** — 순서가 이렇다:
/// ```text
/// reserve_node_and_stage_queued_with_lease
///   1. 노드 점유 검사   -> NodeAlreadyReserved
///   2. stage_new_in_transaction 안에서 상태 검사 -> JobNotQueued
/// ```
/// 그리고 **후보 선택이 예약을 안 본다.** 노드가 둘이어도 늘 같은 노드를
/// 고르므로, 두 번째 시도는 언제나 1번에서 죽는다.
///
/// ★★ **이 테스트는 덫이다.** 후보 선택이 예약을 알게 되면(`B′`,
///   `docs/plans/_열린_작업.md` §A1 4번) 두 번째 시도가 빈 노드를 골라
///   2번에 닿게 되고, **이 테스트가 깨진다.** 그때가 CLI 쪽 상태 관문
///   테스트를 추가할 시점이다. 깨지면 지우지 말고 뒤집어라.
#[test]
fn today_a_restage_cannot_reach_the_state_gate_because_selection_ignores_reservations() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared_two_nodes(dir.path(), JOB_A);
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));

    // 첫 예약 — 노드 하나를 잡는다. 다른 하나는 비어 있다.
    let (ok, output) = stage(
        &keyring,
        &db,
        JOB_A,
        ATTEMPT,
        LEASE,
        "aa0102030405060708090a0b0c0d0e0f",
    );
    assert!(ok, "첫 예약이 실패했다: {output}");
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));

    // 두 번째 — 빈 노드가 **남아 있는데도** 같은 노드를 고른다.
    let (ok2, output2) = stage(
        &keyring,
        &db,
        JOB_A,
        "01JATTEMPTSTAGE00000000004",
        "01JLEASESTAGE000000000004",
        "bb0102030405060708090a0b0c0d0e0f",
    );
    assert!(!ok2, "큐를 떠난 Job 을 다시 예약했다: {output2}");

    // ★ 오늘의 사실 — 막은 것은 **점유**다.
    assert!(
        output2.contains("node is already reserved"),
        "점유가 아닌 이유로 막혔다 — 후보 선택이 예약을 보게 됐다면 이 테스트를 뒤집어라: {output2}"
    );
    assert!(
        !output2.contains("Job is not QUEUED"),
        "상태 관문에 닿았다 — 후보 선택이 바뀐 것이다. CLI 쪽 상태 관문 테스트를 추가할 때다: {output2}"
    );

    // 어느 쪽 이유든 상태는 그대로여야 한다.
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));
}
