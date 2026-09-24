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
    self, ContainerDecision, ContainerExecution, ContainerRunError, ContainerRuntime, CreateInput,
    Mount, RuntimeFlavor,
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
    if std::env::var(FAIL_ENV).ok().as_deref() == Some(command) {
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
            only: false,
            node_id: "node-test".into(),
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
        matches!(&error, ContainerRunError::NotStarted { detail } if detail.contains("create")),
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
    let removed = container::remove_leftovers(&execution().runtime).expect("정리");
    assert_eq!(removed, ["old-1", "old-2"]);
    let calls = calls(&f.state);
    assert!(
        calls
            .lines()
            .any(|l| l == "ps -a -q --filter label=gputeer.node=node-test"),
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
            .any(|l| l == "--label=gputeer.node=node-test"),
        "노드 라벨 없이 만들었다:\n{create}"
    );
}
