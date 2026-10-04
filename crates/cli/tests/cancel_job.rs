//! `gputeer cancel-job` — 명령줄 배선(실행 알림 계약 v18q · 2026-10-05).
//!
//! 판정 · 한 커밋 · 늦은 증거 처리는 `crates/coordinator/tests/run_notice_store.rs` 의 「운영자 취소」 절이 잰다. 여기서는 인자 · 오류 전달 · 성공 출력 ·
//! 멱등 출력만 본다. `release-lost-node --failover-grace-ms` 의 숫자 검사도 여기서 본다.

use std::process::Command;

use gputeer_coordinator::job_store::{AcceptedJobSubmission, CoordinatorJobStore};

fn run_cli(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_gputeer"))
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

#[test]
fn cancel_job_refuses_bad_arguments_cancels_a_submitted_job_once_and_reports_the_first_cancellation_again() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("control.sqlite3");
    let db_s = db.to_string_lossy().to_string();
    CoordinatorJobStore::open(&db)
        .unwrap()
        .submit_accepted(
            &AcceptedJobSubmission {
                idempotency_key: [1; 16],
                job_id: "job-1".into(),
                submitter_device_id: "submitter-1".into(),
                manifest_hash: [1; 32],
                deadline_unix_ms: None,
                max_queue_duration_ms: None,
            },
            100,
        )
        .unwrap();

    let (ok, out) = run_cli(&["cancel-job", "--control-db", &db_s, "--job", "job-1"]);
    assert!(!ok && out.contains("--operator-statement"), "{out}");
    let (ok, out) = run_cli(&["cancel-job", "--control-db", &db_s, "--job", "job-1", "--operator-statement", "s", "--extra", "1"]);
    assert!(!ok && out.contains("모르는 인자"), "{out}");
    let (ok, out) = run_cli(&["cancel-job", "--control-db", &db_s, "--job", "job-1", "--operator-statement", "  "]);
    assert!(!ok && out.contains("진술"), "{out}");
    let (ok, out) = run_cli(&["cancel-job", "--control-db", &db_s, "--job", "job-missing", "--operator-statement", "운영자"]);
    assert!(!ok && out.contains("job-missing"), "{out}");

    let (ok, out) = run_cli(&["cancel-job", "--control-db", &db_s, "--job", "job-1", "--operator-statement", "시험: 잘못 제출"]);
    assert!(ok, "취소하지 못했다: {out}");
    assert!(
        out.contains("JOB_CANCELLED job_id=job-1 from=SUBMITTED latest_attempt=- lease_revoked=false deferred=false already=false"),
        "{out}"
    );
    let (ok, out) = run_cli(&["cancel-job", "--control-db", &db_s, "--job", "job-1", "--operator-statement", "다시"]);
    assert!(ok && out.contains("already=true"), "두 번째 취소가 멱등이 아니다: {out}");

    let (ok, out) = run_cli(&[
        "release-lost-node",
        "--control-db",
        &db_s,
        "--node",
        "node-x",
        "--operator-statement",
        "운영자",
        "--failover-grace-ms",
        "abc",
    ]);
    assert!(!ok && out.contains("--failover-grace-ms"), "{out}");
}
