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
    self, ContainerExecution, ContainerRunError, ContainerRuntime, CreateInput, RuntimeFlavor,
};

const IMAGE: &str = "busybox:1.36";

/// docker 에 붙을 수 있으면 busybox 의 digest(64자리 16진수)를 돌려준다. 못 하면 사유.
fn docker_image_digest() -> &'static Result<String, String> {
    static DIGEST: OnceLock<Result<String, String>> = OnceLock::new();
    DIGEST.get_or_init(|| {
        let info = Command::new("docker")
            .arg("info")
            .output()
            .map_err(|e| format!("docker 를 띄우지 못했다: {e}"))?;
        if !info.status.success() {
            return Err(format!(
                "docker info 실패: {}",
                String::from_utf8_lossy(&info.stderr).trim()
            ));
        }
        let pull = Command::new("docker")
            .args(["pull", "-q", IMAGE])
            .output()
            .map_err(|e| format!("docker pull 을 띄우지 못했다: {e}"))?;
        if !pull.status.success() {
            return Err(format!(
                "{IMAGE} 를 받지 못했다: {}",
                String::from_utf8_lossy(&pull.stderr).trim()
            ));
        }
        let inspect = Command::new("docker")
            .args([
                "image",
                "inspect",
                "--format={{index .RepoDigests 0}}",
                IMAGE,
            ])
            .output()
            .map_err(|e| format!("docker image inspect 실패: {e}"))?;
        let text = String::from_utf8_lossy(&inspect.stdout).trim().to_string();
        let hex = text
            .rsplit_once("@sha256:")
            .map(|(_, hex)| hex.to_string())
            .ok_or_else(|| format!("RepoDigests 를 읽지 못했다({text:?})"))?;
        if hex.len() != 64 {
            return Err(format!("digest 길이가 64 가 아니다({hex:?})"));
        }
        Ok(hex)
    })
}

/// 환경이 없으면 `None` — 그때 제품이 조용히 성공하지 않는지 확인하고 시험을 끝낸다.
fn require_docker(what: &str) -> Option<ContainerExecution> {
    match docker_image_digest() {
        Ok(hex) => Some(ContainerExecution {
            runtime: ContainerRuntime {
                program: PathBuf::from("docker"),
                flavor: RuntimeFlavor::Docker,
                pass_gpu: false,
                only: false,
            },
            pinned_image: format!("busybox@sha256:{hex}"),
            gpu_pin: None,
        }),
        Err(why) => {
            eprintln!(
                "ENVIRONMENT-BLOCKED: docker 를 쓸 수 없다 — {what} 은 측정하지 않았다: {why}"
            );
            let dir = tempfile::tempdir().unwrap();
            let execution = ContainerExecution {
                runtime: ContainerRuntime {
                    program: PathBuf::from("docker"),
                    flavor: RuntimeFlavor::Docker,
                    pass_gpu: false,
                    only: false,
                },
                pinned_image: format!("busybox@sha256:{}", "0".repeat(64)),
                gpu_pin: None,
            };
            let args = vec!["-c".to_string(), "exit 0".to_string()];
            let result = container::run(
                &execution,
                &input("gputeer-blocked-probe", dir.path(), &args, 64),
                None,
                None,
                |_| {},
            );
            assert!(
                matches!(result, Err(ContainerRunError::NotStarted { .. })),
                "런타임을 못 쓰는 환경인데 컨테이너가 '성공' 했다: {result:?}"
            );
            eprintln!("  (확인: 제품이 기동하지 못했다고 보고했다)");
            None
        }
    }
}

fn input<'a>(
    name: &'a str,
    work: &'a Path,
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
        work_dir: work,
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
    let exit = container::run(
        execution,
        &input(name, dir.path(), &args, memory_mib),
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
    let Some(execution) = require_docker("the_root_filesystem_is_read_only") else {
        return;
    };
    let script = "if touch /gputeer-root-probe 2>/dev/null; then echo ROOT_WRITABLE; else echo ROOT_READONLY; fi; \
                  touch /tmp/ok && echo TMP_WRITABLE; \
                  echo from-container > /gputeer/work/probe.txt && echo WORK_WRITABLE";
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
        std::fs::read_to_string(dir.path().join("probe.txt"))
            .unwrap()
            .trim(),
        "from-container",
        "작업 폴더에 쓴 것이 호스트에 없다"
    );
    println!("CONTAINER_ISOLATION_MEASURED read_only_root=yes work_dir_writable=yes");
}

#[test]
fn there_is_no_network_but_loopback() {
    let Some(execution) = require_docker("there_is_no_network_but_loopback") else {
        return;
    };
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
    let Some(execution) = require_docker("every_capability_is_dropped") else {
        return;
    };
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
    let Some(execution) = require_docker("exceeding_the_memory_limit_ends_in_an_oom_kill") else {
        return;
    };
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
    let Some(execution) = require_docker("the_owner_can_stop_a_running_container") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let args = vec!["-c".to_string(), "sleep 300".to_string()];
    let (tx, rx) = std::sync::mpsc::channel();
    let work = dir.path().to_path_buf();
    let runner = std::thread::spawn(move || {
        container::run(
            &execution,
            &input("gputeer-test-stop", &work, &args, 64),
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
