//! ★ **agent-stub 은 모르는 플래그를 받아 두지 않는다** (결함 ⑯ 확장의 Agent 쪽,
//! 2026-09-10).
//!
//! Coordinator 파서가 모르는 이름을 조용히 받아 두던 것을 재검수 14 가 짚었다.
//! Agent 파서도 같은 모양이었다 — `--send-attempt-reprot true` 같은 오타를 주면
//! 보고를 켰다고 믿지만 아무 일도 안 일어난다.

use gputeer_agent::parse_config_from_args;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn base() -> Vec<String> {
    let peer = gputeer_crypto::SigningKey::from_bytes(&[9u8; 32]).verifying_key();
    let own_seed = "22".repeat(32);
    let peer_hex = hex(peer.as_bytes());
    [
        "--connect", "127.0.0.1:9",
        "--own-seed", own_seed.as_str(),
        "--peer-pubkey", peer_hex.as_str(),
        "--coordinator-device-id", "01JCOORDINATORAGENTFLAG01",
        "--agent-device-id", "01JAGENTAGENTFLAG00000001",
        "--fence-db", "fence.sqlite3",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn the_plain_arguments_parse() {
    parse_config_from_args(&base()).expect("정상 인자를 거부했다");
}

#[test]
fn a_misspelled_flag_is_refused_by_name() {
    let mut args = base();
    args.extend(["--send-attempt-reprot".to_string(), "true".to_string()]);
    let error = match parse_config_from_args(&args) {
        Ok(_) => panic!("오타를 받아들였다"),
        Err(error) => error,
    };
    assert!(
        error.starts_with("STARTUP_REFUSED: UNKNOWN_FLAGS"),
        "다른 이유로 거부했다: {error}"
    );
    assert!(error.contains("--send-attempt-reprot"), "이름을 안 댄다: {error}");
}

/// ★ 대조 — 올바른 이름은 받는다. 없으면 "모르는 것이든 아는 것이든 다
///   거부" 로도 위 테스트가 통과한다.
#[test]
fn the_correct_spelling_is_accepted() {
    let mut args = base();
    args.extend(["--send-attempt-report".to_string(), "true".to_string()]);
    let config = parse_config_from_args(&args).expect("올바른 이름을 거부했다");
    assert!(config.send_attempt_report);
}

/// `true`/`false` 가 아닌 불리언은 조용히 false 가 되지 않는다(재검수 15).
#[test]
fn an_invalid_bool_is_refused_by_name() {
    let mut args = base();
    args.extend(["--send-attempt-report".to_string(), "tru".to_string()]);
    let error = match parse_config_from_args(&args) {
        Ok(_) => panic!("잘못된 불리언을 받아들였다"),
        Err(error) => error,
    };
    assert!(error.starts_with("STARTUP_REFUSED: INVALID_BOOL"), "{error}");
    assert!(error.contains("--send-attempt-report"), "{error}");
}

/// 같은 키를 두 번 줘도 앞의 잘못된 값이 검사 전에 사라지지 않는다(재검수 18).
#[test]
fn an_invalid_bool_hidden_by_a_later_duplicate_is_still_refused() {
    let mut args = base();
    args.extend(
        ["--send-attempt-report", "tru", "--send-attempt-report", "false"]
            .iter()
            .map(|s| s.to_string()),
    );
    let error = match parse_config_from_args(&args) {
        Ok(_) => panic!("중복 뒤에 숨은 잘못된 불리언을 받아들였다"),
        Err(error) => error,
    };
    assert!(error.starts_with("STARTUP_REFUSED: INVALID_BOOL"), "{error}");
    assert!(error.contains("--send-attempt-report"), "{error}");
}

/// ★ 대조 — **올바른** 값의 중복은 받고 마지막 값을 쓴다(재검수 20).
///
/// 이게 없으면 "중복이면 전부 INVALID_BOOL" 로 잘못 바뀌어도 위 테스트가 통과한다.
#[test]
fn a_valid_duplicate_bool_is_accepted_and_the_last_value_wins() {
    let mut args = base();
    args.extend(
        ["--send-attempt-report", "false", "--send-attempt-report", "true"]
            .iter()
            .map(|s| s.to_string()),
    );
    let config = parse_config_from_args(&args).expect("올바른 값의 중복을 거부했다");
    assert!(config.send_attempt_report, "마지막 값을 쓰지 않았다");
}
