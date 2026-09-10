//! `gputeer plan-job` — **끊긴 곳 넷 중 셋이 실제로 이어지는가.**
//!
//! ```text
//! submit          제출자가 서명한 Manifest 를 낸다
//! import-manifest 운영자가 그것을 durable job store 에 넣는다   (SUBMITTED)
//! import-inventory 운영자가 노드 목록을 같은 DB 에 넣는다
//! plan-job        저장된 Manifest 를 다시 검증 -> JobRequirements
//!                 -> evaluate_eligibility -> 적격이 있으면 QUEUED
//! ```
//!
//! ★ **이 파일의 무게중심은 "올라간다" 가 아니라 "안 올라간다" 다.**
//!   `QUEUED` 는 `job_store.rs` 의 `enqueue()` 문서상 "실행 가능한 계획이
//!   최소 하나 있다" 는 뜻이다. 적격 노드가 없는데 올리면 큐에 영영
//!   실행 안 될 Job 이 쌓이고, 운영자는 스케줄러가 일하는 줄 안다.

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

const NODE: &str = "node-plan-a";
const GPU: &str = "node-plan-a-gpu-0";
const JOB_ID: &str = "01JJOBPLAN0000000000001";
const SUBMITTER: &str = "01JSUBMITTERPLAN00000001";
const SEED: &str = "55555555555555555555555555555555555555555555555555555555555555ee";
const OWNER: &str = "owner-plan";
/// 노드 소유자와 **같은** 멤버 — 제3자 정책을 이 테스트의 변수로 만들지
/// 않는다(그건 scheduler 쪽에서 이미 재고 있다).
const MEMBER: &str = OWNER;

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

/// 운영자 inventory 문서. `observed_at` 을 인자로 받아 신선도 반례를
/// 만들 수 있게 한다.
fn write_bootstrap(dir: &Path, observed_at: u64) -> PathBuf {
    let key: String = gputeer_crypto::SigningKey::from_bytes(&[9u8; 32])
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
        "device_id": "device-plan",
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
        "observed_at_unix_ms": {observed_at},
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

/// 제출자 keyring.
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

/// 스케줄에 필요한 축을 **전부 선언한** Manifest 를 만든다.
/// `extra` 로 한 축만 빼면 그게 반례가 된다.
fn submit_manifest(dir: &Path, name: &str, declarations: &[&str]) -> PathBuf {
    let manifest = dir.join(name);
    let issued = now_unix_ms().saturating_sub(60_000);
    let expires = now_unix_ms() + 7 * 24 * 3_600_000;
    let issued_s = issued.to_string();
    let expires_s = expires.to_string();
    let mut args: Vec<&str> = vec![
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
        &issued_s,
        "--expires-at-unix-ms",
        &expires_s,
        "--out",
        manifest.to_str().expect("경로"),
    ];
    args.extend_from_slice(declarations);
    let (ok, output) = run_cli(&args);
    assert!(ok, "submit 이 실패했다: {output}");
    manifest
}

/// 스케줄 가능한 완전한 선언.
const FULL: [&str; 22] = [
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

/// `submit` -> `import-manifest` -> `import-inventory` 까지 준비한다.
fn prepare(dir: &Path, declarations: &[&str], observed_at: u64) -> (PathBuf, PathBuf) {
    let manifest = submit_manifest(dir, "manifest.pb", declarations);
    let keyring = write_keyring(dir);
    let db = dir.join("control.sqlite3");

    let (ok, output) = run_cli(&[
        "import-manifest",
        "--manifest",
        manifest.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--job-db",
        db.to_str().unwrap(),
        "--idempotency-key",
        "0102030405060708090a0b0c0d0e0f10",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "import-manifest 실패: {output}");

    let bootstrap = write_bootstrap(dir, observed_at);
    let (ok, output) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory 실패: {output}");

    (keyring, db)
}

fn plan(keyring: &Path, db: &Path, extra: &[&str]) -> (bool, String) {
    let mut args: Vec<&str> = vec![
        "plan-job",
        "--job-id",
        JOB_ID,
        "--control-db",
        db.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--submitter-member",
        MEMBER,
        "--max-snapshot-age-ms",
        "86400000",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ];
    args.extend_from_slice(extra);
    run_cli(&args)
}

fn job_state(db: &Path) -> Option<JobState> {
    let store = CoordinatorJobStore::open(db).expect("job store");
    store.get(JOB_ID).expect("조회").map(|job| job.state)
}

// ─────────────────────────────────────────────────────────────────────

/// ★★ **이 조각의 핵심** — 세 명령이 실제로 이어져 Job 이 큐에 오른다.
#[test]
fn a_submitted_job_with_a_matching_node_reaches_the_queue() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepare(dir.path(), &FULL, now_unix_ms());

    assert_eq!(
        job_state(&db),
        Some(JobState::Submitted),
        "반입 직후에는 SUBMITTED 여야 한다"
    );

    let (ok, output) = plan(&keyring, &db, &[]);
    assert!(ok, "plan-job 실패: {output}");
    assert!(output.contains("QUEUED"), "출력이 QUEUED 가 아니다: {output}");
    assert!(
        output.contains("eligible=1"),
        "적격 노드 수를 말하지 않는다: {output}"
    );
    assert_eq!(
        job_state(&db),
        Some(JobState::Queued),
        "저장된 상태가 QUEUED 로 안 갔다"
    );

    // 큐가 실제로 이 Job 을 돌려준다 — scheduler 가 볼 수 있는 상태인가.
    let store = CoordinatorJobStore::open(&db).expect("job store");
    let queued = store.list_queued().expect("큐 조회");
    assert_eq!(
        queued.iter().map(|j| j.job_id.as_str()).collect::<Vec<_>>(),
        vec![JOB_ID],
        "큐에 이 Job 이 없다"
    );
    assert!(
        queued[0].plan_id.is_some(),
        "QUEUED 인데 plan_id 가 없다 — 계획 없이 올라갔다"
    );
}

/// 같은 입력을 두 번 돌려도 같은 계획이다 — `enqueue()` 는 재시도에
/// 같은 `plan_id` 를 요구하고 다르면 충돌한다.
#[test]
fn planning_the_same_job_twice_keeps_the_same_plan() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepare(dir.path(), &FULL, now_unix_ms());

    let (ok, first) = plan(&keyring, &db, &[]);
    assert!(ok, "1회차 실패: {first}");
    let (ok, second) = plan(&keyring, &db, &[]);
    assert!(ok, "2회차 실패: {second}");

    let plan_of = |line: &str| -> String {
        line.split_whitespace()
            .find_map(|token| token.strip_prefix("plan_id=").map(str::to_string))
            .expect("plan_id 가 출력에 없다")
    };
    assert_eq!(
        plan_of(&first),
        plan_of(&second),
        "같은 입력인데 계획이 달라졌다 — 재시도가 늘 충돌한다"
    );
}

/// ★★ **적격 노드가 없으면 올리지 않는다.**
///
/// inventory 를 비워 두면 후보 자체가 없다. `QUEUED` 는 "실행 가능한
/// 계획이 있다" 는 뜻이므로 이때 올리면 상태가 거짓말을 한다.
#[test]
fn a_job_with_no_eligible_node_is_not_queued() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_manifest(dir.path(), "manifest.pb", &FULL);
    let keyring = write_keyring(dir.path());
    let db = dir.path().join("control.sqlite3");
    let (ok, output) = run_cli(&[
        "import-manifest",
        "--manifest",
        manifest.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--job-db",
        db.to_str().unwrap(),
        "--idempotency-key",
        "0102030405060708090a0b0c0d0e0f10",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "import-manifest 실패: {output}");
    // inventory 를 **반입하지 않는다.**

    let (ok, output) = plan(&keyring, &db, &[]);
    assert!(!ok, "후보가 없는데 큐에 올렸다: {output}");
    assert!(
        output.contains("적격 노드가 없다"),
        "이유를 안 말한다: {output}"
    );
    assert_eq!(
        job_state(&db),
        Some(JobState::Submitted),
        "거부했는데 상태가 바뀌었다"
    );
}

/// 후보는 있는데 요구를 못 맞추면 **어느 노드가 왜 떨어졌는지** 말한다.
///
/// "후보가 없다" 만으로는 운영자가 무엇을 고칠지 모른다.
#[test]
fn a_rejected_candidate_is_named_with_its_reason() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    // 노드가 가진 것(12GiB)보다 큰 VRAM 을 요구한다.
    let mut demanding = FULL;
    demanding[15] = "137438953472"; // 128GiB
    let (keyring, db) = prepare(dir.path(), &demanding, now_unix_ms());

    let (ok, output) = plan(&keyring, &db, &[]);
    assert!(!ok, "요구를 못 맞추는 노드를 골랐다: {output}");
    assert!(
        output.contains(NODE),
        "떨어진 노드 이름을 말하지 않는다: {output}"
    );
    // ★★ **이유까지 확인한다** (2026-09-07 독립 검수 지적).
    //
    //   전에는 `!ok` 와 "노드 이름이 나온다" 만 봤다. 검수가 반례를
    //   만들어 보였다 — inventory 관측 시각을 오래된 값으로 바꾸면
    //   `SnapshotNotFresh` 로 거부되는데, 그때도 같은 노드 이름이
    //   나오므로 **VRAM 검사가 통째로 없어도 이 테스트가 통과한다.**
    //
    //   이 파일이 재려던 것은 "VRAM 이 모자라서 떨어졌다" 이지
    //   "무슨 이유로든 떨어졌다" 가 아니다.
    assert!(
        output.contains("GpuVramInsufficient"),
        "VRAM 부족이 아니라 다른 관문에 걸렸다 — 이 테스트가 재려던 것이 아니다: {output}"
    );
    assert_eq!(job_state(&db), Some(JobState::Submitted));
}

/// ★★ **선언하지 않은 축이 있으면 그 이름을 대며 거부한다.**
///
/// 여섯 축 각각을 하나씩 빼고 각각 확인한다 — 하나로 묶어 "거부됐다"
/// 만 보면 여섯 중 하나만 막는 구현도 통과한다.
#[test]
fn every_undeclared_axis_is_named_and_the_job_stays_submitted() {
    let cases: [(&str, &str); 6] = [
        ("--workload-class", "workload"),
        ("--side-effect-class", "side_effect_class"),
        ("--dataset-sensitivity", "dataset"),
        ("--minimum-security-tier", "minimum_security_tier"),
        ("--minimum-isolation-class", "minimum_isolation_class"),
        ("--minimum-key-protection", "minimum_key_protection"),
    ];
    for (flag, expected) in cases {
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        // 그 축의 플래그와 값을 뺀다.
        let mut declarations: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < FULL.len() {
            if FULL[i] == flag {
                i += 2;
                continue;
            }
            declarations.push(FULL[i]);
            declarations.push(FULL[i + 1]);
            i += 2;
        }
        let (keyring, db) = prepare(dir.path(), &declarations, now_unix_ms());

        let (ok, output) = plan(&keyring, &db, &[]);
        assert!(!ok, "{flag} 를 안 줬는데 큐에 올렸다: {output}");
        assert!(
            output.contains(expected),
            "{flag} 를 뺐는데 {expected} 를 지목하지 않는다: {output}"
        );
        assert_eq!(
            job_state(&db),
            Some(JobState::Submitted),
            "{flag}: 거부했는데 상태가 바뀌었다"
        );
    }
}

/// `submit` 이 오타를 조용히 흘리지 않는다.
///
/// `--workload-class TRAINNING`(오타)을 `UNSPECIFIED` 로 받아 주면
/// 제출자는 선언했다고 믿는데 스케줄러는 못 본다.
#[test]
fn a_misspelled_declaration_is_refused_at_submit_time() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("manifest.pb");
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
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
        &issued,
        "--out",
        out.to_str().unwrap(),
        "--workload-class",
        "TRAINNING",
    ]);
    assert!(!ok, "오타를 받아들였다: {output}");
    assert!(output.contains("모른다"), "이유를 안 말한다: {output}");
    assert!(!out.exists(), "거부했는데 파일을 남겼다");
}

/// ★ 저장된 Manifest 를 **지금 다시** 검증한다.
///
/// keyring 에서 서명자를 빼면 — 저장될 때는 유효했더라도 — 계획하지
/// 않는다. `get_manifest_binding()` 은 raw 이고 "재검증 전 scheduler 에
/// 쓸 수 없다" 고 `DoD-50` 이 못박았다.
#[test]
fn a_signer_no_longer_in_the_keyring_stops_planning() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepare(dir.path(), &FULL, now_unix_ms());

    // 대조 — 지금은 계획된다.
    let (ok, _) = plan(&keyring, &db, &[]);
    assert!(ok, "정상 경로가 실패했다");

    // 새 Job 으로 다시 준비하되 keyring 에서 서명자를 뺀다.
    let dir2 = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring2, db2) = prepare(dir2.path(), &FULL, now_unix_ms());
    let mut ring = gputeer_crypto::PersistentKeyring::new(
        &keyring2,
        gputeer_crypto::KeyProtection::K0Plaintext,
        gputeer_crypto::PlaintextPolicy::Allow,
    )
    .expect("keyring 재생성");
    ring.insert_public(
        "01JSOMEONEELSE0000000001",
        gputeer_crypto::SigningKey::from_bytes(&[7u8; 32]).verifying_key(),
    )
    .expect("공개키 등록");
    ring.save().expect("keyring 저장");

    let (ok, output) = plan(&keyring2, &db2, &[]);
    assert!(!ok, "믿지 않는 서명자의 Job 을 계획했다: {output}");
    // ★★ **공통 포장 문구가 아니라 실제 이유를 본다** (2026-09-07 검수 지적).
    //
    //   "다시 검증하지 못했다" 는 `plan_job.rs:117-121` 이 **모든**
    //   `VerifyError` 에 붙이는 문구다. 검수가 반례를 만들어 보였다 —
    //   keyring 에서 서명자를 지우는 대신 **같은 ID 에 다른 공개키**를
    //   넣으면 `InvalidSignature` 가 나오는데, 그때도 이 단언은 전부
    //   통과한다.
    //
    //   이 테스트가 재려던 것은 "그 서명자를 모른다" 이지 "서명 검증이
    //   어떤 이유로든 실패했다" 가 아니다. 다행히 포장 문구가 `{e:?}` 로
    //   실제 오류를 담으므로 여기서 그것을 직접 확인할 수 있다.
    assert!(
        output.contains("UnknownSigner"),
        "서명자를 모른다는 이유가 아니라 다른 검증 실패다: {output}"
    );
    assert_eq!(job_state(&db2), Some(JobState::Submitted));
}

/// `--submitter-member` 를 안 주면 계획하지 않는다 — Manifest 에 없는
/// 값이고 `team_id` 로 대체하지 않는다.
#[test]
fn a_blank_submitter_member_stops_planning() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepare(dir.path(), &FULL, now_unix_ms());

    let (ok, output) = run_cli(&[
        "plan-job",
        "--job-id",
        JOB_ID,
        "--control-db",
        db.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--submitter-member",
        "   ",
        "--max-snapshot-age-ms",
        "86400000",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(!ok, "빈 멤버로 계획했다: {output}");
    assert!(
        output.contains("submitter_member_id"),
        "이유를 안 말한다: {output}"
    );
    assert_eq!(job_state(&db), Some(JobState::Submitted));
}

/// 오래된 관측으로는 계획하지 않는다 — 운영자가 준 신선도 정책을 쓴다.
#[test]
fn a_stale_observation_is_not_planned_under_the_operators_freshness_policy() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    // 두 시간 전 관측. ★ 대조(24시간 상한)와 **겹치지 않게** 골랐다 —
    //   처음에는 24시간 전 관측에 24시간 상한을 대조로 써서 경계에
    //   걸렸고, 그래서 대조가 실패했다. 대조가 통과할 수 없으면 그
    //   테스트는 "항상 거부" 구현을 배제하지 못한다.
    let (keyring, db) = prepare(
        dir.path(),
        &FULL,
        now_unix_ms().saturating_sub(2 * 3_600_000),
    );

    // 1분 상한 — 떨어진다.
    let (ok, output) = run_cli(&[
        "plan-job",
        "--job-id",
        JOB_ID,
        "--control-db",
        db.to_str().unwrap(),
        "--submitter-keyring",
        keyring.to_str().unwrap(),
        "--submitter-member",
        MEMBER,
        "--max-snapshot-age-ms",
        "60000",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(!ok, "오래된 관측으로 계획했다: {output}");
    assert!(
        output.contains("SnapshotNotFresh"),
        "신선도를 이유로 대지 않는다: {output}"
    );

    // 대조 — 상한을 늘리면 통과한다. 없으면 "항상 거부" 로도 통과한다.
    let (ok, output) = plan(&keyring, &db, &[]);
    assert!(ok, "넉넉한 상한에서도 거부했다: {output}");
}

/// 비영속 DB 는 거부한다 — 올린 상태가 사라지는데 로그는 성공이다.
#[test]
fn a_non_durable_control_db_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let keyring = write_keyring(dir.path());
    for label in [":memory:", ""] {
        let (ok, output) = run_cli(&[
            "plan-job",
            "--job-id",
            JOB_ID,
            "--control-db",
            label,
            "--submitter-keyring",
            keyring.to_str().unwrap(),
            "--submitter-member",
            MEMBER,
            "--max-snapshot-age-ms",
            "86400000",
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

// ══════════════════════════════════════════════════════════════════════
// DB 변조 방어 — 정상 경로로는 만들 수 없는 상태를 만들어 본다
// ══════════════════════════════════════════════════════════════════════

/// ★★ **"저장 당시 서명자 ≠ 지금 검증된 서명자" 를 만들 수 있는가.**
///
/// `plan_job.rs` 에 그 분기가 있는데 뮤테이션으로 지워도 아무 테스트도
/// 안 깨졌다. 나는 "DB 를 직접 고쳐야 재는 상태" 라고 적었다 — 그래서
/// **실제로 고쳐 봤다.**
///
/// 결과: **못 만든다.** 저장소의 load 경로가 먼저 막는다
/// (`job_store.rs:906`·`:912`):
///
/// ```text
///   manifest.submitter_device_id == job.submitter_device_id
///   signer_id_at_submission      == job.submitter_device_id
/// ```
///
/// 그리고 `Verified::signer_id()` 는 **메시지의 필드**에서 온다
/// (`signing.rs:843`, `signer_id: msg.signer_id()`). 셋을 합치면
/// 두 값은 **항상 같다** — 그 분기는 도달 불가능하다.
///
/// ★ 이 테스트는 분기를 재지 않는다. **재려고 했더니 더 앞의 관문이
///   전부 막더라**는 것을 고정한다. 나중에 load 의 대조가 느슨해지면
///   이 테스트가 실패하며 알려 준다.
#[test]
fn tampering_the_stored_signer_is_stopped_by_the_store_before_planning() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepare(dir.path(), &FULL, now_unix_ms());

    // 대조 — 변조 전에는 계획이 성공한다. 없으면 "항상 실패" 로도 통과한다.
    let (ok, output) = plan(&keyring, &db, &[]);
    assert!(ok, "변조 전인데 실패했다: {output}");

    let connection = rusqlite::Connection::open(&db).expect("DB 열기");
    let changed = connection
        .execute(
            "UPDATE coordinator_job_manifests SET verified_signer_id = 'someone-else' WHERE job_id = ?1",
            [JOB_ID],
        )
        .expect("변조");
    assert_eq!(changed, 1, "변조할 행이 없다 — 테이블 이름이 바뀌었나");
    drop(connection);

    let (ok, output) = plan(&keyring, &db, &[]);
    assert!(!ok, "변조했는데 계획이 성공했다: {output}");
    assert!(
        output.contains("손상") || output.to_lowercase().contains("corrupt"),
        "★ 저장소의 손상 판정이 아니라 다른 이유로 막혔다 — 그러면 \
         plan_job 의 서명자 대조가 실제로 도달 가능하다는 뜻이고, \
         그 분기와 이 문서를 같이 고쳐야 한다: {output}"
    );
}
