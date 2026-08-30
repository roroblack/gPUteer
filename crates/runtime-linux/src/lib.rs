//! Linux cgroup v2 로 자원 상한을 **실제로 거는** 계층.
//!
//! # 왜 이 크레이트가 필요한가
//!
//! `crates/agent/src/exec.rs` 의 non-Windows 경로는 지금까지 이렇게
//! 말하고 실행을 거부했다.
//!
//! > 이 플랫폼에는 자원 상한 강제가 연결돼 있지 않다(Linux cgroup
//! > 미착수) — 상한 없이 실행하지 않는다.
//!
//! 그 수단을 만드는 것이 이 크레이트다. `crates/runtime-windows` 의
//! Job Object 경로와 같은 자리를 Linux 에서 채운다.
//!
//! # 순서가 핵심이다
//!
//! ```text
//! 1  하위 cgroup 을 만든다
//! 2  memory.max 를 건다
//! 3  자식을 fork 하고, exec **전에** 자기를 그 cgroup 에 넣는다
//! 4  exec 한다
//! ```
//!
//! ★ 3번과 4번의 순서를 바꾸면 안 된다. 남의 코드가 먼저 돌기
//!   시작하면 cgroup 에 들어가기 전에 이미 메모리를 커밋할 수 있다 —
//!   Windows 쪽이 `CREATE_SUSPENDED` 를 쓰는 것과 정확히 같은 이유다.
//!
//! `std::os::unix::process::CommandExt::pre_exec` 가 fork 후 exec 전에
//! 도는 훅을 준다. 그 안에서 자기 pid 를 `cgroup.procs` 에 쓴다.
//!
//! # 강제할 수 없으면 실행하지 않는다
//!
//! cgroup v2 가 없거나, 위임받은 쓰기 권한이 없거나, `memory` 컨트롤러가
//! 하위에 위임되지 않았으면 **typed error 로 실패한다.** 상한 없이
//! 띄우지 않는다 — 그게 이 크레이트의 존재 이유다(`CLAUDE.md` §0.4).
//!
//! # 이 크레이트가 보장하지 않는 것
//!
//! ```text
//! 호스트 보호      메모리 상한일 뿐이다. 임의 네이티브 코드로부터
//!                  호스트를 지키지 못한다(§0.4 — "S1 이상이면 안전"
//!                  이라 쓰지 않는다)
//! VRAM 상한        cgroup 은 시스템 RAM 만 제한한다. GPU 메모리는
//!                  이것으로 못 막는다 — 그래서 GPU 할당 기본값이
//!                  Exclusive 다
//! 네트워크 차단    방화벽은 이 크레이트 밖이다
//! 파일시스템 격리  mount namespace 는 미착수다
//! CPU·PID 상한     memory 만 건다. 나머지는 후속이다
//! ```

#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// cgroup 을 걸 수 없는 이유. **전부 "실행 안 함" 이다.**
#[derive(Debug)]
pub enum CgroupError {
    /// cgroup v2 파일시스템이 없다.
    NotAvailable { detail: String },
    /// 있지만 이 프로세스가 하위 cgroup 을 만들 수 없다.
    NotDelegated { detail: String },
    /// `memory` 컨트롤러가 하위에 위임되지 않았다.
    ///
    /// ★ 이걸 `NotDelegated` 와 합치지 않는다. 디렉터리는 만들 수
    ///   있는데 `memory.max` 파일이 없는 상황이 실제로 있다 — 부모의
    ///   `cgroup.subtree_control` 에 `memory` 가 없을 때다. 원인이
    ///   다르므로 메시지도 달라야 고치는 사람이 헤매지 않는다.
    MemoryControllerUnavailable { detail: String },
    /// 상한을 쓰지 못했다.
    LimitNotApplied { detail: String },
    /// 자식 기동 실패.
    SpawnFailed { detail: String },
    /// 종료를 관측하지 못했다.
    WaitFailed { detail: String },
}

impl std::fmt::Display for CgroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAvailable { detail } => write!(
                f,
                "CGROUP_UNAVAILABLE: cgroup v2 를 찾을 수 없다 — {detail}"
            ),
            Self::NotDelegated { detail } => write!(
                f,
                "CGROUP_NOT_DELEGATED: 하위 cgroup 을 만들 권한이 없다 — {detail}"
            ),
            Self::MemoryControllerUnavailable { detail } => write!(
                f,
                "CGROUP_NO_MEMORY_CONTROLLER: memory 컨트롤러가 하위에 위임되지 않았다 \
                 (부모의 cgroup.subtree_control 에 +memory 가 필요하다) — {detail}"
            ),
            Self::LimitNotApplied { detail } => write!(
                f,
                "CGROUP_LIMIT_NOT_APPLIED: 상한을 걸지 못해 실행하지 않았다 — {detail}"
            ),
            Self::SpawnFailed { detail } => write!(f, "CGROUP_SPAWN_FAILED: {detail}"),
            Self::WaitFailed { detail } => write!(f, "CGROUP_WAIT_FAILED: {detail}"),
        }
    }
}

impl std::error::Error for CgroupError {}

/// cgroup v2 마운트 지점.
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// 자식을 띄울 때 필요한 최소 입력. `runtime-windows` 의
/// `CreateProcessSpec` 과 같은 자리다.
pub struct SpawnSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    /// 자식의 표준 출력을 받을 파일. `None` 이면 안 받는다.
    ///
    /// ★ 파이프가 아니라 파일이다 — 파이프는 부모가 계속 빨아내야
    ///   하고, 안 빨아내는 사이 버퍼가 차면 자식이 쓰기에서 멈춘다.
    ///   `runtime-windows` 가 같은 이유로 파일을 쓴다.
    pub stdout_path: Option<PathBuf>,
    pub stderr_path: Option<PathBuf>,
}

/// 상한이 걸린 채 도는 자식.
pub struct ConstrainedChild {
    child: std::process::Child,
    cgroup: PathBuf,
}

/// 이 자식을 **다른 스레드에서** 끝낼 수 있는 손잡이.
///
/// Windows 쪽 `JobStopper` 와 같은 자리다. cgroup 은 `cgroup.kill` 로
/// 트리 전체를 한 번에 끝낼 수 있다 — 자식이 손자를 만들었어도 같이
/// 죽는다(`CLAUDE.md` §0.1 이 요구하는 "process tree 종료").
pub struct CgroupStopper {
    cgroup: PathBuf,
}

impl CgroupStopper {
    /// 이 cgroup 에 속한 **모든** 프로세스를 즉시 끝낸다.
    ///
    /// ★ `cgroup.kill` 은 Linux 5.14+ 다. 없으면 `cgroup.procs` 를 읽어
    ///   하나씩 SIGKILL 하는 대신 **실패로 보고한다** — 반쯤 죽이고
    ///   "비웠다" 고 말하지 않는다.
    pub fn stop(&self) -> Result<(), CgroupError> {
        let kill = self.cgroup.join("cgroup.kill");
        std::fs::write(&kill, "1").map_err(|error| CgroupError::LimitNotApplied {
            detail: format!("cgroup.kill 쓰기 실패({kill:?}): {error}"),
        })
    }
}

impl ConstrainedChild {
    /// 정지 손잡이를 만든다. `wait()` 으로 붙잡히기 **전에** 부른다.
    pub fn stopper(&self) -> CgroupStopper {
        CgroupStopper {
            cgroup: self.cgroup.clone(),
        }
    }

    /// 자식이 끝날 때까지 기다린다.
    pub fn wait(&mut self) -> Result<i32, CgroupError> {
        let status = self.child.wait().map_err(|e| CgroupError::WaitFailed {
            detail: e.to_string(),
        })?;
        // ★ 신호로 죽은 경우 `code()` 가 `None` 이다. 0 으로 접으면
        //   강제 종료가 "정상 완료" 로 기록된다.
        Ok(status.code().unwrap_or(-1))
    }

    /// 이 cgroup 이 관측한 메모리 최대치(바이트).
    ///
    /// `memory.peak` 이 없는 커널에서는 `None` 이다 — 0 으로 채우지
    /// 않는다(`CLAUDE.md` §1 — 모르면 비워 둔다).
    pub fn peak_memory_bytes(&self) -> Option<u64> {
        std::fs::read_to_string(self.cgroup.join("memory.peak"))
            .ok()
            .and_then(|text| text.trim().parse().ok())
    }

    /// 이 cgroup 에 실제로 걸린 상한(바이트).
    pub fn memory_limit_bytes(&self) -> Option<u64> {
        std::fs::read_to_string(self.cgroup.join("memory.max"))
            .ok()
            .and_then(|text| text.trim().parse().ok())
    }

    /// 이 cgroup 에 걸린 스왑 상한(바이트).
    ///
    /// `Some(0)` 이어야 상한이 실제로 강제된다 — 0 이 아니면 작업이
    /// 상한을 넘어도 스왑으로 밀려나 살아남는다.
    pub fn swap_limit_bytes(&self) -> Option<u64> {
        std::fs::read_to_string(self.cgroup.join("memory.swap.max"))
            .ok()
            .and_then(|text| text.trim().parse().ok())
    }

    /// 지금 이 cgroup 에 속한 프로세스 ID 들.
    ///
    /// 테스트가 "이 자식이 만든 프로세스" 를 정확히 가리키게 한다 —
    /// 시스템 전체를 세면 귀속이 엄밀하지 않다.
    pub fn process_ids(&self) -> Vec<u32> {
        std::fs::read_to_string(self.cgroup.join("cgroup.procs"))
            .map(|text| text.lines().filter_map(|l| l.trim().parse().ok()).collect())
            .unwrap_or_default()
    }
}

impl Drop for ConstrainedChild {
    /// 남은 프로세스를 끝내고 cgroup 을 지운다.
    ///
    /// ★ 안 지우면 실행마다 빈 cgroup 이 쌓인다. 그리고 프로세스가
    ///   남아 있으면 `rmdir` 이 `EBUSY` 로 실패하므로, 먼저 죽인다.
    fn drop(&mut self) {
        let _ = std::fs::write(self.cgroup.join("cgroup.kill"), "1");
        // 커널이 프로세스를 정리할 시간을 조금 준다. 그래도 남으면
        // rmdir 이 실패하는데, 그건 조용히 넘기지 않고 알린다.
        for _ in 0..50 {
            if std::fs::remove_dir(&self.cgroup).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        eprintln!(
            "gputeer-runtime-linux: cgroup 을 지우지 못했다({:?}) — 빈 cgroup 이 남는다",
            self.cgroup
        );
    }
}

/// 하위 cgroup 을 만들고 상한을 건 뒤 자식을 띄운다.
///
/// # 상한을 걸지 못하면 자식을 띄우지 않는다
///
/// 순서가 그것을 보장한다 — cgroup 생성과 `memory.max` 쓰기가 전부
/// 성공한 **뒤에야** `spawn()` 을 부른다.
pub fn create_constrained_child(
    spec: &SpawnSpec,
    memory_limit_bytes: u64,
    cgroup_name: &str,
) -> Result<ConstrainedChild, CgroupError> {
    if memory_limit_bytes == 0 {
        return Err(CgroupError::LimitNotApplied {
            detail: "memory_limit_bytes 가 0 이다".into(),
        });
    }
    let parent = resolve_parent()?;
    let cgroup = create_child_cgroup(&parent, cgroup_name)?;

    // 상한을 **먼저** 건다.
    //
    // ★ `memory.max` 와 `memory.swap.max` 를 **둘 다** 건다.
    //   2026-08-30 x600 WSL 실측에서 이 테스트가 실패해 알아냈다 —
    //   32MiB 상한에 90MB 를 할당했는데 자식이 **정상 종료했다.**
    //
    //   cgroup 은 `memory.max` 를 넘으면 먼저 **회수**를 시도하고,
    //   스왑이 있으면 페이지를 거기로 밀어낸다. 그래서 상한을 넘겨도
    //   안 죽는다 — 느려질 뿐이다. 남의 PC 에서 도는 작업이 스왑을
    //   무한정 먹으면 소유자의 기계가 기어간다. 그건 상한이 아니다.
    //
    //   `memory.swap.max = 0` 을 같이 걸어야 실제로 OOM 으로 끝난다.
    for (name, value) in [
        ("memory.max", memory_limit_bytes.to_string()),
        ("memory.swap.max", "0".to_string()),
    ] {
        let path = cgroup.join(name);
        // `memory.swap.max` 는 스왑 계정이 꺼진 커널에 없을 수 있다.
        // 그 경우엔 애초에 스왑으로 새어나갈 곳이 없으므로 넘어간다 —
        // 있는데 못 쓰는 것과 아예 없는 것은 다르다.
        if name == "memory.swap.max" && !path.exists() {
            continue;
        }
        if let Err(error) = std::fs::write(&path, &value) {
            let _ = std::fs::remove_dir(&cgroup);
            return Err(CgroupError::LimitNotApplied {
                detail: format!("{name} 쓰기 실패({path:?}): {error}"),
            });
        }
    }

    let mut command = std::process::Command::new(&spec.program);
    command.args(&spec.args);
    if let Some(dir) = &spec.current_dir {
        command.current_dir(dir);
    }
    if let Some(path) = &spec.stdout_path {
        command.stdout(open_for_write(path)?);
    }
    if let Some(path) = &spec.stderr_path {
        command.stderr(open_for_write(path)?);
    }

    // ★ fork 후 exec **전에** 자기를 cgroup 에 넣는다. 이 순서가
    //   "남의 코드가 상한 밖에서 도는 순간" 을 없앤다.
    let procs_path = cgroup.join("cgroup.procs");
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(move || {
            // pre_exec 은 fork 와 exec 사이에서 돈다 — async-signal-safe
            // 해야 한다. 파일 하나에 짧은 숫자를 쓰는 것뿐이다.
            std::fs::write(&procs_path, "0")?;
            Ok(())
        });
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_dir(&cgroup);
            return Err(CgroupError::SpawnFailed {
                detail: error.to_string(),
            });
        }
    };
    Ok(ConstrainedChild { child, cgroup })
}

fn open_for_write(path: &Path) -> Result<std::fs::File, CgroupError> {
    std::fs::File::create(path).map_err(|error| CgroupError::SpawnFailed {
        detail: format!("출력 파일 열기 실패({path:?}): {error}"),
    })
}

/// 하위 cgroup 을 만들 수 있는 부모를 고른다.
///
/// # 왜 "내 cgroup" 만으로는 안 되는가
///
/// ★ 2026-08-30 x600 WSL 실측에서 이 코드가 **설계대로 실행을
///   거부했다.** 초안은 무조건 이 프로세스의 cgroup 을 부모로 썼는데,
///   그게 `/init.scope` 였다.
///
/// ```text
/// /proc/self/cgroup          0::/init.scope
/// init.scope 의 subtree_control   (비어 있음)
/// -> 하위 디렉터리는 만들어지지만 memory.max 파일이 없다
/// ```
///
/// cgroup v2 의 **"내부 프로세스 금지"** 규칙 때문이다 — 프로세스를
/// 직접 담고 있는 cgroup 은 컨트롤러를 하위에 위임할 수 없다.
/// `init.scope` 에는 init 이 들어 있으므로 `+memory` 쓰기가 실패한다.
///
/// # 고르는 순서
///
/// ```text
/// 1  이 프로세스의 cgroup      위임받은 환경(컨테이너·systemd slice)
/// 2  cgroup v2 루트            우리가 root 이고 위임이 없는 환경
/// ```
///
/// 각 후보에 대해 `memory` 가 하위에 위임돼 있는지 보고, 없으면
/// **한 번 켜 보고** 다시 확인한다. 둘 다 안 되면 typed error 다 —
/// 상한 없이 실행하지 않는다.
///
/// ★ 2번으로 내려가면 자식이 이 프로세스의 cgroup **밖**에 놓인다.
///   상한 자체는 동일하게 걸리지만, 이 프로세스를 담은 상위 cgroup 의
///   회계에는 자식이 안 잡힌다. 그 차이를 아는 채로 쓰라고 여기 적는다.
fn resolve_parent() -> Result<PathBuf, CgroupError> {
    let root = Path::new(CGROUP_ROOT);
    if !root.join("cgroup.controllers").exists() {
        return Err(CgroupError::NotAvailable {
            detail: format!("{CGROUP_ROOT}/cgroup.controllers 가 없다 — cgroup v2 가 아니다"),
        });
    }
    let mut reasons = Vec::new();
    for candidate in [current_cgroup_dir()?, root.to_path_buf()] {
        match memory_is_delegated(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(reason) => reasons.push(format!("{candidate:?}: {reason}")),
        }
    }
    Err(CgroupError::MemoryControllerUnavailable {
        detail: reasons.join(" / "),
    })
}

/// 이 cgroup 이 하위에 `memory` 를 위임하는가. 아니면 켜 본다.
fn memory_is_delegated(dir: &Path) -> Result<(), String> {
    let control = dir.join("cgroup.subtree_control");
    let read = || std::fs::read_to_string(&control).unwrap_or_default();
    if read().split_whitespace().any(|c| c == "memory") {
        return Ok(());
    }
    // 아직 안 켜져 있다. 켤 수 있으면 켠다 — 실패는 정상적인 결과다
    // (프로세스를 직접 담은 cgroup 은 위임할 수 없다).
    if let Err(error) = std::fs::write(&control, "+memory") {
        return Err(format!("+memory 쓰기 실패: {error}"));
    }
    if read().split_whitespace().any(|c| c == "memory") {
        Ok(())
    } else {
        Err("+memory 를 썼는데도 subtree_control 에 안 나타난다".to_string())
    }
}

/// 이 프로세스가 속한 cgroup 디렉터리.
///
/// ★ `/proc/self/cgroup` 은 cgroup v2 에서 `0::<path>` 한 줄이다.
///   v1 이 섞인 시스템에서는 여러 줄이 나오는데, 그 경우 `0::` 줄이
///   없으면 v2 가 아니라고 판단한다 — v1 을 v2 인 척 다루면 상한이
///   조용히 안 걸린다.
fn current_cgroup_dir() -> Result<PathBuf, CgroupError> {
    let root = Path::new(CGROUP_ROOT);
    let text =
        std::fs::read_to_string("/proc/self/cgroup").map_err(|error| CgroupError::NotAvailable {
            detail: format!("/proc/self/cgroup 읽기 실패: {error}"),
        })?;
    let relative = text
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| CgroupError::NotAvailable {
            detail: "/proc/self/cgroup 에 v2 줄(0::)이 없다".into(),
        })?
        .trim()
        .to_string();
    Ok(if relative == "/" {
        root.to_path_buf()
    } else {
        root.join(relative.trim_start_matches('/'))
    })
}

/// 하위 cgroup 을 만든다.
///
/// ★ 이름 그대로 만들지 않고 `gputeer-` 접두사를 붙인다. 이 프로세스가
///   만든 것과 남이 만든 것을 사람이 구분할 수 있어야, 뭔가 남았을 때
///   누구 것인지 안다.
fn create_child_cgroup(parent: &Path, name: &str) -> Result<PathBuf, CgroupError> {
    if name.is_empty() || name.contains('/') || name.contains('\0') {
        return Err(CgroupError::NotDelegated {
            detail: format!("cgroup 이름으로 쓸 수 없다: {name:?}"),
        });
    }
    let dir = parent.join(format!("gputeer-{name}"));
    // 이미 있으면 이전 실행이 남긴 것이다. 재사용하지 않고 지운 뒤
    // 새로 만든다 — 남은 프로세스가 있으면 새 상한이 그것들에도
    // 적용돼 관측이 섞인다.
    let _ = std::fs::write(dir.join("cgroup.kill"), "1");
    let _ = std::fs::remove_dir(&dir);
    std::fs::create_dir(&dir).map_err(|error| CgroupError::NotDelegated {
        detail: format!("하위 cgroup 생성 실패({dir:?}): {error}"),
    })?;

    // ★ `memory.max` 가 실제로 있는지 확인한다. 디렉터리는 만들어지는데
    //   컨트롤러가 위임 안 된 경우가 실재한다(부모의 subtree_control 에
    //   `memory` 가 없을 때) — 그러면 상한을 못 건다.
    if !dir.join("memory.max").exists() {
        let _ = std::fs::remove_dir(&dir);
        return Err(CgroupError::MemoryControllerUnavailable {
            detail: format!(
                "{dir:?} 에 memory.max 가 없다 — {:?} 의 cgroup.subtree_control 을 확인하라",
                parent.join("cgroup.subtree_control")
            ),
        });
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 상한 0 은 거부한다.
    ///
    /// 0 을 그대로 쓰면 cgroup 이 "메모리를 전혀 못 쓴다" 가 되어 자식이
    /// 즉시 OOM 으로 죽는다 — 그건 상한이 아니라 실행 불가다.
    #[test]
    fn a_zero_limit_is_refused() {
        let spec = SpawnSpec {
            program: "/bin/true".into(),
            args: vec![],
            current_dir: None,
            stdout_path: None,
            stderr_path: None,
        };
        let error = match create_constrained_child(&spec, 0, "zero") {
            Err(error) => error,
            Ok(_) => panic!("상한 0 이 통과했다"),
        };
        assert!(
            error.to_string().contains("LIMIT_NOT_APPLIED"),
            "거부 사유가 식별되지 않는다: {error}"
        );
    }

    /// cgroup 이름 검사가 경로 탈출을 막는가.
    #[test]
    fn a_name_with_a_separator_is_refused() {
        for bad in ["", "a/b", "../escape"] {
            let error = match create_child_cgroup(Path::new(CGROUP_ROOT), bad) {
                Err(error) => error,
                Ok(dir) => panic!("{bad:?} 가 통과해 {dir:?} 를 만들었다"),
            };
            assert!(
                error.to_string().contains("NOT_DELEGATED"),
                "{bad:?}: {error}"
            );
        }
    }

    /// 오류 종류가 서로 구분되는가.
    ///
    /// ★ "디렉터리를 못 만든다" 와 "만들었는데 memory 컨트롤러가 없다" 는
    ///   원인이 다르다. 같은 문자열로 보고하면 고치는 사람이 헤맨다.
    #[test]
    fn error_kinds_are_distinguishable() {
        let a = CgroupError::NotDelegated { detail: "x".into() }.to_string();
        let b = CgroupError::MemoryControllerUnavailable { detail: "x".into() }.to_string();
        assert_ne!(a, b);
        assert!(a.starts_with("CGROUP_NOT_DELEGATED"));
        assert!(b.starts_with("CGROUP_NO_MEMORY_CONTROLLER"));
    }
}
