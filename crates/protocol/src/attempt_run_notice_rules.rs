//! 실행 알림(`AttemptRunNotice`) · 그 응답(`AttemptRunNoticeAck`)의 조합 규칙 — 서명 검증과 별개다. 둘 다 통과해야 쓴다.
//!
//! 계약 `docs/contracts/proposals/2026-09-28_1034_실행여부불명_재배치보류_Lease_Attempt.md` v17 §1:
//!
//! ```text
//! 둘 다         sequence ≥ 1(0 은 거부 — b6 ③) · 모르는 enum 값은 거부(fail closed)
//! RUN_UNKNOWN   origin · reason 이 0 이 아니고 stop_evidence 는 0
//! STOP_CONFIRMED stop_evidence 가 0 이 아니고 origin · reason 은 0 — 1(기계 증거) · 2(소유자 진술 · 계약 v18 §4) 둘 다 받는다
//! 응답          kind 는 RUN_UNKNOWN · STOP_CONFIRMED 중 하나
//! ```
//! ★ `Verified` 는 서명 통과이지 조합 규칙 통과가 아니다 — 받는 쪽(Coordinator 의 알림 저장 진입 · Agent 의 응답 대조)은 이 함수를 **직접 불러야 한다**
//!   (AttemptReport 와 같은 방식). ★ 2026-10-01 이 조각(계약층)에는 부르는 곳이 아직 없다 — 받는 쪽은 다음 조각이고, 그때의 의무다(검수 rn1).

use crate::pb;

/// 조합 규칙 위반.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunNoticeRuleError {
    /// sequence 가 0 이다.
    ZeroSequence,
    /// kind 가 0 이거나 모르는 값이다.
    UnknownKind(i32),
    /// origin 이 모르는 값이다.
    UnknownOrigin(i32),
    /// reason 이 모르는 값이다.
    UnknownReason(i32),
    /// stop_evidence 가 모르는 값이다.
    UnknownStopEvidence(i32),
    /// RUN_UNKNOWN 인데 origin · reason 이 비었거나 stop_evidence 가 찼다.
    RunUnknownShape,
    /// STOP_CONFIRMED 인데 stop_evidence 가 비었거나 origin · reason 이 찼다.
    StopConfirmedShape,
}

impl std::fmt::Display for RunNoticeRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroSequence => write!(f, "sequence 가 0 이다(1 부터)"),
            Self::UnknownKind(v) => write!(f, "알림 종류를 모른다({v})"),
            Self::UnknownOrigin(v) => write!(f, "출발 상태를 모른다({v})"),
            Self::UnknownReason(v) => write!(f, "불명 사유를 모른다({v})"),
            Self::UnknownStopEvidence(v) => write!(f, "정지 증거를 모른다({v})"),
            Self::RunUnknownShape => {
                write!(
                    f,
                    "RUN_UNKNOWN 은 출발 · 사유가 있고 정지 증거가 없어야 한다"
                )
            }
            Self::StopConfirmedShape => {
                write!(
                    f,
                    "STOP_CONFIRMED 는 정지 증거가 있고 출발 · 사유가 없어야 한다"
                )
            }
        }
    }
}

impl std::error::Error for RunNoticeRuleError {}

fn kind_of(value: i32) -> Result<pb::RunNoticeKind, RunNoticeRuleError> {
    match pb::RunNoticeKind::try_from(value) {
        Ok(pb::RunNoticeKind::Unspecified) | Err(_) => Err(RunNoticeRuleError::UnknownKind(value)),
        Ok(kind) => Ok(kind),
    }
}

/// 알림 **전체**의 조합 규칙을 검사한다.
pub fn validate_attempt_run_notice(n: &pb::AttemptRunNotice) -> Result<(), RunNoticeRuleError> {
    if n.sequence == 0 {
        return Err(RunNoticeRuleError::ZeroSequence);
    }
    let kind = kind_of(n.kind)?;
    let origin = pb::RunUnknownOrigin::try_from(n.origin)
        .map_err(|_| RunNoticeRuleError::UnknownOrigin(n.origin))?;
    let reason = pb::RunUnknownReason::try_from(n.reason)
        .map_err(|_| RunNoticeRuleError::UnknownReason(n.reason))?;
    let evidence = pb::RunStopEvidence::try_from(n.stop_evidence)
        .map_err(|_| RunNoticeRuleError::UnknownStopEvidence(n.stop_evidence))?;
    match kind {
        pb::RunNoticeKind::RunUnknown => {
            if origin == pb::RunUnknownOrigin::Unspecified
                || reason == pb::RunUnknownReason::Unspecified
                || evidence != pb::RunStopEvidence::Unspecified
            {
                return Err(RunNoticeRuleError::RunUnknownShape);
            }
        }
        pb::RunNoticeKind::StopConfirmed => {
            if evidence == pb::RunStopEvidence::Unspecified
                || origin != pb::RunUnknownOrigin::Unspecified
                || reason != pb::RunUnknownReason::Unspecified
            {
                return Err(RunNoticeRuleError::StopConfirmedShape);
            }
        }
        pb::RunNoticeKind::Unspecified => unreachable!("kind_of 가 거른다"),
    }
    Ok(())
}

/// 응답의 조합 규칙을 검사한다.
pub fn validate_attempt_run_notice_ack(
    a: &pb::AttemptRunNoticeAck,
) -> Result<(), RunNoticeRuleError> {
    if a.sequence == 0 {
        return Err(RunNoticeRuleError::ZeroSequence);
    }
    kind_of(a.kind).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unknown() -> pb::AttemptRunNotice {
        pb::AttemptRunNotice {
            schema_version: 1,
            kind: pb::RunNoticeKind::RunUnknown as i32,
            origin: pb::RunUnknownOrigin::Running as i32,
            reason: pb::RunUnknownReason::ExitUnobserved as i32,
            sequence: 1,
            ..Default::default()
        }
    }

    fn stop() -> pb::AttemptRunNotice {
        pb::AttemptRunNotice {
            schema_version: 1,
            kind: pb::RunNoticeKind::StopConfirmed as i32,
            stop_evidence: pb::RunStopEvidence::ContainerAbsentConfirmed as i32,
            sequence: 2,
            ..Default::default()
        }
    }

    #[test]
    fn well_formed_notices_pass() {
        assert_eq!(validate_attempt_run_notice(&unknown()), Ok(()));
        assert_eq!(validate_attempt_run_notice(&stop()), Ok(()));
        // ★ 2026-10-03 계약 v18 §4 — 소유자 진술 해제(값 2)도 STOP_CONFIRMED 의 정지 증거다
        assert_eq!(
            validate_attempt_run_notice(&pb::AttemptRunNotice {
                stop_evidence: pb::RunStopEvidence::ContainerAbsentOwnerAttested as i32,
                ..stop()
            }),
            Ok(())
        );
    }

    /// ★ 2026-10-03 계약 v18 — 값 2 를 더해도 그 다음 값(3)은 여전히 모르는 값이고, RUN_UNKNOWN 에 정지 증거(2 포함)를 실으면 모양 위반이다.
    #[test]
    fn the_owner_attested_value_does_not_widen_anything_else() {
        assert_eq!(
            validate_attempt_run_notice(&pb::AttemptRunNotice {
                stop_evidence: 3,
                ..stop()
            }),
            Err(RunNoticeRuleError::UnknownStopEvidence(3))
        );
        assert_eq!(
            validate_attempt_run_notice(&pb::AttemptRunNotice {
                stop_evidence: pb::RunStopEvidence::ContainerAbsentOwnerAttested as i32,
                ..unknown()
            }),
            Err(RunNoticeRuleError::RunUnknownShape)
        );
    }

    #[test]
    fn every_broken_combination_is_refused() {
        let cases: Vec<(pb::AttemptRunNotice, RunNoticeRuleError)> = vec![
            (
                pb::AttemptRunNotice {
                    sequence: 0,
                    ..unknown()
                },
                RunNoticeRuleError::ZeroSequence,
            ),
            (
                pb::AttemptRunNotice {
                    kind: 0,
                    ..unknown()
                },
                RunNoticeRuleError::UnknownKind(0),
            ),
            (
                pb::AttemptRunNotice {
                    kind: 9,
                    ..unknown()
                },
                RunNoticeRuleError::UnknownKind(9),
            ),
            (
                pb::AttemptRunNotice {
                    origin: 9,
                    ..unknown()
                },
                RunNoticeRuleError::UnknownOrigin(9),
            ),
            (
                pb::AttemptRunNotice {
                    reason: 9,
                    ..unknown()
                },
                RunNoticeRuleError::UnknownReason(9),
            ),
            (
                pb::AttemptRunNotice {
                    stop_evidence: 9,
                    ..stop()
                },
                RunNoticeRuleError::UnknownStopEvidence(9),
            ),
            (
                pb::AttemptRunNotice {
                    origin: 0,
                    ..unknown()
                },
                RunNoticeRuleError::RunUnknownShape,
            ),
            (
                pb::AttemptRunNotice {
                    reason: 0,
                    ..unknown()
                },
                RunNoticeRuleError::RunUnknownShape,
            ),
            (
                pb::AttemptRunNotice {
                    stop_evidence: 1,
                    ..unknown()
                },
                RunNoticeRuleError::RunUnknownShape,
            ),
            (
                pb::AttemptRunNotice {
                    stop_evidence: 0,
                    ..stop()
                },
                RunNoticeRuleError::StopConfirmedShape,
            ),
            (
                pb::AttemptRunNotice {
                    origin: 1,
                    ..stop()
                },
                RunNoticeRuleError::StopConfirmedShape,
            ),
            (
                pb::AttemptRunNotice {
                    reason: 1,
                    ..stop()
                },
                RunNoticeRuleError::StopConfirmedShape,
            ),
        ];
        for (notice, expected) in cases {
            assert_eq!(
                validate_attempt_run_notice(&notice),
                Err(expected.clone()),
                "{notice:?}"
            );
        }
    }

    #[test]
    fn the_ack_needs_a_sequence_and_a_known_kind() {
        let ack = pb::AttemptRunNoticeAck {
            schema_version: 1,
            kind: pb::RunNoticeKind::StopConfirmed as i32,
            sequence: 3,
            ..Default::default()
        };
        assert_eq!(validate_attempt_run_notice_ack(&ack), Ok(()));
        assert_eq!(
            validate_attempt_run_notice_ack(&pb::AttemptRunNoticeAck {
                sequence: 0,
                ..ack.clone()
            }),
            Err(RunNoticeRuleError::ZeroSequence)
        );
        assert_eq!(
            validate_attempt_run_notice_ack(&pb::AttemptRunNoticeAck { kind: 0, ..ack }),
            Err(RunNoticeRuleError::UnknownKind(0))
        );
    }
}
