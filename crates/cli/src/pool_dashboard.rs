//! `gputeer dashboard` — 풀 운영자가 보는 **읽기 전용** 웹 화면(127.0.0.1 전용).
//!
//! # 왜
//!
//! `gputeer status` 는 텍스트 한 번이다. 운영자는 Job 이 어느 노드에서 어떤 상태인지, 노드가 언제 마지막으로 인사했는지,
//! 예약이 만료 의심인지를 **계속** 봐야 한다. 이 화면은 같은 사실(`gputeer_coordinator::status::pool_status`)을 5초마다 다시 읽어
//! 표로 보여 준다.
//!
//! # 하지 않는 것
//!
//! ```text
//! 쓰기        아무것도 바꾸지 않는다 — Job 취소 · 노드 해제 버튼이 없다(그건 CLI 와 런북 절차다: release-lost-node 등)
//! 판정        "살아 있다/죽었다" 를 말하지 않는다 — 마지막으로 들은 시각만 보여 준다(status 와 같다)
//! 외부 공개   127.0.0.1 에만 연다. 주소를 인자로 받지 않는다(Owner Panel 과 같은 이유). Host 가 loopback 이 아니면 거부한다
//! ```

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gputeer_coordinator::status::{pool_status, PoolStatus};

const USAGE: &str =
    "gputeer dashboard --control-db <control.sqlite3> --port <로컬 포트> [--max-requests <n>]";

pub fn run(args: &[String]) -> Result<String, String> {
    let mut control_db: Option<PathBuf> = None;
    let mut port: Option<u16> = None;
    let mut max_requests: Option<u64> = None;
    let mut iter = args.iter();
    while let Some(key) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| format!("DASHBOARD_ARGS: {key} 에 값이 없다\n{USAGE}"))?;
        match key.as_str() {
            "--control-db" => control_db = Some(value.into()),
            "--port" => {
                port = Some(
                    value
                        .parse()
                        .map_err(|e| format!("DASHBOARD_ARGS: --port 파싱 실패({value:?}): {e}"))?,
                )
            }
            // 시험용 — n 번 응답하고 끝난다. 0 이나 생략이면 끝없이.
            "--max-requests" => {
                max_requests = Some(value.parse().map_err(|e| {
                    format!("DASHBOARD_ARGS: --max-requests 파싱 실패({value:?}): {e}")
                })?)
            }
            other => return Err(format!("DASHBOARD_ARGS: 모르는 옵션 {other}\n{USAGE}")),
        }
    }
    let (Some(control_db), Some(port)) = (control_db, port) else {
        return Err(format!(
            "DASHBOARD_ARGS: --control-db · --port 는 반드시 준다\n{USAGE}"
        ));
    };
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .map_err(|e| format!("DASHBOARD: 127.0.0.1:{port} 를 열지 못했다: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    println!("DASHBOARD_LISTENING http://{address}/");
    let mut served: u64 = 0;
    loop {
        if max_requests.is_some_and(|max| max > 0 && served >= max) {
            return Ok(format!("DASHBOARD_DONE served={served}"));
        }
        let (stream, _) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(e) => {
                eprintln!("dashboard: accept 실패(계속 받는다): {e}");
                continue;
            }
        };
        served += 1;
        if let Err(e) = handle(stream, &control_db) {
            eprintln!("dashboard: 요청 처리 실패(계속 받는다): {e}");
        }
    }
}

fn handle(mut stream: TcpStream, control_db: &Path) -> std::io::Result<()> {
    // ★ 시한 — 헤더만 보내고 멈춘 상대 하나가 화면 전체를 잠그지 못하게 한다.
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let head = match read_head(&mut stream) {
        Ok(head) => head,
        Err(why) => return respond(&mut stream, 400, "text/plain; charset=utf-8", &why),
    };
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let host = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("host")
            .then(|| value.trim().to_string())
    });
    // ★ DNS 리바인딩 방어 — Owner Panel 과 같다.
    if !host_is_loopback(host.as_deref()) {
        return respond(
            &mut stream,
            403,
            "text/plain; charset=utf-8",
            "이 화면은 로컬에서만 본다(Host 가 loopback 이 아니다)",
        );
    }
    match (method, path) {
        ("GET", "/") => respond(&mut stream, 200, "text/html; charset=utf-8", PAGE),
        ("GET", "/api/status") => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            match pool_status(control_db, now) {
                Ok(status) => respond(
                    &mut stream,
                    200,
                    "application/json; charset=utf-8",
                    &to_json(&status, now).to_string(),
                ),
                // ★ 읽기 실패를 빈 표로 보여 주지 않는다 — "작업이 없다" 와 "못 읽었다" 는 다르다.
                Err(why) => respond(
                    &mut stream,
                    500,
                    "application/json; charset=utf-8",
                    &serde_json::json!({ "error": why }).to_string(),
                ),
            }
        }
        ("GET", _) => respond(&mut stream, 404, "text/plain; charset=utf-8", "없는 경로다"),
        _ => respond(
            &mut stream,
            405,
            "text/plain; charset=utf-8",
            "읽기 전용 화면이다 — GET 만 받는다",
        ),
    }
}

/// ★ 결함 285 (재검수 89) — 시한은 요청 **전체**에 건다(전에는 read 한 번마다라 1바이트씩 보내면 한 스레드인 화면이 영구히 묶였다).
///   크기 한도는 종료 표시보다 **먼저** 본다(전에는 한도를 넘긴 뒤 종료 표시가 오면 받았다).
const HEAD_DEADLINE: Duration = Duration::from_secs(5);
const HEAD_LIMIT: usize = 16 * 1024;

fn read_head(stream: &mut TcpStream) -> Result<String, String> {
    let deadline = std::time::Instant::now() + HEAD_DEADLINE;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err("요청 머리가 시한 안에 다 오지 않았다".into());
        }
        stream
            .set_read_timeout(Some(left))
            .map_err(|e| format!("시한을 걸지 못했다: {e}"))?;
        let n = stream
            .read(&mut chunk)
            .map_err(|e| format!("요청을 읽지 못했다: {e}"))?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if buffer.len() > HEAD_LIMIT {
            return Err("요청 머리가 너무 길다".into());
        }
        if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(buffer).map_err(|_| "요청 머리가 UTF-8 이 아니다".into())
}

fn host_is_loopback(host: Option<&str>) -> bool {
    let Some(host) = host else { return false };
    let name = host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host);
    matches!(name, "127.0.0.1" | "localhost")
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
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn to_json(status: &PoolStatus, now_unix_ms: u64) -> serde_json::Value {
    let jobs: Option<Vec<serde_json::Value>> = status.jobs.as_ref().map(|jobs| {
        jobs.iter()
            .map(|job| {
                serde_json::json!({
                    "job_id": job.job_id,
                    "state": job.state,
                    "requeued": job.requeued,
                    "ended": job.ended,
                    "attempt_id": job.attempt_id,
                    "attempt_state": job.attempt_state,
                    "node": job.node,
                    "resume_point": job.resume_point,
                })
            })
            .collect()
    });
    let nodes: Option<Vec<serde_json::Value>> = status.nodes.as_ref().map(|nodes| {
        nodes
            .iter()
            .map(|node| {
                serde_json::json!({
                    "node_id": node.node_id,
                    "reserved_by": node.reserved_by,
                    "reservation_expired": node.reservation_expired,
                    "last_fresh_hello_unix_ms": node.last_fresh_hello_unix_ms,
                    "owner_reclaimed": node.owner_reclaimed,
                })
            })
            .collect()
    });
    serde_json::json!({
        "now_unix_ms": now_unix_ms,
        "jobs": jobs,
        "nodes": nodes,
        "summary": status.summary,
    })
}

/// 화면 — 외부 자원을 불러오지 않는다(CDN 없음). 값은 전부 textContent 로 넣는다(HTML 로 해석하지 않는다).
const PAGE: &str = r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>gPUteer 풀</title>
<style>
:root{--bg:#f7f7f5;--fg:#1d1d1b;--muted:#6b6b66;--line:#dcdcd6;--card:#fff;--ok:#1f7a4d;--warn:#a15c00;--bad:#b3261e}
@media (prefers-color-scheme:dark){:root{--bg:#161615;--fg:#ececea;--muted:#9a9a94;--line:#33332f;--card:#1f1f1d;--ok:#5fcf98;--warn:#f0a646;--bad:#ff8a80}}
body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 system-ui,-apple-system,"Segoe UI","Malgun Gothic",sans-serif}
main{max-width:1100px;margin:0 auto;padding:20px 16px}
h1{font-size:20px;margin:0 0 4px} .sub{color:var(--muted);margin:0 0 16px}
.chips{display:flex;flex-wrap:wrap;gap:8px;margin-bottom:16px}
.chip{background:var(--card);border:1px solid var(--line);border-radius:999px;padding:4px 12px}
section{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px;margin-bottom:16px;overflow-x:auto}
h2{font-size:15px;margin:0 0 8px}
table{border-collapse:collapse;width:100%;min-width:640px} th,td{text-align:left;padding:6px 8px;border-bottom:1px solid var(--line);white-space:nowrap}
th{color:var(--muted);font-weight:600} td.mono{font-family:ui-monospace,Consolas,monospace;font-size:12px}
.RUNNING,.COMPLETED{color:var(--ok)} .STAGING,.QUEUED,.INTERRUPTED{color:var(--warn)} .FAILED{color:var(--bad)}
.yes{color:var(--bad)} #err{color:var(--bad)} .empty{color:var(--muted)}
</style></head><body><main>
<h1>gPUteer 풀</h1>
<p class="sub">읽기 전용 · 5초마다 다시 읽는다 · <span id="at">-</span></p>
<p id="err"></p>
<div class="chips" id="summary"></div>
<section><h2>작업</h2><table><thead><tr><th>Job</th><th>상태</th><th>재배치</th><th>끝난 사유</th><th>시도</th><th>시도 상태</th><th>노드</th><th>이어갈 지점</th></tr></thead><tbody id="jobs"></tbody></table></section>
<section><h2>노드</h2><table><thead><tr><th>노드</th><th>예약한 시도</th><th>예약 만료 의심</th><th>마지막 인사</th><th>소유자가 되찾음</th></tr></thead><tbody id="nodes"></tbody></table></section>
<p class="sub">살아 있다/죽었다를 판정하지 않는다 — 마지막으로 들은 시각만 보여 준다. 조치는 CLI(release-lost-node 등)와 런북 절차로 한다.</p>
</main><script>
function cell(tr,text,cls){const td=document.createElement('td');td.textContent=text==null?'-':String(text);if(cls)td.className=cls;tr.appendChild(td);}
function ago(now,at){if(at==null)return '없음';if(at>now)return '미래(시계 확인)';const s=Math.floor((now-at)/1000);return s<60?s+'초 전':s<3600?Math.floor(s/60)+'분 전':Math.floor(s/3600)+'시간 전';}
function empty(tbody,cols,text){const tr=document.createElement('tr');const td=document.createElement('td');td.colSpan=cols;td.className='empty';td.textContent=text;tr.appendChild(td);tbody.appendChild(tr);}
async function refresh(){
 try{const r=await fetch('/api/status',{cache:'no-store'});const d=await r.json();
  if(!r.ok){document.getElementById('err').textContent='읽지 못했다: '+(d.error||r.status);return;}
  document.getElementById('err').textContent='';
  document.getElementById('at').textContent=new Date(d.now_unix_ms).toLocaleString();
  const sum=document.getElementById('summary');sum.replaceChildren();
  for(const [k,v] of Object.entries(d.summary)){const s=document.createElement('span');s.className='chip';s.textContent=k+' '+v;sum.appendChild(s);}
  const jobs=document.getElementById('jobs');jobs.replaceChildren();
  if(d.jobs==null)empty(jobs,8,'Job 테이블이 없다 — 아직 제출된 작업이 없다');else if(!d.jobs.length)empty(jobs,8,'작업 없음');
  for(const j of d.jobs||[]){const tr=document.createElement('tr');cell(tr,j.job_id,'mono');cell(tr,j.state,j.state);cell(tr,j.requeued);cell(tr,j.ended);cell(tr,j.attempt_id,'mono');cell(tr,j.attempt_state,j.attempt_state);cell(tr,j.node,'mono');cell(tr,j.resume_point?'있음':'없음');jobs.appendChild(tr);}
  const nodes=document.getElementById('nodes');nodes.replaceChildren();
  if(d.nodes==null)empty(nodes,5,'노드 테이블이 없다 — import-inventory 를 먼저 한다');else if(!d.nodes.length)empty(nodes,5,'노드 없음');
  for(const n of d.nodes||[]){const tr=document.createElement('tr');cell(tr,n.node_id,'mono');cell(tr,n.reserved_by,'mono');cell(tr,n.reservation_expired==null?'-':n.reservation_expired?'예':'아니오',n.reservation_expired?'yes':'');cell(tr,ago(d.now_unix_ms,n.last_fresh_hello_unix_ms));cell(tr,n.owner_reclaimed?'예':'아니오',n.owner_reclaimed?'yes':'');nodes.appendChild(tr);}
 }catch(e){document.getElementById('err').textContent='읽지 못했다: '+e;}
}
refresh();setInterval(refresh,5000);
</script></body></html>
"#;
