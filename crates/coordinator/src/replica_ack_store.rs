//! Durable inbox for verified signed [`pb::ReplicaAck`] observations.
//!
//! This store binds an immutable ACK observation to the existing durable
//! CheckpointManifest and its exact BLAKE3-256 root. It deliberately does not
//! decide holder membership, failure-domain uniqueness, replica eligibility or
//! freshness, count effective replicas, or advance checkpoint state.

use std::path::Path;

use gputeer_protocol::{canonical::blake3_256, pb, signing::Verified};
use prost::Message;
use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

use crate::checkpoint_manifest_store::{
    self, CheckpointManifestStoreError, StoredCheckpointManifestBinding,
};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// Raw durable evidence. This is intentionally not `Verified<ReplicaAck>`:
/// callers must verify `ack` again against the then-authoritative key directory
/// before making membership, freshness, durability, or state decisions.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredReplicaAckBinding {
    pub ack: pb::ReplicaAck,
    pub ack_hash: [u8; 32],
    pub signer_id_at_submission: String,
    pub bound_checkpoint_id: String,
    pub bound_holder_device_id: String,
    pub bound_acked_at_unix_ms: u64,
    pub bound_root_digest: pb::Digest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoreReplicaAckResult {
    pub binding: StoredReplicaAckBinding,
    /// `false` means an exact semantic replay returned the first durable row.
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplicaAckBindingField {
    HolderAndSigner,
    RootDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplicaAckCorruption {
    EmptyBody,
    UndecodableBody,
    HashEncoding,
    HashMismatch,
    InvalidAck,
    CheckpointIdMismatch,
    HolderDeviceIdMismatch,
    SignerIdMismatch,
    AckedAtEncoding,
    AckedAtMismatch,
    RootDigestEncoding,
    RootDigestMismatch,
    MissingCheckpointAnchor,
    CheckpointRootMismatch,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReplicaAckStoreError {
    InvalidInput(&'static str),
    CheckpointNotFound {
        checkpoint_id: String,
    },
    BindingMismatch(ReplicaAckBindingField),
    AckConflict {
        checkpoint_id: String,
        holder_device_id: String,
        acked_at_unix_ms: u64,
    },
    Corrupt {
        checkpoint_id: String,
        holder_device_id: String,
        kind: ReplicaAckCorruption,
    },
    CheckpointAnchor(CheckpointManifestStoreError),
    Io(String),
    LockTimeout,
    #[cfg(test)]
    InjectedFailure(&'static str),
}

impl std::fmt::Display for ReplicaAckStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(field) => write!(f, "invalid ReplicaAck input: {field}"),
            Self::CheckpointNotFound { checkpoint_id } => {
                write!(f, "ReplicaAck references missing checkpoint: {checkpoint_id}")
            }
            Self::BindingMismatch(field) => {
                write!(f, "ReplicaAck durable binding mismatch: {field:?}")
            }
            Self::AckConflict {
                checkpoint_id,
                holder_device_id,
                acked_at_unix_ms,
            } => write!(
                f,
                "ReplicaAck conflicts with first durable observation: checkpoint={checkpoint_id}, holder={holder_device_id}, acked_at={acked_at_unix_ms}"
            ),
            Self::Corrupt {
                checkpoint_id,
                holder_device_id,
                kind,
            } => write!(
                f,
                "durable ReplicaAck is corrupt: checkpoint={checkpoint_id}, holder={holder_device_id}, kind={kind:?}"
            ),
            Self::CheckpointAnchor(error) => {
                write!(f, "ReplicaAck checkpoint anchor read failed: {error}")
            }
            Self::Io(message) => write!(f, "ReplicaAck store I/O error: {message}"),
            Self::LockTimeout => write!(f, "ReplicaAck store lock acquisition timed out"),
            #[cfg(test)]
            Self::InjectedFailure(point) => {
                write!(f, "injected ReplicaAck store failure: {point}")
            }
        }
    }
}

impl std::error::Error for ReplicaAckStoreError {}

pub struct CoordinatorReplicaAckStore {
    connection: Connection,
}

impl CoordinatorReplicaAckStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ReplicaAckStoreError> {
        let mut connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        checkpoint_manifest_store::initialize_schema(&mut connection)
            .map_err(ReplicaAckStoreError::CheckpointAnchor)?;
        connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS coordinator_replica_acks (
                    checkpoint_id TEXT NOT NULL
                        REFERENCES coordinator_checkpoint_manifests(checkpoint_id),
                    holder_device_id TEXT NOT NULL,
                    acked_at_unix_ms BLOB NOT NULL,
                    verified_signer_id TEXT NOT NULL,
                    root_digest BLOB NOT NULL,
                    ack_hash BLOB NOT NULL CHECK(length(ack_hash) = 32),
                    ack_body BLOB NOT NULL,
                    PRIMARY KEY(checkpoint_id, holder_device_id, acked_at_unix_ms)
                );
                "#,
            )
            .map_err(map_sql_error)?;
        Ok(Self { connection })
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.connection.path(), Some(path) if !path.is_empty() && path != ":memory:")
    }

    /// Returns raw durable evidence, not `Verified<ReplicaAck>`.
    pub fn get_ack_binding(
        &self,
        checkpoint_id: &str,
        holder_device_id: &str,
        acked_at_unix_ms: u64,
    ) -> Result<Option<StoredReplicaAckBinding>, ReplicaAckStoreError> {
        let raw = fetch_raw_ack(
            &self.connection,
            checkpoint_id,
            holder_device_id,
            &encode_u64(acked_at_unix_ms),
        )?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        let anchor = load_anchor_for_stored_ack(&self.connection, &raw)?;
        validate_ack_row(raw, &anchor).map(Some)
    }

    /// Returns raw observations in deterministic `(acked_at, holder)` order.
    /// The result intentionally carries no effective count or eligibility
    /// conclusion.
    pub fn list_ack_bindings(
        &self,
        checkpoint_id: &str,
    ) -> Result<Vec<StoredReplicaAckBinding>, ReplicaAckStoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT checkpoint_id, holder_device_id, acked_at_unix_ms,
                        verified_signer_id, root_digest, ack_hash, ack_body
                 FROM coordinator_replica_acks
                 WHERE checkpoint_id = ?1
                 ORDER BY acked_at_unix_ms ASC, holder_device_id ASC",
            )
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map(rusqlite::params![checkpoint_id], read_raw_ack_row)
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        drop(statement);
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let anchor = load_anchor_for_stored_ack(&self.connection, &rows[0])?;
        rows.into_iter()
            .map(|row| validate_ack_row(row, &anchor))
            .collect()
    }

    /// Stores an observation only after validating the current durable
    /// CheckpointManifest anchor and exact root in the same `BEGIN IMMEDIATE`
    /// transaction as the insert.
    ///
    /// Raw protobuf values cannot cross this API boundary:
    ///
    /// ```compile_fail
    /// use gputeer_coordinator::replica_ack_store::CoordinatorReplicaAckStore;
    /// use gputeer_protocol::pb;
    ///
    /// fn cannot_store_raw(store: &mut CoordinatorReplicaAckStore, ack: &pb::ReplicaAck) {
    ///     store.store_verified_ack(ack);
    /// }
    /// ```
    pub fn store_verified_ack(
        &mut self,
        verified: &Verified<pb::ReplicaAck>,
    ) -> Result<StoreReplicaAckResult, ReplicaAckStoreError> {
        self.store_verified_ack_inner(verified, None)
    }

    fn store_verified_ack_inner(
        &mut self,
        verified: &Verified<pb::ReplicaAck>,
        fault: Option<TestFault>,
    ) -> Result<StoreReplicaAckResult, ReplicaAckStoreError> {
        // No ACK field is observed before the only ACK parameter has crossed
        // the Verified type gate.
        let ack = verified.get();
        let signer_id = verified.signer_id();
        let ack_body = ack.encode_to_vec();
        let ack_hash = blake3_256(&ack_body);

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        // Structural checks and every durable comparison happen only after the
        // write lock is held. In particular, no anchor decision can race the
        // immutable insert.
        validate_ack_input(ack)?;
        if signer_id != ack.holder_device_id {
            return Err(ReplicaAckStoreError::BindingMismatch(
                ReplicaAckBindingField::HolderAndSigner,
            ));
        }
        let root_digest = ack
            .root_digest
            .as_ref()
            .expect("validated root_digest must be present");
        let anchor =
            checkpoint_manifest_store::fetch_manifest_binding(&transaction, &ack.checkpoint_id)
                .map_err(ReplicaAckStoreError::CheckpointAnchor)?
                .ok_or_else(|| ReplicaAckStoreError::CheckpointNotFound {
                    checkpoint_id: ack.checkpoint_id.clone(),
                })?;
        if root_digest != &anchor.bound_root_digest {
            return Err(ReplicaAckStoreError::BindingMismatch(
                ReplicaAckBindingField::RootDigest,
            ));
        }
        if let Some(raw) = fetch_raw_ack(
            &transaction,
            &ack.checkpoint_id,
            &ack.holder_device_id,
            &encode_u64(ack.acked_at_unix_ms),
        )? {
            let binding = validate_ack_row(raw, &anchor)?;
            if binding.ack != *ack || binding.signer_id_at_submission != signer_id {
                return Err(ReplicaAckStoreError::AckConflict {
                    checkpoint_id: ack.checkpoint_id.clone(),
                    holder_device_id: ack.holder_device_id.clone(),
                    acked_at_unix_ms: ack.acked_at_unix_ms,
                });
            }
            transaction.commit().map_err(map_sql_error)?;
            return Ok(StoreReplicaAckResult {
                binding,
                created: false,
            });
        }

        transaction
            .execute(
                "INSERT INTO coordinator_replica_acks(
                    checkpoint_id, holder_device_id, acked_at_unix_ms,
                    verified_signer_id, root_digest, ack_hash, ack_body
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    ack.checkpoint_id,
                    ack.holder_device_id,
                    encode_u64(ack.acked_at_unix_ms),
                    signer_id,
                    root_digest.encode_to_vec(),
                    ack_hash.as_slice(),
                    ack_body,
                ],
            )
            .map_err(map_sql_error)?;
        fail_at(fault, TestFault::AfterAckInsert)?;
        transaction.commit().map_err(map_sql_error)?;

        Ok(StoreReplicaAckResult {
            binding: StoredReplicaAckBinding {
                ack: ack.clone(),
                ack_hash,
                signer_id_at_submission: signer_id.to_string(),
                bound_checkpoint_id: ack.checkpoint_id.clone(),
                bound_holder_device_id: ack.holder_device_id.clone(),
                bound_acked_at_unix_ms: ack.acked_at_unix_ms,
                bound_root_digest: root_digest.clone(),
            },
            created: true,
        })
    }
}

fn validate_ack_input(ack: &pb::ReplicaAck) -> Result<(), ReplicaAckStoreError> {
    if ack.schema_version == 0 {
        return Err(ReplicaAckStoreError::InvalidInput("schema_version"));
    }
    if ack.checkpoint_id.trim().is_empty() {
        return Err(ReplicaAckStoreError::InvalidInput("checkpoint_id"));
    }
    if ack.holder_device_id.trim().is_empty() {
        return Err(ReplicaAckStoreError::InvalidInput("holder_device_id"));
    }
    if ack.holder_signature.is_empty() {
        return Err(ReplicaAckStoreError::InvalidInput("holder_signature"));
    }
    if ack.acked_at_unix_ms == 0 {
        return Err(ReplicaAckStoreError::InvalidInput("acked_at_unix_ms"));
    }
    let digest = ack
        .root_digest
        .as_ref()
        .ok_or(ReplicaAckStoreError::InvalidInput("root_digest"))?;
    validate_root_digest(digest)
}

fn validate_root_digest(digest: &pb::Digest) -> Result<(), ReplicaAckStoreError> {
    match pb::HashAlgorithm::try_from(digest.algo) {
        Ok(pb::HashAlgorithm::Blake3256) if digest.value.len() == 32 => Ok(()),
        _ => Err(ReplicaAckStoreError::InvalidInput("root_digest")),
    }
}

#[derive(Debug)]
struct RawReplicaAckRow {
    checkpoint_id: String,
    holder_device_id: String,
    acked_at_unix_ms: Vec<u8>,
    signer_id: String,
    root_digest: Vec<u8>,
    hash: Vec<u8>,
    body: Vec<u8>,
}

fn read_raw_ack_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawReplicaAckRow> {
    Ok(RawReplicaAckRow {
        checkpoint_id: row.get(0)?,
        holder_device_id: row.get(1)?,
        acked_at_unix_ms: row.get(2)?,
        signer_id: row.get(3)?,
        root_digest: row.get(4)?,
        hash: row.get(5)?,
        body: row.get(6)?,
    })
}

fn fetch_raw_ack(
    connection: &Connection,
    checkpoint_id: &str,
    holder_device_id: &str,
    acked_at_unix_ms: &[u8],
) -> Result<Option<RawReplicaAckRow>, ReplicaAckStoreError> {
    connection
        .query_row(
            "SELECT checkpoint_id, holder_device_id, acked_at_unix_ms,
                    verified_signer_id, root_digest, ack_hash, ack_body
             FROM coordinator_replica_acks
             WHERE checkpoint_id = ?1 AND holder_device_id = ?2
                   AND acked_at_unix_ms = ?3",
            rusqlite::params![checkpoint_id, holder_device_id, acked_at_unix_ms],
            read_raw_ack_row,
        )
        .optional()
        .map_err(map_sql_error)
}

fn load_anchor_for_stored_ack(
    connection: &Connection,
    raw: &RawReplicaAckRow,
) -> Result<StoredCheckpointManifestBinding, ReplicaAckStoreError> {
    checkpoint_manifest_store::fetch_manifest_binding(connection, &raw.checkpoint_id)
        .map_err(ReplicaAckStoreError::CheckpointAnchor)?
        .ok_or_else(|| corrupt(raw, ReplicaAckCorruption::MissingCheckpointAnchor))
}

fn validate_ack_row(
    raw: RawReplicaAckRow,
    anchor: &StoredCheckpointManifestBinding,
) -> Result<StoredReplicaAckBinding, ReplicaAckStoreError> {
    if raw.body.is_empty() {
        return Err(corrupt(&raw, ReplicaAckCorruption::EmptyBody));
    }
    let ack_hash: [u8; 32] = raw
        .hash
        .as_slice()
        .try_into()
        .map_err(|_| corrupt(&raw, ReplicaAckCorruption::HashEncoding))?;
    if blake3_256(&raw.body) != ack_hash {
        return Err(corrupt(&raw, ReplicaAckCorruption::HashMismatch));
    }
    let ack = pb::ReplicaAck::decode(raw.body.as_slice())
        .map_err(|_| corrupt(&raw, ReplicaAckCorruption::UndecodableBody))?;
    if validate_ack_input(&ack).is_err() {
        return Err(corrupt(&raw, ReplicaAckCorruption::InvalidAck));
    }
    if ack.checkpoint_id != raw.checkpoint_id {
        return Err(corrupt(&raw, ReplicaAckCorruption::CheckpointIdMismatch));
    }
    if ack.holder_device_id != raw.holder_device_id {
        return Err(corrupt(&raw, ReplicaAckCorruption::HolderDeviceIdMismatch));
    }
    if raw.signer_id != raw.holder_device_id || raw.signer_id != ack.holder_device_id {
        return Err(corrupt(&raw, ReplicaAckCorruption::SignerIdMismatch));
    }
    let bound_acked_at_unix_ms = decode_u64(&raw.acked_at_unix_ms)
        .map_err(|_| corrupt(&raw, ReplicaAckCorruption::AckedAtEncoding))?;
    if ack.acked_at_unix_ms != bound_acked_at_unix_ms {
        return Err(corrupt(&raw, ReplicaAckCorruption::AckedAtMismatch));
    }
    if raw.root_digest.is_empty() {
        return Err(corrupt(&raw, ReplicaAckCorruption::RootDigestEncoding));
    }
    let bound_root_digest = pb::Digest::decode(raw.root_digest.as_slice())
        .map_err(|_| corrupt(&raw, ReplicaAckCorruption::RootDigestEncoding))?;
    if validate_root_digest(&bound_root_digest).is_err() {
        return Err(corrupt(&raw, ReplicaAckCorruption::RootDigestEncoding));
    }
    if ack.root_digest.as_ref() != Some(&bound_root_digest) {
        return Err(corrupt(&raw, ReplicaAckCorruption::RootDigestMismatch));
    }
    if raw.checkpoint_id != anchor.bound_checkpoint_id {
        return Err(corrupt(&raw, ReplicaAckCorruption::MissingCheckpointAnchor));
    }
    if bound_root_digest != anchor.bound_root_digest {
        return Err(corrupt(&raw, ReplicaAckCorruption::CheckpointRootMismatch));
    }

    Ok(StoredReplicaAckBinding {
        ack,
        ack_hash,
        signer_id_at_submission: raw.signer_id,
        bound_checkpoint_id: raw.checkpoint_id,
        bound_holder_device_id: raw.holder_device_id,
        bound_acked_at_unix_ms,
        bound_root_digest,
    })
}

fn corrupt(raw: &RawReplicaAckRow, kind: ReplicaAckCorruption) -> ReplicaAckStoreError {
    ReplicaAckStoreError::Corrupt {
        checkpoint_id: raw.checkpoint_id.clone(),
        holder_device_id: raw.holder_device_id.clone(),
        kind,
    }
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8]) -> Result<u64, ()> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| ())?;
    Ok(u64::from_be_bytes(bytes))
}

fn map_sql_error(error: SqlError) -> ReplicaAckStoreError {
    match error {
        SqlError::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            ReplicaAckStoreError::LockTimeout
        }
        SqlError::SqliteFailure(code, _) => ReplicaAckStoreError::Io(code.to_string()),
        other => ReplicaAckStoreError::Io(other.to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFault {
    AfterAckInsert,
}

#[cfg(test)]
fn fail_at(fault: Option<TestFault>, point: TestFault) -> Result<(), ReplicaAckStoreError> {
    if fault == Some(point) {
        Err(ReplicaAckStoreError::InjectedFailure(
            "after ReplicaAck insert",
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
fn fail_at(_fault: Option<TestFault>, _point: TestFault) -> Result<(), ReplicaAckStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint_manifest_store::CoordinatorCheckpointManifestStore;
    use crate::inventory_store::{
        AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory,
    };
    use crate::job_store::{AcceptedJobSubmission, CoordinatorJobStore};
    use crate::staging_store::{CoordinatorStagingStore, StageQueuedRequest};
    use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
    use gputeer_protocol::signing::{verify, NoReplayCheck};
    use std::path::PathBuf;

    const CHECKPOINT_ID: &str = "checkpoint-1";
    const JOB_ID: &str = "job-1";
    const ATTEMPT_ID: &str = "attempt-1";
    const LEASE_ID: &str = "lease-1";
    const NODE_ID: &str = "node-1";
    const HOLDER_ID: &str = "holder-1";

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

        let manifest = verified_manifest(3);
        CoordinatorCheckpointManifestStore::open(&path)
            .unwrap()
            .store_verified_manifest(&manifest)
            .unwrap();

        Fixture { _dir: dir, path }
    }

    fn verify_manifest(mut manifest: pb::CheckpointManifest) -> Verified<pb::CheckpointManifest> {
        let key = SigningKey::from_bytes(&[7; 32]);
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

    fn verified_manifest(root_byte: u8) -> Verified<pb::CheckpointManifest> {
        verify_manifest(pb::CheckpointManifest {
            schema_version: 1,
            checkpoint_id: CHECKPOINT_ID.into(),
            job_id: JOB_ID.into(),
            attempt_id: ATTEMPT_ID.into(),
            step: 10,
            epoch: 2,
            root_digest: Some(digest(root_byte)),
            total_bytes: 4096,
            created_at_unix_ms: 300,
            producer_node_id: NODE_ID.into(),
            fence_epoch: 1,
            ..Default::default()
        })
    }

    fn digest(byte: u8) -> pb::Digest {
        pb::Digest {
            algo: pb::HashAlgorithm::Blake3256 as i32,
            value: vec![byte; 32],
        }
    }

    fn verify_ack(mut ack: pb::ReplicaAck, key_seed: u8) -> Verified<pb::ReplicaAck> {
        let key = SigningKey::from_bytes(&[key_seed; 32]);
        ack.holder_signature = sign(&key, &ack).to_vec();
        let mut keys = InMemoryKeyring::new();
        keys.insert(&ack.holder_device_id, key.verifying_key());
        verify(
            &ack,
            1,
            &Ed25519Verifier::new(keys),
            999,
            &mut NoReplayCheck,
        )
        .expect("test ReplicaAck signature must verify")
    }

    fn verified_ack(
        holder_device_id: &str,
        acked_at_unix_ms: u64,
        root_byte: u8,
        key_seed: u8,
    ) -> Verified<pb::ReplicaAck> {
        verify_ack(
            pb::ReplicaAck {
                schema_version: 1,
                checkpoint_id: CHECKPOINT_ID.into(),
                root_digest: Some(digest(root_byte)),
                holder_device_id: holder_device_id.into(),
                kind: pb::ReplicaKind::TrustedPeer as i32,
                failure_domain: "rack-a".into(),
                fsynced: true,
                hash_verified: true,
                stored_bytes: 4096,
                acked_at_unix_ms,
                ..Default::default()
            },
            key_seed,
        )
    }

    fn valid_ack() -> Verified<pb::ReplicaAck> {
        verified_ack(HOLDER_ID, 400, 3, 8)
    }

    fn ack_count(store: &CoordinatorReplicaAckStore) -> u64 {
        store
            .connection
            .query_row("SELECT COUNT(*) FROM coordinator_replica_acks", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn rewrite_ack_body(store: &CoordinatorReplicaAckStore, ack: &pb::ReplicaAck) {
        let body = ack.encode_to_vec();
        let hash = blake3_256(&body);
        store
            .connection
            .execute(
                "UPDATE coordinator_replica_acks SET ack_body = ?1, ack_hash = ?2",
                rusqlite::params![body, hash.as_slice()],
            )
            .unwrap();
    }

    fn rewrite_manifest_root(store: &CoordinatorReplicaAckStore, root_byte: u8) {
        let binding =
            checkpoint_manifest_store::fetch_manifest_binding(&store.connection, CHECKPOINT_ID)
                .unwrap()
                .unwrap();
        let mut manifest = binding.manifest;
        manifest.root_digest = Some(digest(root_byte));
        let body = manifest.encode_to_vec();
        let hash = blake3_256(&body);
        let root = digest(root_byte).encode_to_vec();
        store
            .connection
            .execute(
                "UPDATE coordinator_checkpoint_manifests
                 SET root_digest = ?1, manifest_body = ?2, manifest_hash = ?3
                 WHERE checkpoint_id = ?4",
                rusqlite::params![root, body, hash.as_slice(), CHECKPOINT_ID],
            )
            .unwrap();
    }

    #[test]
    fn complete_signed_ack_and_store_derived_hash_survive_reopen() {
        let fixture = prepare_fixture();
        let verified = valid_ack();
        let original = verified.get().clone();
        let expected_hash = blake3_256(&original.encode_to_vec());
        {
            let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
            assert!(store.is_durable());
            let result = store.store_verified_ack(&verified).unwrap();
            assert!(result.created);
            assert_eq!(result.binding.ack, original);
            assert_eq!(result.binding.ack_hash, expected_hash);
            assert_eq!(result.binding.signer_id_at_submission, HOLDER_ID);
            assert_eq!(result.binding.bound_acked_at_unix_ms, 400);
        }

        let reopened = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        let binding = reopened
            .get_ack_binding(CHECKPOINT_ID, HOLDER_ID, 400)
            .unwrap()
            .unwrap();
        assert_eq!(binding.ack, original);
        assert_eq!(binding.ack.holder_signature, original.holder_signature);
        assert_eq!(binding.ack_hash, expected_hash);
        assert_eq!(binding.bound_root_digest, digest(3));
    }

    #[test]
    fn missing_anchor_wrong_root_sha256_and_wrong_length_create_no_row() {
        let fixture = prepare_fixture();
        let store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        store
            .connection
            .execute(
                "DELETE FROM coordinator_checkpoint_manifests WHERE checkpoint_id = ?1",
                rusqlite::params![CHECKPOINT_ID],
            )
            .unwrap();
        drop(store);
        let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_ack(&valid_ack()),
            Err(ReplicaAckStoreError::CheckpointNotFound {
                checkpoint_id: CHECKPOINT_ID.into(),
            })
        );
        assert_eq!(ack_count(&store), 0);

        for mutation in ["different", "sha256", "length"] {
            let fixture = prepare_fixture();
            let mut ack = valid_ack().get().clone();
            match mutation {
                "different" => ack.root_digest = Some(digest(9)),
                "sha256" => {
                    ack.root_digest.as_mut().unwrap().algo = pb::HashAlgorithm::Sha256 as i32;
                }
                "length" => {
                    ack.root_digest.as_mut().unwrap().value.pop();
                }
                _ => unreachable!(),
            }
            let verified = verify_ack(ack, 8);
            let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
            let result = store.store_verified_ack(&verified);
            if mutation == "different" {
                assert_eq!(
                    result,
                    Err(ReplicaAckStoreError::BindingMismatch(
                        ReplicaAckBindingField::RootDigest,
                    ))
                );
            } else {
                assert_eq!(
                    result,
                    Err(ReplicaAckStoreError::InvalidInput("root_digest"))
                );
            }
            assert_eq!(ack_count(&store), 0, "mutation {mutation} inserted a row");
        }
    }

    #[test]
    fn exact_replay_returns_first_row_changed_replay_conflicts_and_later_ack_is_separate() {
        let fixture = prepare_fixture();
        let ack = valid_ack();
        let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        let first = store.store_verified_ack(&ack).unwrap();
        let replay = store.store_verified_ack(&ack).unwrap();
        assert!(first.created);
        assert!(!replay.created);
        assert_eq!(replay.binding, first.binding);

        let mut changed_body = ack.get().clone();
        changed_body.failure_domain = "rack-b".into();
        let changed_body = verify_ack(changed_body, 8);
        assert!(matches!(
            store.store_verified_ack(&changed_body),
            Err(ReplicaAckStoreError::AckConflict { .. })
        ));
        let changed_signature = verified_ack(HOLDER_ID, 400, 3, 9);
        assert!(matches!(
            store.store_verified_ack(&changed_signature),
            Err(ReplicaAckStoreError::AckConflict { .. })
        ));

        let later = verified_ack(HOLDER_ID, 401, 3, 8);
        assert!(store.store_verified_ack(&later).unwrap().created);
        assert_eq!(ack_count(&store), 2);
    }

    #[test]
    fn list_is_deterministic_and_does_not_filter_replica_claims() {
        let fixture = prepare_fixture();
        let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        let mut ineligible_claim = verified_ack("holder-z", 399, 3, 10).get().clone();
        ineligible_claim.fsynced = false;
        ineligible_claim.hash_verified = false;
        ineligible_claim.kind = pb::ReplicaKind::Unspecified as i32;
        let ineligible_claim = verify_ack(ineligible_claim, 10);
        store.store_verified_ack(&valid_ack()).unwrap();
        store.store_verified_ack(&ineligible_claim).unwrap();
        store
            .store_verified_ack(&verified_ack("holder-a", 400, 3, 11))
            .unwrap();

        let listed = store.list_ack_bindings(CHECKPOINT_ID).unwrap();
        let keys: Vec<_> = listed
            .iter()
            .map(|binding| {
                (
                    binding.bound_acked_at_unix_ms,
                    binding.bound_holder_device_id.as_str(),
                )
            })
            .collect();
        assert_eq!(
            keys,
            vec![(399, "holder-z"), (400, "holder-1"), (400, "holder-a")]
        );
        assert!(!listed[0].ack.fsynced);
        assert!(!listed[0].ack.hash_verified);
    }

    #[test]
    fn failure_after_insert_rolls_back_and_control_rows_are_unchanged() {
        let fixture = prepare_fixture();
        let before: Vec<(String, i64)> = {
            let store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
            [
                "coordinator_jobs",
                "coordinator_attempts",
                "coordinator_leases",
                "coordinator_node_reservations",
                "coordinator_checkpoint_manifests",
            ]
            .into_iter()
            .map(|table| {
                let count = store
                    .connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                (table.to_string(), count)
            })
            .collect()
        };
        let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_ack_inner(&valid_ack(), Some(TestFault::AfterAckInsert)),
            Err(ReplicaAckStoreError::InjectedFailure(
                "after ReplicaAck insert"
            ))
        );
        assert_eq!(ack_count(&store), 0);
        for (table, expected) in before {
            let actual: i64 = store
                .connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(actual, expected, "storage changed control table {table}");
        }
        let columns = {
            let mut statement = store
                .connection
                .prepare("PRAGMA table_info(coordinator_replica_acks)")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(!columns.iter().any(|column| {
            column.contains("state") || column.contains("count") || column.contains("mirrored")
        }));
    }

    #[test]
    fn corrupt_body_hash_identity_signer_time_and_root_fail_closed() {
        for expected in [
            ReplicaAckCorruption::EmptyBody,
            ReplicaAckCorruption::UndecodableBody,
            ReplicaAckCorruption::HashEncoding,
            ReplicaAckCorruption::HashMismatch,
            ReplicaAckCorruption::InvalidAck,
            ReplicaAckCorruption::CheckpointIdMismatch,
            ReplicaAckCorruption::HolderDeviceIdMismatch,
            ReplicaAckCorruption::SignerIdMismatch,
            ReplicaAckCorruption::AckedAtEncoding,
            ReplicaAckCorruption::AckedAtMismatch,
            ReplicaAckCorruption::RootDigestEncoding,
            ReplicaAckCorruption::RootDigestMismatch,
        ] {
            let fixture = prepare_fixture();
            let ack = valid_ack();
            let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
            store.store_verified_ack(&ack).unwrap();
            match expected {
                ReplicaAckCorruption::EmptyBody => {
                    store
                        .connection
                        .execute("UPDATE coordinator_replica_acks SET ack_body = X''", [])
                        .unwrap();
                }
                ReplicaAckCorruption::UndecodableBody => {
                    let body = vec![0x12, 0x05, b'a'];
                    let hash = blake3_256(&body);
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_replica_acks SET ack_body = ?1, ack_hash = ?2",
                            rusqlite::params![body, hash.as_slice()],
                        )
                        .unwrap();
                }
                ReplicaAckCorruption::HashEncoding => {
                    store
                        .connection
                        .execute_batch("PRAGMA ignore_check_constraints = ON;")
                        .unwrap();
                    store
                        .connection
                        .execute("UPDATE coordinator_replica_acks SET ack_hash = X'01'", [])
                        .unwrap();
                }
                ReplicaAckCorruption::HashMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_replica_acks SET ack_hash = ?1",
                            rusqlite::params![[9u8; 32].as_slice()],
                        )
                        .unwrap();
                }
                ReplicaAckCorruption::InvalidAck => {
                    let mut changed = ack.get().clone();
                    changed.root_digest.as_mut().unwrap().algo = 0;
                    rewrite_ack_body(&store, &changed);
                }
                ReplicaAckCorruption::CheckpointIdMismatch => {
                    let mut changed = ack.get().clone();
                    changed.checkpoint_id = "checkpoint-other".into();
                    rewrite_ack_body(&store, &changed);
                }
                ReplicaAckCorruption::HolderDeviceIdMismatch => {
                    let mut changed = ack.get().clone();
                    changed.holder_device_id = "holder-other".into();
                    rewrite_ack_body(&store, &changed);
                }
                ReplicaAckCorruption::SignerIdMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_replica_acks SET verified_signer_id = 'holder-other'",
                            [],
                        )
                        .unwrap();
                }
                ReplicaAckCorruption::AckedAtEncoding => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_replica_acks SET acked_at_unix_ms = X'01'",
                            [],
                        )
                        .unwrap();
                }
                ReplicaAckCorruption::AckedAtMismatch => {
                    let mut changed = ack.get().clone();
                    changed.acked_at_unix_ms = 401;
                    rewrite_ack_body(&store, &changed);
                }
                ReplicaAckCorruption::RootDigestEncoding => {
                    store
                        .connection
                        .execute("UPDATE coordinator_replica_acks SET root_digest = X''", [])
                        .unwrap();
                }
                ReplicaAckCorruption::RootDigestMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_replica_acks SET root_digest = ?1",
                            rusqlite::params![digest(9).encode_to_vec()],
                        )
                        .unwrap();
                }
                _ => unreachable!(),
            }
            let loaded = if expected == ReplicaAckCorruption::AckedAtEncoding {
                store.list_ack_bindings(CHECKPOINT_ID).map(|_| None)
            } else {
                store.get_ack_binding(CHECKPOINT_ID, HOLDER_ID, 400)
            };
            assert_eq!(
                loaded,
                Err(ReplicaAckStoreError::Corrupt {
                    checkpoint_id: CHECKPOINT_ID.into(),
                    holder_device_id: HOLDER_ID.into(),
                    kind: expected,
                })
            );
        }
    }

    #[test]
    fn missing_corrupt_or_changed_checkpoint_anchor_fails_closed() {
        let fixture = prepare_fixture();
        let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        store.store_verified_ack(&valid_ack()).unwrap();
        store
            .connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        store
            .connection
            .execute(
                "DELETE FROM coordinator_checkpoint_manifests WHERE checkpoint_id = ?1",
                rusqlite::params![CHECKPOINT_ID],
            )
            .unwrap();
        assert_eq!(
            store.get_ack_binding(CHECKPOINT_ID, HOLDER_ID, 400),
            Err(ReplicaAckStoreError::Corrupt {
                checkpoint_id: CHECKPOINT_ID.into(),
                holder_device_id: HOLDER_ID.into(),
                kind: ReplicaAckCorruption::MissingCheckpointAnchor,
            })
        );

        let fixture = prepare_fixture();
        let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        store.store_verified_ack(&valid_ack()).unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_checkpoint_manifests SET manifest_hash = ?1",
                rusqlite::params![[9u8; 32].as_slice()],
            )
            .unwrap();
        assert!(matches!(
            store.get_ack_binding(CHECKPOINT_ID, HOLDER_ID, 400),
            Err(ReplicaAckStoreError::CheckpointAnchor(
                CheckpointManifestStoreError::Corrupt { .. }
            ))
        ));

        let fixture = prepare_fixture();
        let mut store = CoordinatorReplicaAckStore::open(&fixture.path).unwrap();
        store.store_verified_ack(&valid_ack()).unwrap();
        rewrite_manifest_root(&store, 9);
        assert_eq!(
            store.get_ack_binding(CHECKPOINT_ID, HOLDER_ID, 400),
            Err(ReplicaAckStoreError::Corrupt {
                checkpoint_id: CHECKPOINT_ID.into(),
                holder_device_id: HOLDER_ID.into(),
                kind: ReplicaAckCorruption::CheckpointRootMismatch,
            })
        );
    }
}
