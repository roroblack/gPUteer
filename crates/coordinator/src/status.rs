//! 풀 상태 한눈에 보기 — `gputeer status` 의 몸통(신뢰망 남은 일 J).
//!
//! ★ **읽기만 한다.** 쓰기 잠금을 잡지 않는다(테이블이 없으면 없는 대로 보여 준다 — 만들지 않는다).
//! ★ 사실만 적는다. "살아 있다/죽었다" 같은 판정은 하지 않는다 — 마지막으로 들은 시각과 표시만 보여 준다
//!   (판정은 스케줄러의 정책 값이 한다, `--silent-after-ms`).

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

fn table_exists(connection: &Connection, name: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            rusqlite::params![name],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .map_err(|e| e.to_string())
}

fn u64_of(bytes: Option<Vec<u8>>) -> Option<u64> {
    bytes.and_then(|bytes| {
        <[u8; 8]>::try_from(bytes.as_slice())
            .ok()
            .map(u64::from_be_bytes)
    })
}

fn ago(now_unix_ms: u64, at: Option<u64>) -> String {
    match at {
        Some(at) if at <= now_unix_ms => format!("{}s-ago", (now_unix_ms - at) / 1000),
        Some(_) => "future".to_string(),
        None => "never".to_string(),
    }
}

/// control DB 의 Job · 노드 · 예약을 사람이 읽는 줄로.
pub fn status_report(control_db: &Path, now_unix_ms: u64) -> Result<String, String> {
    let connection = Connection::open_with_flags(control_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("STATUS: control DB 를 열지 못했다({control_db:?}): {e}"))?;
    let mut out = Vec::new();
    let mut counts: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();

    out.push("JOBS".to_string());
    if table_exists(&connection, "coordinator_jobs")? {
        let ids: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT job_id FROM coordinator_jobs ORDER BY job_id")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        let attempts_exist = table_exists(&connection, "coordinator_attempts")?;
        for job_id in ids {
            let job = crate::job_store::fetch_job(&connection, &job_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("STATUS: {job_id} 가 사라졌다"))?;
            *counts
                .entry(job.state.table_name().to_string())
                .or_default() += 1;
            let latest = if attempts_exist {
                connection
                    .query_row(
                        "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
                         ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
                        rusqlite::params![job_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
            } else {
                None
            };
            let attempt = match latest.as_deref() {
                Some(id) => crate::staging_store::fetch_attempt(&connection, id)
                    .map_err(|e| e.to_string())?,
                None => None,
            };
            let queue_failure = job
                .queue_failure
                .as_ref()
                .map(|failure| format!("{failure:?}"));
            out.push(format!(
                "  {job_id} state={} requeued={} ended={} attempt={} attempt_state={} node={} resume_point={}",
                job.state.table_name(),
                job.requeue_count,
                job.run_terminal
                    .map(|terminal| terminal.trigger().to_string())
                    .or(queue_failure)
                    .unwrap_or_else(|| "-".to_string()),
                attempt
                    .as_ref()
                    .map(|a| a.attempt_id.clone())
                    .unwrap_or_else(|| "-".to_string()),
                attempt
                    .as_ref()
                    .map(|a| a.state.table_name().to_string())
                    .unwrap_or_else(|| "-".to_string()),
                attempt
                    .as_ref()
                    .and_then(|a| a.node_ids.first().cloned())
                    .unwrap_or_else(|| "-".to_string()),
                if job.resume_checkpoint.is_some() {
                    "yes"
                } else {
                    "no"
                },
            ));
        }
    } else {
        out.push("  (Job 테이블이 없다 — 아직 제출된 작업이 없다)".to_string());
    }

    out.push("NODES".to_string());
    if table_exists(&connection, "coordinator_agent_registry")? {
        let nodes: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT node_id FROM coordinator_agent_registry ORDER BY node_id")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        let reservations = table_exists(&connection, "coordinator_node_reservations")?;
        let seen = table_exists(&connection, "coordinator_node_session_seen")?;
        let reclaims = table_exists(&connection, "coordinator_node_reclaims")?;
        for node in nodes {
            let reservation = if reservations {
                crate::staging_store::fetch_node_reservation(&connection, &node)
                    .map_err(|e| e.to_string())?
            } else {
                None
            };
            let last_seen = if seen {
                u64_of(
                    connection
                        .query_row(
                            "SELECT last_seen_unix_ms FROM coordinator_node_session_seen WHERE node_id = ?1",
                            rusqlite::params![node],
                            |row| row.get::<_, Vec<u8>>(0),
                        )
                        .optional()
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            let reclaimed = if reclaims {
                u64_of(
                    connection
                        .query_row(
                            "SELECT reclaimed_at_unix_ms FROM coordinator_node_reclaims WHERE node_id = ?1",
                            rusqlite::params![node],
                            |row| row.get::<_, Vec<u8>>(0),
                        )
                        .optional()
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            out.push(format!(
                "  {node} reserved_by={} reservation_expired={} last_fresh_hello={} owner_reclaimed={}",
                reservation
                    .as_ref()
                    .map(|r| r.attempt_id.clone())
                    .unwrap_or_else(|| "-".to_string()),
                reservation
                    .as_ref()
                    .map(|r| if r.expired_at_unix_ms.is_some() {
                        "yes"
                    } else {
                        "no"
                    })
                    .unwrap_or("-"),
                ago(now_unix_ms, last_seen),
                // 결함 266 — 되찾음 기록이 있으면 "yes"(되찾은 뒤의 FRESH 가 그 기록을 지운다 — 시각을 비교하지 않는다).
                if reclaimed.is_some() {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            ));
        }
    } else {
        out.push("  (노드 테이블이 없다 — import-inventory 를 먼저 한다)".to_string());
    }

    out.push(format!(
        "SUMMARY {}",
        counts
            .iter()
            .map(|(state, count)| format!("{}={count}", state.to_ascii_lowercase()))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    Ok(out.join("\n"))
}
