//! `gputeer submit` — **선언한 축이 진짜 그 값인가.**
//!
//! ★ 이 명령은 사슬의 모든 테스트가 거쳐 가는데도 **자기 테스트가
//!   없었다.** 다른 테스트들은 전부 올바른 값을 주므로 오타를 거부하는
//!   경로를 한 번도 밟지 않았다.
//!
//! ★ **왜 중요한가.** 축 이름을 잘못 쓰면 조용히 기본값으로 떨어지는
//!   설계가 흔한데, 그러면 `TRAINING` 을 쓰려던 Job 이 `UNSPECIFIED` 로
//!   제출되고 나중에 `plan-job` 이 "축이 비었다" 고 거부한다 — 운영자는
//!   **오타가 아니라 파서 결함으로 오해한다.** 제출 시점에 거부해야
//!   무엇이 잘못됐는지 알 수 있다.

use std::path::PathBuf;
use std::process::Command;

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) { "gputeer.exe" } else { "gputeer" })
}

const JOB: &str = "01JJOBSUBMIT000000000001";
const SUBMITTER: &str = "01JSUBMITTERSUB000000001";
const SEED: &str = "1111111111111111111111111111111111111111111111111111111111111122";

fn run_cli(args: &[&str]) -> (bool, String) {
    let out = Command::new(cli_bin()).args(args).output().expect("gputeer 실행");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_millis() as u64
}

/// 올바른 선언 여덟 축. 개별 테스트가 이 중 하나만 바꿔 쓴다.
const GOOD: [(&str, &str); 8] = [
    ("--workload-class", "TRAINING"),
    ("--side-effect-class", "PURE"),
    ("--dataset-sensitivity", "INTERNAL"),
    ("--minimum-security-tier", "S2"),
    ("--minimum-isolation-class", "CONTAINED"),
    ("--minimum-key-protection", "K1"),
    ("--gpu-count", "1"),
    ("--gpu-min-vram-bytes", "8589934592"),
];

fn submit(out: &std::path::Path, override_flag: Option<(&str, &str)>) -> (bool, String) {
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
    let mut args: Vec<String> = vec![
        "submit".into(), "--job-id".into(), JOB.into(),
        "--entrypoint".into(), "python".into(),
        "--submitter-device-id".into(), SUBMITTER.into(),
        "--submitter-seed".into(), SEED.into(),
        "--issued-at-unix-ms".into(), issued,
        "--expires-at-unix-ms".into(), expires,
        "--out".into(), out.to_str().unwrap().into(),
    ];
    for (name, value) in GOOD {
        let value = match override_flag {
            Some((k, v)) if k == name => v,
            _ => value,
        };
        args.push(name.into());
        args.push(value.into());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_cli(&borrowed)
}

/// ★★ **오타는 제출 시점에 거부된다** — 조용히 기본값으로 떨어지지 않는다.
#[test]
fn a_misspelled_declaration_is_refused_at_submit_time() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");

    // 대조 — 올바른 값이면 파일이 나온다. 없으면 "항상 거부" 로도 통과한다.
    let good = dir.path().join("good.pb");
    let (ok, output) = submit(&good, None);
    assert!(ok, "올바른 선언인데 거부했다: {output}");
    assert!(good.exists(), "성공했는데 파일이 없다");

    // 여섯 enum 축 각각에 **그럴듯한 오타**를 준다.
    for (flag, typo) in [
        ("--workload-class", "TRAINNG"),          // 글자 빠짐
        ("--side-effect-class", "PURE_"),         // 꼬리 붙음
        ("--dataset-sensitivity", "INTERNALL"),   // 글자 겹침
        ("--minimum-security-tier", "S22"),       // 숫자 겹침
        ("--minimum-isolation-class", "CONTAIN"), // 잘림
        ("--minimum-key-protection", "K"),        // 잘림
    ] {
        let out = dir.path().join("typo.pb");
        let _ = std::fs::remove_file(&out);
        let (ok, output) = submit(&out, Some((flag, typo)));
        assert!(!ok, "{flag} {typo:?} 를 받아들였다: {output}");
        assert!(
            output.contains(typo) && output.contains("모른다"),
            "{flag}: 무엇이 잘못됐는지 안 말한다: {output}"
        );
        assert!(
            !out.exists(),
            "{flag}: 거부했는데 Manifest 파일을 남겼다"
        );
    }
}

/// 대소문자는 받아 준다 — 거부는 **오타**에만 걸려야 한다.
///
/// 이 대조가 없으면 위 테스트가 "값을 아무것도 못 알아본다" 로도 통과한다.
#[test]
fn declarations_are_case_insensitive() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("lower.pb");
    let (ok, output) = submit(&out, Some(("--workload-class", "training")));
    assert!(ok, "소문자를 거부했다: {output}");
    assert!(out.exists());
}

/// 시각 인자를 직접 통제하는 제출 — 기본 helper 는 항상 만료를 준다.
fn submit_with_times(
    out: &std::path::Path,
    issued: &str,
    expires: Option<&str>,
) -> (bool, String) {
    let mut args: Vec<String> = vec![
        "submit".into(), "--job-id".into(), JOB.into(),
        "--entrypoint".into(), "python".into(),
        "--submitter-device-id".into(), SUBMITTER.into(),
        "--submitter-seed".into(), SEED.into(),
        "--issued-at-unix-ms".into(), issued.into(),
        "--out".into(), out.to_str().unwrap().into(),
    ];
    if let Some(e) = expires {
        args.push("--expires-at-unix-ms".into());
        args.push(e.into());
    }
    for (name, value) in GOOD {
        args.push((*name).into());
        args.push((*value).into());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_cli(&borrowed)
}

/// ★★ **만료를 생략하는 경로를 실제로 돈다** (2026-09-07 검수 지적).
///
/// 기본 helper 가 **항상** 만료를 주기 때문에, 생략 경로(`issued + 7일`)는
/// 테스트에서 한 번도 실행되지 않고 있었다. 제출자가 말하지 않은 값이
/// 서명 대상에 들어가는 자리인데 아무도 안 보고 있었다.
#[test]
fn omitting_the_expiry_uses_the_documented_seven_day_default() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("no-expiry.pb");
    let issued = now_unix_ms().saturating_sub(60_000).to_string();

    let (ok, output) = submit_with_times(&out, &issued, None);
    assert!(ok, "만료를 생략했는데 거부했다: {output}");
    assert!(out.exists(), "성공했다는데 파일이 없다");

    // ★ 정말 7일이 들어갔는지 **서명된 바이트에서** 확인한다.
    //   "성공했다" 만 보면 0 이 들어가도 통과한다.
    let bytes = std::fs::read(&out).expect("Manifest 읽기");
    let manifest = <gputeer_protocol::pb::JobManifest as prost::Message>::decode(bytes.as_slice())
        .expect("Manifest 디코드");
    let want: u64 = issued.parse::<u64>().unwrap() + 7 * 24 * 60 * 60 * 1000;
    assert_eq!(
        manifest.expires_at_unix_ms, want,
        "생략 시 기본값이 규범(issued + 7일)과 다르다"
    );
}

/// ★★ **발급이 만료보다 뒤면 거부한다** (2026-09-07 검수 지적).
///
/// 전에는 이 검사가 **없었고** 그 경로를 도는 테스트도 없었다.
/// 역전된 시각도 그대로 서명한 뒤 바깥 검증에 넘겼다.
#[test]
fn a_manifest_that_expires_before_it_is_issued_is_refused_before_signing() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("reversed.pb");
    let issued = now_unix_ms();

    let (ok, output) = submit_with_times(
        &out,
        &issued.to_string(),
        Some(&(issued - 1).to_string()),
    );
    assert!(!ok, "만료가 발급보다 앞인데 받아들였다: {output}");
    // ★ 이유까지 본다 — 다른 관문에 걸려도 !ok 는 참이다.
    assert!(
        output.contains("발급") && output.contains("만료"),
        "시각 순서가 아니라 다른 이유로 막혔다: {output}"
    );
    assert!(!out.exists(), "거부했는데 파일을 남겼다");
}

/// 발급과 만료가 **같아도** 거부한다 — 만들자마자 만료된 것이다.
#[test]
fn an_expiry_equal_to_the_issue_time_is_refused_too() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("equal.pb");
    let issued = now_unix_ms();

    let (ok, output) =
        submit_with_times(&out, &issued.to_string(), Some(&issued.to_string()));
    assert!(!ok, "발급 == 만료 를 받아들였다: {output}");
    assert!(!out.exists(), "거부했는데 파일을 남겼다");
}
