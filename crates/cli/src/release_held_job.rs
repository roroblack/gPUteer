//! `gputeer release-held-job` — 노드가 **알리지 못한 채** 끊겨 자동으로 이어가지 않고 멈춘(부작용이 있다고 선언된) 작업의 보류를
//! **운영자 확인으로** 푼다(실행 알림 계약 v18k §6 (3) · D6 · 계획 조각 7c · 2026-10-03 13:28).
//!
//! ```text
//! gputeer release-held-job --control-db <db> --job <job_id> --operator-statement "<누가 그 PC 에서 무엇을 확인했나>"
//! ```
//!
//! ★ 실행 여부 불명 **알림** 보류(NOTICE)는 풀지 않는다 — 그것은 그 시도의 정지 확인만 푼다. 진술은 되돌리지 않는 기록으로 남는다.
//! 작업이 아직 끝나지 않았으면 다음 장애 이어받기가 이어서 처리하고, 끝난 작업이면 그 시도의 Lease 를 폐기하고 예약을 푼다.

use std::collections::BTreeMap;

pub fn run(args: &[String]) -> Result<String, String> {
    let mut flags = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let key = &args[index];
        if !key.starts_with("--") {
            return Err(format!("RELEASE_HELD_ARGS_REFUSED: 알 수 없는 인자 {key}"));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("RELEASE_HELD_ARGS_REFUSED: {key} 에 값이 없다"))?;
        flags.insert(key.clone(), value.clone());
        index += 2;
    }
    for known in flags.keys() {
        if !matches!(known.as_str(), "--control-db" | "--job" | "--operator-statement") {
            return Err(format!("RELEASE_HELD_ARGS_REFUSED: 모르는 인자 {known}"));
        }
    }
    let need = |name: &str| {
        flags
            .get(name)
            .cloned()
            .ok_or_else(|| format!("RELEASE_HELD_ARGS_REFUSED: {name} 가 필요하다"))
    };
    let control_db = need("--control-db")?;
    let job = need("--job")?;
    let statement = need("--operator-statement")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let released = gputeer_coordinator::failover::release_held_job_by_operator(
        std::path::Path::new(&control_db),
        &job,
        &statement,
        now,
    )?;
    let mut message = format!(
        "HELD_JOB_RELEASED job_id={} attempts={} job_final={}",
        released.job_id,
        released.released_attempts.join(","),
        released.job_final
    );
    for note in released.notes {
        message.push('\n');
        message.push_str(&note);
    }
    Ok(message)
}
