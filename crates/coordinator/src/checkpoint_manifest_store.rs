//! Durable inbox for verified signed [`pb::CheckpointManifest`] evidence.
//!
//! This store binds a manifest to the existing single-node Attempt and its
//! current node reservation. It deliberately does not inspect checkpoint
//! files, advance checkpoint durability, transition Job or Attempt state,
//! revoke a Lease, or release the reservation.

use std::path::Path;

use gputeer_protocol::{canonical::blake3_256, pb, signing::Verified};
use prost::Message;
use rusqlite::{
    Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior,
};

use crate::staging_store::{self, StoredAttempt, StoredNodeReservation};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// Raw durable evidence. This is intentionally not `Verified<CheckpointManifest>`:
/// callers must verify `manifest` again against the then-authoritative key
/// directory before using it for durability or canonical-selection decisions.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredCheckpointManifestBinding {
    pub manifest: pb::CheckpointManifest,
    pub manifest_hash: [u8; 32],
    pub signer_id_at_submission: String,
    pub bound_checkpoint_id: String,
    pub bound_job_id: String,
    pub bound_attempt_id: String,
    pub bound_producer_node_id: String,
    pub bound_fence_epoch: u64,
    pub bound_root_digest: pb::Digest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoreCheckpointManifestResult {
    pub binding: StoredCheckpointManifestBinding,
    /// `false` means an exact semantic replay returned the first durable row.
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointBindingField {
    JobId,
    AttemptId,
    ProducerAndSigner,
    FenceEpoch,
    ReservationJobId,
    ReservationAttemptId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointManifestCorruption {
    EmptyBody,
    UndecodableBody,
    HashEncoding,
    HashMismatch,
    InvalidManifest,
    CheckpointIdMismatch,
    JobIdMismatch,
    AttemptIdMismatch,
    ProducerNodeIdMismatch,
    SignerIdMismatch,
    FenceEpochEncoding,
    FenceEpochMismatch,
    RootDigestEncoding,
    RootDigestMismatch,
    MissingAttempt,
    AttemptBinding,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CheckpointManifestStoreError {
    InvalidInput(&'static str),
    AttemptNotFound { attempt_id: String },
    ReservationNotFound { node_id: String },
    BindingMismatch(CheckpointBindingField),
    ManifestConflict { checkpoint_id: String },
    Corrupt {
        checkpoint_id: String,
        kind: CheckpointManifestCorruption,
    },
    Staging(String),
    Io(String),
    LockTimeout,
    #[cfg(test)]
    InjectedFailure(&'static str),
}

impl std::fmt::Display for CheckpointManifestStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(field) => {
                write!(f, "invalid CheckpointManifest input: {field}")
            }
            Self::AttemptNotFound { attempt_id } => {
                write!(f, "CheckpointManifest references missing Attempt: {attempt_id}")
            }
            Self::ReservationNotFound { node_id } => write!(
                f,
                "CheckpointManifest producer has no current reservation: {node_id}"
            ),
            Self::BindingMismatch(field) => {
                write!(f, "CheckpointManifest durable binding mismatch: {field:?}")
            }
            Self::ManifestConflict { checkpoint_id } => write!(
                f,
                "CheckpointManifest conflicts with first durable evidence: {checkpoint_id}"
            ),
            Self::Corrupt {
                checkpoint_id,
                kind,
            } => write!(
                f,
                "durable CheckpointManifest is corrupt: checkpoint={checkpoint_id}, kind={kind:?}"
            ),
            Self::Staging(message) => {
                write!(f, "CheckpointManifest staging-state read failed: {message}")
            }
            Self::Io(message) => write!(f, "CheckpointManifest store I/O error: {message}"),
            Self::LockTimeout => write!(f, "CheckpointManifest store lock acquisition timed out"),
            #[cfg(test)]
            Self::InjectedFailure(point) => {
                write!(f, "injected CheckpointManifest store failure: {point}")
            }
        }
    }
}

impl std::error::Error for CheckpointManifestStoreError {}

pub struct CoordinatorCheckpointManifestStore {
    connection: Connection,
}

impl CoordinatorCheckpointManifestStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CheckpointManifestStoreError> {
        let mut connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        staging_store::initialize_schema(&mut connection).map_err(map_staging_error)?;
        connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS coordinator_checkpoint_manifests (
                    checkpoint_id TEXT PRIMARY KEY,
                    job_id TEXT NOT NULL REFERENCES coordinator_jobs(job_id),
                    attempt_id TEXT NOT NULL REFERENCES coordinator_attempts(attempt_id),
                    producer_node_id TEXT NOT NULL,
                    verified_signer_id TEXT NOT NULL,
                    fence_epoch BLOB NOT NULL,
                    root_digest BLOB NOT NULL,
                    manifest_hash BLOB NOT NULL CHECK(length(manifest_hash) = 32),
                    manifest_body BLOB NOT NULL
                );
                "#,
            )
            .map_err(map_sql_error)?;
        Ok(Self { connection })
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.connection.path(), Some(path) if !path.is_empty() && path != ":memory:")
    }

    /// Returns raw durable evidence, not `Verified<CheckpointManifest>`.
    pub fn get_manifest_binding(
        &self,
        checkpoint_id: &str,
    ) -> Result<Option<StoredCheckpointManifestBinding>, CheckpointManifestStoreError> {
        fetch_manifest_binding(&self.connection, checkpoint_id)
    }

    /// Stores evidence only after binding it to the current durable Attempt and
    /// reservation owner in one `BEGIN IMMEDIATE` transaction.
    pub fn store_verified_manifest(
        &mut self,
        verified: &Verified<pb::CheckpointManifest>,
    ) -> Result<StoreCheckpointManifestResult, CheckpointManifestStoreError> {
        self.store_verified_manifest_inner(verified, None)
    }

    fn store_verified_manifest_inner(
        &mut self,
        verified: &Verified<pb::CheckpointManifest>,
        fault: Option<TestFault>,
    ) -> Result<StoreCheckpointManifestResult, CheckpointManifestStoreError> {
        // No manifest field is observed before the only manifest parameter has
        // crossed the Verified type gate.
        let manifest = verified.get();
        validate_manifest_input(manifest)?;
        let signer_id = verified.signer_id();
        let manifest_body = manifest.encode_to_vec();
        let manifest_hash = blake3_256(&manifest_body);
        let root_digest = manifest
            .root_digest
            .as_ref()
            .expect("validated root_digest must be present");
        let root_digest_body = root_digest.encode_to_vec();

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        if let Some(binding) =
            fetch_manifest_binding(&transaction, &manifest.checkpoint_id)?
        {
            if binding.manifest != *manifest || binding.signer_id_at_submission != signer_id {
                return Err(CheckpointManifestStoreError::ManifestConflict {
                    checkpoint_id: manifest.checkpoint_id.clone(),
                });
            }
            transaction.commit().map_err(map_sql_error)?;
            return Ok(StoreCheckpointManifestResult {
                binding,
                created: false,
            });
        }

        let attempt = staging_store::fetch_attempt(&transaction, &manifest.attempt_id)
            .map_err(map_staging_error)?
            .ok_or_else(|| CheckpointManifestStoreError::AttemptNotFound {
                attempt_id: manifest.attempt_id.clone(),
            })?;
        bind_attempt_identity_and_fence(manifest, &attempt)?;

        let durable_node_id = attempt.node_ids[0].as_str();
        let reservation = staging_store::fetch_node_reservation(&transaction, durable_node_id)
            .map_err(map_staging_error)?
            .ok_or_else(|| CheckpointManifestStoreError::ReservationNotFound {
                node_id: durable_node_id.to_string(),
            })?;
        bind_reservation_identity(manifest, &reservation)?;
        bind_producer_and_signer(manifest, signer_id, &attempt, &reservation)?;

        transaction
            .execute(
                "INSERT INTO coordinator_checkpoint_manifests(
                    checkpoint_id, job_id, attempt_id, producer_node_id,
                    verified_signer_id, fence_epoch, root_digest,
                    manifest_hash, manifest_body
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    manifest.checkpoint_id,
                    manifest.job_id,
                    manifest.attempt_id,
                    manifest.producer_node_id,
                    signer_id,
                    encode_u64(manifest.fence_epoch),
                    root_digest_body,
                    manifest_hash.as_slice(),
                    manifest_body,
                ],
            )
            .map_err(map_sql_error)?;
        fail_at(fault, TestFault::AfterManifestInsert)?;
        transaction.commit().map_err(map_sql_error)?;

        Ok(StoreCheckpointManifestResult {
            binding: StoredCheckpointManifestBinding {
                manifest: manifest.clone(),
                manifest_hash,
                signer_id_at_submission: signer_id.to_string(),
                bound_checkpoint_id: manifest.checkpoint_id.clone(),
                bound_job_id: manifest.job_id.clone(),
                bound_attempt_id: manifest.attempt_id.clone(),
                bound_producer_node_id: manifest.producer_node_id.clone(),
                bound_fence_epoch: manifest.fence_epoch,
                bound_root_digest: root_digest.clone(),
            },
            created: true,
        })
    }
}

fn validate_manifest_input(
    manifest: &pb::CheckpointManifest,
) -> Result<(), CheckpointManifestStoreError> {
    if manifest.schema_version == 0 {
        return Err(CheckpointManifestStoreError::InvalidInput("schema_version"));
    }
    if manifest.checkpoint_id.trim().is_empty() {
        return Err(CheckpointManifestStoreError::InvalidInput("checkpoint_id"));
    }
    if manifest.job_id.trim().is_empty() {
        return Err(CheckpointManifestStoreError::InvalidInput("job_id"));
    }
    if manifest.attempt_id.trim().is_empty() {
        return Err(CheckpointManifestStoreError::InvalidInput("attempt_id"));
    }
    if manifest.producer_node_id.trim().is_empty() {
        return Err(CheckpointManifestStoreError::InvalidInput(
            "producer_node_id",
        ));
    }
    if manifest.producer_signature.is_empty() {
        return Err(CheckpointManifestStoreError::InvalidInput(
            "producer_signature",
        ));
    }
    let digest = manifest
        .root_digest
        .as_ref()
        .ok_or(CheckpointManifestStoreError::InvalidInput("root_digest"))?;
    validate_root_digest(digest)
}

fn validate_root_digest(digest: &pb::Digest) -> Result<(), CheckpointManifestStoreError> {
    match pb::HashAlgorithm::try_from(digest.algo) {
        Ok(pb::HashAlgorithm::Blake3256) if digest.value.len() == 32 => Ok(()),
        _ => Err(CheckpointManifestStoreError::InvalidInput("root_digest")),
    }
}

fn bind_attempt_identity_and_fence(
    manifest: &pb::CheckpointManifest,
    attempt: &StoredAttempt,
) -> Result<(), CheckpointManifestStoreError> {
    if manifest.job_id != attempt.job_id {
        return Err(CheckpointManifestStoreError::BindingMismatch(
            CheckpointBindingField::JobId,
        ));
    }
    if manifest.attempt_id != attempt.attempt_id {
        return Err(CheckpointManifestStoreError::BindingMismatch(
            CheckpointBindingField::AttemptId,
        ));
    }
    if manifest.fence_epoch != attempt.fence_epoch {
        return Err(CheckpointManifestStoreError::BindingMismatch(
            CheckpointBindingField::FenceEpoch,
        ));
    }
    Ok(())
}

fn bind_reservation_identity(
    manifest: &pb::CheckpointManifest,
    reservation: &StoredNodeReservation,
) -> Result<(), CheckpointManifestStoreError> {
    if manifest.job_id != reservation.job_id {
        return Err(CheckpointManifestStoreError::BindingMismatch(
            CheckpointBindingField::ReservationJobId,
        ));
    }
    if manifest.attempt_id != reservation.attempt_id {
        return Err(CheckpointManifestStoreError::BindingMismatch(
            CheckpointBindingField::ReservationAttemptId,
        ));
    }
    Ok(())
}

fn bind_producer_and_signer(
    manifest: &pb::CheckpointManifest,
    signer_id: &str,
    attempt: &StoredAttempt,
    reservation: &StoredNodeReservation,
) -> Result<(), CheckpointManifestStoreError> {
    if manifest.producer_node_id != attempt.node_ids[0]
        || manifest.producer_node_id != reservation.node_id
        || signer_id != manifest.producer_node_id
        || signer_id != attempt.node_ids[0]
        || signer_id != reservation.node_id
    {
        return Err(CheckpointManifestStoreError::BindingMismatch(
            CheckpointBindingField::ProducerAndSigner,
        ));
    }
    Ok(())
}

fn fetch_manifest_binding(
    connection: &Connection,
    checkpoint_id: &str,
) -> Result<Option<StoredCheckpointManifestBinding>, CheckpointManifestStoreError> {
    let raw = connection
        .query_row(
            "SELECT checkpoint_id, job_id, attempt_id, producer_node_id,
                    verified_signer_id, fence_epoch, root_digest,
                    manifest_hash, manifest_body
             FROM coordinator_checkpoint_manifests WHERE checkpoint_id = ?1",
            rusqlite::params![checkpoint_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                    row.get::<_, Vec<u8>>(8)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some((
        row_checkpoint_id,
        row_job_id,
        row_attempt_id,
        row_producer_node_id,
        signer_id,
        fence_epoch,
        root_digest_body,
        hash,
        body,
    )) = raw
    else {
        return Ok(None);
    };
    let corrupt = |kind| CheckpointManifestStoreError::Corrupt {
        checkpoint_id: row_checkpoint_id.clone(),
        kind,
    };
    if body.is_empty() {
        return Err(corrupt(CheckpointManifestCorruption::EmptyBody));
    }
    let manifest_hash: [u8; 32] = hash
        .try_into()
        .map_err(|_| corrupt(CheckpointManifestCorruption::HashEncoding))?;
    if blake3_256(&body) != manifest_hash {
        return Err(corrupt(CheckpointManifestCorruption::HashMismatch));
    }
    let manifest = pb::CheckpointManifest::decode(body.as_slice())
        .map_err(|_| corrupt(CheckpointManifestCorruption::UndecodableBody))?;
    if validate_manifest_input(&manifest).is_err() {
        return Err(corrupt(CheckpointManifestCorruption::InvalidManifest));
    }
    if manifest.checkpoint_id != row_checkpoint_id {
        return Err(corrupt(
            CheckpointManifestCorruption::CheckpointIdMismatch,
        ));
    }
    if manifest.job_id != row_job_id {
        return Err(corrupt(CheckpointManifestCorruption::JobIdMismatch));
    }
    if manifest.attempt_id != row_attempt_id {
        return Err(corrupt(CheckpointManifestCorruption::AttemptIdMismatch));
    }
    if manifest.producer_node_id != row_producer_node_id {
        return Err(corrupt(
            CheckpointManifestCorruption::ProducerNodeIdMismatch,
        ));
    }
    if signer_id != row_producer_node_id || signer_id != manifest.producer_node_id {
        return Err(corrupt(CheckpointManifestCorruption::SignerIdMismatch));
    }
    let bound_fence_epoch = decode_u64(&fence_epoch)
        .map_err(|_| corrupt(CheckpointManifestCorruption::FenceEpochEncoding))?;
    if manifest.fence_epoch != bound_fence_epoch {
        return Err(corrupt(CheckpointManifestCorruption::FenceEpochMismatch));
    }
    if root_digest_body.is_empty() {
        return Err(corrupt(
            CheckpointManifestCorruption::RootDigestEncoding,
        ));
    }
    let bound_root_digest = pb::Digest::decode(root_digest_body.as_slice())
        .map_err(|_| corrupt(CheckpointManifestCorruption::RootDigestEncoding))?;
    if validate_root_digest(&bound_root_digest).is_err() {
        return Err(corrupt(
            CheckpointManifestCorruption::RootDigestEncoding,
        ));
    }
    if manifest.root_digest.as_ref() != Some(&bound_root_digest) {
        return Err(corrupt(
            CheckpointManifestCorruption::RootDigestMismatch,
        ));
    }

    let attempt = match staging_store::fetch_attempt(connection, &row_attempt_id) {
        Ok(Some(attempt)) => attempt,
        Ok(None) => return Err(corrupt(CheckpointManifestCorruption::MissingAttempt)),
        Err(staging_store::StagingStoreError::CorruptData(_)) => {
            return Err(corrupt(CheckpointManifestCorruption::AttemptBinding));
        }
        Err(error) => return Err(map_staging_error(error)),
    };
    if attempt.job_id != row_job_id {
        return Err(corrupt(CheckpointManifestCorruption::JobIdMismatch));
    }
    if attempt.node_ids.as_slice() != [row_producer_node_id.as_str()] {
        return Err(corrupt(
            CheckpointManifestCorruption::ProducerNodeIdMismatch,
        ));
    }
    if attempt.fence_epoch != bound_fence_epoch {
        return Err(corrupt(CheckpointManifestCorruption::FenceEpochMismatch));
    }

    Ok(Some(StoredCheckpointManifestBinding {
        manifest,
        manifest_hash,
        signer_id_at_submission: signer_id,
        bound_checkpoint_id: row_checkpoint_id,
        bound_job_id: row_job_id,
        bound_attempt_id: row_attempt_id,
        bound_producer_node_id: row_producer_node_id,
        bound_fence_epoch,
        bound_root_digest,
    }))
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8]) -> Result<u64, ()> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| ())?;
    Ok(u64::from_be_bytes(bytes))
}

fn map_sql_error(error: SqlError) -> CheckpointManifestStoreError {
    match error {
        SqlError::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            CheckpointManifestStoreError::LockTimeout
        }
        SqlError::SqliteFailure(code, _) => CheckpointManifestStoreError::Io(code.to_string()),
        other => CheckpointManifestStoreError::Io(other.to_string()),
    }
}

fn map_staging_error(error: staging_store::StagingStoreError) -> CheckpointManifestStoreError {
    CheckpointManifestStoreError::Staging(error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFault {
    AfterManifestInsert,
}

#[cfg(test)]
fn fail_at(
    fault: Option<TestFault>,
    point: TestFault,
) -> Result<(), CheckpointManifestStoreError> {
    if fault == Some(point) {
        Err(CheckpointManifestStoreError::InjectedFailure(
            "after CheckpointManifest insert",
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
fn fail_at(
    _fault: Option<TestFault>,
    _point: TestFault,
) -> Result<(), CheckpointManifestStoreError> {
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

    const CHECKPOINT_ID: &str = "checkpoint-1";
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

        CoordinatorStagingStore::open(&path)
            .unwrap()
            .reserve_node_and_stage_queued_with_lease(
                &StageQueuedRequest {
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
                },
                7,
            )
            .unwrap();

        Fixture {
            _dir: dir,
            path,
        }
    }

    fn verify_manifest(
        mut manifest: pb::CheckpointManifest,
        key_seed: u8,
    ) -> Verified<pb::CheckpointManifest> {
        let key = SigningKey::from_bytes(&[key_seed; 32]);
        manifest.producer_signature = sign(&key, &manifest).to_vec();
        let mut keys = InMemoryKeyring::new();
        keys.insert(&manifest.producer_node_id, key.verifying_key());
        verify(
            &manifest,
            1,
            &Ed25519Verifier::new(keys),
            999,
            &mut NoReplayCheck,
        )
        .expect("test CheckpointManifest signature must verify")
    }

    #[allow(clippy::too_many_arguments)]
    fn verified_manifest(
        checkpoint_id: &str,
        job_id: &str,
        attempt_id: &str,
        producer_node_id: &str,
        fence_epoch: u64,
        key_seed: u8,
        root_byte: u8,
        step: u64,
    ) -> Verified<pb::CheckpointManifest> {
        verify_manifest(
            pb::CheckpointManifest {
                schema_version: 1,
                checkpoint_id: checkpoint_id.into(),
                job_id: job_id.into(),
                attempt_id: attempt_id.into(),
                step,
                epoch: 2,
                root_digest: Some(pb::Digest {
                    algo: pb::HashAlgorithm::Blake3256 as i32,
                    value: vec![root_byte; 32],
                }),
                total_bytes: 4096,
                created_at_unix_ms: 300,
                producer_node_id: producer_node_id.into(),
                fence_epoch,
                ..Default::default()
            },
            key_seed,
        )
    }

    fn valid_manifest() -> Verified<pb::CheckpointManifest> {
        verified_manifest(
            CHECKPOINT_ID,
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            7,
            3,
            10,
        )
    }

    fn manifest_count(store: &CoordinatorCheckpointManifestStore) -> u64 {
        store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM coordinator_checkpoint_manifests",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn rewrite_body(
        store: &CoordinatorCheckpointManifestStore,
        manifest: &pb::CheckpointManifest,
    ) {
        let body = manifest.encode_to_vec();
        let hash = blake3_256(&body);
        store
            .connection
            .execute(
                "UPDATE coordinator_checkpoint_manifests
                 SET manifest_body = ?1, manifest_hash = ?2
                 WHERE checkpoint_id = ?3",
                rusqlite::params![body, hash.as_slice(), CHECKPOINT_ID],
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
    fn complete_signed_manifest_and_store_derived_hash_survive_reopen() {
        let fixture = prepare_fixture();
        let verified = valid_manifest();
        let original = verified.get().clone();
        let expected_hash = blake3_256(&original.encode_to_vec());
        {
            let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
            assert!(store.is_durable());
            let result = store.store_verified_manifest(&verified).unwrap();
            assert!(result.created);
            assert_eq!(result.binding.manifest, original);
            assert_eq!(result.binding.manifest_hash, expected_hash);
            assert_eq!(result.binding.signer_id_at_submission, NODE_ID);
            assert_eq!(result.binding.bound_fence_epoch, 1);
        }

        let reopened = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        let binding = reopened
            .get_manifest_binding(CHECKPOINT_ID)
            .unwrap()
            .unwrap();
        assert_eq!(binding.manifest, original);
        assert_eq!(binding.manifest.producer_signature, original.producer_signature);
        assert_eq!(binding.manifest_hash, expected_hash);
        assert_eq!(binding.bound_root_digest, original.root_digest.unwrap());
    }

    #[test]
    fn missing_or_unspecified_root_and_blank_identity_create_no_row() {
        for (mutation, expected) in [
            ("checkpoint", "checkpoint_id"),
            ("root-missing", "root_digest"),
            ("root-unspecified", "root_digest"),
            ("root-sha256", "root_digest"),
            ("root-length", "root_digest"),
        ] {
            let fixture = prepare_fixture();
            let mut manifest = valid_manifest().get().clone();
            match mutation {
                "checkpoint" => manifest.checkpoint_id = " ".into(),
                "root-missing" => manifest.root_digest = None,
                "root-unspecified" => manifest.root_digest.as_mut().unwrap().algo = 0,
                "root-sha256" => {
                    manifest.root_digest.as_mut().unwrap().algo =
                        pb::HashAlgorithm::Sha256 as i32;
                }
                "root-length" => {
                    manifest.root_digest.as_mut().unwrap().value.pop();
                }
                _ => unreachable!(),
            }
            let verified = verify_manifest(manifest, 7);
            let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
            assert_eq!(
                store.store_verified_manifest(&verified),
                Err(CheckpointManifestStoreError::InvalidInput(expected)),
                "mutation {mutation} must fail closed"
            );
            assert_eq!(manifest_count(&store), 0);
        }
    }

    #[test]
    fn wrong_job_attempt_and_stale_fence_create_no_row() {
        for (manifest, expected) in [
            (
                verified_manifest(
                    CHECKPOINT_ID,
                    "job-other",
                    ATTEMPT_ID,
                    NODE_ID,
                    1,
                    7,
                    3,
                    10,
                ),
                CheckpointManifestStoreError::BindingMismatch(
                    CheckpointBindingField::JobId,
                ),
            ),
            (
                verified_manifest(
                    CHECKPOINT_ID,
                    JOB_ID,
                    "attempt-other",
                    NODE_ID,
                    1,
                    7,
                    3,
                    10,
                ),
                CheckpointManifestStoreError::AttemptNotFound {
                    attempt_id: "attempt-other".into(),
                },
            ),
            (
                verified_manifest(
                    CHECKPOINT_ID,
                    JOB_ID,
                    ATTEMPT_ID,
                    NODE_ID,
                    0,
                    7,
                    3,
                    10,
                ),
                CheckpointManifestStoreError::BindingMismatch(
                    CheckpointBindingField::FenceEpoch,
                ),
            ),
        ] {
            let fixture = prepare_fixture();
            let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
            assert_eq!(store.store_verified_manifest(&manifest), Err(expected));
            assert_eq!(manifest_count(&store), 0);
        }
    }

    #[test]
    fn producer_signer_mismatch_with_durable_owner_creates_no_row() {
        let fixture = prepare_fixture();
        let manifest = verified_manifest(
            CHECKPOINT_ID,
            JOB_ID,
            ATTEMPT_ID,
            "node-other",
            1,
            8,
            3,
            10,
        );
        let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_manifest(&manifest),
            Err(CheckpointManifestStoreError::BindingMismatch(
                CheckpointBindingField::ProducerAndSigner
            ))
        );
        assert_eq!(manifest_count(&store), 0);
    }

    #[test]
    fn missing_or_different_reservation_owner_creates_no_row() {
        let fixture = prepare_fixture();
        let store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
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
        let manifest = valid_manifest();
        let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_manifest(&manifest),
            Err(CheckpointManifestStoreError::ReservationNotFound {
                node_id: NODE_ID.into()
            })
        );
        assert_eq!(manifest_count(&store), 0);

        let fixture = prepare_fixture();
        insert_other_job(&fixture.path);
        let store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_node_reservations SET job_id = 'job-other'
                 WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        drop(store);
        let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_manifest(&manifest),
            Err(CheckpointManifestStoreError::BindingMismatch(
                CheckpointBindingField::ReservationJobId
            ))
        );
        assert_eq!(manifest_count(&store), 0);

        let fixture = prepare_fixture();
        let store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_node_reservations SET attempt_id = 'attempt-other'
                 WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        drop(store);
        let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_manifest(&manifest),
            Err(CheckpointManifestStoreError::BindingMismatch(
                CheckpointBindingField::ReservationAttemptId
            ))
        );
        assert_eq!(manifest_count(&store), 0);
    }

    #[test]
    fn exact_replay_returns_first_row_and_changed_body_or_signature_conflicts() {
        let fixture = prepare_fixture();
        let manifest = valid_manifest();
        let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        let first = store.store_verified_manifest(&manifest).unwrap();
        let replay = store.store_verified_manifest(&manifest).unwrap();
        assert!(first.created);
        assert!(!replay.created);
        assert_eq!(replay.binding, first.binding);
        assert_eq!(manifest_count(&store), 1);

        let changed_body = verified_manifest(
            CHECKPOINT_ID,
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            7,
            4,
            11,
        );
        assert!(matches!(
            store.store_verified_manifest(&changed_body),
            Err(CheckpointManifestStoreError::ManifestConflict { .. })
        ));
        let changed_signature = verified_manifest(
            CHECKPOINT_ID,
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            8,
            3,
            10,
        );
        assert!(matches!(
            store.store_verified_manifest(&changed_signature),
            Err(CheckpointManifestStoreError::ManifestConflict { .. })
        ));
        assert_eq!(manifest_count(&store), 1);
        assert_eq!(
            store.get_manifest_binding(CHECKPOINT_ID).unwrap(),
            Some(first.binding)
        );
    }

    #[test]
    fn storage_has_no_checkpoint_state_or_control_state_side_effects() {
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

        let manifest = valid_manifest();
        let store = {
            let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
            store.store_verified_manifest(&manifest).unwrap();
            store
        };
        let columns = {
            let mut statement = store
                .connection
                .prepare("PRAGMA table_info(coordinator_checkpoint_manifests)")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(!columns.iter().any(|column| column.contains("state")));
        drop(columns);
        drop(store);

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
    fn failure_after_insert_rolls_back_manifest_and_preserves_reservation() {
        let fixture = prepare_fixture();
        let before_attempt = CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_attempt(ATTEMPT_ID)
            .unwrap()
            .unwrap();
        let before_reservation = CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_node_reservation(NODE_ID)
            .unwrap()
            .unwrap();
        let manifest = valid_manifest();
        let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_manifest_inner(
                &manifest,
                Some(TestFault::AfterManifestInsert)
            ),
            Err(CheckpointManifestStoreError::InjectedFailure(
                "after CheckpointManifest insert"
            ))
        );
        assert_eq!(manifest_count(&store), 0);
        drop(store);
        assert_eq!(
            CoordinatorStagingStore::open(&fixture.path)
                .unwrap()
                .get_attempt(ATTEMPT_ID)
                .unwrap()
                .unwrap(),
            before_attempt
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
    fn corrupt_body_hash_identity_signer_fence_and_root_fail_closed() {
        for expected in [
            CheckpointManifestCorruption::EmptyBody,
            CheckpointManifestCorruption::UndecodableBody,
            CheckpointManifestCorruption::HashEncoding,
            CheckpointManifestCorruption::HashMismatch,
            CheckpointManifestCorruption::InvalidManifest,
            CheckpointManifestCorruption::CheckpointIdMismatch,
            CheckpointManifestCorruption::JobIdMismatch,
            CheckpointManifestCorruption::AttemptIdMismatch,
            CheckpointManifestCorruption::ProducerNodeIdMismatch,
            CheckpointManifestCorruption::SignerIdMismatch,
            CheckpointManifestCorruption::FenceEpochEncoding,
            CheckpointManifestCorruption::FenceEpochMismatch,
            CheckpointManifestCorruption::RootDigestEncoding,
            CheckpointManifestCorruption::RootDigestMismatch,
        ] {
            let fixture = prepare_fixture();
            let manifest = valid_manifest();
            let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
            store.store_verified_manifest(&manifest).unwrap();
            match expected {
                CheckpointManifestCorruption::EmptyBody => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_checkpoint_manifests SET manifest_body = X''",
                            [],
                        )
                        .unwrap();
                }
                CheckpointManifestCorruption::UndecodableBody => {
                    let body = vec![0x12, 0x05, b'a'];
                    let hash = blake3_256(&body);
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_checkpoint_manifests
                             SET manifest_body = ?1, manifest_hash = ?2",
                            rusqlite::params![body, hash.as_slice()],
                        )
                        .unwrap();
                }
                CheckpointManifestCorruption::HashEncoding => {
                    store
                        .connection
                        .execute_batch("PRAGMA ignore_check_constraints = ON;")
                        .unwrap();
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_checkpoint_manifests SET manifest_hash = X'01'",
                            [],
                        )
                        .unwrap();
                }
                CheckpointManifestCorruption::HashMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_checkpoint_manifests SET manifest_hash = ?1",
                            rusqlite::params![[9u8; 32].as_slice()],
                        )
                        .unwrap();
                }
                CheckpointManifestCorruption::InvalidManifest => {
                    let mut changed = manifest.get().clone();
                    changed.root_digest.as_mut().unwrap().algo = 0;
                    rewrite_body(&store, &changed);
                }
                CheckpointManifestCorruption::CheckpointIdMismatch => {
                    let mut changed = manifest.get().clone();
                    changed.checkpoint_id = "checkpoint-other".into();
                    rewrite_body(&store, &changed);
                }
                CheckpointManifestCorruption::JobIdMismatch => {
                    let mut changed = manifest.get().clone();
                    changed.job_id = "job-other".into();
                    rewrite_body(&store, &changed);
                }
                CheckpointManifestCorruption::AttemptIdMismatch => {
                    let mut changed = manifest.get().clone();
                    changed.attempt_id = "attempt-other".into();
                    rewrite_body(&store, &changed);
                }
                CheckpointManifestCorruption::ProducerNodeIdMismatch => {
                    let mut changed = manifest.get().clone();
                    changed.producer_node_id = "node-other".into();
                    rewrite_body(&store, &changed);
                }
                CheckpointManifestCorruption::SignerIdMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_checkpoint_manifests
                             SET verified_signer_id = 'node-other'",
                            [],
                        )
                        .unwrap();
                }
                CheckpointManifestCorruption::FenceEpochEncoding => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_checkpoint_manifests SET fence_epoch = X'01'",
                            [],
                        )
                        .unwrap();
                }
                CheckpointManifestCorruption::FenceEpochMismatch => {
                    let mut changed = manifest.get().clone();
                    changed.fence_epoch = 2;
                    rewrite_body(&store, &changed);
                }
                CheckpointManifestCorruption::RootDigestEncoding => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_checkpoint_manifests SET root_digest = X''",
                            [],
                        )
                        .unwrap();
                }
                CheckpointManifestCorruption::RootDigestMismatch => {
                    let changed = pb::Digest {
                        algo: pb::HashAlgorithm::Blake3256 as i32,
                        value: vec![8; 32],
                    };
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_checkpoint_manifests SET root_digest = ?1",
                            rusqlite::params![changed.encode_to_vec()],
                        )
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(
                store.get_manifest_binding(CHECKPOINT_ID),
                Err(CheckpointManifestStoreError::Corrupt {
                    checkpoint_id: CHECKPOINT_ID.into(),
                    kind: expected,
                })
            );
        }
    }

    #[test]
    fn current_attempt_binding_corruption_fails_closed() {
        let fixture = prepare_fixture();
        let manifest = valid_manifest();
        let mut store = CoordinatorCheckpointManifestStore::open(&fixture.path).unwrap();
        store.store_verified_manifest(&manifest).unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_attempts SET fence_epoch = ?1 WHERE attempt_id = ?2",
                rusqlite::params![encode_u64(2), ATTEMPT_ID],
            )
            .unwrap();
        assert_eq!(
            store.get_manifest_binding(CHECKPOINT_ID),
            Err(CheckpointManifestStoreError::Corrupt {
                checkpoint_id: CHECKPOINT_ID.into(),
                kind: CheckpointManifestCorruption::FenceEpochMismatch,
            })
        );
    }
}
