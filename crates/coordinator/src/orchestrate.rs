//! Crate-internal local placement-to-staging orchestration.
//!
//! This seam performs local node-exclusive inventory-CAS admission together
//! with durable staging and the selected GPU ID binding. It deliberately is
//! **not** per-GPU capacity accounting/partial resource allocation,
//! release/requeue, Grant construction, network dispatch, or a production
//! entrypoint.

use gputeer_scheduler::{
    evaluate_eligibility, rank_best_fit, resource_fit, BestFitPolicy, BestFitRanking,
    EligibilityReport, EligibilityResolution, JobRequirements, MissingFact, Policy, RankingError,
};

use crate::{
    inventory_store::{CoordinatorInventoryStore, InventoryStoreError},
    staging_store::{
        CoordinatorStagingStore, ReservedStageError, StageQueuedRequest, StageQueuedResult,
    },
};

/// Caller-produced identity, clock, and Lease-policy values needed by staging.
///
/// The orchestration kernel does not generate IDs, read a clock, or invent a
/// Lease lifetime policy. Only the selected `node_id` is added locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagingIssuanceInput {
    pub operation_key: [u8; 16],
    pub attempt_id: String,
    pub lease_id: String,
    pub issuing_coordinator_id: String,
    pub coordinator_term: u64,
    pub issued_at_unix_ms: u64,
    pub renew_after_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub max_total_duration_seconds: u64,
}

/// Already validated and normalized scheduler/staging inputs.
///
/// Manifest ingestion and conversion into [`JobRequirements`] are outside this
/// module; callers must supply that domain object explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementToStagingInput {
    pub job_id: String,
    pub job_requirements: JobRequirements,
    pub hard_filter_policy: Policy,
    pub best_fit_policy: BestFitPolicy,
    pub evaluated_at_unix_ms: u64,
    pub issuance: StagingIssuanceInput,
}

/// A zero-candidate result is structurally unable to masquerade as staging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementToStagingOutcome {
    NoEligible {
        eligibility: EligibilityReport,
    },
    Staged {
        eligibility: EligibilityReport,
        /// Present only for the multiple-candidate branch.
        ranking: Option<BestFitRanking>,
        selected_node_id: String,
        selected_gpu_ids: Vec<String>,
        stage: StageQueuedResult,
    },
}

/// Preserves the producing subsystem's typed error instead of flattening it.
#[derive(Debug, PartialEq, Eq)]
pub enum PlacementToStagingError {
    Inventory(InventoryStoreError),
    Ranking(RankingError),
    SelectedCandidateNotUnique {
        node_id: String,
        matches: usize,
    },
    SelectedCandidateMissingInventoryRevision {
        node_id: String,
    },
    SelectedGpuCountMismatch {
        node_id: String,
        required: u32,
        actual: usize,
    },
    Staging(ReservedStageError),
}

impl std::fmt::Display for PlacementToStagingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inventory(error) => write!(f, "inventory projection failed: {error}"),
            Self::Ranking(error) => write!(f, "best-fit ranking failed: {error:?}"),
            Self::SelectedCandidateNotUnique { node_id, matches } => write!(
                f,
                "selected node is not unique in its PoolSnapshot: node={node_id}, matches={matches}"
            ),
            Self::SelectedCandidateMissingInventoryRevision { node_id } => write!(
                f,
                "selected node has no inventory revision in its PoolSnapshot: {node_id}"
            ),
            Self::SelectedGpuCountMismatch { node_id, required, actual } => write!(
                f,
                "selected GPU count differs from the Job requirement: node={node_id}, required={required}, actual={actual}"
            ),
            Self::Staging(error) => write!(f, "durable staging failed: {error}"),
        }
    }
}

impl std::error::Error for PlacementToStagingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Inventory(error) => Some(error),
            Self::Ranking(_) => None,
            Self::SelectedCandidateNotUnique { .. }
            | Self::SelectedCandidateMissingInventoryRevision { .. }
            | Self::SelectedGpuCountMismatch { .. } => None,
            Self::Staging(error) => Some(error),
        }
    }
}

impl From<InventoryStoreError> for PlacementToStagingError {
    fn from(error: InventoryStoreError) -> Self {
        Self::Inventory(error)
    }
}

impl From<RankingError> for PlacementToStagingError {
    fn from(error: RankingError) -> Self {
        Self::Ranking(error)
    }
}

impl From<ReservedStageError> for PlacementToStagingError {
    fn from(error: ReservedStageError) -> Self {
        Self::Staging(error)
    }
}

/// Reads one inventory snapshot, resolves the 0/1/N candidate branches, and
/// stages a selected node exactly once.
///
/// `NoEligibleCandidates` leaves the Job QUEUED. `SingleEligible` bypasses
/// ranking. Only `RankingRequired` invokes best-fit. The inventory read and
/// staging write begin as separate snapshot and write transactions, but the
/// selected revision comparison, node reservation, and staging commit share
/// the staging store's single `BEGIN IMMEDIATE` linearization point.
pub fn orchestrate_placement_to_staging(
    inventory_store: &mut CoordinatorInventoryStore,
    staging_store: &mut CoordinatorStagingStore,
    input: &PlacementToStagingInput,
) -> Result<PlacementToStagingOutcome, PlacementToStagingError> {
    let pool = inventory_store.pool_snapshot(input.evaluated_at_unix_ms)?;
    let eligibility =
        evaluate_eligibility(&pool, &input.job_requirements, &input.hard_filter_policy);

    let (selected_node_id, ranking) = match &eligibility.resolution {
        EligibilityResolution::NoEligibleCandidates => {
            return Ok(PlacementToStagingOutcome::NoEligible { eligibility });
        }
        EligibilityResolution::SingleEligible { node_id } => (node_id.clone(), None),
        EligibilityResolution::RankingRequired => {
            let ranking = rank_best_fit(
                &pool,
                &input.job_requirements,
                &eligibility,
                &input.best_fit_policy,
            )?;
            (ranking.winner.node_id.clone(), Some(ranking))
        }
    };

    let matching_candidates = pool
        .candidates
        .iter()
        .filter(|candidate| candidate.node_id == selected_node_id)
        .collect::<Vec<_>>();
    if matching_candidates.len() != 1 {
        return Err(PlacementToStagingError::SelectedCandidateNotUnique {
            node_id: selected_node_id,
            matches: matching_candidates.len(),
        });
    }
    let selected_gpu_ids = match ranking.as_ref() {
        Some(ranking) => ranking.winner.selected_gpu_ids.clone(),
        None => resource_fit(matching_candidates[0], &input.job_requirements)?.selected_gpu_ids,
    };
    let required_gpu_count = input.job_requirements.minimum_gpu_count.ok_or_else(|| {
        PlacementToStagingError::Ranking(RankingError::MissingRankFact {
            node_id: None,
            fact: MissingFact::JobMinimumGpuCount,
        })
    })?;
    if u32::try_from(selected_gpu_ids.len()).ok() != Some(required_gpu_count) {
        return Err(PlacementToStagingError::SelectedGpuCountMismatch {
            node_id: selected_node_id,
            required: required_gpu_count,
            actual: selected_gpu_ids.len(),
        });
    }
    let expected_inventory_revision =
        matching_candidates[0].inventory_revision.ok_or_else(|| {
            PlacementToStagingError::SelectedCandidateMissingInventoryRevision {
                node_id: selected_node_id.clone(),
            }
        })?;
    let request = StageQueuedRequest {
        operation_key: input.issuance.operation_key,
        job_id: input.job_id.clone(),
        attempt_id: input.issuance.attempt_id.clone(),
        lease_id: input.issuance.lease_id.clone(),
        node_id: selected_node_id.clone(),
        selected_gpu_ids: selected_gpu_ids.clone(),
        issuing_coordinator_id: input.issuance.issuing_coordinator_id.clone(),
        coordinator_term: input.issuance.coordinator_term,
        issued_at_unix_ms: input.issuance.issued_at_unix_ms,
        renew_after_unix_ms: input.issuance.renew_after_unix_ms,
        expires_at_unix_ms: input.issuance.expires_at_unix_ms,
        max_total_duration_seconds: input.issuance.max_total_duration_seconds,
    };
    let stage = staging_store
        .reserve_node_and_stage_queued_with_lease(&request, expected_inventory_revision)?
        .stage;

    Ok(PlacementToStagingOutcome::Staged {
        eligibility,
        ranking,
        selected_node_id,
        selected_gpu_ids,
        stage,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::{Arc, Barrier},
    };

    use gputeer_scheduler::{
        FitAxis, IsolationClass, KeyProtection, NodeState, RiskState, SecurityTier, Sensitivity,
        SideEffectClass, WorkloadClass,
    };
    use tempfile::TempDir;

    use super::*;
    use crate::{
        inventory_store::{AgentInventory, AgentRegistry, GpuInventory},
        job_store::{AcceptedJobSubmission, CoordinatorJobStore, JobState},
        staging_store::StagingStoreError,
    };

    struct Fixture {
        _directory: TempDir,
        job_store: CoordinatorJobStore,
        inventory_store: CoordinatorInventoryStore,
        staging_store: CoordinatorStagingStore,
        input: PlacementToStagingInput,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("control.db");
            let mut job_store = CoordinatorJobStore::open(&path).unwrap();
            job_store
                .submit_accepted(
                    &AcceptedJobSubmission {
                        idempotency_key: [1; 16],
                        job_id: "job-1".into(),
                        submitter_device_id: "submitter-device".into(),
                        manifest_hash: [2; 32],
                        deadline_unix_ms: None,
                        max_queue_duration_ms: None,
                    },
                    10,
                )
                .unwrap();
            job_store.start_planning("job-1", 20).unwrap();
            job_store.enqueue("job-1", "plan-1", 30).unwrap();

            Self {
                _directory: directory,
                job_store,
                inventory_store: CoordinatorInventoryStore::open(&path).unwrap(),
                staging_store: CoordinatorStagingStore::open(&path).unwrap(),
                input: placement_input("job-1", 1),
            }
        }

        fn add_candidate(&mut self, node_id: &str, available_vram_bytes: u64) {
            self.add_candidate_gpus(
                node_id,
                available_vram_bytes as u8,
                vec![GpuInventory {
                    gpu_id: format!("gpu-{node_id}"),
                    model: Some("model-a".into()),
                    healthy: Some(true),
                    available_vram_bytes: Some(available_vram_bytes),
                }],
            );
        }

        fn add_candidate_gpus(&mut self, node_id: &str, key_byte: u8, gpus: Vec<GpuInventory>) {
            self.inventory_store
                .register_agent(&AgentRegistry {
                    node_id: node_id.into(),
                    device_id: format!("device-{node_id}"),
                    owner_member_id: "member-1".into(),
                    verifying_key: vec![key_byte; 32],
                    node_state: Some(NodeState::Online),
                    risk_state: Some(RiskState::Normal),
                    security_tier: Some(SecurityTier::S2),
                    isolation_class: Some(IsolationClass::Contained),
                    key_protection: Some(KeyProtection::K1),
                })
                .unwrap();
            self.inventory_store
                .update_inventory(&AgentInventory {
                    node_id: node_id.into(),
                    inventory_revision: 1,
                    observed_at_unix_ms: 90,
                    gpus: Some(gpus),
                    available_cpu_cores: Some(8),
                    available_ram_bytes: Some(64),
                    available_workspace_bytes: Some(64),
                    allowed_workload_classes: Some(BTreeSet::from([WorkloadClass::Training])),
                    third_party_workloads_opt_in: None,
                })
                .unwrap();
        }

        fn run(&mut self) -> Result<PlacementToStagingOutcome, PlacementToStagingError> {
            orchestrate_placement_to_staging(
                &mut self.inventory_store,
                &mut self.staging_store,
                &self.input,
            )
        }
    }

    fn requirements() -> JobRequirements {
        JobRequirements {
            submitter_member_id: Some("member-1".into()),
            workload_class: Some(WorkloadClass::Training),
            side_effect_class: Some(SideEffectClass::Pure),
            sensitivity: Some(Sensitivity::Internal),
            minimum_security_tier: Some(SecurityTier::S1),
            minimum_isolation_class: Some(IsolationClass::Restricted),
            minimum_key_protection: Some(KeyProtection::K0),
            minimum_gpu_count: Some(1),
            minimum_vram_bytes_per_gpu: Some(8),
            allowed_gpu_models: vec!["model-a".into()],
            cpu_cores: Some(2),
            ram_bytes: Some(8),
            workspace_bytes: Some(8),
        }
    }

    fn placement_input(job_id: &str, ordinal: u8) -> PlacementToStagingInput {
        PlacementToStagingInput {
            job_id: job_id.into(),
            job_requirements: requirements(),
            hard_filter_policy: Policy {
                maximum_snapshot_age_ms: 20,
            },
            best_fit_policy: BestFitPolicy {
                axis_order: [
                    FitAxis::Vram,
                    FitAxis::GpuCount,
                    FitAxis::Cpu,
                    FitAxis::Ram,
                    FitAxis::Workspace,
                ],
            },
            evaluated_at_unix_ms: 100,
            issuance: StagingIssuanceInput {
                operation_key: [ordinal + 2; 16],
                attempt_id: format!("attempt-{ordinal}"),
                lease_id: format!("lease-{ordinal}"),
                issuing_coordinator_id: "coordinator-1".into(),
                coordinator_term: 7,
                issued_at_unix_ms: 100,
                renew_after_unix_ms: 120,
                expires_at_unix_ms: 160,
                max_total_duration_seconds: 1,
            },
        }
    }

    #[test]
    fn zero_candidates_preserves_queued_job_and_creates_no_attempt() {
        let mut fixture = Fixture::new();

        let outcome = fixture.run().unwrap();

        assert!(matches!(
            outcome,
            PlacementToStagingOutcome::NoEligible { ref eligibility }
                if eligibility.resolution == EligibilityResolution::NoEligibleCandidates
        ));
        assert_eq!(
            fixture.job_store.get("job-1").unwrap().unwrap().state,
            JobState::Queued
        );
        assert_eq!(
            fixture.staging_store.get_attempt("attempt-1").unwrap(),
            None
        );
        assert_eq!(fixture.staging_store.fence_epoch().unwrap(), None);
    }

    #[test]
    fn one_candidate_bypasses_ranking_and_stages_that_node() {
        let mut fixture = Fixture::new();
        fixture.add_candidate("node-a", 12);

        let outcome = fixture.run().unwrap();

        let PlacementToStagingOutcome::Staged {
            ranking,
            selected_node_id,
            selected_gpu_ids,
            stage,
            ..
        } = outcome
        else {
            panic!("one eligible candidate must stage");
        };
        assert_eq!(ranking, None);
        assert_eq!(selected_node_id, "node-a");
        assert_eq!(selected_gpu_ids, ["gpu-node-a"]);
        assert_eq!(stage.attempt.node_ids, ["node-a"]);
        assert_eq!(stage.lease.holder_node_id, "node-a");
        assert_eq!(stage.job.state, JobState::Staging);
        assert_eq!(
            fixture
                .staging_store
                .get_node_reservation("node-a")
                .unwrap()
                .unwrap()
                .selected_gpu_ids,
            selected_gpu_ids
        );
    }

    #[test]
    fn multiple_candidates_stage_the_best_fit_winner() {
        let mut fixture = Fixture::new();
        fixture.add_candidate("node-a", 32);
        fixture.add_candidate("node-b", 12);

        let outcome = fixture.run().unwrap();

        let PlacementToStagingOutcome::Staged {
            ranking,
            selected_node_id,
            selected_gpu_ids,
            stage,
            ..
        } = outcome
        else {
            panic!("multiple eligible candidates must stage");
        };
        let ranking = ranking.expect("multiple candidates require ranking");
        assert_eq!(ranking.winner.node_id, "node-b");
        assert_eq!(ranking.winner.selected_gpu_ids, ["gpu-node-b"]);
        assert_eq!(selected_node_id, "node-b");
        assert_eq!(selected_gpu_ids, ["gpu-node-b"]);
        assert_eq!(stage.attempt.node_ids, ["node-b"]);
        assert_eq!(stage.lease.holder_node_id, "node-b");
        assert_eq!(
            fixture
                .staging_store
                .get_node_reservation("node-b")
                .unwrap()
                .unwrap()
                .selected_gpu_ids,
            selected_gpu_ids
        );
    }

    #[test]
    fn single_and_ranked_winner_use_the_same_gpu_assignment_rule() {
        let node_a_gpus = vec![
            GpuInventory {
                gpu_id: "gpu-z".into(),
                model: Some("model-a".into()),
                healthy: Some(true),
                available_vram_bytes: Some(10),
            },
            GpuInventory {
                gpu_id: "gpu-a".into(),
                model: Some("model-a".into()),
                healthy: Some(true),
                available_vram_bytes: Some(10),
            },
        ];

        let mut single = Fixture::new();
        single.add_candidate_gpus("node-a", 10, node_a_gpus.clone());
        let PlacementToStagingOutcome::Staged {
            selected_gpu_ids: single_ids,
            ..
        } = single.run().unwrap()
        else {
            panic!("single candidate must stage");
        };

        let mut multiple = Fixture::new();
        multiple.add_candidate_gpus("node-a", 10, node_a_gpus);
        multiple.add_candidate("node-b", 32);
        let PlacementToStagingOutcome::Staged {
            ranking,
            selected_node_id,
            selected_gpu_ids: ranked_ids,
            ..
        } = multiple.run().unwrap()
        else {
            panic!("ranked candidate must stage");
        };
        let ranking = ranking.expect("multiple candidates require ranking");

        assert_eq!(selected_node_id, "node-a");
        assert_eq!(single_ids, ["gpu-a"]);
        assert_eq!(ranked_ids, single_ids);
        assert_eq!(ranking.winner.selected_gpu_ids, ranked_ids);
    }

    #[test]
    fn concurrent_orchestration_of_one_gpu_stages_exactly_one_job() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.db");
        let mut jobs = CoordinatorJobStore::open(&path).unwrap();
        for ordinal in 1..=2u8 {
            let job_id = format!("job-{ordinal}");
            jobs.submit_accepted(
                &AcceptedJobSubmission {
                    idempotency_key: [ordinal; 16],
                    job_id: job_id.clone(),
                    submitter_device_id: "submitter-device".into(),
                    manifest_hash: [ordinal; 32],
                    deadline_unix_ms: None,
                    max_queue_duration_ms: None,
                },
                10,
            )
            .unwrap();
            jobs.start_planning(&job_id, 20).unwrap();
            jobs.enqueue(&job_id, "plan-1", 30).unwrap();
        }
        drop(jobs);
        let mut inventory = CoordinatorInventoryStore::open(&path).unwrap();
        inventory
            .register_agent(&AgentRegistry {
                node_id: "node-a".into(),
                device_id: "device-node-a".into(),
                owner_member_id: "member-1".into(),
                verifying_key: vec![1; 32],
                node_state: Some(NodeState::Online),
                risk_state: Some(RiskState::Normal),
                security_tier: Some(SecurityTier::S2),
                isolation_class: Some(IsolationClass::Contained),
                key_protection: Some(KeyProtection::K1),
            })
            .unwrap();
        inventory
            .update_inventory(&AgentInventory {
                node_id: "node-a".into(),
                inventory_revision: 1,
                observed_at_unix_ms: 90,
                gpus: Some(vec![GpuInventory {
                    gpu_id: "gpu-node-a".into(),
                    model: Some("model-a".into()),
                    healthy: Some(true),
                    available_vram_bytes: Some(12),
                }]),
                available_cpu_cores: Some(8),
                available_ram_bytes: Some(64),
                available_workspace_bytes: Some(64),
                allowed_workload_classes: Some(BTreeSet::from([WorkloadClass::Training])),
                third_party_workloads_opt_in: None,
            })
            .unwrap();
        drop(inventory);

        let barrier = Arc::new(Barrier::new(2));
        let handles = (1..=2u8)
            .map(|ordinal| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut inventory = CoordinatorInventoryStore::open(&path).unwrap();
                    let mut staging = CoordinatorStagingStore::open(&path).unwrap();
                    let input = placement_input(&format!("job-{ordinal}"), ordinal);
                    barrier.wait();
                    orchestrate_placement_to_staging(&mut inventory, &mut staging, &input)
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Ok(PlacementToStagingOutcome::Staged { .. })))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(PlacementToStagingError::Staging(
                        ReservedStageError::NodeAlreadyReserved { .. }
                    ))
                ))
                .count(),
            1
        );
        let jobs = CoordinatorJobStore::open(&path).unwrap();
        let states = ["job-1", "job-2"].map(|job_id| jobs.get(job_id).unwrap().unwrap().state);
        assert_eq!(
            states
                .iter()
                .filter(|state| **state == JobState::Staging)
                .count(),
            1
        );
        assert_eq!(
            states
                .iter()
                .filter(|state| **state == JobState::Queued)
                .count(),
            1
        );
    }

    #[test]
    fn operation_replay_returns_the_original_durable_staging_result() {
        let mut fixture = Fixture::new();
        fixture.add_candidate("node-a", 12);

        let first = fixture.run().unwrap();
        let replay = fixture.run().unwrap();

        let PlacementToStagingOutcome::Staged { stage: first, .. } = first else {
            panic!("first call must stage");
        };
        let PlacementToStagingOutcome::Staged { stage: replay, .. } = replay else {
            panic!("replay must return staging result");
        };
        assert!(first.created);
        assert!(!replay.created);
        assert_eq!(first.job, replay.job);
        assert_eq!(first.attempt, replay.attempt);
        assert_eq!(first.lease, replay.lease);
        assert_eq!(fixture.staging_store.fence_epoch().unwrap(), Some(1));
    }

    #[test]
    fn invalid_ranking_policy_fails_closed_before_staging() {
        let mut fixture = Fixture::new();
        fixture.add_candidate("node-a", 32);
        fixture.add_candidate("node-b", 12);
        fixture.input.best_fit_policy.axis_order = [FitAxis::Vram; 5];

        assert_eq!(
            fixture.run(),
            Err(PlacementToStagingError::Ranking(
                RankingError::InvalidPolicyAxisOrder
            ))
        );
        assert_eq!(
            fixture.job_store.get("job-1").unwrap().unwrap().state,
            JobState::Queued
        );
        assert_eq!(
            fixture.staging_store.get_attempt("attempt-1").unwrap(),
            None
        );
        assert_eq!(fixture.staging_store.fence_epoch().unwrap(), None);
    }

    #[test]
    fn corrupt_inventory_fails_closed_before_staging() {
        let mut fixture = Fixture::new();
        fixture.add_candidate("node-a", 12);
        let path = fixture._directory.path().join("control.db");
        rusqlite::Connection::open(path)
            .unwrap()
            .execute(
                "UPDATE coordinator_agent_inventory SET payload = x'00' WHERE node_id = 'node-a'",
                [],
            )
            .unwrap();

        assert!(matches!(
            fixture.run(),
            Err(PlacementToStagingError::Inventory(
                InventoryStoreError::CorruptData(_)
            ))
        ));
        assert_eq!(
            fixture.job_store.get("job-1").unwrap().unwrap().state,
            JobState::Queued
        );
        assert_eq!(
            fixture.staging_store.get_attempt("attempt-1").unwrap(),
            None
        );
        assert_eq!(fixture.staging_store.fence_epoch().unwrap(), None);
    }

    #[test]
    fn staging_validation_error_does_not_claim_success_or_consume_epoch() {
        let mut fixture = Fixture::new();
        fixture.add_candidate("node-a", 12);
        fixture.input.issuance.renew_after_unix_ms = fixture.input.issuance.issued_at_unix_ms;

        assert_eq!(
            fixture.run(),
            Err(PlacementToStagingError::Staging(
                ReservedStageError::Staging(StagingStoreError::InvalidLeaseLifetime)
            ))
        );
        assert_eq!(
            fixture.job_store.get("job-1").unwrap().unwrap().state,
            JobState::Queued
        );
        assert_eq!(
            fixture.staging_store.get_attempt("attempt-1").unwrap(),
            None
        );
        assert_eq!(fixture.staging_store.fence_epoch().unwrap(), None);
    }
}
