//! 제출자 화면(`gputeer submit-ui`) → 운영자 화면(`gputeer dashboard --allow-import true`)을 끝에서 끝까지 잇는다.
//!
//! ```text
//! 폼 JSON 으로 서명 Manifest 를 만든다 → 그 파일을 내려받는다 → 운영자 화면에 올린다 → control DB 의 Job 이 QUEUED 다
//! 음성: 토큰 없음 · Host 가 loopback 아님 · 반입이 꺼진 화면 · 경로를 바꾸는 Job id · 인자 안의 쉼표
//! ```

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use gputeer_coordinator::job_store::{CoordinatorJobStore, JobState};

const NODE: &str = "01JWEBUINODE0000000000001";
const OWNER: &str = "owner-web";
const SUBMITTER: &str = "01JWEBUISUBMITTER00000001";
const NODE_SEED: [u8; 32] = [0x5a; 32];

fn cli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gputeer"))
}

fn run_cli(args: &[&str]) -> (bool, String) {
    let out = Command::new(cli_bin())
        .args(args)
        .output()
        .expect("gputeer");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// 노드 하나를 등록한 control DB 와, 제출자 공개키를 넣은 keyring 을 만든다. 제출자 시드 파일 경로도 돌려준다.
fn party(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let db = dir.join("control.sqlite3");
    let node_key = hex(&gputeer_crypto::SigningKey::from_bytes(&NODE_SEED)
        .verifying_key()
        .to_bytes());
    let bootstrap = dir.join("bootstrap.json");
    std::fs::write(
        &bootstrap,
        format!(
            r#"{{"schema_version": 1, "agents": [{{
  "registry": {{"node_id": "{NODE}", "device_id": "{NODE}", "owner_member_id": "{OWNER}", "verifying_key_hex": "{node_key}",
    "node_state": "ONLINE", "risk_state": "NORMAL", "security_tier": "S2", "isolation_class": "RESTRICTED", "key_protection": "K1"}},
  "inventory": {{"inventory_revision": 1, "observed_at_unix_ms": {observed},
    "gpus": [{{"gpu_id": "{NODE}-gpu-0", "model": "RTX 4070 SUPER", "healthy": true, "available_vram_bytes": 12884901888}}],
    "available_cpu_cores": 16, "available_ram_bytes": 34359738368, "available_workspace_bytes": 107374182400,
    "allowed_workload_classes": ["TRAINING"], "third_party_workloads_opt_in": true}}}}]}}"#,
            observed = now_unix_ms()
        ),
    )
    .unwrap();
    let (ok, out) = run_cli(&[
        "import-inventory",
        "--inventory",
        bootstrap.to_str().unwrap(),
        "--inventory-db",
        db.to_str().unwrap(),
    ]);
    assert!(ok, "import-inventory: {out}");

    let seed = dir.join("submitter.seed");
    let (ok, out) = run_cli(&["keygen", "--out", seed.to_str().unwrap()]);
    assert!(ok, "keygen: {out}");
    let seed_hex = std::fs::read_to_string(&seed).unwrap();
    let mut seed_bytes = [0u8; 32];
    for (i, byte) in seed_bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&seed_hex.trim()[i * 2..i * 2 + 2], 16).unwrap();
    }
    let keyring = dir.join("submitters.keyring");
    let mut ring = gputeer_crypto::PersistentKeyring::new(
        &keyring,
        gputeer_crypto::KeyProtection::K0Plaintext,
        gputeer_crypto::PlaintextPolicy::Allow,
    )
    .unwrap();
    ring.insert_public(
        SUBMITTER,
        gputeer_crypto::SigningKey::from_bytes(&seed_bytes).verifying_key(),
    )
    .unwrap();
    ring.save().unwrap();
    (db, keyring, seed)
}

/// 화면을 띄우고 첫 줄들에서 주소와 토큰(있으면)을 읽는다.
fn spawn_ui(args: &[&str], token_prefix: Option<&str>) -> (Child, String, Option<String>) {
    let mut child = Command::new(cli_bin())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ui");
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let address = first
        .trim()
        .split("http://")
        .nth(1)
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap_or_else(|| panic!("주소 줄이 아니다: {first:?}"))
        .to_string();
    let token = token_prefix.map(|prefix| {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with(prefix), "토큰 주소 줄이 아니다: {line:?}");
        line.trim()
            .split("#token=")
            .nth(1)
            .unwrap_or_else(|| panic!("주소에 토큰이 없다: {line:?}"))
            .to_string()
    });
    // 나머지 출력은 버리지 않고 빨아낸다(파이프가 차서 멈추지 않게).
    std::thread::spawn(move || {
        let mut rest = String::new();
        let _ = reader.read_to_string(&mut rest);
    });
    (child, address, token)
}

/// 요청 하나 — (상태 코드, 몸).
fn http(
    address: &str,
    method: &str,
    path: &str,
    host: &str,
    token: Option<&str>,
    content_type: &str,
    body: &[u8],
) -> (u16, Vec<u8>) {
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    let token_header = token
        .map(|t| format!("X-Gputeer-Token: {t}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{token_header}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("응답 머리");
    let status: u16 = String::from_utf8_lossy(&response[..split])
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (status, response[split + 4..].to_vec())
}

fn json(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body)
        .unwrap_or_else(|e| panic!("JSON 이 아니다({e}): {}", String::from_utf8_lossy(body)))
}

#[test]
fn a_job_made_in_the_submit_screen_is_queued_from_the_operator_screen() {
    let dir = tempfile::tempdir().unwrap();
    let (db, keyring, seed) = party(dir.path());
    let out_dir = dir.path().join("manifests");
    std::fs::create_dir_all(&out_dir).unwrap();

    // ── 제출자 화면 ──
    let (mut submit_ui, address, token) = spawn_ui(
        &[
            "submit-ui",
            "--submitter-device-id",
            SUBMITTER,
            "--submitter-seed-file",
            seed.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--port",
            "0",
            "--max-requests",
            "9",
        ],
        Some("SUBMIT_UI_OPEN"),
    );
    let token = token.unwrap();
    let form = serde_json::json!({
        "job_id": "web-job-1", "entrypoint": "python", "args": ["train.py", "--epochs=3"],
        "gpu_count": "1", "gpu_min_vram_bytes": "8589934592", "cpu_cores": "4", "ram_bytes": "8589934592",
        "workspace_bytes": "10737418240", "workload_class": "TRAINING", "side_effect_class": "PURE",
        "durability": "LOCAL", "dataset_sensitivity": "INTERNAL", "minimum_security_tier": "S1",
        "minimum_isolation_class": "RESTRICTED", "minimum_key_protection": "K1"
    })
    .to_string();
    // 1 음성 — 토큰 없음.
    let (status, _) = http(
        &address,
        "POST",
        "/api/submit",
        "127.0.0.1",
        None,
        "application/json",
        form.as_bytes(),
    );
    assert_eq!(status, 403, "토큰 없이 만들었다");
    // 2 음성 — Host 가 loopback 이 아니다(DNS 리바인딩).
    let (status, _) = http(
        &address,
        "POST",
        "/api/submit",
        "attacker.example",
        Some(&token),
        "application/json",
        form.as_bytes(),
    );
    assert_eq!(status, 403, "다른 Host 로 만들었다");
    // 3 음성 — 경로를 바꾸는 Job id.
    let bad = form.replace("web-job-1", "../escape");
    let (status, body) = http(
        &address,
        "POST",
        "/api/submit",
        "127.0.0.1",
        Some(&token),
        "application/json",
        bad.as_bytes(),
    );
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
    assert!(!dir.path().join("escape.manifest").exists());
    // 4 양성.
    let (status, body) = http(
        &address,
        "POST",
        "/api/submit",
        "127.0.0.1",
        Some(&token),
        "application/json",
        form.as_bytes(),
    );
    let made = json(&body);
    assert_eq!(status, 200, "{made}");
    assert_eq!(made["ok"], true, "{made}");
    let file = out_dir.join("web-job-1.manifest");
    assert!(file.is_file(), "파일을 남기지 않았다: {made}");
    // 5 내려받기 — 남긴 파일과 같은 바이트.
    let (status, downloaded) = http(
        &address,
        "GET",
        "/manifest/web-job-1",
        "127.0.0.1",
        None,
        "text/plain",
        b"",
    );
    assert_eq!(status, 200);
    assert_eq!(
        downloaded,
        std::fs::read(&file).unwrap(),
        "내려받은 것이 남긴 파일과 다르다"
    );
    // 7 음성(결함 299) — 같은 Job id 로 다시 보내면 두 번째 작업을 만들지 않는다(응답을 잃고 다시 누른 경우).
    let before = std::fs::read(&file).unwrap();
    let (status, body) = http(
        &address,
        "POST",
        "/api/submit",
        "127.0.0.1",
        Some(&token),
        "application/json",
        form.as_bytes(),
    );
    assert_eq!(status, 409, "{}", String::from_utf8_lossy(&body));
    assert_eq!(json(&body)["download"], "/manifest/web-job-1");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "이미 만든 파일을 바꿨다"
    );
    // 8 음성(결함 299) — Job id 가 없으면 서버가 지어내지 않는다.
    let no_id = form.replace("\"job_id\":\"web-job-1\",", "");
    assert!(!no_id.contains("job_id"), "시험 전제: job_id 를 뺐다");
    let (status, _) = http(
        &address,
        "POST",
        "/api/submit",
        "127.0.0.1",
        Some(&token),
        "application/json",
        no_id.as_bytes(),
    );
    assert_eq!(status, 400, "Job id 없이 만들었다");
    // 9 (결함 298) — 설정 응답은 토큰을 내지 않는다(같은 PC 의 다른 프로세스가 HTTP 로 얻지 못한다).
    let (status, body) = http(
        &address,
        "GET",
        "/api/config",
        "127.0.0.1",
        None,
        "text/plain",
        b"",
    );
    assert_eq!(status, 200);
    assert!(
        !String::from_utf8_lossy(&body).contains(&token),
        "설정 응답이 토큰을 냈다"
    );
    // 6 음성 — 인자 안의 쉼표는 조용히 쪼개지 않고 거부한다.
    let comma = form
        .replace("web-job-1", "web-job-2")
        .replace("--epochs=3", "a,b");
    let (status, body) = http(
        &address,
        "POST",
        "/api/submit",
        "127.0.0.1",
        Some(&token),
        "application/json",
        comma.as_bytes(),
    );
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
    assert!(!out_dir.join("web-job-2.manifest").exists());
    let _ = submit_ui.wait();

    // ── 반입이 꺼진 운영자 화면 — 올리기를 받지 않는다 ──
    let manifest = std::fs::read(&file).unwrap();
    let (mut read_only, address, _) = spawn_ui(
        &[
            "dashboard",
            "--control-db",
            db.to_str().unwrap(),
            "--port",
            "0",
            "--max-requests",
            "1",
        ],
        None,
    );
    let (status, _) = http(
        &address,
        "POST",
        "/api/import",
        "127.0.0.1",
        Some("anything"),
        "application/octet-stream",
        &manifest,
    );
    assert_eq!(status, 403, "반입이 꺼졌는데 받았다");
    let _ = read_only.wait();
    assert_eq!(
        CoordinatorJobStore::open(&db)
            .unwrap()
            .get("web-job-1")
            .unwrap()
            .map(|j| j.state),
        None,
        "꺼진 화면이 반입했다"
    );

    // ── 운영자 화면(반입 켬) ──
    let (mut dashboard, address, token) = spawn_ui(
        &[
            "dashboard",
            "--control-db",
            db.to_str().unwrap(),
            "--port",
            "0",
            "--max-requests",
            "4",
            "--allow-import",
            "true",
            "--submitter-keyring",
            keyring.to_str().unwrap(),
            "--submitter-member",
            OWNER,
            "--max-snapshot-age-ms",
            "86400000",
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ],
        Some("DASHBOARD_IMPORT_ENABLED"),
    );
    let token = token.unwrap();
    let (status, _) = http(
        &address,
        "POST",
        "/api/import",
        "127.0.0.1",
        None,
        "application/octet-stream",
        &manifest,
    );
    assert_eq!(status, 403, "토큰 없이 반입했다");
    // 결함 298 — 설정 응답은 토큰을 내지 않는다.
    let (_, body) = http(
        &address,
        "GET",
        "/api/config",
        "127.0.0.1",
        None,
        "text/plain",
        b"",
    );
    assert!(
        !String::from_utf8_lossy(&body).contains(&token),
        "설정 응답이 토큰을 냈다"
    );
    let (status, body) = http(
        &address,
        "POST",
        "/api/import",
        "127.0.0.1",
        Some(&token),
        "application/octet-stream",
        &manifest,
    );
    let result = json(&body);
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["ok"], true, "{result}");
    assert!(
        result["planned"]
            .as_str()
            .unwrap_or("")
            .starts_with("QUEUED"),
        "{result}"
    );
    let (status, body) = http(
        &address,
        "GET",
        "/api/status",
        "127.0.0.1",
        None,
        "text/plain",
        b"",
    );
    assert_eq!(status, 200);
    assert_eq!(
        json(&body)["summary"]["queued"],
        1,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let _ = dashboard.wait();
    assert_eq!(
        CoordinatorJobStore::open(&db)
            .unwrap()
            .get("web-job-1")
            .unwrap()
            .map(|j| j.state),
        Some(JobState::Queued),
        "control DB 에 QUEUED 로 남지 않았다"
    );
}
