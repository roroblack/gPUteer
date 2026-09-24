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

/// Job 한 줄 — `gputeer status` 와 대시보드가 같은 값을 쓴다(2026-09-25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobStatus {
    pub job_id: String,
    pub state: String,
    pub requeued: u64,
    /// 끝난 사유(실행 종료 사유 또는 큐 실패). 없으면 `None`.
    pub ended: Option<String>,
    pub attempt_id: Option<String>,
    pub attempt_state: Option<String>,
    pub node: Option<String>,
    pub resume_point: bool,
}

/// 노드 한 줄.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeStatus {
    pub node_id: String,
    pub reserved_by: Option<String>,
    /// 예약이 없으면 `None`.
    pub reservation_expired: Option<bool>,
    /// 마지막 FRESH 인사 시각. 들은 적 없으면 `None` — 판정은 하지 않는다.
    pub last_fresh_hello_unix_ms: Option<u64>,
    pub owner_reclaimed: bool,
}

/// 풀 상태. 테이블이 없으면 `None`(아직 만들지 않았다) — 빈 목록(있는데 비었다)과 구분한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolStatus {
    pub jobs: Option<Vec<JobStatus>>,
    pub nodes: Option<Vec<NodeStatus>>,
    /// Job 상태별 개수(소문자 상태 이름).
    pub summary: std::collections::BTreeMap<String, u64>,
}

/// control DB 의 Job · 노드 · 예약을 사람이 읽는 줄로.
pub fn status_report(control_db: &Path, now_unix_ms: u64) -> Result<String, String> {
    let status = pool_status(control_db, now_unix_ms)?;
    let dash = |value: &Option<String>| value.clone().unwrap_or_else(|| "-".to_string());
    let mut out = vec!["JOBS".to_string()];
    match &status.jobs {
        Some(jobs) => {
            for job in jobs {
                out.push(format!(
                    "  {} state={} requeued={} ended={} attempt={} attempt_state={} node={} resume_point={}",
                    job.job_id,
                    job.state,
                    job.requeued,
                    dash(&job.ended),
                    dash(&job.attempt_id),
                    dash(&job.attempt_state),
                    dash(&job.node),
                    if job.resume_point { "yes" } else { "no" },
                ));
            }
        }
        None => out.push("  (Job 테이블이 없다 — 아직 제출된 작업이 없다)".to_string()),
    }
    out.push("NODES".to_string());
    match &status.nodes {
        Some(nodes) => {
            for node in nodes {
                out.push(format!(
                    "  {} reserved_by={} reservation_expired={} last_fresh_hello={} owner_reclaimed={}",
                    node.node_id,
                    dash(&node.reserved_by),
                    match node.reservation_expired {
                        Some(true) => "yes",
                        Some(false) => "no",
                        None => "-",
                    },
                    ago(now_unix_ms, node.last_fresh_hello_unix_ms),
                    // 결함 266 — 되찾음 기록이 있으면 "yes"(되찾은 뒤의 FRESH 가 그 기록을 지운다 — 시각을 비교하지 않는다).
                    if node.owner_reclaimed { "yes" } else { "no" },
                ));
            }
        }
        None => out.push("  (노드 테이블이 없다 — import-inventory 를 먼저 한다)".to_string()),
    }
    out.push(format!(
        "SUMMARY {}",
        status
            .summary
            .iter()
            .map(|(state, count)| format!("{state}={count}"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    Ok(out.join("\n"))
}

/// control DB 의 Job · 노드 · 예약을 구조로 읽는다. 읽기만 한다.
pub fn pool_status(control_db: &Path, _now_unix_ms: u64) -> Result<PoolStatus, String> {
    let connection = Connection::open_with_flags(control_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("STATUS: control DB 를 열지 못했다({control_db:?}): {e}"))?;
    let mut counts: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();

    let jobs = if table_exists(&connection, "coordinator_jobs")? {
        let mut rows_out = Vec::new();
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
            rows_out.push(JobStatus {
                job_id: job_id.clone(),
                state: job.state.table_name().to_string(),
                requeued: job.requeue_count,
                ended: job
                    .run_terminal
                    .map(|terminal| terminal.trigger().to_string())
                    .or(queue_failure),
                attempt_id: attempt.as_ref().map(|a| a.attempt_id.clone()),
                attempt_state: attempt.as_ref().map(|a| a.state.table_name().to_string()),
                node: attempt.as_ref().and_then(|a| a.node_ids.first().cloned()),
                resume_point: job.resume_checkpoint.is_some(),
            });
        }
        Some(rows_out)
    } else {
        None
    };

    let nodes_out = if table_exists(&connection, "coordinator_agent_registry")? {
        let mut rows_out = Vec::new();
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
            rows_out.push(NodeStatus {
                node_id: node.clone(),
                reserved_by: reservation.as_ref().map(|r| r.attempt_id.clone()),
                reservation_expired: reservation.as_ref().map(|r| r.expired_at_unix_ms.is_some()),
                last_fresh_hello_unix_ms: last_seen,
                owner_reclaimed: reclaimed.is_some(),
            });
        }
        Some(rows_out)
    } else {
        None
    };

    Ok(PoolStatus {
        jobs,
        nodes: nodes_out,
        summary: counts
            .into_iter()
            .map(|(state, count)| (state.to_ascii_lowercase(), count))
            .collect(),
    })
}
