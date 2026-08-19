//! `gputeer` — CLI 스트림 (`docs/contracts/01_스트림_소유권.md`).
//!
//! # ★ 지금 이 바이너리가 있는 이유
//!
//! 2026-08-17까지 이 저장소에는 **실행 가능한 것이 없었다.**
//! 계층은 쌓였는데(protocol · crypto · checkpoint) 그것을 조립해
//! 돌려 보는 물건이 없었다. 유일한 바이너리 `ckpt_writer` 는
//! 카오스 테스트용 픽스처다.
//!
//! `cargo test` 가 통과한다는 것과 **"돌아간다"** 는 것은 다르다.
//! 테스트는 각 조각을 따로 부르지만, 실제로 조각들을 한 줄로 꿰어
//! 처음부터 끝까지 통과시켜 본 적이 없었다.
//!
//! ```text
//! gputeer selftest    지금 있는 계층을 끝에서 끝까지 한 번 돌린다
//! ```
//!
//! # ★ 이것이 **아닌** 것
//!
//! ```text
//! 서비스가 아니다        네트워크 수신도, 데몬도, 스케줄러도 없다
//! 제품 기능이 아니다     Job 을 실제로 실행하지 않는다
//! 성능 측정이 아니다     한 번씩만 돈다. 숫자를 SLA 로 읽지 마라
//! ```
//!
//! coordinator · agent · scheduler · runtime 은 여전히 **미착수**다.
//! 이 바이너리는 "지금까지 만든 것이 실제로 맞물리는가" 만 답한다.
//!
//! ★ 2026-08-18 추가 — `coordinator-stub`/`agent-stub`/
//!   `coordinator-agent-selftest`. "완전한 coordinator/agent" 가 아니다
//!   — 별도 OS 프로세스 두 개가 서명된 `ExecutionGrant`/`AgentGrantAck`
//!   를 주고받는 최소 핸드셰이크만 증명한다
//!   (`docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`).

use std::process::ExitCode;

mod coordinator_agent_selftest;
mod selftest;

const USAGE: &str = "\
gputeer — gPUteer CLI

사용법:
    gputeer selftest [작업디렉터리]
    gputeer coordinator-agent-selftest
    gputeer coordinator-stub --listen <addr> --own-seed <hex32> --peer-pubkey <hex32> \\
        --coordinator-device-id <id> --agent-device-id <id> --grant-id <id> --attempt-id <id> \\
        --lease-id <id> --job-id <id> [--lease-db <path> | \\
        --i-understand-legacy-mode-is-unsafe true]
    gputeer agent-stub --connect <addr> --own-seed <hex32> --peer-pubkey <hex32> \\
        --coordinator-device-id <id> --agent-device-id <id>

    selftest                     지금 구현된 계층을 끝에서 끝까지 한 번 돌린다.
                                  작업디렉터리를 주지 않으면 임시 디렉터리를 쓰고 지운다.
    coordinator-agent-selftest   coordinator-stub/agent-stub 을 별도 프로세스로 띄워
                                  실제 프로세스 경계를 넘는 handshake 를 증명한다.
    coordinator-stub/agent-stub  coordinator-agent-selftest 가 내부적으로 띄우는
                                  하위 프로세스다 — 직접 부를 수도 있지만 사람이 쓰라고
                                  만든 인터페이스는 아니다.

★ scheduler · runtime-container/windows 는 여전히 미착수다.
  없는 것을 있는 것처럼 적지 않는다.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("selftest") => match selftest::run(args.get(1).map(String::as_str)) {
            Ok(report) => {
                print!("{}", report.text);
                if report.blocked > 0 {
                    eprintln!(
                        "\n★ 환경 제약으로 건너뛴 검사가 {}개 있다 — 실패가 아니라 \
                         이 환경의 한계다 (RULE.md §7.1).",
                        report.blocked
                    );
                }
                if report.failed > 0 {
                    // ★ 보고서에 실패가 있으면 종료 코드도 실패여야 한다.
                    //   사람이 읽는 글과 기계가 읽는 코드가 다르면
                    //   자동화가 조용히 통과시킨다.
                    //
                    //   ★ 2026-08-17 정정. 전에는 `report.contains("실패")`
                    //   로 텍스트를 검색했다 — 그런데 요약 줄이 항상
                    //   "실패 {count}" 를 적기 때문에, count 가 0이어도
                    //   그 검색어가 항상 존재해서 **정상 실행도 종료 코드
                    //   1이었다.** `SelftestReport::failed` 를 직접 보는
                    //   것으로 고쳤다 — 사람이 읽는 텍스트를 기계가
                    //   파싱하지 않는다.
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(e) => {
                eprintln!("selftest 실행 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("coordinator-agent-selftest") => match coordinator_agent_selftest::run() {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("coordinator-agent-selftest 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("coordinator-stub") => match gputeer_coordinator::run_from_args(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("coordinator-stub 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("agent-stub") => match gputeer_agent::run_from_args(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("agent-stub 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("--help") | Some("-h") | None => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("모르는 명령: {other}\n");
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}
