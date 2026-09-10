//! `--expect-attempt-reports` 를 **닿지 못하는 구성**과 함께 주면 시작하지
//! 않는가.
//!
//! ★ 이웃 신고 관문(`attempt` 이 아니라 `neighbor_report_lane_guard.rs`)이
//!   세운 규칙을 그대로 따른다 — **받아 놓고 안 하는 것이 가장 나쁘다.**
//!   운영자는 종료 증거가 쌓이는 줄 안다.
//!
//! ★ 관문 호출은 세 곳이다 — `run()` · `run_from_args()` ·
//!   `multi_agent::run_multi_agent()`. CLI 만 지나가면 `run()` 과 두
//!   multi-agent 진입점이 가려진다.

use gputeer_coordinator::{parse_config_from_args, CoordinatorConfig};

fn args(extra: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = vec![
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        &"11".repeat(32),
        "--peer-pubkey",
        "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29",
        "--coordinator-device-id",
        "01JCOORDREPORTGUARD000001",
        "--agent-device-id",
        "01JAGENTREPORTGUARD000001",
        "--grant-id",
        "01JGRANTREPORTGUARD000001",
        "--attempt-id",
        "01JATTEMPTREPORTGUARD0001",
        "--lease-id",
        "01JLEASEREPORTGUARD000001",
        "--job-id",
        "01JJOBREPORTGUARD0000001",
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

/// control DB 가 없으면 시작하지 않는다.
///
/// ★ 이 관문이 없으면 세션은 정상적으로 돌다가 **보고를 받은 순간**
///   저장소가 `None` 이라 실패한다. 그때는 이미 Grant 를 발급하고 남의
///   기계에서 작업을 돌린 뒤다.
#[test]
fn reports_without_a_control_db_are_refused_before_bind() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("포트 점유");
    let addr = occupied.local_addr().expect("주소").to_string();
    let mut argv = args(&["--expect-attempt-reports", "1"]);
    let idx = argv.iter().position(|a| a == "--listen").expect("--listen");
    argv[idx + 1] = addr;

    let error = gputeer_coordinator::run(parse_config_from_args(&argv).expect("설정 파싱"))
        .expect_err("거부돼야 한다");
    assert!(
        error.contains("--grant-from-control-db"),
        "거부 사유가 control DB 부재라고 말해야 한다: {error}"
    );
    assert!(
        !error.contains("bind 실패"),
        "관문보다 bind 가 먼저 일어났다: {error}"
    );
}

/// multi-agent lane 은 수신 구간 자체가 없다.
#[test]
fn the_multi_agent_lane_is_refused_from_both_entry_points() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    let db = db.to_str().expect("경로").to_string();

    let error = gputeer_coordinator::run(config(&[
        "--expect-attempt-reports",
        "1",
        "--grant-from-control-db",
        &db,
        // ★ 저장된 예약 lane 은 식별자를 요구한다(결함 ⑯ 확장) — args() 와 같은 값.
        "--stored-grant-job-id",
        "01JJOBREPORTGUARD0000001",
        "--stored-grant-attempt-id",
        "01JATTEMPTREPORTGUARD0001",
        "--stored-grant-lease-id",
        "01JLEASEREPORTGUARD000001",
        "--multi-agent",
        "true",
    ]))
    .expect_err("거부돼야 한다");
    assert!(error.contains("multi-agent"), "실제 오류: {error}");
    assert!(
        error.contains("AttemptReport"),
        "어느 옵션 때문인지 말해야 한다: {error}"
    );

    // ★ 플래그가 꺼져 있어도 그 **진입점**은 거부한다 — 라이브러리
    //   호출자가 `run_multi_agent()` 를 직접 부르는 우회 반례.
    let error = gputeer_coordinator::multi_agent::run_multi_agent(config(&[
        "--expect-attempt-reports",
        "1",
        "--grant-from-control-db",
        &db,
        // ★ 저장된 예약 lane 은 식별자를 요구한다(결함 ⑯ 확장) — args() 와 같은 값.
        "--stored-grant-job-id",
        "01JJOBREPORTGUARD0000001",
        "--stored-grant-attempt-id",
        "01JATTEMPTREPORTGUARD0001",
        "--stored-grant-lease-id",
        "01JLEASEREPORTGUARD000001",
    ]))
    .expect_err("거부돼야 한다");
    assert!(error.contains("multi-agent"), "실제 오류: {error}");
}

/// resume 경로는 수신 구간보다 먼저 반환한다.
#[test]
fn the_resume_lane_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    let db = db.to_str().expect("경로").to_string();

    let error = gputeer_coordinator::run(config(&[
        "--expect-attempt-reports",
        "1",
        "--grant-from-control-db",
        &db,
        // ★ 저장된 예약 lane 은 식별자를 요구한다(결함 ⑯ 확장) — args() 와 같은 값.
        "--stored-grant-job-id",
        "01JJOBREPORTGUARD0000001",
        "--stored-grant-attempt-id",
        "01JATTEMPTREPORTGUARD0001",
        "--stored-grant-lease-id",
        "01JLEASEREPORTGUARD000001",
        "--resume-protocol",
        "true",
    ]))
    .expect_err("거부돼야 한다");
    assert!(error.contains("resume"), "실제 오류: {error}");
    assert!(
        error.contains("AttemptReport"),
        "어느 옵션 때문인지 말해야 한다: {error}"
    );
}

/// ACK 직후 세션을 끝낼 수 있는 플래그도 거부한다.
#[test]
fn flags_that_may_end_the_session_after_ack_are_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    let db = db.to_str().expect("경로").to_string();

    for flag in [
        "--send-grant-twice",
        "--disconnect-after-ack",
        "--drop-connection-after-ack-once",
    ] {
        let error = gputeer_coordinator::run(config(&[
            "--expect-attempt-reports",
            "1",
            "--grant-from-control-db",
            &db,
            // ★ 저장된 예약 lane 은 식별자를 요구한다(결함 ⑯ 확장) — args() 와 같은 값.
            "--stored-grant-job-id",
            "01JJOBREPORTGUARD0000001",
            "--stored-grant-attempt-id",
            "01JATTEMPTREPORTGUARD0001",
            "--stored-grant-lease-id",
            "01JLEASEREPORTGUARD000001",
            flag,
            "true",
        ]))
        .expect_err("거부돼야 한다");
        assert!(error.contains(flag), "{flag}: 실제 오류: {error}");
    }
}

/// ★ 방어가 과하지 않은지 대조한다. 이 대조가 없으면 관문을 "항상 거부"
///   로 바꿔도 위 테스트가 전부 통과한다.
#[test]
fn the_guard_stays_out_of_a_session_that_expects_no_reports() {
    let cfg = config(&["--multi-agent", "true", "--accept-timeout-ms", "200"]);
    assert_eq!(cfg.expect_attempt_reports, 0);
    let error = gputeer_coordinator::run(cfg)
        .expect_err("이 lane 은 신원이 하나라 다른 이유로 거부된다");
    assert!(
        !error.contains("AttemptReport"),
        "보고를 기대하지 않는데 보고 관문이 걸렸다: {error}"
    );
    assert!(
        error.contains("MULTI_AGENT_REFUSED"),
        "다른 이유로 거부된 것이 맞는지 확인한다: {error}"
    );
}
