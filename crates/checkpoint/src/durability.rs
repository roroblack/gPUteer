//! Durability Contract — 기준선 §18.2.
//!
//! v4 는 "Committed checkpoint 에서 복구한다" 고 하면서 committed 의 정의가 없었다.
//! fsync 인지, 복제 완료인지, Raft 커밋인지 불명이었다.
//!
//! 상태 전이는 `docs/protocol/state-machines.md` §4 가 규범이다.
//! 표에 없는 전이는 구현하지 않는다(MUST NOT).
//!
//! 상태는 매니페스트를 덮어써서 기록하지 않는다.
//! ADR-026 의 write-once 원칙을 지키기 위해 상태별 불변 사이드카를 기록한다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic::write_once;
use crate::CheckpointError;

/// Replica observation의 kind를 protobuf와 분리해 표현한 kernel 입력 값.
///
/// `UNSPECIFIED`는 resolver가 해소한 값이 아니므로 의도적으로 표현하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReplicaKind {
    WorkerLocal,
    SubmitterMirror,
    Hub,
    TrustedPeer,
    ExternalObjectStore,
}

/// authority가 해소해야 하는 사실의 결과.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FactResolution<T> {
    Resolved(T),
    Unresolved,
    Ambiguous,
}

/// 현재 key/signature와 holder membership/승인 검증의 결합 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HolderValidation {
    Valid,
    InvalidSignature,
    NotApproved,
    MembershipUnresolved,
    MembershipAmbiguous,
}

/// 외부 resolver가 holder별 freshness와 authority 사실을 해소해 만든 kernel 입력.
///
/// `selected`는 freshness 정책의 결과일 뿐이다. 이 타입이나 kernel은 ACK TTL,
/// 현재 시각 또는 가장 큰 `acked_at_unix_ms`를 freshness 규칙으로 해석하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedHolderObservation {
    pub checkpoint_id: String,
    pub root_digest: String,
    /// authority가 canonical하게 해소한 device ID. 같은 device의 모든 kind는 같은 값이어야 한다.
    pub holder_device_id: String,
    pub acked_at_unix_ms: u64,
    pub kind: ReplicaKind,
    pub selected: bool,
    pub holder_validation: HolderValidation,
    pub is_ephemeral: FactResolution<bool>,
    pub failure_domain: FactResolution<String>,
}

/// report가 평가한 checkpoint/root 범위.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReplicaEvaluationScope {
    pub checkpoint_id: String,
    pub root_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReplicaExclusionReason {
    InvalidSignature,
    HolderNotApproved,
    MembershipUnresolved,
    MembershipAmbiguous,
    EphemeralStatusUnresolved,
    EphemeralStatusAmbiguous,
    FailureDomainUnresolved,
    FailureDomainAmbiguous,
    EphemeralWorkerLocal,
    DuplicateFailureDomain { counted_holder_device_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountedReplica {
    pub observation: ResolvedHolderObservation,
    pub failure_domain: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedReplica {
    pub observation: ResolvedHolderObservation,
    pub reasons: Vec<ReplicaExclusionReason>,
}

/// 순수 effective-replica kernel의 결정적 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveReplicaReport {
    /// observation이 비어 있으면 평가 범위도 없다.
    pub scope: Option<ReplicaEvaluationScope>,
    pub required: Durability,
    pub required_replica_count: u32,
    pub effective_replica_count: u32,
    pub requirement_met: bool,
    pub counted: Vec<CountedReplica>,
    pub excluded: Vec<ExcludedReplica>,
    pub superseded: Vec<ResolvedHolderObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicaEvaluationError {
    EmptyCheckpointId,
    EmptyRootDigest,
    EmptyHolderDeviceId,
    EmptyFailureDomain {
        holder_device_id: String,
        acked_at_unix_ms: u64,
    },
    MixedEvaluationScope {
        scopes: Vec<ReplicaEvaluationScope>,
    },
    MultipleSelectedObservations {
        holder_device_id: String,
        selected: usize,
    },
    ReplicaCountOverflow,
}

/// `docs/protocol/state-machines.md` §4 의 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurabilityState {
    Writing,

    /// 매니페스트 없이 데이터 파일만 존재. 부팅 시 GC 대상.
    Partial,

    LocalWritten,
    HashVerified,
    Replicating,
    Replicated,
    Committed,

    /// COMMITTED 이후 replica 가 유실됨.
    ///
    /// 상태를 되돌리지 않는다. 이미 이 체크포인트를 근거로
    /// 다른 결정이 내려졌을 수 있기 때문이다.
    CommittedDegraded,
}

/// Job 이 요구하는 COMMITTED 조건. `proto/common.proto` 의 `Durability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Durability {
    /// HASH_VERIFIED 로 충분. 워커 디스크 고장 시 유실.
    Local,

    /// REPLICATED(1). 기본값.
    Mirrored,

    /// REPLICATED(2), 서로 다른 failure domain.
    Replicated,
}

impl Durability {
    /// COMMITTED 도달에 필요한 유효 replica 수.
    pub fn required_replicas(self) -> u32 {
        match self {
            Durability::Local => 0,
            Durability::Mirrored => 1,
            Durability::Replicated => 2,
        }
    }
}

/// holder/freshness/membership가 해소된 observation만으로 effective replica 수를 계산한다.
///
/// 외부 상태, 시계, I/O, 난수를 읽지 않는다. malformed scope와 holder별 복수 선택은
/// typed error로 fail closed하며, 미해소 authority 사실은 report에 남기고 세지 않는다.
pub fn evaluate_effective_replicas(
    observations: &[ResolvedHolderObservation],
    required: Durability,
) -> Result<EffectiveReplicaReport, ReplicaEvaluationError> {
    validate_observations(observations)?;

    let scope = observations
        .first()
        .map(|observation| ReplicaEvaluationScope {
            checkpoint_id: observation.checkpoint_id.clone(),
            root_digest: observation.root_digest.clone(),
        });
    let mut selected = observations
        .iter()
        .filter(|observation| observation.selected)
        .cloned()
        .collect::<Vec<_>>();
    let mut superseded = observations
        .iter()
        .filter(|observation| !observation.selected)
        .cloned()
        .collect::<Vec<_>>();
    selected.sort();
    superseded.sort();

    let mut candidates_by_domain = BTreeMap::<String, Vec<ResolvedHolderObservation>>::new();
    let mut excluded = Vec::new();
    for observation in selected {
        let mut reasons = exclusion_reasons(&observation);
        reasons.sort();
        reasons.dedup();
        if reasons.is_empty() {
            let FactResolution::Resolved(failure_domain) = &observation.failure_domain else {
                unreachable!("empty exclusion reasons require a resolved failure domain");
            };
            candidates_by_domain
                .entry(failure_domain.clone())
                .or_default()
                .push(observation);
        } else {
            excluded.push(ExcludedReplica {
                observation,
                reasons,
            });
        }
    }

    let mut counted = Vec::new();
    for (failure_domain, mut candidates) in candidates_by_domain {
        candidates.sort();
        let counted_observation = candidates.remove(0);
        let counted_holder_device_id = counted_observation.holder_device_id.clone();
        counted.push(CountedReplica {
            observation: counted_observation,
            failure_domain,
        });
        for observation in candidates {
            excluded.push(ExcludedReplica {
                observation,
                reasons: vec![ReplicaExclusionReason::DuplicateFailureDomain {
                    counted_holder_device_id: counted_holder_device_id.clone(),
                }],
            });
        }
    }
    excluded.sort_by(|left, right| {
        (&left.observation, &left.reasons).cmp(&(&right.observation, &right.reasons))
    });

    let effective_replica_count =
        u32::try_from(counted.len()).map_err(|_| ReplicaEvaluationError::ReplicaCountOverflow)?;
    let required_replica_count = required.required_replicas();
    Ok(EffectiveReplicaReport {
        scope,
        required,
        required_replica_count,
        effective_replica_count,
        requirement_met: effective_replica_count >= required_replica_count,
        counted,
        excluded,
        superseded,
    })
}

fn validate_observations(
    observations: &[ResolvedHolderObservation],
) -> Result<(), ReplicaEvaluationError> {
    if observations
        .iter()
        .any(|observation| observation.checkpoint_id.trim().is_empty())
    {
        return Err(ReplicaEvaluationError::EmptyCheckpointId);
    }
    if observations
        .iter()
        .any(|observation| observation.root_digest.trim().is_empty())
    {
        return Err(ReplicaEvaluationError::EmptyRootDigest);
    }
    if observations
        .iter()
        .any(|observation| observation.holder_device_id.trim().is_empty())
    {
        return Err(ReplicaEvaluationError::EmptyHolderDeviceId);
    }

    let empty_domain = observations
        .iter()
        .filter_map(|observation| match &observation.failure_domain {
            FactResolution::Resolved(domain) if domain.trim().is_empty() => Some((
                observation.holder_device_id.as_str(),
                observation.acked_at_unix_ms,
            )),
            _ => None,
        })
        .min();
    if let Some((holder_device_id, acked_at_unix_ms)) = empty_domain {
        return Err(ReplicaEvaluationError::EmptyFailureDomain {
            holder_device_id: holder_device_id.to_owned(),
            acked_at_unix_ms,
        });
    }

    let scopes = observations
        .iter()
        .map(|observation| ReplicaEvaluationScope {
            checkpoint_id: observation.checkpoint_id.clone(),
            root_digest: observation.root_digest.clone(),
        })
        .collect::<std::collections::BTreeSet<_>>();
    if scopes.len() > 1 {
        return Err(ReplicaEvaluationError::MixedEvaluationScope {
            scopes: scopes.into_iter().collect(),
        });
    }

    let mut selected_by_holder = BTreeMap::<&str, usize>::new();
    for observation in observations
        .iter()
        .filter(|observation| observation.selected)
    {
        *selected_by_holder
            .entry(observation.holder_device_id.as_str())
            .or_default() += 1;
    }
    if let Some((holder_device_id, selected)) = selected_by_holder
        .into_iter()
        .find(|(_, selected)| *selected > 1)
    {
        return Err(ReplicaEvaluationError::MultipleSelectedObservations {
            holder_device_id: holder_device_id.to_owned(),
            selected,
        });
    }
    Ok(())
}

fn exclusion_reasons(observation: &ResolvedHolderObservation) -> Vec<ReplicaExclusionReason> {
    let mut reasons = Vec::new();
    match observation.holder_validation {
        HolderValidation::Valid => {}
        HolderValidation::InvalidSignature => {
            reasons.push(ReplicaExclusionReason::InvalidSignature);
        }
        HolderValidation::NotApproved => {
            reasons.push(ReplicaExclusionReason::HolderNotApproved);
        }
        HolderValidation::MembershipUnresolved => {
            reasons.push(ReplicaExclusionReason::MembershipUnresolved);
        }
        HolderValidation::MembershipAmbiguous => {
            reasons.push(ReplicaExclusionReason::MembershipAmbiguous);
        }
    }
    if observation.kind == ReplicaKind::WorkerLocal {
        match observation.is_ephemeral {
            FactResolution::Resolved(true) => {
                reasons.push(ReplicaExclusionReason::EphemeralWorkerLocal);
            }
            FactResolution::Resolved(false) => {}
            FactResolution::Unresolved => {
                reasons.push(ReplicaExclusionReason::EphemeralStatusUnresolved);
            }
            FactResolution::Ambiguous => {
                reasons.push(ReplicaExclusionReason::EphemeralStatusAmbiguous);
            }
        }
    }
    match observation.failure_domain {
        FactResolution::Resolved(_) => {}
        FactResolution::Unresolved => {
            reasons.push(ReplicaExclusionReason::FailureDomainUnresolved);
        }
        FactResolution::Ambiguous => {
            reasons.push(ReplicaExclusionReason::FailureDomainAmbiguous);
        }
    }
    reasons
}

impl DurabilityState {
    /// `state-machines.md` §4 전이표.
    ///
    /// 표에 없는 전이는 거부한다.
    pub fn can_transition_to(self, to: DurabilityState) -> bool {
        use DurabilityState::*;

        matches!(
            (self, to),
            (Writing, LocalWritten)
                | (Writing, Partial)
                | (LocalWritten, HashVerified)
                | (LocalWritten, Partial)
                | (HashVerified, Replicating)
                | (HashVerified, Committed)
                | (Replicating, Replicated)
                | (Replicating, HashVerified)
                | (Replicated, Committed)
                | (Replicated, Replicating)
                | (Committed, CommittedDegraded)
                | (CommittedDegraded, Committed)
        )
    }

    pub fn transition(self, to: DurabilityState) -> Result<DurabilityState, CheckpointError> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(CheckpointError::InvalidTransition { from: self, to })
        }
    }

    /// canonical 후보 자격이 있는가.
    ///
    /// `CommittedDegraded` 도 자격을 유지한다.
    pub fn is_canonical_candidate(self) -> bool {
        matches!(
            self,
            DurabilityState::Committed | DurabilityState::CommittedDegraded
        )
    }
}

/// 상태 기록 파일 이름.
///
/// 상태 파일도 write-once artifact 이다. 매니페스트를 덮어쓰지 않는 이유는
/// ADR-026 의 immutable artifact 원칙과 매니페스트 마지막 기록 규칙을
/// 동시에 지키기 위해서다.
pub fn state_marker_name(state: DurabilityState) -> &'static str {
    match state {
        DurabilityState::Writing => ".durability.writing",
        DurabilityState::Partial => ".durability.partial",
        DurabilityState::LocalWritten => ".durability.local-written",
        DurabilityState::HashVerified => ".durability.hash-verified",
        DurabilityState::Replicating => ".durability.replicating",
        DurabilityState::Replicated => ".durability.replicated",
        DurabilityState::Committed => ".durability.committed",
        DurabilityState::CommittedDegraded => ".durability.committed-degraded",
    }
}

pub fn state_marker_path(dir: &Path, state: DurabilityState) -> PathBuf {
    dir.join(state_marker_name(state))
}

/// 포인터 갱신 실패 또는 포인터 이후 상태 기록 실패를 표시한다.
///
/// 완전한 파일과 매니페스트를 삭제하지 않는다. 이 마커가 있는 디렉터리는
/// `find_resume_point*` 에서 제외한다.
pub const PUBLICATION_FAILED_MARKER: &str = ".publication-failed";

pub fn publication_failed_marker_path(dir: &Path) -> PathBuf {
    dir.join(PUBLICATION_FAILED_MARKER)
}

/// WRITING 시작을 디스크에 기록한다.
pub fn record_initial_state(dir: &Path) -> Result<(), CheckpointError> {
    let marker = state_marker_name(DurabilityState::Writing);
    let data = b"Writing\n";
    write_once(dir, marker, data)?;
    Ok(())
}

/// 규범 표의 전이를 검증한 뒤 목적지 상태를 write-once 로 기록한다.
///
/// 매니페스트를 다시 쓰지 않으므로 상태 전이가 원자성을 깨지 않는다.
pub fn record_state_transition(
    dir: &Path,
    from: DurabilityState,
    to: DurabilityState,
) -> Result<(), CheckpointError> {
    from.transition(to)?;

    let marker = state_marker_name(to);
    let data = format!("{to:?}\n");
    write_once(dir, marker, data.as_bytes())?;
    Ok(())
}

/// 포인터 공개 실패를 불변 마커로 기록한다.
pub fn record_publication_failure(dir: &Path) -> Result<(), CheckpointError> {
    write_once(
        dir,
        PUBLICATION_FAILED_MARKER,
        b"checkpoint publication failed\n",
    )?;
    Ok(())
}

/// 상태 마커의 내용까지 확인한다.
///
/// 파일이 없으면 아직 해당 상태에 도달하지 않은 것이다.
pub fn state_recorded(dir: &Path, state: DurabilityState) -> Result<bool, CheckpointError> {
    let expected = format!("{state:?}\n");

    match crate::platform::read_beneath(dir, Path::new(state_marker_name(state))) {
        Ok(actual) => Ok(actual == expected.as_bytes()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn publication_failed(dir: &Path) -> Result<bool, CheckpointError> {
    match crate::platform::read_beneath(dir, Path::new(PUBLICATION_FAILED_MARKER)) {
        Ok(actual) => Ok(actual == b"checkpoint publication failed\n"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// 체크포인트를 구성하는 파일 하나.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointFile {
    /// 체크포인트 루트 기준 상대 경로. `..` · 절대경로 · 심볼릭 링크 금지.
    pub path: String,

    /// BLAKE3-256 hex.
    pub digest: String,

    pub size_bytes: u64,
}

/// `proto/artifact.proto` 의 `CheckpointManifest` 에 대응.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointManifest {
    pub schema_version: u32,
    pub checkpoint_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub step: u64,
    pub files: Vec<CheckpointFile>,
    pub root_digest: String,
    pub total_bytes: u64,
    pub created_at_unix_ms: u64,
    pub producer_node_id: String,
    pub fence_epoch: u64,
}

pub const MANIFEST_FILENAME: &str = "manifest.json";

impl CheckpointManifest {
    /// 매니페스트에 적힌 모든 파일의 해시를 재계산해 대조한다.
    pub fn verify_files(&self, dir: &Path) -> Result<(), CheckpointError> {
        for file in &self.files {
            let path = dir.join(&file.path);
            let data = crate::platform::read_beneath(dir, Path::new(&file.path))?;
            let actual = blake3::hash(&data).to_hex().to_string();

            if actual != file.digest {
                return Err(CheckpointError::HashMismatch {
                    path,
                    expected: file.digest.clone(),
                    actual,
                });
            }
        }

        Ok(())
    }

    pub fn to_json(&self) -> Result<Vec<u8>, CheckpointError> {
        serde_json::to_vec_pretty(self)
            .map_err(|error| CheckpointError::Manifest(error.to_string()))
    }

    pub fn from_json(data: &[u8]) -> Result<Self, CheckpointError> {
        serde_json::from_slice(data).map_err(|error| CheckpointError::Manifest(error.to_string()))
    }
}

/// 기존 호출자 호환용 단순 replica accumulator.
///
/// holder별 freshness, replica kind, current membership 해석을 표현하지 못하므로 새
/// 결정 경로는 [`evaluate_effective_replicas`]를 사용해야 한다.
#[derive(Debug, Clone, Default)]
pub struct ReplicaSet {
    /// failure_domain -> (device_id, is_ephemeral, signature_valid)
    entries: BTreeMap<String, (String, bool, bool)>,
}

impl ReplicaSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(
        &mut self,
        failure_domain: impl Into<String>,
        device_id: impl Into<String>,
        is_ephemeral: bool,
        signature_valid: bool,
    ) -> &mut Self {
        self.entries.insert(
            failure_domain.into(),
            (device_id.into(), is_ephemeral, signature_valid),
        );
        self
    }

    /// legacy entry 형태가 표현할 수 있는 signature/ephemeral/domain 필터 결과.
    pub fn effective_count(&self) -> u32 {
        self.entries
            .values()
            .filter(|(_, is_ephemeral, signature_valid)| *signature_valid && !*is_ephemeral)
            .count() as u32
    }

    pub fn satisfies(&self, required: Durability) -> bool {
        self.effective_count() >= required.required_replicas()
    }
}
