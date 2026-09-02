//! 두 반입 명령이 **한 control DB 파일을 공유할 수 있는가.**
//!
//! # 왜 이걸 재는가
//!
//! `crates/coordinator/src/staging_store.rs` 의 예약 경로는 **자기 연결로**
//! `coordinator_agent_inventory` 와 `coordinator_agent_gpus` 를 읽는다
//! (`fetch_inventory_revision`·`validate_selected_gpus_exist`). 그래서
//! inventory 가 job/staging 과 **다른 파일**에 있으면, 후보 선택은 되는데
//! 예약 CAS 가 붙지 않는다 — `InventoryMissing` 으로 끝난다.
//!
//! `gputeer import-manifest --job-db` 와 `gputeer import-inventory
//! --inventory-db` 는 각각 경로를 받으므로 운영자가 **다른 파일을 줄 수
//! 있다.** 그러면 조용히 안 되는 조합이 만들어진다.
//!
//! 이 테스트는 두 가지를 함께 잰다:
//!
//! ```text
//! 같은 파일    두 명령이 공존하고, staging 이 반입한 inventory 를 본다
//! 다른 파일    똑같은 예약이 InventoryMissing 으로 실패한다   <- 대조
//! ```
//!
//! ★ 대조가 없으면 이 테스트는 아무것도 안 잰다 — "예약이 성공했다" 만
//!   보면 두 DB 가 애초에 분리될 수 없는 경우에도 통과한다.

use std::path::{Path, PathBuf};
use std::process::Command;

use gputeer_coordinator::{
    inventory_store::CoordinatorInventoryStore,
    job_store::CoordinatorJobStore,
    staging_store::{CoordinatorStagingStore, ReservedStageError, StageQueuedRequest},
};

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) { "gputeer.exe" } else { "gputeer" })
}

const NODE: &str = "node-a";
const GPU: &str = "node-a-gpu-0";
const JOB_ID: &str = "01JJOBSHARED000000000001";
const SUBMITTER: &str = "01JSUBMITTERSHARED000001";
const SEED: &str = "44444444444444444444444444444444444444444444444444444444444444dd";
const OBSERVED_AT: u64 = 1_700_000_000_000;

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

/// 운영자 bootstrap 문서 하나를 쓴다.
fn write_bootstrap(dir: &Path) -> PathBuf {
    let key: String = gputeer_crypto::SigningKey::from_bytes(&[3u8; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let body = format!(
        r#"{{
  "schema_version": 1,
  "agents": [
    {{
      "registry": {{
        "node_id": "{NODE}",
        "device_id": "device-shared",
        "owner_member_id": "owner-shared",
        "verifying_key_hex": "{key}",
        "node_state": "ONLINE",
        "risk_state": "NORMAL",
        "security_tier": "S2",
        "isolation_class": "CONTAINED",
        "key_protection": "K1"
      }},
      "inventory": {{
        "inventory_revision": 1,
        "observed_at_unix_ms": {OBSERVED_AT},
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

/// 제출자가 서명한 Manifest 와 그것을 받는 운영자 keyring 을 만든다.
fn write_manifest_and_keyring(dir: &Path) -> (PathBuf, PathBuf) {
    let manifest = dir.join("manifest.pb");
    let issued = now_unix_ms().saturating_sub(60_000);
    let expires = now_unix_ms() + 7 * 24 * 3_600_000;
    let (ok, output) = run_cli(&[
        "submit",
        "--job-id",
        JOB_ID,
        "--entrypoint",
        "python",
        "--submitter-device-id",
        SUBMITTER,
        "--submitter-seed",
        SEED,
        "--issued-at-unix-ms",
        &issued.to_string(),
        "--expires-at-unix-ms",
        &expires.to_string(),
        "--out",
        manifest.to_str().expect("경로"),
    ]);
    assert!(ok, "submit 이 실패했다: {output}");

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
    (manifest, keyring)
}

/// 반입된 Job 을 `QUEUED` 까지 올린다 — staging 은 그 상태만 받는다.
///
/// ★ 이 두 호출이 **production 에 호출자가 없다**는 것이 남은 공백이다
///   (설계 조사가 `SUBMITTED→QUEUED` caller 부재로 지목했다). 여기서는
///   테스트가 그 자리를 대신해, **DB 공유 여부만** 따로 잰다.
fn queue_the_job(db: &Path) {
    let mut store = CoordinatorJobStore::open(db).expect("job store 열기");
    let now = now_unix_ms();
    store.start_planning(JOB_ID, now).expect("PLANNING 전이");
    store.enqueue(JOB_ID, "plan-shared", now).expect("QUEUED 전이");
}

fn reservation_request() -> StageQueuedRequest {
    let now = now_unix_ms();
    StageQueuedRequest {
        operation_key: [0x5Au8; 16],
        job_id: JOB_ID.into(),
        attempt_id: "01JATTEMPTSHARED00000001".into(),
        lease_id: "01JLEASESHARED0000000001".into(),
        node_id: NODE.into(),
        selected_gpu_ids: vec![GPU.into()],
        issuing_coordinator_id: "01JCOORDSHARED0000000001".into(),
        coordinator_term: 1,
        issued_at_unix_ms: now,
        renew_after_unix_ms: now + 30_000,
        expires_at_unix_ms: now + 300_000,
        max_total_duration_seconds: 3_600,
    }
}

/// ★★ 같은 파일에 넣으면 예약이 반입한 inventory 를 실제로 본다.
#[test]
fn both_imports_into_one_control_db_let_staging_see_the_inventory() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let control = dir.path().join("control.sqlite3");
    let bootstrap = write_bootstrap(dir.path());
    let (manifest, keyring) = write_manifest_and_keyring(dir.path());

    // 두 명령이 **같은 파일**을 쓴다.
    let (ok, output) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().expect("경로"),
        "--inventory-db",
        control.to_str().expect("경로"),
    ]);
    assert!(ok, "inventory 반입 실패: {output}");

    let (ok, output) = run_cli(&[
        "import-manifest",
        "--manifest",
        manifest.to_str().expect("경로"),
        "--submitter-keyring",
        keyring.to_str().expect("경로"),
        "--job-db",
        control.to_str().expect("경로"),
        "--idempotency-key",
        "0102030405060708090a0b0c0d0e0f10",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "manifest 반입 실패: {output}");

    // 서로의 스키마를 깨뜨리지 않았는가 — 둘 다 자기 것을 읽는다.
    {
        let store = CoordinatorJobStore::open(&control).expect("job store 열기");
        assert!(
            store.get(JOB_ID).expect("job 조회").is_some(),
            "같은 파일을 쓰자 Job 이 사라졌다"
        );
    }
    {
        let mut store = CoordinatorInventoryStore::open(&control).expect("inventory store 열기");
        let pool = store.pool_snapshot(OBSERVED_AT + 1_000).expect("snapshot");
        assert_eq!(
            pool.candidates.len(),
            1,
            "같은 파일을 쓰자 후보가 사라졌다"
        );
    }

    queue_the_job(&control);

    // ★ 여기가 핵심 — 예약이 반입한 inventory 를 본다.
    let mut staging = CoordinatorStagingStore::open(&control).expect("staging store 열기");
    let result = staging.reserve_node_and_stage_queued_with_lease(&reservation_request(), 1);
    let staged = result.expect("같은 파일인데 예약이 실패했다");
    assert_eq!(staged.reservation.node_id, NODE, "엉뚱한 노드를 예약했다");
    assert_eq!(
        staged.reservation.selected_gpu_ids,
        vec![GPU.to_string()],
        "선택한 GPU 가 예약에 안 실렸다"
    );
    // 반입한 revision 이 그대로 예약에 실렸는가 — 다른 값이면 CAS 가
    // 엉뚱한 스냅샷을 본 것이다.
    assert_eq!(
        staged.reservation.inventory_revision, 1,
        "예약이 다른 inventory revision 을 봤다"
    );
}

/// ★ 대조 — 파일을 나누면 **똑같은 예약이 실패한다.**
///
/// 이게 없으면 위 테스트는 "예약이 되네" 만 확인할 뿐, 파일 공유가
/// 그 성공의 **이유**인지는 못 잰다.
#[test]
fn splitting_the_two_databases_breaks_the_reservation() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let inventory_db = dir.path().join("inventory.sqlite3");
    let job_db = dir.path().join("jobs.sqlite3");
    let bootstrap = write_bootstrap(dir.path());
    let (manifest, keyring) = write_manifest_and_keyring(dir.path());

    let (ok, output) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().expect("경로"),
        "--inventory-db",
        inventory_db.to_str().expect("경로"),
    ]);
    assert!(ok, "inventory 반입 실패: {output}");

    let (ok, output) = run_cli(&[
        "import-manifest",
        "--manifest",
        manifest.to_str().expect("경로"),
        "--submitter-keyring",
        keyring.to_str().expect("경로"),
        "--job-db",
        job_db.to_str().expect("경로"),
        "--idempotency-key",
        "0102030405060708090a0b0c0d0e0f10",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "manifest 반입 실패: {output}");

    // 두 명령 다 성공했다 — 운영자에게는 아무 문제가 없어 보인다.
    queue_the_job(&job_db);

    let mut staging = CoordinatorStagingStore::open(&job_db).expect("staging store 열기");
    let error = staging
        .reserve_node_and_stage_queued_with_lease(&reservation_request(), 1)
        .expect_err("inventory 가 없는 DB 인데 예약이 됐다");

    // ★★ **여기서 예상과 다른 것이 나왔고, 그게 발견이다.**
    //
    //   `InventoryMissing`("그 노드의 inventory 가 없다")이 날 줄 알았다.
    //   실제로는 원시 SQL 오류가 난다 — `CoordinatorStagingStore::open()`
    //   이 `coordinator_agent_inventory` 를 **만들지 않기 때문**이다.
    //   그 테이블은 `CoordinatorInventoryStore::open()` 만 만든다. 즉
    //   staging 은 같은 파일에 inventory 저장소가 이미 다녀갔다는 것을
    //   **선언 없이 전제한다.**
    //
    //   운영자에게 보이는 것: "SQL error or missing database".
    //   진짜 원인: `--inventory-db` 와 `--job-db` 에 다른 파일을 줬다.
    //   그 둘 사이에는 아무 연결도 없다(`CLAUDE.md` §3 — 오류가 사실을
    //   잘못 전하지 않게 한다).
    //
    // ★ 이 단언은 **지금의 결함을 고정한다.** 나중에 누가 이 오류를
    //   진단 가능하게 고치면 이 테스트가 실패하고, 그때 이 주석과 문서도
    //   같이 고치라고 알린다(`runtime-linux` 의 "탈출이 성공하기를
    //   기대하는 테스트" 와 같은 장치).
    let message = format!("{error:?}");
    assert!(
        !matches!(error, ReservedStageError::InventoryMissing { .. }),
        "이 오류가 진단 가능해졌다 — 좋은 일이다. 이 테스트와 주석·문서를 같이 고쳐라: {message}"
    );
    assert!(
        message.contains("SQL error or missing database"),
        "실패 방식이 달라졌다 — 무엇이 바뀌었는지 확인하고 이 고정을 갱신하라: {message}"
    );
}
