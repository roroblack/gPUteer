//! `gputeer release-lost-node` — 끊긴 노드의 옛 예약을 **운영자 확인으로** 푼다(신뢰망 남은 일 G).
//!
//! ```text
//! gputeer release-lost-node --control-db <db> --node <node_id> --operator-statement "<누가 무엇을 확인했나>"
//! ```
//!
//! 장애 이어받기는 옛 예약을 지우지 않는다 — 그 노드가 살아서 옛 작업을 계속 돌리고 있을 수 있다(Coordinator 는 증명할 수
//! 없다). 운영자가 그 PC 를 보고 멈췄음을 확인한 뒤 이 명령으로 푼다. 진술은 되돌리지 않는 기록으로 남는다.
//! ★ 살아 있는 시도의 예약은 거부한다 — 노드를 잘못 짚어도 도는 작업이 풀리지 않는다.

use std::collections::BTreeMap;

pub fn run(args: &[String]) -> Result<String, String> {
    let mut flags = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let key = &args[index];
        if !key.starts_with("--") {
            return Err(format!("RELEASE_ARGS_REFUSED: 알 수 없는 인자 {key}"));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("RELEASE_ARGS_REFUSED: {key} 에 값이 없다"))?;
        flags.insert(key.clone(), value.clone());
        index += 2;
    }
    for known in flags.keys() {
        if !matches!(
            known.as_str(),
            "--control-db" | "--node" | "--operator-statement"
        ) {
            return Err(format!("RELEASE_ARGS_REFUSED: 모르는 인자 {known}"));
        }
    }
    let need = |name: &str| {
        flags
            .get(name)
            .cloned()
            .ok_or_else(|| format!("RELEASE_ARGS_REFUSED: {name} 가 필요하다"))
    };
    let control_db = need("--control-db")?;
    let node = need("--node")?;
    let statement = need("--operator-statement")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let released = gputeer_coordinator::failover::release_lost_node_by_operator(
        std::path::Path::new(&control_db),
        &node,
        &statement,
        now,
    )?;
    Ok(format!(
        "LOST_NODE_RELEASED node_id={} attempt_id={} job_id={} gpus={}",
        released.node_id,
        released.attempt_id,
        released.job_id,
        released.released_gpu_ids.join(",")
    ))
}
