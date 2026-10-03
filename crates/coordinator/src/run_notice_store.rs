//! 실행 알림(`AttemptRunNotice`) 받기 · 저장 · 정지 확인 처리 — **한 트랜잭션**(실행 알림 계약 v18k §2 · §3 · 계획 조각 4d).
//!
//! ```text
//! 저장        coordinator_attempt_run_notices — 기본키 (attempt_id, node_id, sequence) · 추가 전용.
//!             칸: canonical 서명 입력(sig_input) · 서명 · notice_hash(= BLAKE3(sig_input) — ACK 가 echo) · 해석한 필드
//! 같은 바이트  sig_input 과 서명이 둘 다 같으면 멱등(created=false · 효과 없음) · 같은 키에 다르면 거부
//! 순번 규칙    새 STOP_CONFIRMED 의 번호는 그 시도의 저장된 RUN_UNKNOWN 중 가장 큰 번호보다 커야 한다(없으면 비교하지 않는다)
//! STOP 처리   1 알림 저장 · 2 시도 → FAILED(STOP_CONFIRMED) · 3 Job 이 갈 곳(최신 시도일 때만) · 4 Lease 폐기 · 5 예약 해제 · 6 ACK 재료
//! ```
//!
//! ★ 이 조각이 **하지 않는 것**(계약의 나머지 조각):
//! ```text
//! RUN_UNKNOWN 의 효과   시도 → RUN_UNKNOWN · Job 보류 표식 · 보류 해제 — 조각 6. 지금은 저장만 하고 `RunUnknownStoredOnly` 로 알린다
//! ACK 서명 · 세션      REPORT 세션의 FrameType 19 분기 — 조각 5 이후. 여기서는 ACK 에 실을 값만 돌려준다
//! 부르는 곳            REPORT 세션의 FrameType 19 분기(조각 5f — `answer_run_notice`) · `--accept-run-notice` 는 ADR-034 강제 코드 전까지 기동 거부 —
//!                      **격리 시험만**(계획 §3 · §4)
//! ```

use std::path::Path;

use gputeer_protocol::{
    attempt_state::AttemptState,
    canonical::blake3_256,
    pb,
    signing::{signing_input, Verified},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::job_store::{self, StopConfirmedJobEffect, StoredJob};
use crate::reservation_release::{
    self, KeyDirectoryProvenance, ReleaseOutcome, ReservationReleaseError, RuntimeStopProof,
};
use crate::staging_store;

/// 알림 하나를 받은 결과 — ACK 에 실을 값과 한 일.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunNoticeAccepted {
    /// BLAKE3(sig_input) — ACK 의 notice_hash.
    pub notice_hash: [u8; 32],
    pub kind: pb::RunNoticeKind,
    pub sequence: u64,
    /// 첫 저장이면 true, 같은 바이트의 재전송이면 false(ACK 의 created).
    pub created: bool,
    pub effect: RunNoticeEffect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunNoticeEffect {
    /// 같은 바이트의 재전송 — 아무것도 다시 하지 않았다.
    Duplicate,
    /// RUN_UNKNOWN — 저장만 했다. 시도 · Job · 보류 효과는 조각 6 이 붙인다.
    RunUnknownStoredOnly,
    /// STOP_CONFIRMED 를 처리했다.
    StopProcessed(StopProcessed),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopProcessed {
    /// 시도 상태가 바뀌기 전 값.
    pub attempt_state_before: AttemptState,
    /// 이번에 시도를 FAILED(STOP_CONFIRMED)로 옮겼나(이미 종료였으면 false — 증거 · 자원만 다뤘다).
    pub attempt_closed: bool,
    /// 알림의 시도가 그 Job 의 최신 시도(가장 높은 fence)였나. 아니면 늦은 도착이라 Job 은 건드리지 않았다(§7).
    pub latest_attempt: bool,
    /// Job 이 간 곳 — 최신 시도이고 시도를 이번에 닫았을 때만 있다.
    pub job: Option<StopConfirmedJobEffect>,
    pub release: ReleaseOutcome,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunNoticeError {
    KeyDirectoryNotVerified,
    Rule(gputeer_protocol::attempt_run_notice_rules::RunNoticeRuleError),
    InvalidInput(&'static str),
    AttemptNotFound { attempt_id: String },
    /// 알림의 job · node · fence 가 durable 시도와 다르다 · 또는 여러 노드 시도다(§2 단일 노드).
    AttemptIdentityMismatch { attempt_id: String },
    /// 같은 (시도 · 노드 · 번호) 에 다른 바이트가 이미 있다.
    SequenceConflict { sequence: u64 },
    /// STOP_CONFIRMED 의 번호가 그 시도의 저장된 RUN_UNKNOWN 번호보다 크지 않다(b3 ⑧).
    StopNotAfterUnknown { sequence: u64, latest_unknown: u64 },
    Release(ReservationReleaseError),
    /// 이어갈 체크포인트를 찾지 못했다(찾기 자체의 오류 — "없음" 이 아니다).
    ResumeLookup(String),
    Storage(String),
}

/// 이어갈 지점 찾기 — 장애 이어받기와 같은 탐색을 호출자가 넣는다(공유 저장소 · 생산자 키는 이 모듈이 모른다).
/// `Ok(None)` 은 "검증된 체크포인트가 없다" 이다.
pub type ResumeFinder<'a> = dyn FnMut(&Connection, &StoredJob) -> Result<Option<Vec<u8>>, String> + 'a;

pub(crate) fn initialize_schema(connection: &Connection) -> Result<(), RunNoticeError> {
    connection
        .execute_batch(
            r#"
                CREATE TABLE IF NOT EXISTS coordinator_attempt_run_notices (
                    attempt_id TEXT NOT NULL,
                    node_id TEXT NOT NULL,
                    sequence BLOB NOT NULL CHECK(length(sequence) = 8),
                    job_id TEXT NOT NULL,
                    fence_epoch BLOB NOT NULL CHECK(length(fence_epoch) = 8),
                    kind TEXT NOT NULL CHECK(kind IN ('RUN_UNKNOWN', 'STOP_CONFIRMED')),
                    sig_input BLOB NOT NULL,
                    node_signature BLOB NOT NULL,
                    notice_hash BLOB NOT NULL CHECK(length(notice_hash) = 32),
                    received_at_unix_ms BLOB NOT NULL CHECK(length(received_at_unix_ms) = 8),
                    PRIMARY KEY(attempt_id, node_id, sequence)
                );
                "#,
        )
        .map_err(storage)?;
    reservation_release::initialize_release_schema(connection).map_err(RunNoticeError::Release)
}

pub struct CoordinatorRunNoticeStore {
    connection: Connection,
}

impl CoordinatorRunNoticeStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RunNoticeError> {
        let path = path.as_ref();
        job_store::CoordinatorJobStore::open(path).map_err(|e| storage_text(e.to_string()))?;
        staging_store::CoordinatorStagingStore::open(path).map_err(|e| storage_text(format!("{e:?}")))?;
        let connection = Connection::open(path).map_err(storage)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(1))
            .map_err(storage)?;
        initialize_schema(&connection)?;
        Ok(Self { connection })
    }

    /// 한 알림을 `BEGIN IMMEDIATE` 하나에서 받는다 — 오류면 아무것도 남지 않는다.
    pub fn accept(
        &mut self,
        verified: &Verified<pb::AttemptRunNotice>,
        runtime_stop: RuntimeStopProof,
        key_directory: KeyDirectoryProvenance,
        resume: &mut ResumeFinder<'_>,
        now_unix_ms: u64,
    ) -> Result<RunNoticeAccepted, RunNoticeError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let accepted = accept_within(
            &transaction,
            verified,
            runtime_stop,
            key_directory,
            resume,
            now_unix_ms,
        )?;
        transaction.commit().map_err(storage)?;
        Ok(accepted)
    }
}

/// ★ 호출자의 트랜잭션 안에서 한 알림을 받는다. 커밋은 호출자가 한다 — 오류를 돌려주면 호출자는 되돌려야 한다.
///
/// 검증 순서(계약 §2): 키 디렉터리 진술 → 조합 규칙 → 그 시도가 있고 · 단일 노드이고 · 그 노드 · job · fence 인가 → 같은 바이트/충돌 →
/// 순번 규칙 → 저장 → (STOP 이면) 처리.
pub fn accept_within(
    transaction: &Connection,
    verified: &Verified<pb::AttemptRunNotice>,
    runtime_stop: RuntimeStopProof,
    key_directory: KeyDirectoryProvenance,
    resume: &mut ResumeFinder<'_>,
    now_unix_ms: u64,
) -> Result<RunNoticeAccepted, RunNoticeError> {
    if key_directory == KeyDirectoryProvenance::Unverified {
        return Err(RunNoticeError::KeyDirectoryNotVerified);
    }
    // 어떤 필드도 Verified 관문을 지나기 전에 읽지 않는다.
    let notice = verified.get();
    gputeer_protocol::attempt_run_notice_rules::validate_attempt_run_notice(notice)
        .map_err(RunNoticeError::Rule)?;
    let kind = pb::RunNoticeKind::try_from(notice.kind)
        .map_err(|_| RunNoticeError::InvalidInput("kind"))?;
    let kind_text = match kind {
        pb::RunNoticeKind::RunUnknown => "RUN_UNKNOWN",
        pb::RunNoticeKind::StopConfirmed => "STOP_CONFIRMED",
        _ => return Err(RunNoticeError::InvalidInput("kind")),
    };
    // 정지 확인의 등급은 아무것도 쓰기 전에 본다(해제 경로도 다시 본다) — 신뢰망 전용 자기보고 등급만 받는다.
    if kind == pb::RunNoticeKind::StopConfirmed && runtime_stop != RuntimeStopProof::NodeConfirmedStop {
        return Err(RunNoticeError::Release(if runtime_stop == RuntimeStopProof::NotProvenYet {
            ReservationReleaseError::RuntimeStopNotProven
        } else {
            ReservationReleaseError::StopProofGradeMismatch
        }));
    }
    for (value, field) in [
        (&notice.job_id, "job_id"),
        (&notice.attempt_id, "attempt_id"),
        (&notice.node_id, "node_id"),
    ] {
        if value.trim().is_empty() {
            return Err(RunNoticeError::InvalidInput(field));
        }
    }
    let attempt = staging_store::fetch_attempt(transaction, &notice.attempt_id)
        .map_err(|e| storage_text(format!("{e:?}")))?
        .ok_or_else(|| RunNoticeError::AttemptNotFound {
            attempt_id: notice.attempt_id.clone(),
        })?;
    if attempt.job_id != notice.job_id
        || attempt.node_ids != [notice.node_id.clone()]
        || attempt.fence_epoch != notice.fence_epoch
    {
        return Err(RunNoticeError::AttemptIdentityMismatch {
            attempt_id: notice.attempt_id.clone(),
        });
    }

    let sig_input = signing_input(notice);
    let notice_hash = blake3_256(&sig_input);
    let sequence_key = notice.sequence.to_be_bytes().to_vec();
    let existing: Option<(Vec<u8>, Vec<u8>)> = transaction
        .query_row(
            "SELECT sig_input, node_signature FROM coordinator_attempt_run_notices
             WHERE attempt_id = ?1 AND node_id = ?2 AND sequence = ?3",
            rusqlite::params![notice.attempt_id, notice.node_id, sequence_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((stored_input, stored_signature)) = existing {
        if stored_input == sig_input && stored_signature == notice.node_signature {
            return Ok(RunNoticeAccepted {
                notice_hash,
                kind,
                sequence: notice.sequence,
                created: false,
                effect: RunNoticeEffect::Duplicate,
            });
        }
        return Err(RunNoticeError::SequenceConflict {
            sequence: notice.sequence,
        });
    }
    if kind == pb::RunNoticeKind::StopConfirmed {
        if let Some(latest_unknown) = latest_sequence(transaction, &notice.attempt_id, "RUN_UNKNOWN")? {
            if notice.sequence <= latest_unknown {
                return Err(RunNoticeError::StopNotAfterUnknown {
                    sequence: notice.sequence,
                    latest_unknown,
                });
            }
        }
    }
    transaction
        .execute(
            "INSERT INTO coordinator_attempt_run_notices(
                attempt_id, node_id, sequence, job_id, fence_epoch, kind,
                sig_input, node_signature, notice_hash, received_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                notice.attempt_id,
                notice.node_id,
                sequence_key,
                notice.job_id,
                notice.fence_epoch.to_be_bytes().to_vec(),
                kind_text,
                sig_input,
                notice.node_signature,
                notice_hash.as_slice(),
                now_unix_ms.to_be_bytes().to_vec(),
            ],
        )
        .map_err(storage)?;

    let effect = match kind {
        pb::RunNoticeKind::StopConfirmed => RunNoticeEffect::StopProcessed(process_stop(
            transaction,
            verified,
            &attempt,
            runtime_stop,
            key_directory,
            resume,
            now_unix_ms,
        )?),
        _ => RunNoticeEffect::RunUnknownStoredOnly,
    };
    Ok(RunNoticeAccepted {
        notice_hash,
        kind,
        sequence: notice.sequence,
        created: true,
        effect,
    })
}

/// §3 — STOP_CONFIRMED 처리(알림은 이미 저장했다).
fn process_stop(
    transaction: &Connection,
    verified: &Verified<pb::AttemptRunNotice>,
    attempt: &staging_store::StoredAttempt,
    runtime_stop: RuntimeStopProof,
    key_directory: KeyDirectoryProvenance,
    resume: &mut ResumeFinder<'_>,
    now_unix_ms: u64,
) -> Result<StopProcessed, RunNoticeError> {
    // 2) 시도 종결. 불명을 거치지 않고 STOP 이 먼저 와도(§2 "먼저 옴") 불명으로 적고 곧바로 닫는다 — 규범 경로를 메모리에서 대조하고 마지막만 쓴다.
    //    CREATED 는 서명된 알림이 Grant 를 받아들였다는 증거다(b16 ①) — GRANT_ACCEPTED 부터 밟는다.
    //    진입 행이 없는 상태(이미 종료 · STALE 등)는 시도 상태를 바꾸지 않는다(b10 ③) — 자원만 다룬다.
    let path: &[AttemptState] = match attempt.state {
        AttemptState::RunUnknown => &[AttemptState::Failed],
        AttemptState::Starting | AttemptState::Running | AttemptState::Paused => {
            &[AttemptState::RunUnknown, AttemptState::Failed]
        }
        AttemptState::Created => &[
            AttemptState::Starting,
            AttemptState::RunUnknown,
            AttemptState::Failed,
        ],
        _ => &[],
    };
    let attempt_closed = !path.is_empty();
    if attempt_closed {
        staging_store::transition_attempt_state_along(
            transaction,
            &attempt.attempt_id,
            attempt.state,
            path,
        )
        .map_err(|e| storage_text(format!("{e:?}")))?;
    }

    // 3) Job — 그 Job 의 최신 시도(가장 높은 fence)의 알림일 때만. 늦은 도착이면 Job 은 건드리지 않는다(§2 · §7).
    let latest: Option<String> = transaction
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![attempt.job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?;
    let latest_attempt = latest.as_deref() == Some(attempt.attempt_id.as_str());
    let job = if latest_attempt && attempt_closed {
        let stored = job_store::fetch_job(transaction, &attempt.job_id)
            .map_err(|e| storage_text(e.to_string()))?
            .ok_or_else(|| storage_text(format!("시도의 Job {} 이 없다", attempt.job_id)))?;
        let resume_point = if matches!(
            stored.state,
            job_store::JobState::Running | job_store::JobState::Staging
        ) {
            resume(transaction, &stored).map_err(RunNoticeError::ResumeLookup)?
        } else {
            None
        };
        Some(
            job_store::follow_stop_confirmed(transaction, &stored, resume_point, now_unix_ms)
                .map_err(|e| storage_text(e.to_string()))?,
        )
    } else {
        None
    };

    // 4) Lease 폐기(이미 폐기돼 있으면 그대로) — 이후 그 Lease 의 갱신은 같은 트랜잭션 재확인으로 REVOKED 다.
    crate::lease_store::revoke_within(transaction, &attempt.lease_id, now_unix_ms)
        .map_err(|e| storage_text(format!("{e:?}")))?;

    // 5) 예약 해제 — 예약이 없거나 다른 시도의 것이면 아무것도 지우지 않는다(NothingToRelease · 나머지는 그대로 커밋).
    let release = reservation_release::release_for_stop_confirmed_within(
        transaction,
        verified,
        runtime_stop,
        key_directory,
        now_unix_ms,
    )
    .map_err(RunNoticeError::Release)?;

    Ok(StopProcessed {
        attempt_state_before: attempt.state,
        attempt_closed,
        latest_attempt,
        job,
        release,
    })
}

/// 그 시도의 저장된 알림 중 그 종류의 가장 큰 번호. 번호는 8바이트 BE 라 BLOB 비교(memcmp)가 수 비교와 같다.
fn latest_sequence(
    connection: &Connection,
    attempt_id: &str,
    kind: &str,
) -> Result<Option<u64>, RunNoticeError> {
    let raw: Option<Vec<u8>> = connection
        .query_row(
            "SELECT MAX(sequence) FROM coordinator_attempt_run_notices WHERE attempt_id = ?1 AND kind = ?2",
            rusqlite::params![attempt_id, kind],
            |row| row.get(0),
        )
        .map_err(storage)?;
    raw.map(|bytes| {
        let array: [u8; 8] = bytes
            .try_into()
            .map_err(|_| storage_text("저장된 sequence 가 8바이트가 아니다 — 저장소 손상".to_string()))?;
        Ok(u64::from_be_bytes(array))
    })
    .transpose()
}

fn storage(error: rusqlite::Error) -> RunNoticeError {
    RunNoticeError::Storage(error.to_string())
}

fn storage_text(message: String) -> RunNoticeError {
    RunNoticeError::Storage(message)
}

/// ★ 2026-10-03 11:51 (실행 알림 계획 조각 5f · 계약 §1 `AttemptRunNoticeAck` · §2) — REPORT 세션이 받은 알림 하나에 답할 재료.
pub struct RunNoticeAnswerContext<'a> {
    /// 저장 · 처리를 할 control DB(`--grant-from-control-db`).
    pub control_db: &'a Path,
    pub coordinator_id: &'a str,
    /// 이 연결의 노드(Hello 로 확인한 node_id) — 알림의 서명자 · node_id 가 같아야 한다.
    pub expected_node_id: &'a str,
    /// 노드 키를 어디서 왔나 — 풀의 등록 키(권위 있는 디렉터리)만 정지 확인 해제를 받는다.
    pub key_directory: KeyDirectoryProvenance,
    /// 이어갈 지점 찾기(장애 이어받기와 같은 공유 저장소 · 생산자 키).
    pub resume_policy: crate::failover::FailoverPolicy,
    /// REPORT Hello 의 nonce — ACK 가 echo 한다(ShortLived replay nonce).
    pub session_nonce: &'a [u8],
    pub now_unix_ms: u64,
}

/// 답하지 못한 까닭 — 들어온 알림이 문제인가(그 연결만 거부) · 저장소 장애인가(fail-closed).
#[derive(Debug, PartialEq, Eq)]
pub enum RunNoticeAnswerError {
    Rejected(String),
    Storage(String),
}

/// ★ 조각 5f — 검증된 알림을 한 트랜잭션에 받고(저장 · STOP 처리), 서명된 `AttemptRunNoticeAck` 를 만든다. 보내기는 부르는 쪽(세션)이 한다.
pub fn answer_run_notice(
    context: &RunNoticeAnswerContext<'_>,
    signing_key: &gputeer_crypto::SigningKey,
    verified: &Verified<pb::AttemptRunNotice>,
) -> Result<(pb::AttemptRunNoticeAck, RunNoticeAccepted), RunNoticeAnswerError> {
    let notice = verified.get();
    if notice.node_id != context.expected_node_id {
        return Err(RunNoticeAnswerError::Rejected(format!(
            "RUN_NOTICE_REJECTED: node_id 가 이 연결의 노드가 아니다(기대 {})",
            context.expected_node_id
        )));
    }
    let mut store = CoordinatorRunNoticeStore::open(context.control_db)
        .map_err(|e| RunNoticeAnswerError::Storage(format!("{e:?}")))?;
    let mut notes = Vec::new();
    let accepted = {
        let policy = &context.resume_policy;
        let now = context.now_unix_ms;
        let notes = &mut notes;
        let mut finder = |connection: &Connection, job: &StoredJob| {
            crate::failover::resume_body_for(connection, policy, &job.job_id, now, notes)
        };
        store.accept(
            verified,
            RuntimeStopProof::NodeConfirmedStop,
            context.key_directory,
            &mut finder,
            context.now_unix_ms,
        )
    }
    .map_err(|error| match error {
        RunNoticeError::Storage(why) | RunNoticeError::ResumeLookup(why) => RunNoticeAnswerError::Storage(why),
        other => RunNoticeAnswerError::Rejected(format!("RUN_NOTICE_REJECTED: {other:?}")),
    })?;
    for note in notes {
        println!("RUN_NOTICE_RESUME_NOTE {note}");
    }
    let mut ack = pb::AttemptRunNoticeAck {
        schema_version: 1,
        job_id: notice.job_id.clone(),
        attempt_id: notice.attempt_id.clone(),
        node_id: notice.node_id.clone(),
        fence_epoch: notice.fence_epoch,
        notice_hash: Some(pb::Digest {
            algo: 1, // HASH_ALGORITHM_BLAKE3_256
            value: accepted.notice_hash.to_vec(),
        }),
        kind: accepted.kind as i32,
        sequence: accepted.sequence,
        created: accepted.created,
        coordinator_id: context.coordinator_id.to_string(),
        issued_at_unix_ms: context.now_unix_ms,
        session_nonce: context.session_nonce.to_vec(),
        ..Default::default()
    };
    ack.coordinator_signature = gputeer_crypto::sign(signing_key, &ack).to_vec();
    Ok((ack, accepted))
}
