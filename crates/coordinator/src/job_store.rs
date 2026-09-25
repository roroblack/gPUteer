//! Durable Job/Queue control truth for the scheduler roadmap's slice 2a.
//!
//! The standalone API owns accepted submission persistence,
//! `SUBMITTED -> PLANNING -> QUEUED`, queue ordering, queue terminal reasons,
//! and retry idempotency. `QUEUED -> STAGING` is intentionally absent here:
//! [`crate::staging_store`] owns that transition together with Attempt creation,
//! fence allocation, and Lease insertion in one transaction.

use std::path::Path;

use gputeer_protocol::{
    canonical::blake3_256,
    pb,
    signing::{signing_input, Verified},
};
use prost::Message;
use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// ★ 2026-09-23 — 저장소가 쓰던 별도 enum(다섯 값)을 **규범 정본과 하나로 합쳤다**
///   (`gputeer_protocol::job_state`, `state-machines.md` §2). Attempt 와 같은 이유다 —
///   두 벌이면 갈라진다.
pub use gputeer_protocol::job_state::JobState;

/// DB 문자열 — 표의 이름 그대로다.
trait JobStateDb {
    fn as_str(self) -> &'static str;
}

impl JobStateDb for JobState {
    fn as_str(self) -> &'static str {
        self.table_name()
    }
}

/// 이 저장소가 **쓰는** 상태만 읽는다. 표에는 있지만 이 저장소가 최종 상태로 쓰지 않는 상태
/// (INTERRUPTED · REPLANNING · RECONCILING · CANCELLED · ARCHIVED)가 DB 에 있으면 손상이다 —
/// 그 상태들은 판정 순간에만 거친다(코덱스 72 결정 C, Attempt 와 같다).
fn parse_job_state(value: &str) -> Result<JobState, JobStoreError> {
    match value {
        "SUBMITTED" => Ok(JobState::Submitted),
        "PLANNING" => Ok(JobState::Planning),
        "QUEUED" => Ok(JobState::Queued),
        "STAGING" => Ok(JobState::Staging),
        "RUNNING" => Ok(JobState::Running),
        "PAUSED" => Ok(JobState::Paused),
        "COMPLETED" => Ok(JobState::Completed),
        "FAILED" => Ok(JobState::Failed),
        other => Err(JobStoreError::CorruptData(format!(
            "unknown job state in durable store: {other}"
        ))),
    }
}

/// 실행에 들어간 뒤 Job 이 끝난 이유 — 표의 trigger 이름 그대로 저장한다.
///
/// ★ 큐 단계의 실패(`QueueFailure`)와 **따로 둔다.** 둘은 guard 도 effect 도 다르다 —
///   하나로 접으면 "배치조차 못 했다" 와 "돌다가 죽었다" 가 같은 칸에 섞인다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTerminal {
    /// `RUNNING -> COMPLETED` (ATTEMPT_COMPLETED).
    AttemptCompleted,
    /// `STAGING -> FAILED` (STAGING_FAILED) — 실행이 시작되지 못했다.
    StagingFailed,
    /// `RUNNING -> FAILED` (UNRECOVERABLE_ERROR) — 돌다가 실패했고 재시도 정책(0회)이 소진됐다.
    UnrecoverableError,
    /// `INTERRUPTED -> FAILED` (NO_COMMITTED_CHECKPOINT) — 노드를 잃었고 이어갈 체크포인트가 없다.
    NoCommittedCheckpoint,
}

impl RunTerminal {
    pub fn trigger(self) -> &'static str {
        match self {
            Self::AttemptCompleted => "ATTEMPT_COMPLETED",
            Self::StagingFailed => "STAGING_FAILED",
            Self::UnrecoverableError => "UNRECOVERABLE_ERROR",
            Self::NoCommittedCheckpoint => "NO_COMMITTED_CHECKPOINT",
        }
    }

    fn parse(value: &str) -> Result<Self, JobStoreError> {
        match value {
            "ATTEMPT_COMPLETED" => Ok(Self::AttemptCompleted),
            "STAGING_FAILED" => Ok(Self::StagingFailed),
            "UNRECOVERABLE_ERROR" => Ok(Self::UnrecoverableError),
            "NO_COMMITTED_CHECKPOINT" => Ok(Self::NoCommittedCheckpoint),
            other => Err(JobStoreError::CorruptData(format!(
                "unknown run terminal trigger in durable store: {other}"
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
                let reason = detail
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
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
    /// ACK 를 받은 시각(Coordinator 시계) — `STAGING -> RUNNING` 을 관측한 때.
    /// ACK 를 기록하지 않은 경로에서는 비어 있다. 지어내지 않는다.
    pub running_at_unix_ms: Option<u64>,
    /// 실행에 들어간 뒤 끝난 이유. 큐 실패는 `queue_failure` 에 따로 있다.
    pub run_terminal: Option<RunTerminal>,
    /// 끝난 시각 — **워커가 보고한 값**(`AttemptReport.finished_at_unix_ms`, `WORKER_REPORTED`).
    /// Coordinator 시계가 아니므로 순서 검사에 쓰지 않는다.
    pub worker_reported_finished_at_unix_ms: Option<u64>,
    /// ★ 2026-09-23 (신뢰망 남은 일 G) — 장애 이어받기로 큐에 **몇 번** 되돌아왔나. 새 시도의 식별자를 가른다.
    pub requeue_count: u64,
    /// 이어서 시작할 체크포인트 — 생산 노드가 서명한 `CheckpointManifest` 원본 바이트.
    /// 장애 판정 때 서명 · 파일 해시를 검증한 것만 여기 들어온다(`failover`). 다음 Grant 가 그대로 싣는다(v3).
    pub resume_checkpoint: Option<Vec<u8>>,
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

/// A durable protobuf body that was accepted through [`Verified`] at submission
/// time. This type is deliberately not `Verified<JobManifest>`: after restart,
/// callers must verify `manifest` again against the then-authoritative key
/// directory before using any field for scheduling or grant construction.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredManifestBinding {
    pub manifest: pb::JobManifest,
    pub manifest_hash: [u8; 32],
    pub signer_id_at_submission: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManifestBoundSubmitResult {
    pub job: StoredJob,
    pub binding: StoredManifestBinding,
    /// `false` means the idempotency key replayed the original durable result.
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestCorruption {
    EmptyBody,
    UndecodableBody,
    HashMismatch,
    JobIdMismatch,
    SubmitterDeviceIdMismatch,
    SignerIdMismatch,
}

#[derive(Debug, PartialEq, Eq)]
pub enum JobStoreError {
    InvalidInput(&'static str),
    NotFound,
    IdempotencyConflict {
        stored_job_id: String,
        requested_job_id: String,
    },
    JobIdConflict {
        job_id: String,
    },
    InvalidTransition {
        from: JobState,
        to: JobState,
    },
    PlanConflict {
        stored_plan_id: String,
        requested_plan_id: String,
    },
    GuardNotMet(&'static str),
    ManifestIdentityMismatch(&'static str),
    ManifestHashMismatch,
    LegacyManifestMissing {
        job_id: String,
    },
    ManifestCorrupt {
        job_id: String,
        kind: ManifestCorruption,
    },
    ClockRollback {
        earlier_unix_ms: u64,
        later_unix_ms: u64,
    },
    CorruptData(String),
    Io(String),
    LockTimeout,
    InjectedFailure(&'static str),
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
            Self::ManifestIdentityMismatch(field) => {
                write!(f, "verified Manifest identity does not match accepted {field}")
            }
            Self::ManifestHashMismatch => {
                write!(f, "supplied manifest_hash does not match the verified Manifest")
            }
            Self::LegacyManifestMissing { job_id } => write!(
                f,
                "job {job_id} predates durable Manifest bodies and cannot be consumed"
            ),
            Self::ManifestCorrupt { job_id, kind } => {
                write!(f, "durable Manifest for job {job_id} is corrupt: {kind:?}")
            }
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
            Self::InjectedFailure(point) => write!(f, "injected job store failure: {point}"),
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
    let value: [u8; 8] = bytes
        .try_into()
        .map_err(|_| JobStoreError::CorruptData(format!("{field} must contain exactly 8 bytes")))?;
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

            CREATE TABLE IF NOT EXISTS coordinator_job_manifests (
                job_id TEXT PRIMARY KEY REFERENCES coordinator_jobs(job_id),
                verified_signer_id TEXT NOT NULL,
                manifest_body BLOB NOT NULL
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
    // ★ 2026-09-23 — Job 이 시도의 끝을 따라가며 생긴 칸. 옛 DB 는 열 때 보탠다(값은 비어 있다 —
    //   그 행들은 이 칸을 쓰는 상태에 있지 않았다).
    for (column, ty) in [
        ("running_at_unix_ms", "BLOB"),
        ("run_terminal", "TEXT"),
        ("worker_reported_finished_at_unix_ms", "BLOB"),
        ("requeue_count", "BLOB"),
        ("resume_checkpoint", "BLOB"),
    ] {
        let exists = {
            let mut statement = transaction
                .prepare("PRAGMA table_info(coordinator_jobs)")
                .map_err(map_sql_error)?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(map_sql_error)?;
            let mut found = false;
            for row in rows {
                if row.map_err(map_sql_error)? == column {
                    found = true;
                }
            }
            found
        };
        if !exists {
            transaction
                .execute(
                    &format!("ALTER TABLE coordinator_jobs ADD COLUMN {column} {ty}"),
                    [],
                )
                .map_err(map_sql_error)?;
        }
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

    /// Loads a durable Manifest binding without claiming that its signature is
    /// still valid. Existing hash-only Jobs fail closed with
    /// [`JobStoreError::LegacyManifestMissing`].
    pub fn get_manifest_binding(
        &self,
        job_id: &str,
    ) -> Result<Option<StoredManifestBinding>, JobStoreError> {
        let Some(job) = fetch_job(&self.connection, job_id)? else {
            return Ok(None);
        };
        fetch_manifest_binding(&self.connection, &job).map(Some)
    }

    /// Returns the durable queue in deterministic FIFO order. `job_id` is the
    /// tie-breaker when two jobs have the same queue timestamp.
    /// ★ 2026-09-23 (신뢰망 남은 일 H) — 배치할 수 있는 Job: QUEUED 와 **선점으로 멈춘** PAUSED. `(queued_at, job_id)` 순.
    ///   선점된 작업은 원래 큐 진입 시각을 지킨다 — 소유자가 GPU 를 되찾았다고 뒤로 밀지 않는다.
    pub fn list_schedulable(&self) -> Result<Vec<StoredJob>, JobStoreError> {
        let sql = SELECT_QUEUED_SQL.replace(
            "WHERE state = 'QUEUED'",
            "WHERE state IN ('QUEUED', 'PAUSED')",
        );
        let mut statement = self.connection.prepare(&sql).map_err(map_sql_error)?;
        let mut jobs = statement
            .query_map([], row_to_raw)
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?
            .into_iter()
            .map(RawJobRow::into_stored)
            .collect::<Result<Vec<_>, _>>()?;
        jobs.sort_by(|a, b| {
            (a.queued_at_unix_ms, &a.job_id).cmp(&(b.queued_at_unix_ms, &b.job_id))
        });
        Ok(jobs)
    }

    pub fn list_queued(&self) -> Result<Vec<StoredJob>, JobStoreError> {
        let mut statement = self
            .connection
            .prepare(SELECT_QUEUED_SQL)
            .map_err(map_sql_error)?;
        let rows = statement.query_map([], row_to_raw).map_err(map_sql_error)?;
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
            running_at_unix_ms: None,
            run_terminal: None,
            worker_reported_finished_at_unix_ms: None,
            requeue_count: 0,
            resume_checkpoint: None,
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

    /// Atomically persists an accepted Job, its idempotency binding, and the
    /// complete signed Manifest. The type gate ensures that this storage path
    /// cannot receive an unverified Manifest.
    pub fn submit_verified_manifest(
        &mut self,
        submission: &AcceptedJobSubmission,
        manifest: &Verified<pb::JobManifest>,
        submitted_at_unix_ms: u64,
    ) -> Result<ManifestBoundSubmitResult, JobStoreError> {
        self.submit_verified_manifest_inner(submission, manifest, submitted_at_unix_ms, None)
    }

    fn submit_verified_manifest_inner(
        &mut self,
        submission: &AcceptedJobSubmission,
        verified: &Verified<pb::JobManifest>,
        submitted_at_unix_ms: u64,
        fault: Option<TestFault>,
    ) -> Result<ManifestBoundSubmitResult, JobStoreError> {
        validate_submission(submission)?;

        // This is the first point at which Manifest fields are observed: the
        // only Manifest parameter is already wrapped in Verified.
        let manifest = verified.get();
        validate_manifest_identity(submission, manifest, verified.signer_id())?;
        let derived_hash = derive_manifest_hash(manifest);
        if submission.manifest_hash != derived_hash {
            return Err(JobStoreError::ManifestHashMismatch);
        }
        let manifest_body = manifest.encode_to_vec();

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
            let binding = fetch_manifest_binding(&transaction, &stored)?;
            if binding.manifest != *manifest
                || binding.signer_id_at_submission != verified.signer_id()
            {
                return Err(JobStoreError::IdempotencyConflict {
                    stored_job_id: stored.job_id.clone(),
                    requested_job_id: submission.job_id.clone(),
                });
            }
            transaction.commit().map_err(map_sql_error)?;
            return Ok(ManifestBoundSubmitResult {
                job: stored,
                binding,
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
            manifest_hash: derived_hash,
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
            running_at_unix_ms: None,
            run_terminal: None,
            worker_reported_finished_at_unix_ms: None,
            requeue_count: 0,
            resume_checkpoint: None,
            revision: 0,
        };

        insert_job(&transaction, &stored)?;
        transaction
            .execute(
                "INSERT INTO coordinator_job_manifests(
                    job_id, verified_signer_id, manifest_body
                 ) VALUES (?1, ?2, ?3)",
                rusqlite::params![stored.job_id, verified.signer_id(), manifest_body],
            )
            .map_err(map_sql_error)?;
        fail_at(fault, TestFault::AfterManifestInsert)?;
        transaction
            .execute(
                "INSERT INTO job_submission_idempotency(idempotency_key, job_id) VALUES (?1, ?2)",
                rusqlite::params![submission.idempotency_key.as_slice(), stored.job_id],
            )
            .map_err(map_sql_error)?;
        transaction.commit().map_err(map_sql_error)?;

        Ok(ManifestBoundSubmitResult {
            job: stored,
            binding: StoredManifestBinding {
                manifest: manifest.clone(),
                manifest_hash: derived_hash,
                signer_id_at_submission: verified.signer_id().to_string(),
            },
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
            job.revision = job
                .revision
                .checked_add(1)
                .ok_or_else(|| JobStoreError::CorruptData("job revision overflow".to_string()))?;
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
            job.revision = job
                .revision
                .checked_add(1)
                .ok_or_else(|| JobStoreError::CorruptData("job revision overflow".to_string()))?;
            update_job(transaction, &job)?;
            Ok(job)
        })
    }

    /// ★ 2026-09-25 (결함 402) — `SUBMITTED -> PLANNING -> QUEUED` 를 **한 트랜잭션**으로 한다. 두 시각 칸과 revision(+2)을 채우고, 중간에 실패하면
    /// 아무것도 남기지 않는다.
    ///
    /// ★ 결함 425 (재검수 107) — 상태표를 **전부** 지키는 것은 아니다. PLANNING 은 따로 영속되지 않는다 — `plan-job` 은 계획 계산을 쓰기 **전에**
    ///   끝내므로 PLANNING 은 이 호출 안에서만 있다. PLAN_READY 의 효과인 PlacementRationale 기록은 없다(옛 `enqueue` 도 없었다 — 저장할 칸이 없다).
    ///
    /// 전에는 `plan-job` 이 [`start_planning`](Self::start_planning) · [`enqueue`](Self::enqueue) 를 따로 커밋해, 둘째가 실패하면 Job 이 PLANNING 에
    /// 남았다. 그 사이 제출 Manifest 가 만료되면 `plan-job` 이 서명 검증에서 먼저 거부해 다시 돌려도 풀 수 없었다.
    ///
    /// PLANNING 에 있으면 QUEUED 로만 옮긴다(옛 실행이 남긴 것). QUEUED 면 같은 `plan_id` 일 때 그대로 돌려주고 다르면 `PlanConflict` 다.
    pub fn plan_and_enqueue(
        &mut self,
        job_id: &str,
        plan_id: &str,
        at_unix_ms: u64,
    ) -> Result<StoredJob, JobStoreError> {
        self.plan_and_enqueue_inner(job_id, plan_id, at_unix_ms, false)
    }

    fn plan_and_enqueue_inner(
        &mut self,
        job_id: &str,
        plan_id: &str,
        at_unix_ms: u64,
        fail_after_planning: bool,
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
            if job.state == JobState::Submitted {
                ensure_not_before(at_unix_ms, job.submitted_at_unix_ms)?;
                job.state = JobState::Planning;
                job.planning_at_unix_ms = Some(at_unix_ms);
                job.revision = job.revision.checked_add(1).ok_or_else(|| {
                    JobStoreError::CorruptData("job revision overflow".to_string())
                })?;
                update_job(transaction, &job)?;
                if fail_after_planning {
                    return Err(JobStoreError::CorruptData(
                        "시험 주입 — PLANNING 을 적은 뒤 실패".to_string(),
                    ));
                }
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
            job.revision = job
                .revision
                .checked_add(1)
                .ok_or_else(|| JobStoreError::CorruptData("job revision overflow".to_string()))?;
            update_job(transaction, &job)?;
            Ok(job)
        })
    }

    /// 시험 전용 — PLANNING 을 적은 **뒤** 같은 트랜잭션 안에서 실패시킨다(결함 402 의 원자성 시험).
    #[cfg(test)]
    pub(crate) fn plan_and_enqueue_failing_after_planning(
        &mut self,
        job_id: &str,
        plan_id: &str,
        at_unix_ms: u64,
    ) -> Result<StoredJob, JobStoreError> {
        self.plan_and_enqueue_inner(job_id, plan_id, at_unix_ms, true)
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
            return Err(JobStoreError::InvalidInput(
                "permanent infeasibility reason",
            ));
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
                        return Err(JobStoreError::GuardNotMet(
                            "now must be greater than deadline",
                        ));
                    }
                }
                QueueFailure::QueueTimeout => {
                    let limit = job.max_queue_duration_ms.ok_or(JobStoreError::GuardNotMet(
                        "Job has no independent queue timeout",
                    ))?;
                    let elapsed =
                        at_unix_ms
                            .checked_sub(queued_at)
                            .ok_or(JobStoreError::ClockRollback {
                                earlier_unix_ms: at_unix_ms,
                                later_unix_ms: queued_at,
                            })?;
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
            job.revision = job
                .revision
                .checked_add(1)
                .ok_or_else(|| JobStoreError::CorruptData("job revision overflow".to_string()))?;
            update_job(transaction, &job)?;
            Ok(job)
        })
    }

    fn transition<F>(&mut self, job_id: &str, apply: F) -> Result<StoredJob, JobStoreError>
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

fn validate_manifest_identity(
    submission: &AcceptedJobSubmission,
    manifest: &pb::JobManifest,
    verified_signer_id: &str,
) -> Result<(), JobStoreError> {
    if manifest.job_id != submission.job_id {
        return Err(JobStoreError::ManifestIdentityMismatch("job_id"));
    }
    if manifest.submitter_device_id != submission.submitter_device_id {
        return Err(JobStoreError::ManifestIdentityMismatch(
            "submitter_device_id",
        ));
    }
    if verified_signer_id != submission.submitter_device_id {
        return Err(JobStoreError::ManifestIdentityMismatch(
            "verified signer_id",
        ));
    }
    Ok(())
}

fn derive_manifest_hash(manifest: &pb::JobManifest) -> [u8; 32] {
    blake3_256(&signing_input(manifest))
}

fn insert_job(connection: &Connection, stored: &StoredJob) -> Result<(), JobStoreError> {
    connection
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
    Ok(())
}

fn fetch_manifest_binding(
    connection: &Connection,
    job: &StoredJob,
) -> Result<StoredManifestBinding, JobStoreError> {
    let row = connection
        .query_row(
            "SELECT verified_signer_id, manifest_body
             FROM coordinator_job_manifests WHERE job_id = ?1",
            rusqlite::params![job.job_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(map_sql_error)?
        .ok_or_else(|| JobStoreError::LegacyManifestMissing {
            job_id: job.job_id.clone(),
        })?;
    let (signer_id_at_submission, body) = row;
    if body.is_empty() {
        return Err(manifest_corrupt(job, ManifestCorruption::EmptyBody));
    }
    let manifest = pb::JobManifest::decode(body.as_slice())
        .map_err(|_| manifest_corrupt(job, ManifestCorruption::UndecodableBody))?;
    if manifest.job_id != job.job_id {
        return Err(manifest_corrupt(job, ManifestCorruption::JobIdMismatch));
    }
    if manifest.submitter_device_id != job.submitter_device_id {
        return Err(manifest_corrupt(
            job,
            ManifestCorruption::SubmitterDeviceIdMismatch,
        ));
    }
    if signer_id_at_submission != job.submitter_device_id {
        return Err(manifest_corrupt(job, ManifestCorruption::SignerIdMismatch));
    }
    let derived_hash = derive_manifest_hash(&manifest);
    if derived_hash != job.manifest_hash {
        return Err(manifest_corrupt(job, ManifestCorruption::HashMismatch));
    }
    Ok(StoredManifestBinding {
        manifest,
        manifest_hash: derived_hash,
        signer_id_at_submission,
    })
}

fn manifest_corrupt(job: &StoredJob, kind: ManifestCorruption) -> JobStoreError {
    JobStoreError::ManifestCorrupt {
        job_id: job.job_id.clone(),
        kind,
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFault {
    AfterManifestInsert,
}

#[cfg(test)]
fn fail_at(fault: Option<TestFault>, point: TestFault) -> Result<(), JobStoreError> {
    if fault == Some(point) {
        Err(JobStoreError::InjectedFailure("after Manifest insert"))
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
fn fail_at(_fault: Option<TestFault>, _point: TestFault) -> Result<(), JobStoreError> {
    Ok(())
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

/// 실행 단계(STAGING · RUNNING)에 있는 Job 전부 — 장애 판정이 훑는 대상이다. job_id 순.
pub(crate) fn list_in_run_states(connection: &Connection) -> Result<Vec<StoredJob>, JobStoreError> {
    let sql = SELECT_QUEUED_SQL.replace(
        "WHERE state = 'QUEUED'",
        "WHERE state IN ('STAGING', 'RUNNING') ORDER BY job_id",
    );
    let mut statement = connection.prepare(&sql).map_err(map_sql_error)?;
    let rows = statement
        .query_map([], row_to_raw)
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)?;
    rows.into_iter().map(RawJobRow::into_stored).collect()
}

/// 장애 이어받기의 규범 경로로 Job 을 되돌리거나 끝낸다 — **같은 트랜잭션 안에서** 부른다.
///
/// ```text
/// STAGING  -> QUEUED                                       STAGING_NODE_LOST   (ACK 전에 잃었다 — 실행 전이다)
/// RUNNING  -> INTERRUPTED -> REPLANNING -> QUEUED          NODE_LOST · FAILOVER_STARTED · REPLAN_READY
///                                                          (guard: 마지막 COMMITTED 체크포인트가 있다 — `resume` 이 Some)
/// RUNNING  -> INTERRUPTED -> FAILED                         NODE_LOST · NO_COMMITTED_CHECKPOINT
/// ```
///
/// ★ `REPLAN_READY` 의 guard "새 후보 확보" 는 **계획이 여전히 유효하다**(plan_id 를 지킨다)로 읽는다 —
///   실제 노드는 스케줄러가 큐에서 다시 고른다. 후보가 끝내 없으면 큐의 기존 규칙(deadline · queue timeout ·
///   PERMANENTLY_INFEASIBLE)이 끝낸다.
/// ★ 큐 순서는 **원래 큐 진입 시각**을 지킨다 — 이어받은 작업을 뒤로 보내지 않는다.
pub(crate) fn requeue_after_node_lost(
    connection: &Connection,
    job: &StoredJob,
    resume: Option<Vec<u8>>,
    worker_clock_hint_unix_ms: u64,
) -> Result<StoredJob, JobStoreError> {
    let mut next = job.clone();
    let path: &[JobState] = match (job.state, resume.is_some()) {
        (JobState::Staging, _) => &[JobState::Queued],
        (JobState::Running, true) => &[
            JobState::Interrupted,
            JobState::Replanning,
            JobState::Queued,
        ],
        (JobState::Running, false) => &[JobState::Interrupted, JobState::Failed],
        (from, _) => {
            return Err(JobStoreError::InvalidTransition {
                from,
                to: JobState::Queued,
            })
        }
    };
    next.state = gputeer_protocol::job_state::walk(job.state, path).map_err(|rejected| {
        JobStoreError::InvalidTransition {
            from: rejected.from,
            to: rejected.to,
        }
    })?;
    if next.state == JobState::Queued {
        next.staging_at_unix_ms = None;
        next.running_at_unix_ms = None;
        next.requeue_count = job
            .requeue_count
            .checked_add(1)
            .ok_or(JobStoreError::CorruptData(
                "requeue_count overflow".to_string(),
            ))?;
        // 이번에 고른 지점이 없으면(STAGING 에서 잃음) 전에 고른 지점을 그대로 둔다 — 그 사이 진척이 없었다.
        if resume.is_some() {
            next.resume_checkpoint = resume;
        }
    } else {
        next.run_terminal = Some(RunTerminal::NoCommittedCheckpoint);
        next.worker_reported_finished_at_unix_ms = Some(worker_clock_hint_unix_ms);
    }
    next.revision = job
        .revision
        .checked_add(1)
        .ok_or(JobStoreError::CorruptData("revision overflow".to_string()))?;
    update_job(connection, &next)?;
    Ok(next)
}

/// ★ 2026-09-23 (신뢰망 남은 일 H) — 노드 소유자가 GPU 를 되찾았다: `RUNNING -> PAUSED`(OWNER_PREEMPT).
///
/// 규범 effect "checkpoint 후 정지" — 이어갈 지점은 공유 저장소의 검증된 마지막 체크포인트다(없으면 전에 고른 지점을
/// 그대로 둔다 — 처음부터 다시 도는 것은 PURE 작업에서만 안전하다). 새 시도의 식별자를 가르려고 되돌아온 횟수를 올린다.
/// 다른 노드에 다시 배치되면 `PAUSED -> RUNNING`(RESUMED, "새 lease 발급")이다 — 스테이징이 한다.
pub(crate) fn pause_for_owner_preempt(
    connection: &Connection,
    job: &StoredJob,
    resume: Option<Vec<u8>>,
) -> Result<StoredJob, JobStoreError> {
    let mut next = job.clone();
    next.state = gputeer_protocol::job_state::transition(job.state, JobState::Paused).map_err(
        |rejected| JobStoreError::InvalidTransition {
            from: rejected.from,
            to: rejected.to,
        },
    )?;
    next.requeue_count = job
        .requeue_count
        .checked_add(1)
        .ok_or(JobStoreError::CorruptData(
            "requeue_count overflow".to_string(),
        ))?;
    if resume.is_some() {
        next.resume_checkpoint = resume;
    }
    next.revision = job
        .revision
        .checked_add(1)
        .ok_or(JobStoreError::CorruptData("revision overflow".to_string()))?;
    update_job(connection, &next)?;
    Ok(next)
}

/// ACK 가 **이 Job 의 현재 시도**의 것인가 — Job 은 옮기지 않는다(결함 218 · 2026-09-25).
///
/// ★ Job 의 `STAGING -> RUNNING` 은 이제 ACK 가 아니라 **첫 진행 신호**(실행 중 첫 갱신 — `record_staging_complete`)가 옮긴다.
///   ACK 뒤 수신 확인이 유실돼 한 번도 안 돈 시도가 RUNNING 으로 남아 이어받기에서 FAILED 가 되던 것(218)을 막는다 — 그 Job 은
///   STAGING 으로 남아 `STAGING_NODE_LOST` 로 큐에 돌아간다. 시계가 예약 시각보다 뒤면 `ClockRollback`.
pub(crate) fn ack_is_for_current_attempt(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    now_unix_ms: u64,
) -> Result<bool, JobStoreError> {
    let Some(job) = fetch_job(connection, job_id)? else {
        return Err(JobStoreError::NotFound);
    };
    if !matches!(job.state, JobState::Staging | JobState::Running) {
        return Ok(false);
    }
    let latest: Option<String> = connection
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_sql_error)?;
    if latest.as_deref() != Some(attempt_id) {
        return Ok(false);
    }
    if job.state == JobState::Staging {
        if let Some(staging_at) = job.staging_at_unix_ms {
            ensure_not_before(now_unix_ms, staging_at)?;
        }
    }
    Ok(true)
}

/// 첫 진행 신호에 Job 을 `STAGING -> RUNNING`(STAGING_COMPLETE)으로 옮긴다.
///
/// ★ 2026-09-25 (결함 218) — 전에는 ACK 가 불렀다. 이제 실행 중 **첫 갱신**(`record_process_started`)이 부른다.
///
/// ★ 2026-09-23 (신뢰망 남은 일 D). Agent 는 Grant·Lease 를 검증하고 workspace 를 만든 **뒤에** ACK 를 보낸다
///   (결함 ⑱ 설계 A — 실행 전 ACK). 그래서 ACK 는 "환경 준비 완료" 의 관측이다. 그 시각은 Coordinator 시계다.
///
/// `Ok(false)` — 건드리지 않았다: 이 시도가 Job 의 가장 최근 시도가 아니거나 Job 이 STAGING 이 아니다.
pub(crate) fn record_staging_complete(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    now_unix_ms: u64,
) -> Result<bool, JobStoreError> {
    let Some(mut job) = fetch_job(connection, job_id)? else {
        return Err(JobStoreError::NotFound);
    };
    // ★ 2026-09-23 (신뢰망 남은 일 H) — 선점 뒤 다시 배치된 Job 은 새 Lease 를 받는 순간 이미 RUNNING 이다(RESUMED).
    //   그 시도의 ACK 는 Job 을 더 옮기지 않고, 시도만 STARTING 으로 적는다(아래 `latest` 대조는 똑같이 한다).
    if !matches!(job.state, JobState::Staging | JobState::Running) {
        return Ok(false);
    }
    let latest: Option<String> = connection
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_sql_error)?;
    if latest.as_deref() != Some(attempt_id) {
        return Ok(false);
    }
    if job.state == JobState::Running {
        return Ok(true);
    }
    if let Some(staging_at) = job.staging_at_unix_ms {
        ensure_not_before(now_unix_ms, staging_at)?;
    }
    job.state = gputeer_protocol::job_state::transition(job.state, JobState::Running).map_err(
        |rejected| JobStoreError::InvalidTransition {
            from: rejected.from,
            to: rejected.to,
        },
    )?;
    job.running_at_unix_ms = Some(now_unix_ms);
    job.revision = job
        .revision
        .checked_add(1)
        .ok_or(JobStoreError::CorruptData("revision overflow".to_string()))?;
    update_job(connection, &job)?;
    Ok(true)
}

/// 시도의 끝을 Job 에 올린다 — **보고를 저장하는 같은 트랜잭션 안에서** 부른다.
///
/// ★★ 2026-09-23 (신뢰망 남은 일 A). 전에는 시도가 끝나도 Job 이 영원히 `STAGING` 이었다.
///
/// ```text
/// 시도 최종    실행에 들어갔나   Job 경로(규범 §2)                        trigger
/// COMPLETED    -                 (STAGING ->) RUNNING -> COMPLETED         ATTEMPT_COMPLETED
/// FAILED       예                (STAGING ->) RUNNING -> FAILED            UNRECOVERABLE_ERROR
/// FAILED       아니오            STAGING -> FAILED                         STAGING_FAILED
/// CANCELLED    -                 STAGING -> FAILED                         STAGING_FAILED
/// FAILED 아니오 · CANCELLED       RUNNING -> FAILED (이미 RUNNING 일 때)     UNRECOVERABLE_ERROR  ★ 결함 216
/// ```
///
/// ★ 재시도 정책은 **0회**다(신뢰망 계획 §기준선과 다른 점). 그래서 두 실패 trigger 의 guard
///   "재시도 소진" 이 즉시 참이다. 재시도는 운영자가 다시 제출한다.
///
/// ★ **건드리지 않는 경우** — `Ok(None)` 을 돌려준다:
///   * 이 시도가 그 Job 의 **가장 최근 시도가 아니다**(fence 가 더 큰 시도가 있다). 장애 이어받기 뒤
///     옛 노드가 늦게 보고하면 여기 온다 — 옛 시도의 결과로 새 시도의 Job 을 끝내지 않는다.
///     그 보고는 저장되고 옛 시도도 종료로 적힌다. 둘 다 완료했으면 규범의 `DUPLICATE_COMPLETION`
///     조정(§20.3)이 필요한데 **아직 없다** — 그래서 Job 은 새 시도를 따른다.
///   * Job 이 이미 끝났거나 실행 단계(`STAGING`/`RUNNING`)가 아니다.
///
/// ★ 경로는 규범 검증용이다 — DB 에는 **최종 상태만** 쓴다(코덱스 72 결정 C).
pub(crate) fn follow_attempt_terminal(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    attempt_final: gputeer_protocol::attempt_state::AttemptState,
    attempt_ran: bool,
    worker_reported_finished_at_unix_ms: u64,
) -> Result<Option<StoredJob>, JobStoreError> {
    use gputeer_protocol::attempt_state::AttemptState as A;
    let Some(mut job) = fetch_job(connection, job_id)? else {
        return Err(JobStoreError::NotFound);
    };
    if !matches!(job.state, JobState::Staging | JobState::Running) {
        return Ok(None);
    }
    let latest: Option<String> = connection
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_sql_error)?;
    if latest.as_deref() != Some(attempt_id) {
        return Ok(None);
    }
    let path: &[JobState] = match (attempt_final, attempt_ran) {
        (A::Completed, _) => &[JobState::Running, JobState::Completed],
        (A::Failed, true) => &[JobState::Running, JobState::Failed],
        (A::Failed, false) | (A::Cancelled, _) => &[JobState::Failed],
        _ => return Ok(None),
    };
    // 이미 RUNNING 이면 첫 칸(STAGING -> RUNNING)은 이미 밟았다.
    let path = if job.state == JobState::Running && path.first() == Some(&JobState::Running) {
        &path[1..]
    } else {
        path
    };
    // ★ 2026-09-23 (결함 216 · 검수 73) — trigger 는 **마지막 간선**이 정한다. 전에는 시도 결과만 보고 골라서,
    //   선점 뒤 이미 RUNNING 인 Job 의 새 시도가 실행 전에 실패하면 `RUNNING -> FAILED` 를 STAGING_FAILED 로 적었다.
    //   CANCELLED 시도는 Agent 의 Grant 거부(규범 Attempt 표 GRANT_REJECTED)다 — 사용자 취소가 아니므로 Job 은 실패로 끝난다.
    let (last_from, prefix) = match path.split_last() {
        Some((_, prefix)) => (prefix.last().copied().unwrap_or(job.state), prefix),
        None => return Ok(None),
    };
    let final_to = *path.last().expect("위에서 비어 있지 않음을 봤다");
    let terminal = match (last_from, final_to) {
        (JobState::Running, JobState::Completed) => RunTerminal::AttemptCompleted,
        (JobState::Running, JobState::Failed) => RunTerminal::UnrecoverableError,
        (JobState::Staging, JobState::Failed) => RunTerminal::StagingFailed,
        (from, to) => return Err(JobStoreError::InvalidTransition { from, to }),
    };
    let reject = |rejected: gputeer_protocol::job_state::JobTransitionRejected| {
        JobStoreError::InvalidTransition {
            from: rejected.from,
            to: rejected.to,
        }
    };
    gputeer_protocol::job_state::walk(job.state, prefix).map_err(reject)?;
    let final_state =
        gputeer_protocol::job_state::transition_via(last_from, final_to, terminal.trigger())
            .map_err(reject)?;
    job.state = final_state;
    job.run_terminal = Some(terminal);
    job.worker_reported_finished_at_unix_ms = Some(worker_reported_finished_at_unix_ms);
    job.revision = job
        .revision
        .checked_add(1)
        .ok_or(JobStoreError::CorruptData("revision overflow".to_string()))?;
    update_job(connection, &job)?;
    Ok(Some(job))
}

/// ★ 2026-09-24 (결함 235 · 재검수 79) — 풀 Coordinator 가 시작할 때 control DB 에 "이 DB 는 풀이다" 를 적는다. 스케줄러가 이것으로
///   풀 여부를 안다(명령줄 인자로 짐작하지 않는다). 한 번 적으면 지우지 않는다(멱등).
pub fn declare_pool_mode(control_db: &std::path::Path, now_unix_ms: u64) -> Result<(), String> {
    let connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS coordinator_pool_mode (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                declared_at_unix_ms BLOB NOT NULL CHECK(length(declared_at_unix_ms) = 8)
            );",
        )
        .map_err(|e| e.to_string())?;
    connection
        .execute(
            "INSERT OR IGNORE INTO coordinator_pool_mode(singleton, declared_at_unix_ms) VALUES (1, ?1)",
            rusqlite::params![now_unix_ms.to_be_bytes().to_vec()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 이 control DB 를 풀 Coordinator 가 쓰는가([`declare_pool_mode`]). 표가 없으면 아니다(읽기만 한다).
///
/// ★ 2026-09-25 (결함 419 · 재검수 103) — **생성 없음**으로 연다. 기본(읽기 · 쓰기 · 생성)으로 열면, 호출자가 존재를 확인한 뒤 파일이 옮겨졌을 때
///   빈 DB 를 만들고 "표식 없음" 으로 판단했다. 없는 파일이면 열기가 실패한다(호출자는 거부한다 — fail-closed).
/// ★ 결함 420 (재검수 104) — 그렇다고 **읽기 전용**으로 열면 안 된다. 비정상 종료가 남긴 rollback journal 을 되감지 못해 실패하고, 이 검사가 시작의 첫 DB
///   접근이라 재시작마다 같은 곳에서 막혔다(419 조치가 처음에 그랬다). 읽기 · 쓰기로 열되 만들지는 않는다.
/// ★ 결함 421 — URI(`file:…`)는 이 열기에서도 해석된다(bundled SQLite 가 `SQLITE_USE_URI` 로 빌드됐다). URI 경로는 호출자
///   (`refuse_pool_marked_db_without_pool_mode`)가 먼저 거부한다.
pub fn pool_mode_declared(control_db: &std::path::Path) -> Result<bool, String> {
    let connection =
        Connection::open_with_flags(control_db, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let table = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_pool_mode'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if table.is_none() {
        return Ok(false);
    }
    Ok(connection
        .query_row(
            "SELECT 1 FROM coordinator_pool_mode WHERE singleton = 1",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .is_some())
}

impl CoordinatorJobStore {
    /// ★ 2026-09-25 (결함 410 · 재검수 97) — 이 저장소의 DB 에 풀 표식이 있는가([`declare_pool_mode`]). Grant 를 만드는 곳이 설정의 불리언이
    ///   아니라 **DB 의 사실**로 풀 여부를 알게 한다. 표가 없으면 아니다(읽기만 한다).
    pub fn pool_mode_declared(&self) -> Result<bool, JobStoreError> {
        let table = self
            .connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_pool_mode'",
                [],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_sql_error)?;
        if table.is_none() {
            return Ok(false);
        }
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM coordinator_pool_mode WHERE singleton = 1",
                [],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_sql_error)?
            .is_some())
    }
}

pub(crate) fn update_job(connection: &Connection, job: &StoredJob) -> Result<(), JobStoreError> {
    let (failure_kind, failure_detail) = match &job.queue_failure {
        Some(failure) => (Some(failure.code()), failure.detail()),
        None => (None, None),
    };
    let changed = connection
        .execute(
            "UPDATE coordinator_jobs SET
                state = ?2, planning_at_unix_ms = ?3, queued_at_unix_ms = ?4,
                staging_at_unix_ms = ?5, plan_id = ?6, queue_failure_kind = ?7,
                queue_failure_detail = ?8, failed_at_unix_ms = ?9, revision = ?10,
                running_at_unix_ms = ?11, run_terminal = ?12,
                worker_reported_finished_at_unix_ms = ?13,
                requeue_count = ?14, resume_checkpoint = ?15
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
                job.running_at_unix_ms.map(encode_u64),
                job.run_terminal.map(RunTerminal::trigger),
                job.worker_reported_finished_at_unix_ms.map(encode_u64),
                // 0 은 NULL 로 둔다 — 이 칸이 생기기 전의 행과 같은 모양이다.
                (job.requeue_count > 0).then(|| encode_u64(job.requeue_count)),
                job.resume_checkpoint.as_deref(),
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
    running_at_unix_ms: Option<Vec<u8>>,
    run_terminal: Option<String>,
    worker_reported_finished_at_unix_ms: Option<Vec<u8>>,
    requeue_count: Option<Vec<u8>>,
    resume_checkpoint: Option<Vec<u8>>,
}

impl RawJobRow {
    fn into_stored(self) -> Result<StoredJob, JobStoreError> {
        let state = parse_job_state(&self.state)?;
        let run_terminal = self
            .run_terminal
            .as_deref()
            .map(RunTerminal::parse)
            .transpose()?;
        let not_run_yet = self.running_at_unix_ms.is_none()
            && run_terminal.is_none()
            && self.worker_reported_finished_at_unix_ms.is_none();
        let queue_failure = match self.queue_failure_kind {
            Some(kind) => Some(QueueFailure::parse(&kind, self.queue_failure_detail)?),
            None if self.queue_failure_detail.is_none() => None,
            None => {
                return Err(JobStoreError::CorruptData(
                    "queue failure detail exists without a kind".to_string(),
                ))
            }
        };
        let requeue_count = self
            .requeue_count
            .as_deref()
            .map(|bytes| decode_u64(bytes, "requeue_count"))
            .transpose()?
            .unwrap_or(0);
        // 이어갈 체크포인트는 이어받기로 되돌아온 Job 에만 있다.
        if self.resume_checkpoint.is_some() && requeue_count == 0 {
            return Err(JobStoreError::CorruptData(
                "resume_checkpoint exists without a requeue".to_string(),
            ));
        }
        if self.resume_checkpoint.as_ref().is_some_and(Vec::is_empty) {
            return Err(JobStoreError::CorruptData(
                "resume_checkpoint is empty".to_string(),
            ));
        }
        let has_plan = self
            .plan_id
            .as_deref()
            .is_some_and(|plan_id| !plan_id.trim().is_empty());
        let valid_shape = match state {
            JobState::Submitted => {
                not_run_yet
                    && self.planning_at_unix_ms.is_none()
                    && self.queued_at_unix_ms.is_none()
                    && self.staging_at_unix_ms.is_none()
                    && self.plan_id.is_none()
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Planning => {
                not_run_yet
                    && self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_none()
                    && self.staging_at_unix_ms.is_none()
                    && self.plan_id.is_none()
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Queued => {
                not_run_yet
                    && self.planning_at_unix_ms.is_some()
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
                not_run_yet
                    && self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_some()
                    && self.staging_at_unix_ms.is_some()
                    && self
                        .plan_id
                        .as_deref()
                        .is_some_and(|plan_id| !plan_id.trim().is_empty())
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Running | JobState::Paused => {
                self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_some()
                    && self.staging_at_unix_ms.is_some()
                    && self.running_at_unix_ms.is_some()
                    && run_terminal.is_none()
                    && self.worker_reported_finished_at_unix_ms.is_none()
                    && has_plan
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Completed => {
                self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_some()
                    && self.staging_at_unix_ms.is_some()
                    && run_terminal == Some(RunTerminal::AttemptCompleted)
                    && has_plan
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none()
            }
            JobState::Failed => {
                let queue_stage_failure = not_run_yet
                    && self.staging_at_unix_ms.is_none()
                    && queue_failure.is_some()
                    && self.failed_at_unix_ms.is_some();
                let run_stage_failure = self.staging_at_unix_ms.is_some()
                    && matches!(
                        run_terminal,
                        Some(
                            RunTerminal::StagingFailed
                                | RunTerminal::UnrecoverableError
                                | RunTerminal::NoCommittedCheckpoint
                        )
                    )
                    && queue_failure.is_none()
                    && self.failed_at_unix_ms.is_none();
                self.planning_at_unix_ms.is_some()
                    && self.queued_at_unix_ms.is_some()
                    && has_plan
                    && (queue_stage_failure || run_stage_failure)
            }
            _ => false,
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
            submitted_at_unix_ms: decode_u64(&self.submitted_at_unix_ms, "submitted_at_unix_ms")?,
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
            running_at_unix_ms: self
                .running_at_unix_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "running_at_unix_ms"))
                .transpose()?,
            run_terminal,
            requeue_count,
            resume_checkpoint: self.resume_checkpoint,
            worker_reported_finished_at_unix_ms: self
                .worker_reported_finished_at_unix_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "worker_reported_finished_at_unix_ms"))
                .transpose()?,
            revision: decode_u64(&self.revision, "revision")?,
        })
    }
}

const SELECT_JOB_SQL: &str = "SELECT job_id, submitter_device_id, manifest_hash, state, \
    submitted_at_unix_ms, planning_at_unix_ms, queued_at_unix_ms, staging_at_unix_ms, deadline_unix_ms, \
    max_queue_duration_ms, plan_id, queue_failure_kind, queue_failure_detail, \
    failed_at_unix_ms, revision, running_at_unix_ms, run_terminal, \
    worker_reported_finished_at_unix_ms, requeue_count, resume_checkpoint FROM coordinator_jobs WHERE job_id = ?1";

const SELECT_QUEUED_SQL: &str = "SELECT job_id, submitter_device_id, manifest_hash, state, \
    submitted_at_unix_ms, planning_at_unix_ms, queued_at_unix_ms, staging_at_unix_ms, deadline_unix_ms, \
    max_queue_duration_ms, plan_id, queue_failure_kind, queue_failure_detail, \
    failed_at_unix_ms, revision, running_at_unix_ms, run_terminal, \
    worker_reported_finished_at_unix_ms, requeue_count, resume_checkpoint FROM coordinator_jobs WHERE state = 'QUEUED'";

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
        running_at_unix_ms: row.get(15)?,
        run_terminal: row.get(16)?,
        worker_reported_finished_at_unix_ms: row.get(17)?,
        requeue_count: row.get(18)?,
        resume_checkpoint: row.get(19)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
    use gputeer_protocol::signing::{verify, NoReplayCheck};

    const MANIFEST_DEVICE: &str = "submitter-1";

    fn verified_manifest(
        job_id: &str,
        device_id: &str,
        key_seed: u8,
        team_marker: &str,
    ) -> Verified<pb::JobManifest> {
        let key = SigningKey::from_bytes(&[key_seed; 32]);
        let mut manifest = pb::JobManifest {
            schema_version: 1,
            job_id: job_id.to_string(),
            team_id: format!("team-{team_marker}"),
            entrypoint: "train.py".to_string(),
            submitter_device_id: device_id.to_string(),
            issued_at_unix_ms: 10,
            expires_at_unix_ms: 10_000,
            ..Default::default()
        };
        manifest.submitter_signature = sign(&key, &manifest).to_vec();
        let mut keys = InMemoryKeyring::new();
        keys.insert(device_id, key.verifying_key());
        verify(
            &manifest,
            1,
            &Ed25519Verifier::new(keys),
            100,
            &mut NoReplayCheck,
        )
        .expect("test Manifest signature must verify")
    }

    fn bound_submission(
        verified: &Verified<pb::JobManifest>,
        key_byte: u8,
    ) -> AcceptedJobSubmission {
        AcceptedJobSubmission {
            idempotency_key: [key_byte; 16],
            job_id: verified.get().job_id.clone(),
            submitter_device_id: verified.get().submitter_device_id.clone(),
            manifest_hash: derive_manifest_hash(verified.get()),
            deadline_unix_ms: Some(10_000),
            max_queue_duration_ms: Some(1_000),
        }
    }

    fn table_count(store: &CoordinatorJobStore, table: &str) -> u64 {
        store
            .connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

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
    fn verified_manifest_body_signature_and_derived_hash_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite3");
        let verified = verified_manifest("job-bound", MANIFEST_DEVICE, 7, "original");
        let request = bound_submission(&verified, 1);
        let original_manifest = verified.get().clone();
        {
            let mut store = CoordinatorJobStore::open(&path).unwrap();
            let result = store
                .submit_verified_manifest(&request, &verified, 100)
                .unwrap();
            assert!(result.created);
            assert_eq!(result.binding.manifest, original_manifest);
            assert_eq!(result.binding.manifest_hash, request.manifest_hash);
            assert_eq!(result.binding.signer_id_at_submission, MANIFEST_DEVICE);
        }

        let reopened = CoordinatorJobStore::open(&path).unwrap();
        let binding = reopened.get_manifest_binding("job-bound").unwrap().unwrap();
        assert_eq!(binding.manifest, original_manifest);
        assert_eq!(
            binding.manifest.submitter_signature,
            original_manifest.submitter_signature
        );
        assert_eq!(
            binding.manifest_hash,
            derive_manifest_hash(&binding.manifest)
        );
        assert_eq!(binding.signer_id_at_submission, MANIFEST_DEVICE);
    }

    #[test]
    fn identity_and_supplied_hash_mismatches_create_no_rows() {
        let verified = verified_manifest("job-bound", MANIFEST_DEVICE, 7, "original");
        for mutation in ["job_id", "device_id", "hash"] {
            let (mut store, _dir) = open_temp();
            let mut request = bound_submission(&verified, 1);
            let expected = match mutation {
                "job_id" => {
                    request.job_id = "another-job".to_string();
                    JobStoreError::ManifestIdentityMismatch("job_id")
                }
                "device_id" => {
                    request.submitter_device_id = "another-device".to_string();
                    JobStoreError::ManifestIdentityMismatch("submitter_device_id")
                }
                "hash" => {
                    request.manifest_hash[0] ^= 0xff;
                    JobStoreError::ManifestHashMismatch
                }
                _ => unreachable!(),
            };
            assert_eq!(
                store.submit_verified_manifest(&request, &verified, 100),
                Err(expected),
                "mutation {mutation} must fail closed"
            );
            assert_eq!(table_count(&store, "coordinator_jobs"), 0);
            assert_eq!(table_count(&store, "coordinator_job_manifests"), 0);
            assert_eq!(table_count(&store, "job_submission_idempotency"), 0);
        }
    }

    #[test]
    fn verified_manifest_replay_returns_original_and_changed_manifest_conflicts() {
        let (mut store, _dir) = open_temp();
        let verified = verified_manifest("job-bound", MANIFEST_DEVICE, 7, "original");
        let request = bound_submission(&verified, 1);
        let first = store
            .submit_verified_manifest(&request, &verified, 100)
            .unwrap();
        let replay = store
            .submit_verified_manifest(&request, &verified, 999)
            .unwrap();
        assert!(first.created);
        assert!(!replay.created);
        assert_eq!(replay.job, first.job);
        assert_eq!(replay.binding, first.binding);
        assert_eq!(table_count(&store, "coordinator_jobs"), 1);
        assert_eq!(table_count(&store, "coordinator_job_manifests"), 1);
        assert_eq!(table_count(&store, "job_submission_idempotency"), 1);

        let changed = verified_manifest("job-bound", MANIFEST_DEVICE, 7, "changed");
        let changed_same_key = bound_submission(&changed, 1);
        assert!(matches!(
            store.submit_verified_manifest(&changed_same_key, &changed, 200),
            Err(JobStoreError::IdempotencyConflict { .. })
        ));

        let changed_other_key = bound_submission(&changed, 2);
        assert_eq!(
            store.submit_verified_manifest(&changed_other_key, &changed, 200),
            Err(JobStoreError::JobIdConflict {
                job_id: "job-bound".to_string()
            })
        );
        assert_eq!(
            store.get_manifest_binding("job-bound").unwrap().unwrap(),
            first.binding
        );
    }

    #[test]
    fn fault_after_manifest_insert_rolls_back_job_body_and_idempotency() {
        let (mut store, _dir) = open_temp();
        let verified = verified_manifest("job-bound", MANIFEST_DEVICE, 7, "original");
        let request = bound_submission(&verified, 1);
        assert_eq!(
            store.submit_verified_manifest_inner(
                &request,
                &verified,
                100,
                Some(TestFault::AfterManifestInsert)
            ),
            Err(JobStoreError::InjectedFailure("after Manifest insert"))
        );
        assert_eq!(table_count(&store, "coordinator_jobs"), 0);
        assert_eq!(table_count(&store, "coordinator_job_manifests"), 0);
        assert_eq!(table_count(&store, "job_submission_idempotency"), 0);
    }

    #[test]
    fn empty_and_undecodable_manifest_bodies_fail_closed() {
        for (body, expected) in [
            (Vec::new(), ManifestCorruption::EmptyBody),
            (vec![0x12, 0x05, b'a'], ManifestCorruption::UndecodableBody),
        ] {
            let (mut store, _dir) = open_temp();
            let verified = verified_manifest("job-bound", MANIFEST_DEVICE, 7, "original");
            let request = bound_submission(&verified, 1);
            store
                .submit_verified_manifest(&request, &verified, 100)
                .unwrap();
            store
                .connection
                .execute(
                    "UPDATE coordinator_job_manifests SET manifest_body = ?1 WHERE job_id = ?2",
                    rusqlite::params![body, request.job_id],
                )
                .unwrap();
            assert_eq!(
                store.get_manifest_binding(&request.job_id),
                Err(JobStoreError::ManifestCorrupt {
                    job_id: request.job_id.clone(),
                    kind: expected,
                })
            );
        }
    }

    #[test]
    fn body_hash_and_job_identity_corruption_fail_closed_independently() {
        let (mut store, _dir) = open_temp();
        let verified = verified_manifest("job-hash", MANIFEST_DEVICE, 7, "original");
        let request = bound_submission(&verified, 1);
        store
            .submit_verified_manifest(&request, &verified, 100)
            .unwrap();
        let mut changed_body = verified.get().clone();
        changed_body.team_id = "tampered-team".to_string();
        store
            .connection
            .execute(
                "UPDATE coordinator_job_manifests SET manifest_body = ?1 WHERE job_id = ?2",
                rusqlite::params![changed_body.encode_to_vec(), request.job_id],
            )
            .unwrap();
        assert_eq!(
            store.get_manifest_binding(&request.job_id),
            Err(JobStoreError::ManifestCorrupt {
                job_id: request.job_id.clone(),
                kind: ManifestCorruption::HashMismatch,
            })
        );

        let verified = verified_manifest("job-identity", MANIFEST_DEVICE, 8, "original");
        let request = bound_submission(&verified, 2);
        store
            .submit_verified_manifest(&request, &verified, 100)
            .unwrap();
        let mut changed_identity = verified.get().clone();
        changed_identity.job_id = "body-points-elsewhere".to_string();
        let changed_hash = derive_manifest_hash(&changed_identity);
        store
            .connection
            .execute(
                "UPDATE coordinator_jobs SET manifest_hash = ?1 WHERE job_id = ?2",
                rusqlite::params![changed_hash.as_slice(), request.job_id],
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_job_manifests SET manifest_body = ?1 WHERE job_id = ?2",
                rusqlite::params![changed_identity.encode_to_vec(), request.job_id],
            )
            .unwrap();
        assert_eq!(
            store.get_manifest_binding(&request.job_id),
            Err(JobStoreError::ManifestCorrupt {
                job_id: request.job_id.clone(),
                kind: ManifestCorruption::JobIdMismatch,
            })
        );

        let verified = verified_manifest("job-device-identity", MANIFEST_DEVICE, 9, "original");
        let request = bound_submission(&verified, 3);
        store
            .submit_verified_manifest(&request, &verified, 100)
            .unwrap();
        let mut changed_identity = verified.get().clone();
        changed_identity.submitter_device_id = "body-device-elsewhere".to_string();
        let changed_hash = derive_manifest_hash(&changed_identity);
        store
            .connection
            .execute(
                "UPDATE coordinator_jobs SET manifest_hash = ?1 WHERE job_id = ?2",
                rusqlite::params![changed_hash.as_slice(), request.job_id],
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_job_manifests SET manifest_body = ?1 WHERE job_id = ?2",
                rusqlite::params![changed_identity.encode_to_vec(), request.job_id],
            )
            .unwrap();
        assert_eq!(
            store.get_manifest_binding(&request.job_id),
            Err(JobStoreError::ManifestCorrupt {
                job_id: request.job_id.clone(),
                kind: ManifestCorruption::SubmitterDeviceIdMismatch,
            })
        );
    }

    #[test]
    fn device_signer_corruption_and_legacy_missing_body_fail_closed() {
        let (mut store, _dir) = open_temp();
        let verified = verified_manifest("job-device", MANIFEST_DEVICE, 7, "original");
        let request = bound_submission(&verified, 1);
        store
            .submit_verified_manifest(&request, &verified, 100)
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_job_manifests SET verified_signer_id = 'other-device' WHERE job_id = ?1",
                rusqlite::params![request.job_id],
            )
            .unwrap();
        assert_eq!(
            store.get_manifest_binding(&request.job_id),
            Err(JobStoreError::ManifestCorrupt {
                job_id: request.job_id.clone(),
                kind: ManifestCorruption::SignerIdMismatch,
            })
        );

        let legacy = submission("legacy-job", 2);
        store.submit_accepted(&legacy, 100).unwrap();
        assert_eq!(
            store.get_manifest_binding("legacy-job"),
            Err(JobStoreError::LegacyManifestMissing {
                job_id: "legacy-job".to_string()
            })
        );
        assert!(store.get("legacy-job").unwrap().is_some());
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
        assert_eq!(
            store.get_manifest_binding("old-job"),
            Err(JobStoreError::LegacyManifestMissing {
                job_id: "old-job".to_string()
            })
        );
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
        assert!(matches!(
            result,
            Err(JobStoreError::IdempotencyConflict { .. })
        ));
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
        assert_eq!(
            store.get("job-1").unwrap().unwrap().state,
            JobState::Submitted
        );
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
            Err(JobStoreError::InvalidInput(
                "permanent infeasibility reason"
            ))
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
            .execute(
                "UPDATE coordinator_jobs SET state = 'UNKNOWN' WHERE job_id = 'job-1'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.get("job-1"),
            Err(JobStoreError::CorruptData(_))
        ));
        store
            .connection
            .execute(
                "UPDATE coordinator_jobs SET state = 'SUBMITTED', revision = x'01' WHERE job_id = 'job-1'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.get("job-1"),
            Err(JobStoreError::CorruptData(_))
        ));

        let second = submission("job-2", 2);
        queued(&mut store, &second);
        store
            .connection
            .execute(
                "UPDATE coordinator_jobs SET plan_id = ' ' WHERE job_id = 'job-2'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.get("job-2"),
            Err(JobStoreError::CorruptData(_))
        ));
    }

    #[test]
    fn u64_max_timestamp_round_trips_without_sqlite_integer_truncation() {
        let (mut store, _dir) = open_temp();
        let request = submission("job-1", 1);
        let result = store.submit_accepted(&request, u64::MAX).unwrap();
        assert_eq!(result.job.submitted_at_unix_ms, u64::MAX);
        assert_eq!(
            store.get("job-1").unwrap().unwrap().submitted_at_unix_ms,
            u64::MAX
        );
    }

    /// ★ 결함 402 — 한 트랜잭션: SUBMITTED 에서 두 전이를 모두 기록하고(두 시각 · revision +2), PLANNING 을 적은 뒤 실패하면 SUBMITTED 로 남는다.
    #[test]
    fn plan_and_enqueue_is_one_transaction() {
        let (mut store, _dir) = open_temp();
        store.submit_accepted(&submission("job-1", 1), 100).unwrap();
        assert!(store
            .plan_and_enqueue_failing_after_planning("job-1", "plan-1", 200)
            .is_err());
        let after_failure = store.get("job-1").unwrap().unwrap();
        assert_eq!(
            after_failure.state,
            JobState::Submitted,
            "PLANNING 이 남았다"
        );
        assert_eq!(after_failure.revision, 0);
        assert_eq!(after_failure.planning_at_unix_ms, None);

        let queued = store.plan_and_enqueue("job-1", "plan-1", 200).unwrap();
        assert_eq!(queued.state, JobState::Queued);
        assert_eq!(queued.revision, 2);
        assert_eq!(queued.planning_at_unix_ms, Some(200));
        assert_eq!(queued.queued_at_unix_ms, Some(200));
        assert_eq!(queued.plan_id.as_deref(), Some("plan-1"));
        // 재시도는 멱등 · 다른 계획은 거부
        assert_eq!(
            store.plan_and_enqueue("job-1", "plan-1", 999).unwrap(),
            queued
        );
        assert!(matches!(
            store.plan_and_enqueue("job-1", "plan-2", 999),
            Err(JobStoreError::PlanConflict { .. })
        ));
    }

    /// 옛 실행이 PLANNING 에 남긴 Job 은 QUEUED 로만 옮긴다(PLANNING 시각은 그대로).
    #[test]
    fn plan_and_enqueue_finishes_a_job_left_in_planning() {
        let (mut store, _dir) = open_temp();
        store.submit_accepted(&submission("job-1", 1), 100).unwrap();
        store.start_planning("job-1", 150).unwrap();
        let queued = store.plan_and_enqueue("job-1", "plan-1", 200).unwrap();
        assert_eq!(queued.state, JobState::Queued);
        assert_eq!(queued.planning_at_unix_ms, Some(150));
        assert_eq!(queued.revision, 2);
        assert!(matches!(
            store.plan_and_enqueue("job-1", "", 200),
            Err(JobStoreError::InvalidInput("plan_id"))
        ));
    }
}
