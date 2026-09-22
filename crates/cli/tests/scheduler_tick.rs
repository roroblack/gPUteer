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

/// ★★ **큐에 너무 오래 있었으면 예약하지 않는다.**
///
/// Lease 발급 시각을 `queued_at` 으로 쓰므로, TTL 이 짧으면 지금
/// 예약해도 **이미 만료된 Lease** 를 주게 된다. 그걸 주면 Agent 는
/// 받자마자 거부한다 — 조용히 만들지 않고 거부 이유를 말한다.
#[test]
fn a_job_that_waited_longer_than_the_lease_ttl_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let (keyring, db) = prepared(dir.path(), 1);

    // ★★ **처음 쓴 이 테스트는 엉뚱한 관문을 재고 있었다.**
    //
    //   `--lease-ttl-ms 1` 만 줬는데 `--lease-renew-after-ms` 는 기본
    //   300000 이 남아 있었다. 갱신 오프셋 검사가 **먼저** 있으므로
    //   (`scheduler_tick.rs:85` 가 `:128` 보다 앞이다) 거기서 걸렸고,
    //   내 단언이 "작아야 한다" 도 받아 줘서 **통과했다.**
    //   내 뮤테이션 T3(큐 나이 검사를 통째로 지움)이 안 잡히는 것을
    //   보고서야 알았다 — 재려던 것을 하나도 안 재고 있었다.
    //
    //   이제 갱신 검사를 **통과하는** 값을 준다(1 < 2).
    let (ok, output) = tick(
        &keyring,
        &db,
        &["--lease-ttl-ms", "2", "--lease-renew-after-ms", "1"],
    );
    assert!(!ok, "만료될 Lease 로 예약했다: {output}");
    assert!(
        refused_with(&output, "TICK_REFUSED: QUEUE_TOO_OLD"),
        "큐 나이가 아니라 다른 관문에 걸렸다: {output}"
    );
    assert_eq!(
        job_state(&db, JOB_A),
        Some(JobState::Queued),
        "거부했는데 상태가 바뀌었다"
    );

    // 대조 — 넉넉한 TTL 이면 예약된다. 없으면 "항상 거부" 로도 통과한다.
    //
    // ★★ **이 대조군 자체가 부실했다** (2026-09-07 독립 검수 지적).
    //   `assert!(ok)` 만 봤다. 검수가 반례를 만들어 보였다 — 구현이
    //   예약 경로로 안 들어가고 `TICK_IDLE` 을 조기 반환하도록 바뀌어도
    //   **종료 코드는 0 이라 이 단언이 통과한다.** 그러면 "항상 거부"
    //   회귀는 잡아도 **"아무것도 예약 안 함" 회귀는 못 잡는다.**
    //
    //   대조군을 두는 목적이 바로 그 두 번째인데, 그걸 못 재고 있었다.
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

/// ★★ **Lease 발급 시각이 정말 `queued_at` 인가 — 저장된 값으로 잰다.**
///
/// 이 모듈의 무게중심인데 **아무 테스트도 안 재고 있었다** — 멱등
/// 테스트는 attempt/lease **식별자**만 비교하고, 식별자는 시각에서
/// 유도되지 않으므로 발급 시각을 시계로 바꿔도 통과했다(뮤테이션 T2).
///
/// 이제 저장소를 열어 실제 행을 본다.
#[test]
fn the_stored_lease_is_issued_at_the_moment_the_job_entered_the_queue() {
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

    let (ok, output) = tick(&keyring, &db, &[]);
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

    assert_eq!(
        stored.issued_at_unix_ms, queued_at,
        "발급 시각이 큐 진입 시각이 아니다 — 시계를 읽었다"
    );
    assert_eq!(
        stored.expires_at_unix_ms,
        queued_at + 600_000,
        "만료가 queued_at + TTL 이 아니다"
    );
    assert_eq!(
        stored.renew_after_unix_ms,
        queued_at + 300_000,
        "갱신 시점이 queued_at + 오프셋이 아니다"
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
