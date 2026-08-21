use std::sync::{Arc, Barrier};
use std::thread;

use gputeer_coordinator::job_store::{
    AcceptedJobSubmission, CoordinatorJobStore, JobStoreError,
};

fn submission(job_id: &str, manifest_byte: u8) -> AcceptedJobSubmission {
    AcceptedJobSubmission {
        idempotency_key: [42; 16],
        job_id: job_id.to_string(),
        submitter_device_id: "submitter-1".to_string(),
        manifest_hash: [manifest_byte; 32],
        deadline_unix_ms: Some(50_000),
        max_queue_duration_ms: Some(10_000),
    }
}

#[test]
fn concurrent_identical_submit_creates_once_and_replays_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.sqlite3");
    CoordinatorJobStore::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let handles: Vec<_> = [111_u64, 222_u64]
        .into_iter()
        .map(|timestamp| {
            let path = path.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let mut store = CoordinatorJobStore::open(path).unwrap();
                barrier.wait();
                store
                    .submit_accepted(&submission("job-1", 7), timestamp)
                    .unwrap()
            })
        })
        .collect();

    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.created).count(), 1);
    assert_eq!(results.iter().filter(|result| !result.created).count(), 1);
    assert_eq!(results[0].job, results[1].job);
    assert!(matches!(results[0].job.submitted_at_unix_ms, 111 | 222));
}

#[test]
fn concurrent_key_reuse_with_changed_payload_has_one_winner_and_no_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.sqlite3");
    CoordinatorJobStore::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let handles: Vec<_> = [("job-a", 7_u8), ("job-b", 8_u8)]
        .into_iter()
        .map(|(job_id, manifest_byte)| {
            let path = path.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let mut store = CoordinatorJobStore::open(path).unwrap();
                barrier.wait();
                let request = submission(job_id, manifest_byte);
                (request, store.submit_accepted(&submission(job_id, manifest_byte), 100))
            })
        })
        .collect();

    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        results.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|(_, result)| matches!(result, Err(JobStoreError::IdempotencyConflict { .. })))
            .count(),
        1
    );

    let winner = results
        .iter()
        .find_map(|(request, result)| result.as_ref().ok().map(|stored| (request, stored)))
        .unwrap();
    let store = CoordinatorJobStore::open(&path).unwrap();
    let durable = store.get(&winner.0.job_id).unwrap().unwrap();
    assert_eq!(durable, winner.1.job);
    let loser_job_id = if winner.0.job_id == "job-a" { "job-b" } else { "job-a" };
    assert!(store.get(loser_job_id).unwrap().is_none());
}
