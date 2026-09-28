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
//! 루트 fs     --read-only · /tmp tmpfs(런타임이 붙이는 /dev · /dev/shm 도 쓰기 가능한 tmpfs 다 — 아래 "쓰기")
//! 권한        --cap-drop=ALL · no-new-privileges · 런타임 기본 seccomp
//! 네트워크    --network=none (runtime_allow_hosts 가 비었을 때만 받는다 — 허용 목록은 강제할 수단이 없어 거부)
//! 메모리      --memory = --memory-swap = 커밋 상한 (스왑으로 새지 않게 — runtime-linux 의 2026-08-30 실측과 같은 이유)
//! 프로세스    --pids-limit
//! 쓰기        체크포인트 폴더(/gputeer/checkpoints)만 붙여 쓰고 · 이어받기 폴더(/gputeer/resume)는 읽기 전용 ·
//!             그 밖에 쓸 수 있는 곳은 메모리 tmpfs(/tmp · /dev/shm — 메모리 상한에 든다)와 런타임이 붙이는 /dev(tmpfs — 결함 485 ·
//!             docker 는 --read-only 여도 쓰기 가능하게 둔다. 메모리 상한에 드는지는 재지 않았다)다. 로그를 받는 작업 폴더는 붙이지
//!             않는다(결함 273). ★ docker 는 이미지의 VOLUME 을 막지 못한다(끝에 rm -v 로 지운다 · 결함 274)
//! ```
//!
//! # 보장하지 않는 것 (`CLAUDE.md` §0.4)
//!
//! ```text
//! 커널 격리     컨테이너는 호스트 커널을 같이 쓴다(S3). 커널 · 드라이버 취약점은 못 막는다 — 그건 S4(gVisor) · S5(VM)
//! 런타임 신뢰   docker(rootful)의 docker 그룹은 곧 root 다. 권장은 rootless podman
//! GPU 격리      --container-gpu 로 장치를 넘기면 그 GPU 의 드라이버 표면이 컨테이너에 열린다. 2026-09-25 x600 WSL2 docker 29 에서
//!               위 격리 옵션 그대로 `--device=nvidia.com/gpu=all` 로 GPU 를 연 것이 첫 실측이다(`--gpus` 는 거부 · 결함 303)
//! 이미지 CAS    image_digest(BLAKE3) 대조는 하지 않는다 — oci_source_digest 는 런타임이 내용으로 검증한다
//! ```

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gputeer_protocol::pb;

/// 컨테이너 안에서 체크포인트를 쓰는 자리(쓰기 · 호스트의 `<작업 폴더>/checkpoints-out`).
pub const CONTAINER_CHECKPOINT_DIR: &str = "/gputeer/checkpoints";
/// 이어받을 체크포인트가 붙는 자리(읽기 전용 · 호스트의 `<작업 폴더>/resume-in`).
pub const CONTAINER_RESUME_DIR: &str = "/gputeer/resume";

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

/// GPU 를 런타임에 **어떻게 청하는가** — 운영자가 고른다(`--container-gpu-request`). 기계마다 되는 모양이 다르다(결함 303).
///
/// ```text
/// gpus     docker --gpus "device=<n>"          nvidia 런타임이 등록된 docker. docker 기본값(전과 같다)
/// cdi      --device=nvidia.com/gpu=<n> (장치마다) CDI 사양에 번호별 장치가 있는 기계(네이티브 리눅스). podman 기본값
/// cdi-all  --device=nvidia.com/gpu=all          WSL2 — CDI 사양이 번호로 나누지 않고 `all` 하나만 준다(2026-09-25 x600 실측)
/// ```
///
/// ★ `cdi-all` 은 GPU 를 **전부** 넘긴다 — 한 장인 노드에서만 고정과 같은 뜻이다. Agent 는 `--gpu-pin 0` 이고 NVML 이 GPU 를 정확히
///   한 장 볼 때만 이 값을 받는다(`lib.rs` parse_container_runtime).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuRequest {
    Gpus,
    Cdi,
    CdiAll,
}

impl GpuRequest {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "gpus" => Ok(Self::Gpus),
            "cdi" => Ok(Self::Cdi),
            "cdi-all" => Ok(Self::CdiAll),
            other => Err(format!(
                "--container-gpu-request 는 gpus · cdi · cdi-all 중 하나다(받은 값 {other:?})"
            )),
        }
    }

    /// 고르지 않았을 때 — 이 판 전의 동작과 같다.
    pub fn default_for(flavor: RuntimeFlavor) -> Self {
        match flavor {
            RuntimeFlavor::Docker => Self::Gpus,
            RuntimeFlavor::Podman => Self::Cdi,
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
    /// GPU 를 어떻게 청하는가(`--container-gpu-request` · 결함 303).
    pub gpu_request: GpuRequest,
    /// 컨테이너 Job 만 받는가(`--container-only`). 켜면 `OCI_IMAGE` 가 아닌 Job 을 호스트에서 돌리지 않는다.
    pub only: bool,
    /// 이 Agent 의 노드 id — 모든 컨테이너에 `gputeer.node=<id>` 라벨로 붙인다(사람이 알아보는 용도).
    pub node_id: String,
    /// 남은 컨테이너를 찾는 열쇠 — `<노드 id>.<잠근 체크포인트 루트의 해시>`. 같은 루트를 잠근 Agent 는 하나뿐이다(결함 290 · 295).
    /// Agent 가 루트를 잠근 뒤 채운다. 비어 있으면 컨테이너를 만들지도 지우지도 않는다.
    pub owner: String,
    /// ★ 2026-09-27 보수 규칙(재검수 121~124 합의) — 사람이 봐야 하는 상태(시작 여부 · 정지 · 로그 · 컨테이너가 불확실)를 남길 **영속 사건 표식**
    ///   폴더. Agent 가 체크포인트 루트를 잠근 뒤 채운다(`incident_dir_for`). `None` 이면 표식을 쓰지 않는다(node-doctor · 시험 — 호출부가 직접 알린다).
    pub incident_dir: Option<PathBuf>,
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
    // ★ 결함 303 — `cdi-all` 은 GPU 를 전부 넘긴다. 한 장(`0`)만 고정한 노드가 아니면 고정이 샌다 — 받지 않는다.
    if runtime.pass_gpu
        && runtime.gpu_request == GpuRequest::CdiAll
        && gpu_pin.is_some_and(|pin| pin != "0")
    {
        return refused(
            "CONTAINER_GPU_ALL_NOT_PINNED: --container-gpu-request cdi-all 은 GPU 를 전부 넘긴다 — --gpu-pin 0 인 한 장 노드에서만 받는다".into(),
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

/// 컨테이너 이름 — **시도(attempt)** 에서만 만든다. 자르거나 치환하지 않고 해시한다.
///
/// ★ 결함 290 (재검수 90) — 전에는 `grant_id` 도 넣었다. 풀은 연결마다 새 grant_id 를 만들어, Agent 가 죽은 뒤 같은 시도를 다시 받은
///   회차가 옛 컨테이너를 이름으로 찾지 못했다. 한 시도는 한 노드에서만 돈다(이어받기는 새 시도다) — 시도만으로 갈린다.
pub fn derive_container_name(attempt_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gputeer/v2/container-name");
    hasher.update(&(attempt_id.len() as u64).to_be_bytes());
    hasher.update(attempt_id.as_bytes());
    format!("gputeer-{}", &hasher.finalize().to_hex()[..32])
}

/// 이 노드의 라벨이 붙은 컨테이너를 **모두** 멈추고, 멈춤 · 로그 건지기를 확인한 것만 지운다 — Agent 가 회차를 시작할 때 부른다(결함 290 · 291 · 492).
///
/// ★ 한 노드(한 Agent)는 한 번에 한 작업만 돌린다(회차는 순차다). 새 회차가 시작될 때 이 노드의 라벨로 돌고 있는 컨테이너는
///   죽은 회차(Agent 가 죽었거나 종료를 관측하지 못하고 끝난 회차)가 남긴 것이다 — 그대로 두면 패널 손잡이 없이 GPU 를 물고 돌고,
///   이어받은 다른 노드와 **두 번** 돈다. 지운 컨테이너 id 를 돌려준다. 목록을 못 읽으면 오류다(모르는 채 시작하지 않는다).
///
/// ★ 결함 489 (재검수 123) — 지우기 **전에** 로그를 `salvage_dir/<id>.{stdout,stderr}.log` 로 건진다. 죽은 회차가 종료를 보고 로그를 받기 전에 죽었으면
///   런타임 로그가 출력의 유일한 사본이다. 그 시도는 이미 보고할 수 없다 — 사람이 볼 수 있게 남기는 것이다.
///   ★ 결함 492 · 493 (재검수 124) — 멈춘 **뒤에** 건지고, 이미 건진 파일을 덮지 않으며, 못 건지면 **지우지 않고** Err 다(아래 본문).
pub fn remove_leftovers(
    runtime: &ContainerRuntime,
    salvage_dir: Option<&Path>,
) -> Result<Vec<String>, String> {
    if runtime.owner.is_empty() {
        return Err("owner 라벨이 비었다 — 어느 컨테이너가 이 Agent 의 것인지 모른다".into());
    }
    let program = runtime.program.as_path();
    let listed = cli_ok(
        program,
        &[
            "ps".into(),
            "-a".into(),
            "-q".into(),
            "--filter".into(),
            format!("label=gputeer.owner={}", runtime.owner).into(),
        ],
        SHORT_TIMEOUT,
    )?;
    let ids: Vec<String> = listed
        .stdout
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    // ★ 결함 492 (재검수 124) — 로그는 **멈춘 뒤에** 받는다. `logs` 는 그 순간까지의 로그만 준다 — 도는 컨테이너에서 받으면 마지막 출력이 빠진다.
    //   먼저 **모든** 컨테이너를 멈춰 본다(하나가 실패해도 나머지를 멈춘다 — GPU 를 물고 도는 것을 줄인다).
    // ★ 보수 규칙(재검수 124 합의) — 멈춤 · 로그 건지기 · 지우기 중 하나라도 확인하지 못하면 **자동으로 지우지 않고** 모아서 Err 로 돌려준다.
    //   Agent 는 이 Err 로 기동을 멈춘다(새 작업을 받지 않는다) — 사람이 확인해 치울 때까지 컨테이너와 런타임 로그가 남는다.
    let mut problems: Vec<String> = Vec::new();
    let mut stopped: Vec<&String> = Vec::new();
    for id in &ids {
        match stop_and_confirm(program, id) {
            Ok(()) => stopped.push(id),
            Err(why) => problems.push(format!("{id}: 멈췄는지 확인하지 못했다 — {why}")),
        }
    }
    for id in stopped {
        if let Some(dir) = salvage_dir {
            let saved = std::fs::create_dir_all(dir)
                .map_err(|error| format!("{dir:?} 를 만들지 못했다: {error}"))
                .and_then(|()| {
                    // ★ 결함 493 (재검수 124) — 이미 건진 로그를 덮지 않는다. 같은 id 가 다시 오면(앞 회차가 건지고 지우기에 실패) 새 번호로 쓴다.
                    let (stdout, stderr) = unique_salvage_paths(dir, id);
                    save_logs(program, id, Some(&stdout), Some(&stderr))
                        .map(|()| stdout.clone())
                        .map_err(|why| {
                            format!(
                                "{why}{}",
                                discard_partial_outputs(Some(&stdout), Some(&stderr))
                            )
                        })
                });
            match saved {
                Ok(stdout) => eprintln!(
                    "CONTAINER_LEFTOVER_LOGS_SAVED id={id} file={}",
                    stdout.display()
                ),
                Err(why) => {
                    problems.push(format!(
                        "{id}: 로그를 건지지 못해 지우지 않았다(멈춰 있다 — 런타임 로그가 남는다) — {why}"
                    ));
                    continue;
                }
            }
        }
        // ★ 결함 503 (재검수 126) — rm 의 0 이 아니라 사후 조회로 없어졌음을 확인한다.
        let (left, removed) = remove_container_fact(program, id);
        if left != ContainerLeft::Removed {
            problems.push(format!("{id}: 지웠는지 확인하지 못했다 — {removed}"));
        }
    }
    if !problems.is_empty() {
        return Err(format!(
            "남은 컨테이너를 자동으로 치우지 못해 남겼다(사람이 확인한다): {}",
            problems.join(" / ")
        ));
    }
    Ok(ids)
}

/// 컨테이너가 **멈췄음을 확인**한다 — `inspect` 가 멈춤(또는 없음)을 보이면 곧바로 Ok. 아니면 `kill` 하고 **다시 `inspect` 로** 멈춤을 확인한다.
/// 확인하지 못하면 Err(아직 돌 수 있다). 확인 없이 멈췄다고 보지 않는다(재검수 124 합의 — 보수 규칙).
///
/// ★ 결함 502 (재검수 126) — 전에는 `kill` 이 0 이면 곧바로 Ok 였다. `kill` 의 0 은 신호를 **접수했다**는 뜻이지 멈췄다는 뜻이 아니다 — 그 사이
///   받은 로그를 완결로 적고 `rm` 이 실제로 끝내며 마지막 출력을 잃었다. 이제 kill 뒤 `inspect` 를 몇 번(최대 약 2초) 본다.
pub fn stop_and_confirm(program: &Path, name: &str) -> Result<(), String> {
    if let Ok(false) = inspect_running(program, name) {
        return Ok(());
    }
    let kill = match run_cli(program, &["kill".into(), name.into()], CONFIRM_TIMEOUT) {
        Ok(output) if output.status.success() => "kill 접수".to_string(),
        Ok(output) => format!("kill 실패({}): {}", output.status, output.stderr.trim()),
        Err(why) => format!("kill 실패: {why}"),
    };
    confirm_stopped(program, name).map_err(|why| format!("{kill} · {why}"))
}

/// kill 을 보낸 뒤 **멈췄는지 확인**한다 — `inspect_running` 이 "돌지 않는다"(또는 "없다")고 할 때까지 몇 번 다시 본다.
///
/// ★ 결함 508 (재검수 127) — 런타임이 답하지 않으면(시한 초과 · 오류) 더 물어도 시한만 쌓인다 — 곧바로 "모른다" 로 끝낸다. 전에는 매번 120초 시한으로
///   열 번까지 물어 멈춤 확인 하나가 약 24분 걸릴 수 있었다.
fn confirm_stopped(program: &Path, name: &str) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 0..STOP_CONFIRM_TRIES {
        match inspect_running(program, name) {
            Ok(false) => return Ok(()),
            Ok(true) => last = "아직 돈다".into(),
            Err(why) => return Err(format!("상태를 모른다({why})")),
        }
        if attempt + 1 < STOP_CONFIRM_TRIES {
            std::thread::sleep(STOP_CONFIRM_INTERVAL);
        }
    }
    Err(last)
}

/// kill 뒤 멈춤을 몇 번 · 얼마 간격으로 확인할지(결함 502), 확인 조회 한 번의 시한(결함 508 — 상태 조회는 가볍다).
const STOP_CONFIRM_TRIES: u32 = 10;
const STOP_CONFIRM_INTERVAL: Duration = Duration::from_millis(200);
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(15);

/// 런타임이 "그런 컨테이너 없다" 고 답했는가(docker · podman 공통 문구).
fn says_no_such_container(why: &str) -> bool {
    why.to_lowercase().contains("no such container")
}

/// 컨테이너가 지금 **도는가** — `inspect --format={{.State.Running}}`. 없는 컨테이너면 "안 돈다"(Ok(false))다.
/// 종료 코드를 읽는 `inspect_state` 와 따로 둔다 — 멈춤 확인에는 "돌지 않는다" 만 필요하고, 종료 코드는 시작한 흔적까지 봐야 한다(결함 501).
fn inspect_running(program: &Path, name: &str) -> Result<bool, String> {
    match cli_ok(
        program,
        &[
            "inspect".into(),
            "--format={{.State.Running}}".into(),
            name.into(),
        ],
        CONFIRM_TIMEOUT,
    ) {
        Ok(output) => match output.stdout.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(format!("State.Running 을 읽지 못했다({other:?})")),
        },
        Err(why) if says_no_such_container(&why) => Ok(false),
        Err(why) => Err(why),
    }
}

/// 건진 로그를 쓸 **새** 파일 이름 — `<id>.stdout.log` 가 이미 있으면 `<id>.1.stdout.log` … (결함 493 — 덮지 않는다).
fn unique_salvage_paths(dir: &Path, id: &str) -> (PathBuf, PathBuf) {
    let mut n: u32 = 0;
    loop {
        let stem = if n == 0 {
            id.to_string()
        } else {
            format!("{id}.{n}")
        };
        let stdout = dir.join(format!("{stem}.stdout.log"));
        let stderr = dir.join(format!("{stem}.stderr.log"));
        if !stdout.exists() && !stderr.exists() {
            return (stdout, stderr);
        }
        n += 1;
    }
}

/// 컨테이너에 붙일 호스트 폴더 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub host: PathBuf,
    /// 컨테이너 안 자리(절대 경로 · 고정 문자열).
    pub target: &'static str,
    pub read_only: bool,
}

/// `create` 에 줄 입력.
pub struct CreateInput<'a> {
    pub name: &'a str,
    pub entrypoint: &'a str,
    pub args: &'a [String],
    /// Agent 가 만든 환경 변수(호스트 경로 그대로). 붙인 폴더 아래 경로는 컨테이너 안 경로로 바꿔 넘긴다.
    pub environment: &'a [(OsString, OsString)],
    /// 붙일 폴더들. ★ 결함 273 — Agent 가 **호스트에서 쓰는** 폴더(로그를 받는 작업 폴더)를 여기 넣지 않는다.
    ///   작업이 그 안에 링크를 심으면 Agent 가 링크를 따라가 호스트 파일을 덮는다.
    pub mounts: &'a [Mount],
    pub memory_limit_bytes: u64,
    /// 컨테이너 안에서 쓸 uid:gid (리눅스 docker). 붙인 폴더에 호스트 root 로 쓰지 않게 한다.
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
    if execution.runtime.owner.is_empty() {
        return Err("owner 라벨이 비었다 — 다음 회차가 이 컨테이너를 찾지 못한다(결함 290)".into());
    }
    let memory = input.memory_limit_bytes.to_string();
    let mut args: Vec<OsString> = vec![
        "create".into(),
        format!("--name={}", input.name).into(),
        "--label=gputeer.managed=1".into(),
        format!("--label=gputeer.node={}", execution.runtime.node_id).into(),
        format!("--label=gputeer.owner={}", execution.runtime.owner).into(),
        "--pull=missing".into(),
        "--read-only".into(),
        "--tmpfs=/tmp".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        "--network=none".into(),
        // ★ 결함 274 — IPC 네임스페이스를 호스트 · 다른 컨테이너와 나누지 않는다(/dev/shm 은 이 컨테이너의 tmpfs · 메모리 상한에 든다).
        "--ipc=private".into(),
        format!("--pids-limit={CONTAINER_PIDS_LIMIT}").into(),
        format!("--memory={memory}").into(),
        format!("--memory-swap={memory}").into(),
        "--workdir=/tmp".into(),
    ];
    for mount in input.mounts {
        let host = mount
            .host
            .to_str()
            .ok_or_else(|| format!("붙일 폴더 경로가 UTF-8 이 아니다({:?})", mount.host))?;
        // ★ `--mount` 는 쉼표로 필드를 가른다. 경로에 쉼표 · '=' 가 있으면 필드가 바뀐다 — 받지 않는다.
        if host.contains(',') || host.contains('=') {
            return Err(format!(
                "붙일 폴더 경로에 ',' 나 '=' 가 있다({host:?}) — --mount 필드를 바꿀 수 있어 받지 않는다"
            ));
        }
        let readonly = if mount.read_only { ",readonly" } else { "" };
        args.push(
            format!(
                "--mount=type=bind,source={host},target={}{readonly}",
                mount.target
            )
            .into(),
        );
    }
    match (execution.runtime.flavor, input.user) {
        // ★ rootless podman 은 컨테이너 안 uid 를 subuid 로 옮긴다 — keep-id 가 아니면 붙인 폴더에 못 쓴다.
        (RuntimeFlavor::Podman, _) => args.push("--userns=keep-id".into()),
        (RuntimeFlavor::Docker, Some((uid, gid))) => {
            args.push(format!("--user={uid}:{gid}").into())
        }
        (RuntimeFlavor::Docker, None) => {}
    }
    if execution.runtime.flavor == RuntimeFlavor::Podman {
        // ★ 결함 274 — podman 은 이미지의 VOLUME 을 익명 쓰기 볼륨으로 만들고(--read-only 와 무관), --read-only 일 때
        //   /dev · /dev/shm · /run · /tmp · /var/tmp 를 쓰기 tmpfs 로 둔다. 둘 다 끈다(/tmp 는 위에서 따로 준다).
        //   docker 는 이미지 VOLUME 을 막는 create 옵션이 없다 — 끝에 `rm -v` 로 지울 뿐이다(런북 §5a).
        args.push("--image-volume=ignore".into());
        args.push("--read-only-tmpfs=false".into());
    }
    for (key, value) in input.environment {
        let key = key
            .to_str()
            .ok_or_else(|| format!("환경 변수 이름이 UTF-8 이 아니다({key:?})"))?;
        let value = translate_into_container(value, input.mounts)?;
        if key.is_empty() || key.contains('=') {
            return Err(format!("환경 변수 이름이 잘못됐다({key:?})"));
        }
        args.push(format!("--env={key}={value}").into());
    }
    if let Some(pin) = execution.gpu_pin.as_deref() {
        args.extend(gpu_args(execution.runtime.gpu_request, pin));
    }
    args.push(format!("--entrypoint={}", input.entrypoint).into());
    args.push(execution.pinned_image.clone().into());
    args.extend(input.args.iter().map(OsString::from));
    Ok(args)
}

/// `--gpu-pin` 값(장치 번호를 쉼표로 · `parse_gpu_pin` 이 정리한 것)을 런타임의 GPU 인자로 바꾼다.
///
/// ★ 결함 300 — 여러 장을 한 값으로 넘기면 두 런타임 다 받지 않는다(예상 · 문서 기준). podman 의 CDI 이름은 장치 하나씩이고,
///   docker 는 `--gpus` 값을 CSV 로 읽어 `device=0,1` 이 두 필드(`device=0` · `1`=개수)로 쪼개진다. 그래서 podman 은 장치마다
///   `--device` 를 따로 주고, docker 는 값 전체를 큰따옴표로 감싸 한 필드로 만든다(docker 문서의 `"device=0,1"` 형식).
/// ★ 결함 303 — 모양은 런타임 종류가 아니라 운영자가 고른 `GpuRequest` 가 정한다(WSL docker 29 는 `--gpus` 를 전부 거부했고
///   CDI `all` 만 받았다). `CdiAll` 은 핀을 보지 않는다 — 한 장 노드인지는 Agent 시작 · Manifest 해석 · 컨테이너를 만들기 직전(`run`)이 막는다.
pub fn gpu_args(request: GpuRequest, pin: &str) -> Vec<OsString> {
    let ids: Vec<&str> = pin
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .collect();
    match request {
        GpuRequest::Cdi => ids
            .iter()
            .map(|id| format!("--device=nvidia.com/gpu={id}").into())
            .collect(),
        GpuRequest::Gpus => vec![
            "--gpus".into(),
            format!("\"device={}\"", ids.join(",")).into(),
        ],
        GpuRequest::CdiAll => vec!["--device=nvidia.com/gpu=all".into()],
    }
}

/// 붙인 폴더 아래를 가리키는 호스트 경로를 컨테이너 안 경로로 바꾼다. 붙인 폴더 밖 경로는 그대로 둔다(식별자 같은 값).
fn translate_into_container(value: &OsStr, mounts: &[Mount]) -> Result<String, String> {
    let text = value
        .to_str()
        .ok_or_else(|| format!("환경 변수 값이 UTF-8 이 아니다({value:?})"))?;
    let path = Path::new(text);
    for mount in mounts {
        if let Ok(rest) = path.strip_prefix(&mount.host) {
            let mut inside = String::from(mount.target);
            for component in rest.components() {
                inside.push('/');
                inside.push_str(
                    component
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| format!("경로가 UTF-8 이 아니다({rest:?})"))?,
                );
            }
            return Ok(inside);
        }
    }
    Ok(text.to_string())
}

/// 런타임 명령 한 번의 결과.
#[derive(Debug)]
struct CliOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

/// 런타임 명령을 **시한 안에** 부른다. 시한을 넘기면 그 CLI 프로세스를 죽이고 오류다.
fn run_cli(program: &Path, args: &[OsString], timeout: Duration) -> Result<CliOutput, String> {
    run_cli_detailed(program, args, timeout).map_err(|failure| match failure {
        CliFailure::NotSpawned(why) | CliFailure::AfterSpawn(why) => why,
    })
}

/// CLI 를 부르지 못한 까닭 — **띄우지 못했는가**(명령이 런타임에 닿지 않았다)와 **띄운 뒤**(기다리기 실패 · 시한 초과 — 명령이 닿았을 수 있다)를
/// 가른다(결함 536 — 소유자 정지는 kill 을 띄우지 못했으면 정지 신호가 전달되지 않은 것이 확실하다).
enum CliFailure {
    NotSpawned(String),
    AfterSpawn(String),
}

fn run_cli_detailed(
    program: &Path,
    args: &[OsString],
    timeout: Duration,
) -> Result<CliOutput, CliFailure> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            CliFailure::NotSpawned(format!("{program:?} 를 띄우지 못했다: {error}"))
        })?;
    // 파이프가 차서 CLI 가 멈추지 않게 따로 빨아낸다.
    // ★ 결함 545 (재검수 142) — 읽은 것은 채널로 넘기고(끝난 뒤에도 시한 안에서만 기다린다) 앞 `MAX_CLI_OUTPUT_BYTES` 까지만 담는다(나머지는 버리며
    //   계속 빨아내 자식이 막히지 않게). 전에는 `join()` 에 시한이 없어, 파이프를 물려받은 보조 프로세스가 남으면 EOF 가 오지 않아 영원히 멈췄다.
    let stdout_rx = drain_pipe(child.stdout.take().expect("piped"));
    let stderr_rx = drain_pipe(child.stderr.take().expect("piped"));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                return Err(CliFailure::AfterSpawn(format!(
                    "{program:?} 를 기다리지 못했다: {error}"
                )))
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            // ★ 결함 547 (재검수 143) — 거두기(`wait`)는 **뒤에서** 한다. 커널에서 멈춘(D 상태) 프로세스는 SIGKILL 을 받아도 곧 거둘 수 없어, 여기서
            //   기다리면 시한이 뜻을 잃는다(정지 손잡이 등록 · 판정에 닿지 못한다).
            reap_in_background(child);
            return Err(CliFailure::AfterSpawn(format!(
                "{program:?} {:?} 가 {timeout:?} 안에 끝나지 않았다 — 런타임이 멈췄을 수 있다",
                args.first()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let deadline = started + timeout;
    let collect = |rx: std::sync::mpsc::Receiver<Result<(String, bool), String>>, which: &str| {
        let left = deadline
            .saturating_duration_since(Instant::now())
            .max(PIPE_CLOSE_GRACE);
        rx.recv_timeout(left)
            .map_err(|_| {
                CliFailure::AfterSpawn(format!(
                    "{program:?} {:?} 는 끝났지만 {which} 파이프가 시한 안에 닫히지 않았다 — 파이프를 물려받은 프로세스가 남았을 수 있다",
                    args.first()
                ))
            })?
            .map_err(|why| {
                CliFailure::AfterSpawn(format!(
                    "{program:?} {:?} 의 {which} 를 끝까지 읽지 못했다({why}) — 앞부분만 읽은 출력은 쓰지 않는다",
                    args.first()
                ))
            })
    };
    let (stdout, stdout_cut) = collect(stdout_rx, "stdout")?;
    let (stderr, stderr_cut) = collect(stderr_rx, "stderr")?;
    // ★ 결함 548 (재검수 143) — 출력이 상한을 넘어 **잘렸으면** 그 출력을 쓰지 않는다(예: `ps -a -q` 목록이 잘리면 뒤의 컨테이너를 놓친 채 정리 성공이
    //   된다). 실패로 돌려준다 — 부르는 쪽은 확인하지 못한 것으로 다룬다.
    if stdout_cut || stderr_cut {
        return Err(CliFailure::AfterSpawn(format!(
            "{program:?} {:?} 의 출력이 상한({MAX_CLI_OUTPUT_BYTES} 바이트)을 넘어 잘렸다 — 잘린 출력은 쓰지 않는다",
            args.first()
        )));
    }
    Ok(CliOutput {
        status,
        stdout,
        stderr,
    })
}

/// 런타임 CLI 출력 하나에서 담는 상한(결함 545 — 넘는 것은 버린다).
const MAX_CLI_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// 자식이 끝난 뒤 파이프가 닫히기를 기다리는 최소 여유(시한이 이미 지났어도).
const PIPE_CLOSE_GRACE: Duration = Duration::from_secs(2);

/// 파이프를 끝까지 빨아내고, 앞 `MAX_CLI_OUTPUT_BYTES` 를 문자열로 · 잘렸는지를 함께 채널에 넘긴다(EOF 때 한 번). 읽기 오류면 그 오류를 넘긴다.
fn drain_pipe(
    mut pipe: impl Read + Send + 'static,
) -> std::sync::mpsc::Receiver<Result<(String, bool), String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = drain_capped(&mut pipe)
            .map(|(kept, cut)| (String::from_utf8_lossy(&kept).into_owned(), cut))
            .map_err(|error| error.to_string());
        let _ = tx.send(result);
    });
    rx
}

/// 끝까지 읽되 앞 `MAX_CLI_OUTPUT_BYTES` 만 담는다. 넘었으면 `true`(잘렸다).
///
/// ★ 결함 550 (재검수 144) — 읽기 **오류**는 EOF 가 아니다 — 그대로 돌려준다(전에는 EOF 처럼 다뤄 앞부분만 읽은 출력을 "잘림 없음" 으로 넘겼다).
///   `Interrupted` 는 다시 읽는다.
fn drain_capped(pipe: &mut impl Read) -> std::io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::new();
    let mut cut = false;
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let room = MAX_CLI_OUTPUT_BYTES.saturating_sub(kept.len());
                if n > room {
                    cut = true;
                }
                kept.extend_from_slice(&chunk[..n.min(room)]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok((kept, cut))
}

/// 시한이 지나 죽인 자식을 **뒤에서** 거둔다(결함 547 — 커널에서 멈춘 프로세스를 기다리지 않는다).
fn reap_in_background(mut child: std::process::Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn cli_ok(program: &Path, args: &[OsString], timeout: Duration) -> Result<CliOutput, String> {
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

/// 런타임을 **조회**하고 성공이면 stdout(앞뒤 공백 제거)을 돌려준다 — node-doctor 가 쓰는 공개 입구(결함 559).
///
/// ★ 규칙은 [`run_cli_detailed`] 하나다 — 시한(넘으면 죽이고 뒤에서 거둔다) · 파이프는 채널로 받고 끝난 뒤에도 시한까지만 · 출력 4MiB 상한 · 잘리면
///   실패. 전에 node-doctor 는 `Command::output()` 을 따로 써서 daemon 이 멈추면 설치가 무기한 멈췄다. 두 벌로 두지 않는다.
pub fn query_runtime_text(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let output = run_cli(program, &args, timeout)?;
    if output.status.success() {
        Ok(output.stdout.trim().to_string())
    } else {
        Err(format!(
            "{program:?} {} 실패({}): {}",
            args.iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" "),
            output.status,
            output.stderr.trim()
        ))
    }
}

/// `rm -f -v <이름>` — 컨테이너와 그 익명 볼륨까지 지운다(결함 274). 없는 이름이면 런타임이 실패를 돌려준다.
fn remove_container(program: &Path, name: &str) -> Result<(), String> {
    cli_ok(
        program,
        &["rm".into(), "-f".into(), "-v".into(), name.into()],
        SHORT_TIMEOUT,
    )
    .map(|_| ())
}

/// 실행 중인 컨테이너를 **밖에서** 끝내는 손잡이(`CLAUDE.md` §0.1).
#[derive(Debug, Clone)]
pub struct ContainerStopper {
    program: PathBuf,
    name: String,
    /// 실행 쪽이 관측한 종료(코드 · OOM) — 지우기 **전에** 적는다. 소유자 정지가 멈춘 원인을 가를 때, 실행 쪽이 이미 컨테이너를 지웠으면 이것을 본다
    /// (결함 520 수정이 만든 경쟁 — kill 직후 실행 쪽이 종료를 보고 로그를 받아 지우면 사후 조회가 "없다" 가 됐다).
    observed_exit: std::sync::Arc<std::sync::Mutex<Option<(i64, bool)>>>,
}

impl ContainerStopper {
    /// 컨테이너를 즉시 끝낸다(SIGKILL). 컨테이너 안의 프로세스 트리 전체가 같이 끝난다.
    ///
    /// ★ 결함 277 — kill 이 실패했는데 이미 끝나 있으면 **실패**(`ALREADY_EXITED`)다. 전에는 성공으로 바꿔, 스스로 코드 0 으로
    ///   끝난 작업이 "소유자가 멈췄다" 로 보고됐다 — 정지 요청이 받아들여진 것과 정지가 종료 원인인 것은 다르다.
    pub fn stop(&self) -> Result<(), String> {
        // ★ 결함 531 (재검수 136) — kill 을 띄우지 못했거나 응답이 없어도(시한 초과) **곧바로 끝내지 않는다**. 띄운 뒤 응답만 없었으면 SIGKILL 이
        //   적용됐을 수 있다 — 아래의 같은 판정(나눈 관측 → 조회)을 거친다. 시한은 확인 조회와 같은 15초(kill 은 가벼운 신호 명령이다).
        let kill = match run_cli_detailed(
            &self.program,
            &["kill".into(), self.name.clone().into()],
            CONFIRM_TIMEOUT,
        ) {
            // ★ 결함 536 (재검수 138) — kill 을 **띄우지 못했으면** 정지 신호가 전달되지 않은 것이 확실하다. 관측된 137(작업이 스스로 끝남)을
            //   소유자 정지로 삼지 않는다.
            Err(CliFailure::NotSpawned(why)) => {
                return Err(format!(
                    "OWNER_STOP_NOT_SENT: kill 을 띄우지 못해 정지 신호가 전달되지 않았다 — {why}"
                ))
            }
            Err(CliFailure::AfterSpawn(why)) => Err(why),
            Ok(output) => Ok(output),
        };
        // ★ 결함 506 (재검수 127) — kill 의 0 은 접수일 뿐이다. 멈춤을 `inspect` 로 확인해야 소유자에게 "멈췄다" 고 답한다(전에는 곧바로 성공이라
        //   패널이 `owner_stopped` 를 적고, 계속 돈 작업이 나중에 정상 종료해도 INTERRUPTED 로 보고돼 재배치될 수 있었다).
        // ★ 결함 506 · 520 · 525 · 527 — kill 의 응답(성공 · 실패)과 상관없이 **멈췄는지와 이 정지가 원인인지**(SIGKILL 의 종료 코드 137 · OOM 아님)를
        //   본다. 성공 응답은 접수일 뿐이고(506), 실패 응답도 죽이지 않았다는 증거가 아니다(527 — start 에 적용한 490 과 같은 규칙). 실행 쪽이 이미
        //   종료를 보고 지웠으면 그때 나눈 관측으로 판정한다(525). 스스로 끝났으면(137 이 아님) `ALREADY_EXITED` — 소유자 정지로 적지 않는다(520 · 277).
        //   ★ 남는 것 — 작업이 바로 그때 스스로 137 로 끝나면 가르지 못한다.
        let kill_note = match &kill {
            Ok(output) if output.status.success() => "kill 접수".to_string(),
            Ok(output) => format!(
                "kill 실패 응답({}): {}",
                output.status,
                output.stderr.trim()
            ),
            Err(why) => format!("kill 응답을 받지 못했다({why})"),
        };
        let judge = |code: i64, oom: bool| {
            if code == 137 && !oom {
                Ok(())
            } else {
                Err(format!(
                    "ALREADY_EXITED: 멈췄지만 이 정지가 원인이 아니다 — 작업이 스스로 끝났다(종료 코드 {code} · OOM {oom} · {kill_note})"
                ))
            }
        };
        let observed = || *self.observed_exit.lock().unwrap_or_else(|e| e.into_inner());
        let mut last = String::new();
        for attempt in 0..STOP_CONFIRM_TRIES {
            if let Some((code, oom)) = observed() {
                return judge(code, oom);
            }
            match inspect_state_within(&self.program, &self.name, CONFIRM_TIMEOUT) {
                Ok(Some(exit)) => return judge(exit.exit_code, exit.oom_killed),
                // ★ 결함 529 (재검수 135) — kill 이 실패로 답했어도 SIGKILL 이 진행 중일 수 있다 — 성공 응답과 같이 끝까지(10번) 본다.
                Ok(None) => last = "아직 돈다".into(),
                Err(why) => {
                    if let Some((code, oom)) = observed() {
                        return judge(code, oom);
                    }
                    return Err(format!(
                        "KILL_UNCONFIRMED: 멈춤 · 원인(종료 코드)을 확인하지 못했다 — {kill_note} · {why}"
                    ));
                }
            }
            if attempt + 1 < STOP_CONFIRM_TRIES {
                std::thread::sleep(STOP_CONFIRM_INTERVAL);
            }
        }
        Err(format!(
            "KILL_UNCONFIRMED: kill 을 접수했지만 멈춤을 확인하지 못했다 — {kill_note} · {last}"
        ))
    }
}

/// 컨테이너 객체가 지금 어떤가 — 여러 축 판정의 "컨테이너" 축(2026-09-27 보수 규칙).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerLeft {
    /// 없다(처음부터 만들지 않았거나, 지운 뒤 `inspect` 가 "없다" 고 답했다 — `rm` 의 응답만으로는 이 값이 되지 않는다 · 결함 503 · 516).
    Removed,
    /// 남겼다(더 돌지 않는데 로그를 못 받았다 · 멈추지 못해 지우지 않았다 등 — 일부러 보존).
    Kept,
    /// 지우려 했는데 결과를 모른다.
    Unknown,
}

/// 컨테이너가 끝난 뒤 관측한 것.
///
/// ★ 이미 관측한 사실(종료 코드 · 로그 완결)과 정리 결과(`container`)를 **따로** 싣는다 — 정리 실패를 작업 실패로 바꾸면 반대 방향의 거짓
///   보고다(2026-09-27 합의). 정리가 불확실하면 사건 표식이 따로 남는다(`needs_human`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerExit {
    pub exit_code: i64,
    /// 메모리 상한에 걸려 커널이 죽였는가.
    pub oom_killed: bool,
    /// 작업 출력(stdout · stderr)을 끝까지 받았는가. 못 받았으면 반쯤 쓴 파일은 지워져 있다(확정이 실패하게).
    pub logs_complete: bool,
    pub container: ContainerLeft,
    /// 로그 받기 · 정리가 **왜** 그렇게 됐는가(런타임 오류 문장). 사건 표식에 그대로 싣는다(결함 504 — 전에는 eprintln 으로만 나가 사라졌다).
    pub note: String,
}

impl ContainerExit {
    /// 사람이 봐야 하는가 — 컨테이너가 없음을 확인하지 못했다.
    pub fn needs_human(&self) -> bool {
        self.container != ContainerLeft::Removed
    }
}

/// `cdi-all` 로 GPU 를 넘기는 실행이면 NVML 이 GPU 를 **정확히 한 장** 볼 때만 통과한다. 못 보면 거부한다(지어내지 않는다).
/// 그 밖의 실행은 NVML 을 보지 않는다.
pub fn cdi_all_ready(
    execution: &ContainerExecution,
    gpu_count: impl FnOnce() -> Result<usize, String>,
) -> Result<(), String> {
    if !(execution.runtime.pass_gpu && execution.runtime.gpu_request == GpuRequest::CdiAll) {
        return Ok(());
    }
    match gpu_count() {
        Ok(1) => Ok(()),
        Ok(count) => Err(format!(
            "CONTAINER_GPU_ALL_NOT_PINNED: 컨테이너를 만들기 직전 NVML 이 GPU {count}장을 본다 — cdi-all 은 한 장일 때만 넘긴다"
        )),
        Err(why) => Err(format!(
            "CONTAINER_GPU_ALL_NOT_PINNED: 컨테이너를 만들기 직전 GPU 가 한 장임을 NVML 로 확인하지 못했다: {why}"
        )),
    }
}

/// 실행 결과의 실패 — 여러 축으로 나눈다(2026-09-27 보수 규칙 · 재검수 121~124 합의).
///
/// ★ 전에는 `NotObserved` · `RemovedUnobserved` 두 이름으로 뭉개, "컨테이너를 남겼는데 지웠다" 를 돌려주는 모순이 있었다(코덱스 지적).
///   실행 · 정지 · 로그 · 컨테이너를 각각 싣는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerRunError {
    /// 작업은 **돌지 않았다**(만들기 전 · 만들기 · 만든 뒤 확인 실패 — start 를 부르기 전이다). `container` 는 남은 객체의 상태다.
    /// ★ 결함 490 · 528 — start 의 실패(런타임이 실패라고 답함 · 무응답)는 여기로 오지 않는다. 작업이 돌았을 수 있어 `Unobserved` 다.
    NotStarted {
        detail: String,
        container: ContainerLeft,
    },
    /// 작업이 **돌았을 수 있고** 종료 코드를 모른다.
    Unobserved {
        detail: String,
        /// 더 돌지 않음을 확인했는가 — `inspect` 가 "돌지 않는다" · "없다" 고 답했다(kill 의 응답만으로는 true 가 되지 않는다 · 결함 502).
        stopped: bool,
        /// 작업 출력을 끝까지 받았는가.
        logs_complete: bool,
        container: ContainerLeft,
    },
}

impl ContainerRunError {
    /// 사람이 봐야 하는가 — 멈춤을 확인하지 못했거나 컨테이너가 없음을 확인하지 못했다.
    pub fn needs_human(&self) -> bool {
        match self {
            Self::NotStarted { container, .. } => *container != ContainerLeft::Removed,
            Self::Unobserved {
                stopped, container, ..
            } => !*stopped || *container != ContainerLeft::Removed,
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::NotStarted { detail, .. } | Self::Unobserved { detail, .. } => detail,
        }
    }
}

/// 종료를 확인하는 주기와, 연속으로 몇 번 확인에 실패하면 "관측 못 함" 으로 볼지.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const POLL_FAILURES_TOLERATED: u32 = 5;

/// 만들고 → 시작하고 → 끝날 때까지 상태를 확인하고 → 출력을 받고 → 지운다.
///
/// ★ `run` 한 번으로 하지 않는다. `run` 의 종료 코드는 런타임 오류(125~127)와 작업 종료가 섞인다 — 작업이 125 로 끝난 것과
///   이미지를 못 받은 것이 같아 보인다. 단계를 나누면 어디서 실패했는지 안다.
/// ★ 결함 276 — `wait` 를 쓰지 않는다. 시한이 없어 데몬이 멈추면 영원히 기다렸다. 대신 `inspect` 를 주기로 부른다(명령마다 시한).
///   종료를 관측하지 못하면 kill 하고 멈춤을 `inspect` 로 확인한 뒤 로그를 받고, 둘 다 확인됐을 때만 지운다(결함 502 · 503). 하나라도
///   확인하지 못하면 컨테이너를 남기고 사건 표식을 쓴다.
pub fn run(
    execution: &ContainerExecution,
    input: &CreateInput<'_>,
    stdout_path: Option<&Path>,
    stderr_path: Option<&Path>,
    on_started: impl FnOnce(ContainerStopper),
) -> Result<ContainerExit, ContainerRunError> {
    run_with_gpu_count(
        execution,
        input,
        stdout_path,
        stderr_path,
        on_started,
        || {
            gputeer_runtime_nvml::observe()
                .map(|snapshot| snapshot.gpus.len())
                .map_err(|e| format!("{e:?}"))
        },
    )
}

/// `run` 과 같다 — cdi-all 장수를 무엇으로 셀지만 받는다(시험이 NVML 없이 "생성 뒤 늘어난 GPU" 를 흉내 낸다 · 결함 468).
pub fn run_with_gpu_count(
    execution: &ContainerExecution,
    input: &CreateInput<'_>,
    stdout_path: Option<&Path>,
    stderr_path: Option<&Path>,
    on_started: impl FnOnce(ContainerStopper),
    nvml_gpu_count: impl Fn() -> Result<usize, String>,
) -> Result<ContainerExit, ContainerRunError> {
    let result = run_inner(
        execution,
        input,
        stdout_path,
        stderr_path,
        on_started,
        nvml_gpu_count,
    );
    record_incident_if_needed(&execution.runtime, input.name, result)
}

/// 지우고 그 결과를 컨테이너 축으로 돌려준다. `rm` 의 성공 · 실패와 상관없이 뒤이은 `inspect` 가 "없다"(docker · podman 공통 "no such container")고
/// 할 때만 없는 것이다.
///
/// ★ 결함 503 (재검수 126) — `rm` 의 0 은 요청을 **접수했다**는 뜻이다. 뒤이어 `inspect` 가 "없다" 고 답할 때만 없다고 본다(전에는 0 만으로
///   `Removed` 였다 — 삭제 전에 런타임이 멈추면 컨테이너가 남았는데 표식 없이 작업 폴더를 지웠다).
fn remove_container_fact(program: &Path, name: &str) -> (ContainerLeft, String) {
    match remove_container(program, name) {
        Ok(()) => match inspect_exists(program, name) {
            Ok(false) => (ContainerLeft::Removed, "rm 성공 · 없음 확인".into()),
            Ok(true) => (
                ContainerLeft::Unknown,
                "rm 이 성공이라 답했지만 컨테이너가 아직 있다".into(),
            ),
            Err(why) => (
                ContainerLeft::Unknown,
                format!("rm 이 성공이라 답했지만 없어졌는지 확인하지 못했다({why})"),
            ),
        },
        // ★ 결함 516 (재검수 131) — rm 의 "없다" 오류도 조회로 확인한다(전에는 그 문장만으로 `Removed` 였다 — 낡은 오류면 남은 컨테이너를 놓쳤다).
        Err(why) => match inspect_exists(program, name) {
            Ok(false) => (
                ContainerLeft::Removed,
                format!("rm 이 실패했지만 없음 확인({why})"),
            ),
            Ok(true) => (
                ContainerLeft::Unknown,
                format!("rm 실패 · 아직 있다({why})"),
            ),
            Err(inspect) => (
                ContainerLeft::Unknown,
                format!("rm 실패({why}) · 있는지도 모른다({inspect})"),
            ),
        },
    }
}

/// 이 이름의 컨테이너의 owner 라벨 — 없으면 `Ok(None)`, 있는데 라벨이 없으면 `Ok(Some(""))`(결함 522).
///
/// ★ 결함 526 (재검수 134) — owner 와 함께 **컨테이너 ID** 를 한 번에 돌려준다. 확인한 뒤 지울 때 그 ID 로 지운다(이름으로 다시 지우면 그 사이 이름의 대상이
///   바뀌었을 때 다른 것을 지운다).
fn inspect_owner(program: &Path, name: &str) -> Result<Option<(String, String)>, String> {
    match cli_ok(
        program,
        &[
            "inspect".into(),
            "--format={{.Id}} {{index .Config.Labels \"gputeer.owner\"}}".into(),
            name.into(),
        ],
        CONFIRM_TIMEOUT,
    ) {
        Ok(output) => {
            let text = output.stdout.trim();
            let (id, owner) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
            if !((12..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit())) {
                return Err(format!(
                    "inspect 가 컨테이너 ID 를 돌려주지 않았다({text:?})"
                ));
            }
            let owner = owner.trim();
            Ok(Some((
                id.to_string(),
                if owner == "<no value>" {
                    String::new()
                } else {
                    owner.to_string()
                },
            )))
        }
        Err(why) if says_no_such_container(&why) => Ok(None),
        Err(why) => Err(why),
    }
}

/// 컨테이너(ID)의 이름 — docker 는 앞에 `/` 를 붙인다(떼고 돌려준다).
fn inspect_name(program: &Path, id: &str) -> Result<String, String> {
    let output = cli_ok(
        program,
        &["inspect".into(), "--format={{.Name}}".into(), id.into()],
        CONFIRM_TIMEOUT,
    )?;
    Ok(output.stdout.trim().trim_start_matches('/').to_string())
}

/// 컨테이너가 **있는가** — 런타임이 "없다" 고 답하면 false, 상태를 돌려주면 true, 그 밖의 실패는 모른다(Err). 확인 조회라 시한은 15초다(결함 543).
fn inspect_exists(program: &Path, name: &str) -> Result<bool, String> {
    match cli_ok(
        program,
        &[
            "inspect".into(),
            "--format={{.State.Running}}".into(),
            name.into(),
        ],
        CONFIRM_TIMEOUT,
    ) {
        Ok(_) => Ok(true),
        Err(why) if says_no_such_container(&why) => Ok(false),
        Err(why) => Err(why),
    }
}

fn run_inner(
    execution: &ContainerExecution,
    input: &CreateInput<'_>,
    stdout_path: Option<&Path>,
    stderr_path: Option<&Path>,
    on_started: impl FnOnce(ContainerStopper),
    nvml_gpu_count: impl Fn() -> Result<usize, String>,
) -> Result<ContainerExit, ContainerRunError> {
    let program = execution.runtime.program.as_path();
    let not_created = |detail: String| ContainerRunError::NotStarted {
        detail,
        container: ContainerLeft::Removed,
    };
    // ★ 결함 462 (재검수 117) · 468 (재검수 118) — cdi-all 은 GPU 를 전부 넘긴다. 확인과 실제 장치 해석(create) 사이를 좁힌다:
    //   이미지를 **먼저** 받고(create 가 수십 분 이미지를 받는 동안 GPU 가 늘 수 있었다) → 장수 확인 → create → 다시 확인 → start.
    //   ★ 남는 창 — create 한 번 동안의 장치 변경은 닫지 못한다(확인 두 번이지 격리 보장이 아니다).
    let passes_all_gpus =
        execution.runtime.pass_gpu && execution.runtime.gpu_request == GpuRequest::CdiAll;
    if passes_all_gpus {
        cli_ok(
            program,
            &["pull".into(), execution.pinned_image.clone().into()],
            CREATE_TIMEOUT,
        )
        .map_err(|why| not_created(format!("pull: {why}")))?;
    }
    cdi_all_ready(execution, &nvml_gpu_count).map_err(not_created)?;
    let create = create_args(execution, input).map_err(not_created)?;
    // ★ 결함 276 — 같은 이름이 남아 있으면(전 실행의 rm 실패) create 가 충돌한다. 이름은 이 시도의 것이라 남은 것도 이 시도의 것이다.
    // ★ 2026-09-27 보수 규칙(코덱스 지적) — 전에는 결과를 버렸다(`let _`). 지우지 못했으면(남은 것이 무엇인지 모른다) 만들지 않고 사람에게 넘긴다.
    //   Agent 는 기동 때 남은 컨테이너를 로그를 건진 뒤 지우므로(결함 489) 여기서 남아 있는 것은 뜻밖이다.
    // ★ 결함 522 (재검수 133) — 이름은 시도 id 로만 만들어 노드 · owner 를 담지 않는다. 같은 이름이 있으면 **owner 라벨이 이 Agent 의 것일 때만**
    //   지운다. 다른 owner 이거나 확인하지 못하면 지우지 않고 사람에게 넘긴다(다른 Agent · 운영 절차의 컨테이너일 수 있다).
    // ★ 결함 530 (재검수 135) — 이 Agent 의 것이어도 만들기 직전에는 **자동으로 지우지 않는다.** 기동 때 정리(멈춤 확인 → 로그 건지기 → 삭제)를 거치지 않은
    //   강제 삭제가 되고, owner 일치는 "지워도 된다" 의 증거가 아니다(기동 때 목록이 일시적으로 비었다면 이전 실행이 아직 돌 수 있다). 남겨 두고 사람에게
    //   넘긴다 — 해제하면 다음 기동의 남은 컨테이너 정리가 멈춤 · 로그 건지기를 거쳐 치운다.
    match inspect_owner(program, input.name) {
        Ok(None) => {}
        Ok(Some((id, owner))) if owner == execution.runtime.owner => {
            return Err(ContainerRunError::NotStarted {
                detail: format!(
                    "이 Agent 의 같은 이름 컨테이너(ID {id})가 남아 있다 — 만들기 직전에는 자동으로 지우지 않는다(멈춤 · 로그를 확인하지 않은 삭제가 된다) · 사람이 확인한 뒤 해제하면 다음 기동의 남은 컨테이너 정리가 치운다"
                ),
                container: ContainerLeft::Kept,
            })
        }
        Ok(Some((id, owner))) => {
            return Err(ContainerRunError::NotStarted {
                detail: format!(
                    "같은 이름의 컨테이너(ID {id})가 이 Agent 의 것이 아니다(owner 라벨 {owner:?}) — 지우지도 만들지도 않았다(사람이 확인한다)"
                ),
                container: ContainerLeft::Unknown,
            })
        }
        Err(why) => {
            return Err(ContainerRunError::NotStarted {
                detail: format!(
                    "같은 이름의 컨테이너가 있는지 · 누구의 것인지 확인하지 못했다({why}) — 만들지 않았다"
                ),
                container: ContainerLeft::Unknown,
            })
        }
    }
    let created = match run_cli_detailed(program, &create, CREATE_TIMEOUT) {
        Ok(output) if output.status.success() => Ok(output),
        // ★ 결함 546 (재검수 142) — create 를 **띄우지 못했으면** 요청이 런타임에 닿지 않았다 — 이 시도의 컨테이너는 없다(`Removed`). 뒤이은 조회도
        //   실행 파일이 없어 실패할 것이라 조회하지 않는다(조회 실패를 "모름" 으로 굳혀 표식을 남기지 않게).
        Err(CliFailure::NotSpawned(why)) => {
            return Err(ContainerRunError::NotStarted {
                detail: format!(
                    "create 를 띄우지 못했다({why}) — 요청이 런타임에 닿지 않아 컨테이너를 만들지 않았다"
                ),
                container: ContainerLeft::Removed,
            })
        }
        Ok(output) => Err(format!(
            "{:?} 실패({}): {}",
            create.first(),
            output.status,
            output.stderr.trim()
        )),
        Err(CliFailure::AfterSpawn(why)) => Err(why),
    };
    let created = match created {
        Ok(output) => output,
        Err(why) => {
            // ★ 결함 519 (재검수 132) — **이름으로 지우지 않는다.** 사전 정리 뒤 다른 절차가 같은 이름으로 만든 컨테이너 때문에 실패했을 수 있다
            //   (전에는 그것을 `rm -f -v <이름>` 으로 지웠다). 같은 이름이 있으면 무엇인지 모르니 사람에게 넘긴다. 이 시도가 반쯤 만든 것이면
            //   다음 기동의 남은 컨테이너 정리가 owner 라벨로 찾아 ID 로 치운다.
            let (left, fact) = match inspect_exists(program, input.name) {
                Ok(false) => (ContainerLeft::Removed, "같은 이름의 컨테이너 없음 확인".to_string()),
                Ok(true) => (
                    ContainerLeft::Unknown,
                    "같은 이름의 컨테이너가 있다 — 이 시도가 반쯤 만든 것인지 다른 것인지 몰라 지우지 않았다".to_string(),
                ),
                Err(e) => (
                    ContainerLeft::Unknown,
                    format!("같은 이름의 컨테이너가 있는지 모른다({e})"),
                ),
            };
            return Err(ContainerRunError::NotStarted {
                detail: format!("create: {why} · {fact}"),
                container: left,
            });
        }
    };
    // ★ 결함 515 (재검수 131) — 만든 뒤의 모든 조작(start · inspect · kill · logs · rm)은 create 가 돌려준 **컨테이너 ID** 로 한다. 이름으로 하면 그 사이
    //   같은 이름의 다른 컨테이너가 생겼을 때 그 로그를 이 작업의 것으로 확정하고 그것을 지운다(악의 없는 운영 절차로도 난다).
    let id = created.stdout.trim().to_string();
    // ★ 결함 519 (재검수 132) — 모양(16진수 12~64자 — docker · podman 의 컨테이너 ID)만 보지 않고, `inspect` 로 **그 ID 의 이름이 이 시도의 이름**
    //   인지 대조한다(런타임 래퍼가 ID 대신 이름을 찍으면 그 뒤 조작이 다시 이름 기준이 된다). 확인하지 못하면 **지우지 않고** 사람에게 넘긴다.
    let id_shape_ok = (12..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit());
    let bound = if id_shape_ok {
        inspect_name(program, &id).map(|name| name == input.name)
    } else {
        Ok(false)
    };
    if !matches!(bound, Ok(true)) {
        let why = match bound {
            Ok(_) => "모양이 ID 가 아니거나 다른 컨테이너를 가리킨다".to_string(),
            Err(e) => format!("대조하지 못했다({e})"),
        };
        return Err(ContainerRunError::NotStarted {
            detail: format!(
                "create 가 돌려준 ID({id:?})를 이 시도의 컨테이너 {} 로 확인하지 못했다({why}) — 시작하지도 지우지도 않았다(사람이 확인한다)",
                input.name
            ),
            container: ContainerLeft::Unknown,
        });
    }
    let target = id.as_str();
    if let Err(why) = cdi_all_ready(execution, &nvml_gpu_count) {
        let (left, removed) = remove_container_fact(program, target);
        return Err(ContainerRunError::NotStarted {
            detail: format!("create 뒤 다시 확인: {why} · {removed}"),
            container: left,
        });
    }
    // ★ 결함 490 (재검수 124) — start 의 **어떤** 실패도 "돌지 않았다" 의 증거가 아니다. OCI `poststart` 훅은 사용자 프로세스가 돈 **뒤에**
    //   돌고, 실패하면 start 가 실패로 답한다(결함 488 의 "런타임이 실패라고 답하면 시작하지 않았다" 가 틀렸다). 응답이 없거나 시한을 넘긴
    //   경우도 같다. 그래서 start 실패는 전부 "시작했는지 모른다"(Unobserved)이고, 그 뒤는 남은 컨테이너 정리와 같은 순서다:
    //   멈춤 확인(`stop_and_confirm`) → 로그 받기 → 둘 다 확인됐을 때만 지운다. 하나라도 확인하지 못하면 **지우지 않고** 남긴다(사건 표식).
    //   ★ 결함 491 — 전에는 응답 없음 경로가 로그를 못 받아도 지웠다(출력 · 원본 로그를 모두 잃었다).
    //   ★ 대가 — 진입점 오타처럼 정말 시작하지 않은 실패도 "종료 코드 없음"(NoCode)으로 확정 단계를 거친다. 시작 여부를 런타임 답만으로
    //     가를 수 없어서다(보수 규칙 — 불확실하면 성공으로도 "안 돌았다" 로도 단정하지 않는다).
    let start_args: [OsString; 2] = ["start".into(), target.into()];
    // (응답이 있었는가, 사유)
    let start_failure = match run_cli_detailed(program, &start_args, SHORT_TIMEOUT) {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some((
            true,
            format!("start 실패({}): {}", output.status, output.stderr.trim()),
        )),
        // ★ 결함 537 (재검수 139) — start 를 **띄우지 못했으면** 요청이 런타임에 닿지 않았다 — 작업은 시작하지 않았다(`NotStarted`). 정지 손잡이를
        //   넘기지 않고(`WORKLOAD_SPAWNED` · 패널 등록 없음 — 실행 API 계약) 만든 컨테이너는 확인한 ID 로 지운다(한 번도 돌지 않았다). 지웠는지
        //   확인하지 못하면(런타임 실행 파일이 여전히 없으면 조회도 실패한다) 사람에게 넘긴다.
        Err(CliFailure::NotSpawned(why)) => {
            let (left, removed) = remove_container_fact(program, target);
            return Err(ContainerRunError::NotStarted {
                detail: format!(
                    "start 를 띄우지 못했다({why}) — 요청이 런타임에 닿지 않아 시작하지 않았다 · {removed}"
                ),
                container: left,
            });
        }
        Err(CliFailure::AfterSpawn(why)) => {
            Some((false, format!("start 가 응답하지 않았다({why})")))
        }
    };
    if let Some((answered, why)) = start_failure {
        let head = format!("{why} — 시작했는지 모른다");
        // ★ 결함 532 (재검수 137) — start 가 **응답하지 않았으면** 데몬에 접수된 start 가 뒤늦게 적용될 수 있다. 지금 멈춰 보여도(`created`) 멈춤
        //   확인이 아니다 — kill 도 소용없다(아직 안 돈다). 지우지 않고 정지 손잡이를 넘긴 뒤 사람에게 넘긴다(사건 표식).
        if !answered {
            on_started(ContainerStopper {
                program: program.to_path_buf(),
                name: target.to_string(),
                observed_exit: Default::default(),
            });
            return Err(ContainerRunError::Unobserved {
                detail: format!(
                    "{head} · 접수된 start 가 뒤늦게 적용될 수 있어 멈춤을 확인할 수 없다 — 컨테이너 {} (ID {target}) 를 남겼다(사람이 확인한다)",
                    input.name
                ),
                stopped: false,
                logs_complete: false,
                container: ContainerLeft::Kept,
            });
        }
        if let Err(stop) = stop_and_confirm(program, target) {
            // ★ 결함 475 (재검수 120) — 돌고 있을 수 있으니 정지 손잡이를 **넘긴다**(같은 프로세스가 도는 동안의 소유자 손잡이).
            on_started(ContainerStopper {
                program: program.to_path_buf(),
                name: target.to_string(),
                observed_exit: Default::default(),
            });
            return Err(ContainerRunError::Unobserved {
                detail: format!(
                    "{head} · 멈춤을 확인하지 못했다({stop}) — 컨테이너 {} (ID {target}) 를 남겼다(돌고 있을 수 있다 · 사람이 확인한다)",
                    input.name
                ),
                stopped: false,
                logs_complete: false,
                container: ContainerLeft::Kept,
            });
        }
        if let Err(e) = save_logs(program, target, stdout_path, stderr_path) {
            let discard = discard_partial_outputs(stdout_path, stderr_path);
            return Err(ContainerRunError::Unobserved {
                detail: format!(
                    "{head} · 멈췄다 · 로그 못 남김({e}){discard} — 컨테이너를 남겼다(런타임에 로그가 남는다 · 사람이 확인한다)"
                ),
                stopped: true,
                logs_complete: false,
                container: ContainerLeft::Kept,
            });
        }
        let (left, removed) = remove_container_fact(program, target);
        return Err(ContainerRunError::Unobserved {
            detail: format!("{head} · 멈췄다 · 로그 남김 · {removed}"),
            stopped: true,
            logs_complete: true,
            container: left,
        });
    }
    let stopper = ContainerStopper {
        program: program.to_path_buf(),
        name: target.to_string(),
        observed_exit: Default::default(),
    };
    on_started(stopper.clone());
    // ★ 시한 없이 기다린다 — 작업 길이는 Lease 가 정한다. 멈추는 것은 소유자 손잡이(kill)가 한다.
    let mut failures: u32 = 0;
    let mut exit = loop {
        match inspect_state(program, target) {
            Ok(Some(exit)) => break exit,
            Ok(None) => failures = 0,
            Err(why) => {
                failures += 1;
                if failures >= POLL_FAILURES_TOLERATED {
                    // ★ 2026-09-27 보수 규칙 — 멈춤을 확인하지 못하면 지우지 않는다(코덱스 지적: kill 이 실패했는데 `rm -f` 로 가면 로그를 받은 뒤의
                    //   출력 · 체크포인트를 잃는다). 로그를 못 받아도 지우지 않는다(결함 487 — 런타임에 온전한 로그가 남는다).
                    // ★ 결함 502 (재검수 126) — kill 의 0 이 아니라 `inspect` 로 멈춤을 확인한다.
                    let (stopped, killed) = match stop_and_confirm(program, target) {
                        Ok(()) => (true, "멈춤 확인".to_string()),
                        Err(why) => (false, format!("멈춤을 확인하지 못했다({why})")),
                    };
                    // ★ 결함 483 (재검수 122) — 지우기 **전에** 로그를 남긴다. 못 남기면 반쯤 쓴 파일을 지운다(빈 출력이 성공이 되지 않게 · 487).
                    let (saved, logs) = match save_logs(program, target, stdout_path, stderr_path) {
                        Ok(()) if stopped => (true, "로그 남김".to_string()),
                        Ok(()) => (
                            true,
                            "지금까지의 로그만 남김(멈춤을 확인하지 못해 완결이 아니다)"
                                .to_string(),
                        ),
                        Err(e) => {
                            let discard = discard_partial_outputs(stdout_path, stderr_path);
                            (false, format!("로그 못 남김({e}){discard}"))
                        }
                    };
                    // ★ 결함 496 (재검수 125) — 멈춤을 확인하지 못했으면 `logs` 성공은 완결의 증거가 아니다(도는 컨테이너의 로그는 그 순간까지다).
                    let logs_complete = stopped && saved;
                    let head = format!(
                        "종료를 {failures}번 연속 확인하지 못했다({why}) · {killed} · {logs}"
                    );
                    if !stopped || !logs_complete {
                        return Err(ContainerRunError::Unobserved {
                            detail: format!("{head} — 컨테이너를 남겼다(사람이 확인한다)"),
                            stopped,
                            logs_complete,
                            container: ContainerLeft::Kept,
                        });
                    }
                    let (left, removed) = remove_container_fact(program, target);
                    return Err(ContainerRunError::Unobserved {
                        detail: format!("{head} · {removed}"),
                        stopped: true,
                        logs_complete: true,
                        container: left,
                    });
                }
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    // ★ 결함 525 — 관측한 종료를 **지우기 전에** 정지 손잡이와 나눈다(소유자 정지가 멈춘 원인을 가를 때 컨테이너가 이미 없을 수 있다).
    *stopper
        .observed_exit
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some((exit.exit_code, exit.oom_killed));
    // ★ 결함 487 (재검수 123) — 로그를 못 받으면 반쯤 쓴 출력 파일을 지우고(확정이 READ_OUTPUTS 로 실패하게 — 빈 출력이 성공이 되지 않게),
    //   컨테이너를 **지우지 않는다**(런타임에 온전한 로그가 남는다). 종료 코드는 관측한 사실이라 그대로 돌려준다(정리 결과는 따로).
    match save_logs(program, target, stdout_path, stderr_path) {
        Ok(()) => {
            exit.logs_complete = true;
            let (left, removed) = remove_container_fact(program, target);
            exit.container = left;
            if left != ContainerLeft::Removed {
                eprintln!(
                    "CONTAINER_NOT_REMOVED name={} id={target} — {removed}",
                    input.name
                );
            }
            exit.note = format!("로그 받음 · {removed}");
        }
        Err(why) => {
            let discard = discard_partial_outputs(stdout_path, stderr_path);
            exit.logs_complete = false;
            exit.container = ContainerLeft::Kept;
            eprintln!(
                "CONTAINER_KEPT_FOR_LOGS name={} id={target} — 로그를 받지 못해 컨테이너를 남겼다: {why}{discard}",
                input.name
            );
            exit.note = format!("로그를 받지 못해 컨테이너를 남겼다({why}){discard}");
        }
    }
    Ok(exit)
}

/// 사건 표식 폴더 — 체크포인트 루트의 형제 `<루트>.container-incidents/`.
///
/// ★ 결함 523 (재검수 133) — 전에는 부모 아래 고정 이름 `container-incidents` 였다. 같은 부모를 쓰는 두 노드(`/srv/gputeer/a` · `/b`)가 한 폴더를
///   나눠 써, 한 노드의 표식이 다른 노드의 기동을 막고 다른 노드의 해제가 이 노드의 보호 표식을 지웠다. 루트 이름을 앞에 붙여 노드마다 따로 둔다.
pub fn incident_dir_for(checkpoint_root: &Path) -> PathBuf {
    checkpoint_root_sibling_path(checkpoint_root, ".container-incidents")
}

/// 남은 컨테이너에서 건진 로그 폴더 — 체크포인트 루트의 형제 `<루트>.leftover-container-logs/`(결함 523 — 노드마다 따로).
pub fn leftover_logs_dir_for(checkpoint_root: &Path) -> PathBuf {
    checkpoint_root_sibling_path(checkpoint_root, ".leftover-container-logs")
}

fn checkpoint_root_sibling_path(checkpoint_root: &Path, suffix: &str) -> PathBuf {
    let mut name = checkpoint_root
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    checkpoint_root.with_file_name(name)
}

/// 열린 사건 표식을 이름 순으로 돌려준다. 폴더가 없으면 없다.
///
/// ★ 결함 498 (재검수 125) — 확장자로 가르지 않는다. 폴더 안의 **모든** 항목을 열린 것으로 센다 — 쓰다 만 표식 · 모르는 파일도 사람이 본다
///   (전에는 `*.incident` 만 세어, 임시 파일을 쓰고 이름을 바꾸기 전에 죽으면 재기동이 열린 사건 0건으로 봤다).
pub fn open_incidents(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{dir:?} 를 읽지 못했다: {error}")),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("{dir:?} 를 읽지 못했다: {error}"))?
            .path();
        found.push(path);
    }
    found.sort();
    Ok(found)
}

/// 소유자의 **명시적** 해제 — 표식을 지운다(`name` 이 없으면 전부). 지운 표식을 돌려준다. 컨테이너 · 작업 폴더는 건드리지 않는다
/// (해제한 뒤 다음 기동이 남은 컨테이너의 로그를 건지고 지운다 · 결함 489).
pub fn clear_incidents(dir: &Path, name: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let mut cleared = Vec::new();
    for path in open_incidents(dir)? {
        let matches = name.is_none_or(|name| {
            path.file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| file.starts_with(&format!("{name}.")))
        });
        if matches {
            std::fs::remove_file(&path)
                .map_err(|error| format!("{path:?} 를 지우지 못했다: {error}"))?;
            cleared.push(path);
        }
    }
    Ok(cleared)
}

/// 이 컨테이너 이름으로 열린 사건 표식이 있는가 — **확인하지 못해도 있다고 본다**.
///
/// ★ 결함 499 (재검수 125) — 폴더를 읽지 못한 것을 "없다" 로 삼키면 작업 폴더를 지운다. 모르면 남긴다.
pub fn incident_recorded_for(dir: &Path, name: &str) -> bool {
    open_incidents(dir).map_or(true, |found| {
        found.iter().any(|path| {
            path.file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| file.starts_with(&format!("{name}.")))
        })
    })
}

/// 기동 관문 — 사건 표식 폴더에 **실제로 쓸 수 있는가**(새 파일을 만들고 지운다). 못 쓰면 사람에게 넘길 길이 없으니 시작하지 않는다(결함 497).
/// 시험 파일을 지우지 못하면 그 파일이 열린 사건으로 남는다(보수 쪽).
pub fn probe_incident_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("{dir:?} 를 만들지 못했다: {error}"))?;
    let probe = dir.join(format!("write-probe.{}.{}", std::process::id(), unix_ms()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|error| format!("{probe:?} 를 만들지 못했다: {error}"))?;
    std::io::Write::write_all(&mut file, b"probe\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("{probe:?} 에 쓰지 못했다: {error}"))?;
    drop(file);
    std::fs::remove_file(&probe).map_err(|error| format!("{probe:?} 를 지우지 못했다: {error}"))
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// ★ 2026-09-27 보수 규칙 — 사람이 봐야 하는 결과면 **영속 사건 표식**을 남긴다. 표식이 있는 동안 Agent 는 기동하지 않아(새 작업 거부) 남은
///   컨테이너 정리도 돌지 않는다 — 사람에게 넘긴 것을 재기동이 덮지 않는다. 해제는 소유자의 명시적 명령(`gputeer container-incidents --clear`)이다.
///   ★ 결함 497 (재검수 125) — 표식을 쓰지 못해도 **관측한 사실(종료 코드 · 로그 완결)은 그대로 돌려준다**(전에는 결과를 통째로 "멈춤 모름" 으로
///     바꿔 지웠다). 결과가 이미 "사람 필요" 라 작업 폴더는 남고(exec · lib 이 타입으로 받는다), `CONTAINER_INCIDENT_NOT_RECORDED` 를 본
///     agent-loop 가 다음 회차를 돌리지 않는다. 다시 띄우면 기동 관문이 폴더에 실제로 쓸 수 있는지부터 본다(`probe_incident_dir`).
fn record_incident_if_needed(
    runtime: &ContainerRuntime,
    name: &str,
    result: Result<ContainerExit, ContainerRunError>,
) -> Result<ContainerExit, ContainerRunError> {
    let (kind, detail, needs) = match &result {
        Ok(exit) => (
            "EXITED",
            format!(
                "종료 코드 {} · 로그 {} · 컨테이너 {:?} · {}",
                exit.exit_code,
                if exit.logs_complete {
                    "완결"
                } else {
                    "못 받음"
                },
                exit.container,
                exit.note
            ),
            exit.needs_human(),
        ),
        Err(error @ ContainerRunError::NotStarted { .. }) => (
            "NOT_STARTED",
            error.detail().to_string(),
            error.needs_human(),
        ),
        Err(error @ ContainerRunError::Unobserved { .. }) => (
            "UNOBSERVED",
            error.detail().to_string(),
            error.needs_human(),
        ),
    };
    let Some(dir) = runtime.incident_dir.as_ref().filter(|_| needs) else {
        return result;
    };
    match write_incident(dir, name, &runtime.node_id, kind, &detail) {
        Ok(path) => {
            eprintln!(
                "CONTAINER_INCIDENT_RECORDED name={name} file={} — 사람이 확인한 뒤 해제한다",
                path.display()
            );
            result
        }
        Err(why) => {
            eprintln!(
                "CONTAINER_INCIDENT_NOT_RECORDED name={name} kind={kind} — 사건 표식을 쓰지 못했다({why}) · {detail} — 사람이 확인하기 전까지 다음 회차를 돌리지 않는다"
            );
            result
        }
    }
}

/// 표식 한 건을 쓴다 — **최종 이름으로 바로** 새로 만든다(`create_new` — 있으면 번호를 올려 다시 · 덮지 않는다).
///
/// ★ 결함 498 · 500 (재검수 125) — 전에는 임시 파일에 쓰고 rename 했다. rename 전에 죽으면 관문이 못 봤고(498), `exists()` 뒤 밀리초 하나로
///   이름을 정해 같은 밀리초의 두 사건이 서로 덮었다(500). 이제 파일이 **생기는 순간** 열린 사건이다 — 쓰다 죽어 내용이 비어도 관문이 본다.
pub fn write_incident(
    dir: &Path,
    name: &str,
    node_id: &str,
    kind: &str,
    detail: &str,
) -> Result<PathBuf, String> {
    write_incident_at(dir, name, node_id, kind, detail, unix_ms())
}

/// `write_incident` 의 시각을 고정한 판 — 같은 밀리초의 충돌을 시험이 실제로 일으키게 한다(결함 500).
#[doc(hidden)]
pub fn write_incident_at(
    dir: &Path,
    name: &str,
    node_id: &str,
    kind: &str,
    detail: &str,
    now: u128,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("{dir:?} 를 만들지 못했다: {error}"))?;
    let one_line = detail.replace(['\n', '\r'], " ");
    let body = format!(
        "container={name}\nnode={node_id}\nkind={kind}\nrecorded_at_unix_ms={now}\ndetail={one_line}\n"
    );
    for seq in 0u32..1000 {
        let path = dir.join(format!("{name}.{now}.{seq}.incident"));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("{path:?} 를 만들지 못했다: {error}")),
        };
        // ★ 쓰기 · sync 가 실패해도 파일은 이미 생겼다 — 지우지 않는다(열린 사건으로 남아 사람이 본다).
        std::io::Write::write_all(&mut file, body.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("{path:?} 에 쓰지 못했다(파일은 남긴다): {error}"))?;
        sync_dir(dir)?;
        return Ok(path);
    }
    Err(format!(
        "{dir:?} 에 {name} 의 새 표식 이름을 찾지 못했다(1000번 겹침)"
    ))
}

/// 새 파일 이름을 디스크에 확정한다 — 리눅스는 폴더 fsync, Windows 는 폴더 핸들에 FlushFileBuffers(`gputeer_checkpoint::sync_dir`).
///
/// ★ 결함 563 — 전에는 "Windows 에는 폴더 fsync 가 없다" 며 Windows 에서 아무것도 하지 않았다. 틀린 근거였다(561 과 같다) — 전원이 끊기면 새 표식
///   이름이 사라져 재기동 관문이 열린 사건 0건으로 보고 새 작업을 받을 수 있었다.
fn sync_dir(dir: &Path) -> Result<(), String> {
    gputeer_checkpoint::sync_dir(dir)
        .map_err(|error| format!("{dir:?} 를 sync 하지 못했다: {error:?}"))
}

/// 컨테이너 상태 — 끝났으면 `Some(종료)`, 아직 돌면 `None`.
fn inspect_state(program: &Path, name: &str) -> Result<Option<ContainerExit>, String> {
    inspect_state_within(program, name, SHORT_TIMEOUT)
}

/// `inspect_state` 의 시한을 고른 판 — 소유자 정지의 확인 조회는 15초(결함 534 — 런북의 "확인 조회 15초 · 최대 약 45초" 와 맞춘다).
fn inspect_state_within(
    program: &Path,
    name: &str,
    timeout: Duration,
) -> Result<Option<ContainerExit>, String> {
    let output = cli_ok(
        program,
        &[
            "inspect".into(),
            "--format={{.State.Running}} {{.State.ExitCode}} {{.State.OOMKilled}} {{.State.StartedAt}}"
                .into(),
            name.into(),
        ],
        timeout,
    )?;
    parse_inspect_state(&output.stdout)
}

/// `inspect` 출력(`<running> <exit code> <oom> <started at>`)을 읽는다. 아직 돌고 있으면 `None`.
///
/// ★ 결함 501 (재검수 126) — 멈춰 있어도 **시작한 흔적(`StartedAt`)이 없으면** 종료로 읽지 않는다(Err — 관측 실패로 센다). 한 번도 시작하지 않은
///   컨테이너(`created`)도 `Running=false · ExitCode=0` 이라, 전에는 `start` 의 0 만 믿고 "종료 코드 0" 으로 확정했다. 시작하지 않은 시각은
///   docker `0001-01-01T00:00:00Z` · podman `0001-01-01 00:00:00 +0000 UTC` 다(시각 문자열은 공백을 가질 수 있어 네 번째부터 끝까지 합친다).
fn parse_inspect_state(text: &str) -> Result<Option<ContainerExit>, String> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    let [running, code, oom, started @ ..] = fields.as_slice() else {
        return Err(format!("inspect 출력을 읽지 못했다({text:?})"));
    };
    let started_at = started.join(" ");
    match *running {
        "true" => return Ok(None),
        "false" => {}
        other => return Err(format!("State.Running 을 읽지 못했다({other:?})")),
    }
    let exit_code = code
        .parse::<i64>()
        .map_err(|error| format!("ExitCode 를 읽지 못했다({code:?}): {error}"))?;
    let oom_killed = match *oom {
        "true" => true,
        "false" => false,
        other => return Err(format!("OOMKilled 를 읽지 못했다({other:?})")),
    };
    if started_at.is_empty() || started_at.starts_with("0001-01-01") {
        return Err(format!(
            "INSPECT_NEVER_STARTED: 멈춰 있지만 시작한 흔적이 없다(State.StartedAt={started_at:?}) — 종료로 읽지 않는다"
        ));
    }
    Ok(Some(ContainerExit {
        exit_code,
        oom_killed,
        // 아래 값은 `run` 이 로그 받기 · 정리 뒤에 채운다.
        logs_complete: false,
        container: ContainerLeft::Kept,
        note: String::new(),
    }))
}

/// 로그 받기가 실패했을 때 반쯤 쓴 출력 파일을 지운다 — 남겨 두면 확정이 빈 · 일부 출력을 정상 산출물로 읽는다(결함 487).
///
/// ★ 결함 495 (재검수 124) — 전에는 삭제 실패를 버렸다(`let _`). 지우지 못한 부분 파일을 확정이 정상 산출물로 읽었다. 이제 실패를 문장으로
///   돌려주고(호출자가 결과 설명에 싣는다), **확정이 이 삭제에 기대지 않는다** — 로그를 못 받은 결과는 `logs_complete = false` 로 나가고
///   `exec` 가 그것을 "출력 불완전" 으로 실어 확정을 READ_OUTPUTS 로 실패시킨다. 이 함수의 삭제는 이제 두 번째 방어다.
///   반환값은 비었거나(`""` — 지웠다 · 원래 없었다) " · 부분 파일을 지우지 못했다(…)" 다.
fn discard_partial_outputs(stdout_path: Option<&Path>, stderr_path: Option<&Path>) -> String {
    let mut failed = Vec::new();
    for path in [stdout_path, stderr_path].into_iter().flatten() {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failed.push(format!("{path:?}: {error}")),
        }
    }
    if failed.is_empty() {
        String::new()
    } else {
        format!(" · 부분 파일을 지우지 못했다({})", failed.join(" · "))
    }
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
                // ★ 결함 547 — 거두기는 뒤에서(D 상태 프로세스를 기다리지 않는다).
                reap_in_background(child);
                return Err("logs 가 시한 안에 끝나지 않았다".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("logs 를 기다리지 못했다: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    /// 결함 462 — cdi-all 은 컨테이너를 만들기 직전에도 한 장인지 본다. 그 밖에는 NVML 을 보지 않는다.
    #[test]
    fn cdi_all_is_rechecked_right_before_the_container_is_created() {
        let execution = |gpu_request, pass_gpu| super::ContainerExecution {
            runtime: super::ContainerRuntime {
                program: std::path::PathBuf::from("podman"),
                flavor: super::RuntimeFlavor::Podman,
                pass_gpu,
                gpu_request,
                only: true,
                node_id: "node".into(),
                owner: String::new(),
                incident_dir: None,
            },
            pinned_image: "img@sha256:00".into(),
            gpu_pin: Some("0".into()),
        };
        let all = execution(super::GpuRequest::CdiAll, true);
        assert_eq!(super::cdi_all_ready(&all, || Ok(1)), Ok(()));
        for count in [Ok(2), Ok(0), Err("NVML 없음".to_string())] {
            assert!(super::cdi_all_ready(&all, || count)
                .unwrap_err()
                .starts_with("CONTAINER_GPU_ALL_NOT_PINNED"));
        }
        for other in [
            execution(super::GpuRequest::Cdi, true),
            execution(super::GpuRequest::CdiAll, false),
        ] {
            assert_eq!(
                super::cdi_all_ready(&other, || panic!("NVML 을 봤다")),
                Ok(())
            );
        }
    }

    use super::*;

    fn runtime(flavor: RuntimeFlavor) -> ContainerRuntime {
        ContainerRuntime {
            program: PathBuf::from("podman"),
            flavor,
            pass_gpu: false,
            gpu_request: GpuRequest::default_for(flavor),
            only: false,
            node_id: "node-a".into(),
            owner: "node-a.0123456789abcdef".into(),
            incident_dir: None,
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

    fn mounts(run: &Path) -> Vec<Mount> {
        vec![
            Mount {
                host: run.join("checkpoints-out"),
                target: CONTAINER_CHECKPOINT_DIR,
                read_only: false,
            },
            Mount {
                host: run.join("resume-in"),
                target: CONTAINER_RESUME_DIR,
                read_only: true,
            },
        ]
    }

    fn input<'a>(
        mounts: &'a [Mount],
        env: &'a [(OsString, OsString)],
        args: &'a [String],
    ) -> CreateInput<'a> {
        CreateInput {
            name: "gputeer-x",
            entrypoint: "python",
            args,
            environment: env,
            mounts,
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
        let run = PathBuf::from("/var/gputeer/run-1");
        let mounts = mounts(&run);
        let env = vec![
            (
                OsString::from("GPUTEER_CHECKPOINT_DIR"),
                OsString::from("/var/gputeer/run-1/checkpoints-out"),
            ),
            (
                OsString::from("GPUTEER_RESUME_DIR"),
                OsString::from("/var/gputeer/run-1/resume-in"),
            ),
            (OsString::from("GPUTEER_JOB_ID"), OsString::from("job-1")),
        ];
        let job_args = vec!["train.py".to_string(), "--epochs=3".to_string()];
        let args = strings(create_args(&execution, &input(&mounts, &env, &job_args)).unwrap());
        for flag in [
            "--read-only",
            "--tmpfs=/tmp",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--network=none",
            "--ipc=private",
            "--pids-limit=4096",
            "--memory=268435456",
            "--memory-swap=268435456",
            "--workdir=/tmp",
            "--user=1000:1000",
            "--env=GPUTEER_CHECKPOINT_DIR=/gputeer/checkpoints",
            "--env=GPUTEER_RESUME_DIR=/gputeer/resume",
            "--env=GPUTEER_JOB_ID=job-1",
            "--entrypoint=python",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag} 가 없다: {args:?}");
        }
        // 경로 구분자는 플랫폼마다 다르다 — 기대값도 같은 Path 로 만든다.
        for expected in [
            format!(
                "--mount=type=bind,source={},target=/gputeer/checkpoints",
                run.join("checkpoints-out").display()
            ),
            format!(
                "--mount=type=bind,source={},target=/gputeer/resume,readonly",
                run.join("resume-in").display()
            ),
        ] {
            assert!(args.contains(&expected), "{expected} 가 없다: {args:?}");
        }
        // ★ 결함 273 — 로그를 받는 작업 폴더 자체는 붙이지 않는다.
        assert!(
            !args
                .iter()
                .any(|a| a.starts_with("--mount=") && a.contains("source=/var/gputeer/run-1,")),
            "작업 폴더 전체를 붙였다: {args:?}"
        );
        // 이미지 뒤에는 작업 인자만 온다 — 제출자 값이 옵션으로 읽히지 않는다.
        let image_at = args.iter().position(|a| a == "img@sha256:00").unwrap();
        assert_eq!(&args[image_at + 1..], ["train.py", "--epochs=3"]);
        assert!(args[..image_at]
            .iter()
            .all(|a| a == "create" || a.starts_with("--")));
        // docker 는 podman 전용 옵션을 받지 않는다.
        assert!(!args.iter().any(|a| a == "--image-volume=ignore"));
    }

    #[test]
    fn podman_keeps_the_host_uid_ignores_image_volumes_and_gpu_flags_follow_the_runtime() {
        let mut execution = ContainerExecution {
            runtime: runtime(RuntimeFlavor::Podman),
            pinned_image: "img@sha256:00".into(),
            gpu_pin: Some("GPU-1".into()),
        };
        let mounts = mounts(Path::new("/w"));
        let podman = strings(create_args(&execution, &input(&mounts, &[], &[])).unwrap());
        assert!(podman.iter().any(|a| a == "--userns=keep-id"));
        assert!(!podman.iter().any(|a| a.starts_with("--user=")));
        assert!(podman.iter().any(|a| a == "--image-volume=ignore"));
        assert!(podman.iter().any(|a| a == "--read-only-tmpfs=false"));
        assert!(podman.iter().any(|a| a == "--device=nvidia.com/gpu=GPU-1"));
        execution.runtime.flavor = RuntimeFlavor::Docker;
        execution.runtime.gpu_request = GpuRequest::default_for(RuntimeFlavor::Docker);
        let docker = strings(create_args(&execution, &input(&mounts, &[], &[])).unwrap());
        let gpus = docker.iter().position(|a| a == "--gpus").unwrap();
        assert_eq!(docker[gpus + 1], "\"device=GPU-1\"");
    }

    #[test]
    fn several_pinned_gpus_become_one_device_request_per_runtime_syntax() {
        // 결함 300 — "0,1" 을 한 값으로 넘기면 podman 은 없는 CDI 이름을, docker 는 CSV 두 필드(device=0 · 1)를 본다.
        let podman = strings(gpu_args(GpuRequest::Cdi, "0,1"));
        assert_eq!(
            podman,
            ["--device=nvidia.com/gpu=0", "--device=nvidia.com/gpu=1"]
        );
        let docker = strings(gpu_args(GpuRequest::Gpus, "0,1"));
        assert_eq!(docker, ["--gpus", "\"device=0,1\""]);
        // 한 장은 전과 같은 뜻이다.
        assert_eq!(
            strings(gpu_args(GpuRequest::Cdi, "0")),
            ["--device=nvidia.com/gpu=0"]
        );
        assert_eq!(
            strings(gpu_args(GpuRequest::Gpus, "0")),
            ["--gpus", "\"device=0\""]
        );
    }

    #[test]
    fn cdi_all_passes_every_gpu_so_only_a_single_pinned_gpu_may_use_it() {
        // 결함 303 — WSL2 의 CDI 사양은 `all` 하나다(x600 실측). 모양은 핀과 무관하게 all 이고,
        // 그래서 `0` 한 장 고정이 아니면 decide 가 받지 않는다.
        assert_eq!(
            strings(gpu_args(GpuRequest::CdiAll, "0")),
            ["--device=nvidia.com/gpu=all"]
        );
        // docker 도 cdi 를 고르면 장치마다 CDI 이름이다(런타임 종류가 아니라 고른 방식이 모양을 정한다).
        assert_eq!(
            strings(gpu_args(GpuRequest::Cdi, "1")),
            ["--device=nvidia.com/gpu=1"]
        );
        let mut runtime = runtime(RuntimeFlavor::Docker);
        runtime.pass_gpu = true;
        runtime.gpu_request = GpuRequest::CdiAll;
        let manifest = oci_manifest("registry.local/train", sha256(0xcd));
        assert!(matches!(
            decide(&manifest, Some(&runtime), Some("0")),
            ContainerDecision::Container(_)
        ));
        for pin in ["1", "0,1"] {
            match decide(&manifest, Some(&runtime), Some(pin)) {
                ContainerDecision::Refused { detail } => {
                    assert!(
                        detail.starts_with("CONTAINER_GPU_ALL_NOT_PINNED"),
                        "{detail}"
                    )
                }
                other => panic!("{pin} 고정인데 cdi-all 을 받았다: {other:?}"),
            }
        }
        assert_eq!(GpuRequest::parse("cdi-all"), Ok(GpuRequest::CdiAll));
        assert!(GpuRequest::parse("all").is_err());
    }

    #[test]
    fn a_mount_path_that_could_rewrite_the_mount_is_refused() {
        let execution = ContainerExecution {
            runtime: runtime(RuntimeFlavor::Docker),
            pinned_image: "img@sha256:00".into(),
            gpu_pin: None,
        };
        let bad = vec![Mount {
            host: PathBuf::from("/w,readonly=false"),
            target: CONTAINER_CHECKPOINT_DIR,
            read_only: true,
        }];
        assert!(create_args(&execution, &input(&bad, &[], &[])).is_err());
        let good = mounts(Path::new("/w"));
        let mut zero = input(&good, &[], &[]);
        zero.memory_limit_bytes = 0;
        assert!(create_args(&execution, &zero).is_err());
    }

    /// 결함 548 (재검수 143) — 상한을 넘는 출력은 앞부분만 담고 "잘렸다" 를 알린다(그 출력을 완전한 것으로 쓰지 않게).
    #[test]
    fn a_cli_output_over_the_cap_is_marked_as_cut() {
        let big = vec![b'x'; MAX_CLI_OUTPUT_BYTES + 1];
        let (kept, cut) = drain_capped(&mut std::io::Cursor::new(big)).unwrap();
        assert!(cut, "잘렸는데 알리지 않았다");
        assert_eq!(kept.len(), MAX_CLI_OUTPUT_BYTES);
        let (kept, cut) =
            drain_capped(&mut std::io::Cursor::new(b"id-1\nid-2\n".to_vec())).unwrap();
        assert!(!cut);
        assert_eq!(kept, b"id-1\nid-2\n");
    }

    /// 결함 550 (재검수 144) — 읽기 오류는 EOF 가 아니다 — 앞부분만 읽고 오류가 나면 그 오류를 돌려준다(부분 출력을 완전한 것으로 넘기지 않는다).
    #[test]
    fn a_pipe_read_error_is_not_an_end_of_output() {
        struct BreaksAfterOne(bool);
        impl std::io::Read for BreaksAfterOne {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0 {
                    return Err(std::io::Error::other("파이프가 깨졌다"));
                }
                self.0 = true;
                buf[..2].copy_from_slice(b"A\n");
                Ok(2)
            }
        }
        assert!(drain_capped(&mut BreaksAfterOne(false)).is_err());
    }

    #[test]
    fn inspect_output_distinguishes_running_exit_and_oom() {
        const STARTED: &str = "2026-09-28T00:00:00.123456789Z";
        assert_eq!(
            parse_inspect_state(&format!("false 137 true {STARTED}\n")).unwrap(),
            Some(ContainerExit {
                exit_code: 137,
                oom_killed: true,
                logs_complete: false,
                container: ContainerLeft::Kept,
                note: String::new(),
            })
        );
        assert_eq!(
            parse_inspect_state(&format!("false 0 false {STARTED}"))
                .unwrap()
                .map(|e| e.exit_code),
            Some(0)
        );
        // podman 의 시각은 공백을 가진다.
        assert_eq!(
            parse_inspect_state("false 3 false 2026-09-28 00:00:00.1 +0000 UTC")
                .unwrap()
                .map(|e| e.exit_code),
            Some(3)
        );
        assert_eq!(
            parse_inspect_state(&format!("true 0 false {STARTED}")).unwrap(),
            None
        );
        assert!(parse_inspect_state("").is_err());
        assert!(parse_inspect_state(&format!("false x false {STARTED}")).is_err());
        assert!(parse_inspect_state(&format!("maybe 0 false {STARTED}")).is_err());
    }

    /// 결함 501 (재검수 126) — 한 번도 시작하지 않은 컨테이너(`created`)는 `Running=false · ExitCode=0` 이어도 **종료가 아니다**.
    #[test]
    fn a_never_started_container_is_not_an_exit() {
        for never in [
            "false 0 false 0001-01-01T00:00:00Z",
            "false 0 false 0001-01-01 00:00:00 +0000 UTC",
            "false 0 false",
        ] {
            let error = parse_inspect_state(never).expect_err(never);
            assert!(error.contains("INSPECT_NEVER_STARTED"), "{never}: {error}");
        }
    }

    #[test]
    fn container_names_differ_per_attempt() {
        assert_ne!(derive_container_name("a1"), derive_container_name("a2"));
        // 결함 290 — 같은 시도는 연결(grant)이 바뀌어도 같은 이름이다.
        assert_eq!(derive_container_name("a1"), derive_container_name("a1"));
        assert!(derive_container_name("a").starts_with("gputeer-"));
    }
}
