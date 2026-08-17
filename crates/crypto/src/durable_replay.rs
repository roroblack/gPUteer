//! SQLite 기반 영속 replay 저장소.
//!
//! `signing.md` §10의 검사·기록·commit 계약을 구현한다.
//!
//! ## 저장소 선택
//!
//! 직접 파일을 만들면 Windows의 rename 제약을 피할 수 있지만,
//! 프로세스 간 락, 부분 레코드 복구, GC 상태의 원자성, 로그 압축을
//! 이 모듈이 모두 책임져야 한다.
//!
//! 이 구현은 `rusqlite`의 bundled SQLite를 사용한다.
//! SQLite의 rollback journal과 `synchronous=FULL`을 사용하므로
//! 기록 성공 응답은 SQLite commit 성공 뒤에만 반환된다.
//!
//! ## 동시성
//!
//! 같은 파일을 여러 프로세스가 열 수 있다.
//! 각 기록·GC 작업은 `BEGIN IMMEDIATE` 트랜잭션으로 직렬화된다.
//! SQLite가 지정된 시간 안에 락을 얻지 못하면 `LockTimeout`을 반환한다.
//!
//! ## Windows
//!
//! 체크포인트의 write-once rename 방식을 이 저장소에 복사하지 않는다.
//! SQLite rollback journal은 데이터 파일을 열린 상태에서 교체하는 방식이
//! 아니므로 Windows의 열린 파일 rename 제약을 이 경로에 도입하지 않는다.
//! 디렉터리 fsync를 보장한다고 쓰지 않는다. SQLite가 제공하는 파일
//! flush와 commit 결과만 이 저장소의 성공 조건으로 사용한다.

use std::convert::TryFrom;
use std::path::Path;
use std::time::Duration;

use gputeer_protocol::canonical::Domain;
use gputeer_protocol::signing::{ReplayDecision, ReplayGuard, ReplayStoreError, NONCE_LEN};
use rusqlite::{
    params, Connection, Error as SqlError, ErrorCode, OptionalExtension, Transaction,
    TransactionBehavior,
};

use crate::replay::{
    DEFAULT_CAPACITY, DEFAULT_PER_SIGNER_CAPACITY, MAX_GC_ADVANCE_MS,
};

const BUSY_TIMEOUT: Duration = Duration::from_secs(1);
const META_LAST_SEEN_MS: &str = "last_seen_ms";
const META_CLOCK_ROLLBACKS: &str = "clock_rollbacks";
const META_CLOCK_JUMPS: &str = "clock_jumps";

fn map_sql_error(error: SqlError) -> ReplayStoreError {
    match error {
        SqlError::SqliteFailure(code, _) => {
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) {
                ReplayStoreError::LockTimeout
            } else {
                ReplayStoreError::Io(code.to_string())
            }
        }
        other => ReplayStoreError::Io(other.to_string()),
    }
}

fn to_sql_i64(value: u64, field: &str) -> Result<i64, ReplayStoreError> {
    i64::try_from(value).map_err(|_| {
        ReplayStoreError::Io(format!(
            "{field} 값이 SQLite INTEGER 범위를 넘었다"
        ))
    })
}

fn from_sql_i64(value: i64, field: &str) -> Result<u64, ReplayStoreError> {
    u64::try_from(value).map_err(|_| {
        ReplayStoreError::Io(format!(
            "{field} 값이 음수여서 저장소 상태가 잘못되었다"
        ))
    })
}

fn count_to_usize(value: i64, field: &str) -> Result<usize, ReplayStoreError> {
    usize::try_from(value).map_err(|_| {
        ReplayStoreError::Io(format!(
            "{field} 개수가 현재 플랫폼의 usize 범위를 넘었다"
        ))
    })
}

fn read_meta(
    transaction: &Transaction<'_>,
    key: &str,
) -> Result<u64, ReplayStoreError> {
    let value: i64 = transaction
        .query_row(
            "SELECT value FROM replay_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .map_err(map_sql_error)?;

    from_sql_i64(value, key)
}

fn write_meta(
    transaction: &Transaction<'_>,
    key: &str,
    value: u64,
) -> Result<(), ReplayStoreError> {
    let value = to_sql_i64(value, key)?;
    let changed = transaction
        .execute(
            "UPDATE replay_meta SET value = ?1 WHERE key = ?2",
            params![value, key],
        )
        .map_err(map_sql_error)?;

    if changed != 1 {
        return Err(ReplayStoreError::Io(format!(
            "replay 메타데이터 {key} 갱신 대상이 정확히 하나가 아니다"
        )));
    }

    Ok(())
}

fn increment_meta(
    transaction: &Transaction<'_>,
    key: &str,
) -> Result<(), ReplayStoreError> {
    let changed = transaction
        .execute(
            "UPDATE replay_meta SET value = value + 1 WHERE key = ?1",
            params![key],
        )
        .map_err(map_sql_error)?;

    if changed != 1 {
        return Err(ReplayStoreError::Io(format!(
            "replay 메타데이터 {key} 증가 대상이 정확히 하나가 아니다"
        )));
    }

    Ok(())
}

/// SQLite rollback journal을 사용하는 영속 replay guard.
pub struct DurableReplayGuard {
    connection: Connection,
    capacity: usize,
    per_signer_capacity: usize,
}

impl DurableReplayGuard {
    /// 기본 상한으로 저장소를 연다.
    /// ★ 이 guard 는 **재시작을 견딘다.**
    ///
    /// [`InMemoryReplayGuard::is_durable`](crate::InMemoryReplayGuard::is_durable) 은
    /// `false` 를 반환한다. 호출자가 둘을 구분할 수 있어야
    /// "재시작 직후 replay 창이 열린다" 를 알 수 있다.
    ///
    /// ★ 2026-08-17 추가. 초안에는 이 메서드가 **없었다** —
    ///   영속 저장소를 만들어 놓고 그 사실을 알릴 방법이 없었다.
    ///   `is_effective()` 는 "guard 가 동작하는가" 이지 "영속인가" 가 아니다.
    ///
    /// # 왜 `true` 를 그냥 반환하지 않는가
    ///
    /// 처음에는 `true` 를 하드코딩했다. 그러면 이 값은 **주장**이지 사실이 아니다.
    /// 실제로 `Connection::open_in_memory()` 로 바꾸는 뮤테이션에서도
    /// `is_durable()` 은 계속 `true` 였다 — **거짓말을 했다.**
    ///
    /// 지금은 연결이 실제로 파일을 가리키는지 **물어본다.**
    /// 메모리 DB 는 경로가 없거나 `:memory:` 다.
    pub fn is_durable(&self) -> bool {
        match self.connection.path() {
            Some(p) => !p.is_empty() && p != ":memory:",
            None => false,
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ReplayStoreError> {
        Self::open_with_capacities(
            path,
            DEFAULT_CAPACITY,
            DEFAULT_PER_SIGNER_CAPACITY,
        )
    }

    /// 전역 상한과 서명자별 상한을 지정하여 저장소를 연다.
    pub fn open_with_capacities(
        path: impl AsRef<Path>,
        capacity: usize,
        per_signer_capacity: usize,
    ) -> Result<Self, ReplayStoreError> {
        if capacity == 0 {
            return Err(ReplayStoreError::Io(
                "replay 전역 상한은 0일 수 없다".into(),
            ));
        }

        if per_signer_capacity == 0 {
            return Err(ReplayStoreError::Io(
                "replay 서명자별 상한은 0일 수 없다".into(),
            ));
        }

        let connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;

        // rollback journal은 Windows에서 열린 데이터 파일을 rename으로
        // 교체하지 않고도 commit과 복구를 제공하므로 이 저장소의 요구에 맞다.
        connection
            .execute_batch(
                r#"
                PRAGMA journal_mode = DELETE;
                PRAGMA synchronous = FULL;
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS replay_entries (
                    sender_device_id TEXT NOT NULL,
                    domain_tag       TEXT NOT NULL,
                    nonce            BLOB NOT NULL,
                    retain_until_ms  INTEGER NOT NULL,
                    PRIMARY KEY (sender_device_id, domain_tag, nonce)
                ) WITHOUT ROWID;

                CREATE INDEX IF NOT EXISTS replay_entries_signer_idx
                    ON replay_entries(sender_device_id);

                CREATE TABLE IF NOT EXISTS replay_meta (
                    key   TEXT PRIMARY KEY,
                    value INTEGER NOT NULL
                );

                INSERT OR IGNORE INTO replay_meta(key, value)
                    VALUES ('last_seen_ms', 0);

                INSERT OR IGNORE INTO replay_meta(key, value)
                    VALUES ('clock_rollbacks', 0);

                INSERT OR IGNORE INTO replay_meta(key, value)
                    VALUES ('clock_jumps', 0);
                "#,
            )
            .map_err(map_sql_error)?;

        Ok(Self {
            connection,
            capacity,
            per_signer_capacity,
        })
    }

    /// 전역 상한을 반환한다.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 서명자별 상한을 반환한다.
    pub fn per_signer_capacity(&self) -> usize {
        self.per_signer_capacity
    }

    /// 현재 저장된 항목 수를 반환한다.
    pub fn entry_count(&self) -> Result<usize, ReplayStoreError> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM replay_entries", [], |row| {
                row.get(0)
            })
            .map_err(map_sql_error)?;

        count_to_usize(count, "replay 항목")
    }

    /// 저장소가 비었는지 반환한다.
    pub fn is_empty(&self) -> Result<bool, ReplayStoreError> {
        Ok(self.entry_count()? == 0)
    }

    /// 특정 서명자가 사용 중인 항목 수를 반환한다.
    pub fn signer_usage(
        &self,
        signer_id: &str,
    ) -> Result<usize, ReplayStoreError> {
        let count: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*)
                 FROM replay_entries
                 WHERE sender_device_id = ?1",
                params![signer_id],
                |row| row.get(0),
            )
            .map_err(map_sql_error)?;

        count_to_usize(count, "서명자 replay 항목")
    }

    /// 마지막 GC 시각을 반환한다.
    pub fn last_seen_ms(&self) -> Result<u64, ReplayStoreError> {
        self.connection
            .query_row(
                "SELECT value FROM replay_meta WHERE key = ?1",
                params![META_LAST_SEEN_MS],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sql_error)
            .and_then(|value| from_sql_i64(value, META_LAST_SEEN_MS))
    }

    /// 시계 되감김 횟수를 반환한다.
    pub fn clock_rollbacks(&self) -> Result<u64, ReplayStoreError> {
        self.connection
            .query_row(
                "SELECT value FROM replay_meta WHERE key = ?1",
                params![META_CLOCK_ROLLBACKS],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sql_error)
            .and_then(|value| from_sql_i64(value, META_CLOCK_ROLLBACKS))
    }

    /// 과도한 미래 시각을 잘라낸 횟수를 반환한다.
    pub fn clock_jumps(&self) -> Result<u64, ReplayStoreError> {
        self.connection
            .query_row(
                "SELECT value FROM replay_meta WHERE key = ?1",
                params![META_CLOCK_JUMPS],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sql_error)
            .and_then(|value| from_sql_i64(value, META_CLOCK_JUMPS))
    }

    /// 만료 항목을 GC한다.
    ///
    /// 시계가 되감기면 아무 항목도 삭제하지 않는다.
    /// 미래로 과도하게 튄 시각은 `MAX_GC_ADVANCE_MS`까지만 반영한다.
    /// 삭제와 메타데이터 갱신은 하나의 트랜잭션으로 확정한다.
    pub fn gc(&mut self, now_unix_ms: u64) -> Result<usize, ReplayStoreError> {
        // ★ 2026-08-17 (독립 검수) — clamp **뒤에** 변환한다.
        //   전에는 여기서 먼저 i64 로 바꿔서 `gc(u64::MAX)` 가 `Io` 오류였다.
        //   메모리 구현은 그 값을 clock jump 로 보고 clamp 한다.
        //   같은 입력에 다른 답을 내면 계약이 아니다.
        //   (`now_sql` 은 어디에도 쓰이지 않았다 — 변환 위치가 잘못돼 있었다.)
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let last_seen = read_meta(&transaction, META_LAST_SEEN_MS)?;

        if now_unix_ms < last_seen {
            increment_meta(&transaction, META_CLOCK_ROLLBACKS)?;
            transaction.commit().map_err(map_sql_error)?;
            return Ok(0);
        }

        let effective = if last_seen == 0 {
            now_unix_ms
        } else {
            let bound = last_seen.saturating_add(MAX_GC_ADVANCE_MS);

            if now_unix_ms > bound {
                increment_meta(&transaction, META_CLOCK_JUMPS)?;
                bound
            } else {
                now_unix_ms
            }
        };

        // clamp 를 거친 값이므로 정상 범위지만, 첫 GC(last_seen == 0)는
        // 호출자가 준 값을 그대로 쓴다. 방어적으로 잘라 넣는다.
        let effective_sql = i64::try_from(effective).unwrap_or(i64::MAX);

        let removed = transaction
            .execute(
                "DELETE FROM replay_entries
                 WHERE retain_until_ms <= ?1",
                params![effective_sql],
            )
            .map_err(map_sql_error)?;

        write_meta(&transaction, META_LAST_SEEN_MS, effective)?;

        transaction.commit().map_err(map_sql_error)?;

        Ok(removed)
    }
}

impl ReplayGuard for DurableReplayGuard {
    fn check_and_record(
        &mut self,
        signer_id: &str,
        domain: Domain,
        nonce: &[u8],
        retain_until_ms: u64,
    ) -> Result<ReplayDecision, ReplayStoreError> {
        // ★ 2026-08-17 분류 정정 (독립 검수).
        //   전에는 `Io` 였다 — `Io` 는 "우리 쪽 디스크 장애" 라는 뜻이다.
        //   길이가 틀린 nonce 는 디스크 장애가 아니라 **입력 위반**이다.
        if nonce.len() != NONCE_LEN {
            return Err(ReplayStoreError::InvalidNonce { len: nonce.len() });
        }

        // ★ 2026-08-17 (독립 검수) — u64::MAX 를 오류로 만들지 않는다.
        //   메모리 구현은 그 값을 받아들이는데 영속 구현만 거부하면
        //   **같은 입력에 다른 답**이 된다.
        //   i64 범위로 잘라 넣는다 — 서기 292,277,026,596년이면 충분하다.
        let retain_until_ms = i64::try_from(retain_until_ms).unwrap_or(i64::MAX);
        let domain_tag = domain.as_str();

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let duplicate: Option<i64> = transaction
            .query_row(
                "SELECT 1
                 FROM replay_entries
                 WHERE sender_device_id = ?1
                   AND domain_tag = ?2
                   AND nonce = ?3",
                params![signer_id, domain_tag, nonce],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?;

        if duplicate.is_some() {
            transaction.commit().map_err(map_sql_error)?;
            return Ok(ReplayDecision::Duplicate);
        }

        let signer_count: i64 = transaction
            .query_row(
                "SELECT COUNT(*)
                 FROM replay_entries
                 WHERE sender_device_id = ?1",
                params![signer_id],
                |row| row.get(0),
            )
            .map_err(map_sql_error)?;

        let signer_count = count_to_usize(signer_count, "서명자 replay 항목")?;

        if signer_count >= self.per_signer_capacity {
            return Err(ReplayStoreError::SignerQuotaExceeded {
                signer_id: signer_id.to_string(),
                quota: self.per_signer_capacity,
            });
        }

        let total_count: i64 = transaction
            .query_row("SELECT COUNT(*) FROM replay_entries", [], |row| {
                row.get(0)
            })
            .map_err(map_sql_error)?;

        let total_count = count_to_usize(total_count, "전체 replay 항목")?;

        if total_count >= self.capacity {
            return Err(ReplayStoreError::CacheFull);
        }

        transaction
            .execute(
                "INSERT INTO replay_entries(
                    sender_device_id,
                    domain_tag,
                    nonce,
                    retain_until_ms
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![signer_id, domain_tag, nonce, retain_until_ms],
            )
            .map_err(map_sql_error)?;

        // Fresh는 INSERT가 아니라 commit 성공 뒤에만 반환한다.
        transaction.commit().map_err(map_sql_error)?;

        Ok(ReplayDecision::Fresh)
    }

    fn is_effective(&self) -> bool {
        true
    }
}
