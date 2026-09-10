//! Owner Panel — 노드 소유자가 자기 GPU 를 되찾는 로컬 화면.
//!
//! # 이것이 `CLAUDE.md` §0.1 그 자체다
//!
//! §0.1 은 다른 어떤 규칙보다 앞에 이렇게 정한다.
//!
//! ```text
//! 노드 소유자는 언제든 자기 GPU 를 즉시 비울 수 있어야 한다.
//! 네트워크가 끊겨도, quorum 이 없어도, Coordinator 가 죽어도 동작해야 한다.
//! 이것을 원격 서비스에 의존하게 만들지 않는다.
//!
//! 소유자 통제 UI(Owner Panel)는 로컬 Agent 가 제공한다. 127.0.0.1 바인딩 고정.
//! 소유자 화면에는 누가 · 어느 Job 을 · 언제부터 · 얼마나 돌리는지 항상 보인다.
//! 강제 종료 시 손실 범위를 미리 계산해 보여준다.
//! ```
//!
//! 그래서 이 모듈은 **아무 원격 의존이 없다.** Coordinator 에 묻지 않고,
//! 서명을 검증하지 않으며, 네트워크가 죽어도 그대로 돈다. 알고 있는
//! 사실을 로컬 메모리에서 읽어 보여주고, 정지 손잡이를 부를 뿐이다.
//!
//! # 왜 HTTP 라이브러리를 안 쓰는가
//!
//! 이 워크스페이스에는 HTTP 서버 크레이트가 없고, 이 화면 하나 때문에
//! 의존성을 늘리면 그만큼 남의 PC 에서 도는 코드가 늘어난다. 필요한 건
//! `GET` 두 개와 `POST` 하나뿐이라 직접 처리한다.
//!
//! # 로컬 서버라고 안전한 게 아니다
//!
//! ★ `127.0.0.1` 에만 바인딩해도 **브라우저로 열린 아무 웹페이지가**
//!   이 서버에 요청을 보낼 수 있다. 그러면 남의 사이트가 소유자 몰래
//!   작업을 정지시킬 수 있다. 그래서 세 겹으로 막는다.
//!
//! ```text
//! 1  127.0.0.1 바인딩         외부 인터페이스에 절대 안 붙는다
//! 2  Host 헤더 검사           DNS 리바인딩 차단 — 이름으로 들어온 요청은 거부
//! 3  정지에 토큰 필요         기동 시 찍은 토큰을 커스텀 헤더로 요구.
//!                             커스텀 헤더는 브라우저가 preflight 를 거치고
//!                             이 서버는 CORS 를 허용하지 않으므로 막힌다
//! ```
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 인증          토큰은 CSRF 방어지 신원 확인이 아니다. 이 기계에
//!               로그인한 다른 사용자는 토큰을 읽을 수 있다
//! TLS           로컬 루프백 전용이라 안 붙였다
//! 캐시 삭제     §0.5 의 데이터셋 삭제는 이 조각에 없다
//! 일시정지      §0.1 의 정지만 있다. PAUSE 는 상태 전이 계층이 필요하다
//! ```

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::exec::WorkloadStopper;

/// 지금 이 노드에서 도는 작업 하나.
///
/// ★ 필드가 §0.1 의 "누가 · 어느 Job 을 · 언제부터 · 얼마나" 에 1:1 로
///   대응한다. 하나라도 비면 소유자는 무엇을 멈추는지 모른 채 누른다.
pub struct RunningWorkload {
    /// 어느 Job 인가.
    pub job_id: String,
    /// 어느 시도인가. 같은 Job 이 여러 번 시도될 수 있다.
    pub attempt_id: String,
    /// **누가** 냈는가. 이것이 없으면 소유자는 자기 GPU 를 누가 쓰는지 모른다.
    pub submitter_device_id: String,
    /// 언제부터 도는가(밀리초).
    pub started_at_unix_ms: u64,
    /// 무엇을 돌리는가.
    pub entrypoint: String,
    /// 마지막으로 확정된 체크포인트. 손실 범위 계산의 기준점이다.
    ///
    /// `None` 은 **아직 하나도 확정 안 됐다**는 뜻이며, 그 경우 지금
    /// 멈추면 시작부터 지금까지 전부 잃는다.
    pub last_checkpoint_at_unix_ms: Option<u64>,
    /// 이 작업을 멈추는 손잡이.
    pub stopper: WorkloadStopper,
}

/// 지금 멈추면 무엇을 잃는가.
///
/// ★ §0.1 은 "'최대 12분 진행 손실' 을 모른 채 누르게 하지 않는다" 고
///   요구한다. 그 문장을 만들기 위한 계산이다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LossEstimate {
    /// 마지막 확정 지점 이후 흐른 시간(밀리초).
    pub lost_ms: u64,
    /// 확정된 체크포인트가 하나도 없는가.
    ///
    /// `true` 면 `lost_ms` 는 "체크포인트 이후" 가 아니라 **시작 이후
    /// 전부**다. 두 경우를 같은 숫자로 보여주면 소유자가 오해한다.
    pub nothing_committed_yet: bool,
}

/// 손실 범위를 계산한다. 순수 함수다 — 시계를 스스로 읽지 않는다.
///
/// # 왜 `now` 를 인자로 받는가
///
/// 안에서 시계를 읽으면 테스트가 시간에 의존하게 되고, 같은 입력에
/// 다른 답이 나올 수 있다. 이 저장소의 다른 순수 kernel 들과 같은 규칙이다.
///
/// # 시계가 거꾸로 간 경우
///
/// ★ `now` 가 기준 시각보다 이전이면 **음수 대신 0** 을 낸다(`saturating_sub`).
///   일반 뺄셈이었다면 overflow 검사 설정에 따라 panic 하거나 거대한 값으로
///   감싸돌 수 있다 — 감싸돌면 "3억 년 손실" 같은 숫자가 화면까지 갈 수 있다.
///   시계 보정·NTP 점프로 `now` 가 기준보다 이전이 되는 일은 일어날 수 있다
///   (재검수 32 — 전에는 감싸돌아 화면에 뜬다고만 적었다).
pub fn estimate_loss(workload: &RunningWorkload, now_unix_ms: u64) -> LossEstimate {
    let (baseline, nothing_committed_yet) = match workload.last_checkpoint_at_unix_ms {
        Some(at) => (at, false),
        None => (workload.started_at_unix_ms, true),
    };
    LossEstimate {
        lost_ms: now_unix_ms.saturating_sub(baseline),
        nothing_committed_yet,
    }
}

/// Owner Panel 이 보여주고 조작하는 대상.
///
/// `Arc<Mutex<..>>` 로 Agent 실행 스레드와 공유한다 — Agent 가 작업을
/// 시작하면 여기 넣고, 끝나면 뺀다.
#[derive(Clone)]
pub struct OwnerPanelState {
    inner: Arc<Mutex<BTreeMap<String, RunningWorkload>>>,
}

impl Default for OwnerPanelState {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnerPanelState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// 작업이 시작됐음을 등록한다. 키는 `attempt_id` 다.
    ///
    /// ★ `job_id` 가 아니라 `attempt_id` 를 키로 쓴다. 같은 Job 이 실패해
    ///   다시 시도되면 두 시도가 동시에 보일 수 있는데, `job_id` 를
    ///   키로 쓰면 뒤엣것이 앞엣것을 덮어써 **멈출 수 없는 작업이
    ///   생긴다**.
    pub fn register(&self, workload: RunningWorkload) {
        self.lock().insert(workload.attempt_id.clone(), workload);
    }

    /// 작업이 끝났음을 알린다.
    pub fn unregister(&self, attempt_id: &str) {
        self.lock().remove(attempt_id);
    }

    /// 지금 도는 작업들을 사람이 읽는 요약으로 낸다.
    pub fn snapshot(&self, now_unix_ms: u64) -> Vec<WorkloadSummary> {
        self.lock()
            .values()
            .map(|workload| {
                let loss = estimate_loss(workload, now_unix_ms);
                WorkloadSummary {
                    job_id: workload.job_id.clone(),
                    attempt_id: workload.attempt_id.clone(),
                    submitter_device_id: workload.submitter_device_id.clone(),
                    entrypoint: workload.entrypoint.clone(),
                    started_at_unix_ms: workload.started_at_unix_ms,
                    running_ms: now_unix_ms.saturating_sub(workload.started_at_unix_ms),
                    loss,
                }
            })
            .collect()
    }

    /// 작업 하나를 멈춘다.
    ///
    /// 목록에서 빼지 않는다 — 실제로 끝났는지는 실행 스레드가
    /// `unregister()` 로 알려준다. 여기서 미리 빼면 정지가 실패했을 때
    /// 화면에서 사라져 소유자가 멈춘 줄 알게 된다.
    pub fn stop(&self, attempt_id: &str) -> Result<(), String> {
        let guard = self.lock();
        let workload = guard
            .get(attempt_id)
            .ok_or_else(|| format!("그런 작업이 없다: {attempt_id}"))?;
        workload.stopper.stop().map_err(|error| error.to_string())
    }

    /// ★ 락이 poisoned 여도 계속 간다.
    ///
    ///   어떤 스레드가 이 상태를 들고 panic 했다는 뜻인데, 그렇다고
    ///   Owner Panel 을 죽이면 **소유자가 자기 GPU 를 되찾을 방법이
    ///   사라진다.** §0.1 은 "Coordinator 가 죽어도 동작해야 한다" 고
    ///   했고, 같은 이유로 이 프로세스 안의 다른 스레드가 죽어도
    ///   동작해야 한다. 목록이 조금 이상해도 정지 버튼은 살아 있어야 한다.
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, RunningWorkload>> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// 화면에 보여줄 작업 하나.
#[derive(Clone, Debug)]
pub struct WorkloadSummary {
    pub job_id: String,
    pub attempt_id: String,
    pub submitter_device_id: String,
    pub entrypoint: String,
    pub started_at_unix_ms: u64,
    pub running_ms: u64,
    pub loss: LossEstimate,
}

/// 로컬 전용 패널 서버.
pub struct OwnerPanel {
    listener: TcpListener,
    state: OwnerPanelState,
    /// 정지 요청에 요구하는 토큰. 기동 시 표준 출력에 찍는다.
    token: String,
}

impl OwnerPanel {
    /// `127.0.0.1` 에 바인딩한다. **이 주소는 인자가 아니다.**
    ///
    /// ★ 주소를 받게 만들면 언젠가 누가 `0.0.0.0` 을 넣는다. §0.1 은
    ///   "외부 인터페이스에 바인딩하지 않는다" 고 못박았으므로,
    ///   그걸 표현할 방법 자체를 두지 않는다.
    ///
    /// 포트 0 을 주면 OS 가 고른다 — `local_addr()` 로 확인한다.
    pub fn bind(port: u16, state: OwnerPanelState, token: String) -> std::io::Result<Self> {
        if token.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "빈 토큰으로는 시작하지 않는다 — 토큰 검사가 통과만 하게 된다",
            ));
        }
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let listener = TcpListener::bind(addr)?;
        Ok(Self {
            listener,
            state,
            token,
        })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// 요청 하나를 처리한다. 테스트가 한 걸음씩 몰기 위해 분리했다.
    pub fn serve_one(&self) -> std::io::Result<()> {
        let (stream, _peer) = self.listener.accept()?;
        // ★ 타임아웃을 반드시 건다. 없으면 헤더만 보내고 멈춘 상대
        //   하나가 패널 전체를 잠근다 — 그러면 소유자가 정지를 못 한다.
        //   `framed_ingress` 에서 이미 같은 교훈을 얻었다.
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        self.handle(stream)
    }

    /// 계속 받는다.
    pub fn serve_forever(&self) -> std::io::Result<()> {
        loop {
            // 요청 하나가 실패해도 패널을 죽이지 않는다 — 소유자의
            // 정지 수단이 사라지면 안 된다.
            if let Err(error) = self.serve_one() {
                eprintln!("owner-panel: 요청 처리 실패(계속 받는다): {error}");
            }
        }
    }

    fn handle(&self, mut stream: TcpStream) -> std::io::Result<()> {
        let request = match read_request(&mut stream) {
            Ok(request) => request,
            Err(message) => return respond(&mut stream, 400, "text/plain; charset=utf-8", &message),
        };

        // ★ Host 검사 — DNS 리바인딩 방어.
        //   공격자가 자기 도메인을 127.0.0.1 로 가리키게 만들면, 브라우저는
        //   그 도메인의 페이지로 이 서버에 붙을 수 있다. 그때 Host 헤더는
        //   그 도메인 이름이므로 여기서 걸린다.
        if !host_is_loopback(request.host.as_deref()) {
            return respond(
                &mut stream,
                403,
                "text/plain; charset=utf-8",
                "이 패널은 로컬에서만 쓴다(Host 가 loopback 이 아니다)",
            );
        }

        let now = now_unix_ms();
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/") => respond(&mut stream, 200, "text/html; charset=utf-8", &render_html()),
            ("GET", "/api/workloads") => respond(
                &mut stream,
                200,
                "application/json; charset=utf-8",
                &render_workloads_json(&self.state.snapshot(now), now, &self.token),
            ),
            ("POST", "/api/stop") => {
                // ★ 토큰을 **먼저** 본다. 어떤 작업을 멈추라는 요청인지
                //   읽기도 전에 막는다.
                if request.token.as_deref() != Some(self.token.as_str()) {
                    return respond(
                        &mut stream,
                        403,
                        "text/plain; charset=utf-8",
                        "토큰이 없거나 다르다 — 정지는 이 기계의 패널에서만 할 수 있다",
                    );
                }
                let attempt_id = request.body.trim();
                if attempt_id.is_empty() {
                    return respond(
                        &mut stream,
                        400,
                        "text/plain; charset=utf-8",
                        "멈출 attempt_id 가 없다",
                    );
                }
                match self.state.stop(attempt_id) {
                    Ok(()) => respond(
                        &mut stream,
                        200,
                        "text/plain; charset=utf-8",
                        &format!("STOP_REQUESTED attempt_id={attempt_id}"),
                    ),
                    // ★ 정지 실패를 200 으로 돌려주지 않는다. 소유자가
                    //   멈춘 줄 알면 안 된다.
                    Err(message) => respond(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        &format!("STOP_FAILED {message}"),
                    ),
                }
            }
            _ => respond(&mut stream, 404, "text/plain; charset=utf-8", "없는 경로다"),
        }
    }
}

struct Request {
    method: String,
    path: String,
    host: Option<String>,
    token: Option<String>,
    body: String,
}

/// 정지 요청에 요구하는 헤더 이름.
///
/// ★ **커스텀 헤더여야 한다.** 브라우저는 커스텀 헤더가 붙은 교차
///   출처 요청 앞에 preflight(OPTIONS)를 보내는데, 이 서버는 CORS 를
///   허용하지 않으므로 그 지점에서 막힌다. 쿠키나 폼 필드로 하면
///   그 방어가 없다.
const TOKEN_HEADER: &str = "x-gputeer-owner-token";

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut reader = BufReader::new(stream);
    let mut start_line = String::new();
    reader
        .read_line(&mut start_line)
        .map_err(|error| format!("요청 줄을 읽지 못했다: {error}"))?;
    let mut parts = start_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    if method.is_empty() || path.is_empty() {
        return Err("요청 줄이 비었다".to_string());
    }

    let mut host = None;
    let mut token = None;
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| format!("헤더를 읽지 못했다: {error}"))?;
        if read == 0 || line.trim().is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        match name.as_str() {
            "host" => host = Some(value),
            TOKEN_HEADER => token = Some(value),
            "content-length" => {
                content_length = value.parse().map_err(|_| {
                    format!("Content-Length 를 읽을 수 없다: {value:?}")
                })?;
            }
            _ => {}
        }
    }

    // ★ 본문 길이에 상한을 둔다. 없으면 상대가 거대한 Content-Length 를
    //   선언해 이 프로세스의 메모리를 먹는다.
    const MAX_BODY: usize = 4096;
    if content_length > MAX_BODY {
        return Err(format!("본문이 너무 크다({content_length} > {MAX_BODY})"));
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|error| format!("본문을 읽지 못했다: {error}"))?;
    }

    Ok(Request {
        method,
        path,
        host,
        token,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

/// `Host` 헤더가 loopback authority 인가.
///
/// ★ **문법을 실제로 검사한다**(2026-08-29, 독립 검수 지적). 초안은
///   대괄호 뒤에 무엇이 오든 무시하고, 비대괄호도 첫 `:` 뒤를 전부
///   버렸다. 그래서 아래 같은 깨진 Host 가 전부 통과했다.
///
///   ```text
///   [::1                    닫는 괄호가 없다
///   [::1]evil.example       괄호 뒤에 다른 이름이 붙었다
///   localhost:not-a-port    포트가 숫자가 아니다
///   127.0.0.1:1:2           콜론이 두 개다
///   ```
///
///   표준 브라우저는 이런 Host 를 만들지 못하므로 실제 CSRF 우회는
///   아니었지만, "Host 를 파싱해 loopback 만 허용한다" 는 주장이
///   코드보다 컸다. 애매하면 거부한다.
///
/// `localhost` 도 허용한다 — 소유자가 주소창에 그렇게 친다. 그 이름은
/// hosts 파일에서 loopback 으로 고정되며, 공격자가 자기 도메인을
/// 127.0.0.1 로 가리켜도 그 **도메인 이름**이 Host 에 들어오므로 걸린다.
fn host_is_loopback(host: Option<&str>) -> bool {
    let Some(host) = host else {
        // Host 없는 HTTP/1.1 요청은 규격 위반이다. 통과시키지 않는다.
        return false;
    };
    let (name, port) = match host.strip_prefix('[') {
        // IPv6 리터럴: `[::1]` 또는 `[::1]:8765` 만 받는다.
        Some(rest) => match rest.split_once(']') {
            Some((inside, after)) => (inside, after),
            // 닫는 대괄호가 없다 — 문법 위반이다.
            None => return false,
        },
        None => match host.split_once(':') {
            Some((name, port)) => (name, port),
            None => (host, ""),
        },
    };

    // 대괄호 형태의 나머지는 비었거나 `:포트` 여야 한다.
    let port = if host.starts_with('[') {
        match port {
            "" => "",
            rest => match rest.strip_prefix(':') {
                Some(port) => port,
                // `]` 뒤에 콜론 없이 뭔가 붙었다.
                None => return false,
            },
        }
    } else {
        port
    };

    // 포트가 있으면 숫자여야 하고 u16 범위여야 한다.
    if !port.is_empty() && port.parse::<u16>().is_err() {
        return false;
    }

    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    // ★ 캐시 금지 + 프레임 금지. 남의 페이지가 이 화면을 iframe 으로
    //   덮어 소유자가 다른 걸 누르게 만드는 클릭재킹을 막는다.
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Frame-Options: DENY\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
        body.as_bytes().len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// JSON 을 손으로 만든다 — 값은 전부 이스케이프한다.
fn render_workloads_json(items: &[WorkloadSummary], now_unix_ms: u64, token: &str) -> String {
    let entries: Vec<String> = items
        .iter()
        .map(|item| {
            format!(
                "{{\"job_id\":{},\"attempt_id\":{},\"submitter_device_id\":{},\
                 \"entrypoint\":{},\"started_at_unix_ms\":{},\"running_ms\":{},\
                 \"lost_ms\":{},\"nothing_committed_yet\":{}}}",
                json_string(&item.job_id),
                json_string(&item.attempt_id),
                json_string(&item.submitter_device_id),
                json_string(&item.entrypoint),
                item.started_at_unix_ms,
                item.running_ms,
                item.loss.lost_ms,
                item.loss.nothing_committed_yet
            )
        })
        .collect();
    format!(
        "{{\"now_unix_ms\":{now_unix_ms},\"token\":{},\"workloads\":[{}]}}",
        json_string(token),
        entries.join(",")
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render_html() -> String {
    // 화면은 의도적으로 최소다. 목적은 예쁜 UI 가 아니라 §0.1 이
    // 요구하는 네 가지(누가·무엇을·언제부터·얼마나 잃는지)를 보이고
    // 멈추는 것이다.
    include_str!("owner_panel.html").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workload(started: u64, checkpoint: Option<u64>) -> RunningWorkload {
        RunningWorkload {
            job_id: "job".into(),
            attempt_id: "attempt".into(),
            submitter_device_id: "device".into(),
            started_at_unix_ms: started,
            entrypoint: "x".into(),
            last_checkpoint_at_unix_ms: checkpoint,
            stopper: crate::exec::WorkloadStopper::for_test(),
        }
    }

    /// 체크포인트가 있으면 그 이후만 잃는다.
    #[test]
    fn loss_is_measured_from_the_last_checkpoint() {
        let estimate = estimate_loss(&workload(1_000, Some(5_000)), 8_000);
        assert_eq!(estimate.lost_ms, 3_000);
        assert!(!estimate.nothing_committed_yet);
    }

    /// 체크포인트가 없으면 시작부터 전부 잃는다.
    ///
    /// ★ 두 경우를 같은 숫자로만 보여주면 소유자가 "3초만 잃는구나" 로
    ///   오해한다. 확정된 게 하나도 없다는 사실을 따로 표시한다.
    #[test]
    fn with_no_checkpoint_everything_since_start_is_lost() {
        let estimate = estimate_loss(&workload(1_000, None), 8_000);
        assert_eq!(estimate.lost_ms, 7_000);
        assert!(
            estimate.nothing_committed_yet,
            "확정된 체크포인트가 없다는 사실이 표시되지 않는다"
        );
    }

    /// 시계가 거꾸로 가도 거대한 숫자가 나오지 않는다.
    ///
    /// ★ NTP 보정으로 실제로 일어난다. u64 뺄셈이 underflow 하면
    ///   "3억 년 손실" 이 화면에 뜬다.
    #[test]
    fn a_backwards_clock_does_not_produce_a_giant_number() {
        let estimate = estimate_loss(&workload(1_000, Some(9_000)), 5_000);
        assert_eq!(estimate.lost_ms, 0, "시계 역행이 underflow 로 감싸돌았다");
    }

    /// `Host` 검사가 실제로 갈라내는가.
    #[test]
    fn host_check_accepts_loopback_and_rejects_names() {
        for good in ["127.0.0.1", "127.0.0.1:8765", "localhost", "localhost:1", "[::1]:9"] {
            assert!(host_is_loopback(Some(good)), "{good} 이 거부됐다");
        }
        // ★ 문법이 깨진 Host 도 전부 거부해야 한다. 초안은 이것들을
        //   전부 통과시켰다(2026-08-29 독립 검수가 하나하나 짚어줌).
        for bad in [
            "evil.example.com",
            "evil.example.com:8765",
            "0.0.0.0",
            "192.168.0.5",
            "[::1",
            "[::1]evil.example",
            "[127.0.0.1]evil.example",
            "localhost:not-a-port",
            "127.0.0.1:1:2",
            "127.0.0.1:99999",
        ] {
            assert!(!host_is_loopback(Some(bad)), "{bad} 이 통과했다");
        }
        assert!(!host_is_loopback(None), "Host 없는 요청이 통과했다");
    }

    /// 빈 토큰으로는 시작하지 않는다.
    ///
    /// 빈 토큰을 허용하면 헤더 없는 요청의 `None` 과 비교가 어긋나
    /// 검사가 사실상 통과만 하게 될 위험이 있다 — 아예 막는다.
    #[test]
    fn an_empty_token_is_refused_at_bind_time() {
        let error = match OwnerPanel::bind(0, OwnerPanelState::new(), String::new()) {
            Err(error) => error,
            Ok(_) => panic!("빈 토큰이 받아들여졌다"),
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    /// 같은 Job 의 두 시도가 서로를 덮지 않는가.
    ///
    /// ★ `job_id` 를 키로 썼다면 뒤엣것이 앞엣것을 덮어써 **멈출 수 없는
    ///   작업**이 생긴다.
    #[test]
    fn two_attempts_of_one_job_are_both_visible() {
        let state = OwnerPanelState::new();
        let mut first = workload(1_000, None);
        first.attempt_id = "attempt-1".into();
        let mut second = workload(2_000, None);
        second.attempt_id = "attempt-2".into();
        state.register(first);
        state.register(second);

        let snapshot = state.snapshot(3_000);
        assert_eq!(snapshot.len(), 2, "한 시도가 다른 시도를 덮어썼다");
    }

    /// JSON 이스케이프가 실제로 되는가.
    #[test]
    fn json_values_are_escaped() {
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
