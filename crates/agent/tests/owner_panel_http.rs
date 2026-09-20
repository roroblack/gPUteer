//! Owner Panel 을 **실제 TCP 소켓으로** 두드린다.
//!
//! # 왜 단위 테스트로 부족한가
//!
//! `owner_panel.rs` 의 단위 테스트는 `host_is_loopback()` 같은 함수를
//! 직접 부른다. 그건 그 함수가 옳다는 것만 말해 줄 뿐, **서버가 그
//! 함수를 실제로 부르는지**는 말해 주지 않는다. 이 저장소는 이미 그
//! 함정을 여러 번 겪었다(판정만 하고 아무도 안 쓰던 `runtime-policy`).
//!
//! 여기서는 진짜 소켓을 열고 진짜 HTTP 바이트를 보낸다.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use gputeer_agent::owner_panel::{OwnerPanel, OwnerPanelState};

const TOKEN: &str = "test-token-0123456789";

/// 패널을 띄우고 요청 N 개를 받게 한다.
fn panel_on_thread(state: OwnerPanelState, requests: usize) -> (u16, std::thread::JoinHandle<()>) {
    let panel = OwnerPanel::bind(0, state, TOKEN.to_string()).expect("bind");
    let port = panel.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        for _ in 0..requests {
            if let Err(error) = panel.serve_one() {
                eprintln!("serve_one: {error}");
            }
        }
    });
    (port, handle)
}

/// 날것의 HTTP 요청을 보내고 응답 전문을 받는다.
fn raw_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    stream.write_all(request.as_bytes()).expect("write");
    stream.flush().expect("flush");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    response
}

fn status_line(response: &str) -> &str {
    response.lines().next().unwrap_or("")
}

/// 정상 경로 — 목록을 받을 수 있는가.
#[test]
fn the_workload_list_is_served_over_loopback() {
    let (port, handle) = panel_on_thread(OwnerPanelState::new(), 1);
    let response = raw_request(
        port,
        "GET /api/workloads HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    handle.join().expect("panel thread");

    assert!(
        status_line(&response).contains("200"),
        "목록을 못 받았다: {response}"
    );
    assert!(
        response.contains("\"workloads\":[]"),
        "빈 목록 형태가 아니다: {response}"
    );
}

/// ★ DNS 리바인딩 방어가 **서버 경로에서** 실제로 걸리는가.
///
/// 공격자가 자기 도메인을 127.0.0.1 로 가리키면 브라우저는 그 페이지로
/// 이 서버에 붙을 수 있다. 그때 Host 헤더는 그 도메인 이름이다.
#[test]
fn a_request_with_a_foreign_host_header_is_refused() {
    let (port, handle) = panel_on_thread(OwnerPanelState::new(), 1);
    let response = raw_request(
        port,
        "GET /api/workloads HTTP/1.1\r\nHost: evil.example.com\r\nConnection: close\r\n\r\n",
    );
    handle.join().expect("panel thread");

    assert!(
        status_line(&response).contains("403"),
        "외부 Host 요청이 통과했다 — DNS 리바인딩이 막히지 않는다: {response}"
    );
    // 거부됐는데 목록이 새어 나가면 안 된다.
    assert!(
        !response.contains("\"workloads\""),
        "거부 응답에 목록이 실려 나갔다: {response}"
    );
}

/// Host 헤더가 아예 없는 요청도 거부되는가.
#[test]
fn a_request_without_a_host_header_is_refused() {
    let (port, handle) = panel_on_thread(OwnerPanelState::new(), 1);
    let response = raw_request(
        port,
        "GET /api/workloads HTTP/1.1\r\nConnection: close\r\n\r\n",
    );
    handle.join().expect("panel thread");

    assert!(
        status_line(&response).contains("403"),
        "Host 없는 요청이 통과했다: {response}"
    );
}

/// ★ 토큰 없는 정지 요청이 막히는가.
///
/// 이것이 막히지 않으면, 소유자가 열어 둔 아무 웹페이지가 남의 작업을
/// 몰래 멈출 수 있다.
#[test]
fn a_stop_without_the_token_is_refused() {
    let (port, handle) = panel_on_thread(OwnerPanelState::new(), 2);

    let no_token = raw_request(
        port,
        "POST /api/stop HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 7\r\nConnection: close\r\n\r\nattempt",
    );
    assert!(
        status_line(&no_token).contains("403"),
        "토큰 없는 정지가 통과했다: {no_token}"
    );

    let wrong_token = raw_request(
        port,
        "POST /api/stop HTTP/1.1\r\nHost: 127.0.0.1\r\nx-gputeer-owner-token: wrong\r\n\
         Content-Length: 7\r\nConnection: close\r\n\r\nattempt",
    );
    handle.join().expect("panel thread");
    assert!(
        status_line(&wrong_token).contains("403"),
        "틀린 토큰이 통과했다: {wrong_token}"
    );
}

/// 토큰이 맞아도 없는 작업은 정지할 수 없다.
///
/// ★ 그리고 그 실패가 **200 이 아니어야** 한다. 200 이면 소유자는
///   멈춘 줄 안다.
#[test]
fn stopping_an_unknown_workload_reports_failure_not_success() {
    let (port, handle) = panel_on_thread(OwnerPanelState::new(), 1);
    let response = raw_request(
        port,
        &format!(
            "POST /api/stop HTTP/1.1\r\nHost: 127.0.0.1\r\nx-gputeer-owner-token: {TOKEN}\r\n\
             Content-Length: 7\r\nConnection: close\r\n\r\nabsent1"
        ),
    );
    handle.join().expect("panel thread");

    assert!(
        !status_line(&response).contains("200"),
        "없는 작업 정지가 성공으로 보고됐다 — 소유자가 멈춘 줄 안다: {response}"
    );
    assert!(
        response.contains("STOP_FAILED"),
        "실패 사유가 구분되지 않는다: {response}"
    );
}

/// 화면이 클릭재킹·스니핑 방어 헤더를 실제로 붙이는가.
#[test]
fn the_html_page_carries_its_defensive_headers() {
    let (port, handle) = panel_on_thread(OwnerPanelState::new(), 1);
    let response = raw_request(
        port,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    handle.join().expect("panel thread");

    assert!(status_line(&response).contains("200"), "{response}");
    for header in [
        "X-Frame-Options: DENY",
        "X-Content-Type-Options: nosniff",
        "Cache-Control: no-store",
    ] {
        assert!(
            response.contains(header),
            "{header} 가 없다 — 남의 페이지가 이 화면을 덮을 수 있다: {response}"
        );
    }
}

/// 거대한 본문을 선언한 요청이 메모리를 먹지 않는가.
///
/// ★ 상한이 없으면 `vec![0u8; content_length]` 가 그대로 할당된다.
#[test]
fn an_absurd_content_length_is_refused_without_allocating() {
    let (port, handle) = panel_on_thread(OwnerPanelState::new(), 1);
    let response = raw_request(
        port,
        &format!(
            "POST /api/stop HTTP/1.1\r\nHost: 127.0.0.1\r\nx-gputeer-owner-token: {TOKEN}\r\n\
             Content-Length: 99999999999\r\nConnection: close\r\n\r\n"
        ),
    );
    handle.join().expect("panel thread");

    assert!(
        status_line(&response).contains("400"),
        "거대한 Content-Length 가 거부되지 않았다: {response}"
    );
}

/// ★ 패널은 외부 인터페이스에 붙지 않는다.
///
/// §0.1 이 "외부 인터페이스에 바인딩하지 않는다" 고 못박은 것을
/// 실제 소켓 주소로 확인한다.
#[test]
fn the_panel_binds_only_to_loopback() {
    let panel = OwnerPanel::bind(0, OwnerPanelState::new(), TOKEN.to_string()).expect("bind");
    let addr = panel.local_addr().expect("addr");
    assert!(
        addr.ip().is_loopback(),
        "패널이 loopback 이 아닌 주소에 붙었다: {addr}"
    );
}
