//! `gputeer agent-loop` — Agent 를 **되풀이해** 붙인다(신뢰망 남은 일 K 의 Agent 쪽).
//!
//! ```text
//! 풀 Coordinator 가 계속 떠 있다(`coordinator-stub --pool-mode true`)
//! agent-loop 가 agent-stub 을 한 번 돌린다 -> 붙어서 Hello
//!   배정이 있으면   Grant -> ACK -> 실행 -> 보고(REPORT 연결) -> 곧바로 다음 회차
//!   배정이 없으면   Coordinator 가 닫는다(NO_WORK_FOR_NODE) -> 간격만큼 기다렸다 다음 회차
//! ```
//!
//! # 왜 agent-stub 을 **프로세스로** 되풀이하나
//!
//! agent-stub 은 한 번의 세션을 정확하게 하도록 만들어졌고, 그 위에 시험이 잔뜩 있다. 그 안에 루프를 넣으면
//! 상태(연결 번호 · replay 방어 · 체크포인트 잠금)가 회차를 건너 새어 나간다. 회차마다 새 프로세스면 회차 사이에
//! 남는 것은 **디스크에 영속된 것뿐**이다(fence watermark · outbox · 체크포인트 루트) — 그게 원래 재시작을 넘도록
//! 설계된 것들이다. 한 회차가 죽어도 다음 회차는 새로 시작한다.
//!
//! # 정책 — 전부 명시한다(`scheduler-loop` 와 같은 원칙)
//!
//! ```text
//! 간격        --interval-ms (필수). 일이 없었던 회차 뒤에만 기다린다
//! 멈춤        --max-rounds (필수). 0 이면 무한
//! 일을 했다    agent-stub 출력에 WORKLOAD_RESULT 가 있다 -> 기다리지 않고 바로 다음 회차
//! 그 밖       일이 없었거나 거부됐다 -> 마지막 줄을 적고 기다린다. ★ 둘을 가르지 않는다 — Coordinator 가
//!             "일 없음" 을 서명해 알려 주는 메시지가 계약에 없다. 이름을 지어 가르지 않는다
//! ```
//!
//! ★ `--` 뒤의 인자는 **그대로** agent-stub 에 넘긴다. 여기서 해석하지 않는다(두 곳에서 해석하면 규칙이 갈라진다).

use std::process::Command;
use std::time::Duration;

/// agent-stub 출력에서 루프가 그대로 옮겨 찍는 줄.
const FORWARDED_PREFIXES: [&str; 6] = [
    "OWNER_STOPPED",
    "RESUME_PREPARED",
    "CHECKPOINT_PUBLISHED",
    "CHECKPOINT_PUBLISH_FAILED",
    "ATTEMPT_REPORT_ACKNOWLEDGED",
    "RESUME_REFUSED",
];

pub fn run(args: &[String]) -> Result<String, String> {
    let split = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| "AGENT_LOOP_ARGS_REFUSED: `--` 뒤에 agent-stub 인자를 준다".to_string())?;
    let (own, rest) = args.split_at(split);
    let agent_args = &rest[1..];
    let mut interval_ms = None;
    let mut max_rounds = None;
    let mut index = 0;
    while index < own.len() {
        let value = || -> Result<u64, String> {
            own.get(index + 1)
                .ok_or_else(|| format!("AGENT_LOOP_ARGS_REFUSED: {} 에 값이 없다", own[index]))?
                .parse::<u64>()
                .map_err(|_| format!("AGENT_LOOP_ARGS_REFUSED: {} 가 숫자가 아니다", own[index]))
        };
        match own[index].as_str() {
            "--interval-ms" => interval_ms = Some(value()?),
            "--max-rounds" => max_rounds = Some(value()?),
            other => {
                return Err(format!(
                "AGENT_LOOP_ARGS_REFUSED: 모르는 인자 {other} — agent-stub 인자는 `--` 뒤에 둔다"
            ))
            }
        }
        index += 2;
    }
    let interval_ms = interval_ms.ok_or(
        "AGENT_LOOP_ARGS_REFUSED: --interval-ms 가 필요하다 — 얼마나 자주 붙을지는 운영자가 정한다",
    )?;
    let max_rounds = max_rounds.ok_or(
        "AGENT_LOOP_ARGS_REFUSED: --max-rounds 가 필요하다 — 0 이면 무한히 돈다. 무한을 쓰려면 그렇게 적어라",
    )?;
    if agent_args.first().map(String::as_str) == Some("agent-stub") {
        return Err(
            "AGENT_LOOP_ARGS_REFUSED: `--` 뒤에는 agent-stub 의 **인자**만 둔다(명령 이름은 이 루프가 붙인다)"
                .to_string(),
        );
    }
    let exe = std::env::current_exe().map_err(|e| format!("AGENT_LOOP: 실행 파일 경로: {e}"))?;

    let (mut rounds, mut worked, mut idle) = (0u64, 0u64, 0u64);
    loop {
        if max_rounds != 0 && rounds >= max_rounds {
            break;
        }
        rounds += 1;
        let output = Command::new(&exe)
            .arg("agent-stub")
            .args(agent_args)
            .output()
            .map_err(|e| format!("AGENT_LOOP: agent-stub 을 띄우지 못했다: {e}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let did_work = text.contains("WORKLOAD_RESULT");
        let last_line = text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        // ★ 운영자가 봐야 하는 관측 줄은 그대로 옮겨 찍는다 — 재개 · 체크포인트 게시 · 보고 확인 · 거부 사유.
        for line in text.lines().filter(|line| {
            FORWARDED_PREFIXES
                .iter()
                .any(|prefix| line.starts_with(prefix))
        }) {
            println!("  {line}");
        }
        if did_work {
            worked += 1;
            let result = text
                .lines()
                .find(|line| line.starts_with("WORKLOAD_RESULT"))
                .unwrap_or("");
            println!(
                "AGENT_ROUND {rounds} outcome=worked exit_ok={} {result}",
                output.status.success()
            );
        } else {
            idle += 1;
            println!("AGENT_ROUND {rounds} outcome=idle_or_refused last={last_line}");
        }
        let more = max_rounds == 0 || rounds < max_rounds;
        if more && !did_work && interval_ms > 0 {
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    }
    Ok(format!(
        "AGENT_LOOP_DONE rounds={rounds} worked={worked} idle_or_refused={idle}"
    ))
}
