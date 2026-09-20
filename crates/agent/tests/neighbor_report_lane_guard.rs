//! 라이브러리 진입점을 **직접** 불렀을 때도 관문이 걸리는가(Agent 쪽).
//!
//! ★ 독립 검수 7라운드가 짚은 공백이다.
//!
//!   ★ 9라운드 정정 — Agent 의 `run_from_args()` 에는 관문이 없고 설정을
//!     `run()` 에 넘길 뿐이다. 그래서 **`run()` 의 관문은 selftest 도 실제로
//!     실행한다** — 전에 여기 "한 번도 실행되지 않는다" 고 쓴 것은 과장이었다
//!     (그 서술은 관문을 따로 가진 Coordinator 쪽 이야기다).
//!
//!   실제로 아무도 안 재던 것은 `multi_agent::run_multi_agent_session()` 의
//!   관문이다 — `run()` 의 관문이 먼저 걸려 지운 채로도 통과했다.
//!
//! 그리고 **연결 전에 막혔는지**는 오류 문구로 알 수 없다 — 여기서는
//! 테스트가 자기 listener 를 소유하고 `accept()` 로 **연결 시도 자체를
//! 관측**한다(10라운드 정정 — 전에는 "거부되는 주소" 를 가정했다).

use gputeer_agent::{parse_config_from_args, AgentConfig};

fn args(extra: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = vec![
        // `--connect` 값은 `run_against_a_listener_we_own()` 이 덮어쓴다.
        "--connect",
        "127.0.0.1:0",
        "--own-seed",
        &"22".repeat(32),
        "--peer-pubkey",
        &"3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29",
        "--coordinator-device-id",
        "01JCOORDLANEGUARD00000001",
        "--agent-device-id",
        "01JAGENTLANEGUARD00000001",
        "--disable-reconnect",
        "true",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    out.extend(extra.iter().map(|s| s.to_string()));
    out
}

/// 테스트가 **자기 listener 를 소유하고** 그 주소를 Agent 에게 준다.
///
/// ★ 10라운드 지적 — 전에는 `127.0.0.1:1` 을 주고 "연결이 거부되는 주소"
///   라고 **가정**했다. 그 포트를 점유하지도, 닫혀 있음을 확인하지도 않았다
///   — 환경에 따라 무엇이 일어나는지가 달라진다.
///
/// 이제 순서를 가정이 아니라 **관측**으로 판정한다. Agent 가 끝난 뒤
/// `accept()` 가 `WouldBlock` 이면 **연결 시도 자체가 없었다** — 관문이
/// 연결보다 먼저다(시나리오 96 과 같은 기법).
fn run_against_a_listener_we_own(extra: &[&str]) -> (String, bool) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
    let addr = listener.local_addr().expect("주소").to_string();
    listener.set_nonblocking(true).expect("nonblocking");

    let mut argv = args(extra);
    let idx = argv
        .iter()
        .position(|a| a == "--connect")
        .expect("--connect");
    argv[idx + 1] = addr;

    let error = gputeer_agent::run(parse_config_from_args(&argv).expect("설정 파싱"))
        .expect_err("이 구성들은 전부 실패로 끝난다");
    // ★ `WouldBlock` 이 아닌 것을 전부 "연결됨" 으로 세지 않는다(독립 검수
    //   11라운드) — 다른 accept 오류는 연결 성공이 아니다. 셋을 구분한다.
    let connected = match listener.accept() {
        Ok(_) => true,
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(e) => panic!("accept 가 연결도 WouldBlock 도 아닌 오류를 냈다: {e}"),
    };
    (error, connected)
}

const TARGET: &str = "01JNODETARGETGUARD000001";

fn config(extra: &[&str]) -> AgentConfig {
    parse_config_from_args(&args(extra)).expect("설정 파싱")
}

/// ★ `run()` 을 직접 불러도 구현하지 않은 lane 조합은 거부된다.
///
/// multi-agent lane 은 `Hello -> Grant -> ACK` 을 하고, resume lane 은
/// `AgentSessionHello`/`ResumeLeaseRequest` 를 보내고 `ResumeLeaseResult` 를
/// 받는다 — **둘 다 이웃 신고 송신 루프가 없다.**
///
/// ★ 연결 관측으로 순서까지 함께 잰다(10라운드 지적).
#[test]
fn run_refuses_unsupported_lane_combinations_before_connecting() {
    for (label, lane_flag, expected) in [
        ("multi-agent", "--multi-agent", "multi-agent"),
        ("resume", "--resume-protocol", "resume"),
    ] {
        let (error, connected) = run_against_a_listener_we_own(&[
            "--neighbor-report-rounds",
            "1",
            "--neighbor-report-target",
            TARGET,
            lane_flag,
            "true",
        ]);
        assert!(
            error.contains("NEIGHBOR_REPORT_REFUSED"),
            "{label}: 실제 오류: {error}"
        );
        assert!(error.contains(expected), "{label}: 실제 오류: {error}");
        assert!(!connected, "{label}: 관문보다 연결이 먼저 일어났다");
    }
}

/// ★ 대상 없음·공백도 **연결 전에** 걸린다.
#[test]
fn run_refuses_a_missing_or_blank_target_before_connecting() {
    for (label, extra) in [
        ("대상 없음", vec!["--neighbor-report-rounds", "1"]),
        (
            "대상이 공백",
            vec![
                "--neighbor-report-rounds",
                "1",
                "--neighbor-report-target",
                "   ",
            ],
        ),
    ] {
        let (error, connected) = run_against_a_listener_we_own(&extra);
        assert!(
            error.contains("NEIGHBOR_REPORT_REFUSED"),
            "{label}: 실제 오류: {error}"
        );
        assert!(!connected, "{label}: 관문보다 연결이 먼저 일어났다");
    }
}

/// 조합이 아니면 관문이 막지 않는다 — 방어가 과하지 않은지 대조한다.
///
/// ★ 이 대조가 **연결이 실제로 일어났음**까지 확인한다. 없으면 "항상 거부" 로
///   바꿔도 위 테스트들이 통과한다.
#[test]
fn the_report_guard_stays_out_of_a_lane_that_wants_no_reports() {
    let (error, connected) = run_against_a_listener_we_own(&["--multi-agent", "true"]);
    assert!(
        !error.contains("NEIGHBOR_REPORT_REFUSED"),
        "신고를 보내지 않는데 신고 관문이 걸렸다: {error}"
    );
    assert!(
        connected,
        "관문이 아닌데도 연결조차 하지 않았다 — 이 대조가 무의미해진다: {error}"
    );
}

/// ★ `multi_agent::run_multi_agent_session()` **자체**도 관문을 지난다 —
///   `multi_agent` 플래그가 **꺼져 있어도**.
///
/// ★ 독립 검수 11라운드가 찾은 우회다. 관문이 `config.multi_agent` 를 읽어
///   판정했는데 **이 함수가 곧 그 lane 이다** — 플래그를 끄고 부르면 관문이
///   "순차 lane 이구나" 하고 통과시켰다. 관문이 자기가 어디 있는지를 남에게
///   물어본 셈이다.
///
/// ★ 전에는 이 테스트가 `--multi-agent true` 를 같이 줘서 반례를 놓쳤다.
#[test]
fn the_multi_agent_entry_point_refuses_reports_even_with_the_flag_off() {
    for (label, extra) in [
        (
            "플래그 켜짐",
            vec![
                "--neighbor-report-rounds",
                "1",
                "--neighbor-report-target",
                TARGET,
                "--multi-agent",
                "true",
            ],
        ),
        (
            "플래그 꺼짐 — 우회 반례",
            vec![
                "--neighbor-report-rounds",
                "1",
                "--neighbor-report-target",
                TARGET,
            ],
        ),
        (
            "플래그 꺼짐 + 대상도 없음",
            vec!["--neighbor-report-rounds", "1"],
        ),
    ] {
        let cfg = config(&extra);
        let error =
            gputeer_agent::multi_agent::run_multi_agent_session(&cfg).expect_err("거부돼야 한다");
        assert!(
            error.contains("NEIGHBOR_REPORT_REFUSED"),
            "{label}: 실제 오류: {error}"
        );
        assert!(error.contains("multi-agent"), "{label}: 실제 오류: {error}");
    }
}
