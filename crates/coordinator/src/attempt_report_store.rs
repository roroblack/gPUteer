//! Durable inbox for verified terminal [`pb::AttemptReport`] evidence.
//!
//! This store binds a report to the existing single-node Attempt and its
//! current node reservation. It deliberately does not transition Job or
//! Attempt state, revoke a Lease, or release the reservation.

use std::path::Path;

use gputeer_protocol::{canonical::blake3_256, pb, signing::Verified};
use prost::Message;
use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

use crate::staging_store::{self, StoredAttempt, StoredNodeReservation};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq)]
pub struct StoredAttemptReportBinding {
    pub report: pb::AttemptReport,
    pub report_hash: [u8; 32],
    pub signer_id_at_submission: String,
    pub bound_job_id: String,
    pub bound_attempt_id: String,
    pub bound_node_id: String,
    pub bound_fence_epoch: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoreAttemptReportResult {
    pub binding: StoredAttemptReportBinding,
    /// `false` means an exact semantic replay returned the first durable row.
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingField {
    JobId,
    AttemptId,
    NodeId,
    SignerId,
    FenceEpoch,
    ReservationJobId,
    ReservationAttemptId,
    ReservationNodeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptReportCorruption {
    EmptyBody,
    UndecodableBody,
    HashEncoding,
    HashMismatch,
    JobIdMismatch,
    AttemptIdMismatch,
    NodeIdMismatch,
    SignerIdMismatch,
    FenceEpochEncoding,
    FenceEpochMismatch,
    InvalidOutcome,
    /// 저장본이 필드 조합 규칙(`attempt_report_rules`)을 어긴다 — 저장 진입에서 막았어야 할 것이 들어 있다.
    ReportRule,
    MissingAttempt,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AttemptReportStoreError {
    InvalidInput(&'static str),
    InvalidOutcome(i32),
    /// 서명은 유효하지만 필드 조합 규칙을 어긴다(B+E 계획서 §5.7 · §5.8, 결함 70 · 71 · 72).
    ReportRule(gputeer_protocol::attempt_report_rules::ReportRuleError),
    AttemptNotFound {
        attempt_id: String,
    },
    ReservationNotFound {
        node_id: String,
    },
    BindingMismatch(BindingField),
    ReportConflict {
        attempt_id: String,
        node_id: String,
    },
    Corrupt {
        attempt_id: String,
        node_id: String,
        kind: AttemptReportCorruption,
    },
    Staging(String),
    Io(String),
    LockTimeout,
    #[cfg(test)]
    InjectedFailure(&'static str),
}

impl std::fmt::Display for AttemptReportStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(field) => write!(f, "invalid AttemptReport input: {field}"),
            Self::InvalidOutcome(outcome) => {
                write!(
                    f,
                    "AttemptReport outcome is not a known terminal value: {outcome}"
                )
            }
            Self::ReportRule(rule) => write!(f, "AttemptReport field combination rejected: {rule}"),
            Self::AttemptNotFound { attempt_id } => {
                write!(f, "AttemptReport references missing Attempt: {attempt_id}")
            }
            Self::ReservationNotFound { node_id } => {
                write!(
                    f,
                    "AttemptReport node has no current reservation: {node_id}"
                )
            }
            Self::BindingMismatch(field) => {
                write!(f, "AttemptReport durable binding mismatch: {field:?}")
            }
            Self::ReportConflict {
                attempt_id,
                node_id,
            } => write!(
                f,
                "AttemptReport conflicts with first durable evidence: \
                 attempt={attempt_id}, node={node_id}"
            ),
            Self::Corrupt {
                attempt_id,
                node_id,
                kind,
            } => write!(
                f,
                "durable AttemptReport is corrupt: \
                 attempt={attempt_id}, node={node_id}, kind={kind:?}"
            ),
            Self::Staging(message) => {
                write!(f, "AttemptReport staging-state read failed: {message}")
            }
            Self::Io(message) => write!(f, "AttemptReport store I/O error: {message}"),
            Self::LockTimeout => write!(f, "AttemptReport store lock acquisition timed out"),
            #[cfg(test)]
            Self::InjectedFailure(point) => {
                write!(f, "injected AttemptReport store failure: {point}")
            }
        }
    }
}

impl std::error::Error for AttemptReportStoreError {}

pub struct CoordinatorAttemptReportStore {
    connection: Connection,
}

impl CoordinatorAttemptReportStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AttemptReportStoreError> {
        let mut connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        staging_store::initialize_schema(&mut connection).map_err(map_staging_error)?;
        initialize_report_schema(&connection)?;
        Ok(Self { connection })
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.connection.path(), Some(path) if !path.is_empty() && path != ":memory:")
    }

    /// Returns raw durable evidence, not `Verified<AttemptReport>`.
    /// Consumers must verify the signature again against their authoritative
    /// key directory before using report fields for terminal decisions.
    pub fn get_report_binding(
        &self,
        attempt_id: &str,
        node_id: &str,
    ) -> Result<Option<StoredAttemptReportBinding>, AttemptReportStoreError> {
        fetch_report_binding(&self.connection, attempt_id, node_id)
    }

    /// Stores terminal evidence only after binding it to the current durable
    /// Attempt and reservation owner in one `BEGIN IMMEDIATE` transaction.
    pub fn store_verified_terminal_report(
        &mut self,
        verified: &Verified<pb::AttemptReport>,
    ) -> Result<StoreAttemptReportResult, AttemptReportStoreError> {
        self.store_verified_terminal_report_inner(verified, None)
    }

    fn store_verified_terminal_report_inner(
        &mut self,
        verified: &Verified<pb::AttemptReport>,
        fault: Option<TestFault>,
    ) -> Result<StoreAttemptReportResult, AttemptReportStoreError> {
        // No report field is observed before the only report parameter has
        // crossed the Verified type gate.
        let report = verified.get();
        validate_report_input(report)?;
        let signer_id = verified.signer_id();
        let report_body = report.encode_to_vec();
        let report_hash = blake3_256(&report_body);

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        if let Some(binding) =
            fetch_report_binding(&transaction, &report.attempt_id, &report.node_id)?
        {
            if binding.report != *report || binding.signer_id_at_submission != signer_id {
                return Err(AttemptReportStoreError::ReportConflict {
                    attempt_id: report.attempt_id.clone(),
                    node_id: report.node_id.clone(),
                });
            }
            transaction.commit().map_err(map_sql_error)?;
            return Ok(StoreAttemptReportResult {
                binding,
                created: false,
            });
        }

        let attempt = staging_store::fetch_attempt(&transaction, &report.attempt_id)
            .map_err(map_staging_error)?
            .ok_or_else(|| AttemptReportStoreError::AttemptNotFound {
                attempt_id: report.attempt_id.clone(),
            })?;
        bind_attempt(report, signer_id, &attempt)?;

        let reservation = staging_store::fetch_node_reservation(&transaction, &report.node_id)
            .map_err(map_staging_error)?
            .ok_or_else(|| AttemptReportStoreError::ReservationNotFound {
                node_id: report.node_id.clone(),
            })?;
        bind_reservation(report, &reservation)?;

        transaction
            .execute(
                "INSERT INTO coordinator_attempt_reports(
                    attempt_id, node_id, job_id, fence_epoch,
                    verified_signer_id, report_hash, report_body
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    report.attempt_id,
                    report.node_id,
                    report.job_id,
                    encode_u64(report.fence_epoch),
                    signer_id,
                    report_hash.as_slice(),
                    report_body,
                ],
            )
            .map_err(map_sql_error)?;
        fail_at(fault, TestFault::AfterReportInsert)?;
        transaction.commit().map_err(map_sql_error)?;

        Ok(StoreAttemptReportResult {
            binding: StoredAttemptReportBinding {
                report: report.clone(),
                report_hash,
                signer_id_at_submission: signer_id.to_string(),
                bound_job_id: report.job_id.clone(),
                bound_attempt_id: report.attempt_id.clone(),
                bound_node_id: report.node_id.clone(),
                bound_fence_epoch: report.fence_epoch,
            },
            created: true,
        })
    }
}

/// 보고서 테이블을 만든다.
///
/// `reservation_release` 가 같은 control DB 를 열 때도 이 스키마가 있어야
/// 한 트랜잭션에서 증거와 예약을 함께 볼 수 있다.
pub(crate) fn initialize_report_schema(
    connection: &Connection,
) -> Result<(), AttemptReportStoreError> {
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS coordinator_attempt_reports (
                attempt_id TEXT NOT NULL REFERENCES coordinator_attempts(attempt_id),
                node_id TEXT NOT NULL,
                job_id TEXT NOT NULL,
                fence_epoch BLOB NOT NULL,
                verified_signer_id TEXT NOT NULL,
                report_hash BLOB NOT NULL CHECK(length(report_hash) = 32),
                report_body BLOB NOT NULL,
                PRIMARY KEY(attempt_id, node_id)
            );
            "#,
        )
        .map_err(map_sql_error)
}

fn validate_report_input(report: &pb::AttemptReport) -> Result<(), AttemptReportStoreError> {
    if report.job_id.trim().is_empty() {
        return Err(AttemptReportStoreError::InvalidInput("job_id"));
    }
    if report.attempt_id.trim().is_empty() {
        return Err(AttemptReportStoreError::InvalidInput("attempt_id"));
    }
    if report.node_id.trim().is_empty() {
        return Err(AttemptReportStoreError::InvalidInput("node_id"));
    }
    validate_terminal_outcome(report.outcome)?;
    // ★ `Verified` 는 서명 통과이지 조합 규칙 통과가 아니다 — 저장 진입이 직접 부른다(§5.7 (4)).
    gputeer_protocol::attempt_report_rules::validate_attempt_report_semantics(report)
        .map_err(AttemptReportStoreError::ReportRule)
}

/// terminal outcome 인가.
///
/// ★ `reservation_release` 가 **같은 규칙**을 써야 한다 — 여기서
///   terminal 이라 저장한 것을 저기서 아니라고 하면 증거는 있는데 못 푸는
///   상태가 된다.
pub(crate) fn is_terminal_outcome(outcome: i32) -> bool {
    validate_terminal_outcome(outcome).is_ok()
}

fn validate_terminal_outcome(outcome: i32) -> Result<(), AttemptReportStoreError> {
    match pb::AttemptOutcome::try_from(outcome) {
        Ok(pb::AttemptOutcome::Completed)
        | Ok(pb::AttemptOutcome::Failed)
        | Ok(pb::AttemptOutcome::Interrupted)
        | Ok(pb::AttemptOutcome::Cancelled)
        | Ok(pb::AttemptOutcome::StaleCompleted)
        // B+E — 산출물 확정 실패도 끝난 Attempt 다(state-machines.md §3 RUNNING -> FAILED). v1 에서 쓰면 조합 규칙이 거부한다.
        | Ok(pb::AttemptOutcome::OutputFinalizationFailed) => Ok(()),
        Ok(pb::AttemptOutcome::Unspecified) | Err(_) => {
            Err(AttemptReportStoreError::InvalidOutcome(outcome))
        }
    }
}

fn bind_attempt(
    report: &pb::AttemptReport,
    signer_id: &str,
    attempt: &StoredAttempt,
) -> Result<(), AttemptReportStoreError> {
    if report.job_id != attempt.job_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::JobId,
        ));
    }
    if report.attempt_id != attempt.attempt_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::AttemptId,
        ));
    }
    if attempt.node_ids.as_slice() != [report.node_id.as_str()] {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::NodeId,
        ));
    }
    if signer_id != report.node_id || signer_id != attempt.node_ids[0] {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::SignerId,
        ));
    }
    if report.fence_epoch != attempt.fence_epoch {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::FenceEpoch,
        ));
    }
    Ok(())
}

fn bind_reservation(
    report: &pb::AttemptReport,
    reservation: &StoredNodeReservation,
) -> Result<(), AttemptReportStoreError> {
    if report.job_id != reservation.job_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::ReservationJobId,
        ));
    }
    if report.attempt_id != reservation.attempt_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::ReservationAttemptId,
        ));
    }
    if report.node_id != reservation.node_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::ReservationNodeId,
        ));
    }
    Ok(())
}

pub(crate) fn fetch_report_binding(
    connection: &Connection,
    attempt_id: &str,
    node_id: &str,
) -> Result<Option<StoredAttemptReportBinding>, AttemptReportStoreError> {
    let raw = connection
        .query_row(
            "SELECT attempt_id, node_id, job_id, fence_epoch,
                    verified_signer_id, report_hash, report_body
             FROM coordinator_attempt_reports
             WHERE attempt_id = ?1 AND node_id = ?2",
            rusqlite::params![attempt_id, node_id],
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
    let Some((row_attempt_id, row_node_id, row_job_id, epoch, signer_id, hash, body)) = raw else {
        return Ok(None);
    };
    let corrupt = |kind| AttemptReportStoreError::Corrupt {
        attempt_id: row_attempt_id.clone(),
        node_id: row_node_id.clone(),
        kind,
    };
    if body.is_empty() {
        return Err(corrupt(AttemptReportCorruption::EmptyBody));
    }
    let report_hash: [u8; 32] = hash
        .try_into()
        .map_err(|_| corrupt(AttemptReportCorruption::HashEncoding))?;
    if blake3_256(&body) != report_hash {
        return Err(corrupt(AttemptReportCorruption::HashMismatch));
    }
    let report = pb::AttemptReport::decode(body.as_slice())
        .map_err(|_| corrupt(AttemptReportCorruption::UndecodableBody))?;
    if validate_terminal_outcome(report.outcome).is_err() {
        return Err(corrupt(AttemptReportCorruption::InvalidOutcome));
    }
    // 재조회 뒤 재검증(§5.7 (4)) — 저장 진입과 같은 함수다. 어긋나면 손상으로 보고 fail-closed.
    if gputeer_protocol::attempt_report_rules::validate_attempt_report_semantics(&report).is_err() {
        return Err(corrupt(AttemptReportCorruption::ReportRule));
    }
    if report.job_id != row_job_id {
        return Err(corrupt(AttemptReportCorruption::JobIdMismatch));
    }
    if report.attempt_id != row_attempt_id {
        return Err(corrupt(AttemptReportCorruption::AttemptIdMismatch));
    }
    if report.node_id != row_node_id {
        return Err(corrupt(AttemptReportCorruption::NodeIdMismatch));
    }
    if signer_id != row_node_id || signer_id != report.node_id {
        return Err(corrupt(AttemptReportCorruption::SignerIdMismatch));
    }
    let bound_fence_epoch =
        decode_u64(&epoch).map_err(|_| corrupt(AttemptReportCorruption::FenceEpochEncoding))?;
    if report.fence_epoch != bound_fence_epoch {
        return Err(corrupt(AttemptReportCorruption::FenceEpochMismatch));
    }

    let attempt = staging_store::fetch_attempt(connection, &row_attempt_id)
        .map_err(map_staging_error)?
        .ok_or_else(|| corrupt(AttemptReportCorruption::MissingAttempt))?;
    if attempt.job_id != row_job_id {
        return Err(corrupt(AttemptReportCorruption::JobIdMismatch));
    }
    if attempt.node_ids.as_slice() != [row_node_id.as_str()] {
        return Err(corrupt(AttemptReportCorruption::NodeIdMismatch));
    }
    if attempt.fence_epoch != bound_fence_epoch {
        return Err(corrupt(AttemptReportCorruption::FenceEpochMismatch));
    }

    Ok(Some(StoredAttemptReportBinding {
        report,
        report_hash,
        signer_id_at_submission: signer_id,
        bound_job_id: row_job_id,
        bound_attempt_id: row_attempt_id,
        bound_node_id: row_node_id,
        bound_fence_epoch,
    }))
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8]) -> Result<u64, ()> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| ())?;
    Ok(u64::from_be_bytes(bytes))
}

fn map_sql_error(error: SqlError) -> AttemptReportStoreError {
    match error {
        SqlError::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            AttemptReportStoreError::LockTimeout
        }
        SqlError::SqliteFailure(code, _) => AttemptReportStoreError::Io(code.to_string()),
        other => AttemptReportStoreError::Io(other.to_string()),
    }
}

fn map_staging_error(error: staging_store::StagingStoreError) -> AttemptReportStoreError {
    AttemptReportStoreError::Staging(error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFault {
    AfterReportInsert,
}

#[cfg(test)]
fn fail_at(fault: Option<TestFault>, point: TestFault) -> Result<(), AttemptReportStoreError> {
    if fault == Some(point) {
        Err(AttemptReportStoreError::InjectedFailure(
            "after AttemptReport insert",
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
fn fail_at(_fault: Option<TestFault>, _point: TestFault) -> Result<(), AttemptReportStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory_store::{
        AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory,
    };
    use crate::job_store::{AcceptedJobSubmission, CoordinatorJobStore, JobState};
    use crate::lease_store::CoordinatorLeaseStore;
    use crate::staging_store::{CoordinatorStagingStore, StageQueuedRequest};
    use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
    use gputeer_protocol::signing::{verify, NoReplayCheck};
    use std::path::{Path, PathBuf};

    const JOB_ID: &str = "job-1";
    const ATTEMPT_ID: &str = "attempt-1";
    const LEASE_ID: &str = "lease-1";
    const NODE_ID: &str = "node-1";

    struct Fixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
    }

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
                gpus: Some(vec![GpuInventory {
                    gpu_id: "gpu-1".into(),
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

        let request = StageQueuedRequest {
            operation_key: [2; 16],
            job_id: JOB_ID.into(),
            attempt_id: ATTEMPT_ID.into(),
            lease_id: LEASE_ID.into(),
            node_id: NODE_ID.into(),
            selected_gpu_ids: vec!["gpu-1".into()],
            issuing_coordinator_id: "coordinator-1".into(),
            coordinator_term: 1,
            issued_at_unix_ms: 200,
            renew_after_unix_ms: 500,
            expires_at_unix_ms: 900,
            max_total_duration_seconds: 1,
        };
        CoordinatorStagingStore::open(&path)
            .unwrap()
            .reserve_node_and_stage_queued_with_lease(&request, 7)
            .unwrap();

        Fixture { _dir: dir, path }
    }

    fn verified_report(
        job_id: &str,
        attempt_id: &str,
        node_id: &str,
        fence_epoch: u64,
        outcome: i32,
        key_seed: u8,
        final_step: u64,
    ) -> Verified<pb::AttemptReport> {
        let key = SigningKey::from_bytes(&[key_seed; 32]);
        let mut report = pb::AttemptReport {
            schema_version: 1,
            job_id: job_id.into(),
            attempt_id: attempt_id.into(),
            node_id: node_id.into(),
            fence_epoch,
            outcome,
            final_step,
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
        .expect("test AttemptReport signature must verify")
    }

    fn completed_report(fence_epoch: u64) -> Verified<pb::AttemptReport> {
        verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            fence_epoch,
            pb::AttemptOutcome::Completed as i32,
            7,
            10,
        )
    }

    /// B+E 필드(14 · 15 · 16)나 v2 를 쓰는 보고 — 서명 뒤 v2 까지 읽는 검증기로 통과시킨다.
    fn base_report(schema_version: u32, outcome: pb::AttemptOutcome) -> pb::AttemptReport {
        pb::AttemptReport {
            schema_version,
            job_id: JOB_ID.into(),
            attempt_id: ATTEMPT_ID.into(),
            node_id: NODE_ID.into(),
            fence_epoch: 1,
            outcome: outcome as i32,
            final_step: 10,
            started_at_unix_ms: 210,
            finished_at_unix_ms: 300,
            issued_at_unix_ms: 301,
            ..Default::default()
        }
    }

    fn verified_custom(mut report: pb::AttemptReport, key_seed: u8) -> Verified<pb::AttemptReport> {
        let key = SigningKey::from_bytes(&[key_seed; 32]);
        report.node_signature = sign(&key, &report).to_vec();
        let mut keys = InMemoryKeyring::new();
        keys.insert(NODE_ID, key.verifying_key());
        verify(
            &report,
            gputeer_protocol::constants::ATTEMPT_REPORT_MAX_SCHEMA_VERSION,
            &Ed25519Verifier::new(keys),
            999,
            &mut NoReplayCheck,
        )
        .expect("테스트 보고는 서명 검증을 통과해야 한다")
    }

    fn report_count(store: &CoordinatorAttemptReportStore) -> u64 {
        store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM coordinator_attempt_reports",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn rewrite_body(store: &CoordinatorAttemptReportStore, report: &pb::AttemptReport) {
        let body = report.encode_to_vec();
        let hash = blake3_256(&body);
        store
            .connection
            .execute(
                "UPDATE coordinator_attempt_reports
                 SET report_body = ?1, report_hash = ?2
                 WHERE attempt_id = ?3 AND node_id = ?4",
                rusqlite::params![body, hash.as_slice(), ATTEMPT_ID, NODE_ID],
            )
            .unwrap();
    }

    fn insert_other_job(path: &Path) {
        let mut jobs = CoordinatorJobStore::open(path).unwrap();
        jobs.submit_accepted(
            &AcceptedJobSubmission {
                idempotency_key: [9; 16],
                job_id: "job-other".into(),
                submitter_device_id: "submitter-1".into(),
                manifest_hash: [9; 32],
                deadline_unix_ms: None,
                max_queue_duration_ms: None,
            },
            100,
        )
        .unwrap();
    }

    #[test]
    fn all_five_known_terminal_outcomes_are_stored() {
        for outcome in [
            pb::AttemptOutcome::Completed,
            pb::AttemptOutcome::Failed,
            pb::AttemptOutcome::Interrupted,
            pb::AttemptOutcome::Cancelled,
            pb::AttemptOutcome::StaleCompleted,
        ] {
            let fixture = prepare_fixture();
            let report = verified_report(JOB_ID, ATTEMPT_ID, NODE_ID, 1, outcome as i32, 7, 10);
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            let result = store.store_verified_terminal_report(&report).unwrap();
            assert!(result.created, "outcome {outcome:?}");
            assert_eq!(result.binding.report.outcome, outcome as i32);
            assert_eq!(report_count(&store), 1);
        }
    }

    #[test]
    fn unspecified_and_unknown_outcomes_create_no_row() {
        for outcome in [pb::AttemptOutcome::Unspecified as i32, 99] {
            let fixture = prepare_fixture();
            let report = verified_report(JOB_ID, ATTEMPT_ID, NODE_ID, 1, outcome, 7, 10);
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            assert_eq!(
                store.store_verified_terminal_report(&report),
                Err(AttemptReportStoreError::InvalidOutcome(outcome))
            );
            assert_eq!(report_count(&store), 0);
        }
    }

    /// 결함 70 · 74 — 서명은 유효하지만 조합 규칙을 어긴 보고는 행을 만들지 않고, 거부 **종류**가 규칙 위반이다.
    #[test]
    fn signed_reports_that_break_the_field_rules_create_no_row() {
        use gputeer_protocol::attempt_report_rules::ReportRuleError as R;
        let mut v1_new_field = base_report(1, pb::AttemptOutcome::Failed);
        v1_new_field.exit_observation = pb::ExitObservation::ObservedWithCode as i32;
        v1_new_field.exit_code = 7;
        let v1_new_outcome = base_report(1, pb::AttemptOutcome::OutputFinalizationFailed);
        let mut v2_unobserved_completion = base_report(2, pb::AttemptOutcome::Completed);
        v2_unobserved_completion.exit_observation = pb::ExitObservation::NotObserved as i32;
        for (report, expected) in [
            (v1_new_field, R::V1UsesNewFields),
            (v1_new_outcome, R::V1UsesNewOutcome),
            (
                v2_unobserved_completion,
                R::CombinationRejected {
                    outcome: pb::AttemptOutcome::Completed as i32,
                    exit_observation: pb::ExitObservation::NotObserved as i32,
                    exit_code: 0,
                    stage: 0,
                },
            ),
        ] {
            let fixture = prepare_fixture();
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            assert_eq!(
                store.store_verified_terminal_report(&verified_custom(report, 7)),
                Err(AttemptReportStoreError::ReportRule(expected))
            );
            assert_eq!(report_count(&store), 0);
        }
    }

    /// v2 산출물 확정 실패(outcome 6)는 terminal 증거로 저장되고, 같은 보고 재제출은 재조회 검사를 지나 첫 행을 돌려준다.
    #[test]
    fn a_v2_output_finalization_failure_is_stored_and_replays() {
        let fixture = prepare_fixture();
        let mut report = base_report(2, pb::AttemptOutcome::OutputFinalizationFailed);
        report.exit_observation = pb::ExitObservation::ObservedWithCode as i32;
        report.finalization_failure_stage = pb::FinalizationFailureStage::ReadOutputs as i32;
        let verified = verified_custom(report, 7);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let first = store.store_verified_terminal_report(&verified).unwrap();
        assert!(first.created);
        assert_eq!(first.binding.report, verified.get().clone());
        let again = store.store_verified_terminal_report(&verified).unwrap();
        assert!(!again.created);
        assert_eq!(report_count(&store), 1);
    }

    /// 재조회 뒤 재검증(§5.7 (4)) — 규칙을 어긴 저장본은 손상으로 fail-closed.
    #[test]
    fn a_stored_body_that_breaks_the_field_rules_fails_closed() {
        let fixture = prepare_fixture();
        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store.store_verified_terminal_report(&report).unwrap();
        let mut changed = report.get().clone();
        // v1 에 v2 필드 — 저장 진입이면 거부됐을 몸통.
        changed.exit_observation = pb::ExitObservation::ObservedWithCode as i32;
        rewrite_body(&store, &changed);
        assert_eq!(
            store.get_report_binding(ATTEMPT_ID, NODE_ID),
            Err(AttemptReportStoreError::Corrupt {
                attempt_id: ATTEMPT_ID.into(),
                node_id: NODE_ID.into(),
                kind: AttemptReportCorruption::ReportRule,
            })
        );
    }

    #[test]
    fn wrong_job_attempt_and_stale_fence_create_no_row() {
        for (report, expected) in [
            (
                verified_report(
                    "job-other",
                    ATTEMPT_ID,
                    NODE_ID,
                    1,
                    pb::AttemptOutcome::Completed as i32,
                    7,
                    10,
                ),
                AttemptReportStoreError::BindingMismatch(BindingField::JobId),
            ),
            (
                verified_report(
                    JOB_ID,
                    "attempt-other",
                    NODE_ID,
                    1,
                    pb::AttemptOutcome::Completed as i32,
                    7,
                    10,
                ),
                AttemptReportStoreError::AttemptNotFound {
                    attempt_id: "attempt-other".into(),
                },
            ),
            (
                completed_report(0),
                AttemptReportStoreError::BindingMismatch(BindingField::FenceEpoch),
            ),
        ] {
            let fixture = prepare_fixture();
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            assert_eq!(store.store_verified_terminal_report(&report), Err(expected));
            assert_eq!(report_count(&store), 0);
        }
    }

    #[test]
    fn attempt_node_and_verified_signer_are_bound_to_durable_owner() {
        let fixture = prepare_fixture();
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_attempt_nodes SET node_id = 'node-other'
                 WHERE attempt_id = ?1",
                rusqlite::params![ATTEMPT_ID],
            )
            .unwrap();
        drop(store);

        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_terminal_report(&report),
            Err(AttemptReportStoreError::BindingMismatch(
                BindingField::NodeId
            ))
        );
        assert_eq!(report_count(&store), 0);
    }

    #[test]
    fn missing_or_different_reservation_owner_creates_no_row() {
        let fixture = prepare_fixture();
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute(
                "DELETE FROM coordinator_node_reservation_gpus WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        store
            .connection
            .execute(
                "DELETE FROM coordinator_node_reservations WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        drop(store);
        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_terminal_report(&report),
            Err(AttemptReportStoreError::ReservationNotFound {
                node_id: NODE_ID.into()
            })
        );
        assert_eq!(report_count(&store), 0);

        let fixture = prepare_fixture();
        insert_other_job(&fixture.path);
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_node_reservations SET job_id = 'job-other'
                 WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        drop(store);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_terminal_report(&report),
            Err(AttemptReportStoreError::BindingMismatch(
                BindingField::ReservationJobId
            ))
        );
        assert_eq!(report_count(&store), 0);
    }

    #[test]
    fn exact_replay_returns_first_row_and_changed_body_or_signature_conflicts() {
        let fixture = prepare_fixture();
        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let first = store.store_verified_terminal_report(&report).unwrap();
        let replay = store.store_verified_terminal_report(&report).unwrap();
        assert!(first.created);
        assert!(!replay.created);
        assert_eq!(replay.binding, first.binding);
        assert_eq!(report_count(&store), 1);

        let changed_body = verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            pb::AttemptOutcome::Failed as i32,
            7,
            11,
        );
        assert!(matches!(
            store.store_verified_terminal_report(&changed_body),
            Err(AttemptReportStoreError::ReportConflict { .. })
        ));
        let changed_signature = verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            pb::AttemptOutcome::Completed as i32,
            8,
            10,
        );
        assert!(matches!(
            store.store_verified_terminal_report(&changed_signature),
            Err(AttemptReportStoreError::ReportConflict { .. })
        ));
        assert_eq!(report_count(&store), 1);
        assert_eq!(
            store.get_report_binding(ATTEMPT_ID, NODE_ID).unwrap(),
            Some(first.binding)
        );
    }

    #[test]
    fn signature_and_binding_survive_reopen_without_state_or_release_side_effects() {
        let fixture = prepare_fixture();
        let before_job = CoordinatorJobStore::open(&fixture.path)
            .unwrap()
            .get(JOB_ID)
            .unwrap()
            .unwrap();
        let before_attempt = CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_attempt(ATTEMPT_ID)
            .unwrap()
            .unwrap();
        let before_lease = CoordinatorLeaseStore::open(&fixture.path)
            .unwrap()
            .get(LEASE_ID)
            .unwrap()
            .unwrap();
        let before_reservation = CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_node_reservation(NODE_ID)
            .unwrap()
            .unwrap();
        assert_eq!(before_job.state, JobState::Staging);

        let report = completed_report(1);
        let original = report.get().clone();
        {
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            store.store_verified_terminal_report(&report).unwrap();
        }
        let reopened = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let binding = reopened
            .get_report_binding(ATTEMPT_ID, NODE_ID)
            .unwrap()
            .unwrap();
        assert_eq!(binding.report, original);
        assert_eq!(binding.report.node_signature, original.node_signature);
        assert_eq!(binding.signer_id_at_submission, NODE_ID);
        assert_eq!(binding.bound_fence_epoch, 1);
        drop(reopened);

        assert_eq!(
            CoordinatorJobStore::open(&fixture.path)
                .unwrap()
                .get(JOB_ID)
                .unwrap()
                .unwrap(),
            before_job
        );
        assert_eq!(
            CoordinatorStagingStore::open(&fixture.path)
                .unwrap()
                .get_attempt(ATTEMPT_ID)
                .unwrap()
                .unwrap(),
            before_attempt
        );
        assert_eq!(
            CoordinatorLeaseStore::open(&fixture.path)
                .unwrap()
                .get(LEASE_ID)
                .unwrap()
                .unwrap(),
            before_lease
        );
        assert_eq!(
            CoordinatorStagingStore::open(&fixture.path)
                .unwrap()
                .get_node_reservation(NODE_ID)
                .unwrap()
                .unwrap(),
            before_reservation
        );
    }

    #[test]
    fn failure_after_insert_rolls_back_report_and_preserves_control_state() {
        let fixture = prepare_fixture();
        let before_job = CoordinatorJobStore::open(&fixture.path)
            .unwrap()
            .get(JOB_ID)
            .unwrap()
            .unwrap();
        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_terminal_report_inner(&report, Some(TestFault::AfterReportInsert)),
            Err(AttemptReportStoreError::InjectedFailure(
                "after AttemptReport insert"
            ))
        );
        assert_eq!(report_count(&store), 0);
        drop(store);
        assert_eq!(
            CoordinatorJobStore::open(&fixture.path)
                .unwrap()
                .get(JOB_ID)
                .unwrap()
                .unwrap(),
            before_job
        );
        assert!(CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_node_reservation(NODE_ID)
            .unwrap()
            .is_some());
    }

    #[test]
    fn corrupt_body_hash_identity_signer_and_fence_fail_closed() {
        for expected in [
            AttemptReportCorruption::EmptyBody,
            AttemptReportCorruption::UndecodableBody,
            AttemptReportCorruption::HashMismatch,
            AttemptReportCorruption::JobIdMismatch,
            AttemptReportCorruption::AttemptIdMismatch,
            AttemptReportCorruption::NodeIdMismatch,
            AttemptReportCorruption::SignerIdMismatch,
            AttemptReportCorruption::FenceEpochMismatch,
        ] {
            let fixture = prepare_fixture();
            let report = completed_report(1);
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            store.store_verified_terminal_report(&report).unwrap();
            match expected {
                AttemptReportCorruption::EmptyBody => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_attempt_reports SET report_body = X''",
                            [],
                        )
                        .unwrap();
                }
                AttemptReportCorruption::UndecodableBody => {
                    let body = vec![0x12, 0x05, b'a'];
                    let hash = blake3_256(&body);
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_attempt_reports
                             SET report_body = ?1, report_hash = ?2",
                            rusqlite::params![body, hash.as_slice()],
                        )
                        .unwrap();
                }
                AttemptReportCorruption::HashMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_attempt_reports SET report_hash = ?1",
                            rusqlite::params![[9u8; 32].as_slice()],
                        )
                        .unwrap();
                }
                AttemptReportCorruption::JobIdMismatch => {
                    let mut changed = report.get().clone();
                    changed.job_id = "job-other".into();
                    rewrite_body(&store, &changed);
                }
                AttemptReportCorruption::AttemptIdMismatch => {
                    let mut changed = report.get().clone();
                    changed.attempt_id = "attempt-other".into();
                    rewrite_body(&store, &changed);
                }
                AttemptReportCorruption::NodeIdMismatch => {
                    let mut changed = report.get().clone();
                    changed.node_id = "node-other".into();
                    rewrite_body(&store, &changed);
                }
                AttemptReportCorruption::SignerIdMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_attempt_reports
                             SET verified_signer_id = 'node-other'",
                            [],
                        )
                        .unwrap();
                }
                AttemptReportCorruption::FenceEpochMismatch => {
                    let mut changed = report.get().clone();
                    changed.fence_epoch = 2;
                    rewrite_body(&store, &changed);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                store.get_report_binding(ATTEMPT_ID, NODE_ID),
                Err(AttemptReportStoreError::Corrupt {
                    attempt_id: ATTEMPT_ID.into(),
                    node_id: NODE_ID.into(),
                    kind: expected,
                })
            );
        }
    }
}
