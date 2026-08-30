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
//! # ★ 협조하는 작업에는 상한이고, 적대적인 코드에는 아니다
//!
//! 2026-08-30 독립 검수가 짚은 것을 그대로 적는다. 자식은 부모와 **같은
//! 권한**으로 돈다. `exec` 전에 제한 cgroup 에 넣지만, 실행이 시작된 뒤
//! 자기 pid 를 상위 cgroup 의 `cgroup.procs` 에 써서 **스스로 빠져나갈
//! 수 있다.**
//!
//! ```text
//! 막는다        실수로 메모리를 너무 먹는 작업
//!               상한을 몰라서 넘기는 작업
//!               소유자가 즉시 끝내야 하는 작업(cgroup.kill)
//! 못 막는다     빠져나가려고 작정한 코드
//! ```
//!
//! 이걸 닫으려면 cgroup namespace(`CLONE_NEWCGROUP`)로 상위를 안 보이게
//! 하고 권한을 낮춰야 하는데, 둘 다 이 조각보다 크다. `CLAUDE.md` §0.4 가
//! "강제할 수 없는 것을 보장으로 선언하지 않는다" 고 했으므로, 여기서
//! **선언하지 않는다** — `runtime-windows` 가 "S1 은 호스트를 지키지
//! 못한다" 고 적어 둔 것과 같은 자리다.
//!
//! # 이 크레이트가 보장하지 않는 것
//!
//! ```text
//! 탈출 방지        위 참조. 자식이 상위 cgroup.procs 에 자기를 쓰면 나간다
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
    /// 테스트 전용 — 아무것도 안 멈추는 손잡이.
    ///
    /// ★ `stop()` 이 **성공을 돌려주지 않는다.** 멈춘 척하면 그걸 쓰는
    ///   테스트가 "멈췄다" 를 통과시켜 공허해진다. Windows 쪽
    ///   `JobStopper::inert_for_test()` 와 같은 규칙이다.
    pub fn inert_for_test() -> Self {
        Self {
            cgroup: PathBuf::from("/nonexistent/gputeer-inert-for-test"),
        }
    }

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
    parent: &CgroupParent,
) -> Result<ConstrainedChild, CgroupError> {
    if memory_limit_bytes == 0 {
        return Err(CgroupError::LimitNotApplied {
            detail: "memory_limit_bytes 가 0 이다".into(),
        });
    }
    let parent = resolve_parent(parent)?;
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
    let limit_path = cgroup.join("memory.max");
    if let Err(error) = std::fs::write(&limit_path, memory_limit_bytes.to_string()) {
        let _ = std::fs::remove_dir(&cgroup);
        return Err(CgroupError::LimitNotApplied {
            detail: format!("memory.max 쓰기 실패({limit_path:?}): {error}"),
        });
    }

    // ★ `memory.swap.max` 가 없으면 **넘어가지 않는다**(2026-08-30 독립
    //   검수 지적). 초안은 "없으면 새어나갈 곳도 없다" 고 **추정**했는데,
    //   파일 부재는 스왑 계정이 꺼졌다는 뜻일 뿐 스왑이 없다는 뜻이
    //   아니다. 계정이 꺼진 채 스왑이 켜져 있으면 상한이 조용히 샌다 —
    //   정확히 이 실측이 잡았던 결함으로 되돌아간다.
    //
    //   그래서 실제로 확인한다. 스왑이 진짜 없을 때만 넘어간다.
    let swap_path = cgroup.join("memory.swap.max");
    if swap_path.exists() {
        if let Err(error) = std::fs::write(&swap_path, "0") {
            let _ = std::fs::remove_dir(&cgroup);
            return Err(CgroupError::LimitNotApplied {
                detail: format!("memory.swap.max 쓰기 실패({swap_path:?}): {error}"),
            });
        }
    } else if system_has_swap() {
        let _ = std::fs::remove_dir(&cgroup);
        return Err(CgroupError::LimitNotApplied {
            detail: "memory.swap.max 가 없는데 스왑은 켜져 있다(/proc/swaps) —                      상한을 넘겨도 스왑으로 살아남으므로 실행하지 않는다"
                .into(),
        });
    }

    let mut command = std::process::Command::new(&spec.program);
    command.args(&spec.args);
    if let Some(dir) = &spec.current_dir {
        command.current_dir(dir);
    }
    // ★ 여기서 `?` 로 바로 나가면 이미 만든 cgroup 이 남는다
    //   (2026-08-30 독립 검수 지적). 남은 cgroup 은 같은 이름의 다음
    //   실행을 계속 실패시킨다.
    let mut open_outputs = || -> Result<(), CgroupError> {
        if let Some(path) = &spec.stdout_path {
            command.stdout(open_for_write(path)?);
        }
        if let Some(path) = &spec.stderr_path {
            command.stderr(open_for_write(path)?);
        }
        Ok(())
    };
    if let Err(error) = open_outputs() {
        let _ = std::fs::remove_dir(&cgroup);
        return Err(error);
    }

    // ★ fork 후 exec **전에** 자기를 cgroup 에 넣는다. 이 순서가
    //   "남의 코드가 상한 밖에서 도는 순간" 을 없앤다.
    let procs_path = cgroup.join("cgroup.procs");
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(move || {
            // ★ pre_exec 은 fork 와 exec 사이에서 돈다. 엄밀히는
            //   async-signal-safe 한 호출만 써야 하는데,
            //   `std::fs::write` 가 그것을 **보장하지는 않는다**
            //   (2026-08-30 독립 검수 지적 — 초안 주석은 보장한다고
            //   썼다. 틀린 서술이었다).
            //
            //   실제로 하는 일은 open/write/close 뿐이고 셋 다
            //   async-signal-safe 한 syscall 이지만, `std` 가 그 경로만
            //   쓴다는 계약은 없다. 할당이나 잠금이 끼면 다중 스레드
            //   프로세스에서 fork 뒤 교착할 수 있다.
            //
            //   지금 이것을 없애려면 raw syscall 을 직접 부르거나 libc
            //   의존을 추가해야 한다. 둘 다 이 조각보다 크므로, 위험을
            //   숨기지 않고 여기 적어 둔다.
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

/// 이 cgroup 이 **비어 있음이 증명되는가.**
///
/// ★ "확인 못 했다" 를 "비었다" 로 접지 않는다. 읽기에 실패하면
///   `Err` 다 — 모르는 상태에서 남의 작업을 지우면 되돌릴 수 없다.
fn cgroup_is_provably_empty(dir: &Path) -> Result<bool, String> {
    let procs = std::fs::read_to_string(dir.join("cgroup.procs"))
        .map_err(|error| format!("cgroup.procs 읽기 실패: {error}"))?;
    if procs.lines().any(|line| !line.trim().is_empty()) {
        return Ok(false);
    }
    // `cgroup.events` 는 `populated 0` / `populated 1` 줄을 낸다.
    // 하위 트리까지 통틀어 아무도 없을 때만 0 이다.
    let events = std::fs::read_to_string(dir.join("cgroup.events"))
        .map_err(|error| format!("cgroup.events 읽기 실패: {error}"))?;
    let populated = events
        .lines()
        .find_map(|line| line.strip_prefix("populated "))
        .ok_or_else(|| "cgroup.events 에 populated 줄이 없다".to_string())?;
    Ok(populated.trim() == "0")
}

/// 이 시스템에 스왑이 실제로 켜져 있는가.
///
/// `/proc/swaps` 는 헤더 한 줄 뒤에 활성 스왑 장치를 한 줄씩 낸다.
/// 읽지 못하면 **있다고 본다** — 모르면 안전한 쪽으로 기운다.
fn system_has_swap() -> bool {
    match std::fs::read_to_string("/proc/swaps") {
        Ok(text) => text.lines().skip(1).any(|line| !line.trim().is_empty()),
        Err(_) => true,
    }
}

fn open_for_write(path: &Path) -> Result<std::fs::File, CgroupError> {
    std::fs::File::create(path).map_err(|error| CgroupError::SpawnFailed {
        detail: format!("출력 파일 열기 실패({path:?}): {error}"),
    })
}

/// 하위 cgroup 을 어디에 만들 것인가.
///
/// ★ 2026-08-30 독립 검수가 초안의 자동 루트 폴백을 반려했다. 자동으로
///   v2 루트까지 내려가면 자식이 systemd unit·사용자 slice·컨테이너에
///   걸린 CPU·메모리·PID 상한 **밖**으로 나간다. 개별 `memory.max` 는
///   걸려도 운영자가 설정한 상위 총량 제한을 벗어나므로 안전한 폴백이
///   아니다 — 게다가 루트의 `subtree_control` 을 건드리는 것 자체가
///   자기 위임 범위를 넘는 시스템 전역 변경이다.
///
///   그래서 선택을 호출부에 넘긴다. 기본값은 거부다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CgroupParent {
    /// 이 프로세스가 속한 cgroup 아래. 위임받은 환경의 정상 경로다.
    Current,
    /// 운영자가 지정한 cgroup 아래. 위임받은 subtree 를 명시할 때 쓴다.
    Explicit(PathBuf),
    /// cgroup v2 루트 아래.
    ///
    /// ★ 상위 제한을 우회한다는 것을 **아는 채로** 고르는 값이다.
    ///   이름이 길고 불편한 것이 의도다 — 실수로 고를 수 없어야 한다.
    RootBypassingAncestorLimits,
}

impl Default for CgroupParent {
    fn default() -> Self {
        Self::Current
    }
}

/// 부모 후보를 실제 경로로 바꾸고 `memory` 위임을 확인한다.
///
/// # 왜 "내 cgroup" 이 항상 되지는 않는가
///
/// ★ 2026-08-30 x600 WSL 실측에서 이 코드가 설계대로 실행을 거부했다 —
///   `/proc/self/cgroup` 이 `/init.scope` 였다.
///
/// ```text
/// init.scope 의 subtree_control   (비어 있음)
/// -> 하위 디렉터리는 만들어지지만 memory.max 파일이 없다
/// ```
///
/// cgroup v2 의 **"내부 프로세스 금지"** 규칙 때문이다 — 프로세스를
/// 직접 담고 있는 cgroup 은 컨트롤러를 하위에 위임할 수 없다.
///
/// 그런 환경에서는 호출부가 `Explicit` 로 위임받은 subtree 를 주거나,
/// 위험을 감수하고 `RootBypassingAncestorLimits` 를 고른다. 자동으로
/// 내려가지 않는다.
fn resolve_parent(parent: &CgroupParent) -> Result<PathBuf, CgroupError> {
    let root = Path::new(CGROUP_ROOT);
    if !root.join("cgroup.controllers").exists() {
        return Err(CgroupError::NotAvailable {
            detail: format!("{CGROUP_ROOT}/cgroup.controllers 가 없다 — cgroup v2 가 아니다"),
        });
    }
    let candidate = match parent {
        CgroupParent::Current => current_cgroup_dir()?,
        CgroupParent::Explicit(path) => validate_explicit_parent(path, root)?,
        CgroupParent::RootBypassingAncestorLimits => root.to_path_buf(),
    };
    memory_is_delegated(&candidate).map_err(|reason| {
        CgroupError::MemoryControllerUnavailable {
            detail: format!("{candidate:?}: {reason}"),
        }
    })?;
    Ok(candidate)
}

/// 위임 디렉터리 이름이 반드시 시작해야 하는 접두사.
///
/// ★ 운영자가 **우리 몫이라고 이름으로 밝힌** 디렉터리만 받는다.
///   `system.slice` 를 실수로 가리킬 수 없게 하는 가장 확실한 방법이다 —
///   그 이름은 이 접두사로 시작하지 않는다.
pub const DELEGATED_PARENT_PREFIX: &str = "gputeer-";

/// 운영자가 지정한 부모가 **위임받은 subtree 로 쓸 수 있는 모양인가.**
///
/// # 왜 아무 경로나 받으면 안 되는가
///
/// ★ 2026-08-30 독립 검수 2라운드 지적. 1차 수정은 "루트 아래인가,
///   루트 자신은 아닌가" 만 봤는데, 그러면 아래가 **전부 통과했다.**
///
/// ```text
/// /sys/fs/cgroup/system.slice
/// /sys/fs/cgroup/user.slice
/// /sys/fs/cgroup/<다른 서비스의 subtree>
/// ```
///
/// 위험을 드러내려고 이름을 길게 만든 `RootBypassingAncestorLimits` 를
/// `Explicit` 가 사실상 우회하는 셈이었다.
///
/// # 무엇을 요구하는가
///
/// ```text
/// cgroup v2 루트의 **직속 자식**   조상 계층을 탈 수 없다
/// 이름이 `gputeer-` 로 시작        운영자가 우리 몫이라고 밝힌 것만
/// `..` 성분이 없다                 정규화 전에 탈출하는 경로를 막는다
/// 실제로 존재하는 디렉터리         없는 곳을 만들어 주지 않는다
/// ```
///
/// ★ **직속 자식** 조건이 핵심이다(2026-08-30 독립 검수 3라운드 지적).
///   1·2차 수정은 "루트 아래인가" 와 "마지막 이름이 `gputeer-` 인가" 만
///   봐서 아래가 그대로 통과했다.
///
/// ```text
/// /sys/fs/cgroup/system.slice/gputeer-agent.service
/// /sys/fs/cgroup/other-service/gputeer-workloads
/// ```
///
/// 막으려던 "다른 서비스 subtree 사용" 을 정확히 우회한 것이다. 조상을
/// 하나하나 금지하는 대신 **깊이를 1 로 고정한다** — 조상이 루트뿐이면
/// 탈 수 있는 계층 자체가 없다.
///
/// # 이 규칙이 배제하는 정상 배치 — 정직하게 적는다
///
/// ★ systemd 가 관리하는 위임 slice(`gputeer.slice` 아래 `.service`
///   cgroup)는 이 규칙을 통과하지 못한다. 그건 보통 루트의 직속 자식이
///   아니고 이름 규칙도 다르다. 지금은 **지원하지 않는다** — 지원하려면
///   위임 여부를 커널에 물어야 하는데(소유권·`cgroup.subtree_control`
///   위임 확인) 그건 이 조각보다 크다.
///
///   운영자는 `/sys/fs/cgroup/gputeer-workloads` 를 직접 만들어 쓴다.
///
/// # 막지 못하는 것
///
/// ★ **bind mount 는 못 막는다.** cgroup 루트를 루트 아래의 다른
///   이름에 bind mount 하면 겉보기 경로는 직속 자식인데 실제 대상은
///   루트일 수 있다. `canonicalize()` 로도 안 잡힌다 — mount 정보를
///   봐야 하고 그건 이 조각보다 크다.
///
///   symlink 는 `canonicalize()` 로 잡는다.
///
///   즉 이 함수는 **실수를 막는 것**이지 적대적인 운영자를 막는 것이
///   아니다(`CLAUDE.md` §0.4).
fn validate_explicit_parent(path: &Path, root: &Path) -> Result<PathBuf, CgroupError> {
    let refuse = |detail: String| CgroupError::NotDelegated { detail };

    if path.components().any(|c| c.as_os_str() == "..") {
        return Err(refuse(format!(
            "{path:?} 에 `..` 가 있다 — 정규화로 cgroup 루트 밖을 가리킬 수 있다"
        )));
    }
    if !path.is_dir() {
        return Err(refuse(format!(
            "{path:?} 가 디렉터리가 아니다 — 위임받은 subtree 는 이미 있어야 한다"
        )));
    }
    // symlink 를 따라간 **실제** 경로로 판정한다. 겉보기 경로로만 보면
    // 링크 하나로 검사를 통과할 수 있다.
    let resolved = path
        .canonicalize()
        .map_err(|error| refuse(format!("{path:?} 를 정규화하지 못했다: {error}")))?;

    if !resolved.starts_with(root) {
        return Err(refuse(format!(
            "{resolved:?} 가 {root:?} 아래가 아니다 — cgroup 이 아닌 경로다"
        )));
    }
    if resolved == root {
        return Err(refuse(format!(
            "{resolved:?} 는 cgroup v2 루트다 — 상위 제한을 우회하려면              RootBypassingAncestorLimits 를 명시적으로 골라야 한다"
        )));
    }
    // ★ 루트의 **직속 자식**인가. 조상 계층을 못 타게 하는 조건이다.
    let parent_of = resolved
        .parent()
        .ok_or_else(|| refuse(format!("{resolved:?} 의 상위를 읽지 못했다")))?;
    if parent_of != root {
        return Err(refuse(format!(
            "{resolved:?} 가 {root:?} 의 직속 자식이 아니다 — system.slice 같은 다른              서비스 계층 아래를 쓰지 못하게 깊이를 1 로 고정한다"
        )));
    }
    let name = resolved
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| refuse(format!("{resolved:?} 의 이름을 읽지 못했다")))?;
    if !name.starts_with(DELEGATED_PARENT_PREFIX) {
        return Err(refuse(format!(
            "{resolved:?} 의 이름이 `{DELEGATED_PARENT_PREFIX}` 로 시작하지 않는다 —              운영자가 이 Agent 몫으로 만든 디렉터리만 받는다"
        )));
    }
    Ok(resolved)
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
    if name.is_empty() || name.contains('/') || name.contains(' ') {
        return Err(CgroupError::NotDelegated {
            detail: format!("cgroup 이름으로 쓸 수 없다: {name:?}"),
        });
    }
    let dir = parent.join(format!("gputeer-{name}"));

    // ★ 이미 있으면 **죽이지 않는다**(2026-08-30 독립 검수 1라운드).
    //
    //   초안은 `cgroup.kill` 을 쓰고 지운 뒤 새로 만들었다. 이름이
    //   충돌하면 그건 "찌꺼기 청소" 가 아니라 **다른 사람의 작업을
    //   죽이는 것**이다. `CLAUDE.md` §0.1 은 소유자만이 자기 GPU 를
    //   비울 수 있다고 정했는데, 이 코드는 이름이 겹쳤다는 이유만으로
    //   남의 작업을 끝냈다.
    //
    // ★ 그렇다고 무조건 거부하면 다른 문제가 생긴다(같은 검수 2라운드).
    //   Agent 가 비정상 종료하거나 전원이 나가면 빈 cgroup 이 남고,
    //   이름은 결정적이므로 **같은 attempt 의 재시도가 영영 막힌다.**
    //   사람이 손으로 지울 때까지 그 작업은 다시 못 뜬다.
    //
    //   그래서 **비어 있음이 증명될 때만** 회수한다.
    //
    //   ```text
    //   cgroup.procs 가 비었다        직계 프로세스가 없다
    //   cgroup.events 가 populated 0  하위 트리까지 통틀어 아무도 없다
    //   ```
    //
    //   둘 다여야 한다. `cgroup.procs` 만 보면 손자가 남아 있는 트리를
    //   비었다고 오인한다. 하나라도 아니면 그건 남의 살아 있는 작업일
    //   수 있으므로 지우지 않고 멈춘다.
    if dir.exists() {
        match cgroup_is_provably_empty(&dir) {
            Ok(true) => {
                std::fs::remove_dir(&dir).map_err(|error| CgroupError::NotDelegated {
                    detail: format!("빈 cgroup 회수 실패({dir:?}): {error}"),
                })?;
            }
            Ok(false) => {
                return Err(CgroupError::NotDelegated {
                    detail: format!(
                        "{dir:?} 에 프로세스가 남아 있다 — 남의 작업일 수 있으므로 지우지 않는다"
                    ),
                });
            }
            Err(detail) => {
                return Err(CgroupError::NotDelegated {
                    detail: format!("{dir:?} 가 비었는지 확인하지 못했다: {detail}"),
                });
            }
        }
    }
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
        let error = match create_constrained_child(&spec, 0, "zero", &CgroupParent::Current) {
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

#[cfg(test)]
mod explicit_parent_tests {
    use super::*;

    /// ★ 검수가 든 실제 우회 경로를 막는가.
    ///
    /// 1차 수정은 `system.slice` 를 그대로 통과시켰다. 이름 조건이
    /// 그것을 막는지 확인한다.
    #[test]
    fn system_slices_are_refused_by_name() {
        let root = Path::new(CGROUP_ROOT);
        for dangerous in ["system.slice", "user.slice", "init.scope", "someservice"] {
            let error = validate_explicit_parent(&root.join(dangerous), root)
                .expect_err(&format!("{dangerous} 가 통과했다"));
            let message = error.to_string();
            assert!(
                message.contains("NOT_DELEGATED"),
                "{dangerous}: {message}"
            );
        }
    }

    /// ★ 조상 계층 아래에 숨은 것도 막는가(2026-08-30 검수 3라운드).
    ///
    /// 마지막 이름만 보면 `system.slice/gputeer-agent.service` 가 통과한다 —
    /// 막으려던 것을 정확히 우회하는 경로다.
    #[test]
    fn a_correct_name_under_a_system_ancestor_is_still_refused() {
        let root = Path::new(CGROUP_ROOT);
        for sneaky in [
            "system.slice/gputeer-agent.service",
            "user.slice/gputeer-workloads",
            "gputeer-a/gputeer-b",
        ] {
            let error = validate_explicit_parent(&root.join(sneaky), root)
                .expect_err(&format!("{sneaky} 가 통과했다"));
            assert!(
                error.to_string().contains("NOT_DELEGATED"),
                "{sneaky}: {error}"
            );
        }
    }

    /// 루트 자신과 밖은 여전히 막는가.
    #[test]
    fn the_root_and_outside_paths_are_refused() {
        let root = Path::new(CGROUP_ROOT);
        assert!(validate_explicit_parent(root, root).is_err(), "루트가 통과했다");
        assert!(
            validate_explicit_parent(Path::new("/etc"), root).is_err(),
            "cgroup 밖이 통과했다"
        );
        assert!(
            validate_explicit_parent(&root.join("gputeer-x/.."), root).is_err(),
            "`..` 가 통과했다"
        );
    }
}
