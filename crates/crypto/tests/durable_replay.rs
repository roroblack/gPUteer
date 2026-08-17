//! SQLite 영속 replay 저장소의 계약·negative test.
//!
//! 정상 경로만으로는 영속성과 크래시 안전성을 입증할 수 없으므로,
//! 재시작·상한·시계 오류·비정상 nonce·미완료 트랜잭션을 함께 검사한다.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use gputeer_crypto::durable_replay::DurableReplayGuard;
use gputeer_crypto::replay::MAX_GC_ADVANCE_MS;
use gputeer_protocol::canonical::Domain;
use gputeer_protocol::signing::{
    ReplayDecision, ReplayGuard, ReplayStoreError,
};
use rusqlite::{params, Connection};
use tempfile::tempdir;

const NOW: u64 = 1_755_200_000_000;

fn nonce(value: u8) -> [u8; 16] {
    let mut result = [0u8; 16];
    result[0] = value;
    result
}

fn record(
    guard: &mut DurableReplayGuard,
    signer: &str,
    domain: Domain,
    value: u8,
    retain_until_ms: u64,
) -> Result<ReplayDecision, ReplayStoreError> {
    let nonce = nonce(value);
    guard.check_and_record(signer, domain, &nonce, retain_until_ms)
}

fn journal_path(database: &PathBuf) -> PathBuf {
    let file_name = database
        .file_name()
        .expect("테스트 데이터베이스 파일 이름이 있어야 한다")
        .to_string_lossy();

    database.with_file_name(format!("{file_name}-journal"))
}

#[test]
fn first_use_is_fresh_and_restart_is_duplicate() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replay.sqlite");

    {
        let mut guard = DurableReplayGuard::open(&database).unwrap();

        assert_eq!(
            record(&mut guard, "device-a", Domain::Grant, 1, NOW + 120_000)
                .unwrap(),
            ReplayDecision::Fresh
        );

        assert_eq!(
            record(&mut guard, "device-a", Domain::Grant, 1, NOW + 120_000)
                .unwrap(),
            ReplayDecision::Duplicate
        );

        assert_eq!(guard.entry_count().unwrap(), 1);
        assert!(guard.is_effective());
    }

    let mut reopened = DurableReplayGuard::open(&database).unwrap();

    assert_eq!(
        record(
            &mut reopened,
            "device-a",
            Domain::Grant,
            1,
            NOW + 120_000
        )
        .unwrap(),
        ReplayDecision::Duplicate,
        "저장소를 닫았다 다시 열었는데 replay 기록이 사라졌다"
    );

    assert_eq!(
        record(
            &mut reopened,
            "device-a",
            Domain::Grant,
            2,
            NOW + 120_000
        )
        .unwrap(),
        ReplayDecision::Fresh,
        "새 nonce까지 무조건 거부하는 구현은 정상 경로가 아니다"
    );

    assert_eq!(reopened.entry_count().unwrap(), 2);
}

#[test]
fn key_contains_device_domain_and_nonce() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replay.sqlite");
    let mut guard = DurableReplayGuard::open(&database).unwrap();

    assert_eq!(
        record(&mut guard, "device-a", Domain::Grant, 1, NOW + 120_000)
            .unwrap(),
        ReplayDecision::Fresh
    );

    assert_eq!(
        record(&mut guard, "device-b", Domain::Grant, 1, NOW + 120_000)
            .unwrap(),
        ReplayDecision::Fresh,
        "sender_device_id가 키에서 빠졌다"
    );

    assert_eq!(
        record(
            &mut guard,
            "device-a",
            Domain::LeaseRenew,
            1,
            NOW + 120_000
        )
        .unwrap(),
        ReplayDecision::Fresh,
        "domain_tag가 키에서 빠졌다"
    );

    assert_eq!(guard.entry_count().unwrap(), 3);
}

#[test]
fn signer_quota_does_not_starve_other_signers() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replay.sqlite");

    let mut guard =
        DurableReplayGuard::open_with_capacities(&database, 6, 3).unwrap();

    for value in 0..3 {
        assert_eq!(
            record(
                &mut guard,
                "noisy-device",
                Domain::Grant,
                value,
                NOW + 120_000
            )
            .unwrap(),
            ReplayDecision::Fresh
        );
    }

    match record(
        &mut guard,
        "noisy-device",
        Domain::Grant,
        99,
        NOW + 120_000,
    )
    .unwrap_err()
    {
        ReplayStoreError::SignerQuotaExceeded { signer_id, quota } => {
            assert_eq!(signer_id, "noisy-device");
            assert_eq!(quota, 3);
        }
        other => panic!("서명자 quota가 아닌 오류가 반환되었다: {other:?}"),
    }

    assert_eq!(
        record(
            &mut guard,
            "quiet-device",
            Domain::Grant,
            1,
            NOW + 120_000
        )
        .unwrap(),
        ReplayDecision::Fresh,
        "한 서명자의 quota가 다른 서명자를 막았다"
    );

    assert_eq!(guard.entry_count().unwrap(), 4);
}

#[test]
fn cache_full_does_not_evict_unexpired_entries() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replay.sqlite");

    let mut guard =
        DurableReplayGuard::open_with_capacities(&database, 2, 2).unwrap();

    assert_eq!(
        record(&mut guard, "device-a", Domain::Grant, 1, NOW + 100_000)
            .unwrap(),
        ReplayDecision::Fresh
    );
    assert_eq!(
        record(&mut guard, "device-b", Domain::Grant, 1, NOW + 100_000)
            .unwrap(),
        ReplayDecision::Fresh
    );

    assert_eq!(
        record(&mut guard, "device-a", Domain::Grant, 2, NOW + 100_000)
            .unwrap_err(),
        ReplayStoreError::CacheFull
    );

    assert_eq!(
        record(&mut guard, "device-a", Domain::Grant, 1, NOW + 100_000)
            .unwrap(),
        ReplayDecision::Duplicate
    );
    assert_eq!(
        record(&mut guard, "device-b", Domain::Grant, 1, NOW + 100_000)
            .unwrap(),
        ReplayDecision::Duplicate
    );
    assert_eq!(guard.entry_count().unwrap(), 2);
}

#[test]
fn invalid_nonce_does_not_consume_a_valid_nonce_slot() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replay.sqlite");
    let mut guard = DurableReplayGuard::open(&database).unwrap();

    let error = guard
        .check_and_record(
            "device-a",
            Domain::Grant,
            &[7u8; 15],
            NOW + 120_000,
        )
        .unwrap_err();

    // ★ 2026-08-17 분류 정정 (독립 검수).
    //   전에는 `Io` 였다 — 그것은 "우리 쪽 디스크 장애" 라는 뜻이다.
    //   길이가 틀린 nonce 는 **입력 위반**이지 디스크 고장이 아니다.
    //   섞으면 운영자가 malformed request 를 디스크 고장으로 읽는다.
    assert!(
        matches!(error, ReplayStoreError::InvalidNonce { len: 15 }),
        "nonce 길이 위반이 {error:?} 로 보고됐다 — 입력 위반과 저장소 장애를 섞었다"
    );
    assert_eq!(
        guard.entry_count().unwrap(),
        0,
        "잘못된 nonce가 저장소 공간을 소비했다"
    );

    assert_eq!(
        record(&mut guard, "device-a", Domain::Grant, 7, NOW + 120_000)
            .unwrap(),
        ReplayDecision::Fresh
    );
}

#[test]
fn gc_releases_quota_only_after_expiration() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replay.sqlite");

    let mut guard =
        DurableReplayGuard::open_with_capacities(&database, 10, 2).unwrap();

    guard
        .check_and_record(
            "device-a",
            Domain::Grant,
            &nonce(1),
            NOW + 1_000,
        )
        .unwrap();
    guard
        .check_and_record(
            "device-a",
            Domain::Grant,
            &nonce(2),
            NOW + 100_000,
        )
        .unwrap();

    assert_eq!(guard.signer_usage("device-a").unwrap(), 2);

    assert_eq!(guard.gc(NOW).unwrap(), 0);
    assert_eq!(guard.entry_count().unwrap(), 2);

    assert_eq!(guard.gc(NOW + 2_000).unwrap(), 1);
    assert_eq!(guard.entry_count().unwrap(), 1);
    assert_eq!(guard.signer_usage("device-a").unwrap(), 1);

    assert_eq!(
        record(
            &mut guard,
            "device-a",
            Domain::Grant,
            3,
            NOW + 100_000
        )
        .unwrap(),
        ReplayDecision::Fresh
    );
}

#[test]
fn clock_state_survives_restart_and_blocks_bad_gc() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replay.sqlite");
    let retain_until = NOW + 16 * 60 * 1_000;

    {
        let mut guard =
            DurableReplayGuard::open_with_capacities(&database, 10, 10)
                .unwrap();

        guard
            .check_and_record(
                "device-a",
                Domain::Grant,
                &nonce(1),
                retain_until,
            )
            .unwrap();

        assert_eq!(guard.gc(NOW).unwrap(), 0);
    }

    let mut reopened =
        DurableReplayGuard::open_with_capacities(&database, 10, 10).unwrap();

    assert_eq!(
        reopened
            .gc(NOW + MAX_GC_ADVANCE_MS * 100)
            .unwrap(),
        0,
        "미래 시각 한 번으로 유효한 nonce가 삭제되었다"
    );
    assert_eq!(reopened.clock_jumps().unwrap(), 1);
    assert_eq!(
        reopened
            .check_and_record(
                "device-a",
                Domain::Grant,
                &nonce(1),
                retain_until,
            )
            .unwrap(),
        ReplayDecision::Duplicate
    );

    assert_eq!(reopened.gc(NOW).unwrap(), 0);
    assert_eq!(reopened.clock_rollbacks().unwrap(), 1);
    assert_eq!(
        reopened
            .check_and_record(
                "device-a",
                Domain::Grant,
                &nonce(1),
                retain_until,
            )
            .unwrap(),
        ReplayDecision::Duplicate,
        "시계 되감김 뒤 유효한 nonce가 재사용 가능해졌다"
    );
}

#[test]
fn crash_recovery_discards_uncommitted_partial_record() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replay.sqlite");
    let marker = directory.path().join("child.marker");

    {
        let mut guard = DurableReplayGuard::open(&database).unwrap();
        assert_eq!(
            record(&mut guard, "device-a", Domain::Grant, 1, NOW + 120_000)
                .unwrap(),
            ReplayDecision::Fresh
        );
    }

    let status = Command::new(env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "crash_child_writes_uncommitted_record",
        ])
        .env("GPUTEER_REPLAY_CRASH_DB", &database)
        .env("GPUTEER_REPLAY_CRASH_MARKER", &marker)
        .status()
        .unwrap();

    assert!(status.success(), "크래시 시뮬레이션 프로세스가 실패했다");
    assert!(
        marker.is_file(),
        "자식 프로세스가 미완료 트랜잭션을 만들지 않았다"
    );

    let journal = journal_path(&database);
    assert!(
        journal.is_file(),
        "자식 프로세스 종료 뒤 rollback journal이 남지 않았다"
    );

    let mut reopened = DurableReplayGuard::open(&database).unwrap();

    assert_eq!(
        record(&mut reopened, "device-a", Domain::Grant, 1, NOW + 120_000)
            .unwrap(),
        ReplayDecision::Duplicate,
        "commit된 기존 기록이 크래시 복구 뒤 사라졌다"
    );

    assert_eq!(
        record(&mut reopened, "device-a", Domain::Grant, 2, NOW + 120_000)
            .unwrap(),
        ReplayDecision::Fresh,
        "commit되지 않은 부분 기록이 복구 뒤 남았다"
    );
}

/// 부모 테스트가 자식 프로세스로 실행하는 크래시 시뮬레이션 도우미.
#[test]
#[ignore]
fn crash_child_writes_uncommitted_record() {
    let database = env::var_os("GPUTEER_REPLAY_CRASH_DB")
        .expect("크래시 시뮬레이션 데이터베이스 경로가 없다");
    let marker = env::var_os("GPUTEER_REPLAY_CRASH_MARKER")
        .expect("크래시 시뮬레이션 표식 경로가 없다");

    let connection = Connection::open(PathBuf::from(database)).unwrap();

    connection
        .execute_batch("BEGIN IMMEDIATE;")
        .unwrap();

    connection
        .execute(
            "INSERT INTO replay_entries(
                sender_device_id,
                domain_tag,
                nonce,
                retain_until_ms
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                "device-a",
                Domain::Grant.as_str(),
                nonce(2).as_slice(),
                NOW + 120_000
            ],
        )
        .unwrap();

    fs::write(marker, b"uncommitted transaction created").unwrap();

    // 정상 종료하지 않고 프로세스를 끝내 rollback journal을 남긴다.
    std::process::exit(0);
}

// ══════════════════════════════════════════════════════════════════
// ★ 2026-08-17 추가 — 초안이 **주장했지만 검증하지 않은 것**
//
//   초안 문서는 "동시 프로세스는 지원합니다. SQLite 가 직렬화하며,
//   락 대기 초과는 LockTimeout 으로 반환합니다" 라고 적었다.
//   그런데 그것을 확인하는 테스트가 **하나도 없었다.**
//
//   ★ 강제 장치가 없는 규범이 규범이 아니듯,
//     테스트가 없는 주장은 주장이 아니라 희망이다.
// ══════════════════════════════════════════════════════════════════

/// 두 연결이 같은 파일을 열고 **같은 nonce** 를 기록하면
/// 정확히 하나만 `Fresh` 여야 한다.
///
/// ★ **이름을 고쳤다** (독립 검수 2026-08-17).
///   원래 이름은 `two_connections_race_on_the_same_nonce` 였는데
///   **경쟁 테스트가 아니다** — 두 연결을 만들지만 호출은 순차적이다.
///   실제 락 경쟁 · `busy_timeout` · `LockTimeout` 은 검증하지 않는다.
///   이름이 사실을 잘못 말하면 "동시성을 검증했다" 고 오해하게 된다.
///
/// 이 테스트가 실제로 보는 것: **두 연결이 같은 파일 상태를 공유하는가.**
/// 진짜 경쟁 테스트는 별도 작업이다 (DoD-10 limitations).
#[test]
fn two_connections_share_state_sequentially() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("replay.sqlite3");

    let mut a = DurableReplayGuard::open(&path).unwrap();
    let mut b = DurableReplayGuard::open(&path).unwrap();

    let n = nonce(7);
    let r1 = a.check_and_record("dev-a", Domain::Grant, &n, NOW + 120_000);
    let r2 = b.check_and_record("dev-a", Domain::Grant, &n, NOW + 120_000);

    let outcomes = [r1, r2];
    let fresh = outcomes
        .iter()
        .filter(|r| matches!(r, Ok(ReplayDecision::Fresh)))
        .count();
    let dup = outcomes
        .iter()
        .filter(|r| matches!(r, Ok(ReplayDecision::Duplicate)))
        .count();

    assert_eq!(
        (fresh, dup),
        (1, 1),
        "★ 같은 nonce 를 두 연결이 각각 Fresh 로 받았다 — \
         다중 프로세스에서 replay 방어가 없다. 결과: {outcomes:?}"
    );
}

/// 서로 다른 연결이 **서로 다른** nonce 를 쓰면 둘 다 통과해야 한다.
///
/// 비공허성 — 위 테스트가 "무조건 하나는 거부" 로 통과하는 것이 아님을 보인다.
#[test]
fn two_connections_with_different_nonces_both_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("replay.sqlite3");

    let mut a = DurableReplayGuard::open(&path).unwrap();
    let mut b = DurableReplayGuard::open(&path).unwrap();

    assert_eq!(
        a.check_and_record("dev-a", Domain::Grant, &nonce(1), NOW + 120_000)
            .unwrap(),
        ReplayDecision::Fresh
    );
    assert_eq!(
        b.check_and_record("dev-a", Domain::Grant, &nonce(2), NOW + 120_000)
            .unwrap(),
        ReplayDecision::Fresh
    );
}

/// ★ 호출자가 **영속 여부를 알 수 있어야 한다.**
///
/// 초안에는 `is_durable()` 이 없었다. 영속 저장소를 만들어 놓고
/// 그 사실을 알릴 방법이 없으면, 메모리 구현과 구분되지 않는다.
#[test]
fn durable_guard_says_it_is_durable() {
    let dir = tempfile::tempdir().unwrap();
    let g = DurableReplayGuard::open(dir.path().join("replay.sqlite3")).unwrap();
    assert!(g.is_durable(), "영속 저장소가 is_durable() 로 false 를 반환한다");
    assert!(g.is_effective());

    // 대조군 — 메모리 구현은 false 다. 둘이 구분되지 않으면 신호가 없는 것이다.
    assert!(!gputeer_crypto::InMemoryReplayGuard::new().is_durable());
}
