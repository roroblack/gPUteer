//! `gputeer dashboard` — 풀 운영자가 보는 웹 화면(127.0.0.1 전용).
//!
//! # 왜
//!
//! `gputeer status` 는 텍스트 한 번이다. 운영자는 Job 이 어느 노드에서 어떤 상태인지, 노드가 언제 마지막으로 인사했는지,
//! 예약이 만료 의심인지를 **계속** 봐야 한다. 이 화면은 같은 사실(`gputeer_coordinator::status::pool_status`)을 5초마다 다시 읽어
//! 표로 보여 준다.
//!
//! # 쓰는 동작은 하나 — 켰을 때만
//!
//! 기본은 읽기 전용이다. `--allow-import true` 를 주면 제출자가 건넨 서명 Manifest 파일을 화면에서 올려 반입 · 계획까지 한다
//! (2026-09-25 · `docs/plans/2026-09-25_1535_제출자_운영자_웹_UI.md`). 판정은 `gputeer import-manifest` · `gputeer plan-job` 과
//! **같은 함수**가 한다 — 제출자 keyring 검증 · 영속 저장 · 자원 판정을 화면이 따로 만들지 않는다.
//!
//! # 하지 않는 것
//!
//! ```text
//! 다른 쓰기   Job 취소 · 노드 해제 버튼이 없다(CLI 와 런북 절차다: release-lost-node 등)
//! 판정        "살아 있다/죽었다" 를 말하지 않는다 — 마지막으로 들은 시각만 보여 준다(status 와 같다)
//! 외부 공개   127.0.0.1 에만 연다. 주소를 인자로 받지 않는다. Host 가 loopback 이 아니면 거부한다.
//!             쓰는 요청은 시작할 때 만든 토큰 헤더가 있어야 받는다(다른 사이트의 페이지가 브라우저로 보내는 요청을 막는다)
//! ```

use std::net::TcpStream;
use std::path::{Path, PathBuf};

use gputeer_coordinator::status::{pool_status, PoolStatus};

use crate::local_http::{self, Request};

const USAGE: &str = "gputeer dashboard --control-db <control.sqlite3> --port <로컬 포트> [--max-requests <n>] \
[--allow-import true --submitter-keyring <keyring> --submitter-member <owner id> --max-snapshot-age-ms <ms> \
[--i-understand-plaintext-keyring-is-unsafe true]]";

/// 올리는 Manifest 한도 — 서명 Manifest 는 수 KB 다. 넉넉히 1MiB.
const MANIFEST_LIMIT: usize = 1024 * 1024;

/// 반입을 켰을 때의 설정 — 전부 운영자가 시작할 때 준다.
struct ImportConfig {
    keyring: String,
    submitter_member: String,
    max_snapshot_age_ms: String,
    plaintext_keyring: bool,
    token: String,
}

pub fn run(args: &[String]) -> Result<String, String> {
    let mut control_db: Option<PathBuf> = None;
    let mut port: Option<u16> = None;
    let mut max_requests: Option<u64> = None;
    let mut allow_import = false;
    let mut keyring: Option<String> = None;
    let mut member: Option<String> = None;
    let mut max_age: Option<String> = None;
    let mut plaintext_keyring = false;
    let mut iter = args.iter();
    while let Some(key) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| format!("DASHBOARD_ARGS: {key} 에 값이 없다\n{USAGE}"))?;
        let flag = |value: &str| match value {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(format!(
                "DASHBOARD_ARGS: {key} 는 true · false 다(받은 값 {other:?})"
            )),
        };
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
            "--allow-import" => allow_import = flag(value)?,
            "--submitter-keyring" => keyring = Some(value.clone()),
            "--submitter-member" => member = Some(value.clone()),
            "--max-snapshot-age-ms" => max_age = Some(value.clone()),
            "--i-understand-plaintext-keyring-is-unsafe" => plaintext_keyring = flag(value)?,
            other => return Err(format!("DASHBOARD_ARGS: 모르는 옵션 {other}\n{USAGE}")),
        }
    }
    let (Some(control_db), Some(port)) = (control_db, port) else {
        return Err(format!(
            "DASHBOARD_ARGS: --control-db · --port 는 반드시 준다\n{USAGE}"
        ));
    };
    let import = if allow_import {
        let (Some(keyring), Some(submitter_member), Some(max_snapshot_age_ms)) =
            (keyring, member, max_age)
        else {
            return Err(format!(
                "DASHBOARD_ARGS: --allow-import true 는 --submitter-keyring · --submitter-member · --max-snapshot-age-ms 와 함께 준다\n{USAGE}"
            ));
        };
        Some(ImportConfig {
            keyring,
            submitter_member,
            max_snapshot_age_ms,
            plaintext_keyring,
            token: local_http::new_token()?,
        })
    } else {
        if keyring.is_some() || member.is_some() || max_age.is_some() || plaintext_keyring {
            return Err("DASHBOARD_ARGS: 반입 설정은 --allow-import true 와 함께만 쓴다".into());
        }
        None
    };
    let listener = local_http::bind(port).map_err(|e| format!("DASHBOARD: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    println!("DASHBOARD_LISTENING http://{address}/");
    if let Some(import) = import.as_ref() {
        println!("DASHBOARD_IMPORT_ENABLED token={}", import.token);
    }
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
        if let Err(e) = handle(stream, &control_db, import.as_ref()) {
            eprintln!("dashboard: 요청 처리 실패(계속 받는다): {e}");
        }
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn handle(
    mut stream: TcpStream,
    control_db: &Path,
    import: Option<&ImportConfig>,
) -> std::io::Result<()> {
    // ★ 결함 285 — 요청 전체 시한 · 머리 한도는 `local_http` 가 건다.
    // 반입이 꺼져 있어도 몸은 한도까지 읽는다 — 거부 사유를 "몸이 크다" 가 아니라 "반입이 꺼져 있다" 로 알린다.
    let request = match local_http::read_request(&mut stream, MANIFEST_LIMIT) {
        Ok(request) => request,
        Err(why) => return local_http::respond_text(&mut stream, 400, &why),
    };
    // ★ DNS 리바인딩 방어 — Owner Panel 과 같다.
    if !request.host_is_loopback() {
        return local_http::respond_text(
            &mut stream,
            403,
            "이 화면은 로컬에서만 본다(Host 가 loopback 이 아니다)",
        );
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => local_http::respond(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            "",
            PAGE.as_bytes(),
        ),
        ("GET", "/api/status") => {
            let now = now_unix_ms();
            match pool_status(control_db, now) {
                Ok(status) => local_http::respond_json(&mut stream, 200, &to_json(&status, now)),
                // ★ 읽기 실패를 빈 표로 보여 주지 않는다 — "작업이 없다" 와 "못 읽었다" 는 다르다.
                Err(why) => {
                    local_http::respond_json(&mut stream, 500, &serde_json::json!({ "error": why }))
                }
            }
        }
        // 화면이 반입 칸을 보일지 · 쓸 토큰. 같은 출처의 페이지만 이 응답을 읽을 수 있다.
        ("GET", "/api/config") => local_http::respond_json(
            &mut stream,
            200,
            &serde_json::json!({
                "import_enabled": import.is_some(),
                "token": import.map(|i| i.token.as_str()),
            }),
        ),
        ("POST", "/api/import") => match import {
            None => local_http::respond_text(
                &mut stream,
                403,
                "반입이 꺼져 있다(--allow-import true 로 켠다)",
            ),
            Some(import) if !request.token_matches(&import.token) => local_http::respond_text(
                &mut stream,
                403,
                "토큰이 없거나 다르다 — 이 화면에서만 반입할 수 있다",
            ),
            Some(import) => {
                let (status, body) = import_manifest_bytes(&request, control_db, import);
                local_http::respond_json(&mut stream, status, &body)
            }
        },
        ("GET", _) => local_http::respond_text(&mut stream, 404, "없는 경로다"),
        _ => local_http::respond_text(&mut stream, 405, "받지 않는 요청이다"),
    }
}

/// 올린 Manifest 를 `import-manifest` · `plan-job` 과 **같은 함수**로 반입 · 계획한다.
///
/// ★ 멱등 키는 올린 바이트의 BLAKE3 앞 16바이트다 — 같은 파일을 두 번 올리면 같은 반입이다(두 번 반입되지 않는다).
fn import_manifest_bytes(
    request: &Request,
    control_db: &Path,
    import: &ImportConfig,
) -> (u16, serde_json::Value) {
    if request.body.is_empty() {
        return (
            400,
            serde_json::json!({ "ok": false, "error": "올린 파일이 비었다" }),
        );
    }
    let digest = gputeer_protocol::canonical::blake3_256(&request.body);
    let key: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    // 반입 함수는 파일 경로를 받는다 — 임시 파일에 쓰고 끝나면 지운다(이름은 내용 해시 · 프로세스 id).
    let temp = std::env::temp_dir().join(format!(
        "gputeer-dashboard-{}-{}.manifest",
        std::process::id(),
        digest[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    if let Err(e) = std::fs::write(&temp, &request.body) {
        return (
            500,
            serde_json::json!({ "ok": false, "error": format!("임시 파일을 쓰지 못했다: {e}") }),
        );
    }
    let db = control_db.to_string_lossy().to_string();
    let temp_s = temp.to_string_lossy().to_string();
    let mut import_args = vec![
        "--manifest".to_string(),
        temp_s,
        "--submitter-keyring".to_string(),
        import.keyring.clone(),
        "--job-db".to_string(),
        db.clone(),
        "--idempotency-key".to_string(),
        key,
    ];
    if import.plaintext_keyring {
        import_args.push("--i-understand-plaintext-keyring-is-unsafe".into());
        import_args.push("true".into());
    }
    let imported = crate::import_manifest::run(&import_args);
    let _ = std::fs::remove_file(&temp);
    let imported = match imported {
        Ok(message) => message,
        Err(error) => {
            return (
                400,
                serde_json::json!({ "ok": false, "stage": "import", "error": error }),
            )
        }
    };
    let Some(job_id) = imported
        .split_whitespace()
        .find_map(|word| word.strip_prefix("job_id="))
        .map(str::to_string)
    else {
        return (
            500,
            serde_json::json!({ "ok": false, "stage": "import", "error": format!("반입 결과에서 job_id 를 읽지 못했다: {imported}") }),
        );
    };
    let mut plan_args = vec![
        "--job-id".to_string(),
        job_id.clone(),
        "--control-db".to_string(),
        db,
        "--submitter-keyring".to_string(),
        import.keyring.clone(),
        "--submitter-member".to_string(),
        import.submitter_member.clone(),
        "--max-snapshot-age-ms".to_string(),
        import.max_snapshot_age_ms.clone(),
    ];
    if import.plaintext_keyring {
        plan_args.push("--i-understand-plaintext-keyring-is-unsafe".into());
        plan_args.push("true".into());
    }
    match crate::plan_job::run(&plan_args) {
        Ok(planned) => (
            200,
            serde_json::json!({ "ok": true, "job_id": job_id, "imported": imported, "planned": planned }),
        ),
        // 반입은 됐다 — 계획만 실패했다(예: 맞는 노드가 없다). 둘을 가른다.
        Err(error) => (
            400,
            serde_json::json!({ "ok": false, "stage": "plan", "job_id": job_id, "imported": imported, "error": error }),
        ),
    }
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
button{font:inherit;padding:4px 12px;border:1px solid var(--line);border-radius:6px;background:var(--card);color:var(--fg);cursor:pointer}
pre{white-space:pre-wrap;word-break:break-all;font-family:ui-monospace,Consolas,monospace;font-size:12px}
</style></head><body><main>
<h1>gPUteer 풀</h1>
<p class="sub">읽기 전용 · 5초마다 다시 읽는다 · <span id="at">-</span></p>
<p id="err"></p>
<div class="chips" id="summary"></div>
<section id="import" hidden><h2>작업 올리기</h2>
<p class="sub">제출자가 건넨 서명 Manifest 파일을 올리면 제출자 keyring 으로 검증하고 반입 · 계획까지 한다(gputeer import-manifest · plan-job 과 같은 판정).</p>
<input type="file" id="file"> <button id="send" type="button">올리기</button>
<pre id="result"></pre></section>
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
let token=null;
async function config(){
 try{const r=await fetch('/api/config',{cache:'no-store'});const c=await r.json();
  if(c.import_enabled){token=c.token;document.getElementById('import').hidden=false;}}catch(e){}
}
document.getElementById('send').addEventListener('click',async()=>{
 const out=document.getElementById('result');const f=document.getElementById('file').files[0];
 if(!f){out.textContent='파일을 고른다';return;}
 out.textContent='올리는 중…';
 try{const r=await fetch('/api/import',{method:'POST',headers:{'X-Gputeer-Token':token,'Content-Type':'application/octet-stream'},body:await f.arrayBuffer()});
  const t=await r.text();let d;try{d=JSON.parse(t);}catch(e){d=null;}
  out.textContent=d?(d.ok?'반입 · 계획 완료\n'+d.imported+'\n'+d.planned:'실패('+(d.stage||'?')+'): '+d.error+(d.imported?'\n'+d.imported:'')):t;
  refresh();
 }catch(e){out.textContent='실패: '+e;}
});
config();refresh();setInterval(refresh,5000);
</script></body></html>
"#;
