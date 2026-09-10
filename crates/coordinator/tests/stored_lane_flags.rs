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
