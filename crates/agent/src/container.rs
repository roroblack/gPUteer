//! `OCI_IMAGE` Job 을 **컨테이너 런타임(podman · docker)** 으로 격리해 실행하는 계층.
//!
//! # 왜 이 모듈이 있나
//!
//! 규범(`JobManifest.env` · 기준선 §9.1)은 `ENV_KIND_OCI_IMAGE` 를 S2~S5 의 실행 형태로 정해 두었다. 그런데 2026-09-25 까지
//! 그 계약을 **실행하는 코드가 없었다** — Agent 는 `entrypoint` 를 호스트에서 소유자와 같은 권한으로 띄웠고, 걸린 것은
//! 메모리 상한(Job Object · cgroup)뿐이었다. 설계 `docs/plans/2026-09-25_0053_컨테이너_실행_backend.md`.
//!
//! # 결정 — 네임스페이스를 직접 짜지 않고 런타임 CLI 를 부른다
//!
//! 읽기 전용 루트 · 기본 seccomp · capability 제거 · 네트워크 · PID 네임스페이스를 검증된 도구가 이미 준다.
//! 기준선의 `LinuxContainer` 가 rootless podman 이고, Windows 에서는 Docker Desktop 이 같은 CLI 로 WSL2 컨테이너를 띄운다.
//!
//! # 거는 것
//!
//! ```text
//! 이미지      oci_source_digest(sha256)로 고정 — image_ref@sha256:<hex>. 태그만으로는 받지 않는다(태그는 움직인다)
//! 루트 fs     --read-only · /tmp 만 tmpfs
//! 권한        --cap-drop=ALL · no-new-privileges · 런타임 기본 seccomp
//! 네트워크    --network=none (runtime_allow_hosts 가 비었을 때만 받는다 — 허용 목록은 강제할 수단이 없어 거부)
//! 메모리      --memory = --memory-swap = 커밋 상한 (스왑으로 새지 않게 — runtime-linux 의 2026-08-30 실측과 같은 이유)
//! 프로세스    --pids-limit
//! 쓰기        작업 폴더 하나만 /gputeer/work 로 붙인다
//! ```
//!
//! # 보장하지 않는 것 (`CLAUDE.md` §0.4)
//!
//! ```text
//! 커널 격리     컨테이너는 호스트 커널을 같이 쓴다(S3). 커널 · 드라이버 취약점은 못 막는다 — 그건 S4(gVisor) · S5(VM)
//! 런타임 신뢰   docker(rootful)의 docker 그룹은 곧 root 다. 권장은 rootless podman
//! GPU 격리      --container-gpu 로 장치를 넘기면 그 GPU 의 드라이버 표면이 컨테이너에 열린다. 그리고 이 경로는 아직 실측하지 않았다
//! 이미지 CAS    image_digest(BLAKE3) 대조는 하지 않는다 — oci_source_digest 는 런타임이 내용으로 검증한다
//! ```

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gputeer_protocol::pb;

/// 컨테이너 안에서 작업 폴더가 붙는 자리.
pub const CONTAINER_WORK_DIR: &str = "/gputeer/work";

/// 한 작업이 만들 수 있는 프로세스 수 상한. fork 폭탄으로 소유자 기계를 멈추지 못하게 한다.
pub const CONTAINER_PIDS_LIMIT: u32 = 4096;

/// 이미지를 받아 컨테이너를 만드는 데 줄 시간. 큰 이미지(수 GB)를 처음 받을 수 있어 길게 둔다.
const CREATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 나머지 짧은 명령(start · inspect · kill · logs · rm)의 시한. 런타임 데몬이 멈췄을 때 Agent 가 같이 멈추지 않게 한다.
const SHORT_TIMEOUT: Duration = Duration::from_secs(120);

/// 어느 런타임인가 — **운영자가 적는다.** 실행 파일 이름으로 추측하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFlavor {
    Podman,
    Docker,
}

impl RuntimeFlavor {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "podman" => Ok(Self::Podman),
            "docker" => Ok(Self::Docker),
            other => Err(format!(
                "--container-runtime-kind 는 podman 또는 docker 다(받은 값 {other:?})"
            )),
        }
    }
}

/// 운영자가 켠 컨테이너 런타임.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerRuntime {
    /// podman · docker 실행 파일.
    pub program: PathBuf,
    pub flavor: RuntimeFlavor,
    /// GPU 를 컨테이너에 넘기는가(`--container-gpu`). 끄면 GPU 를 고정한(`--gpu-pin`) 노드는 컨테이너 Job 을 거부한다.
    pub pass_gpu: bool,
    /// 컨테이너 Job 만 받는가(`--container-only`). 켜면 `OCI_IMAGE` 가 아닌 Job 을 호스트에서 돌리지 않는다.
    pub only: bool,
}

/// 이 Job 을 어떻게 실행하는가 — ACK **전에** 정한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerDecision {
    /// 컨테이너가 아니다 — 전처럼 호스트에서(Job Object · cgroup) 돌린다.
    Host,
    /// 컨테이너로 돌린다.
    Container(ContainerExecution),
    /// 받을 수 없다 — 실행하지 않는다. 사유는 운영자 로그에 그대로 남는다.
    Refused { detail: String },
}

/// 컨테이너 실행에 필요한 것 — 전부 검증된 Manifest 와 운영자 설정에서 온다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerExecution {
    pub runtime: ContainerRuntime,
    /// `image_ref@sha256:<hex>` — digest 로 고정된 참조.
    pub pinned_image: String,
    /// 컨테이너에 넘길 GPU(`--gpu-pin` 값). 없으면 GPU 를 넘기지 않는다.
    pub gpu_pin: Option<String>,
}

/// 검증된 Manifest 와 운영자 설정으로 실행 방식을 정한다. 순수 함수다(I/O 없음).
///
/// ★ `manifest` 는 서명 검증을 통과한 것이어야 한다(`CLAUDE.md` §0.2) — 호출부가 `Verified` 에서 꺼내 넘긴다.
pub fn decide(
    manifest: &pb::JobManifest,
    runtime: Option<&ContainerRuntime>,
    gpu_pin: Option<&str>,
) -> ContainerDecision {
    let refused = |detail: String| ContainerDecision::Refused { detail };
    let is_oci = manifest
        .env
        .as_ref()
        .is_some_and(|env| env.kind == pb::EnvKind::OciImage as i32);
    if !is_oci {
        return match runtime {
            Some(runtime) if runtime.only => refused(
                "CONTAINER_ONLY: 이 Agent 는 컨테이너 Job 만 받는다(--container-only) — OCI_IMAGE 가 아닌 Job 을 호스트에서 돌리지 않는다"
                    .into(),
            ),
            _ => ContainerDecision::Host,
        };
    }
    let Some(runtime) = runtime else {
        return refused(
            "CONTAINER_RUNTIME_MISSING: OCI_IMAGE Job 인데 --container-runtime 이 없다 — 호스트에서 대신 돌리지 않는다".into(),
        );
    };
    let env = manifest.env.as_ref().expect("is_oci 가 env 를 확인했다");
    if let Err(why) = check_image_ref(&env.image_ref) {
        return refused(format!("CONTAINER_IMAGE_REF: {why}"));
    }
    let digest_hex = match env.oci_source_digest.as_ref() {
        Some(digest)
            if digest.algo == pb::HashAlgorithm::Sha256 as i32 && digest.value.len() == 32 =>
        {
            digest
                .value
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        }
        Some(_) => {
            return refused(
                "CONTAINER_IMAGE_DIGEST: oci_source_digest 는 32바이트 sha256 이어야 한다".into(),
            )
        }
        None => {
            return refused(
                "CONTAINER_IMAGE_DIGEST: oci_source_digest 가 없다 — 태그만으로는 받지 않는다(태그는 움직인다)"
                    .into(),
            )
        }
    };
    // ★ 허용 목록을 강제할 수단이 없다 — `--network=none` 과 호스트 네트워크 사이에 "이 호스트만" 이 없다.
    //   강제 못 하는 것을 선언하지 않는다(`CLAUDE.md` §0.4). 받지 않는다.
    if manifest
        .network
        .as_ref()
        .is_some_and(|network| !network.runtime_allow_hosts.is_empty())
    {
        return refused(
            "CONTAINER_NETWORK_ALLOWLIST: runtime_allow_hosts 를 강제할 수단이 없다 — 네트워크 없는 Job 만 받는다".into(),
        );
    }
    if gpu_pin.is_some() && !runtime.pass_gpu {
        return refused(
            "CONTAINER_GPU_OFF: 이 노드는 GPU 를 고정했는데(--gpu-pin) 컨테이너에 GPU 를 넘기는 설정(--container-gpu)이 꺼져 있다".into(),
        );
    }
    ContainerDecision::Container(ContainerExecution {
        runtime: runtime.clone(),
        pinned_image: format!("{}@sha256:{digest_hex}", env.image_ref),
        gpu_pin: gpu_pin.map(str::to_string),
    })
}

/// 이미지 참조가 명령줄에서 **옵션으로 읽힐 수 없는가** — 제출자가 쓴 값이다.
///
/// ★ `-` 로 시작하면 런타임이 옵션으로 읽는다(인자 주입). 허용 문자를 좁힌다 — OCI 참조 문법(`[host[:port]/]name[:tag]`)이
///   쓰는 문자뿐이다. `@` 는 받지 않는다 — digest 는 `oci_source_digest` 로만 붙인다(두 digest 가 어긋나는 일을 없앤다).
fn check_image_ref(image_ref: &str) -> Result<(), String> {
    if image_ref.is_empty() {
        return Err("image_ref 가 비었다".into());
    }
    if image_ref.starts_with('-') {
        return Err(format!("image_ref 가 '-' 로 시작한다({image_ref:?})"));
    }
    if image_ref.len() > 255 {
        return Err("image_ref 가 255바이트를 넘는다".into());
    }
    if let Some(bad) = image_ref
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':')))
    {
        return Err(format!(
            "image_ref 에 쓸 수 없는 문자 {bad:?} 가 있다({image_ref:?})"
        ));
    }
    Ok(())
}

/// 컨테이너 이름 — attempt 별로 다르다. 같으면 두 작업이 같은 이름을 다퉈 한쪽을 멈출 때 다른 쪽이 죽는다
/// (cgroup 이름과 같은 이유 · `exec::derive_cgroup_name`). 자르거나 치환하지 않고 해시한다.
pub fn derive_container_name(grant_id: &str, attempt_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gputeer/v1/container-name");
    for component in [grant_id, attempt_id] {
        hasher.update(&(component.len() as u64).to_be_bytes());
        hasher.update(component.as_bytes());
    }
    format!("gputeer-{}", &hasher.finalize().to_hex()[..32])
}

/// `create` 에 줄 입력.
pub struct CreateInput<'a> {
    pub name: &'a str,
    pub entrypoint: &'a str,
    pub args: &'a [String],
    /// Agent 가 만든 환경 변수(호스트 경로 그대로). 작업 폴더 아래 경로는 컨테이너 안 경로로 바꿔 넘긴다.
    pub environment: &'a [(OsString, OsString)],
    /// 쓰기로 붙일 유일한 폴더(호스트).
    pub work_dir: &'a Path,
    pub memory_limit_bytes: u64,
    /// 컨테이너 안에서 쓸 uid:gid (리눅스 docker). 작업 폴더에 호스트 root 로 쓰지 않게 한다.
    pub user: Option<(u32, u32)>,
}

/// `create` 명령의 인자를 만든다. 순수 함수다.
///
/// ★ 제출자가 쓴 값(entrypoint · args · 이미지)이 옵션으로 읽히지 않게 한다 — entrypoint 는 `--entrypoint=<값>` 한 토큰,
///   이미지는 모든 옵션 **뒤**, args 는 이미지 뒤(런타임이 거기서부터 옵션 해석을 멈춘다).
pub fn create_args(
    execution: &ContainerExecution,
    input: &CreateInput<'_>,
) -> Result<Vec<OsString>, String> {
    if input.memory_limit_bytes == 0 {
        return Err("메모리 상한이 0 이다 — 상한 없이 띄우지 않는다".into());
    }
    if input.entrypoint.is_empty() {
        return Err("entrypoint 가 비었다".into());
    }
    let work = input
        .work_dir
        .to_str()
        .ok_or_else(|| format!("작업 폴더 경로가 UTF-8 이 아니다({:?})", input.work_dir))?;
    // ★ `--mount` 는 쉼표로 필드를 가른다. 경로에 쉼표가 있으면 필드가 바뀐다 — 받지 않는다.
    if work.contains(',') || work.contains('=') {
        return Err(format!(
            "작업 폴더 경로에 ',' 나 '=' 가 있다({work:?}) — --mount 필드를 바꿀 수 있어 받지 않는다"
        ));
    }
    let memory = input.memory_limit_bytes.to_string();
    let mut args: Vec<OsString> = vec![
        "create".into(),
        format!("--name={}", input.name).into(),
        "--label=gputeer.managed=1".into(),
        "--pull=missing".into(),
        "--read-only".into(),
        "--tmpfs=/tmp".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        "--network=none".into(),
        format!("--pids-limit={CONTAINER_PIDS_LIMIT}").into(),
        format!("--memory={memory}").into(),
        format!("--memory-swap={memory}").into(),
        format!("--mount=type=bind,source={work},target={CONTAINER_WORK_DIR}").into(),
        format!("--workdir={CONTAINER_WORK_DIR}").into(),
    ];
    match (execution.runtime.flavor, input.user) {
        // ★ rootless podman 은 컨테이너 안 uid 를 subuid 로 옮긴다 — keep-id 가 아니면 붙인 폴더에 못 쓴다.
        (RuntimeFlavor::Podman, _) => args.push("--userns=keep-id".into()),
        (RuntimeFlavor::Docker, Some((uid, gid))) => {
            args.push(format!("--user={uid}:{gid}").into())
        }
        (RuntimeFlavor::Docker, None) => {}
    }
    for (key, value) in input.environment {
        let key = key
            .to_str()
            .ok_or_else(|| format!("환경 변수 이름이 UTF-8 이 아니다({key:?})"))?;
        let value = translate_into_container(value, input.work_dir)?;
        if key.is_empty() || key.contains('=') {
            return Err(format!("환경 변수 이름이 잘못됐다({key:?})"));
        }
        args.push(format!("--env={key}={value}").into());
    }
    if let Some(pin) = execution.gpu_pin.as_deref() {
        // ★ 아직 실측하지 않은 경로다(설계 문서 Out · 조각 2). 모양은 각 런타임의 문서를 따른다.
        match execution.runtime.flavor {
            RuntimeFlavor::Podman => args.push(format!("--device=nvidia.com/gpu={pin}").into()),
            RuntimeFlavor::Docker => {
                args.push("--gpus".into());
                args.push(format!("device={pin}").into());
            }
        }
    }
    args.push(format!("--entrypoint={}", input.entrypoint).into());
    args.push(execution.pinned_image.clone().into());
    args.extend(input.args.iter().map(OsString::from));
    Ok(args)
}

/// 작업 폴더 아래를 가리키는 호스트 경로를 컨테이너 안 경로로 바꾼다. 작업 폴더 밖 경로는 그대로 둔다(식별자 같은 값).
fn translate_into_container(value: &OsStr, work_dir: &Path) -> Result<String, String> {
    let text = value
        .to_str()
        .ok_or_else(|| format!("환경 변수 값이 UTF-8 이 아니다({value:?})"))?;
    let path = Path::new(text);
    match path.strip_prefix(work_dir) {
        Ok(rest) => {
            let mut inside = String::from(CONTAINER_WORK_DIR);
            for component in rest.components() {
                inside.push('/');
                inside.push_str(
                    component
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| format!("경로가 UTF-8 이 아니다({rest:?})"))?,
                );
            }
            Ok(inside)
        }
        Err(_) => Ok(text.to_string()),
    }
}

/// 런타임 명령 한 번의 결과.
#[derive(Debug)]
struct CliOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

/// 런타임 명령을 **시한 안에** 부른다. 시한을 넘기면 그 CLI 프로세스를 죽이고 오류다.
fn run_cli(
    program: &Path,
    args: &[OsString],
    timeout: Option<Duration>,
) -> Result<CliOutput, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{program:?} 를 띄우지 못했다: {error}"))?;
    // 파이프가 차서 CLI 가 멈추지 않게 따로 빨아낸다.
    let mut stdout_pipe = child.stdout.take().expect("piped");
    let mut stderr_pipe = child.stderr.take().expect("piped");
    let stdout_reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stdout_pipe.read_to_string(&mut buffer);
        buffer
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stderr_pipe.read_to_string(&mut buffer);
        buffer
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => return Err(format!("{program:?} 를 기다리지 못했다: {error}")),
        }
        if timeout.is_some_and(|limit| started.elapsed() >= limit) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{program:?} {:?} 가 {:?} 안에 끝나지 않았다 — 런타임이 멈췄을 수 있다",
                args.first(),
                timeout.unwrap_or_default()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ok(CliOutput {
        status,
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

fn cli_ok(
    program: &Path,
    args: &[OsString],
    timeout: Option<Duration>,
) -> Result<CliOutput, String> {
    let output = run_cli(program, args, timeout)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!(
            "{:?} 실패({}): {}",
            args.first(),
            output.status,
            output.stderr.trim()
        ))
    }
}

/// 실행 중인 컨테이너를 **밖에서** 끝내는 손잡이(`CLAUDE.md` §0.1).
#[derive(Debug, Clone)]
pub struct ContainerStopper {
    program: PathBuf,
    name: String,
}

impl ContainerStopper {
    /// 컨테이너를 즉시 끝낸다(SIGKILL). 컨테이너 안의 프로세스 트리 전체가 같이 끝난다.
    ///
    /// 이미 끝난 컨테이너에 불러도 성공한다 — 정지 버튼을 두 번 누르는 것은 정상이다. 그러나 끝났는지 **확인하지 못하면**
    /// 성공이라고 하지 않는다.
    pub fn stop(&self) -> Result<(), String> {
        let kill = run_cli(
            &self.program,
            &["kill".into(), self.name.clone().into()],
            Some(SHORT_TIMEOUT),
        )?;
        if kill.status.success() {
            return Ok(());
        }
        match inspect_running(&self.program, &self.name) {
            Ok(false) => Ok(()),
            Ok(true) => Err(format!(
                "kill 이 실패했고 컨테이너가 아직 돈다: {}",
                kill.stderr.trim()
            )),
            Err(why) => Err(format!(
                "kill 이 실패했고({}) 상태도 확인하지 못했다: {why}",
                kill.stderr.trim()
            )),
        }
    }
}

fn inspect_running(program: &Path, name: &str) -> Result<bool, String> {
    let output = cli_ok(
        program,
        &[
            "inspect".into(),
            "--format={{.State.Running}}".into(),
            name.into(),
        ],
        Some(SHORT_TIMEOUT),
    )?;
    match output.stdout.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("State.Running 을 읽지 못했다({other:?})")),
    }
}

/// 컨테이너가 끝난 뒤 관측한 것.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerExit {
    pub exit_code: i64,
    /// 메모리 상한에 걸려 커널이 죽였는가.
    pub oom_killed: bool,
}

/// 실행 결과의 실패 — 어디서 멈췄는지 나눈다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerRunError {
    /// 컨테이너를 만들거나 시작하지 못했다 — 작업은 돌지 않았다.
    NotStarted { detail: String },
    /// 시작했는데 종료를 관측하지 못했다 — 컨테이너가 살아 있을 수 있다(지우지 않는다).
    NotObserved { detail: String },
}

/// 만들고 → 시작하고 → 끝날 때까지 기다리고 → 종료를 읽고 → 출력을 받고 → 지운다.
///
/// ★ `run` 한 번으로 하지 않는다. `run` 의 종료 코드는 런타임 오류(125~127)와 작업 종료가 섞인다 — 작업이 125 로 끝난 것과
///   이미지를 못 받은 것이 같아 보인다. 단계를 나누면 어디서 실패했는지 안다.
pub fn run(
    execution: &ContainerExecution,
    input: &CreateInput<'_>,
    stdout_path: Option<&Path>,
    stderr_path: Option<&Path>,
    on_started: impl FnOnce(ContainerStopper),
) -> Result<ContainerExit, ContainerRunError> {
    let program = execution.runtime.program.as_path();
    let not_started = |detail: String| ContainerRunError::NotStarted { detail };
    let create = create_args(execution, input).map_err(not_started)?;
    let remove = || {
        let _ = run_cli(
            program,
            &["rm".into(), "-f".into(), input.name.into()],
            Some(SHORT_TIMEOUT),
        );
    };
    if let Err(why) = cli_ok(program, &create, Some(CREATE_TIMEOUT)) {
        // 반쯤 만들어졌을 수 있다 — 같은 이름의 다음 시도가 막히지 않게 지운다.
        remove();
        return Err(not_started(format!("create: {why}")));
    }
    if let Err(why) = cli_ok(
        program,
        &["start".into(), input.name.into()],
        Some(SHORT_TIMEOUT),
    ) {
        remove();
        return Err(not_started(format!("start: {why}")));
    }
    on_started(ContainerStopper {
        program: program.to_path_buf(),
        name: input.name.to_string(),
    });
    // ★ 시한 없이 기다린다 — 작업 길이는 Lease 가 정한다. 멈추는 것은 소유자 손잡이(kill)가 한다.
    let waited = cli_ok(program, &["wait".into(), input.name.into()], None);
    let exit = match inspect_exit(program, input.name) {
        Ok(exit) => exit,
        Err(inspect_why) => {
            let detail = match &waited {
                Ok(output) => format!(
                    "wait 는 {:?} 를 냈는데 inspect 로 종료를 확인하지 못했다: {inspect_why}",
                    output.stdout.trim()
                ),
                Err(wait_why) => format!("wait: {wait_why} · inspect: {inspect_why}"),
            };
            return Err(ContainerRunError::NotObserved { detail });
        }
    };
    if let Err(why) = save_logs(program, input.name, stdout_path, stderr_path) {
        eprintln!("CONTAINER_LOGS_NOT_SAVED name={} — {why}", input.name);
    }
    if let Err(why) = cli_ok(
        program,
        &["rm".into(), "-f".into(), input.name.into()],
        Some(SHORT_TIMEOUT),
    ) {
        eprintln!("CONTAINER_NOT_REMOVED name={} — {why}", input.name);
    }
    Ok(exit)
}

fn inspect_exit(program: &Path, name: &str) -> Result<ContainerExit, String> {
    let output = cli_ok(
        program,
        &[
            "inspect".into(),
            "--format={{.State.Running}} {{.State.ExitCode}} {{.State.OOMKilled}}".into(),
            name.into(),
        ],
        Some(SHORT_TIMEOUT),
    )?;
    parse_inspect_exit(&output.stdout)
}

/// `inspect` 출력(`<running> <exit code> <oom>`)을 읽는다. 아직 돌고 있으면 종료가 아니다.
fn parse_inspect_exit(text: &str) -> Result<ContainerExit, String> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    let [running, code, oom] = fields.as_slice() else {
        return Err(format!("inspect 출력을 읽지 못했다({text:?})"));
    };
    if *running != "false" {
        return Err(format!(
            "컨테이너가 아직 끝나지 않았다(State.Running={running})"
        ));
    }
    let exit_code = code
        .parse::<i64>()
        .map_err(|error| format!("ExitCode 를 읽지 못했다({code:?}): {error}"))?;
    let oom_killed = match *oom {
        "true" => true,
        "false" => false,
        other => return Err(format!("OOMKilled 를 읽지 못했다({other:?})")),
    };
    Ok(ContainerExit {
        exit_code,
        oom_killed,
    })
}

fn save_logs(
    program: &Path,
    name: &str,
    stdout_path: Option<&Path>,
    stderr_path: Option<&Path>,
) -> Result<(), String> {
    let (Some(stdout_path), Some(stderr_path)) = (stdout_path, stderr_path) else {
        return Ok(());
    };
    let stdout = std::fs::File::create(stdout_path)
        .map_err(|error| format!("{stdout_path:?} 열기 실패: {error}"))?;
    let stderr = std::fs::File::create(stderr_path)
        .map_err(|error| format!("{stderr_path:?} 열기 실패: {error}"))?;
    let mut child = Command::new(program)
        .args(["logs", name])
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|error| format!("logs 를 띄우지 못했다: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("logs 실패({status})")),
            Ok(None) if started.elapsed() >= SHORT_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("logs 가 시한 안에 끝나지 않았다".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("logs 를 기다리지 못했다: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(flavor: RuntimeFlavor) -> ContainerRuntime {
        ContainerRuntime {
            program: PathBuf::from("podman"),
            flavor,
            pass_gpu: false,
            only: false,
        }
    }

    fn oci_manifest(image_ref: &str, digest: Option<pb::Digest>) -> pb::JobManifest {
        pb::JobManifest {
            env: Some(pb::ExecutionEnvironment {
                kind: pb::EnvKind::OciImage as i32,
                image_ref: image_ref.to_string(),
                oci_source_digest: digest,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn sha256(byte: u8) -> Option<pb::Digest> {
        Some(pb::Digest {
            algo: pb::HashAlgorithm::Sha256 as i32,
            value: vec![byte; 32],
        })
    }

    fn refused(decision: ContainerDecision) -> String {
        match decision {
            ContainerDecision::Refused { detail } => detail,
            other => panic!("거부돼야 한다: {other:?}"),
        }
    }

    #[test]
    fn a_non_container_job_runs_on_the_host_unless_the_agent_is_container_only() {
        let manifest = pb::JobManifest::default();
        assert_eq!(decide(&manifest, None, None), ContainerDecision::Host);
        assert_eq!(
            decide(&manifest, Some(&runtime(RuntimeFlavor::Podman)), None),
            ContainerDecision::Host
        );
        let only = ContainerRuntime {
            only: true,
            ..runtime(RuntimeFlavor::Podman)
        };
        assert!(refused(decide(&manifest, Some(&only), None)).starts_with("CONTAINER_ONLY"));
    }

    #[test]
    fn an_oci_job_is_pinned_by_its_sha256_digest() {
        let manifest = oci_manifest("registry.local:5000/team/train:v1", sha256(0xab));
        let ContainerDecision::Container(execution) =
            decide(&manifest, Some(&runtime(RuntimeFlavor::Docker)), None)
        else {
            panic!("컨테이너로 가야 한다");
        };
        assert_eq!(
            execution.pinned_image,
            format!(
                "registry.local:5000/team/train:v1@sha256:{}",
                "ab".repeat(32)
            )
        );
    }

    /// negative — 실행 **전에** 거부해야 하는 것들. 하나라도 통과하면 호스트나 움직이는 태그로 돈다.
    #[test]
    fn what_cannot_be_enforced_is_refused_before_anything_runs() {
        let podman = runtime(RuntimeFlavor::Podman);
        let cases: Vec<(
            &str,
            pb::JobManifest,
            Option<&ContainerRuntime>,
            Option<&str>,
        )> = vec![
            (
                "CONTAINER_RUNTIME_MISSING",
                oci_manifest("img", sha256(1)),
                None,
                None,
            ),
            (
                "CONTAINER_IMAGE_DIGEST",
                oci_manifest("img", None),
                Some(&podman),
                None,
            ),
            (
                "CONTAINER_IMAGE_DIGEST",
                oci_manifest(
                    "img",
                    Some(pb::Digest {
                        algo: pb::HashAlgorithm::Blake3256 as i32,
                        value: vec![1; 32],
                    }),
                ),
                Some(&podman),
                None,
            ),
            (
                "CONTAINER_IMAGE_DIGEST",
                oci_manifest(
                    "img",
                    Some(pb::Digest {
                        algo: pb::HashAlgorithm::Sha256 as i32,
                        value: vec![1; 31],
                    }),
                ),
                Some(&podman),
                None,
            ),
            (
                "CONTAINER_IMAGE_REF",
                oci_manifest("", sha256(1)),
                Some(&podman),
                None,
            ),
            (
                "CONTAINER_IMAGE_REF",
                oci_manifest("--privileged", sha256(1)),
                Some(&podman),
                None,
            ),
            (
                "CONTAINER_IMAGE_REF",
                oci_manifest("img@sha256:00", sha256(1)),
                Some(&podman),
                None,
            ),
            (
                "CONTAINER_IMAGE_REF",
                oci_manifest("img tag", sha256(1)),
                Some(&podman),
                None,
            ),
            (
                "CONTAINER_GPU_OFF",
                oci_manifest("img", sha256(1)),
                Some(&podman),
                Some("0"),
            ),
        ];
        for (code, manifest, runtime, pin) in cases {
            let detail = refused(decide(&manifest, runtime, pin));
            assert!(detail.starts_with(code), "{code} 를 기대했다: {detail}");
        }
        let mut allowlisted = oci_manifest("img", sha256(1));
        allowlisted.network = Some(pb::NetworkPolicy {
            runtime_allow_hosts: vec!["pypi.local".into()],
            ..Default::default()
        });
        assert!(refused(decide(&allowlisted, Some(&podman), None))
            .starts_with("CONTAINER_NETWORK_ALLOWLIST"));
    }

    fn input<'a>(
        work: &'a Path,
        env: &'a [(OsString, OsString)],
        args: &'a [String],
    ) -> CreateInput<'a> {
        CreateInput {
            name: "gputeer-x",
            entrypoint: "python",
            args,
            environment: env,
            work_dir: work,
            memory_limit_bytes: 256 * 1024 * 1024,
            user: Some((1000, 1000)),
        }
    }

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter()
            .map(|a| a.into_string().expect("utf-8"))
            .collect()
    }

    #[test]
    fn the_create_command_carries_every_isolation_flag() {
        let execution = ContainerExecution {
            runtime: runtime(RuntimeFlavor::Docker),
            pinned_image: "img@sha256:00".into(),
            gpu_pin: None,
        };
        let work = PathBuf::from("/var/gputeer/run-1");
        let env = vec![
            (
                OsString::from("GPUTEER_CHECKPOINT_DIR"),
                OsString::from("/var/gputeer/run-1/checkpoints-out"),
            ),
            (OsString::from("GPUTEER_JOB_ID"), OsString::from("job-1")),
        ];
        let job_args = vec!["train.py".to_string(), "--epochs=3".to_string()];
        let args = strings(create_args(&execution, &input(&work, &env, &job_args)).unwrap());
        for flag in [
            "--read-only",
            "--tmpfs=/tmp",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--network=none",
            "--pids-limit=4096",
            "--memory=268435456",
            "--memory-swap=268435456",
            "--mount=type=bind,source=/var/gputeer/run-1,target=/gputeer/work",
            "--user=1000:1000",
            "--env=GPUTEER_CHECKPOINT_DIR=/gputeer/work/checkpoints-out",
            "--env=GPUTEER_JOB_ID=job-1",
            "--entrypoint=python",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag} 가 없다: {args:?}");
        }
        // 이미지 뒤에는 작업 인자만 온다 — 제출자 값이 옵션으로 읽히지 않는다.
        let image_at = args.iter().position(|a| a == "img@sha256:00").unwrap();
        assert_eq!(&args[image_at + 1..], ["train.py", "--epochs=3"]);
        assert!(args[..image_at]
            .iter()
            .all(|a| a == "create" || a.starts_with("--")));
    }

    #[test]
    fn podman_keeps_the_host_uid_and_gpu_flags_follow_the_runtime() {
        let mut execution = ContainerExecution {
            runtime: runtime(RuntimeFlavor::Podman),
            pinned_image: "img@sha256:00".into(),
            gpu_pin: Some("GPU-1".into()),
        };
        let work = PathBuf::from("/w");
        let podman = strings(create_args(&execution, &input(&work, &[], &[])).unwrap());
        assert!(podman.iter().any(|a| a == "--userns=keep-id"));
        assert!(!podman.iter().any(|a| a.starts_with("--user=")));
        assert!(podman.iter().any(|a| a == "--device=nvidia.com/gpu=GPU-1"));
        execution.runtime.flavor = RuntimeFlavor::Docker;
        let docker = strings(create_args(&execution, &input(&work, &[], &[])).unwrap());
        let gpus = docker.iter().position(|a| a == "--gpus").unwrap();
        assert_eq!(docker[gpus + 1], "device=GPU-1");
    }

    #[test]
    fn a_mount_path_that_could_rewrite_the_mount_is_refused() {
        let execution = ContainerExecution {
            runtime: runtime(RuntimeFlavor::Docker),
            pinned_image: "img@sha256:00".into(),
            gpu_pin: None,
        };
        let work = PathBuf::from("/w,readonly=false");
        assert!(create_args(&execution, &input(&work, &[], &[])).is_err());
        let mut zero = input(Path::new("/w"), &[], &[]);
        zero.memory_limit_bytes = 0;
        assert!(create_args(&execution, &zero).is_err());
    }

    #[test]
    fn inspect_output_distinguishes_running_exit_and_oom() {
        assert_eq!(
            parse_inspect_exit("false 137 true\n").unwrap(),
            ContainerExit {
                exit_code: 137,
                oom_killed: true
            }
        );
        assert_eq!(parse_inspect_exit("false 0 false").unwrap().exit_code, 0);
        assert!(parse_inspect_exit("true 0 false").is_err());
        assert!(parse_inspect_exit("").is_err());
        assert!(parse_inspect_exit("false x false").is_err());
    }

    #[test]
    fn container_names_differ_per_attempt() {
        assert_ne!(
            derive_container_name("g", "a1"),
            derive_container_name("g", "a2")
        );
        assert_ne!(
            derive_container_name("ga", "b"),
            derive_container_name("g", "ab")
        );
        assert!(derive_container_name("g", "a").starts_with("gputeer-"));
    }
}
