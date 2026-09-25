//! `gputeer scheduler-tick` — **스스로 골라 예약한다.**
//!
//! ★ **무게중심은 멱등이다.** `stage-job` 은 운영자가 식별자와 시각을
//!   직접 주므로 재시도가 같은 값이면 같은 결과다. tick 은 그것을
//!   **스스로 만들어야** 하는데, 시계나 난수로 만들면 재시도가 같은 값을
//!   갖는다고 보장할 수 없다(재검수 28·30 — 전에는 "재시도마다 달라져 멱등이
//!   거짓말이 된다" 고 적었다).
//!
//!   그래서 식별자는 `(job_id, plan_id)` 에서, Lease 시각은 저장된 `queued_at` 과
//!   운영자 오프셋에서 유도한다.
//!   이 파일이 그게 실제로 성립하는지 잰다.

use std::path::{Path, PathBuf};
use std::process::Command;

use gputeer_coordinator::job_store::{CoordinatorJobStore, JobState};

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) {
        "gputeer.exe"
    } else {
        "gputeer"
    })
}

const NODE: &str = "node-tick-a";
const SUBMITTER: &str = "01JSUBMITTERTICK00000001";
const SEED: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc11";
const OWNER: &str = "owner-tick";
const COORDINATOR: &str = "01JCOORDINATORTICK000001";
const AXES: &str = "vram,gpu_count,cpu,ram,workspace";
const JOB_A: &str = "01JJOBTICKA0000000000001";
const JOB_B: &str = "01JJOBTICKB0000000000001";

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

/// 거부 사유를 **오류 줄의 시작**으로 확인한다.
///
/// ★★ `output.contains("사유")` 로 보면 안 된다. 2026-09-10 독립 검수가
///   찾아낸 함정이다 — 인자 검사 오류가 사용자 입력을 메시지에 그대로
///   넣으므로, `--best-fit-axes "already reserved,..."` 처럼 주면
///   **축 파서에서 죽으면서도** `contains("already reserved")` 를 통과한다.
///   그러면 테스트는 초록인데 재려던 관문은 한 번도 안 돈다.
///
/// 그래서 코드를 줄 **머리**에서 본다. 사용자 입력은 코드 뒤에만 들어가므로
/// 앞 관문의 오류가 뒤 관문의 코드를 흉내낼 수 없다.
fn refused_with(output: &str, code: &str) -> bool {
    let want = format!("scheduler-tick 실패: {code}");
    output.lines().any(|line| line.starts_with(&want))
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_millis() as u64
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

/// `gpu_count` 로 노드 수를 정한다 — 두 Job 을 각각 예약하려면 둘이 필요하다.
fn write_bootstrap(dir: &Path, nodes: usize) -> PathBuf {
    let key: String = gputeer_crypto::SigningKey::from_bytes(&[31u8; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let observed = now_unix_ms();
    let agents: Vec<String> = (0..nodes)
        .map(|i| {
            let node = if i == 0 {
                NODE.to_string()
            } else {
                format!("{NODE}-{i}")
            };
            let gpu = format!("{node}-gpu-0");
            let device = format!("device-tick-{i}");
            let node_key = if i == 0 {
                key.clone()
            } else {
                gputeer_crypto::SigningKey::from_bytes(&[40u8 + i as u8; 32])
                    .verifying_key()
                    .to_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect()
            };
            format!(
                r#"    {{
      "registry": {{
        "node_id": "{node}", "device_id": "{device}",
        "owner_member_id": "{OWNER}", "verifying_key_hex": "{node_key}",
        "node_state": "ONLINE", "risk_state": "NORMAL",
        "security_tier": "S2", "isolation_class": "CONTAINED",
        "key_protection": "K1"
      }},
      "inventory": {{
        "inventory_revision": 1, "observed_at_unix_ms": {observed},
        "gpus": [{{ "gpu_id": "{gpu}", "model": "RTX 4070 SUPER",
                    "healthy": true, "available_vram_bytes": 12884901888 }}],
        "available_cpu_cores": 16, "available_ram_bytes": 34359738368,
        "available_workspace_bytes": 107374182400,
        "allowed_workload_classes": ["TRAINING"],
        "third_party_workloads_opt_in": true
      }}
    }}"#
            )
        })
        .collect();
    let path = dir.join("bootstrap.json");
    std::fs::write(
        &path,
        format!(
            "{{\n  \"schema_version\": 1,\n  \"agents\": [\n{}\n  ]\n}}",
            agents.join(",\n")
        ),
    )
    .expect("문서 쓰기");
    path
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
    queue_job_expiring(
        dir,
        keyring,
        db,
        job_id,
        idem,
        now_unix_ms() + 7 * 24 * 3_600_000,
    );
}

/// Manifest 만료 시각을 정해 큐에 넣는다(결함 423 시험).
fn queue_job_expiring(
    dir: &Path,
    keyring: &Path,
    db: &Path,
    job_id: &str,
    idem: &str,
    expires_at_unix_ms: u64,
) {
    let manifest = dir.join(format!("{job_id}.pb"));
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = expires_at_unix_ms.to_string();
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
    let (ok, out) = run_cli(&args);
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
        idem,
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ]);
    assert!(ok, "import-manifest 실패: {out}");

    let (ok, out) = run_cli(&[
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
    assert!(ok, "plan-job 실패: {out}");
}

fn tick(keyring: &Path, db: &Path, extra: &[&str]) -> (bool, String) {
    let mut args: Vec<&str> = vec![
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
        "300000",
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ];
    args.extend_from_slice(extra);
    run_cli(&args)
}

/// `scheduler-loop` 은 tick 과 **같은 인자**를 받고, 루프 인자만 더 붙인다.
fn loop_run(keyring: &Path, db: &Path, interval_ms: &str, max_ticks: &str) -> (bool, String) {
    let args: Vec<&str> = vec![
        "scheduler-loop",
        "--interval-ms",
        interval_ms,
        "--max-ticks",
        max_ticks,
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
        "300000",
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ];
    run_cli(&args)
}

fn job_state(db: &Path, job_id: &str) -> Option<JobState> {
    let store = CoordinatorJobStore::open(db).expect("job store");
    store.get(job_id).expect("조회").map(|job| job.state)
}

/// inventory 를 넣고 Job 하나를 큐에 올린다.
fn prepared(dir: &Path, nodes: usize) -> (PathBuf, PathBuf) {
    let db = dir.join("control.sqlite3");
    let bootstrap = write_bootstrap(dir, nodes);
    let (ok, out) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory 실패: {out}");
    let keyring = write_keyring(dir);
    queue_job(
        dir,
        &keyring,
        &db,
        JOB_A,
        "0102030405060708090a0b0c0d0e0f10",
    );
    (keyring, db)
}

// ─────────────────────────────────────────────────────────────────────

/// ★★ **운영자가 식별자를 안 줘도 스스로 골라 예약한다.**
#[test]
fn a_tick_picks_a_queued_job_and_stages_it_without_operator_identifiers() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));

    let (ok, output) = tick(&keyring, &db, &[]);
    assert!(ok, "tick 실패: {output}");
    assert!(output.contains("TICK_STAGED"), "출력이 다르다: {output}");
    assert!(output.contains(NODE), "어느 노드인지 안 말한다: {output}");
    assert!(
        output.contains("created=true"),
        "최초 예약이 아니다: {output}"
    );
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));
}

/// ★★ **같은 Job 을 다시 tick 해도 같은 Attempt 다.**
///
/// 시계나 난수로 식별자를 만들면 두 번째 tick 이
/// `operation key payload conflict` 로 실패하거나 새 Attempt 를 만든다.
#[test]
fn ticking_the_same_job_twice_returns_the_same_attempt() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);

    // 예약 **전** 상태를 떠 둔다 — 이게 재현의 기준점이다.
    let snapshot_before_tick = dir.path().join("before.sqlite3");
    std::fs::copy(&db, &snapshot_before_tick).expect("DB 복사");

    let (ok, first) = tick(&keyring, &db, &[]);
    assert!(ok, "1회차 실패: {first}");
    // 두 번째는 Job 이 이미 STAGING 이므로 큐에서 빠졌다 — 유휴다.
    let (ok, second) = tick(&keyring, &db, &[]);
    assert!(ok, "2회차 실패: {second}");
    assert!(
        second.contains("TICK_IDLE"),
        "예약된 Job 이 아직 큐에 있다: {second}"
    );

    // ★ 그러나 **식별자 유도 자체**는 재현 가능해야 한다.
    //
    //   ★ 처음에는 `prepared()` 를 다시 불러 새 DB 를 만들고 비교했는데
    //     **그건 같은 입력이 아니었다** — `submit` 이 `issued_at` 에
    //     현재 시각을 넣어 Manifest 바이트가 달라지고, 그러면
    //     `manifest_hash` -> `plan_id` -> Attempt 가 전부 달라진다.
    //     그건 결함이 아니라 **의도한 동작**이다(다른 계획이면 다른
    //     Attempt 여야 한다). 내 전제가 틀렸다.
    //
    //   진짜 "같은 입력" 은 **예약 전 DB 를 그대로 복사**하는 것이다.
    let _ = &first;
    let replay_db = dir.path().join("replay.sqlite3");
    std::fs::copy(&snapshot_before_tick, &replay_db).expect("DB 복사");
    let (ok, replay) = tick(&keyring, &replay_db, &[]);
    assert!(ok, "복제본 tick 실패: {replay}");

    let field = |line: &str, key: &str| -> String {
        line.split_whitespace()
            .find_map(|t| t.strip_prefix(key).map(str::to_string))
            .unwrap_or_else(|| panic!("{key} 가 출력에 없다: {line}"))
    };
    assert_eq!(
        field(&first, "attempt="),
        field(&replay, "attempt="),
        "같은 (job_id, plan_id) 인데 Attempt 가 달라졌다 — 시계나 난수를 썼다"
    );
    assert_eq!(
        field(&first, "lease="),
        field(&replay, "lease="),
        "같은 (job_id, plan_id) 인데 Lease 가 달라졌다"
    );
}

/// 큐가 비면 **오류가 아니다.**
///
/// 루프가 이걸 실패로 세면 정상 유휴 상태가 장애로 보인다.
#[test]
fn an_empty_queue_is_idle_not_an_error() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    let bootstrap = write_bootstrap(dir.path(), 1);
    let (ok, out) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory 실패: {out}");
    let keyring = write_keyring(dir.path());

    let (ok, output) = tick(&keyring, &db, &[]);
    assert!(ok, "빈 큐를 오류로 다뤘다: {output}");
    assert!(output.contains("TICK_IDLE"), "출력이 다르다: {output}");
}

/// ★★ **덫을 뒤집었다 — 후보 선택이 이제 예약을 안다**(2026-09-22 · 결정 `B′`).
///
/// 전에는 이 자리에 "찾은 공백" 이 있었다. 노드가 **둘**인데 두 번째 tick 이
/// `TICK_REFUSED: node is already reserved` 로 실패했다 — `evaluate_eligibility` 가
/// **inventory 만** 보고 예약은 다른 테이블에 있어서, best-fit 이 늘 같은 노드를 골라
/// 예약 관문에서 막혔기 때문이다. 안전 문제는 아니었지만 **루프를 돌리면 큐 맨 앞에서
/// 영영 멈추는** 구조였고, 그게 데몬화의 실질적 차단 요인이었다.
///
/// 그때 이 테스트는 **오늘의 동작을 고정하는 덫**이었고, 주석에 이렇게 적어 뒀다 —
/// "메우면 이 테스트가 실패하며 문서도 같이 고치라고 알린다". 실제로 그렇게 됐다.
///
/// 이제 기대를 뒤집는다: **두 번째 tick 은 비어 있는 다른 노드를 골라 성공해야 한다.**
/// 예약은 `pool_snapshot()` 한 곳에서 접히고(결정 `B′`), hard-filter 가
/// `AlreadyReserved` 로 거른다.
#[test]
fn a_second_tick_picks_the_free_node_instead_of_the_reserved_one() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 2);
    // 두 번째 Job 을 뒤이어 큐에 올린다.
    queue_job(
        dir.path(),
        &keyring,
        &db,
        JOB_B,
        "1102030405060708090a0b0c0d0e0f10",
    );

    let (ok, first) = tick(&keyring, &db, &[]);
    assert!(ok, "1회차 실패: {first}");
    assert!(
        first.contains(&format!("job_id={JOB_A}")),
        "먼저 들어온 Job 을 안 골랐다: {first}"
    );

    // ★ 노드가 둘이므로 두 번째도 간다 — 잡힌 노드를 피해 간다.
    let (ok, second) = tick(&keyring, &db, &[]);
    assert!(
        ok,
        "2회차가 막혔다 — 예약이 후보 선택에 반영되지 않는다: {second}"
    );
    assert!(
        second.contains(&format!("job_id={JOB_B}")),
        "두 번째 Job 을 안 골랐다: {second}"
    );

    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));
    assert_eq!(
        job_state(&db, JOB_B),
        Some(JobState::Staging),
        "두 번째 Job 도 STAGING 으로 가야 한다 — 큐가 더 이상 맨 앞에서 막히지 않는다"
    );
}

/// ★★ 결함 211 (2026-09-23) — **큐에서 TTL 보다 오래 기다린 작업도 배치되고, 살아 있는 Lease 를 받는다.**
///
/// 뒤집은 덫이다. 전에는 이 시험이 `QUEUE_TOO_OLD` 거부를 고정했다 — 발급 시각이 `queued_at` 이라 오래
/// 기다린 작업의 Lease 는 태어나자마자 만료였고, 그래서 거부했다. 거부는 정직했지만 그 작업은 **영영**
/// 배치되지 않았다. 이제 발급 시각은 배치하는 순간이다.
#[test]
fn a_job_that_waited_longer_than_the_lease_ttl_still_gets_a_live_lease() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);
    // queued_at 보다 TTL(2ms) 이상 지난 뒤에 돈다 — 전에는 여기서 QUEUE_TOO_OLD 였다.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let before = now_ms();
    let (ok, output) = tick(
        &keyring,
        &db,
        &["--lease-ttl-ms", "2", "--lease-renew-after-ms", "1"],
    );
    assert!(ok, "오래 기다린 작업을 거부했다: {output}");
    assert!(output.contains("TICK_STAGED"), "예약하지 않았다: {output}");
    let lease_id = output
        .split_whitespace()
        .find_map(|t| t.strip_prefix("lease="))
        .expect("출력에 lease= 가 없다")
        .to_string();
    let stored = gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&db)
        .expect("lease store")
        .get(&lease_id)
        .expect("조회")
        .expect("Lease");
    assert!(
        stored.issued_at_unix_ms >= before,
        "발급 시각이 배치 순간보다 앞이다(큐 진입 시각을 썼다): issued={} before={before}",
        stored.issued_at_unix_ms
    );
    assert_eq!(stored.expires_at_unix_ms, stored.issued_at_unix_ms + 2);
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));
}

#[test]
fn a_generous_ttl_still_places_the_job() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);
    // 대조 — 넉넉한 TTL 이면 예약된다. `assert!(ok)` 만 보지 않는다(2026-09-07 검수 — TICK_IDLE 조기 반환도
    //   종료 코드 0 이다).
    let (ok, output) = tick(&keyring, &db, &[]);
    assert!(ok, "넉넉한 TTL 에서도 거부했다: {output}");
    assert!(
        output.contains("TICK_STAGED"),
        "성공했다는데 예약을 안 했다 — TICK_IDLE 로 빠졌을 수 있다: {output}"
    );
    assert_eq!(
        job_state(&db, JOB_A),
        Some(JobState::Staging),
        "TICK_STAGED 라고 찍었는데 Job 상태가 안 바뀌었다"
    );
}

/// ★★ **Lease 발급 시각이 정말 배치하는 순간인가 — 저장된 값으로 잰다.**
///
/// ★ 결함 211 (2026-09-23) 로 기대를 바꿨다 — 전에는 `queued_at` 이어야 했다.
///
/// 이 모듈의 무게중심인데 **아무 테스트도 안 재고 있었다** — 멱등
/// 테스트는 attempt/lease **식별자**만 비교하고, 식별자는 시각에서
/// 유도되지 않으므로 발급 시각을 시계로 바꿔도 통과했다(뮤테이션 T2).
///
/// 이제 저장소를 열어 실제 행을 본다.
#[test]
fn the_stored_lease_is_issued_at_placement_time_not_queue_entry() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);

    let queued_at = {
        let store = CoordinatorJobStore::open(&db).expect("job store");
        store
            .get(JOB_A)
            .expect("조회")
            .expect("Job")
            .queued_at_unix_ms
            .expect("queued_at")
    };

    let before = now_ms();
    let (ok, output) = tick(&keyring, &db, &[]);
    let after = now_ms();
    assert!(ok, "tick 실패: {output}");
    let lease_id = output
        .split_whitespace()
        .find_map(|t| t.strip_prefix("lease="))
        .expect("출력에 lease= 가 없다")
        .to_string();

    let stored = gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&db)
        .expect("lease store")
        .get(&lease_id)
        .expect("조회")
        .expect("Lease");

    let issued = stored.issued_at_unix_ms;
    assert!(
        issued >= queued_at && issued >= before && issued <= after,
        "발급 시각이 배치 순간이 아니다: issued={issued} queued={queued_at} before={before} after={after}"
    );
    assert_eq!(
        stored.expires_at_unix_ms,
        issued + 600_000,
        "만료가 발급 시각 + TTL 이 아니다"
    );
    assert_eq!(
        stored.renew_after_unix_ms,
        issued + 300_000,
        "갱신 시점이 발급 시각 + 오프셋이 아니다"
    );
}

/// 갱신 시점이 만료 뒤면 받아 주지 않는다.
#[test]
fn a_renew_offset_at_or_after_the_ttl_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);

    for renew in ["600000", "700000", "0"] {
        let (ok, output) = tick(&keyring, &db, &["--lease-renew-after-ms", renew]);
        assert!(
            !ok,
            "--lease-renew-after-ms {renew} 를 받아들였다: {output}"
        );
        assert!(
            refused_with(&output, "TICK_ARGS_REFUSED: RENEW_AFTER_NOT_BEFORE_TTL"),
            "이유를 안 말한다: {output}"
        );
        assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));
    }
}

/// 비영속 DB 는 거부한다.
#[test]
fn a_non_durable_control_db_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let keyring = write_keyring(dir.path());
    for label in [":memory:", ""] {
        let (ok, output) = run_cli(&[
            "scheduler-tick",
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
            "3",
            "--lease-ttl-ms",
            "600000",
            "--lease-renew-after-ms",
            "300000",
            "--lease-max-total-duration-seconds",
            "86400",
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ]);
        assert!(!ok, "{label:?} 를 받아들였다: {output}");
        assert!(
            refused_with(&output, "TICK_ARGS_REFUSED: CONTROL_DB_NOT_DURABLE"),
            "{label:?}: 이유를 안 말한다: {output}"
        );
    }
}

/// 앞 관문의 오류가 뒤 관문의 사유를 흉내내지 못한다.
///
/// ★★ 2026-09-10 독립 검수가 찾은 함정의 회귀 테스트다.
///
/// 인자 검사 오류는 사용자 입력을 메시지에 넣는다. 그래서 뒤 관문의 사유
/// 문구를 입력에 심으면, **인자 검사에서 죽으면서도** 그 문구가 출력에
/// 나타난다. 사유를 `output.contains()` 로 보던 시절에는 그것만으로 테스트가
/// 통과했다 — 재려던 관문은 한 번도 안 돌았는데.
///
/// ★ 이 테스트를 만들면서 두 번 고쳤다. 그 과정이 교훈이다:
///   1차: 사유 문구만 심었다. **뮤테이션이 안 잡혔다** — 단언이 코드를
///        보는데 사유 문구를 심었으니, 헬퍼가 `contains` 든 `starts_with`
///        든 결과가 같았다. "줄 시작으로 본다" 를 아무것도 증명 못 했다.
///   2차: 코드를 축 이름에 심었다. **전제가 깨졌다** — 축 파서가 입력을
///        소문자로 바꿔서 `TICK_REFUSED:` 가 `tick_refused:` 가 됐다.
///        (뜻밖의 방어다. 축 경로로는 코드를 못 심는다.)
///   3차: **입력을 그대로 출력하는 자리**를 찾았다 — 알 수 없는 인자
///        오류다. 거기로 심는다.
#[test]
fn an_argument_error_cannot_impersonate_a_later_gate() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);

    // ── 갈래 1 — 사유 문구를 축 이름에 심는다 ────────────────
    //    검수가 지적한 바로 그 입력이다.
    for planted in [
        "already reserved",
        "너무 오래 있었다",
        "작아야 한다",
        "영속이 아니다",
    ] {
        let axes = format!("{planted},gpu_count,cpu,ram,workspace");
        let (ok, output) = tick(&keyring, &db, &["--best-fit-axes", &axes]);

        assert!(!ok, "잘못된 축을 받아들였다: {output}");
        assert!(
            output.contains(planted),
            "심은 문구가 출력에 없다 — 이 테스트의 전제가 깨졌다: {output}"
        );
        assert!(
            refused_with(&output, "TICK_ARGS_REFUSED: AXES_UNKNOWN"),
            "축 파서에서 죽지 않았다: {output}"
        );
        // ★ 문구는 있어도 뒤 관문의 코드로는 인정되지 않는다.
        assert!(
            !refused_with(&output, "TICK_REFUSED:"),
            "축 오류가 실행 중 관문을 흉내냈다: {output}"
        );
        assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));
    }

    // ── 갈래 2 — 코드 자체를 심는다 ──────────────────────────
    //    ★ 이 갈래만이 `refused_with()` 가 **줄 시작**으로 본다는 것을
    //      증명한다. 알 수 없는 인자 오류는 입력을 소문자화하지 않고
    //      그대로 넣으므로, 대문자 코드가 출력에 그대로 나타난다.
    for planted in [
        "TICK_REFUSED:",
        "TICK_REFUSED: QUEUE_TOO_OLD",
        "TICK_ARGS_REFUSED: CONTROL_DB_NOT_DURABLE",
    ] {
        let (ok, output) = tick(&keyring, &db, &[planted]);

        assert!(!ok, "알 수 없는 인자를 받아들였다: {output}");
        assert!(
            output.contains(planted),
            "심은 코드가 출력에 그대로 안 나온다 — 이 갈래의 전제가 깨졌다: {output}"
        );
        assert!(
            refused_with(&output, "TICK_ARGS_REFUSED: UNKNOWN_FLAG"),
            "인자 검사에서 죽지 않았다: {output}"
        );
        // ★★ 핵심 단언. `contains` 로 보면 여기서 속는다.
        assert!(
            !refused_with(&output, planted),
            "심은 코드가 진짜 거부 코드로 인정됐다 — 사유 확인이 줄 시작을 안 본다: {output}"
        );
        assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));
    }
}

/// ★★ **루프가 큐를 실제로 비운다** — 신뢰망 P1 의 마지막 고리.
///
/// 노드 둘 · Job 둘을 두고 루프를 세 번 돌린다. 두 Job 이 STAGING 으로 가고
/// 세 번째 tick 은 빈 큐(`TICK_IDLE`)여야 한다.
///
/// ★ 이 시험이 **전에는 불가능했다** — 후보 선택이 예약을 몰라 두 번째 tick 이
///   늘 같은 노드를 골라 막혔다(그 사실을 고정하던 덫을 2026-09-22 에 뒤집었다).
#[test]
fn the_loop_drains_the_queue_and_then_reports_idle() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 2);
    queue_job(
        dir.path(),
        &keyring,
        &db,
        JOB_B,
        "1102030405060708090a0b0c0d0e0f10",
    );

    // 간격 0 — 시험에서 기다릴 이유가 없다. 정책은 운영자가 정한다.
    let (ok, output) = loop_run(&keyring, &db, "0", "3");
    assert!(ok, "루프가 실패했다: {output}");
    assert!(
        output.contains("LOOP_DONE ticks=3 staged=2 idle=1 refused=0"),
        "센 값이 기대와 다르다: {output}"
    );
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));
    assert_eq!(job_state(&db, JOB_B), Some(JobState::Staging));
}

/// 설정이 틀리면 **즉시 멈춘다** — 같은 실패를 영원히 반복하지 않는다.
#[test]
fn the_loop_stops_immediately_when_its_own_settings_are_wrong() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);
    // 갱신 시점이 만료 뒤 — tick 이 TICK_ARGS_REFUSED 로 거부하는 설정이다.
    let args: Vec<&str> = vec![
        "scheduler-loop",
        "--interval-ms",
        "0",
        "--max-ticks",
        "5",
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
        "600000",
        "--lease-max-total-duration-seconds",
        "86400",
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
    ];
    let (ok, output) = run_cli(&args);
    assert!(!ok, "틀린 설정으로 계속 돌았다: {output}");
    assert!(
        output.contains("LOOP_STOPPED: 설정이 틀렸다"),
        "멈추긴 했는데 이유가 다르다: {output}"
    );
}

/// 루프 인자를 안 주면 **기본값을 지어내지 않고 거부한다**.
#[test]
fn the_loop_refuses_to_invent_its_own_policy() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);
    for missing in ["--interval-ms", "--max-ticks"] {
        let mut args: Vec<&str> = vec!["scheduler-loop"];
        if missing != "--interval-ms" {
            args.extend_from_slice(&["--interval-ms", "0"]);
        }
        if missing != "--max-ticks" {
            args.extend_from_slice(&["--max-ticks", "1"]);
        }
        args.extend_from_slice(&[
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
            "300000",
            "--lease-max-total-duration-seconds",
            "86400",
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ]);
        let (ok, output) = run_cli(&args);
        assert!(!ok, "{missing} 없이 돌았다: {output}");
        assert!(
            output.contains("LOOP_ARGS_REFUSED") && output.contains(missing),
            "{missing} 를 짚어 말하지 않는다: {output}"
        );
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_millis() as u64
}

/// ★ 2026-09-23 (신뢰망 남은 일 C) — `--silent-after-ms` 를 주면 **한 번도 소식이 없던 노드에는 새 일을 주지 않는다.**
///
/// 풀 Coordinator 가 검증된 Hello 를 받으면 그 시각을 적는다(`record_session_seen`). 그 뒤에는 준다.
/// 대조군이 같은 시험 안에 있다 — "항상 거부" 도 "항상 줌" 도 이 시험을 통과하지 못한다.
#[test]
fn with_silence_policy_a_never_heard_node_gets_no_work_until_it_says_hello() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);

    let (ok, output) = tick(&keyring, &db, &["--silent-after-ms", "60000"]);
    assert!(!ok, "소식이 없던 노드에 일을 줬다: {output}");
    assert!(output.contains("TICK_REFUSED"), "{output}");
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Queued));

    gputeer_coordinator::node_liveness_store::CoordinatorNodeLivenessStore::open(&db)
        .expect("liveness store")
        .record_session_seen(NODE, 1, now_ms())
        .expect("Hello 관측 기록");

    let (ok, output) = tick(&keyring, &db, &["--silent-after-ms", "60000"]);
    assert!(ok, "Hello 를 한 노드에도 일을 안 줬다: {output}");
    assert!(output.contains("TICK_STAGED"), "{output}");
    assert_eq!(job_state(&db, JOB_A), Some(JobState::Staging));
}

/// ★ 결함 423 (재검수 107) — 큐 맨 앞 Job 의 Manifest 가 만료되면 큐에서 내리고 **뒤의 Job 을 배치한다.**
///   전에는 맨 앞을 고른 뒤 검증해 tick 이 매번 거부로 끝났고, 뒤의 Job 은 영영 배치되지 않았다.
#[test]
fn an_expired_manifest_at_the_head_does_not_hold_the_queue_hostage() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    let bootstrap = write_bootstrap(dir.path(), 1);
    let (ok, out) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory 실패: {out}");
    let keyring = write_keyring(dir.path());
    let expires = now_unix_ms() + 4_000;
    queue_job_expiring(
        dir.path(),
        &keyring,
        &db,
        "01JEXPIREDHEAD0000000001",
        "a1a2a3a4a5a6a7a8a9aaabacadaeaf00",
        expires,
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    queue_job(
        dir.path(),
        &keyring,
        &db,
        "01JVALIDBEHIND0000000001",
        "b1b2b3b4b5b6b7b8b9babbbcbdbebf00",
    );
    while now_unix_ms() <= expires + 100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let (ok, output) = tick(&keyring, &db, &[]);
    assert!(ok, "tick 실패: {output}");
    assert!(
        output.contains("TICK_JOB_FAILED_MANIFEST_EXPIRED 01JEXPIREDHEAD0000000001"),
        "만료된 맨 앞을 내리지 않았다: {output}"
    );
    assert!(
        output.contains("TICK_STAGED"),
        "뒤의 Job 을 배치하지 않았다: {output}"
    );
    assert_eq!(
        job_state(&db, "01JEXPIREDHEAD0000000001"),
        Some(JobState::Failed)
    );
    assert_eq!(
        job_state(&db, "01JVALIDBEHIND0000000001"),
        Some(JobState::Staging)
    );
}
