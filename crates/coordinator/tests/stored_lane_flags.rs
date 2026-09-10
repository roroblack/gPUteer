//! ★★ **coordinator-stub 은 받아 두고 말없이 버리는 플래그를 시작 전에 거부한다**
//! (결함 ⑯ 확장, 2026-09-10 재검수 14).
//!
//! ⑯ 는 저장된 예약 lane 의 `--manifest-file` 하나만 막았다. 재검수가 같은
//! 유형을 더 찾았다 — 그중 가장 나쁜 것은 `--corrupt-lease-signature` 였다.
//! 저장된 예약 lane 에서 그것을 주면 **정상 서명이 나가서**, 그 플래그로
//! 부정 경로를 재려던 테스트가 아무것도 안 재게 된다.
//!
//! ★ 설정 파서를 **직접** 부른다 — 프로세스를 띄울 필요가 없는 판정이다.
//! ★ 각 경우를 **그 이름으로** 거부하는지 본다. "뭔가 거부됐다" 만 보면
//!   목록에서 하나가 빠져도 다른 이유로 통과한다.

use gputeer_coordinator::parse_config_from_args;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 저장된 예약 lane 의 정상 인자.
fn stored_lane(job: &str, attempt: &str, lease: &str) -> Vec<String> {
    let peer = gputeer_crypto::SigningKey::from_bytes(&[7u8; 32]).verifying_key();
    let own_seed = "11".repeat(32);
    let peer_hex = hex(peer.as_bytes());
    [
        "--listen", "127.0.0.1:0",
        "--own-seed", own_seed.as_str(),
        "--peer-pubkey", peer_hex.as_str(),
        "--coordinator-device-id", "01JCOORDINATORFLAGS000001",
        "--agent-device-id", "01JAGENTFLAGS00000000001",
        "--grant-id", "01JGRANTFLAGS00000000001",
        "--attempt-id", attempt,
        "--lease-id", lease,
        "--job-id", job,
        "--lease-db", "lease.sqlite3",
        "--grant-from-control-db", "control.sqlite3",
        "--stored-grant-job-id", "J",
        "--stored-grant-attempt-id", "A",
        "--stored-grant-lease-id", "L",
        "--submitter-keyring", "submitters.keyring",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// 레거시 lane 의 정상 인자 — 저장된 예약 플래그가 없다.
fn legacy_lane() -> Vec<String> {
    let mut args = stored_lane("J", "A", "L");
    let cut = args
        .iter()
        .position(|a| a == "--grant-from-control-db")
        .expect("위치");
    args.truncate(cut);
    args
}

fn with(mut base: Vec<String>, extra: &[&str]) -> Vec<String> {
    base.extend(extra.iter().map(|s| s.to_string()));
    base
}

fn refused_by_name(args: Vec<String>, code: &str, name: &str) {
    let error = match parse_config_from_args(&args) {
        Ok(_) => panic!("{name} 를 받아들였다"),
        Err(error) => error,
    };
    assert!(
        error.starts_with(&format!("STARTUP_REFUSED: {code}")),
        "{name}: 다른 이유로 거부했다 — {error}"
    );
    assert!(error.contains(name), "{name}: 거부는 했는데 이름을 안 댄다 — {error}");
}

const NOT_APPLIED: [(&str, &str); 9] = [
    ("--manifest-file", "m.pb"),
    ("--corrupt-manifest-signature", "true"),
    ("--corrupt-manifest-hash", "true"),
    ("--corrupt-lease-signature", "true"),
    ("--expire-lease", "true"),
    ("--lease-ttl-ms", "1"),
    ("--fence-epoch", "999"),
    ("--max-total-duration-seconds", "1"),
    ("--renewed-fence-epoch", "999"),
];

#[test]
fn the_plain_stored_lane_parses() {
    parse_config_from_args(&stored_lane("J", "A", "L")).expect("정상 인자를 거부했다");
}

#[test]
fn each_flag_the_stored_lane_does_not_apply_is_refused_by_name() {
    for (flag, value) in NOT_APPLIED {
        refused_by_name(
            with(stored_lane("J", "A", "L"), &[flag, value]),
            "STORED_LANE_IGNORES",
            flag,
        );
    }
}

/// ★ 대조 — 같은 플래그를 **레거시 lane** 에서는 받는다. 그 lane 은 적용한다.
///
/// 이게 없으면 "그 플래그들을 언제나 거부한다" 로 고쳐도 위 테스트가 통과한다.
#[test]
fn the_same_flags_are_accepted_on_the_legacy_lane() {
    for (flag, value) in NOT_APPLIED {
        parse_config_from_args(&with(legacy_lane(), &[flag, value]))
            .unwrap_or_else(|e| panic!("레거시 lane 에서 {flag} 를 거부했다: {e}"));
    }
}

#[test]
fn an_id_that_disagrees_with_the_stored_one_is_refused() {
    refused_by_name(stored_lane("OTHER", "A", "L"), "STORED_LANE_ID_CONFLICT", "--job-id");
    refused_by_name(stored_lane("J", "OTHER", "L"), "STORED_LANE_ID_CONFLICT", "--attempt-id");
    refused_by_name(stored_lane("J", "A", "OTHER"), "STORED_LANE_ID_CONFLICT", "--lease-id");
}

#[test]
fn an_unknown_flag_is_refused_by_name_on_either_lane() {
    refused_by_name(
        with(stored_lane("J", "A", "L"), &["--manifest-fiel", "m.pb"]),
        "UNKNOWN_FLAGS",
        "--manifest-fiel",
    );
    refused_by_name(
        with(legacy_lane(), &["--renew-roundz", "3"]),
        "UNKNOWN_FLAGS",
        "--renew-roundz",
    );
}

/// ★ `--session-id` 는 Coordinator 가 **어느 lane 에서도** 읽지 않았다.
///
/// 처음엔 "다중 Agent 전용" 으로 분류했는데, 그 필드를 읽는 코드가 저장소
/// 어디에도 없었다(대조는 Agent 가 보낸 hello 와 요청 사이에서만 한다).
/// 필드를 지웠으니 --multi-agent 여도 모르는 이름이다.
#[test]
fn session_id_is_not_a_coordinator_flag_on_any_lane() {
    refused_by_name(
        with(legacy_lane(), &["--session-id", "s"]),
        "UNKNOWN_FLAGS",
        "--session-id",
    );
    refused_by_name(
        with(legacy_lane(), &["--multi-agent", "true", "--session-id", "s"]),
        "UNKNOWN_FLAGS",
        "--session-id",
    );
}

#[test]
fn multi_agent_only_flags_are_refused_on_the_sequential_lane() {
    for (flag, value) in [("--extra-agents", "x"), ("--require-concurrent-sessions", "2")] {
        refused_by_name(with(legacy_lane(), &[flag, value]), "MULTI_AGENT_ONLY", flag);
        // 대조 — --multi-agent lane 에서는 읽힌다.
        parse_config_from_args(&with(legacy_lane(), &["--multi-agent", "true", flag, value]))
            .unwrap_or_else(|e| panic!("--multi-agent 인데 {flag} 를 거부했다: {e}"));
    }
}

// ─────────────────────────────────────────────────────────────────────
// 결함 ㉑ (재검수 15) — lane 조건을 사실에 맞춘 뒤
// ─────────────────────────────────────────────────────────────────────

fn without(mut args: Vec<String>, flag: &str) -> Vec<String> {
    let at = args.iter().position(|a| a == flag).expect("그 플래그가 있어야 한다");
    args.drain(at..at + 2);
    args
}

#[test]
fn an_absent_stored_id_is_refused() {
    refused_by_name(
        without(stored_lane("J", "A", "L"), "--stored-grant-job-id"),
        "STORED_LANE_ID_MISSING",
        "--stored-grant-job-id",
    );
}

/// ★ 대조 — `--lease-db` 가 **없으면** 갱신 경로가 이 셋을 실제로 쓴다.
///
/// 처음엔 저장된 예약 lane 이면 무조건 거부했다. `fence_epoch` 는 저장소가
/// 없을 때 갱신의 기대 epoch 이고, 나머지 둘은 `build_renew_result` 가 쓴다.
#[test]
fn lease_db_dependent_flags_are_accepted_without_a_lease_db() {
    for (flag, value) in [
        ("--fence-epoch", "3"),
        ("--max-total-duration-seconds", "60"),
        ("--renewed-fence-epoch", "3"),
    ] {
        let args = with(without(stored_lane("J", "A", "L"), "--lease-db"), &[flag, value]);
        parse_config_from_args(&args)
            .unwrap_or_else(|e| panic!("--lease-db 없이 준 {flag} 를 거부했다: {e}"));
    }
}

/// Resume 은 저장된 예약 분기보다 먼저 반환한다 — 파서는 식별자를 요구하지
/// 않고, 그 조합 자체는 **시작 관문**이 거부한다.
#[test]
fn resume_with_a_control_db_is_refused_at_startup_not_by_the_stored_lane_checks() {
    // ★ 이 테스트만 `run()` 을 실제로 부른다. `run()` 은 시작 관문보다 **먼저**
    //   Lease 저장소를 열므로, 상대 경로를 주면 저장소 안에 DB 파일을 남긴다 —
    //   처음에 그렇게 해서 `crates/coordinator/lease.sqlite3` 가 생겼다.
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let lease_db = dir.path().join("lease.sqlite3");
    let mut args = legacy_lane();
    let at = args.iter().position(|a| a == "--lease-db").expect("--lease-db");
    args[at + 1] = lease_db.to_str().expect("경로").to_string();
    args.extend(
        ["--resume-protocol", "true", "--grant-from-control-db", "control.sqlite3"]
            .iter()
            .map(|s| s.to_string()),
    );
    let config = parse_config_from_args(&args).expect("Resume 이면 식별자를 요구하지 않는다");
    let error = gputeer_coordinator::run(config).expect_err("시작 관문이 거부해야 한다");
    assert!(
        error.contains("--resume-protocol") && error.contains("--grant-from-control-db"),
        "다른 이유로 거부했다: {error}"
    );
}

#[test]
fn stored_lane_only_flags_are_refused_on_the_legacy_lane() {
    refused_by_name(
        with(legacy_lane(), &["--stored-grant-ttl-ms", "1"]),
        "STORED_LANE_ONLY",
        "--stored-grant-ttl-ms",
    );
}

/// `--corrupt-own-signature tru` 가 조용히 false 가 되면 그 플래그로 재려던
/// 부정 경로가 아무것도 안 잰다(재검수 15).
#[test]
fn an_invalid_bool_is_refused_by_name() {
    refused_by_name(
        with(legacy_lane(), &["--corrupt-own-signature", "tru"]),
        "INVALID_BOOL",
        "--corrupt-own-signature",
    );
    // 대조 — false 는 값이다.
    parse_config_from_args(&with(legacy_lane(), &["--corrupt-own-signature", "false"]))
        .expect("false 를 거부했다");
}

#[test]
fn a_neighbor_report_db_without_expected_reports_is_refused() {
    refused_by_name(
        with(legacy_lane(), &["--neighbor-report-db", "reports.sqlite3"]),
        "NEEDS_EXPECT",
        "--neighbor-report-db",
    );
    // 대조 — 기대값이 있으면 받는다.
    parse_config_from_args(&with(
        legacy_lane(),
        &["--expect-neighbor-reports", "1", "--neighbor-report-db", "reports.sqlite3"],
    ))
    .expect("기대값이 있는데 거부했다");
}

// ─────────────────────────────────────────────────────────────────────
// 결함 ㉒ (재검수 18) — 중복 키 · 경계 테스트 공백
// ─────────────────────────────────────────────────────────────────────

/// 같은 키를 두 번 주면 앞 값이 덮여 **검사 전에** 사라졌다.
#[test]
fn an_invalid_bool_hidden_by_a_later_duplicate_is_still_refused() {
    refused_by_name(
        with(legacy_lane(), &["--corrupt-own-signature", "tru", "--corrupt-own-signature", "false"]),
        "INVALID_BOOL",
        "--corrupt-own-signature",
    );
    // 대조 — 올바른 값의 중복은 받는다(마지막 값을 쓴다).
    let config = parse_config_from_args(&with(
        legacy_lane(),
        &["--corrupt-own-signature", "false", "--corrupt-own-signature", "true"],
    ))
    .expect("올바른 값의 중복을 거부했다");
    assert!(config.corrupt_own_signature, "마지막 값을 쓰지 않았다");
    // 반대 방향도 — 마지막 값이 false 면 false 다(재검수 20).
    let config = parse_config_from_args(&with(
        legacy_lane(),
        &["--corrupt-own-signature", "true", "--corrupt-own-signature", "false"],
    ))
    .expect("올바른 값의 중복을 거부했다");
    assert!(!config.corrupt_own_signature, "마지막 값을 쓰지 않았다");
}

/// "항상 무시 6" 은 `--lease-db` 가 **없어도** 거부한다. 이게 없으면 9개를 전부
/// "--lease-db 있을 때만" 으로 바꿔도 다른 테스트가 통과한다.
#[test]
fn never_applied_flags_are_refused_without_a_lease_db_too() {
    for (flag, value) in &NOT_APPLIED[..6] {
        refused_by_name(
            with(without(stored_lane("J", "A", "L"), "--lease-db"), &[flag, value]),
            "STORED_LANE_IGNORES",
            flag,
        );
    }
}

#[test]
fn every_absent_stored_id_is_refused_by_name() {
    for flag in ["--stored-grant-job-id", "--stored-grant-attempt-id", "--stored-grant-lease-id"] {
        refused_by_name(without(stored_lane("J", "A", "L"), flag), "STORED_LANE_ID_MISSING", flag);
    }
}

#[test]
fn every_stored_lane_only_flag_is_refused_on_the_legacy_lane() {
    for (flag, value) in [
        ("--stored-grant-job-id", "J"),
        ("--stored-grant-attempt-id", "A"),
        ("--stored-grant-lease-id", "L"),
        ("--stored-grant-ttl-ms", "1"),
        ("--submitter-keyring", "k.keyring"),
        ("--i-understand-plaintext-keyring-is-unsafe", "true"),
    ] {
        refused_by_name(with(legacy_lane(), &[flag, value]), "STORED_LANE_ONLY", flag);
    }
}

/// ★ 저장된 예약 lane 은 제출자 keyring 이 **없으면** 시작하지 않는다(§A1 1.5 선행).
///   저장된 Manifest 를 싣기 전에 지금 신뢰하는 제출자 키로 다시 검증해야 한다.
#[test]
fn the_stored_lane_without_a_submitter_keyring_is_refused() {
    refused_by_name(
        without(stored_lane("J", "A", "L"), "--submitter-keyring"),
        "STORED_LANE_KEYRING_MISSING",
        "--submitter-keyring",
    );
}

/// ★ 결함 ㉞ — 숫자 인자의 **앞 값**이 잘못됐으면 뒤의 중복에 가려도 거부한다.
///   세 읽기 함수(기본값 u64 · 선택 u64 · 기본값 u32)를 하나씩 본다.
#[test]
fn an_invalid_number_hidden_by_a_later_duplicate_is_still_refused() {
    for (base, flag) in [
        (legacy_lane(), "--renew-extension-ms"),
        (stored_lane("J", "A", "L"), "--stored-grant-ttl-ms"),
        (legacy_lane(), "--expect-attempt-reports"),
    ] {
        let error = match parse_config_from_args(&with(base, &[flag, "abc", flag, "5"])) {
            Ok(_) => panic!("{flag}: 잘못된 앞 값을 받아들였다"),
            Err(e) => e,
        };
        assert!(error.contains(flag) && error.contains("abc"), "{flag}: 이유를 안 말한다: {error}");
    }
}

/// 대조 — 올바른 중복은 받고 **마지막 값**을 쓴다. 없으면 "중복이면 거부" 로도 위가 통과한다.
#[test]
fn a_valid_numeric_duplicate_still_uses_the_last_value() {
    let config = parse_config_from_args(&with(
        legacy_lane(),
        &["--renew-extension-ms", "7", "--renew-extension-ms", "5"],
    ))
    .expect("올바른 중복을 거부했다");
    assert_eq!(config.renew_extension_ms, 5);
}
