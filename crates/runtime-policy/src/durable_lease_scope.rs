//! SQLite 기반 영속 `FenceWatermark` — `lease_scope.rs` 의
//! `restart_resets_watermark_and_lets_stale_epoch_through` 가 고정한
//! 위험을 메운다.
//!
//! # 왜 `DurableReplayGuard` 를 재사용하지 않는가
//!
//! `crates/crypto/src/durable_replay.rs` 는 이미 SQLite 기반 영속
//! 저장소를 갖고 있지만, **의미가 다르다.**
//!
//! ```text
//! replay guard    nonce 재사용을 막는다 — 같은 값이 두 번 오면 거부(Duplicate)
//! fence watermark 낮은 epoch 만 막는다 — 같은 값은 통과해야 한다(갱신은
//!                 같은 epoch 를 유지한다, same_epoch_reuse_is_allowed_by_design)
//! ```
//!
//! epoch 를 nonce 저장소에 억지로 넣으면 "같은 값 재사용 허용" 계약이
//! "Duplicate 는 거부" 의미와 충돌한다. 게다가 replay guard 의 GC 는
//! 오래된 항목을 **지운다** — fence watermark 를 지우면 그 resource 의
//! stale epoch 가 다시 통과하게 된다(§10 replay 와 정반대 요구).
//! 그래서 SQLite 연결·트랜잭션·에러 매핑 **패턴만** 재사용하고,
//! 별도 타입으로 만든다(`docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md`
//! 설계 근거).

use std::path::Path;

use rusqlite::{Connection, Error as SqlError, ErrorCode, OptionalExtension, TransactionBehavior};

use crate::lease_scope::LeaseScopeViolation;

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// [`crate::FenceWatermark`] 과 같은 계약을 SQLite 파일에 영속한다.
///
/// ★ 정책 거부([`DurableFenceError::Stale`])와 저장소 장애
/// ([`DurableFenceError::Io`]/[`DurableFenceError::LockTimeout`])를
/// 분리한다 — "공격을 막았다"와 "우리 쪽 디스크/락 문제다"를 같은
/// 값으로 섞으면, 저장소 장애를 정책 위반으로 오인해 정당한 갱신을
/// 계속 거부하거나, 그 반대로 장애를 "그냥 stale 이었다"고 조용히
/// 넘길 수 있다.
pub struct DurableFenceWatermark {
    connection: Connection,
}

/// [`DurableFenceWatermark::check_and_advance`] 의 오류.
#[derive(Debug)]
pub enum DurableFenceError {
    /// 정책상 정상 거부 — 들어온 epoch 가 기록된 watermark 보다 낮다.
    Stale(LeaseScopeViolation),
    /// 저장소 I/O 오류 — 공격이 아니라 우리 쪽 문제다.
    Io(String),
    /// `busy_timeout` 안에 락을 얻지 못했다.
    LockTimeout,
}

impl std::fmt::Display for DurableFenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stale(violation) => write!(f, "{violation}"),
            Self::Io(msg) => write!(f, "fence watermark 저장소 I/O 오류: {msg}"),
            Self::LockTimeout => write!(f, "fence watermark 저장소 락 획득 시간 초과"),
        }
    }
}

impl std::error::Error for DurableFenceError {}

fn map_sql_error(error: SqlError) -> DurableFenceError {
    match error {
        SqlError::SqliteFailure(code, _) => {
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) {
                DurableFenceError::LockTimeout
            } else {
                DurableFenceError::Io(code.to_string())
            }
        }
        other => DurableFenceError::Io(other.to_string()),
    }
}

impl DurableFenceWatermark {
    /// SQLite 파일을 열거나 만든다. 파일이 없으면 생성한다.
    ///
    /// ★ **fail closed** — 이 호출이 실패하면 호출자는 handshake 를
    ///   계속 진행하면 안 된다(`crates/agent/src/lib.rs` 가 ACK/갱신
    ///   전에 이 호출을 둔다). 열 수 없는 저장소로 epoch 를 검증하는
    ///   척하지 않는다.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DurableFenceError> {
        let connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;

        // rollback journal — DurableReplayGuard 와 같은 이유로 Windows
        // 에서 열린 데이터 파일을 rename 으로 교체하지 않고도 commit·
        // 복구를 제공한다(durable_replay.rs 모듈 문서 참조).
        connection
            .execute_batch(
                r#"
                PRAGMA journal_mode = DELETE;
                PRAGMA synchronous = FULL;

                CREATE TABLE IF NOT EXISTS fence_watermarks (
                    resource  TEXT PRIMARY KEY,
                    watermark BLOB NOT NULL
                ) WITHOUT ROWID;
                "#,
            )
            .map_err(map_sql_error)?;

        Ok(Self { connection })
    }

    /// `crate::FenceWatermark::is_durable` 과 같은 관례 — 연결이 실제로
    /// 파일을 가리키는지 물어본다(주장이 아니라 사실을 반환한다).
    pub fn is_durable(&self) -> bool {
        match self.connection.path() {
            Some(p) => !p.is_empty() && p != ":memory:",
            None => false,
        }
    }

    /// `resource` 에 대해 들어온 `epoch` 가 유효한가.
    ///
    /// `crate::FenceWatermark::check_and_advance` 와 **정확히 같은
    /// 계약**이다 — 낮은 epoch 만 거부하고, 같은 epoch 는 통과시키며
    /// (lease 갱신이 같은 epoch 를 유지하기 때문), 통과하면 watermark
    /// 를 그 값으로 올린다. 조회·비교·기록을 **같은 트랜잭션** 안에서
    /// 하지 않으면 두 프로세스가 동시에 오래된 값을 읽는 창이 생긴다.
    pub fn check_and_advance(
        &mut self,
        resource: &str,
        epoch: u64,
    ) -> Result<(), DurableFenceError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        let current: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT watermark FROM fence_watermarks WHERE resource = ?1",
                rusqlite::params![resource],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?;

        let watermark = match current {
            Some(bytes) => decode_watermark(&bytes)?,
            None => 0,
        };

        if epoch < watermark {
            // ★ 아무것도 쓰지 않았다 — commit 은 no-op 이지만, 락을 오래
            //   붙들지 않도록 명시적으로 끝낸다(DurableReplayGuard 의
            //   Duplicate 경로와 같은 패턴).
            transaction.commit().map_err(map_sql_error)?;
            return Err(DurableFenceError::Stale(LeaseScopeViolation::StaleEpoch {
                resource: resource.to_string(),
                incoming: epoch,
                watermark,
            }));
        }

        transaction
            .execute(
                "INSERT INTO fence_watermarks(resource, watermark) VALUES (?1, ?2)
                 ON CONFLICT(resource) DO UPDATE SET watermark = excluded.watermark",
                rusqlite::params![resource, encode_watermark(epoch)],
            )
            .map_err(map_sql_error)?;

        transaction.commit().map_err(map_sql_error)?;
        Ok(())
    }
}

/// `u64` 를 8바이트 big-endian `BLOB` 으로 인코딩한다.
///
/// ★ SQLite `INTEGER` 는 부호 있는 64비트라 `i64::MAX` 를 넘는 epoch
///   는 저장할 수 없다 — `BLOB` 로 저장해 `u64` 전체 범위를 그대로
///   보존한다(`DurableReplayGuard` 가 `retain_until_ms` 를 `i64` 로
///   잘라 넣는 것과 다른 선택 — epoch 는 안전 경계 값이라 절삭을
///   허용하지 않는다).
fn encode_watermark(epoch: u64) -> Vec<u8> {
    epoch.to_be_bytes().to_vec()
}

fn decode_watermark(bytes: &[u8]) -> Result<u64, DurableFenceError> {
    let array: [u8; 8] = bytes.try_into().map_err(|_| {
        DurableFenceError::Io(format!(
            "fence_watermarks.watermark 이 8바이트가 아니다 (실제 {}바이트) — 저장소 손상",
            bytes.len()
        ))
    })?;
    Ok(u64::from_be_bytes(array))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_temp() -> (DurableFenceWatermark, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("fence.sqlite3");
        let w = DurableFenceWatermark::open(&path).expect("open");
        (w, dir)
    }

    #[test]
    fn is_durable_reports_true_for_file_backed_db() {
        let (w, _dir) = open_temp();
        assert!(w.is_durable());
    }

    #[test]
    fn stale_lease_is_rejected() {
        let (mut w, _dir) = open_temp();
        w.check_and_advance("cas://jobs/1", 5).unwrap();

        let r = w.check_and_advance("cas://jobs/1", 3);
        assert!(
            matches!(
                r,
                Err(DurableFenceError::Stale(LeaseScopeViolation::StaleEpoch {
                    incoming: 3,
                    watermark: 5,
                    ..
                }))
            ),
            "{r:?}"
        );
    }

    #[test]
    fn same_epoch_reuse_is_allowed_by_design() {
        let (mut w, _dir) = open_temp();
        w.check_and_advance("cas://jobs/1", 5).unwrap();
        assert!(
            w.check_and_advance("cas://jobs/1", 5).is_ok(),
            "같은 epoch 재사용(lease 갱신)이 거부됐다 — 의도와 다르다"
        );
    }

    #[test]
    fn higher_epoch_advances_watermark() {
        let (mut w, _dir) = open_temp();
        w.check_and_advance("cas://jobs/1", 5).unwrap();
        w.check_and_advance("cas://jobs/1", 10).unwrap();

        let r = w.check_and_advance("cas://jobs/1", 7);
        assert!(matches!(
            r,
            Err(DurableFenceError::Stale(LeaseScopeViolation::StaleEpoch {
                watermark: 10,
                ..
            }))
        ));
    }

    #[test]
    fn watermark_is_per_resource() {
        let (mut w, _dir) = open_temp();
        w.check_and_advance("cas://jobs/1", 100).unwrap();
        assert!(w.check_and_advance("cas://jobs/2", 1).is_ok());
    }

    /// ★ 비공허성의 핵심 — 재시작을 실제 프로세스 재시작 없이도
    /// 재현한다. 새 `DurableFenceWatermark` 인스턴스로 **같은 파일**을
    /// 다시 열면, 이전 인스턴스가 기록한 watermark 가 그대로 보인다.
    /// 이것이 `restart_resets_watermark_and_lets_stale_epoch_through`
    /// (`lease_scope.rs`)가 메모리 버전에서 고정한 위험의 **반대
    /// 증명**이다.
    #[test]
    fn reopening_the_same_file_preserves_the_watermark() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("fence.sqlite3");

        {
            let mut w = DurableFenceWatermark::open(&path).expect("open 1");
            w.check_and_advance("cas://jobs/1", 10).unwrap();
        } // 첫 인스턴스를 드롭한다 — 재시작을 흉내 낸다.

        let mut w2 = DurableFenceWatermark::open(&path).expect("open 2");
        let result = w2.check_and_advance("cas://jobs/1", 3);

        assert!(
            matches!(
                result,
                Err(DurableFenceError::Stale(LeaseScopeViolation::StaleEpoch {
                    incoming: 3,
                    watermark: 10,
                    ..
                }))
            ),
            "재오픈 후에도 watermark 가 보존돼야 한다: {result:?}"
        );
    }

    #[test]
    fn u64_max_epoch_round_trips_without_truncation() {
        let (mut w, _dir) = open_temp();
        w.check_and_advance("cas://jobs/1", u64::MAX).unwrap();

        let r = w.check_and_advance("cas://jobs/1", u64::MAX - 1);
        assert!(
            matches!(
                r,
                Err(DurableFenceError::Stale(LeaseScopeViolation::StaleEpoch {
                    watermark: u64::MAX,
                    ..
                }))
            ),
            "u64::MAX 가 BLOB 인코딩에서 잘렸다: {r:?}"
        );
    }
}
