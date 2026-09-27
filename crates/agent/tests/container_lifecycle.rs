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
            "a_failed_start_is_not_started_only_when_the_container_was_removed",
            a_failed_start_is_not_started_only_when_the_container_was_removed,
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
fn fake_runtime(state: &Path) -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    append(state, "calls", &format!("{}\n", args.join(" ")));
    // 쉼표로 여럿을 실패시킬 수 있다(예 "start,rm").
    let fail_list = std::env::var(FAIL_ENV).unwrap_or_default();
    let fails = |token: &str| fail_list.split(',').any(|c| c == token);
    // "rm-created" — 컨테이너를 만든 뒤의 rm 만 실패시킨다(만들기 전 같은 이름 지우기는 통과 · 2026-09-27 보수 규칙 시험).
    let created = state.join("create.args").exists();
    if fails(command) || (command == "rm" && created && fails("rm-created")) {
        eprintln!("fake: {command} 실패를 흉내낸다");
        return 125;
    }
    let behaviour = || std::fs::read_to_string(state.join("behaviour")).unwrap_or_default();
    match command {
        "create" => {
            let entry = args
                .iter()
                .find_map(|a| a.strip_prefix("--entrypoint="))
                .unwrap_or("");
            std::fs::write(state.join("behaviour"), entry).unwrap();
            std::fs::write(state.join("create.args"), args.join("\n")).unwrap();
            println!("fake-container-id");
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
            let format = args.get(1).map(String::as_str).unwrap_or("");
            let done = finished(state, &behaviour());
            if format == "--format={{.State.Running}}" {
                println!("{}", done.is_none());
            } else {
                match done {
                    Some((code, oom)) => println!("false {code} {oom}"),
                    None => println!("true 0 false"),
                }
            }
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
            std::fs::write(state.join("removed"), "").unwrap();
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
    // 남은 같은 이름을 먼저 치우고(결함 276), wait 대신 inspect 로 종료를 본다(시한 있는 명령만 쓴다).
    assert_eq!(order, ["rm", "create", "start", "inspect", "logs", "rm"]);
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
    assert!(
        calls(&f.state)
            .lines()
            .last()
            .is_some_and(|l| l.starts_with("rm -f -v ")),
        "반쯤 만든 컨테이너를 볼륨까지 치우지 않았다:\n{}",
        calls(&f.state)
    );
    assert!(
        f.state.join("removed").exists(),
        "반쯤 만든 컨테이너를 치우지 않았다"
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
    }
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
        &order(&f.state)[..4],
        ["pull", "rm", "create", "start"],
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
        ["pull", "rm", "create", "rm"],
        "{}",
        calls(&f.state)
    );
}

/// 결함 471 (재검수 119) — start 가 실패해도 지우기가 성공해야만 "돌지 않았다"(NotStarted)다. 지우기도 실패하면 시작 여부를 모른다(NotObserved).
fn a_failed_start_is_not_started_only_when_the_container_was_removed() {
    let f = fixture(Some("start"));
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("start 가 실패했는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::NotStarted { detail, container: ContainerLeft::Removed } if detail.contains("지웠다")),
        "{error:?}"
    );

    let f = fixture(Some("start,rm-created"));
    let mut handed_stopper = false;
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| handed_stopper = true,
    )
    .expect_err("start 가 실패했는데 성공했다");
    assert!(
        matches!(&error, ContainerRunError::Unobserved { detail, stopped: false, container: ContainerLeft::Unknown, .. } if detail.contains("시작했는지 모른다")),
        "{error:?}"
    );
    // 결함 475 — 돌고 있을 수 있으니 소유자가 멈출 수 있게 정지 손잡이를 넘겼다.
    assert!(
        handed_stopper,
        "시작 여부를 모르는데 정지 손잡이를 넘기지 않았다"
    );
}

/// 결함 481 (재검수 121) — 종료를 다섯 번 못 봐 kill · rm 을 했을 때, rm 이 성공하면 "지웠다"(RemovedUnobserved), 실패하면 "남아 돌 수 있다"(NotObserved).
fn an_unobserved_exit_is_removed_only_when_rm_succeeded() {
    let f = fixture(Some("inspect"));
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

    let f = fixture(Some("inspect,rm-created"));
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
    let f = fixture(Some("inspect,logs"));
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
    let f = fixture(Some("rm"));
    let error = container::run(
        &execution(),
        &input(&mounts(&f.work), "exit-0", &[]),
        None,
        None,
        |_| {},
    )
    .expect_err("남은 것을 못 지웠는데 만들었다");
    assert!(
        matches!(
            &error,
            ContainerRunError::NotStarted {
                container: ContainerLeft::Unknown,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(error.needs_human());
    assert!(
        !call_order(&f.state).iter().any(|c| c == "create"),
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
