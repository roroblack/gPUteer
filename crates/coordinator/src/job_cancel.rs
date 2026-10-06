//! ★ 2026-10-05 (실행 알림 계약 v18q · 계획 §5 5번) — 운영자 취소 `gputeer cancel-job`.
//!
//! ```text
//! SUBMITTED · PLANNING · QUEUED · PAUSED     → CANCELLED 만. Lease · 예약은 건드리지 않는다(옛 시도의 것은 이어받기 · 선점이 이미 폐기했다)
//! STAGING · RUNNING  (가) 최신 시도에 풀리지 않은 불명 · Job 에 보류  → CANCELLED 만(Lease · 예약 · 보류 · 시도 그대로 — STOP · 운영자 해제가 푼다)
//!                    (나) 아니면                                    → CANCELLED + 최신 시도의 Lease 를 같은 커밋에서 폐기
//! COMPLETED · FAILED · ARCHIVED              → 거부(표에 행이 없다)
//! ```
//!
//! ★ 예약은 **어느 경우에도 풀지 않는다** — 해제는 종료 보고 · STOP · 운영자 해제(증거)로만 한다(DoD-62). 돌고 있는 작업은 다음 갱신에서 서명된
//!   RENEW_OUTCOME_REVOKED 를 받아 멈추고, 끊긴 노드는 자기 끊김 시한에 멈춘다 — 즉시 정지를 보장하지 않는다(v18q ③ 한계).
//! ★ 감사 행(`coordinator_job_cancellations`)을 같은 커밋에 쓴다. 이미 취소됐으면 그 행을 그대로 돌려준다(진술이 달라도 덮지 않는다).

use std::path::Path;

use gputeer_protocol::job_state::JobState;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

pub(crate) const CANCELLATIONS_DDL: &str = "CREATE TABLE IF NOT EXISTS coordinator_job_cancellations (
    job_id TEXT PRIMARY KEY,
    from_state TEXT NOT NULL,
    latest_attempt_id TEXT,
    deferred INTEGER NOT NULL CHECK(deferred IN (0, 1)),
    lease_revoked INTEGER NOT NULL CHECK(lease_revoked IN (0, 1)),
    operator_statement TEXT NOT NULL,
    cancelled_at_unix_ms BLOB NOT NULL CHECK(length(cancelled_at_unix_ms) = 8)
);";

/// 취소 한 건의 감사 행.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobCancellation {
    pub job_id: String,
    /// 취소 직전 Job 상태(표의 이름).
    pub from_state: String,
    pub latest_attempt_id: Option<String>,
    /// (가) — 불명 · 보류가 있어 Lease · 예약 · 보류를 그대로 두었다.
    pub deferred: bool,
    /// (나) — 최신 시도의 Lease 를 폐기했다.
    pub lease_revoked: bool,
    pub operator_statement: String,
    pub cancelled_at_unix_ms: u64,
    /// false 면 이미 있던 취소를 돌려준 것이다(이번 호출은 아무것도 바꾸지 않았다).
    pub created: bool,
}

pub fn cancel_job_by_operator(
    control_db: &Path,
    job_id: &str,
    operator_statement: &str,
    now_unix_ms: u64,
) -> Result<JobCancellation, String> {
    if operator_statement.trim().is_empty() {
        return Err("CANCEL_REFUSED: 운영자 진술(누가 · 왜 취소하나)이 비었다".to_string());
    }
    crate::job_store::CoordinatorJobStore::open(control_db).map_err(|e| e.to_string())?;
    crate::staging_store::CoordinatorStagingStore::open(control_db).map_err(|e| e.to_string())?;
    let mut connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;
    crate::job_holds::initialize_schema(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    // (코드 검수 cancel_code ④) 감사 표는 취소와 같은 트랜잭션에서 만든다 — 실패한 취소가 스키마만 남기지 않는다. 다른 저장소를 여는 위 단계는
    //   기존 명령들과 같은 멱등 스키마 준비다(업무 행은 쓰지 않는다).
    transaction.execute_batch(CANCELLATIONS_DDL).map_err(|e| e.to_string())?;
    if let Some(existing) = fetch_cancellation(&transaction, job_id)? {
        transaction.commit().map_err(|e| e.to_string())?;
        return Ok(existing);
    }
    let job = crate::job_store::fetch_job(&transaction, job_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("CANCEL_REFUSED: Job {job_id} 이 없다"))?;
    if job.state == JobState::Cancelled {
        return Err(format!(
            "CANCEL_REFUSED: Job {job_id} 은 CANCELLED 인데 취소 감사 행이 없다 — 저장소가 어긋났다(사람이 봐야 한다)"
        ));
    }
    let latest_attempt_id: Option<String> = transaction
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (deferred, lease_revoked) = match (job.state, latest_attempt_id.as_deref()) {
        (JobState::Staging | JobState::Running, Some(attempt_id)) => {
            let unknown = crate::run_notice_store::has_unresolved_run_unknown(&transaction, attempt_id)?;
            if unknown || crate::job_holds::job_is_held(&transaction, job_id)? {
                (true, false)
            } else {
                let attempt = crate::staging_store::fetch_attempt(&transaction, attempt_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("CANCEL_REFUSED: 최신 시도 {attempt_id} 의 행이 없다(저장소 손상)"))?;
                crate::lease_store::revoke_within(&transaction, &attempt.lease_id, now_unix_ms)
                    .map_err(|e| format!("CANCEL_REFUSED: 시도 {attempt_id} 의 Lease 를 폐기하지 못했다 — {e}"))?;
                (false, true)
            }
        }
        // (코드 검수 cancel_code ③) STAGING · RUNNING 인데 시도 행이 없으면 Lease 를 폐기할 수 없다 — 조용히 취소하지 않고 손상으로 거부한다
        (JobState::Staging | JobState::Running, None) => {
            return Err(format!(
                "CANCEL_REFUSED: Job {job_id} 은 {} 인데 시도 행이 없다(저장소 손상 — 사람이 봐야 한다)",
                job.state.table_name()
            ))
        }
        _ => (false, false),
    };
    crate::job_store::cancel_within(&transaction, &job).map_err(|e| match e {
        crate::job_store::JobStoreError::InvalidTransition { from, .. } => format!(
            "CANCEL_REFUSED: Job {job_id} 은 {} 이라 취소할 수 없다(표에 그 행이 없다)",
            from.table_name()
        ),
        other => other.to_string(),
    })?;
    let cancellation = JobCancellation {
        job_id: job_id.to_string(),
        from_state: job.state.table_name().to_string(),
        latest_attempt_id,
        deferred,
        lease_revoked,
        operator_statement: operator_statement.to_string(),
        cancelled_at_unix_ms: now_unix_ms,
        created: true,
    };
    transaction
        .execute(
            "INSERT INTO coordinator_job_cancellations VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                cancellation.job_id,
                cancellation.from_state,
                cancellation.latest_attempt_id,
                cancellation.deferred,
                cancellation.lease_revoked,
                cancellation.operator_statement,
                now_unix_ms.to_be_bytes().to_vec(),
            ],
        )
        .map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(cancellation)
}

fn fetch_cancellation(connection: &Connection, job_id: &str) -> Result<Option<JobCancellation>, String> {
    let row = connection
        .query_row(
            "SELECT from_state, latest_attempt_id, deferred, lease_revoked, operator_statement, cancelled_at_unix_ms
             FROM coordinator_job_cancellations WHERE job_id = ?1",
            rusqlite::params![job_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((from_state, latest_attempt_id, deferred, lease_revoked, operator_statement, at)) = row else {
        return Ok(None);
    };
    let at = <[u8; 8]>::try_from(at.as_slice())
        .map(u64::from_be_bytes)
        .map_err(|_| format!("Job {job_id} 의 취소 시각을 읽지 못했다(저장소 손상)"))?;
    Ok(Some(JobCancellation {
        job_id: job_id.to_string(),
        from_state,
        latest_attempt_id,
        deferred,
        lease_revoked,
        operator_statement,
        cancelled_at_unix_ms: at,
        created: false,
    }))
}
