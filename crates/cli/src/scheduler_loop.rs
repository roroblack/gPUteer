//! `gputeer scheduler-loop` — 큐를 **되풀이해** 돈다(신뢰망 P1 마지막 고리).
//!
//! ```text
//! plan-job         운영자가 Job 하나를 큐에 올린다        (QUEUED)
//! scheduler-tick   큐에서 한 건을 골라 예약한다           (STAGING)
//! scheduler-loop   그 tick 을 간격을 두고 반복한다        <- 이 파일
//! ```
//!
//! # ★★ 왜 이제야 만드나
//!
//! `scheduler_tick` 이 이렇게 적어 뒀다 — "루프는 쉽고 **틀리기도 쉽다**.
//! 실패를 어떻게 다룰지, 얼마나 자주 돌지, 언제 멈출지가 전부 정책이고
//! 이 저장소에 그 규범이 없다."
//!
//! 두 가지가 바뀌어서 이제 만든다:
//!
//! ```text
//! 1  후보 선택이 예약을 안다(결정 `B′`, 2026-09-22 구현)
//!    -> 전에는 루프를 돌려도 **큐 맨 앞에서 영영 막혔다.**
//!       잡힌 노드를 계속 고르고 계속 거부당했다
//! 2  시도가 "끝났다" 를 적고, 그 근거로 예약이 풀린다(§A1 4c · P1-2)
//!    -> 전에는 한 번 잡힌 노드가 영영 안 풀렸다
//! ```
//!
//! # 이 루프의 정책 — **전부 명시한다**
//!
//! ```text
//! 간격       --interval-ms (필수). 기본값을 두지 않는다 — 운영자가 정한다
//! 멈춤       --max-ticks (필수). 0 이면 무한. 시험과 운영 둘 다 이 값으로 가른다
//! 빈 큐      TICK_IDLE 은 실패가 아니다. 세고, 기다렸다 다시 본다
//! 한 건 거부  TICK_REFUSED 는 **그 Job 의 문제**다. 세고 계속 돈다 —
//!            한 건 때문에 루프를 세우면 나머지 큐가 인질이 된다
//! 설정 거부   TICK_ARGS_REFUSED 는 **내 문제**다. 즉시 멈춘다 —
//!            인자가 틀린 채로 계속 돌면 같은 실패를 영원히 반복한다
//! 그 밖 오류   저장소 장애 등 — 즉시 멈춘다(fail-closed).
//!            Coordinator dispatcher 가 이미 같은 원칙을 쓴다(`DoD-37`)
//! ```
//!
//! # 이 루프가 하지 않는 것
//!
//! ```text
//! 시각 관리      tick 이 자기 시계를 본다. 여기서 시각을 만들지 않는다
//! 병렬 처리      한 번에 한 건이다. 동시에 여러 건을 잡는 것은 별도 설계다
//! 재시도 정책    거부된 Job 을 특별히 다시 시도하지 않는다 — 큐 순서대로 다시 만난다
//! 종료 신호 처리  Ctrl+C 등은 OS 에 맡긴다. 신호 처리기를 두지 않는다
//! ```

use std::time::Duration;

use crate::scheduler_tick;

/// 한 번 돌 때 무슨 일이 있었나.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoopCounters {
    pub ticks: u64,
    pub staged: u64,
    pub idle: u64,
    pub refused: u64,
}

pub fn run(args: &[String]) -> Result<String, String> {
    let (interval_ms, max_ticks, tick_args) = split_flags(args)?;
    let mut counters = LoopCounters::default();
    let mut lines = Vec::new();

    loop {
        if max_ticks != 0 && counters.ticks >= max_ticks {
            break;
        }
        counters.ticks += 1;
        match scheduler_tick::run(&tick_args) {
            Ok(output) => {
                if output.starts_with("TICK_IDLE") {
                    counters.idle += 1;
                } else {
                    counters.staged += 1;
                    lines.push(output);
                }
            }
            Err(error) => {
                // ★ 내 설정이 틀린 것과 그 Job 이 안 되는 것을 가른다.
                if error.starts_with("TICK_ARGS_REFUSED") {
                    return Err(format!(
                        "LOOP_STOPPED: 설정이 틀렸다 — 고치기 전에는 몇 번을 돌려도 같다: {error}"
                    ));
                }
                if error.starts_with("TICK_REFUSED") {
                    counters.refused += 1;
                    lines.push(error);
                } else {
                    // 저장소 장애 등 — 조용히 넘기지 않는다(fail-closed).
                    return Err(format!("LOOP_STOPPED: {error}"));
                }
            }
        }

        let more_to_do = max_ticks == 0 || counters.ticks < max_ticks;
        if more_to_do && interval_ms > 0 {
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    }

    lines.push(format!(
        "LOOP_DONE ticks={} staged={} idle={} refused={}",
        counters.ticks, counters.staged, counters.idle, counters.refused
    ));
    Ok(lines.join("\n"))
}

/// 루프 전용 인자를 떼어내고 나머지는 tick 에 그대로 넘긴다.
///
/// ★ tick 의 인자를 여기서 **해석하지 않는다.** 두 곳에서 해석하면 규칙이 갈라진다 —
///   이 저장소가 이미 여러 번 겪은 사고다(같은 것을 두 곳에서 판단하기).
fn split_flags(args: &[String]) -> Result<(u64, u64, Vec<String>), String> {
    let mut interval_ms = None;
    let mut max_ticks = None;
    let mut rest = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--interval-ms" => {
                interval_ms = Some(number(args.get(index + 1), "--interval-ms")?);
                index += 2;
            }
            "--max-ticks" => {
                max_ticks = Some(number(args.get(index + 1), "--max-ticks")?);
                index += 2;
            }
            other => {
                rest.push(other.to_string());
                index += 1;
            }
        }
    }
    let interval_ms = interval_ms.ok_or_else(|| {
        "LOOP_ARGS_REFUSED: --interval-ms 가 필요하다 — 얼마나 자주 돌지는 운영자가 정한다"
            .to_string()
    })?;
    let max_ticks = max_ticks.ok_or_else(|| {
        "LOOP_ARGS_REFUSED: --max-ticks 가 필요하다 — 0 이면 무한히 돈다. 무한을 쓰려면 그렇게 적어라"
            .to_string()
    })?;
    Ok((interval_ms, max_ticks, rest))
}

fn number(raw: Option<&String>, flag: &str) -> Result<u64, String> {
    let raw = raw.ok_or_else(|| format!("LOOP_ARGS_REFUSED: {flag} 에 값이 없다"))?;
    raw.parse::<u64>()
        .map_err(|_| format!("LOOP_ARGS_REFUSED: {flag} 가 숫자가 아니다: {raw}"))
}
