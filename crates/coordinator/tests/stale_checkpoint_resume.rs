//! ★ 실행 알림 계약 D6 보강(계획 §5 4번) — 더 높은 fence 의 시도가 생긴 뒤에는 옛 시도의 체크포인트를 재개 후보로 올리지 않는다.
//!
//! 이어받은 뒤에도 옛 노드는 공유 저장소에 계속 쓸 수 있다(저장소에 fence 관문이 없다). 그 파일은 옛 시도의 서명 · fence 와 맞으므로
//! 전에는 새 시도가 체크포인트를 내기 전에 다음 이어받기가 일어나면 **더 높은 step 이라는 이유로** 뽑혔다 — 새 시도가 시작한 지점과
//! 갈라진 이력이다. 실제 저장소 · 실제 서명 · 실제 이어받기(`failover_lost_attempts`) 두 번으로 확인한다.

use std::path::{Path, PathBuf};

use gputeer_checkpoint::commit::ManifestMeta;
use gputeer_checkpoint::shared;
use gputeer_coordinator::failover::{failover_lost_attempts, FailoverOutcome, FailoverPolicy};
use gputeer_coordinator::inventory_store::{AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory};
use gputeer_coordinator::job_store::{AcceptedJobSubmission, CoordinatorJobStore};
use gputeer_coordinator::staging_store::{CoordinatorStagingStore, StageQueuedRequest};
use gputeer_coordinator::supersede_notice_store::NoticeSigner;
use gputeer_crypto::{sign, SigningKey};
use prost::Message;

const JOB_ID: &str = "job-1";

struct Fixture {
    _dir: tempfile::TempDir,
    db: PathBuf,
    shared: PathBuf,
    work: PathBuf,
}

fn key(node: &str) -> SigningKey {
    SigningKey::from_bytes(&[if node == "node-1" { 1 } else { 2 }; 32])
}

fn register(path: &Path, node: &str, gpu: &str, revision: u64) {
    let mut inventory = CoordinatorInventoryStore::open(path).unwrap();
    inventory
        .register_agent(&AgentRegistry {
            node_id: node.into(),
            device_id: format!("device-{node}"),
            owner_member_id: format!("owner-{node}"),
            verifying_key: key(node).verifying_key().to_bytes().to_vec(),
            node_state: None,
            risk_state: None,
            security_tier: None,
            isolation_class: None,
            key_protection: None,
        })
        .unwrap();
    inventory
        .update_inventory(&AgentInventory {
            node_id: node.into(),
            inventory_revision: revision,
            observed_at_unix_ms: 90,
            gpus: Some(vec![GpuInventory {
                gpu_id: gpu.into(),
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
}

/// 시도를 node 에 배정하고 실행 중으로 만든다. `at` 는 발급 시각.
#[allow(clippy::too_many_arguments)]
fn stage_and_run(path: &Path, attempt: &str, lease: &str, node: &str, gpu: &str, revision: u64, at: u64, op: u8) {
    let mut staging = CoordinatorStagingStore::open(path).unwrap();
    staging
        .reserve_node_and_stage_queued_with_lease(
            &StageQueuedRequest {
                operation_key: [op; 16],
                job_id: JOB_ID.into(),
                attempt_id: attempt.into(),
                lease_id: lease.into(),
                node_id: node.into(),
                selected_gpu_ids: vec![gpu.into()],
                issuing_coordinator_id: "coordinator-1".into(),
                coordinator_term: 1,
                issued_at_unix_ms: at,
                renew_after_unix_ms: at + 300,
                expires_at_unix_ms: at + 700,
                max_total_duration_seconds: 1,
            },
            revision,
        )
        .unwrap_or_else(|e| panic!("{attempt} 배정 실패: {e:?}"));
    staging.record_grant_accepted(attempt, at + 50, false, false).unwrap();
    staging.record_process_started(lease, at + 60).unwrap();
}

fn prepare() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("control.sqlite3");
    let shared = dir.path().join("shared");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    register(&db, "node-1", "gpu-1", 7);
    register(&db, "node-2", "gpu-9", 3);
    let mut jobs = CoordinatorJobStore::open(&db).unwrap();
    jobs.submit_accepted(
        &AcceptedJobSubmission {
            idempotency_key: [1; 16],
            job_id: JOB_ID.into(),
            submitter_device_id: "submitter-1".into(),
            manifest_hash: [1; 32],
            deadline_unix_ms: Some(10_000_000),
            max_queue_duration_ms: Some(5_000_000),
        },
        100,
    )
    .unwrap();
    jobs.start_planning(JOB_ID, 110).unwrap();
    jobs.enqueue(JOB_ID, "plan-1", 120).unwrap();
    drop(jobs);
    stage_and_run(&db, "attempt-1", "lease-1", "node-1", "gpu-1", 7, 200, 2);
    // 정확히 PURE 로 선언된 작업이라 알림 없는 Lease 만료에도 자동으로 이어간다(D6)
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute(
            "INSERT INTO coordinator_job_side_effects(job_id, side_effect_class, source) VALUES (?1, 'PURE', 'SUBMISSION')",
            rusqlite::params![JOB_ID],
        )
        .unwrap();
    Fixture { _dir: dir, db, shared, work }
}

fn fence_of(path: &Path, attempt: &str) -> u64 {
    let bytes: Vec<u8> = rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT fence_epoch FROM coordinator_attempts WHERE attempt_id = ?1", [attempt], |row| row.get(0))
        .unwrap();
    u64::from_be_bytes(bytes.try_into().unwrap())
}

/// 그 시도 · 노드로 서명된 체크포인트를 공유 저장소에 게시한다(Agent 의 게시와 같은 순서 — 데이터 확정 → 서명 매니페스트).
fn publish(fixture: &Fixture, checkpoint_id: &str, attempt: &str, node: &str, step: u64) {
    let source = fixture.work.join(checkpoint_id);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("state.bin"), format!("{attempt} step {step}")).unwrap();
    let committed = shared::publish_or_recover(
        &fixture.shared,
        JOB_ID,
        checkpoint_id,
        &source,
        &ManifestMeta {
            job_id: JOB_ID.into(),
            attempt_id: attempt.into(),
            step,
            fence_epoch: fence_of(&fixture.db, attempt),
            producer_node_id: node.into(),
            created_at_unix_ms: 300,
        },
    )
    .unwrap();
    let mut manifest = shared::to_unsigned_pb(&committed).unwrap();
    manifest.producer_signature = sign(&key(node), &manifest).to_vec();
    shared::write_signed_manifest(&fixture.shared, JOB_ID, checkpoint_id, &manifest.encode_to_vec()).unwrap();
}

fn failover(fixture: &Fixture, now: u64) -> (Vec<FailoverOutcome>, Vec<String>) {
    let mut notes = Vec::new();
    let outcomes = failover_lost_attempts(
        &fixture.db,
        &FailoverPolicy {
            grace_ms: 0,
            shared_checkpoint_root: Some(fixture.shared.clone()),
            producer_keys: vec![
                ("node-1".into(), key("node-1").verifying_key()),
                ("node-2".into(), key("node-2").verifying_key()),
            ],
        },
        &NoticeSigner { coordinator_id: "coordinator-1".into(), key: SigningKey::from_bytes(&[9; 32]) },
        now,
        &mut notes,
    )
    .unwrap();
    (outcomes, notes)
}

fn resumed_from(outcomes: &[FailoverOutcome], notes: &[String]) -> String {
    match outcomes {
        [FailoverOutcome::Requeued { resume_checkpoint_id: Some(id), .. }] => id.clone(),
        other => panic!("이어갈 지점과 함께 다시 대기열에 올라야 한다: {other:?}\n{notes:#?}"),
    }
}

/// 옛 노드가 이어받기 뒤에 쓴 더 높은 step 은 다음 이어받기에서 뽑히지 않는다 — 새 시도가 시작한 기준 지점에서 다시 이어간다.
#[test]
fn a_checkpoint_the_old_node_wrote_after_takeover_is_never_a_resume_point() {
    let fixture = prepare();
    publish(&fixture, "ckpt-a1-2", "attempt-1", "node-1", 2);
    let (outcomes, notes) = failover(&fixture, 1_000_000);
    assert_eq!(resumed_from(&outcomes, &notes), "ckpt-a1-2");

    // 새 시도가 node-2 에서 ckpt-a1-2 부터 돈다. 그 사이 끊겼던 node-1 이 살아나 옛 시도로 step 5 를 써 둔다
    stage_and_run(&fixture.db, "attempt-2", "lease-2", "node-2", "gpu-9", 3, 1_000_100, 3);
    assert!(fence_of(&fixture.db, "attempt-2") > fence_of(&fixture.db, "attempt-1"));
    publish(&fixture, "ckpt-a1-5", "attempt-1", "node-1", 5);

    // 새 시도도 체크포인트를 내기 전에 끊긴다 — 옛 시도의 step 5 가 아니라 새 시도가 시작한 ckpt-a1-2 로 이어가야 한다
    let (outcomes, notes) = failover(&fixture, 2_000_000);
    assert_eq!(resumed_from(&outcomes, &notes), "ckpt-a1-2", "{notes:#?}");
    assert!(
        notes.iter().any(|n| n.starts_with("FAILOVER_CHECKPOINT_SKIPPED") && n.contains("checkpoint_id=ckpt-a1-5")),
        "옛 시도의 늦은 체크포인트를 건너뛴 사실이 남아야 한다: {notes:#?}"
    );
}

/// 새 시도가 낸 체크포인트는 그대로 이어갈 지점이다(관문이 정상 이어받기를 막지 않는다).
#[test]
fn the_current_attempts_own_checkpoint_still_wins() {
    let fixture = prepare();
    publish(&fixture, "ckpt-a1-2", "attempt-1", "node-1", 2);
    let (outcomes, notes) = failover(&fixture, 1_000_000);
    assert_eq!(resumed_from(&outcomes, &notes), "ckpt-a1-2");
    stage_and_run(&fixture.db, "attempt-2", "lease-2", "node-2", "gpu-9", 3, 1_000_100, 3);
    publish(&fixture, "ckpt-a1-5", "attempt-1", "node-1", 5);
    publish(&fixture, "ckpt-a2-3", "attempt-2", "node-2", 3);
    let (outcomes, notes) = failover(&fixture, 2_000_000);
    assert_eq!(resumed_from(&outcomes, &notes), "ckpt-a2-3", "{notes:#?}");
}
