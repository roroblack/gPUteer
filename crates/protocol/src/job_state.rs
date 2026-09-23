//! Job 상태기계 — `docs/protocol/state-machines.md` §2 의 구현.
//!
//! # 이 파일이 생긴 이유 (2026-09-23)
//!
//! Job 은 `SUBMITTED -> PLANNING -> QUEUED -> STAGING` 까지만 저장소가
//! 움직였고, 그 뒤는 **아무도 적지 않았다.** 시도가 끝나도(`attempt_state`)
//! Job 은 영원히 `STAGING` 이었다 — 사용자가 "내 작업 끝났나" 를 물을 곳이 없었다.
//! 신뢰망 P1 이 시도의 끝을 적게 되면서, 그 사실을 Job 까지 올릴 수 있게 됐다.
//! 만들어낼 수 있는 전이가 생겼으므로 표를 코드로 옮긴다(`attempt_state` 와 같은 원칙).
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! guard 판정       표의 guard 열(“canonical attempt 확정”, “lease 만료 + grace 경과” …)은
//!                  호출부의 책임이다. 여기서 흉내 내면 강제하지 못하는 것을
//!                  강제한다고 주장하게 된다(CLAUDE.md §0.4)
//! durability 강제  COMMITTED 등급을 채우는 것은 저장소 계층이다. 지금 저장소는
//!                  단일 Coordinator 의 로컬 DURABLE 이다 — COMMITTED 가 아니다
//! 상태 보관        순수 판정 함수만 둔다 — 시계·I/O·전역 상태가 없다
//! ```
//!
//! 그래서 `crates/checkpoint/tests/state_table_parity.rs` 가 문서 표와 이 코드를
//! **양방향으로** 대조한다.

/// `state-machines.md` §2 의 Job 상태. 표의 이름과 1:1 이다.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum JobState {
    Submitted,
    Planning,
    Queued,
    Staging,
    Running,
    Interrupted,
    Replanning,
    Paused,
    Reconciling,
    Completed,
    Failed,
    Cancelled,
    Archived,
}

/// 이 저장소가 아는 모든 Job 상태 — parity 전수 조사의 정의역.
pub const ALL_JOB_STATES: &[JobState] = &[
    JobState::Submitted,
    JobState::Planning,
    JobState::Queued,
    JobState::Staging,
    JobState::Running,
    JobState::Interrupted,
    JobState::Replanning,
    JobState::Paused,
    JobState::Reconciling,
    JobState::Completed,
    JobState::Failed,
    JobState::Cancelled,
    JobState::Archived,
];

impl JobState {
    /// 표의 상태 이름. 문서와 코드를 잇는 유일한 지점이다.
    pub fn table_name(self) -> &'static str {
        match self {
            Self::Submitted => "SUBMITTED",
            Self::Planning => "PLANNING",
            Self::Queued => "QUEUED",
            Self::Staging => "STAGING",
            Self::Running => "RUNNING",
            Self::Interrupted => "INTERRUPTED",
            Self::Replanning => "REPLANNING",
            Self::Paused => "PAUSED",
            Self::Reconciling => "RECONCILING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::Archived => "ARCHIVED",
        }
    }

    /// 여기서 나가는 전이가 하나도 없는가.
    ///
    /// ★ `COMPLETED` 는 terminal 이 아니다 — `COMPLETED -> ARCHIVED` 가 있다.
    ///   "끝났는가" 를 물을 때는 [`JobState::is_finished`] 를 쓴다.
    pub fn is_terminal(self) -> bool {
        !ALL_JOB_STATES
            .iter()
            .any(|to| !transition_triggers(self, *to).is_empty())
    }

    /// 사용자에게 "끝났다" 고 말할 수 있는가 — 더 실행되지 않는다.
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Archived
        )
    }
}

/// `from -> to` 전이를 일으키는 trigger 이름들. 표에 없으면 빈 슬라이스.
///
/// 이름까지 돌려주는 이유는 `attempt_state::transition_triggers` 와 같다 —
/// 같은 쌍에 trigger 가 여럿인 행(`QUEUED -> FAILED` 셋, `RUNNING -> PAUSED` 셋)을
/// 하나로 접으면 나머지 행을 아무도 검사하지 않게 된다.
pub fn transition_triggers(from: JobState, to: JobState) -> &'static [&'static str] {
    use JobState::*;
    match (from, to) {
        (Submitted, Planning) => &["PLANNING_STARTED"],
        (Submitted, Cancelled) => &["USER_CANCELLED"],
        (Planning, Queued) => &["PLAN_READY"],
        (Planning, Failed) => &["NO_FEASIBLE_PLAN"],
        (Planning, Cancelled) => &["USER_CANCELLED"],
        (Queued, Staging) => &["RESOURCE_AVAILABLE"],
        (Queued, Planning) => &["REPLAN_REQUIRED"],
        (Queued, Failed) => &["DEADLINE_PASSED", "QUEUE_TIMEOUT", "PERMANENTLY_INFEASIBLE"],
        (Queued, Cancelled) => &["USER_CANCELLED"],
        (Staging, Running) => &["STAGING_COMPLETE"],
        (Staging, Failed) => &["STAGING_FAILED"],
        (Staging, Queued) => &["STAGING_NODE_LOST"],
        (Staging, Cancelled) => &["USER_CANCELLED"],
        (Running, Completed) => &["ATTEMPT_COMPLETED"],
        (Running, Interrupted) => &["NODE_LOST"],
        (Running, Failed) => &["UNRECOVERABLE_ERROR"],
        (Running, Paused) => &["PARTITION_PAUSE", "OWNER_PREEMPT", "USER_PAUSED"],
        (Running, Reconciling) => &["DUPLICATE_COMPLETION"],
        (Running, Cancelled) => &["USER_CANCELLED"],
        (Interrupted, Replanning) => &["FAILOVER_STARTED"],
        (Interrupted, Failed) => &["NO_COMMITTED_CHECKPOINT"],
        (Interrupted, Cancelled) => &["USER_CANCELLED"],
        (Replanning, Queued) => &["REPLAN_READY"],
        (Replanning, Staging) => &["REPLAN_DIRECT"],
        (Replanning, Failed) => &["NO_FEASIBLE_PLAN"],
        (Replanning, Cancelled) => &["USER_CANCELLED"],
        (Paused, Running) => &["RESUMED"],
        (Paused, Failed) => &["PAUSE_TIMEOUT"],
        (Paused, Cancelled) => &["USER_CANCELLED"],
        (Reconciling, Completed) => &["CANONICAL_CHOSEN"],
        (Reconciling, Failed) => &["ALL_ATTEMPTS_INVALID"],
        (Reconciling, Reconciling) => &["TIE_UNRESOLVED"],
        (Completed, Archived) => &["RETENTION_EXPIRED"],
        _ => &[],
    }
}

/// 전이가 거부된 이유.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobTransitionRejected {
    pub from: JobState,
    pub to: JobState,
}

impl std::fmt::Display for JobTransitionRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JOB_TRANSITION_NOT_IN_TABLE: {} -> {} 는 state-machines.md §2 표에 없다",
            self.from.table_name(),
            self.to.table_name()
        )
    }
}

impl std::error::Error for JobTransitionRejected {}

/// 전이를 시도한다. 표에 없으면 거부한다. guard 는 호출부가 확인한다.
pub fn transition(from: JobState, to: JobState) -> Result<JobState, JobTransitionRejected> {
    if !transition_triggers(from, to).is_empty() {
        Ok(to)
    } else {
        Err(JobTransitionRejected { from, to })
    }
}

/// 전이를 **그 trigger 로** 시도한다 — 상태 쌍이 표에 있어도 trigger 가 그 행의 것이 아니면 거부한다.
///
/// ★ 2026-09-23 (결함 216 · 검수 73) — [`transition`] 은 상태 쌍만 본다. 그래서 `RUNNING -> FAILED` 를
///   `STAGING_FAILED` 로 적는 오류가 통과했다(그 쌍의 trigger 는 `UNRECOVERABLE_ERROR` 다).
///   저장하는 trigger 가 있는 곳은 이것을 쓴다.
pub fn transition_via(
    from: JobState,
    to: JobState,
    trigger: &str,
) -> Result<JobState, JobTransitionRejected> {
    if transition_triggers(from, to).contains(&trigger) {
        Ok(to)
    } else {
        Err(JobTransitionRejected { from, to })
    }
}

/// `from` 에서 `path` 를 차례로 밟는다. 한 칸이라도 표에 없으면 거부한다.
///
/// ★ 저장소는 **최종 상태만** 쓴다(코덱스 논의 72 결정 C — Attempt 와 같다).
///   이 함수는 "그 경로가 규범 안에 있는가" 만 확인한다.
pub fn walk(from: JobState, path: &[JobState]) -> Result<JobState, JobTransitionRejected> {
    let mut current = from;
    for step in path {
        current = transition(current, *step)?;
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_is_not_terminal_but_is_finished() {
        assert!(!JobState::Completed.is_terminal());
        assert!(JobState::Completed.is_finished());
        assert!(JobState::Failed.is_terminal());
        assert!(!JobState::Running.is_finished());
    }

    /// 게이트를 건너뛰는 지름길은 막혀야 한다.
    ///
    /// ★ `STAGING -> COMPLETED`(실행을 건너뛴 완료)와 `INTERRUPTED -> QUEUED`
    ///   (체크포인트 확인 없이 재배치)가 특히 그렇다. 후자를 열면
    ///   `FAILOVER_STARTED` 의 guard(마지막 COMMITTED checkpoint 존재)를
    ///   아무도 확인하지 않고 작업을 처음부터 다시 돌리게 된다.
    #[test]
    fn shortcuts_that_skip_a_gate_are_rejected() {
        for (from, to) in [
            (JobState::Staging, JobState::Completed),
            (JobState::Interrupted, JobState::Queued),
            (JobState::Queued, JobState::Running),
            (JobState::Failed, JobState::Queued),
        ] {
            assert_eq!(
                transition(from, to),
                Err(JobTransitionRejected { from, to })
            );
        }
    }

    #[test]
    fn walk_accepts_a_normative_path_and_rejects_a_broken_one() {
        assert_eq!(
            walk(JobState::Staging, &[JobState::Running, JobState::Completed]),
            Ok(JobState::Completed)
        );
        assert_eq!(
            walk(
                JobState::Running,
                &[
                    JobState::Interrupted,
                    JobState::Replanning,
                    JobState::Queued
                ]
            ),
            Ok(JobState::Queued)
        );
        assert!(walk(JobState::Staging, &[JobState::Completed]).is_err());
    }

    /// 결함 216 — 상태 쌍이 맞아도 trigger 가 그 행의 것이 아니면 거부한다.
    #[test]
    fn transition_via_rejects_a_trigger_from_another_row() {
        assert_eq!(
            transition_via(JobState::Running, JobState::Failed, "UNRECOVERABLE_ERROR"),
            Ok(JobState::Failed)
        );
        assert_eq!(
            transition_via(JobState::Running, JobState::Failed, "STAGING_FAILED"),
            Err(JobTransitionRejected {
                from: JobState::Running,
                to: JobState::Failed
            })
        );
        assert_eq!(
            transition_via(JobState::Staging, JobState::Failed, "STAGING_FAILED"),
            Ok(JobState::Failed)
        );
    }
}
