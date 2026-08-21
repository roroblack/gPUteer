//! Durable Job/Queue control truth for the scheduler roadmap's slice 2a.
//!
//! The standalone API owns accepted submission persistence,
//! `SUBMITTED -> PLANNING -> QUEUED`, queue ordering, queue terminal reasons,
//! and retry idempotency. `QUEUED -> STAGING` is intentionally absent here:
//! [`crate::staging_store`] owns that transition together with Attempt creation,
//! fence allocation, and Lease insertion in one transaction.

use std::path::Path;

use rusqlite::{
    Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior,
};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Submitted,
    Planning,
    Queued,
    Staging,
    Failed,
}

impl JobState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "SUBMITTED",
            Self::Planning => "PLANNING",
            Self::Queued => "QUEUED",
            Self::Staging => "STAGING",
            Self::Failed => "FAILED",
        }
    }

    fn parse(value: &str) -> Result<Self, JobStoreError> {
        match value {
            "SUBMITTED" => Ok(Self::Submitted),
            "PLANNING" => Ok(Self::Planning),
            "QUEUED" => Ok(Self::Queued),
            "STAGING" => Ok(Self::Staging),
            "FAILED" => Ok(Self::Failed),
            other => Err(JobStoreError::CorruptData(format!(
                "unknown job state in durable store: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueFailure {
    DeadlinePassed,
    QueueTimeout,
    PermanentlyInfeasible { reason: String },
}

impl QueueFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::DeadlinePassed => "DEADLINE_PASSED",
            Self::QueueTimeout => "QUEUE_TIMEOUT",
            Self::PermanentlyInfeasible { .. } => "PERMANENTLY_INFEASIBLE",
        }
    }

    fn detail(&self) -> Option<&str> {
        match self {
            Self::PermanentlyInfeasible { reason } => Some(reason),
            Self::DeadlinePassed | Self::QueueTimeout => None,
        }
    }

    fn parse(code: &str, detail: Option<String>) -> Result<Self, JobStoreError> {
        match code {
            "DEADLINE_PASSED" if detail.is_none() => Ok(Self::DeadlinePassed),
            "QUEUE_TIMEOUT" if detail.is_none() => Ok(Self::QueueTimeout),
            "PERMANENTLY_INFEASIBLE" => {
                let reason = detail.filter(|value| !value.trim().is_empty()).ok_or_else(|| {
                    JobStoreError::CorruptData(
                        "PERMANENTLY_INFEASIBLE row has no reason".to_string(),
                    )
                })?;
                Ok(Self::PermanentlyInfeasible { reason })
            }
            other => Err(JobStoreError::CorruptData(format!(
                "invalid queue failure in durable store: {other}"
            ))),
        }
    }
}

/// A submission that has already passed signature, quorum, hard-filter, and
/// duplicate-risk policy validation. Rejected submissions never reach this
/// store and therefore never create a Job, matching the normative state table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedJobSubmission {
    pub idempotency_key: [u8; 16],
    pub job_id: String,
    pub submitter_device_id: String,
    pub manifest_hash: [u8; 32],
    pub deadline_unix_ms: Option<u64>,
    /// `None` represents the manifest's `max_queue_minutes == 0` contract:
    /// there is no independent queue timeout and the deadline governs.
    pub max_queue_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJob {
    pub job_id: String,
    pub submitter_device_id: String,
    pub manifest_hash: [u8; 32],
    pub state: JobState,
    pub submitted_at_unix_ms: u64,
    pub planning_at_unix_ms: Option<u64>,
    pub queued_at_unix_ms: Option<u64>,
    pub staging_at_unix_ms: Option<u64>,
    pub deadline_unix_ms: Option<u64>,
    pub max_queue_duration_ms: Option<u64>,
    pub plan_id: Option<String>,
    pub queue_failure: Option<QueueFailure>,
    pub failed_at_unix_ms: Option<u64>,
    pub revision: u64,
}

impl StoredJob {
    fn matches_submission(&self, submission: &AcceptedJobSubmission) -> bool {
        self.job_id == submission.job_id
            && self.submitter_device_id == submission.submitter_device_id
            && self.manifest_hash == submission.manifest_hash
            && self.deadline_unix_ms == submission.deadline_unix_ms
            && self.max_queue_duration_ms == submission.max_queue_duration_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitResult {
    pub job: StoredJob,
    /// `false` means the idempotency key replayed the original durable result.
    pub created: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum JobStoreError {
    InvalidInput(&'static str),
    NotFound,
    IdempotencyConflict {
        stored_job_id: String,
        requested_job_id: String,
    },
    JobIdConflict { job_id: String },
    InvalidTransition {
        from: JobState,
        to: JobState,
    },
    PlanConflict {
        stored_plan_id: String,
        requested_plan_id: String,
    },
    GuardNotMet(&'static str),
    ClockRollback {
        earlier_unix_ms: u64,
        later_unix_ms: u64,
    },
    CorruptData(String),
    Io(String),
    LockTimeout,
}

impl std::fmt::Display for JobStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(field) => write!(f, "invalid job store input: {field}"),
            Self::NotFound => write!(f, "job_id is not present in the durable store"),
            Self::IdempotencyConflict {
                stored_job_id,
                requested_job_id,
            } => write!(
                f,
                "idempotency key conflict: stored job={stored_job_id}, requested job={requested_job_id}"
            ),
            Self::JobIdConflict { job_id } => {
                write!(f, "job_id was already submitted with another key: {job_id}")
            }
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid durable Job transition: {from:?} -> {to:?}")
            }
            Self::PlanConflict {
                stored_plan_id,
                requested_plan_id,
            } => write!(
                f,
                "queued Job plan conflict: stored={stored_plan_id}, requested={requested_plan_id}"
            ),
            Self::GuardNotMet(guard) => write!(f, "Job transition guard not met: {guard}"),
            Self::ClockRollback {
                earlier_unix_ms,
                later_unix_ms,
            } => write!(
                f,
                "clock moved backwards: event={earlier_unix_ms}, prior={later_unix_ms}"
            ),
            Self::CorruptData(message) => write!(f, "job store corruption: {message}"),
            Self::Io(message) => write!(f, "job store I/O error: {message}"),
            Self::LockTimeout => write!(f, "job store lock acquisition timed out"),
        }
    }
}

impl std::error::Error for JobStoreError {}

pub(crate) fn map_sql_error(error: SqlError) -> JobStoreError {
    match error {
        SqlError::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            JobStoreError::LockTimeout
        }
        SqlError::SqliteFailure(code, _) => JobStoreError::Io(code.to_string()),
        other => JobStoreError::Io(other.to_string()),
    }
}

pub(crate) fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8], field: &str) -> Result<u64, JobStoreError> {
    let value: [u8; 8] = bytes.try_into().map_err(|_| {
        JobStoreError::CorruptData(format!("{field} must contain exactly 8 bytes"))
    })?;
    Ok(u64::from_be_bytes(value))
}

fn decode_hash(bytes: Vec<u8>) -> Result<[u8; 32], JobStoreError> {
    bytes.try_into().map_err(|_| {
        JobStoreError::CorruptData("manifest_hash must contain exactly 32 bytes".to_string())
    })
}

pub struct CoordinatorJobStore {
    connection: Connection,
}

pub(crate) fn initialize_schema(connection: &mut Connection) -> Result<(), JobStoreError> {
    connection
        .execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = DELETE;
            PRAGMA synchronous = FULL;

            CREATE TABLE IF NOT EXISTS coordinator_jobs (
                job_id TEXT PRIMARY KEY,
                submitter_device_id TEXT NOT NULL,
                manifest_hash BLOB NOT NULL,
                state TEXT NOT NULL,
                submitted_at_unix_ms BLOB NOT NULL,
                planning_at_unix_ms BLOB,
                queued_at_unix_ms BLOB,
                staging_at_unix_ms BLOB,
                deadline_unix_ms BLOB,
                max_queue_duration_ms BLOB,
                plan_id TEXT,
                queue_failure_kind TEXT,
                queue_failure_detail TEXT,
                failed_at_unix_ms BLOB,
                revision BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS job_submission_idempotency (
                idempotency_key BLOB PRIMARY KEY,
                job_id TEXT NOT NULL REFERENCES coordinator_jobs(job_id)
            );
            "#,
        )
        .map_err(map_sql_error)?;

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_sql_error)?;
    let has_staging_column = {
        let mut statement = transaction
            .prepare("PRAGMA table_info(coordinator_jobs)")
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(map_sql_error)?;
        let mut found = false;
        for row in rows {
            if row.map_err(map_sql_error)? == "staging_at_unix_ms" {
                found = true;
                break;
            }
        }
        found
    };
    if !has_staging_column {
        transaction
            .execute(
                "ALTER TABLE coordinator_jobs ADD COLUMN staging_at_unix_ms BLOB",
                [],
            )
            .map_err(map_sql_error)?;
    }
    transaction.commit().map_err(map_sql_error)
}

impl CoordinatorJobStore {
    /// Opens a file-backed SQLite control store. Callers must reject
    /// `!is_durable()` in production, as the existing Lease store does.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JobStoreError> {
        let mut connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        initialize_schema(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn is_durable(&self) -> bool {
        match self.connection.path() {
            Some(path) => !path.is_empty() && path != ":memory:",
            None => false,
        }
    }

    pub fn get(&self, job_id: &str) -> Result<Option<StoredJob>, JobStoreError> {
        fetch_job(&self.connection, job_id)
    }

    /// Returns the durable queue in deterministic FIFO order. `job_id` is the
    /// tie-breaker when two jobs have the same queue timestamp.
    pub fn list_queued(&self) -> Result<Vec<StoredJob>, JobStoreError> {
        let mut statement = self
            .connection
            .prepare(SELECT_QUEUED_SQL)
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map([], row_to_raw)
            .map_err(map_sql_error)?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(map_sql_error)?.into_stored()?);
        }
        jobs.sort_by_key(|job| (job.queued_at_unix_ms, job.job_id.clone()));
        Ok(jobs)
    }

    /// Atomically creates an accepted Job and binds its 16-byte control-plane
    /// idempotency key. A byte-identical logical retry returns the original
    /// record, including its original submit timestamp; key reuse with changed
    /// immutable input fails closed.
    pub fn submit_accepted(
        &mut self,
        submission: &AcceptedJobSubmission,
        submitted_at_unix_ms: u64,
    ) -> Result<SubmitResult, JobStoreError> {
        validate_submission(submission)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let mapped_job_id = transaction
            .query_row(
                "SELECT job_id FROM job_submission_idempotency WHERE idempotency_key = ?1",
                rusqlite::params![submission.idempotency_key.as_slice()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?;

        if let Some(stored_job_id) = mapped_job_id {
            let stored = fetch_job(&transaction, &stored_job_id)?.ok_or_else(|| {
                JobStoreError::CorruptData(format!(
                    "idempotency key points to missing job: {stored_job_id}"
                ))
            })?;
            if !stored.matches_submission(submission) {
                return Err(JobStoreError::IdempotencyConflict {
                    stored_job_id,
                    requested_job_id: submission.job_id.clone(),
                });
            }
            transaction.commit().map_err(map_sql_error)?;
            return Ok(SubmitResult {
                job: stored,
                created: false,
            });
        }

        if fetch_job(&transaction, &submission.job_id)?.is_some() {
            return Err(JobStoreError::JobIdConflict {
                job_id: submission.job_id.clone(),
            });
        }

        let stored = StoredJob {
            job_id: submission.job_id.clone(),
            submitter_device_id: submission.submitter_device_id.clone(),
            manifest_hash: submission.manifest_hash,
            state: JobState::Submitted,
            submitted_at_unix_ms,
            planning_at_unix_ms: None,
            queued_at_unix_ms: None,
            staging_at_unix_ms: None,
            deadline_unix_ms: submission.deadline_unix_ms,
            max_queue_duration_ms: submission.max_queue_duration_ms,
            plan_id: None,
            queue_failure: None,
            failed_at_unix_ms: None,
            revision: 0,
        };

        transaction
            .execute(
                "INSERT INTO coordinator_jobs(
                    job_id, submitter_device_id, manifest_hash, state,
                    submitted_at_unix_ms, planning_at_unix_ms, queued_at_unix_ms, staging_at_unix_ms,
                    deadline_unix_ms, max_queue_duration_ms, plan_id,
                    queue_failure_kind, queue_failure_detail, failed_at_unix_ms, revision
                 ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, ?6, ?7, NULL, NULL, NULL, NULL, ?8)",
                rusqlite::params![
                    stored.job_id,
                    stored.submitter_device_id,
                    stored.manifest_hash.as_slice(),
                    stored.state.as_str(),
                    encode_u64(stored.submitted_at_unix_ms),
                    stored.deadline_unix_ms.map(encode_u64),
                    stored.max_queue_duration_ms.map(encode_u64),
                    encode_u64(stored.revision),
                ],
            )
            .map_err(map_sql_error)?;
        transaction
            .execute(
                "INSERT INTO job_submission_idempotency(idempotency_key, job_id) VALUES (?1, ?2)",
                rusqlite::params![submission.idempotency_key.as_slice(), stored.job_id],
            )
            .map_err(map_sql_error)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(SubmitResult {
            job: stored,
            created: true,
        })
    }

    /// Applies only the normative `SUBMITTED -> PLANNING` transition. Repeating
    /// the call after an ambiguous response returns the original record.
    pub fn start_planning(
        &mut self,
        job_id: &str,
        at_unix_ms: u64,
    ) -> Result<StoredJob, JobStoreError> {
        self.transition(job_id, |transaction, mut job| {
            if job.state == JobState::Planning {
                return Ok(job);
            }
            if job.state != JobState::Submitted {
                return Err(JobStoreError::InvalidTransition {
                    from: job.state,
                    to: JobState::Planning,
                });
            }
            ensure_not_before(at_unix_ms, job.submitted_at_unix_ms)?;
            job.state = JobState::Planning;
            job.planning_at_unix_ms = Some(at_unix_ms);
            job.revision = job.revision.checked_add(1).ok_or_else(|| {
                JobStoreError::CorruptData("job revision overflow".to_string())
            })?;
            update_job(transaction, &job)?;
            Ok(job)
        })
    }

    /// Applies only `PLANNING -> QUEUED`, which represents `PLAN_READY` with
    /// at least one executable plan. A retry must name the same plan.
    pub fn enqueue(
        &mut self,
        job_id: &str,
        plan_id: &str,
        at_unix_ms: u64,
    ) -> Result<StoredJob, JobStoreError> {
        if plan_id.trim().is_empty() {
            return Err(JobStoreError::InvalidInput("plan_id"));
        }
        self.transition(job_id, |transaction, mut job| {
            if job.state == JobState::Queued {
                if job.plan_id.as_deref() == Some(plan_id) {
                    return Ok(job);
                }
                return Err(JobStoreError::PlanConflict {
                    stored_plan_id: job.plan_id.unwrap_or_default(),
                    requested_plan_id: plan_id.to_string(),
                });
            }
            if job.state != JobState::Planning {
                return Err(JobStoreError::InvalidTransition {
                    from: job.state,
                    to: JobState::Queued,
                });
            }
            let planning_at = job.planning_at_unix_ms.ok_or_else(|| {
                JobStoreError::CorruptData("PLANNING job has no planning timestamp".to_string())
            })?;
            ensure_not_before(at_unix_ms, planning_at)?;
            job.state = JobState::Queued;
            job.queued_at_unix_ms = Some(at_unix_ms);
            job.plan_id = Some(plan_id.to_string());
            job.revision = job.revision.checked_add(1).ok_or_else(|| {
                JobStoreError::CorruptData("job revision overflow".to_string())
            })?;
            update_job(transaction, &job)?;
            Ok(job)
        })
    }

    /// Records one of the three distinct normative `QUEUED -> FAILED`
    /// outcomes. Deadline and timeout guards are checked inside the same
    /// `BEGIN IMMEDIATE` transaction as the state update. Permanent
    /// infeasibility must be supplied by a later inventory/hard-filter caller
    /// with a non-empty durable reason; this store never infers it from a
    /// transient empty pool.
    pub fn fail_queued(
        &mut self,
        job_id: &str,
        failure: QueueFailure,
        at_unix_ms: u64,
    ) -> Result<StoredJob, JobStoreError> {
        if matches!(
            &failure,
            QueueFailure::PermanentlyInfeasible { reason } if reason.trim().is_empty()
        ) {
            return Err(JobStoreError::InvalidInput("permanent infeasibility reason"));
        }
        self.transition(job_id, |transaction, mut job| {
            if job.state == JobState::Failed && job.queue_failure.as_ref() == Some(&failure) {
                return Ok(job);
            }
            if job.state != JobState::Queued {
                return Err(JobStoreError::InvalidTransition {
                    from: job.state,
                    to: JobState::Failed,
                });
            }
            let queued_at = job.queued_at_unix_ms.ok_or_else(|| {
                JobStoreError::CorruptData("QUEUED job has no queue timestamp".to_string())
            })?;
            ensure_not_before(at_unix_ms, queued_at)?;
            match &failure {
                QueueFailure::DeadlinePassed => {
                    let deadline = job
                        .deadline_unix_ms
                        .ok_or(JobStoreError::GuardNotMet("Job has no deadline"))?;
                    if at_unix_ms <= deadline {
                        return Err(JobStoreError::GuardNotMet("now must be greater than deadline"));
                    }
                }
                QueueFailure::QueueTimeout => {
                    let limit = job.max_queue_duration_ms.ok_or(
                        JobStoreError::GuardNotMet("Job has no independent queue timeout"),
                    )?;
                    let elapsed = at_unix_ms.checked_sub(queued_at).ok_or(
                        JobStoreError::ClockRollback {
                            earlier_unix_ms: at_unix_ms,
                            later_unix_ms: queued_at,
                        },
                    )?;
                    if elapsed <= limit {
                        return Err(JobStoreError::GuardNotMet(
                            "queue wait must be greater than max queue duration",
                        ));
                    }
                }
                QueueFailure::PermanentlyInfeasible { .. } => {}
            }
            job.state = JobState::Failed;
            job.queue_failure = Some(failure.clone());
            job.failed_at_unix_ms = Some(at_unix_ms);
            job.revision = job.revision.checked_add(1).ok_or_else(|| {
                JobStoreError::CorruptData("job revision overflow".to_string())
            })?;
            update_job(transaction, &job)?;
            Ok(job)
        })
    }

    fn transition<F>(
        &mut self,
        job_id: &str,
        apply: F,
    ) -> Result<StoredJob, JobStoreError>
    where
        F: FnOnce(&Connection, StoredJob) -> Result<StoredJob, JobStoreError>,
    {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        let job = fetch_job(&transaction, job_id)?.ok_or(JobStoreError::NotFound)?;
        let result = apply(&transaction, job)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(result)
    }
}

fn validate_submission(submission: &AcceptedJobSubmission) -> Result<(), JobStoreError> {
    if submission.job_id.trim().is_empty() {
        return Err(JobStoreError::InvalidInput("job_id"));
    }
    if submission.submitter_device_id.trim().is_empty() {
        return Err(JobStoreError::InvalidInput("submitter_device_id"));
    }
    if submission.max_queue_duration_ms == Some(0) {
        return Err(JobStoreError::InvalidInput("max_queue_duration_ms"));
    }
    Ok(())
}

fn ensure_not_before(event: u64, prior: u64) -> Result<(), JobStoreError> {
    if event < prior {
        Err(JobStoreError::ClockRollback {
            earlier_unix_ms: event,
            later_unix_ms: prior,
        })
    } else {
        Ok(())
    }
}

pub(crate) fn fetch_job(
    connection: &Connection,
    job_id: &str,
) -> Result<Option<StoredJob>, JobStoreError> {
    connection
        .query_row(SELECT_JOB_SQL, rusqlite::params![job_id], row_to_raw)
        .optional()
        .map_err(map_sql_error)?
        .map(RawJobRow::into_stored)
        .transpose()
}

pub(crate) fn update_job(
    connection: &Connection,
    job: &StoredJob,
) -> Result<(), JobStoreError> {
    let (failure_kind, failure_detail) = match &job.queue_failure {
        Some(failure) => (Some(failure.code()), failure.detail()),
        None => (None, None),
    };
    let changed = connection
        .execute(
            "UPDATE coordinator_jobs SET
                state = ?2, planning_at_unix_ms = ?3, queued_at_unix_ms = ?4,
                staging_at_unix_ms = ?5, plan_id = ?6, queue_failure_kind = ?7,
                queue_failure_detail = ?8, failed_at_unix_ms = ?9, revision = ?10
             WHERE job_id = ?1",
            rusqlite::params![
                job.job_id,
                job.state.as_str(),
                job.planning_at_unix_ms.map(encode_u64),
                job.queued_at_unix_ms.map(encode_u64),
                job.staging_at_unix_ms.map(encode_u64),
                job.plan_id,
                failure_kind,
                failure_detail,
                job.failed_at_unix_ms.map(encode_u64),
                encode_u64(job.revision),
            ],
        )
        .map_err(map_sql_error)?;
    if changed != 1 {
        return Err(JobStoreError::CorruptData(format!(
            "updating {} changed {changed} rows",
            job.job_id
        )));
    }
    Ok(())
}

struct RawJobRow {
    job_id: String,
    submitter_device_id: String,
    manifest_hash: Vec<u8>,
    state: String,
    submitted_at_unix_ms: Vec<u8>,
    planning_at_unix_ms: Option<Vec<u8>>,
    queued_at_unix_ms: Option<Vec<u8>>,
    staging_at_unix_ms: Option<Vec<u8>>,
    deadline_unix_ms: Option<Vec<u8>>,
    max_queue_duration_ms: Option<Vec<u8>>,
    plan_id: Option<String>,
    queue_failure_kind: Option<String>,
    queue_failure_detail: Option<String>,
    failed_at_unix_ms: Option<Vec<u8>>,
    revision: Vec<u8>,
}

impl RawJobRow {
    fn into_stored(self) -> Result<StoredJob, JobStoreError> {
        let state = JobState::parse(&self.state)?;
        let queue_failure = match self.queue_failure_kind {
            Some(kind) => Some(QueueFailure::parse(&kind, self.queue_failure_detail)?),
            None if self.queue_failure_detail.is_none() => None,
            None => {
                return Err(JobStoreError::CorruptData(
                    "queue failure detail exists without a kind".to_string(),
                ))
            }
        };
        let valid_shape = match state {
            JobState::Submitted => {
                self.planning_at_unix_ms.is_none()
                    && self.queued_at_unix_ms.is_none()
                    && self.staging_at_unix_ms.is_none()
                    && self.plan_id.is_none()
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Planning => {
                self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_none()
                    && self.staging_at_unix_ms.is_none()
                    && self.plan_id.is_none()
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Queued => {
                self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_some()
                    && self.staging_at_unix_ms.is_none()
                    && self
                        .plan_id
                        .as_deref()
                        .is_some_and(|plan_id| !plan_id.trim().is_empty())
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Staging => {
                self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_some()
                    && self.staging_at_unix_ms.is_some()
                    && self
                        .plan_id
                        .as_deref()
                        .is_some_and(|plan_id| !plan_id.trim().is_empty())
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Failed => {
                self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_some()
                    && self.staging_at_unix_ms.is_none()
                    && self
                        .plan_id
                        .as_deref()
                        .is_some_and(|plan_id| !plan_id.trim().is_empty())
                    && queue_failure.is_some()
                    && self.failed_at_unix_ms.is_some()
            }
        };
        if !valid_shape {
            return Err(JobStoreError::CorruptData(format!(
                "{} row shape does not match its state",
                state.as_str()
            )));
        }
        Ok(StoredJob {
            job_id: self.job_id,
            submitter_device_id: self.submitter_device_id,
            manifest_hash: decode_hash(self.manifest_hash)?,
            state,
            submitted_at_unix_ms: decode_u64(
                &self.submitted_at_unix_ms,
                "submitted_at_unix_ms",
            )?,
            planning_at_unix_ms: self
                .planning_at_unix_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "planning_at_unix_ms"))
                .transpose()?,
            queued_at_unix_ms: self
                .queued_at_unix_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "queued_at_unix_ms"))
                .transpose()?,
            staging_at_unix_ms: self
                .staging_at_unix_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "staging_at_unix_ms"))
                .transpose()?,
            deadline_unix_ms: self
                .deadline_unix_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "deadline_unix_ms"))
                .transpose()?,
            max_queue_duration_ms: self
                .max_queue_duration_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "max_queue_duration_ms"))
                .transpose()?,
            plan_id: self.plan_id,
            queue_failure,
            failed_at_unix_ms: self
                .failed_at_unix_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "failed_at_unix_ms"))
                .transpose()?,
            revision: decode_u64(&self.revision, "revision")?,
        })
    }
}

const SELECT_JOB_SQL: &str = "SELECT job_id, submitter_device_id, manifest_hash, state, \
    submitted_at_unix_ms, planning_at_unix_ms, queued_at_unix_ms, staging_at_unix_ms, deadline_unix_ms, \
    max_queue_duration_ms, plan_id, queue_failure_kind, queue_failure_detail, \
    failed_at_unix_ms, revision FROM coordinator_jobs WHERE job_id = ?1";

const SELECT_QUEUED_SQL: &str = "SELECT job_id, submitter_device_id, manifest_hash, state, \
    submitted_at_unix_ms, planning_at_unix_ms, queued_at_unix_ms, staging_at_unix_ms, deadline_unix_ms, \
    max_queue_duration_ms, plan_id, queue_failure_kind, queue_failure_detail, \
    failed_at_unix_ms, revision FROM coordinator_jobs WHERE state = 'QUEUED'";

fn row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawJobRow> {
    Ok(RawJobRow {
        job_id: row.get(0)?,
        submitter_device_id: row.get(1)?,
        manifest_hash: row.get(2)?,
        state: row.get(3)?,
        submitted_at_unix_ms: row.get(4)?,
        planning_at_unix_ms: row.get(5)?,
        queued_at_unix_ms: row.get(6)?,
        staging_at_unix_ms: row.get(7)?,
        deadline_unix_ms: row.get(8)?,
        max_queue_duration_ms: row.get(9)?,
        plan_id: row.get(10)?,
        queue_failure_kind: row.get(11)?,
        queue_failure_detail: row.get(12)?,
        failed_at_unix_ms: row.get(13)?,
        revision: row.get(14)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission(job_id: &str, key_byte: u8) -> AcceptedJobSubmission {
        AcceptedJobSubmission {
            idempotency_key: [key_byte; 16],
            job_id: job_id.to_string(),
            submitter_device_id: "submitter-1".to_string(),
            manifest_hash: [7; 32],
            deadline_unix_ms: Some(10_000),
            max_queue_duration_ms: Some(1_000),
        }
    }

    fn open_temp() -> (CoordinatorJobStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CoordinatorJobStore::open(dir.path().join("jobs.sqlite3")).expect("open");
        (store, dir)
    }

    fn queued(store: &mut CoordinatorJobStore, submission: &AcceptedJobSubmission) -> StoredJob {
        store.submit_accepted(submission, 100).unwrap();
        store.start_planning(&submission.job_id, 200).unwrap();
        store.enqueue(&submission.job_id, "plan-1", 300).unwrap()
    }

    #[test]
    fn file_store_survives_reopen_and_reports_durable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite3");
        {
            let mut store = CoordinatorJobStore::open(&path).unwrap();
            assert!(store.is_durable());
            queued(&mut store, &submission("job-1", 1));
        }
        let reopened = CoordinatorJobStore::open(&path).unwrap();
        let job = reopened.get("job-1").unwrap().unwrap();
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.revision, 2);
        assert_eq!(job.plan_id.as_deref(), Some("plan-1"));
    }

    #[test]
    fn opens_pre_staging_schema_and_preserves_existing_job() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite3");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE coordinator_jobs (
                        job_id TEXT PRIMARY KEY, submitter_device_id TEXT NOT NULL,
                        manifest_hash BLOB NOT NULL, state TEXT NOT NULL,
                        submitted_at_unix_ms BLOB NOT NULL, planning_at_unix_ms BLOB,
                        queued_at_unix_ms BLOB, deadline_unix_ms BLOB,
                        max_queue_duration_ms BLOB, plan_id TEXT,
                        queue_failure_kind TEXT, queue_failure_detail TEXT,
                        failed_at_unix_ms BLOB, revision BLOB NOT NULL
                     );
                     CREATE TABLE job_submission_idempotency (
                        idempotency_key BLOB PRIMARY KEY,
                        job_id TEXT NOT NULL REFERENCES coordinator_jobs(job_id)
                     );",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO coordinator_jobs(
                        job_id, submitter_device_id, manifest_hash, state,
                        submitted_at_unix_ms, revision
                     ) VALUES ('old-job', 'submitter', ?1, 'SUBMITTED', ?2, ?3)",
                    rusqlite::params![[4u8; 32].as_slice(), encode_u64(100), encode_u64(0)],
                )
                .unwrap();
        }

        let store = CoordinatorJobStore::open(&path).unwrap();
        let job = store.get("old-job").unwrap().unwrap();
        assert_eq!(job.state, JobState::Submitted);
        assert_eq!(job.staging_at_unix_ms, None);
    }

    #[test]
    fn submission_retry_returns_original_timestamp_and_no_duplicate() {
        let (mut store, _dir) = open_temp();
        let request = submission("job-1", 1);
        let first = store.submit_accepted(&request, 100).unwrap();
        let retry = store.submit_accepted(&request, 999).unwrap();
        assert!(first.created);
        assert!(!retry.created);
        assert_eq!(retry.job.submitted_at_unix_ms, 100);
        assert_eq!(retry.job, first.job);
    }

    #[test]
    fn changed_payload_with_same_idempotency_key_is_rejected_without_overwrite() {
        let (mut store, _dir) = open_temp();
        let original = submission("job-1", 1);
        store.submit_accepted(&original, 100).unwrap();
        let mut changed = original.clone();
        changed.manifest_hash = [8; 32];
        let result = store.submit_accepted(&changed, 200);
        assert!(matches!(result, Err(JobStoreError::IdempotencyConflict { .. })));
        assert_eq!(store.get("job-1").unwrap().unwrap().manifest_hash, [7; 32]);
    }

    #[test]
    fn same_job_id_with_another_key_is_rejected() {
        let (mut store, _dir) = open_temp();
        store.submit_accepted(&submission("job-1", 1), 100).unwrap();
        let result = store.submit_accepted(&submission("job-1", 2), 200);
        assert_eq!(
            result,
            Err(JobStoreError::JobIdConflict {
                job_id: "job-1".to_string()
            })
        );
    }

    #[test]
    fn blank_identity_and_zero_queue_duration_fail_closed() {
        let (mut store, _dir) = open_temp();
        let mut request = submission("   ", 1);
        assert_eq!(
            store.submit_accepted(&request, 100),
            Err(JobStoreError::InvalidInput("job_id"))
        );
        request.job_id = "job-1".to_string();
        request.submitter_device_id = "".to_string();
        assert_eq!(
            store.submit_accepted(&request, 100),
            Err(JobStoreError::InvalidInput("submitter_device_id"))
        );
        request.submitter_device_id = "submitter".to_string();
        request.max_queue_duration_ms = Some(0);
        assert_eq!(
            store.submit_accepted(&request, 100),
            Err(JobStoreError::InvalidInput("max_queue_duration_ms"))
        );
        assert!(store.get("job-1").unwrap().is_none());
    }

    #[test]
    fn fixed_width_zero_bytes_are_not_invented_as_an_invalid_encoding() {
        let (mut store, _dir) = open_temp();
        let mut request = submission("job-1", 1);
        request.idempotency_key = [0; 16];
        request.manifest_hash = [0; 32];
        let stored = store.submit_accepted(&request, 100).unwrap().job;
        assert_eq!(stored.manifest_hash, [0; 32]);
    }

    #[test]
    fn only_normative_initial_transition_order_is_allowed() {
        let (mut store, _dir) = open_temp();
        let request = submission("job-1", 1);
        store.submit_accepted(&request, 100).unwrap();
        let skipped = store.enqueue("job-1", "plan-1", 200);
        assert!(matches!(
            skipped,
            Err(JobStoreError::InvalidTransition {
                from: JobState::Submitted,
                to: JobState::Queued
            })
        ));
        assert_eq!(store.get("job-1").unwrap().unwrap().revision, 0);
    }

    #[test]
    fn planning_and_enqueue_retries_are_idempotent_but_plan_change_is_not() {
        let (mut store, _dir) = open_temp();
        let request = submission("job-1", 1);
        store.submit_accepted(&request, 100).unwrap();
        let planning = store.start_planning("job-1", 200).unwrap();
        assert_eq!(store.start_planning("job-1", 999).unwrap(), planning);
        let queued = store.enqueue("job-1", "plan-1", 300).unwrap();
        assert_eq!(store.enqueue("job-1", "plan-1", 999).unwrap(), queued);
        assert!(matches!(
            store.enqueue("job-1", "plan-2", 999),
            Err(JobStoreError::PlanConflict { .. })
        ));
        assert_eq!(store.get("job-1").unwrap().unwrap(), queued);
    }

    #[test]
    fn clock_rollback_does_not_advance_state() {
        let (mut store, _dir) = open_temp();
        let request = submission("job-1", 1);
        store.submit_accepted(&request, 100).unwrap();
        assert!(matches!(
            store.start_planning("job-1", 99),
            Err(JobStoreError::ClockRollback { .. })
        ));
        assert_eq!(store.get("job-1").unwrap().unwrap().state, JobState::Submitted);
    }

    #[test]
    fn deadline_boundary_is_strict_and_one_millisecond_after_fails() {
        let (mut store, _dir) = open_temp();
        queued(&mut store, &submission("job-1", 1));
        assert!(matches!(
            store.fail_queued("job-1", QueueFailure::DeadlinePassed, 10_000),
            Err(JobStoreError::GuardNotMet(_))
        ));
        assert_eq!(store.get("job-1").unwrap().unwrap().state, JobState::Queued);
        let failed = store
            .fail_queued("job-1", QueueFailure::DeadlinePassed, 10_001)
            .unwrap();
        assert_eq!(failed.queue_failure, Some(QueueFailure::DeadlinePassed));
    }

    #[test]
    fn queue_timeout_boundary_is_strict_and_one_millisecond_after_fails() {
        let (mut store, _dir) = open_temp();
        queued(&mut store, &submission("job-1", 1));
        assert!(matches!(
            store.fail_queued("job-1", QueueFailure::QueueTimeout, 1_300),
            Err(JobStoreError::GuardNotMet(_))
        ));
        let failed = store
            .fail_queued("job-1", QueueFailure::QueueTimeout, 1_301)
            .unwrap();
        assert_eq!(failed.queue_failure, Some(QueueFailure::QueueTimeout));
    }

    #[test]
    fn no_independent_timeout_never_turns_into_zero_duration_timeout() {
        let (mut store, _dir) = open_temp();
        let mut request = submission("job-1", 1);
        request.max_queue_duration_ms = None;
        queued(&mut store, &request);
        assert!(matches!(
            store.fail_queued("job-1", QueueFailure::QueueTimeout, u64::MAX),
            Err(JobStoreError::GuardNotMet(_))
        ));
        assert_eq!(store.get("job-1").unwrap().unwrap().state, JobState::Queued);
    }

    #[test]
    fn permanent_infeasibility_requires_reason_and_stays_distinct() {
        let (mut store, _dir) = open_temp();
        queued(&mut store, &submission("job-1", 1));
        assert_eq!(
            store.fail_queued(
                "job-1",
                QueueFailure::PermanentlyInfeasible {
                    reason: "  ".to_string()
                },
                500
            ),
            Err(JobStoreError::InvalidInput("permanent infeasibility reason"))
        );
        let reason = "all enrolled GPUs are below required VRAM".to_string();
        let failed = store
            .fail_queued(
                "job-1",
                QueueFailure::PermanentlyInfeasible {
                    reason: reason.clone(),
                },
                500,
            )
            .unwrap();
        assert_eq!(
            failed.queue_failure,
            Some(QueueFailure::PermanentlyInfeasible { reason })
        );
    }

    #[test]
    fn terminal_retry_returns_original_and_different_reason_cannot_overwrite() {
        let (mut store, _dir) = open_temp();
        queued(&mut store, &submission("job-1", 1));
        let first = store
            .fail_queued("job-1", QueueFailure::QueueTimeout, 1_301)
            .unwrap();
        let retry = store
            .fail_queued("job-1", QueueFailure::QueueTimeout, 9_999)
            .unwrap();
        assert_eq!(retry, first);
        assert!(matches!(
            store.fail_queued("job-1", QueueFailure::DeadlinePassed, 10_001),
            Err(JobStoreError::InvalidTransition { .. })
        ));
        assert_eq!(store.get("job-1").unwrap().unwrap(), first);
    }

    #[test]
    fn queue_is_deterministic_fifo_with_job_id_tie_breaker() {
        let (mut store, _dir) = open_temp();
        for (job_id, key, queued_at) in [("job-c", 3, 500), ("job-b", 2, 400), ("job-a", 1, 400)] {
            let request = submission(job_id, key);
            store.submit_accepted(&request, 100).unwrap();
            store.start_planning(job_id, 200).unwrap();
            store.enqueue(job_id, "plan-1", queued_at).unwrap();
        }
        let ids: Vec<_> = store
            .list_queued()
            .unwrap()
            .into_iter()
            .map(|job| job.job_id)
            .collect();
        assert_eq!(ids, ["job-a", "job-b", "job-c"]);
    }

    #[test]
    fn corrupted_state_and_truncated_integer_fail_closed() {
        let (mut store, _dir) = open_temp();
        store.submit_accepted(&submission("job-1", 1), 100).unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_jobs SET state = 'QUEUED' WHERE job_id = 'job-1'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.get("job-1"),
            Err(JobStoreError::CorruptData(_))
        ));
        store
            .connection
            .execute("UPDATE coordinator_jobs SET state = 'UNKNOWN' WHERE job_id = 'job-1'", [])
            .unwrap();
        assert!(matches!(store.get("job-1"), Err(JobStoreError::CorruptData(_))));
        store
            .connection
            .execute(
                "UPDATE coordinator_jobs SET state = 'SUBMITTED', revision = x'01' WHERE job_id = 'job-1'",
                [],
            )
            .unwrap();
        assert!(matches!(store.get("job-1"), Err(JobStoreError::CorruptData(_))));

        let second = submission("job-2", 2);
        queued(&mut store, &second);
        store
            .connection
            .execute("UPDATE coordinator_jobs SET plan_id = ' ' WHERE job_id = 'job-2'", [])
            .unwrap();
        assert!(matches!(store.get("job-2"), Err(JobStoreError::CorruptData(_))));
    }

    #[test]
    fn u64_max_timestamp_round_trips_without_sqlite_integer_truncation() {
        let (mut store, _dir) = open_temp();
        let request = submission("job-1", 1);
        let result = store.submit_accepted(&request, u64::MAX).unwrap();
        assert_eq!(result.job.submitted_at_unix_ms, u64::MAX);
        assert_eq!(store.get("job-1").unwrap().unwrap().submitted_at_unix_ms, u64::MAX);
    }
}
