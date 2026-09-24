//! `gputeer node-doctor` — 노드를 풀에 넣기 **전에** 그 기계가 준비됐는지 한 번에 점검한다.
//!
//! # 왜 필요한가
//!
//! 신뢰망 노드를 붙이려면 키 · 폴더 · Coordinator 연결 · 공유 저장소 · Owner Panel 포트 · GPU · 컨테이너 런타임이 전부
//! 맞아야 한다. 하나라도 어긋나면 Agent 는 **돌기 시작한 뒤에** 제각각의 오류로 멈춘다 — 무엇이 빠졌는지 한 줄씩 찾아야
//! 했다(런북 §5). 이 명령은 그것을 Agent 를 띄우기 전에 한 화면으로 보여 준다.
//!
//! # 판정
//!
//! ```text
//! OK    확인했다
//! WARN  돌기는 하지만 약하다(예: rootful docker · GPU 를 NVML 로 확인 못 함) — 사유를 읽고 정한다
//! FAIL  이대로 띄우면 Agent 가 실패한다 — 종료 코드가 0 이 아니다
//! ```
//!
//! ★ 이 점검은 **시작 조건**을 본다. 통과가 "작업이 잘 돈다" 를 뜻하지 않는다 — 컨테이너 GPU 넘기기는 "런타임에 nvidia 가 보였다"
//!   까지만 본다(실행 확인이 아니다). Coordinator 는 TCP 로 붙는지만 본다(서명 교환 없음 — 이름이 `coordinator_tcp` 다).
//! ★ 결함 282 (재검수 89) — 폴더를 **만들지 않는다**(없으면 FAIL). 쓰기 확인은 무작위 이름의 새 파일을 **배타 생성**(있으면 실패)해
//!   쓰고 곧바로 지운다 — 남의 파일 · 링크 대상을 건드리지 않는다. 지우지 못하면 FAIL 로 그 이름을 알린다.

use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Warn,
    Fail,
}

struct Check {
    name: &'static str,
    level: Level,
    detail: String,
}

fn check(name: &'static str, level: Level, detail: impl Into<String>) -> Check {
    Check {
        name,
        level,
        detail: detail.into(),
    }
}

struct Options {
    seed_file: Option<PathBuf>,
    node_dir: Option<PathBuf>,
    connect: Option<String>,
    shared_root: Option<PathBuf>,
    owner_panel_port: Option<u16>,
    gpu_pin: Option<String>,
    container_runtime: Option<PathBuf>,
    container_kind: Option<String>,
}

const USAGE: &str = "gputeer node-doctor --seed-file <node.seed> --node-dir <노드 폴더> --connect <coordinator 주소>:<포트> \
[--shared-checkpoint-root <공유 저장소>] [--owner-panel-port <포트>] [--gpu-pin <GPU 번호>] \
[--container-runtime <podman|docker 실행 파일> --container-runtime-kind podman|docker]";

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        seed_file: None,
        node_dir: None,
        connect: None,
        shared_root: None,
        owner_panel_port: None,
        gpu_pin: None,
        container_runtime: None,
        container_kind: None,
    };
    let mut iter = args.iter();
    while let Some(key) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| format!("NODE_DOCTOR_ARGS: {key} 에 값이 없다\n{USAGE}"))?;
        match key.as_str() {
            "--seed-file" => options.seed_file = Some(value.into()),
            "--node-dir" => options.node_dir = Some(value.into()),
            "--connect" => options.connect = Some(value.clone()),
            "--shared-checkpoint-root" => options.shared_root = Some(value.into()),
            "--owner-panel-port" => {
                options.owner_panel_port = Some(value.parse().map_err(|e| {
                    format!("NODE_DOCTOR_ARGS: --owner-panel-port 파싱 실패({value:?}): {e}")
                })?)
            }
            "--gpu-pin" => options.gpu_pin = Some(value.clone()),
            "--container-runtime" => options.container_runtime = Some(value.into()),
            "--container-runtime-kind" => options.container_kind = Some(value.clone()),
            other => return Err(format!("NODE_DOCTOR_ARGS: 모르는 옵션 {other}\n{USAGE}")),
        }
    }
    if options.seed_file.is_none() || options.node_dir.is_none() || options.connect.is_none() {
        return Err(format!(
            "NODE_DOCTOR_ARGS: --seed-file · --node-dir · --connect 는 반드시 준다\n{USAGE}"
        ));
    }
    if options.container_runtime.is_some() != options.container_kind.is_some() {
        return Err(
            "NODE_DOCTOR_ARGS: --container-runtime 과 --container-runtime-kind 는 함께 준다".into(),
        );
    }
    Ok(options)
}

/// 점검을 모두 돌린다. FAIL 이 하나라도 있으면 `Err`(보고 전체를 담아) — 종료 코드로 알 수 있게 한다.
pub fn run(args: &[String]) -> Result<String, String> {
    let options = parse(args)?;
    let mut checks = vec![
        check_seed(options.seed_file.as_deref().expect("parse")),
        check_node_dir(options.node_dir.as_deref().expect("parse")),
        check_coordinator(options.connect.as_deref().expect("parse")),
    ];
    if let Some(root) = options.shared_root.as_deref() {
        checks.push(check_shared_root(root));
    }
    if let Some(port) = options.owner_panel_port {
        checks.push(check_owner_panel_port(port));
    }
    checks.push(check_gpu(options.gpu_pin.as_deref()));
    if let (Some(program), Some(kind)) = (
        options.container_runtime.as_deref(),
        options.container_kind.as_deref(),
    ) {
        checks.extend(check_container_runtime(
            program,
            kind,
            options.gpu_pin.is_some(),
        ));
    }
    let mut report = String::new();
    for c in &checks {
        let level = match c.level {
            Level::Ok => "OK",
            Level::Warn => "WARN",
            Level::Fail => "FAIL",
        };
        report.push_str(&format!("CHECK {} {level} — {}\n", c.name, c.detail));
    }
    let count = |level| checks.iter().filter(|c| c.level == level).count();
    let (ok, warn, fail) = (count(Level::Ok), count(Level::Warn), count(Level::Fail));
    report.push_str(&format!("NODE_DOCTOR ok={ok} warn={warn} fail={fail}"));
    if fail > 0 {
        Err(report)
    } else {
        Ok(report)
    }
}

fn check_seed(path: &Path) -> Check {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            return check(
                "seed",
                Level::Fail,
                format!("{path:?} 를 읽지 못했다: {e} — `gputeer keygen --out <파일>` 로 만든다"),
            )
        }
    };
    let hex = text.trim();
    let bytes: Option<Vec<u8>> = (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| {
            (0..32)
                .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
                .collect::<Option<Vec<u8>>>()
        })
        .flatten();
    let Some(bytes) = bytes else {
        return check(
            "seed",
            Level::Fail,
            format!("{path:?} 가 32바이트 16진수 시드가 아니다"),
        );
    };
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    let public: String = gputeer_crypto::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.permissions().mode() & 0o077 != 0 {
                return check(
                    "seed",
                    Level::Warn,
                    format!(
                        "공개키 {public} · 그런데 다른 사용자가 읽을 수 있다(mode {:o}) — chmod 600",
                        meta.permissions().mode() & 0o777
                    ),
                );
            }
        }
    }
    check("seed", Level::Ok, format!("공개키 {public}"))
}

/// 폴더가 **있고** 쓸 수 있는지 — 새 파일을 배타 생성해 쓰고 지운다. 폴더를 만들지 않는다.
fn probe_writable(dir: &Path) -> Result<(), String> {
    use std::io::Write;
    let meta = std::fs::metadata(dir).map_err(|e| {
        format!("{dir:?} 가 없거나 읽을 수 없다: {e} — 먼저 만든다(점검은 만들지 않는다)")
    })?;
    if !meta.is_dir() {
        return Err(format!("{dir:?} 는 폴더가 아니다"));
    }
    let mut nonce = [0u8; 8];
    getrandom::getrandom(&mut nonce).map_err(|e| format!("무작위 이름을 만들지 못했다: {e}"))?;
    let name: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let probe = dir.join(format!(".gputeer-doctor-{name}"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| format!("{dir:?} 에 새 파일을 만들지 못했다: {e}"))?;
    let written = file.write_all(b"probe");
    drop(file);
    let removed = std::fs::remove_file(&probe);
    written.map_err(|e| format!("{dir:?} 에 쓰지 못했다: {e}"))?;
    removed.map_err(|e| format!("확인용 파일 {probe:?} 를 지우지 못했다(남았다): {e}"))
}

/// 노드 폴더 — 있고 쓸 수 있고, Agent 가 여는 하위 자리(`fence.sqlite3` · `checkpoints`)가 다른 종류로 막혀 있지 않은가(결함 283).
fn check_node_dir(dir: &Path) -> Check {
    if let Err(why) = probe_writable(dir) {
        return check("node_dir", Level::Fail, why);
    }
    for (child, want_dir) in [("fence.sqlite3", false), ("checkpoints", true)] {
        let path = dir.join(child);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return check(
                    "node_dir",
                    Level::Fail,
                    format!("{path:?} 를 읽지 못했다: {e}"),
                )
            }
            Ok(meta) if meta.file_type().is_symlink() => {
                return check(
                    "node_dir",
                    Level::Fail,
                    format!("{path:?} 가 링크다 — Agent 가 여는 자리는 링크가 아니어야 한다"),
                )
            }
            Ok(meta) if meta.is_dir() != want_dir => {
                return check(
                    "node_dir",
                    Level::Fail,
                    format!(
                        "{path:?} 가 {} 이어야 하는데 아니다 — Agent 가 열지 못한다",
                        if want_dir { "폴더" } else { "파일" }
                    ),
                )
            }
            Ok(_) => {}
        }
    }
    check(
        "node_dir",
        Level::Ok,
        format!("{dir:?} 에 쓸 수 있고 fence.sqlite3 · checkpoints 자리가 비었거나 맞는 종류다"),
    )
}

fn check_coordinator(address: &str) -> Check {
    let addrs: Vec<_> = match address.to_socket_addrs() {
        Ok(addrs) => addrs.collect(),
        Err(e) => {
            return check(
                "coordinator_tcp",
                Level::Fail,
                format!("{address} 를 주소로 풀지 못했다: {e}"),
            )
        }
    };
    let mut last = String::from("주소가 없다");
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, Duration::from_secs(3)) {
            // ★ 연결만 본다 — 인사(Hello)를 보내지 않는다. Coordinator 는 서명 없는 연결을 곧 닫는다.
            Ok(_) => {
                return check(
                    "coordinator_tcp",
                    Level::Ok,
                    format!(
                    "{address} 에 TCP 로 붙었다 — 서명 교환은 하지 않았다(키가 맞는지는 모른다)"
                ),
                )
            }
            Err(e) => last = format!("{addr}: {e}"),
        }
    }
    check(
        "coordinator_tcp",
        Level::Fail,
        format!("{address} 에 붙지 못했다 — {last} (Coordinator 가 떠 있는가 · 방화벽 · 포트)"),
    )
}

fn check_shared_root(root: &Path) -> Check {
    if !root.exists() {
        return check(
            "shared_checkpoint_root",
            Level::Fail,
            format!("{root:?} 가 없다 — 공유 저장소가 붙어 있는가"),
        );
    }
    if let Err(why) = probe_writable(root) {
        return check("shared_checkpoint_root", Level::Fail, why);
    }
    // ★ 루트 자체가 링크면 그 대상이 공유 저장소다 — 코드는 루트 아래 링크만 거부한다(런북 §6 · 결함 243).
    match std::fs::symlink_metadata(root) {
        Ok(meta) if meta.file_type().is_symlink() => check(
            "shared_checkpoint_root",
            Level::Warn,
            format!("{root:?} 는 링크다 — 그 대상이 실제 공유 저장소다(의도한 것인지 확인)"),
        ),
        _ => check(
            "shared_checkpoint_root",
            Level::Ok,
            format!("{root:?} 에 쓸 수 있다"),
        ),
    }
}

fn check_owner_panel_port(port: u16) -> Check {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => check(
            "owner_panel_port",
            Level::Ok,
            format!("127.0.0.1:{port} 가 비어 있다"),
        ),
        Err(e) => check(
            "owner_panel_port",
            Level::Fail,
            format!("127.0.0.1:{port} 를 열 수 없다(이미 쓰는 중?) — {e}"),
        ),
    }
}

fn check_gpu(pin: Option<&str>) -> Check {
    // ★ 결함 284 — Agent 는 --gpu-pin 에 **장치 번호**(쉼표로 여럿)만 받는다. UUID 를 OK 로 보여 주면 Agent 가 시작에서 거부한다.
    let pins: Option<Vec<&str>> = pin.map(|raw| raw.split(',').map(str::trim).collect());
    if let (Some(raw), Some(ids)) = (pin, pins.as_ref()) {
        if ids
            .iter()
            .any(|id| id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()))
        {
            return check(
                "gpu",
                Level::Fail,
                format!("--gpu-pin {raw:?} — Agent 는 장치 번호(예 \"0\" · \"0,1\")만 받는다(UUID 는 받지 않는다)"),
            );
        }
    }
    match gputeer_runtime_nvml::observe() {
        Ok(snapshot) => {
            let listed: Vec<String> = snapshot
                .gpus
                .iter()
                .map(|g| format!("{}:{}", g.index, g.name))
                .collect();
            match (pin, pins.as_ref()) {
                (Some(pin), Some(ids)) => {
                    let found = ids
                        .iter()
                        .all(|id| snapshot.gpus.iter().any(|g| g.index.to_string() == *id));
                    if found {
                        check(
                            "gpu",
                            Level::Ok,
                            format!("--gpu-pin {pin} 가 있다 · 전체 {listed:?}"),
                        )
                    } else {
                        check(
                            "gpu",
                            Level::Fail,
                            format!("--gpu-pin {pin} 인 GPU 가 없다 · 보이는 것 {listed:?}"),
                        )
                    }
                }
                _ => check(
                    "gpu",
                    Level::Ok,
                    format!("GPU {}개 {listed:?}", listed.len()),
                ),
            }
        }
        // ★ "없다" 가 아니라 "모른다" 다 — gpu-probe 와 같은 구분(`CLAUDE.md` §1).
        Err(e) => check(
            "gpu",
            if pin.is_some() {
                Level::Fail
            } else {
                Level::Warn
            },
            format!("NVML 로 GPU 를 확인하지 못했다(0개라는 뜻이 아니다) — {e:?}"),
        ),
    }
}

fn run_text(program: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program:?} 를 띄우지 못했다: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "{program:?} {} 실패({}): {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn check_container_runtime(program: &Path, kind: &str, wants_gpu: bool) -> Vec<Check> {
    let mut checks = Vec::new();
    if !matches!(kind, "podman" | "docker") {
        checks.push(check(
            "container_runtime",
            Level::Fail,
            format!("--container-runtime-kind 는 podman 또는 docker 다(받은 값 {kind:?})"),
        ));
        return checks;
    }
    match run_text(program, &["version", "--format", "{{.Client.Version}}"]) {
        Ok(version) => checks.push(check(
            "container_runtime",
            Level::Ok,
            format!("{kind} {version}"),
        )),
        Err(why) => {
            checks.push(check("container_runtime", Level::Fail, why));
            return checks;
        }
    }
    // rootless 인가 — docker(rootful)의 docker 그룹은 곧 root 다(런북 §5a).
    let rootless = match kind {
        "podman" => run_text(
            program,
            &["info", "--format", "{{.Host.Security.Rootless}}"],
        )
        .map(|text| text == "true"),
        _ => run_text(program, &["info", "--format", "{{json .SecurityOptions}}"])
            .map(|text| text.contains("rootless")),
    };
    checks.push(match rootless {
        Ok(true) => check("container_rootless", Level::Ok, "rootless 로 돈다"),
        Ok(false) => check(
            "container_rootless",
            Level::Warn,
            "rootful 이다 — 런타임 그룹에 든 계정은 사실상 root 다. rootless podman 을 권한다",
        ),
        Err(why) => check(
            "container_rootless",
            Level::Warn,
            format!("rootless 여부를 읽지 못했다 — {why}"),
        ),
    });
    if wants_gpu {
        // ★ "보였다" 까지다 — 컨테이너 안에서 GPU 를 실제로 연 것이 아니다(설계 문서 Out · 조각 2).
        let seen = match kind {
            "podman" => Ok(["/etc/cdi/nvidia.yaml", "/var/run/cdi/nvidia.yaml"]
                .iter()
                .any(|p| Path::new(p).exists())),
            _ => run_text(program, &["info", "--format", "{{json .Runtimes}}"])
                .map(|text| text.contains("nvidia")),
        };
        checks.push(match seen {
            Ok(true) => check(
                "container_gpu",
                Level::Ok,
                "NVIDIA 런타임/CDI 설정이 보였다(컨테이너 안에서 GPU 를 연 것은 아니다)",
            ),
            Ok(false) => check(
                "container_gpu",
                Level::Warn,
                "NVIDIA Container Toolkit(CDI · nvidia 런타임)이 보이지 않는다 — --container-gpu 로 GPU 를 넘길 수 없다",
            ),
            Err(why) => check("container_gpu", Level::Warn, why),
        });
    }
    checks
}
