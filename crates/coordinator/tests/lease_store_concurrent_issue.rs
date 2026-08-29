//! `CoordinatorLeaseStore::get_or_issue()`의 실제 동시 최초 발급 검증.
//!
//! `crates/crypto/tests/durable_replay_race.rs`와 같은 방식으로 스키마는
//! 미리 만들고, 각 스레드가 같은 파일에 자기 SQLite 연결을 연 다음
//! `Barrier` 직후 저장 호출을 시작한다. 순차적인 두 연결 검사가 아니다.

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use gputeer_coordinator::lease_store::{CoordinatorLeaseStore, LeaseStoreError, StoredLease};
use rusqlite::{Connection, OptionalExtension};

const ROUNDS: usize = 16;
const NOW_UNIX_MS: u64 = 1_755_200_000_000;

#[derive(Clone, Copy)]
enum CandidateMarker {
    A,
    B,
}

fn candidate(round: usize, marker: CandidateMarker) -> StoredLease {
    let (
        holder_node_id,
        marker_offset,
        issuing_coordinator_id,
        coordinator_term,
        max_total_duration_seconds,
    ) = match marker {
        CandidateMarker::A => ("node-a", 1_000, "coordinator-a", 101, 60),
        CandidateMarker::B => ("node-b", 2_000, "coordinator-b", 202, 61),
    };

    StoredLease {
        lease_id: format!("lease-concurrent-issue-{round:02}"),
        job_id: format!("job-concurrent-issue-{round:02}"),
        attempt_id: format!("attempt-concurrent-issue-{round:02}"),
        holder_node_id: holder_node_id.to_string(),
        fence_epoch: marker_offset + round as u64,
        expires_at_unix_ms: NOW_UNIX_MS + 600_000 + marker_offset + round as u64,
        issuing_coordinator_id: issuing_coordinator_id.to_string(),
        coordinator_term,
        issued_at_unix_ms: NOW_UNIX_MS + marker_offset + round as u64,
        renew_after_unix_ms: NOW_UNIX_MS + 300_000 + marker_offset + round as u64,
        max_total_duration_seconds,
        revoked_at_unix_ms: None,
    }
}

fn assert_candidates_are_distinguishable(a: &StoredLease, b: &StoredLease) {
    // 같은 lease를 경쟁시키고 holder_node_id에서 충돌시키려면 이 값들은 같아야 한다.
    assert_eq!(a.lease_id, b.lease_id);
    assert_eq!(a.job_id, b.job_id);
    assert_eq!(a.attempt_id, b.attempt_id);

    // issuing_coordinator_id는 holder_node_id보다 나중에 비교되므로 달라도 충돌 필드는
    // holder_node_id다. 최초 발급 INSERT가 항상 NULL로 쓰는 revoke 상태만 표식에서 제외한다.
    assert_ne!(a.holder_node_id, b.holder_node_id);
    assert_ne!(a.fence_epoch, b.fence_epoch);
    assert_ne!(a.expires_at_unix_ms, b.expires_at_unix_ms);
    assert_ne!(a.issuing_coordinator_id, b.issuing_coordinator_id);
    assert_ne!(a.coordinator_term, b.coordinator_term);
    assert_ne!(a.issued_at_unix_ms, b.issued_at_unix_ms);
    assert_ne!(a.renew_after_unix_ms, b.renew_after_unix_ms);
    assert_ne!(a.max_total_duration_seconds, b.max_total_duration_seconds);
    assert_eq!(a.revoked_at_unix_ms, None);
    assert_eq!(b.revoked_at_unix_ms, None);
}

fn assert_complete_winner(stored: &StoredLease, winner: &StoredLease, loser: &StoredLease) {
    assert_eq!(
        stored, winner,
        "durable row is not the complete winning candidate"
    );

    // 충돌 판정상 같아야 하는 identity와 최초 발급 시 항상 NULL인 revoke 상태를
    // 제외한 모든 저장 필드가 패자의 표식과 다름을 명시해 부분 덮어쓰기를 검출한다.
    assert_ne!(stored.holder_node_id, loser.holder_node_id);
    assert_ne!(stored.fence_epoch, loser.fence_epoch);
    assert_ne!(stored.expires_at_unix_ms, loser.expires_at_unix_ms);
    assert_ne!(stored.issuing_coordinator_id, loser.issuing_coordinator_id);
    assert_ne!(stored.coordinator_term, loser.coordinator_term);
    assert_ne!(stored.issued_at_unix_ms, loser.issued_at_unix_ms);
    assert_ne!(stored.renew_after_unix_ms, loser.renew_after_unix_ms);
    assert_ne!(
        stored.max_total_duration_seconds,
        loser.max_total_duration_seconds
    );
}

#[test]
fn complete_winner_assertion_rejects_each_loser_marker() {
    let winner = candidate(0, CandidateMarker::A);
    let loser = candidate(0, CandidateMarker::B);
    assert_candidates_are_distinguishable(&winner, &loser);

    macro_rules! assert_loser_field_is_rejected {
        ($field:ident) => {{
            let mut mixed = winner.clone();
            mixed.$field = loser.$field.clone();
            let rejected = std::panic::catch_unwind(|| {
                assert_complete_winner(&mixed, &winner, &loser);
            });
            assert!(
                rejected.is_err(),
                "winner assertion accepted loser field {}",
                stringify!($field)
            );
        }};
    }

    assert_loser_field_is_rejected!(holder_node_id);
    assert_loser_field_is_rejected!(fence_epoch);
    assert_loser_field_is_rejected!(expires_at_unix_ms);
    assert_loser_field_is_rejected!(issuing_coordinator_id);
    assert_loser_field_is_rejected!(coordinator_term);
    assert_loser_field_is_rejected!(issued_at_unix_ms);
    assert_loser_field_is_rejected!(renew_after_unix_ms);
    assert_loser_field_is_rejected!(max_total_duration_seconds);
}

fn decode_u64(bytes: Vec<u8>, field: &str) -> u64 {
    let bytes: [u8; 8] = bytes
        .try_into()
        .unwrap_or_else(|_| panic!("{field} is not an eight-byte u64"));
    u64::from_be_bytes(bytes)
}

fn query_stored_directly(connection: &Connection, lease_id: &str) -> StoredLease {
    let raw = connection
        .query_row(
            "SELECT lease_id, job_id, attempt_id, holder_node_id, fence_epoch,
                    expires_at_unix_ms, issuing_coordinator_id, coordinator_term,
                    issued_at_unix_ms, renew_after_unix_ms,
                    max_total_duration_seconds, revoked_at_unix_ms
             FROM coordinator_leases WHERE lease_id = ?1",
            [lease_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                    row.get::<_, Vec<u8>>(8)?,
                    row.get::<_, Vec<u8>>(9)?,
                    row.get::<_, Vec<u8>>(10)?,
                    row.get::<_, Option<Vec<u8>>>(11)?,
                ))
            },
        )
        .optional()
        .expect("direct SQLite query failed")
        .unwrap_or_else(|| panic!("lease row {lease_id} is missing"));

    StoredLease {
        lease_id: raw.0,
        job_id: raw.1,
        attempt_id: raw.2,
        holder_node_id: raw.3,
        fence_epoch: decode_u64(raw.4, "fence_epoch"),
        expires_at_unix_ms: decode_u64(raw.5, "expires_at_unix_ms"),
        issuing_coordinator_id: raw.6,
        coordinator_term: decode_u64(raw.7, "coordinator_term"),
        issued_at_unix_ms: decode_u64(raw.8, "issued_at_unix_ms"),
        renew_after_unix_ms: decode_u64(raw.9, "renew_after_unix_ms"),
        max_total_duration_seconds: decode_u64(raw.10, "max_total_duration_seconds"),
        revoked_at_unix_ms: raw.11.map(|bytes| decode_u64(bytes, "revoked_at_unix_ms")),
    }
}

#[test]
fn concurrent_first_issue_has_one_complete_winner() {
    let dir = tempfile::tempdir().expect("temp directory creation failed");
    let path = dir.path().join("lease.sqlite3");

    // 스키마 생성 자체의 경합을 측정에 섞지 않는다.
    drop(CoordinatorLeaseStore::open(&path).expect("schema initialization failed"));
    let blocker = Connection::open(&path).expect("contention connection failed");

    let mut winner_loser_pairs = Vec::with_capacity(ROUNDS);

    for round in 0..ROUNDS {
        let candidate_a = candidate(round, CandidateMarker::A);
        let candidate_b = candidate(round, CandidateMarker::B);
        assert_candidates_are_distinguishable(&candidate_a, &candidate_b);
        let candidates = [candidate_a, candidate_b];
        let connections_ready = Arc::new(Barrier::new(candidates.len() + 1));
        let calls_start = Arc::new(Barrier::new(candidates.len() + 1));
        let mut handles = Vec::with_capacity(candidates.len());

        for candidate in candidates {
            let path = path.clone();
            let connections_ready = Arc::clone(&connections_ready);
            let calls_start = Arc::clone(&calls_start);
            handles.push(thread::spawn(move || {
                // 각 스레드가 별도 SQLite 연결을 준비한 뒤 같은 barrier에서 출발한다.
                let mut store =
                    CoordinatorLeaseStore::open(path).expect("thread-local connection failed");
                connections_ready.wait();
                calls_start.wait();
                let result = store.get_or_issue(&candidate, NOW_UNIX_MS);
                (candidate, result)
            }));
        }

        // 두 store 연결이 모두 준비된 뒤 잠깐 RESERVED lock을 쥔다. Immediate
        // transaction이면 두 호출 모두 SELECT 전에 여기서 대기하고, 해제 뒤
        // 한 호출씩 직렬화된다. 50ms는 store의 1초 busy_timeout보다 짧다.
        connections_ready.wait();
        blocker
            .execute_batch("BEGIN IMMEDIATE;")
            .expect("contention lock acquisition failed");
        calls_start.wait();
        thread::sleep(Duration::from_millis(50));
        blocker
            .execute_batch("COMMIT;")
            .expect("contention lock release failed");

        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("issuer thread panicked"))
            .collect();

        let successful: Vec<_> = outcomes
            .iter()
            .filter_map(|(candidate, result)| match result {
                Ok(stored) => {
                    assert_eq!(
                        stored, candidate,
                        "round {round}: success did not return its candidate"
                    );
                    Some(candidate.clone())
                }
                Err(_) => None,
            })
            .collect();
        assert_eq!(
            successful.len(),
            1,
            "round {round}: exactly one first issuer must succeed; outcomes={outcomes:?}"
        );
        let winner = successful.into_iter().next().unwrap();

        let mut rejected = 0;
        let mut losing_candidate = None;
        for (loser, result) in outcomes {
            if result.is_ok() {
                continue;
            }
            rejected += 1;
            losing_candidate = Some(loser.clone());
            match result {
                Err(LeaseStoreError::IdentityConflict {
                    field,
                    stored,
                    requested,
                }) => {
                    assert_eq!(field, "holder_node_id", "round {round}");
                    assert_eq!(stored, winner.holder_node_id, "round {round}");
                    assert_eq!(requested, loser.holder_node_id, "round {round}");
                }
                Err(LeaseStoreError::LockTimeout) => {
                    // LockTimeout은 안전한 거부다. 경쟁이 끝난 뒤에는 반드시
                    // 영속 승자를 읽고 holder identity conflict로 수렴해야 한다.
                    let mut retry_store = CoordinatorLeaseStore::open(&path)
                        .expect("sequential retry connection failed");
                    match retry_store.get_or_issue(&loser, NOW_UNIX_MS) {
                        Err(LeaseStoreError::IdentityConflict {
                            field,
                            stored,
                            requested,
                        }) => {
                            assert_eq!(field, "holder_node_id", "round {round} retry");
                            assert_eq!(stored, winner.holder_node_id, "round {round} retry");
                            assert_eq!(requested, loser.holder_node_id, "round {round} retry");
                        }
                        other => panic!(
                            "round {round}: LockTimeout retry did not converge to holder conflict: {other:?}"
                        ),
                    }
                }
                other => panic!("round {round}: unexpected losing outcome: {other:?}"),
            }
        }
        assert_eq!(
            rejected, 1,
            "round {round}: exactly one candidate must be rejected"
        );
        winner_loser_pairs.push((
            winner,
            losing_candidate.expect("rejected candidate must be recorded"),
        ));
    }

    // Store API가 아니라 별도 raw SQLite 연결로 모든 identity/epoch/time 필드를
    // 직접 읽어, 패자 값의 부분 덮어쓰기 없이 승자 전체가 남았는지 확인한다.
    let connection = Connection::open(&path).expect("final direct SQLite connection failed");
    let row_count: usize = connection
        .query_row("SELECT COUNT(*) FROM coordinator_leases", [], |row| {
            row.get(0)
        })
        .expect("lease row count query failed");
    assert_eq!(row_count, ROUNDS, "one durable row must remain per round");

    for (winner, loser) in winner_loser_pairs {
        let stored = query_stored_directly(&connection, &winner.lease_id);
        assert_complete_winner(&stored, &winner, &loser);
    }
}
