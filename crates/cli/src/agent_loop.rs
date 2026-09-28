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
//!
//! ★ 결함 497 (재검수 125) — 회차가 사람에게 넘길 컨테이너 사건을 **표식으로 남기지 못했으면**(`CONTAINER_INCIDENT_NOT_RECORDED`) 루프를 멈춘다.
//!   다음 회차가 뜨면 기동 관문은 빈 표식 폴더를 보고 남은 컨테이너 정리를 돌리기 때문이다. 다시 띄우면 기동 관문이 표식 폴더에 실제로 쓸 수
//!   있는지부터 본다.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// agent-stub 출력 한 줄에서 담는 상한(결함 560 — 넘는 부분은 버린다).
const MAX_LINE_BYTES: usize = 64 * 1024;

/// agent-stub 이 끝난 뒤 stdout · stderr 파이프가 닫히기를 기다리는 여유 — 파이프를 물려받은 손자가 남으면 EOF 가 오지 않는다(결함 545 와 같은 모양).
const PIPE_CLOSE_GRACE: Duration = Duration::from_secs(5);

/// ★ 결함 560 — 한 회차의 출력에서 **남기는 것**. 전에는 stdout · stderr 전체를 `Command::output()` 으로 모았다 — 긴 회차(학습 몇 시간)에
///   체크포인트 게시 줄이 수만 개 쌓이면 부모(서비스)가 그만큼 메모리를 썼다. 이제 줄 단위로 흘려 읽고 쓰는 넷만 남긴다(각각 한 줄 상한 안).
#[derive(Debug, Default, Clone)]
struct RoundLines {
    /// 어느 줄에든 WORKLOAD_RESULT 가 있었다(전의 `text.contains` 와 같다).
    worked: bool,
    /// WORKLOAD_RESULT 로 시작하는 첫 줄.
    workload_result: Option<String>,
    /// CONTAINER_INCIDENT_NOT_RECORDED 로 시작하는 첫 줄.
    incident_not_recorded: Option<String>,
    /// RUN_LEDGER_FATAL 로 시작하는 첫 줄 — 노드 실행 원장을 쓰지 못했다(계획 2026-09-29_0212 r1i ①).
    ledger_fatal: Option<String>,
    /// 마지막 빈 줄 아닌 줄.
    last_nonempty: Option<String>,
    /// 상한을 넘어 뒷부분을 버린 줄 수.
    cut_lines: u64,
}

impl RoundLines {
    fn take(&mut self, line: &str, forward: &mut dyn FnMut(&str)) {
        if FORWARDED_PREFIXES
            .iter()
            .any(|prefix| line.starts_with(prefix))
        {
            forward(line);
        }
        if line.contains("WORKLOAD_RESULT") {
            self.worked = true;
        }
        if self.workload_result.is_none() && line.starts_with("WORKLOAD_RESULT") {
            self.workload_result = Some(line.to_string());
        }
        if self.incident_not_recorded.is_none() && line.starts_with(INCIDENT_NOT_RECORDED) {
            self.incident_not_recorded = Some(line.to_string());
        }
        if self.ledger_fatal.is_none() && line.starts_with(RUN_LEDGER_FATAL) {
            self.ledger_fatal = Some(line.to_string());
        }
        if !line.trim().is_empty() {
            self.last_nonempty = Some(line.to_string());
        }
    }
}

/// 한 줄을 읽되 `MAX_LINE_BYTES` 까지만 담는다 — 개행이 없는 아주 긴 출력도 버퍼가 커지지 않는다. `(읽은 바이트, 잘렸는가)`. 0 이면 EOF.
fn read_line_capped(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
) -> std::io::Result<(usize, bool)> {
    let (mut consumed, mut cut) = (0usize, false);
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok((consumed, cut));
        }
        let (take, done) = match available.iter().position(|byte| *byte == b'\n') {
            Some(end) => (end + 1, true),
            None => (available.len(), false),
        };
        let room = MAX_LINE_BYTES.saturating_sub(line.len());
        if take > room {
            cut = true;
        }
        line.extend_from_slice(&available[..take.min(room)]);
        reader.consume(take);
        consumed += take;
        if done {
            return Ok((consumed, cut));
        }
    }
}

/// 한 스트림을 줄 단위로 흘려 읽어 `lines` 에 반영한다 — 옮겨 찍을 줄은 도착하는 즉시 `forward` 로 넘긴다.
fn scan_lines(
    reader: impl Read,
    lines: &Mutex<RoundLines>,
    forward: &mut dyn FnMut(&str),
) -> std::io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        let (read, cut) = read_line_capped(&mut reader, &mut line)?;
        if read == 0 {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        let mut state = lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cut {
            state.cut_lines += 1;
        }
        state.take(text, forward);
    }
}

/// 한 회차의 결과.
struct Round {
    exit_ok: bool,
    lines: RoundLines,
    /// 파이프 · 읽기에서 생긴 일(판정은 그때까지 읽은 줄로 한다).
    notes: Vec<String>,
}

/// agent-stub 을 한 번 돌린다 — 출력은 흘려 읽는다(결함 560). agent-stub 자체에는 시한을 걸지 않는다(회차가 학습 몇 시간일 수 있다).
fn run_round(exe: &Path, agent_args: &[String]) -> Result<Round, String> {
    let mut command = Command::new(exe);
    command.arg("agent-stub").args(agent_args);
    run_command_round(command)
}

/// 준비된 명령으로 한 회차를 돈다 — 시험이 "끝난 뒤 손자가 파이프를 쥐는" 명령을 넣는다.
fn run_command_round(mut command: Command) -> Result<Round, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("AGENT_LOOP: agent-stub 을 띄우지 못했다: {e}"))?;
    let stdout_lines = Arc::new(Mutex::new(RoundLines::default()));
    let stderr_lines = Arc::new(Mutex::new(RoundLines::default()));
    let spawn_scan = |pipe: Box<dyn Read + Send>, lines: Arc<Mutex<RoundLines>>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            // ★ 운영자가 봐야 하는 관측 줄은 그대로 옮겨 찍는다 — 재개 · 체크포인트 게시 · 보고 확인 · 거부 사유. 도착하는 즉시.
            let result = scan_lines(pipe, &lines, &mut |line| println!("  {line}"));
            let _ = tx.send(result.map_err(|error| error.to_string()));
        });
        rx
    };
    let stdout_rx = spawn_scan(
        Box::new(child.stdout.take().expect("piped")),
        stdout_lines.clone(),
    );
    let stderr_rx = spawn_scan(
        Box::new(child.stderr.take().expect("piped")),
        stderr_lines.clone(),
    );
    let status = child
        .wait()
        .map_err(|e| format!("AGENT_LOOP: agent-stub 을 기다리지 못했다: {e}"))?;
    let deadline = Instant::now() + PIPE_CLOSE_GRACE;
    let mut notes = Vec::new();
    for (rx, which) in [(stdout_rx, "stdout"), (stderr_rx, "stderr")] {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => notes.push(format!(
                "{which} 를 끝까지 읽지 못했다({error}) — 그때까지 읽은 줄로 판정한다"
            )),
            Err(_) => notes.push(format!(
                "agent-stub 은 끝났지만 {which} 파이프가 {PIPE_CLOSE_GRACE:?} 안에 닫히지 않았다(파이프를 물려받은 프로세스가 남았을 수 있다) — 그때까지 읽은 줄로 판정한다"
            )),
        }
    }
    let snapshot = |lines: &Arc<Mutex<RoundLines>>| {
        lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    };
    Ok(Round {
        exit_ok: status.success(),
        lines: merge(snapshot(&stdout_lines), snapshot(&stderr_lines)),
        notes,
    })
}

/// stdout · stderr 에서 모은 것을 합친다 — 전에 "stdout 뒤에 stderr 를 붙인 글" 에서 찾던 것과 같은 답(첫 줄은 stdout 먼저, 마지막 줄은 stderr 먼저).
fn merge(stdout: RoundLines, stderr: RoundLines) -> RoundLines {
    RoundLines {
        worked: stdout.worked || stderr.worked,
        workload_result: stdout.workload_result.or(stderr.workload_result),
        incident_not_recorded: stdout
            .incident_not_recorded
            .or(stderr.incident_not_recorded),
        ledger_fatal: stdout.ledger_fatal.or(stderr.ledger_fatal),
        last_nonempty: stderr.last_nonempty.or(stdout.last_nonempty),
        cut_lines: stdout.cut_lines + stderr.cut_lines,
    }
}

/// agent-stub 출력에서 루프가 그대로 옮겨 찍는 줄.
///
/// ★ 결함 504 (재검수 126) — 컨테이너를 남긴 · 지우지 못한 이유와 사건 표식 기록도 옮겨 찍는다(전에는 자식의 출력에만 있어 서비스 로그에서 사라졌다).
const FORWARDED_PREFIXES: [&str; 17] = [
    INCIDENT_NOT_RECORDED,
    RUN_LEDGER_FATAL,
    "CONTAINER_INCIDENT_RECORDED",
    "CONTAINER_INCIDENT_OPEN",
    "CONTAINER_INCIDENT_UNKNOWN",
    "CONTAINER_NOT_REMOVED",
    "CONTAINER_KEPT_FOR_LOGS",
    "CONTAINER_LEFTOVER",
    "WORKLOAD_DIR_KEPT",
    "WORKLOAD_MAY_BE_RUNNING",
    "ACK_RECEIPT_VERIFIED",
    "OWNER_STOPPED",
    "RESUME_PREPARED",
    "CHECKPOINT_PUBLISHED",
    "CHECKPOINT_PUBLISH_FAILED",
    "ATTEMPT_REPORT_ACKNOWLEDGED",
    "RESUME_REFUSED",
];

/// 회차가 사건 표식을 쓰지 못했다는 줄(`container::record_incident_if_needed`). 이 줄이 있으면 이 회차 뒤 루프를 멈춘다.
const INCIDENT_NOT_RECORDED: &str = "CONTAINER_INCIDENT_NOT_RECORDED";

/// 회차가 노드 실행 원장에 차단 근거 · 종료를 적지 못했다는 줄(`gputeer_agent::run_ledger`). 이 줄이 있으면 이 회차 뒤 루프를 멈춘다 —
/// 다시 띄우면(systemd) 다음 기동이 원장의 풀기 규칙으로 판정한다(계획 `docs/plans/2026-09-29_0212_노드_실행원장_기존노드_이관_구현계획.md`).
const RUN_LEDGER_FATAL: &str = "RUN_LEDGER_FATAL";

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
    run_rounds(interval_ms, max_rounds, || run_round(&exe, agent_args))
}

/// 회차를 되풀이한다 — 회차 하나를 돌리는 것은 `next_round` 가 한다(시험이 실제 반복 경로를 가짜 회차로 돌릴 수 있게 — 계획 2026-09-29_0212 R8d).
fn run_rounds(
    interval_ms: u64,
    max_rounds: u64,
    mut next_round: impl FnMut() -> Result<Round, String>,
) -> Result<String, String> {
    let (mut rounds, mut worked, mut idle) = (0u64, 0u64, 0u64);
    loop {
        if max_rounds != 0 && rounds >= max_rounds {
            break;
        }
        rounds += 1;
        let round = next_round()?;
        for note in &round.notes {
            println!("  AGENT_ROUND_OUTPUT_NOTE round={rounds}: {note}");
        }
        if round.lines.cut_lines > 0 {
            println!(
                "  AGENT_ROUND_OUTPUT_NOTE round={rounds}: {} 줄이 {MAX_LINE_BYTES} 바이트를 넘어 뒷부분을 버렸다",
                round.lines.cut_lines
            );
        }
        let did_work = round.lines.worked;
        let last_line = round.lines.last_nonempty.as_deref().unwrap_or("");
        if let Some(line) = &round.lines.incident_not_recorded {
            return Err(format!(
                "AGENT_LOOP_STOPPED round={rounds}: 사람에게 넘길 컨테이너 사건을 표식으로 남기지 못해 다음 회차를 돌리지 않는다 — 소유자가 컨테이너 · \
                 작업 폴더 · 표식 폴더를 확인한 뒤 다시 띄운다: {line}"
            ));
        }
        if let Some(line) = &round.lines.ledger_fatal {
            return Err(format!(
                "AGENT_LOOP_STOPPED round={rounds}: 노드 실행 원장에 기록하지 못해 다음 회차를 돌리지 않는다 — 다시 띄우면 원장의 풀기 규칙이 \
                 판정한다: {line}"
            ));
        }
        if did_work {
            worked += 1;
            let result = round.lines.workload_result.as_deref().unwrap_or("");
            println!(
                "AGENT_ROUND {rounds} outcome=worked exit_ok={} {result}",
                round.exit_ok
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(text: &[u8]) -> (RoundLines, Vec<String>) {
        let lines = Mutex::new(RoundLines::default());
        let mut forwarded = Vec::new();
        scan_lines(text, &lines, &mut |line| forwarded.push(line.to_string())).unwrap();
        (lines.into_inner().unwrap(), forwarded)
    }

    #[test]
    fn a_round_that_could_not_record_an_incident_stops_the_loop() {
        let (lines, _) = scan(
            "WORKLOAD_RESULT ok=true\nCONTAINER_INCIDENT_NOT_RECORDED name=gputeer-x kind=EXITED — 쓰지 못했다\n"
                .as_bytes(),
        );
        assert!(lines
            .incident_not_recorded
            .is_some_and(|line| line.contains("gputeer-x")));
        let (lines, _) =
            scan(b"WORKLOAD_RESULT ok=true\nCONTAINER_INCIDENT_RECORDED name=gputeer-x\n");
        assert!(lines.incident_not_recorded.is_none());
    }

    /// 계획 2026-09-29_0212 R8d — **실제 반복 경로**에서 원장 치명 오류가 난 회차 뒤에 다음 회차를 돌리지 않는다(무한 반복 설정이어도).
    #[test]
    fn the_loop_does_not_run_another_round_after_a_fatal_ledger_line() {
        let mut calls = 0u32;
        let result = run_rounds(0, 0, || {
            calls += 1;
            let (lines, _) = scan(
                "WORKLOAD_RESULT ok=true\nRUN_LEDGER_FATAL attempt_id=a-1 — CLOSED 를 쓰지 못했다\n".as_bytes(),
            );
            Ok(Round {
                lines,
                exit_ok: false,
                notes: Vec::new(),
            })
        });
        let error = result.unwrap_err();
        assert!(error.contains("AGENT_LOOP_STOPPED round=1"), "{error}");
        assert!(error.contains("RUN_LEDGER_FATAL"), "{error}");
        assert_eq!(calls, 1, "치명 오류 뒤 회차를 더 돌렸다");
        // 대조군 — 치명 오류가 없으면 정해진 회차만큼 돈다.
        let mut plain = 0u32;
        let done = run_rounds(0, 3, || {
            plain += 1;
            let (lines, _) = scan(b"WORKLOAD_RESULT ok=true\n");
            Ok(Round {
                lines,
                exit_ok: true,
                notes: Vec::new(),
            })
        })
        .unwrap();
        assert_eq!(plain, 3);
        assert!(done.contains("rounds=3"), "{done}");
    }

    /// 계획 2026-09-29_0212 R8d — 원장 치명 오류 줄도 루프를 멈추는 줄로 잡히고, 서비스 로그로 옮겨 찍힌다.
    #[test]
    fn a_round_with_a_fatal_ledger_error_stops_the_loop() {
        let (lines, forwarded) = scan(
            "WORKLOAD_RESULT ok=true\nRUN_LEDGER_FATAL attempt_id=a-1 — CLOSED 를 쓰지 못했다\n"
                .as_bytes(),
        );
        assert!(lines.ledger_fatal.is_some_and(|line| line.contains("a-1")));
        assert!(forwarded
            .iter()
            .any(|line| line.starts_with("RUN_LEDGER_FATAL")));
        let (lines, _) = scan(b"WORKLOAD_RESULT ok=true\nRUN_LEDGER_CLOSED attempt_id=a-1\n");
        assert!(lines.ledger_fatal.is_none());
    }

    /// 결함 560 — 아주 긴 줄(개행 없는 출력)과 많은 줄이 와도 남기는 것은 상한 안쪽의 네 가지뿐이다. 판정(일했나 · 멈춰야 하나 · 옮겨 찍기)은 전과 같다.
    #[test]
    fn a_huge_round_output_keeps_only_bounded_lines() {
        let mut text = Vec::new();
        text.extend_from_slice(b"CHECKPOINT_PUBLISHED checkpoint_id=c1 step=1\n");
        text.extend(std::iter::repeat_n(b'x', 1024 * 1024));
        text.extend_from_slice(b"\nWORKLOAD_RESULT ok=true\n");
        text.extend_from_slice(b"CONTAINER_INCIDENT_NOT_RECORDED name=gputeer-y\r\n");
        for _ in 0..10_000 {
            text.extend_from_slice(b"noise line\n");
        }
        text.extend(std::iter::repeat_n(b'y', 1024 * 1024)); // 마지막 줄 — 개행 없음
        let (lines, forwarded) = scan(&text);
        assert!(lines.worked);
        assert_eq!(
            lines.workload_result.as_deref(),
            Some("WORKLOAD_RESULT ok=true")
        );
        assert_eq!(
            lines.incident_not_recorded.as_deref(),
            Some("CONTAINER_INCIDENT_NOT_RECORDED name=gputeer-y")
        );
        assert_eq!(
            forwarded,
            vec![
                "CHECKPOINT_PUBLISHED checkpoint_id=c1 step=1",
                "CONTAINER_INCIDENT_NOT_RECORDED name=gputeer-y"
            ],
            "옮겨 찍는 줄(FORWARDED_PREFIXES)만, 도착 순서대로"
        );
        let last = lines.last_nonempty.unwrap();
        assert!(
            last.len() <= MAX_LINE_BYTES && last.starts_with('y'),
            "마지막 줄을 상한까지만 담지 않았다({} 바이트)",
            last.len()
        );
        assert_eq!(lines.cut_lines, 2);
    }

    /// 전에 "stdout 뒤에 stderr 를 붙인 글" 에서 찾던 답과 같다 — 첫 WORKLOAD_RESULT 는 stdout 먼저, 마지막 줄은 stderr 먼저.
    #[test]
    fn stdout_and_stderr_merge_like_the_old_concatenation() {
        let (out, _) = scan(b"WORKLOAD_RESULT ok=true\nout last\n");
        let (err, _) = scan(b"WORKLOAD_RESULT ok=false\nerr last\n");
        let merged = merge(out.clone(), err);
        assert_eq!(
            merged.workload_result.as_deref(),
            Some("WORKLOAD_RESULT ok=true")
        );
        assert_eq!(merged.last_nonempty.as_deref(), Some("err last"));
        let merged = merge(out, RoundLines::default());
        assert_eq!(merged.last_nonempty.as_deref(), Some("out last"));
    }

    /// 결함 560 — 회차 프로세스가 끝났는데 파이프를 물려받은 손자가 남으면(EOF 가 오지 않는다) 여유(`PIPE_CLOSE_GRACE`)만 기다리고 그때까지 읽은
    /// 줄로 판정한다 — 손자가 끝날 때까지(30초) 멈추지 않는다.
    #[test]
    fn a_grandchild_holding_the_pipe_does_not_hang_the_round() {
        let command = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args([
                "/C",
                "echo WORKLOAD_RESULT ok=true& start /B powershell -NoProfile -Command Start-Sleep -Seconds 30",
            ]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", "echo WORKLOAD_RESULT ok=true; sleep 30 & exit 0"]);
            c
        };
        let started = Instant::now();
        let round = run_command_round(command).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "손자가 끝날 때까지 기다렸다({:?})",
            started.elapsed()
        );
        assert!(round.lines.worked, "끝나기 전에 읽은 줄을 잃었다");
        assert!(
            round
                .notes
                .iter()
                .any(|note| note.contains("닫히지 않았다")),
            "{:?}",
            round.notes
        );
    }
}
