//! Single-node local atomic `QUEUED -> STAGING` storage kernel.
//!
//! One file, one SQLite connection, and one `BEGIN IMMEDIATE` transaction own
//! the Job transition, Attempt/node creation, team-global fence allocation,
//! Lease insertion, and operation idempotency record. The reservation-aware
//! entrypoint additionally compares the selected Agent inventory revision and
//! validates and binds the selected GPU IDs to a node-exclusive reservation in
//! that same transaction. This is local durable state only; it is not a
//! Raft/ControlStore `COMMITTED` transition and does not dispatch a Grant.

use std::path::Path;

use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

use crate::job_store::{self, JobState, StoredJob};
use crate::lease_store::{self, StoredLease};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageQueuedRequest {
    pub operation_key: [u8; 16],
    pub job_id: String,
    pub attempt_id: String,
    pub lease_id: String,
    pub node_id: String,
    /// Canonical scheduler GPU IDs selected from the admitted inventory revision.
    ///
    /// The reservation-aware entrypoint requires a non-empty, strictly sorted
    /// list. The legacy reservation-free entrypoint deliberately ignores it.
    pub selected_gpu_ids: Vec<String>,
    pub issuing_coordinator_id: String,
    pub coordinator_term: u64,
    pub issued_at_unix_ms: u64,
    pub renew_after_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub max_total_duration_seconds: u64,
}

/// Attempt 상태 — **규범 정본을 그대로 쓴다.**
///
/// ★★ 2026-09-22 (§A1 4c) — 전에는 이 파일에 `Created` 하나짜리 **별도 enum**
///   이 있었다. 같은 이름의 타입이 둘이면 저장소 쪽에 상태를 추가할 때 규범 표를
///   거치지 않고 늘어난다 — `attempt_state_parity.rs` 가 그 위험 때문에 생겼다.
///   이제 **하나로 합쳤으므로 갈라질 여지 자체가 없다.**
///   SQLite 에 적히는 문자열은 아래 `state_to_db`/`state_from_db` 가 책임진다.
pub use gputeer_protocol::attempt_state::AttemptState;

/// 상태 -> SQLite 문자열.
///
/// ★ 이 값은 **디스크에 남는다.** 이름을 바꾸면 옛 행을 못 읽는다 — 바꾸려면 마이그레이션이 필요하다.
pub fn state_to_db(state: AttemptState) -> &'static str {
    match state {
        AttemptState::Created => "CREATED",
        AttemptState::Starting => "STARTING",
        AttemptState::Running => "RUNNING",
        AttemptState::Paused => "PAUSED",
        AttemptState::Stale => "STALE",
        AttemptState::Completed => "COMPLETED",
        AttemptState::Failed => "FAILED",
        AttemptState::Cancelled => "CANCELLED",
        AttemptState::Reconciling => "RECONCILING",
        AttemptState::Canonical => "CANONICAL",
        AttemptState::Superseded => "SUPERSEDED",
    }
}

/// SQLite 문자열 -> 상태. 모르는 값은 **손상으로 거부한다**(조용히 Created 로 읽지 않는다).
pub fn state_from_db(raw: &str) -> Result<AttemptState, StagingStoreError> {
    gputeer_protocol::attempt_state::ALL_ATTEMPT_STATES
        .iter()
        .copied()
        .find(|s| state_to_db(*s) == raw)
        .ok_or_else(|| StagingStoreError::CorruptData(format!("unknown Attempt state: {raw}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAttempt {
    pub attempt_id: String,
    pub job_id: String,
    pub state: AttemptState,
    pub node_ids: Vec<String>,
    pub fence_epoch: u64,
    pub lease_id: String,
    pub created_at_unix_ms: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageQueuedResult {
    pub job: StoredJob,
    pub attempt: StoredAttempt,
    pub lease: StoredLease,
    /// `false` means the operation key replayed its original durable result.
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredNodeReservation {
    pub node_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub inventory_revision: u64,
    pub reserved_at_unix_ms: u64,
    pub selected_gpu_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedStageResult {
    pub reservation: StoredNodeReservation,
    pub stage: StageQueuedResult,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StagingStoreError {
    InvalidInput(&'static str),
    JobNotFound,
    JobNotQueued(JobState),
    OperationConflict,
    AttemptIdConflict(String),
    LeaseIdConflict(String),
    ClockRollback {
        issued_at_unix_ms: u64,
        queued_at_unix_ms: u64,
    },
    InvalidLeaseLifetime,
    FenceEpochOverflow,
    /// 규범 표에 없는 Attempt 전이다(§A1 4c).
    AttemptTransitionRejected {
        from: AttemptState,
        to: AttemptState,
    },
    /// 읽은 상태와 지금 저장된 상태가 다르다 — 다른 쓰기가 끼어들었다(CAS 실패).
    AttemptStateRaced {
        expected: AttemptState,
    },
    CorruptData(String),
    Io(String),
    LockTimeout,
    #[cfg(test)]
    InjectedFailure(&'static str),
}

impl std::fmt::Display for StagingStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(field) => write!(f, "invalid staging input: {field}"),
            Self::JobNotFound => write!(f, "job_id is not present in the control DB"),
            Self::JobNotQueued(state) => write!(f, "Job is not QUEUED: {state:?}"),
            Self::OperationConflict => write!(f, "staging operation key payload conflict"),
            Self::AttemptTransitionRejected { from, to } => write!(
                f,
                "규범 표에 없는 Attempt 전이다: {from:?} -> {to:?} (docs/protocol/state-machines.md §3)"
            ),
            Self::AttemptStateRaced { expected } => write!(
                f,
                "Attempt 상태가 읽은 값({expected:?})과 달라 갱신하지 않았다 — 다른 쓰기가 끼어들었다"
            ),
            Self::AttemptIdConflict(id) => write!(f, "attempt_id already exists: {id}"),
            Self::LeaseIdConflict(id) => write!(f, "lease_id already exists: {id}"),
            Self::ClockRollback { issued_at_unix_ms, queued_at_unix_ms } => write!(
                f,
                "staging clock moved backwards: issued={issued_at_unix_ms}, queued={queued_at_unix_ms}"
            ),
            Self::InvalidLeaseLifetime => write!(f, "invalid Lease lifetime ordering"),
            Self::FenceEpochOverflow => write!(f, "team-global fence epoch overflow"),
            Self::CorruptData(message) => write!(f, "staging store corruption: {message}"),
            Self::Io(message) => write!(f, "staging store I/O error: {message}"),
            Self::LockTimeout => write!(f, "staging store lock acquisition timed out"),
            #[cfg(test)]
            Self::InjectedFailure(point) => write!(f, "injected staging failure: {point}"),
        }
    }
}

impl std::error::Error for StagingStoreError {}

#[derive(Debug, PartialEq, Eq)]
pub enum ReservedStageError {
    InventoryMissing {
        node_id: String,
    },
    InventoryRevisionMismatch {
        node_id: String,
        expected: u64,
        actual: u64,
    },
    SelectedGpuMissing {
        node_id: String,
        gpu_id: String,
    },
    NodeAlreadyReserved {
        node_id: String,
        owning_job_id: String,
        owning_attempt_id: String,
    },
    Staging(StagingStoreError),
}

impl std::fmt::Display for ReservedStageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InventoryMissing { node_id } => {
                write!(f, "inventory is missing for selected node: {node_id}")
            }
            Self::InventoryRevisionMismatch {
                node_id,
                expected,
                actual,
            } => write!(
                f,
                "inventory revision CAS conflict for {node_id}: expected={expected}, actual={actual}"
            ),
            Self::SelectedGpuMissing { node_id, gpu_id } => write!(
                f,
                "selected GPU is missing from admitted inventory: node={node_id}, gpu={gpu_id}"
            ),
            Self::NodeAlreadyReserved {
                node_id,
                owning_job_id,
                owning_attempt_id,
            } => write!(
                f,
                "node is already reserved: node={node_id}, job={owning_job_id}, attempt={owning_attempt_id}"
            ),
            Self::Staging(error) => write!(f, "durable staging failed: {error}"),
        }
    }
}

impl std::error::Error for ReservedStageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Staging(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StagingStoreError> for ReservedStageError {
    fn from(error: StagingStoreError) -> Self {
        Self::Staging(error)
    }
}

pub struct CoordinatorStagingStore {
    connection: Connection,
}

pub(crate) fn initialize_schema(connection: &mut Connection) -> Result<(), StagingStoreError> {
    job_store::initialize_schema(connection).map_err(map_job_error)?;
    lease_store::initialize_schema(connection).map_err(map_lease_error)?;
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS coordinator_attempts (
                attempt_id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL REFERENCES coordinator_jobs(job_id),
                state TEXT NOT NULL,
                fence_epoch BLOB NOT NULL,
                lease_id TEXT NOT NULL,
                created_at_unix_ms BLOB NOT NULL,
                revision BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS coordinator_attempt_nodes (
                attempt_id TEXT NOT NULL REFERENCES coordinator_attempts(attempt_id),
                node_id TEXT NOT NULL,
                ordinal INTEGER NOT NULL,
                PRIMARY KEY(attempt_id, ordinal),
                UNIQUE(attempt_id, node_id)
            );
            CREATE TABLE IF NOT EXISTS coordinator_fence_state (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                max_issued_epoch BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS staging_operation_idempotency (
                operation_key BLOB PRIMARY KEY CHECK(length(operation_key) = 16),
                job_id TEXT NOT NULL REFERENCES coordinator_jobs(job_id),
                attempt_id TEXT NOT NULL REFERENCES coordinator_attempts(attempt_id),
                lease_id TEXT NOT NULL REFERENCES coordinator_leases(lease_id),
                request_payload BLOB NOT NULL,
                fence_epoch BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS coordinator_node_reservations (
                node_id TEXT PRIMARY KEY
                    REFERENCES coordinator_agent_inventory(node_id),
                job_id TEXT NOT NULL REFERENCES coordinator_jobs(job_id),
                attempt_id TEXT NOT NULL UNIQUE
                    REFERENCES coordinator_attempts(attempt_id)
                    DEFERRABLE INITIALLY DEFERRED,
                inventory_revision BLOB NOT NULL,
                reserved_at_unix_ms BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS coordinator_node_reservation_gpus (
                node_id TEXT NOT NULL
                    REFERENCES coordinator_node_reservations(node_id),
                gpu_id TEXT NOT NULL,
                ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
                PRIMARY KEY(node_id, ordinal),
                UNIQUE(node_id, gpu_id)
            );
            "#,
        )
        .map_err(map_sql_error)
}

impl CoordinatorStagingStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StagingStoreError> {
        let mut connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        initialize_schema(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.connection.path(), Some(path) if !path.is_empty() && path != ":memory:")
    }

    pub fn get_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<Option<StoredAttempt>, StagingStoreError> {
        fetch_attempt(&self.connection, attempt_id)
    }

    pub fn fence_epoch(&self) -> Result<Option<u64>, StagingStoreError> {
        read_counter(&self.connection)
    }

    pub fn get_node_reservation(
        &self,
        node_id: &str,
    ) -> Result<Option<StoredNodeReservation>, StagingStoreError> {
        fetch_node_reservation(&self.connection, node_id)
    }

    pub fn stage_queued_with_lease(
        &mut self,
        request: &StageQueuedRequest,
    ) -> Result<StageQueuedResult, StagingStoreError> {
        self.stage(request, None)
    }

    pub fn reserve_node_and_stage_queued_with_lease(
        &mut self,
        request: &StageQueuedRequest,
        expected_inventory_revision: u64,
    ) -> Result<ReservedStageResult, ReservedStageError> {
        self.reserve_and_stage(request, expected_inventory_revision, None)
    }

    fn stage(
        &mut self,
        request: &StageQueuedRequest,
        fault: Option<TestFault>,
    ) -> Result<StageQueuedResult, StagingStoreError> {
        validate_request(request)?;
        let request_payload = encode_request(request);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        if let Some(operation) = fetch_operation(&transaction, &request.operation_key)? {
            if operation.job_id != request.job_id
                || operation.attempt_id != request.attempt_id
                || operation.lease_id != request.lease_id
                || operation.request_payload != request_payload
            {
                return Err(StagingStoreError::OperationConflict);
            }
            let result = load_result(&transaction, request, operation.fence_epoch)?;
            transaction.commit().map_err(map_sql_error)?;
            return Ok(StageQueuedResult {
                created: false,
                ..result
            });
        }

        let result = stage_new_in_transaction(&transaction, request, &request_payload, fault)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(result)
    }

    fn reserve_and_stage(
        &mut self,
        request: &StageQueuedRequest,
        expected_inventory_revision: u64,
        fault: Option<TestFault>,
    ) -> Result<ReservedStageResult, ReservedStageError> {
        validate_request(request)?;
        validate_selected_gpu_ids(&request.selected_gpu_ids)?;
        let request_payload = encode_reserved_request(request, expected_inventory_revision);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        if let Some(operation) = fetch_operation(&transaction, &request.operation_key)? {
            if operation.job_id != request.job_id
                || operation.attempt_id != request.attempt_id
                || operation.lease_id != request.lease_id
                || operation.request_payload != request_payload
            {
                return Err(StagingStoreError::OperationConflict.into());
            }
            let stage = load_result(&transaction, request, operation.fence_epoch)?;
            let reservation =
                fetch_node_reservation(&transaction, &request.node_id)?.ok_or_else(|| {
                    StagingStoreError::CorruptData(
                        "reserved staging operation points to missing reservation".into(),
                    )
                })?;
            validate_replayed_reservation(&reservation, request, expected_inventory_revision)?;
            transaction.commit().map_err(map_sql_error)?;
            return Ok(ReservedStageResult {
                reservation,
                stage: StageQueuedResult {
                    created: false,
                    ..stage
                },
            });
        }

        let actual_inventory_revision = fetch_inventory_revision(&transaction, &request.node_id)?
            .ok_or_else(|| ReservedStageError::InventoryMissing {
            node_id: request.node_id.clone(),
        })?;
        if actual_inventory_revision != expected_inventory_revision {
            return Err(ReservedStageError::InventoryRevisionMismatch {
                node_id: request.node_id.clone(),
                expected: expected_inventory_revision,
                actual: actual_inventory_revision,
            });
        }
        validate_selected_gpus_exist(&transaction, &request.node_id, &request.selected_gpu_ids)?;
        if let Some(existing) = fetch_node_reservation(&transaction, &request.node_id)? {
            return Err(ReservedStageError::NodeAlreadyReserved {
                node_id: existing.node_id,
                owning_job_id: existing.job_id,
                owning_attempt_id: existing.attempt_id,
            });
        }

        let reservation = StoredNodeReservation {
            node_id: request.node_id.clone(),
            job_id: request.job_id.clone(),
            attempt_id: request.attempt_id.clone(),
            inventory_revision: expected_inventory_revision,
            reserved_at_unix_ms: request.issued_at_unix_ms,
            selected_gpu_ids: request.selected_gpu_ids.clone(),
        };
        insert_node_reservation(&transaction, &reservation)?;
        fail_at(fault, TestFault::AfterReservationInsert)?;
        insert_gpu_binding(&transaction, &reservation)?;
        fail_at(fault, TestFault::AfterGpuBindingInsert)?;
        let stage = stage_new_in_transaction(&transaction, request, &request_payload, fault)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(ReservedStageResult { reservation, stage })
    }
}

fn stage_new_in_transaction(
    connection: &Connection,
    request: &StageQueuedRequest,
    request_payload: &[u8],
    fault: Option<TestFault>,
) -> Result<StageQueuedResult, StagingStoreError> {
    let mut job = job_store::fetch_job(connection, &request.job_id)
        .map_err(map_job_error)?
        .ok_or(StagingStoreError::JobNotFound)?;
    if job.state != JobState::Queued {
        return Err(StagingStoreError::JobNotQueued(job.state));
    }
    let queued_at = job.queued_at_unix_ms.ok_or_else(|| {
        StagingStoreError::CorruptData("QUEUED Job has no queued timestamp".into())
    })?;
    if request.issued_at_unix_ms < queued_at {
        return Err(StagingStoreError::ClockRollback {
            issued_at_unix_ms: request.issued_at_unix_ms,
            queued_at_unix_ms: queued_at,
        });
    }
    if fetch_attempt(connection, &request.attempt_id)?.is_some() {
        return Err(StagingStoreError::AttemptIdConflict(
            request.attempt_id.clone(),
        ));
    }
    if lease_store::fetch_lease(connection, &request.lease_id)
        .map_err(map_lease_error)?
        .is_some()
    {
        return Err(StagingStoreError::LeaseIdConflict(request.lease_id.clone()));
    }

    let epoch = allocate_epoch(connection)?;
    let attempt = StoredAttempt {
        attempt_id: request.attempt_id.clone(),
        job_id: request.job_id.clone(),
        state: AttemptState::Created,
        node_ids: vec![request.node_id.clone()],
        fence_epoch: epoch,
        lease_id: request.lease_id.clone(),
        created_at_unix_ms: request.issued_at_unix_ms,
        revision: 0,
    };
    insert_attempt(connection, &attempt)?;
    fail_at(fault, TestFault::AfterAttemptInsert)?;

    let lease = request.to_lease(epoch);
    lease_store::insert_lease(connection, &lease).map_err(map_lease_error)?;
    fail_at(fault, TestFault::AfterLeaseInsert)?;
    fail_at(fault, TestFault::BeforeJobUpdate)?;

    job.state = JobState::Staging;
    job.staging_at_unix_ms = Some(request.issued_at_unix_ms);
    job.revision = job
        .revision
        .checked_add(1)
        .ok_or_else(|| StagingStoreError::CorruptData("job revision overflow".into()))?;
    job_store::update_job(connection, &job).map_err(map_job_error)?;
    insert_operation(connection, request, request_payload, epoch)?;
    Ok(StageQueuedResult {
        job,
        attempt,
        lease,
        created: true,
    })
}

impl StageQueuedRequest {
    fn to_lease(&self, fence_epoch: u64) -> StoredLease {
        StoredLease {
            lease_id: self.lease_id.clone(),
            job_id: self.job_id.clone(),
            attempt_id: self.attempt_id.clone(),
            holder_node_id: self.node_id.clone(),
            fence_epoch,
            expires_at_unix_ms: self.expires_at_unix_ms,
            issuing_coordinator_id: self.issuing_coordinator_id.clone(),
            coordinator_term: self.coordinator_term,
            issued_at_unix_ms: self.issued_at_unix_ms,
            renew_after_unix_ms: self.renew_after_unix_ms,
            max_total_duration_seconds: self.max_total_duration_seconds,
            revoked_at_unix_ms: None,
        }
    }
}

fn validate_request(request: &StageQueuedRequest) -> Result<(), StagingStoreError> {
    for (name, value) in [
        ("job_id", &request.job_id),
        ("attempt_id", &request.attempt_id),
        ("lease_id", &request.lease_id),
        ("node_id", &request.node_id),
        ("issuing_coordinator_id", &request.issuing_coordinator_id),
    ] {
        if value.trim().is_empty() {
            return Err(StagingStoreError::InvalidInput(name));
        }
    }
    let max_duration_ms = request
        .max_total_duration_seconds
        .checked_mul(1_000)
        .ok_or(StagingStoreError::InvalidLeaseLifetime)?;
    let lifetime_ms = request
        .expires_at_unix_ms
        .checked_sub(request.issued_at_unix_ms)
        .ok_or(StagingStoreError::InvalidLeaseLifetime)?;
    if request.max_total_duration_seconds == 0
        || request.renew_after_unix_ms <= request.issued_at_unix_ms
        || request.renew_after_unix_ms >= request.expires_at_unix_ms
        || lifetime_ms > max_duration_ms
    {
        return Err(StagingStoreError::InvalidLeaseLifetime);
    }
    Ok(())
}

fn validate_selected_gpu_ids(selected_gpu_ids: &[String]) -> Result<(), StagingStoreError> {
    if selected_gpu_ids.is_empty() {
        return Err(StagingStoreError::InvalidInput("selected_gpu_ids"));
    }
    for gpu_id in selected_gpu_ids {
        if gpu_id.trim().is_empty() {
            return Err(StagingStoreError::InvalidInput(
                "selected_gpu_ids blank gpu_id",
            ));
        }
    }
    for pair in selected_gpu_ids.windows(2) {
        if pair[0] == pair[1] {
            return Err(StagingStoreError::InvalidInput(
                "selected_gpu_ids duplicate gpu_id",
            ));
        }
        if pair[0] > pair[1] {
            return Err(StagingStoreError::InvalidInput(
                "selected_gpu_ids canonical order",
            ));
        }
    }
    Ok(())
}

fn allocate_epoch(connection: &Connection) -> Result<u64, StagingStoreError> {
    let mut base = read_counter(connection)?.unwrap_or(0);
    let mut statement = connection
        .prepare("SELECT fence_epoch FROM coordinator_leases")
        .map_err(map_sql_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(map_sql_error)?;
    for row in rows {
        base = base.max(decode_u64(
            &row.map_err(map_sql_error)?,
            "lease fence_epoch",
        )?);
    }
    let epoch = base
        .checked_add(1)
        .ok_or(StagingStoreError::FenceEpochOverflow)?;
    connection
        .execute(
            "INSERT INTO coordinator_fence_state(singleton, max_issued_epoch) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET max_issued_epoch = excluded.max_issued_epoch",
            rusqlite::params![encode_u64(epoch)],
        )
        .map_err(map_sql_error)?;
    Ok(epoch)
}

fn read_counter(connection: &Connection) -> Result<Option<u64>, StagingStoreError> {
    connection
        .query_row(
            "SELECT max_issued_epoch FROM coordinator_fence_state WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(map_sql_error)?
        .map(|bytes| decode_u64(&bytes, "max_issued_epoch"))
        .transpose()
}

fn insert_attempt(
    connection: &Connection,
    attempt: &StoredAttempt,
) -> Result<(), StagingStoreError> {
    connection
        .execute(
            "INSERT INTO coordinator_attempts(
                attempt_id, job_id, state, fence_epoch, lease_id, created_at_unix_ms, revision
             ) VALUES (?1, ?2, ?7, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                attempt.attempt_id,
                attempt.job_id,
                encode_u64(attempt.fence_epoch),
                attempt.lease_id,
                encode_u64(attempt.created_at_unix_ms),
                encode_u64(attempt.revision),
                state_to_db(attempt.state),
            ],
        )
        .map_err(map_sql_error)?;
    connection
        .execute(
            "INSERT INTO coordinator_attempt_nodes(attempt_id, node_id, ordinal) VALUES (?1, ?2, 0)",
            rusqlite::params![attempt.attempt_id, attempt.node_ids[0]],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

/// Attempt 상태를 `from` 에서 `to` 로 옮긴다 — **규범 검증 + CAS**.
///
/// ★ 두 가지를 같이 한다. 하나라도 빠지면 조용히 틀린다:
///   1. `transition()` 이 규범 표(`state-machines.md` §3)에 있는 전이인지 본다.
///      표에 없는 전이는 **여기서** 막는다 — 저장소가 규범 밖 상태를 만들지 못하게.
///   2. `WHERE state = <읽은 값>` 으로만 쓴다. 읽고 쓰는 사이에 다른 쓰기가
///      끼어들었으면 0행이 바뀌고, 그것을 **경쟁으로 보고한다**(덮어쓰지 않는다).
///
/// 호출자는 같은 트랜잭션 안에서 부른다 — 보고 저장과 상태 변경이 갈라지면
/// "보고는 있는데 상태는 CREATED" 인 행이 다시 생긴다(§A1 4c 가 그 상태였다).
pub(crate) fn transition_attempt_state(
    connection: &Connection,
    attempt_id: &str,
    from: AttemptState,
    to: AttemptState,
) -> Result<(), StagingStoreError> {
    transition_attempt_state_along(connection, attempt_id, from, &[to])
}

/// 여러 칸짜리 경로를 **메모리에서 규범으로 검증**하고, DB 에는 **마지막 상태만** 쓴다.
///
/// ★★ 왜 중간 상태를 DB 에 안 쓰나(코덱스 72 권고 C) — 중간 상태를 쓰면
///   "그 순간 그 상태였다" 고 말하는 셈인데, 우리는 그것을 **관측하지 않았다.**
///   한 트랜잭션 안이라 관측 순간도 따로 없다. 경로는 "규범 안의 이야기인가" 를
///   확인하는 데만 쓰고, 남기는 사실은 최종 상태 하나다.
pub(crate) fn transition_attempt_state_along(
    connection: &Connection,
    attempt_id: &str,
    from: AttemptState,
    path: &[AttemptState],
) -> Result<(), StagingStoreError> {
    let mut current = from;
    for step in path {
        current = gputeer_protocol::attempt_state::transition(current, *step).map_err(|_| {
            StagingStoreError::AttemptTransitionRejected {
                from: current,
                to: *step,
            }
        })?;
    }
    let to = current;
    let changed = connection
        .execute(
            "UPDATE coordinator_attempts SET state = ?1 WHERE attempt_id = ?2 AND state = ?3",
            rusqlite::params![state_to_db(to), attempt_id, state_to_db(from)],
        )
        .map_err(map_sql_error)?;
    if changed != 1 {
        return Err(StagingStoreError::AttemptStateRaced { expected: from });
    }
    Ok(())
}

pub(crate) fn fetch_attempt(
    connection: &Connection,
    attempt_id: &str,
) -> Result<Option<StoredAttempt>, StagingStoreError> {
    let raw = connection
        .query_row(
            "SELECT attempt_id, job_id, state, fence_epoch, lease_id, created_at_unix_ms, revision
             FROM coordinator_attempts WHERE attempt_id = ?1",
            rusqlite::params![attempt_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some((attempt_id, job_id, state, epoch, lease_id, created_at, revision)) = raw else {
        return Ok(None);
    };
    // ★ 2026-09-22 (§A1 4c) — 전에는 'CREATED' 가 아니면 손상으로 거부하고
    //   반환값도 무조건 Created 였다. 그래서 종료 상태를 **적을 수도 읽을 수도** 없었다.
    let state = state_from_db(&state)?;
    let mut statement = connection
        .prepare("SELECT node_id, ordinal FROM coordinator_attempt_nodes WHERE attempt_id = ?1 ORDER BY ordinal")
        .map_err(map_sql_error)?;
    let nodes = statement
        .query_map(rusqlite::params![attempt_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)?;
    if nodes.len() != 1 || nodes[0].1 != 0 || nodes[0].0.trim().is_empty() {
        return Err(StagingStoreError::CorruptData(
            "single-node Attempt row shape is invalid".into(),
        ));
    }
    Ok(Some(StoredAttempt {
        attempt_id,
        job_id,
        state,
        node_ids: vec![nodes[0].0.clone()],
        fence_epoch: decode_u64(&epoch, "Attempt fence_epoch")?,
        lease_id,
        created_at_unix_ms: decode_u64(&created_at, "Attempt created_at_unix_ms")?,
        revision: decode_u64(&revision, "Attempt revision")?,
    }))
}

#[derive(Debug)]
struct StoredOperation {
    job_id: String,
    attempt_id: String,
    lease_id: String,
    request_payload: Vec<u8>,
    fence_epoch: u64,
}

fn fetch_operation(
    connection: &Connection,
    key: &[u8; 16],
) -> Result<Option<StoredOperation>, StagingStoreError> {
    connection
        .query_row(
            "SELECT job_id, attempt_id, lease_id, request_payload, fence_epoch
             FROM staging_operation_idempotency WHERE operation_key = ?1",
            rusqlite::params![key.as_slice()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?
        .map(|(job_id, attempt_id, lease_id, request_payload, epoch)| {
            Ok(StoredOperation {
                job_id,
                attempt_id,
                lease_id,
                request_payload,
                fence_epoch: decode_u64(&epoch, "operation fence_epoch")?,
            })
        })
        .transpose()
}

fn insert_operation(
    connection: &Connection,
    request: &StageQueuedRequest,
    request_payload: &[u8],
    epoch: u64,
) -> Result<(), StagingStoreError> {
    connection
        .execute(
            "INSERT INTO staging_operation_idempotency(
            operation_key, job_id, attempt_id, lease_id, request_payload, fence_epoch
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                request.operation_key.as_slice(),
                request.job_id,
                request.attempt_id,
                request.lease_id,
                request_payload,
                encode_u64(epoch)
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

fn encode_request(request: &StageQueuedRequest) -> Vec<u8> {
    let mut payload = Vec::new();
    for value in [
        &request.job_id,
        &request.attempt_id,
        &request.lease_id,
        &request.node_id,
        &request.issuing_coordinator_id,
    ] {
        payload.extend_from_slice(&(value.len() as u64).to_be_bytes());
        payload.extend_from_slice(value.as_bytes());
    }
    for value in [
        request.coordinator_term,
        request.issued_at_unix_ms,
        request.renew_after_unix_ms,
        request.expires_at_unix_ms,
        request.max_total_duration_seconds,
    ] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload
}

fn encode_reserved_request(
    request: &StageQueuedRequest,
    expected_inventory_revision: u64,
) -> Vec<u8> {
    let mut payload = encode_request(request);
    payload.extend_from_slice(&expected_inventory_revision.to_be_bytes());
    payload.extend_from_slice(&(request.selected_gpu_ids.len() as u64).to_be_bytes());
    for gpu_id in &request.selected_gpu_ids {
        payload.extend_from_slice(&(gpu_id.len() as u64).to_be_bytes());
        payload.extend_from_slice(gpu_id.as_bytes());
    }
    payload
}

fn fetch_inventory_revision(
    connection: &Connection,
    node_id: &str,
) -> Result<Option<u64>, StagingStoreError> {
    connection
        .query_row(
            "SELECT inventory_revision FROM coordinator_agent_inventory WHERE node_id = ?1",
            rusqlite::params![node_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(map_sql_error)?
        .map(|bytes| decode_u64(&bytes, "inventory_revision"))
        .transpose()
}

fn validate_selected_gpus_exist(
    connection: &Connection,
    node_id: &str,
    selected_gpu_ids: &[String],
) -> Result<(), ReservedStageError> {
    for gpu_id in selected_gpu_ids {
        let exists = connection
            .query_row(
                "SELECT 1 FROM coordinator_agent_gpus WHERE node_id = ?1 AND gpu_id = ?2",
                rusqlite::params![node_id, gpu_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_sql_error)?
            .is_some();
        if !exists {
            return Err(ReservedStageError::SelectedGpuMissing {
                node_id: node_id.to_owned(),
                gpu_id: gpu_id.clone(),
            });
        }
    }
    Ok(())
}

pub(crate) fn fetch_node_reservation(
    connection: &Connection,
    node_id: &str,
) -> Result<Option<StoredNodeReservation>, StagingStoreError> {
    let raw = connection
        .query_row(
            "SELECT node_id, job_id, attempt_id, inventory_revision, reserved_at_unix_ms
             FROM coordinator_node_reservations WHERE node_id = ?1",
            rusqlite::params![node_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some((node_id, job_id, attempt_id, revision, reserved_at)) = raw else {
        return Ok(None);
    };
    if node_id.trim().is_empty() || job_id.trim().is_empty() || attempt_id.trim().is_empty() {
        return Err(StagingStoreError::CorruptData(
            "node reservation owner identity is blank".into(),
        ));
    }
    let selected_gpu_ids = fetch_gpu_binding(connection, &node_id)?;
    Ok(Some(StoredNodeReservation {
        node_id,
        job_id,
        attempt_id,
        inventory_revision: decode_u64(&revision, "reservation inventory_revision")?,
        reserved_at_unix_ms: decode_u64(&reserved_at, "reservation reserved_at_unix_ms")?,
        selected_gpu_ids,
    }))
}

fn fetch_gpu_binding(
    connection: &Connection,
    node_id: &str,
) -> Result<Vec<String>, StagingStoreError> {
    let mut statement = connection
        .prepare(
            "SELECT gpu_id, ordinal FROM coordinator_node_reservation_gpus
             WHERE node_id = ?1 ORDER BY ordinal",
        )
        .map_err(map_sql_error)?;
    let rows = statement
        .query_map(rusqlite::params![node_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)?;
    if rows.is_empty() {
        return Err(StagingStoreError::CorruptData(
            "node reservation has no selected GPU binding".into(),
        ));
    }
    let mut selected_gpu_ids = Vec::with_capacity(rows.len());
    for (expected_ordinal, (gpu_id, ordinal)) in rows.into_iter().enumerate() {
        if ordinal != expected_ordinal as i64 {
            return Err(StagingStoreError::CorruptData(
                "node reservation GPU ordinals are not contiguous".into(),
            ));
        }
        selected_gpu_ids.push(gpu_id);
    }
    validate_selected_gpu_ids(&selected_gpu_ids).map_err(|_| {
        StagingStoreError::CorruptData(
            "node reservation selected GPU binding is not canonical".into(),
        )
    })?;
    Ok(selected_gpu_ids)
}

fn insert_node_reservation(
    connection: &Connection,
    reservation: &StoredNodeReservation,
) -> Result<(), StagingStoreError> {
    connection
        .execute(
            "INSERT INTO coordinator_node_reservations(
                node_id, job_id, attempt_id, inventory_revision, reserved_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                reservation.node_id,
                reservation.job_id,
                reservation.attempt_id,
                encode_u64(reservation.inventory_revision),
                encode_u64(reservation.reserved_at_unix_ms),
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

fn insert_gpu_binding(
    connection: &Connection,
    reservation: &StoredNodeReservation,
) -> Result<(), StagingStoreError> {
    for (ordinal, gpu_id) in reservation.selected_gpu_ids.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO coordinator_node_reservation_gpus(node_id, gpu_id, ordinal)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![reservation.node_id, gpu_id, ordinal as i64],
            )
            .map_err(map_sql_error)?;
    }
    Ok(())
}

fn validate_replayed_reservation(
    reservation: &StoredNodeReservation,
    request: &StageQueuedRequest,
    expected_inventory_revision: u64,
) -> Result<(), StagingStoreError> {
    if reservation.node_id != request.node_id
        || reservation.job_id != request.job_id
        || reservation.attempt_id != request.attempt_id
        || reservation.inventory_revision != expected_inventory_revision
        || reservation.reserved_at_unix_ms != request.issued_at_unix_ms
        || reservation.selected_gpu_ids != request.selected_gpu_ids
    {
        return Err(StagingStoreError::CorruptData(
            "reserved staging operation result is inconsistent".into(),
        ));
    }
    Ok(())
}

fn load_result(
    connection: &Connection,
    request: &StageQueuedRequest,
    epoch: u64,
) -> Result<StageQueuedResult, StagingStoreError> {
    let job = job_store::fetch_job(connection, &request.job_id)
        .map_err(map_job_error)?
        .ok_or_else(|| StagingStoreError::CorruptData("operation points to missing Job".into()))?;
    let attempt = fetch_attempt(connection, &request.attempt_id)?.ok_or_else(|| {
        StagingStoreError::CorruptData("operation points to missing Attempt".into())
    })?;
    let current_lease = lease_store::fetch_lease(connection, &request.lease_id)
        .map_err(map_lease_error)?
        .ok_or_else(|| {
            StagingStoreError::CorruptData("operation points to missing Lease".into())
        })?;
    let original_lease = request.to_lease(epoch);
    if job.state != JobState::Staging
        || job.staging_at_unix_ms != Some(request.issued_at_unix_ms)
        || attempt.job_id != request.job_id
        || attempt.lease_id != request.lease_id
        || attempt.node_ids != [request.node_id.clone()]
        || attempt.fence_epoch != epoch
        || attempt.created_at_unix_ms != request.issued_at_unix_ms
        || current_lease.lease_id != original_lease.lease_id
        || current_lease.job_id != original_lease.job_id
        || current_lease.attempt_id != original_lease.attempt_id
        || current_lease.holder_node_id != original_lease.holder_node_id
        || current_lease.fence_epoch != original_lease.fence_epoch
        || current_lease.issuing_coordinator_id != original_lease.issuing_coordinator_id
        || current_lease.coordinator_term != original_lease.coordinator_term
        || current_lease.issued_at_unix_ms != original_lease.issued_at_unix_ms
        || current_lease.max_total_duration_seconds != original_lease.max_total_duration_seconds
        || read_counter(connection)?.is_none_or(|counter| counter < epoch)
    {
        return Err(StagingStoreError::CorruptData(
            "staging operation result is inconsistent".into(),
        ));
    }
    Ok(StageQueuedResult {
        job,
        attempt,
        lease: original_lease,
        created: false,
    })
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8], field: &str) -> Result<u64, StagingStoreError> {
    let value: [u8; 8] = bytes.try_into().map_err(|_| {
        StagingStoreError::CorruptData(format!("{field} must contain exactly 8 bytes"))
    })?;
    Ok(u64::from_be_bytes(value))
}

fn map_sql_error(error: SqlError) -> StagingStoreError {
    match error {
        SqlError::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            StagingStoreError::LockTimeout
        }
        SqlError::SqliteFailure(code, _) => StagingStoreError::Io(code.to_string()),
        other => StagingStoreError::Io(other.to_string()),
    }
}

fn map_job_error(error: job_store::JobStoreError) -> StagingStoreError {
    match error {
        job_store::JobStoreError::LockTimeout => StagingStoreError::LockTimeout,
        job_store::JobStoreError::CorruptData(message) => StagingStoreError::CorruptData(message),
        other => StagingStoreError::Io(other.to_string()),
    }
}

fn map_lease_error(error: lease_store::LeaseStoreError) -> StagingStoreError {
    match error {
        lease_store::LeaseStoreError::LockTimeout => StagingStoreError::LockTimeout,
        lease_store::LeaseStoreError::Io(message) => StagingStoreError::CorruptData(message),
        other => StagingStoreError::Io(other.to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFault {
    AfterReservationInsert,
    AfterGpuBindingInsert,
    AfterAttemptInsert,
    AfterLeaseInsert,
    BeforeJobUpdate,
}

#[cfg(test)]
fn fail_at(fault: Option<TestFault>, point: TestFault) -> Result<(), StagingStoreError> {
    if fault == Some(point) {
        let name = match point {
            TestFault::AfterReservationInsert => "after node reservation insert",
            TestFault::AfterGpuBindingInsert => "after selected GPU binding insert",
            TestFault::AfterAttemptInsert => "after Attempt insert",
            TestFault::AfterLeaseInsert => "after Lease insert",
            TestFault::BeforeJobUpdate => "before Job update",
        };
        return Err(StagingStoreError::InjectedFailure(name));
    }
    Ok(())
}

#[cfg(not(test))]
fn fail_at(_fault: Option<TestFault>, _point: TestFault) -> Result<(), StagingStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::*;
    use crate::inventory_store::{
        AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory,
    };
    use crate::job_store::{AcceptedJobSubmission, CoordinatorJobStore};
    use crate::lease_store::CoordinatorLeaseStore;

    fn prepare_queued(path: &Path, job_id: &str, key: u8) {
        let mut store = CoordinatorJobStore::open(path).unwrap();
        store
            .submit_accepted(
                &AcceptedJobSubmission {
                    idempotency_key: [key; 16],
                    job_id: job_id.into(),
                    submitter_device_id: "submitter-1".into(),
                    manifest_hash: [key; 32],
                    deadline_unix_ms: Some(10_000),
                    max_queue_duration_ms: Some(5_000),
                },
                100,
            )
            .unwrap();
        store.start_planning(job_id, 110).unwrap();
        store.enqueue(job_id, "plan-1", 120).unwrap();
    }

    fn prepare_inventory(path: &Path, node_id: &str, revision: u64, seed: u8) {
        prepare_inventory_gpus(path, node_id, revision, seed, &[format!("gpu-{node_id}")]);
    }

    fn prepare_inventory_gpus(
        path: &Path,
        node_id: &str,
        revision: u64,
        seed: u8,
        gpu_ids: &[String],
    ) {
        let mut store = CoordinatorInventoryStore::open(path).unwrap();
        if store.get_agent(node_id).unwrap().is_none() {
            store
                .register_agent(&AgentRegistry {
                    node_id: node_id.into(),
                    device_id: format!("device-{seed}"),
                    owner_member_id: "owner-1".into(),
                    verifying_key: vec![seed; 32],
                    node_state: None,
                    risk_state: None,
                    security_tier: None,
                    isolation_class: None,
                    key_protection: None,
                })
                .unwrap();
        }
        store
            .update_inventory(&AgentInventory {
                node_id: node_id.into(),
                inventory_revision: revision,
                observed_at_unix_ms: revision,
                gpus: Some(
                    gpu_ids
                        .iter()
                        .map(|gpu_id| GpuInventory {
                            gpu_id: gpu_id.clone(),
                            model: Some("model-a".into()),
                            healthy: Some(true),
                            available_vram_bytes: Some(16),
                        })
                        .collect(),
                ),
                available_cpu_cores: Some(8),
                available_ram_bytes: Some(64),
                available_workspace_bytes: Some(64),
                allowed_workload_classes: None,
                third_party_workloads_opt_in: None,
            })
            .unwrap();
    }

    fn request(job_id: &str, operation: u8) -> StageQueuedRequest {
        StageQueuedRequest {
            operation_key: [operation; 16],
            job_id: job_id.into(),
            attempt_id: format!("attempt-{operation}"),
            lease_id: format!("lease-{operation}"),
            node_id: "node-1".into(),
            selected_gpu_ids: vec!["gpu-node-1".into()],
            issuing_coordinator_id: "coordinator-1".into(),
            coordinator_term: 7,
            issued_at_unix_ms: 200,
            renew_after_unix_ms: 500,
            expires_at_unix_ms: 900,
            max_total_duration_seconds: 1,
        }
    }

    fn assert_queued_and_no_side_effects(
        store: &CoordinatorStagingStore,
        request: &StageQueuedRequest,
    ) {
        let job = job_store::fetch_job(&store.connection, &request.job_id)
            .unwrap()
            .unwrap();
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.revision, 2);
        assert_eq!(job.staging_at_unix_ms, None);
        assert_eq!(store.get_attempt(&request.attempt_id).unwrap(), None);
        assert_eq!(
            lease_store::fetch_lease(&store.connection, &request.lease_id).unwrap(),
            None
        );
        assert_eq!(store.fence_epoch().unwrap(), None);
        assert!(fetch_operation(&store.connection, &request.operation_key)
            .unwrap()
            .is_none());
        assert_eq!(store.get_node_reservation(&request.node_id).unwrap(), None);
    }

    /// 상태를 **읽은 값이 그대로일 때만** 쓴다(CAS).
    ///
    /// ★★ 이 시험이 있는 이유 — 2026-09-22 뮤테이션에서 `WHERE state = <읽은 값>` 을
    ///   빼도 다른 시험이 **하나도 실패하지 않았다.** 장치가 있었는데 아무도 지키지
    ///   않았다는 뜻이다(`CLAUDE.md` §4 — 뮤테이션이 안 잡히면 테스트가 약한 것이다).
    ///
    ///   스레드 없이 결정적으로 만든다: 같은 `from` 으로 **두 번** 부른다.
    ///   두 번째 호출은 이미 낡은 값을 들고 있으므로 거부돼야 한다.
    #[test]
    fn a_state_write_with_a_stale_source_state_is_refused_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-cas", 1);
        prepare_inventory(&path, "node-1", 7, 1);
        let revision = {
            let mut inventory = CoordinatorInventoryStore::open(&path).unwrap();
            inventory.pool_snapshot(7).unwrap().candidates[0]
                .inventory_revision
                .unwrap()
        };
        let staged = {
            let mut store = CoordinatorStagingStore::open(&path).unwrap();
            let request = request("job-cas", 1);
            store
                .reserve_node_and_stage_queued_with_lease(&request, revision)
                .unwrap();
            request.attempt_id.clone()
        };

        let connection = Connection::open(&path).unwrap();
        // 첫 번째 — 읽은 값(Created)이 아직 그대로다. 통과한다.
        transition_attempt_state(
            &connection,
            &staged,
            AttemptState::Created,
            AttemptState::Starting,
        )
        .expect("첫 전이");
        // 두 번째 — 여전히 Created 를 들고 있다. 그 사이 상태가 바뀌었으므로 **거부**다.
        let error = transition_attempt_state(
            &connection,
            &staged,
            AttemptState::Created,
            AttemptState::Starting,
        )
        .expect_err("낡은 값으로 덮어썼다 — CAS 가 없는 것이다");
        assert_eq!(
            error,
            StagingStoreError::AttemptStateRaced {
                expected: AttemptState::Created
            }
        );

        // 그리고 상태는 첫 전이 결과 그대로여야 한다.
        let store = CoordinatorStagingStore::open(&path).unwrap();
        assert_eq!(
            store.get_attempt(&staged).unwrap().unwrap().state,
            AttemptState::Starting
        );
    }

    #[test]
    fn stale_or_missing_inventory_fails_before_any_staging_side_effect() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-stale", 1);
        prepare_queued(&path, "job-missing", 2);
        prepare_inventory(&path, "node-1", 7, 1);
        let expected = {
            let mut inventory = CoordinatorInventoryStore::open(&path).unwrap();
            inventory.pool_snapshot(7).unwrap().candidates[0]
                .inventory_revision
                .unwrap()
        };
        prepare_inventory(&path, "node-1", 8, 1);
        let mut store = CoordinatorStagingStore::open(&path).unwrap();

        let stale = request("job-stale", 1);
        assert_eq!(
            store.reserve_node_and_stage_queued_with_lease(&stale, expected),
            Err(ReservedStageError::InventoryRevisionMismatch {
                node_id: "node-1".into(),
                expected: 7,
                actual: 8,
            })
        );
        assert_queued_and_no_side_effects(&store, &stale);

        let mut missing = request("job-missing", 2);
        missing.node_id = "node-missing".into();
        assert_eq!(
            store.reserve_node_and_stage_queued_with_lease(&missing, 1),
            Err(ReservedStageError::InventoryMissing {
                node_id: "node-missing".into(),
            })
        );
        assert_queued_and_no_side_effects(&store, &missing);
    }

    #[test]
    fn sequential_distinct_jobs_cannot_reserve_the_same_node_gpu() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        prepare_queued(&path, "job-2", 2);
        prepare_inventory(&path, "node-1", 5, 1);
        let expected = {
            let mut inventory = CoordinatorInventoryStore::open(&path).unwrap();
            inventory.pool_snapshot(5).unwrap().candidates[0]
                .inventory_revision
                .unwrap()
        };
        let first = request("job-1", 1);
        let second = request("job-2", 2);
        let mut store = CoordinatorStagingStore::open(&path).unwrap();

        let winner = store
            .reserve_node_and_stage_queued_with_lease(&first, expected)
            .unwrap();
        assert_eq!(winner.reservation.job_id, "job-1");
        assert_eq!(
            store.reserve_node_and_stage_queued_with_lease(&second, expected),
            Err(ReservedStageError::NodeAlreadyReserved {
                node_id: "node-1".into(),
                owning_job_id: "job-1".into(),
                owning_attempt_id: "attempt-1".into(),
            })
        );
        assert_eq!(
            job_store::fetch_job(&store.connection, "job-2")
                .unwrap()
                .unwrap()
                .state,
            JobState::Queued
        );
        assert_eq!(store.get_attempt("attempt-2").unwrap(), None);
        assert_eq!(
            lease_store::fetch_lease(&store.connection, "lease-2").unwrap(),
            None
        );
        assert_eq!(store.fence_epoch().unwrap(), Some(1));
    }

    #[test]
    fn concurrent_distinct_jobs_reserving_one_gpu_commit_exactly_once() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        prepare_queued(&path, "job-2", 2);
        prepare_inventory(&path, "node-1", 5, 1);
        let stores = [
            CoordinatorStagingStore::open(&path).unwrap(),
            CoordinatorStagingStore::open(&path).unwrap(),
        ];
        let barrier = Arc::new(Barrier::new(2));
        let handles = stores
            .into_iter()
            .enumerate()
            .map(|(index, mut store)| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let request = request(&format!("job-{}", index + 1), index as u8 + 1);
                    barrier.wait();
                    store.reserve_node_and_stage_queued_with_lease(&request, 5)
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(ReservedStageError::NodeAlreadyReserved { .. })
                ))
                .count(),
            1
        );
        let winner = results
            .iter()
            .find_map(|result| result.as_ref().ok())
            .unwrap();
        let loser_job = if winner.reservation.job_id == "job-1" {
            "job-2"
        } else {
            "job-1"
        };
        let store = CoordinatorStagingStore::open(&path).unwrap();
        assert_eq!(
            store.get_node_reservation("node-1").unwrap(),
            Some(winner.reservation.clone())
        );
        assert_eq!(
            job_store::fetch_job(&store.connection, loser_job)
                .unwrap()
                .unwrap()
                .state,
            JobState::Queued
        );
        for table in [
            "coordinator_node_reservations",
            "coordinator_node_reservation_gpus",
            "coordinator_attempts",
            "coordinator_leases",
            "staging_operation_idempotency",
        ] {
            let count: u64 = store
                .connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 1, "loser must leave no row in {table}");
        }
        assert_eq!(store.fence_epoch().unwrap(), Some(1));
    }

    #[test]
    fn distinct_nodes_do_not_share_a_global_reservation_lock() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        prepare_queued(&path, "job-2", 2);
        prepare_inventory(&path, "node-1", 5, 1);
        prepare_inventory(&path, "node-2", u64::MAX, 2);
        let mut store = CoordinatorStagingStore::open(&path).unwrap();
        let first = store
            .reserve_node_and_stage_queued_with_lease(&request("job-1", 1), 5)
            .unwrap();
        let mut second_request = request("job-2", 2);
        second_request.node_id = "node-2".into();
        second_request.selected_gpu_ids = vec!["gpu-node-2".into()];
        let second = store
            .reserve_node_and_stage_queued_with_lease(&second_request, u64::MAX)
            .unwrap();

        assert_eq!(first.reservation.node_id, "node-1");
        assert_eq!(second.reservation.node_id, "node-2");
        assert_eq!(second.reservation.inventory_revision, u64::MAX);
        assert_eq!(first.stage.lease.fence_epoch, 1);
        assert_eq!(second.stage.lease.fence_epoch, 2);
    }

    #[test]
    fn exact_reserved_replay_survives_inventory_refresh_but_changed_revision_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        prepare_inventory(&path, "node-1", 5, 1);
        let request = request("job-1", 1);
        let mut store = CoordinatorStagingStore::open(&path).unwrap();
        let first = store
            .reserve_node_and_stage_queued_with_lease(&request, 5)
            .unwrap();
        drop(store);
        prepare_inventory(&path, "node-1", 6, 1);
        let mut store = CoordinatorStagingStore::open(&path).unwrap();

        let replay = store
            .reserve_node_and_stage_queued_with_lease(&request, 5)
            .unwrap();
        assert!(!replay.stage.created);
        assert_eq!(replay.reservation, first.reservation);
        assert_eq!(
            replay.stage.lease.fence_epoch,
            first.stage.lease.fence_epoch
        );
        assert_eq!(
            store.reserve_node_and_stage_queued_with_lease(&request, 6),
            Err(ReservedStageError::Staging(
                StagingStoreError::OperationConflict
            ))
        );
        assert_eq!(store.fence_epoch().unwrap(), Some(1));
    }

    #[test]
    fn failure_after_reservation_insert_rolls_back_reservation_and_all_staging_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        prepare_inventory(&path, "node-1", 5, 1);
        let request = request("job-1", 1);
        let mut store = CoordinatorStagingStore::open(&path).unwrap();

        assert!(matches!(
            store.reserve_and_stage(&request, 5, Some(TestFault::AfterReservationInsert)),
            Err(ReservedStageError::Staging(
                StagingStoreError::InjectedFailure("after node reservation insert")
            ))
        ));
        assert_queued_and_no_side_effects(&store, &request);

        let result = store
            .reserve_node_and_stage_queued_with_lease(&request, 5)
            .unwrap();
        assert_eq!(result.stage.lease.fence_epoch, 1);
    }

    #[test]
    fn failure_after_gpu_binding_insert_rolls_back_every_staging_side_effect() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        prepare_inventory(&path, "node-1", 5, 1);
        let request = request("job-1", 1);
        let mut store = CoordinatorStagingStore::open(&path).unwrap();

        assert!(matches!(
            store.reserve_and_stage(&request, 5, Some(TestFault::AfterGpuBindingInsert)),
            Err(ReservedStageError::Staging(
                StagingStoreError::InjectedFailure("after selected GPU binding insert")
            ))
        ));
        assert_queued_and_no_side_effects(&store, &request);
        let binding_count: u64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM coordinator_node_reservation_gpus",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(binding_count, 0);

        let result = store
            .reserve_node_and_stage_queued_with_lease(&request, 5)
            .unwrap();
        assert_eq!(result.stage.lease.fence_epoch, 1);
        assert_eq!(result.reservation.selected_gpu_ids, ["gpu-node-1"]);
    }

    #[test]
    fn invalid_or_missing_selected_gpu_ids_fail_before_staging() {
        for (index, selected_gpu_ids) in [
            Vec::<String>::new(),
            vec![" ".into()],
            vec!["gpu-node-1".into(), "gpu-node-1".into()],
            vec!["gpu-z".into(), "gpu-a".into()],
        ]
        .into_iter()
        .enumerate()
        {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("control.sqlite3");
            prepare_queued(&path, "job-1", 1);
            prepare_inventory(&path, "node-1", 5, 1);
            let mut request = request("job-1", 1);
            request.selected_gpu_ids = selected_gpu_ids;
            let mut store = CoordinatorStagingStore::open(&path).unwrap();

            assert!(
                matches!(
                    store.reserve_node_and_stage_queued_with_lease(&request, 5),
                    Err(ReservedStageError::Staging(
                        StagingStoreError::InvalidInput(_)
                    ))
                ),
                "invalid selected GPU case {index} must fail closed"
            );
            assert_queued_and_no_side_effects(&store, &request);
        }

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-missing-gpu", 1);
        prepare_inventory(&path, "node-1", 5, 1);
        prepare_inventory_gpus(&path, "node-2", 5, 2, &["gpu-missing".into()]);
        let mut request = request("job-missing-gpu", 1);
        request.selected_gpu_ids = vec!["gpu-missing".into()];
        let mut store = CoordinatorStagingStore::open(&path).unwrap();
        assert_eq!(
            store.reserve_node_and_stage_queued_with_lease(&request, 5),
            Err(ReservedStageError::SelectedGpuMissing {
                node_id: "node-1".into(),
                gpu_id: "gpu-missing".into(),
            })
        );
        assert_queued_and_no_side_effects(&store, &request);
    }

    #[test]
    fn selected_gpu_binding_is_in_operation_payload_and_replay_returns_original() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        prepare_inventory_gpus(&path, "node-1", 5, 1, &["gpu-a".into(), "gpu-b".into()]);
        let mut request = request("job-1", 1);
        request.selected_gpu_ids = vec!["gpu-a".into()];
        let mut store = CoordinatorStagingStore::open(&path).unwrap();

        let first = store
            .reserve_node_and_stage_queued_with_lease(&request, 5)
            .unwrap();
        let replay = store
            .reserve_node_and_stage_queued_with_lease(&request, 5)
            .unwrap();
        assert!(!replay.stage.created);
        assert_eq!(replay.reservation, first.reservation);

        let mut changed = request.clone();
        changed.selected_gpu_ids = vec!["gpu-b".into()];
        assert_eq!(
            store.reserve_node_and_stage_queued_with_lease(&changed, 5),
            Err(ReservedStageError::Staging(
                StagingStoreError::OperationConflict
            ))
        );
        assert_eq!(
            store.get_node_reservation("node-1").unwrap(),
            Some(first.reservation)
        );
        assert_eq!(store.fence_epoch().unwrap(), Some(1));
    }

    #[test]
    fn missing_blank_noncanonical_or_gapped_stored_gpu_binding_fails_closed() {
        for corruption in ["missing", "blank", "noncanonical", "gapped"] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("control.sqlite3");
            prepare_queued(&path, "job-1", 1);
            prepare_inventory_gpus(&path, "node-1", 5, 1, &["gpu-a".into(), "gpu-b".into()]);
            let mut request = request("job-1", 1);
            request.selected_gpu_ids = vec!["gpu-a".into(), "gpu-b".into()];
            let mut store = CoordinatorStagingStore::open(&path).unwrap();
            store
                .reserve_node_and_stage_queued_with_lease(&request, 5)
                .unwrap();

            match corruption {
                "missing" => {
                    store
                        .connection
                        .execute(
                            "DELETE FROM coordinator_node_reservation_gpus WHERE node_id = 'node-1'",
                            [],
                        )
                        .unwrap();
                }
                "blank" => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_node_reservation_gpus SET gpu_id = ' ' WHERE ordinal = 0",
                            [],
                        )
                        .unwrap();
                }
                "noncanonical" => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_node_reservation_gpus SET gpu_id = 'gpu-z' WHERE ordinal = 0",
                            [],
                        )
                        .unwrap();
                }
                "gapped" => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_node_reservation_gpus SET ordinal = 2 WHERE ordinal = 1",
                            [],
                        )
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert!(matches!(
                store.get_node_reservation("node-1"),
                Err(StagingStoreError::CorruptData(_))
            ));
            assert!(matches!(
                store.reserve_node_and_stage_queued_with_lease(&request, 5),
                Err(ReservedStageError::Staging(StagingStoreError::CorruptData(
                    _
                )))
            ));
            assert_eq!(store.fence_epoch().unwrap(), Some(1));
        }
    }

    #[test]
    fn revision_zero_and_multiple_gpu_ids_survive_reopen_in_canonical_order() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        prepare_inventory_gpus(&path, "node-1", 0, 1, &["gpu-z".into(), "gpu-a".into()]);
        let mut request = request("job-1", 1);
        request.selected_gpu_ids = vec!["gpu-a".into(), "gpu-z".into()];
        let issued = {
            let mut store = CoordinatorStagingStore::open(&path).unwrap();
            store
                .reserve_node_and_stage_queued_with_lease(&request, 0)
                .unwrap()
        };

        let mut reopened = CoordinatorStagingStore::open(&path).unwrap();
        assert_eq!(
            reopened.get_node_reservation("node-1").unwrap(),
            Some(issued.reservation.clone())
        );
        assert_eq!(issued.reservation.inventory_revision, 0);
        assert_eq!(
            issued.reservation.selected_gpu_ids,
            ["gpu-a".to_owned(), "gpu-z".to_owned()]
        );
        let replay = reopened
            .reserve_node_and_stage_queued_with_lease(&request, 0)
            .unwrap();
        assert!(!replay.stage.created);
        assert_eq!(replay.reservation, issued.reservation);
    }

    #[test]
    fn corrupt_inventory_and_reservation_revision_or_owner_fail_closed() {
        for corruption in [
            "inventory-revision",
            "reservation-revision",
            "reservation-owner",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("control.sqlite3");
            prepare_queued(&path, "job-1", 1);
            prepare_inventory(&path, "node-1", 5, 1);
            let request = request("job-1", 1);
            let mut store = CoordinatorStagingStore::open(&path).unwrap();
            if corruption == "inventory-revision" {
                store
                    .connection
                    .execute(
                        "UPDATE coordinator_agent_inventory SET inventory_revision = x'01' WHERE node_id = 'node-1'",
                        [],
                    )
                    .unwrap();
                assert!(matches!(
                    store.reserve_node_and_stage_queued_with_lease(&request, 5),
                    Err(ReservedStageError::Staging(StagingStoreError::CorruptData(
                        _
                    )))
                ));
                assert_queued_and_no_side_effects(&store, &request);
                continue;
            }

            store
                .reserve_node_and_stage_queued_with_lease(&request, 5)
                .unwrap();
            if corruption == "reservation-revision" {
                store
                    .connection
                    .execute(
                        "UPDATE coordinator_node_reservations SET inventory_revision = x'01' WHERE node_id = 'node-1'",
                        [],
                    )
                    .unwrap();
            } else {
                store
                    .connection
                    .execute_batch("PRAGMA foreign_keys = OFF;")
                    .unwrap();
                store
                    .connection
                    .execute(
                        "UPDATE coordinator_node_reservations SET job_id = ' ' WHERE node_id = 'node-1'",
                        [],
                    )
                    .unwrap();
                store
                    .connection
                    .execute_batch("PRAGMA foreign_keys = ON;")
                    .unwrap();
            }
            assert!(matches!(
                store.reserve_node_and_stage_queued_with_lease(&request, 5),
                Err(ReservedStageError::Staging(StagingStoreError::CorruptData(
                    _
                )))
            ));
            assert_eq!(store.fence_epoch().unwrap(), Some(1));
        }
    }

    #[test]
    fn success_is_consistent_and_identical_retry_does_not_consume_epoch() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        let mut request = request("job-1", 1);
        request.selected_gpu_ids.clear();
        let mut store = CoordinatorStagingStore::open(&path).unwrap();

        let first = store.stage_queued_with_lease(&request).unwrap();
        assert!(first.created);
        assert_eq!(first.job.state, JobState::Staging);
        assert_eq!(
            first.job.staging_at_unix_ms,
            Some(request.issued_at_unix_ms)
        );
        assert_eq!(first.job.revision, 3);
        assert_eq!(first.attempt.state, AttemptState::Created);
        assert_eq!(first.attempt.node_ids, [request.node_id.clone()]);
        assert_eq!(first.attempt.job_id, request.job_id);
        assert_eq!(first.attempt.attempt_id, request.attempt_id);
        assert_eq!(first.attempt.lease_id, request.lease_id);
        assert_eq!(first.attempt.fence_epoch, first.lease.fence_epoch);
        assert_eq!(first.lease.job_id, first.job.job_id);
        assert_eq!(first.lease.attempt_id, first.attempt.attempt_id);
        assert_eq!(first.lease.holder_node_id, first.attempt.node_ids[0]);
        assert_eq!(store.fence_epoch().unwrap(), Some(first.lease.fence_epoch));

        let retry = store.stage_queued_with_lease(&request).unwrap();
        assert!(!retry.created);
        assert_eq!(retry.lease.fence_epoch, first.lease.fence_epoch);
        assert_eq!(store.fence_epoch().unwrap(), Some(first.lease.fence_epoch));
    }

    #[test]
    fn every_injected_write_failure_rolls_back_job_attempt_lease_counter_and_operation() {
        for (index, fault) in [
            TestFault::AfterAttemptInsert,
            TestFault::AfterLeaseInsert,
            TestFault::BeforeJobUpdate,
        ]
        .into_iter()
        .enumerate()
        {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("control.sqlite3");
            let job_id = format!("job-{index}");
            prepare_queued(&path, &job_id, index as u8 + 1);
            let request = request(&job_id, index as u8 + 1);
            let mut store = CoordinatorStagingStore::open(&path).unwrap();
            assert!(matches!(
                store.stage(&request, Some(fault)),
                Err(StagingStoreError::InjectedFailure(_))
            ));
            assert_queued_and_no_side_effects(&store, &request);

            let result = store.stage_queued_with_lease(&request).unwrap();
            assert_eq!(
                result.lease.fence_epoch, 1,
                "rollback must not consume epoch"
            );
        }
    }

    #[test]
    fn two_connections_stage_one_queued_job_exactly_once() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-race", 1);
        let stores = [
            CoordinatorStagingStore::open(&path).unwrap(),
            CoordinatorStagingStore::open(&path).unwrap(),
        ];
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = stores
            .into_iter()
            .enumerate()
            .map(|(index, mut store)| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let request = request("job-race", index as u8 + 1);
                    request_assert_distinct(&request, index);
                    barrier.wait();
                    store.stage_queued_with_lease(&request)
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(StagingStoreError::JobNotQueued(JobState::Staging))
                ))
                .count(),
            1
        );

        let store = CoordinatorStagingStore::open(&path).unwrap();
        let connection = &store.connection;
        for table in [
            "coordinator_attempts",
            "coordinator_attempt_nodes",
            "coordinator_leases",
            "staging_operation_idempotency",
        ] {
            let count: u64 = connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 1, "loser must leave no row in {table}");
        }
        assert_eq!(store.fence_epoch().unwrap(), Some(1));
    }

    fn request_assert_distinct(request: &StageQueuedRequest, index: usize) {
        assert_eq!(request.operation_key[0], index as u8 + 1);
        assert_eq!(request.attempt_id, format!("attempt-{}", index + 1));
        assert_eq!(request.lease_id, format!("lease-{}", index + 1));
    }

    #[test]
    fn changed_operation_payload_and_reused_attempt_or_lease_ids_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        for (index, job_id) in ["job-1", "job-2", "job-3"].into_iter().enumerate() {
            prepare_queued(&path, job_id, index as u8 + 1);
        }
        let mut store = CoordinatorStagingStore::open(&path).unwrap();
        let first = request("job-1", 1);
        store.stage_queued_with_lease(&first).unwrap();

        let mut changed = first.clone();
        changed.node_id = "node-2".into();
        assert_eq!(
            store.stage_queued_with_lease(&changed),
            Err(StagingStoreError::OperationConflict)
        );

        let mut attempt_conflict = request("job-2", 2);
        attempt_conflict.attempt_id = first.attempt_id.clone();
        assert_eq!(
            store.stage_queued_with_lease(&attempt_conflict),
            Err(StagingStoreError::AttemptIdConflict(
                first.attempt_id.clone()
            ))
        );
        let mut lease_conflict = request("job-3", 3);
        lease_conflict.lease_id = first.lease_id.clone();
        assert_eq!(
            store.stage_queued_with_lease(&lease_conflict),
            Err(StagingStoreError::LeaseIdConflict(first.lease_id.clone()))
        );
        assert_eq!(store.fence_epoch().unwrap(), Some(1));
        assert_eq!(
            store
                .get_attempt(&attempt_conflict.attempt_id)
                .unwrap()
                .unwrap()
                .job_id,
            "job-1"
        );
        assert_eq!(
            job_store::fetch_job(&store.connection, "job-2")
                .unwrap()
                .unwrap()
                .state,
            JobState::Queued
        );
        assert_eq!(
            job_store::fetch_job(&store.connection, "job-3")
                .unwrap()
                .unwrap()
                .state,
            JobState::Queued
        );
    }

    #[test]
    fn invalid_state_identity_lifetime_and_clock_leave_state_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        let mut jobs = CoordinatorJobStore::open(&path).unwrap();
        jobs.submit_accepted(
            &AcceptedJobSubmission {
                idempotency_key: [9; 16],
                job_id: "submitted".into(),
                submitter_device_id: "s".into(),
                manifest_hash: [9; 32],
                deadline_unix_ms: None,
                max_queue_duration_ms: None,
            },
            100,
        )
        .unwrap();
        drop(jobs);
        prepare_queued(&path, "queued", 8);
        let mut store = CoordinatorStagingStore::open(&path).unwrap();
        assert_eq!(
            store.stage_queued_with_lease(&request("submitted", 9)),
            Err(StagingStoreError::JobNotQueued(JobState::Submitted))
        );

        let base = request("queued", 8);
        let mut invalids = Vec::new();
        let mut blank = base.clone();
        blank.job_id = " ".into();
        invalids.push(blank);
        let mut blank = base.clone();
        blank.attempt_id = " ".into();
        invalids.push(blank);
        let mut blank = base.clone();
        blank.lease_id = " ".into();
        invalids.push(blank);
        let mut blank = base.clone();
        blank.node_id = " ".into();
        invalids.push(blank);
        let mut blank = base.clone();
        blank.issuing_coordinator_id = " ".into();
        invalids.push(blank);
        let mut rollback = base.clone();
        rollback.issued_at_unix_ms = 119;
        rollback.renew_after_unix_ms = 500;
        invalids.push(rollback);
        let mut ordering = base.clone();
        ordering.renew_after_unix_ms = ordering.expires_at_unix_ms;
        invalids.push(ordering);
        let mut over_max = base.clone();
        over_max.expires_at_unix_ms = 1_201;
        invalids.push(over_max);
        for invalid in invalids {
            assert!(store.stage_queued_with_lease(&invalid).is_err());
            assert_queued_and_no_side_effects(&store, &base);
        }
    }

    #[test]
    fn existing_lease_watermark_is_used_and_corrupt_or_max_epoch_rolls_back() {
        for (epoch_bytes, expected) in [
            (41u64.to_be_bytes().to_vec(), Ok(42)),
            (vec![1, 2, 3], Err("corrupt")),
            (u64::MAX.to_be_bytes().to_vec(), Err("overflow")),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("control.sqlite3");
            prepare_queued(&path, "job-1", 1);
            let store = CoordinatorStagingStore::open(&path).unwrap();
            store.connection.execute(
                "INSERT INTO coordinator_leases(lease_id, job_id, attempt_id, holder_node_id,
                 fence_epoch, expires_at_unix_ms, issuing_coordinator_id, coordinator_term,
                 issued_at_unix_ms, renew_after_unix_ms, max_total_duration_seconds, revoked_at_unix_ms)
                 VALUES ('old', 'old-job', 'old-attempt', 'old-node', ?1, ?2, 'old-coordinator', ?2, ?2, ?3, ?3, NULL)",
                rusqlite::params![epoch_bytes, encode_u64(1), encode_u64(2)],
            ).unwrap();
            drop(store);
            let mut store = CoordinatorStagingStore::open(&path).unwrap();
            let request = request("job-1", 1);
            match expected {
                Ok(epoch) => assert_eq!(
                    store
                        .stage_queued_with_lease(&request)
                        .unwrap()
                        .lease
                        .fence_epoch,
                    epoch
                ),
                Err("corrupt") => {
                    assert!(matches!(
                        store.stage_queued_with_lease(&request),
                        Err(StagingStoreError::CorruptData(_))
                    ));
                    assert_queued_and_no_side_effects(&store, &request);
                }
                Err("overflow") => {
                    assert_eq!(
                        store.stage_queued_with_lease(&request),
                        Err(StagingStoreError::FenceEpochOverflow)
                    );
                    assert_queued_and_no_side_effects(&store, &request);
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn corrupt_or_max_counter_fails_without_partial_state() {
        for counter in [vec![1, 2, 3], u64::MAX.to_be_bytes().to_vec()] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("control.sqlite3");
            prepare_queued(&path, "job-1", 1);
            let mut store = CoordinatorStagingStore::open(&path).unwrap();
            store.connection.execute(
                "INSERT INTO coordinator_fence_state(singleton, max_issued_epoch) VALUES (1, ?1)",
                rusqlite::params![counter.clone()],
            ).unwrap();
            let request = request("job-1", 1);
            assert!(store.stage_queued_with_lease(&request).is_err());
            assert_eq!(
                job_store::fetch_job(&store.connection, "job-1")
                    .unwrap()
                    .unwrap()
                    .state,
                JobState::Queued
            );
            assert_eq!(store.get_attempt(&request.attempt_id).unwrap(), None);
            assert_eq!(
                lease_store::fetch_lease(&store.connection, &request.lease_id).unwrap(),
                None
            );
            let persisted: Vec<u8> = store
                .connection
                .query_row(
                    "SELECT max_issued_epoch FROM coordinator_fence_state WHERE singleton = 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                persisted, counter,
                "failed allocation must not rewrite the counter"
            );
        }
    }

    #[test]
    fn reopen_preserves_all_state_and_standalone_stores_read_and_mutate_the_lease() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.sqlite3");
        prepare_queued(&path, "job-1", 1);
        let request = request("job-1", 1);
        let issued = {
            let mut store = CoordinatorStagingStore::open(&path).unwrap();
            store.stage_queued_with_lease(&request).unwrap()
        };
        let reopened = CoordinatorStagingStore::open(&path).unwrap();
        assert_eq!(
            reopened.fence_epoch().unwrap(),
            Some(issued.lease.fence_epoch)
        );
        assert_eq!(
            reopened.get_attempt(&request.attempt_id).unwrap(),
            Some(issued.attempt.clone())
        );
        drop(reopened);

        let jobs = CoordinatorJobStore::open(&path).unwrap();
        assert_eq!(jobs.get(&request.job_id).unwrap().unwrap(), issued.job);
        drop(jobs);
        let mut leases = CoordinatorLeaseStore::open(&path).unwrap();
        assert_eq!(
            leases.get(&request.lease_id).unwrap(),
            Some(issued.lease.clone())
        );
        let renewed = leases.renew_existing(&request.lease_id, 950, 600).unwrap();
        assert_eq!(renewed.fence_epoch, issued.lease.fence_epoch);
        leases.mark_revoked(&request.lease_id, 700).unwrap();
        drop(leases);
        let leases = CoordinatorLeaseStore::open(&path).unwrap();
        assert_eq!(
            leases
                .get(&request.lease_id)
                .unwrap()
                .unwrap()
                .revoked_at_unix_ms,
            Some(700)
        );
        drop(leases);

        let mut staging = CoordinatorStagingStore::open(&path).unwrap();
        let retry = staging.stage_queued_with_lease(&request).unwrap();
        assert!(!retry.created);
        assert_eq!(
            retry.lease, issued.lease,
            "retry returns the original operation result"
        );
    }
}
