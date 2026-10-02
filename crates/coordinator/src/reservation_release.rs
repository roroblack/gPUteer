//! 노드 예약을 푸는 durable 경로 — **오늘 정직한 호출은 전부 거부된다.**
//!
//! ★ 2026-10-03 (조각 4a 보조 검수) — 위 문장은 **공개 풀 기준**이다. 신뢰망 배치에는 자기보고 등급으로 푸는 길이 있다 —
//!   종료 관측이 든 서명 보고(`ObservedExitInSignedReport`) · 서명된 정지 확인 알림(`NodeConfirmedStop`) · 운영자 해제(진술 기록).
//!   셋 다 등급을 호출 지점에 값으로 남긴다. 공개 풀에서는 이 등급들로 풀지 않는다.
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

use gputeer_protocol::{
    canonical::blake3_256,
    pb,
    signing::{signing_input, Verified},
};
use prost::Message;
use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

use crate::attempt_report_store::{self, AttemptReportStoreError};
use crate::staging_store;

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
    /// ★★ 2026-09-22 추가 — **노드가 서명한 보고에 "종료를 관측했다" 가 들어 있다.**
    ///
    /// 등급을 이름에 박아 둔다. 이것은 `CLAUDE.md` §1 의 `WORKER_REPORTED` 다 —
    /// 노드 **자기보고**이고, 정상 키를 가진 노드가 "끝났다" 고 서명해 놓고 계속 돌면
    /// 막지 못한다. 그래서 `ProvenByCaller` 와 **같은 값으로 두지 않는다.**
    ///
    /// 쓸 수 있는 조건(호출부가 아니라 이 모듈이 값으로 확인한다):
    /// 보고의 `exit_observation` 이 `OBSERVED_WITH_CODE` 또는 `OBSERVED_NO_CODE` 여야 한다.
    /// 옛 v1 보고(정보 없음)에는 **쓸 수 없다**.
    ///
    /// 신뢰망(서로 믿는 참여자) 배치에서 쓰라고 만든 등급이다. 공개 풀에서는
    /// 이 값으로 풀지 않는다 — 그때는 실제 종료 증명이 필요하다.
    ObservedExitInSignedReport,
    /// ★ 2026-10-03 (실행 알림 계약 v18 §3 · 조각 4a 보조 검수) — **노드(Agent)가 서명한 STOP_CONFIRMED 실행 알림**이 근거다.
    ///
    /// 역시 `WORKER_REPORTED` 다 — 정상 키를 가진 노드가 "멈췄다" 고 서명해 놓고 계속 돌면 막지 못한다. 신뢰망 전용이고 공개 풀에서는 쓰지 않는다.
    /// 정지 확인 경로(`release_for_stop_confirmed_within`)만 받는다 — 종료 보고 경로에 이 값을 쓰면 거부한다(`StopProofGradeMismatch`).
    NodeConfirmedStop,
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

/// ★ 2026-10-03 (실행 알림 계약 v18k §3 "해제 증거 일반화" · 계획 조각 4a) — 예약을 푼 **근거**의 종류.
///   전에는 해제 기록에 종료 보고 해시가 필수라 정지 확인 · 운영자 해제를 담을 자리가 없었다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReleaseEvidenceKind {
    /// 서명된 terminal `AttemptReport` — 해시는 BLAKE3(보고 protobuf 바이트) · payload 는 그 바이트.
    TerminalReport,
    /// 서명된 STOP_CONFIRMED `AttemptRunNotice` — 해시는 BLAKE3(sig_input · ACK 의 notice_hash 와 같다) · payload 는 서명 포함 protobuf 바이트.
    StopConfirmed,
    /// 운영자 해제(release-lost-node · release-held-job) — 해시는 BLAKE3(payload) · payload 는 [`operator_release_payload`] 의 고정 인코딩.
    OperatorRelease,
}

impl ReleaseEvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TerminalReport => "TERMINAL_REPORT",
            Self::StopConfirmed => "STOP_CONFIRMED",
            Self::OperatorRelease => "OPERATOR_RELEASE",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "TERMINAL_REPORT" => Some(Self::TerminalReport),
            "STOP_CONFIRMED" => Some(Self::StopConfirmed),
            "OPERATOR_RELEASE" => Some(Self::OperatorRelease),
            _ => None,
        }
    }
}

/// 해제 근거 한 줄(추가 전용). payload 원문은 표에 남지만 여기서는 싣지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseEvidenceRecord {
    pub kind: ReleaseEvidenceKind,
    pub hash: [u8; 32],
}

/// 예약 해제 사실. 되돌리지 않는 기록이다.
///
/// ★ 2026-10-03 — "사실"(한 시도에 한 행)과 "근거"(추가 전용 · 여럿)로 나눴다. 같은 신원(job · attempt · node · fence)을 다른 근거로 다시 풀면
///   "이미 해제됨" 이고 근거 행만 더한다. 신원이 다르면 충돌이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredReservationRelease {
    pub attempt_id: String,
    pub node_id: String,
    pub job_id: String,
    pub fence_epoch: u64,
    pub released_at_unix_ms: u64,
    /// 풀려난 GPU 들. 예약이 잡고 있던 것 그대로다.
    pub released_gpu_ids: Vec<String>,
    /// 이 해제의 근거들(종류 · 해시 순). 적어도 하나다.
    pub evidence: Vec<ReleaseEvidenceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// 이번 호출이 실제로 풀었다.
    Released(StoredReservationRelease),
    /// 이미 같은 신원으로 풀려 있었다 — 재시도는 안전하다(새 근거면 근거 행만 더했다).
    AlreadyReleased(StoredReservationRelease),
    /// ★ 2026-10-03 (계약 §3 "예약이 이미 없을 때") — 정지 확인 경로에서만: 이 시도가 쥔 예약이 없다(없거나 다른 시도의 것이다).
    ///   아무것도 지우거나 적지 않았다 — 호출자는 알림 저장 · 시도 종결 · ACK 를 그대로 커밋한다. 다른 시도의 예약은 절대 지우지 않는다.
    NothingToRelease { holder_attempt_id: Option<String> },
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
    /// 근거 종류 문자열을 모른다.
    EvidenceKind,
    /// 해제 사실에 근거 행이 하나도 없다.
    MissingEvidence,
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
    /// ★ 2026-10-03 — 정지 확인 경로: 알림이 STOP_CONFIRMED 가 아니다.
    NotStopConfirmed,
    /// ★ 2026-10-03 — 종료 증명 등급이 이 경로의 것이 아니다(종료 보고 경로에 `NodeConfirmedStop` · 정지 확인 경로에 그 밖의 값).
    StopProofGradeMismatch,
    /// 정지 확인 알림이 조합 규칙을 어긴다.
    NoticeRule(gputeer_protocol::attempt_run_notice_rules::RunNoticeRuleError),
    /// 알림의 시도가 durable 저장소에 없다.
    AttemptNotFound {
        attempt_id: String,
    },
    /// 알림의 job · node · fence 가 durable 시도와 다르다(단일 노드 시도만 — 계약 §2).
    AttemptIdentityMismatch {
        attempt_id: String,
    },
    /// 옛 해제 기록을 새 표로 옮기려는데 그 근거(저장된 종료 보고)를 찾지 못했다 — 옮기지 않고 멈춘다(fail closed).
    MigrationEvidenceMissing {
        attempt_id: String,
    },
    /// ★ 2026-10-03 (조각 4b) — 옛 운영자 해제 기록을 옮기려는데 그 시도 행이 없다(fence 를 채울 수 없다) — 옮기지 않고 멈춘다(fail closed — 사람).
    MigrationAttemptMissing {
        attempt_id: String,
    },
    Evidence(AttemptReportStoreError),
    Storage(String),
}

/// 해제 기록 테이블을 만든다.
///
/// ★★ 2026-09-22 — `attempt_report_store` 가 **같은 트랜잭션에서** 예약을 풀 수 있게 되면서
///   이 스키마가 그 경로에서도 필요해졌다. 전에는 이 저장소를 여는 사람만 만들었고,
///   그래서 보고 저장 경로에서 부르면 "no such table" 이 났다.
/// ★ 2026-10-03 (계약 v18k §3 · 조각 4a) — 해제 기록을 둘로 나눴다:
///   `coordinator_release_facts`(사실 — attempt 기본키) · `coordinator_release_fact_gpus` · `coordinator_release_evidence`(근거 — 추가 전용).
///   옛 표 `coordinator_reservation_releases`(종료 보고 해시 필수)는 **더 쓰지 않고** 감사 원본으로 남긴다. 그 행은 열 때마다(멱등) 새 표로 옮기고,
///   근거(TERMINAL_REPORT)의 payload 는 저장된 종료 보고 바이트에서 가져온다 — 못 찾으면 옮기지 않고 거부한다.
pub(crate) fn initialize_release_schema(
    connection: &Connection,
) -> Result<(), ReservationReleaseError> {
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
                CREATE TABLE IF NOT EXISTS coordinator_release_facts (
                    attempt_id TEXT PRIMARY KEY
                        REFERENCES coordinator_attempts(attempt_id),
                    node_id TEXT NOT NULL,
                    job_id TEXT NOT NULL,
                    fence_epoch BLOB NOT NULL CHECK(length(fence_epoch) = 8),
                    released_at_unix_ms BLOB NOT NULL CHECK(length(released_at_unix_ms) = 8)
                );
                CREATE TABLE IF NOT EXISTS coordinator_release_fact_gpus (
                    attempt_id TEXT NOT NULL
                        REFERENCES coordinator_release_facts(attempt_id),
                    gpu_id TEXT NOT NULL,
                    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
                    PRIMARY KEY(attempt_id, ordinal),
                    UNIQUE(attempt_id, gpu_id)
                );
                CREATE TABLE IF NOT EXISTS coordinator_release_evidence (
                    attempt_id TEXT NOT NULL
                        REFERENCES coordinator_release_facts(attempt_id),
                    evidence_kind TEXT NOT NULL
                        CHECK(evidence_kind IN ('TERMINAL_REPORT', 'STOP_CONFIRMED', 'OPERATOR_RELEASE')),
                    evidence_hash BLOB NOT NULL CHECK(length(evidence_hash) = 32),
                    payload BLOB NOT NULL,
                    recorded_at_unix_ms BLOB NOT NULL CHECK(length(recorded_at_unix_ms) = 8),
                    PRIMARY KEY(attempt_id, evidence_kind, evidence_hash)
                );
                CREATE TABLE IF NOT EXISTS coordinator_operator_releases (
                    node_id TEXT NOT NULL,
                    attempt_id TEXT NOT NULL,
                    job_id TEXT NOT NULL,
                    operator_statement TEXT NOT NULL,
                    released_at_unix_ms BLOB NOT NULL,
                    PRIMARY KEY(node_id, attempt_id)
                );
                "#,
        )
        .map_err(map_sql_error)?;
    // ★ 2026-10-03 (조각 4b) — 두 이관을 savepoint 하나로 묶는다. 도중에 멈추면(근거 · 시도 행 없음) 하나도 옮기지 않는다 —
    //   전에는 문장마다 따로 커밋될 수 있어, 사실만 옮겨지고 근거가 빠진 채 다음 열기가 "옮길 것 없음" 으로 넘어갈 수 있었다.
    //   호출자의 트랜잭션 안(보고 저장 경로)에서 불려도 savepoint 는 그 트랜잭션을 건드리지 않는다.
    connection
        .execute_batch("SAVEPOINT release_migration")
        .map_err(map_sql_error)?;
    let migrated =
        migrate_legacy_releases(connection).and_then(|()| migrate_legacy_operator_releases(connection));
    match migrated {
        Ok(()) => connection
            .execute_batch("RELEASE release_migration")
            .map_err(map_sql_error),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK TO release_migration; RELEASE release_migration");
            Err(error)
        }
    }
}

/// 옛 표의 해제 기록을 새 표로 옮긴다(멱등 — 이미 옮긴 행은 건너뛴다).
fn migrate_legacy_releases(connection: &Connection) -> Result<(), ReservationReleaseError> {
    let pending: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM coordinator_reservation_releases r
             WHERE NOT EXISTS (SELECT 1 FROM coordinator_release_facts f WHERE f.attempt_id = r.attempt_id)",
            [],
            |row| row.get(0),
        )
        .map_err(map_sql_error)?;
    if pending == 0 {
        return Ok(());
    }
    let reports_table: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_attempt_reports'",
            [],
            |row| row.get(0),
        )
        .map_err(map_sql_error)?;
    // 근거를 먼저 확인한다 — 하나라도 못 찾으면 아무것도 옮기지 않는다.
    let missing: Option<String> = if reports_table == 0 {
        connection
            .query_row(
                "SELECT attempt_id FROM coordinator_reservation_releases ORDER BY attempt_id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?
    } else {
        connection
            .query_row(
                "SELECT r.attempt_id FROM coordinator_reservation_releases r
                 WHERE NOT EXISTS (SELECT 1 FROM coordinator_release_facts f WHERE f.attempt_id = r.attempt_id)
                   AND NOT EXISTS (SELECT 1 FROM coordinator_attempt_reports a
                                   WHERE a.attempt_id = r.attempt_id AND a.node_id = r.node_id
                                     AND a.report_hash = r.report_hash)
                 ORDER BY r.attempt_id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?
    };
    if let Some(attempt_id) = missing {
        return Err(ReservationReleaseError::MigrationEvidenceMissing { attempt_id });
    }
    connection
        .execute_batch(
            r#"
                INSERT INTO coordinator_release_facts(attempt_id, node_id, job_id, fence_epoch, released_at_unix_ms)
                    SELECT r.attempt_id, r.node_id, r.job_id, r.fence_epoch, r.released_at_unix_ms
                    FROM coordinator_reservation_releases r
                    WHERE NOT EXISTS (SELECT 1 FROM coordinator_release_facts f WHERE f.attempt_id = r.attempt_id);
                INSERT INTO coordinator_release_fact_gpus(attempt_id, gpu_id, ordinal)
                    SELECT g.attempt_id, g.gpu_id, g.ordinal
                    FROM coordinator_reservation_release_gpus g
                    WHERE NOT EXISTS (SELECT 1 FROM coordinator_release_fact_gpus x
                                      WHERE x.attempt_id = g.attempt_id AND x.ordinal = g.ordinal);
                INSERT INTO coordinator_release_evidence(attempt_id, evidence_kind, evidence_hash, payload, recorded_at_unix_ms)
                    SELECT r.attempt_id, 'TERMINAL_REPORT', r.report_hash, a.report_body, r.released_at_unix_ms
                    FROM coordinator_reservation_releases r
                    JOIN coordinator_attempt_reports a
                      ON a.attempt_id = r.attempt_id AND a.node_id = r.node_id AND a.report_hash = r.report_hash
                    WHERE NOT EXISTS (SELECT 1 FROM coordinator_release_evidence e
                                      WHERE e.attempt_id = r.attempt_id AND e.evidence_kind = 'TERMINAL_REPORT'
                                        AND e.evidence_hash = r.report_hash);
                "#,
        )
        .map_err(map_sql_error)
}

/// ★ 2026-10-03 (계약 v18k §3 b16 ③ · 조각 4b) — 옛 `coordinator_operator_releases`(node · attempt · job · 진술 · 시각 — fence · GPU 없음) 행마다
///   해제 사실 + OPERATOR_RELEASE 근거를 만든다. 옛 표는 지우지 않는다(감사 원본).
///   - fence 는 시도 표에서 채운다 — 시도 행이 없으면 멈춘다(`MigrationAttemptMissing`). 시도의 job · 노드가 다르면 `AttemptIdentityMismatch`.
///   - 옛 표는 GPU 목록을 남기지 않았다 — 옮긴 사실의 GPU 목록은 비어 있다(지어내지 않는다).
///   - 같은 신원의 사실이 이미 있으면(종료 보고 이관이 먼저 만들었다) 근거 행만 더한다. 신원이 다르면 충돌이다.
///   - 멱등 — 그 시도의 OPERATOR_RELEASE 근거가 이미 있으면 건너뛴다(4b 뒤의 운영자 해제는 옛 표에 쓰지 않는다).
fn migrate_legacy_operator_releases(connection: &Connection) -> Result<(), ReservationReleaseError> {
    let rows: Vec<(String, String, String, String, Vec<u8>)> = {
        let mut statement = connection
            .prepare(
                "SELECT o.node_id, o.attempt_id, o.job_id, o.operator_statement, o.released_at_unix_ms
                 FROM coordinator_operator_releases o
                 WHERE NOT EXISTS (SELECT 1 FROM coordinator_release_evidence e
                                   WHERE e.attempt_id = o.attempt_id AND e.evidence_kind = 'OPERATOR_RELEASE')
                 ORDER BY o.attempt_id, o.node_id",
            )
            .map_err(map_sql_error)?;
        let mapped = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })
            .map_err(map_sql_error)?;
        mapped
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?
    };
    for (node_id, attempt_id, job_id, operator_statement, raw_released_at) in rows {
        let released_at_unix_ms =
            decode_u64(&raw_released_at).map_err(|_| ReservationReleaseError::Corrupt {
                attempt_id: attempt_id.clone(),
                kind: ReleaseCorruption::ReleasedAtEncoding,
            })?;
        let attempt = staging_store::fetch_attempt(connection, &attempt_id)
            .map_err(map_staging_error)?
            .ok_or_else(|| ReservationReleaseError::MigrationAttemptMissing {
                attempt_id: attempt_id.clone(),
            })?;
        if attempt.job_id != job_id || attempt.node_ids != [node_id.clone()] {
            return Err(ReservationReleaseError::AttemptIdentityMismatch { attempt_id });
        }
        let payload = operator_release_payload(
            OperatorReleaseCommand::ReleaseLostNode,
            &operator_statement,
            &node_id,
            &job_id,
            &attempt_id,
            attempt.fence_epoch,
            released_at_unix_ms,
        )?;
        let hash = blake3_256(&payload);
        match fetch_release(connection, &attempt_id)? {
            Some(existing) => {
                if existing.node_id != node_id
                    || existing.job_id != job_id
                    || existing.fence_epoch != attempt.fence_epoch
                {
                    return Err(ReservationReleaseError::ReleaseConflict { attempt_id });
                }
            }
            None => {
                connection
                    .execute(
                        "INSERT INTO coordinator_release_facts(
                            attempt_id, node_id, job_id, fence_epoch, released_at_unix_ms
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
                            attempt_id,
                            node_id,
                            job_id,
                            encode_u64(attempt.fence_epoch),
                            encode_u64(released_at_unix_ms),
                        ],
                    )
                    .map_err(map_sql_error)?;
            }
        }
        insert_evidence(
            connection,
            &attempt_id,
            ReleaseEvidenceKind::OperatorRelease,
            &hash,
            &payload,
            released_at_unix_ms,
        )?;
    }
    Ok(())
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
        initialize_release_schema(&connection)?;
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
        check_observed_exit(report, authorization.runtime_stop)?;
        let report_body = report.encode_to_vec();
        let report_hash = blake3_256(&report_body);

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        // 1) durable terminal 증거가 있어야 하고, 재검증된 보고서와 **같아야** 한다.
        //
        //    이게 없으면 저장된 행 하나로 예약을 풀 수 있고, 그건
        //    `attempt_report_store` 가 "raw 는 terminal decision 에 쓰지
        //    말라" 고 적어 둔 계약을 어기는 것이다.
        //    ★ 여기 "지금 Attempt 의 fence 와 같은가" 검사를 따로 뒀다가
        //      지웠다 — `fetch_report_binding` 이 이미 durable Attempt 의
        //      job_id·node_ids·fence_epoch 를 전부 재대조한다(R5).
        check_terminal_binding(&transaction, verified, &report_hash)?;

        // 2) 예약이 **이 attempt 의 것**이어야 하고, 풀고 사실 · 근거를 적는다(공통 핵심).
        let outcome = record_release_within(
            &transaction,
            &ReleaseIdentity::of_report(report),
            ReleaseEvidenceKind::TerminalReport,
            &report_hash,
            &report_body,
            released_at_unix_ms,
            ReservationPolicy::MustHold,
        )?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(outcome)
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
    // 정지 확인 알림의 등급으로 종료 보고를 풀지 않는다 — 보고에 담긴 것은 알림이 아니다.
    if authorization.runtime_stop == RuntimeStopProof::NodeConfirmedStop {
        return Err(ReservationReleaseError::StopProofGradeMismatch);
    }
    if authorization.key_directory == KeyDirectoryProvenance::Unverified {
        return Err(ReservationReleaseError::KeyDirectoryNotVerified);
    }
    Ok(())
}

/// **이미 열려 있는 트랜잭션 안에서** 예약을 푼다 — 계획서 조건 3 을 실제로 만족시키는 길.
///
/// ★★ 2026-09-22 (신뢰망 P1-2) — 모듈 문서가 "이 API 로는 조건 3(Attempt 전이·해제를
///   같은 트랜잭션에 묶기)을 만족시킬 수 없다" 고 적어 뒀다. **맞는 말이었다** — 그 API 는
///   자기 connection 으로 자기 트랜잭션을 연다. 그래서 **트랜잭션을 인자로 받는** 길을 새로 낸다.
///   `attempt_report_store` 가 보고 저장 · Attempt 종료 전이 · 예약 해제를 **한 커밋**에 넣는다.
///
/// 관문은 그대로다 — 진술 셋을 확인하고, 증거와 재검증 보고가 같은지 보고, 예약 주인을 대조한다.
/// 다른 점은 **커밋을 여기서 하지 않는다**는 것뿐이다(부른 쪽이 한다).
pub fn release_within_transaction(
    transaction: &Connection,
    verified: &Verified<pb::AttemptReport>,
    authorization: ReleaseAuthorization,
    released_at_unix_ms: u64,
) -> Result<ReleaseOutcome, ReservationReleaseError> {
    check_authorization(authorization)?;
    let report = verified.get();
    validate_input(report)?;
    check_artifact_guard(report.outcome, authorization.artifact_durability)?;
    check_observed_exit(report, authorization.runtime_stop)?;
    let report_body = report.encode_to_vec();
    let report_hash = blake3_256(&report_body);
    check_terminal_binding(transaction, verified, &report_hash)?;
    record_release_within(
        transaction,
        &ReleaseIdentity::of_report(report),
        ReleaseEvidenceKind::TerminalReport,
        &report_hash,
        &report_body,
        released_at_unix_ms,
        ReservationPolicy::MustHold,
    )
}

/// ★ 2026-10-03 (실행 알림 계약 v18k §3 · 계획 조각 4a) — 서명된 **STOP_CONFIRMED** 알림을 근거로 **호출자의 트랜잭션 안에서** 예약을 푼다.
///
/// ```text
/// 근거 등급   NodeConfirmedStop — WORKER_REPORTED(노드 키 서명 · 신뢰망 전용). stop_evidence 1(기계 증거) · 2(소유자 진술) 둘 다 받는다
/// 신원       알림의 job · node · fence 가 durable 시도와 같고 단일 노드 시도여야 한다 — 아니면 거부(계약 §2)
/// 이미 풀림   같은 신원이면 "이미 해제됨"(근거 행만 더한다) · 신원이 다르면 충돌
/// 예약 없음   ★ 종료 보고 경로와 다르다: 예약이 없거나 다른 시도의 것이면 오류가 아니라 `NothingToRelease` — 아무것도 지우거나 적지 않고,
///            호출자가 알림 저장 · 시도 종결 · ACK 를 그대로 커밋한다. 다른 시도의 예약은 절대 지우지 않는다(§0.1)
/// ```
/// ★ 부르는 곳이 아직 없다 — 알림 저장 · 시도 전이와 같은 트랜잭션에 묶는 것은 조각 4d 다. 키 디렉터리 진술은 종료 보고 경로와 같은 이유로 값으로 요구한다.
///
/// ★ 2026-10-03 (조각 4a 보조 검수) — 종료 증명 등급을 **값으로** 받는다. 받는 값은 `NodeConfirmedStop` 하나다(신뢰망 전용 자기보고 —
///   공개 풀 배치는 이 값을 쓸 수 없다). 진술 없이 노드 자기보고로 GPU 를 넘기는 길을 남기지 않는다.
pub fn release_for_stop_confirmed_within(
    transaction: &Connection,
    verified: &Verified<pb::AttemptRunNotice>,
    runtime_stop: RuntimeStopProof,
    key_directory: KeyDirectoryProvenance,
    released_at_unix_ms: u64,
) -> Result<ReleaseOutcome, ReservationReleaseError> {
    match runtime_stop {
        RuntimeStopProof::NodeConfirmedStop => {}
        RuntimeStopProof::NotProvenYet => return Err(ReservationReleaseError::RuntimeStopNotProven),
        RuntimeStopProof::ProvenByCaller | RuntimeStopProof::ObservedExitInSignedReport => {
            return Err(ReservationReleaseError::StopProofGradeMismatch)
        }
    }
    if key_directory == KeyDirectoryProvenance::Unverified {
        return Err(ReservationReleaseError::KeyDirectoryNotVerified);
    }
    // 어떤 필드도 Verified 관문을 지나기 전에 읽지 않는다.
    let notice = verified.get();
    gputeer_protocol::attempt_run_notice_rules::validate_attempt_run_notice(notice)
        .map_err(ReservationReleaseError::NoticeRule)?;
    if notice.kind != pb::RunNoticeKind::StopConfirmed as i32 {
        return Err(ReservationReleaseError::NotStopConfirmed);
    }
    for (value, field) in [
        (&notice.job_id, "job_id"),
        (&notice.attempt_id, "attempt_id"),
        (&notice.node_id, "node_id"),
    ] {
        if value.trim().is_empty() {
            return Err(ReservationReleaseError::InvalidInput(field));
        }
    }
    let attempt = staging_store::fetch_attempt(transaction, &notice.attempt_id)
        .map_err(map_staging_error)?
        .ok_or_else(|| ReservationReleaseError::AttemptNotFound {
            attempt_id: notice.attempt_id.clone(),
        })?;
    if attempt.job_id != notice.job_id
        || attempt.node_ids != [notice.node_id.clone()]
        || attempt.fence_epoch != notice.fence_epoch
    {
        return Err(ReservationReleaseError::AttemptIdentityMismatch {
            attempt_id: notice.attempt_id.clone(),
        });
    }
    let notice_hash = blake3_256(&signing_input(notice));
    record_release_within(
        transaction,
        &ReleaseIdentity {
            attempt_id: &notice.attempt_id,
            job_id: &notice.job_id,
            node_id: &notice.node_id,
            fence_epoch: notice.fence_epoch,
        },
        ReleaseEvidenceKind::StopConfirmed,
        &notice_hash,
        &notice.encode_to_vec(),
        released_at_unix_ms,
        ReservationPolicy::TolerateGoneOrOther,
    )
}

/// ★ 2026-10-03 (계약 v18k §3 b16 ③ · 조각 4b) — 운영자 해제를 낸 명령.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorReleaseCommand {
    /// `gputeer release-lost-node` — 끊긴 노드의 옛 예약(failover.rs).
    ReleaseLostNode,
    /// `release-held-job` — 최종 Job 의 보류 해제(계약 §6 D6 · 아직 없다).
    ReleaseHeldJob,
}

impl OperatorReleaseCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReleaseLostNode => "release-lost-node",
            Self::ReleaseHeldJob => "release-held-job",
        }
    }
}

/// OPERATOR_RELEASE 근거의 payload — 계약 b16 ③ 의 고정 인코딩. 근거 해시는 BLAKE3(이 바이트)다.
///
/// ```text
/// 순서 고정   명령 · 운영자 진술 · node_id · job_id · attempt_id · fence_epoch · released_at_unix_ms
/// 문자열      u32 BE 길이 + UTF-8 바이트
/// 정수        u64 BE
/// ```
pub fn operator_release_payload(
    command: OperatorReleaseCommand,
    operator_statement: &str,
    node_id: &str,
    job_id: &str,
    attempt_id: &str,
    fence_epoch: u64,
    released_at_unix_ms: u64,
) -> Result<Vec<u8>, ReservationReleaseError> {
    let mut payload = Vec::new();
    for text in [command.as_str(), operator_statement, node_id, job_id, attempt_id] {
        let length = u32::try_from(text.len())
            .map_err(|_| ReservationReleaseError::InvalidInput("operator_release_field_too_long"))?;
        payload.extend_from_slice(&length.to_be_bytes());
        payload.extend_from_slice(text.as_bytes());
    }
    payload.extend_from_slice(&fence_epoch.to_be_bytes());
    payload.extend_from_slice(&released_at_unix_ms.to_be_bytes());
    Ok(payload)
}

/// ★ 2026-10-03 (계약 v18k §3 · 조각 4b) — 운영자 해제를 **호출자의 트랜잭션 안에서** 해제 사실 + OPERATOR_RELEASE 근거로 적고 예약을 지운다.
///
/// 판정(풀어도 되는가 — 예: release-lost-node 의 "시도가 대체됐거나 Job 이 끝났다")은 호출자가 먼저 한다. 여기서는 기록 방식만 맡는다:
/// ```text
/// 진술      비면 거부
/// 신원      시도 행에서 job · fence 를 읽는다 — 시도가 없으면 `AttemptNotFound` · 그 노드 하나의 시도가 아니면 `AttemptIdentityMismatch`
/// 예약      그 시도의 것이어야 한다(종료 보고 경로와 같은 MustHold — 없거나 남의 것이면 오류 · 남의 예약은 지우지 않는다)
/// 이미 풀림  같은 신원이면 "이미 해제됨"(근거 행만 더한다) · 다르면 충돌
/// ```
pub(crate) fn release_by_operator_within(
    transaction: &Connection,
    command: OperatorReleaseCommand,
    operator_statement: &str,
    node_id: &str,
    attempt_id: &str,
    released_at_unix_ms: u64,
) -> Result<ReleaseOutcome, ReservationReleaseError> {
    if operator_statement.trim().is_empty() {
        return Err(ReservationReleaseError::InvalidInput("operator_statement"));
    }
    let attempt = staging_store::fetch_attempt(transaction, attempt_id)
        .map_err(map_staging_error)?
        .ok_or_else(|| ReservationReleaseError::AttemptNotFound {
            attempt_id: attempt_id.to_string(),
        })?;
    if attempt.node_ids != [node_id.to_string()] {
        return Err(ReservationReleaseError::AttemptIdentityMismatch {
            attempt_id: attempt_id.to_string(),
        });
    }
    let payload = operator_release_payload(
        command,
        operator_statement,
        node_id,
        &attempt.job_id,
        attempt_id,
        attempt.fence_epoch,
        released_at_unix_ms,
    )?;
    let hash = blake3_256(&payload);
    record_release_within(
        transaction,
        &ReleaseIdentity {
            attempt_id,
            job_id: &attempt.job_id,
            node_id,
            fence_epoch: attempt.fence_epoch,
        },
        ReleaseEvidenceKind::OperatorRelease,
        &hash,
        &payload,
        released_at_unix_ms,
        ReservationPolicy::MustHold,
    )
}

/// 저장된 종료 보고와 재검증된 보고가 같은지 본다(해시 · 내용 · 제출 당시 서명자).
fn check_terminal_binding(
    transaction: &Connection,
    verified: &Verified<pb::AttemptReport>,
    report_hash: &[u8; 32],
) -> Result<(), ReservationReleaseError> {
    let report = verified.get();
    let binding = attempt_report_store::fetch_report_binding(
        transaction,
        &report.attempt_id,
        &report.node_id,
    )
    .map_err(map_evidence_error)?
    .ok_or_else(|| ReservationReleaseError::NoTerminalEvidence {
        attempt_id: report.attempt_id.clone(),
        node_id: report.node_id.clone(),
    })?;
    if binding.report_hash != *report_hash
        || binding.report != *report
        || binding.signer_id_at_submission != verified.signer_id()
    {
        return Err(ReservationReleaseError::EvidenceMismatch {
            attempt_id: report.attempt_id.clone(),
            node_id: report.node_id.clone(),
        });
    }
    Ok(())
}

/// 해제할 시도의 신원.
struct ReleaseIdentity<'a> {
    attempt_id: &'a str,
    job_id: &'a str,
    node_id: &'a str,
    fence_epoch: u64,
}

impl<'a> ReleaseIdentity<'a> {
    fn of_report(report: &'a pb::AttemptReport) -> Self {
        Self {
            attempt_id: &report.attempt_id,
            job_id: &report.job_id,
            node_id: &report.node_id,
            fence_epoch: report.fence_epoch,
        }
    }
}

/// 예약이 없거나 다른 시도의 것일 때.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReservationPolicy {
    /// 종료 보고 경로 — 오류로 되돌린다(지금까지의 동작).
    MustHold,
    /// 정지 확인 경로 — `NothingToRelease` 로 알리고 나머지는 호출자가 커밋한다(계약 §3).
    TolerateGoneOrOther,
}

/// 공통 핵심 — **호출자의 트랜잭션 안에서** 사실 · 근거를 적고 예약을 지운다. 커밋은 호출자가 한다.
fn record_release_within(
    transaction: &Connection,
    identity: &ReleaseIdentity<'_>,
    kind: ReleaseEvidenceKind,
    evidence_hash: &[u8; 32],
    payload: &[u8],
    released_at_unix_ms: u64,
    policy: ReservationPolicy,
) -> Result<ReleaseOutcome, ReservationReleaseError> {
    // 1) 이미 풀렸는가 — 같은 신원이면 "이미 해제됨"(새 근거면 근거 행만 더한다), 다르면 충돌.
    if let Some(existing) = fetch_release(transaction, identity.attempt_id)? {
        if existing.node_id != identity.node_id
            || existing.job_id != identity.job_id
            || existing.fence_epoch != identity.fence_epoch
        {
            return Err(ReservationReleaseError::ReleaseConflict {
                attempt_id: identity.attempt_id.to_string(),
            });
        }
        insert_evidence(
            transaction,
            identity.attempt_id,
            kind,
            evidence_hash,
            payload,
            released_at_unix_ms,
        )?;
        let refreshed = fetch_release(transaction, identity.attempt_id)?.ok_or_else(|| {
            ReservationReleaseError::Storage("방금 읽은 해제 사실이 사라졌다".to_string())
        })?;
        return Ok(ReleaseOutcome::AlreadyReleased(refreshed));
    }

    // 2) 예약이 **이 attempt 의 것**이어야 한다. ★ 다르면 절대 지우지 않는다(§0.1).
    let reservation = match (
        staging_store::fetch_node_reservation(transaction, identity.node_id)
            .map_err(map_staging_error)?,
        policy,
    ) {
        (Some(reservation), _) => reservation,
        (None, ReservationPolicy::MustHold) => {
            return Err(ReservationReleaseError::ReservationNotFound {
                node_id: identity.node_id.to_string(),
            })
        }
        (None, ReservationPolicy::TolerateGoneOrOther) => {
            return Ok(ReleaseOutcome::NothingToRelease {
                holder_attempt_id: None,
            })
        }
    };
    if reservation.attempt_id != identity.attempt_id {
        return match policy {
            ReservationPolicy::MustHold => Err(
                ReservationReleaseError::ReservationBelongsToAnotherAttempt {
                    node_id: identity.node_id.to_string(),
                    holder_attempt_id: reservation.attempt_id.clone(),
                },
            ),
            ReservationPolicy::TolerateGoneOrOther => Ok(ReleaseOutcome::NothingToRelease {
                holder_attempt_id: Some(reservation.attempt_id.clone()),
            }),
        };
    }
    if reservation.job_id != identity.job_id {
        return Err(ReservationReleaseError::ReservationJobMismatch {
            node_id: identity.node_id.to_string(),
            reservation_job_id: reservation.job_id.clone(),
            report_job_id: identity.job_id.to_string(),
        });
    }

    // 3) 풀고 기록한다. 순서상 자식 행이 먼저다.
    let released_gpu_ids = reservation.selected_gpu_ids.clone();
    transaction
        .execute(
            "DELETE FROM coordinator_node_reservation_gpus WHERE node_id = ?1",
            rusqlite::params![identity.node_id],
        )
        .map_err(map_sql_error)?;
    let removed = transaction
        .execute(
            "DELETE FROM coordinator_node_reservations WHERE node_id = ?1 AND attempt_id = ?2",
            rusqlite::params![identity.node_id, identity.attempt_id],
        )
        .map_err(map_sql_error)?;
    if removed != 1 {
        // 방금 읽은 예약이 사라졌다 — 같은 트랜잭션 안이므로 있을 수 없는 일이다. 조용히 넘기지 않는다.
        return Err(ReservationReleaseError::Storage(format!(
            "예약 삭제가 {removed} 행을 지웠다 — 1 이어야 한다"
        )));
    }
    transaction
        .execute(
            "INSERT INTO coordinator_release_facts(
                attempt_id, node_id, job_id, fence_epoch, released_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                identity.attempt_id,
                identity.node_id,
                identity.job_id,
                encode_u64(identity.fence_epoch),
                encode_u64(released_at_unix_ms),
            ],
        )
        .map_err(map_sql_error)?;
    for (ordinal, gpu_id) in released_gpu_ids.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO coordinator_release_fact_gpus(
                    attempt_id, gpu_id, ordinal
                 ) VALUES (?1, ?2, ?3)",
                rusqlite::params![identity.attempt_id, gpu_id, ordinal as i64],
            )
            .map_err(map_sql_error)?;
    }
    insert_evidence(
        transaction,
        identity.attempt_id,
        kind,
        evidence_hash,
        payload,
        released_at_unix_ms,
    )?;

    Ok(ReleaseOutcome::Released(StoredReservationRelease {
        attempt_id: identity.attempt_id.to_string(),
        node_id: identity.node_id.to_string(),
        job_id: identity.job_id.to_string(),
        fence_epoch: identity.fence_epoch,
        released_at_unix_ms,
        released_gpu_ids,
        evidence: vec![ReleaseEvidenceRecord {
            kind,
            hash: *evidence_hash,
        }],
    }))
}

/// 근거 한 줄을 더한다(같은 종류 · 해시가 이미 있으면 그대로 — 추가 전용 · 멱등).
fn insert_evidence(
    transaction: &Connection,
    attempt_id: &str,
    kind: ReleaseEvidenceKind,
    hash: &[u8; 32],
    payload: &[u8],
    recorded_at_unix_ms: u64,
) -> Result<(), ReservationReleaseError> {
    transaction
        .execute(
            "INSERT OR IGNORE INTO coordinator_release_evidence(
                attempt_id, evidence_kind, evidence_hash, payload, recorded_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                attempt_id,
                kind.as_str(),
                hash.as_slice(),
                payload,
                encode_u64(recorded_at_unix_ms),
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

/// `ObservedExitInSignedReport` 를 **값으로** 확인한다.
///
/// ★ 진술만 받고 넘어가면 이 등급은 `ProvenByCaller` 와 같아진다 — 이름만 정직하고
///   동작은 같은 꼴이다. 보고에 실제로 종료 관측이 들어 있는지 여기서 본다.
fn check_observed_exit(
    report: &pb::AttemptReport,
    proof: RuntimeStopProof,
) -> Result<(), ReservationReleaseError> {
    if proof != RuntimeStopProof::ObservedExitInSignedReport {
        return Ok(());
    }
    let observed = matches!(
        pb::ExitObservation::try_from(report.exit_observation),
        Ok(pb::ExitObservation::ObservedWithCode) | Ok(pb::ExitObservation::ObservedNoCode)
    );
    if observed {
        Ok(())
    } else {
        Err(ReservationReleaseError::RuntimeStopNotProven)
    }
}

/// 계획서 조건 4 — `Completed` 일 때만 artifact durability 를 요구한다.
///
/// outcome 을 봐야 하므로 `Verified` 관문을 지난 뒤에 부른다.
fn check_artifact_guard(
    outcome: i32,
    guard: ArtifactDurabilityGuard,
) -> Result<(), ReservationReleaseError> {
    // ★ 2026-09-23 (결함 214 · 검수 73) — STALE_COMPLETED 도 완료다. 빼면 늦은 완료 보고가 이 관문을 비껴간다.
    let completed = outcome == pb::AttemptOutcome::Completed as i32
        || outcome == pb::AttemptOutcome::StaleCompleted as i32;
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

fn fetch_release(
    connection: &Connection,
    attempt_id: &str,
) -> Result<Option<StoredReservationRelease>, ReservationReleaseError> {
    let raw = connection
        .query_row(
            "SELECT attempt_id, node_id, job_id, fence_epoch, released_at_unix_ms
             FROM coordinator_release_facts
             WHERE attempt_id = ?1",
            rusqlite::params![attempt_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    let Some((row_attempt_id, node_id, job_id, fence, released_at)) = raw else {
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

    let released_gpu_ids = fetch_released_gpu_ids(connection, &row_attempt_id)?;
    let evidence = fetch_release_evidence(connection, &row_attempt_id)?;
    if evidence.is_empty() {
        return Err(corrupt(ReleaseCorruption::MissingEvidence));
    }

    Ok(Some(StoredReservationRelease {
        attempt_id: row_attempt_id,
        node_id,
        job_id,
        fence_epoch,
        released_at_unix_ms,
        released_gpu_ids,
        evidence,
    }))
}

/// 근거 행을 (종류 · 해시) 순으로 읽는다. 모르는 종류 · 해시 길이는 손상이다.
fn fetch_release_evidence(
    connection: &Connection,
    attempt_id: &str,
) -> Result<Vec<ReleaseEvidenceRecord>, ReservationReleaseError> {
    let mut statement = connection
        .prepare(
            "SELECT evidence_kind, evidence_hash FROM coordinator_release_evidence
             WHERE attempt_id = ?1",
        )
        .map_err(map_sql_error)?;
    let rows = statement
        .query_map(rusqlite::params![attempt_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(map_sql_error)?;
    let corrupt = |kind| ReservationReleaseError::Corrupt {
        attempt_id: attempt_id.to_string(),
        kind,
    };
    let mut records = Vec::new();
    for row in rows {
        let (raw_kind, raw_hash) = row.map_err(map_sql_error)?;
        let kind = ReleaseEvidenceKind::parse(&raw_kind)
            .ok_or_else(|| corrupt(ReleaseCorruption::EvidenceKind))?;
        let hash: [u8; 32] = raw_hash
            .try_into()
            .map_err(|_| corrupt(ReleaseCorruption::HashEncoding))?;
        records.push(ReleaseEvidenceRecord { kind, hash });
    }
    records.sort_by_key(|record| (record.kind, record.hash));
    Ok(records)
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
            "SELECT gpu_id, ordinal FROM coordinator_release_fact_gpus
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
