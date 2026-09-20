//! Attempt 상태기계 — `docs/protocol/state-machines.md` §3 의 구현.
//!
//! # 이 파일이 늦게 생긴 이유
//!
//! `CLAUDE.md` §2 는 "표에 없는 상태 전이를 구현하지 않는다" 고 정하고,
//! `state-machines.md` §6 은 표를 테스트가 파싱해 대조하라고 요구한다.
//! 그런데 §6 의 검사 범위표는 **Checkpoint 만** 강제된다고 적고 있었다 —
//! Node·Job·Attempt·Lease 는 표만 있고 코드가 없었다.
//!
//! Attempt 를 먼저 채우는 이유는 **지금 이 저장소가 실제로 그 전이를
//! 만들어내기 시작했기 때문**이다. Agent 가 프로세스를 띄우고 종료
//! 코드를 관측하며(`crates/agent/src/exec.rs`) 산출물을 확정하는데,
//! 그게 정확히 표의 `WORKLOAD_EXITED_OK`/`WORKLOAD_EXITED_ERROR` 다.
//! 만들어낼 수 없는 전이를 먼저 구현하지 않는다.
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! guard 판정      표의 guard 열은 "언제 그 전이를 시도해도 되는가" 이지
//!                 "그 전이가 표에 있는가" 가 아니다. 전자는 호출부의
//!                 책임이다 — 여기서 흉내 내면 실제로 강제하지 못하는
//!                 것을 강제한다고 주장하게 된다(§0.4)
//! durability 강제  표의 durability 열이 요구하는 합의/영속화를 이
//!                 모듈이 수행하지 않는다. 어떤 등급이 필요한지 알려줄
//!                 뿐이고, 채우는 것은 저장소 계층이다
//! 상태 보관       현재 상태를 어디에 저장할지 정하지 않는다. 순수
//!                 판정 함수만 제공한다 — 시계·I/O·전역 상태가 없다
//! ```
//!
//! # 순수 커널이다
//!
//! 시계·I/O·난수·전역 상태를 쓰지 않는다. 같은 입력에 항상 같은 답을
//! 낸다. 그래서 `crates/checkpoint/tests/state_table_parity.rs` 가
//! 문서 표와 이 코드를 **양방향으로** 대조할 수 있다.

/// `state-machines.md` §3 의 Attempt 상태.
///
/// ★ 표에 있는 이름과 1:1 로 대응한다. 표에 없는 상태를 여기에
///   추가하면 parity 테스트가 실패한다 — 그게 이 설계의 목적이다.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AttemptState {
    Created,
    Starting,
    Running,
    Paused,
    Stale,
    Completed,
    Failed,
    Cancelled,
    Reconciling,
    Canonical,
    Superseded,
}

/// 이 저장소가 아는 모든 Attempt 상태.
///
/// parity 테스트가 "구현이 허용하는 전이가 표에 전부 있는가" 를 검사할
/// 때 전수 조사의 정의역으로 쓴다.
pub const ALL_ATTEMPT_STATES: &[AttemptState] = &[
    AttemptState::Created,
    AttemptState::Starting,
    AttemptState::Running,
    AttemptState::Paused,
    AttemptState::Stale,
    AttemptState::Completed,
    AttemptState::Failed,
    AttemptState::Cancelled,
    AttemptState::Reconciling,
    AttemptState::Canonical,
    AttemptState::Superseded,
];

impl AttemptState {
    /// 표의 상태 이름. 문서와 코드를 잇는 유일한 지점이다.
    pub fn table_name(self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::Starting => "STARTING",
            Self::Running => "RUNNING",
            Self::Paused => "PAUSED",
            Self::Stale => "STALE",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::Reconciling => "RECONCILING",
            Self::Canonical => "CANONICAL",
            Self::Superseded => "SUPERSEDED",
        }
    }

    /// 여기서 나가는 전이가 하나도 없는가.
    ///
    /// ★ `COMPLETED` 는 terminal 이 **아니다.** 표에
    ///   `COMPLETED -> RECONCILING (DUPLICATE_DETECTED)` 이 있다 —
    ///   같은 Job 의 다른 attempt 도 완료를 보고하면 순위를 다시
    ///   매겨야 하기 때문이다. "완료했으니 끝" 으로 접으면 중복 완료를
    ///   영영 조정하지 못한다.
    pub fn is_terminal(self) -> bool {
        !ALL_ATTEMPT_STATES
            .iter()
            .any(|to| !transition_triggers(self, *to).is_empty())
    }
}

/// `from -> to` 전이를 일으키는 trigger 이름들. 표에 없으면 빈 슬라이스.
///
/// # 왜 bool 이 아니라 trigger 목록인가
///
/// bool 이면 "전이가 허용된다" 만 알 수 있고 **어느 행 때문에** 허용되는지는
/// 모른다. parity 테스트가 문서 행과 코드 분기를 1:1 로 대조하려면 이름이
/// 필요하다 — 이름 없이 대조하면 표에 두 행이 있는데 코드에 하나만 있어도
/// 통과한다.
///
/// # 왜 하나가 아니라 목록인가
///
/// ★ 초안은 `Option<&str>` 로 **하나만** 돌려줬다. parity 테스트가 바로
///   잡았다 — 표에는 같은 `from -> to` 에 trigger 가 여럿인 행이 있다.
///
///   ```text
///   CREATED -> CANCELLED   GRANT_REJECTED         Agent 검증 실패
///                          JOB_CANCELLED          Job 자체가 취소됨
///   RUNNING -> FAILED      WORKLOAD_EXITED_ERROR  종료 코드 != 0
///                          WATCHDOG_KILLED        no-progress 판정
///   ```
///
///   둘은 **원인도 effect 도 다르다**(전자는 감사 로그에 실패 단계 번호를
///   남기고 후자는 lease 를 반납한다 / 전자는 로그를 보존하고 후자는
///   process tree 를 죽이고 VRAM 반환을 확인한다). 하나로 접으면 그 차이가
///   사라지고, 표의 나머지 행은 아무도 검사하지 않게 된다.
pub fn transition_triggers(from: AttemptState, to: AttemptState) -> &'static [&'static str] {
    use AttemptState::*;
    match (from, to) {
        (Created, Starting) => &["GRANT_ACCEPTED"],
        (Created, Cancelled) => &["GRANT_REJECTED", "JOB_CANCELLED"],
        (Starting, Running) => &["PROCESS_STARTED"],
        (Starting, Failed) => &["START_FAILED"],
        (Running, Completed) => &["WORKLOAD_EXITED_OK"],
        (Running, Failed) => &[
            "WORKLOAD_EXITED_ERROR",
            "WATCHDOG_KILLED",
            // B+E(결정 D3) — 정상 종료 뒤 필요한 산출물 확정 실패.
            "OUTPUT_FINALIZATION_FAILED",
        ],
        (Running, Paused) => &["PAUSE_REQUESTED"],
        (Running, Stale) => &["LEASE_EXPIRED"],
        (Paused, Running) => &["RESUME_REQUESTED"],
        (Paused, Cancelled) => &["JOB_CANCELLED"],
        (Stale, Running) => &["LEASE_RENEWED"],
        (Stale, Completed) => &["STALE_WORKLOAD_FINISHED"],
        (Stale, Failed) => &["STALE_WORKLOAD_FAILED"],
        (Stale, Reconciling) => &["RECONNECTED_WITH_RESULT"],
        (Completed, Reconciling) => &["DUPLICATE_DETECTED"],
        (Reconciling, Canonical) => &["SELECTED"],
        (Reconciling, Superseded) => &["NOT_SELECTED"],
        (Reconciling, Failed) => &["VALIDITY_FILTER_REJECTED"],
        _ => &[],
    }
}

/// 전이가 거부된 이유.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttemptTransitionRejected {
    pub from: AttemptState,
    pub to: AttemptState,
}

impl std::fmt::Display for AttemptTransitionRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ATTEMPT_TRANSITION_NOT_IN_TABLE: {} -> {} 는 state-machines.md §3 표에 없다",
            self.from.table_name(),
            self.to.table_name()
        )
    }
}

impl std::error::Error for AttemptTransitionRejected {}

/// 전이를 시도한다. 표에 없으면 거부한다.
///
/// ★ **표에 있다 = 지금 해도 된다** 가 아니다. guard 는 호출부가
///   확인한다. 이 함수는 "그 전이가 규범에 존재하는가" 만 답한다.
pub fn transition(
    from: AttemptState,
    to: AttemptState,
) -> Result<AttemptState, AttemptTransitionRejected> {
    if !transition_triggers(from, to).is_empty() {
        Ok(to)
    } else {
        Err(AttemptTransitionRejected { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_is_not_terminal_because_duplicates_must_be_reconciled() {
        assert!(
            !AttemptState::Completed.is_terminal(),
            "COMPLETED 를 terminal 로 보면 중복 완료를 영영 조정하지 못한다"
        );
        assert_eq!(
            transition_triggers(AttemptState::Completed, AttemptState::Reconciling),
            ["DUPLICATE_DETECTED"]
        );
    }

    #[test]
    fn terminal_states_have_no_outgoing_transitions() {
        for state in [
            AttemptState::Failed,
            AttemptState::Cancelled,
            AttemptState::Canonical,
            AttemptState::Superseded,
        ] {
            assert!(
                state.is_terminal(),
                "{} 에서 나가는 전이가 있다 — 표를 다시 보라",
                state.table_name()
            );
        }
    }

    /// 표에 없는 전이는 거부돼야 한다.
    ///
    /// ★ 특히 `CREATED -> RUNNING`(STARTING 건너뛰기)과
    ///   `RUNNING -> CANONICAL`(RECONCILING 건너뛰기)이 막혀야 한다.
    ///   전자를 열면 Agent 검증 없이 실행됐다고 기록할 수 있고,
    ///   후자를 열면 순위 결정 없이 canonical 이 정해진다.
    #[test]
    fn shortcuts_that_skip_a_gate_are_rejected() {
        for (from, to) in [
            (AttemptState::Created, AttemptState::Running),
            (AttemptState::Running, AttemptState::Canonical),
            (AttemptState::Failed, AttemptState::Running),
            (AttemptState::Completed, AttemptState::Canonical),
        ] {
            let error = transition(from, to).expect_err(&format!(
                "{} -> {} 가 통과했다",
                from.table_name(),
                to.table_name()
            ));
            assert_eq!(error.from, from);
            assert_eq!(error.to, to);
        }
    }
}
