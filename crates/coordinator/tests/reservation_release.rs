//! 예약 해제 — **오늘 정직한 호출은 아무것도 풀지 못한다.**
//!
//! ★ 처음엔 이 파일 머리에 "검증된 실행 종료 증거가 있을 때만 푼다" 고
//!   썼는데 **사실이 아니다**(2026-08-30 독립 검수 지적). terminal
//!   `AttemptReport` 는 노드 **자기보고**이지 프로세스가 멈췄다는 증명이
//!   아니다 — `DoD-51` evidence 가 직접 그렇게 적어 뒀다.
//!
//! `DoD-49` 가 "실행 종료 증명 없이 구현하면 중복 실행 위험이 생긴다" 며
//! 미뤄 둔 경로다. 여기서 고정하는 것은 네 가지다.
//!
//! ```text
//! 오늘은 못 푼다            실행 종료 증명 진술이 없으면 거부한다
//! 증거 없이는 못 푼다        durable terminal 보고서가 있어야 한다
//! 저장된 행만으로는 못 푼다   재검증한 Verified 와 바이트 단위로 같아야 한다
//! 남의 예약은 안 지운다       다른 attempt 의 예약이면 거부한다
//! ```

use std::path::{Path, PathBuf};

use gputeer_coordinator::attempt_report_store::{
    AttemptReportCorruption, AttemptReportStoreError, CoordinatorAttemptReportStore,
};
use gputeer_coordinator::inventory_store::{
    AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory,
};
use gputeer_coordinator::job_store::{AcceptedJobSubmission, CoordinatorJobStore};
use gputeer_coordinator::reservation_release::{
    ArtifactDurabilityGuard, CoordinatorReservationReleaseStore, KeyDirectoryProvenance,
    ReleaseAuthorization, ReleaseOutcome, ReservationReleaseError, RuntimeStopProof,
};
use gputeer_coordinator::staging_store::{CoordinatorStagingStore, StageQueuedRequest};
use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
use gputeer_protocol::signing::{verify, NoReplayCheck, Verified};
use gputeer_protocol::pb;

const JOB_ID: &str = "job-1";
const ATTEMPT_ID: &str = "attempt-1";
const LEASE_ID: &str = "lease-1";
const NODE_ID: &str = "node-1";
const RELEASED_AT: u64 = 4_000;

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

fn staged_fence_epoch(path: &Path) -> u64 {
    CoordinatorStagingStore::open(path)
        .unwrap()
        .get_attempt(ATTEMPT_ID)
        .unwrap()
        .expect("fixture 가 Attempt 를 만들었다")
        .fence_epoch
}

fn verified_report(
    job_id: &str,
    attempt_id: &str,
    node_id: &str,
    fence_epoch: u64,
    outcome: i32,
    key_seed: u8,
) -> Verified<pb::AttemptReport> {
    let key = SigningKey::from_bytes(&[key_seed; 32]);
    let mut report = pb::AttemptReport {
        schema_version: 1,
        job_id: job_id.into(),
        attempt_id: attempt_id.into(),
        node_id: node_id.into(),
        fence_epoch,
        outcome,
        final_step: 10,
        started_at_unix_ms: 210,
        finished_at_unix_ms: 300,
        issued_at_unix_ms: 301,
        ..Default::default()
    };
    report.node_signature = sign(&key, &report).to_vec();
    let mut keys = InMemoryKeyring::new();
    keys.insert(node_id, key.verifying_key());
    verify(
        &report,
        1,
        &Ed25519Verifier::new(keys),
        999,
        &mut NoReplayCheck,
    )
    .expect("테스트 보고서는 서명 검증을 통과해야 한다")
}

fn completed_report(path: &Path) -> Verified<pb::AttemptReport> {
    verified_report(
        JOB_ID,
        ATTEMPT_ID,
        NODE_ID,
        staged_fence_epoch(path),
        pb::AttemptOutcome::Completed as i32,
        7,
    )
}

/// terminal 증거를 durable 하게 저장한다 — 해제의 선행 조건이다.
fn store_evidence(path: &Path, report: &Verified<pb::AttemptReport>) {
    CoordinatorAttemptReportStore::open(path)
        .unwrap()
        .store_verified_terminal_report(report)
        .expect("증거 저장은 성공해야 한다");
}

/// 두 번째 Job/Attempt 를 다른 노드에 올린다.
///
/// "노드 주인이 바뀌었다" 를 **실재하는 attempt** 로 재현하기 위한 것이다.
fn stage_second_attempt_on_node_two(path: &Path) {
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
    drop(jobs);

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
        .unwrap();
}

/// 세 진술을 전부 "충족" 으로 채운 허가.
///
/// ★ **오늘 이렇게 부를 수 있는 정직한 호출부는 없다.** 실행 종료를
///   증명하는 producer 도, 권위 있는 키 디렉터리 재검증도, 전이 결합도
///   이 저장소에 아직 없다. 테스트가 나머지 방어를 재려면 이 관문을
///   넘어야 하므로 여기서만 쓴다.
fn fully_authorized() -> ReleaseAuthorization {
    ReleaseAuthorization {
        runtime_stop: RuntimeStopProof::ProvenByCaller,
        key_directory: KeyDirectoryProvenance::AuthoritativeDirectoryVerifiedByCaller,
        artifact_durability: ArtifactDurabilityGuard::SatisfiedByCaller,
    }
}

/// 오늘의 정직한 호출 — 셋 다 "아직 증명 못 함".
fn todays_honest_authorization() -> ReleaseAuthorization {
    ReleaseAuthorization {
        runtime_stop: RuntimeStopProof::NotProvenYet,
        key_directory: KeyDirectoryProvenance::Unverified,
        artifact_durability: ArtifactDurabilityGuard::NotSatisfiedYet,
    }
}

fn reservation_exists(path: &Path) -> bool {
    CoordinatorStagingStore::open(path)
        .unwrap()
        .get_node_reservation(NODE_ID)
        .unwrap()
        .is_some()
}

// ---------------------------------------------------------------------------
// 정상 경로
// ---------------------------------------------------------------------------

/// 증거가 있으면 풀리고, **GPU 목록이 기록에 남는다.**
#[test]
fn a_verified_terminal_report_releases_the_reservation() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);
    assert!(reservation_exists(&fixture.path), "풀기 전에는 예약이 있다");

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    let outcome = store
        .release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT)
        .unwrap();

    let ReleaseOutcome::Released(record) = outcome else {
        panic!("첫 호출은 실제로 풀어야 한다: {outcome:?}");
    };
    assert_eq!(record.attempt_id, ATTEMPT_ID);
    assert_eq!(record.node_id, NODE_ID);
    assert_eq!(record.job_id, JOB_ID);
    assert_eq!(record.released_at_unix_ms, RELEASED_AT);
    assert_eq!(
        record.released_gpu_ids,
        vec!["gpu-1".to_string(), "gpu-2".to_string()],
        "예약이 잡고 있던 GPU 가 그대로 반환돼야 한다"
    );

    // ★ 반환값만 보면 **자식 행을 안 써도 통과한다** — 내 뮤테이션이
    //   그걸 잡았다(R8). 저장된 것을 다시 읽어 확인한다.
    let persisted = store
        .get_release(ATTEMPT_ID)
        .unwrap()
        .expect("해제 기록이 저장돼 있어야 한다");
    assert_eq!(
        persisted, record,
        "저장된 기록이 반환값과 같아야 한다 — GPU 목록 포함"
    );

    assert!(
        !reservation_exists(&fixture.path),
        "예약이 실제로 사라져야 한다"
    );
}

/// 같은 보고서로 다시 부르면 **멱등**이다.
#[test]
fn releasing_twice_with_the_same_evidence_is_idempotent() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    let first = store
        .release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT)
        .unwrap();
    let second = store
        .release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT + 5_000)
        .unwrap();

    let (ReleaseOutcome::Released(first), ReleaseOutcome::AlreadyReleased(second)) =
        (first, second)
    else {
        panic!("두 번째는 AlreadyReleased 여야 한다");
    };
    assert_eq!(
        first, second,
        "두 번째 호출의 늦은 시각이 최초 기록을 덮어쓰면 안 된다"
    );
}

/// 해제 기록은 재시작을 넘어 남는다.
#[test]
fn the_release_record_survives_reopening_the_store() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    let record = {
        let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
        match store
            .release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT)
            .unwrap()
        {
            ReleaseOutcome::Released(record) => record,
            other => panic!("{other:?}"),
        }
    };

    let reopened = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(reopened.get_release(ATTEMPT_ID).unwrap(), Some(record));
}

/// 파일 경로는 durable 이고 `:memory:` 는 아니다.
#[test]
fn an_in_memory_store_is_not_durable() {
    let fixture = prepare_fixture();
    assert!(CoordinatorReservationReleaseStore::open(&fixture.path)
        .unwrap()
        .is_durable());
    assert!(!CoordinatorReservationReleaseStore::open(":memory:")
        .unwrap()
        .is_durable());
}

// ---------------------------------------------------------------------------
// ★ 증거 없이는 풀지 못한다
// ---------------------------------------------------------------------------

/// **durable terminal 증거가 없으면 풀지 않는다.**
///
/// 이게 이 조각의 존재 이유다 — 증거 없이 풀면 아직 돌고 있는 노드의
/// GPU 를 남에게 내주게 된다.
#[test]
fn without_stored_terminal_evidence_the_reservation_is_not_released() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    // 일부러 store_evidence 를 부르지 않는다.

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT),
        Err(ReservationReleaseError::NoTerminalEvidence {
            attempt_id: ATTEMPT_ID.into(),
            node_id: NODE_ID.into(),
        }),
    );
    assert!(
        reservation_exists(&fixture.path),
        "거부했으면 예약은 그대로 있어야 한다"
    );
}

/// **저장된 행과 재검증된 보고서가 다르면 풀지 않는다.**
///
/// ★ `attempt_report_store` 가 "raw 는 terminal decision 에 쓰지 말라" 고
///   적어 둔 계약을 이 관문이 지킨다. 저장된 행 하나만으로는 못 푼다.
#[test]
fn a_report_that_does_not_match_the_stored_evidence_cannot_release() {
    let fixture = prepare_fixture();
    let stored = completed_report(&fixture.path);
    store_evidence(&fixture.path, &stored);

    // 같은 job/attempt/node/fence 지만 **다른 서명자**의 보고서.
    let impostor = verified_report(
        JOB_ID,
        ATTEMPT_ID,
        NODE_ID,
        staged_fence_epoch(&fixture.path),
        pb::AttemptOutcome::Completed as i32,
        9,
    );

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.release_for_verified_terminal_report(&impostor, fully_authorized(), RELEASED_AT),
        Err(ReservationReleaseError::EvidenceMismatch {
            attempt_id: ATTEMPT_ID.into(),
            node_id: NODE_ID.into(),
        }),
    );
    assert!(reservation_exists(&fixture.path));
}

/// terminal 이 아닌 outcome 으로는 풀지 못한다 — 아직 안 끝났다는 뜻이다.
#[test]
fn a_non_terminal_outcome_cannot_release() {
    let fixture = prepare_fixture();
    let running = verified_report(
        JOB_ID,
        ATTEMPT_ID,
        NODE_ID,
        staged_fence_epoch(&fixture.path),
        pb::AttemptOutcome::Unspecified as i32,
        7,
    );

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.release_for_verified_terminal_report(&running, fully_authorized(), RELEASED_AT),
        Err(ReservationReleaseError::NotTerminalOutcome(
            pb::AttemptOutcome::Unspecified as i32
        )),
    );
    assert!(reservation_exists(&fixture.path));
}

// ---------------------------------------------------------------------------
// ★ 남의 예약을 지우지 않는다
// ---------------------------------------------------------------------------

/// **예약이 다른 attempt 의 것이면 지우지 않는다.**
///
/// 옛 보고서로 새 예약을 밀어내면 지금 돌고 있는 남의 작업을 죽인다
/// (`CLAUDE.md` §0.1). `runtime-linux` 의 cgroup 회수에서 이미 한 번
/// 밟았던 함정이다.
#[test]
fn a_reservation_held_by_another_attempt_is_never_deleted() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    // 증거를 저장한 **뒤에** 노드 주인이 바뀐 상황을 만든다 — 옛 보고서가
    // 남았고 노드는 이미 다른 attempt 가 잡고 있다.
    //
    // 진짜 두 번째 Attempt 를 만든 뒤 예약을 그쪽으로 옮긴다. 존재하지
    // 않는 attempt 를 적으면 외래키가 먼저 막아 실제 상황이 재현되지 않는다.
    stage_second_attempt_on_node_two(&fixture.path);
    let connection = rusqlite::Connection::open(&fixture.path).unwrap();
    connection
        .execute("DELETE FROM coordinator_node_reservation_gpus WHERE node_id = 'node-2'", [])
        .unwrap();
    connection
        .execute("DELETE FROM coordinator_node_reservations WHERE node_id = 'node-2'", [])
        .unwrap();
    connection
        .execute(
            "UPDATE coordinator_node_reservations SET attempt_id = 'attempt-2'
             WHERE node_id = ?1",
            rusqlite::params![NODE_ID],
        )
        .unwrap();
    drop(connection);

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT),
        Err(ReservationReleaseError::ReservationBelongsToAnotherAttempt {
            node_id: NODE_ID.into(),
            holder_attempt_id: "attempt-2".into(),
        }),
    );
    assert!(
        reservation_exists(&fixture.path),
        "남의 예약은 그대로 있어야 한다"
    );
}

/// **옛 fence 의 보고서는 증거 대조에서 먼저 걸린다.**
///
/// fence 가 다르면 보고서 바이트가 다르고, 그러면 저장된 증거와 해시가
/// 어긋난다. 여기서는 그 사실을 정확한 오류로 고정한다 — `matches!` 로
/// 둘 중 아무거나 받으면 어느 관문이 막았는지 모른다.
#[test]
fn a_report_from_an_older_fence_is_caught_by_the_evidence_check() {
    let fixture = prepare_fixture();
    let current = staged_fence_epoch(&fixture.path);
    let stale = verified_report(
        JOB_ID,
        ATTEMPT_ID,
        NODE_ID,
        current.saturating_sub(1),
        pb::AttemptOutcome::Completed as i32,
        7,
    );
    store_evidence(&fixture.path, &completed_report(&fixture.path));

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.release_for_verified_terminal_report(&stale, fully_authorized(), RELEASED_AT),
        Err(ReservationReleaseError::EvidenceMismatch {
            attempt_id: ATTEMPT_ID.into(),
            node_id: NODE_ID.into(),
        }),
    );
    assert!(reservation_exists(&fixture.path));
}

/// **Attempt 의 fence 가 증거 저장 뒤에 움직이면 풀지 않는다.**
///
/// ★ 막는 주체는 이 모듈이 **아니라** `fetch_report_binding` 이다 — 그
///   함수가 durable Attempt 의 fence 를 재대조한다. 내가 여기 따로 둔
///   검사는 도달 불가능한 죽은 코드였고, 내 뮤테이션이 그걸 잡아(R5)
///   지웠다. 이 테스트는 **누가 막든 막힌다**는 사실을 고정한다.
#[test]
fn a_moved_attempt_fence_blocks_the_release() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    let moved = staged_fence_epoch(&fixture.path) + 1;
    let connection = rusqlite::Connection::open(&fixture.path).unwrap();
    connection
        .execute(
            "UPDATE coordinator_attempts SET fence_epoch = ?1 WHERE attempt_id = ?2",
            rusqlite::params![moved.to_be_bytes().to_vec(), ATTEMPT_ID],
        )
        .unwrap();
    drop(connection);

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    let outcome = store.release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT);
    assert!(
        matches!(
            outcome,
            Err(ReservationReleaseError::Evidence(
                AttemptReportStoreError::Corrupt {
                    kind: AttemptReportCorruption::FenceEpochMismatch,
                    ..
                }
            ))
        ),
        "Attempt fence 가 움직였으면 증거 대조가 막아야 한다: {outcome:?}"
    );
    let _ = moved;
    assert!(
        reservation_exists(&fixture.path),
        "거부했으면 예약은 그대로 있어야 한다"
    );
}

/// 예약이 아예 없으면 **조용히 성공하지 않는다.**
///
/// 해제 기록도 없는데 예약이 사라진 것은 누군가 밖에서 지웠다는 뜻이다 —
/// 조용히 넘기면 그 사실이 묻힌다(`CLAUDE.md` §3).
#[test]
fn a_missing_reservation_without_a_release_record_fails_closed() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    let connection = rusqlite::Connection::open(&fixture.path).unwrap();
    connection
        .execute("DELETE FROM coordinator_node_reservation_gpus", [])
        .unwrap();
    connection
        .execute("DELETE FROM coordinator_node_reservations", [])
        .unwrap();
    drop(connection);

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT),
        Err(ReservationReleaseError::ReservationNotFound {
            node_id: NODE_ID.into(),
        }),
    );
}

// ---------------------------------------------------------------------------
// 원자성
// ---------------------------------------------------------------------------

/// 거부된 해제는 **아무것도 남기지 않는다.**
///
/// 예약도 그대로고, 해제 기록도 GPU 자식 행도 생기지 않아야 한다.
#[test]
fn a_refused_release_leaves_no_trace() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    // 증거를 저장하지 않아 거부된다.

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert!(store
        .release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT)
        .is_err());

    assert_eq!(store.get_release(ATTEMPT_ID).unwrap(), None);
    assert!(reservation_exists(&fixture.path));

    let connection = rusqlite::Connection::open(&fixture.path).unwrap();
    let release_gpus: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM coordinator_reservation_release_gpus",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(release_gpus, 0, "해제 GPU 기록이 남으면 안 된다");
    let reserved_gpus: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM coordinator_node_reservation_gpus",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reserved_gpus, 2, "예약 GPU 는 그대로여야 한다");
}

/// 푼 뒤에는 예약 GPU 자식 행도 같이 사라진다.
#[test]
fn releasing_removes_the_reserved_gpu_rows_too() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    CoordinatorReservationReleaseStore::open(&fixture.path)
        .unwrap()
        .release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT)
        .unwrap();

    let connection = rusqlite::Connection::open(&fixture.path).unwrap();
    let reserved_gpus: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM coordinator_node_reservation_gpus",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reserved_gpus, 0, "고아 자식 행이 남으면 안 된다");
}

/// **푼 뒤에는 다른 Job 이 그 노드를 잡을 수 있다.**
///
/// 이게 해제의 목적이다 — 기록만 남기고 노드가 계속 묶여 있으면 아무
/// 소용이 없다.
#[test]
fn after_release_another_job_can_reserve_the_same_node() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    // 풀기 전에는 다른 Job 이 같은 노드를 못 잡는다.
    let mut jobs = CoordinatorJobStore::open(&fixture.path).unwrap();
    jobs.submit_accepted(
        &AcceptedJobSubmission {
            idempotency_key: [3; 16],
            job_id: "job-2".into(),
            submitter_device_id: "submitter-1".into(),
            manifest_hash: [3; 32],
            deadline_unix_ms: Some(20_000),
            max_queue_duration_ms: Some(5_000),
        },
        1_000,
    )
    .unwrap();
    jobs.start_planning("job-2", 1_010).unwrap();
    jobs.enqueue("job-2", "plan-2", 1_020).unwrap();
    drop(jobs);

    let second = StageQueuedRequest {
        operation_key: [4; 16],
        job_id: "job-2".into(),
        attempt_id: "attempt-2".into(),
        lease_id: "lease-2".into(),
        node_id: NODE_ID.into(),
        selected_gpu_ids: vec!["gpu-1".into(), "gpu-2".into()],
        issuing_coordinator_id: "coordinator-1".into(),
        coordinator_term: 1,
        issued_at_unix_ms: 1_100,
        renew_after_unix_ms: 1_500,
        expires_at_unix_ms: 1_900,
        max_total_duration_seconds: 1,
    };
    assert!(
        CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .reserve_node_and_stage_queued_with_lease(&second, 7)
            .is_err(),
        "풀기 전에는 같은 노드를 잡을 수 없어야 한다"
    );

    CoordinatorReservationReleaseStore::open(&fixture.path)
        .unwrap()
        .release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT)
        .unwrap();

    CoordinatorStagingStore::open(&fixture.path)
        .unwrap()
        .reserve_node_and_stage_queued_with_lease(&second, 7)
        .expect("푼 뒤에는 다른 Job 이 같은 노드를 잡을 수 있어야 한다");
}

// ---------------------------------------------------------------------------
// ★ 오늘 정직한 호출은 아무 예약도 풀지 못한다
// ---------------------------------------------------------------------------

/// **증거가 완벽해도 거부된다.**
///
/// terminal 보고서는 노드 **자기보고**이지 프로세스가 멈췄다는 증명이
/// 아니다 — `DoD-51` evidence 가 직접 그렇게 적어 뒀다. 초안은 그걸
/// 전제로 예약을 풀었고, 독립 검수가 정면으로 반박했다.
///
/// ★ 이 테스트가 실패하면 버그가 아니라 **신호**다 — 누군가
///   `RuntimeStopProof::ProvenByCaller` 를 정직하게 넘길 수 있게 됐다는
///   뜻이고, 그때는 계획서의 나머지 조건과 이 파일을 같이 고친다.
#[test]
fn todays_honest_caller_cannot_release_anything() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.release_for_verified_terminal_report(
            &report,
            todays_honest_authorization(),
            RELEASED_AT
        ),
        Err(ReservationReleaseError::RuntimeStopNotProven),
    );
    assert!(
        reservation_exists(&fixture.path),
        "예약은 그대로 있어야 한다"
    );
}

/// 세 진술을 **하나씩** 열어도 나머지가 막는다.
#[test]
fn each_missing_declaration_blocks_on_its_own() {
    let cases = [
        (
            ReleaseAuthorization {
                runtime_stop: RuntimeStopProof::NotProvenYet,
                ..fully_authorized()
            },
            ReservationReleaseError::RuntimeStopNotProven,
        ),
        (
            ReleaseAuthorization {
                key_directory: KeyDirectoryProvenance::Unverified,
                ..fully_authorized()
            },
            ReservationReleaseError::KeyDirectoryNotVerified,
        ),
        (
            ReleaseAuthorization {
                artifact_durability: ArtifactDurabilityGuard::NotSatisfiedYet,
                ..fully_authorized()
            },
            ReservationReleaseError::ArtifactDurabilityNotSatisfied,
        ),
        (
            ReleaseAuthorization {
                artifact_durability: ArtifactDurabilityGuard::NotApplicableNonCompleted,
                ..fully_authorized()
            },
            ReservationReleaseError::ArtifactGuardMarkedNotApplicableForCompleted,
        ),
    ];

    for (authorization, expected) in cases {
        let fixture = prepare_fixture();
        let report = completed_report(&fixture.path);
        store_evidence(&fixture.path, &report);

        let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.release_for_verified_terminal_report(&report, authorization, RELEASED_AT),
            Err(expected),
            "{authorization:?} 는 거부돼야 한다"
        );
        assert!(reservation_exists(&fixture.path));
    }
}

/// 관문은 **증거보다 먼저** 본다.
///
/// 증거가 아예 없어도 허가 오류가 먼저 나와야 한다 — 그래야 운영자가
/// "증거를 만들면 풀리겠구나" 로 오해하지 않는다.
#[test]
fn the_authorization_gate_is_checked_before_the_evidence() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    // 증거를 일부러 저장하지 않는다.

    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    assert_eq!(
        store.release_for_verified_terminal_report(
            &report,
            todays_honest_authorization(),
            RELEASED_AT
        ),
        Err(ReservationReleaseError::RuntimeStopNotProven),
        "증거 부재보다 허가 부재가 먼저 보고돼야 한다"
    );
}

// ---------------------------------------------------------------------------
// 원자성 · 경쟁
// ---------------------------------------------------------------------------

/// **삭제 도중 실패하면 아무것도 안 남는다.**
///
/// 위험 구간은 예약 GPU 삭제 → 예약 삭제 → 해제 기록 삽입 사이다.
/// 해제 기록의 자식 행을 미리 넣어 두면 부모가 없어 외래키가 걸리고,
/// 그 지점은 정확히 그 구간 안이다.
#[test]
fn a_failure_between_the_deletes_and_the_insert_rolls_everything_back() {
    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    // store 를 먼저 열어 해제 테이블을 만든 뒤, 자식 행을 미리 심는다.
    // 부모가 없으므로 외래키를 끈 별도 연결로 넣는다.
    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    {
        let connection = rusqlite::Connection::open(&fixture.path).unwrap();
        connection.execute("PRAGMA foreign_keys = OFF", []).unwrap();
        connection
            .execute(
                "INSERT INTO coordinator_reservation_release_gpus(attempt_id, gpu_id, ordinal)
                 VALUES (?1, 'gpu-1', 0)",
                rusqlite::params![ATTEMPT_ID],
            )
            .unwrap();
    }

    let outcome = store.release_for_verified_terminal_report(
        &report,
        fully_authorized(),
        RELEASED_AT,
    );
    assert!(outcome.is_err(), "삽입 충돌로 실패해야 한다: {outcome:?}");

    // ★ 예약이 살아 있어야 한다 — 부분 커밋이면 예약만 사라지고 해제
    //   기록은 없는 최악의 상태가 된다.
    assert!(
        reservation_exists(&fixture.path),
        "rollback 되어 예약이 남아 있어야 한다"
    );
    assert_eq!(
        store.get_release(ATTEMPT_ID).unwrap(),
        None,
        "해제 기록이 생기면 안 된다"
    );

    let connection = rusqlite::Connection::open(&fixture.path).unwrap();
    let reserved_gpus: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM coordinator_node_reservation_gpus",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reserved_gpus, 2, "예약 GPU 자식 행도 살아 있어야 한다");
}

/// **두 연결이 동시에 풀어도 정확히 한 번만 풀린다.**
///
/// `durable_replay_race.rs` 가 쓰는 패턴 — 연결마다 별도 store, `Barrier`
/// 로 동시에 출발한다.
#[test]
fn two_concurrent_releases_produce_exactly_one_release() {
    use std::sync::{Arc, Barrier};

    let fixture = prepare_fixture();
    let report = completed_report(&fixture.path);
    store_evidence(&fixture.path, &report);

    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let path = fixture.path.clone();
        let barrier = Arc::clone(&barrier);
        let report = report.clone();
        handles.push(std::thread::spawn(move || {
            let mut store = CoordinatorReservationReleaseStore::open(&path).unwrap();
            barrier.wait();
            store.release_for_verified_terminal_report(&report, fully_authorized(), RELEASED_AT)
        }));
    }

    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let released = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok(ReleaseOutcome::Released(_))))
        .count();
    let already = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok(ReleaseOutcome::AlreadyReleased(_))))
        .count();

    assert_eq!(released, 1, "정확히 하나만 실제로 풀어야 한다: {outcomes:?}");
    assert_eq!(
        already, 1,
        "나머지 하나는 멱등 경로여야 한다: {outcomes:?}"
    );
    assert!(!reservation_exists(&fixture.path));
}

/// **완료가 아닌 terminal 은 artifact guard 를 요구하지 않는다.**
///
/// 계획서는 "**완료 Job** 은 최종 artifact durability guard 를 따로
/// 만족한다" 고 했다. 실패·취소에까지 요구하면 없는 조건으로 정직한
/// 호출을 막는 것이다.
#[test]
fn a_non_completed_outcome_does_not_need_the_artifact_guard() {
    let fixture = prepare_fixture();
    let failed = verified_report(
        JOB_ID,
        ATTEMPT_ID,
        NODE_ID,
        staged_fence_epoch(&fixture.path),
        pb::AttemptOutcome::Failed as i32,
        7,
    );
    store_evidence(&fixture.path, &failed);

    let authorization = ReleaseAuthorization {
        artifact_durability: ArtifactDurabilityGuard::NotApplicableNonCompleted,
        ..fully_authorized()
    };
    let mut store = CoordinatorReservationReleaseStore::open(&fixture.path).unwrap();
    let outcome = store
        .release_for_verified_terminal_report(&failed, authorization, RELEASED_AT)
        .expect("실패 보고서에는 artifact guard 가 필요 없다");
    assert!(matches!(outcome, ReleaseOutcome::Released(_)), "{outcome:?}");
}
