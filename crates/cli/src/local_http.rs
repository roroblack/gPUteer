//! 127.0.0.1 전용 작은 HTTP 조각 — 운영자 대시보드 · 제출자 화면이 같이 쓴다.
//!
//! ★ 결함 285 에서 배운 것을 한 곳에 둔다: 시한은 요청 **전체**에(read 한 번마다가 아니다), 크기 한도는 종료 표시보다 **먼저**.
//! ★ 주소를 인자로 받지 않는다 — 여는 곳은 늘 127.0.0.1 이다(Owner Panel 과 같은 이유).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::time::{Duration, Instant};

/// 요청 전체에 주는 시간.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(5);
/// 요청 머리 한도.
pub const HEAD_LIMIT: usize = 16 * 1024;

pub struct Request {
    pub method: String,
    pub path: String,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// 머리 값(이름은 대소문자를 가리지 않는다).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Host 가 loopback 인가 — DNS 리바인딩 방어.
    pub fn host_is_loopback(&self) -> bool {
        let Some(host) = self.header("host") else {
            return false;
        };
        let name = host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host);
        matches!(name, "127.0.0.1" | "localhost")
    }

    /// 쓰는 요청의 토큰이 맞는가 — 다른 사이트의 페이지는 이 헤더를 붙여 보낼 수 없다(교차 출처 요청은 사용자 헤더를 못 단다 · 응답도 못 읽는다).
    pub fn token_matches(&self, token: &str) -> bool {
        !token.is_empty() && self.header("x-gputeer-token") == Some(token)
    }
}

/// 127.0.0.1:<port> 를 연다. 포트 0 이면 OS 가 고른다.
pub fn bind(port: u16) -> Result<TcpListener, String> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .map_err(|e| format!("127.0.0.1:{port} 를 열지 못했다: {e}"))
}

/// 무작위 토큰(16바이트 16진수).
pub fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("토큰을 만들지 못했다: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// 요청을 읽는다 — 머리 · 몸 전체에 `REQUEST_DEADLINE`, 머리 16KiB · 몸 `body_limit` 한도.
pub fn read_request(stream: &mut TcpStream, body_limit: usize) -> Result<Request, String> {
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > HEAD_LIMIT {
            return Err("요청 머리가 너무 길다".into());
        }
        let n = read_some(stream, &mut chunk, deadline)?;
        if n == 0 {
            return Err("요청 머리가 끝나기 전에 연결이 닫혔다".into());
        }
        buffer.extend_from_slice(&chunk[..n]);
    };
    if head_end > HEAD_LIMIT {
        return Err("요청 머리가 너무 길다".into());
    }
    let head = String::from_utf8(buffer[..head_end].to_vec())
        .map_err(|_| "요청 머리가 UTF-8 이 아니다".to_string())?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    let content_length = match headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        Some((_, value)) => value
            .parse::<usize>()
            .map_err(|_| format!("Content-Length 를 읽지 못했다({value:?})"))?,
        None => 0,
    };
    if content_length > body_limit {
        return Err(format!(
            "요청 몸이 한도({body_limit}바이트)를 넘는다({content_length})"
        ));
    }
    let mut body: Vec<u8> = buffer[head_end + 4..].to_vec();
    while body.len() < content_length {
        let n = read_some(stream, &mut chunk, deadline)?;
        if n == 0 {
            return Err("요청 몸이 다 오기 전에 연결이 닫혔다".into());
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn read_some(stream: &mut TcpStream, chunk: &mut [u8], deadline: Instant) -> Result<usize, String> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err("요청이 시한 안에 다 오지 않았다".into());
    }
    stream
        .set_read_timeout(Some(left))
        .map_err(|e| format!("시한을 걸지 못했다: {e}"))?;
    stream
        .read(chunk)
        .map_err(|e| format!("요청을 읽지 못했다: {e}"))
}

/// 응답을 쓴다 — 캐시 금지 · 프레임 금지 · 형식 추측 금지 · 외부 자원 금지(CSP).
pub fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    extra_headers: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    };
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'\r\n\
         {extra_headers}Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

pub fn respond_text(stream: &mut TcpStream, status: u16, text: &str) -> std::io::Result<()> {
    respond(
        stream,
        status,
        "text/plain; charset=utf-8",
        "",
        text.as_bytes(),
    )
}

pub fn respond_json(
    stream: &mut TcpStream,
    status: u16,
    value: &serde_json::Value,
) -> std::io::Result<()> {
    respond(
        stream,
        status,
        "application/json; charset=utf-8",
        "",
        value.to_string().as_bytes(),
    )
}

/// 동시에 처리하는 연결 수 상한(결함 403).
pub const MAX_CONCURRENT: usize = 16;

/// 연결마다 스레드로 처리한다 — 끝나지 않는 연결 하나가 다른 요청을 막지 못하게(결함 403 · 재검수 93). 동시에 `MAX_CONCURRENT` 개까지 받고,
/// 넘으면 받자마자 닫는다. `max_requests`(시험용)는 **처리를 끝낸** 요청만 센다 — 다 세면 진행 중인 처리가 끝나기를 기다렸다 돌아온다.
///
/// ★ 남는 것: 로컬 프로세스가 연결 16개를 계속 채우면 여전히 밀린다(로컬 전용 화면의 한계).
pub fn serve<F>(listener: TcpListener, max_requests: Option<u64>, handler: F) -> Result<u64, String>
where
    F: Fn(TcpStream) -> std::io::Result<()> + Send + Sync + 'static,
{
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("듣기 소켓을 설정하지 못했다: {e}"))?;
    let handler = Arc::new(handler);
    let active = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicU64::new(0));
    loop {
        if max_requests.is_some_and(|max| max > 0 && done.load(Ordering::SeqCst) >= max) {
            while active.load(Ordering::SeqCst) > 0 {
                std::thread::sleep(Duration::from_millis(10));
            }
            return Ok(done.load(Ordering::SeqCst));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                if active.load(Ordering::SeqCst) >= MAX_CONCURRENT {
                    // 받자마자 닫는다 — 기다리게 하지 않는다.
                    drop(stream);
                    continue;
                }
                active.fetch_add(1, Ordering::SeqCst);
                let (handler, active, done) = (handler.clone(), active.clone(), done.clone());
                std::thread::spawn(move || {
                    if let Err(e) = handler(stream) {
                        eprintln!("local-http: 요청 처리 실패(계속 받는다): {e}");
                    }
                    done.fetch_add(1, Ordering::SeqCst);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => eprintln!("local-http: accept 실패(계속 받는다): {e}"),
        }
    }
}
