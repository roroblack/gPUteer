//! `AttemptReport` 의 **필드 조합 규칙** — B+E 계약 단계 1(계획서 §5.7 · 제안서 조건 (e)).
//!
//! 서명 검증(`Verified`)은 "누가 보냈고 바뀌지 않았다" 를 보장할 뿐, **필드끼리의 조합이 뜻이 통하는가**는 보지 않는다.
//! 이 함수가 그것을 본다. 보고를 **만드는 쪽**(Agent, 서명 전)과 **소비하는 모든 쪽**(Coordinator 의 수신 · 저장 진입 ·
//! 재조회 뒤 재검증 · 예약 해제)이 같은 함수를 부른다 — 경로마다 다른 검사가 생기지 않게.
//!
//! ```text
//! 모든 버전          outcome UNSPECIFIED 거부 · 모르는 enum 값 거부
//! schema_version 1   outcome 은 {COMPLETED, FAILED, INTERRUPTED, CANCELLED, STALE_COMPLETED} 만(결함 71) ·
//!                    새 필드(14 · 15 · 16)는 전부 기본값
//! schema_version 2   outcome 별 허용 조합(아래 표) · exit_observation 필수 · exit_code 는 OBSERVED_WITH_CODE 일 때만
//!
//! outcome                  exit_observation                 exit_code   stage
//! COMPLETED                OBSERVED_WITH_CODE               0           UNSPECIFIED
//! STALE_COMPLETED          OBSERVED_WITH_CODE               0           UNSPECIFIED
//! OUTPUT_FINALIZATION_     OBSERVED_WITH_CODE               0           != UNSPECIFIED
//!   FAILED
//! FAILED                   아무 관측                         관측 규칙대로 아무 값
//! INTERRUPTED · CANCELLED  아무 관측(코드를 강제하지 않는다) 관측 규칙대로 아무 값
//! ```
//!
//! ★ 결함 72(재검수 55) — FAILED + OBSERVED_WITH_CODE + 0 을 전에는 거부했다("정상 종료인데 확정만 실패했으면 outcome 6").
//!   그러나 watchdog 판정(no-progress) -> terminate(0) -> 코드 0 관측은 규범 · 어댑터가 배제하지 않는다 — exit 0 관측만으로
//!   실패 원인을 확정 실패로 한정할 수 없다. 그래서 받는다. 대가: "정상 종료를 FAILED 로 잘못 적는 것" 은 이 세 필드로 막지
//!   못한다 — 실패 원인 필드가 생기기 전까지 열린 항목이다.
//! ★ OBSERVED_NO_CODE 는 신호 종료와 코드 조회 실패를 합친다 — 이 값만으로 재시도 · 실패 책임을 정하지 않는다.
//!
//! ★ `FINALIZATION_FAILURE_STAGE_UNSPECIFIED` 는 "이 보고가 확정 실패를 적지 않았다" 이다 — v1 에서는 정보가 없다는 뜻이지
//!   실패가 없었다는 증거가 아니다.
//! ★ 모르는 enum 값은 거부한다(fail closed) — 옛 수신자가 새 값을 모른 채 통과시키지 않게.

use crate::constants::ATTEMPT_REPORT_MAX_SCHEMA_VERSION;
use crate::pb;

/// 조합 규칙 위반.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportRuleError {
    UnknownOutcome(i32),
    UnspecifiedOutcome,
    UnknownExitObservation(i32),
    UnknownFinalizationStage(i32),
    UnsupportedSchemaVersion(u32),
    /// v1 보고가 v2 에서 생긴 필드(14 · 15 · 16)를 썼다 — 옛 검증자가 canonical 을 재구성하지 못한다(signing.md §7).
    V1UsesNewFields,
    /// v1 보고가 v2 에서 생긴 outcome(6)을 썼다.
    V1UsesNewOutcome,
    /// v2 보고가 종료 관측을 적지 않았다.
    V2MissingExitObservation,
    /// 종료 코드를 관측하지 않았는데 0 이 아닌 종료 코드가 있다.
    ExitCodeWithoutObservedCode,
    /// outcome 과 종료 관측 · 종료 코드 · 확정 실패 단계의 조합이 허용 표 밖이다.
    CombinationRejected {
        outcome: i32,
        exit_observation: i32,
        exit_code: u32,
        stage: i32,
    },
}

impl std::fmt::Display for ReportRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownOutcome(v) => write!(f, "ATTEMPT_REPORT_RULE: 모르는 outcome {v}"),
            Self::UnspecifiedOutcome => write!(f, "ATTEMPT_REPORT_RULE: outcome 이 UNSPECIFIED 다"),
            Self::UnknownExitObservation(v) => write!(f, "ATTEMPT_REPORT_RULE: 모르는 exit_observation {v}"),
            Self::UnknownFinalizationStage(v) => {
                write!(f, "ATTEMPT_REPORT_RULE: 모르는 finalization_failure_stage {v}")
            }
            Self::UnsupportedSchemaVersion(v) => {
                write!(f, "ATTEMPT_REPORT_RULE: 지원하지 않는 schema_version {v}")
            }
            Self::V1UsesNewFields => write!(f, "ATTEMPT_REPORT_RULE: schema_version 1 보고가 v2 필드를 썼다"),
            Self::V1UsesNewOutcome => {
                write!(f, "ATTEMPT_REPORT_RULE: schema_version 1 보고가 OUTPUT_FINALIZATION_FAILED 를 썼다")
            }
            Self::V2MissingExitObservation => {
                write!(f, "ATTEMPT_REPORT_RULE: schema_version 2 보고에 exit_observation 이 없다")
            }
            Self::ExitCodeWithoutObservedCode => write!(
                f,
                "ATTEMPT_REPORT_RULE: 종료 코드를 관측하지 않았는데 0 이 아닌 exit_code 가 있다"
            ),
            Self::CombinationRejected {
                outcome,
                exit_observation,
                exit_code,
                stage,
            } => write!(
                f,
                "ATTEMPT_REPORT_RULE: 허용하지 않는 조합 outcome={outcome} exit_observation={exit_observation} \
                 exit_code={exit_code} stage={stage}"
            ),
        }
    }
}

impl std::error::Error for ReportRuleError {}

/// 보고 **전체**의 조합 규칙을 검사한다. 서명 검증과 별개다 — 둘 다 통과해야 한다.
pub fn validate_attempt_report_semantics(r: &pb::AttemptReport) -> Result<(), ReportRuleError> {
    use pb::AttemptOutcome as O;
    use pb::ExitObservation as E;
    use pb::FinalizationFailureStage as S;

    let outcome = O::try_from(r.outcome).map_err(|_| ReportRuleError::UnknownOutcome(r.outcome))?;
    let obs = E::try_from(r.exit_observation)
        .map_err(|_| ReportRuleError::UnknownExitObservation(r.exit_observation))?;
    let stage = S::try_from(r.finalization_failure_stage)
        .map_err(|_| ReportRuleError::UnknownFinalizationStage(r.finalization_failure_stage))?;
    if outcome == O::Unspecified {
        return Err(ReportRuleError::UnspecifiedOutcome);
    }

    match r.schema_version {
        1 => {
            if obs != E::Unspecified || r.exit_code != 0 || stage != S::Unspecified {
                return Err(ReportRuleError::V1UsesNewFields);
            }
            // ★ 결함 71 — 허용 집합을 명시한다. UNSPECIFIED 는 위에서 이미 걸렀지만, 앞선 검사를 옮기거나 지워도
            //   v1 이 UNSPECIFIED 를 받지 않게 여기서도 닫는다.
            match outcome {
                O::Completed | O::Failed | O::Interrupted | O::Cancelled | O::StaleCompleted => Ok(()),
                O::OutputFinalizationFailed => Err(ReportRuleError::V1UsesNewOutcome),
                O::Unspecified => Err(ReportRuleError::UnspecifiedOutcome),
            }
        }
        v if v == ATTEMPT_REPORT_MAX_SCHEMA_VERSION => {
            if obs == E::Unspecified {
                return Err(ReportRuleError::V2MissingExitObservation);
            }
            if r.exit_code != 0 && obs != E::ObservedWithCode {
                return Err(ReportRuleError::ExitCodeWithoutObservedCode);
            }
            let allowed = match outcome {
                O::Completed | O::StaleCompleted => {
                    obs == E::ObservedWithCode && r.exit_code == 0 && stage == S::Unspecified
                }
                O::OutputFinalizationFailed => {
                    obs == E::ObservedWithCode && r.exit_code == 0 && stage != S::Unspecified
                }
                // ★ 결함 72 — FAILED 는 exit 0 관측도 받는다(watchdog -> terminate(0)). 관측 · 코드의 공통 규칙은 위에서 봤다.
                O::Failed => true,
                O::Interrupted | O::Cancelled => true,
                O::Unspecified => false,
            };
            if allowed {
                Ok(())
            } else {
                Err(ReportRuleError::CombinationRejected {
                    outcome: r.outcome,
                    exit_observation: r.exit_observation,
                    exit_code: r.exit_code,
                    stage: r.finalization_failure_stage,
                })
            }
        }
        v => Err(ReportRuleError::UnsupportedSchemaVersion(v)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pb::AttemptOutcome as O;
    use pb::ExitObservation as E;
    use pb::FinalizationFailureStage as S;

    fn report(v: u32, outcome: O, obs: E, code: u32, stage: S) -> pb::AttemptReport {
        pb::AttemptReport {
            schema_version: v,
            outcome: outcome as i32,
            exit_observation: obs as i32,
            exit_code: code,
            finalization_failure_stage: stage as i32,
            ..Default::default()
        }
    }

    fn ok(v: u32, o: O, e: E, c: u32, s: S) {
        assert_eq!(validate_attempt_report_semantics(&report(v, o, e, c, s)), Ok(()), "{v} {o:?} {e:?} {c} {s:?}");
    }

    fn rejected(v: u32, o: O, e: E, c: u32, s: S) {
        assert!(
            validate_attempt_report_semantics(&report(v, o, e, c, s)).is_err(),
            "받으면 안 되는 조합을 받았다: {v} {o:?} {e:?} {c} {s:?}"
        );
    }

    #[test]
    fn v1_reports_keep_working_and_cannot_use_new_fields() {
        for o in [O::Completed, O::Failed, O::Interrupted, O::Cancelled, O::StaleCompleted] {
            ok(1, o, E::Unspecified, 0, S::Unspecified);
        }
        rejected(1, O::Failed, E::ObservedWithCode, 7, S::Unspecified);
        rejected(1, O::Failed, E::Unspecified, 0, S::ReadOutputs);
        rejected(1, O::Failed, E::Unspecified, 7, S::Unspecified);
        rejected(1, O::OutputFinalizationFailed, E::Unspecified, 0, S::Unspecified);
    }

    #[test]
    fn v1_unspecified_outcome_is_refused_with_its_own_error() {
        // 결함 71 — 새 필드 기본값 · outcome 6 금지를 모두 만족해도 UNSPECIFIED 는 받지 않는다.
        assert_eq!(
            validate_attempt_report_semantics(&report(1, O::Unspecified, E::Unspecified, 0, S::Unspecified)),
            Err(ReportRuleError::UnspecifiedOutcome)
        );
        assert_eq!(
            validate_attempt_report_semantics(&report(1, O::OutputFinalizationFailed, E::Unspecified, 0, S::Unspecified)),
            Err(ReportRuleError::V1UsesNewOutcome)
        );
    }

    #[test]
    fn v2_completed_and_stale_completed_need_an_observed_zero_exit_and_no_failure() {
        for o in [O::Completed, O::StaleCompleted] {
            ok(2, o, E::ObservedWithCode, 0, S::Unspecified);
            rejected(2, o, E::NotObserved, 0, S::Unspecified);
            rejected(2, o, E::ObservedNoCode, 0, S::Unspecified);
            rejected(2, o, E::ObservedWithCode, 7, S::Unspecified);
            rejected(2, o, E::ObservedWithCode, 0, S::CommitCheckpoint);
        }
    }

    #[test]
    fn v2_output_finalization_failed_needs_an_observed_zero_exit_and_a_stage() {
        for s in [S::ReadOutputs, S::EncodeResult, S::CommitCheckpoint] {
            ok(2, O::OutputFinalizationFailed, E::ObservedWithCode, 0, s);
        }
        rejected(2, O::OutputFinalizationFailed, E::ObservedWithCode, 0, S::Unspecified);
        rejected(2, O::OutputFinalizationFailed, E::ObservedWithCode, 7, S::ReadOutputs);
        rejected(2, O::OutputFinalizationFailed, E::NotObserved, 0, S::ReadOutputs);
    }

    #[test]
    fn v2_failed_keeps_a_coexisting_finalization_failure() {
        // 재검수 54 의 반례 — exit 7 뒤 출력 읽기도 실패한 경우를 표현할 수 있어야 한다.
        ok(2, O::Failed, E::ObservedWithCode, 7, S::ReadOutputs);
        ok(2, O::Failed, E::ObservedWithCode, 7, S::Unspecified);
        ok(2, O::Failed, E::ObservedNoCode, 0, S::Unspecified);
        ok(2, O::Failed, E::NotObserved, 0, S::CommitCheckpoint);
        // 결함 72 — watchdog 판정 뒤 terminate(0) 으로 끝나 코드 0 을 관측한 실패. COMPLETED 도 outcome 6 도 아니다.
        ok(2, O::Failed, E::ObservedWithCode, 0, S::Unspecified);
        ok(2, O::Failed, E::ObservedWithCode, 0, S::ReadOutputs);
        // Windows 에서 실제로 관측한 u32::MAX 는 숫자 그대로 보존한다(Linux 합성값과 숫자로 가르지 않는다 — 결함 69).
        ok(2, O::Failed, E::ObservedWithCode, u32::MAX, S::Unspecified);
        // 코드 없는 관측에 코드를 붙이면 공통 규칙이 거부한다.
        assert_eq!(
            validate_attempt_report_semantics(&report(2, O::Failed, E::ObservedNoCode, 7, S::Unspecified)),
            Err(ReportRuleError::ExitCodeWithoutObservedCode)
        );
    }

    #[test]
    fn v2_interrupted_and_cancelled_do_not_force_an_exit_code() {
        for o in [O::Interrupted, O::Cancelled] {
            ok(2, o, E::NotObserved, 0, S::Unspecified);
            ok(2, o, E::ObservedWithCode, 0, S::Unspecified);
            ok(2, o, E::ObservedWithCode, 137, S::CommitCheckpoint);
            ok(2, o, E::ObservedNoCode, 0, S::ReadOutputs);
        }
    }

    #[test]
    fn v2_common_rules() {
        rejected(2, O::Failed, E::Unspecified, 0, S::Unspecified);
        rejected(2, O::Interrupted, E::NotObserved, 7, S::Unspecified);
        rejected(2, O::Unspecified, E::ObservedWithCode, 0, S::Unspecified);
        rejected(3, O::Completed, E::ObservedWithCode, 0, S::Unspecified);
        rejected(0, O::Completed, E::Unspecified, 0, S::Unspecified);
    }

    #[test]
    fn unknown_enum_values_are_refused() {
        let mut r = report(2, O::Failed, E::ObservedWithCode, 7, S::Unspecified);
        r.outcome = 99;
        assert_eq!(validate_attempt_report_semantics(&r), Err(ReportRuleError::UnknownOutcome(99)));
        let mut r = report(2, O::Failed, E::ObservedWithCode, 7, S::Unspecified);
        r.exit_observation = 99;
        assert_eq!(
            validate_attempt_report_semantics(&r),
            Err(ReportRuleError::UnknownExitObservation(99))
        );
        let mut r = report(2, O::Failed, E::ObservedWithCode, 7, S::Unspecified);
        r.finalization_failure_stage = 99;
        assert_eq!(
            validate_attempt_report_semantics(&r),
            Err(ReportRuleError::UnknownFinalizationStage(99))
        );
    }
}
