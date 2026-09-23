//! 워크로드가 끝났다는 **관측**을 서명된 [`pb::AttemptReport`] 로 만든다.
//!
//! # 왜 이 모듈이 따로 있나 (2026-09-06)
//!
//! `exec.rs` 는 자식 프로세스를 띄우고 **종료 코드까지 관측**하는데,
//! 그 사실이 `stdout` 한 줄로 찍히고 끝났다 — `crates/agent/` 전체에
//! `AttemptReport` 라는 이름이 한 번도 나오지 않았다. 받을 쪽
//! (`FrameType::AttemptReport = 6`, `pb::AttemptReport`,
//! `CoordinatorAttemptReportStore::store_verified_terminal_report()`)은
//! 이미 다 있는데 **보내는 쪽이 없었다.**
//!
//! # ★ 이 모듈이 하지 않는 것 — 없는 값을 만들지 않는다
//!
//! `CLAUDE.md` §1 "지어내지 않는다". 아래 필드는 **관측한 적이 없으므로
//! 비운다.** 비운 이유를 여기 적어 두지 않으면, 다음 사람이 "0 이니까
//! 그런 값인가 보다" 라고 읽는다.
//!
//! ```text
//! final_step        이 Agent 에는 step 계수기가 없다. 실행 중 체크포인트가
//!                   없으므로 "몇 번째 step 에서 끝났다" 를 관측할 수단이
//!                   전혀 없다 -> 0 (미상)
//! artifacts         `ArtifactRef` 는 그 자체가 서명 대상 메시지다
//!                   (`artifact.proto` 필드 90). 서명하지 않은 것을 넣으면
//!                   받는 쪽이 독립 검증할 수 없다 -> 비움
//! final_checkpoint  같은 이유. 체크포인트는 로컬에 확정되지만
//!                   `gputeer_checkpoint::CheckpointManifest`(도메인 타입)
//!                   이지 서명된 `pb::CheckpointManifest` 가 아니다 -> 비움
//! metrics           Job 이 스스로 산출한 값이다. 이 Agent 는 자식의
//!                   stdout 을 해석하지 않는다 -> 비움
//! ```
//!
//! ★ **새 proto 필드를 만들지 않았다.** 위 넷은 전부 이미 있는 필드이며,
//!   채울 근거가 없어서 비운 것이다.
//!
//! # ★ 시각은 관측값이다 — 상대값을 계산하지 않는다
//!
//! [`TerminalObservation::started_at_unix_ms`] 와
//! [`TerminalObservation::finished_at_unix_ms`] 는 각각 자식을 띄우기
//! 직전·거둔 직후에 **시계를 읽은 값**이다. `started + 걸린시간` 같은
//! 계산으로 만들지 않는다. 그래서 시계가 뒤로 갔으면 그 사실이 그대로
//! 드러나고, 이 모듈은 그것을 **오류로 거부한다**(보정하지 않는다).
//!
//! # outcome 으로 무엇을 쓸 수 있나 (알려진 한계)
//!
//! ```text
//! COMPLETED         종료 코드 0 을 관측
//! FAILED            0 이 아닌 코드를 관측, 또는 종료는 관측했지만 코드가 없다(OBSERVED_NO_CODE —
//!                   신호 종료 · 코드 조회 실패. B+E schema_version 2, 결함 69)
//! INTERRUPTED       ★ 만들 수 없다
//! CANCELLED         ★ 만들 수 없다
//! STALE_COMPLETED   ★ 만들 수 없다
//! ```
//!
//! `exec::ExecutionOutcome` 에는 "소유자가 멈췄는가" · "네트워크가
//! 끊겼는가" 가 **없다.** 소유자가 Owner Panel 에서 정지시켜도 이 계층이
//! 보는 것은 0 이 아닌 종료 코드뿐이다. 그래서 그 경우 `FAILED` 로
//! 보고되며, 그것은 **거짓이 아니라 덜 정확한 것**이다 — 없는 구분을
//! 있는 척하지 않기 위해 추측으로 `CANCELLED` 를 쓰지 않는다.

use gputeer_crypto::{sign, write_frame, FrameType, SigningKey};
use gputeer_protocol::pb;
use prost::Message;

/// 이 Agent 가 만드는 `AttemptReport` 의 스키마 버전.
///
/// ★ 상수를 여기 두는 이유는 이것이 **이 발신자가 무엇을 쓰는가**이지
///   프로토콜 상수가 아니기 때문이다. 프레임 상한 같은 협상 대상 값은
///   `crates/protocol/src/constants.rs` 가 소유한다(`RULE.md` §3.1).
///
/// ★ 1 -> 2 (2026-09-14, B+E) — 종료 관측의 존재 여부(필드 14 · 15)를 싣는다. 받는 쪽은 v2 까지 읽는다
///   (`gputeer_protocol::constants::ATTEMPT_REPORT_MAX_SCHEMA_VERSION`). 옛 Coordinator 는 v2 를 거부한다 —
///   계획서 조건 (a).
pub const ATTEMPT_REPORT_SCHEMA_VERSION: u32 = 2;

/// 워크로드가 끝났다는 **관측**. 전부 실제로 본 값이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalObservation {
    /// 보유 Lease 가 말하는 Job.
    pub job_id: String,
    /// 이 연결의 Grant 가 말하는 Attempt.
    pub attempt_id: String,
    /// 이 Agent 자신. `AttemptReport::signer_id()` 가 이 필드다 —
    /// 다른 값을 넣으면 받는 쪽이 그 이름의 키로 검증하려다 실패한다.
    pub node_id: String,
    /// 보유 Lease 의 세대.
    pub fence_epoch: u64,
    /// `exec` 가 관측한 종료 코드. `None` 은 "종료는 관측했지만 코드가 없다" 이다(결함 69) — 미관측이 아니다.
    pub exit_code: Option<u32>,
    /// 종료를 관측한 **뒤** 산출물 확정이 실패한 단계(결함 ⑲). `None` 은 "이 보고가 확정 실패를 적지 않았다" 이다.
    ///
    /// ★ 전에는 이 실패가 Agent 오류로 끝나 종료 보고 자체가 사라졌다 — 종료를 관측한 사실까지 잃었다.
    pub finalization_failure: Option<pb::FinalizationFailureStage>,
    /// 자식을 띄우기 **직전에** 읽은 시계.
    pub started_at_unix_ms: u64,
    /// 자식을 거둔 **직후에** 읽은 시계.
    pub finished_at_unix_ms: u64,
    /// 이 보고를 만드는 시점에 읽은 시계.
    pub issued_at_unix_ms: u64,
    /// ★ 2026-09-23 (신뢰망 남은 일 H) — 이 노드의 **소유자가** Owner Panel 로 멈췄다(관측, `OwnerPanelState`).
    ///   그러면 outcome 은 FAILED 가 아니라 INTERRUPTED 다 — 작업이 잘못된 게 아니라 GPU 를 되찾긴 것이다.
    pub stopped_by_owner: bool,
}

/// 보고를 만들 수 없는 이유. **전부 "안 보낸다" 다** — 부분 보고가 없다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptReportError {
    /// 신원 필드가 비었다. 빈 이름으로 보고하면 받는 쪽이 어느 Attempt 의
    /// 것인지 결합할 수 없다.
    MissingField(&'static str),
    /// 끝난 시각이 시작보다 앞선다 — 시계가 뒤로 갔다.
    ///
    /// ★ 보정하지 않는다. 두 값 모두 관측값이므로, 어긋났다는 것 자체가
    ///   사실이고 그 사실을 감추면 시각을 근거로 한 뒤 판단이 조용히
    ///   틀린다.
    FinishedBeforeStarted {
        started_at_unix_ms: u64,
        finished_at_unix_ms: u64,
    },
    /// 발행 시각이 종료 시각보다 앞선다 — 같은 이유로 거부한다.
    IssuedBeforeFinished {
        finished_at_unix_ms: u64,
        issued_at_unix_ms: u64,
    },
    /// 프레임 인코딩 실패.
    Frame(String),
    /// 만든 보고가 필드 조합 규칙을 어긴다 — 보내는 쪽 자기 검사(B+E 계획서 §5.7 (4)). 서명하지 않는다.
    Rule(gputeer_protocol::attempt_report_rules::ReportRuleError),
}

impl std::fmt::Display for AttemptReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(field) => write!(
                f,
                "ATTEMPT_REPORT_REFUSED: {field} 가 비어 있다 — \
                 신원 없는 종료 보고를 만들지 않는다"
            ),
            Self::FinishedBeforeStarted {
                started_at_unix_ms,
                finished_at_unix_ms,
            } => write!(
                f,
                "ATTEMPT_REPORT_REFUSED: 종료 시각({finished_at_unix_ms})이 \
                 시작 시각({started_at_unix_ms})보다 앞선다 — \
                 관측값을 보정하지 않는다"
            ),
            Self::IssuedBeforeFinished {
                finished_at_unix_ms,
                issued_at_unix_ms,
            } => write!(
                f,
                "ATTEMPT_REPORT_REFUSED: 발행 시각({issued_at_unix_ms})이 \
                 종료 시각({finished_at_unix_ms})보다 앞선다 — \
                 관측값을 보정하지 않는다"
            ),
            Self::Frame(detail) => {
                write!(f, "ATTEMPT_REPORT_REFUSED: 프레임 인코딩 실패: {detail}")
            }
            Self::Rule(rule) => write!(f, "ATTEMPT_REPORT_REFUSED: {rule}"),
        }
    }
}

impl std::error::Error for AttemptReportError {}

/// 종료 코드를 outcome 으로 옮긴다.
///
/// `state-machines.md` §3 이 `WORKLOAD_EXITED_OK` 와
/// `WORKLOAD_EXITED_ERROR` 를 다른 전이로 두는 것과 같은 구분이다.
/// 모듈 문서의 "알려진 한계" 를 함께 읽는다 — 나머지 세 outcome 은
/// 이 계층이 관측할 수 없어 **만들지 않는다.**
pub fn outcome_for_exit_code(exit_code: Option<u32>) -> pb::AttemptOutcome {
    outcome_for(exit_code, None)
}

/// 종료 관측과 산출물 확정 결과를 outcome 으로 옮긴다(state-machines.md §3 · 계획서 §5.7 (2)).
///
/// ```text
/// 코드 0 · 확정 성공        COMPLETED
/// 코드 0 · 확정 실패        OUTPUT_FINALIZATION_FAILED   RUNNING -> FAILED (OUTPUT_FINALIZATION_FAILED)
/// 그 밖(코드 != 0 · 코드 없음) FAILED — 확정 실패가 겹쳤으면 단계도 적는다(공존하는 실패, 결함 66)
/// ```
pub fn outcome_for(
    exit_code: Option<u32>,
    finalization_failure: Option<pb::FinalizationFailureStage>,
) -> pb::AttemptOutcome {
    match (exit_code, finalization_failure) {
        (Some(0), None) => pb::AttemptOutcome::Completed,
        (Some(0), Some(_)) => pb::AttemptOutcome::OutputFinalizationFailed,
        // 코드가 없는 종료는 성공으로 세지 않는다 — 0 이라는 관측이 없다.
        _ => pb::AttemptOutcome::Failed,
    }
}

/// 관측을 검사하고 **서명된** 보고를 만든다.
///
/// ★ 서명은 마지막이다. 필드를 하나라도 서명 뒤에 고치면 그 보고는
///   받는 쪽에서 검증에 실패한다.
pub fn build_signed_attempt_report(
    key: &SigningKey,
    observation: &TerminalObservation,
) -> Result<pb::AttemptReport, AttemptReportError> {
    if observation.job_id.trim().is_empty() {
        return Err(AttemptReportError::MissingField("job_id"));
    }
    if observation.attempt_id.trim().is_empty() {
        return Err(AttemptReportError::MissingField("attempt_id"));
    }
    if observation.node_id.trim().is_empty() {
        return Err(AttemptReportError::MissingField("node_id"));
    }
    if observation.finished_at_unix_ms < observation.started_at_unix_ms {
        return Err(AttemptReportError::FinishedBeforeStarted {
            started_at_unix_ms: observation.started_at_unix_ms,
            finished_at_unix_ms: observation.finished_at_unix_ms,
        });
    }
    if observation.issued_at_unix_ms < observation.finished_at_unix_ms {
        return Err(AttemptReportError::IssuedBeforeFinished {
            finished_at_unix_ms: observation.finished_at_unix_ms,
            issued_at_unix_ms: observation.issued_at_unix_ms,
        });
    }

    let mut report = pb::AttemptReport {
        schema_version: ATTEMPT_REPORT_SCHEMA_VERSION,
        job_id: observation.job_id.clone(),
        attempt_id: observation.attempt_id.clone(),
        node_id: observation.node_id.clone(),
        fence_epoch: observation.fence_epoch,
        outcome: if observation.stopped_by_owner {
            pb::AttemptOutcome::Interrupted as i32
        } else {
            outcome_for(observation.exit_code, observation.finalization_failure) as i32
        },
        // v2 — 종료 관측의 **존재 여부**를 그대로 옮긴다(B+E 계획서 §5.7 (3)). 코드가 없으면 NO_CODE 로 적고
        //   exit_code 는 기본값이다 — 숫자로 "없음" 을 나타내지 않는다.
        exit_observation: match observation.exit_code {
            Some(_) => pb::ExitObservation::ObservedWithCode,
            None => pb::ExitObservation::ObservedNoCode,
        } as i32,
        exit_code: observation.exit_code.unwrap_or(0),
        finalization_failure_stage: observation
            .finalization_failure
            .map_or(0, |stage| stage as i32),
        // ★ 아래 넷은 **비운다.** 이유는 모듈 문서에 있다.
        final_step: 0,
        started_at_unix_ms: observation.started_at_unix_ms,
        finished_at_unix_ms: observation.finished_at_unix_ms,
        artifacts: Vec::new(),
        final_checkpoint: None,
        metrics: Vec::new(),
        issued_at_unix_ms: observation.issued_at_unix_ms,
        ..Default::default()
    };
    // 보내는 쪽 자기 검사 — 받는 쪽과 같은 함수다. 어기면 서명하지 않는다.
    gputeer_protocol::attempt_report_rules::validate_attempt_report_semantics(&report)
        .map_err(AttemptReportError::Rule)?;
    report.node_signature = sign(key, &report).to_vec();
    Ok(report)
}

/// 보고를 **이미 있는** 프레임 타입으로 감싼다.
///
/// ★ 새 프레임 타입을 만들지 않는다 — `FrameType::AttemptReport = 6` 은
///   `crates/crypto/src/framed_ingress.rs:90` 에 이미 있었고, 받는 쪽
///   `read_frame` 이 그 태그를 `decode_and_verify::<pb::AttemptReport>()`
///   로 보낸다.
pub fn attempt_report_frame(report: &pb::AttemptReport) -> Result<Vec<u8>, AttemptReportError> {
    write_frame(FrameType::AttemptReport, &report.encode_to_vec())
        .map_err(|error| AttemptReportError::Frame(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gputeer_crypto::{Ed25519Verifier, InMemoryKeyring};
    use gputeer_protocol::signing::{verify, NoReplayCheck};

    const JOB: &str = "job-1";
    const ATTEMPT: &str = "attempt-1";
    const NODE: &str = "node-1";

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[9u8; 32])
    }

    fn observation() -> TerminalObservation {
        TerminalObservation {
            stopped_by_owner: false,
            job_id: JOB.into(),
            attempt_id: ATTEMPT.into(),
            node_id: NODE.into(),
            fence_epoch: 7,
            exit_code: Some(0),
            finalization_failure: None,
            started_at_unix_ms: 1_000,
            finished_at_unix_ms: 1_500,
            issued_at_unix_ms: 1_600,
        }
    }

    #[test]
    fn a_built_report_verifies_under_the_signing_node_identity() {
        let key = key();
        let report = build_signed_attempt_report(&key, &observation()).expect("정상 관측");
        let mut keys = InMemoryKeyring::new();
        keys.insert(NODE, key.verifying_key());
        let verified = verify(
            &report,
            ATTEMPT_REPORT_SCHEMA_VERSION,
            &Ed25519Verifier::new(keys),
            9_999,
            &mut NoReplayCheck,
        )
        .expect("Agent 가 만든 보고는 검증을 통과해야 한다");
        assert_eq!(verified.signer_id(), NODE);
    }

    /// ★ 관측하지 않은 필드가 실제로 비어 있는지 **값으로** 고정한다.
    ///   주석에만 적어 두면 나중에 누가 채워도 아무도 모른다.
    #[test]
    fn unobserved_fields_stay_empty() {
        let report = build_signed_attempt_report(&key(), &observation()).expect("정상 관측");
        assert_eq!(report.final_step, 0, "step 계수기가 없다 — 0 은 미상이다");
        assert!(report.artifacts.is_empty(), "서명된 ArtifactRef 가 없다");
        assert!(
            report.final_checkpoint.is_none(),
            "서명된 pb::CheckpointManifest 가 없다"
        );
        assert!(report.metrics.is_empty(), "Job 자기보고 지표를 읽지 않는다");
    }

    #[test]
    fn exit_code_zero_is_completed_and_anything_else_is_failed() {
        assert_eq!(
            outcome_for_exit_code(Some(0)),
            pb::AttemptOutcome::Completed
        );
        assert_eq!(outcome_for_exit_code(Some(1)), pb::AttemptOutcome::Failed);
        assert_eq!(
            outcome_for_exit_code(Some(u32::MAX)),
            pb::AttemptOutcome::Failed,
            "0 이 아닌 모든 값은 실패다"
        );
        assert_eq!(
            outcome_for_exit_code(None),
            pb::AttemptOutcome::Failed,
            "코드가 없는 종료는 성공이 아니다"
        );
    }

    /// 결함 69 — 종료는 관측했지만 코드가 없으면 v2 FAILED + OBSERVED_NO_CODE + 코드 기본값으로 보고한다.
    ///   전에는 이 경우 보고 자체를 만들지 않았다(Windows) 또는 -1 을 u32 로 옮겨 코드처럼 보고했다(Linux).
    #[test]
    fn an_observed_exit_without_a_code_is_a_failed_v2_report_with_no_code() {
        let mut observed = observation();
        observed.exit_code = None;
        let report =
            build_signed_attempt_report(&key(), &observed).expect("코드 없는 종료도 보고한다");
        assert_eq!(report.schema_version, 2);
        assert_eq!(report.outcome, pb::AttemptOutcome::Failed as i32);
        assert_eq!(
            report.exit_observation,
            pb::ExitObservation::ObservedNoCode as i32
        );
        assert_eq!(report.exit_code, 0);
        assert_eq!(report.finalization_failure_stage, 0);
    }

    /// 결함 ⑲ — 종료를 관측한 뒤 확정이 실패해도 보고를 만든다. 코드 0 이면 outcome 6, 아니면 FAILED + 단계.
    ///   만든 보고는 받는 쪽과 같은 조합 규칙을 통과한다(서명 전 자기 검사).
    #[test]
    fn a_finalization_failure_after_an_observed_exit_is_still_reported() {
        use pb::FinalizationFailureStage as S;
        for (code, stage, outcome, expected_observation) in [
            (
                Some(0),
                S::ReadOutputs,
                pb::AttemptOutcome::OutputFinalizationFailed,
                pb::ExitObservation::ObservedWithCode,
            ),
            (
                Some(0),
                S::CommitCheckpoint,
                pb::AttemptOutcome::OutputFinalizationFailed,
                pb::ExitObservation::ObservedWithCode,
            ),
            (
                Some(7),
                S::EncodeResult,
                pb::AttemptOutcome::Failed,
                pb::ExitObservation::ObservedWithCode,
            ),
            (
                None,
                S::ReadOutputs,
                pb::AttemptOutcome::Failed,
                pb::ExitObservation::ObservedNoCode,
            ),
        ] {
            let mut observed = observation();
            observed.exit_code = code;
            observed.finalization_failure = Some(stage);
            let report = build_signed_attempt_report(&key(), &observed)
                .unwrap_or_else(|e| panic!("{code:?} {stage:?}: 확정 실패도 보고해야 한다: {e}"));
            assert_eq!(report.outcome, outcome as i32, "{code:?} {stage:?}");
            assert_eq!(
                report.exit_observation, expected_observation as i32,
                "{code:?} {stage:?}"
            );
            assert_eq!(
                report.finalization_failure_stage, stage as i32,
                "{code:?} {stage:?}"
            );
        }
        assert_eq!(outcome_for(Some(0), None), pb::AttemptOutcome::Completed);
    }

    /// 관측한 코드는 숫자 그대로 옮긴다 — Windows 에서 실제로 볼 수 있는 u32::MAX 포함(결함 69).
    #[test]
    fn an_observed_code_is_carried_as_is_including_u32_max() {
        for code in [0, 7, u32::MAX] {
            let mut observed = observation();
            observed.exit_code = Some(code);
            let report = build_signed_attempt_report(&key(), &observed).expect("관측한 코드");
            assert_eq!(report.schema_version, 2);
            assert_eq!(
                report.exit_observation,
                pb::ExitObservation::ObservedWithCode as i32
            );
            assert_eq!(report.exit_code, code);
        }
    }

    #[test]
    fn observed_timestamps_are_copied_not_recomputed() {
        let mut observed = observation();
        observed.started_at_unix_ms = 1_234_567;
        observed.finished_at_unix_ms = 1_234_567; // 같은 밀리초에 끝날 수 있다
        observed.issued_at_unix_ms = 1_234_567;
        let report = build_signed_attempt_report(&key(), &observed).expect("같은 밀리초는 정상");
        assert_eq!(report.started_at_unix_ms, 1_234_567);
        assert_eq!(report.finished_at_unix_ms, 1_234_567);
        assert_eq!(report.issued_at_unix_ms, 1_234_567);
    }

    // ── negative ────────────────────────────────────────────────────

    #[test]
    fn an_empty_identity_field_is_refused_by_name() {
        for (field, mutate) in [
            (
                "job_id",
                (|o: &mut TerminalObservation| o.job_id.clear()) as fn(&mut _),
            ),
            ("attempt_id", |o: &mut TerminalObservation| {
                o.attempt_id = "   ".into()
            }),
            ("node_id", |o: &mut TerminalObservation| o.node_id.clear()),
        ] {
            let mut observed = observation();
            mutate(&mut observed);
            let error = build_signed_attempt_report(&key(), &observed)
                .expect_err("빈 신원으로 보고를 만들면 안 된다");
            assert_eq!(
                error,
                AttemptReportError::MissingField(field),
                "거부 사유가 어느 필드인지 말해야 한다"
            );
            assert!(
                error.to_string().contains(field),
                "메시지에 필드 이름이 없다: {error}"
            );
        }
    }

    #[test]
    fn a_backwards_clock_between_start_and_finish_is_refused_with_both_values() {
        let mut observed = observation();
        observed.started_at_unix_ms = 5_000;
        observed.finished_at_unix_ms = 4_999;
        let error = build_signed_attempt_report(&key(), &observed)
            .expect_err("시계가 뒤로 간 관측을 보고하면 안 된다");
        assert_eq!(
            error,
            AttemptReportError::FinishedBeforeStarted {
                started_at_unix_ms: 5_000,
                finished_at_unix_ms: 4_999,
            }
        );
        let message = error.to_string();
        assert!(message.contains("5000"), "시작 시각이 없다: {message}");
        assert!(message.contains("4999"), "종료 시각이 없다: {message}");
    }

    #[test]
    fn an_issue_time_before_the_finish_time_is_refused_with_both_values() {
        let mut observed = observation();
        observed.finished_at_unix_ms = 9_000;
        observed.issued_at_unix_ms = 8_999;
        let error = build_signed_attempt_report(&key(), &observed)
            .expect_err("종료보다 앞선 발행 시각을 보고하면 안 된다");
        assert_eq!(
            error,
            AttemptReportError::IssuedBeforeFinished {
                finished_at_unix_ms: 9_000,
                issued_at_unix_ms: 8_999,
            }
        );
        let message = error.to_string();
        assert!(message.contains("9000"), "종료 시각이 없다: {message}");
        assert!(message.contains("8999"), "발행 시각이 없다: {message}");
    }

    /// ★ 서명이 **내용을 실제로 덮는지** 본다. 한 바이트만 바꿔도
    ///   검증이 깨져야 한다 — 안 깨지면 그 서명은 아무것도 재지 않는다.
    #[test]
    fn a_field_changed_after_signing_no_longer_verifies() {
        let key = key();
        let mut report = build_signed_attempt_report(&key, &observation()).expect("정상 관측");
        report.outcome = pb::AttemptOutcome::Failed as i32;
        let mut keys = InMemoryKeyring::new();
        keys.insert(NODE, key.verifying_key());
        let outcome = verify(
            &report,
            ATTEMPT_REPORT_SCHEMA_VERSION,
            &Ed25519Verifier::new(keys),
            9_999,
            &mut NoReplayCheck,
        );
        assert!(
            outcome.is_err(),
            "서명 뒤에 바꾼 outcome 이 그대로 통과했다"
        );
    }

    /// ★ H — 소유자가 멈춘 시도는 INTERRUPTED 로 보고한다(종료 관측은 그대로 싣는다 — v2 규칙이 허용한다).
    #[test]
    fn an_owner_stop_is_reported_as_interrupted_not_failed() {
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let mut observed = observation();
        observed.exit_code = Some(0xC000_0013);
        observed.stopped_by_owner = true;
        let report = build_signed_attempt_report(&key, &observed).unwrap();
        assert_eq!(report.outcome, pb::AttemptOutcome::Interrupted as i32);
        gputeer_protocol::attempt_report_rules::validate_attempt_report_semantics(&report).unwrap();
        observed.stopped_by_owner = false;
        let report = build_signed_attempt_report(&key, &observed).unwrap();
        assert_eq!(report.outcome, pb::AttemptOutcome::Failed as i32);
    }
}
