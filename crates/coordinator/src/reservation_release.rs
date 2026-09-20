//! 노드 예약을 푸는 durable 경로 — **오늘 정직한 호출은 전부 거부된다.**
//!
//! # ★ 내 전제가 틀렸다
//!
//! 초안은 "`DoD-51` 의 검증된 terminal `AttemptReport` 가 실행 종료의
//! 증명이다" 를 전제로 예약을 풀었다. **틀렸다**(2026-08-30 독립 검수
//! 지적). `DoD-51` evidence 가 스스로 정반대를 적어 뒀다 —
//!
//! > 저장 성공은 **프로세스 종료나 artifact/checkpoint durability 를
//! > 증명하지 않으며 reservation release 의 충분조건이 아니다.**
//!
//! terminal 보고서는 **노드 자신이 보낸 자기보고**다(`CLAUDE.md` §1 의
//! `WORKER_REPORTED`). 정상 키를 가진 노드가 "끝났다" 고 서명해 놓고
//! 계속 돌면, 위조도 DB 조작도 없이 그 GPU 가 남에게 넘어간다 — 그게
//! 정확히 `DoD-49` 가 release 를 미룬 이유다.
//!
//! # 그래서 무엇이 더 필요한가
//!
//! `docs/plans/2026-08-24_1142_...` 가 "안전한 release proof 의 최소
//! 형태" 로 네 가지를 적어 뒀다. 이 모듈은 그중 **1번만** 코드로
//! 확인한다.
//!
//! ```text
//! 1 서명·identity 가 검증된 terminal report 가 예약의 정확한
//!   (job, attempt, node, fence) 와 일치한다          <- 이 모듈이 확인한다
//! 2 outcome 이 실제 workload exit 뒤 생성됐다        <- 값으로 요구한다
//! 3 Attempt terminal 전이·Lease 종료가 예약 삭제와
//!   같은 durable transaction 에 결합된다              <- ★ 이 API 로는
//!                                                       만족시킬 수 없다
//! 4 완료 Job 이 최종 artifact durability guard 를
//!   따로 만족한다                                     <- 값으로 요구한다
//! ```
//!
//! 2 와 4 는 순수 SQL 로 확인할 수 있는 것이 아니다. 그래서 **값으로
//! 요구한다** — [`ReleaseAuthorization`] 의 진술에 기본값이 없으므로
//! 호출부는 반드시 쓰고, 오늘 정직하게 쓸 수 있는 값은 전부
//! "아직 증명 못 함" 이라 **이 API 는 오늘 아무 예약도 풀지 못한다.**
//!
//! # ★ 조건 3 은 진술로도 요구하지 않는다 — 만족시킬 방법이 없어서다
//!
//! 초안은 `TerminalTransitionBinding::BoundByCaller` 라는 진술을 요구했다.
//! 독립 검수 2라운드가 짚었듯 **그 진술은 정직하게 만족시킬 수 없다** —
//! 이 메서드는 자기 connection 으로 자기 트랜잭션을 열고, 호출부가 거기에
//! Attempt/Lease 전이를 끼워 넣을 방법이 없다.
//!
//! 만족시킬 수 없는 것을 요구하면 통과하려는 사람은 거짓말밖에 할 수
//! 없다. 그래서 그 진술을 **없앴다.** 대신 사실을 여기 적는다 —
//!
//! > 이 API 는 **예약 삭제만** 원자적으로 한다. Attempt terminal 전이와
//! > Lease 종료는 별개 트랜잭션이다. 계획서 조건 3 을 만족하려면 이
//! > 저장소들이 트랜잭션을 공유하는 API 가 먼저 필요하고, 그건 이
//! > 조각 밖이다.
//!
//! 지금 안전한 이유는 조건 3 이 충족돼서가 아니라 **조건 2 관문이
//! 모든 호출을 막고 있어서**다.
//!
//! `ADR-033` §8 관문(`crates/scheduler/src/reassignment.rs`)과 같은
//! 모양이다 — **거짓말을 하지 않고는 통과할 수 없고, 그 거짓말이 호출
//! 지점에 남는다.** 확인하는 척은 하지 않는다(`CLAUDE.md` §0.4).
//!
//! # 그럼 왜 지금 만드는가
//!
//! 증명 생산자가 생겼을 때 **붙일 자리와 원자적 삭제 기계**를 미리
//! 갖춰 두기 위해서다. 그 자리를 안 만들어 두면 나중에 급하게 만들면서
//! 위 네 조건 중 몇 개를 건너뛰게 된다.
//!
//! # 남의 예약을 절대 지우지 않는다
//!
//! 예약이 **다른 attempt** 의 것이면 거부한다(`CLAUDE.md` §0.1). 옛
//! 보고서로 새 예약을 밀어내면 지금 돌고 있는 남의 작업을 죽인다 —
//! `runtime-linux` 의 cgroup 회수에서 같은 함정을 이미 한 번 밟았다.
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 실행 종료 확인       못 한다. 호출부의 진술을 요구할 뿐이다
//! 키 디렉터리 검증     못 한다. `Verified` 는 아무 keyring 으로도 만들 수
//!                     있다 — 어느 디렉터리로 검증했는지는 타입에 없다
//! Lease revoke        하지 않는다 — 그리고 이 API 와 **같은 트랜잭션에
//!                     넣을 방법도 없다**(계획서 조건 3 미충족)
//! Job/Attempt 전이    같은 이유로 하지 않는다
//! Attempt fence 대조   따로 하지 않는다 — `fetch_report_binding` 이
//!                     이미 durable Attempt 의 job/node/fence 를 재대조한다.
//!                     여기서 또 하면 도달 불가능한 죽은 코드가 된다.
//!                     ★ 그러나 fence 일치는 **프로세스 종료의 증명이
//!                       아니다** — 둘을 섞어 말하면 안 된다
//! 시계 읽기           하지 않는다. `released_at_unix_ms` 를 인자로 받는다
//! production 연결     없다. 부르는 곳이 아직 없다
//! ```

use std::path::Path;

use gputeer_protocol::{canonical::blake3_256, pb, signing::Verified};
use prost::Message;
use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

use crate::attempt_report_store::{self, AttemptReportStoreError};
use crate::staging_store::{self, StoredNodeReservation};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// 계획서가 요구하는 **실행 종료 증명**에 대한 호출부의 진술.
///
/// > process tree 종료와 VRAM 반환을 확인하는 signed stop ACK, 또는 lease
/// > 만료+grace+fencing 및 partition behavior 가 중복 실행을 막는다는 계약
///
/// ★ 그런 producer 가 이 저장소에 아직 없다. 오늘 정직한 값은
///   [`Self::NotProvenYet`] 하나다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStopProof {
    /// 호출부가 프로세스 종료와 자원 반환을 확인했다고 **진술한다**.
    ProvenByCaller,
    /// 확인하지 못했다 — 오늘의 정직한 값.
    NotProvenYet,
}

/// `DoD-51` 이 남긴 재검증 조건에 대한 호출부의 진술.
///
/// > load 시 서명을 재검증하지 않는 것은 의도된 설계 — terminal consumer 가
/// > 당시 authoritative key directory 로 다시 검증해야 한다
///
/// ★ `Verified<T>` 만으로는 **어느 디렉터리로 검증했는지 알 수 없다** —
///   테스트용 keyring 으로도 만들 수 있다(독립 검수 지적). 타입이 못 하는
///   구분이므로 값으로 요구한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDirectoryProvenance {
    /// 권위 있는 키 디렉터리로 다시 검증했다고 진술한다.
    AuthoritativeDirectoryVerifiedByCaller,
    /// 그러지 않았다 — 오늘의 정직한 값.
    Unverified,
}

/// 계획서 조건 4 — 완료 Job 의 artifact durability guard 진술.
///
/// > 완료 Job 은 최종 artifact/durability guard 를 별도로 만족한다
///
/// ★ `Completed` 보고서에만 적용된다. 실패·취소·중단은 산출물 내구성을
///   전제하지 않는다 — 없는 조건을 요구하면 정직한 호출이 막힌다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactDurabilityGuard {
    /// 최종 artifact 의 durability 요구를 만족했다고 진술한다.
    SatisfiedByCaller,
    /// 만족하지 못했다 — 오늘의 정직한 값.
    NotSatisfiedYet,
    /// 완료가 아니어서 해당 없다.
    ///
    /// `Completed` 보고서에 이 값을 쓰면 거부된다.
    NotApplicableNonCompleted,
}

/// 해제를 허가하는 진술들.
///
/// 기본값이 **없다** — 호출부가 전부 써야 컴파일된다.
///
/// ★ 계획서 조건 3(전이 결합)은 여기 없다. 이 API 로는 만족시킬 방법이
///   없기 때문이다 — 모듈 문서 참조.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseAuthorization {
    pub runtime_stop: RuntimeStopProof,
    pub key_directory: KeyDirectoryProvenance,
    pub artifact_durability: ArtifactDurabilityGuard,
}

/// 예약 해제 사실. 되돌리지 않는 기록이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredReservationRelease {
    pub attempt_id: String,
    pub node_id: String,
    pub job_id: String,
    pub fence_epoch: u64,
    /// 이 해제의 근거가 된 terminal report 의 해시.
    pub report_hash: [u8; 32],
    pub released_at_unix_ms: u64,
    /// 풀려난 GPU 들. 예약이 잡고 있던 것 그대로다.
    pub released_gpu_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// 이번 호출이 실제로 풀었다.
    Released(StoredReservationRelease),
    /// 이미 같은 근거로 풀려 있었다 — 재시도는 안전하다.
    AlreadyReleased(StoredReservationRelease),
}

/// 저장된 해제 기록이 깨졌다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseCorruption {
    FenceEpochEncoding,
    ReleasedAtEncoding,
    HashEncoding,
    GpuOrdinalGap,
    BlankGpuId,
    UnsortedGpuIds,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReservationReleaseError {
    InvalidInput(&'static str),
    /// terminal 이 아닌 outcome 으로는 풀 수 없다.
    NotTerminalOutcome(i32),
    /// 서명은 유효하지만 필드 조합 규칙을 어긴다(B+E 계획서 §5.7 (4) — 해제 진입도 같은 검사).
    ReportRule(gputeer_protocol::attempt_report_rules::ReportRuleError),
    /// ★ 실행이 실제로 멈췄다는 증명이 없다 — **오늘 모든 정직한 호출**이
    ///   여기서 막힌다.
    RuntimeStopNotProven,
    /// 권위 있는 키 디렉터리로 재검증했다는 진술이 없다.
    KeyDirectoryNotVerified,
    /// 완료 Job 인데 artifact durability guard 를 만족했다는 진술이 없다.
    ArtifactDurabilityNotSatisfied,
    /// 완료 보고서에 "완료가 아니라 해당 없음" 을 썼다.
    ArtifactGuardMarkedNotApplicableForCompleted,
    /// 이 attempt·node 에 대한 durable terminal 증거가 없다.
    NoTerminalEvidence {
        attempt_id: String,
        node_id: String,
    },
    /// 저장된 증거와 호출부가 재검증한 보고서가 다르다.
    ///
    /// ★ 저장된 행만으로 푸는 것을 막는 관문이다.
    EvidenceMismatch {
        attempt_id: String,
        node_id: String,
    },
    ReservationNotFound {
        node_id: String,
    },
    /// ★ 이 노드의 예약이 **다른 attempt** 의 것이다 — 지우지 않는다.
    ReservationBelongsToAnotherAttempt {
        node_id: String,
        holder_attempt_id: String,
    },
    ReservationJobMismatch {
        node_id: String,
        reservation_job_id: String,
        report_job_id: String,
    },
    /// 같은 attempt 를 다른 사실로 다시 풀려 했다.
    ReleaseConflict {
        attempt_id: String,
    },
    Corrupt {
        attempt_id: String,
        kind: ReleaseCorruption,
    },
    Evidence(AttemptReportStoreError),
    Storage(String),
}

pub struct CoordinatorReservationReleaseStore {
    connection: Connection,
}

impl CoordinatorReservationReleaseStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ReservationReleaseError> {
        let mut connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        // 같은 control DB 파일을 쓴다 — 예약과 증거가 한 트랜잭션 안에
        // 있어야 원자적으로 풀 수 있다.
        staging_store::initialize_schema(&mut connection).map_err(map_staging_error)?;
        attempt_report_store::initialize_report_schema(&connection).map_err(map_evidence_error)?;
        connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS coordinator_reservation_releases (
                    attempt_id TEXT PRIMARY KEY
                        REFERENCES coordinator_attempts(attempt_id),
                    node_id TEXT NOT NULL,
                    job_id TEXT NOT NULL,
                    fence_epoch BLOB NOT NULL,
                    report_hash BLOB NOT NULL CHECK(length(report_hash) = 32),
                    released_at_unix_ms BLOB NOT NULL
                );
                CREATE TABLE IF NOT EXISTS coordinator_reservation_release_gpus (
                    attempt_id TEXT NOT NULL
                        REFERENCES coordinator_reservation_releases(attempt_id),
                    gpu_id TEXT NOT NULL,
                    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
                    PRIMARY KEY(attempt_id, ordinal),
                    UNIQUE(attempt_id, gpu_id)
                );
                "#,
            )
            .map_err(map_sql_error)?;
        Ok(Self { connection })
    }

    /// `:memory:` 는 durable 이 아니다 — 재시작을 넘지 못한다.
    pub fn is_durable(&self) -> bool {
        matches!(self.connection.path(), Some(path) if !path.is_empty() && path != ":memory:")
    }

    pub fn get_release(
        &self,
        attempt_id: &str,
    ) -> Result<Option<StoredReservationRelease>, ReservationReleaseError> {
        fetch_release(&self.connection, attempt_id)
    }

    /// 노드 예약을 푼다 — **세 진술이 전부 충족될 때만.**
    ///
    /// 하나의 `BEGIN IMMEDIATE` 안에서 증거 대조·예약 소유 확인·삭제·
    /// 해제 기록을 전부-or-none 으로 처리한다.
    ///
    /// ★ 오늘 정직한 호출은 [`ReservationReleaseError::RuntimeStopNotProven`]
    ///   으로 막힌다. 그건 미완성이 아니라 이 모듈이 하는 일이다.
    ///
    /// ★ 계획서 조건 3(Attempt·Lease 전이 결합)은 이 API 로 만족시킬 수
    ///   없다 — 모듈 문서 참조. 이 메서드는 **예약 삭제만** 원자적으로 한다.
    ///
    /// # 멱등성
    ///
    /// 같은 보고서로 다시 부르면 [`ReleaseOutcome::AlreadyReleased`] 다.
    /// 같은 attempt 를 **다른 사실**로 풀려 하면
    /// [`ReservationReleaseError::ReleaseConflict`] 다.
    pub fn release_for_verified_terminal_report(
        &mut self,
        verified: &Verified<pb::AttemptReport>,
        authorization: ReleaseAuthorization,
        released_at_unix_ms: u64,
    ) -> Result<ReleaseOutcome, ReservationReleaseError> {
        // ★ 계획서가 요구하는 세 사실부터 본다. 오늘은 여기서 전부 막힌다.
        check_authorization(authorization)?;

        // 어떤 필드도 Verified 관문을 지나기 전에 읽지 않는다.
        let report = verified.get();
        validate_input(report)?;
        // 조건 4 는 outcome 에 따라 달라지므로 관문을 지난 뒤에 본다.
        check_artifact_guard(report.outcome, authorization.artifact_durability)?;
        let report_body = report.encode_to_vec();
        let report_hash = blake3_256(&report_body);
        let signer_id = verified.signer_id();

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        // 1) 이미 풀렸는가 — 같은 사실이면 멱등, 다르면 충돌.
        if let Some(existing) = fetch_release(&transaction, &report.attempt_id)? {
            if existing.node_id != report.node_id
                || existing.job_id != report.job_id
                || existing.fence_epoch != report.fence_epoch
                || existing.report_hash != report_hash
            {
                return Err(ReservationReleaseError::ReleaseConflict {
                    attempt_id: report.attempt_id.clone(),
                });
            }
            transaction.commit().map_err(map_sql_error)?;
            return Ok(ReleaseOutcome::AlreadyReleased(existing));
        }

        // 2) durable terminal 증거가 있어야 한다.
        let binding = attempt_report_store::fetch_report_binding(
            &transaction,
            &report.attempt_id,
            &report.node_id,
        )
        .map_err(map_evidence_error)?
        .ok_or_else(|| ReservationReleaseError::NoTerminalEvidence {
            attempt_id: report.attempt_id.clone(),
            node_id: report.node_id.clone(),
        })?;

        // 3) ★ 저장된 증거와 재검증된 보고서가 **같아야** 한다.
        //
        //    이게 없으면 저장된 행 하나로 예약을 풀 수 있고, 그건
        //    `attempt_report_store` 가 "raw 는 terminal decision 에 쓰지
        //    말라" 고 적어 둔 계약을 어기는 것이다.
        if binding.report_hash != report_hash
            || binding.report != *report
            || binding.signer_id_at_submission != signer_id
        {
            return Err(ReservationReleaseError::EvidenceMismatch {
                attempt_id: report.attempt_id.clone(),
                node_id: report.node_id.clone(),
            });
        }

        // 4) 예약이 **이 attempt 의 것**이어야 한다.
        //
        //    ★ 여기 "지금 Attempt 의 fence 와 같은가" 검사를 따로 뒀다가
        //      지웠다 — `fetch_report_binding` 이 이미 durable Attempt 의
        //      job_id·node_ids·fence_epoch 를 전부 재대조하고 어긋나면
        //      `Corrupt { FenceEpochMismatch }` 로 막는다. 내 뮤테이션이
        //      그 검사가 **도달 불가능한 죽은 코드**임을 잡았다(R5).
        //      막지도 못하면서 막는 것처럼 보이는 코드는 남기지 않는다.
        let reservation = staging_store::fetch_node_reservation(&transaction, &report.node_id)
            .map_err(map_staging_error)?
            .ok_or_else(|| ReservationReleaseError::ReservationNotFound {
                node_id: report.node_id.clone(),
            })?;
        check_reservation_owner(report, &reservation)?;

        // 6) 풀고 기록한다. 순서상 자식 행이 먼저다.
        let released_gpu_ids = reservation.selected_gpu_ids.clone();
        transaction
            .execute(
                "DELETE FROM coordinator_node_reservation_gpus WHERE node_id = ?1",
                rusqlite::params![report.node_id],
            )
            .map_err(map_sql_error)?;
        let removed = transaction
            .execute(
                "DELETE FROM coordinator_node_reservations WHERE node_id = ?1 AND attempt_id = ?2",
                rusqlite::params![report.node_id, report.attempt_id],
            )
            .map_err(map_sql_error)?;
        if removed != 1 {
            // 방금 읽은 예약이 사라졌다 — 같은 트랜잭션 안이므로 있을 수
            // 없는 일이다. 조용히 넘기지 않는다.
            return Err(ReservationReleaseError::Storage(format!(
                "예약 삭제가 {removed} 행을 지웠다 — 1 이어야 한다"
            )));
        }

        transaction
            .execute(
                "INSERT INTO coordinator_reservation_releases(
                    attempt_id, node_id, job_id, fence_epoch, report_hash, released_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    report.attempt_id,
                    report.node_id,
                    report.job_id,
                    encode_u64(report.fence_epoch),
                    report_hash.as_slice(),
                    encode_u64(released_at_unix_ms),
                ],
            )
            .map_err(map_sql_error)?;
        for (ordinal, gpu_id) in released_gpu_ids.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO coordinator_reservation_release_gpus(
                        attempt_id, gpu_id, ordinal
                     ) VALUES (?1, ?2, ?3)",
                    rusqlite::params![report.attempt_id, gpu_id, ordinal as i64],
                )
                .map_err(map_sql_error)?;
        }

        transaction.commit().map_err(map_sql_error)?;

        Ok(ReleaseOutcome::Released(StoredReservationRelease {
            attempt_id: report.attempt_id.clone(),
            node_id: report.node_id.clone(),
            job_id: report.job_id.clone(),
            fence_epoch: report.fence_epoch,
            report_hash,
            released_at_unix_ms,
            released_gpu_ids,
        }))
    }
}

/// 계획서가 요구하는 세 사실이 진술됐는지 본다.
///
/// ★ 이 커널은 진술이 **참인지** 확인하지 못한다. 확인하는 척하지 않고,
///   진술하지 않으면 통과할 수 없게만 한다(`CLAUDE.md` §0.4).
fn check_authorization(authorization: ReleaseAuthorization) -> Result<(), ReservationReleaseError> {
    if authorization.runtime_stop == RuntimeStopProof::NotProvenYet {
        return Err(ReservationReleaseError::RuntimeStopNotProven);
    }
    if authorization.key_directory == KeyDirectoryProvenance::Unverified {
        return Err(ReservationReleaseError::KeyDirectoryNotVerified);
    }
    Ok(())
}

/// 계획서 조건 4 — `Completed` 일 때만 artifact durability 를 요구한다.
///
/// outcome 을 봐야 하므로 `Verified` 관문을 지난 뒤에 부른다.
fn check_artifact_guard(
    outcome: i32,
    guard: ArtifactDurabilityGuard,
) -> Result<(), ReservationReleaseError> {
    let completed = outcome == pb::AttemptOutcome::Completed as i32;
    match (completed, guard) {
        (true, ArtifactDurabilityGuard::SatisfiedByCaller) => Ok(()),
        (true, ArtifactDurabilityGuard::NotSatisfiedYet) => {
            Err(ReservationReleaseError::ArtifactDurabilityNotSatisfied)
        }
        (true, ArtifactDurabilityGuard::NotApplicableNonCompleted) => {
            Err(ReservationReleaseError::ArtifactGuardMarkedNotApplicableForCompleted)
        }
        (false, _) => Ok(()),
    }
}

fn validate_input(report: &pb::AttemptReport) -> Result<(), ReservationReleaseError> {
    if report.job_id.trim().is_empty() {
        return Err(ReservationReleaseError::InvalidInput("job_id"));
    }
    if report.attempt_id.trim().is_empty() {
        return Err(ReservationReleaseError::InvalidInput("attempt_id"));
    }
    if report.node_id.trim().is_empty() {
        return Err(ReservationReleaseError::InvalidInput("node_id"));
    }
    // terminal 이 아닌 보고서로는 풀 수 없다 — 아직 안 끝났다는 뜻이다.
    if !attempt_report_store::is_terminal_outcome(report.outcome) {
        return Err(ReservationReleaseError::NotTerminalOutcome(report.outcome));
    }
    gputeer_protocol::attempt_report_rules::validate_attempt_report_semantics(report)
        .map_err(ReservationReleaseError::ReportRule)
}

/// 예약이 이 보고서의 것인지 본다.
///
/// ★ 다르면 **지우지 않고 거부한다.** 남의 예약을 지우면 지금 돌고 있는
///   남의 작업을 죽인다(`CLAUDE.md` §0.1).
fn check_reservation_owner(
    report: &pb::AttemptReport,
    reservation: &StoredNodeReservation,
) -> Result<(), ReservationReleaseError> {
    if reservation.attempt_id != report.attempt_id {
        return Err(
            ReservationReleaseError::ReservationBelongsToAnotherAttempt {
                node_id: report.node_id.clone(),
                holder_attempt_id: reservation.attempt_id.clone(),
            },
        );
    }
    if reservation.job_id != report.job_id {
        return Err(ReservationReleaseError::ReservationJobMismatch {
            node_id: report.node_id.clone(),
            reservation_job_id: reservation.job_id.clone(),
            report_job_id: report.job_id.clone(),
        });
    }
    Ok(())
}

fn fetch_release(
    connection: &Connection,
    attempt_id: &str,
) -> Result<Option<StoredReservationRelease>, ReservationReleaseError> {
    let raw = connection
        .query_row(
            "SELECT attempt_id, node_id, job_id, fence_epoch, report_hash, released_at_unix_ms
             FROM coordinator_reservation_releases
             WHERE attempt_id = ?1",
            rusqlite::params![attempt_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some((row_attempt_id, node_id, job_id, fence, hash, released_at)) = raw else {
        return Ok(None);
    };
    let corrupt = |kind| ReservationReleaseError::Corrupt {
        attempt_id: row_attempt_id.clone(),
        kind,
    };
    let fence_epoch =
        decode_u64(&fence).map_err(|_| corrupt(ReleaseCorruption::FenceEpochEncoding))?;
    let released_at_unix_ms =
        decode_u64(&released_at).map_err(|_| corrupt(ReleaseCorruption::ReleasedAtEncoding))?;
    let report_hash: [u8; 32] = hash
        .try_into()
        .map_err(|_| corrupt(ReleaseCorruption::HashEncoding))?;

    let released_gpu_ids = fetch_released_gpu_ids(connection, &row_attempt_id)?;

    Ok(Some(StoredReservationRelease {
        attempt_id: row_attempt_id,
        node_id,
        job_id,
        fence_epoch,
        report_hash,
        released_at_unix_ms,
        released_gpu_ids,
    }))
}

/// 자식 행을 읽으면서 **저장 손상까지 다시 본다.**
///
/// ordinal 구멍·빈 ID·정렬 깨짐은 조용히 넘기지 않는다 — 넘기면 어떤
/// GPU 가 풀렸는지에 대한 기록이 사실과 달라진다.
fn fetch_released_gpu_ids(
    connection: &Connection,
    attempt_id: &str,
) -> Result<Vec<String>, ReservationReleaseError> {
    let mut statement = connection
        .prepare(
            "SELECT gpu_id, ordinal FROM coordinator_reservation_release_gpus
             WHERE attempt_id = ?1 ORDER BY ordinal ASC",
        )
        .map_err(map_sql_error)?;
    let rows = statement
        .query_map(rusqlite::params![attempt_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(map_sql_error)?;

    let corrupt = |kind| ReservationReleaseError::Corrupt {
        attempt_id: attempt_id.to_string(),
        kind,
    };

    let mut ids: Vec<String> = Vec::new();
    for (index, row) in rows.enumerate() {
        let (gpu_id, ordinal) = row.map_err(map_sql_error)?;
        if ordinal != index as i64 {
            return Err(corrupt(ReleaseCorruption::GpuOrdinalGap));
        }
        if gpu_id.trim().is_empty() {
            return Err(corrupt(ReleaseCorruption::BlankGpuId));
        }
        if let Some(previous) = ids.last() {
            if gpu_id.as_str() <= previous.as_str() {
                return Err(corrupt(ReleaseCorruption::UnsortedGpuIds));
            }
        }
        ids.push(gpu_id);
    }
    Ok(ids)
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8]) -> Result<u64, ()> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| ())?;
    Ok(u64::from_be_bytes(bytes))
}

fn map_sql_error(error: SqlError) -> ReservationReleaseError {
    match error {
        SqlError::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            ReservationReleaseError::Storage("SQLite 가 잠겨 있다".to_string())
        }
        other => ReservationReleaseError::Storage(other.to_string()),
    }
}

fn map_staging_error(error: staging_store::StagingStoreError) -> ReservationReleaseError {
    ReservationReleaseError::Storage(format!("staging: {error:?}"))
}

fn map_evidence_error(error: AttemptReportStoreError) -> ReservationReleaseError {
    ReservationReleaseError::Evidence(error)
}
