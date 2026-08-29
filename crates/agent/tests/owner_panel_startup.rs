//! Owner Panel 이 **Agent 기동 경로에서 실제로 뜨는가**.
//!
//! `owner_panel_http.rs` 는 패널을 직접 만들어 두드린다. 여기서 확인하는
//! 것은 그 위 — `AgentConfig` 에 포트를 주면 정말로 그 포트가 열리고,
//! 못 열면 **실행 자체가 거부되는가** 다.
//!
//! ★ 후자가 더 중요하다. 패널이 조용히 안 뜨면 소유자는 멈출 수 있다고
//!   믿는데 실제로는 없는 상태가 된다(`CLAUDE.md` §0.4 — 강제할 수
//!   없는 것을 보장으로 선언하지 않는다).

#![cfg(windows)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;

use gputeer_agent::owner_panel::{OwnerPanel, OwnerPanelState};

/// 패널이 뜬 뒤 실제 브라우저처럼 두 번 연달아 요청해도 되는가.
///
/// `serve_one()` 이 연결마다 제대로 닫히지 않으면 두 번째 요청이 막힌다.
#[test]
fn consecutive_requests_are_served() {
    let panel = OwnerPanel::bind(0, OwnerPanelState::new(), "token".into()).expect("bind");
    let port = panel.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let _ = panel.serve_one();
        }
    });

    for round in 0..2 {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "{round}번째 요청이 실패했다: {response}"
        );
    }
    handle.join().expect("panel thread");
}

/// 화면이 실제로 §0.1 의 네 가지를 말하는가.
///
/// ★ HTML 이 비어 있어도 위 테스트들은 전부 통과한다. 소유자가 볼
///   내용이 실제로 들어 있는지는 따로 봐야 한다.
#[test]
fn the_page_actually_shows_what_the_owner_needs() {
    let panel = OwnerPanel::bind(0, OwnerPanelState::new(), "token".into()).expect("bind");
    let port = panel.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let _ = panel.serve_one();
    });

    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .expect("write");
    let mut page = String::new();
    stream.read_to_string(&mut page).expect("read");
    handle.join().expect("panel thread");

    // §0.1: 누가 · 어느 Job · 언제부터 · 얼마나 잃는지
    for needed in ["누가", "어느 작업", "언제부터", "잃는 것"] {
        assert!(
            page.contains(needed),
            "화면에 '{needed}' 열이 없다 — 소유자가 무엇을 멈추는지 모른 채 누른다"
        );
    }
    // 정지 버튼과 손실 확인이 있는가
    assert!(page.contains("지금 비우기"), "정지 버튼이 없다");
    assert!(
        page.contains("confirm("),
        "손실 범위를 누르기 전에 확인하지 않는다 — §0.1 위반"
    );
}

/// ★ 실행을 켜면 패널이 **자동으로** 뜬다 — 포트를 안 줘도.
///
/// 전에는 `--owner-panel-port` 를 생략하면 패널 없이 실행됐다. 그러면
/// 남의 코드가 도는데 소유자는 볼 수도 멈출 수도 없다(2026-08-29
/// 독립 검수 지적). 여기서 확인하는 것은 그 구멍이 닫혔는가다.
///
/// Coordinator 가 없으므로 연결은 실패한다 — 그건 상관없다. 연결
/// **전에** 패널이 떴다는 사실이 표준 출력에 남는지를 본다.
#[test]
fn enabling_execution_starts_the_panel_even_without_a_port() {
    let output = run_agent_stub(&["--i-understand-this-executes-untrusted-code", "true"]);
    assert!(
        output.contains("OWNER_PANEL listening=http://127.0.0.1:"),
        "실행을 켰는데 패널이 안 떴다 — 소유자가 멈출 수단 없이 남의 코드가 돈다:\n{output}"
    );
}

/// 실행을 안 켜면 패널도 안 뜬다.
///
/// 멈출 대상이 없는데 포트를 여는 것은 공격면만 늘린다.
#[test]
fn without_execution_no_panel_is_opened() {
    let output = run_agent_stub(&[]);
    assert!(
        !output.contains("OWNER_PANEL listening"),
        "실행이 꺼져 있는데 패널 포트가 열렸다:\n{output}"
    );
}

/// 명시한 포트가 이미 점유돼 있으면 **실행 자체가 거부**되는가.
///
/// ★ 이전 판의 같은 이름 테스트는 `OwnerPanel::bind` 만 불러봤다 —
///   Agent 의 fail-closed 기동 경로는 검사하지 않았다(독립 검수 지적).
///   여기서는 실제 Agent 를 띄운다.
#[test]
fn a_taken_port_makes_the_agent_refuse_to_start() {
    let squatter = TcpListener::bind(("127.0.0.1", 0)).expect("squatter");
    let port = squatter.local_addr().unwrap().port();

    let output = run_agent_stub(&["--owner-panel-port", &port.to_string()]);
    assert!(
        output.contains("OWNER_PANEL_REFUSED"),
        "점유된 포트인데 Agent 가 조용히 계속 갔다 — 소유자는 패널이 있다고 믿는다:\n{output}"
    );
}

/// 같은 씨앗은 같은 토큰을 만든다.
///
/// ★ 이전 판은 같은 문자열 `"same-token"` 을 두 서버에 넣을 뿐이라
///   파생 함수를 전혀 부르지 않았다(독립 검수 지적). 여기서는 Agent 를
///   **두 번** 띄워 실제 파생 결과를 비교한다.
///
/// 재시작마다 토큰이 바뀌면 브라우저에 열어 둔 패널이 죽고, 그 사이
/// 소유자는 정지 버튼을 못 쓴다.
#[test]
fn the_panel_token_is_stable_across_restarts() {
    let first = token_from_a_fresh_agent();
    let second = token_from_a_fresh_agent();
    assert_eq!(
        first, second,
        "같은 씨앗인데 재시작 후 토큰이 바뀌었다 — 열어 둔 패널의 정지 버튼이 죽는다"
    );
    assert_eq!(first.len(), 64, "토큰이 BLAKE3 hex 가 아니다: {first}");
    // 씨앗 자체가 그대로 나오면 안 된다.
    assert_ne!(
        first, SEED_HEX,
        "토큰이 씨앗과 같다 — 화면에 서명키가 그대로 노출된다"
    );
}

/// 다른 씨앗이면 다른 토큰이 나온다.
#[test]
fn a_different_seed_yields_a_different_token() {
    let first = token_from_a_fresh_agent();
    let other = token_from_agent_with_seed(OTHER_SEED_HEX);
    assert_ne!(
        first, other,
        "씨앗이 다른데 토큰이 같다 — 파생이 씨앗을 안 쓴다"
    );
}

const SEED_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const OTHER_SEED_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn token_from_a_fresh_agent() -> String {
    token_from_agent_with_seed(SEED_HEX)
}

/// Agent 를 띄워 패널이 준 토큰을 읽어 온다.
fn token_from_agent_with_seed(seed_hex: &str) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("port pick");
    let port = listener.local_addr().unwrap().port();
    drop(listener); // 포트를 비워 Agent 가 그 자리에 붙게 한다

    let mut child = agent_stub_command(seed_hex, &["--owner-panel-port", &port.to_string()])
        .spawn()
        .expect("agent-stub");

    // 패널이 뜰 때까지 기다렸다가 토큰을 읽는다.
    let mut token = String::new();
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            continue;
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        if stream
            .write_all(b"GET /api/workloads HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .is_err()
        {
            continue;
        }
        let mut body = String::new();
        if stream.read_to_string(&mut body).is_err() {
            continue;
        }
        if let Some(rest) = body.split("\"token\":\"").nth(1) {
            token = rest.split('"').next().unwrap_or("").to_string();
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(!token.is_empty(), "패널에서 토큰을 못 받았다");
    token
}

fn agent_stub_command(seed_hex: &str, extra: &[&str]) -> Command {
    let mut command = Command::new(agent_binary());
    command
        .arg("agent-stub")
        // 붙을 Coordinator 가 없다 — 연결은 실패한다. 패널은 그 전에 뜬다.
        .args(["--connect", "127.0.0.1:1"])
        .args(["--own-seed", seed_hex])
        .args([
            "--peer-pubkey",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ])
        .args(["--coordinator-device-id", "01JCOORD0000000000000000001"])
        .args(["--agent-device-id", "01JAGENT00000000000000000001"])
        .args(extra);
    command
}

/// Agent 를 끝까지 돌려 표준 출력·오류를 합쳐 돌려준다.
fn run_agent_stub(extra: &[&str]) -> String {
    let output = agent_stub_command(SEED_HEX, extra)
        .output()
        .expect("agent-stub");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// 이 테스트가 부를 `gputeer` 실행 파일.
fn agent_binary() -> std::path::PathBuf {
    // 테스트 바이너리는 target/<profile>/deps/ 에 있다. 두 단계 위가
    // 프로파일 디렉터리다.
    let mut path = std::env::current_exe().expect("current_exe");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("gputeer.exe")
}
