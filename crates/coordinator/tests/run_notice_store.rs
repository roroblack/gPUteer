//! 실행 알림 받기 · 저장 · 정지 확인 처리 — 한 트랜잭션(실행 알림 계약 v18k §2 · §3 · 계획 조각 4d).
//!
//! ★ 2026-10-03 05:01 — 격리 시험이다. 부르는 production 경로는 아직 없다(활성화 관문 전).

use std::path::{Path, PathBuf};

use gputeer_coordinator::inventory_store::{
    AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory,
};
use gputeer_coordinator::job_store::{AcceptedJobSubmission, CoordinatorJobStore, JobState};
use gputeer_coordinator::reservation_release::{
    CoordinatorReservationReleaseStore, KeyDirectoryProvenance, ReleaseEvidenceKind,
    ReleaseOutcome, RuntimeStopProof,
};
use gputeer_coordinator::run_notice_store::{
    answer_run_notice, CoordinatorRunNoticeStore, RunNoticeAnswerContext, RunNoticeAnswerError,
    RunNoticeEffect, RunNoticeError,
};
use gputeer_coordinator::staging_store::{CoordinatorStagingStore, StageQueuedRequest};
use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
use gputeer_protocol::attempt_state::AttemptState;
use gputeer_protocol::pb;
use gputeer_protocol::signing::{verify, NoReplayCheck, Verified};

const JOB_ID: &str = "job-1";
const ATTEMPT_ID: &str = "attempt-1";
const LEASE_ID: &str = "lease-1";
const NODE_ID: &str = "node-1";
const NOW: u64 = 4_000;
const VERIFIED_DIR: KeyDirectoryProvenance = KeyDirectoryProvenance::AuthoritativeDirectoryVerifiedByCaller;

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

/// 예약과 Attempt 가 durable 하게 존재하는 control DB 를 만든다.
///
/// `attempt_report_store` 의 단위 테스트 fixture 와 같은 모양이다 —
/// 같은 상태를 두 곳에서 다르게 만들면 어느 쪽이 맞는지 알 수 없다.
fn prepare_fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("control.sqlite3");

    let mut inventory = CoordinatorInventoryStore::open(&path).unwrap();
    inventory
        .register_agent(&AgentRegistry {
            node_id: NODE_ID.into(),
            device_id: "device-1".into(),
            owner_member_id: "owner-1".into(),
            verifying_key: vec![1; 32],
            node_state: None,
            risk_state: None,
            security_tier: None,
            isolation_class: None,
            key_protection: None,
        })
        .unwrap();
    inventory
        .update_inventory(&AgentInventory {
            node_id: NODE_ID.into(),
            inventory_revision: 7,
            observed_at_unix_ms: 90,
            gpus: Some(vec![
                GpuInventory {
                    gpu_id: "gpu-1".into(),
                    model: Some("model-1".into()),
                    healthy: Some(true),
                    available_vram_bytes: Some(16),
                },
                GpuInventory {
                    gpu_id: "gpu-2".into(),
                    model: Some("model-1".into()),
                    healthy: Some(true),
                    available_vram_bytes: Some(16),
                },
            ]),
            available_cpu_cores: Some(8),
            available_ram_bytes: Some(64),
            available_workspace_bytes: Some(64),
            allowed_workload_classes: None,
            third_party_workloads_opt_in: None,
        })
        .unwrap();
    drop(inventory);

    let mut jobs = CoordinatorJobStore::open(&path).unwrap();
    jobs.submit_accepted(
        &AcceptedJobSubmission {
            idempotency_key: [1; 16],
            job_id: JOB_ID.into(),
            submitter_device_id: "submitter-1".into(),
            manifest_hash: [1; 32],
            deadline_unix_ms: Some(10_000),
            max_queue_duration_ms: Some(5_000),
        },
        100,
    )
    .unwrap();
    jobs.start_planning(JOB_ID, 110).unwrap();
    jobs.enqueue(JOB_ID, "plan-1", 120).unwrap();
    drop(jobs);

    CoordinatorStagingStore::open(&path)
        .unwrap()
        .reserve_node_and_stage_queued_with_lease(
            &StageQueuedRequest {
                operation_key: [2; 16],
                job_id: JOB_ID.into(),
                attempt_id: ATTEMPT_ID.into(),
                lease_id: LEASE_ID.into(),
                node_id: NODE_ID.into(),
                selected_gpu_ids: vec!["gpu-1".into(), "gpu-2".into()],
                issuing_coordinator_id: "coordinator-1".into(),
                coordinator_term: 1,
                issued_at_unix_ms: 200,
                renew_after_unix_ms: 500,
                expires_at_unix_ms: 900,
                max_total_duration_seconds: 1,
            },
            7,
        )
        .unwrap();

    Fixture { _dir: dir, path }
}


fn attempt_state(path: &Path, attempt_id: &str) -> AttemptState {
    CoordinatorStagingStore::open(path)
        .unwrap()
        .get_attempt(attempt_id)
        .unwrap()
        .expect("시도가 있어야 한다")
        .state
}

fn fence(path: &Path) -> u64 {
    CoordinatorStagingStore::open(path)
        .unwrap()
        .get_attempt(ATTEMPT_ID)
        .unwrap()
        .unwrap()
        .fence_epoch
}

fn job_state(path: &Path) -> JobState {
    CoordinatorJobStore::open(path)
        .unwrap()
        .get(JOB_ID)
        .unwrap()
        .unwrap()
        .state
}

fn reservation_exists(path: &Path) -> bool {
    CoordinatorStagingStore::open(path)
        .unwrap()
        .get_node_reservation(NODE_ID)
        .unwrap()
        .is_some()
}

fn lease_revoked(path: &Path) -> bool {
    let raw: Option<Vec<u8>> = rusqlite::Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT revoked_at_unix_ms FROM coordinator_leases WHERE lease_id = ?1",
            [LEASE_ID],
            |row| row.get(0),
        )
        .unwrap();
    raw.is_some()
}

fn notice_rows(path: &Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM coordinator_attempt_run_notices", [], |row| row.get(0))
        .unwrap_or(0)
}

fn notice(
    fence_epoch: u64,
    kind: pb::RunNoticeKind,
    sequence: u64,
    observed_at: u64,
) -> Verified<pb::AttemptRunNotice> {
    let key = SigningKey::from_bytes(&[7; 32]);
    let stop = kind == pb::RunNoticeKind::StopConfirmed;
    let mut notice = pb::AttemptRunNotice {
        schema_version: 1,
        job_id: JOB_ID.into(),
        attempt_id: ATTEMPT_ID.into(),
        node_id: NODE_ID.into(),
        fence_epoch,
        kind: kind as i32,
        origin: if stop { 0 } else { pb::RunUnknownOrigin::Running as i32 },
        reason: if stop { 0 } else { pb::RunUnknownReason::ExitUnobserved as i32 },
        stop_evidence: if stop {
            pb::RunStopEvidence::ContainerAbsentConfirmed as i32
        } else {
            0
        },
        sequence,
        observed_at_unix_ms: observed_at,
        issued_at_unix_ms: observed_at + 1,
        ..Default::default()
    };
    notice.node_signature = sign(&key, &notice).to_vec();
    let mut keys = InMemoryKeyring::new();
    keys.insert(NODE_ID, key.verifying_key());
    verify(&notice, 1, &Ed25519Verifier::new(keys), 999, &mut NoReplayCheck)
        .expect("시험 알림은 서명 검증을 통과해야 한다")
}

fn stop(path: &Path, sequence: u64) -> Verified<pb::AttemptRunNotice> {
    notice(fence(path), pb::RunNoticeKind::StopConfirmed, sequence, 350)
}

fn accept_with(
    path: &Path,
    verified: &Verified<pb::AttemptRunNotice>,
    resume: Option<Vec<u8>>,
) -> Result<gputeer_coordinator::run_notice_store::RunNoticeAccepted, RunNoticeError> {
    let mut finder = move |_: &rusqlite::Connection, _: &gputeer_coordinator::job_store::StoredJob| {
        Ok(resume.clone())
    };
    CoordinatorRunNoticeStore::open(path).unwrap().accept(
        verified,
        RuntimeStopProof::NodeConfirmedStop,
        VERIFIED_DIR,
        &mut finder,
        NOW,
    )
}

/// §3 — 정지 확인이 먼저 온(불명 알림을 못 받은) CREATED 시도: 한 커밋에 알림 저장 · 시도 FAILED · Job 이 갈 곳 · Lease 폐기 · 예약 해제.
///   이어갈 지점이 있으면 Job 은 큐로, 없으면 FAILED(NO_COMMITTED_CHECKPOINT).
#[test]
fn a_stop_closes_the_attempt_moves_the_job_revokes_the_lease_and_releases_in_one_commit() {
    for (resume, expected_job) in [(Some(vec![1, 2]), JobState::Queued), (None, JobState::Failed)] {
        let fixture = prepare_fixture();
        let verified = stop(&fixture.path, 1);
        let accepted = accept_with(&fixture.path, &verified, resume.clone()).unwrap();
        assert!(accepted.created);
        assert_eq!(accepted.sequence, 1);
        assert_eq!(accepted.kind, pb::RunNoticeKind::StopConfirmed);
        assert_eq!(
            accepted.notice_hash,
            gputeer_protocol::canonical::blake3_256(&gputeer_protocol::signing::signing_input(
                verified.get()
            ))
        );
        let RunNoticeEffect::StopProcessed(done) = &accepted.effect else {
            panic!("정지 처리여야 한다: {accepted:?}");
        };
        assert_eq!(done.attempt_state_before, AttemptState::Created);
        assert!(done.attempt_closed && done.latest_attempt);
        assert!(matches!(done.release, ReleaseOutcome::Released(_)), "{:?}", done.release);
        assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Failed);
        assert_eq!(job_state(&fixture.path), expected_job, "{resume:?}");
        assert!(lease_revoked(&fixture.path));
        assert!(!reservation_exists(&fixture.path));
        assert_eq!(notice_rows(&fixture.path), 1);
        let release = CoordinatorReservationReleaseStore::open(&fixture.path)
            .unwrap()
            .get_release(ATTEMPT_ID)
            .unwrap()
            .unwrap();
        assert_eq!(release.evidence[0].kind, ReleaseEvidenceKind::StopConfirmed);
        assert_eq!(release.evidence[0].hash, accepted.notice_hash, "해제 근거 해시 = ACK 의 notice_hash");
    }
}

/// 같은 바이트 재전송은 created=false 이고 아무것도 다시 하지 않는다. 같은 번호에 다른 바이트는 거부되고 아무것도 남기지 않는다.
#[test]
fn a_resend_is_idempotent_and_other_bytes_on_the_same_sequence_are_refused() {
    let fixture = prepare_fixture();
    let verified = stop(&fixture.path, 1);
    accept_with(&fixture.path, &verified, None).unwrap();
    let again = accept_with(&fixture.path, &verified, None).unwrap();
    assert!(!again.created);
    assert_eq!(again.effect, RunNoticeEffect::Duplicate);
    assert_eq!(notice_rows(&fixture.path), 1);
    let other = notice(fence(&fixture.path), pb::RunNoticeKind::StopConfirmed, 1, 999);
    assert_eq!(
        accept_with(&fixture.path, &other, None),
        Err(RunNoticeError::SequenceConflict { sequence: 1 })
    );
    assert_eq!(notice_rows(&fixture.path), 1);
}

/// ★ 조각 6a — 불명 알림: 최신 시도(CREATED)를 GRANT_ACCEPTED 를 거쳐 RUN_UNKNOWN 으로 옮기고 Job 에 NOTICE 보류를 건다(예약 · Lease 는 그대로).
///   그 뒤의 정지 확인은 불명보다 큰 번호여야 하고, 받으면 그 시도의 보류를 푼다.
#[test]
fn a_run_unknown_holds_the_job_and_a_later_stop_releases_it() {
    let fixture = prepare_fixture();
    let unknown = notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 3, 300);
    let accepted = accept_with(&fixture.path, &unknown, None).unwrap();
    let RunNoticeEffect::RunUnknownApplied(applied) = &accepted.effect else {
        panic!("불명 처리여야 한다: {accepted:?}");
    };
    assert!(applied.attempt_moved && applied.hold_installed && applied.latest_attempt);
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::RunUnknown);
    assert_eq!(holds(&fixture.path), vec![(ATTEMPT_ID.to_string(), "NOTICE_RUN_UNKNOWN".to_string())]);
    assert_eq!(job_state(&fixture.path), JobState::Staging, "불명은 Job 상태를 옮기지 않는다");
    assert!(reservation_exists(&fixture.path) && !lease_revoked(&fixture.path));
    assert_eq!(
        accept_with(&fixture.path, &stop(&fixture.path, 2), None),
        Err(RunNoticeError::StopNotAfterUnknown {
            sequence: 2,
            latest_unknown: 3
        })
    );
    // 같은 번호는 종류가 달라도 다른 바이트 — 기본키 충돌로 거부(계약 §2 "같은 번호에 다른 바이트")
    assert_eq!(
        accept_with(&fixture.path, &stop(&fixture.path, 3), None),
        Err(RunNoticeError::SequenceConflict { sequence: 3 })
    );
    assert_eq!(notice_rows(&fixture.path), 1, "거부된 정지 확인이 남았다");
    let done = accept_with(&fixture.path, &stop(&fixture.path, 4), None).unwrap();
    let RunNoticeEffect::StopProcessed(processed) = &done.effect else {
        panic!("정지 처리여야 한다");
    };
    assert_eq!(processed.attempt_state_before, AttemptState::RunUnknown);
    assert_eq!(processed.holds_released, 1);
    assert!(holds(&fixture.path).is_empty(), "정지 확인이 보류를 풀지 않았다");
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Failed);
    assert!(!reservation_exists(&fixture.path));
}

/// 늦은 도착(§2 · §7) — 같은 Job 에 더 높은 fence 의 시도가 있으면 옛 시도 · 옛 Lease · 옛 시도의 예약만 다루고 Job 은 건드리지 않는다.
#[test]
fn a_late_stop_for_an_older_attempt_does_not_touch_the_job() {
    let fixture = prepare_fixture();
    let old_fence = fence(&fixture.path);
    {
        let connection = rusqlite::Connection::open(&fixture.path).unwrap();
        connection
            .execute(
                "INSERT INTO coordinator_attempts VALUES ('attempt-new', ?1, 'CREATED', ?2, 'lease-new', ?3, ?4)",
                rusqlite::params![
                    JOB_ID,
                    (old_fence + 1).to_be_bytes().to_vec(),
                    500u64.to_be_bytes().to_vec(),
                    0u64.to_be_bytes().to_vec()
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO coordinator_attempt_nodes VALUES ('attempt-new', 'node-9', 0)",
                [],
            )
            .unwrap();
    }
    let before = job_state(&fixture.path);
    let accepted = accept_with(&fixture.path, &stop(&fixture.path, 1), Some(vec![1])).unwrap();
    let RunNoticeEffect::StopProcessed(done) = accepted.effect else {
        panic!("정지 처리여야 한다");
    };
    assert!(!done.latest_attempt);
    assert_eq!(done.job, None);
    assert_eq!(job_state(&fixture.path), before, "늦은 도착이 Job 을 옮겼다");
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Failed);
    assert_eq!(attempt_state(&fixture.path, "attempt-new"), AttemptState::Created);
    assert!(lease_revoked(&fixture.path));
    assert!(!reservation_exists(&fixture.path));
}

/// 이미 끝난 시도(진입 행이 없는 상태 — b10 ③)는 상태를 되돌리지 않고 Job 도 건드리지 않는다. 남은 Lease · 예약만 정리한다(b16 ②).
#[test]
fn a_stop_for_an_already_finished_attempt_only_cleans_up_resources() {
    let fixture = prepare_fixture();
    rusqlite::Connection::open(&fixture.path)
        .unwrap()
        .execute(
            "UPDATE coordinator_attempts SET state = 'COMPLETED' WHERE attempt_id = ?1",
            [ATTEMPT_ID],
        )
        .unwrap();
    let before = job_state(&fixture.path);
    let accepted = accept_with(&fixture.path, &stop(&fixture.path, 1), Some(vec![1])).unwrap();
    let RunNoticeEffect::StopProcessed(done) = accepted.effect else {
        panic!("정지 처리여야 한다");
    };
    assert!(!done.attempt_closed);
    assert_eq!(done.job, None);
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Completed);
    assert_eq!(job_state(&fixture.path), before);
    assert!(lease_revoked(&fixture.path));
    assert!(!reservation_exists(&fixture.path));
}

/// 거부는 아무것도 남기지 않는다 — 키 디렉터리 진술 없음 · 다른 fence · 다른 노드 · 정지 등급 아님 · 이어갈 지점 찾기 실패(처리 도중 실패해도 알림 행까지 되돌린다).
#[test]
fn every_refusal_leaves_nothing_behind() {
    let fixture = prepare_fixture();
    let good = stop(&fixture.path, 1);
    let mut finder = |_: &rusqlite::Connection, _: &gputeer_coordinator::job_store::StoredJob| {
        Ok(None)
    };
    let mut store = CoordinatorRunNoticeStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.accept(&good, RuntimeStopProof::NodeConfirmedStop, KeyDirectoryProvenance::Unverified, &mut finder, NOW),
        Err(RunNoticeError::KeyDirectoryNotVerified)
    );
    let wrong_fence = notice(fence(&fixture.path) + 1, pb::RunNoticeKind::StopConfirmed, 1, 350);
    assert_eq!(
        store.accept(&wrong_fence, RuntimeStopProof::NodeConfirmedStop, VERIFIED_DIR, &mut finder, NOW),
        Err(RunNoticeError::AttemptIdentityMismatch { attempt_id: ATTEMPT_ID.into() })
    );
    assert!(matches!(
        store.accept(&good, RuntimeStopProof::ObservedExitInSignedReport, VERIFIED_DIR, &mut finder, NOW),
        Err(RunNoticeError::Release(_))
    ));
    let mut broken = |_: &rusqlite::Connection, _: &gputeer_coordinator::job_store::StoredJob| {
        Err("공유 저장소를 읽지 못했다".to_string())
    };
    // fixture 의 Job 은 STAGING 이라 지점 찾기를 부른다
    assert_eq!(
        store.accept(&good, RuntimeStopProof::NodeConfirmedStop, VERIFIED_DIR, &mut broken, NOW),
        Err(RunNoticeError::ResumeLookup("공유 저장소를 읽지 못했다".into()))
    );
    drop(store);
    assert_eq!(notice_rows(&fixture.path), 0, "거부했는데 알림 행이 남았다");
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Created);
    assert_eq!(job_state(&fixture.path), JobState::Staging);
    assert!(reservation_exists(&fixture.path) && !lease_revoked(&fixture.path));
}

// ─── ★ 조각 5f — REPORT 세션의 ACK(FrameType 20) ───────────────────────────────

const COORD_SEED: [u8; 32] = [9; 32];

fn context<'a>(path: &'a Path, node: &'a str, key_directory: KeyDirectoryProvenance, nonce: &'a [u8]) -> RunNoticeAnswerContext<'a> {
    RunNoticeAnswerContext {
        control_db: path,
        coordinator_id: "coordinator-1",
        expected_node_id: node,
        key_directory,
        resume_policy: gputeer_coordinator::failover::FailoverPolicy {
            grace_ms: 0,
            shared_checkpoint_root: None,
            producer_keys: vec![],
        },
        session_nonce: nonce,
        now_unix_ms: NOW,
    }
}

/// 처리를 마친 뒤 서명된 ACK — 시도 · 노드 · 세대 · 종류 · 번호 · notice_hash(= sig_input 의 BLAKE3) · session_nonce echo · 처음이면 created.
///   같은 바이트 재전송은 created=false 의 같은 해시. 다른 노드 · 권위 없는 키 디렉터리는 거부하고 아무것도 처리하지 않는다.
#[test]
fn a_processed_notice_is_answered_with_a_signed_ack_that_echoes_its_hash_and_session() {
    let fixture = prepare_fixture();
    let verified = stop(&fixture.path, 1);
    let key = SigningKey::from_bytes(&COORD_SEED);
    let nonce = vec![7u8; 16];
    // 권위 없는 키 디렉터리 · 다른 노드 — 거부 · 아무것도 남지 않는다
    assert!(matches!(
        answer_run_notice(&context(&fixture.path, NODE_ID, KeyDirectoryProvenance::Unverified, &nonce), &key, &verified),
        Err(RunNoticeAnswerError::Rejected(_))
    ));
    assert!(matches!(
        answer_run_notice(&context(&fixture.path, "node-other", VERIFIED_DIR, &nonce), &key, &verified),
        Err(RunNoticeAnswerError::Rejected(_))
    ));
    assert_eq!(notice_rows(&fixture.path), 0);
    assert!(reservation_exists(&fixture.path));

    let (ack, accepted) = answer_run_notice(&context(&fixture.path, NODE_ID, VERIFIED_DIR, &nonce), &key, &verified).unwrap();
    assert!(ack.created && accepted.created);
    let expected_hash = gputeer_protocol::canonical::blake3_256(&gputeer_protocol::signing::signing_input(verified.get()));
    assert_eq!(ack.notice_hash.as_ref().map(|d| (d.algo, d.value.clone())), Some((1, expected_hash.to_vec())));
    assert_eq!(
        (ack.attempt_id.as_str(), ack.node_id.as_str(), ack.fence_epoch, ack.sequence, ack.kind),
        (ATTEMPT_ID, NODE_ID, fence(&fixture.path), 1, pb::RunNoticeKind::StopConfirmed as i32)
    );
    assert_eq!(ack.session_nonce, nonce);
    assert_eq!(ack.coordinator_id, "coordinator-1");
    // 서명은 Coordinator 키로 검증된다(ShortLived · replay nonce = session_nonce)
    let mut keys = InMemoryKeyring::new();
    keys.insert("coordinator-1", key.verifying_key());
    verify(&ack, 1, &Ed25519Verifier::new(keys), NOW, &mut NoReplayCheck).expect("ACK 서명이 검증돼야 한다");
    gputeer_protocol::attempt_run_notice_rules::validate_attempt_run_notice_ack(&ack).unwrap();
    assert!(!reservation_exists(&fixture.path), "처리를 마친 뒤에 답했다 — 예약이 풀렸다");

    let (again, _) = answer_run_notice(&context(&fixture.path, NODE_ID, VERIFIED_DIR, &[8u8; 16]), &key, &verified).unwrap();
    assert!(!again.created, "재전송인데 처음이라고 답했다");
    assert_eq!(again.notice_hash, ack.notice_hash);
    assert_eq!(again.session_nonce, vec![8u8; 16], "세션마다 그 세션의 nonce 를 echo 한다");
}

// ─── ★ 조각 6a — 불명의 효과(계약 §2 전이 표) ─────────────────────────────────

fn holds(path: &Path) -> Vec<(String, String)> {
    let connection = rusqlite::Connection::open(path).unwrap();
    gputeer_coordinator::job_holds::holds_for_job(&connection, JOB_ID)
        .unwrap()
        .into_iter()
        .map(|hold| (hold.attempt_id, hold.hold_kind))
        .collect()
}

fn events(path: &Path) -> Vec<String> {
    let connection = rusqlite::Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT event FROM coordinator_run_notice_events ORDER BY rowid")
        .unwrap();
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
    rows.map(|row| row.unwrap()).collect()
}

fn insert_newer_attempt(path: &Path) {
    let old_fence = fence(path);
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO coordinator_attempts VALUES ('attempt-new', ?1, 'CREATED', ?2, 'lease-new', ?3, ?4)",
            rusqlite::params![
                JOB_ID,
                (old_fence + 1).to_be_bytes().to_vec(),
                500u64.to_be_bytes().to_vec(),
                0u64.to_be_bytes().to_vec()
            ],
        )
        .unwrap();
    connection
        .execute("INSERT INTO coordinator_attempt_nodes VALUES ('attempt-new', 'node-9', 0)", [])
        .unwrap();
}

/// 늦은 도착 — 새 시도가 있으면 옛 시도의 불명은 Job · 보류를 건드리지 않고 DUPLICATE_RISK 만 남긴다(옛 시도가 CREATED 면 상태도 그대로).
#[test]
fn a_late_run_unknown_for_an_older_attempt_only_records_a_duplicate_risk() {
    let fixture = prepare_fixture();
    insert_newer_attempt(&fixture.path);
    let unknown = notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 1, 300);
    let accepted = accept_with(&fixture.path, &unknown, None).unwrap();
    let RunNoticeEffect::RunUnknownApplied(applied) = &accepted.effect else {
        panic!("불명 처리여야 한다");
    };
    assert!(!applied.latest_attempt && !applied.attempt_moved && !applied.hold_installed);
    assert_eq!(applied.events, vec!["DUPLICATE_RISK"]);
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Created);
    assert!(holds(&fixture.path).is_empty());
    assert_eq!(events(&fixture.path), vec!["DUPLICATE_RISK".to_string()]);
}

/// 이미 끝난 시도(COMPLETED)의 불명 — 시도는 되돌리지 않고, Job 이 최종이 아니면 보류를 건다 · RECONCILE_NEEDED. 같은 시도의 정지 확인이 보류를 푼다.
#[test]
fn a_run_unknown_for_a_finished_attempt_holds_a_live_job_and_asks_a_human() {
    let fixture = prepare_fixture();
    rusqlite::Connection::open(&fixture.path)
        .unwrap()
        .execute("UPDATE coordinator_attempts SET state = 'COMPLETED' WHERE attempt_id = ?1", [ATTEMPT_ID])
        .unwrap();
    let unknown = notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 1, 300);
    let accepted = accept_with(&fixture.path, &unknown, None).unwrap();
    let RunNoticeEffect::RunUnknownApplied(applied) = &accepted.effect else {
        panic!("불명 처리여야 한다");
    };
    assert!(!applied.attempt_moved && applied.hold_installed);
    assert_eq!(applied.events, vec!["RECONCILE_NEEDED"]);
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Completed);
    assert_eq!(holds(&fixture.path).len(), 1);
    let done = accept_with(&fixture.path, &stop(&fixture.path, 2), None).unwrap();
    let RunNoticeEffect::StopProcessed(processed) = &done.effect else {
        panic!("정지 처리여야 한다");
    };
    assert_eq!(processed.holds_released, 1);
    assert!(holds(&fixture.path).is_empty());
}

/// 정지 확인으로 끝난 시도 — 더 낮은 번호의 늦은 불명은 저장만(흡수) · 더 높은 번호의 불명은 보류를 다시 건다(b7 ①) · 그것은 더 높은 정지 확인으로만 풀린다.
#[test]
fn after_a_stop_a_lower_unknown_is_absorbed_and_a_higher_one_holds_again() {
    let fixture = prepare_fixture();
    accept_with(&fixture.path, &stop(&fixture.path, 5), None).unwrap();
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Failed);
    // 번호 2 의 불명은 STOP(5) 보다 낮다 — 순번 규칙은 STOP 에만 걸리므로 저장은 되고 효과는 없다
    let low = notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 2, 300);
    let accepted = accept_with(&fixture.path, &low, None).unwrap();
    let RunNoticeEffect::RunUnknownApplied(applied) = &accepted.effect else {
        panic!("불명 처리여야 한다");
    };
    assert!(!applied.hold_installed && applied.events.is_empty());
    assert!(holds(&fixture.path).is_empty(), "늦은 옛 불명이 보류를 걸었다");
    // Job 은 이미 큐(이어갈 지점 없음 → FAILED) — 최종이면 보류를 걸지 않는다. Job 을 다시 비최종으로 두고 본다
    rusqlite::Connection::open(&fixture.path)
        .unwrap()
        .execute(
            "UPDATE coordinator_jobs SET state = 'QUEUED', staging_at_unix_ms = NULL, run_terminal = NULL, worker_reported_finished_at_unix_ms = NULL WHERE job_id = ?1",
            [JOB_ID],
        )
        .unwrap();
    let high = notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 7, 300);
    let accepted = accept_with(&fixture.path, &high, None).unwrap();
    let RunNoticeEffect::RunUnknownApplied(applied) = &accepted.effect else {
        panic!("불명 처리여야 한다");
    };
    assert!(applied.hold_installed, "STOP 뒤의 더 높은 불명이 보류를 걸지 않았다");
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::Failed, "흡수 상태가 되돌려졌다");
    let done = accept_with(&fixture.path, &stop(&fixture.path, 8), None).unwrap();
    assert!(matches!(done.effect, RunNoticeEffect::StopProcessed(ref p) if p.holds_released == 1));
    assert!(holds(&fixture.path).is_empty());
}

/// 같은 시도의 UNREPORTED 보류(D6)는 불명 알림이 NOTICE 로 치환하고, 그 시도의 정지 확인 하나로 풀린다. 다른 시도의 행은 그대로다(집합).
#[test]
fn a_same_attempt_unreported_hold_is_replaced_and_other_attempts_holds_stay() {
    let fixture = prepare_fixture();
    CoordinatorRunNoticeStore::open(&fixture.path).unwrap();
    {
        let connection = rusqlite::Connection::open(&fixture.path).unwrap();
        for attempt in [ATTEMPT_ID, "attempt-other"] {
            connection
                .execute(
                    "INSERT INTO coordinator_job_holds VALUES (?1, ?2, 'UNREPORTED_SIDE_EFFECT_RISK', NULL, ?3)",
                    rusqlite::params![JOB_ID, attempt, 1u64.to_be_bytes().to_vec()],
                )
                .unwrap();
        }
    }
    accept_with(&fixture.path, &notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 1, 300), None).unwrap();
    assert_eq!(
        holds(&fixture.path),
        vec![
            (ATTEMPT_ID.to_string(), "NOTICE_RUN_UNKNOWN".to_string()),
            ("attempt-other".to_string(), "UNREPORTED_SIDE_EFFECT_RISK".to_string()),
        ]
    );
    accept_with(&fixture.path, &stop(&fixture.path, 2), None).unwrap();
    assert_eq!(
        holds(&fixture.path),
        vec![("attempt-other".to_string(), "UNREPORTED_SIDE_EFFECT_RISK".to_string())],
        "다른 시도의 보류까지 풀었다"
    );
}

// ─── ★ 조각 6b — 보류가 새 시도를 막는다(계약 §2 "새 시도를 만들지 않는다" · §9) ─────────

/// 두 번째 Job(job-2)을 큐에 올린다(node-2 등록 · 인벤토리 · 제출 · 계획 · 큐). 아직 시도는 없다.
fn queue_second_job(path: &Path) {
    let mut inventory = CoordinatorInventoryStore::open(path).unwrap();
    inventory
        .register_agent(&AgentRegistry {
            node_id: "node-2".into(),
            device_id: "device-2".into(),
            owner_member_id: "owner-2".into(),
            verifying_key: vec![2; 32],
            node_state: None,
            risk_state: None,
            security_tier: None,
            isolation_class: None,
            key_protection: None,
        })
        .unwrap();
    inventory
        .update_inventory(&AgentInventory {
            node_id: "node-2".into(),
            inventory_revision: 3,
            observed_at_unix_ms: 95,
            gpus: Some(vec![GpuInventory {
                gpu_id: "gpu-9".into(),
                model: Some("model-1".into()),
                healthy: Some(true),
                available_vram_bytes: Some(16),
            }]),
            available_cpu_cores: Some(8),
            available_ram_bytes: Some(64),
            available_workspace_bytes: Some(64),
            allowed_workload_classes: None,
            third_party_workloads_opt_in: None,
        })
        .unwrap();
    drop(inventory);
    let mut jobs = CoordinatorJobStore::open(path).unwrap();
    jobs.submit_accepted(
        &AcceptedJobSubmission {
            idempotency_key: [5; 16],
            job_id: "job-2".into(),
            submitter_device_id: "submitter-1".into(),
            manifest_hash: [5; 32],
            deadline_unix_ms: Some(20_000),
            max_queue_duration_ms: Some(5_000),
        },
        1_000,
    )
    .unwrap();
    jobs.start_planning("job-2", 1_010).unwrap();
    jobs.enqueue("job-2", "plan-2", 1_020).unwrap();
}

fn stage_second(path: &Path) -> Result<(), String> {
    CoordinatorStagingStore::open(path)
        .unwrap()
        .reserve_node_and_stage_queued_with_lease(
            &StageQueuedRequest {
                operation_key: [6; 16],
                job_id: "job-2".into(),
                attempt_id: "attempt-2".into(),
                lease_id: "lease-2".into(),
                node_id: "node-2".into(),
                selected_gpu_ids: vec!["gpu-9".into()],
                issuing_coordinator_id: "coordinator-1".into(),
                coordinator_term: 1,
                issued_at_unix_ms: 1_100,
                renew_after_unix_ms: 1_500,
                expires_at_unix_ms: 1_900,
                max_total_duration_seconds: 1,
            },
            3,
        )
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

fn hold_job(path: &Path, job_id: &str, attempt_id: &str) {
    CoordinatorRunNoticeStore::open(path).unwrap();
    rusqlite::Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO coordinator_job_holds VALUES (?1, ?2, 'NOTICE_RUN_UNKNOWN', NULL, ?3)",
            rusqlite::params![job_id, attempt_id, 1u64.to_be_bytes().to_vec()],
        )
        .unwrap();
}

/// 보류된 Job 은 배정 후보에서 빠지고, 시도를 만드는 마지막 관문(staging 트랜잭션)이 막는다(아무것도 남기지 않음). 보류가 풀리면 다시 된다.
#[test]
fn a_held_job_is_not_schedulable_and_the_last_gate_refuses_a_new_attempt() {
    let fixture = prepare_fixture();
    queue_second_job(&fixture.path);
    hold_job(&fixture.path, "job-2", "attempt-old");
    let schedulable: Vec<String> = CoordinatorJobStore::open(&fixture.path)
        .unwrap()
        .list_schedulable()
        .unwrap()
        .into_iter()
        .map(|job| job.job_id)
        .collect();
    assert!(!schedulable.contains(&"job-2".to_string()), "보류된 Job 을 후보로 골랐다: {schedulable:?}");
    let refused = stage_second(&fixture.path).unwrap_err();
    assert!(refused.contains("JobHeld"), "{refused}");
    assert!(
        CoordinatorStagingStore::open(&fixture.path).unwrap().get_attempt("attempt-2").unwrap().is_none(),
        "막혔는데 시도가 남았다"
    );
    rusqlite::Connection::open(&fixture.path)
        .unwrap()
        .execute("DELETE FROM coordinator_job_holds WHERE job_id = 'job-2'", [])
        .unwrap();
    assert!(CoordinatorJobStore::open(&fixture.path)
        .unwrap()
        .list_schedulable()
        .unwrap()
        .iter()
        .any(|job| job.job_id == "job-2"));
    stage_second(&fixture.path).expect("보류가 풀리면 시도를 만든다");
}

/// 장애 이어받기 — 시도가 RUN_UNKNOWN(보류)인 Job 은 Lease 가 끝나도 되돌리지 않고 사유를 남긴다. 예약 · Lease 도 그대로다.
#[test]
fn failover_never_requeues_a_job_whose_attempt_is_run_unknown() {
    let fixture = prepare_fixture();
    accept_with(&fixture.path, &notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 1, 300), None).unwrap();
    let mut notes = Vec::new();
    let outcomes = gputeer_coordinator::failover::failover_lost_attempts(
        &fixture.path,
        &gputeer_coordinator::failover::FailoverPolicy {
            grace_ms: 0,
            shared_checkpoint_root: None,
            producer_keys: vec![],
        },
        &gputeer_coordinator::supersede_notice_store::NoticeSigner {
            coordinator_id: "coordinator-1".into(),
            key: SigningKey::from_bytes(&[9; 32]),
        },
        1_000_000,
        &mut notes,
    )
    .unwrap();
    assert!(outcomes.is_empty(), "불명인 시도의 Job 을 되돌렸다: {outcomes:?}");
    assert!(notes.iter().any(|n| n.starts_with("FAILOVER_HELD")), "{notes:?}");
    assert_eq!(job_state(&fixture.path), JobState::Staging);
    assert!(reservation_exists(&fixture.path) && !lease_revoked(&fixture.path));
}

/// release-lost-node 는 보류된 Job 의 예약을 풀지 않는다(정지 확인 · release-held-job 이 푼다).
#[test]
fn release_lost_node_refuses_a_held_job() {
    let fixture = prepare_fixture();
    accept_with(&fixture.path, &notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 1, 300), None).unwrap();
    // 판정(시도 대체 · Job 이 넘어감)은 지나게 한다 — Job 을 큐로
    rusqlite::Connection::open(&fixture.path)
        .unwrap()
        .execute("UPDATE coordinator_jobs SET state = 'QUEUED', staging_at_unix_ms = NULL WHERE job_id = ?1", [JOB_ID])
        .unwrap();
    let refused = match gputeer_coordinator::failover::release_lost_node_by_operator(&fixture.path, NODE_ID, "운영자: 확인했다", NOW) {
        Ok(_) => panic!("보류된 Job 의 예약을 풀었다"),
        Err(refused) => refused,
    };
    assert!(refused.contains("실행 여부 불명 보류"), "{refused}");
    assert!(reservation_exists(&fixture.path));
}

// ─── ★ 조각 6c — 보류된 시도의 Lease 는 갱신 · Resume 되지 않는다(계약 §8 · 시험 7) ─────────

fn resume_identity(path: &Path) -> gputeer_coordinator::lease_store::ResumeRequestIdentity {
    gputeer_coordinator::lease_store::ResumeRequestIdentity {
        lease_id: LEASE_ID.into(),
        node_id: NODE_ID.into(),
        job_id: JOB_ID.into(),
        attempt_id: ATTEMPT_ID.into(),
        fence_epoch: fence(path),
    }
}

fn lease_expires_at(path: &Path) -> u64 {
    gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(path)
        .unwrap()
        .get(LEASE_ID)
        .unwrap()
        .unwrap()
        .expires_at_unix_ms
}

/// 순서 A(알림 먼저) — 갱신 · Resume 둘 다 RUN_UNKNOWN_HELD. 갱신은 만료시각을 늘리지 않는다. Lease 가 이미 만료됐어도 HELD 다(D8 — EXPIRED -> HELD_UNKNOWN).
#[test]
fn a_held_lease_is_neither_renewed_nor_resumed_even_after_it_expires() {
    use gputeer_coordinator::lease_store::{CoordinatorLeaseStore, RenewDecision, ResumeDecision};
    let fixture = prepare_fixture();
    accept_with(&fixture.path, &notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 1, 300), None).unwrap();
    let mut leases = CoordinatorLeaseStore::open(&fixture.path).unwrap();
    match leases.renew_existing_within_duration(LEASE_ID, 400, 5_000, 2_000).unwrap() {
        RenewDecision::RunUnknownHeld(stored) => assert_eq!(stored.attempt_id, ATTEMPT_ID),
        other => panic!("보류된 Lease 를 갱신 판정했다: {other:?}"),
    }
    assert_eq!(lease_expires_at(&fixture.path), 900, "보류된 Lease 의 만료시각이 늘었다");
    for now in [400, 900, 5_000] {
        match leases.classify_resume(&resume_identity(&fixture.path), now).unwrap() {
            ResumeDecision::RunUnknownHeld { stored } => assert_eq!(stored.lease_id, LEASE_ID),
            other => panic!("now={now} 보류된 Lease 를 Resume 판정했다: {other:?}"),
        }
    }
    assert!(leases.is_run_unknown_held(&leases.get(LEASE_ID).unwrap().unwrap()).unwrap());
    // 정지 확인이 보류를 풀면 HELD 가 아니다 — 시도는 FAILED · Lease 는 폐기됐으니 REVOKED 다
    accept_with(&fixture.path, &stop(&fixture.path, 2), None).unwrap();
    match leases.classify_resume(&resume_identity(&fixture.path), 400).unwrap() {
        ResumeDecision::Revoked { .. } => {}
        other => panic!("정지 확인 뒤에는 REVOKED 여야 한다: {other:?}"),
    }
}

/// 순서 B(failover 가 먼저 Lease 를 폐기) — 그 뒤 불명이 와도 갱신 · Resume 은 REVOKED 그대로다(HELD 로 바꾸지 않는다).
#[test]
fn a_lease_revoked_before_the_notice_stays_revoked() {
    use gputeer_coordinator::lease_store::{CoordinatorLeaseStore, LeaseStoreError, ResumeDecision};
    let fixture = prepare_fixture();
    CoordinatorLeaseStore::open(&fixture.path).unwrap().mark_revoked(LEASE_ID, 250).unwrap();
    accept_with(&fixture.path, &notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 1, 300), None).unwrap();
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::RunUnknown);
    let mut leases = CoordinatorLeaseStore::open(&fixture.path).unwrap();
    match leases.renew_existing_within_duration(LEASE_ID, 400, 5_000, 2_000) {
        Err(LeaseStoreError::Revoked { revoked_at_unix_ms: 250 }) => {}
        other => panic!("폐기된 Lease 는 REVOKED 여야 한다: {other:?}"),
    }
    match leases.classify_resume(&resume_identity(&fixture.path), 400).unwrap() {
        ResumeDecision::Revoked { .. } => {}
        other => panic!("폐기된 Lease 는 REVOKED 여야 한다: {other:?}"),
    }
}

/// 첫 진행 신호(record_process_started)는 RUN_UNKNOWN 시도를 바꾸지 않는다 — STARTING -> RUNNING 만 한다(계약 §8). Job 도 그대로다.
#[test]
fn the_first_progress_signal_does_not_move_a_run_unknown_attempt() {
    let fixture = prepare_fixture();
    accept_with(&fixture.path, &notice(fence(&fixture.path), pb::RunNoticeKind::RunUnknown, 1, 300), None).unwrap();
    CoordinatorStagingStore::open(&fixture.path)
        .unwrap()
        .record_process_started(LEASE_ID, 400)
        .unwrap();
    assert_eq!(attempt_state(&fixture.path, ATTEMPT_ID), AttemptState::RunUnknown);
    assert_eq!(job_state(&fixture.path), JobState::Staging);
}

/// 보류가 아닌 Lease 는 그대로 갱신된다 — 보류 검사가 정상 갱신을 막지 않는다(대조군).
#[test]
fn a_lease_without_a_held_attempt_is_still_renewed() {
    use gputeer_coordinator::lease_store::{CoordinatorLeaseStore, RenewDecision};
    let fixture = prepare_fixture();
    let mut leases = CoordinatorLeaseStore::open(&fixture.path).unwrap();
    match leases.renew_existing_within_duration(LEASE_ID, 400, 950, 700).unwrap() {
        RenewDecision::Renewed(stored) => assert_eq!(stored.expires_at_unix_ms, 950),
        other => panic!("보류 없는 Lease 를 갱신하지 않았다: {other:?}"),
    }
    assert!(!leases.is_run_unknown_held(&leases.get(LEASE_ID).unwrap().unwrap()).unwrap());
}
