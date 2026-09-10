//! `gputeer issue-grant` — **여섯 단계 끝에 실제로 검증되는 Grant 가 나온다.**
//!
//! ```text
//! submit → import-manifest → import-inventory → plan-job → stage-job
//!                                                          → issue-grant
//! ```
//!
//! ★ **무게중심은 "파일이 나온다" 가 아니다.** 나온 Grant 가 Coordinator
//!   공개키로 **실제로 검증되고**, 그 안의 값이 **저장소가 아는 사실과
//!   같아야** 한다. 지금까지 Grant 는 명령줄 값으로 만들어졌고
//!   `coordinator_term` 은 리터럴 1 이었다.

use std::path::{Path, PathBuf};
use std::process::Command;

use gputeer_crypto::{Ed25519Verifier, InMemoryKeyring, SigningKey};
use gputeer_protocol::pb;
use gputeer_protocol::signing::{verify, NoReplayCheck};
use prost::Message;

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) { "gputeer.exe" } else { "gputeer" })
}

const NODE: &str = "node-grant-a";
const GPU: &str = "node-grant-a-gpu-0";
const JOB: &str = "01JJOBGRANT000000000001";
const SUBMITTER: &str = "01JSUBMITTERGRANT0000001";
const SEED: &str = "77777777777777777777777777777777777777777777777777777777777777aa";
const OWNER: &str = "owner-grant";
const COORDINATOR: &str = "01JCOORDINATORGRANT00001";
/// Coordinator 서명키 seed. **파일로만** 넘긴다 — 명령줄에 두면
/// 개인키가 프로세스 목록에 뜬다.
const COORD_SEED: &str = "88888888888888888888888888888888888888888888888888888888888888bb";
const ATTEMPT: &str = "01JATTEMPTGRANT00000000001";
const LEASE: &str = "01JLEASEGRANT000000000001";
const GRANT: &str = "01JGRANTGRANT000000000001";
const AXES: &str = "vram,gpu_count,cpu,ram,workspace";

/// ★ 고정 상수다 — 시계를 읽으면 재발급이 멱등하지 않다.
///   `--lease-*` 와 `--grant-*` 를 **손으로** 골라, 기대값을 검사 대상의
///   출력에서 만들지 않는다.
const LEASE_ISSUED: u64 = 1_800_000_000_000;
const LEASE_RENEW: u64 = 1_800_000_300_000;
const LEASE_EXPIRES: u64 = 1_800_000_600_000;
const GRANT_ISSUED: u64 = 1_800_000_010_000;
const GRANT_EXPIRES: u64 = 1_800_000_070_000;
const TERM: u64 = 7;

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

fn seed_bytes(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("seed hex");
    }
    out
}

/// 준비: inventory 반입 → submit → import-manifest → plan-job → stage-job.
/// 반환은 `(control-db, coordinator 키 파일)`.
fn staged(dir: &Path) -> (PathBuf, PathBuf) {
    let db = dir.join("control.sqlite3");

    // inventory
    let node_key: String = SigningKey::from_bytes(&[21u8; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
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
        "node_id": "{NODE}", "device_id": "device-grant",
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

    // submit
    let manifest = dir.join("manifest.pb");
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
    let (ok, out) = run_cli(&[
        "submit",
        "--job-id",
        JOB,
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
    ]);
    assert!(ok, "submit 실패: {out}");

    // keyring
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
        "stage-job",
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
        "--best-fit-axes",
        AXES,
        "--coordinator-id",
        COORDINATOR,
        "--coordinator-term",
        &TERM.to_string(),
        "--attempt-id",
        ATTEMPT,
        "--lease-id",
        LEASE,
        "--operation-key",
        "aa0102030405060708090a0b0c0d0e0f",
        "--lease-issued-at-unix-ms",
        &LEASE_ISSUED.to_string(),
        "--lease-renew-after-unix-ms",
        &LEASE_RENEW.to_string(),
        "--lease-expires-at-unix-ms",
        &LEASE_EXPIRES.to_string(),
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "stage-job 실패: {out}");

    let key_file = dir.join("coordinator.key");
    std::fs::write(&key_file, COORD_SEED).expect("키 파일 쓰기");
    (db, key_file)
}

fn issue(db: &Path, key_file: &Path, out: &Path, extra: &[&str]) -> (bool, String) {
    let mut args: Vec<&str> = vec![
        "issue-grant",
        "--job-id",
        JOB,
        "--control-db",
        db.to_str().unwrap(),
        "--attempt-id",
        ATTEMPT,
        "--lease-id",
        LEASE,
        "--grant-id",
        GRANT,
        "--grant-issued-at-unix-ms",
        "1800000010000",
        "--grant-expires-at-unix-ms",
        "1800000070000",
        "--coordinator-key-file",
        key_file.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    run_cli(&args)
}

/// 한 Job 을 `submit` -> `import-manifest` -> `plan-job` 까지만 올린다.
/// 예약(`stage-job`)은 하지 않는다.
fn queue_only(dir: &Path, db: &Path, job_id: &str, idem: &str) {
    let manifest = dir.join(format!("{job_id}.pb"));
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
    let (ok, out) = run_cli(&[
        "submit", "--job-id", job_id, "--entrypoint", "python",
        "--submitter-device-id", SUBMITTER, "--submitter-seed", SEED,
        "--issued-at-unix-ms", &issued, "--expires-at-unix-ms", &expires,
        "--out", manifest.to_str().unwrap(),
        "--workload-class", "TRAINING", "--side-effect-class", "PURE",
        "--dataset-sensitivity", "INTERNAL", "--minimum-security-tier", "S2",
        "--minimum-isolation-class", "CONTAINED", "--minimum-key-protection", "K1",
        "--gpu-count", "1", "--gpu-min-vram-bytes", "8589934592",
        // ★ 2026-09-10 — 변환기가 생략된 자원을 더 이상 0 으로 채우지 않는다.
        "--cpu-cores", "4", "--ram-bytes", "8589934592",
        "--workspace-bytes", "10737418240",
    ]);
    assert!(ok, "submit 실패: {out}");

    let keyring = dir.join("submitters.keyring");
    let (ok, out) = run_cli(&[
        "import-manifest", "--manifest", manifest.to_str().unwrap(),
        "--submitter-keyring", keyring.to_str().unwrap(),
        "--job-db", db.to_str().unwrap(), "--idempotency-key", idem,
        "--i-understand-plaintext-keyring-is-unsafe", "true",
    ]);
    assert!(ok, "import-manifest 실패: {out}");

    let (ok, out) = run_cli(&[
        "plan-job", "--job-id", job_id, "--control-db", db.to_str().unwrap(),
        "--submitter-keyring", keyring.to_str().unwrap(),
        "--submitter-member", OWNER, "--max-snapshot-age-ms", "86400000",
        "--i-understand-plaintext-keyring-is-unsafe", "true",
    ]);
    assert!(ok, "plan-job 실패: {out}");
}

/// 발급된 Grant 를 **Coordinator 공개키로 실제 검증한다.**
fn verified_grant(path: &Path) -> pb::ExecutionGrant {
    let bytes = std::fs::read(path).expect("Grant 파일 읽기");
    let grant = pb::ExecutionGrant::decode(bytes.as_slice()).expect("protobuf 해석");
    let mut keys = InMemoryKeyring::new();
    keys.insert(
        COORDINATOR,
        SigningKey::from_bytes(&seed_bytes(COORD_SEED)).verifying_key(),
    );
    let verified = verify(
        &grant,
        2,
        &Ed25519Verifier::new(keys),
        GRANT_ISSUED,
        &mut NoReplayCheck,
    )
    .expect("발급된 Grant 가 Coordinator 공개키로 검증되지 않는다");
    verified.get().clone()
}

// ─────────────────────────────────────────────────────────────────────

/// ★★ **저장된 사실로 만든 Grant 가 실제로 검증된다.**
#[test]
fn a_grant_is_signed_from_stored_facts_and_verifies() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    let (ok, output) = issue(&db, &key_file, &out, &[]);
    assert!(ok, "issue-grant 실패: {output}");
    assert!(output.contains("GRANTED"), "출력이 GRANTED 가 아니다: {output}");

    let grant = verified_grant(&out);

    // ★ 기대값은 전부 **내가 손으로 정한 입력**에서 온다 — 검사 대상의
    //   출력을 다시 읽어 기대값으로 쓰지 않는다.
    assert_eq!(grant.grant_id, GRANT);
    assert_eq!(grant.attempt_id, ATTEMPT, "저장된 Attempt 를 안 썼다");
    assert_eq!(grant.coordinator_device_id, COORDINATOR);
    assert_eq!(
        grant.coordinator_term, TERM,
        "coordinator_term 이 저장된 값이 아니다 — 기존 경로는 리터럴 1 을 박아 넣었다"
    );
    assert_eq!(grant.issued_at_unix_ms, GRANT_ISSUED);
    assert_eq!(grant.expires_at_unix_ms, GRANT_EXPIRES);
    assert!(
        grant.lease_from_durable_store,
        "저장소에서만 읽었는데 그 비트가 false 다"
    );

    // nested Lease 도 **독립적으로** 검증돼야 한다.
    let lease = grant.lease.as_ref().expect("Grant 에 Lease 가 없다");
    let mut keys = InMemoryKeyring::new();
    keys.insert(
        COORDINATOR,
        SigningKey::from_bytes(&seed_bytes(COORD_SEED)).verifying_key(),
    );
    let verified_lease = verify(
        lease,
        1,
        &Ed25519Verifier::new(keys),
        GRANT_ISSUED,
        &mut NoReplayCheck,
    )
    .expect("nested Lease 가 독립 검증되지 않는다");
    let lease = verified_lease.get();
    assert_eq!(lease.lease_id, LEASE);
    assert_eq!(lease.job_id, JOB);
    assert_eq!(lease.attempt_id, ATTEMPT);
    assert_eq!(lease.holder_node_id, NODE);
    assert_eq!(lease.coordinator_term, TERM);
    assert_eq!(lease.issued_at_unix_ms, LEASE_ISSUED);
    assert_eq!(lease.expires_at_unix_ms, LEASE_EXPIRES);
    assert_eq!(lease.renew_after_unix_ms, LEASE_RENEW);
    // fence epoch 는 저장소가 채번한다 — 값을 손으로 못 정하므로
    // **0 이 아니라는 것**만 본다. 그 이상을 주장하면 지어내는 것이다.
    assert!(lease.fence_epoch > 0, "fence epoch 가 채번되지 않았다");
}

/// 같은 입력이면 **바이트까지 같은** Grant 가 나온다.
///
/// ★ 시계를 읽었다면 이 테스트는 통과할 수 없다. `stage-job` 에서 배운
///   것을 여기서도 지키고 있는지 재는 장치다.
#[test]
fn reissuing_with_the_same_input_produces_an_identical_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let first = dir.path().join("a.pb");
    let second = dir.path().join("b.pb");

    assert!(issue(&db, &key_file, &first, &[]).0);
    assert!(issue(&db, &key_file, &second, &[]).0);

    let a = std::fs::read(&first).expect("읽기");
    let b = std::fs::read(&second).expect("읽기");
    assert!(!a.is_empty(), "빈 파일을 비교하고 있다");
    assert_eq!(a, b, "같은 입력인데 Grant 바이트가 다르다");
}

/// ★★ **STAGING 이 아닌 Job 에는 발급하지 않는다.**
///
/// 예약이 없는데 Grant 를 내면 Agent 는 자기가 그 노드를 쓸 권한이
/// 있다고 믿는다.
///
/// ★ 이 테스트는 처음에 **이름이 거짓말했다** — `not_staging` 이라고
///   해 놓고 실제로는 *모르는 Job* 을 시험했다. 그래서 STAGING 관문을
///   통째로 지우는 뮤테이션(G3)이 안 잡혔다. 이제 진짜로 `QUEUED` 인
///   Job 을 준다.
#[test]
fn a_queued_but_unstaged_job_gets_no_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    // 두 번째 Job 을 큐까지만 올린다 — 노드가 하나뿐이라 예약은 못 한다.
    let second = "01JJOBGRANTB00000000001";
    queue_only(dir.path(), &db, second, "1102030405060708090a0b0c0d0e0f10");

    let (ok, output) = run_cli(&[
        "issue-grant",
        "--job-id",
        second,
        "--control-db",
        db.to_str().unwrap(),
        "--attempt-id",
        ATTEMPT,
        "--lease-id",
        LEASE,
        "--grant-id",
        GRANT,
        "--grant-issued-at-unix-ms",
        "1800000010000",
        "--grant-expires-at-unix-ms",
        "1800000070000",
        "--coordinator-key-file",
        key_file.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(!ok, "예약 없는 Job 에 Grant 를 냈다: {output}");
    assert!(
        output.contains("STAGING 이 아니다"),
        "이유를 안 말한다: {output}"
    );
    assert!(!out.exists(), "거부했는데 파일을 남겼다");

    // 대조 — 예약된 Job 은 발급된다. 없으면 "항상 거부" 로도 통과한다.
    assert!(issue(&db, &key_file, &out, &[]).0, "정상 경로가 실패했다");
}

/// 모르는 Job 은 발급하지 않는다.
#[test]
fn an_unknown_job_gets_no_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    let (ok, output) = run_cli(&[
        "issue-grant",
        "--job-id",
        "01JJOBGRANTNOSUCH000001",
        "--control-db",
        db.to_str().unwrap(),
        "--attempt-id",
        ATTEMPT,
        "--lease-id",
        LEASE,
        "--grant-id",
        GRANT,
        "--grant-issued-at-unix-ms",
        "1800000010000",
        "--grant-expires-at-unix-ms",
        "1800000070000",
        "--coordinator-key-file",
        key_file.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(!ok, "모르는 Job 으로 발급했다: {output}");
    assert!(output.contains("모른다"), "이유를 안 말한다: {output}");
    assert!(!out.exists(), "거부했는데 파일을 남겼다");
}

/// Attempt 나 Lease 가 저장소에 없으면 발급하지 않는다.
#[test]
fn an_unknown_attempt_or_lease_gets_no_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    for (flag, value, expected) in [
        ("--attempt-id", "01JATTEMPTNOSUCH0000000001", "Attempt"),
        ("--lease-id", "01JLEASENOSUCH0000000001", "Lease"),
    ] {
        let (ok, output) = issue(&db, &key_file, &out, &[flag, value]);
        assert!(!ok, "{flag} 가 없는데 발급했다: {output}");
        assert!(
            output.contains(expected) && output.contains("저장소에 없다"),
            "{flag}: 무엇이 없는지 안 말한다: {output}"
        );
        assert!(!out.exists(), "{flag}: 거부했는데 파일을 남겼다");
    }
}

/// Grant 가 Lease 보다 오래 살면 거부한다.
///
/// Agent 가 Lease 없이도 계속 돌아도 된다고 읽을 수 있다.
#[test]
fn a_grant_outliving_its_lease_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    let (ok, output) = issue(
        &db,
        &key_file,
        &out,
        &["--grant-expires-at-unix-ms", "1800000600001"], // Lease 만료 +1ms
    );
    assert!(!ok, "Lease 보다 오래 사는 Grant 를 냈다: {output}");
    assert!(output.contains("보다 늦다"), "이유를 안 말한다: {output}");
    assert!(!out.exists());

    // 대조 — 정확히 Lease 만료까지는 허용한다. 없으면 "항상 거부" 로도
    // 통과한다.
    let (ok, output) = issue(
        &db,
        &key_file,
        &out,
        &["--grant-expires-at-unix-ms", "1800000600000"],
    );
    assert!(ok, "Lease 만료와 같은 시각을 거부했다: {output}");
}

/// 발급 시각에 Lease 가 이미 만료면 거부한다(경계 포함).
#[test]
fn an_expired_lease_at_issue_time_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    // 정확히 만료 시각 — `<=` 규칙상 거부다.
    let (ok, output) = issue(
        &db,
        &key_file,
        &out,
        &[
            "--grant-issued-at-unix-ms",
            "1800000600000",
            "--grant-expires-at-unix-ms",
            "1800000600001",
        ],
    );
    assert!(!ok, "만료 경계에서 발급했다: {output}");
    // ★★ **이유까지 봐야 이 관문이 재진다.** 이 입력은 "Lease 가 이미
    //   만료" 와 "Grant 만료가 Lease 만료보다 늦다" **둘 다** 성립한다
    //   — `!ok` 만 보면 어느 쪽이 막았는지 모르고, 실제로 앞의 검사를
    //   지우는 뮤테이션(H3)이 안 잡혔다.
    //
    //   ★ 둘을 **떼어 낼 입력은 없다**: 앞선 검사가 `발급 < 만료` 를
    //     요구하므로 `lease.만료 <= 발급` 이면 `만료 > lease.만료` 도
    //     반드시 참이다. 그래서 이 검사는 홀로 막는 일이 없고, 값어치는
    //     **더 정확한 이유를 먼저 말하는 것**이다(`CLAUDE.md` §3).
    assert!(
        output.contains("Lease 가 이미 만료"),
        "다른 관문이 먼저 막았다 — 이유가 덜 정확해진다: {output}"
    );
    assert!(!out.exists());

    // 대조 — 만료 1ms 전은 발급된다.
    let (ok, output) = issue(
        &db,
        &key_file,
        &out,
        &[
            "--grant-issued-at-unix-ms",
            "1800000599999",
            "--grant-expires-at-unix-ms",
            "1800000600000",
        ],
    );
    assert!(ok, "만료 직전을 거부했다: {output}");
}

/// 서명키 파일이 형식에 안 맞으면 발급하지 않는다.
#[test]
fn a_malformed_key_file_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, _) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    for (name, body) in [("short.key", "abcd"), ("bad.key", &"zz".repeat(32)[..])] {
        let bad = dir.path().join(name);
        std::fs::write(&bad, body).expect("키 파일 쓰기");
        let (ok, output) = issue(&db, &bad, &out, &[]);
        assert!(!ok, "{name} 을 받아들였다: {output}");
        assert!(!out.exists(), "{name}: 거부했는데 파일을 남겼다");
    }
}

/// 비영속 DB 는 거부한다 — 없는 예약으로 Grant 를 만들게 된다.
#[test]
fn a_non_durable_control_db_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let key_file = dir.path().join("coordinator.key");
    std::fs::write(&key_file, COORD_SEED).expect("키 파일 쓰기");
    let out = dir.path().join("grant.pb");

    for label in [":memory:", ""] {
        let (ok, output) = run_cli(&[
            "issue-grant",
            "--job-id",
            JOB,
            "--control-db",
            label,
            "--attempt-id",
            ATTEMPT,
            "--lease-id",
            LEASE,
            "--grant-id",
            GRANT,
            "--grant-issued-at-unix-ms",
            "1800000010000",
            "--grant-expires-at-unix-ms",
            "1800000070000",
            "--coordinator-key-file",
            key_file.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        assert!(!ok, "{label:?} 를 받아들였다: {output}");
        assert!(
            output.contains("영속이 아니다"),
            "{label:?}: 이유를 안 말한다: {output}"
        );
    }
}

/// ★★ **폐기된 Lease 로는 Grant 를 못 낸다.**
///
/// `grant_from_stored.rs` 에 그 관문이 있는데 **아무 테스트도 그 자리를
/// 밟지 않았다** — 뮤테이션 G7(폐기 검사를 무력화)을 걸어도 9건이 전부
/// 통과했다. CLI 로는 Lease 를 폐기할 방법이 없어서 저장소 API 를 직접
/// 부른다(`send_revoke_notice` 가 wire 전송 전에 하는 것과 같은 호출).
#[test]
fn a_revoked_lease_yields_no_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());

    // 대조 — 폐기 전에는 실제로 발급된다. 없으면 "항상 거부" 로도 통과한다.
    let before = dir.path().join("before.pb");
    let (ok, output) = issue(&db, &key_file, &before, &[]);
    assert!(ok, "폐기 전인데 거부했다: {output}");

    gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&db)
        .expect("lease store")
        .mark_revoked(LEASE, LEASE_ISSUED + 1)
        .expect("폐기");

    let out = dir.path().join("after.pb");
    let (ok, output) = issue(&db, &key_file, &out, &[]);
    assert!(!ok, "폐기된 Lease 로 Grant 를 냈다: {output}");
    assert!(
        output.contains("폐기") || output.to_lowercase().contains("revoked"),
        "거부 이유가 폐기가 아니다: {output}"
    );
    assert!(!out.exists(), "거부했는데 파일을 남겼다");
}

// ══════════════════════════════════════════════════════════════════════
// DB 변조 방어 — 뮤테이션 G5·G6 가 안 잡혔던 두 관문
// ══════════════════════════════════════════════════════════════════════

/// ★★ **Attempt 와 Lease 의 fence epoch 를 어긋나게 만든다.**
///
/// `grant_from_stored.rs` 의 대조를 지워도 아무 테스트가 안 깨졌다
/// (뮤테이션 G5). 셋이 한 트랜잭션에서 함께 쓰이므로 정상 경로로는 이
/// 상태를 못 만든다 — 그래서 **DB 를 직접 고쳐** 만든다.
#[test]
fn a_lease_whose_fence_disagrees_with_its_attempt_yields_no_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());

    let before = dir.path().join("before.pb");
    let (ok, output) = issue(&db, &key_file, &before, &[]);
    assert!(ok, "변조 전인데 거부했다: {output}");

    // ★ `fence_epoch` 은 **BLOB(8바이트 big-endian)** 이다. 처음에
    //   `fence_epoch + 1` 로 정수를 넣었더니 저장소가 타입 오류로 막았고
    //   — 재려던 관문이 아니라 **디코더**가 잡은 것이었다. 뮤테이션이
    //   안 잡히는 걸 보고 알았다.
    let connection = rusqlite::Connection::open(&db).expect("DB 열기");
    let current: Vec<u8> = connection
        .query_row(
            "SELECT fence_epoch FROM coordinator_attempts WHERE attempt_id = ?1",
            [ATTEMPT],
            |row| row.get(0),
        )
        .expect("현재 fence 조회");
    assert_eq!(current.len(), 8, "fence_epoch 인코딩이 바뀌었다");
    let bumped = (u64::from_be_bytes(current.try_into().unwrap()) + 1).to_be_bytes();
    let changed = connection
        .execute(
            "UPDATE coordinator_attempts SET fence_epoch = ?2 WHERE attempt_id = ?1",
            rusqlite::params![ATTEMPT, bumped.to_vec()],
        )
        .expect("변조");
    assert_eq!(changed, 1, "변조할 Attempt 행이 없다");
    drop(connection);

    let out = dir.path().join("after.pb");
    let (ok, output) = issue(&db, &key_file, &out, &[]);
    assert!(!ok, "fence 가 어긋나는데 Grant 를 냈다: {output}");
    assert!(
        output.contains("fence epoch 가 다르다"),
        "재려던 관문이 아니라 다른 곳에서 막혔다: {output}"
    );
    assert!(!out.exists(), "거부했는데 파일을 남겼다");
}

/// ★★ **예약이 다른 Job 의 것일 때 거부하는가.**
///
/// ★ 처음에는 예약을 **지웠는데** 그건 *존재* 검사에 걸렸다 — 뮤테이션
///   G6 가 지운 것은 *불일치* 검사라 안 잡혔다. 같은 자리를 재려면 행을
///   남겨 두고 **주인만 바꿔야** 한다.
///
/// 이 상태가 통과하면 **남이 잡고 있는 노드에 실행 허가가 나간다.**
#[test]
fn a_reservation_held_by_another_job_yields_no_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());

    let before = dir.path().join("before.pb");
    let (ok, output) = issue(&db, &key_file, &before, &[]);
    assert!(ok, "변조 전인데 거부했다: {output}");

    let connection = rusqlite::Connection::open(&db).expect("DB 열기");
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("외래 키 끄기");
    let changed = connection
        .execute(
            "UPDATE coordinator_node_reservations SET job_id = 'someone-elses-job'",
            [],
        )
        .expect("변조");
    assert_eq!(changed, 1, "변조할 예약이 없다");
    drop(connection);

    let out = dir.path().join("after.pb");
    let (ok, output) = issue(&db, &key_file, &out, &[]);
    assert!(!ok, "남의 예약인데 Grant 를 냈다: {output}");
    assert!(
        output.contains("someone-elses-job"),
        "재려던 관문이 아니라 다른 곳에서 막혔다: {output}"
    );
    assert!(!out.exists(), "거부했는데 파일을 남겼다");
}

/// ★★ **Attempt 가 다른 Job 의 것일 때 거부하는가** (뮤테이션 H2).
///
/// 정상 경로로는 못 만든다 — Attempt 와 Job 은 함께 쓰인다. DB 를 고친다.
#[test]
fn an_attempt_belonging_to_another_job_yields_no_grant() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());

    let before = dir.path().join("before.pb");
    let (ok, output) = issue(&db, &key_file, &before, &[]);
    assert!(ok, "변조 전인데 거부했다: {output}");

    let connection = rusqlite::Connection::open(&db).expect("DB 열기");
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("외래 키 끄기");
    // ★★ **Attempt 만 고치면 이 관문을 못 잰다.** 더 앞에 Attempt↔Lease
    //   대조가 있어서 거기서 먼저 걸리고, 그 메시지에도 'another-job' 이
    //   들어가 내 단언이 통과해 버렸다 — 뮤테이션이 안 잡히는 걸 보고
    //   알았다. 둘을 **같이** 고쳐야 앞 대조를 통과하고 이 자리에 닿는다.
    let changed = connection
        .execute(
            "UPDATE coordinator_attempts SET job_id = 'another-job' WHERE attempt_id = ?1",
            [ATTEMPT],
        )
        .expect("Attempt 변조");
    assert_eq!(changed, 1, "변조할 Attempt 행이 없다");
    let changed = connection
        .execute(
            "UPDATE coordinator_leases SET job_id = 'another-job' WHERE lease_id = ?1",
            [LEASE],
        )
        .expect("Lease 변조");
    assert_eq!(changed, 1, "변조할 Lease 행이 없다");
    drop(connection);

    let out = dir.path().join("after.pb");
    let (ok, output) = issue(&db, &key_file, &out, &[]);
    assert!(!ok, "남의 Job 의 Attempt 인데 Grant 를 냈다: {output}");
    assert!(
        output.contains("다른 Job 의 것이다"),
        "재려던 관문이 아니라 다른 곳에서 막혔다: {output}"
    );
    assert!(!out.exists(), "거부했는데 파일을 남겼다");
}

/// 이미 있는 출력 파일을 말없이 덮지 않는다.
///
/// ★★ 2026-09-10 독립 검수 지적의 회귀 테스트다.
///
/// 검수가 든 반례: 운영자가 `--coordinator-key-file` 에 **엉뚱한 키**를 주면
/// 이 명령은 그것을 잡지 못한다(발급자 이름에 그 키의 공개키를 등록해
/// 자기 검증하므로 자기 검증도 못 잡는다). 그러면 `GRANTED` 를 찍으면서
/// **멀쩡하던 Grant 파일을 못 쓰는 것으로 바꿔 놓는다.**
///
/// ★ 키와 신원의 불일치 자체는 여기서 못 고친다 — Coordinator 공개키
///   목록이 있어야 하고 아직 없다. 고칠 수 있는 것은 **그 실수가 기존
///   파일을 파괴하지 않게** 하는 것이고, 이 테스트가 그것을 고정한다.
#[test]
fn an_existing_out_file_is_not_clobbered() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    let (ok, output) = issue(&db, &key_file, &out, &[]);
    assert!(ok, "정상 경로가 실패했다: {output}");
    let first = std::fs::read(&out).expect("첫 Grant 를 읽는다");
    assert!(!first.is_empty(), "첫 Grant 가 비어 있다");

    // 같은 자리에 다시 낸다 — 거부해야 한다.
    let (ok2, output2) = issue(&db, &key_file, &out, &[]);
    assert!(!ok2, "이미 있는 파일을 말없이 덮었다: {output2}");
    assert!(
        output2.contains("GRANT_REFUSED: OUT_EXISTS"),
        "거부했는데 이유가 파일 존재가 아니다: {output2}"
    );

    // ★ 핵심 — 원래 파일이 **바이트 그대로** 남아야 한다.
    let after = std::fs::read(&out).expect("거부 뒤에도 파일이 있어야 한다");
    assert_eq!(first, after, "거부했는데 기존 파일이 바뀌었다");

    // 임시 파일이 남지 않는다.
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .expect("디렉터리")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".tmp."))
        .collect();
    assert!(leftovers.is_empty(), "임시 파일이 남았다: {leftovers:?}");
}

/// 명시적으로 요청하면 덮어쓴다.
///
/// ★ 이 대조군이 없으면 "항상 거부" 로 고쳐도 위 테스트가 통과한다.
#[test]
fn an_explicit_flag_allows_replacing_the_file() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (db, key_file) = staged(dir.path());
    let out = dir.path().join("grant.pb");

    // 운영자가 손으로 만들어 둔 것을 흉내낸다.
    std::fs::write(&out, b"stale bytes that are not a grant").expect("미리 쓴다");

    let (ok, output) = issue(
        &db,
        &key_file,
        &out,
        &["--overwrite-existing-grant", "true"],
    );
    assert!(ok, "명시적 덮어쓰기가 실패했다: {output}");

    let bytes = std::fs::read(&out).expect("Grant 를 읽는다");
    assert_ne!(
        bytes, b"stale bytes that are not a grant",
        "덮어쓴다고 했는데 옛 내용이 그대로다"
    );
    assert!(!bytes.is_empty(), "덮어썼는데 비어 있다");
}
