use std::collections::BTreeSet;

/// 마스터 플랜 §27.1의 Node 상태.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NodeState {
    Discovered,
    Enrolling,
    EnrollRejected,
    Approved,
    Online,
    Suspect,
    Unreachable,
    Lost,
    Recovering,
    Draining,
    Offline,
    Quarantined,
    Revoked,
    Terminated,
}

/// 마스터 플랜 §8.4의 독립된 위험 축.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RiskState {
    Normal,
    Suspect,
    Quarantined,
    Revoked,
}

/// Capability probe로 도출되는 §9.1 등급. 선언값으로 사용하면 안 된다.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SecurityTier {
    S0,
    S1,
    S2,
    S3,
    S4,
    S5,
}

/// 격리의 성격을 나타내는 §9.1의 독립 축.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IsolationClass {
    Restricted,
    Contained,
    Virtualized,
}

/// §7.3.1의 개인키 보호 수준.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum KeyProtection {
    K0,
    K1,
    K2,
}

/// §11.5에 정의된 workload class.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum WorkloadClass {
    Training,
    Inference,
    Preprocessing,
    Evaluation,
    Rendering,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SideEffectClass {
    Pure,
    Idempotent,
    SideEffecting,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Sensitivity {
    Public,
    Internal,
    Sensitive,
}

/// 한 GPU의 고정 관측값. `None`은 요구 없음이 아니라 관측되지 않음을 뜻한다.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GpuSnapshot {
    pub gpu_id: String,
    pub healthy: Option<bool>,
    pub available_vram_bytes: Option<u64>,
    pub model: Option<String>,
}

/// 한 노드에 대해 같은 시점에 고정된 scheduler 입력.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSnapshot {
    pub node_id: String,
    /// Agent inventory generation used to build this candidate. This is an
    /// admission CAS token, not a filtering or ranking input.
    pub inventory_revision: Option<u64>,
    pub owner_member_id: Option<String>,
    pub node_state: Option<NodeState>,
    pub risk_state: Option<RiskState>,
    pub observed_at_unix_ms: Option<u64>,
    pub security_tier: Option<SecurityTier>,
    pub isolation_class: Option<IsolationClass>,
    pub key_protection: Option<KeyProtection>,
    pub gpus: Option<Vec<GpuSnapshot>>,
    pub available_cpu_cores: Option<u32>,
    pub available_ram_bytes: Option<u64>,
    pub available_workspace_bytes: Option<u64>,
    pub allowed_workload_classes: Option<BTreeSet<WorkloadClass>>,
    /// `RESTRICTED` 격리 장치 단위 제3자 Job opt-in(§9.2).
    /// 관련 없는 노드에서는 `None`이어도 된다.
    pub third_party_workloads_opt_in: Option<bool>,
}

/// Manifest/proto 기본값을 그대로 사용하지 않는 명시적 domain 요구사항.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobRequirements {
    pub submitter_member_id: Option<String>,
    pub workload_class: Option<WorkloadClass>,
    pub side_effect_class: Option<SideEffectClass>,
    pub sensitivity: Option<Sensitivity>,
    pub minimum_security_tier: Option<SecurityTier>,
    pub minimum_isolation_class: Option<IsolationClass>,
    pub minimum_key_protection: Option<KeyProtection>,
    pub minimum_gpu_count: Option<u32>,
    pub minimum_vram_bytes_per_gpu: Option<u64>,
    /// 빈 목록은 마스터 플랜의 `GpuRequest`와 같이 모델 제약이 없음을 뜻한다.
    pub allowed_gpu_models: Vec<String>,
    pub cpu_cores: Option<u32>,
    pub ram_bytes: Option<u64>,
    pub workspace_bytes: Option<u64>,
}

/// `evaluated_at_unix_ms`도 입력이므로 kernel은 시스템 clock을 읽지 않는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolSnapshot {
    pub evaluated_at_unix_ms: u64,
    pub candidates: Vec<CandidateSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    pub maximum_snapshot_age_ms: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MissingFact {
    NodeId,
    NodeState,
    RiskState,
    ObservedAt,
    SecurityTier,
    IsolationClass,
    KeyProtection,
    OwnerMemberId,
    GpuInventory,
    GpuHealth { gpu_id: String },
    GpuVram { gpu_id: String },
    GpuModel { gpu_id: String },
    AvailableCpu,
    AvailableRam,
    AvailableWorkspace,
    AllowedWorkloadClasses,
    ThirdPartyOptIn,
    JobSubmitterMemberId,
    JobWorkloadClass,
    JobSideEffectClass,
    JobSensitivity,
    JobMinimumSecurityTier,
    JobMinimumIsolationClass,
    JobMinimumKeyProtection,
    JobMinimumGpuCount,
    JobMinimumVram,
    JobCpu,
    JobRam,
    JobWorkspace,
}

/// 순서는 보고서 안의 결정적 reason 순서이기도 하다.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RejectionReason {
    MissingFact(MissingFact),
    NodeNotOnline { actual: NodeState },
    RiskNotNormal { actual: RiskState },
    SnapshotNotFresh {
        observed_at_unix_ms: u64,
        evaluated_at_unix_ms: u64,
        maximum_age_ms: u64,
    },
    SecurityTierTooLow { available: SecurityTier, required: SecurityTier },
    IsolationClassTooLow { available: IsolationClass, required: IsolationClass },
    KeyProtectionTooLow { available: KeyProtection, required: KeyProtection },
    GpuCountInsufficient { available: u32, required: u32 },
    HealthyGpuCountInsufficient { available: u32, required: u32 },
    GpuModelMismatch { matching: u32, required: u32, allowed: Vec<String> },
    GpuVramInsufficient { matching: u32, required: u32, minimum_bytes: u64 },
    CpuInsufficient { available: u32, required: u32 },
    RamInsufficient { available: u64, required: u64 },
    WorkspaceInsufficient { available: u64, required: u64 },
    WorkloadClassNotAllowed { workload_class: WorkloadClass },
    ThirdPartyJobForbiddenOnS0,
    ThirdPartyOptInRequiredOnRestrictedIsolation,
    ThirdPartyJobMustBePureOnRestrictedIsolation { actual: SideEffectClass },
    ThirdPartySensitiveDataForbiddenOnRestrictedIsolation,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EligibleCandidate {
    pub node_id: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RejectedCandidate {
    pub node_id: String,
    pub reasons: Vec<RejectionReason>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EligibilityResolution {
    NoEligibleCandidates,
    SingleEligible { node_id: String },
    RankingRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EligibilityReport {
    pub eligible: Vec<EligibleCandidate>,
    pub rejected: Vec<RejectedCandidate>,
    pub resolution: EligibilityResolution,
}

/// resource-tight best-fit에서 비교할 자원 축.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FitAxis {
    Vram,
    GpuCount,
    Cpu,
    Ram,
    Workspace,
}

/// 마스터 플랜에는 v0.1 자원 축의 고정 우선순위가 없으므로 기본값을 두지 않는다.
/// 호출자는 다섯 축을 중복 없이 모두 나열해야 한다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BestFitPolicy {
    pub axis_order: [FitAxis; 5],
}

/// Job을 배치한 뒤 남는 자원량. 각 값은 작을수록 더 tight한 fit이다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FitKey {
    pub vram_remaining_bytes: u64,
    pub gpu_count_remaining: u32,
    pub cpu_cores_remaining: u32,
    pub ram_remaining_bytes: u64,
    pub workspace_remaining_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankedCandidate {
    pub node_id: String,
    pub fit_key: FitKey,
}

/// `winner`는 `ranked[0]`과 항상 같다. 이 결과는 reservation이나 Grant가 아니다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BestFitRanking {
    pub winner: RankedCandidate,
    pub ranked: Vec<RankedCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RankingError {
    InvalidPolicyAxisOrder,
    ResolutionNotRankingRequired,
    EligibleCandidateCountNotMultiple { actual: usize },
    EmptyNodeId,
    DuplicatePoolNodeId { node_id: String },
    DuplicateReportNodeId { node_id: String },
    ReportCandidateMissingFromPool { node_id: String },
    PoolCandidateMissingFromReport { node_id: String },
    MissingRankFact { node_id: Option<String>, fact: MissingFact },
    EligibleCandidateMismatch { node_id: String, axis: FitAxis },
    FitOverflow { node_id: String, axis: FitAxis },
}
