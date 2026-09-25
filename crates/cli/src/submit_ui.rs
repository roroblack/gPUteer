//! `gputeer submit-ui` — 제출자가 **자기 기계의 127.0.0.1** 에서 폼으로 작업을 내는 화면.
//!
//! # 무엇을 하나
//!
//! 폼 → `gputeer submit` 과 **같은 함수**(`submit::run`)로 서명 Manifest 를 만들어 `--out-dir` 에 남기고 내려받게 한다.
//! 그 파일을 운영자에게 건네면 운영자는 대시보드(`gputeer dashboard --allow-import true`)에서 올린다
//! (`docs/plans/2026-09-25_1535_제출자_운영자_웹_UI.md`).
//!
//! # 지키는 것
//!
//! ```text
//! 서명 시드   시작할 때 파일(--submitter-seed-file)로만 받는다 — 브라우저로 가지 않고 화면에 나오지 않는다
//! 판정        값 검사 · 서명 · 자기 검증은 `submit` 이 한다 — 화면이 규칙을 따로 만들지 않는다
//! 로컬 전용   127.0.0.1 · Host 가 loopback 이 아니면 거부 · 만드는 요청(POST)은 시작할 때 만든 토큰 헤더가 있어야 받는다
//! 파일        Job id 는 파일 이름이 된다 — 영숫자 · `.` `_` `-` 만 받는다. 이미 있는 파일은 덮지 않는다(`submit` 의 규칙)
//! ```

use std::net::TcpStream;
use std::path::{Path, PathBuf};

use crate::local_http::{self, Request};

const USAGE: &str = "gputeer submit-ui --submitter-device-id <id> --submitter-seed-file <시드 파일> --out-dir <폴더> --port <로컬 포트> [--max-requests <n>]";

/// 폼 몸 한도.
const FORM_LIMIT: usize = 64 * 1024;

struct Config {
    device_id: String,
    seed_hex: String,
    out_dir: PathBuf,
    token: String,
}

pub fn run(args: &[String]) -> Result<String, String> {
    let mut device_id: Option<String> = None;
    let mut seed_file: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut port: Option<u16> = None;
    let mut max_requests: Option<u64> = None;
    let mut iter = args.iter();
    while let Some(key) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| format!("SUBMIT_UI_ARGS: {key} 에 값이 없다\n{USAGE}"))?;
        match key.as_str() {
            "--submitter-device-id" => device_id = Some(value.clone()),
            "--submitter-seed-file" => seed_file = Some(value.into()),
            "--out-dir" => out_dir = Some(value.into()),
            "--port" => {
                port = Some(
                    value
                        .parse()
                        .map_err(|e| format!("SUBMIT_UI_ARGS: --port 파싱 실패({value:?}): {e}"))?,
                )
            }
            "--max-requests" => {
                max_requests = Some(value.parse().map_err(|e| {
                    format!("SUBMIT_UI_ARGS: --max-requests 파싱 실패({value:?}): {e}")
                })?)
            }
            other => return Err(format!("SUBMIT_UI_ARGS: 모르는 옵션 {other}\n{USAGE}")),
        }
    }
    let (Some(device_id), Some(seed_file), Some(out_dir), Some(port)) =
        (device_id, seed_file, out_dir, port)
    else {
        return Err(format!(
            "SUBMIT_UI_ARGS: --submitter-device-id · --submitter-seed-file · --out-dir · --port 는 반드시 준다\n{USAGE}"
        ));
    };
    let seed_hex = std::fs::read_to_string(&seed_file)
        .map_err(|e| format!("SUBMIT_UI: 시드 파일을 읽지 못했다({seed_file:?}): {e}"))?
        .trim()
        .to_string();
    if seed_hex.len() != 64 || !seed_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "SUBMIT_UI: {seed_file:?} 가 32바이트 16진수 시드가 아니다(gputeer keygen 으로 만든 파일)"
        ));
    }
    if !out_dir.is_dir() {
        return Err(format!(
            "SUBMIT_UI: --out-dir {out_dir:?} 가 없다 — 먼저 만든다"
        ));
    }
    let config = Config {
        device_id,
        seed_hex,
        out_dir,
        token: local_http::new_token()?,
    };
    let listener = local_http::bind(port).map_err(|e| format!("SUBMIT_UI: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    println!("SUBMIT_UI_LISTENING http://{address}/");
    // ★ 결함 298 (재검수 93) — 토큰은 이 주소로만 준다(`#` 뒤는 서버로 가지 않는다). 이 주소를 연다.
    println!("SUBMIT_UI_OPEN http://{address}/#token={}", config.token);
    let served = local_http::serve(listener, max_requests, move |stream| {
        handle(stream, &config)
    })?;
    Ok(format!("SUBMIT_UI_DONE served={served}"))
}

fn handle(mut stream: TcpStream, config: &Config) -> std::io::Result<()> {
    let request = match local_http::read_request(&mut stream, FORM_LIMIT) {
        Ok(request) => request,
        Err(why) => return local_http::respond_text(&mut stream, 400, &why),
    };
    if !request.host_is_loopback() {
        return local_http::respond_text(
            &mut stream,
            403,
            "이 화면은 로컬에서만 쓴다(Host 가 loopback 이 아니다)",
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
        ("GET", "/api/config") => local_http::respond_json(
            &mut stream,
            200,
            &serde_json::json!({ "submitter_device_id": config.device_id }),
        ),
        ("POST", "/api/submit") => {
            if !request.token_matches(&config.token) {
                return local_http::respond_text(
                    &mut stream,
                    403,
                    "토큰이 없거나 다르다 — 이 화면에서만 낼 수 있다",
                );
            }
            let (status, body) = submit_from_form(&request, config);
            local_http::respond_json(&mut stream, status, &body)
        }
        ("GET", path) if path.starts_with("/manifest/") => {
            let job_id = &path["/manifest/".len()..];
            if check_job_id(job_id).is_err() {
                return local_http::respond_text(&mut stream, 404, "없는 파일이다");
            }
            match std::fs::read(manifest_path(&config.out_dir, job_id)) {
                Ok(bytes) => local_http::respond(
                    &mut stream,
                    200,
                    "application/octet-stream",
                    &format!("Content-Disposition: attachment; filename=\"{job_id}.manifest\"\r\n"),
                    &bytes,
                ),
                Err(_) => local_http::respond_text(&mut stream, 404, "없는 파일이다"),
            }
        }
        ("GET", _) => local_http::respond_text(&mut stream, 404, "없는 경로다"),
        _ => local_http::respond_text(&mut stream, 405, "받지 않는 요청이다"),
    }
}

fn manifest_path(out_dir: &Path, job_id: &str) -> PathBuf {
    out_dir.join(format!("{job_id}.manifest"))
}

/// Job id 는 파일 이름이 된다 — 경로를 바꿀 수 있는 문자를 받지 않는다.
fn check_job_id(job_id: &str) -> Result<(), String> {
    if job_id.is_empty() || job_id.len() > 128 {
        return Err("Job id 는 1~128자다".into());
    }
    if job_id.starts_with('.') {
        return Err("Job id 는 '.' 로 시작하지 않는다".into());
    }
    if !job_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(format!(
            "Job id {job_id:?} 에 쓸 수 없는 문자가 있다(영숫자 · . _ - 만)"
        ));
    }
    Ok(())
}

/// 폼 몸(JSON)을 `submit` 의 인자로 옮겨 그대로 부른다.
fn submit_from_form(request: &Request, config: &Config) -> (u16, serde_json::Value) {
    let fail =
        |status: u16, error: String| (status, serde_json::json!({ "ok": false, "error": error }));
    let form: serde_json::Value = match serde_json::from_slice(&request.body) {
        Ok(value) => value,
        Err(e) => return fail(400, format!("폼을 JSON 으로 읽지 못했다: {e}")),
    };
    let text = |key: &str| -> Option<String> {
        match form.get(key) {
            Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {
                Some(s.trim().to_string())
            }
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    };
    // ★ 결함 299 (재검수 93) — Job id 는 **화면이** 폼을 열 때 한 번 만든다. 서버가 요청마다 지어내면 응답을 잃고 다시 누를 때 두 번째 작업이
    //   생겼다. 같은 id 로 다시 오면 이미 만든 파일이라 거부하고 내려받기를 알려 준다.
    let Some(job_id) = text("job_id") else {
        return fail(400, "Job id 가 비었다 — 화면이 만든 값을 쓴다".into());
    };
    if let Err(why) = check_job_id(&job_id) {
        return fail(400, why);
    }
    if manifest_path(&config.out_dir, &job_id).exists() {
        return (
            409,
            serde_json::json!({
                "ok": false,
                "exists": true,
                "job_id": job_id,
                "download": format!("/manifest/{job_id}"),
                "error": "이 Job id 로 이미 만들었다 — 다시 만들지 않는다(두 번 도는 것을 막는다). 이미 만든 파일을 내려받는다",
            }),
        );
    }
    let Some(entrypoint) = text("entrypoint") else {
        return fail(400, "실행할 것(entrypoint)이 비었다".into());
    };
    let args: Vec<String> = match form.get("args") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(arg) => out.push(arg.to_string()),
                    None => return fail(400, "인자(args)는 글자 목록이다".into()),
                }
            }
            out
        }
        Some(_) => return fail(400, "인자(args)는 글자 목록이다".into()),
    };
    // ★ `submit --args` 는 쉼표로 나눈다 — 인자 안의 쉼표는 표현할 수 없다. 조용히 쪼개지 않고 거부한다.
    if let Some(bad) = args.iter().find(|a| a.contains(',')) {
        return fail(
            400,
            format!("인자 {bad:?} 에 쉼표가 있다 — 지금 제출 형식(submit --args)은 인자 안의 쉼표를 담지 못한다"),
        );
    }
    let path = manifest_path(&config.out_dir, &job_id);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut submit_args: Vec<String> = vec![
        "--job-id".into(),
        job_id.clone(),
        "--entrypoint".into(),
        entrypoint,
        "--submitter-device-id".into(),
        config.device_id.clone(),
        "--submitter-seed".into(),
        config.seed_hex.clone(),
        "--issued-at-unix-ms".into(),
        now.to_string(),
        "--out".into(),
        path.to_string_lossy().to_string(),
    ];
    if !args.is_empty() {
        submit_args.push("--args".into());
        submit_args.push(args.join(","));
    }
    for (key, flag) in [
        ("image_ref", "--image-ref"),
        ("image_sha256", "--image-sha256"),
        ("workload_class", "--workload-class"),
        ("side_effect_class", "--side-effect-class"),
        ("durability", "--durability"),
        ("dataset_sensitivity", "--dataset-sensitivity"),
        ("minimum_security_tier", "--minimum-security-tier"),
        ("minimum_isolation_class", "--minimum-isolation-class"),
        ("minimum_key_protection", "--minimum-key-protection"),
        ("gpu_count", "--gpu-count"),
        ("gpu_min_vram_bytes", "--gpu-min-vram-bytes"),
        ("cpu_cores", "--cpu-cores"),
        ("ram_bytes", "--ram-bytes"),
        ("workspace_bytes", "--workspace-bytes"),
        ("expires_at_unix_ms", "--expires-at-unix-ms"),
    ] {
        if let Some(value) = text(key) {
            submit_args.push(flag.into());
            submit_args.push(value);
        }
    }
    match crate::submit::run(&submit_args) {
        Ok(message) => (
            200,
            serde_json::json!({
                "ok": true,
                "job_id": job_id,
                "file": path.to_string_lossy(),
                "download": format!("/manifest/{job_id}"),
                "message": message,
            }),
        ),
        // ★ 결함 406 (재검수 94) — 같은 id 두 요청이 동시에 존재 확인을 지나면 쓰기는 하나만 이기고(원자적 생성) 진 쪽은 OUT_EXISTS 다.
        //   그 쪽도 409 · 내려받기로 알린다(두 번째 작업은 생기지 않았다).
        Err(error) if error.contains("OUT_EXISTS") => (
            409,
            serde_json::json!({
                "ok": false,
                "exists": true,
                "job_id": job_id,
                "download": format!("/manifest/{job_id}"),
                "error": "이 Job id 로 이미 만들었다 — 다시 만들지 않는다(두 번 도는 것을 막는다). 이미 만든 파일을 내려받는다",
            }),
        ),
        Err(error) => fail(400, error),
    }
}

/// 화면 — 외부 자원 없음. 결과는 textContent 로만 넣는다.
const PAGE: &str = r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>gPUteer 작업 내기</title>
<style>
:root{--bg:#f7f7f5;--fg:#1d1d1b;--muted:#6b6b66;--line:#dcdcd6;--card:#fff;--ok:#1f7a4d;--bad:#b3261e}
@media (prefers-color-scheme:dark){:root{--bg:#161615;--fg:#ececea;--muted:#9a9a94;--line:#33332f;--card:#1f1f1d;--ok:#5fcf98;--bad:#ff8a80}}
body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 system-ui,-apple-system,"Segoe UI","Malgun Gothic",sans-serif}
main{max-width:860px;margin:0 auto;padding:20px 16px}
h1{font-size:20px;margin:0 0 4px} .sub{color:var(--muted);margin:0 0 16px}
section{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px;margin-bottom:16px}
h2{font-size:15px;margin:0 0 8px}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:10px 14px}
label{display:flex;flex-direction:column;gap:4px;color:var(--muted);font-size:13px}
input,select,textarea{font:inherit;color:var(--fg);background:var(--bg);border:1px solid var(--line);border-radius:6px;padding:6px 8px}
textarea{min-height:64px;font-family:ui-monospace,Consolas,monospace}
button{font:inherit;padding:6px 16px;border:1px solid var(--line);border-radius:6px;background:var(--fg);color:var(--bg);cursor:pointer}
pre{white-space:pre-wrap;word-break:break-all;font-family:ui-monospace,Consolas,monospace;font-size:12px}
.ok{color:var(--ok)} .bad{color:var(--bad)}
</style></head><body><main>
<h1>gPUteer 작업 내기</h1>
<p class="sub">제출자 <span id="who">-</span> 의 키로 서명한 작업 파일을 만든다. 만든 파일을 운영자에게 건네면 운영자 화면에서 올린다.</p>
<form id="f">
<section><h2>무엇을 돌리나</h2><div class="grid">
<label>Job id (화면이 만든다 · 바꿔도 된다)<input name="job_id" required></label>
<label>실행할 것(entrypoint)<input name="entrypoint" required placeholder="python 또는 /usr/bin/python3"></label>
<label style="grid-column:1/-1">인자 — 한 줄에 하나(쉼표는 쓸 수 없다)<textarea name="args" placeholder="train.py&#10;--epochs=3"></textarea></label>
</div></section>
<section><h2>컨테이너 (선택)</h2><p class="sub">이미지를 주면 컨테이너 노드에서만 돈다 — 두 칸을 함께 준다. digest 는 docker image inspect --format '{{index .RepoDigests 0}}' 로 읽는다.</p><div class="grid">
<label>이미지 참조<input name="image_ref" placeholder="registry.local/team/train:v1"></label>
<label>sha256 (64자리)<input name="image_sha256" placeholder="…"></label>
</div></section>
<section><h2>자원</h2><div class="grid">
<label>GPU 개수<input name="gpu_count" type="number" min="0" value="1"></label>
<label>GPU 최소 VRAM (GiB)<input name="gpu_min_vram_gib" type="number" min="0" step="0.5" value="8"></label>
<label>CPU 코어<input name="cpu_cores" type="number" min="1" value="4"></label>
<label>RAM (GiB)<input name="ram_gib" type="number" min="1" value="8"></label>
<label>작업 디스크 (GiB)<input name="workspace_gib" type="number" min="1" value="10"></label>
</div></section>
<section><h2>분류 · 보안</h2><div class="grid">
<label>작업 종류<select name="workload_class"><option>TRAINING</option><option>INFERENCE</option><option>PREPROCESSING</option><option>EVALUATION</option><option>RENDERING</option><option>OTHER</option></select></label>
<label>부작용<select name="side_effect_class"><option>PURE</option><option>IDEMPOTENT</option><option>SIDE_EFFECTING</option></select></label>
<label>산출물 내구성<select name="durability"><option>LOCAL</option><option>MIRRORED</option><option>REPLICATED</option></select></label>
<label>데이터 민감도<select name="dataset_sensitivity"><option>INTERNAL</option><option>PUBLIC</option><option>SENSITIVE</option></select></label>
<label>최소 보안 등급<select name="minimum_security_tier"><option>S1</option><option>S0</option><option>S2</option><option>S3</option><option>S4</option><option>S5</option></select></label>
<label>최소 격리<select name="minimum_isolation_class"><option>RESTRICTED</option><option>CONTAINED</option><option>VIRTUALIZED</option></select></label>
<label>최소 키 보호<select name="minimum_key_protection"><option>K1</option><option>K0</option><option>K2</option></select></label>
</div><p class="sub">★ 신뢰망은 복제를 하지 않는다 — LOCAL 이 아니면 끝나도 노드 예약이 풀리지 않는다. 컨테이너가 아닌 작업을 CONTAINED 로 내면 호스트 실행 노드(RESTRICTED)에는 배치되지 않는다.</p></section>
<button type="submit">서명해 만들기</button>
</form>
<section><h2>결과</h2><pre id="result">아직 없다</pre><p><a id="download" hidden>작업 파일 내려받기</a></p></section>
</main><script>
function pageToken(){
 // ★ 결함 298 — 토큰은 HTTP 로 받지 않는다. 시작할 때 찍힌 주소의 # 뒤(서버로 가지 않는다)에서 읽어 이 탭에만 둔다.
 const fromHash=new URLSearchParams(location.hash.slice(1)).get('token');
 try{if(fromHash){sessionStorage.setItem('gputeer-token',fromHash);history.replaceState(null,'',location.pathname);}
  return fromHash||sessionStorage.getItem('gputeer-token');}catch(e){return fromHash;}
}
const token=pageToken();
function newJobId(){const b=new Uint8Array(4);crypto.getRandomValues(b);return 'job-'+Date.now()+'-'+Array.from(b,x=>x.toString(16).padStart(2,'0')).join('');}
document.querySelector('input[name=job_id]').value=newJobId();
if(!token)document.getElementById('result').textContent='만들려면 시작할 때 찍힌 주소(#token=… 이 붙은 것)로 연다';
(async()=>{try{const r=await fetch('/api/config',{cache:'no-store'});const c=await r.json();document.getElementById('who').textContent=c.submitter_device_id;}catch(e){document.getElementById('result').textContent='설정을 읽지 못했다: '+e;}})();
const GiB=1024*1024*1024;
document.getElementById('f').addEventListener('submit',async(ev)=>{
 ev.preventDefault();
 const f=new FormData(ev.target);const v=k=>(f.get(k)||'').toString().trim();
 const bytes=k=>v(k)===''?'':String(Math.round(parseFloat(v(k))*GiB));
 const body={job_id:v('job_id'),entrypoint:v('entrypoint'),
  args:v('args')===''?[]:v('args').split('\n').map(s=>s.replace(/\r$/,'')).filter(s=>s!==''),
  image_ref:v('image_ref'),image_sha256:v('image_sha256'),
  gpu_count:v('gpu_count'),gpu_min_vram_bytes:bytes('gpu_min_vram_gib'),cpu_cores:v('cpu_cores'),
  ram_bytes:bytes('ram_gib'),workspace_bytes:bytes('workspace_gib'),
  workload_class:v('workload_class'),side_effect_class:v('side_effect_class'),durability:v('durability'),
  dataset_sensitivity:v('dataset_sensitivity'),minimum_security_tier:v('minimum_security_tier'),
  minimum_isolation_class:v('minimum_isolation_class'),minimum_key_protection:v('minimum_key_protection')};
 const out=document.getElementById('result');const dl=document.getElementById('download');dl.hidden=true;
 const button=ev.target.querySelector('button[type=submit]');button.disabled=true;
 out.className='';out.textContent='만드는 중…';
 try{const r=await fetch('/api/submit',{method:'POST',headers:{'X-Gputeer-Token':token,'Content-Type':'application/json'},body:JSON.stringify(body)});
  const t=await r.text();let d=null;try{d=JSON.parse(t);}catch(e){}
  if(d&&d.ok){out.className='ok';out.textContent='만들었다 — '+d.job_id+'\n'+d.file+'\n'+d.message;dl.href=d.download;dl.hidden=false;
   document.querySelector('input[name=job_id]').value=newJobId();}
  else{out.className='bad';out.textContent='만들지 못했다: '+(d?d.error:t);if(d&&d.exists){dl.href=d.download;dl.hidden=false;}}
 }catch(e){out.className='bad';out.textContent='만들지 못했다: '+e;}
 finally{button.disabled=false;}
});
</script></body></html>
"#;
