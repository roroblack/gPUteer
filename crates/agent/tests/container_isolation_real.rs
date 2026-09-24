//! 컨테이너 격리를 **실제 docker 로** 잰다 — 리눅스에서만 돈다(CI 러너 · docker 그룹 사용자).
//!
//! # 무엇을 재는가
//!
//! `container::create_args` 가 건 플래그가 **실제로 효과가 있는가.** 인자 목록에 `--read-only` 가 있다는 시험(단위 시험)은
//! 런타임이 그것을 무시해도 통과한다 — 여기서는 컨테이너 안에서 직접 해 본다. 각 시험은 **대조군**을 같이 둔다:
//! 막혀야 할 것이 막히고, 같은 컨테이너에서 허용된 것은 된다(명령 자체가 망가져 실패한 것과 구분한다).
//!
//! # 환경이 없을 때
//!
//! docker 가 없거나 데몬에 못 붙으면 `ENVIRONMENT-BLOCKED` 를 찍고, 제품이 **조용히 성공하지 않는지**만 본다.
//! 환경 판정에 제품 코드를 쓰지 않는다 — `docker info` 를 직접 부른다(`CLAUDE.md` §4).
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use gputeer_agent::container::{
    self, ContainerExecution, ContainerRunError, ContainerRuntime, CreateInput, Mount,
    RuntimeFlavor,
};

/// 완전한 이름으로 받는다 — podman 은 짧은 이름을 레지스트리 없이 풀지 않는다.
const IMAGE: &str = "docker.io/library/busybox:1.36";

/// 이 런타임으로 **컨테이너가 실제로 도는지** 직접 물어보고(제품 코드가 아니다 — `CLAUDE.md` §4), 되면 busybox digest(64자리)를 돌려준다.
///
/// ★ 확인 명령에는 `--memory` 만 준다 — `--memory-swap` 을 같이 주면 결함 275(podman 이 같은 값을 받는가)를 환경 문제로 숨긴다.
fn probe(program: &str) -> Result<String, String> {
    let run = |args: &[&str]| -> Result<String, String> {
        let out = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("{program} 를 띄우지 못했다: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(format!(
                "{program} {} 실패: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };
    run(&["info"])?;
    run(&["pull", "-q", IMAGE])?;
    run(&["run", "--rm", "--memory=67108864", IMAGE, "true"])?;
    let text = run(&[
        "image",
        "inspect",
        "--format={{index .RepoDigests 0}}",
        IMAGE,
    ])?;
    let hex = text
        .rsplit_once("@sha256:")
        .map(|(_, hex)| hex.to_string())
        .ok_or_else(|| format!("RepoDigests 를 읽지 못했다({text:?})"))?;
    if hex.len() != 64 {
        return Err(format!("digest 길이가 64 가 아니다({hex:?})"));
    }
    Ok(hex)
}

fn probed(flavor: RuntimeFlavor) -> &'static Result<String, String> {
    static DOCKER: OnceLock<Result<String, String>> = OnceLock::new();
    static PODMAN: OnceLock<Result<String, String>> = OnceLock::new();
    match flavor {
        RuntimeFlavor::Docker => DOCKER.get_or_init(|| probe("docker")),
        RuntimeFlavor::Podman => PODMAN.get_or_init(|| probe("podman")),
    }
}

fn execution(flavor: RuntimeFlavor, hex: &str) -> ContainerExecution {
    ContainerExecution {
        runtime: ContainerRuntime {
            program: PathBuf::from(match flavor {
                RuntimeFlavor::Docker => "docker",
                RuntimeFlavor::Podman => "podman",
            }),
            flavor,
            pass_gpu: false,
            only: false,
            node_id: "gputeer-ci-node".into(),
        },
        pinned_image: format!("docker.io/library/busybox@sha256:{hex}"),
        gpu_pin: None,
    }
}

/// 이 기계에서 쓸 수 있는 런타임들(docker · podman). 못 쓰는 것은 `ENVIRONMENT-BLOCKED` 를 찍고, 그 런타임에서 제품이
/// **조용히 성공하지 않는지**만 본다.
fn runtimes(what: &str) -> Vec<ContainerExecution> {
    let mut usable = Vec::new();
    for flavor in [RuntimeFlavor::Docker, RuntimeFlavor::Podman] {
        match probed(flavor) {
            Ok(hex) => usable.push(execution(flavor, hex)),
            Err(why) => {
                eprintln!(
                    "ENVIRONMENT-BLOCKED: {flavor:?} 로 컨테이너를 돌릴 수 없다 — {what} 은 {flavor:?} 에서 측정하지 않았다: {why}"
                );
                let dir = tempfile::tempdir().unwrap();
                let args = vec!["-c".to_string(), "exit 0".to_string()];
                let mounts = mounts(dir.path());
                let result = container::run(
                    &execution(flavor, &"0".repeat(64)),
                    &input("gputeer-blocked-probe", dir.path(), &mounts, &args, 64),
                    None,
                    None,
                    |_| {},
                );
                assert!(
                    matches!(result, Err(ContainerRunError::NotStarted { .. })),
                    "런타임을 못 쓰는 환경인데 컨테이너가 '성공' 했다: {result:?}"
                );
            }
        }
    }
    usable
}

/// 체크포인트 폴더 하나만 붙인다(Agent 와 같은 배치 — 로그를 받는 작업 폴더는 붙이지 않는다 · 결함 273).
fn mounts(work: &Path) -> Vec<Mount> {
    let checkpoints = work.join("checkpoints-out");
    std::fs::create_dir_all(&checkpoints).unwrap();
    vec![Mount {
        host: checkpoints,
        target: container::CONTAINER_CHECKPOINT_DIR,
        read_only: false,
    }]
}

fn input<'a>(
    name: &'a str,
    work: &'a Path,
    mounts: &'a [Mount],
    args: &'a [String],
    memory_mib: u64,
) -> CreateInput<'a> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(work).unwrap();
    CreateInput {
        name,
        entrypoint: "sh",
        args,
        environment: &[],
        mounts,
        memory_limit_bytes: memory_mib * 1024 * 1024,
        user: Some((meta.uid(), meta.gid())),
    }
}

/// `sh -c <script>` 를 돌리고 (종료, stdout) 를 돌려준다.
fn run_script(
    execution: &ContainerExecution,
    name: &str,
    script: &str,
    memory_mib: u64,
) -> (container::ContainerExit, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("stdout.log");
    let err = dir.path().join("stderr.log");
    let args = vec!["-c".to_string(), script.to_string()];
    let mounts = mounts(dir.path());
    let exit = container::run(
        execution,
        &input(name, dir.path(), &mounts, &args, memory_mib),
        Some(&out),
        Some(&err),
        |_| {},
    )
    .unwrap_or_else(|e| panic!("{name}: 컨테이너를 돌리지 못했다: {e:?}"));
    let stdout = std::fs::read_to_string(&out).unwrap_or_default();
    (exit, stdout, dir)
}

#[test]
fn the_root_filesystem_is_read_only_but_the_work_dir_is_writable() {
    for execution in runtimes("the_root_filesystem_is_read_only_but_the_work_dir_is_writable") {
        the_root_filesystem_is_read_only_but_the_work_dir_is_writable_on(&execution);
    }
}

fn the_root_filesystem_is_read_only_but_the_work_dir_is_writable_on(
    execution: &ContainerExecution,
) {
    let execution = execution.clone();
    println!(
        "CONTAINER_ISOLATION_MEASURED runtime={:?}",
        execution.runtime.flavor
    );
    let script = "if touch /gputeer-root-probe 2>/dev/null; then echo ROOT_WRITABLE; else echo ROOT_READONLY; fi; \
                  touch /tmp/ok && echo TMP_WRITABLE; \
                  echo from-container > /gputeer/checkpoints/probe.txt && echo WORK_WRITABLE; \
                  if touch /gputeer/stdout-plant 2>/dev/null; then echo PARENT_WRITABLE; else echo PARENT_READONLY; fi";
    let (exit, stdout, dir) = run_script(&execution, "gputeer-test-rofs", script, 64);
    assert_eq!(exit.exit_code, 0, "스크립트가 끝까지 못 갔다: {stdout}");
    assert!(
        stdout.contains("ROOT_READONLY"),
        "루트에 쓸 수 있었다: {stdout}"
    );
    // 대조군 — 같은 컨테이너에서 허용된 곳은 된다.
    assert!(
        stdout.contains("TMP_WRITABLE"),
        "/tmp 도 막혔다 — 쓰기 자체가 망가졌다: {stdout}"
    );
    assert!(
        stdout.contains("WORK_WRITABLE"),
        "작업 폴더에 못 썼다: {stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("checkpoints-out").join("probe.txt"))
            .unwrap()
            .trim(),
        "from-container",
        "작업 폴더에 쓴 것이 호스트에 없다"
    );
    // ★ 결함 273 — 로그를 받는 작업 폴더는 컨테이너에서 보이지도 쓰이지도 않는다.
    assert!(
        stdout.contains("PARENT_READONLY"),
        "붙인 폴더 밖(/gputeer)에 쓸 수 있었다: {stdout}"
    );
    println!("CONTAINER_ISOLATION_MEASURED read_only_root=yes checkpoint_dir_writable=yes");
}

#[test]
fn there_is_no_network_but_loopback() {
    for execution in runtimes("there_is_no_network_but_loopback") {
        there_is_no_network_but_loopback_on(&execution);
    }
}

fn there_is_no_network_but_loopback_on(execution: &ContainerExecution) {
    let execution = execution.clone();
    println!(
        "CONTAINER_ISOLATION_MEASURED runtime={:?}",
        execution.runtime.flavor
    );
    let (exit, stdout, _dir) = run_script(
        &execution,
        "gputeer-test-net",
        "ls /sys/class/net | tr '\\n' ' '",
        64,
    );
    assert_eq!(exit.exit_code, 0, "{stdout}");
    assert_eq!(
        stdout.trim(),
        "lo",
        "루프백 말고 다른 인터페이스가 있다: {stdout:?}"
    );
    println!(
        "CONTAINER_ISOLATION_MEASURED network_interfaces={:?}",
        stdout.trim()
    );
}

#[test]
fn every_capability_is_dropped() {
    for execution in runtimes("every_capability_is_dropped") {
        every_capability_is_dropped_on(&execution);
    }
}

fn every_capability_is_dropped_on(execution: &ContainerExecution) {
    let execution = execution.clone();
    println!(
        "CONTAINER_ISOLATION_MEASURED runtime={:?}",
        execution.runtime.flavor
    );
    let (exit, stdout, _dir) = run_script(
        &execution,
        "gputeer-test-caps",
        "grep -E '^(CapEff|CapPrm|NoNewPrivs)' /proc/self/status",
        64,
    );
    assert_eq!(exit.exit_code, 0, "{stdout}");
    assert!(
        stdout.contains("CapEff:\t0000000000000000"),
        "유효 capability 가 남아 있다: {stdout}"
    );
    assert!(
        stdout.contains("NoNewPrivs:\t1"),
        "no-new-privileges 가 안 걸렸다: {stdout}"
    );
    println!("CONTAINER_ISOLATION_MEASURED capabilities=none no_new_privs=1");
}

#[test]
fn exceeding_the_memory_limit_ends_in_an_oom_kill() {
    for execution in runtimes("exceeding_the_memory_limit_ends_in_an_oom_kill") {
        exceeding_the_memory_limit_ends_in_an_oom_kill_on(&execution);
    }
}

fn exceeding_the_memory_limit_ends_in_an_oom_kill_on(execution: &ContainerExecution) {
    let execution = execution.clone();
    println!(
        "CONTAINER_ISOLATION_MEASURED runtime={:?}",
        execution.runtime.flavor
    );
    // tail 은 줄바꿈이 없는 입력을 통째로 메모리에 담는다 — 200MB 를 64MiB 상한 안에서 잡으려 한다.
    //   커널은 가장 큰 프로세스(tail)를 죽인다. sh 는 살아남을 수 있으므로 tail 의 실패를 종료 코드 99 로 옮긴다.
    let (exit, stdout, _dir) = run_script(
        &execution,
        "gputeer-test-oom",
        "head -c 200000000 /dev/zero | tail > /dev/null || exit 99; echo SURVIVED",
        64,
    );
    assert!(
        !stdout.contains("SURVIVED") && exit.exit_code != 0,
        "상한(64MiB)을 넘겨 200MB 를 잡았는데 살아남았다: {exit:?} {stdout}"
    );
    // 대조군 — 상한 안의 작업은 멀쩡히 끝난다.
    let (small, small_out, _dir) = run_script(
        &execution,
        "gputeer-test-oom-control",
        "head -c 1000000 /dev/zero | tail > /dev/null; echo SURVIVED",
        64,
    );
    assert_eq!(small.exit_code, 0, "상한 안의 작업도 죽었다: {small_out}");
    assert!(!small.oom_killed);
    println!(
        "CONTAINER_ISOLATION_MEASURED oom_exit={} oom_killed={}",
        exit.exit_code, exit.oom_killed
    );
}

#[test]
fn the_owner_can_stop_a_running_container() {
    for execution in runtimes("the_owner_can_stop_a_running_container") {
        the_owner_can_stop_a_running_container_on(&execution);
    }
}

fn the_owner_can_stop_a_running_container_on(execution: &ContainerExecution) {
    let execution = execution.clone();
    println!(
        "CONTAINER_ISOLATION_MEASURED runtime={:?}",
        execution.runtime.flavor
    );
    let dir = tempfile::tempdir().unwrap();
    let args = vec!["-c".to_string(), "sleep 300".to_string()];
    let (tx, rx) = std::sync::mpsc::channel();
    let work = dir.path().to_path_buf();
    let runner = std::thread::spawn(move || {
        let mounts = mounts(&work);
        container::run(
            &execution,
            &input("gputeer-test-stop", &work, &mounts, &args, 64),
            None,
            None,
            move |stopper| tx.send(stopper).unwrap(),
        )
    });
    let stopper = rx
        .recv_timeout(Duration::from_secs(120))
        .expect("정지 손잡이");
    let started = std::time::Instant::now();
    stopper.stop().expect("정지");
    let exit = runner.join().unwrap().expect("정지 뒤 종료 관측");
    assert_ne!(exit.exit_code, 0, "정지된 작업이 성공으로 끝났다");
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "정지에 너무 오래 걸렸다: {:?}",
        started.elapsed()
    );
    println!(
        "CONTAINER_ISOLATION_MEASURED owner_stop_exit={} elapsed_ms={}",
        exit.exit_code,
        started.elapsed().as_millis()
    );
}
