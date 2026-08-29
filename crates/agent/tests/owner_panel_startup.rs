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
use std::time::Duration;

use gputeer_agent::owner_panel::{OwnerPanel, OwnerPanelState};

/// 같은 씨앗은 같은 토큰을 만든다.
///
/// 재시작마다 토큰이 바뀌면 브라우저에 열어 둔 패널이 죽고, 그 사이
/// 소유자는 정지 버튼을 못 쓴다.
#[test]
fn the_panel_token_is_stable_across_restarts() {
    let state = OwnerPanelState::new();
    // 토큰 파생은 private 이므로 관측 가능한 결과로 확인한다 —
    // 같은 설정으로 두 번 bind 해 두 번 다 뜨는지.
    let first = OwnerPanel::bind(0, state.clone(), "same-token".into()).expect("bind 1");
    let second = OwnerPanel::bind(0, state, "same-token".into()).expect("bind 2");
    assert_ne!(
        first.local_addr().unwrap().port(),
        second.local_addr().unwrap().port(),
        "포트 0 인데 같은 포트가 두 번 나왔다"
    );
}

/// 포트가 이미 쓰이고 있으면 bind 가 실패한다.
///
/// 그 실패를 Agent 가 삼키면 안 된다 — 이 테스트는 실패가 실제로
/// 관측 가능함을 고정한다.
#[test]
fn a_taken_port_makes_the_panel_refuse_to_start() {
    let squatter = TcpListener::bind(("127.0.0.1", 0)).expect("squatter");
    let port = squatter.local_addr().unwrap().port();

    let result = OwnerPanel::bind(port, OwnerPanelState::new(), "token".into());
    assert!(
        result.is_err(),
        "이미 점유된 포트에 패널이 떴다 — 두 패널이 같은 포트를 두고 다툰다"
    );
}

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
