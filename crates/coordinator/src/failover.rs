//! 장애 이어받기 — 끊긴 노드의 작업을 **규범 경로로** 되돌린다(신뢰망 남은 일 G).
//!
//! # 무엇을 끊겼다고 보나
//!
//! ```text
//! Job 이 STAGING · RUNNING 이고, 그 Job 의 **가장 최근 시도**가 끝나지 않았는데
//! 그 시도의 Lease 만료 + 유예(grace) 가 지났다
//! ```
//!
//! 규범의 guard 그대로다 — `RUNNING -> INTERRUPTED (NODE_LOST)`: "lease 만료 + grace 경과".
//! 실행 중인 Agent 는 갱신 세션으로 Lease 를 늘린다(`--renew-during-execution-ms`) — 그게 멈췄다는 것이 신호다.
//!
//! # 어디로 되돌리나 (규범 §2)
//!
//! ```text
//! STAGING(ACK 전)  -> QUEUED                               STAGING_NODE_LOST — 아직 실행하지 않았다
//! RUNNING          -> INTERRUPTED -> REPLANNING -> QUEUED  마지막 체크포인트가 있을 때 — 새 시도가 거기서 이어간다
//! RUNNING          -> INTERRUPTED -> FAILED                없을 때(NO_COMMITTED_CHECKPOINT) — 처음부터 다시 돌리지 않는다
//! ```
//!
//! # 어느 체크포인트를 믿나 — 넷을 **전부** 본다
//!
//! ```text
//! 1  생산자 서명 — 풀 노드 키로 검증(§0.2)
//! 2  이 Job 의 시도가 만든 것 — 그 시도의 fence · 노드와 서명된 값이 같다(다른 시도 · 다른 노드가 흉내 낸 것 거부)
//! 3  공유 저장소의 파일을 다시 해시해 매니페스트 · 머클 루트와 맞다
//! 4  그중 (fence, step) 이 가장 큰 것
//! ```
//!
//! ★ "COMMITTED 체크포인트" 를 **공유 저장소 하나에 확정 + 서명**으로 읽는다 — 신뢰망 계획의 "기준선과 다른 점" 이다.
//!   공개 풀에서 이 완화를 쓰면 안 된다.
//!
//! # 하지 않는 것
//!
//! ```text
//! 옛 노드 멈추기      못 한다. 그 노드가 살아 있으면 계속 돌 수 있다 — 옛 Lease 를 되돌리는 같은 커밋에서 **폐기**해
//!                     그 뒤의 갱신을 거부할 뿐이다(§0.4 — 억제이지 방지가 아니다). PURE 작업을 전제한다
//!                     ★ 결함 227(검수 76) — 전에는 "새 시도의 더 큰 fence 가 옛 Lease 갱신을 막는다" 고 적었는데, 옛 Lease 갱신은
//!                       자기 행의 fence 만 봐서 막히지 않았다. 판정 직전에 시작된 갱신이 옛 Lease 를 되살릴 수 있었다
//! 옛 예약 풀기        풀지 않는다. 만료 **표시**만 한다(§A1 4a). 그 노드는 운영자가 멈췄음을 확인하고 풀 때까지 새 일을
//!                     받지 않는다 — 살아 있는 좀비와 새 작업이 같은 GPU 를 다투지 않게
//! 옛 시도 상태        바꾸지 않는다. Coordinator 가 그 시도의 끝을 관측하지 못했다 — 지어내지 않는다
//! ```

use std::path::{Path, PathBuf};

use gputeer_crypto::{Ed25519Verifier, InMemoryKeyring, VerifyingKey};
use gputeer_protocol::attempt_state::AttemptState;
use gputeer_protocol::job_state::JobState;
use gputeer_protocol::{pb, verify};
use prost::Message;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

/// 장애 판정 정책 — 기본값을 두지 않는다(호출부가 정한다).
#[derive(Debug, Clone)]
pub struct FailoverPolicy {
    /// Lease 만료 뒤 기다리는 시간. 짧으면 잠깐 끊긴 노드의 작업을 뺏는다.
    pub grace_ms: u64,
    /// 공유 저장소. 없으면 실행 중이던 작업은 이어갈 체크포인트가 없어 FAILED 로 끝난다.
    pub shared_checkpoint_root: Option<PathBuf>,
    /// 체크포인트 생산자 서명을 검증할 풀 노드 키.
    pub producer_keys: Vec<(String, VerifyingKey)>,
}

/// 한 Job 에 대해 한 일.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailoverOutcome {
    Requeued {
        job_id: String,
        lost_attempt_id: String,
        lost_node_id: String,
        was_running: bool,
        resume_checkpoint_id: Option<String>,
        resume_step: Option<u64>,
    },
    FailedNoCheckpoint {
        job_id: String,
        lost_attempt_id: String,
        lost_node_id: String,
    },
}

impl FailoverOutcome {
    /// 로그 한 줄.
    pub fn line(&self) -> String {
        match self {
            Self::Requeued {
                job_id,
                lost_attempt_id,
                lost_node_id,
                was_running,
                resume_checkpoint_id,
                resume_step,
            } => format!(
                "FAILOVER_REQUEUED job_id={job_id} lost_attempt={lost_attempt_id} lost_node={lost_node_id} \
                 was_running={was_running} resume_checkpoint={} resume_step={}",
                resume_checkpoint_id.as_deref().unwrap_or("-"),
                resume_step.map(|step| step.to_string()).unwrap_or_else(|| "-".to_string())
            ),
            Self::FailedNoCheckpoint {
                job_id,
                lost_attempt_id,
                lost_node_id,
            } => format!(
                "FAILOVER_FAILED job_id={job_id} lost_attempt={lost_attempt_id} lost_node={lost_node_id} \
                 reason=NO_COMMITTED_CHECKPOINT"
            ),
        }
    }
}

/// control DB 를 훑어 끊긴 시도의 Job 을 되돌린다. Job 하나에 트랜잭션 하나.
///
/// `notes` 에는 믿지 않고 버린 체크포인트와 그 이유가 쌓인다 — 조용히 버리지 않는다.
pub fn failover_lost_attempts(
    control_db: &Path,
    policy: &FailoverPolicy,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
) -> Result<Vec<FailoverOutcome>, String> {
    // 스키마가 없으면 만든다(아직 아무것도 예약하지 않은 DB 도 훑을 수 있게).
    crate::job_store::CoordinatorJobStore::open(control_db).map_err(|e| e.to_string())?;
    crate::staging_store::CoordinatorStagingStore::open(control_db).map_err(|e| e.to_string())?;
    let mut connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;

    let candidates =
        crate::job_store::list_in_run_states(&connection).map_err(|e| e.to_string())?;
    let mut outcomes = Vec::new();
    for listed in candidates {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        // 잠금을 잡은 뒤 다시 읽는다 — 그 사이 보고가 Job 을 끝냈을 수 있다.
        let Some(job) =
            crate::job_store::fetch_job(&transaction, &listed.job_id).map_err(|e| e.to_string())?
        else {
            continue;
        };
        if !matches!(job.state, JobState::Staging | JobState::Running) {
            continue;
        }
        let latest: Option<String> = transaction
            .query_row(
                "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
                 ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
                rusqlite::params![job.job_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(attempt_id) = latest else {
            continue;
        };
        let attempt = crate::staging_store::fetch_attempt(&transaction, &attempt_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("시도 {attempt_id} 가 사라졌다"))?;
        if matches!(
            attempt.state,
            AttemptState::Completed
                | AttemptState::Failed
                | AttemptState::Cancelled
                | AttemptState::Canonical
                | AttemptState::Superseded
        ) {
            continue;
        }
        let lease = crate::lease_store::fetch_lease(&transaction, &attempt.lease_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("시도 {attempt_id} 의 Lease {} 가 없다", attempt.lease_id))?;
        if now_unix_ms <= lease.expires_at_unix_ms.saturating_add(policy.grace_ms) {
            continue;
        }
        let lost_node_id = attempt.node_ids.first().cloned().unwrap_or_default();
        let resume = if job.state == JobState::Running {
            find_resume_point(&transaction, policy, &job.job_id, now_unix_ms, notes)?
        } else {
            None
        };
        let updated = crate::job_store::requeue_after_node_lost(
            &transaction,
            &job,
            resume.as_ref().map(|found| found.body.clone()),
            now_unix_ms,
        )
        .map_err(|e| e.to_string())?;
        // ★ 2026-09-23 (결함 227 · 검수 76) — 옛 Lease 를 **같은 커밋에서** 폐기한다. 전에는 만료만 보고 되돌렸는데, 판정 직전에
        //   시작된 갱신이 판정 뒤에 저장되면(옛 시각을 잡은 채) 옛 Lease 가 미래로 늘어나 옛 노드와 새 노드가 같이 돌 수 있었다.
        //   갱신 저장은 자기 트랜잭션 안에서 폐기를 다시 읽으므로, 이 커밋 뒤의 갱신은 전부 REVOKED 로 거부된다.
        crate::lease_store::revoke_within(&transaction, &attempt.lease_id, now_unix_ms)
            .map_err(|e| e.to_string())?;
        if !lost_node_id.is_empty() {
            // 표시만 한다 — 지우지 않는다(§A1 4a).
            crate::staging_store::mark_reservation_expired(
                &transaction,
                &lost_node_id,
                now_unix_ms,
            )
            .map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        outcomes.push(if updated.state == JobState::Queued {
            FailoverOutcome::Requeued {
                job_id: job.job_id.clone(),
                lost_attempt_id: attempt_id,
                lost_node_id,
                was_running: job.state == JobState::Running,
                resume_checkpoint_id: resume.as_ref().map(|found| found.checkpoint_id.clone()),
                resume_step: resume.as_ref().map(|found| found.step),
            }
        } else {
            FailoverOutcome::FailedNoCheckpoint {
                job_id: job.job_id.clone(),
                lost_attempt_id: attempt_id,
                lost_node_id,
            }
        });
    }
    Ok(outcomes)
}

struct ResumePoint {
    checkpoint_id: String,
    step: u64,
    body: Vec<u8>,
}

fn find_resume_point(
    connection: &Connection,
    policy: &FailoverPolicy,
    job_id: &str,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
) -> Result<Option<ResumePoint>, String> {
    let Some(shared_root) = policy.shared_checkpoint_root.as_ref() else {
        notes.push(format!(
            "FAILOVER_NO_SHARED_ROOT job_id={job_id} — 공유 저장소가 없어 이어갈 체크포인트를 찾지 않는다"
        ));
        return Ok(None);
    };
    let mut keyring = InMemoryKeyring::new();
    for (id, key) in &policy.producer_keys {
        keyring.insert(id.clone(), *key);
    }
    let verifier = Ed25519Verifier::new(&keyring);
    let mut best: Option<(u64, u64, ResumePoint)> = None;
    for (checkpoint_id, body) in
        gputeer_checkpoint::shared::list_signed_manifests(shared_root, job_id)?
    {
        let mut skip = |why: String| {
            notes.push(format!(
                "FAILOVER_CHECKPOINT_SKIPPED job_id={job_id} checkpoint_id={checkpoint_id} reason={why}"
            ))
        };
        let Ok(manifest) = pb::CheckpointManifest::decode(body.as_slice()) else {
            skip("디코드 실패".into());
            continue;
        };
        if manifest.checkpoint_id != checkpoint_id {
            skip("파일 이름과 checkpoint_id 가 다르다".into());
            continue;
        }
        if let Err(error) = verify(
            &manifest,
            1,
            &verifier,
            now_unix_ms,
            &mut gputeer_protocol::signing::NoReplayCheck,
        ) {
            skip(format!("생산자 서명 검증 실패 {error:?}"));
            continue;
        }
        if manifest.job_id != job_id {
            skip("다른 Job 의 체크포인트다".into());
            continue;
        }
        let Some(attempt) = crate::staging_store::fetch_attempt(connection, &manifest.attempt_id)
            .map_err(|e| e.to_string())?
        else {
            skip("모르는 시도가 만들었다".into());
            continue;
        };
        if attempt.job_id != job_id
            || attempt.fence_epoch != manifest.fence_epoch
            || !attempt.node_ids.contains(&manifest.producer_node_id)
        {
            skip("그 시도의 fence · 노드와 서명된 값이 다르다".into());
            continue;
        }
        if let Err(error) = gputeer_checkpoint::shared::verify_on_disk(shared_root, &manifest) {
            skip(error);
            continue;
        }
        let key = (manifest.fence_epoch, manifest.step);
        if best
            .as_ref()
            .is_none_or(|(fence, step, _)| key > (*fence, *step))
        {
            best = Some((
                key.0,
                key.1,
                ResumePoint {
                    checkpoint_id: checkpoint_id.clone(),
                    step: manifest.step,
                    body,
                },
            ));
        }
    }
    Ok(best.map(|(_, _, point)| point))
}

/// 운영자가 풀어 준 끊긴 노드의 예약.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorRelease {
    pub node_id: String,
    pub attempt_id: String,
    pub job_id: String,
    pub released_gpu_ids: Vec<String>,
}

/// ★ 2026-09-23 (신뢰망 남은 일 G) — **운영자가 "그 노드는 멈췄다" 고 확인한 뒤** 끊긴 노드의 옛 예약을 푼다.
///
/// 장애 이어받기는 옛 예약을 지우지 않는다(§A1 4a — 살아 있는 좀비와 새 작업이 같은 GPU 를 다투지 않게).
/// 그래서 그 노드를 다시 쓰려면 누군가 멈췄음을 확인해야 한다 — Coordinator 는 증명할 수 없다(§0.4).
///
/// 관문: 그 예약의 시도가 **대체됐거나 끝난 Job 의 것**일 때만 푼다. Job 이 아직 그 시도로 도는 중(STAGING · RUNNING ·
/// PAUSED)이면 거부한다 — 운영자가 노드를 잘못 짚어 지금 도는 남의 작업을 푸는 실수를 막는다(§0.1).
///
/// ★ 2026-09-23 (결함 228 · 검수 76 — 전에는 "살아 있는 시도의 예약은 거부한다" 고 적었다) — 이 관문은 **Job 이 넘어갔는지**를
///   볼 뿐, 옛 시도의 **프로세스가 멈췄는지**는 증명하지 않는다. 장애 이어받기 뒤 옛 시도의 저장 상태는 STARTING · RUNNING 으로 남아
///   있을 수 있고, 그 예약도 풀린다. 멈춤의 근거는 **운영자 진술**뿐이다(그래서 진술을 남긴다) — Coordinator 는 증명할 수 없다(§0.4).
///
/// 기록은 `coordinator_operator_releases` 에 남는다 — 누가 · 언제 · 무엇을 풀었나. 되돌리지 않는 기록이다.
pub fn release_lost_node_by_operator(
    control_db: &Path,
    node_id: &str,
    operator_statement: &str,
    now_unix_ms: u64,
) -> Result<OperatorRelease, String> {
    if operator_statement.trim().is_empty() {
        return Err("RELEASE_REFUSED: 운영자 진술(누가 · 무엇을 확인했나)이 비었다".to_string());
    }
    crate::staging_store::CoordinatorStagingStore::open(control_db).map_err(|e| e.to_string())?;
    let mut connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS coordinator_operator_releases (
                node_id TEXT NOT NULL,
                attempt_id TEXT NOT NULL,
                job_id TEXT NOT NULL,
                operator_statement TEXT NOT NULL,
                released_at_unix_ms BLOB NOT NULL,
                PRIMARY KEY(node_id, attempt_id)
            );",
        )
        .map_err(|e| e.to_string())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let reservation = crate::staging_store::fetch_node_reservation(&transaction, node_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("RELEASE_REFUSED: {node_id} 에 예약이 없다"))?;
    let job = crate::job_store::fetch_job(&transaction, &reservation.job_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("RELEASE_REFUSED: 예약의 Job {} 이 없다", reservation.job_id))?;
    let latest: Option<String> = transaction
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![reservation.job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let superseded = latest.as_deref() != Some(reservation.attempt_id.as_str());
    let job_moved_on = matches!(
        job.state,
        JobState::Queued | JobState::Completed | JobState::Failed
    );
    if !superseded && !job_moved_on {
        return Err(format!(
            "RELEASE_REFUSED: {node_id} 의 예약은 **살아 있는 시도**({}) 의 것이다(Job {:?}) — 풀지 않는다. \
             장애 이어받기가 먼저 그 Job 을 되돌려야 한다",
            reservation.attempt_id, job.state
        ));
    }
    transaction
        .execute(
            "DELETE FROM coordinator_node_reservation_gpus WHERE node_id = ?1",
            rusqlite::params![node_id],
        )
        .map_err(|e| e.to_string())?;
    let removed = transaction
        .execute(
            "DELETE FROM coordinator_node_reservations WHERE node_id = ?1 AND attempt_id = ?2",
            rusqlite::params![node_id, reservation.attempt_id],
        )
        .map_err(|e| e.to_string())?;
    if removed != 1 {
        return Err(format!(
            "RELEASE_REFUSED: 예약 삭제가 {removed} 행을 지웠다"
        ));
    }
    transaction
        .execute(
            "INSERT INTO coordinator_operator_releases(
                node_id, attempt_id, job_id, operator_statement, released_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                node_id,
                reservation.attempt_id,
                reservation.job_id,
                operator_statement,
                now_unix_ms.to_be_bytes().to_vec(),
            ],
        )
        .map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(OperatorRelease {
        node_id: node_id.to_string(),
        attempt_id: reservation.attempt_id,
        job_id: reservation.job_id,
        released_gpu_ids: reservation.selected_gpu_ids,
    })
}

/// ★ 2026-09-23 (신뢰망 남은 일 H) — 노드 소유자가 작업을 멈췄다(그 노드가 서명한 INTERRUPTED 보고 · 종료 관측).
///
/// ```text
/// Job  RUNNING -> PAUSED (OWNER_PREEMPT)      이어갈 지점 = 공유 저장소의 검증된 마지막 체크포인트
/// 노드 "되찾김" 표시                          pool_snapshot 이 그 노드를 **다시 Hello 할 때까지** 소식 없음으로 접는다
/// ```
///
/// 스케줄러는 PAUSED Job 도 배치한다 — 다른 노드에서 `PAUSED -> RUNNING`(RESUMED)으로 이어간다. Lease 만료를 기다리지 않는다.
/// 이 Job 의 최근 시도가 아니거나 Job 이 RUNNING 이 아니면 아무것도 하지 않는다(`Ok(None)`).
pub fn pause_for_owner_preempt(
    control_db: &Path,
    job_id: &str,
    attempt_id: &str,
    node_id: &str,
    policy: &FailoverPolicy,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
) -> Result<Option<String>, String> {
    let mut connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;
    ensure_reclaim_table(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let line = pause_for_owner_preempt_within(
        &transaction,
        job_id,
        attempt_id,
        node_id,
        policy,
        now_unix_ms,
        notes,
    )?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(line)
}

/// [`pause_for_owner_preempt`] 의 본체 — **호출자의 트랜잭션 안에서** 돈다(커밋하지 않는다).
///
/// ★ 2026-09-23 (결함 215 · 검수 73) — 종료 보고 저장 · 예약 해제와 **같은 커밋**에 넣으려고 뗐다. 따로 커밋하면
///   그 사이에 죽었을 때 "예약 없음 + Job RUNNING" 이 남고, 재전송도 그것을 되살리지 못했다.
pub(crate) fn pause_for_owner_preempt_within(
    transaction: &Connection,
    job_id: &str,
    attempt_id: &str,
    node_id: &str,
    policy: &FailoverPolicy,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
) -> Result<Option<String>, String> {
    ensure_reclaim_table(transaction)?;
    let Some(job) = crate::job_store::fetch_job(transaction, job_id).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if job.state != JobState::Running {
        return Ok(None);
    }
    let latest: Option<String> = transaction
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if latest.as_deref() != Some(attempt_id) {
        return Ok(None);
    }
    // ★ 2026-09-24 (결함 234 · 재검수 79) — 이 함수는 종료 보고 저장과 **같은 트랜잭션**에서 돈다(215). 이어갈 지점 조회가 공유 저장소
    //   장애로 실패해도 오류로 올리지 않는다 — 올리면 보고 저장까지 롤백되고, 저장소가 계속 죽어 있으면 보고가 영영 안 남는다(증거 유실).
    //   그때는 새 지점 없이 멈춘다(전에 고른 지점은 그대로 둔다) — 사실을 한 줄 남긴다.
    let resume = match find_resume_point(transaction, policy, job_id, now_unix_ms, notes) {
        Ok(found) => found,
        Err(error) => {
            notes.push(format!(
                "RESUME_POINT_UNAVAILABLE job_id={job_id} detail={error} — 새 이어갈 지점 없이 멈춘다(보고는 저장한다)"
            ));
            None
        }
    };
    crate::job_store::pause_for_owner_preempt(
        transaction,
        &job,
        resume.as_ref().map(|found| found.body.clone()),
    )
    .map_err(|e| e.to_string())?;
    // ★ 결함 227 — 선점으로 옮기는 Job 도 옛 Lease 를 같은 커밋에서 폐기한다(늦은 갱신이 옛 시도를 되살리지 않게).
    let attempt = crate::staging_store::fetch_attempt(transaction, attempt_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("시도 {attempt_id} 가 사라졌다"))?;
    crate::lease_store::revoke_within(transaction, &attempt.lease_id, now_unix_ms)
        .map_err(|e| e.to_string())?;
    // ★ 2026-09-24 (결함 237 · 재검수 80) — 되찾음 판정은 "되찾은 시각 >= 마지막 FRESH 시각" 이다. 시계가 뒤로 가면 방금 되찾은 노드가
    //   다시 후보가 됐다. 그래서 되찾은 시각을 **이미 적힌 마지막 FRESH 이상**으로 적는다 — 되찾기 전의 인사로는 풀리지 않는다.
    let last_fresh: Option<u64> = if transaction
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_node_session_seen'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        transaction
            .query_row(
                "SELECT last_seen_unix_ms FROM coordinator_node_session_seen WHERE node_id = ?1",
                rusqlite::params![node_id],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .and_then(|raw| <[u8; 8]>::try_from(raw.as_slice()).ok())
            .map(u64::from_be_bytes)
    } else {
        None
    };
    let reclaimed_at = last_fresh.map_or(now_unix_ms, |seen| seen.max(now_unix_ms));
    transaction
        .execute(
            "INSERT INTO coordinator_node_reclaims(node_id, reclaimed_at_unix_ms) VALUES (?1, ?2)
             ON CONFLICT(node_id) DO UPDATE SET reclaimed_at_unix_ms = excluded.reclaimed_at_unix_ms",
            rusqlite::params![node_id, reclaimed_at.to_be_bytes().to_vec()],
        )
        .map_err(|e| e.to_string())?;
    Ok(Some(format!(
        "OWNER_PREEMPTED job_id={job_id} attempt_id={attempt_id} node_id={node_id} resume_step={}",
        resume
            .as_ref()
            .map(|found| found.step.to_string())
            .unwrap_or_else(|| "-".to_string())
    )))
}

/// "소유자가 되찾음" 표시. `pool_snapshot()` 이 읽는다(있을 때만).
pub(crate) fn ensure_reclaim_table(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS coordinator_node_reclaims (
                node_id TEXT PRIMARY KEY,
                reclaimed_at_unix_ms BLOB NOT NULL CHECK(length(reclaimed_at_unix_ms) = 8)
            );",
        )
        .map_err(|e| e.to_string())
}
