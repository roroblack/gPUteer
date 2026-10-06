//! 재배치 차단 보류 — 실행 알림 계약 v18k §2 "보류는 집합이다"(b3 ③) · §6 · §9 · 계획 조각 6a.
//!
//! ```text
//! 표           coordinator_job_holds(job_id · attempt_id · hold_kind) — 한 Job 에 여러 행. 하나라도 있으면 그 Job 은 새 시도의 대상이 아니다(조각 6b)
//! 종류         NOTICE_RUN_UNKNOWN            서명된 불명 알림으로 생긴다 · 같은 시도의 STOP_CONFIRMED 로만 풀린다(운영자 명령으로 풀지 못한다)
//!              UNREPORTED_SIDE_EFFECT_RISK   D6(조각 7) — 알림 없이 Lease 만 끝난 작업 · 운영자 release-held-job 으로만 풀린다
//!              ★ 같은 시도의 유효한 RUN_UNKNOWN 이 오면 그 시도의 UNREPORTED 행을 NOTICE 행으로 **원자적으로 치환**한다(b4 ④ · 감사 기록)
//! 감사         coordinator_job_hold_events — 설치 · 치환 · 해제마다 한 줄(근거 해시 · 시각). 되돌리지 않는 기록이다
//! ```
//! 이 모듈은 **호출자의 트랜잭션** 안에서만 쓴다(알림 저장 · 시도 전이와 한 커밋 — 계약 §2 "한 트랜잭션").

use rusqlite::{params, Connection, OptionalExtension};

pub const NOTICE_RUN_UNKNOWN: &str = "NOTICE_RUN_UNKNOWN";
pub const UNREPORTED_SIDE_EFFECT_RISK: &str = "UNREPORTED_SIDE_EFFECT_RISK";

pub(crate) fn initialize_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
                CREATE TABLE IF NOT EXISTS coordinator_job_holds (
                    job_id TEXT NOT NULL,
                    attempt_id TEXT NOT NULL,
                    hold_kind TEXT NOT NULL CHECK(hold_kind IN ('NOTICE_RUN_UNKNOWN', 'UNREPORTED_SIDE_EFFECT_RISK')),
                    evidence_hash BLOB CHECK(evidence_hash IS NULL OR length(evidence_hash) = 32),
                    installed_at_unix_ms BLOB NOT NULL CHECK(length(installed_at_unix_ms) = 8),
                    PRIMARY KEY(job_id, attempt_id, hold_kind)
                );
                -- ★ 2026-10-03 13:11 (계약 v18k §6 "풀면 그 시도에 override" · b9 ① · 조각 7b) — release-held-job 이 푼 시도. failover 가 같은 시도로 다시
                --   UNREPORTED 보류를 걸지 않고 이어받기로 간다. 되돌리지 않는 기록이다.
                CREATE TABLE IF NOT EXISTS coordinator_attempt_hold_overrides (
                    attempt_id TEXT PRIMARY KEY,
                    job_id TEXT NOT NULL,
                    operator_statement TEXT NOT NULL,
                    at_unix_ms BLOB NOT NULL CHECK(length(at_unix_ms) = 8)
                );
                CREATE TABLE IF NOT EXISTS coordinator_job_hold_events (
                    job_id TEXT NOT NULL,
                    attempt_id TEXT NOT NULL,
                    hold_kind TEXT NOT NULL,
                    event TEXT NOT NULL CHECK(event IN ('INSTALLED', 'REPLACED', 'RELEASED')),
                    evidence_hash BLOB,
                    detail TEXT NOT NULL,
                    at_unix_ms BLOB NOT NULL CHECK(length(at_unix_ms) = 8)
                );
                "#,
        )
        .map_err(|e| format!("JOB_HOLDS: 표를 만들지 못했다: {e}"))
}

#[allow(clippy::too_many_arguments)]
fn event(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    hold_kind: &str,
    event: &str,
    evidence_hash: Option<&[u8; 32]>,
    detail: &str,
    now_unix_ms: u64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO coordinator_job_hold_events(job_id, attempt_id, hold_kind, event, evidence_hash, detail, at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                job_id,
                attempt_id,
                hold_kind,
                event,
                evidence_hash.map(|h| h.to_vec()),
                detail,
                now_unix_ms.to_be_bytes().to_vec()
            ],
        )
        .map(|_| ())
        .map_err(|e| format!("JOB_HOLDS: 감사 기록을 적지 못했다: {e}"))
}

/// NOTICE 보류를 건다(있으면 그대로 — 멱등). 같은 시도의 UNREPORTED 행이 있으면 NOTICE 로 치환한다(b4 ④). 새로 걸었으면 true.
pub(crate) fn install_notice_hold(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    notice_hash: &[u8; 32],
    now_unix_ms: u64,
) -> Result<bool, String> {
    let replaced = connection
        .execute(
            "DELETE FROM coordinator_job_holds WHERE job_id = ?1 AND attempt_id = ?2 AND hold_kind = ?3",
            params![job_id, attempt_id, UNREPORTED_SIDE_EFFECT_RISK],
        )
        .map_err(|e| format!("JOB_HOLDS: UNREPORTED 행을 치환하지 못했다: {e}"))?;
    if replaced > 0 {
        event(
            connection,
            job_id,
            attempt_id,
            UNREPORTED_SIDE_EFFECT_RISK,
            "REPLACED",
            Some(notice_hash),
            "같은 시도의 서명된 RUN_UNKNOWN — NOTICE 보류로 치환(그 시도의 STOP_CONFIRMED 하나로 풀린다)",
            now_unix_ms,
        )?;
    }
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO coordinator_job_holds(job_id, attempt_id, hold_kind, evidence_hash, installed_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                job_id,
                attempt_id,
                NOTICE_RUN_UNKNOWN,
                notice_hash.to_vec(),
                now_unix_ms.to_be_bytes().to_vec()
            ],
        )
        .map_err(|e| format!("JOB_HOLDS: NOTICE 보류를 걸지 못했다: {e}"))?;
    if inserted == 1 {
        event(
            connection,
            job_id,
            attempt_id,
            NOTICE_RUN_UNKNOWN,
            "INSTALLED",
            Some(notice_hash),
            "서명된 RUN_UNKNOWN",
            now_unix_ms,
        )?;
    }
    Ok(inserted == 1)
}

/// 그 시도의 STOP_CONFIRMED — 그 시도의 NOTICE 행을 지우고, 같은 시도의 UNREPORTED 행은 NOTICE 로 치환한 뒤 지운다(b7 ③ — STOP 이 먼저 온 경우는
/// RUN_UNKNOWN 을 받은 것으로 친다). **다른 시도의 행은 그대로 둔다**(집합 모델). 지운 행 수.
pub(crate) fn release_holds_for_stop(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    stop_hash: &[u8; 32],
    now_unix_ms: u64,
) -> Result<usize, String> {
    let mut released = 0;
    for kind in [UNREPORTED_SIDE_EFFECT_RISK, NOTICE_RUN_UNKNOWN] {
        let removed = connection
            .execute(
                "DELETE FROM coordinator_job_holds WHERE job_id = ?1 AND attempt_id = ?2 AND hold_kind = ?3",
                params![job_id, attempt_id, kind],
            )
            .map_err(|e| format!("JOB_HOLDS: 보류를 풀지 못했다: {e}"))?;
        if removed > 0 {
            if kind == UNREPORTED_SIDE_EFFECT_RISK {
                event(connection, job_id, attempt_id, kind, "REPLACED", Some(stop_hash),
                      "같은 시도의 STOP_CONFIRMED(먼저 옴) — NOTICE 로 치환한 뒤 풀었다", now_unix_ms)?;
            }
            event(connection, job_id, attempt_id, NOTICE_RUN_UNKNOWN, "RELEASED", Some(stop_hash),
                  "같은 시도의 STOP_CONFIRMED", now_unix_ms)?;
            released += removed;
        }
    }
    Ok(released)
}

/// ★ 2026-10-03 13:11 (계약 v18k §9 UNREPORTED_RISK_HELD · D6 · 조각 7b) — 알림 없이 Lease 만 끝난(부작용 있다고 선언된) 시도에 UNREPORTED 보류를 건다.
/// 이미 있으면 그대로(멱등 — 두 번째 failover 가 행을 더 만들지 않는다). 새로 걸었으면 true.
pub(crate) fn install_unreported_hold(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    detail: &str,
    now_unix_ms: u64,
) -> Result<bool, String> {
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO coordinator_job_holds(job_id, attempt_id, hold_kind, evidence_hash, installed_at_unix_ms)
             VALUES (?1, ?2, ?3, NULL, ?4)",
            params![job_id, attempt_id, UNREPORTED_SIDE_EFFECT_RISK, now_unix_ms.to_be_bytes().to_vec()],
        )
        .map_err(|e| format!("JOB_HOLDS: UNREPORTED 보류를 걸지 못했다: {e}"))?;
    if inserted == 1 {
        event(connection, job_id, attempt_id, UNREPORTED_SIDE_EFFECT_RISK, "INSTALLED", None, detail, now_unix_ms)?;
    }
    Ok(inserted == 1)
}

/// ★ 2026-10-03 13:11 (조각 7b) — 그 시도에 운영자 override 가 있는가(release-held-job 이 푼 시도). 표가 없으면 없다(읽기만).
pub fn attempt_has_override(connection: &Connection, attempt_id: &str) -> Result<bool, String> {
    let table: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_attempt_hold_overrides'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("JOB_HOLDS: override 표를 확인하지 못했다: {e}"))?;
    if table.is_none() {
        return Ok(false);
    }
    let present: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM coordinator_attempt_hold_overrides WHERE attempt_id = ?1",
            params![attempt_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("JOB_HOLDS: override 를 읽지 못했다: {e}"))?;
    Ok(present.is_some())
}

/// ★ 2026-10-03 13:28 (계약 v18k §6 b15 ② · 조각 7c) — 그 시도에 UNREPORTED 보류 행이 있는가(표가 없으면 없다 · 읽기만).
pub fn has_unreported_hold(connection: &Connection, job_id: &str, attempt_id: &str) -> Result<bool, String> {
    let table: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_job_holds'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("JOB_HOLDS: 표를 확인하지 못했다: {e}"))?;
    if table.is_none() {
        return Ok(false);
    }
    let present: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM coordinator_job_holds WHERE job_id = ?1 AND attempt_id = ?2 AND hold_kind = ?3",
            params![job_id, attempt_id, UNREPORTED_SIDE_EFFECT_RISK],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("JOB_HOLDS: 보류를 읽지 못했다: {e}"))?;
    Ok(present.is_some())
}

/// ★ 2026-10-03 13:28 (계약 v18k §6 b15 ② · 조각 7c) — 그 시도의 종료를 **관측한** 서명된 보고가 D6 위험("알리지 않은 채 돌고 있을 수 있다")을 닫았다.
/// 그 시도의 UNREPORTED 행을 지운다(감사 — 근거는 report_hash). 지웠으면 true.
pub(crate) fn release_unreported_by_observed_report(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    report_hash: &[u8; 32],
    now_unix_ms: u64,
) -> Result<bool, String> {
    let removed = connection
        .execute(
            "DELETE FROM coordinator_job_holds WHERE job_id = ?1 AND attempt_id = ?2 AND hold_kind = ?3",
            params![job_id, attempt_id, UNREPORTED_SIDE_EFFECT_RISK],
        )
        .map_err(|e| format!("JOB_HOLDS: UNREPORTED 보류를 풀지 못했다: {e}"))?;
    if removed > 0 {
        event(connection, job_id, attempt_id, UNREPORTED_SIDE_EFFECT_RISK, "RELEASED", Some(report_hash),
              "그 시도의 종료를 관측한 서명된 보고", now_unix_ms)?;
    }
    Ok(removed > 0)
}

/// ★ 2026-10-03 13:28 (계약 v18k §6 (3) · §9 · 조각 7c) — 운영자 release-held-job: 그 시도의 UNREPORTED 행을 지우고(감사 — 진술) override 를 적는다
/// (failover 가 같은 시도로 다시 보류하지 않는다). NOTICE 행은 건드리지 않는다(그것은 STOP_CONFIRMED 만 푼다 — 호출자가 먼저 거부한다).
pub(crate) fn release_unreported_by_operator(
    connection: &Connection,
    job_id: &str,
    attempt_id: &str,
    operator_statement: &str,
    now_unix_ms: u64,
) -> Result<bool, String> {
    let removed = connection
        .execute(
            "DELETE FROM coordinator_job_holds WHERE job_id = ?1 AND attempt_id = ?2 AND hold_kind = ?3",
            params![job_id, attempt_id, UNREPORTED_SIDE_EFFECT_RISK],
        )
        .map_err(|e| format!("JOB_HOLDS: UNREPORTED 보류를 풀지 못했다: {e}"))?;
    if removed > 0 {
        event(connection, job_id, attempt_id, UNREPORTED_SIDE_EFFECT_RISK, "RELEASED", None,
              &format!("운영자 release-held-job: {operator_statement}"), now_unix_ms)?;
    }
    connection
        .execute(
            "INSERT OR IGNORE INTO coordinator_attempt_hold_overrides(attempt_id, job_id, operator_statement, at_unix_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![attempt_id, job_id, operator_statement, now_unix_ms.to_be_bytes().to_vec()],
        )
        .map_err(|e| format!("JOB_HOLDS: override 를 적지 못했다: {e}"))?;
    Ok(removed > 0)
}

/// 그 Job 에 재배치 차단 보류가 하나라도 있는가(종류 · 시도 무관). 표가 아직 없으면(알림을 받은 적 없는 DB) 보류도 없다 —
/// 관문은 읽기만 하고 표를 만들지 않는다.
pub fn job_is_held(connection: &Connection, job_id: &str) -> Result<bool, String> {
    let table: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_job_holds'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("JOB_HOLDS: 표를 확인하지 못했다: {e}"))?;
    if table.is_none() {
        return Ok(false);
    }
    let present: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM coordinator_job_holds WHERE job_id = ?1 LIMIT 1",
            params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("JOB_HOLDS: 보류를 읽지 못했다: {e}"))?;
    Ok(present.is_some())
}

/// 보류 행 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobHold {
    pub job_id: String,
    pub attempt_id: String,
    pub hold_kind: String,
}

/// 그 Job 의 보류 행들(시도 · 종류 순).
pub fn holds_for_job(connection: &Connection, job_id: &str) -> Result<Vec<JobHold>, String> {
    let mut statement = connection
        .prepare(
            "SELECT job_id, attempt_id, hold_kind FROM coordinator_job_holds WHERE job_id = ?1 ORDER BY attempt_id, hold_kind",
        )
        .map_err(|e| format!("JOB_HOLDS: 보류를 읽지 못했다: {e}"))?;
    let rows = statement
        .query_map(params![job_id], |row| {
            Ok(JobHold {
                job_id: row.get(0)?,
                attempt_id: row.get(1)?,
                hold_kind: row.get(2)?,
            })
        })
        .map_err(|e| format!("JOB_HOLDS: 보류를 읽지 못했다: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("JOB_HOLDS: 보류를 읽지 못했다: {e}"))
}
