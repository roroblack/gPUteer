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
const GOOD: [(&str, &str); 11] = [
    ("--workload-class", "TRAINING"),
    ("--side-effect-class", "PURE"),
    ("--dataset-sensitivity", "INTERNAL"),
    ("--minimum-security-tier", "S2"),
    ("--minimum-isolation-class", "CONTAINED"),
    ("--minimum-key-protection", "K1"),
    ("--gpu-count", "1"),
    ("--gpu-min-vram-bytes", "8589934592"),
    // ★ 2026-09-10 — 변환기가 생략된 자원을 더 이상 0 으로 채우지 않는다.
    ("--cpu-cores", "4"),
    ("--ram-bytes", "8589934592"),
    ("--workspace-bytes", "10737418240"),
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

/// 이미 있는 Manifest 파일을 말없이 덮지 않는다.
///
/// ★★ 2026-09-10 — `issue-grant` 에서 독립 검수가 찾은 결함이
///   **이 명령에도 똑같이 있었다.** `fs::write` 한 줄이라 (1) 기존 파일을
///   말없이 덮고 (2) 중간에 실패하면 잘린 채 남았다.
///   고치면서 도우미를 `crate::out_file` 한 곳으로 모았다 — 한쪽만
///   고쳐지는 것이 이 결함이 두 곳에 생긴 방식이기 때문이다.
#[test]
fn an_existing_manifest_file_is_not_clobbered() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("manifest.pb");

    let (ok, output) = submit(&out, None);
    assert!(ok, "정상 경로가 실패했다: {output}");
    let first = std::fs::read(&out).expect("첫 Manifest");
    assert!(!first.is_empty(), "첫 Manifest 가 비어 있다");

    let (ok2, output2) = submit(&out, None);
    assert!(!ok2, "이미 있는 파일을 말없이 덮었다: {output2}");
    assert!(
        output2.contains("SUBMIT_REFUSED: OUT_EXISTS"),
        "거부했는데 이유가 파일 존재가 아니다: {output2}"
    );

    // ★ 핵심 — 원래 파일이 바이트 그대로 남아야 한다.
    let after = std::fs::read(&out).expect("거부 뒤에도 파일이 있어야 한다");
    assert_eq!(first, after, "거부했는데 기존 파일이 바뀌었다");

    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .expect("디렉터리")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".tmp."))
        .collect();
    assert!(leftovers.is_empty(), "임시 파일이 남았다: {leftovers:?}");
}

/// 명시적으로 요청하면 덮어쓴다.
///
/// ★ 대조군. 없으면 "항상 거부" 로 고쳐도 위 테스트가 통과한다.
#[test]
fn an_explicit_flag_allows_replacing_the_manifest() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("manifest.pb");
    std::fs::write(&out, b"stale bytes").expect("미리 쓴다");

    // ★ `submit()` 헬퍼는 값을 **교체**만 할 수 있어 새 플래그를 못 넣는다.
    //   그래서 여기서는 직접 부른다.
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
        "--overwrite-existing-manifest".into(), "true".into(),
    ];
    for (name, value) in GOOD {
        args.push(name.into());
        args.push(value.into());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let (ok, output) = run_cli(&borrowed);
    assert!(ok, "명시적 덮어쓰기가 실패했다: {output}");
    let bytes = std::fs::read(&out).expect("Manifest 를 읽는다");
    assert_ne!(bytes, b"stale bytes", "덮어쓴다고 했는데 옛 내용이 그대로다");
    assert!(!bytes.is_empty(), "덮어썼는데 비어 있다");
}

/// 선언한 값이 **실제로 서명 대상에 들어갔는지** 확인한다.
///
/// ★★ 2026-09-10 독립 검수 지적. 그전까지 정상 경로 테스트는
///   "성공했다 + 파일이 있다" 만 봤다. 그러면 **매핑이 틀려도 통과한다** —
///   예를 들어 `TRAINING` 을 `Inference` 로 잘못 넣어도 성공은 성공이다.
///   "받아들였다" 와 "요청한 값을 서명했다" 는 다른 말이다.
#[test]
fn the_declared_values_are_the_ones_that_get_signed() {
    use gputeer_protocol::pb;
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("declared.pb");

    let (ok, output) = submit(&out, None);
    assert!(ok, "정상 선언이 실패했다: {output}");

    let bytes = std::fs::read(&out).expect("Manifest 읽기");
    let m = <pb::JobManifest as prost::Message>::decode(bytes.as_slice()).expect("디코드");

    // 여섯 축을 **하나씩** 대조한다. GOOD 에 적힌 그 값이어야 한다.
    assert_eq!(
        m.workload.as_ref().expect("workload").class,
        pb::WorkloadClass::Training as i32,
        "workload.class 가 선언과 다르다"
    );
    assert_eq!(
        m.side_effect_class,
        pb::SideEffectClass::Pure as i32,
        "side_effect_class 가 선언과 다르다"
    );
    assert_eq!(
        m.dataset.as_ref().expect("dataset").sensitivity,
        pb::Sensitivity::Internal as i32,
        "dataset.sensitivity 가 선언과 다르다"
    );
    assert_eq!(
        m.minimum_security_tier,
        pb::SecurityTier::S2 as i32,
        "minimum_security_tier 가 선언과 다르다"
    );
    assert_eq!(
        m.minimum_isolation_class,
        pb::IsolationClass::Contained as i32,
        "minimum_isolation_class 가 선언과 다르다"
    );
    assert_eq!(
        m.minimum_key_protection,
        pb::KeyProtection::K1 as i32,
        "minimum_key_protection 이 선언과 다르다"
    );

    // 자원도 같이 본다 — 여기도 "넣었다" 와 "그 값이다" 는 다르다.
    let r = m.resources.as_ref().expect("resources");
    assert_eq!(r.cpu_cores, 4, "cpu_cores 가 선언과 다르다");
    assert_eq!(r.ram_bytes, 8_589_934_592, "ram_bytes 가 선언과 다르다");
    assert_eq!(
        r.workspace_bytes, 10_737_418_240,
        "workspace_bytes 가 선언과 다르다"
    );
}

/// 대소문자를 다르게 써도 **서명 대상 바이트가 같다.**
///
/// ★ 검수가 물었다 — 대소문자를 안 가리는 게 의도라면, 같은 선언인데
///   서명이 달라지지는 않는지. 코드를 보면 원래 철자를 저장하지 않으므로
///   같아야 한다. 그런데 **그것을 고정하는 테스트가 없었다.**
#[test]
fn declaration_case_does_not_change_the_signed_bytes() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();

    let run_with = |out: &std::path::Path, upper: bool| {
        let mut args: Vec<String> = vec![
            "submit".into(), "--job-id".into(), JOB.into(),
            "--entrypoint".into(), "python".into(),
            "--submitter-device-id".into(), SUBMITTER.into(),
            "--submitter-seed".into(), SEED.into(),
            "--issued-at-unix-ms".into(), issued.clone(),
            "--expires-at-unix-ms".into(), expires.clone(),
            "--out".into(), out.to_str().unwrap().into(),
        ];
        for (name, value) in GOOD {
            args.push(name.into());
            args.push(if upper {
                value.to_uppercase()
            } else {
                value.to_lowercase()
            });
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        run_cli(&borrowed)
    };

    let up = dir.path().join("upper.pb");
    let low = dir.path().join("lower.pb");
    let (ok1, o1) = run_with(&up, true);
    assert!(ok1, "대문자 선언이 거부됐다: {o1}");
    let (ok2, o2) = run_with(&low, false);
    assert!(ok2, "소문자 선언이 거부됐다: {o2}");

    // ★ 핵심 — 바이트가 **완전히** 같아야 한다. 서명까지 포함해서.
    assert_eq!(
        std::fs::read(&up).expect("upper"),
        std::fs::read(&low).expect("lower"),
        "같은 선언인데 대소문자에 따라 서명 대상이 달라진다"
    );
}

/// 발급 시각이 너무 커서 기본 만료를 더할 수 없으면 **거부한다**.
///
/// ★★ 2026-09-10 독립 검수 지적. 그전에는 `issued + 7일` 이 그냥
///   덧셈이라 넘칠 수 있었고, **재현했다**:
///     --issued-at-unix-ms 18446744073709551615
///     -> panicked at submit.rs:78 "attempt to add with overflow"
///   오버플로 검사가 꺼진 빌드에서는 panic 대신 값이 되감겨 역전 검사에
///   걸린다 — 즉 **빌드 설정에 따라 오류 동작이 달랐다.**
///   사용자 입력으로 패닉이 나면 그건 오류 보고가 아니다.
#[test]
fn an_issued_time_too_large_for_the_default_expiry_is_refused_not_a_panic() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("overflow.pb");
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();

    // 만료를 **안 주고** 발급만 최대값으로 준다 — 기본값 계산이 도는 경로다.
    let mut args: Vec<String> = vec![
        "submit".into(), "--job-id".into(), JOB.into(),
        "--entrypoint".into(), "python".into(),
        "--submitter-device-id".into(), SUBMITTER.into(),
        "--submitter-seed".into(), SEED.into(),
        "--issued-at-unix-ms".into(), u64::MAX.to_string(),
        "--out".into(), out.to_str().unwrap().into(),
    ];
    for (name, value) in GOOD {
        args.push(name.into());
        args.push(value.into());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let (ok, output) = run_cli(&borrowed);

    assert!(!ok, "넘치는 발급 시각을 받아들였다: {output}");
    assert!(
        output.contains("SUBMIT_REFUSED: ISSUED_AT_TOO_LARGE"),
        "거부는 했는데 이유가 오버플로가 아니다 — panic 이었을 수 있다: {output}"
    );
    assert!(
        !output.contains("panicked"),
        "패닉으로 죽었다 — 오류 보고가 아니다: {output}"
    );
    assert!(!out.exists(), "거부했는데 파일을 남겼다");

    // 대조 — 만료를 직접 주면 그 큰 발급 시각도 문제가 아니다…가 아니라
    // 역전 검사에 걸린다. 어느 쪽이든 **패닉이 아니어야** 한다.
    let mut args2: Vec<String> = vec![
        "submit".into(), "--job-id".into(), JOB.into(),
        "--entrypoint".into(), "python".into(),
        "--submitter-device-id".into(), SUBMITTER.into(),
        "--submitter-seed".into(), SEED.into(),
        "--issued-at-unix-ms".into(), u64::MAX.to_string(),
        "--expires-at-unix-ms".into(), expires,
        "--out".into(), out.to_str().unwrap().into(),
    ];
    for (name, value) in GOOD {
        args2.push(name.into());
        args2.push(value.into());
    }
    let borrowed2: Vec<&str> = args2.iter().map(String::as_str).collect();
    let (ok2, output2) = run_cli(&borrowed2);
    assert!(!ok2, "발급이 만료보다 뒤인데 받아들였다: {output2}");
    assert!(!output2.contains("panicked"), "여기서도 패닉이 났다: {output2}");
}

/// 64**바이트**지만 hex 가 아닌 seed 는 **패닉이 아니라 거부**한다.
///
/// ★★ 2026-09-10 독립 재검수가 찾았다. `hex.len()` 은 바이트 길이인데
///   슬라이스도 바이트로 잘랐다. 한글 한 글자(3바이트) + '1' 61개 =
///   정확히 64바이트라 길이 검사를 통과하고, 첫 `hex[0..2]` 가
///   **문자 경계를 갈라 패닉**했다.
///   ★ 만료 오버플로 패닉과 **다른 경로**다 — 그건 산술, 이건 문자열이다.
#[test]
fn a_sixty_four_byte_but_non_hex_seed_is_refused_not_a_panic() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");

    // "가"(3바이트) + '1' 61개 = 64바이트. 문자 수는 62 다.
    let multibyte = format!("가{}", "1".repeat(61));
    assert_eq!(multibyte.len(), 64, "이 테스트의 전제가 깨졌다");

    for bad_seed in [
        multibyte.as_str(),
        // ASCII 지만 hex 가 아닌 것도 같이 본다.
        "zz0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    ] {
        let out = dir.path().join("seed.pb");
        let _ = std::fs::remove_file(&out);
        let issued = now_unix_ms().saturating_sub(60_000).to_string();
        let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();
        let mut args: Vec<String> = vec![
            "submit".into(), "--job-id".into(), JOB.into(),
            "--entrypoint".into(), "python".into(),
            "--submitter-device-id".into(), SUBMITTER.into(),
            "--submitter-seed".into(), bad_seed.into(),
            "--issued-at-unix-ms".into(), issued,
            "--expires-at-unix-ms".into(), expires,
            "--out".into(), out.to_str().unwrap().into(),
        ];
        for (name, value) in GOOD {
            args.push(name.into());
            args.push(value.into());
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let (ok, output) = run_cli(&borrowed);

        assert!(!ok, "hex 가 아닌 seed 를 받아들였다: {output}");
        assert!(
            !output.contains("panicked"),
            "패닉으로 죽었다 — 오류 보고가 아니다: {output}"
        );
        assert!(
            output.contains("SUBMIT_REFUSED: SEED_"),
            "거부는 했는데 이유가 seed 가 아니다: {output}"
        );
        assert!(!out.exists(), "거부했는데 파일을 남겼다");
    }
}

/// 자원을 **일부만** 선언하면 거부한다 — 선언한 값을 버리지 않는다.
///
/// ★★ 2026-09-10 독립 재검수가 찾았다. `--gpu-count` 하나가 자원 전체의
///   스위치여서, 그것만 빼고 `--cpu-cores 4 --ram-bytes ...` 를 주면
///   **선언한 셋을 말없이 버리고** `resources: None` 을 만들었다.
///   숫자 파싱조차 안 했다.
#[test]
fn declaring_only_some_resources_is_refused_rather_than_silently_dropped() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let issued = now_unix_ms().saturating_sub(60_000).to_string();
    let expires = (now_unix_ms() + 7 * 24 * 3_600_000).to_string();

    let run = |out: &std::path::Path, resource_flags: &[(&str, &str)]| {
        let mut args: Vec<String> = vec![
            "submit".into(), "--job-id".into(), JOB.into(),
            "--entrypoint".into(), "python".into(),
            "--submitter-device-id".into(), SUBMITTER.into(),
            "--submitter-seed".into(), SEED.into(),
            "--issued-at-unix-ms".into(), issued.clone(),
            "--expires-at-unix-ms".into(), expires.clone(),
            "--out".into(), out.to_str().unwrap().into(),
        ];
        // 여섯 enum 축만 GOOD 에서 가져온다(자원은 인자로 받는다).
        for (name, value) in GOOD {
            if name.starts_with("--gpu") || name.starts_with("--cpu")
                || name.starts_with("--ram") || name.starts_with("--workspace") {
                continue;
            }
            args.push(name.into());
            args.push(value.into());
        }
        for (name, value) in resource_flags {
            args.push((*name).into());
            args.push((*value).into());
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        run_cli(&borrowed)
    };

    // ★ 검수가 든 반례 그대로 — gpu-count 만 빼고 셋을 준다.
    let partial = dir.path().join("partial.pb");
    let (ok, output) = run(&partial, &[
        ("--cpu-cores", "4"),
        ("--ram-bytes", "8589934592"),
        ("--workspace-bytes", "10737418240"),
    ]);
    assert!(ok, "cpu·ram·workspace 를 다 줬는데 거부했다: {output}");
    let m = <gputeer_protocol::pb::JobManifest as prost::Message>::decode(
        std::fs::read(&partial).expect("읽기").as_slice(),
    ).expect("디코드");
    let r = m.resources.as_ref().expect("★ 선언한 자원이 버려졌다");
    assert_eq!(r.cpu_cores, 4, "선언한 cpu_cores 가 버려졌다");
    assert_eq!(r.ram_bytes, 8_589_934_592, "선언한 ram_bytes 가 버려졌다");

    // ★ 반대로 일부만 주면 **거부**해야 한다 — 0 으로 채우지 않는다.
    let missing = dir.path().join("missing.pb");
    let (ok2, output2) = run(&missing, &[("--gpu-count", "1")]);
    assert!(!ok2, "자원을 일부만 선언했는데 받아들였다: {output2}");
    assert!(
        output2.contains("SUBMIT_REFUSED: RESOURCE_PARTIAL"),
        "거부는 했는데 이유가 부분 선언이 아니다: {output2}"
    );
    assert!(!missing.exists(), "거부했는데 파일을 남겼다");

    // ★ 2026-09-10 재검수 11 — 경계 둘을 더 고정한다(결함 ⑮).
    //
    //   (1) GPU 모델 목록만 준 것도 **자원을 선언한 것**이다.
    //       `RESOURCE_FLAGS` 에서 이 항목이 빠지면 여기서 잡힌다.
    let models_only = dir.path().join("models_only.pb");
    let (ok3, output3) = run(&models_only, &[("--allowed-gpu-models", "RTX 4070 SUPER")]);
    assert!(!ok3, "GPU 모델만 선언했는데 받아들였다: {output3}");
    assert!(
        output3.contains("SUBMIT_REFUSED: RESOURCE_PARTIAL"),
        "거부는 했는데 이유가 부분 선언이 아니다: {output3}"
    );
    assert!(!models_only.exists(), "거부했는데 파일을 남겼다");

    //   (2) 자원 플래그를 **하나도** 안 주면 정상이고 `resources` 가 없다.
    //       "하나라도 있으면 선언" 판정이 늘 참이 되는 회귀를 여기서 잡는다.
    let none = dir.path().join("none.pb");
    let no_flags: [(&str, &str); 0] = [];
    let (ok4, output4) = run(&none, &no_flags);
    assert!(ok4, "자원을 안 쓰는 제출을 막았다: {output4}");
    let m4 = <gputeer_protocol::pb::JobManifest as prost::Message>::decode(
        std::fs::read(&none).expect("읽기").as_slice(),
    ).expect("디코드");
    assert!(m4.resources.is_none(), "선언하지 않은 자원이 생겼다: {:?}", m4.resources);
}

/// 서명이 **선언한 seed 의 공개키로** 검증된다.
///
/// ★★ 2026-09-10 독립 재검수 지적. 값 대조 테스트가 필드는 봤지만
///   **누가 서명했는지**는 안 봤다. 올바른 필드를 다른 키로 서명하는
///   회귀를 못 잡는다 — `submit` 의 자기 검증도 `AlwaysValid` 라
///   보완하지 못한다.
#[test]
fn the_signature_verifies_with_the_declared_seeds_public_key() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let out = dir.path().join("signed.pb");
    let (ok, output) = submit(&out, None);
    assert!(ok, "정상 경로가 실패했다: {output}");

    let bytes = std::fs::read(&out).expect("Manifest 읽기");
    let manifest =
        <gputeer_protocol::pb::JobManifest as prost::Message>::decode(bytes.as_slice())
            .expect("디코드");

    // 선언한 seed 에서 공개키를 도출해 그 키만 담은 keyring 으로 검증한다.
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&SEED[i * 2..i * 2 + 2], 16).expect("seed hex");
    }
    let mut ring = gputeer_crypto::InMemoryKeyring::new();
    ring.insert(
        SUBMITTER.to_string(),
        gputeer_crypto::SigningKey::from_bytes(&seed).verifying_key(),
    );
    let verifier = gputeer_crypto::Ed25519Verifier::new(&ring);

    let verified = gputeer_protocol::verify(
        &manifest,
        1,
        &verifier,
        manifest.issued_at_unix_ms,
        &mut gputeer_protocol::signing::NoReplayCheck,
    );
    assert!(
        verified.is_ok(),
        "선언한 seed 의 공개키로 서명이 검증되지 않는다: {:?}",
        verified.err()
    );
}
