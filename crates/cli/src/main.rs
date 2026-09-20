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
mod gpu_probe;
mod import_inventory;
mod import_manifest;
mod issue_grant;
mod out_file;
mod plan_job;
mod scheduler_tick;
mod selftest;
mod stage_job;
mod submit;

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
    gputeer submit --job-id <id> --entrypoint <cmd> --submitter-device-id <id> \\
        --submitter-seed <hex32> --issued-at-unix-ms <ms> --out <path>
    gputeer import-manifest --manifest <path> --submitter-keyring <path> \\
        --job-db <path> --idempotency-key <hex16>
    gputeer import-inventory --inventory <path> --inventory-db <path>
    gputeer issue-grant --job-id <id> --control-db <path> \
        --attempt-id <ulid> --lease-id <ulid> --grant-id <id> \
        --grant-issued-at-unix-ms <ms> --grant-expires-at-unix-ms <ms> \
        --coordinator-key-file <path> --out <path> --submitter-keyring <path> \
        [--i-understand-plaintext-keyring-is-unsafe true] [--overwrite-existing-grant true]
    gputeer scheduler-tick --control-db <path> --submitter-keyring <path> \
        --submitter-member <id> --max-snapshot-age-ms <ms> \
        --best-fit-axes <a,b,c,d,e> --coordinator-id <id> --coordinator-term <n> \
        --lease-ttl-ms <ms> --lease-renew-after-ms <ms> \
        --lease-max-total-duration-seconds <s>
    gputeer stage-job --job-id <id> --control-db <path> \
        --submitter-keyring <path> --submitter-member <id> \
        --max-snapshot-age-ms <ms> --best-fit-axes <a,b,c,d,e> \
        --coordinator-id <id> --coordinator-term <n> \
        --attempt-id <ulid> --lease-id <ulid> --operation-key <hex16> \
        --lease-issued-at-unix-ms <ms> --lease-renew-after-unix-ms <ms> \
        --lease-expires-at-unix-ms <ms> --lease-max-total-duration-seconds <s>
    gputeer plan-job --job-id <id> --control-db <path> \
        --submitter-keyring <path> --submitter-member <id> \
        --max-snapshot-age-ms <ms>

    selftest                     지금 구현된 계층을 끝에서 끝까지 한 번 돌린다.
                                  작업디렉터리를 주지 않으면 임시 디렉터리를 쓰고 지운다.
    coordinator-agent-selftest   coordinator-stub/agent-stub 을 별도 프로세스로 띄워
                                  실제 프로세스 경계를 넘는 handshake 를 증명한다.
    coordinator-stub/agent-stub  coordinator-agent-selftest 가 내부적으로 띄우는
                                  하위 프로세스다 — 직접 부를 수도 있지만 사람이 쓰라고
                                  만든 인터페이스는 아니다.
    submit                       제출자가 자기 키로 JobManifest 를 서명해 파일로 낸다.
                                  ★ Coordinator 는 제출자 개인키를 갖지 않는다.
    import-manifest              운영자가 그 파일을 durable job store 에 **반입**한다.
                                  ★ 제출 \"접수\" 가 아니다 — 신뢰 경계는 운영자가
                                    provision 한 keyring 파일이고, 거기 없는 서명자는
                                    거부된다. 멤버십 판정은 하지 않는다.
    import-inventory             운영자가 선언한 노드 목록을 durable inventory
                                  store 에 **반입**한다 — 이게 있어야 scheduler 가
                                  고를 후보가 생긴다.
                                  ★ 서명된 inventory 메시지가 없으므로 이건
                                    \"검증\" 이 아니다. 신뢰 경계는 이 명령을
                                    실행할 권한과 그 파일의 OS 권한이다.

★ scheduler 커널은 있고 production 연결만 없다 — import-inventory 로
  후보를 넣으면 evaluate_eligibility 가 실제로 고른다. 그 결과를 받아
  Grant 를 만드는 경로가 아직 없다. runtime-container 는 미착수다.
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
        Some("gpu-probe") => match gpu_probe::run(&args[1..]) {
            Ok(report) => {
                print!("{report}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("gpu-probe 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("import-manifest") => match import_manifest::run(&args[1..]) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("import-manifest 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("issue-grant") => match issue_grant::run(&args[1..]) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("issue-grant 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("scheduler-tick") => match scheduler_tick::run(&args[1..]) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("scheduler-tick 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("stage-job") => match stage_job::run(&args[1..]) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("stage-job 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("plan-job") => match plan_job::run(&args[1..]) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("plan-job 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("import-inventory") => match import_inventory::run(&args[1..]) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("import-inventory 실패: {e}");
                ExitCode::FAILURE
            }
        },
        Some("submit") => match submit::run(&args[1..]) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("submit 실패: {e}");
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
