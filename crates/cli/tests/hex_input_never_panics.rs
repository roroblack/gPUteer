//! ★★ **hex 를 받는 어떤 명령도 입력으로 패닉하지 않는다** (결함 ⑬, 2026-09-10).
//!
//! 같은 모양의 hex 해석이 저장소에 일곱 곳 있었다. 전부 길이만 보고
//! **바이트로 잘라서**, 한글 한 글자(3바이트) + '1' 들로 길이를 맞춘 입력에
//! 패닉했다. 독립 검수가 `submit` 에서 찾았고, 재검수가 `issue-grant` 에서
//! 같은 것을 다시 찾았고, grep 으로 다섯 곳이 더 나왔다. 해석은
//! `gputeer_crypto::hex` 한 곳으로 모았다.
//!
//! ★ `!ok` 만 보면 안 된다 — **패닉도 실패 코드로 끝난다**(재검수 11 이
//!   기존 `issue-grant` 테스트에서 정확히 이것을 짚었다). 그래서 셋을 본다:
//!   1. 실패로 끝났다
//!   2. **패닉 흔적이 없다**
//!   3. 그 입력을 읽는 **바로 그 칸**의 말로 거부했다 — 다른 관문이 먼저
//!      막아 hex 해석에 닿지도 못한 경우를 가른다(결함 ① 의 모양)

use std::path::{Path, PathBuf};
use std::process::Command;

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test exe");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(format!("gputeer{}", std::env::consts::EXE_SUFFIX))
}

fn run_cli(args: &[&str]) -> (bool, String) {
    let out = Command::new(cli_bin())
        .args(args)
        .output()
        .expect("gputeer 실행");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// 바이트 길이는 `len` 인데 첫 글자가 3바이트인 문자열.
fn multibyte(len: usize) -> String {
    let s = format!("가{}", "1".repeat(len - 3));
    assert_eq!(s.len(), len, "전제: 바이트 길이가 맞아야 길이 검사를 통과한다");
    s
}

fn refused_by_name(label: &str, (ok, output): (bool, String), marker: &str) {
    assert!(!ok, "{label}: 받아들였다\n{output}");
    assert!(
        !output.contains("panicked"),
        "{label}: ★ 패닉했다\n{output}"
    );
    assert!(
        output.contains(marker),
        "{label}: `{marker}` 가 아닌 이유로 막혔다 — hex 해석에 닿았는지 모른다\n{output}"
    );
}

const GOOD_SEED: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const DECLARATIONS: [&str; 12] = [
    "--workload-class", "TRAINING",
    "--side-effect-class", "PURE",
    "--dataset-sensitivity", "INTERNAL",
    "--minimum-security-tier", "S2",
    "--minimum-isolation-class", "CONTAINED",
    "--minimum-key-protection", "K1",
];

fn submit_args<'a>(seed: &'a str, out: &'a str) -> Vec<&'a str> {
    let mut args = vec![
        "submit",
        "--job-id", "01JJOBHEXPANIC000000000001",
        "--entrypoint", "python",
        "--submitter-device-id", "01JSUBMITTERHEXPANIC00001",
        "--submitter-seed", seed,
        "--issued-at-unix-ms", "1800000000000",
        "--expires-at-unix-ms", "1800000600000",
        "--out", out,
    ];
    args.extend_from_slice(&DECLARATIONS);
    args
}

fn path_str(p: &Path) -> &str {
    p.to_str().expect("UTF-8 경로")
}

#[test]
fn submit_seed() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("m.pb");
    let seed = multibyte(64);
    refused_by_name("submit --submitter-seed", run_cli(&submit_args(&seed, path_str(&out))), "SEED_NOT_HEX");
    assert!(!out.exists());
}

#[test]
fn issue_grant_key_file() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let key = dir.path().join("bad.key");
    std::fs::write(&key, multibyte(64)).expect("키 파일");
    let out = dir.path().join("g.pb");
    let db = dir.path().join("control.sqlite3");
    let result = run_cli(&[
        "issue-grant",
        "--job-id", "01JJOBHEXPANIC000000000001",
        "--control-db", path_str(&db),
        "--attempt-id", "01JATTEMPTHEXPANIC0000001",
        "--lease-id", "01JLEASEHEXPANIC000000001",
        "--grant-id", "01JGRANTHEXPANIC000000001",
        "--grant-issued-at-unix-ms", "1",
        "--grant-expires-at-unix-ms", "2",
        "--coordinator-key-file", path_str(&key),
        "--out", path_str(&out),
    ]);
    refused_by_name("issue-grant --coordinator-key-file", result, "KEY_NOT_HEX");
    assert!(!out.exists());
}

#[test]
fn stage_job_operation_key() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("control.sqlite3");
    let keyring = dir.path().join("k.keyring");
    let key = multibyte(32);
    let result = run_cli(&[
        "stage-job",
        "--job-id", "01JJOBHEXPANIC000000000001",
        "--control-db", path_str(&db),
        "--submitter-keyring", path_str(&keyring),
        "--submitter-member", "owner",
        "--max-snapshot-age-ms", "86400000",
        "--best-fit-axes", "vram,gpu_count,cpu,ram,workspace",
        "--coordinator-id", "01JCOORDINATORHEXPANIC001",
        "--coordinator-term", "1",
        "--attempt-id", "01JATTEMPTHEXPANIC0000001",
        "--lease-id", "01JLEASEHEXPANIC000000001",
        "--operation-key", &key,
        "--lease-issued-at-unix-ms", "1800000000000",
        "--lease-renew-after-unix-ms", "1800000300000",
        "--lease-expires-at-unix-ms", "1800000600000",
        "--lease-max-total-duration-seconds", "86400",
        "--i-understand-plaintext-keyring-is-unsafe", "true",
    ]);
    refused_by_name("stage-job --operation-key", result, "--operation-key");
}

#[test]
fn import_manifest_idempotency_key() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    // Manifest 는 **정상**으로 만든다 — 파일 쪽에서 먼저 막히지 않게.
    let manifest = dir.path().join("m.pb");
    let (ok, output) = run_cli(&submit_args(GOOD_SEED, path_str(&manifest)));
    assert!(ok, "준비용 submit 실패: {output}");
    let db = dir.path().join("control.sqlite3");
    let keyring = dir.path().join("k.keyring");
    let key = multibyte(32);
    let result = run_cli(&[
        "import-manifest",
        "--manifest", path_str(&manifest),
        "--submitter-keyring", path_str(&keyring),
        "--job-db", path_str(&db),
        "--idempotency-key", &key,
        "--i-understand-plaintext-keyring-is-unsafe", "true",
    ]);
    refused_by_name("import-manifest --idempotency-key", result, "--idempotency-key");
}

#[test]
fn import_inventory_verifying_key() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let doc = dir.path().join("inventory.json");
    std::fs::write(
        &doc,
        format!(
            r#"{{
  "schema_version": 1,
  "agents": [
    {{
      "registry": {{
        "node_id": "node-hex", "device_id": "device-hex",
        "owner_member_id": "owner-hex", "verifying_key_hex": "{}",
        "node_state": "ONLINE", "risk_state": "NORMAL",
        "security_tier": "S2", "isolation_class": "CONTAINED",
        "key_protection": "K1"
      }},
      "inventory": {{
        "inventory_revision": 1, "observed_at_unix_ms": 1800000000000,
        "gpus": [{{ "gpu_id": "node-hex-gpu-0", "model": "RTX 4070 SUPER",
                    "healthy": true, "available_vram_bytes": 12884901888 }}],
        "available_cpu_cores": 16, "available_ram_bytes": 34359738368,
        "available_workspace_bytes": 107374182400,
        "allowed_workload_classes": ["TRAINING"],
        "third_party_workloads_opt_in": true
      }}
    }}
  ]
}}"#,
            multibyte(64)
        ),
    )
    .expect("문서 쓰기");
    let db = dir.path().join("control.sqlite3");
    let result = run_cli(&[
        "import-inventory",
        "--inventory", path_str(&doc),
        "--inventory-db", path_str(&db),
    ]);
    refused_by_name("import-inventory verifying_key_hex", result, "verifying_key_hex");
}

#[test]
fn coordinator_stub_own_seed() {
    let seed = multibyte(64);
    let result = run_cli(&[
        "coordinator-stub",
        "--listen", "127.0.0.1:0",
        "--own-seed", &seed,
        "--peer-pubkey", GOOD_SEED,
        "--coordinator-device-id", "01JCOORDINATORHEXPANIC001",
        "--agent-device-id", "01JAGENTHEXPANIC000000001",
        "--grant-id", "01JGRANTHEXPANIC000000001",
        "--attempt-id", "01JATTEMPTHEXPANIC0000001",
        "--lease-id", "01JLEASEHEXPANIC000000001",
        "--job-id", "01JJOBHEXPANIC000000000001",
        "--accept-timeout-ms", "2000",
    ]);
    refused_by_name("coordinator-stub --own-seed", result, "hex 가 아니다");
}

#[test]
fn agent_stub_own_seed() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let fence = dir.path().join("fence.sqlite3");
    let seed = multibyte(64);
    let result = run_cli(&[
        "agent-stub",
        "--connect", "127.0.0.1:9",
        "--own-seed", &seed,
        "--peer-pubkey", GOOD_SEED,
        "--coordinator-device-id", "01JCOORDINATORHEXPANIC001",
        "--agent-device-id", "01JAGENTHEXPANIC000000001",
        "--fence-db", path_str(&fence),
    ]);
    refused_by_name("agent-stub --own-seed", result, "hex 가 아니다");
}
