//! SQLite 기반 영속 Lease 저장소 — Coordinator 재시작을 넘어 자신이
//! 발급한 Lease 의 신원(identity)과 epoch 를 기억한다.
//!
//! # 왜 필요한가
//!
//! `crates/runtime-policy/src/durable_lease_scope.rs::DurableFenceWatermark`
//! 는 Agent 쪽에서 재시작을 넘는 epoch 강등 방어를 이미 증명했다
//! (`DoD-14`). 그러나 Coordinator 는 여전히 Lease 상태를 전혀
//! 저장하지 않는다 — 매 프로세스 실행이 CLI 인자로만 상태를 구성하고
//! 끝나면 사라진다(`docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md`
//! 설계 근거).
//!
//! # 왜 `DurableFenceWatermark` 를 재사용하지 않는가
//!
//! `DurableFenceWatermark` 는 `resource -> 최대 epoch` 하나만
//! 저장하는 좁은 타입이다. Coordinator 는 Lease **전체 신원**
//! (`lease_id`·`job_id`·`attempt_id`·`holder_node_id`·`fence_epoch`·
//! `expires_at_unix_ms` 등)을 복원해야 하므로 별도 타입이 필요하다.
//! SQLite 연결·트랜잭션·PRAGMA·에러 매핑 **패턴만** 재사용한다.
//!
//! # 이 저장소가 결정하지 않는 것
//!
//! 새 `lease_id` 를 언제 발급할지, `SUPERSEDED`/`QUARANTINED` 를
//! 언제 내릴지, Lease 를 언제 revoke 할지 — 전부 범위 밖이다. 이
//! 저장소는 **이미 존재하는 Lease 사실을 훼손 없이 기억**만 한다.

use std::path::Path;

use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// 저장된 Lease 레코드. `coordinator_signature` 는 담지 않는다 —
/// 전송 시 매번 재계산되는 서명 필드이지 저장 상태의 핵심 사실이
/// 아니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLease {
    pub lease_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub holder_node_id: String,
    pub fence_epoch: u64,
    pub expires_at_unix_ms: u64,
    pub issuing_coordinator_id: String,
    pub coordinator_term: u64,
    pub issued_at_unix_ms: u64,
    pub renew_after_unix_ms: u64,
    pub max_total_duration_seconds: u64,
    pub revoked_at_unix_ms: Option<u64>,
}

/// Identity and fencing fields carried by an explicit Resume request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeRequestIdentity {
    pub lease_id: String,
    pub node_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub fence_epoch: u64,
}

/// Read-only classification of an explicit Resume request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeDecision {
    Resumed(StoredLease),
    UnknownLease,
    IdentityConflict {
        field: &'static str,
        stored: String,
        requested: String,
    },
    Revoked {
        stored: StoredLease,
    },
    Expired {
        stored: StoredLease,
    },
    Superseded {
        stored: StoredLease,
    },
    EpochAhead {
        stored: StoredLease,
    },
}

impl StoredLease {
    /// `now_unix_ms` 기준으로 `max_total_duration_seconds` 누적 시간을
    /// 초과했는지 판정하는 순수 함수 — 저장소를 바꾸지 않는다.
    /// `renew_existing_within_duration()` 이 트랜잭션 안에서 쓰고,
    /// 호출자가 저장소를 건드리지 않고 미리 판정만 하고 싶을 때도
    /// (예: override 가 저장소를 건드리기 전에 실제 초과가 우선하는지
    /// 확인) 쓸 수 있다.
    ///
    /// `now_unix_ms < issued_at_unix_ms`(clock rollback) 는 경과시간을
    /// 0 으로 취급해 계속 허용하지 않고, **초과로 취급해 fail closed**
    /// 한다.
    pub fn is_max_duration_exceeded(&self, now_unix_ms: u64) -> bool {
        let max_duration_ms = self.max_total_duration_seconds.saturating_mul(1_000);
        match now_unix_ms.checked_sub(self.issued_at_unix_ms) {
            Some(elapsed_ms) => elapsed_ms > max_duration_ms,
            None => true,
        }
    }
}

#[derive(Debug)]
pub enum LeaseStoreError {
    /// 요청한 `lease_id` 가 저장소에 없다 — 갱신 경로에서 새 Lease 를
    /// 발급하지 않는다(그것은 별도 최초 발급 경로의 책임이다).
    NotFound,
    /// 저장된 레코드와 이번에 발급하려는 값의 identity 필드가
    /// 다르다. **재발급 정책이 아니라 이미 존재하는 Lease 사실의
    /// 훼손 방지**다 — 같은 `lease_id` 를 다른 job/attempt/holder 로
    /// 덮어쓰지 않는다.
    IdentityConflict {
        field: &'static str,
        stored: String,
        requested: String,
    },
    /// The lease was durably revoked and cannot be reissued or renewed.
    Revoked { revoked_at_unix_ms: u64 },
    /// The stored Lease had already expired when reissuance or renewal was attempted.
    Expired { expires_at_unix_ms: u64 },
    /// 저장소 I/O 오류 — 공격이 아니라 우리 쪽 문제다.
    Io(String),
    /// `busy_timeout` 안에 락을 얻지 못했다.
    LockTimeout,
}

impl std::fmt::Display for LeaseStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "lease_id 가 저장소에 없다"),
            Self::IdentityConflict {
                field,
                stored,
                requested,
            } => write!(
                f,
                "identity 충돌: {field} 저장값={stored} 요청값={requested}"
            ),
            Self::Revoked { revoked_at_unix_ms } => {
                write!(f, "lease revoked at unix ms: {revoked_at_unix_ms}")
            }
            Self::Expired { expires_at_unix_ms } => {
                write!(f, "lease expired at unix ms: {expires_at_unix_ms}")
            }
            Self::Io(msg) => write!(f, "lease store 저장소 I/O 오류: {msg}"),
            Self::LockTimeout => write!(f, "lease store 저장소 락 획득 시간 초과"),
        }
    }
}

impl std::error::Error for LeaseStoreError {}

pub(crate) fn map_sql_error(error: SqlError) -> LeaseStoreError {
    match error {
        SqlError::SqliteFailure(code, _) => {
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) {
                LeaseStoreError::LockTimeout
            } else {
                LeaseStoreError::Io(code.to_string())
            }
        }
        other => LeaseStoreError::Io(other.to_string()),
    }
}

pub(crate) fn encode_u64(v: u64) -> Vec<u8> {
    v.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8], field: &str) -> Result<u64, LeaseStoreError> {
    let array: [u8; 8] = bytes
        .try_into()
        .map_err(|_| LeaseStoreError::Io(format!("{field} 이 8바이트가 아니다 — 저장소 손상")))?;
    Ok(u64::from_be_bytes(array))
}

pub struct CoordinatorLeaseStore {
    connection: Connection,
}

pub(crate) fn initialize_schema(connection: &mut Connection) -> Result<(), LeaseStoreError> {
    connection
        .execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = DELETE;
            PRAGMA synchronous = FULL;

            CREATE TABLE IF NOT EXISTS coordinator_leases (
                lease_id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                attempt_id TEXT NOT NULL,
                holder_node_id TEXT NOT NULL,
                fence_epoch BLOB NOT NULL,
                expires_at_unix_ms BLOB NOT NULL,
                issuing_coordinator_id TEXT NOT NULL,
                coordinator_term BLOB NOT NULL,
                issued_at_unix_ms BLOB NOT NULL,
                renew_after_unix_ms BLOB NOT NULL,
                max_total_duration_seconds BLOB NOT NULL,
                revoked_at_unix_ms BLOB
            );
            "#,
        )
        .map_err(map_sql_error)?;
    migrate_revoked_at_column(connection)
}

impl CoordinatorLeaseStore {
    /// SQLite 파일을 열거나 만든다. **fail closed** — 이 호출이
    /// 실패하면 호출자는 handshake 를 계속 진행하면 안 된다.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LeaseStoreError> {
        let mut connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;

        initialize_schema(&mut connection)?;

        Ok(Self { connection })
    }

    /// `DurableFenceWatermark::is_durable` 와 같은 관례 — 연결이
    /// 실제로 파일을 가리키는지 물어본다.
    pub fn is_durable(&self) -> bool {
        match self.connection.path() {
            Some(p) => !p.is_empty() && p != ":memory:",
            None => false,
        }
    }

    /// `lease_id` 로 저장된 레코드를 조회한다. 없으면 `Ok(None)`.
    pub fn get(&self, lease_id: &str) -> Result<Option<StoredLease>, LeaseStoreError> {
        fetch_lease(&self.connection, lease_id)
    }

    /// Classify an explicit Resume without issuing or mutating a lease.
    ///
    /// The order is part of the wire contract: identity, revoke, expiry, then
    /// epoch direction. In particular, a revoked-and-expired lease is REVOKED.
    pub fn classify_resume(
        &self,
        request_identity: &ResumeRequestIdentity,
        now_unix_ms: u64,
    ) -> Result<ResumeDecision, LeaseStoreError> {
        let Some(stored) = self.get(&request_identity.lease_id)? else {
            return Ok(ResumeDecision::UnknownLease);
        };

        if stored.holder_node_id != request_identity.node_id {
            return Ok(ResumeDecision::IdentityConflict {
                field: "node_id",
                stored: stored.holder_node_id.clone(),
                requested: request_identity.node_id.clone(),
            });
        }
        if stored.job_id != request_identity.job_id {
            return Ok(ResumeDecision::IdentityConflict {
                field: "job_id",
                stored: stored.job_id.clone(),
                requested: request_identity.job_id.clone(),
            });
        }
        if stored.attempt_id != request_identity.attempt_id {
            return Ok(ResumeDecision::IdentityConflict {
                field: "attempt_id",
                stored: stored.attempt_id.clone(),
                requested: request_identity.attempt_id.clone(),
            });
        }
        if stored.revoked_at_unix_ms.is_some() {
            return Ok(ResumeDecision::Revoked { stored });
        }
        if stored.expires_at_unix_ms <= now_unix_ms {
            return Ok(ResumeDecision::Expired { stored });
        }
        if request_identity.fence_epoch < stored.fence_epoch {
            return Ok(ResumeDecision::Superseded { stored });
        }
        if request_identity.fence_epoch > stored.fence_epoch {
            return Ok(ResumeDecision::EpochAhead { stored });
        }
        Ok(ResumeDecision::Resumed(stored))
    }

    /// 최초 발급 경로 — `lease_id` 가 저장소에 **없을 때만** `candidate`
    /// 를 그대로 삽입하고 반환한다. **있으면** identity 필드
    /// (`job_id`·`attempt_id`·`holder_node_id`·`issuing_coordinator_id`)
    /// 를 대조해 다르면 [`LeaseStoreError::IdentityConflict`] 로
    /// 거부한다(재발급 정책이 아니라 사실 훼손 방지) — 같으면 **저장된
    /// 레코드를 그대로 반환한다**(CLI 값이 아니라 저장값이 우선).
    pub fn get_or_issue(
        &mut self,
        candidate: &StoredLease,
        now_unix_ms: u64,
    ) -> Result<StoredLease, LeaseStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let existing = fetch_lease(&transaction, &candidate.lease_id)?;

        if let Some(stored) = existing {
            check_identity_conflict(&stored, candidate)?;
            if let Some(revoked_at_unix_ms) = stored.revoked_at_unix_ms {
                transaction.commit().map_err(map_sql_error)?;
                return Err(LeaseStoreError::Revoked { revoked_at_unix_ms });
            }
            if stored.expires_at_unix_ms <= now_unix_ms {
                transaction.commit().map_err(map_sql_error)?;
                return Err(LeaseStoreError::Expired {
                    expires_at_unix_ms: stored.expires_at_unix_ms,
                });
            }
            transaction.commit().map_err(map_sql_error)?;
            return Ok(stored);
        }

        insert_lease(&transaction, candidate)?;

        transaction.commit().map_err(map_sql_error)?;
        Ok(candidate.clone())
    }

    /// Durably mark a lease revoked. Repeating the operation preserves the
    /// first revocation timestamp and succeeds idempotently.
    pub fn mark_revoked(
        &mut self,
        lease_id: &str,
        revoked_at_unix_ms: u64,
    ) -> Result<StoredLease, LeaseStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let Some(mut stored) = transaction
            .query_row(SELECT_LEASE_SQL, rusqlite::params![lease_id], row_to_raw)
            .optional()
            .map_err(map_sql_error)?
            .map(RawLeaseRow::into_stored)
            .transpose()?
        else {
            transaction.commit().map_err(map_sql_error)?;
            return Err(LeaseStoreError::NotFound);
        };

        let timestamp = stored.revoked_at_unix_ms.unwrap_or(revoked_at_unix_ms);
        if stored.revoked_at_unix_ms.is_none() {
            transaction
                .execute(
                    "UPDATE coordinator_leases
                     SET revoked_at_unix_ms = ?2
                     WHERE lease_id = ?1",
                    rusqlite::params![lease_id, encode_u64(timestamp)],
                )
                .map_err(map_sql_error)?;
        }

        transaction.commit().map_err(map_sql_error)?;
        stored.revoked_at_unix_ms = Some(timestamp);
        Ok(stored)
    }

    /// 갱신 경로 — `lease_id` 가 저장소에 **없으면**
    /// [`LeaseStoreError::NotFound`] 로 거부한다(새 Lease 를 여기서
    /// 발급하지 않는다). 있으면 identity·`fence_epoch` 는 그대로 두고
    /// `expires_at_unix_ms`/`renew_after_unix_ms` 만 갱신한 뒤, 갱신된
    /// 레코드를 반환한다.
    pub fn renew_existing(
        &mut self,
        lease_id: &str,
        new_expires_at_unix_ms: u64,
        new_renew_after_unix_ms: u64,
    ) -> Result<StoredLease, LeaseStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let existing = transaction
            .query_row(SELECT_LEASE_SQL, rusqlite::params![lease_id], row_to_raw)
            .optional()
            .map_err(map_sql_error)?
            .map(RawLeaseRow::into_stored)
            .transpose()?;

        let Some(mut stored) = existing else {
            // ★ 아무것도 쓰지 않았다 — commit 은 no-op.
            transaction.commit().map_err(map_sql_error)?;
            return Err(LeaseStoreError::NotFound);
        };

        if let Some(revoked_at_unix_ms) = stored.revoked_at_unix_ms {
            transaction.commit().map_err(map_sql_error)?;
            return Err(LeaseStoreError::Revoked { revoked_at_unix_ms });
        }

        transaction
            .execute(
                "UPDATE coordinator_leases
                 SET expires_at_unix_ms = ?2, renew_after_unix_ms = ?3
                 WHERE lease_id = ?1",
                rusqlite::params![
                    lease_id,
                    encode_u64(new_expires_at_unix_ms),
                    encode_u64(new_renew_after_unix_ms),
                ],
            )
            .map_err(map_sql_error)?;

        transaction.commit().map_err(map_sql_error)?;

        stored.expires_at_unix_ms = new_expires_at_unix_ms;
        stored.renew_after_unix_ms = new_renew_after_unix_ms;
        Ok(stored)
    }

    /// `renew_existing` 과 같은 갱신 경로지만, UPDATE 전에
    /// `max_total_duration_seconds` 누적 시간 초과 여부를 판정한다.
    /// 조회→판정→조건부 UPDATE 를 트랜잭션 하나로 묶어 TOCTOU 없이
    /// 판정한다 — 초과했으면 저장소를 전혀 바꾸지 않는다(만료시각을
    /// 연장해버리면 다음 판정 시각이 밀려 정책이 무력화된다).
    ///
    /// `now_unix_ms < stored.issued_at_unix_ms`(clock rollback) 는
    /// 경과시간을 0 으로 취급해 갱신을 계속 허용하지 않고, **초과로
    /// 취급해 fail closed** 한다.
    pub fn renew_existing_within_duration(
        &mut self,
        lease_id: &str,
        now_unix_ms: u64,
        new_expires_at_unix_ms: u64,
        new_renew_after_unix_ms: u64,
    ) -> Result<RenewDecision, LeaseStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let existing = transaction
            .query_row(SELECT_LEASE_SQL, rusqlite::params![lease_id], row_to_raw)
            .optional()
            .map_err(map_sql_error)?
            .map(RawLeaseRow::into_stored)
            .transpose()?;

        let Some(stored) = existing else {
            // ★ 아무것도 쓰지 않았다 — commit 은 no-op.
            transaction.commit().map_err(map_sql_error)?;
            return Err(LeaseStoreError::NotFound);
        };

        if let Some(revoked_at_unix_ms) = stored.revoked_at_unix_ms {
            transaction.commit().map_err(map_sql_error)?;
            return Err(LeaseStoreError::Revoked { revoked_at_unix_ms });
        }

        if stored.expires_at_unix_ms <= now_unix_ms {
            // Commit the read-only transaction and fail closed without updating.
            transaction.commit().map_err(map_sql_error)?;
            return Err(LeaseStoreError::Expired {
                expires_at_unix_ms: stored.expires_at_unix_ms,
            });
        }

        if stored.is_max_duration_exceeded(now_unix_ms) {
            // ★ 판정만 하고 아무것도 쓰지 않는다 — expires_at 을
            //   연장하지 않아야 다음 요청에서도 같은 issued_at 기준으로
            //   다시 초과 판정된다.
            transaction.commit().map_err(map_sql_error)?;
            return Ok(RenewDecision::MaxDurationExceeded(stored));
        }

        transaction
            .execute(
                "UPDATE coordinator_leases
                 SET expires_at_unix_ms = ?2, renew_after_unix_ms = ?3
                 WHERE lease_id = ?1",
                rusqlite::params![
                    lease_id,
                    encode_u64(new_expires_at_unix_ms),
                    encode_u64(new_renew_after_unix_ms),
                ],
            )
            .map_err(map_sql_error)?;

        transaction.commit().map_err(map_sql_error)?;

        let mut renewed = stored;
        renewed.expires_at_unix_ms = new_expires_at_unix_ms;
        renewed.renew_after_unix_ms = new_renew_after_unix_ms;
        Ok(RenewDecision::Renewed(renewed))
    }
}

/// [`CoordinatorLeaseStore::renew_existing_within_duration`] 의 결과 —
/// 저장소가 실제로 만료시각을 갱신했는지, 누적 시간 초과로 갱신하지
/// 않았는지를 호출자에게 구분해 알려준다. 두 variant 모두 판정 시점의
/// `StoredLease` 값을 담는다(`Renewed` 는 새 만료시각 반영, `MaxDurationExceeded`
/// 는 저장된 값 그대로).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenewDecision {
    Renewed(StoredLease),
    MaxDurationExceeded(StoredLease),
}

/// 저장된 레코드와 새로 발급하려는 값의 identity 가 일치하는지
/// 확인한다. `fence_epoch`/`expires_at` 는 이 검사에 들어가지 않는다
/// — 그 값들은 애초에 CLI 가 아니라 저장소가 권위를 갖는다(호출자가
/// 저장된 값을 그대로 쓴다).
fn check_identity_conflict(
    stored: &StoredLease,
    candidate: &StoredLease,
) -> Result<(), LeaseStoreError> {
    if stored.job_id != candidate.job_id {
        return Err(LeaseStoreError::IdentityConflict {
            field: "job_id",
            stored: stored.job_id.clone(),
            requested: candidate.job_id.clone(),
        });
    }
    if stored.attempt_id != candidate.attempt_id {
        return Err(LeaseStoreError::IdentityConflict {
            field: "attempt_id",
            stored: stored.attempt_id.clone(),
            requested: candidate.attempt_id.clone(),
        });
    }
    if stored.holder_node_id != candidate.holder_node_id {
        return Err(LeaseStoreError::IdentityConflict {
            field: "holder_node_id",
            stored: stored.holder_node_id.clone(),
            requested: candidate.holder_node_id.clone(),
        });
    }
    if stored.issuing_coordinator_id != candidate.issuing_coordinator_id {
        return Err(LeaseStoreError::IdentityConflict {
            field: "issuing_coordinator_id",
            stored: stored.issuing_coordinator_id.clone(),
            requested: candidate.issuing_coordinator_id.clone(),
        });
    }
    Ok(())
}

/// `query_row` 는 `rusqlite::Result` 만 반환할 수 있으므로, SQLite
/// 오류(`rusqlite::Error`)와 BLOB 디코딩 오류(`LeaseStoreError`)를
/// 같은 `?` 체인에 섞지 않도록 원시 컬럼만 먼저 뽑는다 — u64 디코딩은
/// [`RawLeaseRow::into_stored`] 에서 별도로 한다.
struct RawLeaseRow {
    lease_id: String,
    job_id: String,
    attempt_id: String,
    holder_node_id: String,
    fence_epoch: Vec<u8>,
    expires_at_unix_ms: Vec<u8>,
    issuing_coordinator_id: String,
    coordinator_term: Vec<u8>,
    issued_at_unix_ms: Vec<u8>,
    renew_after_unix_ms: Vec<u8>,
    max_total_duration_seconds: Vec<u8>,
    revoked_at_unix_ms: Option<Vec<u8>>,
}

impl RawLeaseRow {
    fn into_stored(self) -> Result<StoredLease, LeaseStoreError> {
        Ok(StoredLease {
            lease_id: self.lease_id,
            job_id: self.job_id,
            attempt_id: self.attempt_id,
            holder_node_id: self.holder_node_id,
            fence_epoch: decode_u64(&self.fence_epoch, "fence_epoch")?,
            expires_at_unix_ms: decode_u64(&self.expires_at_unix_ms, "expires_at_unix_ms")?,
            issuing_coordinator_id: self.issuing_coordinator_id,
            coordinator_term: decode_u64(&self.coordinator_term, "coordinator_term")?,
            issued_at_unix_ms: decode_u64(&self.issued_at_unix_ms, "issued_at_unix_ms")?,
            renew_after_unix_ms: decode_u64(&self.renew_after_unix_ms, "renew_after_unix_ms")?,
            max_total_duration_seconds: decode_u64(
                &self.max_total_duration_seconds,
                "max_total_duration_seconds",
            )?,
            revoked_at_unix_ms: self
                .revoked_at_unix_ms
                .as_deref()
                .map(|bytes| decode_u64(bytes, "revoked_at_unix_ms"))
                .transpose()?,
        })
    }
}

const SELECT_LEASE_SQL: &str = "SELECT lease_id, job_id, attempt_id, holder_node_id, fence_epoch, \
     expires_at_unix_ms, issuing_coordinator_id, coordinator_term, issued_at_unix_ms, \
     renew_after_unix_ms, max_total_duration_seconds, revoked_at_unix_ms \
     FROM coordinator_leases WHERE lease_id = ?1";

pub(crate) fn fetch_lease(
    connection: &Connection,
    lease_id: &str,
) -> Result<Option<StoredLease>, LeaseStoreError> {
    connection
        .query_row(SELECT_LEASE_SQL, rusqlite::params![lease_id], row_to_raw)
        .optional()
        .map_err(map_sql_error)?
        .map(RawLeaseRow::into_stored)
        .transpose()
}

pub(crate) fn insert_lease(
    connection: &Connection,
    candidate: &StoredLease,
) -> Result<(), LeaseStoreError> {
    connection
        .execute(
            "INSERT INTO coordinator_leases(
                lease_id, job_id, attempt_id, holder_node_id, fence_epoch,
                expires_at_unix_ms, issuing_coordinator_id, coordinator_term,
                issued_at_unix_ms, renew_after_unix_ms, max_total_duration_seconds,
                revoked_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL)",
            rusqlite::params![
                candidate.lease_id,
                candidate.job_id,
                candidate.attempt_id,
                candidate.holder_node_id,
                encode_u64(candidate.fence_epoch),
                encode_u64(candidate.expires_at_unix_ms),
                candidate.issuing_coordinator_id,
                encode_u64(candidate.coordinator_term),
                encode_u64(candidate.issued_at_unix_ms),
                encode_u64(candidate.renew_after_unix_ms),
                encode_u64(candidate.max_total_duration_seconds),
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

fn row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawLeaseRow> {
    Ok(RawLeaseRow {
        lease_id: row.get(0)?,
        job_id: row.get(1)?,
        attempt_id: row.get(2)?,
        holder_node_id: row.get(3)?,
        fence_epoch: row.get(4)?,
        expires_at_unix_ms: row.get(5)?,
        issuing_coordinator_id: row.get(6)?,
        coordinator_term: row.get(7)?,
        issued_at_unix_ms: row.get(8)?,
        renew_after_unix_ms: row.get(9)?,
        max_total_duration_seconds: row.get(10)?,
        revoked_at_unix_ms: row.get(11)?,
    })
}

fn migrate_revoked_at_column(connection: &mut Connection) -> Result<(), LeaseStoreError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_sql_error)?;

    let has_revoked_at_column = {
        let mut statement = transaction
            .prepare("PRAGMA table_info(coordinator_leases)")
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(map_sql_error)?;
        let mut found = false;
        for row in rows {
            if row.map_err(map_sql_error)? == "revoked_at_unix_ms" {
                found = true;
                break;
            }
        }
        found
    };

    if !has_revoked_at_column {
        transaction
            .execute(
                "ALTER TABLE coordinator_leases ADD COLUMN revoked_at_unix_ms BLOB",
                [],
            )
            .map_err(map_sql_error)?;
    }

    transaction.commit().map_err(map_sql_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(lease_id: &str) -> StoredLease {
        StoredLease {
            lease_id: lease_id.to_string(),
            job_id: "job-1".into(),
            attempt_id: "attempt-1".into(),
            holder_node_id: "agent-1".into(),
            fence_epoch: 5,
            expires_at_unix_ms: 1_000,
            issuing_coordinator_id: "coord-1".into(),
            coordinator_term: 1,
            issued_at_unix_ms: 500,
            renew_after_unix_ms: 800,
            max_total_duration_seconds: 3600,
            revoked_at_unix_ms: None,
        }
    }

    fn open_temp() -> (CoordinatorLeaseStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("leases.sqlite3");
        let s = CoordinatorLeaseStore::open(&path).expect("open");
        (s, dir)
    }

    #[test]
    fn is_durable_reports_true_for_file_backed_db() {
        let (s, _dir) = open_temp();
        assert!(s.is_durable());
    }

    #[test]
    fn open_insert_reopen_get_preserves_all_fields() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("leases.sqlite3");
        let record = sample("lease-1");

        {
            let mut s = CoordinatorLeaseStore::open(&path).expect("open 1");
            let issued = s.get_or_issue(&record, 0).expect("issue");
            assert_eq!(issued, record);
        } // 드롭 — 재시작을 흉내 낸다.

        let s2 = CoordinatorLeaseStore::open(&path).expect("open 2");
        let fetched = s2.get("lease-1").expect("get").expect("present");
        assert_eq!(fetched, record);
    }

    #[test]
    fn renew_existing_updates_only_expiry_fields() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let renewed = s.renew_existing("lease-1", 2_000, 1_800).unwrap();
        assert_eq!(renewed.expires_at_unix_ms, 2_000);
        assert_eq!(renewed.renew_after_unix_ms, 1_800);
        // identity·epoch 는 그대로다.
        assert_eq!(renewed.fence_epoch, record.fence_epoch);
        assert_eq!(renewed.job_id, record.job_id);
    }

    #[test]
    fn renew_missing_lease_id_is_not_found_and_creates_nothing() {
        let (mut s, _dir) = open_temp();
        let result = s.renew_existing("no-such-lease", 2_000, 1_800);
        assert!(matches!(result, Err(LeaseStoreError::NotFound)));
        assert!(s.get("no-such-lease").unwrap().is_none());
    }

    #[test]
    fn conflicting_identity_is_rejected_without_overwrite() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let mut conflicting = sample("lease-1");
        conflicting.job_id = "different-job".into();
        let result = s.get_or_issue(&conflicting, 0);
        assert!(matches!(
            result,
            Err(LeaseStoreError::IdentityConflict {
                field: "job_id",
                ..
            })
        ));

        // 원본이 훼손되지 않았다.
        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.job_id, "job-1");
    }

    // ★ 코덱스 감사(2026-08-19, p116)가 지적 — check_identity_conflict()
    //   는 네 필드(job_id·attempt_id·holder_node_id·issuing_coordinator_id)
    //   를 검사하지만(`:404-437` 부근), 위 테스트는 job_id 충돌만
    //   확인했다. 나머지 세 필드도 각각 확인한다.
    #[test]
    fn conflicting_attempt_id_is_rejected_without_overwrite() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let mut conflicting = sample("lease-1");
        conflicting.attempt_id = "different-attempt".into();
        let result = s.get_or_issue(&conflicting, 0);
        assert!(matches!(
            result,
            Err(LeaseStoreError::IdentityConflict {
                field: "attempt_id",
                ..
            })
        ));

        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.attempt_id, "attempt-1");
    }

    #[test]
    fn conflicting_holder_node_id_is_rejected_without_overwrite() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let mut conflicting = sample("lease-1");
        conflicting.holder_node_id = "different-agent".into();
        let result = s.get_or_issue(&conflicting, 0);
        assert!(matches!(
            result,
            Err(LeaseStoreError::IdentityConflict {
                field: "holder_node_id",
                ..
            })
        ));

        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.holder_node_id, "agent-1");
    }

    #[test]
    fn conflicting_issuing_coordinator_id_is_rejected_without_overwrite() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let mut conflicting = sample("lease-1");
        conflicting.issuing_coordinator_id = "different-coordinator".into();
        let result = s.get_or_issue(&conflicting, 0);
        assert!(matches!(
            result,
            Err(LeaseStoreError::IdentityConflict {
                field: "issuing_coordinator_id",
                ..
            })
        ));

        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.issuing_coordinator_id, "coord-1");
    }

    #[test]
    fn same_identity_reissue_returns_stored_value_not_candidate() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let mut candidate_with_different_epoch = sample("lease-1");
        candidate_with_different_epoch.fence_epoch = 999; // CLI 가 다른 값을 줘도

        let result = s.get_or_issue(&candidate_with_different_epoch, 0).unwrap();
        assert_eq!(result.fence_epoch, 5, "저장된 값이 우선해야 한다");
    }

    #[test]
    fn unexpired_existing_lease_is_reissued() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let result = s.get_or_issue(&record, 999).unwrap();
        assert_eq!(result, record);
    }

    #[test]
    fn expiry_boundary_is_inclusive() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let before_expiry = s.get_or_issue(&record, 999).unwrap();
        assert_eq!(before_expiry, record);

        let result = s.get_or_issue(&record, 1_000);
        assert!(matches!(
            result,
            Err(LeaseStoreError::Expired {
                expires_at_unix_ms: 1_000
            })
        ));
    }

    #[test]
    fn expired_existing_lease_is_rejected_without_overwrite() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record, 0).unwrap();

        let mut candidate = record.clone();
        candidate.fence_epoch = 999;
        let result = s.get_or_issue(&candidate, 1_001);
        assert!(matches!(
            result,
            Err(LeaseStoreError::Expired {
                expires_at_unix_ms: 1_000
            })
        ));
        assert_eq!(
            s.get("lease-1").unwrap().unwrap(),
            record,
            "만료 거부가 저장값을 덮어쓰지 않아야 한다"
        );
    }

    #[test]
    fn renew_within_duration_extends_expiry_when_not_exceeded() {
        let (mut s, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.issued_at_unix_ms = 1_000;
        record.expires_at_unix_ms = 4_000_000;
        record.max_total_duration_seconds = 3_600; // 1시간
        s.get_or_issue(&record, 0).unwrap();

        // issued_at + 30분 경과 — 아직 한도(1시간) 안이다.
        let now = 1_000 + 30 * 60 * 1_000;
        let result = s
            .renew_existing_within_duration("lease-1", now, now + 60_000, now + 30_000)
            .unwrap();

        match result {
            RenewDecision::Renewed(renewed) => {
                assert_eq!(renewed.expires_at_unix_ms, now + 60_000);
                assert_eq!(renewed.renew_after_unix_ms, now + 30_000);
            }
            other => panic!("한도 안인데 Renewed 가 아니다: {other:?}"),
        }
        // 저장소에도 실제로 반영됐다.
        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.expires_at_unix_ms, now + 60_000);
    }

    #[test]
    fn renew_within_duration_refuses_and_does_not_extend_when_exceeded() {
        let (mut s, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.issued_at_unix_ms = 1_000;
        record.expires_at_unix_ms = 4_000_000;
        record.renew_after_unix_ms = 3_000;
        record.max_total_duration_seconds = 3_600; // 1시간
        s.get_or_issue(&record, 0).unwrap();

        // issued_at + 1시간 + 1ms — 정확히 한도를 넘겼다.
        let now = 1_000 + 3_600 * 1_000 + 1;
        let result = s
            .renew_existing_within_duration("lease-1", now, now + 60_000, now + 30_000)
            .unwrap();

        match result {
            RenewDecision::MaxDurationExceeded(stored) => {
                // 만료시각이 원본 그대로다 — 연장되지 않았다.
                assert_eq!(stored.expires_at_unix_ms, 4_000_000);
                assert_eq!(stored.renew_after_unix_ms, 3_000);
            }
            other => panic!("한도를 넘겼는데 MaxDurationExceeded 가 아니다: {other:?}"),
        }
        // 저장소도 실제로 바뀌지 않았다.
        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.expires_at_unix_ms, 4_000_000);
        assert_eq!(fetched.renew_after_unix_ms, 3_000);
    }

    #[test]
    fn renew_within_duration_boundary_is_inclusive_of_the_limit() {
        let (mut s, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.issued_at_unix_ms = 1_000;
        record.expires_at_unix_ms = 4_000_000;
        record.max_total_duration_seconds = 3_600;
        s.get_or_issue(&record, 0).unwrap();

        // issued_at + 정확히 1시간 — 한도와 같다(아직 허용, `>` 만 거부).
        let now = 1_000 + 3_600 * 1_000;
        let result = s
            .renew_existing_within_duration("lease-1", now, now + 60_000, now + 30_000)
            .unwrap();

        assert!(
            matches!(result, RenewDecision::Renewed(_)),
            "경계값(정확히 한도)은 아직 허용해야 한다: {result:?}"
        );
    }

    #[test]
    fn renew_within_duration_treats_clock_rollback_as_exceeded() {
        let (mut s, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.issued_at_unix_ms = 10_000;
        record.expires_at_unix_ms = 20_000;
        record.max_total_duration_seconds = 3_600;
        s.get_or_issue(&record, 0).unwrap();

        // now < issued_at — 시계가 뒤로 갔다. elapsed=0 으로 보고
        // 계속 허용하면 안 된다 — fail closed 로 초과 취급해야 한다.
        let now = 5_000;
        let result = s
            .renew_existing_within_duration("lease-1", now, now + 60_000, now + 30_000)
            .unwrap();

        match result {
            RenewDecision::MaxDurationExceeded(stored) => {
                assert_eq!(stored.expires_at_unix_ms, 20_000, "연장되지 않았다");
            }
            other => panic!("clock rollback 은 fail closed(초과 취급)여야 한다: {other:?}"),
        }
    }

    #[test]
    fn renew_within_duration_missing_lease_id_is_not_found() {
        let (mut s, _dir) = open_temp();
        let result = s.renew_existing_within_duration("no-such-lease", 1_000, 2_000, 1_800);
        assert!(matches!(result, Err(LeaseStoreError::NotFound)));
    }

    #[test]
    fn renew_expiry_at_now_returns_expired_without_update() {
        let (mut s, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.expires_at_unix_ms = 1_000;
        s.get_or_issue(&record, 0).unwrap();

        let result = s.renew_existing_within_duration("lease-1", 1_000, 2_000, 1_500);
        assert!(matches!(
            result,
            Err(LeaseStoreError::Expired {
                expires_at_unix_ms: 1_000
            })
        ));
        assert_eq!(s.get("lease-1").unwrap().unwrap(), record);
    }

    #[test]
    fn renew_expiry_one_millisecond_after_now_succeeds() {
        let (mut s, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.expires_at_unix_ms = 1_001;
        s.get_or_issue(&record, 0).unwrap();

        let result = s
            .renew_existing_within_duration("lease-1", 1_000, 2_000, 1_500)
            .unwrap();
        match result {
            RenewDecision::Renewed(renewed) => {
                assert_eq!(renewed.expires_at_unix_ms, 2_000);
                assert_eq!(renewed.renew_after_unix_ms, 1_500);
            }
            other => panic!("expiry one millisecond in the future must renew: {other:?}"),
        }
        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.expires_at_unix_ms, 2_000);
        assert_eq!(fetched.renew_after_unix_ms, 1_500);
    }

    #[test]
    fn u64_max_fields_round_trip_without_truncation() {
        let (mut s, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.fence_epoch = u64::MAX;
        record.expires_at_unix_ms = u64::MAX;
        s.get_or_issue(&record, 0).unwrap();

        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.fence_epoch, u64::MAX);
        assert_eq!(fetched.expires_at_unix_ms, u64::MAX);
    }

    #[test]
    fn opens_old_schema_and_preserves_existing_lease_as_active() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old-leases.sqlite3");
        let record = sample("old-lease");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE coordinator_leases (
                        lease_id TEXT PRIMARY KEY,
                        job_id TEXT NOT NULL,
                        attempt_id TEXT NOT NULL,
                        holder_node_id TEXT NOT NULL,
                        fence_epoch BLOB NOT NULL,
                        expires_at_unix_ms BLOB NOT NULL,
                        issuing_coordinator_id TEXT NOT NULL,
                        coordinator_term BLOB NOT NULL,
                        issued_at_unix_ms BLOB NOT NULL,
                        renew_after_unix_ms BLOB NOT NULL,
                        max_total_duration_seconds BLOB NOT NULL
                    );",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO coordinator_leases VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    rusqlite::params![
                        record.lease_id,
                        record.job_id,
                        record.attempt_id,
                        record.holder_node_id,
                        encode_u64(record.fence_epoch),
                        encode_u64(record.expires_at_unix_ms),
                        record.issuing_coordinator_id,
                        encode_u64(record.coordinator_term),
                        encode_u64(record.issued_at_unix_ms),
                        encode_u64(record.renew_after_unix_ms),
                        encode_u64(record.max_total_duration_seconds),
                    ],
                )
                .unwrap();
        }

        let store = CoordinatorLeaseStore::open(&path).unwrap();
        let fetched = store.get("old-lease").unwrap().unwrap();
        assert_eq!(fetched.revoked_at_unix_ms, None);
        assert_eq!(fetched.lease_id, "old-lease");
    }

    #[test]
    fn mark_revoked_persists_and_is_idempotent() {
        let (mut store, _dir) = open_temp();
        store.get_or_issue(&sample("lease-1"), 0).unwrap();

        let revoked = store.mark_revoked("lease-1", 1_234).unwrap();
        assert_eq!(revoked.revoked_at_unix_ms, Some(1_234));
        assert_eq!(
            store.get("lease-1").unwrap().unwrap().revoked_at_unix_ms,
            Some(1_234)
        );

        let repeated = store.mark_revoked("lease-1", 9_999).unwrap();
        assert_eq!(repeated.revoked_at_unix_ms, Some(1_234));
        assert_eq!(
            store.get("lease-1").unwrap().unwrap().revoked_at_unix_ms,
            Some(1_234)
        );
    }

    #[test]
    fn revoked_lease_cannot_be_reissued_or_overwritten() {
        let (mut store, _dir) = open_temp();
        let record = sample("lease-1");
        store.get_or_issue(&record, 0).unwrap();
        store.mark_revoked("lease-1", 7_000).unwrap();

        let mut candidate = record.clone();
        candidate.fence_epoch = 999;
        candidate.expires_at_unix_ms = 999_999;
        let result = store.get_or_issue(&candidate, 0);
        assert!(matches!(
            result,
            Err(LeaseStoreError::Revoked {
                revoked_at_unix_ms: 7_000
            })
        ));

        let fetched = store.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.fence_epoch, record.fence_epoch);
        assert_eq!(fetched.expires_at_unix_ms, record.expires_at_unix_ms);
        assert_eq!(fetched.revoked_at_unix_ms, Some(7_000));
    }

    #[test]
    fn revoked_lease_cannot_be_renewed() {
        let (mut store, _dir) = open_temp();
        store.get_or_issue(&sample("lease-1"), 0).unwrap();
        store.mark_revoked("lease-1", 8_000).unwrap();

        let result = store.renew_existing_within_duration("lease-1", 1_000, 2_000, 1_500);
        assert!(matches!(
            result,
            Err(LeaseStoreError::Revoked {
                revoked_at_unix_ms: 8_000
            })
        ));
        assert_eq!(
            store.get("lease-1").unwrap().unwrap().expires_at_unix_ms,
            1_000
        );
    }

    #[test]
    fn classify_resume_obeys_identity_revoke_expiry_and_epoch_order() {
        let (mut store, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.expires_at_unix_ms = 10_000;
        record.fence_epoch = 7;
        store.get_or_issue(&record, 0).unwrap();

        let identity = |epoch| ResumeRequestIdentity {
            lease_id: "lease-1".into(),
            node_id: "agent-1".into(),
            job_id: "job-1".into(),
            attempt_id: "attempt-1".into(),
            fence_epoch: epoch,
        };

        assert!(matches!(
            store.classify_resume(&identity(7), 9_999).unwrap(),
            ResumeDecision::Resumed(ref returned) if returned.expires_at_unix_ms == 10_000
        ));
        assert!(matches!(
            store.classify_resume(&identity(6), 9_999).unwrap(),
            ResumeDecision::Superseded { .. }
        ));
        assert!(matches!(
            store.classify_resume(&identity(8), 9_999).unwrap(),
            ResumeDecision::EpochAhead { .. }
        ));

        let mut wrong_node = identity(7);
        wrong_node.node_id = "other-node".into();
        assert!(matches!(
            store.classify_resume(&wrong_node, 9_999).unwrap(),
            ResumeDecision::IdentityConflict {
                field: "node_id",
                ..
            }
        ));
        let mut missing = identity(7);
        missing.lease_id = "missing".into();
        assert!(matches!(
            store.classify_resume(&missing, 9_999).unwrap(),
            ResumeDecision::UnknownLease
        ));

        store.mark_revoked("lease-1", 9_000).unwrap();
        assert!(
            matches!(
                store.classify_resume(&identity(7), 10_000).unwrap(),
                ResumeDecision::Revoked { .. }
            ),
            "revoke must win over the expiry boundary"
        );

        let (mut expired_store, _expired_dir) = open_temp();
        let mut expired = sample("expired");
        expired.expires_at_unix_ms = 10_000;
        expired_store.get_or_issue(&expired, 0).unwrap();
        let expired_identity = ResumeRequestIdentity {
            lease_id: "expired".into(),
            node_id: expired.holder_node_id.clone(),
            job_id: expired.job_id.clone(),
            attempt_id: expired.attempt_id.clone(),
            fence_epoch: expired.fence_epoch,
        };
        assert!(matches!(
            expired_store
                .classify_resume(&expired_identity, 10_000)
                .unwrap(),
            ResumeDecision::Expired { .. }
        ));
    }
}
