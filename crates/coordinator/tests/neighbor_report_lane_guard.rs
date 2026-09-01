//! 라이브러리 진입점을 **직접** 불렀을 때도 관문이 걸리는가.
//!
//! ★ 독립 검수 7라운드가 짚은 공백이다. 관문 호출은 두 crate 를 합쳐 **다섯**
//!   곳에 있다 — Coordinator 의 `run()`·`run_from_args()`·`run_multi_agent()`,
//!   Agent 의 `run()`·`run_multi_agent_session()`.
//!
//!   ★ 10라운드 정정 — 전에 "네 진입점" 이라 세고 "나머지는 한 번도 실행되지
//!     않는다" 고 썼는데, 둘 다 틀렸다. **Agent 의 `run_from_args()` 에는
//!     관문이 없어** 설정을 `run()` 에 넘길 뿐이므로 selftest 도 Agent 의
//!     `run()` 관문은 실제로 실행한다.
//!
//!   CLI 로 가려지는 것은 **Coordinator 의 `run()`** (`run_from_args()` 가
//!   자기 관문을 먼저 가진다)과 **두 `multi_agent` 진입점**이다. 지운 채로도
//!   selftest 는 전부 통과한다 — 그래서 여기서 따로 잰다.
//!
//! 그래서 라이브러리 호출자처럼 설정을 만들어 진입점을 직접 부른다.

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

/// ★ `run()` 을 직접 불러도 multi-agent 조합은 거부된다.
///
/// CLI 를 거치지 않으므로 `run_from_args` 의 관문은 지나지 않는다.
#[test]
fn run_refuses_the_multi_agent_combination() {
    let error = gputeer_coordinator::run(config(&[
        "--expect-neighbor-reports",
        "1",
        "--multi-agent",
        "true",
    ]))
    .expect_err("거부돼야 한다");
    assert!(error.contains("multi-agent"), "실제 오류: {error}");
}

/// ★ resume 조합도 같다 — 그 lane 도 수신 루프가 없다.
#[test]
fn run_refuses_the_resume_combination() {
    let error = gputeer_coordinator::run(config(&[
        "--expect-neighbor-reports",
        "1",
        "--resume-protocol",
        "true",
    ]))
    .expect_err("거부돼야 한다");
    assert!(error.contains("resume"), "실제 오류: {error}");
}

/// ★ **listener 를 열기 전에** 거부하는가 — 두 lane 전부.
///
/// 이미 점유된 포트를 준다 — 관문이 먼저면 lane 오류가, 나중이면 bind 실패가
/// 난다. 문구가 다르므로 순서가 값으로 드러난다.
///
/// ★ 8라운드 지적 — 전에는 multi-agent 만 시험해서, **resume 관문만**
///   bind 뒤로 옮겨도 잡지 못했다. 이름은 두 lane 을 말하는데 재는 것은
///   하나뿐이었다.
///
/// ★ 유효한 저장소 경로를 준다 — 경로 부재 관문이 먼저 걸리면 이 테스트가
///   재려는 lane 관문이 아니라 그것을 재게 된다.
#[test]
fn the_lane_guard_runs_before_bind_for_every_lane() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("neighbor.sqlite3");
    let db = db.to_str().expect("경로").to_string();

    for (label, lane_flag, expected) in [
        ("multi-agent", "--multi-agent", "multi-agent"),
        ("resume", "--resume-protocol", "resume"),
    ] {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("포트 점유");
        let addr = occupied.local_addr().expect("주소").to_string();

        let mut argv = args(&[
            "--expect-neighbor-reports",
            "1",
            "--neighbor-report-db",
            &db,
            lane_flag,
            "true",
        ]);
        let idx = argv.iter().position(|a| a == "--listen").expect("--listen");
        argv[idx + 1] = addr;

        let error = gputeer_coordinator::run(parse_config_from_args(&argv).expect("설정 파싱"))
            .expect_err("거부돼야 한다");
        assert!(
            !error.contains("bind 실패"),
            "{label}: 관문보다 bind 가 먼저 일어났다: {error}"
        );
        assert!(error.contains(expected), "{label}: 실제 오류: {error}");
    }
}

/// 조합이 아니면 관문이 막지 않는다 — 방어가 과하지 않은지 대조한다.
///
/// ★ 이 대조가 없으면 "항상 거부" 로 바꿔도 위 세 테스트가 통과한다.
#[test]
fn the_report_guard_stays_out_of_a_lane_that_wants_no_reports() {
    // 신고를 기대하지 않으면 multi-agent 든 resume 이든 이 관문과 무관하다.
    // ★ accept 를 기다리지 않도록 짧은 타임아웃을 준다 — 이 테스트가 보려는
    //   것은 "신고 관문이 안 걸린다" 이지 lane 의 정상 동작이 아니다.
    let cfg = config(&["--multi-agent", "true", "--accept-timeout-ms", "200"]);
    assert_eq!(cfg.expect_neighbor_reports, 0);
    // 관문 판정만 따로 확인한다 — 실제 실행은 다른 이유로 실패할 수 있다.
    let error = gputeer_coordinator::run(cfg).expect_err("이 lane 은 신원이 하나라 다른 이유로 거부된다");
    assert!(
        !error.contains("이웃 신고"),
        "신고를 기대하지 않는데 신고 관문이 걸렸다: {error}"
    );
    // ★ **분기 자체도 여기서 고정한다**(8라운드 지적). 전에는 "신고 관문이
    //   안 걸린다" 만 봐서, run() 의 multi-agent 분기를 지워도 순차 lane 이
    //   accept 타임아웃으로 실패하며 그대로 통과했다 — 옮긴 분기를 아무도
    //   지키지 않았다. 이 문구는 **multi-agent lane 에 실제로 들어갔을 때만**
    //   나온다.
    assert!(
        error.contains("MULTI_AGENT_REFUSED"),
        "run() 이 multi-agent lane 으로 분기하지 않았다: {error}"
    );
}

/// ★ `multi_agent::run_multi_agent()` 도 `multi_agent` 플래그가 **꺼져 있어도**
///   신고 옵션을 거부한다 — 독립 검수 11라운드가 찾은 우회 반례.
#[test]
fn the_multi_agent_entry_point_refuses_reports_even_with_the_flag_off() {
    for (label, extra) in [
        (
            "플래그 켜짐",
            vec!["--expect-neighbor-reports", "1", "--multi-agent", "true"],
        ),
        ("플래그 꺼짐 — 우회 반례", vec!["--expect-neighbor-reports", "1"]),
    ] {
        let error = gputeer_coordinator::multi_agent::run_multi_agent(config(&extra))
            .expect_err("거부돼야 한다");
        assert!(error.contains("multi-agent"), "{label}: 실제 오류: {error}");
    }
}

/// ★ 순차 lane 안에서 **ACK 직후 세션을 끝낼 수 있는 플래그**도 거부한다 —
///   독립 검수 11라운드 지적. 받아 놓고 안 하는 것이 가장 나쁘다.
///
/// ★ 13라운드 정정 — 이름을 `ack_only_modes...` 라 지었는데 셋 중 하나는
///   기본 설정(`max_connections == 1`)에서 아예 동작하지 않아 "ACK-only
///   mode" 가 아니다. 이름을 실제 재는 것에 맞췄다.
///
/// ★ 12라운드 정정 — 셋이 전부 "항상" 끝내지는 않는다.
///   `--drop-connection-after-ack-once` 는 `max_connections > 1` 이고 첫
///   연결일 때만 실제로 끊는다. 그래도 **조건부로 닿는 구성**을 받아 주지
///   않는다 — 여기서 재는 것은 "관문이 이 조합을 거부한다" 이지 "이 플래그가
///   항상 세션을 끊는다" 가 아니다.
#[test]
fn flags_that_may_end_the_session_after_ack_are_refused() {
    for flag in [
        "--send-grant-twice",
        "--disconnect-after-ack",
        "--drop-connection-after-ack-once",
    ] {
        let error = gputeer_coordinator::run(config(&["--expect-neighbor-reports", "1", flag, "true"]))
            .expect_err("거부돼야 한다");
        assert!(error.contains(flag), "{flag}: 실제 오류: {error}");
    }
}
