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
            Self::Io(msg) => write!(f, "lease store 저장소 I/O 오류: {msg}"),
            Self::LockTimeout => write!(f, "lease store 저장소 락 획득 시간 초과"),
        }
    }
}

impl std::error::Error for LeaseStoreError {}

fn map_sql_error(error: SqlError) -> LeaseStoreError {
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

fn encode_u64(v: u64) -> Vec<u8> {
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

impl CoordinatorLeaseStore {
    /// SQLite 파일을 열거나 만든다. **fail closed** — 이 호출이
    /// 실패하면 호출자는 handshake 를 계속 진행하면 안 된다.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LeaseStoreError> {
        let connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;

        connection
            .execute_batch(
                r#"
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
                    max_total_duration_seconds BLOB NOT NULL
                );
                "#,
            )
            .map_err(map_sql_error)?;

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
        self.connection
            .query_row(SELECT_LEASE_SQL, rusqlite::params![lease_id], row_to_raw)
            .optional()
            .map_err(map_sql_error)?
            .map(RawLeaseRow::into_stored)
            .transpose()
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
    ) -> Result<StoredLease, LeaseStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let existing = transaction
            .query_row(
                SELECT_LEASE_SQL,
                rusqlite::params![candidate.lease_id],
                row_to_raw,
            )
            .optional()
            .map_err(map_sql_error)?
            .map(RawLeaseRow::into_stored)
            .transpose()?;

        if let Some(stored) = existing {
            check_identity_conflict(&stored, candidate)?;
            transaction.commit().map_err(map_sql_error)?;
            return Ok(stored);
        }

        transaction
            .execute(
                "INSERT INTO coordinator_leases(
                    lease_id, job_id, attempt_id, holder_node_id, fence_epoch,
                    expires_at_unix_ms, issuing_coordinator_id, coordinator_term,
                    issued_at_unix_ms, renew_after_unix_ms, max_total_duration_seconds
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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

        transaction.commit().map_err(map_sql_error)?;
        Ok(candidate.clone())
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
        })
    }
}

const SELECT_LEASE_SQL: &str = "SELECT lease_id, job_id, attempt_id, holder_node_id, fence_epoch, \
     expires_at_unix_ms, issuing_coordinator_id, coordinator_term, issued_at_unix_ms, \
     renew_after_unix_ms, max_total_duration_seconds \
     FROM coordinator_leases WHERE lease_id = ?1";

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
    })
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
            let issued = s.get_or_issue(&record).expect("issue");
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
        s.get_or_issue(&record).unwrap();

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
        s.get_or_issue(&record).unwrap();

        let mut conflicting = sample("lease-1");
        conflicting.job_id = "different-job".into();
        let result = s.get_or_issue(&conflicting);
        assert!(matches!(
            result,
            Err(LeaseStoreError::IdentityConflict { field: "job_id", .. })
        ));

        // 원본이 훼손되지 않았다.
        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.job_id, "job-1");
    }

    #[test]
    fn same_identity_reissue_returns_stored_value_not_candidate() {
        let (mut s, _dir) = open_temp();
        let record = sample("lease-1");
        s.get_or_issue(&record).unwrap();

        let mut candidate_with_different_epoch = sample("lease-1");
        candidate_with_different_epoch.fence_epoch = 999; // CLI 가 다른 값을 줘도

        let result = s.get_or_issue(&candidate_with_different_epoch).unwrap();
        assert_eq!(result.fence_epoch, 5, "저장된 값이 우선해야 한다");
    }

    #[test]
    fn u64_max_fields_round_trip_without_truncation() {
        let (mut s, _dir) = open_temp();
        let mut record = sample("lease-1");
        record.fence_epoch = u64::MAX;
        record.expires_at_unix_ms = u64::MAX;
        s.get_or_issue(&record).unwrap();

        let fetched = s.get("lease-1").unwrap().unwrap();
        assert_eq!(fetched.fence_epoch, u64::MAX);
        assert_eq!(fetched.expires_at_unix_ms, u64::MAX);
    }
}
