//! ★ 결함 ㉟ · ㊲ — `--expect-heartbeats` 를 heartbeat 수신에 **닿지 못하는 구성**과 함께 주면
//! 시작하지 않는가. 종료 보고 관문(`attempt_report_lane_guard.rs`)과 같은 모양이다.
//!
//! ★ 관문 호출은 세 곳이다 — `run()` · `run_from_args()` · `multi_agent::run_multi_agent()`.
//!   여기서는 `run()` 과 `run_multi_agent()` 를 직접 부른다. `run_from_args()` 호출부는
//!   이 파일이 재지 않는다.
//!
//! ★ 구현 검수 41 — 처음엔 빈 포트로 돌려 관문이 **bind 전**인지 판별하지 못했다. 이제 점유된
//!   주소를 준다 — 관문이 bind 보다 늦으면 bind 오류가 먼저 나온다.

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
        "01JCOORDHEARTBEATGUARD001",
        "--agent-device-id",
        "01JAGENTHEARTBEATGUARD001",
        "--grant-id",
        "01JGRANTHEARTBEATGUARD001",
        "--attempt-id",
        "01JATTEMPTHEARTBEATGUARD1",
        "--lease-id",
        "01JLEASEHEARTBEATGUARD001",
        "--job-id",
        "01JJOBHEARTBEATGUARD00001",
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

/// 점유된 주소에서 설정을 만든다. 리스너를 돌려줘 테스트 동안 점유를 유지한다.
fn config_on_occupied_port(extra: &[&str]) -> (std::net::TcpListener, CoordinatorConfig) {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("포트 점유");
    let addr = occupied.local_addr().expect("주소").to_string();
    let mut argv = args(extra);
    let idx = argv.iter().position(|a| a == "--listen").expect("--listen");
    argv[idx + 1] = addr;
    (occupied, parse_config_from_args(&argv).expect("설정 파싱"))
}

fn assert_refused_before_bind(error: &str, must_contain: &[&str]) {
    for word in must_contain {
        assert!(error.contains(word), "{word} 가 없다 — 실제 오류: {error}");
    }
    assert!(
        !error.contains("bind 실패"),
        "관문보다 bind 가 먼저 일어났다: {error}"
    );
}

/// Resume 은 heartbeat 수신 구간보다 먼저 반환한다 — `run()` 이 bind 전에 거부한다.
#[test]
fn heartbeats_with_the_resume_lane_are_refused_before_bind() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let lease_db = dir.path().join("lease.sqlite3");
    let (_occupied, cfg) = config_on_occupied_port(&[
        "--expect-heartbeats",
        "1",
        "--resume-protocol",
        "true",
        "--lease-db",
        lease_db.to_str().expect("경로"),
        "--accept-timeout-ms",
        "500",
    ]);
    let error = gputeer_coordinator::run(cfg).expect_err("거부돼야 한다");
    assert_refused_before_bind(&error, &["resume", "heartbeat"]);
}

/// multi-agent lane 에는 heartbeat 수신 구간이 없다 — 두 진입점 모두 거부한다.
#[test]
fn heartbeats_on_the_multi_agent_lane_are_refused_from_both_entry_points() {
    let (_occupied, cfg) = config_on_occupied_port(&[
        "--expect-heartbeats",
        "1",
        "--multi-agent",
        "true",
        "--accept-timeout-ms",
        "500",
    ]);
    let error = gputeer_coordinator::run(cfg).expect_err("거부돼야 한다");
    assert_refused_before_bind(&error, &["multi-agent", "heartbeat"]);

    // ★ 라이브러리 호출자가 `run()` 을 지나쳐 `run_multi_agent()` 를 바로 부르는 우회.
    let error =
        gputeer_coordinator::multi_agent::run_multi_agent(config(&["--expect-heartbeats", "1"]))
            .expect_err("거부돼야 한다");
    assert!(
        error.contains("multi-agent") && error.contains("heartbeat"),
        "실제 오류: {error}"
    );
}

/// ★ 결함 ㊲ — CLI 파서(NEEDS_EXPECT)를 지나쳐 `run(config)` 에 liveness 경로만 넘겨도 bind 전에 거부한다.
#[test]
fn a_liveness_path_without_expected_heartbeats_is_refused_by_run_before_bind() {
    let (_occupied, mut cfg) = config_on_occupied_port(&["--accept-timeout-ms", "500"]);
    cfg.liveness_db_path = Some("live.sqlite3".into());
    assert_eq!(cfg.expect_heartbeats, 0);
    let error = gputeer_coordinator::run(cfg).expect_err("거부돼야 한다");
    assert_refused_before_bind(&error, &["--liveness-db"]);
}
