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

use std::process::ExitCode;

mod selftest;

const USAGE: &str = "\
gputeer — gPUteer CLI

사용법:
    gputeer selftest [작업디렉터리]

    selftest    지금 구현된 계층을 끝에서 끝까지 한 번 돌린다.
                작업디렉터리를 주지 않으면 임시 디렉터리를 쓰고 지운다.

★ 이 CLI 는 아직 selftest 하나뿐이다.
  coordinator · agent · scheduler 가 미착수이므로 그것들을 부르는
  명령도 없다. 없는 것을 있는 것처럼 적지 않는다.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("selftest") => match selftest::run(args.get(1).map(String::as_str)) {
            Ok(report) => {
                print!("{report}");
                if report.contains("실패") {
                    // ★ 보고서에 실패가 있으면 종료 코드도 실패여야 한다.
                    //   사람이 읽는 글과 기계가 읽는 코드가 다르면
                    //   자동화가 조용히 통과시킨다.
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
