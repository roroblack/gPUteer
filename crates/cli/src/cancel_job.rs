//! `gputeer cancel-job` — 운영자 취소(실행 알림 계약 v18q · 2026-10-05).
//!
//! ```text
//! gputeer cancel-job --control-db <db> --job <job_id> --operator-statement "<누가 · 왜 취소하나>"
//! ```
//!
//! Job 을 CANCELLED 로 옮긴다. 돌고 있으면 그 시도의 Lease 를 같은 커밋에서 폐기한다 — 노드는 다음 갱신에서 서명된 거부를 받아 멈춘다
//! (즉시 정지는 아니다). 실행 여부 불명 · 보류가 있으면 Job 상태만 옮긴다. **GPU 예약은 풀지 않는다** — 종료 보고 · 정지 확인 · 운영자 해제로만 풀린다.

use std::collections::BTreeMap;

pub fn run(args: &[String]) -> Result<String, String> {
    let mut flags = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let key = &args[index];
        if !key.starts_with("--") {
            return Err(format!("CANCEL_ARGS_REFUSED: 알 수 없는 인자 {key}"));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("CANCEL_ARGS_REFUSED: {key} 에 값이 없다"))?;
        flags.insert(key.clone(), value.clone());
        index += 2;
    }
    for known in flags.keys() {
        if !matches!(known.as_str(), "--control-db" | "--job" | "--operator-statement") {
            return Err(format!("CANCEL_ARGS_REFUSED: 모르는 인자 {known}"));
        }
    }
    let need = |name: &str| {
        flags
            .get(name)
            .cloned()
            .ok_or_else(|| format!("CANCEL_ARGS_REFUSED: {name} 가 필요하다"))
    };
    let control_db = need("--control-db")?;
    let job = need("--job")?;
    let statement = need("--operator-statement")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let cancelled = gputeer_coordinator::job_cancel::cancel_job_by_operator(
        std::path::Path::new(&control_db),
        &job,
        &statement,
        now,
    )?;
    Ok(format!(
        "JOB_CANCELLED job_id={} from={} latest_attempt={} lease_revoked={} deferred={} already={}",
        cancelled.job_id,
        cancelled.from_state,
        cancelled.latest_attempt_id.as_deref().unwrap_or("-"),
        cancelled.lease_revoked,
        cancelled.deferred,
        !cancelled.created
    ))
}
