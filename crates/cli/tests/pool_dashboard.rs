//! `gputeer dashboard` — 느린 연결 하나가 화면을 묶지 못한다(결함 285 · 재검수 89).
//!
//! ★ 화면은 요청을 한 스레드에서 차례로 받는다. 전에는 읽기 시한이 read 한 번마다라, 4초마다 1바이트씩 보내는 연결 하나가
//!   화면 전체를 끝없이 붙잡았다. 이제 요청 머리 전체에 5초 시한이 걸린다.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn cli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gputeer"))
}

#[test]
fn a_slow_client_cannot_hold_the_dashboard() {
    let dir = tempfile::tempdir().unwrap();
    // 없는 DB 여도 `/` 화면은 낸다(`/api/status` 만 읽기 실패를 500 으로 알린다).
    let db = dir.path().join("control.sqlite3");
    let mut child = Command::new(cli_bin())
        .args([
            "dashboard",
            "--control-db",
            db.to_str().unwrap(),
            "--port",
            "0",
            "--max-requests",
            "2",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("dashboard");
    let mut first = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut first)
        .unwrap();
    let address = first
        .trim()
        .strip_prefix("DASHBOARD_LISTENING http://")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap_or_else(|| panic!("주소 줄이 아니다: {first:?}"))
        .to_string();

    // 느린 연결 — 1초마다 1바이트, 끝 표시를 보내지 않는다(20초 동안).
    let slow_address = address.clone();
    let slow = std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(&slow_address).unwrap();
        for byte in b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Slow: aaaa"
            .iter()
            .cycle()
            .take(20)
        {
            if stream.write_all(&[*byte]).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
    std::thread::sleep(Duration::from_millis(500));

    // 정상 연결 — 느린 연결의 시한(5초)이 지나면 받는다.
    let started = Instant::now();
    let mut stream = std::net::TcpStream::connect(&address).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let waited = started.elapsed();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(
        waited < Duration::from_secs(10),
        "느린 연결 하나가 화면을 {waited:?} 동안 붙잡았다"
    );
    let _ = child.wait();
    let _ = slow.join();
}
