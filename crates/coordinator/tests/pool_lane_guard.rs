//! ★ 결함 408 (재검수 96) — 라이브러리 호출자가 진입점을 **직접** 불러도 풀 전제를 건너뛰지 못한다.
//!
//!   전에는 풀 시작 검사가 CLI 파서에만 있었다. `run(config)` 는 `multi_agent` 로 먼저 분기해, 풀 예약으로 풀 신호(`pool_mode`) 없는
//!   Grant 를 냈다 — 수신 확인 없는 Agent 가 ACK 하고 실행했고, 그 lane 은 Job 을 RUNNING 으로 옮기지 않아 두 번 돌 수 있었다.

use gputeer_coordinator::{parse_config_from_args, CoordinatorConfig};

fn args(extra: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = vec![
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        &"11".repeat(32),
        "--peer-pubkey",
        // 유효한 Ed25519 공개키 한 개(테스트 fixture 와 같은 씨앗에서 나온 값).
        &"3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29",
        "--coordinator-device-id",
        "01JCOORDLANEGUARD00000001",
        "--agent-device-id",
        "01JAGENTLANEGUARD00000001",
        "--grant-id",
        "01JGRANTLANEGUARD00000001",
        "--attempt-id",
        "01JATTEMPTLANEGUARD000001",
        "--lease-id",
        "01JLEASELANEGUARD00000001",
        "--job-id",
        "01JJOBLANEGUARD0000000001",
        "--i-understand-legacy-mode-is-unsafe",
        "true",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    out.extend(extra.iter().map(|s| s.to_string()));
    out
}

fn config(extra: &[&str]) -> CoordinatorConfig {
    parse_config_from_args(&args(extra)).expect("설정 파싱")
}

/// `run()` 을 풀 · multi-agent 조합으로 직접 부르면 풀 시작 검사가 거부한다(연결을 받기 전에).
#[test]
fn run_refuses_a_pool_config_before_choosing_a_lane() {
    let mut pooled = config(&[]);
    pooled.pool_mode = true;
    pooled.multi_agent = true;
    let error = gputeer_coordinator::run(pooled).expect_err("거부돼야 한다");
    // ★ 풀 시작 검사(제어 DB 부터 본다)가 lane 분기보다 **먼저** 돈다 — multi-agent 진입점의 두 번째 방어(POOL_LANE_CONFLICT)가 아니라.
    assert!(
        error.contains("POOL_NEEDS_CONTROL_DB"),
        "실제 오류: {error}"
    );
}

/// multi-agent 진입점을 바로 불러도 풀 모드는 거부한다.
#[test]
fn run_multi_agent_refuses_pool_mode() {
    let mut pooled = config(&[]);
    pooled.pool_mode = true;
    pooled.multi_agent = true;
    let error =
        gputeer_coordinator::multi_agent::run_multi_agent(pooled).expect_err("거부돼야 한다");
    assert!(error.contains("POOL_LANE_CONFLICT"), "실제 오류: {error}");
}

/// ★ 결함 410 (재검수 97) — 풀 표식이 있는 제어 DB 를 `--pool-mode` 없이 쓰면 `run()` 이 시작하지 않는다.
#[test]
fn run_refuses_a_pool_marked_control_db_without_pool_mode() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    gputeer_coordinator::job_store::declare_pool_mode(&db, 1).expect("풀 표식");
    let mut unpooled = config(&["--max-connections", "1", "--accept-timeout-ms", "200"]);
    unpooled.grant_from_control_db = Some(db);
    let error = gputeer_coordinator::run(unpooled).expect_err("거부돼야 한다");
    assert!(
        error.contains("POOL_DB_WITHOUT_POOL_MODE"),
        "실제 오류: {error}"
    );
}

/// ★ 결함 412 (재검수 98) — 풀 DB 를 Lease DB 로만 열어도(예약 없는 옛 발급 경로) `--pool-mode` 없이는 시작하지 않는다.
///   `run()` 과 `run_multi_agent()` 를 각각 직접 부른다.
#[test]
fn a_pool_marked_lease_db_is_refused_on_every_entry_point() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("pool.sqlite3");
    gputeer_coordinator::job_store::declare_pool_mode(&db, 1).expect("풀 표식");
    let unpooled = || {
        let mut c = config(&["--max-connections", "1", "--accept-timeout-ms", "200"]);
        c.lease_db_path = Some(db.clone());
        c
    };
    let error = gputeer_coordinator::run(unpooled()).expect_err("run 이 거부해야 한다");
    assert!(
        error.contains("POOL_DB_WITHOUT_POOL_MODE"),
        "실제 오류: {error}"
    );
    let mut multi = unpooled();
    multi.multi_agent = true;
    let error = gputeer_coordinator::multi_agent::run_multi_agent(multi)
        .expect_err("run_multi_agent 가 거부해야 한다");
    assert!(
        error.contains("POOL_DB_WITHOUT_POOL_MODE"),
        "실제 오류: {error}"
    );
}

/// ★ 결함 414 (재검수 99) — Coordinator 가 여는 DB 경로 **어느 것이든** 풀 DB 면 `--pool-mode` 없이 시작하지 않는다. 경로마다 하나씩 잰다.
#[test]
fn every_database_path_is_checked_for_the_pool_mark() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("pool.sqlite3");
    gputeer_coordinator::job_store::declare_pool_mode(&db, 1).expect("풀 표식");
    let db_string = db.to_str().expect("경로").to_string();
    let setters: [(&str, fn(&mut CoordinatorConfig, &std::path::Path, &str)); 6] = [
        ("control", |c, p, _| {
            c.grant_from_control_db = Some(p.to_path_buf())
        }),
        ("lease", |c, p, _| c.lease_db_path = Some(p.to_path_buf())),
        ("liveness", |c, _, s| {
            c.liveness_db_path = Some(s.to_string())
        }),
        ("neighbor", |c, _, s| {
            c.neighbor_report_db_path = Some(s.to_string())
        }),
        ("replay", |c, p, _| c.replay_db = Some(p.to_path_buf())),
        ("hello-replay", |c, p, _| {
            c.hello_replay_db = Some(p.to_path_buf())
        }),
    ];
    for (label, set) in setters {
        let mut c = config(&["--max-connections", "1", "--accept-timeout-ms", "200"]);
        set(&mut c, &db, &db_string);
        let error = gputeer_coordinator::run(c).expect_err(label);
        assert!(
            error.contains("POOL_DB_WITHOUT_POOL_MODE"),
            "{label}: 실제 오류: {error}"
        );
    }
}

/// ★ 결함 415 (재검수 100) — DB 경로를 SQLite URI 로 주면(`file:…`) 존재 검사는 거짓인데 SQLite 는 그 파일을 연다. URI 는 받지 않는다.
#[test]
fn a_sqlite_uri_database_path_is_refused() {
    // 관문이 망가져도 저장소에 파일을 만들지 않게 임시 폴더 · 읽기 전용 URI 를 쓴다.
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("pool.sqlite3");
    gputeer_coordinator::job_store::declare_pool_mode(&db, 1).expect("풀 표식");
    let path = db.to_str().expect("경로").replace('\\', "/");
    for uri in [
        format!("file:{path}?mode=ro"),
        format!("FILE:{path}?mode=ro"),
    ] {
        let mut c = config(&["--max-connections", "1", "--accept-timeout-ms", "200"]);
        c.lease_db_path = Some(std::path::PathBuf::from(&uri));
        let error = gputeer_coordinator::run(c).expect_err(&uri);
        assert!(
            error.contains("SQLITE_URI_PATH"),
            "{uri}: 실제 오류: {error}"
        );
    }
}

/// ★ 결함 416 (재검수 101) — 풀 설정에서도 URI 경로를 받지 않는다(처음엔 풀이면 URI 검사를 건너뛰었다).
#[test]
fn a_sqlite_uri_database_path_is_refused_in_pool_mode_too() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let path = dir
        .path()
        .join("control.sqlite3")
        .to_str()
        .expect("경로")
        .replace('\\', "/");
    let uri = std::path::PathBuf::from(format!("file:{path}?mode=ro"));
    let mut c = config(&[]);
    c.pool_mode = true;
    c.grant_from_control_db = Some(uri.clone());
    c.lease_db_path = Some(uri.clone());
    c.liveness_db_path = Some(uri.to_string_lossy().to_string());
    let error = gputeer_coordinator::run(c).expect_err("거부돼야 한다");
    assert!(error.contains("SQLITE_URI_PATH"), "실제 오류: {error}");
}

/// ★ 결함 417 (재검수 102) — CLI 파서 경로에서도, 풀 설정의 다른 결함(제출자 keyring 없음)보다 URI 를 먼저 보고한다.
///   `run_multi_agent()` 를 풀 설정으로 바로 불러도 URI 가 먼저다.
#[test]
fn the_cli_parser_and_multi_agent_report_a_uri_before_other_pool_problems() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let path = dir
        .path()
        .join("control.sqlite3")
        .to_str()
        .expect("경로")
        .replace('\\', "/");
    let uri = format!("file:{path}?mode=ro");
    let argv: Vec<String> = [
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        &"11".repeat(32),
        "--coordinator-device-id",
        "01JCOORDLANEGUARD00000001",
        "--pool-mode",
        "true",
        "--pool-agents",
        "01JAGENTLANEGUARD00000001=3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29",
        "--grant-from-control-db",
        &uri,
        "--lease-db",
        &uri,
        "--liveness-db",
        &uri,
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let Err(error) = parse_config_from_args(&argv) else {
        panic!("거부돼야 한다");
    };
    assert!(error.contains("SQLITE_URI_PATH"), "CLI: 실제 오류: {error}");

    let mut pooled = config(&[]);
    pooled.pool_mode = true;
    pooled.lease_db_path = Some(std::path::PathBuf::from(&uri));
    let error =
        gputeer_coordinator::multi_agent::run_multi_agent(pooled).expect_err("거부돼야 한다");
    assert!(
        error.contains("SQLITE_URI_PATH"),
        "multi-agent: 실제 오류: {error}"
    );
}

/// ★ 결함 419 (재검수 103) — 풀 표식 읽기는 없는 파일을 만들지 않는다(읽기 전용 · 생성 없음). 없으면 오류다(호출자가 거부한다).
#[test]
fn reading_the_pool_mark_never_creates_a_database_file() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let missing = dir.path().join("missing.sqlite3");
    assert!(gputeer_coordinator::job_store::pool_mode_declared(&missing).is_err());
    assert!(!missing.exists(), "풀 표식을 읽다가 빈 DB 를 만들었다");
}

/// ★ 결함 420 (재검수 104) — 비정상 종료가 남긴 rollback journal(hot journal)이 있는 DB 도 풀 표식 검사가 읽는다(되감는다).
///   처음 419 조치는 읽기 전용으로 열어 되감지 못하고 실패했다 — 시작 검사가 첫 DB 접근이라 재시작마다 같은 곳에서 막혔다.
///   hot journal 은 쓰기 도중인 DB 와 journal 을 **잠금 없이** 복사해 만든다(복사본에는 잠금을 쥔 연결이 없다).
#[test]
fn a_hot_journal_left_by_a_crash_does_not_block_the_pool_mark_check() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    gputeer_coordinator::job_store::declare_pool_mode(&db, 1).expect("풀 표식");
    let writer = rusqlite::Connection::open(&db).expect("열기");
    writer
        .execute_batch(
            "PRAGMA journal_mode = DELETE; PRAGMA cache_size = 1;
             CREATE TABLE filler(x BLOB);",
        )
        .expect("준비");
    writer.execute_batch("BEGIN IMMEDIATE;").expect("쓰기 시작");
    for _ in 0..200 {
        writer
            .execute("INSERT INTO filler VALUES (zeroblob(4096))", [])
            .expect("쓰기");
    }
    let copy = dir.path().join("crashed.sqlite3");
    std::fs::copy(&db, &copy).expect("DB 복사");
    let journal = dir.path().join("control.sqlite3-journal");
    assert!(
        journal.exists(),
        "쓰기 도중인데 journal 이 없다 — 시험 전제가 깨졌다"
    );
    std::fs::copy(&journal, dir.path().join("crashed.sqlite3-journal")).expect("journal 복사");
    drop(writer);
    assert_eq!(
        gputeer_coordinator::job_store::pool_mode_declared(&copy),
        Ok(true),
        "hot journal 이 남은 DB 의 풀 표식을 읽지 못했다"
    );
}
