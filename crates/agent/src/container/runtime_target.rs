//! 런타임 대상 고정 — 증거 절차가 **언제나 같은 런타임에** 묻게 한다(실행 알림 계약 v18k §4 "런타임" · 계획 조각 5b).
//!
//! ```text
//! 연결 대상   create 때 실제로 쓴 엔드포인트를 해석한 값. 증거 절차의 모든 명령에 **명시 인자**로 준다 — 환경 변수 · context 기본값에 기대지 않는다
//!             docker         `-H <host>`(context inspect 가 돌려준 값 — DOCKER_HOST 를 반영한다)
//!             podman 로컬    `--root <graphRoot> --runroot <runRoot>` ★ v18l — 서비스 없이 CLI 가 엔진을 직접 도는 podman 에 `--url` 을 주면
//!                            서비스가 없는 노드에서 명령이 실패해 증거를 영영 못 얻는다. 저장소 위치가 로컬 podman 의 "대상" 이다
//!             podman 원격    자동 증거 대상이 아니다 — 해석이 거부된다(값이 없는 행은 자동 증거 없이 OPEN · 소유자 해제)
//! 대상 신원   docker — 데몬 ID(`info {{.ID}}`) · podman — 호스트 이름 + graphRoot + runRoot. 증거 절차 앞 · 뒤에 같은 명시 대상으로 두 번 읽어 원장 값과 대조
//! 조회        그 ID · 그 이름(`inspect --type container`) · owner 라벨 목록 안의 그 이름. not-found 만 "없음" 이다(목록에 나오면 상태가 무엇이든 있음 — v18g)
//! ```
//! ★ 한계(계약 그대로) — 같은 엔드포인트 뒤의 데몬이 다른 기계로 바뀌고 신원까지 같게 보이는 경우는 가르지 못한다.

use std::ffi::OsString;
use std::path::Path;

use super::{
    cli_ok, save_logs_with, says_no_such_container, sync_dir, unique_salvage_paths, RuntimeFlavor,
    CONFIRM_TIMEOUT, SHORT_TIMEOUT,
};

const DOCKER_PREFIX: &str = "docker-host:";
const PODMAN_LOCAL_PREFIX: &str = "podman-local:";

/// 증거 명령이 물을 런타임 — 원장의 `connection_target` 칸에 [`RuntimeEndpoint::to_ledger`] 로 적는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEndpoint {
    /// docker — `-H <host>`.
    DockerHost(String),
    /// podman 로컬 — `--root <graph_root> --runroot <run_root>`.
    PodmanLocal { graph_root: String, run_root: String },
}

impl RuntimeEndpoint {
    /// 모든 증거 명령 앞에 붙일 전역 인자.
    pub fn global_args(&self) -> Vec<OsString> {
        match self {
            RuntimeEndpoint::DockerHost(host) => vec!["-H".into(), host.into()],
            RuntimeEndpoint::PodmanLocal {
                graph_root,
                run_root,
            } => vec![
                "--root".into(),
                graph_root.into(),
                "--runroot".into(),
                run_root.into(),
            ],
        }
    }

    pub fn flavor(&self) -> RuntimeFlavor {
        match self {
            RuntimeEndpoint::DockerHost(_) => RuntimeFlavor::Docker,
            RuntimeEndpoint::PodmanLocal { .. } => RuntimeFlavor::Podman,
        }
    }

    /// 원장에 적는 글. podman 은 두 경로를 줄바꿈으로 가른다(경로에 줄바꿈이 있으면 해석이 거부한다).
    pub fn to_ledger(&self) -> String {
        match self {
            RuntimeEndpoint::DockerHost(host) => format!("{DOCKER_PREFIX}{host}"),
            RuntimeEndpoint::PodmanLocal {
                graph_root,
                run_root,
            } => format!("{PODMAN_LOCAL_PREFIX}{graph_root}\n{run_root}"),
        }
    }

    /// 원장의 글을 읽는다. 모르는 모양이면 거부한다(추측하지 않는다).
    pub fn from_ledger(text: &str) -> Result<Self, String> {
        if let Some(host) = text.strip_prefix(DOCKER_PREFIX) {
            return checked(host, "docker 엔드포인트").map(|h| RuntimeEndpoint::DockerHost(h.into()));
        }
        if let Some(rest) = text.strip_prefix(PODMAN_LOCAL_PREFIX) {
            let (graph_root, run_root) = rest
                .split_once('\n')
                .ok_or_else(|| "원장의 podman 대상에 runRoot 가 없다".to_string())?;
            return Ok(RuntimeEndpoint::PodmanLocal {
                graph_root: checked(graph_root, "podman graphRoot")?.into(),
                run_root: checked(run_root, "podman runRoot")?.into(),
            });
        }
        Err(format!("원장의 연결 대상 모양을 모른다({text:?})"))
    }
}

fn checked<'a>(value: &'a str, what: &str) -> Result<&'a str, String> {
    if value.trim().is_empty() || value != value.trim() || value.contains('\n') {
        return Err(format!("{what} 값이 비었거나 모양이 틀렸다({value:?})"));
    }
    Ok(value)
}

/// create **직전에** 이 Agent 가 쓰는 런타임의 대상을 해석한다. 해석하지 못하면 `Err` — 부르는 쪽은 원장에 대상을 적지 않는다(그 행은 자동 증거 없음).
pub fn resolve_endpoint(program: &Path, flavor: RuntimeFlavor) -> Result<RuntimeEndpoint, String> {
    match flavor {
        RuntimeFlavor::Docker => {
            let host = query(
                program,
                &["context", "inspect", "--format", "{{.Endpoints.docker.Host}}"],
            )?;
            checked(&host, "docker 엔드포인트")?;
            Ok(RuntimeEndpoint::DockerHost(host))
        }
        RuntimeFlavor::Podman => {
            let remote = query(program, &["info", "--format", "{{.Host.ServiceIsRemote}}"])?;
            if remote != "false" {
                return Err(format!(
                    "podman 이 원격 모드다(ServiceIsRemote={remote:?}) — 자동 증거의 대상으로 고정하지 않는다"
                ));
            }
            let store = query(program, &["info", "--format", "{{.Store.GraphRoot}}\n{{.Store.RunRoot}}"])?;
            let (graph_root, run_root) = store
                .split_once('\n')
                .ok_or_else(|| format!("podman info 가 graphRoot · runRoot 를 주지 않았다({store:?})"))?;
            Ok(RuntimeEndpoint::PodmanLocal {
                graph_root: checked(graph_root.trim_end_matches('\r'), "podman graphRoot")?.into(),
                run_root: checked(run_root.trim_end_matches('\r'), "podman runRoot")?.into(),
            })
        }
    }
}

/// 런타임 대상 신원 — **고정한 대상으로** 읽는다. docker `docker:<데몬 ID>` · podman `podman:<호스트 이름>|<graphRoot>|<runRoot>`.
pub fn read_identity(program: &Path, endpoint: &RuntimeEndpoint) -> Result<String, String> {
    match endpoint {
        RuntimeEndpoint::DockerHost(_) => {
            let id = pinned_query(program, endpoint, &["info", "--format", "{{.ID}}"])?;
            checked(&id, "docker 데몬 ID")?;
            Ok(format!("docker:{id}"))
        }
        RuntimeEndpoint::PodmanLocal { .. } => {
            let text = pinned_query(
                program,
                endpoint,
                &["info", "--format", "{{.Host.Hostname}}\n{{.Store.GraphRoot}}\n{{.Store.RunRoot}}"],
            )?;
            let parts: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
            if parts.len() != 3 || parts.iter().any(|p| p.trim().is_empty() || p.contains('|')) {
                return Err(format!("podman info 의 신원 값 모양이 틀렸다({text:?})"));
            }
            Ok(format!("podman:{}|{}|{}", parts[0], parts[1], parts[2]))
        }
    }
}

/// 조회 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// 런타임이 정상 응답했고 그런 것이 **있다**(상태가 무엇이든).
    Present,
    /// 런타임이 not-found 로 답했다(목록 조회는 정상 응답에 그 이름이 없다).
    Absent,
}

/// 무엇으로 묻는가(계약 §4 ③ — 새 조회 셋).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<'a> {
    /// 확인한 컨테이너 ID.
    Id(&'a str),
    /// 그 시도의 컨테이너 이름.
    Name(&'a str),
    /// 이 Agent 의 owner 라벨이 붙은 것 가운데 그 이름.
    OwnerLabelWithName { owner: &'a str, name: &'a str },
}

/// 고정한 대상으로 **새로** 묻는다. 무응답 · 시한 · not-found 가 아닌 오류는 `Err`(증거 없음).
pub fn pinned_lookup(
    program: &Path,
    endpoint: &RuntimeEndpoint,
    lookup: &Lookup<'_>,
) -> Result<Presence, String> {
    match lookup {
        Lookup::Id(target) | Lookup::Name(target) => {
            if target.trim().is_empty() {
                return Err("조회 대상이 비었다".into());
            }
            let mut args = endpoint.global_args();
            args.extend(
                ["inspect", "--type", "container", "--format", "{{.Id}}"]
                    .iter()
                    .map(OsString::from),
            );
            args.push(OsString::from(*target));
            match cli_ok(program, &args, CONFIRM_TIMEOUT) {
                Ok(_) => Ok(Presence::Present),
                Err(why) if says_no_such_container(&why) => Ok(Presence::Absent),
                Err(why) => Err(why),
            }
        }
        Lookup::OwnerLabelWithName { owner, name } => {
            if owner.trim().is_empty() || name.trim().is_empty() {
                return Err("owner 라벨 · 이름이 비었다 — 빈 라벨로 \"0개\" 를 얻지 않는다".into());
            }
            let mut args = endpoint.global_args();
            args.extend(
                ["ps", "-a", "--format", "{{.Names}}", "--filter"]
                    .iter()
                    .map(OsString::from),
            );
            args.push(format!("label=gputeer.owner={owner}").into());
            let listed = cli_ok(program, &args, CONFIRM_TIMEOUT)?;
            let found = listed
                .stdout
                .lines()
                .flat_map(|line| line.split(','))
                .map(|n| n.trim().trim_start_matches('/'))
                .any(|n| n == *name);
            Ok(if found { Presence::Present } else { Presence::Absent })
        }
    }
}

fn query(program: &Path, args: &[&str]) -> Result<String, String> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    cli_ok(program, &args, CONFIRM_TIMEOUT).map(|out| out.stdout.trim().to_string())
}

fn pinned_query(program: &Path, endpoint: &RuntimeEndpoint, args: &[&str]) -> Result<String, String> {
    let mut all = endpoint.global_args();
    all.extend(args.iter().map(OsString::from));
    cli_ok(program, &all, CONFIRM_TIMEOUT).map(|out| out.stdout.trim().to_string())
}

/// ★ 2026-10-03 11:25 (조각 5d2 · 계약 §4 ① — 해제 명령만) — 그 컨테이너의 로그를 `dir` 에 건지고 **파일 · 폴더를 sync** 한다(고정 대상으로).
///   건지지 못하면 `Err` — 부르는 쪽은 지우지 않는다(증거 없음). 돌려준 두 경로는 감사에 남긴다.
pub fn pinned_salvage_logs(
    program: &Path,
    endpoint: &RuntimeEndpoint,
    target: &str,
    dir: &Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("로그 보존 폴더를 만들지 못했다({dir:?}): {error}"))?;
    let (stdout_path, stderr_path) = unique_salvage_paths(dir, target);
    save_logs_with(
        program,
        &endpoint.global_args(),
        target,
        Some(&stdout_path),
        Some(&stderr_path),
    )?;
    for path in [&stdout_path, &stderr_path] {
        std::fs::File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("건진 로그를 sync 하지 못했다({path:?}): {error}"))?;
    }
    sync_dir(dir)?;
    Ok((stdout_path, stderr_path))
}

/// ★ 조각 5d2(계약 §4 ② — 해제 명령만) — 고정 대상에서 `rm -f -v <대상>`. 응답만으로 지웠다고 보지 않는다 — 뒤이은 새 조회 셋이 판정한다.
pub fn pinned_remove(program: &Path, endpoint: &RuntimeEndpoint, target: &str) -> Result<(), String> {
    let mut args = endpoint.global_args();
    args.extend(["rm", "-f", "-v"].iter().map(OsString::from));
    args.push(OsString::from(target));
    cli_ok(program, &args, SHORT_TIMEOUT).map(|_| ())
}
