//! ★ 2026-10-01 (검수 gr1 · gr2 · signing.md §6.8) — 서명된 재배치 유예를 켰는데 Lease 저장소가 없으면, 파서를 지나치는 라이브러리
//!   호출자(`run()` · `multi_agent::run_multi_agent()`)도 시작에서 거부된다. 거부하지 않으면 유예 없는 v1 Lease 가 조용히 나간다.

use gputeer_coordinator::{parse_config_from_args, CoordinatorConfig};

fn config_with_grace_but_no_lease_db() -> CoordinatorConfig {
    let args: Vec<String> = [
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        &"11".repeat(32),
        "--peer-pubkey",
        "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29",
        "--coordinator-device-id",
        "01JCOORDGRACEGUARD0000001",
        "--agent-device-id",
        "01JAGENTGRACEGUARD0000001",
        "--grant-id",
        "01JGRANTGRACEGUARD0000001",
        "--attempt-id",
        "01JATTEMPTGRACEGUARD00001",
        "--lease-id",
        "01JLEASEGRACEGUARD0000001",
        "--job-id",
        "01JJOBGRACEGUARD000000001",
        "--i-understand-legacy-mode-is-unsafe",
        "true",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    // 파서는 스위치가 꺼진 설정만 받는다 — 라이브러리 호출자가 그 뒤에 켜는 경우를 흉내 낸다.
    let mut config = parse_config_from_args(&args).expect("설정 파싱");
    config.signed_reassignment_grace_ms = 30_000;
    config
}

#[test]
fn run_refuses_a_signed_grace_without_a_lease_store() {
    let error = gputeer_coordinator::run(config_with_grace_but_no_lease_db())
        .expect_err("저장소 없이 유예를 켠 설정을 받았다");
    assert!(error.contains("SIGNED_GRACE_NEEDS_LEASE_DB"), "{error}");
}

#[test]
fn the_multi_agent_entry_refuses_a_signed_grace_without_a_lease_store() {
    let error =
        gputeer_coordinator::multi_agent::run_multi_agent(config_with_grace_but_no_lease_db())
            .expect_err("저장소 없이 유예를 켠 설정을 받았다");
    assert!(error.contains("SIGNED_GRACE_NEEDS_LEASE_DB"), "{error}");
}
