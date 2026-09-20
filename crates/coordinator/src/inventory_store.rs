//! Single-Coordinator durable Agent inventory repository kernel.
//!
//! Callers provide identity-checked, normalized facts. This module performs no
//! enrollment, signature verification, heartbeat collection, session ownership,
//! clock reads, or network I/O. It atomically persists the latest per-Agent
//! inventory and projects it into [`gputeer_scheduler::PoolSnapshot`].

use std::{collections::BTreeSet, path::Path};

use gputeer_scheduler::{
    CandidateSnapshot, GpuSnapshot, IsolationClass, KeyProtection, NodeState, PoolSnapshot,
    RiskState, SecurityTier, WorkloadClass,
};
use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRegistry {
    pub node_id: String,
    pub device_id: String,
    pub owner_member_id: String,
    pub verifying_key: Vec<u8>,
    pub node_state: Option<NodeState>,
    pub risk_state: Option<RiskState>,
    pub security_tier: Option<SecurityTier>,
    pub isolation_class: Option<IsolationClass>,
    pub key_protection: Option<KeyProtection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInventory {
    pub gpu_id: String,
    pub model: Option<String>,
    pub healthy: Option<bool>,
    pub available_vram_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInventory {
    pub node_id: String,
    pub inventory_revision: u64,
    pub observed_at_unix_ms: u64,
    /// `None` means the GPU inventory was not observed; `Some(vec![])` means it
    /// was observed and the node reported no GPUs.
    pub gpus: Option<Vec<GpuInventory>>,
    pub available_cpu_cores: Option<u32>,
    pub available_ram_bytes: Option<u64>,
    pub available_workspace_bytes: Option<u64>,
    /// `None` means the policy fact was not observed; an empty set explicitly
    /// means that no workload class is allowed.
    pub allowed_workload_classes: Option<BTreeSet<WorkloadClass>>,
    pub third_party_workloads_opt_in: Option<bool>,
}

/// One Agent's registration and its inventory, imported together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentBootstrap {
    pub registry: AgentRegistry,
    pub inventory: AgentInventory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBootstrapResult {
    pub registered: usize,
    pub inventories_updated: usize,
    pub entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterAgentResult {
    pub registry: AgentRegistry,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInventoryResult {
    pub inventory: AgentInventory,
    pub updated: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InventoryStoreError {
    InvalidInput(&'static str),
    AgentNotFound {
        node_id: String,
    },
    RegistryConflict {
        field: &'static str,
    },
    LowerRevision {
        stored: u64,
        requested: u64,
    },
    RevisionConflict {
        revision: u64,
    },
    /// A bootstrap entry failed. Carries the offending entry so the caller can
    /// say *which* Agent stopped the import — nothing was written.
    BootstrapEntry {
        index: usize,
        node_id: String,
        source: Box<InventoryStoreError>,
    },
    CorruptData(String),
    Io(String),
    LockTimeout,
    #[cfg(test)]
    InjectedFailure(&'static str),
}

impl std::fmt::Display for InventoryStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(field) => write!(f, "invalid inventory store input: {field}"),
            Self::AgentNotFound { node_id } => {
                write!(f, "Agent registry row is absent: {node_id}")
            }
            Self::RegistryConflict { field } => {
                write!(f, "Agent registry identity conflict: {field}")
            }
            Self::LowerRevision { stored, requested } => write!(
                f,
                "inventory revision moved backwards: stored={stored}, requested={requested}"
            ),
            Self::RevisionConflict { revision } => write!(
                f,
                "inventory revision {revision} already has a different payload"
            ),
            Self::BootstrapEntry {
                index,
                node_id,
                source,
            } => write!(
                f,
                "bootstrap entry {index} ({node_id}) rejected, nothing was imported: {source}"
            ),
            Self::CorruptData(message) => write!(f, "inventory store corruption: {message}"),
            Self::Io(message) => write!(f, "inventory store I/O error: {message}"),
            Self::LockTimeout => write!(f, "inventory store lock acquisition timed out"),
            #[cfg(test)]
            Self::InjectedFailure(point) => write!(f, "injected inventory failure: {point}"),
        }
    }
}

impl std::error::Error for InventoryStoreError {}

pub struct CoordinatorInventoryStore {
    connection: Connection,
}

impl CoordinatorInventoryStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, InventoryStoreError> {
        let connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        initialize_schema(&connection)?;
        Ok(Self { connection })
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.connection.path(), Some(path) if !path.is_empty() && path != ":memory:")
    }

    pub fn get_agent(&self, node_id: &str) -> Result<Option<AgentRegistry>, InventoryStoreError> {
        fetch_registry(&self.connection, node_id)
    }

    pub fn get_inventory(
        &self,
        node_id: &str,
    ) -> Result<Option<AgentInventory>, InventoryStoreError> {
        fetch_inventory(&self.connection, node_id)
    }

    /// Registers immutable local identity and scheduler facts. Only a
    /// byte-equivalent normalized payload is idempotent.
    pub fn register_agent(
        &mut self,
        registry: &AgentRegistry,
    ) -> Result<RegisterAgentResult, InventoryStoreError> {
        // ★ Validate **before** taking the write lock. The refactor that
        //   extracted the in-transaction body moved validation inside the
        //   lock, so bad input against a busy database started returning
        //   `LockTimeout` instead of `InvalidInput` — an input error reported
        //   as a contention error (independent review round 1).
        validate_registry(registry).map_err(InventoryStoreError::InvalidInput)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        let result = register_agent_in_tx(&transaction, registry)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(result)
    }
}

/// Registers one Agent **inside a caller-owned transaction**. Does not commit.
///
/// Extracted so that [`CoordinatorInventoryStore::import_bootstrap`] can put
/// several registrations and inventory updates under one `BEGIN IMMEDIATE`.
/// Composing the public single-entry APIs would not be equivalent: each of
/// those commits on its own, so a later failure would leave earlier rows
/// behind.
///
/// # Precondition
///
/// `registry` must already have passed [`validate_registry`]. Callers do that
/// **before** opening the transaction so that an input error is never reported
/// as lock contention.
fn register_agent_in_tx(
    transaction: &rusqlite::Transaction<'_>,
    registry: &AgentRegistry,
) -> Result<RegisterAgentResult, InventoryStoreError> {
    {
        debug_assert!(
            validate_registry(registry).is_ok(),
            "caller must validate first"
        );
        let payload = encode_registry(registry);

        if let Some(stored) = fetch_registry(transaction, &registry.node_id)? {
            if encode_registry(&stored) == payload {
                return Ok(RegisterAgentResult {
                    registry: stored,
                    created: false,
                });
            }
            return Err(InventoryStoreError::RegistryConflict { field: "node_id" });
        }
        if registry_identity_exists(transaction, "device_id", &registry.device_id)? {
            return Err(InventoryStoreError::RegistryConflict { field: "device_id" });
        }
        if registry_key_exists(transaction, &registry.verifying_key)? {
            return Err(InventoryStoreError::RegistryConflict {
                field: "verifying_key",
            });
        }

        transaction
            .execute(
                "INSERT INTO coordinator_agent_registry(
                    node_id, device_id, owner_member_id, verifying_key, node_state,
                    risk_state, security_tier, isolation_class, key_protection, payload
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    registry.node_id,
                    registry.device_id,
                    registry.owner_member_id,
                    registry.verifying_key.as_slice(),
                    registry.node_state.map(node_state_code),
                    registry.risk_state.map(risk_state_code),
                    registry.security_tier.map(security_tier_code),
                    registry.isolation_class.map(isolation_class_code),
                    registry.key_protection.map(key_protection_code),
                    payload,
                ],
            )
            .map_err(map_sql_error)?;
        Ok(RegisterAgentResult {
            registry: registry.clone(),
            created: true,
        })
    }
}

impl CoordinatorInventoryStore {
    pub fn update_inventory(
        &mut self,
        inventory: &AgentInventory,
    ) -> Result<UpdateInventoryResult, InventoryStoreError> {
        self.update_inventory_inner(inventory, None)
    }

    fn update_inventory_inner(
        &mut self,
        inventory: &AgentInventory,
        fault: Option<TestFault>,
    ) -> Result<UpdateInventoryResult, InventoryStoreError> {
        // ★ Same reason as `register_agent`: normalize and validate before
        //   taking the write lock (independent review round 1).
        let normalized = normalized_and_validated(inventory)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        let result = update_inventory_in_tx(&transaction, &normalized, fault)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(result)
    }
}

/// Sorts GPUs into canonical order and validates. Runs **outside** any
/// transaction so an input error never surfaces as lock contention.
fn normalized_and_validated(
    inventory: &AgentInventory,
) -> Result<AgentInventory, InventoryStoreError> {
    let mut normalized = inventory.clone();
    if let Some(gpus) = normalized.gpus.as_mut() {
        gpus.sort_by(|left, right| left.gpu_id.cmp(&right.gpu_id));
    }
    validate_inventory(&normalized).map_err(InventoryStoreError::InvalidInput)?;
    Ok(normalized)
}

/// Replaces one Agent's inventory **inside a caller-owned transaction**.
/// Does not commit. See [`register_agent_in_tx`] for why this is extracted.
///
/// # Precondition
///
/// `inventory` must already have gone through [`normalized_and_validated`].
fn update_inventory_in_tx(
    transaction: &rusqlite::Transaction<'_>,
    inventory: &AgentInventory,
    fault: Option<TestFault>,
) -> Result<UpdateInventoryResult, InventoryStoreError> {
    {
        debug_assert!(
            validate_inventory(inventory).is_ok(),
            "caller must normalize and validate first"
        );
        let payload = encode_inventory_payload(inventory);
        if fetch_registry(transaction, &inventory.node_id)?.is_none() {
            return Err(InventoryStoreError::AgentNotFound {
                node_id: inventory.node_id.clone(),
            });
        }
        if let Some(stored) = fetch_inventory(transaction, &inventory.node_id)? {
            if inventory.inventory_revision < stored.inventory_revision {
                return Err(InventoryStoreError::LowerRevision {
                    stored: stored.inventory_revision,
                    requested: inventory.inventory_revision,
                });
            }
            if inventory.inventory_revision == stored.inventory_revision {
                if payload == encode_inventory_payload(&stored) {
                    return Ok(UpdateInventoryResult {
                        inventory: stored,
                        updated: false,
                    });
                }
                return Err(InventoryStoreError::RevisionConflict {
                    revision: inventory.inventory_revision,
                });
            }
        }

        transaction
            .execute(
                "INSERT INTO coordinator_agent_inventory(
                    node_id, inventory_revision, observed_at_unix_ms, gpus_observed,
                    available_cpu_cores, available_ram_bytes, available_workspace_bytes,
                    workload_classes_observed, third_party_workloads_opt_in, payload
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(node_id) DO UPDATE SET
                    inventory_revision = excluded.inventory_revision,
                    observed_at_unix_ms = excluded.observed_at_unix_ms,
                    gpus_observed = excluded.gpus_observed,
                    available_cpu_cores = excluded.available_cpu_cores,
                    available_ram_bytes = excluded.available_ram_bytes,
                    available_workspace_bytes = excluded.available_workspace_bytes,
                    workload_classes_observed = excluded.workload_classes_observed,
                    third_party_workloads_opt_in = excluded.third_party_workloads_opt_in,
                    payload = excluded.payload",
                rusqlite::params![
                    inventory.node_id,
                    encode_u64(inventory.inventory_revision),
                    encode_u64(inventory.observed_at_unix_ms),
                    bool_to_i64(inventory.gpus.is_some()),
                    inventory
                        .available_cpu_cores
                        .map(|value| encode_u64(value.into())),
                    inventory.available_ram_bytes.map(encode_u64),
                    inventory.available_workspace_bytes.map(encode_u64),
                    bool_to_i64(inventory.allowed_workload_classes.is_some()),
                    inventory.third_party_workloads_opt_in.map(bool_to_i64),
                    payload,
                ],
            )
            .map_err(map_sql_error)?;
        fail_at(fault, TestFault::AfterParentWrite)?;

        transaction
            .execute(
                "DELETE FROM coordinator_agent_gpus WHERE node_id = ?1",
                rusqlite::params![inventory.node_id],
            )
            .map_err(map_sql_error)?;
        let mut gpus = inventory.gpus.clone().unwrap_or_default();
        gpus.sort_by(|left, right| left.gpu_id.cmp(&right.gpu_id));
        if gpus.is_empty() {
            fail_at(fault, TestFault::DuringGpuReplacement)?;
        }
        for (index, gpu) in gpus.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO coordinator_agent_gpus(
                        node_id, gpu_id, model, healthy, available_vram_bytes
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        inventory.node_id,
                        gpu.gpu_id,
                        gpu.model,
                        gpu.healthy.map(bool_to_i64),
                        gpu.available_vram_bytes.map(encode_u64),
                    ],
                )
                .map_err(map_sql_error)?;
            if index == 0 {
                fail_at(fault, TestFault::DuringGpuReplacement)?;
            }
        }

        transaction
            .execute(
                "DELETE FROM coordinator_agent_workload_classes WHERE node_id = ?1",
                rusqlite::params![inventory.node_id],
            )
            .map_err(map_sql_error)?;
        let workloads = inventory
            .allowed_workload_classes
            .clone()
            .unwrap_or_default();
        if workloads.is_empty() {
            fail_at(fault, TestFault::DuringWorkloadReplacement)?;
        }
        for (index, workload) in workloads.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO coordinator_agent_workload_classes(node_id, workload_class)
                     VALUES (?1, ?2)",
                    rusqlite::params![inventory.node_id, workload_class_code(*workload)],
                )
                .map_err(map_sql_error)?;
            if index == 0 {
                fail_at(fault, TestFault::DuringWorkloadReplacement)?;
            }
        }
        Ok(UpdateInventoryResult {
            inventory: inventory.clone(),
            updated: true,
        })
    }
}

impl CoordinatorInventoryStore {
    /// Imports several Agents' registration **and** inventory in **one**
    /// `BEGIN IMMEDIATE` transaction. Either every entry lands or none does.
    ///
    /// # Why this exists instead of calling the two single-entry APIs
    ///
    /// [`Self::register_agent`] and [`Self::update_inventory`] each own their
    /// transaction and commit on their own. Calling them in sequence is **not**
    /// atomic: once a registration commits, a later inventory failure (node
    /// mismatch, revision conflict, I/O) leaves the registry row behind. A
    /// caller that reports "rejected" while rows survive is reporting something
    /// that did not happen.
    ///
    /// This kernel performs no signature verification, membership judgment, or
    /// clock read. `entries` are facts the caller already decided to trust; the
    /// store only checks that they are internally consistent and do not
    /// conflict with what is already stored.
    pub fn import_bootstrap(
        &mut self,
        entries: &[AgentBootstrap],
    ) -> Result<ImportBootstrapResult, InventoryStoreError> {
        // ★ Everything that can be judged from the input alone is judged
        //   **before** the write lock: shape, identity agreement, and each
        //   half's own validity. Only checks that need stored rows (identity
        //   conflicts, revision ordering) happen under the lock — those cannot
        //   be hoisted, because they are questions about the database.
        let mut prepared = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let attribute = |source: InventoryStoreError| InventoryStoreError::BootstrapEntry {
                index,
                node_id: entry.registry.node_id.clone(),
                source: Box::new(source),
            };
            // The two halves must name the same node. The store would otherwise
            // register one node and attach the inventory to another — or to
            // nothing at all, which surfaces as a confusing `AgentNotFound`.
            if entry.registry.node_id != entry.inventory.node_id {
                return Err(attribute(InventoryStoreError::InvalidInput(
                    "registry/inventory node_id mismatch",
                )));
            }
            validate_registry(&entry.registry)
                .map_err(|field| attribute(InventoryStoreError::InvalidInput(field)))?;
            prepared.push(normalized_and_validated(&entry.inventory).map_err(attribute)?);
        }

        // The empty import is not an error, but it must not be reported as work
        // either — `registered: 0` says exactly what happened.
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let mut registered = 0usize;
        let mut inventories_updated = 0usize;
        for (index, (entry, inventory)) in entries.iter().zip(prepared.iter()).enumerate() {
            let attribute = |source: InventoryStoreError| InventoryStoreError::BootstrapEntry {
                index,
                node_id: entry.registry.node_id.clone(),
                source: Box::new(source),
            };
            let registration =
                register_agent_in_tx(&transaction, &entry.registry).map_err(attribute)?;
            if registration.created {
                registered += 1;
            }
            let update =
                update_inventory_in_tx(&transaction, inventory, None).map_err(attribute)?;
            if update.updated {
                inventories_updated += 1;
            }
        }

        transaction.commit().map_err(map_sql_error)?;
        Ok(ImportBootstrapResult {
            registered,
            inventories_updated,
            entries: entries.len(),
        })
    }

    /// Reads registry and inventory rows from one SQLite snapshot and projects
    /// candidates in deterministic `node_id` order. The supplied time is copied
    /// only to the pool; stored observation timestamps are never rewritten.
    pub fn pool_snapshot(
        &mut self,
        evaluated_at_unix_ms: u64,
    ) -> Result<PoolSnapshot, InventoryStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(map_sql_error)?;
        let registries = fetch_all_registries(&transaction)?;
        let mut candidates = Vec::with_capacity(registries.len());
        for registry in registries {
            let inventory = fetch_inventory(&transaction, &registry.node_id)?;
            candidates.push(project_candidate(registry, inventory));
        }
        candidates.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        transaction.commit().map_err(map_sql_error)?;
        Ok(PoolSnapshot {
            evaluated_at_unix_ms,
            candidates,
        })
    }
}

fn initialize_schema(connection: &Connection) -> Result<(), InventoryStoreError> {
    connection
        .execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = DELETE;
            PRAGMA synchronous = FULL;

            CREATE TABLE IF NOT EXISTS coordinator_agent_registry (
                node_id TEXT PRIMARY KEY,
                device_id TEXT NOT NULL UNIQUE,
                owner_member_id TEXT NOT NULL,
                verifying_key BLOB NOT NULL UNIQUE,
                node_state TEXT,
                risk_state TEXT,
                security_tier TEXT,
                isolation_class TEXT,
                key_protection TEXT,
                payload BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS coordinator_agent_inventory (
                node_id TEXT PRIMARY KEY REFERENCES coordinator_agent_registry(node_id),
                inventory_revision BLOB NOT NULL,
                observed_at_unix_ms BLOB NOT NULL,
                gpus_observed INTEGER NOT NULL,
                available_cpu_cores BLOB,
                available_ram_bytes BLOB,
                available_workspace_bytes BLOB,
                workload_classes_observed INTEGER NOT NULL,
                third_party_workloads_opt_in INTEGER,
                payload BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS coordinator_agent_gpus (
                node_id TEXT NOT NULL REFERENCES coordinator_agent_inventory(node_id),
                gpu_id TEXT NOT NULL,
                model TEXT,
                healthy INTEGER,
                available_vram_bytes BLOB,
                PRIMARY KEY(node_id, gpu_id)
            );
            CREATE TABLE IF NOT EXISTS coordinator_agent_workload_classes (
                node_id TEXT NOT NULL REFERENCES coordinator_agent_inventory(node_id),
                workload_class TEXT NOT NULL,
                PRIMARY KEY(node_id, workload_class)
            );
            "#,
        )
        .map_err(map_sql_error)
}

fn validate_registry(registry: &AgentRegistry) -> Result<(), &'static str> {
    for (field, value) in [
        ("node_id", &registry.node_id),
        ("device_id", &registry.device_id),
        ("owner_member_id", &registry.owner_member_id),
    ] {
        if value.trim().is_empty() {
            return Err(field);
        }
    }
    if registry.verifying_key.len() != 32 {
        return Err("verifying_key");
    }
    Ok(())
}

fn validate_inventory(inventory: &AgentInventory) -> Result<(), &'static str> {
    if inventory.node_id.trim().is_empty() {
        return Err("node_id");
    }
    if let Some(gpus) = &inventory.gpus {
        let mut ids = BTreeSet::new();
        for gpu in gpus {
            if gpu.gpu_id.trim().is_empty() {
                return Err("gpu_id");
            }
            if !ids.insert(gpu.gpu_id.as_str()) {
                return Err("duplicate gpu_id");
            }
            if gpu
                .model
                .as_deref()
                .is_some_and(|model| model.trim().is_empty())
            {
                return Err("gpu model");
            }
        }
    }
    Ok(())
}

fn registry_identity_exists(
    connection: &Connection,
    column: &str,
    value: &str,
) -> Result<bool, InventoryStoreError> {
    debug_assert_eq!(column, "device_id");
    connection
        .query_row(
            "SELECT 1 FROM coordinator_agent_registry WHERE device_id = ?1",
            rusqlite::params![value],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(map_sql_error)
}

fn registry_key_exists(connection: &Connection, key: &[u8]) -> Result<bool, InventoryStoreError> {
    connection
        .query_row(
            "SELECT 1 FROM coordinator_agent_registry WHERE verifying_key = ?1",
            rusqlite::params![key],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(map_sql_error)
}

#[derive(Debug)]
struct RawRegistry {
    node_id: String,
    device_id: String,
    owner_member_id: String,
    verifying_key: Vec<u8>,
    node_state: Option<String>,
    risk_state: Option<String>,
    security_tier: Option<String>,
    isolation_class: Option<String>,
    key_protection: Option<String>,
    payload: Vec<u8>,
}

impl RawRegistry {
    fn into_registry(self) -> Result<AgentRegistry, InventoryStoreError> {
        let registry = AgentRegistry {
            node_id: self.node_id,
            device_id: self.device_id,
            owner_member_id: self.owner_member_id,
            verifying_key: self.verifying_key,
            node_state: self
                .node_state
                .as_deref()
                .map(parse_node_state)
                .transpose()?,
            risk_state: self
                .risk_state
                .as_deref()
                .map(parse_risk_state)
                .transpose()?,
            security_tier: self
                .security_tier
                .as_deref()
                .map(parse_security_tier)
                .transpose()?,
            isolation_class: self
                .isolation_class
                .as_deref()
                .map(parse_isolation_class)
                .transpose()?,
            key_protection: self
                .key_protection
                .as_deref()
                .map(parse_key_protection)
                .transpose()?,
        };
        validate_registry(&registry).map_err(|field| {
            InventoryStoreError::CorruptData(format!("registry has invalid {field}"))
        })?;
        if encode_registry(&registry) != self.payload {
            return Err(InventoryStoreError::CorruptData(
                "registry normalized payload does not match its columns".into(),
            ));
        }
        Ok(registry)
    }
}

fn row_to_raw_registry(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRegistry> {
    Ok(RawRegistry {
        node_id: row.get(0)?,
        device_id: row.get(1)?,
        owner_member_id: row.get(2)?,
        verifying_key: row.get(3)?,
        node_state: row.get(4)?,
        risk_state: row.get(5)?,
        security_tier: row.get(6)?,
        isolation_class: row.get(7)?,
        key_protection: row.get(8)?,
        payload: row.get(9)?,
    })
}

const SELECT_REGISTRY: &str = "SELECT node_id, device_id, owner_member_id, verifying_key, \
    node_state, risk_state, security_tier, isolation_class, key_protection, payload \
    FROM coordinator_agent_registry";

fn fetch_registry(
    connection: &Connection,
    node_id: &str,
) -> Result<Option<AgentRegistry>, InventoryStoreError> {
    connection
        .query_row(
            &format!("{SELECT_REGISTRY} WHERE node_id = ?1"),
            rusqlite::params![node_id],
            row_to_raw_registry,
        )
        .optional()
        .map_err(map_sql_error)?
        .map(RawRegistry::into_registry)
        .transpose()
}

fn fetch_all_registries(
    connection: &Connection,
) -> Result<Vec<AgentRegistry>, InventoryStoreError> {
    let mut statement = connection.prepare(SELECT_REGISTRY).map_err(map_sql_error)?;
    let rows = statement
        .query_map([], row_to_raw_registry)
        .map_err(map_sql_error)?;
    let mut registries = Vec::new();
    for row in rows {
        registries.push(row.map_err(map_sql_error)?.into_registry()?);
    }
    Ok(registries)
}

#[derive(Debug)]
struct RawInventory {
    node_id: String,
    inventory_revision: Vec<u8>,
    observed_at_unix_ms: Vec<u8>,
    gpus_observed: i64,
    available_cpu_cores: Option<Vec<u8>>,
    available_ram_bytes: Option<Vec<u8>>,
    available_workspace_bytes: Option<Vec<u8>>,
    workload_classes_observed: i64,
    third_party_workloads_opt_in: Option<i64>,
    payload: Vec<u8>,
}

fn fetch_inventory(
    connection: &Connection,
    node_id: &str,
) -> Result<Option<AgentInventory>, InventoryStoreError> {
    let raw = connection
        .query_row(
            "SELECT node_id, inventory_revision, observed_at_unix_ms, gpus_observed,
                    available_cpu_cores, available_ram_bytes, available_workspace_bytes,
                    workload_classes_observed, third_party_workloads_opt_in, payload
             FROM coordinator_agent_inventory WHERE node_id = ?1",
            rusqlite::params![node_id],
            |row| {
                Ok(RawInventory {
                    node_id: row.get(0)?,
                    inventory_revision: row.get(1)?,
                    observed_at_unix_ms: row.get(2)?,
                    gpus_observed: row.get(3)?,
                    available_cpu_cores: row.get(4)?,
                    available_ram_bytes: row.get(5)?,
                    available_workspace_bytes: row.get(6)?,
                    workload_classes_observed: row.get(7)?,
                    third_party_workloads_opt_in: row.get(8)?,
                    payload: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some(raw) = raw else { return Ok(None) };

    let all_gpus = fetch_gpus(connection, node_id)?;
    let gpus = if parse_bool(raw.gpus_observed, "gpus_observed")? {
        Some(all_gpus)
    } else if all_gpus.is_empty() {
        None
    } else {
        return Err(InventoryStoreError::CorruptData(
            "GPU child rows exist while gpus_observed is false".into(),
        ));
    };
    let all_workloads = fetch_workloads(connection, node_id)?;
    let allowed_workload_classes =
        if parse_bool(raw.workload_classes_observed, "workload_classes_observed")? {
            Some(all_workloads)
        } else if all_workloads.is_empty() {
            None
        } else {
            return Err(InventoryStoreError::CorruptData(
                "workload child rows exist while workload_classes_observed is false".into(),
            ));
        };
    let cpu = raw
        .available_cpu_cores
        .as_deref()
        .map(|bytes| decode_u64(bytes, "available_cpu_cores"))
        .transpose()?
        .map(|value| {
            u32::try_from(value).map_err(|_| {
                InventoryStoreError::CorruptData(
                    "available_cpu_cores exceeds the scheduler u32 range".into(),
                )
            })
        })
        .transpose()?;
    let inventory = AgentInventory {
        node_id: raw.node_id,
        inventory_revision: decode_u64(&raw.inventory_revision, "inventory_revision")?,
        observed_at_unix_ms: decode_u64(&raw.observed_at_unix_ms, "observed_at_unix_ms")?,
        gpus,
        available_cpu_cores: cpu,
        available_ram_bytes: raw
            .available_ram_bytes
            .as_deref()
            .map(|bytes| decode_u64(bytes, "available_ram_bytes"))
            .transpose()?,
        available_workspace_bytes: raw
            .available_workspace_bytes
            .as_deref()
            .map(|bytes| decode_u64(bytes, "available_workspace_bytes"))
            .transpose()?,
        allowed_workload_classes,
        third_party_workloads_opt_in: raw
            .third_party_workloads_opt_in
            .map(|value| parse_bool(value, "third_party_workloads_opt_in"))
            .transpose()?,
    };
    validate_inventory(&inventory).map_err(|field| {
        InventoryStoreError::CorruptData(format!("inventory has invalid {field}"))
    })?;
    if encode_inventory_payload(&inventory) != raw.payload {
        return Err(InventoryStoreError::CorruptData(
            "inventory normalized payload does not match parent/child rows".into(),
        ));
    }
    Ok(Some(inventory))
}

fn fetch_gpus(
    connection: &Connection,
    node_id: &str,
) -> Result<Vec<GpuInventory>, InventoryStoreError> {
    let mut statement = connection
        .prepare(
            "SELECT gpu_id, model, healthy, available_vram_bytes
             FROM coordinator_agent_gpus WHERE node_id = ?1 ORDER BY gpu_id",
        )
        .map_err(map_sql_error)?;
    let rows = statement
        .query_map(rusqlite::params![node_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
            ))
        })
        .map_err(map_sql_error)?;
    let mut gpus = Vec::new();
    for row in rows {
        let (gpu_id, model, healthy, available_vram) = row.map_err(map_sql_error)?;
        gpus.push(GpuInventory {
            gpu_id,
            model,
            healthy: healthy
                .map(|value| parse_bool(value, "GPU healthy"))
                .transpose()?,
            available_vram_bytes: available_vram
                .as_deref()
                .map(|bytes| decode_u64(bytes, "GPU available_vram_bytes"))
                .transpose()?,
        });
    }
    Ok(gpus)
}

fn fetch_workloads(
    connection: &Connection,
    node_id: &str,
) -> Result<BTreeSet<WorkloadClass>, InventoryStoreError> {
    let mut statement = connection
        .prepare(
            "SELECT workload_class FROM coordinator_agent_workload_classes
             WHERE node_id = ?1 ORDER BY workload_class",
        )
        .map_err(map_sql_error)?;
    let rows = statement
        .query_map(rusqlite::params![node_id], |row| row.get::<_, String>(0))
        .map_err(map_sql_error)?;
    let mut workloads = BTreeSet::new();
    for row in rows {
        let value = parse_workload_class(&row.map_err(map_sql_error)?)?;
        if !workloads.insert(value) {
            return Err(InventoryStoreError::CorruptData(
                "duplicate workload class child row".into(),
            ));
        }
    }
    Ok(workloads)
}

fn project_candidate(
    registry: AgentRegistry,
    inventory: Option<AgentInventory>,
) -> CandidateSnapshot {
    let (revision, observed_at, gpus, cpu, ram, workspace, workloads, third_party) = match inventory
    {
        Some(inventory) => (
            Some(inventory.inventory_revision),
            Some(inventory.observed_at_unix_ms),
            inventory.gpus.map(|gpus| {
                gpus.into_iter()
                    .map(|gpu| GpuSnapshot {
                        gpu_id: gpu.gpu_id,
                        healthy: gpu.healthy,
                        available_vram_bytes: gpu.available_vram_bytes,
                        model: gpu.model,
                    })
                    .collect()
            }),
            inventory.available_cpu_cores,
            inventory.available_ram_bytes,
            inventory.available_workspace_bytes,
            inventory.allowed_workload_classes,
            inventory.third_party_workloads_opt_in,
        ),
        None => (None, None, None, None, None, None, None, None),
    };
    CandidateSnapshot {
        node_id: registry.node_id,
        inventory_revision: revision,
        owner_member_id: Some(registry.owner_member_id),
        node_state: registry.node_state,
        risk_state: registry.risk_state,
        observed_at_unix_ms: observed_at,
        security_tier: registry.security_tier,
        isolation_class: registry.isolation_class,
        key_protection: registry.key_protection,
        gpus,
        available_cpu_cores: cpu,
        available_ram_bytes: ram,
        available_workspace_bytes: workspace,
        allowed_workload_classes: workloads,
        third_party_workloads_opt_in: third_party,
    }
}

fn encode_registry(registry: &AgentRegistry) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, &registry.node_id);
    push_string(&mut bytes, &registry.device_id);
    push_string(&mut bytes, &registry.owner_member_id);
    bytes.extend_from_slice(&registry.verifying_key);
    push_optional_code(&mut bytes, registry.node_state.map(node_state_u8));
    push_optional_code(&mut bytes, registry.risk_state.map(risk_state_u8));
    push_optional_code(&mut bytes, registry.security_tier.map(security_tier_u8));
    push_optional_code(&mut bytes, registry.isolation_class.map(isolation_class_u8));
    push_optional_code(&mut bytes, registry.key_protection.map(key_protection_u8));
    bytes
}

fn encode_inventory_payload(inventory: &AgentInventory) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&inventory.inventory_revision.to_be_bytes());
    bytes.extend_from_slice(&inventory.observed_at_unix_ms.to_be_bytes());
    match &inventory.gpus {
        None => bytes.push(0),
        Some(gpus) => {
            bytes.push(1);
            let mut gpus = gpus.clone();
            gpus.sort_by(|left, right| left.gpu_id.cmp(&right.gpu_id));
            bytes.extend_from_slice(&(gpus.len() as u64).to_be_bytes());
            for gpu in gpus {
                push_string(&mut bytes, &gpu.gpu_id);
                push_optional_string(&mut bytes, gpu.model.as_deref());
                push_optional_bool(&mut bytes, gpu.healthy);
                push_optional_u64(&mut bytes, gpu.available_vram_bytes);
            }
        }
    }
    push_optional_u64(&mut bytes, inventory.available_cpu_cores.map(u64::from));
    push_optional_u64(&mut bytes, inventory.available_ram_bytes);
    push_optional_u64(&mut bytes, inventory.available_workspace_bytes);
    match &inventory.allowed_workload_classes {
        None => bytes.push(0),
        Some(workloads) => {
            bytes.push(1);
            bytes.extend_from_slice(&(workloads.len() as u64).to_be_bytes());
            bytes.extend(workloads.iter().copied().map(workload_class_u8));
        }
    }
    push_optional_bool(&mut bytes, inventory.third_party_workloads_opt_in);
    bytes
}

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn push_optional_string(bytes: &mut Vec<u8>, value: Option<&str>) {
    match value {
        None => bytes.push(0),
        Some(value) => {
            bytes.push(1);
            push_string(bytes, value);
        }
    }
}

fn push_optional_bool(bytes: &mut Vec<u8>, value: Option<bool>) {
    match value {
        None => bytes.push(0),
        Some(false) => bytes.extend_from_slice(&[1, 0]),
        Some(true) => bytes.extend_from_slice(&[1, 1]),
    }
}

fn push_optional_u64(bytes: &mut Vec<u8>, value: Option<u64>) {
    match value {
        None => bytes.push(0),
        Some(value) => {
            bytes.push(1);
            bytes.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn push_optional_code(bytes: &mut Vec<u8>, value: Option<u8>) {
    match value {
        None => bytes.push(0),
        Some(value) => bytes.extend_from_slice(&[1, value]),
    }
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8], field: &str) -> Result<u64, InventoryStoreError> {
    let value: [u8; 8] = bytes.try_into().map_err(|_| {
        InventoryStoreError::CorruptData(format!("{field} must contain exactly 8 bytes"))
    })?;
    Ok(u64::from_be_bytes(value))
}

fn bool_to_i64(value: bool) -> i64 {
    i64::from(value)
}

fn parse_bool(value: i64, field: &str) -> Result<bool, InventoryStoreError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(InventoryStoreError::CorruptData(format!(
            "{field} is outside the boolean enum range: {value}"
        ))),
    }
}

fn map_sql_error(error: SqlError) -> InventoryStoreError {
    match error {
        SqlError::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            InventoryStoreError::LockTimeout
        }
        SqlError::SqliteFailure(code, _) => InventoryStoreError::Io(code.to_string()),
        other => InventoryStoreError::Io(other.to_string()),
    }
}

macro_rules! enum_codec {
    ($to_code:ident, $to_u8:ident, $parse:ident, $ty:ty, $label:literal, [$($variant:path => ($stored:literal, $code:literal)),+ $(,)?]) => {
        fn $to_code(value: $ty) -> &'static str {
            match value { $($variant => $stored,)+ }
        }
        fn $to_u8(value: $ty) -> u8 {
            match value { $($variant => $code,)+ }
        }
        fn $parse(value: &str) -> Result<$ty, InventoryStoreError> {
            match value {
                $($stored => Ok($variant),)+
                other => Err(InventoryStoreError::CorruptData(format!(
                    "unknown {} enum value: {other}", $label
                ))),
            }
        }
    };
}

enum_codec!(node_state_code, node_state_u8, parse_node_state, NodeState, "node_state", [
    NodeState::Discovered => ("DISCOVERED", 1), NodeState::Enrolling => ("ENROLLING", 2),
    NodeState::EnrollRejected => ("ENROLL_REJECTED", 3), NodeState::Approved => ("APPROVED", 4),
    NodeState::Online => ("ONLINE", 5), NodeState::Suspect => ("SUSPECT", 6),
    NodeState::Unreachable => ("UNREACHABLE", 7), NodeState::Lost => ("LOST", 8),
    NodeState::Recovering => ("RECOVERING", 9), NodeState::Draining => ("DRAINING", 10),
    NodeState::Offline => ("OFFLINE", 11), NodeState::Quarantined => ("QUARANTINED", 12),
    NodeState::Revoked => ("REVOKED", 13), NodeState::Terminated => ("TERMINATED", 14),
]);
enum_codec!(risk_state_code, risk_state_u8, parse_risk_state, RiskState, "risk_state", [
    RiskState::Normal => ("NORMAL", 1), RiskState::Suspect => ("SUSPECT", 2),
    RiskState::Quarantined => ("QUARANTINED", 3), RiskState::Revoked => ("REVOKED", 4),
]);
enum_codec!(security_tier_code, security_tier_u8, parse_security_tier, SecurityTier, "security_tier", [
    SecurityTier::S0 => ("S0", 1), SecurityTier::S1 => ("S1", 2),
    SecurityTier::S2 => ("S2", 3), SecurityTier::S3 => ("S3", 4),
    SecurityTier::S4 => ("S4", 5), SecurityTier::S5 => ("S5", 6),
]);
enum_codec!(isolation_class_code, isolation_class_u8, parse_isolation_class, IsolationClass, "isolation_class", [
    IsolationClass::Restricted => ("RESTRICTED", 1), IsolationClass::Contained => ("CONTAINED", 2),
    IsolationClass::Virtualized => ("VIRTUALIZED", 3),
]);
enum_codec!(key_protection_code, key_protection_u8, parse_key_protection, KeyProtection, "key_protection", [
    KeyProtection::K0 => ("K0", 1), KeyProtection::K1 => ("K1", 2),
    KeyProtection::K2 => ("K2", 3),
]);
enum_codec!(workload_class_code, workload_class_u8, parse_workload_class, WorkloadClass, "workload_class", [
    WorkloadClass::Training => ("TRAINING", 1), WorkloadClass::Inference => ("INFERENCE", 2),
    WorkloadClass::Preprocessing => ("PREPROCESSING", 3), WorkloadClass::Evaluation => ("EVALUATION", 4),
    WorkloadClass::Rendering => ("RENDERING", 5), WorkloadClass::Other => ("OTHER", 6),
]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFault {
    AfterParentWrite,
    DuringGpuReplacement,
    DuringWorkloadReplacement,
}

#[cfg(test)]
fn fail_at(fault: Option<TestFault>, point: TestFault) -> Result<(), InventoryStoreError> {
    if fault == Some(point) {
        let name = match point {
            TestFault::AfterParentWrite => "after inventory parent write",
            TestFault::DuringGpuReplacement => "during GPU child replacement",
            TestFault::DuringWorkloadReplacement => "during workload child replacement",
        };
        Err(InventoryStoreError::InjectedFailure(name))
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
fn fail_at(_fault: Option<TestFault>, _point: TestFault) -> Result<(), InventoryStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use gputeer_scheduler::{
        evaluate_eligibility, JobRequirements, Policy, RejectionReason, Sensitivity,
        SideEffectClass,
    };

    use super::*;

    fn registry(node: &str, seed: u8) -> AgentRegistry {
        AgentRegistry {
            node_id: node.into(),
            device_id: format!("device-{seed}"),
            owner_member_id: format!("owner-{seed}"),
            verifying_key: vec![seed; 32],
            node_state: Some(NodeState::Online),
            risk_state: Some(RiskState::Normal),
            security_tier: Some(SecurityTier::S2),
            isolation_class: Some(IsolationClass::Contained),
            key_protection: Some(KeyProtection::K1),
        }
    }

    fn inventory(node: &str, revision: u64, observed: u64, marker: u64) -> AgentInventory {
        AgentInventory {
            node_id: node.into(),
            inventory_revision: revision,
            observed_at_unix_ms: observed,
            gpus: Some(vec![
                GpuInventory {
                    gpu_id: format!("gpu-a-{marker}"),
                    model: Some(format!("model-a-{marker}")),
                    healthy: Some(true),
                    available_vram_bytes: Some(marker + 10),
                },
                GpuInventory {
                    gpu_id: format!("gpu-z-{marker}"),
                    model: Some(format!("model-z-{marker}")),
                    healthy: Some(false),
                    available_vram_bytes: Some(marker + 20),
                },
            ]),
            available_cpu_cores: Some(marker as u32),
            available_ram_bytes: Some(marker + 30),
            available_workspace_bytes: Some(marker + 40),
            allowed_workload_classes: Some(BTreeSet::from([
                WorkloadClass::Training,
                WorkloadClass::Evaluation,
            ])),
            third_party_workloads_opt_in: Some(marker % 2 == 0),
        }
    }

    fn bootstrap(node: &str, seed: u8, revision: u64, marker: u64) -> AgentBootstrap {
        AgentBootstrap {
            registry: registry(node, seed),
            inventory: inventory(node, revision, 1_700_000_000_000, marker),
        }
    }

    /// The whole reason this API exists: a later entry's failure must not leave
    /// earlier entries behind.
    ///
    /// ★ Composing `register_agent()` + `update_inventory()` in a loop would
    ///   fail this — each of those commits on its own.
    #[test]
    fn a_failing_entry_rolls_back_every_earlier_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.sqlite3");
        let mut store = CoordinatorInventoryStore::open(&path).unwrap();

        // Entry 0 is fine. Entry 1 reuses entry 0's device_id, which the store
        // already rejects — we do not reimplement that judgment here.
        let good = bootstrap("node-a", 1, 1, 100);
        let mut clashing = bootstrap("node-b", 2, 1, 200);
        clashing.registry.device_id = good.registry.device_id.clone();

        let error = store
            .import_bootstrap(&[good.clone(), clashing])
            .expect_err("device_id 충돌을 받아들였다");
        match error {
            InventoryStoreError::BootstrapEntry {
                index,
                ref node_id,
                ref source,
            } => {
                assert_eq!(index, 1, "실패한 항목 번호를 잘못 말한다");
                assert_eq!(node_id, "node-b", "실패한 노드를 잘못 말한다");
                assert_eq!(
                    **source,
                    InventoryStoreError::RegistryConflict { field: "device_id" },
                    "원인을 잘못 말한다"
                );
            }
            other => panic!("항목을 지목하지 않는 오류: {other}"),
        }

        // ★ 앞 항목이 남지 않았는가 — 이것이 이 API 의 값어치다.
        assert!(
            store.get_agent("node-a").unwrap().is_none(),
            "앞 항목의 registry 가 남았다"
        );
        assert!(
            store.get_inventory("node-a").unwrap().is_none(),
            "앞 항목의 inventory 가 남았다"
        );
        assert!(
            store.get_agent("node-b").unwrap().is_none(),
            "실패한 항목이 남았다"
        );
        // 파일을 다시 열어도 마찬가지여야 한다(커밋되지 않았음을 확인).
        drop(store);
        let mut reopened = CoordinatorInventoryStore::open(&path).unwrap();
        assert!(reopened.get_agent("node-a").unwrap().is_none());
        assert_eq!(
            reopened
                .pool_snapshot(1_700_000_001_000)
                .unwrap()
                .candidates
                .len(),
            0,
            "거부했는데 후보가 생겼다"
        );
    }

    /// registry 와 inventory 가 다른 노드를 가리키면 거부한다.
    ///
    /// 이걸 막지 않으면 한 노드를 등록하고 inventory 는 다른 노드에 붙인다 —
    /// 혹은 아무 데도 안 붙어 `AgentNotFound` 라는 엉뚱한 이름으로 나온다.
    #[test]
    fn the_two_halves_of_an_entry_must_name_the_same_node() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = CoordinatorInventoryStore::open(dir.path().join("i.sqlite3")).unwrap();
        let mut entry = bootstrap("node-a", 1, 1, 100);
        entry.inventory.node_id = "node-elsewhere".into();

        let error = store
            .import_bootstrap(&[entry])
            .expect_err("서로 다른 노드를 가리키는 항목을 받아들였다");
        match error {
            InventoryStoreError::BootstrapEntry { ref source, .. } => assert_eq!(
                **source,
                InventoryStoreError::InvalidInput("registry/inventory node_id mismatch"),
                "원인을 잘못 말한다"
            ),
            other => panic!("항목을 지목하지 않는 오류: {other}"),
        }
        assert!(store.get_agent("node-a").unwrap().is_none());
    }

    /// 정상 경로 — 여러 Agent 가 한 번에 들어가고 실제 후보가 된다.
    #[test]
    fn a_bootstrap_import_produces_schedulable_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.sqlite3");
        let mut store = CoordinatorInventoryStore::open(&path).unwrap();

        let result = store
            .import_bootstrap(&[
                bootstrap("node-a", 1, 1, 100),
                bootstrap("node-b", 2, 1, 200),
            ])
            .expect("정상 반입");
        assert_eq!(
            (
                result.registered,
                result.inventories_updated,
                result.entries
            ),
            (2, 2, 2),
            "무엇을 했는지 잘못 보고한다"
        );

        // 다른 프로세스처럼 파일을 다시 열어 확인한다.
        drop(store);
        let mut reopened = CoordinatorInventoryStore::open(&path).unwrap();
        let pool = reopened.pool_snapshot(1_700_000_001_000).unwrap();
        let ids: Vec<&str> = pool.candidates.iter().map(|c| c.node_id.as_str()).collect();
        assert_eq!(ids, vec!["node-a", "node-b"], "후보가 안 생겼다");
    }

    /// 같은 파일을 그대로 다시 반입하면 아무것도 새로 만들지 않는다.
    #[test]
    fn importing_the_same_bootstrap_twice_creates_nothing_new() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = CoordinatorInventoryStore::open(dir.path().join("i.sqlite3")).unwrap();
        let entries = [bootstrap("node-a", 1, 1, 100)];

        let first = store.import_bootstrap(&entries).unwrap();
        assert_eq!((first.registered, first.inventories_updated), (1, 1));
        let second = store.import_bootstrap(&entries).unwrap();
        assert_eq!(
            (second.registered, second.inventories_updated),
            (0, 0),
            "재반입이 새로 만들었다고 보고한다"
        );
        assert_eq!(second.entries, 1, "처리한 항목 수는 여전히 1 이어야 한다");
    }

    /// 잘못된 입력은 **잠금을 기다리지 않고** 즉시 입력 오류로 거부된다.
    ///
    /// ★ 리팩터가 이 순서를 뒤집었고 독립 검수 1라운드가 잡았다 — 검증을
    ///   트랜잭션 안으로 옮기면, 바쁜 DB 에 잘못된 입력을 주었을 때
    ///   `InvalidInput` 대신 `LockTimeout` 이 난다. **입력 오류가 경합
    ///   오류로 보고된다**(`CLAUDE.md` §3 — 오류가 사실을 잘못 전하지
    ///   않게 한다). 운영자는 오타를 고치는 대신 누가 DB 를 잡고 있는지
    ///   찾으러 간다.
    ///
    /// 다른 연결이 실제로 쓰기 잠금을 잡은 상태에서 잰다.
    #[test]
    fn bad_input_is_rejected_without_waiting_for_the_write_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.sqlite3");
        // 스키마를 먼저 만들어 둔다.
        drop(CoordinatorInventoryStore::open(&path).unwrap());

        // 다른 연결이 쓰기 잠금을 잡고 놓지 않는다.
        let holder = Connection::open(&path).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();

        let mut store = CoordinatorInventoryStore::open(&path).unwrap();
        let mut invalid = registry("node-a", 1);
        invalid.node_id = "   ".into(); // 공백뿐인 식별자 — 저장소가 막는다

        let error = store
            .register_agent(&invalid)
            .expect_err("공백 식별자를 받아들였다");
        assert!(
            matches!(error, InventoryStoreError::InvalidInput(_)),
            "입력 오류를 다른 것으로 보고한다: {error}"
        );

        let mut bad_inventory = inventory("node-a", 1, 1_700_000_000_000, 100);
        bad_inventory.node_id = "   ".into();
        let error = store
            .update_inventory(&bad_inventory)
            .expect_err("공백 식별자를 받아들였다");
        assert!(
            matches!(error, InventoryStoreError::InvalidInput(_)),
            "입력 오류를 다른 것으로 보고한다: {error}"
        );

        // 대조 — 잠금이 진짜로 잡혀 있었는가. 멀쩡한 입력은 실제로
        // 경합에 막혀야 한다. 이게 없으면 잠기지 않은 DB 로도 통과한다.
        let blocked = store
            .register_agent(&registry("node-a", 1))
            .expect_err("잠긴 DB 에 썼다");
        assert!(
            matches!(blocked, InventoryStoreError::LockTimeout),
            "잠금이 실제로 잡혀 있지 않았다 — 이 테스트는 아무것도 안 쟀다: {blocked}"
        );
        drop(holder);
    }

    /// bulk 반입도 같다 — 문서가 틀렸으면 잠그기 전에 끝난다.
    #[test]
    fn a_bad_bootstrap_entry_is_rejected_without_waiting_for_the_write_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.sqlite3");
        drop(CoordinatorInventoryStore::open(&path).unwrap());
        let holder = Connection::open(&path).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();

        let mut store = CoordinatorInventoryStore::open(&path).unwrap();
        let mut bad = bootstrap("node-a", 1, 1, 100);
        bad.registry.device_id = "  ".into();
        bad.inventory.node_id = bad.registry.node_id.clone();

        let error = store
            .import_bootstrap(&[bad])
            .expect_err("공백 device_id 를 받아들였다");
        match error {
            InventoryStoreError::BootstrapEntry { ref source, .. } => assert!(
                matches!(**source, InventoryStoreError::InvalidInput(_)),
                "입력 오류를 다른 것으로 보고한다: {source}"
            ),
            other => panic!("항목을 지목하지 않는 오류: {other}"),
        }

        // 대조 — 잠금이 실제로 잡혀 있었는가.
        let blocked = store
            .import_bootstrap(&[bootstrap("node-a", 1, 1, 100)])
            .expect_err("잠긴 DB 에 썼다");
        assert!(
            matches!(blocked, InventoryStoreError::LockTimeout),
            "잠금이 실제로 잡혀 있지 않았다: {blocked}"
        );
        drop(holder);
    }

    fn prepared_store(path: &Path) -> CoordinatorInventoryStore {
        let mut store = CoordinatorInventoryStore::open(path).unwrap();
        store.register_agent(&registry("node-a", 1)).unwrap();
        store
    }

    #[test]
    fn multiple_agents_reopen_and_project_in_node_order() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inventory.sqlite3");
        let mut store = CoordinatorInventoryStore::open(&path).unwrap();
        let agent_b = registry("node-b", 2);
        let agent_a = registry("node-a", 1);
        store.register_agent(&agent_b).unwrap();
        store.register_agent(&agent_a).unwrap();
        store
            .update_inventory(&inventory("node-b", 4, 90, 20))
            .unwrap();
        store
            .update_inventory(&inventory("node-a", 7, 80, 10))
            .unwrap();
        drop(store);

        let mut reopened = CoordinatorInventoryStore::open(&path).unwrap();
        assert!(reopened.is_durable());
        assert_eq!(reopened.get_agent("node-a").unwrap(), Some(agent_a));
        assert_eq!(
            reopened.get_inventory("node-b").unwrap(),
            Some(inventory("node-b", 4, 90, 20))
        );
        let snapshot = reopened.pool_snapshot(100).unwrap();
        assert_eq!(snapshot.evaluated_at_unix_ms, 100);
        assert_eq!(
            snapshot
                .candidates
                .iter()
                .map(|candidate| candidate.node_id.as_str())
                .collect::<Vec<_>>(),
            ["node-a", "node-b"]
        );
        assert_eq!(snapshot.candidates[0].inventory_revision, Some(7));
        assert_eq!(snapshot.candidates[1].inventory_revision, Some(4));
        assert_eq!(
            snapshot.candidates[0]
                .gpus
                .as_ref()
                .unwrap()
                .iter()
                .map(|gpu| gpu.gpu_id.as_str())
                .collect::<Vec<_>>(),
            ["gpu-a-10", "gpu-z-10"]
        );
    }

    #[test]
    fn registry_retry_is_idempotent_and_every_identity_conflict_preserves_row() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inventory.sqlite3");
        let mut store = CoordinatorInventoryStore::open(&path).unwrap();
        let original = registry("node-a", 1);
        assert!(store.register_agent(&original).unwrap().created);
        for _ in 0..10 {
            assert!(!store.register_agent(&original).unwrap().created);
        }

        for changed in [
            {
                let mut changed = original.clone();
                changed.device_id = "other-device".into();
                changed
            },
            {
                let mut changed = original.clone();
                changed.verifying_key = vec![9; 32];
                changed
            },
            {
                let mut changed = original.clone();
                changed.owner_member_id = "other-owner".into();
                changed
            },
        ] {
            assert!(matches!(
                store.register_agent(&changed),
                Err(InventoryStoreError::RegistryConflict { field: "node_id" })
            ));
        }
        let mut other = registry("node-b", 2);
        other.device_id = original.device_id.clone();
        assert!(matches!(
            store.register_agent(&other),
            Err(InventoryStoreError::RegistryConflict { field: "device_id" })
        ));
        other.device_id = "device-2".into();
        other.verifying_key = original.verifying_key.clone();
        assert!(matches!(
            store.register_agent(&other),
            Err(InventoryStoreError::RegistryConflict {
                field: "verifying_key"
            })
        ));
        assert_eq!(store.get_agent("node-a").unwrap(), Some(original));
        assert_eq!(store.get_agent("node-b").unwrap(), None);
    }

    #[test]
    fn same_revision_requires_byte_equivalent_normalized_payload() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inventory.sqlite3");
        let mut store = prepared_store(&path);
        let original = inventory("node-a", 8, 100, 10);
        assert!(store.update_inventory(&original).unwrap().updated);
        assert!(!store.update_inventory(&original).unwrap().updated);
        let mut reordered = original.clone();
        reordered.gpus.as_mut().unwrap().reverse();
        assert!(!store.update_inventory(&reordered).unwrap().updated);

        let mut changed = original.clone();
        changed.available_ram_bytes = Some(999);
        assert_eq!(
            store.update_inventory(&changed),
            Err(InventoryStoreError::RevisionConflict { revision: 8 })
        );
        let mut lower = original.clone();
        lower.inventory_revision = 7;
        assert_eq!(
            store.update_inventory(&lower),
            Err(InventoryStoreError::LowerRevision {
                stored: 8,
                requested: 7
            })
        );
        assert_eq!(store.get_inventory("node-a").unwrap(), Some(original));
    }

    #[test]
    fn every_replacement_fault_rolls_back_parent_and_all_children() {
        for fault in [
            TestFault::AfterParentWrite,
            TestFault::DuringGpuReplacement,
            TestFault::DuringWorkloadReplacement,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("inventory.sqlite3");
            let mut store = prepared_store(&path);
            let old = inventory("node-a", 1, 100, 10);
            store.update_inventory(&old).unwrap();
            let replacement = inventory("node-a", 2, 200, 90);
            assert!(matches!(
                store.update_inventory_inner(&replacement, Some(fault)),
                Err(InventoryStoreError::InjectedFailure(_))
            ));
            drop(store);
            let reopened = CoordinatorInventoryStore::open(&path).unwrap();
            assert_eq!(reopened.get_inventory("node-a").unwrap(), Some(old));
        }
    }

    #[test]
    fn invalid_registry_and_inventory_inputs_have_no_side_effects() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inventory.sqlite3");
        let mut store = CoordinatorInventoryStore::open(&path).unwrap();
        for field in ["node", "device", "owner", "key"] {
            let mut invalid = registry("node-a", 1);
            match field {
                "node" => invalid.node_id = " ".into(),
                "device" => invalid.device_id.clear(),
                "owner" => invalid.owner_member_id = "\t".into(),
                "key" => invalid.verifying_key = vec![1; 31],
                _ => unreachable!(),
            }
            assert!(matches!(
                store.register_agent(&invalid),
                Err(InventoryStoreError::InvalidInput(_))
            ));
        }
        assert!(store.pool_snapshot(0).unwrap().candidates.is_empty());
        store.register_agent(&registry("node-a", 1)).unwrap();

        let old = inventory("node-a", 1, 100, 10);
        store.update_inventory(&old).unwrap();
        let mut invalids = Vec::new();
        let mut blank = inventory("node-a", 2, 200, 20);
        blank.gpus.as_mut().unwrap()[0].gpu_id.clear();
        invalids.push(blank);
        let mut duplicate = inventory("node-a", 2, 200, 20);
        duplicate.gpus.as_mut().unwrap()[1].gpu_id =
            duplicate.gpus.as_ref().unwrap()[0].gpu_id.clone();
        invalids.push(duplicate);
        let mut empty_model = inventory("node-a", 2, 200, 20);
        empty_model.gpus.as_mut().unwrap()[0].model = Some(" ".into());
        invalids.push(empty_model);
        for invalid in invalids {
            assert!(matches!(
                store.update_inventory(&invalid),
                Err(InventoryStoreError::InvalidInput(_))
            ));
            assert_eq!(store.get_inventory("node-a").unwrap(), Some(old.clone()));
        }
        let absent = inventory("node-missing", 1, 0, 1);
        assert!(matches!(
            store.update_inventory(&absent),
            Err(InventoryStoreError::AgentNotFound { .. })
        ));
    }

    #[test]
    fn unknown_and_explicit_zero_facts_remain_distinct_in_projection() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inventory.sqlite3");
        let mut store = CoordinatorInventoryStore::open(&path).unwrap();
        let mut unknown = registry("node-a", 1);
        unknown.node_state = None;
        unknown.risk_state = None;
        unknown.security_tier = None;
        unknown.isolation_class = None;
        unknown.key_protection = None;
        store.register_agent(&unknown).unwrap();
        let missing = store.pool_snapshot(50).unwrap().candidates.remove(0);
        assert_eq!(missing.inventory_revision, None);
        assert_eq!(missing.node_state, None);
        assert_eq!(missing.observed_at_unix_ms, None);
        assert_eq!(missing.gpus, None);
        assert_eq!(missing.available_cpu_cores, None);
        assert_eq!(missing.allowed_workload_classes, None);

        let zero = AgentInventory {
            node_id: "node-a".into(),
            inventory_revision: 0,
            observed_at_unix_ms: 0,
            gpus: Some(vec![]),
            available_cpu_cores: Some(0),
            available_ram_bytes: Some(0),
            available_workspace_bytes: Some(0),
            allowed_workload_classes: Some(BTreeSet::new()),
            third_party_workloads_opt_in: Some(false),
        };
        store.update_inventory(&zero).unwrap();
        let explicit = store.pool_snapshot(50).unwrap().candidates.remove(0);
        assert_eq!(explicit.inventory_revision, Some(0));
        assert_eq!(explicit.observed_at_unix_ms, Some(0));
        assert_eq!(explicit.gpus, Some(vec![]));
        assert_eq!(explicit.available_cpu_cores, Some(0));
        assert_eq!(explicit.available_ram_bytes, Some(0));
        assert_eq!(explicit.allowed_workload_classes, Some(BTreeSet::new()));
        assert_eq!(explicit.third_party_workloads_opt_in, Some(false));
    }

    #[test]
    fn stale_and_future_observation_times_are_preserved_and_rejected_by_scheduler() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inventory.sqlite3");
        let mut store = CoordinatorInventoryStore::open(&path).unwrap();
        store.register_agent(&registry("node-a", 1)).unwrap();
        store.register_agent(&registry("node-b", 2)).unwrap();
        store
            .update_inventory(&inventory("node-a", 1, 10, 10))
            .unwrap();
        store
            .update_inventory(&inventory("node-b", 1, 110, 20))
            .unwrap();
        let snapshot = store.pool_snapshot(100).unwrap();
        assert_eq!(snapshot.candidates[0].observed_at_unix_ms, Some(10));
        assert_eq!(snapshot.candidates[1].observed_at_unix_ms, Some(110));

        let job = JobRequirements {
            submitter_member_id: Some("owner-1".into()),
            workload_class: Some(WorkloadClass::Training),
            side_effect_class: Some(SideEffectClass::Pure),
            sensitivity: Some(Sensitivity::Internal),
            minimum_security_tier: Some(SecurityTier::S0),
            minimum_isolation_class: Some(IsolationClass::Restricted),
            minimum_key_protection: Some(KeyProtection::K0),
            minimum_gpu_count: Some(1),
            minimum_vram_bytes_per_gpu: Some(1),
            allowed_gpu_models: vec![],
            cpu_cores: Some(1),
            ram_bytes: Some(1),
            workspace_bytes: Some(1),
        };
        let report = evaluate_eligibility(
            &snapshot,
            &job,
            &Policy {
                maximum_snapshot_age_ms: 50,
            },
        );
        for rejected in report.rejected {
            assert!(rejected
                .reasons
                .iter()
                .any(|reason| matches!(reason, RejectionReason::SnapshotNotFresh { .. })));
        }
    }

    #[test]
    fn maximum_revision_timestamp_and_resource_values_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inventory.sqlite3");
        let mut store = prepared_store(&path);
        let maximum = AgentInventory {
            node_id: "node-a".into(),
            inventory_revision: u64::MAX,
            observed_at_unix_ms: u64::MAX,
            gpus: Some(vec![GpuInventory {
                gpu_id: "gpu-max".into(),
                model: None,
                healthy: None,
                available_vram_bytes: Some(u64::MAX),
            }]),
            available_cpu_cores: Some(u32::MAX),
            available_ram_bytes: Some(u64::MAX),
            available_workspace_bytes: Some(u64::MAX),
            allowed_workload_classes: Some(BTreeSet::from([WorkloadClass::Other])),
            third_party_workloads_opt_in: None,
        };
        store.update_inventory(&maximum).unwrap();
        assert!(!store.update_inventory(&maximum).unwrap().updated);
        assert_eq!(
            store.get_inventory("node-a").unwrap(),
            Some(maximum.clone())
        );
        let snapshot = store.pool_snapshot(u64::MAX).unwrap();
        assert_eq!(snapshot.candidates[0].observed_at_unix_ms, Some(u64::MAX));
        assert_eq!(snapshot.candidates[0].available_cpu_cores, Some(u32::MAX));
        assert_eq!(
            snapshot.candidates[0].gpus.as_ref().unwrap()[0].available_vram_bytes,
            Some(u64::MAX)
        );
    }

    #[test]
    fn two_connections_competing_for_one_revision_leave_one_complete_payload() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inventory.sqlite3");
        let mut initial = prepared_store(&path);
        initial
            .update_inventory(&inventory("node-a", 1, 100, 10))
            .unwrap();
        drop(initial);

        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for marker in [20, 91] {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                let mut store = CoordinatorInventoryStore::open(path).unwrap();
                let mut candidate = inventory("node-a", 2, 200 + marker, marker);
                candidate.allowed_workload_classes = Some(if marker == 20 {
                    BTreeSet::from([WorkloadClass::Training])
                } else {
                    BTreeSet::from([WorkloadClass::Inference, WorkloadClass::Rendering])
                });
                barrier.wait();
                let result = store.update_inventory(&candidate);
                (candidate, result)
            }));
        }
        barrier.wait();
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes.iter().filter(|(_, result)| result.is_ok()).count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|(_, result)| matches!(
                    result,
                    Err(InventoryStoreError::RevisionConflict { revision: 2 })
                ))
                .count(),
            1
        );
        let winner = outcomes
            .iter()
            .find_map(|(candidate, result)| result.is_ok().then_some(candidate))
            .unwrap();
        let reopened = CoordinatorInventoryStore::open(&path).unwrap();
        assert_eq!(
            reopened.get_inventory("node-a").unwrap(),
            Some(winner.clone())
        );
    }

    #[test]
    fn corrupted_enum_key_revision_and_gpu_child_fail_closed_after_reopen() {
        let corruptions: [(&str, &str); 4] = [
            (
                "UPDATE coordinator_agent_registry SET node_state = 'UNKNOWN' WHERE node_id = 'node-a'",
                "enum",
            ),
            (
                "UPDATE coordinator_agent_registry SET verifying_key = x'01' WHERE node_id = 'node-a'",
                "key",
            ),
            (
                "UPDATE coordinator_agent_inventory SET inventory_revision = x'0000000000000009' WHERE node_id = 'node-a'",
                "revision",
            ),
            (
                "UPDATE coordinator_agent_gpus SET healthy = 2 WHERE node_id = 'node-a'",
                "GPU child",
            ),
        ];
        for (sql, label) in corruptions {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join(format!("{label}.sqlite3"));
            let mut store = prepared_store(&path);
            store
                .update_inventory(&inventory("node-a", 1, 100, 10))
                .unwrap();
            store.connection.execute(sql, []).unwrap();
            drop(store);
            let mut reopened = CoordinatorInventoryStore::open(&path).unwrap();
            assert!(
                matches!(
                    reopened.pool_snapshot(100),
                    Err(InventoryStoreError::CorruptData(_))
                ),
                "corruption must not default or disappear: {label}"
            );
        }
    }

    #[test]
    fn corrupt_payload_and_observation_markers_fail_closed() {
        for sql in [
            "UPDATE coordinator_agent_inventory SET payload = x'00' WHERE node_id = 'node-a'",
            "UPDATE coordinator_agent_inventory SET gpus_observed = 2 WHERE node_id = 'node-a'",
            "UPDATE coordinator_agent_inventory SET workload_classes_observed = 2 WHERE node_id = 'node-a'",
            "UPDATE coordinator_agent_inventory SET third_party_workloads_opt_in = 2 WHERE node_id = 'node-a'",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("inventory.sqlite3");
            let mut store = prepared_store(&path);
            store.update_inventory(&inventory("node-a", 1, 100, 10)).unwrap();
            store.connection.execute(sql, []).unwrap();
            drop(store);
            let reopened = CoordinatorInventoryStore::open(&path).unwrap();
            assert!(matches!(
                reopened.get_inventory("node-a"),
                Err(InventoryStoreError::CorruptData(_))
            ));
        }
    }
}
