//! 결정론적인 scheduler hard-filter kernel.
//!
//! 이 크레이트는 마스터 플랜 §13.2의 전체 scheduler가 아니라
//! `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md` 조각 1에서
//! 허용한 hard gate, resource-tight best-fit, selected GPU assignment와
//! 검증된 입력으로부터의 `ScopeCandidate`만 계산한다.
//! 외부 I/O, 현재 시각 조회, 무작위 선택, 자원 예약을 하지 않는다.

mod filter;
mod model;
mod rank;
mod scope;

pub use filter::evaluate_eligibility;
pub use model::{
    BestFitPolicy, BestFitRanking, CandidateSnapshot, EligibilityReport, EligibilityResolution,
    EligibleCandidate, FitAxis, FitKey, GpuSnapshot, IsolationClass, JobRequirements, KeyProtection,
    MissingFact, NodeState, Policy, PoolSnapshot, RankedCandidate, RankingError, RejectedCandidate,
    RejectionReason, ResourceFit, RiskState, SecurityTier, Sensitivity, SideEffectClass,
    WorkloadClass,
};
pub use rank::{rank_best_fit, resource_fit};
pub use scope::{
    gpu_scope_candidate, AvailableVramObservation, GpuAllocationMode, GpuObservationSnapshot,
    JobGpuRequirements, ProvenanceGate, ScopeCandidate, ScopeError, ScopeGpuObservation,
    ScopeMissingFact, ScopeResourceInput,
};
