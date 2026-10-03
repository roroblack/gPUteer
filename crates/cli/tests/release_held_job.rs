//! `gputeer release-held-job` — 명령줄 배선(실행 알림 계약 v18k §6 (3) · 계획 조각 7c).
//!
//! 판정 · 한 커밋 · 최종 Job 의 자원 해제는 `crates/coordinator/tests/run_notice_store.rs` 가 잰다. 여기서는 인자 · 오류 전달 · 성공 출력만 본다.

use std::path::Path;
use std::process::Command;

use gputeer_coordinator::job_store::{AcceptedJobSubmission, CoordinatorJobStore};
use gputeer_coordinator::run_notice_store::CoordinatorRunNoticeStore;

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

fn job_with_unreported_hold(path: &Path) {
    CoordinatorJobStore::open(path)
        .unwrap()
        .submit_accepted(
            &AcceptedJobSubmission {
                idempotency_key: [1; 16],
                job_id: "job-held".into(),
                submitter_device_id: "submitter-1".into(),
                manifest_hash: [1; 32],
                deadline_unix_ms: None,
                max_queue_duration_ms: None,
            },
            100,
        )
        .unwrap();
    CoordinatorRunNoticeStore::open(path).unwrap();
    rusqlite::Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO coordinator_job_holds VALUES ('job-held', 'attempt-old', 'UNREPORTED_SIDE_EFFECT_RISK', NULL, ?1)",
            [1u64.to_be_bytes().to_vec()],
        )
        .unwrap();
}

#[test]
fn release_held_job_refuses_bad_arguments_and_unknown_jobs_and_releases_a_held_job() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("control.sqlite3");
    let db_s = db.to_string_lossy().to_string();

    let (ok, out) = run_cli(&["release-held-job", "--control-db", &db_s, "--job", "job-held"]);
    assert!(!ok && out.contains("--operator-statement"), "{out}");
    let (ok, out) = run_cli(&["release-held-job", "--control-db", &db_s, "--job", "x", "--operator-statement", "s", "--extra", "1"]);
    assert!(!ok && out.contains("모르는 인자"), "{out}");
    let (ok, out) = run_cli(&["release-held-job", "--control-db", &db_s, "--job", "job-missing", "--operator-statement", "운영자: 확인"]);
    assert!(!ok && out.contains("job-missing"), "{out}");

    job_with_unreported_hold(&db);
    let (ok, out) = run_cli(&[
        "release-held-job",
        "--control-db",
        &db_s,
        "--job",
        "job-held",
        "--operator-statement",
        "시험: 그 PC 가 꺼진 것을 확인",
    ]);
    assert!(ok, "보류를 풀지 못했다: {out}");
    assert!(out.contains("HELD_JOB_RELEASED job_id=job-held attempts=attempt-old job_final=false"), "{out}");
    let (ok, out) = run_cli(&["release-held-job", "--control-db", &db_s, "--job", "job-held", "--operator-statement", "다시"]);
    assert!(!ok && out.contains("풀 보류"), "두 번째 해제가 통과했다: {out}");
}
