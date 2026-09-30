//! 컨테이너 수명(만들기 → 시작 → 대기 → 종료 읽기 → 출력 → 지우기)과 소유자 정지를 **가짜 런타임**으로 잰다.
//!
//! # 왜 가짜인가
//!
//! 개발 기계(Windows)에는 podman · docker 가 없다. 실제 격리(읽기 전용 루트 · 네트워크 없음 · OOM)는 CI 리눅스의
//! 실제 docker 로 잰다(`container_isolation_real.rs`). 여기서는 **Agent 가 런타임을 어떤 순서로 부르고 결과를 어떻게
//! 읽는지** — 런타임 오류와 작업 종료를 섞지 않는지, 정지가 kill 로 가는지, 거부가 아무것도 부르기 전에 나는지 — 를 본다.
//!
//! # 가짜 런타임은 이 시험 실행 파일 자신이다
//!
//! `harness = false` 라 `main` 을 직접 쓴다. `GPUTEER_FAKE_RUNTIME_STATE` 가 있으면 런타임 흉내만 내고 끝난다.
//! 없으면 시험을 돌린다 — 시험은 자기 자신(`current_exe`)을 런타임으로 넘긴다.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gputeer_agent::container::{
    self, ContainerDecision, ContainerExecution, ContainerLeft, ContainerRunError,
    ContainerRuntime, CreateInput, Mount, RuntimeFlavor,
};

const STATE_ENV: &str = "GPUTEER_FAKE_RUNTIME_STATE";
const FAIL_ENV: &str = "GPUTEER_FAKE_RUNTIME_FAIL";

fn main() {
    // 파이프를 물려받아 쥐고 있는 보조 프로세스 흉내(결함 545) — 주어진 초만큼 자고 끝난다.
    if let Ok(secs) = std::env::var("GPUTEER_FAKE_SLEEP_SECS") {
        std::thread::sleep(Duration::from_secs(secs.parse().unwrap()));
        return;
    }
    if let Ok(state) = std::env::var(STATE_ENV) {
        std::process::exit(fake_runtime(Path::new(&state)));
    }
    let tests: &[(&str, fn())] = &[
        (
            "a_finished_container_reports_its_own_exit_code",
            a_finished_container_reports_its_own_exit_code,
        ),
        (
            "an_oom_kill_is_reported_as_such",
            an_oom_kill_is_reported_as_such,
        ),
        (
            "a_failed_create_is_not_a_workload_exit",
            a_failed_create_is_not_a_workload_exit,
        ),
        (
            "the_owner_stop_kills_the_container",
            the_owner_stop_kills_the_container,
        ),
        (
            "a_paused_container_is_confirmed_and_thawed_before_the_owner_stop",
            a_paused_container_is_confirmed_and_thawed_before_the_owner_stop,
        ),
        (
            "a_pause_that_does_not_take_is_not_reported_as_paused",
            a_pause_that_does_not_take_is_not_reported_as_paused,
        ),
        (
            "an_applied_but_unconfirmed_pause_is_unknown_and_thawed_before_the_stop",
            an_applied_but_unconfirmed_pause_is_unknown_and_thawed_before_the_stop,
        ),
        (
            "stopping_an_already_finished_container_is_not_reported_as_a_stop",
            stopping_an_already_finished_container_is_not_reported_as_a_stop,
        ),
        (
            "a_refused_decision_calls_no_runtime",
            a_refused_decision_calls_no_runtime,
        ),
        (
            "execute_runs_the_container_path_end_to_end",
            execute_runs_the_container_path_end_to_end,
        ),
        (
            "leftovers_of_this_node_are_removed_before_a_round",
            leftovers_of_this_node_are_removed_before_a_round,
        ),
        (
            "a_cdi_all_container_pulls_first_and_is_rechecked_after_create",
            a_cdi_all_container_pulls_first_and_is_rechecked_after_create,
        ),
        (
            "a_failed_start_is_never_reported_as_not_started",
            a_failed_start_is_never_reported_as_not_started,
        ),
        (
            "a_failed_start_keeps_the_container_unless_stop_and_logs_are_confirmed",
            a_failed_start_keeps_the_container_unless_stop_and_logs_are_confirmed,
        ),
        (
            "an_undeletable_partial_output_is_reported_and_never_complete",
            an_undeletable_partial_output_is_reported_and_never_complete,
        ),
        (
            "execute_marks_unreceived_logs_as_incomplete_outputs",
            execute_marks_unreceived_logs_as_incomplete_outputs,
        ),
        (
            "an_unobserved_exit_is_removed_only_when_rm_succeeded",
            an_unobserved_exit_is_removed_only_when_rm_succeeded,
        ),
        (
            "unsaved_logs_leave_no_partial_output_and_keep_the_container",
            unsaved_logs_leave_no_partial_output_and_keep_the_container,
        ),
        (
            "an_unconfirmed_stop_never_removes_the_container",
            an_unconfirmed_stop_never_removes_the_container,
        ),
        (
            "an_unremovable_same_name_container_blocks_create",
            an_unremovable_same_name_container_blocks_create,
        ),
        (
            "a_cleanup_failure_keeps_the_observed_exit_and_records_an_incident",
            a_cleanup_failure_keeps_the_observed_exit_and_records_an_incident,
        ),
        (
            "salvaged_leftover_logs_are_never_overwritten",
            salvaged_leftover_logs_are_never_overwritten,
        ),
        (
            "a_leftover_is_kept_when_stop_or_log_salvage_is_unconfirmed",
            a_leftover_is_kept_when_stop_or_log_salvage_is_unconfirmed,
        ),
        (
            "an_unwritable_incident_dir_keeps_the_observed_exit",
            an_unwritable_incident_dir_keeps_the_observed_exit,
        ),
        (
            "every_file_in_the_incident_dir_is_open_and_unknown_means_open",
            every_file_in_the_incident_dir_is_open_and_unknown_means_open,
        ),
        (
            "incidents_for_the_same_name_never_overwrite_each_other",
            incidents_for_the_same_name_never_overwrite_each_other,
        ),
        (
            "execute_hands_the_needs_human_verdict_to_the_caller",
            execute_hands_the_needs_human_verdict_to_the_caller,
        ),
        (
            "a_never_started_container_is_never_read_as_exit_zero",
            a_never_started_container_is_never_read_as_exit_zero,
        ),
        (
            "a_kill_that_does_not_stop_is_not_a_stop",
            a_kill_that_does_not_stop_is_not_a_stop,
        ),
        (
            "an_rm_that_leaves_the_container_is_not_a_removal",
            an_rm_that_leaves_the_container_is_not_a_removal,
        ),
        (
            "the_owner_stop_is_not_reported_until_the_container_stopped",
            the_owner_stop_is_not_reported_until_the_container_stopped,
        ),
        (
            "everything_after_create_uses_the_container_id",
            everything_after_create_uses_the_container_id,
        ),
        (
            "a_stale_no_such_container_from_rm_is_checked",
            a_stale_no_such_container_from_rm_is_checked,
        ),
        (
            "a_create_that_cannot_be_bound_to_this_attempt_deletes_nothing",
            a_create_that_cannot_be_bound_to_this_attempt_deletes_nothing,
        ),
        (
            "a_natural_exit_during_an_owner_stop_is_not_an_owner_stop",
            a_natural_exit_during_an_owner_stop_is_not_an_owner_stop,
        ),
        (
            "a_same_name_container_of_another_owner_is_never_removed",
            a_same_name_container_of_another_owner_is_never_removed,
        ),
        (
            "an_owner_stop_is_judged_even_after_the_run_cleaned_up",
            an_owner_stop_is_judged_even_after_the_run_cleaned_up,
        ),
        (
            "our_same_name_leftover_is_never_force_removed_before_create",
            our_same_name_leftover_is_never_force_removed_before_create,
        ),
        (
            "a_kill_that_answers_failure_after_killing_is_still_an_owner_stop",
            a_kill_that_answers_failure_after_killing_is_still_an_owner_stop,
        ),
        (
            "a_kill_that_answers_failure_then_takes_effect_is_still_an_owner_stop",
            a_kill_that_answers_failure_then_takes_effect_is_still_an_owner_stop,
        ),
        (
            "a_kill_that_never_answers_but_took_effect_is_still_an_owner_stop",
            a_kill_that_never_answers_but_took_effect_is_still_an_owner_stop,
        ),
        (
            "a_kill_that_could_not_even_be_sent_is_never_an_owner_stop",
            a_kill_that_could_not_even_be_sent_is_never_an_owner_stop,
        ),
        (
            "a_start_that_could_not_be_sent_did_not_start",
            a_start_that_could_not_be_sent_did_not_start,
        ),
        (
            "a_create_that_could_not_be_sent_leaves_nothing_for_a_human",
            a_create_that_could_not_be_sent_leaves_nothing_for_a_human,
        ),
        (
            "a_pipe_held_open_after_exit_does_not_hang_the_owner_stop",
            a_pipe_held_open_after_exit_does_not_hang_the_owner_stop,
        ),
        (
            "a_start_that_never_answers_keeps_the_container_for_a_human",
            a_start_that_never_answers_keeps_the_container_for_a_human,
        ),
    ];
    let mut failed = 0;
    for (name, test) in tests {
        let result = std::panic::catch_unwind(test);
        match result {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed += 1;
            }
        }
    }
    println!(
        "test result: {}. {} passed; {failed} failed; 0 ignored; 0 measured; 0 filtered out",
        if failed == 0 { "ok" } else { "FAILED" },
        tests.len() - failed
    );
    if failed > 0 {
        std::process::exit(101);
    }
}

// ─── 가짜 런타임 ───────────────────────────────────────────────────────────

/// 상태 폴더에 파일로 기억한다. 작업의 행동은 `--entrypoint=` 값으로 정한다:
/// `exit-<N>` 곧바로 N 으로 끝남 · `oom` 137 + OOMKilled · `sleep` kill 될 때까지 돈다.
/// 가짜 런타임이 create 에서 돌려주는 컨테이너 ID(docker · podman 처럼 16진수 64자).
const FAKE_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// 이름으로 남아 있는 컨테이너(`leftovers` 의 `gputeer-…`)의 ID — owner 조회가 이 ID 를 돌려주고, 확인한 뒤 이 ID 로 지운다(결함 526).
/// 시험은 그 이름과 이 ID 를 **둘 다** `leftovers` 에 적는다(가짜 런타임은 이름 · ID 를 따로 셈한다).
const LEFTOVER_ID: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";

fn fake_runtime(state: &Path) -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    append(state, "calls", &format!("{}\n", args.join(" ")));
    // 쉼표로 여럿을 실패시킬 수 있다(예 "start,rm").
    let fail_list = std::env::var(FAIL_ENV).unwrap_or_default();
    let fails = |token: &str| fail_list.split(',').any(|c| c == token);
    // "rm-created" — 컨테이너를 만든 뒤의 rm 만 실패시킨다(만들기 전 같은 이름 지우기는 통과 · 2026-09-27 보수 규칙 시험).
    let created = state.join("create.args").exists();
    // "start-hangs" — start 가 접수만 하고 답하지 않는다(시한을 넘긴다 · 결함 532). 작업은 아직 안 돈다 — 조회는 created(멈춤 · 시작 흔적 없음)로
    // 답한다. 멈춰 보이는 것이 멈춤 확인이 아니라는 반례다(접수된 start 가 뒤늦게 적용될 수 있다).
    if command == "start" && fails("start-hangs") {
        std::fs::write(state.join("start-noop"), "").unwrap();
        std::thread::sleep(Duration::from_secs(20));
        return 0;
    }
    // "start-after-run" — 작업 프로세스는 돌고(started) start 는 실패로 답한다(OCI poststart 훅 실패 · 결함 490).
    if command == "start" && fails("start-after-run") {
        std::fs::write(state.join("started"), "").unwrap();
        eprintln!("fake: poststart 훅 실패를 흉내낸다");
        return 126;
    }
    // "inspect-exit" — 종료 상태를 읽는 inspect 만 실패시킨다(멈춤 · 존재 확인용 `{{.State.Running}}` 은 통과 · 결함 502 · 503 시험).
    let running_only = args.get(1).map(String::as_str) == Some("--format={{.State.Running}}");
    // `{{.Name}}` — create 가 돌려준 ID 가 이 시도의 컨테이너인지 대조하는 조회(결함 519). inspect 실패 토큰은 이것을 건드리지 않는다.
    let owner_only = args
        .get(1)
        .is_some_and(|a| a.starts_with("--format={{.Id}} {{index .Config.Labels"));
    // owner 조회(결함 522 · 526)도 같은 대조용 조회다.
    let name_only = args.get(1).map(String::as_str) == Some("--format={{.Name}}") || owner_only;
    let inspect_exit_fails =
        command == "inspect" && !running_only && !name_only && fails("inspect-exit");
    // "start-noop" — start 가 0 으로 답하지만 아무것도 띄우지 않는다(결함 501). "kill-noop" — kill 이 0 이지만 멈추지 않는다(결함 502).
    // "rm-noop" — 만든 뒤의 rm 이 0 이지만 지우지 않는다(결함 503).
    if command == "start" && fails("start-noop") {
        std::fs::write(state.join("start-noop"), "").unwrap();
        return 0;
    }
    if command == "kill" && fails("kill-noop") {
        return 0;
    }
    // ★ 2026-09-30 — "pause-noop": pause 가 0 으로 답하지만 얼리지 않는다(접수는 적용이 아니다).
    if command == "pause" && fails("pause-noop") {
        return 0;
    }
    // 실제 docker 처럼 얼린 컨테이너의 kill 은 거부한다("container is paused") — 먼저 풀어야 끝난다.
    if command == "kill" && state.join("paused").exists() {
        eprintln!("Error: container is paused. Unpause the container before stopping or killing");
        return 1;
    }
    // "kill-leaves-pipe-holder" — kill 이 작업을 끝내고(137) 곧 0 으로 끝나지만, stdout · stderr 를 물려받은 보조 프로세스(60초)를 남긴다(결함 545).
    if command == "kill" && fails("kill-leaves-pipe-holder") {
        std::fs::write(state.join("killed"), "").unwrap();
        std::process::Command::new(std::env::current_exe().unwrap())
            .env("GPUTEER_FAKE_SLEEP_SECS", "60")
            .spawn()
            .unwrap();
        return 0;
    }
    // "kill-waits-for-rm" — kill 이 작업을 끝내고(137), 실행 쪽이 종료를 보고 컨테이너를 지울 때까지 기다렸다가 0 으로 답한다(결함 525 — 정지
    // 손잡이의 사후 조회가 "없다" 를 받는 경쟁을 결정적으로 만든다).
    if command == "kill" && fails("kill-waits-for-rm") {
        std::fs::write(state.join("killed"), "").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !state.join("gone").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        return 0;
    }
    // "kill-hangs-after-kill" — kill 이 작업을 끝내고(137) 응답 없이 멈춘다(시한 15초를 넘긴다 · 결함 531).
    if command == "kill" && fails("kill-hangs-after-kill") {
        std::fs::write(state.join("killed"), "").unwrap();
        std::thread::sleep(Duration::from_secs(20));
        return 0;
    }
    // "kill-fails-then-dies" — kill 이 실패로 답하고, 작업은 **조금 뒤에**(0.5초) 137 로 끝난다(SIGKILL 이 진행 중 · 결함 529).
    if command == "kill" && fails("kill-fails-then-dies") {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            + 500;
        std::fs::write(state.join("die-at"), at.to_string()).unwrap();
        eprintln!("fake: kill 후처리 실패를 흉내낸다(작업은 곧 끝난다)");
        return 125;
    }
    // "kill-fails-after-kill" — kill 이 실제로 작업을 끝내고(137) 실패로 답한다(후처리 실패 흉내 · 결함 527).
    if command == "kill" && fails("kill-fails-after-kill") {
        std::fs::write(state.join("killed"), "").unwrap();
        eprintln!("fake: kill 후처리 실패를 흉내낸다");
        return 125;
    }
    // "kill-noop-exit0" — kill 은 0 이지만 무동작이고, 그 사이 작업이 스스로 코드 0 으로 끝난다(결함 520).
    if command == "kill" && fails("kill-noop-exit0") {
        std::fs::write(state.join("natural-exit"), "").unwrap();
        return 0;
    }
    let fails_now = fails(command) && (command != "inspect" || (created && !name_only));
    if inspect_exit_fails || fails_now || (command == "rm" && created && fails("rm-created")) {
        eprintln!("fake: {command} 실패를 흉내낸다");
        return 125;
    }
    let behaviour = || std::fs::read_to_string(state.join("behaviour")).unwrap_or_default();
    // 컨테이너가 **있는가** — 이 시도의 것(만들었고 지우지 않음) 또는 죽은 회차가 남긴 것(state/leftovers · 지우지 않음).
    // 없는 이름의 inspect · rm 은 실제 런타임처럼 "No such container" 로 답한다(결함 503 — rm 뒤 사후 조회).
    let target = args.last().cloned().unwrap_or_default();
    let is_leftover = std::fs::read_to_string(state.join("leftovers"))
        .unwrap_or_default()
        .lines()
        .any(|id| id.trim() == target);
    let gone_marker = if is_leftover {
        state.join(format!("gone.{target}"))
    } else {
        state.join("gone")
    };
    let exists = (is_leftover || created) && !gone_marker.exists();
    match command {
        "create" => {
            let entry = args
                .iter()
                .find_map(|a| a.strip_prefix("--entrypoint="))
                .unwrap_or("");
            std::fs::write(state.join("behaviour"), entry).unwrap();
            std::fs::write(state.join("create.args"), args.join("\n")).unwrap();
            let _ = std::fs::remove_file(state.join("gone"));
            if !fails("create-no-id") {
                println!("{FAKE_ID}");
            }
            0
        }
        "start" => {
            std::fs::write(state.join("started"), "").unwrap();
            0
        }
        "wait" => {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let Some((code, _)) = finished(state, &behaviour()) {
                    println!("{code}");
                    return 0;
                }
                if Instant::now() > deadline {
                    return 1;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        "inspect" => {
            if !exists {
                eprintln!("Error: No such container: {target}");
                // "vanish-before-create" — 만들기 전 owner 조회에 "없다" 로 답한 뒤 런타임 사본의 이름을 바꿔 create 를 띄울 수 없게 한다(결함 546).
                if owner_only && fails("vanish-before-create") {
                    let me = std::env::current_exe().unwrap();
                    if me
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("runtime-copy"))
                    {
                        std::fs::rename(&me, me.with_extension("gone")).unwrap();
                    }
                }
                return 1;
            }
            if args.get(1).map(String::as_str) == Some("--format={{.State.Paused}}") {
                // "inspect-paused" — 일시정지 상태 조회만 실패한다(pause 는 적용됐는데 확인하지 못함 · 검수 pz1).
                if fails("inspect-paused") {
                    eprintln!("fake: State.Paused 조회 실패를 흉내낸다");
                    return 125;
                }
                println!("{}", state.join("paused").exists());
                return 0;
            }
            if owner_only {
                // 남은 컨테이너(leftovers)의 owner 는 이 Agent 의 것 — "leftover-foreign" 이면 다른 owner(결함 522). ID 를 함께 찍는다(526).
                let id = if is_leftover { LEFTOVER_ID } else { FAKE_ID };
                let owner = if is_leftover {
                    if fails("leftover-foreign") {
                        "someone-else".to_string()
                    } else {
                        "node-test.root".to_string()
                    }
                } else {
                    std::fs::read_to_string(state.join("create.args"))
                        .unwrap_or_default()
                        .lines()
                        .find_map(|l| l.strip_prefix("--label=gputeer.owner=").map(str::to_string))
                        .unwrap_or_default()
                };
                println!("{id} {owner}");
                return 0;
            }
            if name_only {
                // "create-name-mismatch" — ID 가 다른 컨테이너를 가리킨다(결함 519).
                let name = if fails("create-name-mismatch") {
                    "someone-else".to_string()
                } else {
                    std::fs::read_to_string(state.join("create.args"))
                        .unwrap_or_default()
                        .lines()
                        .find_map(|l| l.strip_prefix("--name=").map(str::to_string))
                        .unwrap_or_default()
                };
                println!("/{name}");
                // "create-then-vanish" — ID 대조 조회에 답한 뒤 런타임 실행 파일(시험이 둔 **사본**만)의 이름을 바꿔, 이어지는 start 를 띄울 수 없게
                // 한다(결함 537).
                if fails("create-then-vanish") && !owner_only {
                    let me = std::env::current_exe().unwrap();
                    if me
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("runtime-copy"))
                    {
                        std::fs::rename(&me, me.with_extension("gone")).unwrap();
                    }
                }
                return 0;
            }
            // start 가 0 으로 답했지만 아무것도 띄우지 않았다 — created · 멈춤 · 시작 흔적 없음(결함 501).
            let never_started = state.join("start-noop").exists() && !state.join("killed").exists();
            let done = finished(state, &behaviour());
            if running_only {
                println!("{}", !never_started && done.is_none());
            } else if never_started {
                println!("false 0 false 0001-01-01T00:00:00Z");
            } else {
                match done {
                    Some((code, oom)) => println!("false {code} {oom} 2026-09-28T00:00:00Z"),
                    None => println!("true 0 false 2026-09-28T00:00:00Z"),
                }
            }
            0
        }
        "pause" => {
            if !exists {
                eprintln!("Error: No such container: {target}");
                return 1;
            }
            std::fs::write(state.join("paused"), "").unwrap();
            0
        }
        "unpause" => {
            if !exists {
                eprintln!("Error: No such container: {target}");
                return 1;
            }
            let _ = std::fs::remove_file(state.join("paused"));
            0
        }
        "kill" => {
            if finished(state, &behaviour()).is_some() {
                eprintln!("fake: container is not running");
                return 1;
            }
            std::fs::write(state.join("killed"), "").unwrap();
            0
        }
        "logs" => {
            println!("hello-out");
            eprintln!("hello-err");
            0
        }
        "rm" => {
            if fails("rm-stale-nosuch") {
                eprintln!("Error: No such container: {target}");
                return 1;
            }
            if !exists {
                eprintln!("Error: No such container: {target}");
                return 1;
            }
            std::fs::write(state.join("removed"), "").unwrap();
            if !fails("rm-noop") {
                std::fs::write(&gone_marker, "").unwrap();
            }
            0
        }
        "pull" => 0,
        "ps" => {
            // 죽은 회차가 남긴 컨테이너 — 시험이 state/leftovers 에 적어 둔다.
            print!(
                "{}",
                std::fs::read_to_string(state.join("leftovers")).unwrap_or_default()
            );
            0
        }
        other => {
            eprintln!("fake: 모르는 명령 {other}");
            2
        }
    }
}

fn finished(state: &Path, behaviour: &str) -> Option<(i64, bool)> {
    if state.join("killed").exists() {
        return Some((137, false));
    }
    if let Ok(at) = std::fs::read_to_string(state.join("die-at")) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        if now >= at.trim().parse::<u128>().unwrap() {
            return Some((137, false));
        }
    }
    if state.join("natural-exit").exists() {
        return Some((0, false));
    }
    if !state.join("started").exists() {
        return None;
    }
    if let Some(code) = behaviour.strip_prefix("exit-") {
        return Some((code.parse().unwrap(), false));
    }
    if behaviour == "oom" {
        return Some((137, true));
    }
    None
}

fn append(state: &Path, name: &str, text: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join(name))
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

// ─── 시험 ──────────────────────────────────────────────────────────────────

struct Fixture {
    _dir: tempfile::TempDir,
    state: PathBuf,
    work: PathBuf,
}

/// ★ 시험은 차례로 돈다(harness = false). 환경 변수를 시험마다 새로 건다 — 가짜 런타임은 부모 환경을 물려받는다.
fn fixture(fail: Option<&str>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::env::set_var(STATE_ENV, &state);
    match fail {
        Some(command) => std::env::set_var(FAIL_ENV, command),
        None => std::env::remove_var(FAIL_ENV),
    }
    Fixture {
        _dir: dir,
        state,
        work,
    }
}

fn execution() -> ContainerExecution {
    ContainerExecution {
        runtime: ContainerRuntime {
            program: std::env::current_exe().unwrap(),
            flavor: RuntimeFlavor::Docker,
            pass_gpu: false,
            gpu_request: container::GpuRequest::Gpus,
            only: false,
            node_id: "node-test".into(),
            owner: "node-test.root".into(),
            incident_dir: None,
            start_timeout: container::DEFAULT_START_TIMEOUT,
        },
        pinned_image: format!("registry.local/train@sha256:{}", "ab".repeat(32)),
        gpu_pin: None,
    }
}

fn input<'a>(
    mounts: &'a [Mount],
    entrypoint: &'a str,
    env: &'a [(OsString, OsString)],
) -> CreateInput<'a> {
    CreateInput {
        name: "gputeer-test",
        entrypoint,
        args: &[],
        environment: env,
        mounts,
        memory_limit_bytes: 64 * 1024 * 1024,
        user: None,
    }
}

fn mounts(work: &Path) -> Vec<Mount> {
    vec![Mount {
        host: work.join("checkpoints-out"),
        target: container::CONTAINER_CHECKPOINT_DIR,
        read_only: false,
    }]
}

/// `calls` 에서 그 줄이 처음 나온 자리.
fn at(calls: &str, line: &str) -> usize {
    calls.lines().position(|l| l == line).unwrap_or_else(|| {
        panic!(
            "{line} 가 없다:
{calls}"
        )
    })
}

fn calls(state: &Path) -> String {
    std::fs::read_to_string(state.join("calls")).unwrap_or_default()
}

fn a_finished_container_reports_its_own_exit_code() {
    let f = fixture(None);
    let out = f.work.join("stdout.log");
    let err = f.work.join("stderr.log");
    let mut started = false;
    let exit = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-3", &[]),
        Some(&out),
        Some(&err),
        |_| started = true,
    )
    .expect("실행");
    assert!(started, "시작 뒤 정지 손잡이를 넘기지 않았다");
    assert_eq!(exit.exit_code, 3);
    assert!(!exit.oom_killed);
    assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), "hello-out");
    assert_eq!(std::fs::read_to_string(&err).unwrap().trim(), "hello-err");
    assert!(
        f.state.join("removed").exists(),
        "끝난 컨테이너를 지우지 않았다"
    );
    let order: Vec<String> = calls(&f.state)
        .lines()
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect();
    // 남은 같은 이름을 먼저 치우고(결함 276), wait 대신 inspect 로 종료를 본다(시한 있는 명령만 쓴다). 지운 뒤 inspect 로 없어졌는지 본다(결함 503).
    assert_eq!(
        order,
        ["inspect", "create", "inspect", "start", "inspect", "logs", "rm", "inspect"]
    );
    let create = std::fs::read_to_string(f.state.join("create.args")).unwrap();
    for flag in [
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--memory-swap=67108864",
    ] {
        assert!(
            create.lines().any(|l| l == flag),
            "{flag} 없이 만들었다:\n{create}"
        );
    }
}

fn an_oom_kill_is_reported_as_such() {
    let f = fixture(None);
    let exit = container::run(
        &execution(),
        &input(&mounts(&f.work), "oom", &[]),
        None,
        None,
        |_| {},
    )
    .expect("실행");
    assert_eq!(exit.exit_code, 137);
    assert!(exit.oom_killed, "OOM 을 보고하지 않았다");
}

fn a_failed_create_is_not_a_workload_exit() {
    let f = fixture(Some("create"));
    let mut started = false;
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| started = true,
    )
    .expect_err("create 가 실패했는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { detail, container: ContainerLeft::Removed } if detail.contains("create")),
        "{error:?}"
    );
    assert!(!started, "시작하지 않았는데 정지 손잡이를 넘겼다");
    assert!(
        !calls(&f.state).contains("start"),
        "create 실패 뒤 start 를 불렀다"
    );
    // 결함 519 (재검수 132) — create 가 실패하면 **이름으로 지우지 않는다**(같은 이름의 다른 컨테이너일 수 있다). 있는지만 조회한다.
    assert!(
        !calls(&f.state)
            .lines()
            .skip_while(|l| !l.starts_with("create "))
            .any(|l| l.starts_with("rm ")),
        "create 실패 뒤 이름으로 지웠다:\n{}",
        calls(&f.state)
    );
}

fn the_owner_stop_kills_the_container() {
    let f = fixture(None);
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            container::run(
                &execution(),
                &input(&mounts(&work), "sleep", &[]),
                None,
                None,
                move |stopper| {
                    tx.send(stopper).unwrap();
                },
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    stopper.stop().expect("정지");
    let exit = runner.join().unwrap().expect("정지 뒤 종료 관측");
    assert_eq!(exit.exit_code, 137);
    assert!(calls(&f.state).lines().any(|l| l.starts_with("kill ")));
}

/// ★ 2026-09-30 (소유자 "일시정지") — 컨테이너 실행의 손잡이는 얼릴 수 있다고 말하고, pause · unpause 를 **상태 조회로 확인한 뒤에만** 성공이다.
///   얼린 채 "지금 멈춤" 을 누르면 먼저 풀고(docker 는 얼린 컨테이너의 kill 을 거부한다) kill 한다.
fn a_paused_container_is_confirmed_and_thawed_before_the_owner_stop() {
    let f = fixture(None);
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            gputeer_agent::exec::execute_with_control(
                &spec("sleep"),
                policy(&work, ContainerDecision::Container(execution())),
                move |stopper| tx.send(stopper).unwrap(),
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    stopper
        .pause_support()
        .expect("컨테이너 실행은 얼릴 수 있어야 한다");
    stopper.pause().expect("일시정지");
    assert!(stopper.is_paused());
    assert!(f.state.join("paused").exists(), "pause 를 보내지 않았다");
    stopper.resume().expect("다시 시작");
    assert!(!stopper.is_paused());
    assert!(!f.state.join("paused").exists(), "unpause 를 보내지 않았다");
    stopper.pause().expect("다시 일시정지");
    stopper.stop().expect("얼린 채 정지");
    let exit = runner.join().unwrap().expect("정지 뒤 종료 관측");
    assert_eq!(exit.exit.code(), Some(137));
    let calls = calls(&f.state);
    let order: Vec<&str> = calls
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|c| matches!(*c, "pause" | "unpause" | "kill"))
        .collect();
    assert_eq!(
        order,
        ["pause", "unpause", "pause", "unpause", "kill"],
        "얼린 채 kill 을 보냈거나 순서가 다르다:\n{calls}"
    );
}

/// ★ 2026-09-30 — pause 가 0 으로 답해도 상태가 얼지 않았으면 실패다. 얼린 줄 알고 자리를 비우게 하지 않는다.
fn a_pause_that_does_not_take_is_not_reported_as_paused() {
    let f = fixture(Some("pause-noop"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            gputeer_agent::exec::execute_with_control(
                &spec("sleep"),
                policy(&work, ContainerDecision::Container(execution())),
                move |stopper| tx.send(stopper).unwrap(),
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    let error = stopper.pause().expect_err("얼지 않았는데 성공이라 했다");
    assert!(error.contains("CONTAINER_PAUSE_UNCONFIRMED"), "{error}");
    assert!(!stopper.is_paused(), "얼지 않았는데 얼렸다고 적었다");
    stopper.stop().expect("정지");
    runner.join().unwrap().expect("정지 뒤 종료 관측");
}

/// ★ 검수 pz1 — pause 는 적용됐는데 확인 조회가 실패하면 "얼리지 않았다" 로 적지 않는다(상태 "모름"). 그 상태의 정지는 먼저 풀고 kill 한다 —
///   전에는 확인된 경우만 풀어, 얼어 있는 컨테이너에 kill 을 보내 도커가 거부했다.
fn an_applied_but_unconfirmed_pause_is_unknown_and_thawed_before_the_stop() {
    let f = fixture(Some("inspect-paused"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            gputeer_agent::exec::execute_with_control(
                &spec("sleep"),
                policy(&work, ContainerDecision::Container(execution())),
                move |stopper| tx.send(stopper).unwrap(),
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    let error = stopper
        .pause()
        .expect_err("확인하지 못했는데 성공이라 했다");
    assert!(error.contains("CONTAINER_PAUSE_UNCONFIRMED"), "{error}");
    assert!(
        f.state.join("paused").exists(),
        "전제 — pause 는 적용됐어야 한다"
    );
    assert_eq!(
        stopper.pause_state(),
        gputeer_agent::exec::PauseState::Unknown,
        "얼었을 수 있는데 모른다고 적지 않았다"
    );
    stopper.stop().expect("얼었을 수 있는 채 정지");
    let exit = runner.join().unwrap().expect("정지 뒤 종료 관측");
    assert_eq!(exit.exit.code(), Some(137));
    let calls = calls(&f.state);
    let order: Vec<&str> = calls
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|c| matches!(*c, "pause" | "unpause" | "kill"))
        .collect();
    assert_eq!(
        order,
        ["pause", "unpause", "kill"],
        "상태를 모르는 채 풀지 않고 kill 했다:\n{calls}"
    );
}

fn stopping_an_already_finished_container_is_not_reported_as_a_stop() {
    let f = fixture(None);
    let (tx, rx) = std::sync::mpsc::channel();
    container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        move |stopper| {
            tx.send(stopper).unwrap();
        },
    )
    .expect("실행");
    let stopper = rx.recv().unwrap();
    // ★ 결함 277 — kill 은 "안 돈다" 로 실패하고 inspect 는 이미 끝났다고 한다. 이 정지는 종료 원인이 아니다 — 성공이라 하지 않는다
    //   (성공이라 하면 스스로 끝난 작업이 "소유자가 멈췄다" 로 보고된다).
    let error = stopper
        .stop()
        .expect_err("이미 끝난 컨테이너의 정지를 성공이라 했다");
    assert!(error.starts_with("ALREADY_EXITED"), "{error}");
}

fn policy(work: &Path, decision: ContainerDecision) -> gputeer_agent::exec::ExecutionPolicy {
    gputeer_agent::exec::ExecutionPolicy {
        workload_environment: vec![
            (
                OsString::from("GPUTEER_CHECKPOINT_DIR"),
                work.join("checkpoints-out").into_os_string(),
            ),
            (OsString::from("CUDA_VISIBLE_DEVICES"), OsString::from("1")),
        ],
        opted_in: true,
        commit_limit_bytes: 128 * 1024 * 1024,
        gpu_requirements: None,
        capture_dir: Some(work.to_path_buf()),
        isolation: gputeer_agent::exec::IsolationIdentity {
            grant_id: "grant".into(),
            attempt_id: "attempt".into(),
        },
        cgroup_parent: None,
        container: decision,
        allow_elevated_host: false,
    }
}

fn spec(entrypoint: &str) -> gputeer_protocol::execution_spec::ExecutionSpec {
    gputeer_protocol::execution_spec::ExecutionSpec {
        job_id: "job".into(),
        entrypoint: entrypoint.into(),
        args: vec!["--flag".into()],
        env_vars: Default::default(),
    }
}

fn a_refused_decision_calls_no_runtime() {
    let f = fixture(None);
    let error = gputeer_agent::exec::execute(
        &spec("exit-0"),
        policy(
            &f.work,
            ContainerDecision::Refused {
                detail: "CONTAINER_RUNTIME_MISSING: 시험".into(),
            },
        ),
    )
    .expect_err("거부된 Job 이 실행됐다");
    assert!(
        error
            .to_string()
            .starts_with("EXEC_REFUSED:CONTAINER_RUNTIME_MISSING"),
        "{error}"
    );
    assert!(calls(&f.state).is_empty(), "거부했는데 런타임을 불렀다");
}

fn execute_runs_the_container_path_end_to_end() {
    let f = fixture(None);
    let outcome = gputeer_agent::exec::execute(
        &spec("exit-7"),
        policy(&f.work, ContainerDecision::Container(execution())),
    )
    .expect("실행");
    assert_eq!(outcome.exit.code(), Some(7));
    assert_eq!(
        outcome.outputs_incomplete, None,
        "로그를 다 받았는데 불완전이라 했다"
    );
    assert_eq!(
        outcome.peak_commit_bytes, None,
        "재지 않은 최댓값을 지어냈다"
    );
    let create = std::fs::read_to_string(f.state.join("create.args")).unwrap();
    assert!(
        create
            .lines()
            .any(|l| l == "--env=GPUTEER_CHECKPOINT_DIR=/gputeer/checkpoints"),
        "작업 폴더 경로를 컨테이너 안 경로로 바꾸지 않았다:\n{create}"
    );
    assert!(
        !create.contains("CUDA_VISIBLE_DEVICES"),
        "호스트 GPU 번호를 컨테이너에 넘겼다:\n{create}"
    );
    assert!(
        create.lines().any(|l| l == "--memory=134217728"),
        "정책의 상한을 걸지 않았다:\n{create}"
    );
    let last_two: Vec<&str> = create.lines().rev().take(2).collect();
    assert_eq!(last_two, ["--flag", execution().pinned_image.as_str()]);
    assert!(
        f.work.join("stdout.log").exists(),
        "컨테이너 출력을 작업 폴더에 남기지 않았다"
    );
}

/// 결함 290 · 291 (재검수 90) — 회차를 시작하기 전에 **이 노드의 라벨**로 남은 컨테이너를 모두 지운다. 다른 노드 라벨은 묻지 않는다.
fn leftovers_of_this_node_are_removed_before_a_round() {
    let f = fixture(None);
    std::fs::write(f.state.join("leftovers"), "old-1\nold-2\n").unwrap();
    let salvage = f.work.join("leftover-container-logs");
    let removed = container::remove_leftovers(&execution().runtime, Some(&salvage)).expect("정리");
    assert_eq!(removed, ["old-1", "old-2"]);
    // 결함 489 — 지우기 전에 로그를 건졌다.
    for id in ["old-1", "old-2"] {
        assert_eq!(
            std::fs::read_to_string(salvage.join(format!("{id}.stdout.log")))
                .unwrap()
                .trim(),
            "hello-out"
        );
    }
    let calls = calls(&f.state);
    assert!(
        calls
            .lines()
            .any(|l| l == "ps -a -q --filter label=gputeer.owner=node-test.root"),
        "이 노드 라벨로 찾지 않았다:\n{calls}"
    );
    for id in ["old-1", "old-2"] {
        assert!(
            calls.lines().any(|l| l == format!("rm -f -v {id}")),
            "{id} 를 지우지 않았다:\n{calls}"
        );
        // 결함 492 — 로그는 지우기 전에 받는다.
        let (logs, rm) = (
            at(&calls, &format!("logs {id}")),
            at(&calls, &format!("rm -f -v {id}")),
        );
        assert!(logs < rm, "{id}: logs → rm 순서가 아니다:\n{calls}");
    }
    // 결함 492 — 도는 컨테이너는 **멈춘 뒤에** 로그를 받는다. (가짜 런타임의 "멈춤" 은 컨테이너별이 아니라 하나라,
    //   old-1 을 멈추면 old-2 는 이미 멈춘 것으로 보여 kill 을 부르지 않는다 — 그래서 첫 컨테이너만 kill 순서를 본다.)
    assert!(
        at(&calls, "kill old-1") < at(&calls, "logs old-1"),
        "old-1: 멈추기 전에 로그를 받았다:\n{calls}"
    );
    // 만드는 컨테이너에도 같은 라벨이 붙는다 — 다음 회차가 찾을 수 있다.
    let _ = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    );
    let create = std::fs::read_to_string(f.state.join("create.args")).unwrap();
    assert!(
        create
            .lines()
            .any(|l| l == "--label=gputeer.owner=node-test.root"),
        "노드 라벨 없이 만들었다:\n{create}"
    );
}

/// 결함 468 (재검수 118) — cdi-all 은 이미지를 **먼저** 받고, create 직전 · 직후에 GPU 장수를 본다. create 뒤에 늘었으면 지우고 시작하지 않는다.
fn a_cdi_all_container_pulls_first_and_is_rechecked_after_create() {
    let mut cdi_all = execution();
    cdi_all.runtime.pass_gpu = true;
    cdi_all.runtime.gpu_request = container::GpuRequest::CdiAll;
    cdi_all.gpu_pin = Some("0".into());
    let order = |state: &Path| -> Vec<String> {
        calls(state)
            .lines()
            .map(|l| l.split(' ').next().unwrap().to_string())
            .collect()
    };

    // 한 장 그대로 — pull 이 create 보다 먼저다.
    let f = fixture(None);
    container::run_with_gpu_count(
        &cdi_all,
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
        || Ok(1),
    )
    .expect("한 장이면 돈다");
    assert_eq!(
        &order(&f.state)[..5],
        ["pull", "inspect", "create", "inspect", "start"],
        "{}",
        calls(&f.state)
    );

    // create 뒤에 두 장이 됐다 — 지우고 시작하지 않는다.
    let f = fixture(None);
    let seen = std::cell::Cell::new(0);
    let mut started = false;
    let error = container::run_with_gpu_count(
        &cdi_all,
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| started = true,
        || {
            seen.set(seen.get() + 1);
            Ok(seen.get())
        },
    )
    .expect_err("create 뒤 GPU 가 늘었는데 시작했다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { detail, container: ContainerLeft::Removed } if detail.contains("create 뒤 다시 확인")),
        "{error:?}"
    );
    assert!(!started);
    assert_eq!(
        order(&f.state),
        ["pull", "inspect", "create", "inspect", "rm", "inspect"],
        "{}",
        calls(&f.state)
    );
}

/// 결함 490 (재검수 124) — start 의 실패는 "돌지 않았다" 의 증거가 아니다(poststart 훅은 작업이 돈 뒤에 실패한다). 런타임이 실패로 답해도
/// 결과는 NotStarted 가 아니라 "시작했는지 모른다"(Unobserved)이고, 멈춤 확인 → 로그 → 지우기 순서를 탄다.
fn a_failed_start_is_never_reported_as_not_started() {
    for fail in ["start-after-run", "start"] {
        let f = fixture(Some(fail));
        let out = f.work.join("stdout.log");
        let err = f.work.join("stderr.log");
        let error = container::run(
            &execution(),
            &input(&mounts(&f.work), "exit-0", &[]),
            Some(&out),
            Some(&err),
            |_| {},
        )
        .expect_err("start 가 실패했는데 성공했다");
        assert!(
            matches!(&error, ContainerRunError::Unobserved { detail, stopped: true, logs_complete: true, container: ContainerLeft::Removed } if detail.contains("시작했는지 모른다")),
            "{fail}: {error:?}"
        );
        assert!(!error.needs_human(), "{fail}: {error:?}");
        // 돌았을 수 있으니 지우기 전에 로그를 받았다.
        assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), "hello-out");
        let after_start: Vec<String> = call_order(&f.state)
            .into_iter()
            .skip_while(|c| c != "start")
            .collect();
        let logs = after_start.iter().position(|c| c == "logs");
        let rm = after_start.iter().position(|c| c == "rm");
        assert!(
            matches!((logs, rm), (Some(l), Some(r)) if l < r),
            "{fail}: logs → rm 순서가 아니다: {after_start:?}"
        );
    }
}

/// 결함 490 · 491 (재검수 124) — start 실패 뒤 멈춤을 확인하지 못하거나(정지 손잡이를 넘긴다) 로그를 받지 못하면 **지우지 않는다**
/// (전에는 응답 없음 경로가 로그를 못 받아도 지워 출력 · 원본 로그를 모두 잃었다). 둘 다 사람이 봐야 한다.
fn a_failed_start_keeps_the_container_unless_stop_and_logs_are_confirmed() {
    let f = fixture(Some("start,kill,inspect"));
    let mut handed_stopper = false;
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "sleep", &[]),
        None,
        None,
        |_| handed_stopper = true,
    )
    .expect_err("start 가 실패했는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::Unobserved { detail, stopped: false, container: ContainerLeft::Kept, .. } if detail.contains("멈춤을 확인하지 못했다")),
        "{error:?}"
    );
    assert!(error.needs_human());
    // 결함 475 — 돌고 있을 수 있으니 소유자가 멈출 수 있게 정지 손잡이를 넘겼다.
    assert!(
        handed_stopper,
        "시작 여부를 모르는데 정지 손잡이를 넘기지 않았다"
    );
    let after_start: Vec<String> = call_order(&f.state)
        .into_iter()
        .skip_while(|c| c != "start")
        .collect();
    assert!(
        !after_start.iter().any(|c| c == "rm"),
        "멈춤을 확인하지 못했는데 지웠다: {after_start:?}"
    );

    let f = fixture(Some("start-after-run,logs"));
    let out = f.work.join("stdout.log");
    let err = f.work.join("stderr.log");
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        Some(&out),
        Some(&err),
        |_| {},
    )
    .expect_err("start 가 실패했는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::Unobserved { detail, stopped: true, logs_complete: false, container: ContainerLeft::Kept } if detail.contains("로그 못 남김")),
        "{error:?}"
    );
    assert!(error.needs_human());
    assert!(!out.exists() && !err.exists(), "부분 출력 파일이 남았다");
    let after_start: Vec<String> = call_order(&f.state)
        .into_iter()
        .skip_while(|c| c != "start")
        .collect();
    assert!(
        !after_start.iter().any(|c| c == "rm"),
        "로그를 못 받았는데 지웠다: {after_start:?}"
    );
}

/// 결함 532 (재검수 137) — start 가 **응답 없이** 시한을 넘기면 그 start 가 뒤늦게 적용될 수 있다. 지금 멈춰 보여도(created) 멈춤 확인 · 로그 ·
/// 삭제로 가지 않고, 정지 손잡이를 넘긴 뒤 컨테이너를 남겨 사람에게 넘긴다. start 시한을 1초로 줄여 2분을 기다리지 않는다.
fn a_start_that_never_answers_keeps_the_container_for_a_human() {
    let f = fixture(Some("start-hangs"));
    let mut execution = execution();
    execution.runtime.start_timeout = Duration::from_secs(1);
    let mut handed_stopper = false;
    let begun = Instant::now();
    let error = container::run(
        &execution,
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| handed_stopper = true,
    )
    .expect_err("start 가 답하지 않았는데 성공했다");
    assert!(
        begun.elapsed() < Duration::from_secs(60),
        "start 시한을 1초로 줄였는데 오래 걸렸다: {:?}",
        begun.elapsed()
    );
    assert!(
        matches!(&error, ContainerRunError::Unobserved { detail, stopped: false, logs_complete: false, container: ContainerLeft::Kept } if detail.contains("뒤늦게 적용")),
        "{error:?}"
    );
    assert!(error.needs_human());
    assert!(
        handed_stopper,
        "start 가 뒤늦게 적용될 수 있는데 정지 손잡이를 넘기지 않았다"
    );
    let after_start: Vec<String> = call_order(&f.state)
        .into_iter()
        .skip_while(|c| c != "start")
        .skip(1)
        .collect();
    assert!(
        !after_start
            .iter()
            .any(|c| c == "rm" || c == "kill" || c == "logs"),
        "멈춰 보인다는 것만으로 정지 · 로그 · 삭제로 갔다: {after_start:?}"
    );
}

/// 결함 495 (재검수 124) — 로그를 못 받았는데 부분 파일도 지우지 못하면 그 사실을 알린다. 결과는 여전히 "로그 불완전 · 컨테이너 남김"
/// 이다(확정은 이 삭제가 아니라 `logs_complete` 로 실패한다 — exec · lib 시험). stdout 자리에 지울 수 없는 것(비지 않은 폴더)을 둔다.
fn an_undeletable_partial_output_is_reported_and_never_complete() {
    let f = fixture(None);
    let out = f.work.join("stdout.log");
    let err = f.work.join("stderr.log");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("keep"), "x").unwrap();
    let exit = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        Some(&out),
        Some(&err),
        |_| {},
    )
    .expect("종료는 봤다");
    assert_eq!(exit.exit_code, 0);
    assert!(!exit.logs_complete, "로그를 못 받았는데 완결이라 했다");
    assert_eq!(exit.container, ContainerLeft::Kept);
    assert!(exit.needs_human());
    assert!(
        out.is_dir(),
        "시험 전제 — 지울 수 없는 것이 남아 있어야 한다"
    );
}

/// 결함 481 (재검수 121) — 종료를 다섯 번 못 봐 kill · rm 을 했을 때, rm 이 성공하면 "지웠다"(RemovedUnobserved), 실패하면 "남아 돌 수 있다"(NotObserved).
fn an_unobserved_exit_is_removed_only_when_rm_succeeded() {
    // 종료 상태만 못 읽는다 — 멈춤 · 존재 확인은 된다(결함 502 · 503 뒤로는 그 확인 없이 지우지 않는다).
    let f = fixture(Some("inspect-exit"));
    let out = f.work.join("stdout.log");
    let err = f.work.join("stderr.log");
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "sleep", &[]),
        Some(&out),
        Some(&err),
        |_| {},
    )
    .expect_err("종료를 못 봤는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::Unobserved { detail, stopped: true, logs_complete: true, container: ContainerLeft::Removed } if detail.contains("rm 성공")),
        "{error:?}"
    );
    // 결함 483 — 지우기 전에 로그를 남겼다(지운 뒤에는 런타임 로그도 없다).
    assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), "hello-out");
    let order: Vec<String> = calls(&f.state)
        .lines()
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect();
    let logs = order
        .iter()
        .rposition(|c| c == "logs")
        .expect("logs 를 부르지 않았다");
    let removed = order.iter().rposition(|c| c == "rm").unwrap();
    assert!(logs < removed, "지운 뒤에 로그를 받으려 했다: {order:?}");

    let f = fixture(Some("inspect-exit,rm-created"));
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "sleep", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("종료를 못 봤는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::Unobserved { stopped: true, container: ContainerLeft::Unknown, detail, .. } if detail.contains("rm 실패")),
        "{error:?}"
    );
}

/// 결함 501 (재검수 126) — `start` 가 0 으로 답해도 컨테이너가 **시작한 흔적이 없으면** 종료 코드 0 으로 읽지 않는다(한 번도 돌지 않은 작업이
/// `WORKLOAD_RESULT ok=true` 가 되지 않게). 종료를 모르는 것으로 다룬다.
fn a_never_started_container_is_never_read_as_exit_zero() {
    let f = fixture(Some("start-noop"));
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        Some(&f.work.join("stdout.log")),
        Some(&f.work.join("stderr.log")),
        |_| {},
    )
    .expect_err("시작한 흔적이 없는데 종료 코드를 돌려줬다");
    assert!(
        matches!(&error, ContainerRunError::Unobserved { detail, .. } if detail.contains("INSPECT_NEVER_STARTED")),
        "{error:?}"
    );
    let f = fixture(Some("start-noop"));
    let outcome = gputeer_agent::exec::execute(
        &spec("exit-0"),
        policy(&f.work, ContainerDecision::Container(execution())),
    )
    .expect("멈춤은 확인했다 — 종료 코드 없음으로 확정 단계를 거친다");
    assert_eq!(
        outcome.exit.code(),
        None,
        "시작하지 않은 작업이 종료 코드를 가졌다"
    );
}

/// 결함 502 (재검수 126) — `kill` 이 0 으로 답해도 `inspect` 로 멈춤을 확인하기 전에는 멈췄다고 보지 않는다. 확인하지 못하면 지우지 않는다.
fn a_kill_that_does_not_stop_is_not_a_stop() {
    let f = fixture(Some("inspect-exit,kill-noop"));
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "sleep", &[]),
        Some(&f.work.join("stdout.log")),
        Some(&f.work.join("stderr.log")),
        |_| {},
    )
    .expect_err("종료를 못 봤는데 성공했다");
    assert!(
        matches!(
            &error,
            ContainerRunError::Unobserved {
                stopped: false,
                logs_complete: false,
                container: ContainerLeft::Kept,
                ..
            }
        ),
        "{error:?}"
    );
    let after_start: Vec<String> = call_order(&f.state)
        .into_iter()
        .skip_while(|c| c != "start")
        .collect();
    assert!(
        !after_start.iter().any(|c| c == "rm"),
        "멈춤을 확인하지 못했는데 지웠다: {after_start:?}"
    );
    // 남은 컨테이너 정리도 같다.
    let f = fixture(Some("kill-noop"));
    std::fs::write(f.state.join("leftovers"), "old-1\n").unwrap();
    let error = container::remove_leftovers(&execution().runtime, Some(&f.work.join("salvage")))
        .expect_err("멈추지 않은 것을 치웠다고 했다");
    assert!(error.contains("멈췄는지 확인하지 못했다"), "{error}");
    assert!(!calls(&f.state).lines().any(|l| l.starts_with("rm ")));
}

/// 결함 503 (재검수 126) — `rm` 이 0 으로 답해도 `inspect` 가 "없다" 고 할 때만 지웠다고 본다. 남아 있으면 사람이 본다(작업 폴더 · 표식).
fn an_rm_that_leaves_the_container_is_not_a_removal() {
    let f = fixture(Some("rm-noop"));
    let exit = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        Some(&f.work.join("stdout.log")),
        Some(&f.work.join("stderr.log")),
        |_| {},
    )
    .expect("종료는 봤다");
    assert_eq!(exit.exit_code, 0);
    assert_eq!(exit.container, ContainerLeft::Unknown);
    assert!(exit.needs_human());
    // 결함 504 — 사유가 결과(와 표식)에 실린다.
    assert!(exit.note.contains("아직 있다"), "{}", exit.note);
    let f = fixture(Some("rm-noop"));
    std::fs::write(f.state.join("leftovers"), "old-1\n").unwrap();
    let error = container::remove_leftovers(&execution().runtime, Some(&f.work.join("salvage")))
        .expect_err("남아 있는 것을 치웠다고 했다");
    assert!(error.contains("지웠는지 확인하지 못했다"), "{error}");
}

/// 결함 487 (재검수 123) — 로그를 못 받으면 반쯤 쓴 출력 파일을 지우고(빈 출력이 성공이 되지 않게) 컨테이너를 남긴다(런타임에 온전한 로그가 남는다).
fn unsaved_logs_leave_no_partial_output_and_keep_the_container() {
    let order = |state: &Path| -> Vec<String> {
        calls(state)
            .lines()
            .map(|l| l.split(' ').next().unwrap().to_string())
            .collect()
    };
    // 정상 종료 — 종료 코드는 돌려주지만 출력 파일은 없고 마지막 rm 도 없다.
    let f = fixture(Some("logs"));
    let out = f.work.join("stdout.log");
    let err = f.work.join("stderr.log");
    let exit = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        Some(&out),
        Some(&err),
        |_| {},
    )
    .expect("종료는 봤다");
    assert_eq!(exit.exit_code, 0);
    assert!(!out.exists() && !err.exists(), "반쯤 쓴 출력 파일이 남았다");
    assert_eq!(
        order(&f.state).last().map(String::as_str),
        Some("logs"),
        "{}",
        calls(&f.state)
    );

    // 종료를 못 봄 — kill 은 성공, 로그 실패 → 컨테이너를 남기고(지우지 않고) 출력 파일도 없다.
    let f = fixture(Some("inspect-exit,logs"));
    let out = f.work.join("stdout.log");
    let err = f.work.join("stderr.log");
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "sleep", &[]),
        Some(&out),
        Some(&err),
        |_| {},
    )
    .expect_err("종료를 못 봤는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::Unobserved { detail, stopped: true, logs_complete: false, container: ContainerLeft::Kept } if detail.contains("컨테이너를 남겼다")),
        "{error:?}"
    );
    assert!(!out.exists() && !err.exists(), "반쯤 쓴 출력 파일이 남았다");
    let calls_after_start: Vec<String> = order(&f.state)
        .into_iter()
        .skip_while(|c| c != "start")
        .collect();
    assert!(
        !calls_after_start.iter().any(|c| c == "rm"),
        "로그를 못 받았는데 지웠다: {calls_after_start:?}"
    );
}

fn call_order(state: &Path) -> Vec<String> {
    calls(state)
        .lines()
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect()
}

/// 2026-09-27 보수 규칙(코덱스 지적) — 종료를 못 봐 kill 했는데 멈춤을 확인하지 못하면 **지우지 않는다**(전에는 `rm -f` 로 가 로그를 받은 뒤의
/// 출력 · 체크포인트를 잃을 수 있었다). 결과는 "멈춤 모름 · 컨테이너 남김" 이다.
fn an_unconfirmed_stop_never_removes_the_container() {
    let f = fixture(Some("inspect,kill"));
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "sleep", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("종료를 못 봤는데 성공했다");
    assert!(
        matches!(
            &error,
            ContainerRunError::Unobserved {
                stopped: false,
                container: ContainerLeft::Kept,
                logs_complete: false,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(error.needs_human());
    let after_start: Vec<String> = call_order(&f.state)
        .into_iter()
        .skip_while(|c| c != "start")
        .collect();
    assert!(
        !after_start.iter().any(|c| c == "rm"),
        "멈춤을 확인하지 못했는데 지웠다: {after_start:?}"
    );
}

/// 2026-09-27 보수 규칙(코덱스 지적) — 만들기 전에 같은 이름의 남은 컨테이너를 지우지 못하면 **만들지 않는다**(전에는 결과를 버렸다).
fn an_unremovable_same_name_container_blocks_create() {
    // 결함 530 뒤로 만들기 전에는 지우지 않는다 — 같은 이름이 남아 있으면(이 Agent 의 것이어도) 만들지 않고 남긴다.
    let f = fixture(None);
    std::fs::write(
        f.state.join("leftovers"),
        format!("gputeer-test\n{LEFTOVER_ID}\n"),
    )
    .unwrap();
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("같은 이름이 남아 있는데 만들었다");
    assert!(
        matches!(
            &error,
            ContainerRunError::NotStarted {
                container: ContainerLeft::Kept,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(error.needs_human());
    assert!(
        !call_order(&f.state)
            .iter()
            .any(|c| c == "create" || c == "rm"),
        "{}",
        calls(&f.state)
    );
}

/// 2026-09-27 보수 규칙 — 정상 종료 뒤 지우기만 실패하면 **관측한 종료는 그대로** 돌려주고(정리 실패를 작업 실패로 바꾸지 않는다) 정리 결과는
/// 따로 싣는다. 사건 표식 폴더가 있으면 표식을 **남긴다**(덮지 않는다) — 정리가 된 실행은 표식을 남기지 않는다. 해제는 명시적이다.
fn a_cleanup_failure_keeps_the_observed_exit_and_records_an_incident() {
    let f = fixture(Some("rm-created"));
    let incidents = f.work.join("container-incidents");
    let mut execution = execution();
    execution.runtime.incident_dir = Some(incidents.clone());
    let out = f.work.join("stdout.log");
    let err = f.work.join("stderr.log");
    let exit = container::run(
        &execution,
        &input(&mounts(&f.work), "exit-3", &[]),
        Some(&out),
        Some(&err),
        |_| {},
    )
    .expect("종료는 봤다");
    assert_eq!(exit.exit_code, 3, "관측한 종료 코드를 잃었다");
    assert!(exit.logs_complete);
    assert_eq!(exit.container, ContainerLeft::Unknown);
    assert!(exit.needs_human());
    let open = container::open_incidents(&incidents).unwrap();
    assert_eq!(open.len(), 1, "{open:?}");
    let body = std::fs::read_to_string(&open[0]).unwrap();
    assert!(
        body.contains("kind=EXITED") && body.contains("container=gputeer-test"),
        "{body}"
    );
    // 결함 504 (재검수 126) — 표식에 **왜**(런타임의 rm 오류)가 남는다.
    assert!(
        body.contains("rm 실패") && body.contains("fake: rm 실패를 흉내낸다"),
        "{body}"
    );
    assert!(container::incident_recorded_for(&incidents, "gputeer-test"));

    // 같은 이름이 다시 사건을 남기면 덮지 않고 새 파일이다.
    let f2 = fixture(Some("rm-created"));
    let exit = container::run(
        &execution,
        &input(&mounts(&f2.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect("종료는 봤다");
    assert!(exit.needs_human());
    assert_eq!(container::open_incidents(&incidents).unwrap().len(), 2);

    // 정리가 된 실행은 표식을 남기지 않는다.
    let f3 = fixture(None);
    container::run(
        &execution,
        &input(&mounts(&f3.work), "exit-0", &[]),
        Some(&f3.work.join("o.log")),
        Some(&f3.work.join("e.log")),
        |_| {},
    )
    .expect("정상");
    assert_eq!(container::open_incidents(&incidents).unwrap().len(), 2);

    // 해제는 명시적 — 이름으로 지운다.
    let cleared = container::clear_incidents(&incidents, Some("gputeer-test")).unwrap();
    assert_eq!(cleared.len(), 2);
    assert!(container::open_incidents(&incidents).unwrap().is_empty());
    assert!(!container::incident_recorded_for(
        &incidents,
        "gputeer-test"
    ));
}

/// 결함 493 (재검수 124) — 같은 id 의 로그를 전에 건졌으면 덮지 않고 새 번호로 쓴다.
fn salvaged_leftover_logs_are_never_overwritten() {
    let f = fixture(None);
    std::fs::write(f.state.join("leftovers"), "old-1\n").unwrap();
    let salvage = f.work.join("leftover-container-logs");
    std::fs::create_dir_all(&salvage).unwrap();
    std::fs::write(salvage.join("old-1.stdout.log"), "earlier").unwrap();
    container::remove_leftovers(&execution().runtime, Some(&salvage)).expect("정리");
    assert_eq!(
        std::fs::read_to_string(salvage.join("old-1.stdout.log")).unwrap(),
        "earlier",
        "전에 건진 로그를 덮었다"
    );
    assert_eq!(
        std::fs::read_to_string(salvage.join("old-1.1.stdout.log"))
            .unwrap()
            .trim(),
        "hello-out"
    );
}

/// 보수 규칙(재검수 124 합의) — 멈췄는지 확인하지 못하거나 로그를 건지지 못하면 **지우지 않고** Err(기동을 막는다).
fn a_leftover_is_kept_when_stop_or_log_salvage_is_unconfirmed() {
    for (fail, expected) in [
        ("kill,inspect", "멈췄는지 확인하지 못했다"),
        ("logs", "로그를 건지지 못해 지우지 않았다"),
    ] {
        let f = fixture(Some(fail));
        std::fs::write(f.state.join("leftovers"), "old-1\n").unwrap();
        let salvage = f.work.join("leftover-container-logs");
        let error = container::remove_leftovers(&execution().runtime, Some(&salvage))
            .expect_err("확인하지 못했는데 치웠다고 했다");
        assert!(error.contains(expected), "{fail}: {error}");
        let calls = calls(&f.state);
        assert!(
            !calls.lines().any(|l| l.starts_with("rm ")),
            "{fail}: 확인하지 못했는데 지웠다:\n{calls}"
        );
        assert!(
            !salvage.join("old-1.stdout.log").exists(),
            "{fail}: 부분 로그 파일이 남았다"
        );
    }
}

/// 결함 490 · 495 (재검수 124) — 실행기 수준에서: 로그를 못 받았으면 종료 관측은 그대로 두고 **출력 불완전**을 싣는다(확정이 부분 파일 삭제에
/// 기대지 않게). start 가 실패로 답해도 SpawnFailed(작업 폴더 삭제)가 아니라 "종료는 봤고 코드가 없다" 로 확정 단계를 거친다.
fn execute_marks_unreceived_logs_as_incomplete_outputs() {
    let f = fixture(Some("logs"));
    let outcome = gputeer_agent::exec::execute(
        &spec("exit-7"),
        policy(&f.work, ContainerDecision::Container(execution())),
    )
    .expect("종료는 봤다");
    assert_eq!(outcome.exit.code(), Some(7), "관측한 종료 코드를 잃었다");
    assert!(
        outcome.outputs_incomplete.is_some(),
        "로그를 못 받았는데 출력이 완결이라 했다"
    );

    for (fail, incomplete) in [("start-after-run", false), ("start-after-run,logs", true)] {
        let f = fixture(Some(fail));
        let outcome = gputeer_agent::exec::execute(
            &spec("exit-0"),
            policy(&f.work, ContainerDecision::Container(execution())),
        )
        .unwrap_or_else(|e| panic!("{fail}: start 실패를 실행 안 함으로 보고했다: {e:?}"));
        assert_eq!(outcome.exit.code(), None, "{fail}");
        assert_eq!(
            outcome.outputs_incomplete.is_some(),
            incomplete,
            "{fail}: {outcome:?}"
        );
    }
}

/// 결함 497 (재검수 125) — 표식 폴더에 쓰지 못해도 **관측한 종료 코드 · 로그 완결을 지우지 않는다**(전에는 결과를 통째로 "멈춤 모름" 으로 바꿨다).
/// 결과는 여전히 "사람 필요" 라 작업 폴더가 남고, 회차를 멈추는 것은 agent-loop 가 `CONTAINER_INCIDENT_NOT_RECORDED` 를 보고 한다.
fn an_unwritable_incident_dir_keeps_the_observed_exit() {
    let f = fixture(Some("rm-created"));
    // 표식 폴더 자리에 **파일**이 있다 — 폴더를 만들 수도, 그 안에 쓸 수도 없다.
    let blocked = f.work.join("container-incidents");
    std::fs::write(&blocked, b"not a directory").unwrap();
    let mut execution = execution();
    execution.runtime.incident_dir = Some(blocked.clone());
    let exit = container::run(
        &execution,
        &input(&mounts(&f.work), "exit-3", &[]),
        Some(&f.work.join("stdout.log")),
        Some(&f.work.join("stderr.log")),
        |_| {},
    )
    .expect("표식을 못 써도 관측한 종료는 그대로다");
    assert_eq!(exit.exit_code, 3, "관측한 종료 코드를 잃었다");
    assert!(exit.logs_complete, "완결된 로그를 잃었다");
    assert_eq!(exit.container, ContainerLeft::Unknown);
    assert!(exit.needs_human());
    assert!(
        container::probe_incident_dir(&blocked).is_err(),
        "쓸 수 없는 폴더가 기동 관문을 통과했다"
    );
}

/// 결함 498 · 499 (재검수 125) — 표식 폴더의 **모든** 항목이 열린 사건이다(쓰다 만 파일 · 모르는 파일도). 폴더를 읽지 못하면 "있다" 로 본다.
/// 쓰기 시험은 흔적을 남기지 않는다.
fn every_file_in_the_incident_dir_is_open_and_unknown_means_open() {
    let dir = tempfile::tempdir().unwrap();
    let incidents = dir.path().join("container-incidents");
    container::probe_incident_dir(&incidents).expect("빈 폴더에는 쓸 수 있다");
    assert!(
        container::open_incidents(&incidents).unwrap().is_empty(),
        "쓰기 시험이 흔적을 남겼다"
    );
    std::fs::write(incidents.join(".gputeer-test.1.tmp"), b"").unwrap();
    assert_eq!(
        container::open_incidents(&incidents).unwrap().len(),
        1,
        "쓰다 만 파일을 세지 않았다"
    );
    assert!(!container::incident_recorded_for(
        &incidents,
        "gputeer-other"
    ));
    // 폴더 자리에 파일 — 읽지 못한다 → 있다고 본다.
    let unreadable = dir.path().join("not-a-dir");
    std::fs::write(&unreadable, b"x").unwrap();
    assert!(container::open_incidents(&unreadable).is_err());
    assert!(
        container::incident_recorded_for(&unreadable, "gputeer-test"),
        "표식 폴더를 읽지 못했는데 없다고 봤다"
    );
}

/// 결함 500 (재검수 125) — 같은 이름의 사건을 몰아서 써도(같은 밀리초) 서로 덮지 않는다.
fn incidents_for_the_same_name_never_overwrite_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let incidents = dir.path().join("container-incidents");
    let mut written = std::collections::BTreeSet::new();
    for n in 0..50 {
        // 같은 밀리초로 고정한다 — 쓰기마다 시각이 달라지면 충돌이 일어나지 않아 시험이 공허하다.
        let path = container::write_incident_at(
            &incidents,
            "gputeer-test",
            "node",
            "EXITED",
            &format!("사건 {n}"),
            1_700_000_000_000,
        )
        .unwrap();
        assert!(written.insert(path), "같은 파일에 두 번 썼다");
    }
    let open = container::open_incidents(&incidents).unwrap();
    assert_eq!(open.len(), 50);
    for n in 0..50 {
        let needle = format!("detail=사건 {n}\n");
        assert!(
            open.iter()
                .any(|path| std::fs::read_to_string(path).unwrap().contains(&needle)),
            "사건 {n} 이 덮였다"
        );
    }
}

/// 결함 499 (재검수 125) — 실행기가 "사람이 봐야 한다" 를 **타입으로** 호출자(lib — 작업 폴더를 남길지)에게 넘긴다. 문자열 접두사가 아니다.
fn execute_hands_the_needs_human_verdict_to_the_caller() {
    // 정상 종료 · 지우기 성공 → 사람 불필요.
    let f = fixture(None);
    let outcome = gputeer_agent::exec::execute(
        &spec("exit-0"),
        policy(&f.work, ContainerDecision::Container(execution())),
    )
    .expect("실행");
    assert!(!outcome.container_needs_human, "{outcome:?}");
    // 정상 종료 · 지우기만 실패 → 종료 코드는 그대로 · 사람 필요.
    let f = fixture(Some("rm-created"));
    let outcome = gputeer_agent::exec::execute(
        &spec("exit-0"),
        policy(&f.work, ContainerDecision::Container(execution())),
    )
    .expect("종료는 봤다");
    assert_eq!(outcome.exit.code(), Some(0));
    assert!(
        outcome.container_needs_human,
        "정리 실패를 호출자에게 넘기지 않았다"
    );
    // start 실패 · 로그 못 받음(컨테이너 남김) → 종료 코드 없음 · 사람 필요.
    let f = fixture(Some("start-after-run,logs"));
    let outcome = gputeer_agent::exec::execute(
        &spec("exit-0"),
        policy(&f.work, ContainerDecision::Container(execution())),
    )
    .expect("start 실패는 종료 코드 없음으로 확정 단계를 거친다");
    assert!(
        outcome.container_needs_human,
        "남긴 컨테이너를 호출자에게 넘기지 않았다"
    );
    // 종료를 못 봤고 멈춤도 확인하지 못함 → 작업이 돌 수 있다(작업 폴더를 남긴다).
    let f = fixture(Some("inspect,kill"));
    let error = gputeer_agent::exec::execute(
        &spec("sleep"),
        policy(&f.work, ContainerDecision::Container(execution())),
    )
    .expect_err("멈춤을 모르는데 종료를 봤다고 했다");
    assert!(error.workload_may_be_alive(), "{error:?}");
    // 같은 이름을 못 지워 만들지 않음 → 남은 것이 돌 수 있다.
    let f = fixture(Some("rm"));
    // 실행기는 시도 id 에서 이름을 만든다 — 그 이름의 컨테이너가 실제로 남아 있다.
    std::fs::write(
        f.state.join("leftovers"),
        format!(
            "{}\n{LEFTOVER_ID}\n",
            container::derive_container_name("attempt")
        ),
    )
    .unwrap();
    let error = gputeer_agent::exec::execute(
        &spec("exit-0"),
        policy(&f.work, ContainerDecision::Container(execution())),
    )
    .expect_err("남은 것을 못 지웠는데 만들었다");
    assert!(error.workload_may_be_alive(), "{error:?}");
}

/// 결함 506 (재검수 127) — 소유자 화면의 정지도 `kill` 의 0 만으로 "멈췄다" 고 답하지 않는다. 멈춤을 확인하지 못하면 실패로 돌려준다(패널이
/// `owner_stopped` 를 적지 않게 — 계속 돈 작업이 나중에 끝나도 INTERRUPTED 로 바뀌어 재배치되지 않게).
fn the_owner_stop_is_not_reported_until_the_container_stopped() {
    let f = fixture(Some("kill-noop"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            container::run(
                &execution(),
                &input(&mounts(&work), "sleep", &[]),
                None,
                None,
                move |stopper| {
                    tx.send(stopper).unwrap();
                },
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    let error = stopper.stop().expect_err("멈추지 않았는데 멈췄다고 했다");
    assert!(error.starts_with("KILL_UNCONFIRMED"), "{error}");
    // 이제 실제로 멈춘다(가짜 런타임 안에서) — 실행이 종료를 본다.
    std::fs::write(f.state.join("killed"), "").unwrap();
    let exit = runner.join().unwrap().expect("멈춘 뒤 종료 관측");
    assert_eq!(exit.exit_code, 137);
}

/// 결함 515 (재검수 131) — 만든 뒤의 모든 조작은 create 가 돌려준 **컨테이너 ID** 로 한다(이름이 아니다 — 그 사이 같은 이름의 다른 컨테이너를
/// 이 작업의 것으로 읽고 지우지 않게). 만들기 전의 같은 이름 정리만 이름을 쓴다.
fn everything_after_create_uses_the_container_id() {
    let f = fixture(None);
    container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        Some(&f.work.join("stdout.log")),
        Some(&f.work.join("stderr.log")),
        |_| {},
    )
    .expect("실행");
    let calls = calls(&f.state);
    let after_create: Vec<&str> = calls
        .lines()
        .skip_while(|l| !l.starts_with("create "))
        .skip(1)
        .collect();
    assert!(!after_create.is_empty(), "{calls}");
    for line in &after_create {
        assert!(
            line.ends_with(&format!(" {FAKE_ID}")) && !line.contains("gputeer-test"),
            "만든 뒤의 조작을 이름으로 했다: {line}\n{calls}"
        );
    }
    // create 가 ID 를 돌려주지 않으면 시작하지 않는다.
    let f = fixture(Some("create-no-id"));
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("ID 없이 시작했다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { detail, .. } if detail.contains("ID")),
        "{error:?}"
    );
    assert!(!call_order(&f.state).iter().any(|c| c == "start"));
}

/// 결함 516 (재검수 131) — rm 이 "No such container" 로 실패해도 조회로 확인하기 전에는 없다고 보지 않는다(낡은 오류면 남은 컨테이너를 놓친다).
fn a_stale_no_such_container_from_rm_is_checked() {
    // 실행 뒤 지우기 — rm 이 "No such container" 로 실패해도(낡은 오류) 조회가 "있다" 고 하면 지웠다고 보지 않는다.
    let f = fixture(Some("rm-stale-nosuch"));
    let exit = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        Some(&f.work.join("stdout.log")),
        Some(&f.work.join("stderr.log")),
        |_| {},
    )
    .expect("종료는 봤다");
    assert_eq!(exit.exit_code, 0);
    assert_eq!(exit.container, ContainerLeft::Unknown, "{exit:?}");
    assert!(exit.needs_human());
}

/// 결함 519 (재검수 132) — create 가 실패하거나 돌려준 ID 를 이 시도의 컨테이너로 확인하지 못하면 **아무것도 지우지 않고** 사람에게 넘긴다
/// (같은 이름의 다른 컨테이너일 수 있다).
fn a_create_that_cannot_be_bound_to_this_attempt_deletes_nothing() {
    let rm_after_create = |state: &Path| {
        calls(state)
            .lines()
            .skip_while(|l| !l.starts_with("create "))
            .any(|l| l.starts_with("rm "))
    };
    // create 가 실패 — 이름으로 지우지 않고 같은 이름이 있는지만 본다(가짜 런타임은 사전 정리 뒤 B 가 생기는 경쟁을 흉내 내지 못한다 —
    // 여기서는 create 실패 뒤 rm 이 없는지만 본다).
    let f = fixture(Some("create"));
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("create 가 실패했는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { .. }),
        "{error:?}"
    );
    assert!(!rm_after_create(&f.state), "{}", calls(&f.state));
    // create 가 ID 를 찍지 않음 · 다른 컨테이너를 가리키는 ID — 시작도 삭제도 없이 사람에게.
    for fail in ["create-no-id", "create-name-mismatch"] {
        let f = fixture(Some(fail));
        let error = container::run(
            &execution(),
            &input(&mounts(&f.work), "exit-0", &[]),
            None,
            None,
            |_| {},
        )
        .expect_err(fail);
        assert!(
            matches!(&error, ContainerRunError::NotStarted { container: ContainerLeft::Unknown, detail } if detail.contains("확인하지 못했다")),
            "{fail}: {error:?}"
        );
        assert!(error.needs_human(), "{fail}");
        assert!(!call_order(&f.state).iter().any(|c| c == "start"), "{fail}");
        assert!(!rm_after_create(&f.state), "{fail}: {}", calls(&f.state));
    }
}

/// 결함 520 (재검수 132) — 소유자 정지 중 kill 이 무동작인 사이 작업이 스스로 코드 0 으로 끝나면, 그것은 소유자 정지가 **아니다**
/// (성공이라 하면 코드 0 이 INTERRUPTED 로 바뀌어 다시 실행된다).
fn a_natural_exit_during_an_owner_stop_is_not_an_owner_stop() {
    let f = fixture(Some("kill-noop-exit0"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            container::run(
                &execution(),
                &input(&mounts(&work), "sleep", &[]),
                None,
                None,
                move |stopper| {
                    tx.send(stopper).unwrap();
                },
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    let error = stopper
        .stop()
        .expect_err("스스로 끝난 것을 소유자 정지라 했다");
    assert!(error.starts_with("ALREADY_EXITED"), "{error}");
    let exit = runner.join().unwrap().expect("종료 관측");
    assert_eq!(exit.exit_code, 0, "관측한 자연 종료를 잃었다");
}

/// 결함 522 (재검수 133) — 만들기 전에 같은 이름의 컨테이너가 있어도 **owner 라벨이 이 Agent 의 것이 아니면** 지우지 않는다(다른 Agent · 운영 절차의
/// 것일 수 있다). 만들지도 않고 사람에게 넘긴다.
fn a_same_name_container_of_another_owner_is_never_removed() {
    let f = fixture(Some("leftover-foreign"));
    std::fs::write(f.state.join("leftovers"), "gputeer-test\n").unwrap();
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("남의 컨테이너가 있는데 만들었다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { container: ContainerLeft::Unknown, detail } if detail.contains("이 Agent 의 것이 아니다")),
        "{error:?}"
    );
    assert!(error.needs_human());
    let order = call_order(&f.state);
    assert!(
        !order.iter().any(|c| c == "rm" || c == "create"),
        "{order:?}"
    );
}

/// 결함 525 — 소유자 정지 직후 실행 쪽이 종료(137)를 보고 로그를 받아 컨테이너를 **먼저 지워도**, 정지 손잡이는 실행 쪽이 지우기 전에 나눈 관측으로
/// "이 정지가 원인" 을 판정한다(전에는 사후 조회가 "없다" 를 받아 정지 실패로 보고했다 — 부하에서 전체 시험이 한 번 떨어져 드러났다).
fn an_owner_stop_is_judged_even_after_the_run_cleaned_up() {
    let f = fixture(Some("kill-waits-for-rm"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            container::run(
                &execution(),
                &input(&mounts(&work), "sleep", &[]),
                None,
                None,
                move |stopper| {
                    tx.send(stopper).unwrap();
                },
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    stopper
        .stop()
        .expect("실행 쪽이 이미 지웠어도 이 정지가 원인이다");
    let exit = runner.join().unwrap().expect("종료 관측");
    assert_eq!(exit.exit_code, 137);
    assert_eq!(exit.container, ContainerLeft::Removed);
}

/// 결함 530 (재검수 135) — 만들기 직전 같은 이름 · **같은 owner** 컨테이너를 찾아도 자동으로 지우지 않는다(멈춤 · 로그를 확인하지 않은 강제 삭제가
/// 된다). 만들지 않고 사람에게 넘긴다 — 해제하면 다음 기동의 남은 컨테이너 정리가 멈춤 · 로그 건지기를 거쳐 치운다. (526 — 이름으로도 지우지 않는다.)
fn our_same_name_leftover_is_never_force_removed_before_create() {
    let f = fixture(None);
    std::fs::write(
        f.state.join("leftovers"),
        format!("gputeer-test\n{LEFTOVER_ID}\n"),
    )
    .unwrap();
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("남은 것이 있는데 만들었다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { container: ContainerLeft::Kept, detail } if detail.contains(LEFTOVER_ID)),
        "{error:?}"
    );
    assert!(error.needs_human());
    let order = call_order(&f.state);
    assert!(
        !order
            .iter()
            .any(|c| c == "rm" || c == "kill" || c == "create"),
        "{order:?}"
    );
}

/// 결함 527 (재검수 134) — kill 이 실제로 끝냈는데 **실패로 답해도**, 멈춘 원인(137)을 보고 소유자 정지로 판정한다(실패 응답은 죽이지 않았다는 증거가 아니다).
fn a_kill_that_answers_failure_after_killing_is_still_an_owner_stop() {
    let f = fixture(Some("kill-fails-after-kill"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            container::run(
                &execution(),
                &input(&mounts(&work), "sleep", &[]),
                None,
                None,
                move |stopper| {
                    tx.send(stopper).unwrap();
                },
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    stopper
        .stop()
        .expect("kill 이 실패로 답했어도 137 로 멈췄으면 소유자 정지다");
    let exit = runner.join().unwrap().expect("종료 관측");
    assert_eq!(exit.exit_code, 137);
}

/// 결함 529 (재검수 135) — kill 이 실패로 답했고 첫 조회에서 아직 돌아도 곧바로 정지 실패라 하지 않는다 — SIGKILL 이 진행 중이면 곧 137 로 끝난다.
fn a_kill_that_answers_failure_then_takes_effect_is_still_an_owner_stop() {
    let f = fixture(Some("kill-fails-then-dies"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            container::run(
                &execution(),
                &input(&mounts(&work), "sleep", &[]),
                None,
                None,
                move |stopper| {
                    tx.send(stopper).unwrap();
                },
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    stopper
        .stop()
        .expect("kill 이 실패로 답했어도 곧 137 로 멈췄으면 소유자 정지다");
    let exit = runner.join().unwrap().expect("종료 관측");
    assert_eq!(exit.exit_code, 137);
}

/// 결함 531 (재검수 136) — kill 이 작업을 끝냈는데 **응답 없이 시한을 넘겨도**, 나눈 관측 · 조회로 이 정지가 원인(137)임을 판정한다(전에는 확인 없이
/// 곧바로 실패였다). ★ kill 시한(15초)을 넘겨야 하므로 이 시험은 15초쯤 걸린다.
fn a_kill_that_never_answers_but_took_effect_is_still_an_owner_stop() {
    let f = fixture(Some("kill-hangs-after-kill"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            container::run(
                &execution(),
                &input(&mounts(&work), "sleep", &[]),
                None,
                None,
                move |stopper| {
                    tx.send(stopper).unwrap();
                },
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    stopper
        .stop()
        .expect("kill 이 응답하지 않았어도 137 로 멈췄으면 소유자 정지다");
    let exit = runner.join().unwrap().expect("종료 관측");
    assert_eq!(exit.exit_code, 137);
}

/// 결함 536 (재검수 138) — kill 을 **띄우지 못했으면**(런타임 실행 파일이 없다) 정지 신호가 전달되지 않은 것이 확실하다 — 작업이 스스로 137 로 끝난
/// 관측이 있어도 소유자 정지로 삼지 않는다.
fn a_kill_that_could_not_even_be_sent_is_never_an_owner_stop() {
    let f = fixture(None);
    // 런타임을 사본으로 두고, 실행이 끝난 뒤 지운다 — 그 뒤의 kill 은 띄워지지 않는다.
    let runtime_copy = f.work.join("runtime-copy.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &runtime_copy).unwrap();
    let mut execution = execution();
    execution.runtime.program = runtime_copy.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let exit = container::run(
        &execution,
        &input(&mounts(&f.work), "exit-137", &[]),
        None,
        None,
        move |stopper| {
            tx.send(stopper).unwrap();
        },
    )
    .expect("작업이 스스로 137 로 끝났다");
    assert_eq!(exit.exit_code, 137);
    let stopper = rx.recv().expect("손잡이");
    std::fs::remove_file(&runtime_copy).unwrap();
    let error = stopper
        .stop()
        .expect_err("kill 을 띄우지 못했는데 소유자 정지라 했다");
    assert!(error.starts_with("OWNER_STOP_NOT_SENT"), "{error}");
}

/// 결함 537 (재검수 139) — start 를 **띄우지 못했으면** 요청이 런타임에 닿지 않았다 — 시작하지 않았다(`NotStarted`). 정지 손잡이를 넘기지 않는다
/// (`WORKLOAD_SPAWNED` · 패널 등록 없음).
fn a_start_that_could_not_be_sent_did_not_start() {
    let f = fixture(Some("create-then-vanish"));
    let runtime_copy = f.work.join("runtime-copy.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &runtime_copy).unwrap();
    let mut execution = execution();
    execution.runtime.program = runtime_copy.clone();
    let mut started = false;
    let error = container::run(
        &execution,
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| started = true,
    )
    .expect_err("start 를 띄우지 못했는데 성공했다");
    assert!(!started, "시작하지 않았는데 정지 손잡이를 넘겼다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { detail, .. } if detail.contains("start 를 띄우지 못했다")),
        "{error:?}"
    );
    assert!(
        !call_order(&f.state).iter().any(|c| c == "start"),
        "{}",
        calls(&f.state)
    );
}

/// 결함 546 (재검수 142) — create 를 **띄우지 못했으면** 요청이 런타임에 닿지 않았다 — 컨테이너는 없다(`Removed` · 사람 불필요 · 표식 없음).
fn a_create_that_could_not_be_sent_leaves_nothing_for_a_human() {
    // 가짜 런타임은 만들기 전 owner 조회에 "없다" 로 답한 뒤 자기 사본의 이름을 바꾼다 — 이어지는 create 는 띄워지지 않는다.
    let f = fixture(Some("vanish-before-create"));
    let runtime_copy = f.work.join("runtime-copy.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &runtime_copy).unwrap();
    let mut execution = execution();
    execution.runtime.program = runtime_copy.clone();
    let error = container::run(
        &execution,
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("create 를 띄우지 못했는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { container: ContainerLeft::Removed, detail } if detail.contains("create 를 띄우지 못했다")),
        "{error:?}"
    );
    assert!(
        !error.needs_human(),
        "만들지 않은 컨테이너를 사람에게 넘겼다"
    );
}

/// 결함 545 (재검수 142) — kill 은 끝났지만 파이프를 물려받은 보조 프로세스가 남아 EOF 가 오지 않아도, 소유자 정지는 시한(15초 + 여유) 안에 판정을
/// 마친다(전에는 보조 프로세스가 끝날 때까지 — 여기서는 60초 — 멈췄다).
fn a_pipe_held_open_after_exit_does_not_hang_the_owner_stop() {
    let f = fixture(Some("kill-leaves-pipe-holder"));
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = {
        let work = f.work.clone();
        std::thread::spawn(move || {
            container::run(
                &execution(),
                &input(&mounts(&work), "sleep", &[]),
                None,
                None,
                move |stopper| {
                    tx.send(stopper).unwrap();
                },
            )
        })
    };
    let stopper = rx.recv_timeout(Duration::from_secs(20)).expect("손잡이");
    let started = Instant::now();
    stopper.stop().expect("137 로 멈췄으면 소유자 정지다");
    assert!(
        started.elapsed() < Duration::from_secs(40),
        "파이프가 닫히기를 시한 없이 기다렸다({:?})",
        started.elapsed()
    );
    let exit = runner.join().unwrap().expect("종료 관측");
    assert_eq!(exit.exit_code, 137);
}
