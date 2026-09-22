//! Durable inbox for verified terminal [`pb::AttemptReport`] evidence.
//!
//! This store binds a report to the existing single-node Attempt and, when the
//! node's reservation still belongs to that Attempt, to the reservation too.
//! It deliberately does not transition Job or Attempt state, revoke a Lease,
//! or release the reservation.
//!
//! ★ 결정 D1 (2026-09-14) — Lease 만료 뒤 온 보고도 **과거 실행의 보고로** 저장한다. 예약이 없어졌거나 다른 실행으로 바뀐 늦은 보고는
//!   Attempt 의 배정 기록(job · 유일한 노드 · 서명자 · 세대)으로만 결합하고 그 사실을 `bound_via` 로 남긴다.

use std::path::Path;

use gputeer_protocol::{canonical::blake3_256, pb, signing::Verified};
use prost::Message;
use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

use crate::staging_store::{self, AttemptState, StoredAttempt, StoredNodeReservation};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// 결정 D1 — 보고를 무엇에 결합해 저장했나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportBindingSource {
    /// 저장할 때 그 노드의 **현재 예약**이 이 Attempt 의 것이었다 — 예약까지 대조했다.
    CurrentReservation,
    /// 예약이 없어졌거나 다른 실행으로 바뀐 **늦은 보고** — Attempt 의 배정 기록으로만 결합했다. 과거 실행의 보고로 저장만 한다 —
    /// 이 보고로 예약을 풀거나 결과를 채택하지 않는다.
    AssignmentRecord,
}

impl ReportBindingSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CurrentReservation => "current_reservation",
            Self::AssignmentRecord => "assignment_record",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "current_reservation" => Some(Self::CurrentReservation),
            "assignment_record" => Some(Self::AssignmentRecord),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredAttemptReportBinding {
    pub report: pb::AttemptReport,
    pub report_hash: [u8; 32],
    pub signer_id_at_submission: String,
    pub bound_job_id: String,
    pub bound_attempt_id: String,
    pub bound_node_id: String,
    pub bound_fence_epoch: u64,
    /// 결정 D1 — 현재 예약까지 대조했는가, 배정 기록으로만 결합했는가.
    pub bound_via: ReportBindingSource,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoreAttemptReportResult {
    pub binding: StoredAttemptReportBinding,
    /// `false` means an exact semantic replay returned the first durable row.
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingField {
    JobId,
    AttemptId,
    NodeId,
    SignerId,
    FenceEpoch,
    ReservationJobId,
    ReservationAttemptId,
    ReservationNodeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptReportCorruption {
    EmptyBody,
    UndecodableBody,
    HashEncoding,
    HashMismatch,
    JobIdMismatch,
    AttemptIdMismatch,
    NodeIdMismatch,
    SignerIdMismatch,
    FenceEpochEncoding,
    FenceEpochMismatch,
    InvalidOutcome,
    /// 저장본이 필드 조합 규칙(`attempt_report_rules`)을 어긴다 — 저장 진입에서 막았어야 할 것이 들어 있다.
    ReportRule,
    MissingAttempt,
    /// 결합 경로 칸의 값을 모른다.
    BindingSource,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AttemptReportStoreError {
    InvalidInput(&'static str),
    InvalidOutcome(i32),
    /// 서명은 유효하지만 필드 조합 규칙을 어긴다(B+E 계획서 §5.7 · §5.8, 결함 70 · 71 · 72).
    ReportRule(gputeer_protocol::attempt_report_rules::ReportRuleError),
    AttemptNotFound {
        attempt_id: String,
    },
    ReservationNotFound {
        node_id: String,
    },
    BindingMismatch(BindingField),
    /// 이 보고가 가리키는 종료 상태로 **규범 표를 따라 갈 수 없다**(§A1 4c).
    /// 지어내지 않고 거부한다 — 규범 밖 상태를 만드는 것보다 낫다.
    AttemptStateUnreachable {
        from: AttemptState,
        to: AttemptState,
    },
    ReportConflict {
        attempt_id: String,
        node_id: String,
    },
    Corrupt {
        attempt_id: String,
        node_id: String,
        kind: AttemptReportCorruption,
    },
    Staging(String),
    Io(String),
    LockTimeout,
    #[cfg(test)]
    InjectedFailure(&'static str),
}

impl std::fmt::Display for AttemptReportStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(field) => write!(f, "invalid AttemptReport input: {field}"),
            Self::AttemptStateUnreachable { from, to } => write!(
                f,
                "이 보고가 가리키는 종료 상태로 규범 표를 따라갈 수 없다: {from:?} -> {to:?}"
            ),
            Self::InvalidOutcome(outcome) => {
                write!(
                    f,
                    "AttemptReport outcome is not a known terminal value: {outcome}"
                )
            }
            Self::ReportRule(rule) => write!(f, "AttemptReport field combination rejected: {rule}"),
            Self::AttemptNotFound { attempt_id } => {
                write!(f, "AttemptReport references missing Attempt: {attempt_id}")
            }
            Self::ReservationNotFound { node_id } => {
                write!(
                    f,
                    "AttemptReport node has no current reservation: {node_id}"
                )
            }
            Self::BindingMismatch(field) => {
                write!(f, "AttemptReport durable binding mismatch: {field:?}")
            }
            Self::ReportConflict {
                attempt_id,
                node_id,
            } => write!(
                f,
                "AttemptReport conflicts with first durable evidence: \
                 attempt={attempt_id}, node={node_id}"
            ),
            Self::Corrupt {
                attempt_id,
                node_id,
                kind,
            } => write!(
                f,
                "durable AttemptReport is corrupt: \
                 attempt={attempt_id}, node={node_id}, kind={kind:?}"
            ),
            Self::Staging(message) => {
                write!(f, "AttemptReport staging-state read failed: {message}")
            }
            Self::Io(message) => write!(f, "AttemptReport store I/O error: {message}"),
            Self::LockTimeout => write!(f, "AttemptReport store lock acquisition timed out"),
            #[cfg(test)]
            Self::InjectedFailure(point) => {
                write!(f, "injected AttemptReport store failure: {point}")
            }
        }
    }
}

impl std::error::Error for AttemptReportStoreError {}

pub struct CoordinatorAttemptReportStore {
    connection: Connection,
}

impl CoordinatorAttemptReportStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AttemptReportStoreError> {
        let mut connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        staging_store::initialize_schema(&mut connection).map_err(map_staging_error)?;
        initialize_report_schema(&connection)?;
        // 예약 해제를 같은 트랜잭션에서 하려면 해제 기록 테이블도 있어야 한다.
        crate::reservation_release::initialize_release_schema(&connection)
            .map_err(|error| AttemptReportStoreError::Staging(format!("해제 스키마: {error:?}")))?;
        backfill_terminal_states(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.connection.path(), Some(path) if !path.is_empty() && path != ":memory:")
    }

    /// Returns raw durable evidence, not `Verified<AttemptReport>`.
    /// Consumers must verify the signature again against their authoritative
    /// key directory before using report fields for terminal decisions.
    pub fn get_report_binding(
        &self,
        attempt_id: &str,
        node_id: &str,
    ) -> Result<Option<StoredAttemptReportBinding>, AttemptReportStoreError> {
        fetch_report_binding(&self.connection, attempt_id, node_id)
    }

    /// Stores terminal evidence only after binding it to the durable Attempt
    /// (and, when it still belongs to that Attempt, the node reservation) in
    /// one `BEGIN IMMEDIATE` transaction.
    pub fn store_verified_terminal_report(
        &mut self,
        verified: &Verified<pb::AttemptReport>,
    ) -> Result<StoreAttemptReportResult, AttemptReportStoreError> {
        self.store_verified_terminal_report_inner(verified, None, None)
    }

    /// 보고 저장 · Attempt 종료 전이 · **예약 해제**를 한 커밋에 넣는다.
    ///
    /// ★★ 2026-09-22 (신뢰망 P1-2) — 계획서가 요구한 조건 3(전이와 해제를 같은
    ///   durable transaction 에 묶기)을 실제로 만족시키는 경로다. 전에는
    ///   `reservation_release` 가 자기 트랜잭션을 열어서 **만족시킬 방법이 없었다.**
    ///
    /// ★ 관문은 그대로다 — 진술 셋(`ReleaseAuthorization`)을 통과해야 하고,
    ///   `ObservedExitInSignedReport` 등급은 보고에 실제 종료 관측이 있어야 한다.
    ///   관문에 막히면 **보고 저장까지 통째로 롤백된다**(부분 적용을 만들지 않는다).
    pub fn store_verified_terminal_report_and_release(
        &mut self,
        verified: &Verified<pb::AttemptReport>,
        authorization: crate::reservation_release::ReleaseAuthorization,
        released_at_unix_ms: u64,
    ) -> Result<StoreAttemptReportResult, AttemptReportStoreError> {
        self.store_verified_terminal_report_inner(
            verified,
            None,
            Some((authorization, released_at_unix_ms)),
        )
    }

    fn store_verified_terminal_report_inner(
        &mut self,
        verified: &Verified<pb::AttemptReport>,
        fault: Option<TestFault>,
        release: Option<(crate::reservation_release::ReleaseAuthorization, u64)>,
    ) -> Result<StoreAttemptReportResult, AttemptReportStoreError> {
        // No report field is observed before the only report parameter has
        // crossed the Verified type gate.
        let report = verified.get();
        validate_report_input(report)?;
        let signer_id = verified.signer_id();
        let report_body = report.encode_to_vec();
        let report_hash = blake3_256(&report_body);

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        if let Some(binding) =
            fetch_report_binding(&transaction, &report.attempt_id, &report.node_id)?
        {
            if binding.report != *report || binding.signer_id_at_submission != signer_id {
                return Err(AttemptReportStoreError::ReportConflict {
                    attempt_id: report.attempt_id.clone(),
                    node_id: report.node_id.clone(),
                });
            }
            transaction.commit().map_err(map_sql_error)?;
            return Ok(StoreAttemptReportResult {
                binding,
                created: false,
            });
        }

        let attempt = staging_store::fetch_attempt(&transaction, &report.attempt_id)
            .map_err(map_staging_error)?
            .ok_or_else(|| AttemptReportStoreError::AttemptNotFound {
                attempt_id: report.attempt_id.clone(),
            })?;
        bind_attempt(report, signer_id, &attempt)?;

        // ★ 결정 D1 — 그 노드의 예약이 **이 Attempt 의 것**이면 예약까지 대조한다(어긋나면 전처럼 거부). 예약이 없어졌거나 다른 실행으로
        //   바뀌었으면 위의 배정 기록 대조(bind_attempt)만으로 결합한다 — 과거 실행의 보고로 저장만 한다. 늦은 보고 때문에 새 작업의
        //   예약을 건드리지 않는다(해제 API 는 현재 예약 소유를 따로 대조한다 — reservation_release::check_reservation_owner).
        let reservation = staging_store::fetch_node_reservation(&transaction, &report.node_id)
            .map_err(map_staging_error)?;
        let bound_via = match reservation {
            Some(reservation) if reservation.attempt_id == report.attempt_id => {
                bind_reservation(report, &reservation)?;
                ReportBindingSource::CurrentReservation
            }
            _ => ReportBindingSource::AssignmentRecord,
        };

        transaction
            .execute(
                "INSERT INTO coordinator_attempt_reports(
                    attempt_id, node_id, job_id, fence_epoch,
                    verified_signer_id, report_hash, report_body, bound_via
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    report.attempt_id,
                    report.node_id,
                    report.job_id,
                    encode_u64(report.fence_epoch),
                    signer_id,
                    report_hash.as_slice(),
                    report_body,
                    bound_via.as_str(),
                ],
            )
            .map_err(map_sql_error)?;
        fail_at(fault, TestFault::AfterReportInsert)?;

        // ★★ 2026-09-22 (§A1 4c) — **보고 저장과 같은 트랜잭션에서** Attempt 를 종료 상태로 옮긴다.
        //   갈라 놓으면 "보고는 있는데 상태는 CREATED" 인 행이 다시 생기고, 그게 정확히
        //   예약을 풀지 못하게 만들던 상태였다.
        if let Some(target) = terminal_state_for(report) {
            let observed_exit = matches!(
                pb::ExitObservation::try_from(report.exit_observation),
                Ok(pb::ExitObservation::ObservedWithCode) | Ok(pb::ExitObservation::ObservedNoCode)
            );
            if attempt.state != target {
                let Some(path) = normative_path(attempt.state, target, observed_exit) else {
                    return Err(AttemptReportStoreError::AttemptStateUnreachable {
                        from: attempt.state,
                        to: target,
                    });
                };
                // 경로는 규범 검증용이다 — DB 에는 최종 상태만 쓴다(중간 상태는 관측하지 않았다).
                staging_store::transition_attempt_state_along(
                    &transaction,
                    &report.attempt_id,
                    attempt.state,
                    &path,
                )
                .map_err(|error| AttemptReportStoreError::Staging(error.to_string()))?;
            }
        }

        // ★ 예약 해제까지 같은 커밋에 넣는다(요청했을 때만). 관문에 막히면
        //   오류가 그대로 올라가고 **보고 저장도 롤백된다** — 반쪽 적용을 만들지 않는다.
        if let Some((authorization, released_at)) = release {
            crate::reservation_release::release_within_transaction(
                &transaction,
                verified,
                authorization,
                released_at,
            )
            .map_err(|error| AttemptReportStoreError::Staging(format!("예약 해제: {error:?}")))?;
        }

        transaction.commit().map_err(map_sql_error)?;

        Ok(StoreAttemptReportResult {
            binding: StoredAttemptReportBinding {
                report: report.clone(),
                report_hash,
                signer_id_at_submission: signer_id.to_string(),
                bound_job_id: report.job_id.clone(),
                bound_attempt_id: report.attempt_id.clone(),
                bound_node_id: report.node_id.clone(),
                bound_fence_epoch: report.fence_epoch,
                bound_via,
            },
            created: true,
        })
    }
}

/// 4c 이전에 저장된 행을 고친다 — **보고는 있는데 Attempt 는 CREATED** 인 것들.
///
/// ★★ 왜 필요한가(코덱스 72 권고 3) — 새로 저장하는 경로만 고치면 **이미 있는 행은
///   조용히 CREATED 로 남는다.** 그 행들의 예약은 영영 못 푼다. 지금 고치지 않으면
///   "고쳤다" 는 말이 그 DB 들에 대해서는 거짓이 된다.
///
/// ★ 지어내지 않는다 — 보고가 증명하는 상태로만 옮기고, 규범 표를 따라갈 수 없는
///   조합(예: INTERRUPTED)은 **그대로 둔다.**
fn backfill_terminal_states(connection: &mut Connection) -> Result<(), AttemptReportStoreError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_sql_error)?;
    let rows: Vec<(String, Vec<u8>)> = {
        let mut statement = transaction
            .prepare(
                "SELECT r.attempt_id, r.report_body
                 FROM coordinator_attempt_reports r
                 JOIN coordinator_attempts a ON a.attempt_id = r.attempt_id
                 WHERE a.state = 'CREATED'",
            )
            .map_err(map_sql_error)?;
        let mapped = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        mapped
    };
    for (attempt_id, body) in rows {
        let Ok(report) = pb::AttemptReport::decode(body.as_slice()) else {
            // 디코드가 안 되는 행은 이 자리에서 판단하지 않는다 — 저장 경로의 손상 검사가 본다.
            continue;
        };
        let Some(target) = terminal_state_for(&report) else {
            continue;
        };
        let observed_exit = matches!(
            pb::ExitObservation::try_from(report.exit_observation),
            Ok(pb::ExitObservation::ObservedWithCode) | Ok(pb::ExitObservation::ObservedNoCode)
        );
        let Some(path) = normative_path(AttemptState::Created, target, observed_exit) else {
            continue;
        };
        staging_store::transition_attempt_state_along(
            &transaction,
            &attempt_id,
            AttemptState::Created,
            &path,
        )
        .map_err(|error| AttemptReportStoreError::Staging(error.to_string()))?;
    }
    transaction.commit().map_err(map_sql_error)?;
    Ok(())
}

/// 보고서 테이블을 만든다.
///
/// `reservation_release` 가 같은 control DB 를 열 때도 이 스키마가 있어야
/// 한 트랜잭션에서 증거와 예약을 함께 볼 수 있다.
pub(crate) fn initialize_report_schema(
    connection: &Connection,
) -> Result<(), AttemptReportStoreError> {
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS coordinator_attempt_reports (
                attempt_id TEXT NOT NULL REFERENCES coordinator_attempts(attempt_id),
                node_id TEXT NOT NULL,
                job_id TEXT NOT NULL,
                fence_epoch BLOB NOT NULL,
                verified_signer_id TEXT NOT NULL,
                report_hash BLOB NOT NULL CHECK(length(report_hash) = 32),
                report_body BLOB NOT NULL,
                bound_via TEXT NOT NULL DEFAULT 'current_reservation',
                PRIMARY KEY(attempt_id, node_id)
            );
            "#,
        )
        .map_err(map_sql_error)?;
    migrate_bound_via_column(connection)
}

/// 결정 D1 — 결합 경로 칸이 없던 DB 에 칸을 더한다. 그 전의 행은 전부 현재 예약으로 결합됐다(그때는 그 경로뿐이었다).
///
/// ★ 두 프로세스가 함께 열면 둘 다 칸이 없다고 보고 더하려 할 수 있다 — 더하기가 실패하면 다시 확인해 이미 있으면 성공으로 본다.
fn migrate_bound_via_column(connection: &Connection) -> Result<(), AttemptReportStoreError> {
    if has_bound_via_column(connection)? {
        return Ok(());
    }
    match connection.execute(
        "ALTER TABLE coordinator_attempt_reports ADD COLUMN bound_via TEXT NOT NULL DEFAULT 'current_reservation'",
        [],
    ) {
        Ok(_) => Ok(()),
        Err(error) => {
            if has_bound_via_column(connection)? {
                Ok(())
            } else {
                Err(map_sql_error(error))
            }
        }
    }
}

fn has_bound_via_column(connection: &Connection) -> Result<bool, AttemptReportStoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(coordinator_attempt_reports)")
        .map_err(map_sql_error)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_sql_error)?;
    for name in names {
        if name.map_err(map_sql_error)? == "bound_via" {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_report_input(report: &pb::AttemptReport) -> Result<(), AttemptReportStoreError> {
    if report.job_id.trim().is_empty() {
        return Err(AttemptReportStoreError::InvalidInput("job_id"));
    }
    if report.attempt_id.trim().is_empty() {
        return Err(AttemptReportStoreError::InvalidInput("attempt_id"));
    }
    if report.node_id.trim().is_empty() {
        return Err(AttemptReportStoreError::InvalidInput("node_id"));
    }
    validate_terminal_outcome(report.outcome)?;
    // ★ `Verified` 는 서명 통과이지 조합 규칙 통과가 아니다 — 저장 진입이 직접 부른다(§5.7 (4)).
    gputeer_protocol::attempt_report_rules::validate_attempt_report_semantics(report)
        .map_err(AttemptReportStoreError::ReportRule)
}

/// terminal outcome 인가.
///
/// ★ `reservation_release` 가 **같은 규칙**을 써야 한다 — 여기서
///   terminal 이라 저장한 것을 저기서 아니라고 하면 증거는 있는데 못 푸는
///   상태가 된다.

/// 종료 보고가 **증명하는** 마지막 Attempt 상태.
///
/// ★★ 2026-09-22 (§A1 4c · 코덱스 72 권고 C) — **보지 않은 중간 상태를 합성하지 않는다.**
///   경로는 규범 표로 검증하되 DB 에는 최종 상태만 쓴다. 판단 근거:
///
/// ```text
/// COMPLETED · STALE_COMPLETED      Completed   완료 보고는 워크로드가 돌았다는 뜻이다(계약)
/// OUTPUT_FINALIZATION_FAILED       Failed      규범 표가 Running -> Failed 의 trigger 로 이 이름을 적어 뒀다
/// FAILED + 종료를 관측했다          Failed      exit_observation 이 OBSERVED_* 면 돌았다는 증거다
/// FAILED + 관측 없음(v1 포함)       Failed      ★ 기동 실패 쪽(Starting -> Failed)으로 보수적으로 적는다
/// CANCELLED                        Cancelled   Created -> Cancelled
/// INTERRUPTED                      없음        ★ 규범 Attempt 표에 INTERRUPTED 상태가 없다(그건 Job 쪽이다).
///                                              상태를 바꾸지 않고 보고만 저장한다
/// ```
///
/// ★ **한계(설계 문서 §2 와 같은 내용).** v1 보고는 "기동 실패" 와 "돌다가 실패" 를 구분할 정보를
///   담지 못한다 — 둘 다 `Starting -> Failed` 로 적힌다. 그래서 **실행 실패가 과소계상된다.**
///   구분하려면 v2 의 `exit_observation` 이 있어야 하고, 그건 Agent 쪽 계약이다.
fn terminal_state_for(report: &pb::AttemptReport) -> Option<AttemptState> {
    let observed_exit = matches!(
        pb::ExitObservation::try_from(report.exit_observation),
        Ok(pb::ExitObservation::ObservedWithCode) | Ok(pb::ExitObservation::ObservedNoCode)
    );
    match pb::AttemptOutcome::try_from(report.outcome) {
        Ok(pb::AttemptOutcome::Completed) | Ok(pb::AttemptOutcome::StaleCompleted) => {
            Some(AttemptState::Completed)
        }
        Ok(pb::AttemptOutcome::Failed) | Ok(pb::AttemptOutcome::OutputFinalizationFailed) => {
            let _ = observed_exit; // 어느 쪽이든 최종 상태는 Failed 다 — 경로만 다르다(아래 path_to).
            Some(AttemptState::Failed)
        }
        Ok(pb::AttemptOutcome::Cancelled) => Some(AttemptState::Cancelled),
        // INTERRUPTED 는 Attempt 규범 표에 대응 상태가 없다 — 만들지 않는다.
        _ => None,
    }
}

/// `from` 에서 `to` 까지 **규범 표에 있는 전이만 밟는** 경로. 없으면 None.
///
/// ★ 경로가 필요한 이유 — 규범 표에 `Created -> Completed` 같은 지름길이 없다.
///   `Created -> Starting` 의 trigger 는 `GRANT_ACCEPTED` 이고, **보고가 왔다는 것은
///   Grant 가 수락됐다는 뜻**이므로 그 한 칸은 보고가 증명한다.
///   여기서 하는 것은 "그 경로가 규범 안에 있는가" 확인이다 — 중간 상태를 DB 에 쓰지는 않는다.
fn normative_path(
    from: AttemptState,
    to: AttemptState,
    observed_exit: bool,
) -> Option<Vec<AttemptState>> {
    use gputeer_protocol::attempt_state::transition;
    let candidates: [&[AttemptState]; 4] = [
        &[to],
        &[AttemptState::Starting, to],
        &[AttemptState::Starting, AttemptState::Running, to],
        &[AttemptState::Running, to],
    ];
    for path in candidates {
        // 관측 없는 실패에는 Running 을 끼우지 않는다(합성 금지).
        if !observed_exit && to == AttemptState::Failed && path.contains(&AttemptState::Running) {
            continue;
        }
        let mut current = from;
        let mut ok = true;
        for step in path {
            match transition(current, *step) {
                Ok(next) => current = next,
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && current == to {
            return Some(path.to_vec());
        }
    }
    None
}

pub(crate) fn is_terminal_outcome(outcome: i32) -> bool {
    validate_terminal_outcome(outcome).is_ok()
}

fn validate_terminal_outcome(outcome: i32) -> Result<(), AttemptReportStoreError> {
    match pb::AttemptOutcome::try_from(outcome) {
        Ok(pb::AttemptOutcome::Completed)
        | Ok(pb::AttemptOutcome::Failed)
        | Ok(pb::AttemptOutcome::Interrupted)
        | Ok(pb::AttemptOutcome::Cancelled)
        | Ok(pb::AttemptOutcome::StaleCompleted)
        // B+E — 산출물 확정 실패도 끝난 Attempt 다(state-machines.md §3 RUNNING -> FAILED). v1 에서 쓰면 조합 규칙이 거부한다.
        | Ok(pb::AttemptOutcome::OutputFinalizationFailed) => Ok(()),
        Ok(pb::AttemptOutcome::Unspecified) | Err(_) => {
            Err(AttemptReportStoreError::InvalidOutcome(outcome))
        }
    }
}

fn bind_attempt(
    report: &pb::AttemptReport,
    signer_id: &str,
    attempt: &StoredAttempt,
) -> Result<(), AttemptReportStoreError> {
    if report.job_id != attempt.job_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::JobId,
        ));
    }
    if report.attempt_id != attempt.attempt_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::AttemptId,
        ));
    }
    if attempt.node_ids.as_slice() != [report.node_id.as_str()] {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::NodeId,
        ));
    }
    if signer_id != report.node_id || signer_id != attempt.node_ids[0] {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::SignerId,
        ));
    }
    if report.fence_epoch != attempt.fence_epoch {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::FenceEpoch,
        ));
    }
    Ok(())
}

fn bind_reservation(
    report: &pb::AttemptReport,
    reservation: &StoredNodeReservation,
) -> Result<(), AttemptReportStoreError> {
    if report.job_id != reservation.job_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::ReservationJobId,
        ));
    }
    if report.attempt_id != reservation.attempt_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::ReservationAttemptId,
        ));
    }
    if report.node_id != reservation.node_id {
        return Err(AttemptReportStoreError::BindingMismatch(
            BindingField::ReservationNodeId,
        ));
    }
    Ok(())
}

pub(crate) fn fetch_report_binding(
    connection: &Connection,
    attempt_id: &str,
    node_id: &str,
) -> Result<Option<StoredAttemptReportBinding>, AttemptReportStoreError> {
    let raw = connection
        .query_row(
            "SELECT attempt_id, node_id, job_id, fence_epoch,
                    verified_signer_id, report_hash, report_body, bound_via
             FROM coordinator_attempt_reports
             WHERE attempt_id = ?1 AND node_id = ?2",
            rusqlite::params![attempt_id, node_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some((
        row_attempt_id,
        row_node_id,
        row_job_id,
        epoch,
        signer_id,
        hash,
        body,
        bound_via_text,
    )) = raw
    else {
        return Ok(None);
    };
    let corrupt = |kind| AttemptReportStoreError::Corrupt {
        attempt_id: row_attempt_id.clone(),
        node_id: row_node_id.clone(),
        kind,
    };
    if body.is_empty() {
        return Err(corrupt(AttemptReportCorruption::EmptyBody));
    }
    let report_hash: [u8; 32] = hash
        .try_into()
        .map_err(|_| corrupt(AttemptReportCorruption::HashEncoding))?;
    if blake3_256(&body) != report_hash {
        return Err(corrupt(AttemptReportCorruption::HashMismatch));
    }
    let report = pb::AttemptReport::decode(body.as_slice())
        .map_err(|_| corrupt(AttemptReportCorruption::UndecodableBody))?;
    if validate_terminal_outcome(report.outcome).is_err() {
        return Err(corrupt(AttemptReportCorruption::InvalidOutcome));
    }
    // 재조회 뒤 재검증(§5.7 (4)) — 저장 진입과 같은 함수다. 어긋나면 손상으로 보고 fail-closed.
    if gputeer_protocol::attempt_report_rules::validate_attempt_report_semantics(&report).is_err() {
        return Err(corrupt(AttemptReportCorruption::ReportRule));
    }
    if report.job_id != row_job_id {
        return Err(corrupt(AttemptReportCorruption::JobIdMismatch));
    }
    if report.attempt_id != row_attempt_id {
        return Err(corrupt(AttemptReportCorruption::AttemptIdMismatch));
    }
    if report.node_id != row_node_id {
        return Err(corrupt(AttemptReportCorruption::NodeIdMismatch));
    }
    if signer_id != row_node_id || signer_id != report.node_id {
        return Err(corrupt(AttemptReportCorruption::SignerIdMismatch));
    }
    let bound_fence_epoch =
        decode_u64(&epoch).map_err(|_| corrupt(AttemptReportCorruption::FenceEpochEncoding))?;
    if report.fence_epoch != bound_fence_epoch {
        return Err(corrupt(AttemptReportCorruption::FenceEpochMismatch));
    }

    let attempt = staging_store::fetch_attempt(connection, &row_attempt_id)
        .map_err(map_staging_error)?
        .ok_or_else(|| corrupt(AttemptReportCorruption::MissingAttempt))?;
    if attempt.job_id != row_job_id {
        return Err(corrupt(AttemptReportCorruption::JobIdMismatch));
    }
    if attempt.node_ids.as_slice() != [row_node_id.as_str()] {
        return Err(corrupt(AttemptReportCorruption::NodeIdMismatch));
    }
    if attempt.fence_epoch != bound_fence_epoch {
        return Err(corrupt(AttemptReportCorruption::FenceEpochMismatch));
    }

    let bound_via = ReportBindingSource::parse(&bound_via_text)
        .ok_or_else(|| corrupt(AttemptReportCorruption::BindingSource))?;

    Ok(Some(StoredAttemptReportBinding {
        report,
        report_hash,
        signer_id_at_submission: signer_id,
        bound_job_id: row_job_id,
        bound_attempt_id: row_attempt_id,
        bound_node_id: row_node_id,
        bound_fence_epoch,
        bound_via,
    }))
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8]) -> Result<u64, ()> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| ())?;
    Ok(u64::from_be_bytes(bytes))
}

fn map_sql_error(error: SqlError) -> AttemptReportStoreError {
    match error {
        SqlError::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            AttemptReportStoreError::LockTimeout
        }
        SqlError::SqliteFailure(code, _) => AttemptReportStoreError::Io(code.to_string()),
        other => AttemptReportStoreError::Io(other.to_string()),
    }
}

fn map_staging_error(error: staging_store::StagingStoreError) -> AttemptReportStoreError {
    AttemptReportStoreError::Staging(error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFault {
    AfterReportInsert,
}

#[cfg(test)]
fn fail_at(fault: Option<TestFault>, point: TestFault) -> Result<(), AttemptReportStoreError> {
    if fault == Some(point) {
        Err(AttemptReportStoreError::InjectedFailure(
            "after AttemptReport insert",
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
fn fail_at(_fault: Option<TestFault>, _point: TestFault) -> Result<(), AttemptReportStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory_store::{
        AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory,
    };
    use crate::job_store::{AcceptedJobSubmission, CoordinatorJobStore, JobState};
    use crate::lease_store::CoordinatorLeaseStore;
    use crate::staging_store::{CoordinatorStagingStore, StageQueuedRequest};
    use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
    use gputeer_protocol::signing::{verify, NoReplayCheck};
    use std::path::{Path, PathBuf};

    const JOB_ID: &str = "job-1";
    const ATTEMPT_ID: &str = "attempt-1";
    const LEASE_ID: &str = "lease-1";
    const NODE_ID: &str = "node-1";

    struct Fixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
    }

    fn prepare_fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control.sqlite3");

        let mut inventory = CoordinatorInventoryStore::open(&path).unwrap();
        inventory
            .register_agent(&AgentRegistry {
                node_id: NODE_ID.into(),
                device_id: "device-1".into(),
                owner_member_id: "owner-1".into(),
                verifying_key: vec![1; 32],
                node_state: None,
                risk_state: None,
                security_tier: None,
                isolation_class: None,
                key_protection: None,
            })
            .unwrap();
        inventory
            .update_inventory(&AgentInventory {
                node_id: NODE_ID.into(),
                inventory_revision: 7,
                observed_at_unix_ms: 90,
                gpus: Some(vec![GpuInventory {
                    gpu_id: "gpu-1".into(),
                    model: Some("model-1".into()),
                    healthy: Some(true),
                    available_vram_bytes: Some(16),
                }]),
                available_cpu_cores: Some(8),
                available_ram_bytes: Some(64),
                available_workspace_bytes: Some(64),
                allowed_workload_classes: None,
                third_party_workloads_opt_in: None,
            })
            .unwrap();
        drop(inventory);

        let mut jobs = CoordinatorJobStore::open(&path).unwrap();
        jobs.submit_accepted(
            &AcceptedJobSubmission {
                idempotency_key: [1; 16],
                job_id: JOB_ID.into(),
                submitter_device_id: "submitter-1".into(),
                manifest_hash: [1; 32],
                deadline_unix_ms: Some(10_000),
                max_queue_duration_ms: Some(5_000),
            },
            100,
        )
        .unwrap();
        jobs.start_planning(JOB_ID, 110).unwrap();
        jobs.enqueue(JOB_ID, "plan-1", 120).unwrap();
        drop(jobs);

        let request = StageQueuedRequest {
            operation_key: [2; 16],
            job_id: JOB_ID.into(),
            attempt_id: ATTEMPT_ID.into(),
            lease_id: LEASE_ID.into(),
            node_id: NODE_ID.into(),
            selected_gpu_ids: vec!["gpu-1".into()],
            issuing_coordinator_id: "coordinator-1".into(),
            coordinator_term: 1,
            issued_at_unix_ms: 200,
            renew_after_unix_ms: 500,
            expires_at_unix_ms: 900,
            max_total_duration_seconds: 1,
        };
        CoordinatorStagingStore::open(&path)
            .unwrap()
            .reserve_node_and_stage_queued_with_lease(&request, 7)
            .unwrap();

        Fixture { _dir: dir, path }
    }

    fn verified_report(
        job_id: &str,
        attempt_id: &str,
        node_id: &str,
        fence_epoch: u64,
        outcome: i32,
        key_seed: u8,
        final_step: u64,
    ) -> Verified<pb::AttemptReport> {
        let key = SigningKey::from_bytes(&[key_seed; 32]);
        let mut report = pb::AttemptReport {
            schema_version: 1,
            job_id: job_id.into(),
            attempt_id: attempt_id.into(),
            node_id: node_id.into(),
            fence_epoch,
            outcome,
            final_step,
            started_at_unix_ms: 210,
            finished_at_unix_ms: 300,
            issued_at_unix_ms: 301,
            ..Default::default()
        };
        report.node_signature = sign(&key, &report).to_vec();
        let mut keys = InMemoryKeyring::new();
        keys.insert(node_id, key.verifying_key());
        verify(
            &report,
            1,
            &Ed25519Verifier::new(keys),
            999,
            &mut NoReplayCheck,
        )
        .expect("test AttemptReport signature must verify")
    }

    fn completed_report(fence_epoch: u64) -> Verified<pb::AttemptReport> {
        verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            fence_epoch,
            pb::AttemptOutcome::Completed as i32,
            7,
            10,
        )
    }

    /// B+E 필드(14 · 15 · 16)나 v2 를 쓰는 보고 — 서명 뒤 v2 까지 읽는 검증기로 통과시킨다.
    fn base_report(schema_version: u32, outcome: pb::AttemptOutcome) -> pb::AttemptReport {
        pb::AttemptReport {
            schema_version,
            job_id: JOB_ID.into(),
            attempt_id: ATTEMPT_ID.into(),
            node_id: NODE_ID.into(),
            fence_epoch: 1,
            outcome: outcome as i32,
            final_step: 10,
            started_at_unix_ms: 210,
            finished_at_unix_ms: 300,
            issued_at_unix_ms: 301,
            ..Default::default()
        }
    }

    fn verified_custom(mut report: pb::AttemptReport, key_seed: u8) -> Verified<pb::AttemptReport> {
        let key = SigningKey::from_bytes(&[key_seed; 32]);
        report.node_signature = sign(&key, &report).to_vec();
        let mut keys = InMemoryKeyring::new();
        keys.insert(NODE_ID, key.verifying_key());
        verify(
            &report,
            gputeer_protocol::constants::ATTEMPT_REPORT_MAX_SCHEMA_VERSION,
            &Ed25519Verifier::new(keys),
            999,
            &mut NoReplayCheck,
        )
        .expect("테스트 보고는 서명 검증을 통과해야 한다")
    }

    fn report_count(store: &CoordinatorAttemptReportStore) -> u64 {
        store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM coordinator_attempt_reports",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn rewrite_body(store: &CoordinatorAttemptReportStore, report: &pb::AttemptReport) {
        let body = report.encode_to_vec();
        let hash = blake3_256(&body);
        store
            .connection
            .execute(
                "UPDATE coordinator_attempt_reports
                 SET report_body = ?1, report_hash = ?2
                 WHERE attempt_id = ?3 AND node_id = ?4",
                rusqlite::params![body, hash.as_slice(), ATTEMPT_ID, NODE_ID],
            )
            .unwrap();
    }

    fn insert_other_job(path: &Path) {
        let mut jobs = CoordinatorJobStore::open(path).unwrap();
        jobs.submit_accepted(
            &AcceptedJobSubmission {
                idempotency_key: [9; 16],
                job_id: "job-other".into(),
                submitter_device_id: "submitter-1".into(),
                manifest_hash: [9; 32],
                deadline_unix_ms: None,
                max_queue_duration_ms: None,
            },
            100,
        )
        .unwrap();
    }

    #[test]
    fn all_five_known_terminal_outcomes_are_stored() {
        for outcome in [
            pb::AttemptOutcome::Completed,
            pb::AttemptOutcome::Failed,
            pb::AttemptOutcome::Interrupted,
            pb::AttemptOutcome::Cancelled,
            pb::AttemptOutcome::StaleCompleted,
        ] {
            let fixture = prepare_fixture();
            let report = verified_report(JOB_ID, ATTEMPT_ID, NODE_ID, 1, outcome as i32, 7, 10);
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            let result = store.store_verified_terminal_report(&report).unwrap();
            assert!(result.created, "outcome {outcome:?}");
            assert_eq!(result.binding.report.outcome, outcome as i32);
            assert_eq!(report_count(&store), 1);
        }
    }

    /// 보고의 결과마다 **어떤 종료 상태로 적히는가**(§A1 4c · 설계 §2).
    ///
    /// ★ 이 표가 설계 문서와 어긋나면 둘 중 하나가 거짓말을 하는 것이다.
    #[test]
    fn each_outcome_writes_the_state_its_evidence_supports() {
        for (outcome, expected) in [
            (pb::AttemptOutcome::Completed, Some(AttemptState::Completed)),
            (
                pb::AttemptOutcome::StaleCompleted,
                Some(AttemptState::Completed),
            ),
            (pb::AttemptOutcome::Failed, Some(AttemptState::Failed)),
            (pb::AttemptOutcome::Cancelled, Some(AttemptState::Cancelled)),
            // ★ INTERRUPTED 는 Attempt 규범 표에 대응 상태가 없다 — 상태를 바꾸지 않는다.
            //   보고는 저장되지만 Attempt 는 Created 그대로다.
            (pb::AttemptOutcome::Interrupted, None),
        ] {
            let fixture = prepare_fixture();
            let report = verified_report(JOB_ID, ATTEMPT_ID, NODE_ID, 1, outcome as i32, 7, 10);
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            store.store_verified_terminal_report(&report).unwrap();
            drop(store);

            let attempt = CoordinatorStagingStore::open(&fixture.path)
                .unwrap()
                .get_attempt(ATTEMPT_ID)
                .unwrap()
                .unwrap();
            let want = expected.unwrap_or(AttemptState::Created);
            assert_eq!(
                attempt.state, want,
                "{outcome:?} 보고 뒤 Attempt 상태가 {:?} 다 — 기대는 {want:?}",
                attempt.state
            );
        }
    }

    /// 규범 표 밖의 전이는 **거부한다**(지어내지 않는다).
    ///
    /// ★ 이미 종료된(COMPLETED) Attempt 에 취소 보고가 오면, 규범 표에
    ///   `Completed -> Cancelled` 가 없으므로 거부돼야 한다. 조용히 덮어쓰면
    ///   끝난 작업이 취소된 것으로 뒤바뀐다.
    #[test]
    fn a_transition_outside_the_norm_table_is_refused() {
        let fixture = prepare_fixture();
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let completed = verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            pb::AttemptOutcome::Completed as i32,
            7,
            10,
        );
        store.store_verified_terminal_report(&completed).unwrap();
        drop(store);

        // 같은 Attempt · 같은 노드에 다른 결과의 보고를 넣으려 한다.
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let cancelled = verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            pb::AttemptOutcome::Cancelled as i32,
            7,
            10,
        );
        let error = store
            .store_verified_terminal_report(&cancelled)
            .expect_err("종료된 Attempt 가 취소로 덮였다");
        // 보고 충돌로 먼저 걸리든(같은 키) 전이 거부로 걸리든, **덮어쓰지 않는 것**이 계약이다.
        assert!(
            matches!(
                error,
                AttemptReportStoreError::ReportConflict { .. }
                    | AttemptReportStoreError::AttemptStateUnreachable { .. }
                    | AttemptReportStoreError::Staging(_)
            ),
            "예상 밖 오류: {error}"
        );
        drop(store);

        let attempt = CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_attempt(ATTEMPT_ID)
            .unwrap()
            .unwrap();
        assert_eq!(
            attempt.state,
            AttemptState::Completed,
            "거부했는데 상태가 바뀌었다"
        );
    }

    /// 4c 이전에 만들어진 행(보고는 있는데 Attempt 는 CREATED)을 **열 때 고친다**.
    ///
    /// ★ 새 경로만 고치면 옛 DB 는 계속 예약을 못 푼다. 그 상태를 직접 만들어 확인한다 —
    ///   보고를 저장한 뒤 상태를 손으로 CREATED 로 되돌리고, 다시 열어 본다.
    #[test]
    fn an_old_row_stored_before_4c_is_backfilled_on_open() {
        let fixture = prepare_fixture();
        let report = verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            pb::AttemptOutcome::Completed as i32,
            7,
            10,
        );
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store.store_verified_terminal_report(&report).unwrap();
        drop(store);

        // 4c 이전 상태를 재현한다 — 보고는 있고 Attempt 는 CREATED.
        {
            let connection = Connection::open(&fixture.path).unwrap();
            connection
                .execute(
                    "UPDATE coordinator_attempts SET state = 'CREATED' WHERE attempt_id = ?1",
                    rusqlite::params![ATTEMPT_ID],
                )
                .unwrap();
        }
        assert_eq!(
            CoordinatorStagingStore::open(&fixture.path)
                .unwrap()
                .get_attempt(ATTEMPT_ID)
                .unwrap()
                .unwrap()
                .state,
            AttemptState::Created,
            "옛 상태 재현이 안 됐다 — 이 시험이 아무것도 안 보고 있다"
        );

        // 여는 것만으로 고쳐져야 한다.
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        drop(store);
        assert_eq!(
            CoordinatorStagingStore::open(&fixture.path)
                .unwrap()
                .get_attempt(ATTEMPT_ID)
                .unwrap()
                .unwrap()
                .state,
            AttemptState::Completed,
            "옛 행이 그대로 CREATED 다 — 그 DB 들은 예약을 영영 못 푼다"
        );
    }

    #[test]
    fn unspecified_and_unknown_outcomes_create_no_row() {
        for outcome in [pb::AttemptOutcome::Unspecified as i32, 99] {
            let fixture = prepare_fixture();
            let report = verified_report(JOB_ID, ATTEMPT_ID, NODE_ID, 1, outcome, 7, 10);
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            assert_eq!(
                store.store_verified_terminal_report(&report),
                Err(AttemptReportStoreError::InvalidOutcome(outcome))
            );
            assert_eq!(report_count(&store), 0);
        }
    }

    /// 결함 70 · 74 — 서명은 유효하지만 조합 규칙을 어긴 보고는 행을 만들지 않고, 거부 **종류**가 규칙 위반이다.
    #[test]
    fn signed_reports_that_break_the_field_rules_create_no_row() {
        use gputeer_protocol::attempt_report_rules::ReportRuleError as R;
        let mut v1_new_field = base_report(1, pb::AttemptOutcome::Failed);
        v1_new_field.exit_observation = pb::ExitObservation::ObservedWithCode as i32;
        v1_new_field.exit_code = 7;
        let v1_new_outcome = base_report(1, pb::AttemptOutcome::OutputFinalizationFailed);
        let mut v2_unobserved_completion = base_report(2, pb::AttemptOutcome::Completed);
        v2_unobserved_completion.exit_observation = pb::ExitObservation::NotObserved as i32;
        for (report, expected) in [
            (v1_new_field, R::V1UsesNewFields),
            (v1_new_outcome, R::V1UsesNewOutcome),
            (
                v2_unobserved_completion,
                R::CombinationRejected {
                    outcome: pb::AttemptOutcome::Completed as i32,
                    exit_observation: pb::ExitObservation::NotObserved as i32,
                    exit_code: 0,
                    stage: 0,
                },
            ),
        ] {
            let fixture = prepare_fixture();
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            assert_eq!(
                store.store_verified_terminal_report(&verified_custom(report, 7)),
                Err(AttemptReportStoreError::ReportRule(expected))
            );
            assert_eq!(report_count(&store), 0);
        }
    }

    /// v2 산출물 확정 실패(outcome 6)는 terminal 증거로 저장되고, 같은 보고 재제출은 재조회 검사를 지나 첫 행을 돌려준다.
    #[test]
    fn a_v2_output_finalization_failure_is_stored_and_replays() {
        let fixture = prepare_fixture();
        let mut report = base_report(2, pb::AttemptOutcome::OutputFinalizationFailed);
        report.exit_observation = pb::ExitObservation::ObservedWithCode as i32;
        report.finalization_failure_stage = pb::FinalizationFailureStage::ReadOutputs as i32;
        let verified = verified_custom(report, 7);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let first = store.store_verified_terminal_report(&verified).unwrap();
        assert!(first.created);
        assert_eq!(first.binding.report, verified.get().clone());
        let again = store.store_verified_terminal_report(&verified).unwrap();
        assert!(!again.created);
        assert_eq!(report_count(&store), 1);
    }

    /// 신뢰망 등급으로 **보고 저장 · 종료 전이 · 예약 해제가 한 커밋에** 들어간다.
    ///
    /// ★ 계획서 조건 3 을 실제로 만족시키는 경로다(전에는 API 구조상 불가능했다).
    #[test]
    fn a_report_with_an_observed_exit_releases_the_reservation_in_the_same_commit() {
        use crate::reservation_release::{
            ArtifactDurabilityGuard, KeyDirectoryProvenance, ReleaseAuthorization, RuntimeStopProof,
        };
        let fixture = prepare_fixture();
        let mut report = base_report(2, pb::AttemptOutcome::Failed);
        report.exit_observation = pb::ExitObservation::ObservedWithCode as i32;
        report.exit_code = 3;
        let verified = verified_custom(report, 7);

        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .store_verified_terminal_report_and_release(
                &verified,
                ReleaseAuthorization {
                    runtime_stop: RuntimeStopProof::ObservedExitInSignedReport,
                    key_directory: KeyDirectoryProvenance::AuthoritativeDirectoryVerifiedByCaller,
                    artifact_durability: ArtifactDurabilityGuard::NotApplicableNonCompleted,
                },
                777,
            )
            .expect("해제까지 한 커밋에 들어가야 한다");
        drop(store);

        let staging = CoordinatorStagingStore::open(&fixture.path).unwrap();
        assert_eq!(
            staging.get_attempt(ATTEMPT_ID).unwrap().unwrap().state,
            AttemptState::Failed,
            "상태가 안 바뀌었다"
        );
        assert_eq!(
            staging.get_node_reservation(NODE_ID).unwrap(),
            None,
            "예약이 아직 남아 있다 — 그 노드는 계속 묶인다"
        );
    }

    /// 옛 보고(종료 관측 없음)로는 **풀지 않는다** — 등급을 이름만 바꾼 게 아님을 고정한다.
    #[test]
    fn a_report_without_an_observed_exit_refuses_to_release_and_rolls_back() {
        use crate::reservation_release::{
            ArtifactDurabilityGuard, KeyDirectoryProvenance, ReleaseAuthorization, RuntimeStopProof,
        };
        let fixture = prepare_fixture();
        // v1 — exit_observation 이 없다(정보 없음).
        let verified = verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            pb::AttemptOutcome::Failed as i32,
            7,
            10,
        );
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .store_verified_terminal_report_and_release(
                &verified,
                ReleaseAuthorization {
                    runtime_stop: RuntimeStopProof::ObservedExitInSignedReport,
                    key_directory: KeyDirectoryProvenance::AuthoritativeDirectoryVerifiedByCaller,
                    artifact_durability: ArtifactDurabilityGuard::NotApplicableNonCompleted,
                },
                777,
            )
            .expect_err("종료 관측이 없는데 풀렸다");
        drop(store);

        // ★ 보고 저장까지 통째로 롤백돼야 한다 — 반쪽 적용을 만들지 않는다.
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(report_count(&store), 0, "관문에 막혔는데 보고가 남았다");
        drop(store);
        let staging = CoordinatorStagingStore::open(&fixture.path).unwrap();
        assert!(
            staging.get_node_reservation(NODE_ID).unwrap().is_some(),
            "막혔는데 예약이 사라졌다"
        );
        assert_eq!(
            staging.get_attempt(ATTEMPT_ID).unwrap().unwrap().state,
            AttemptState::Created,
            "막혔는데 상태가 바뀌었다"
        );
    }

    /// 재조회 뒤 재검증(§5.7 (4)) — 규칙을 어긴 저장본은 손상으로 fail-closed.
    #[test]
    fn a_stored_body_that_breaks_the_field_rules_fails_closed() {
        let fixture = prepare_fixture();
        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store.store_verified_terminal_report(&report).unwrap();
        let mut changed = report.get().clone();
        // v1 에 v2 필드 — 저장 진입이면 거부됐을 몸통.
        changed.exit_observation = pb::ExitObservation::ObservedWithCode as i32;
        rewrite_body(&store, &changed);
        assert_eq!(
            store.get_report_binding(ATTEMPT_ID, NODE_ID),
            Err(AttemptReportStoreError::Corrupt {
                attempt_id: ATTEMPT_ID.into(),
                node_id: NODE_ID.into(),
                kind: AttemptReportCorruption::ReportRule,
            })
        );
    }

    #[test]
    fn wrong_job_attempt_and_stale_fence_create_no_row() {
        for (report, expected) in [
            (
                verified_report(
                    "job-other",
                    ATTEMPT_ID,
                    NODE_ID,
                    1,
                    pb::AttemptOutcome::Completed as i32,
                    7,
                    10,
                ),
                AttemptReportStoreError::BindingMismatch(BindingField::JobId),
            ),
            (
                verified_report(
                    JOB_ID,
                    "attempt-other",
                    NODE_ID,
                    1,
                    pb::AttemptOutcome::Completed as i32,
                    7,
                    10,
                ),
                AttemptReportStoreError::AttemptNotFound {
                    attempt_id: "attempt-other".into(),
                },
            ),
            (
                completed_report(0),
                AttemptReportStoreError::BindingMismatch(BindingField::FenceEpoch),
            ),
        ] {
            let fixture = prepare_fixture();
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            assert_eq!(store.store_verified_terminal_report(&report), Err(expected));
            assert_eq!(report_count(&store), 0);
        }
    }

    #[test]
    fn attempt_node_and_verified_signer_are_bound_to_durable_owner() {
        let fixture = prepare_fixture();
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_attempt_nodes SET node_id = 'node-other'
                 WHERE attempt_id = ?1",
                rusqlite::params![ATTEMPT_ID],
            )
            .unwrap();
        drop(store);

        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_terminal_report(&report),
            Err(AttemptReportStoreError::BindingMismatch(
                BindingField::NodeId
            ))
        );
        assert_eq!(report_count(&store), 0);
    }

    #[test]
    fn missing_or_different_reservation_owner_creates_no_row() {
        let fixture = prepare_fixture();
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute(
                "DELETE FROM coordinator_node_reservation_gpus WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        store
            .connection
            .execute(
                "DELETE FROM coordinator_node_reservations WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        drop(store);
        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        // ★ 결정 D1 — 예약이 없어진 늦은 보고는 거부하지 않고 배정 기록으로 결합해 저장한다. 전에는 ReservationNotFound 로 행이 없었다.
        //   이 테스트의 이름은 DoD-51 증거가 가리켜 그대로 둔다 — 예약이 **같은 Attempt 의 것인데** 어긋나면 아래처럼 여전히 행이 없다.
        let stored = store.store_verified_terminal_report(&report).unwrap();
        assert!(stored.created);
        assert_eq!(
            stored.binding.bound_via,
            ReportBindingSource::AssignmentRecord
        );
        assert_eq!(
            store
                .get_report_binding(ATTEMPT_ID, NODE_ID)
                .unwrap()
                .expect("저장됐다")
                .bound_via,
            ReportBindingSource::AssignmentRecord,
            "결합 경로가 저장돼야 한다"
        );
        assert_eq!(report_count(&store), 1);

        let fixture = prepare_fixture();
        insert_other_job(&fixture.path);
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute(
                "UPDATE coordinator_node_reservations SET job_id = 'job-other'
                 WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        drop(store);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_terminal_report(&report),
            Err(AttemptReportStoreError::BindingMismatch(
                BindingField::ReservationJobId
            ))
        );
        assert_eq!(report_count(&store), 0);
    }

    /// 결정 D1 — 늦은 보고도 배정 기록과 어긋나면(세대) 거부하고 행을 만들지 않는다.
    #[test]
    fn a_late_report_still_has_to_match_the_assignment_record() {
        let fixture = prepare_fixture();
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        store
            .connection
            .execute(
                "DELETE FROM coordinator_node_reservation_gpus WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        store
            .connection
            .execute(
                "DELETE FROM coordinator_node_reservations WHERE node_id = ?1",
                rusqlite::params![NODE_ID],
            )
            .unwrap();
        drop(store);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_terminal_report(&completed_report(2)),
            Err(AttemptReportStoreError::BindingMismatch(
                BindingField::FenceEpoch
            ))
        );
        assert_eq!(report_count(&store), 0);
    }

    /// 결정 D1 — 예약이 그 Attempt 의 것이면 예약까지 대조한 결합으로 기록한다.
    #[test]
    fn a_report_with_its_current_reservation_records_the_reservation_binding() {
        let fixture = prepare_fixture();
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let stored = store
            .store_verified_terminal_report(&completed_report(1))
            .unwrap();
        assert_eq!(
            stored.binding.bound_via,
            ReportBindingSource::CurrentReservation
        );
    }

    /// 결정 D1 — 결합 경로 칸이 없던 DB 를 열면 칸을 더하고, 그 전의 행은 현재 예약 결합으로 읽는다.
    #[test]
    fn a_database_without_the_binding_source_column_is_migrated() {
        let fixture = prepare_fixture();
        {
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            store
                .store_verified_terminal_report(&completed_report(1))
                .unwrap();
            store
                .connection
                .execute_batch("ALTER TABLE coordinator_attempt_reports DROP COLUMN bound_via")
                .unwrap();
        }
        let store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store
                .get_report_binding(ATTEMPT_ID, NODE_ID)
                .unwrap()
                .expect("옛 행")
                .bound_via,
            ReportBindingSource::CurrentReservation
        );
    }

    #[test]
    fn exact_replay_returns_first_row_and_changed_body_or_signature_conflicts() {
        let fixture = prepare_fixture();
        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let first = store.store_verified_terminal_report(&report).unwrap();
        let replay = store.store_verified_terminal_report(&report).unwrap();
        assert!(first.created);
        assert!(!replay.created);
        assert_eq!(replay.binding, first.binding);
        assert_eq!(report_count(&store), 1);

        let changed_body = verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            pb::AttemptOutcome::Failed as i32,
            7,
            11,
        );
        assert!(matches!(
            store.store_verified_terminal_report(&changed_body),
            Err(AttemptReportStoreError::ReportConflict { .. })
        ));
        let changed_signature = verified_report(
            JOB_ID,
            ATTEMPT_ID,
            NODE_ID,
            1,
            pb::AttemptOutcome::Completed as i32,
            8,
            10,
        );
        assert!(matches!(
            store.store_verified_terminal_report(&changed_signature),
            Err(AttemptReportStoreError::ReportConflict { .. })
        ));
        assert_eq!(report_count(&store), 1);
        assert_eq!(
            store.get_report_binding(ATTEMPT_ID, NODE_ID).unwrap(),
            Some(first.binding)
        );
    }

    /// ★★ 2026-09-22 (§A1 4c) — **이 시험이 고정하던 계약의 절반을 의도적으로 뒤집었다.**
    ///
    /// 원래 이름은 `..._without_state_or_release_side_effects` 였고, "보고를 저장해도
    /// Attempt 상태를 **바꾸지 않는다**" 를 고정했다(DoD-51). 그런데 바로 그것 때문에
    /// "이 시도는 끝났다" 를 적을 곳이 없어 예약을 영영 풀지 못했다(§A1 4c).
    ///
    /// 이제 **Attempt 상태는 바뀐다**(종료 보고와 같은 트랜잭션에서). 대신 나머지는
    /// 그대로다 — Job · Lease · 예약은 건드리지 않는다. **해제는 여전히 별도 관문이다.**
    /// 그래서 이름에서 `state` 를 뺐다.
    #[test]
    fn signature_and_binding_survive_reopen_without_release_side_effects() {
        let fixture = prepare_fixture();
        let before_job = CoordinatorJobStore::open(&fixture.path)
            .unwrap()
            .get(JOB_ID)
            .unwrap()
            .unwrap();
        let before_attempt = CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_attempt(ATTEMPT_ID)
            .unwrap()
            .unwrap();
        let before_lease = CoordinatorLeaseStore::open(&fixture.path)
            .unwrap()
            .get(LEASE_ID)
            .unwrap()
            .unwrap();
        let before_reservation = CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_node_reservation(NODE_ID)
            .unwrap()
            .unwrap();
        assert_eq!(before_job.state, JobState::Staging);

        let report = completed_report(1);
        let original = report.get().clone();
        {
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            store.store_verified_terminal_report(&report).unwrap();
        }
        let reopened = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        let binding = reopened
            .get_report_binding(ATTEMPT_ID, NODE_ID)
            .unwrap()
            .unwrap();
        assert_eq!(binding.report, original);
        assert_eq!(binding.report.node_signature, original.node_signature);
        assert_eq!(binding.signer_id_at_submission, NODE_ID);
        assert_eq!(binding.bound_fence_epoch, 1);
        drop(reopened);

        assert_eq!(
            CoordinatorJobStore::open(&fixture.path)
                .unwrap()
                .get(JOB_ID)
                .unwrap()
                .unwrap(),
            before_job
        );
        // Attempt 는 **종료 상태로 바뀌어야 한다** — 그것이 4c 의 목적이다.
        let after_attempt = CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_attempt(ATTEMPT_ID)
            .unwrap()
            .unwrap();
        assert_eq!(
            after_attempt.state,
            AttemptState::Completed,
            "종료 보고를 저장했는데 Attempt 가 아직 {:?} 다 — 끝났다를 적을 곳이 다시 없어졌다",
            after_attempt.state
        );
        // 나머지 칸은 그대로다 — 상태 말고는 아무것도 건드리지 않는다.
        assert_eq!(
            StoredAttempt {
                state: before_attempt.state,
                ..after_attempt.clone()
            },
            before_attempt,
            "상태 외의 칸이 바뀌었다"
        );
        assert_eq!(
            CoordinatorLeaseStore::open(&fixture.path)
                .unwrap()
                .get(LEASE_ID)
                .unwrap()
                .unwrap(),
            before_lease
        );
        assert_eq!(
            CoordinatorStagingStore::open(&fixture.path)
                .unwrap()
                .get_node_reservation(NODE_ID)
                .unwrap()
                .unwrap(),
            before_reservation
        );
    }

    #[test]
    fn failure_after_insert_rolls_back_report_and_preserves_control_state() {
        let fixture = prepare_fixture();
        let before_job = CoordinatorJobStore::open(&fixture.path)
            .unwrap()
            .get(JOB_ID)
            .unwrap()
            .unwrap();
        let report = completed_report(1);
        let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
        assert_eq!(
            store.store_verified_terminal_report_inner(
                &report,
                Some(TestFault::AfterReportInsert),
                None
            ),
            Err(AttemptReportStoreError::InjectedFailure(
                "after AttemptReport insert"
            ))
        );
        assert_eq!(report_count(&store), 0);
        drop(store);
        assert_eq!(
            CoordinatorJobStore::open(&fixture.path)
                .unwrap()
                .get(JOB_ID)
                .unwrap()
                .unwrap(),
            before_job
        );
        assert!(CoordinatorStagingStore::open(&fixture.path)
            .unwrap()
            .get_node_reservation(NODE_ID)
            .unwrap()
            .is_some());
    }

    #[test]
    fn corrupt_body_hash_identity_signer_and_fence_fail_closed() {
        for expected in [
            AttemptReportCorruption::EmptyBody,
            AttemptReportCorruption::UndecodableBody,
            AttemptReportCorruption::HashMismatch,
            AttemptReportCorruption::JobIdMismatch,
            AttemptReportCorruption::AttemptIdMismatch,
            AttemptReportCorruption::NodeIdMismatch,
            AttemptReportCorruption::SignerIdMismatch,
            AttemptReportCorruption::FenceEpochMismatch,
        ] {
            let fixture = prepare_fixture();
            let report = completed_report(1);
            let mut store = CoordinatorAttemptReportStore::open(&fixture.path).unwrap();
            store.store_verified_terminal_report(&report).unwrap();
            match expected {
                AttemptReportCorruption::EmptyBody => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_attempt_reports SET report_body = X''",
                            [],
                        )
                        .unwrap();
                }
                AttemptReportCorruption::UndecodableBody => {
                    let body = vec![0x12, 0x05, b'a'];
                    let hash = blake3_256(&body);
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_attempt_reports
                             SET report_body = ?1, report_hash = ?2",
                            rusqlite::params![body, hash.as_slice()],
                        )
                        .unwrap();
                }
                AttemptReportCorruption::HashMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_attempt_reports SET report_hash = ?1",
                            rusqlite::params![[9u8; 32].as_slice()],
                        )
                        .unwrap();
                }
                AttemptReportCorruption::JobIdMismatch => {
                    let mut changed = report.get().clone();
                    changed.job_id = "job-other".into();
                    rewrite_body(&store, &changed);
                }
                AttemptReportCorruption::AttemptIdMismatch => {
                    let mut changed = report.get().clone();
                    changed.attempt_id = "attempt-other".into();
                    rewrite_body(&store, &changed);
                }
                AttemptReportCorruption::NodeIdMismatch => {
                    let mut changed = report.get().clone();
                    changed.node_id = "node-other".into();
                    rewrite_body(&store, &changed);
                }
                AttemptReportCorruption::SignerIdMismatch => {
                    store
                        .connection
                        .execute(
                            "UPDATE coordinator_attempt_reports
                             SET verified_signer_id = 'node-other'",
                            [],
                        )
                        .unwrap();
                }
                AttemptReportCorruption::FenceEpochMismatch => {
                    let mut changed = report.get().clone();
                    changed.fence_epoch = 2;
                    rewrite_body(&store, &changed);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                store.get_report_binding(ATTEMPT_ID, NODE_ID),
                Err(AttemptReportStoreError::Corrupt {
                    attempt_id: ATTEMPT_ID.into(),
                    node_id: NODE_ID.into(),
                    kind: expected,
                })
            );
        }
    }
}
