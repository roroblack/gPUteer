//! 검증된 실행 지시를 **실제 프로세스로 띄우는** 계층.
//!
//! # 이 모듈이 특별히 위험한 이유
//!
//! 이 저장소에서 **처음으로 남의 코드를 실제로 실행하는 곳**이다.
//! `CLAUDE.md` §0 이 "이 시스템은 남의 개인 PC 에서 코드를 돌린다"
//! 로 시작하는 이유가 바로 여기다. 그래서 다른 모듈보다 거부 조건이
//! 많고, 애매하면 실행하지 않는다.
//!
//! # 세 가지 게이트 — 하나라도 못 넘으면 실행하지 않는다
//!
//! ```text
//! 1  운영자 opt-in     명시적 플래그 없이는 실행하지 않는다
//! 2  자원 상한 강제     상한을 실제로 걸지 못하면 실행하지 않는다
//! 3  플랫폼 지원        강제 수단이 없는 플랫폼에서는 실행하지 않는다
//! ```
//!
//! ★ **2번과 3번이 핵심이다.** "일단 띄우고 상한은 나중에" 를 하지
//!   않는다 — 그 순간 `CLAUDE.md` §0.4 가 금지하는 "강제할 수 없는 것을
//!   보장으로 선언" 이 된다. Linux 는 cgroup 연결이 아직 없으므로
//!   **무방비로 실행하느니 거부한다.**
//!
//! # 이 모듈이 보장하지 않는 것
//!
//! ```text
//! 호스트 보호       Windows Job Object 는 커밋 메모리 상한일 뿐이다.
//!                   임의 네이티브 코드로부터 호스트를 지키지 못한다
//!                   (CLAUDE.md §0.4 — "S1 이상이면 안전" 이라 쓰지 않는다)
//! 네트워크 차단     방화벽 강제는 미착수다
//! 파일시스템 격리   artifact_scope 강제는 이 경로에 아직 연결되지 않았다
//! GPU 할당          NVML 확인·GPU 배타 할당은 후속 조각이다
//! ```
//!
//! # 상태 전이를 하지 않는다
//!
//! `docs/protocol/state-machines.md` §3 은 `STARTING -> RUNNING` 을
//! "프로세스 기동 + **첫 progress 수신**" 으로 정한다. 이 모듈은
//! 프로세스를 띄우고 종료를 관측할 뿐 progress 채널이 없으므로 그
//! 전이를 만들지 않는다 — 표에 없는 전이를 구현하지 않는다
//! (`CLAUDE.md` §2). 상태 전이는 별도 조각이다.

use gputeer_protocol::execution_spec::ExecutionSpec;

/// 자식의 표준 출력을 받는 파일 이름.
pub const STDOUT_FILENAME: &str = "stdout.log";
/// 자식의 표준 오류를 받는 파일 이름.
pub const STDERR_FILENAME: &str = "stderr.log";

/// 프로세스를 실제로 띄운 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutcome {
    /// 프로세스가 끝난 뒤의 종료 코드.
    pub exit_code: u32,
    /// 이 실행에 실제로 걸린 커밋 메모리 상한(바이트).
    ///
    /// "상한을 걸었다" 는 주장을 값으로 남긴다 — 0 이면 안 걸린 것이고,
    /// 그 경우 애초에 실행되지 않았어야 한다.
    pub commit_limit_bytes: u64,
    /// Job 에 연결된 프로세스들이 커밋한 메모리의 최댓값.
    ///
    /// ★ 이 값이 `commit_limit_bytes` 를 **넘을 수 있다.** Job Object 는
    ///   하드 리밋이 아니라 소프트 제한이며, 이 저장소는 이미 700~850KiB
    ///   오버슈트를 실측했다(`ADR-027`). 넘었다고 결함이 아니다 —
    ///   "상한이 하드하다" 고 쓰지 않기 위해 값을 그대로 남긴다.
    pub peak_commit_bytes: u64,
}

/// 실행하지 못한 이유. **전부 "실행 안 함" 이다** — 부분 실행이 없다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    /// 운영자가 명시적으로 켜지 않았다.
    NotOptedIn,
    /// 이 플랫폼에는 자원 상한 강제 수단이 연결돼 있지 않다.
    UnsupportedPlatform { detail: String },
    /// 상한을 거는 데 실패했다 — 상한 없이 띄우지 않는다.
    LimitNotApplied { detail: String },
    /// 프로세스 기동 자체가 실패했다.
    SpawnFailed { detail: String },
    /// 띄우기는 했는데 종료를 관측하지 못했다.
    ///
    /// ★ 이 경우 자식이 **아직 살아 있을 수 있다.** "실패" 로 뭉개면
    ///   고아 프로세스가 남은 것을 못 본다.
    WaitFailed { detail: String },
    /// 종료 코드를 읽지 못했다.
    ExitCodeUnavailable { detail: String },
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotOptedIn => write!(
                f,
                "EXEC_REFUSED:NOT_OPTED_IN: 실행이 명시적으로 켜지지 않았다"
            ),
            Self::UnsupportedPlatform { detail } => {
                write!(f, "EXEC_REFUSED:UNSUPPORTED_PLATFORM: {detail}")
            }
            Self::LimitNotApplied { detail } => write!(
                f,
                "EXEC_REFUSED:LIMIT_NOT_APPLIED: 자원 상한을 걸지 못해 실행하지 않았다 — {detail}"
            ),
            Self::SpawnFailed { detail } => write!(f, "EXEC_FAILED:SPAWN: {detail}"),
            Self::WaitFailed { detail } => write!(
                f,
                "EXEC_FAILED:WAIT: 종료를 관측하지 못했다(자식이 살아 있을 수 있다) — {detail}"
            ),
            Self::ExitCodeUnavailable { detail } => {
                write!(f, "EXEC_FAILED:EXIT_CODE: {detail}")
            }
        }
    }
}

impl std::error::Error for ExecutionError {}

/// 실행 정책 — caller 가 명시적으로 채운다.
#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    /// 운영자가 "이 Agent 는 실제로 코드를 실행해도 된다" 를 켰는가.
    pub opted_in: bool,
    /// Job Object 에 걸 **커밋 메모리 상한**(바이트). 0 은 허용하지 않는다.
    ///
    /// ★ `create_constrained_child_for_ram_limit()` 을 쓰지 않는다.
    ///   그 함수는 `ADR-027` 의 `VRAM 최대 ≈ RAM 제한 − 2000MiB` 변환을
    ///   적용하는 **VRAM 상한 전용** 경로다. 여기에 작은 값을 넣으면
    ///   0 으로 포화해 "상한 0" 이 된다(실측으로 확인). 이 모듈이
    ///   원하는 것은 커밋 상한 자체이므로 변환 없는 경로를 쓴다.
    pub commit_limit_bytes: u64,
    /// 자식의 표준 출력·오류를 받을 디렉터리. `None` 이면 받지 않는다.
    ///
    /// ★ **체크포인트 디렉터리를 직접 가리키지 마라.** 그 네임스페이스는
    ///   `write_once()` 가 소유하며(`DoD-21` 계약), 남의 프로세스가 그
    ///   안에 직접 쓰게 하면 그 계약이 깨진다. 별도 작업 디렉터리로
    ///   받은 뒤 부모가 읽어 `write_once()` 로 옮긴다.
    pub capture_dir: Option<std::path::PathBuf>,
}

/// 검증된 실행 지시를 실제 프로세스로 띄우고 종료까지 관측한다.
///
/// # 순서
///
/// ```text
/// 1  opt-in 확인            아니면 NotOptedIn
/// 2  상한 값 확인            0 이면 LimitNotApplied
/// 3  플랫폼별 기동 + 상한     실패하면 실행 안 함
/// 4  종료 대기
/// 5  종료 코드 읽기
/// ```
pub fn execute(
    spec: &ExecutionSpec,
    policy: ExecutionPolicy,
) -> Result<ExecutionOutcome, ExecutionError> {
    if !policy.opted_in {
        return Err(ExecutionError::NotOptedIn);
    }
    if policy.commit_limit_bytes == 0 {
        return Err(ExecutionError::LimitNotApplied {
            detail: "commit_limit_bytes 가 0 이다".into(),
        });
    }
    platform::execute(spec, &policy)
}

#[cfg(windows)]
mod platform {
    use super::{ExecutionError, ExecutionOutcome, ExecutionPolicy};
    use gputeer_protocol::execution_spec::ExecutionSpec;

    pub(super) fn execute(
        spec: &ExecutionSpec,
        policy: &ExecutionPolicy,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        // ★ 명령줄 조립은 `runtime-windows` 가 한다. MSVC 인자 분해 규칙
        //   때문에 손으로 이어 붙이면 인용이 어긋난다 — 이미 그 버그를
        //   한 번 겪었다(`DoD` 이력의 "명령줄 인용 버그 2건").
        use std::ffi::OsStr;

        use super::{STDERR_FILENAME, STDOUT_FILENAME};

        let exe: std::ffi::OsString = spec.entrypoint.clone().into();
        let args: Vec<std::ffi::OsString> = spec.args.iter().map(|a| a.clone().into()).collect();
        let arg_refs: Vec<&OsStr> = args.iter().map(std::ffi::OsString::as_os_str).collect();
        let command_line = gputeer_runtime_windows::quote_command_line(&exe, &arg_refs);
        let create = gputeer_runtime_windows::CreateProcessSpec {
            application_name: exe,
            command_line,
            current_dir: None,
            stdout_path: policy
                .capture_dir
                .as_ref()
                .map(|dir| dir.join(STDOUT_FILENAME)),
            stderr_path: policy
                .capture_dir
                .as_ref()
                .map(|dir| dir.join(STDERR_FILENAME)),
        };

        // ★ 상한을 **먼저** 걸고 재개한다. `create_constrained_child_for_ram_limit`
        //   이 그 순서를 보장한다(CREATE_SUSPENDED -> Job -> Assign -> Resume).
        let child =
            gputeer_runtime_windows::create_constrained_child(&create, policy.commit_limit_bytes)
                .map_err(|e| {
                // 기동 실패와 상한 실패를 구분한다 — 전자는 Manifest 문제일
                // 수 있고 후자는 이 Agent 의 환경 문제다.
                if e.kind() == std::io::ErrorKind::Unsupported
                    || e.kind() == std::io::ErrorKind::InvalidInput
                {
                    ExecutionError::LimitNotApplied {
                        detail: e.to_string(),
                    }
                } else {
                    ExecutionError::SpawnFailed {
                        detail: e.to_string(),
                    }
                }
            })?;

        child.wait().map_err(|e| ExecutionError::WaitFailed {
            detail: e.to_string(),
        })?;
        let exit_code = child
            .exit_code()
            .map_err(|e| ExecutionError::ExitCodeUnavailable {
                detail: e.to_string(),
            })?;
        let (peak, limit) = child.query_memory_limits().map_err(|e| {
            // 여기까지 왔으면 자식은 이미 끝났다. 관측 실패를 실행 실패로
            // 뭉개지 않고 별도로 보고한다.
            ExecutionError::ExitCodeUnavailable {
                detail: format!("종료 코드는 {exit_code} 인데 메모리 관측에 실패했다: {e}"),
            }
        })?;

        Ok(ExecutionOutcome {
            exit_code,
            commit_limit_bytes: limit as u64,
            peak_commit_bytes: peak as u64,
        })
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{ExecutionError, ExecutionOutcome, ExecutionPolicy};
    use gputeer_protocol::execution_spec::ExecutionSpec;

    /// ★ **무방비로 실행하느니 거부한다.**
    ///
    /// Linux 에는 cgroup 으로 강제할 수단이 실제로 있고 `ENV-03` 이
    /// 네이티브에서 4종(memory/CPU/PID/freezer)을 확인했지만, **이
    /// 저장소의 코드가 그것을 걸지는 않는다.** 연결되지 않은 강제를
    /// "있다" 고 취급해 프로세스를 띄우면 `CLAUDE.md` §0.4 위반이다.
    pub(super) fn execute(
        _spec: &ExecutionSpec,
        _policy: &ExecutionPolicy,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        Err(ExecutionError::UnsupportedPlatform {
            detail: "이 플랫폼에는 자원 상한 강제가 연결돼 있지 않다(Linux cgroup 미착수) — 상한 없이 실행하지 않는다".into(),
        })
    }
}
