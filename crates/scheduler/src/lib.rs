//! 결정론적인 scheduler hard-filter kernel.
//!
//! 이 크레이트는 마스터 플랜 §13.2의 전체 scheduler가 아니라
//! `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md` 조각 1에서
//! 허용한 gate만 판정한다. 외부 I/O, 현재 시각 조회, 무작위 선택, 순위 계산,
//! 자원 예약을 하지 않는다.

mod filter;
mod model;

pub use filter::evaluate_eligibility;
pub use model::{
    CandidateSnapshot, EligibilityReport, EligibilityResolution, EligibleCandidate, GpuSnapshot,
    IsolationClass, JobRequirements, KeyProtection, MissingFact, NodeState, Policy, PoolSnapshot,
    RejectedCandidate, RejectionReason, RiskState, SecurityTier, Sensitivity, SideEffectClass,
    WorkloadClass,
};
