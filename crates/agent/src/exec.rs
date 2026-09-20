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

/// 종료를 관측한 결과 — 종료 코드의 **존재 여부**를 보존한다(B+E 계획서 §5.7 (3) · 결함 69).
///
/// ★ 숫자로 "없음" 을 나타내지 않는다. 0 은 정상 종료이고, Linux 의 합성값 -1 은 u32 로 옮기면
///   Windows 에서 실제로 관측할 수 있는 u32::MAX 와 같아진다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitObserved {
    /// 종료 코드를 관측했다 — 값 그대로다.
    Code(u32),
    /// 종료는 관측했지만 코드가 없다 — Linux 신호 종료, 또는 `wait()` 뒤 코드 조회 실패.
    NoCode { detail: String },
}

impl ExitObserved {
    /// 관측한 종료 코드. 없으면 `None`.
    pub fn code(&self) -> Option<u32> {
        match self {
            Self::Code(code) => Some(*code),
            Self::NoCode { .. } => None,
        }
    }
}

/// 프로세스를 실제로 띄운 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutcome {
    /// 프로세스가 끝난 뒤의 종료 관측.
    pub exit: ExitObserved,
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
    ///
    /// 모르면 `None` 이다 — 0 으로 채우지 않는다(`CLAUDE.md` §1).
    pub peak_commit_bytes: Option<u64>,
    /// 종료 뒤 메모리 관측이 실패했으면 그 사유. 실패해도 종료 관측(`exit`)은 그대로 남긴다(결함 69 (ii)).
    pub memory_observation_error: Option<String>,
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
    // ★ `ExitCodeUnavailable` 은 없앴다(결함 69, 2026-09-14). `wait()` 뒤 코드 조회 실패는 **종료를 관측한** 것이라
    //   실행 오류가 아니라 `Ok` 의 `ExitObserved::NoCode` 다. 메모리 관측 실패도 오류가 아니라
    //   `memory_observation_error` 다 — 전에는 둘 다 여기로 와 종료 보고가 사라졌다.
    /// 소유자의 정지 요청을 실행하지 못했다.
    ///
    /// ★ 이건 다른 오류들보다 심각하다. 소유자가 "비워라" 라고 했는데
    ///   못 비운 것이므로, `CLAUDE.md` §0.1 이 약속한 것을 못 지킨
    ///   상태다. 조용히 넘기지 않는다.
    StopFailed { detail: String },

    /// ★ 요구한 GPU 가 **지금** 그 노드에 없다 (2026-09-07 신설).
    ///
    /// 스케줄러가 골랐을 때와 실제로 띄우는 순간 사이에는 시차가 있다.
    /// 그 사이에 GPU 가 빠지거나, 다른 프로세스가 VRAM 을 먹거나,
    /// 재부팅으로 장치 번호가 바뀔 수 있다.
    ///
    /// ★★ **`GpuUnverifiable` 과 갈라 둔다.** 아래 항목 참조.
    GpuRequirementUnmet { detail: String },

    /// ★ GPU 를 **확인하지 못했다** — 모자란 것이 아니다 (2026-09-07 신설).
    ///
    /// NVML 을 못 열었거나 조회가 실패한 경우다.
    ///
    /// ★★ **이 둘을 합치면 안 된다.** 합치면 NVML 이 잠깐 안 열린 노드가
    ///   "GPU 요구를 못 맞추는 노드" 로 낙인찍혀 계속 배제된다.
    ///   `runtime-nvml` 이 같은 이유로 `is_unknown()` 을 따로 두었고,
    ///   그 구분을 여기서 뭉개면 그 설계가 무의미해진다.
    ///
    ///   2026-09-07 실측에서 GPU 없는 기계가 정확히 이 모양으로 나오는
    ///   것을 확인했다(`docs/evidence/_raw/NVML_preflight_실측.txt`).
    GpuUnverifiable { detail: String },
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
            Self::StopFailed { detail } => write!(
                f,
                "OWNER_STOP_FAILED: 소유자의 정지 요청을 실행하지 못했다 — {detail}"
            ),
            // ★ 접두사를 다르게 둔다. 로그만 보고도 "모자라다" 와
            //   "확인 못 했다" 를 갈라야 운영자가 엉뚱한 곳을 고치지 않는다.
            Self::GpuRequirementUnmet { detail } => write!(
                f,
                "EXEC_REFUSED:GPU_REQUIREMENT_UNMET: 요구한 GPU 가 지금 이 노드에 없다 — {detail}"
            ),
            Self::GpuUnverifiable { detail } => write!(
                f,
                "EXEC_REFUSED:GPU_UNVERIFIABLE: GPU 를 확인하지 못했다(모자란 것이 아니다) — {detail}"
            ),
        }
    }
}

impl std::error::Error for ExecutionError {}

/// 실행 중인 작업을 **밖에서** 멈추는 손잡이.
///
/// # 왜 필요한가
///
/// `CLAUDE.md` §0.1 은 다른 어떤 규칙보다 앞에 "노드 소유자는 언제든
/// 자기 GPU 를 즉시 비울 수 있어야 한다 — 네트워크가 끊겨도, quorum 이
/// 없어도, Coordinator 가 죽어도" 를 둔다. `execute()` 는 자식이 끝날
/// 때까지 그 스레드를 붙잡으므로, 이 손잡이가 없으면 그 규칙을 지킬
/// 수단 자체가 없다.
///
/// # 이 손잡이가 보장하지 않는 것
///
/// ```text
/// 정중한 종료      강제 종료다. 자식은 정리할 기회를 얻지 못한다
/// 손실 없음        진행 중이던 작업은 잃는다. 얼마를 잃는지 계산해
///                  보여주는 것은 §0.1 이 요구하는 호출부의 책임이다
/// GPU 메모리 반환  프로세스가 죽으면 드라이버가 회수하지만, 이
///                  계층이 그것을 확인하지는 않는다
/// ```
pub struct WorkloadStopper {
    #[cfg(windows)]
    inner: gputeer_runtime_windows::JobStopper,
    #[cfg(target_os = "linux")]
    inner: gputeer_runtime_linux::CgroupStopper,
}

impl WorkloadStopper {
    /// 테스트 전용 — 아무것도 안 멈추는 손잡이.
    ///
    /// ★ 이건 **성공을 돌려주지 않는다.** 멈춘 척하면 그걸 쓰는 테스트가
    ///   "멈췄다" 를 통과시켜 공허해진다.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            #[cfg(windows)]
            inner: gputeer_runtime_windows::JobStopper::inert_for_test(),
            #[cfg(target_os = "linux")]
            inner: gputeer_runtime_linux::CgroupStopper::inert_for_test(),
        }
    }

    /// 작업 프로세스 트리를 즉시 끝낸다.
    ///
    /// ★ 프로세스 하나가 아니라 **트리 전체**다. 자식이 손자를 만들었으면
    ///   (예: `cmd /c python train.py`) 그 손자도 죽여야 GPU 가 실제로
    ///   비워진다 — 실측으로 확인했다(`owner_stop.rs`).
    ///
    /// 이미 끝난 작업에 불러도 성공한다. 소유자가 정지 버튼을 두 번
    /// 누르는 것은 정상적인 일이다.
    ///
    /// ★ 이 문서는 `for_test()` 가 위에 끼어들면서 그쪽으로 밀려나 있었다
    ///   — 속성·문서는 **바로 아래 항목**에 붙는다.
    pub fn stop(&self) -> Result<(), ExecutionError> {
        #[cfg(windows)]
        {
            self.inner
                .terminate(EXIT_CODE_OWNER_STOPPED)
                .map_err(|error| ExecutionError::StopFailed {
                    detail: error.to_string(),
                })
        }
        #[cfg(target_os = "linux")]
        {
            // cgroup 전체를 끝낸다 — 자식이 손자를 만들었어도 같이 죽는다.
            // Windows 의 `TerminateJobObject` 와 같은 자리다.
            self.inner
                .stop()
                .map_err(|error| ExecutionError::StopFailed {
                    detail: error.to_string(),
                })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            // 이 플랫폼은 애초에 실행하지 않으므로(아래 `platform::execute`)
            // 멈출 대상이 존재하지 않는다. 그래도 "성공했다" 고 거짓말하지
            // 않는다.
            Err(ExecutionError::UnsupportedPlatform {
                detail: "이 플랫폼에서는 작업을 실행하지 않으므로 멈출 대상도 없다".into(),
            })
        }
    }
}

/// 소유자가 멈춘 작업의 종료 코드.
///
/// ★ 0 이 아니어야 한다. 0 이면 정상 완료와 구분되지 않아, 소유자가
///   중단시킨 작업이 "성공" 으로 기록된다.
pub const EXIT_CODE_OWNER_STOPPED: u32 = 0xC000_0013;

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
    /// 띄우기 **직전에** 확인할 GPU 요구. `None` 이면 확인하지 않는다.
    ///
    /// ★ **기본이 `None` 인 것은 의도다.** GPU 를 안 쓰는 워크로드까지
    ///   NVML 을 요구하면, NVIDIA 카드 없는 노드가 CPU 작업조차 못 받는다.
    ///   요구를 선언한 Job 만 이 관문을 지난다.
    ///
    /// ★★ **이것은 "확인" 이지 "예약" 이 아니다.** 통과한 뒤 다른
    ///   프로세스가 VRAM 을 먹는 것을 막지 못한다 — 그 창은 열려 있다.
    ///   `runtime-nvml` 의 통과 타입에 반납할 핸들이 없는 것이 그 뜻이다.
    ///   `CLAUDE.md` §0.4 — 강제할 수 없는 것을 보장으로 선언하지 않는다.
    pub gpu_requirements: Option<gputeer_runtime_nvml::preflight::GpuRequirements>,
    /// 자식의 표준 출력·오류를 받을 디렉터리. `None` 이면 받지 않는다.
    ///
    /// ★ **체크포인트 디렉터리를 직접 가리키지 마라.** 그 네임스페이스는
    ///   `write_once()` 가 소유하며(`DoD-21` 계약), 남의 프로세스가 그
    ///   안에 직접 쓰게 하면 그 계약이 깨진다. 별도 작업 디렉터리로
    ///   받은 뒤 부모가 읽어 `write_once()` 로 옮긴다.
    pub capture_dir: Option<std::path::PathBuf>,
    /// 이 실행을 다른 실행과 구분하는 이름.
    ///
    /// ★ Linux 에서 cgroup 디렉터리 이름이 된다. **attempt 마다 달라야
    ///   한다** — 같으면 두 작업이 같은 cgroup 을 공유해, 소유자가 A 를
    ///   멈출 때 B 도 같이 죽는다.
    ///
    /// ★ `ExecutionSpec.job_id` 를 쓰지 않는다(2026-08-30). 같은 Job 의
    ///   두 attempt 가 같은 이름을 받아 정확히 그 사고가 난다. 우연한
    ///   유일성에 기대지 않고 호출부가 명시한다.
    ///
    /// Windows 는 Job Object 가 익명 커널 객체라 이 값을 쓰지 않는다.
    pub isolation: IsolationIdentity,
    /// Linux 에서 하위 cgroup 을 만들 부모.
    ///
    /// ★ 2026-08-30 독립 검수 지적. 초안은 무조건 "내 cgroup" 을 썼는데,
    ///   보통의 배포 환경에서 Agent 자신이 그 cgroup 안에 있으므로
    ///   cgroup v2 의 **"내부 프로세스 금지"** 규칙 때문에 `+memory` 를
    ///   하위에 켤 수 없다 — 거부될 **가능성**이 아니라 구조적으로 거부되는
    ///   경로였다. 위임받은 subtree 를 전달할 방법 자체가 없었다.
    ///
    ///   `None` 이면 "내 cgroup"(fail-closed 기본값)이고, 운영자가
    ///   `--workload-cgroup-parent` 로 지정할 수 있다.
    ///
    /// ★ **아무 위임 경로나 되지 않는다**(2026-08-30 독립 검수 4라운드
    ///   지적 — 이 문서가 그렇게 읽혔다). 받는 것은 정확히 하나다.
    ///
    /// ```text
    /// /sys/fs/cgroup/gputeer-<이름>     cgroup v2 루트의 직속 자식
    /// ```
    ///
    ///   깊이 1 로 고정한 이유는 `system.slice/gputeer-x` 같은 다른 서비스
    ///   계층 아래를 쓰지 못하게 하기 위해서다. 그 대가로 **systemd 가
    ///   관리하는 중첩 위임 slice 는 지원하지 않는다** — 운영자가
    ///   `/sys/fs/cgroup/gputeer-workloads` 를 직접 만들어야 한다.
    ///
    ///   Windows 는 이 값을 쓰지 않는다.
    pub cgroup_parent: Option<std::path::PathBuf>,
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
    execute_with_control(spec, policy, |_| {})
}

/// `execute()` 와 같지만, 자식이 뜬 **직후** 정지 손잡이를 caller 에게
/// 넘긴다.
///
/// # 왜 콜백인가
///
/// 손잡이는 `wait()` 으로 스레드가 붙잡히기 **전에** 나가야 한다.
/// 반환값으로 주면 이미 늦다 — 반환은 자식이 끝난 뒤에나 일어난다.
///
/// ```text
/// 기동 + 상한 적용
///   -> on_started(손잡이)      <- 여기서 나가야 소유자가 멈출 수 있다
///   -> wait()                  <- 여기서 붙잡힌다
///   -> 종료 코드 관측
/// ```
///
/// ★ `on_started` 는 **빨리 돌아와야 한다.** 여기서 오래 걸리면 그만큼
///   자식 관측이 늦어진다. 손잡이를 어딘가에 등록만 하고 나가는 용도다.
///
/// # 실행하지 못하면 콜백도 안 부른다
///
/// opt-in 거부·상한 실패·기동 실패는 전부 자식이 없는 상태이므로
/// 멈출 대상도 없다. "멈출 수 있다" 는 손잡이를 주고 나서 실은 아무것도
/// 안 뜬 상태로 두지 않는다.
pub fn execute_with_control(
    spec: &ExecutionSpec,
    policy: ExecutionPolicy,
    on_started: impl FnOnce(WorkloadStopper),
) -> Result<ExecutionOutcome, ExecutionError> {
    // ★ 사전 관문은 `preflight` 한 곳에 있다 — Agent 가 ACK **전에** 같은 함수를 부른다(결함 ⑱).
    preflight(&policy)?;
    platform::execute(spec, &policy, on_started)
}

/// 자식을 띄우기 **전에** 판정할 수 있는 관문 — opt-in · 상한 값 · GPU 요구.
///
/// ★ 결함 ⑱ (설계 A, 2026-09-14) — Agent 는 이 함수로 Grant 를 받아들일 수 있는지 먼저 보고
///   ACK 를 보낸 **뒤에** 실행한다. 규범(`docs/protocol/state-machines.md` §3)에서 Grant 수락
///   (GRANT_ACCEPTED)은 프로세스 기동(PROCESS_STARTED) **전**의 사건이다.
///   [`execute_with_control`] 도 같은 함수를 부른다 — 관문을 두 벌 두지 않는다.
///   ★ 그래서 ACK 뒤에 한 번 더 돈다. GPU 는 **새로 관측**하므로 첫 검사를 통과하고 두 번째에서
///     거부될 수 있다 — 그때는 ACK 가 이미 갔다(결함 ㊼, 구현 검수 49).
///
/// 플랫폼 지원 · 실제 상한 적용 · 기동은 여기서 보지 않는다 — 띄워 봐야 아는 것이다.
pub fn preflight(policy: &ExecutionPolicy) -> Result<(), ExecutionError> {
    if !policy.opted_in {
        return Err(ExecutionError::NotOptedIn);
    }
    if policy.commit_limit_bytes == 0 {
        return Err(ExecutionError::LimitNotApplied {
            detail: "commit_limit_bytes 가 0 이다".into(),
        });
    }
    // ★★ **GPU 확인은 여기다 — 자식을 띄우기 전이다** (2026-09-07 신설).
    //
    //   띄운 뒤에 확인하면 "요구를 못 맞추는데 이미 남의 GPU 를 물고
    //   있는" 순간이 생긴다. 그 순간이 짧다고 없는 것이 아니다.
    //
    //   ★ 상한 검사 **뒤에** 둔 것도 의도다. 상한을 못 걸면 어차피 안
    //     띄우므로, 그 경우 NVML 을 부를 이유가 없다. 값싼 관문이 먼저다.
    if let Some(requirements) = policy.gpu_requirements.as_ref() {
        if let Err(rejection) =
            gputeer_runtime_nvml::preflight::check_gpu_requirements_now(requirements)
        {
            // ★★ **여기서 두 갈래로 나눈다.** `runtime-nvml` 이
            //   `is_unknown()` 을 따로 둔 이유가 바로 이 자리다 —
            //   "모자라다" 와 "확인 못 했다" 를 합치면 NVML 이 잠깐 안
            //   열린 노드가 계속 배제된다.
            return Err(if rejection.is_unknown() {
                ExecutionError::GpuUnverifiable {
                    detail: format!("{rejection:?}"),
                }
            } else {
                ExecutionError::GpuRequirementUnmet {
                    detail: format!("{rejection:?}"),
                }
            });
        }
    }
    Ok(())
}

#[cfg(windows)]
mod platform {
    use super::{ExecutionError, ExecutionOutcome, ExecutionPolicy};
    use gputeer_protocol::execution_spec::ExecutionSpec;

    pub(super) fn execute(
        spec: &ExecutionSpec,
        policy: &ExecutionPolicy,
        on_started: impl FnOnce(super::WorkloadStopper),
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

        // ★ 자식을 **정지 상태로** 받는다. 상한은 이미 걸려 있다.
        //
        //   재개를 뒤로 미루는 이유는 등록을 먼저 끝내기 위해서다 —
        //   돌기 시작한 뒤 등록하면 짧은 작업은 등록 전에 끝나 소유자
        //   화면에 한 번도 안 보일 수 있다(2026-08-29 독립 검수 지적).
        let suspended = gputeer_runtime_windows::create_constrained_child_suspended(
            &create,
            policy.commit_limit_bytes,
        )
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

        // 정지 손잡이를 먼저 만든다. 못 만들면 재개하지 않고 끝낸다 —
        // 이 경우 자식은 단 한 번도 돌지 않았고, `SuspendedChild` 가
        // drop 되면 `KILL_ON_JOB_CLOSE` 가 정리한다.
        //
        // ★ 이전 판은 이미 돌고 있는 자식에 대해 손잡이를 만들다 실패해
        //   "돌기 시작했는데 멈출 수 없는" 순간이 존재했다. 이제 그
        //   순간 자체가 없다.
        let stopper = suspended
            .stopper()
            .map_err(|error| ExecutionError::SpawnFailed {
                detail: format!(
                    "정지 손잡이를 만들 수 없어 실행하지 않았다(자식은 한 번도 돌지 않았고 정리됨) \
                     — 멈출 수 없는 작업은 시작하지 않는다: {error}"
                ),
            })?;
        on_started(super::WorkloadStopper { inner: stopper });

        // 이제서야 돌린다. 소유자는 첫 명령이 실행되기 전부터 이 작업을
        // 보고 멈출 수 있다.
        let child = suspended
            .resume()
            .map_err(|e| ExecutionError::SpawnFailed {
                detail: e.to_string(),
            })?;

        // ★ 결함 77 — 기다리기와 코드 조회를 한 호출로 묶는다. 기다리기가 끝났으면 259 도 실제 종료 코드다
        //   (`exit_code()` 의 STILL_ACTIVE 가드를 여기서 쓰면 실제 값 259 를 "코드 없음" 으로 바꾼다 — 재검수 57 실측).
        // ★ 결함 69 (i) — 기다리기는 성공했는데 코드만 못 읽었으면 NoCode. 전에는 실행 오류로 돌려 종료 보고가 사라졌다.
        //   기다리기 자체의 실패는 여전히 오류다 — 자식이 살아 있을 수 있어 종료를 보고하지 않는다.
        // ★ 결함 79 — 어댑터 결과를 보고 칸으로 옮기는 분류는 `classify_wait_result` · `classify_windows_memory` 가 한다(실패 입력을 넣어 시험한다).
        let exit = super::classify_wait_result(child.wait_then_exit_code())?;
        // ★ 결함 69 (ii) — 메모리 관측 실패는 종료 관측과 따로 남긴다. 코드는 코드대로 보고한다.
        let (commit_limit_bytes, peak_commit_bytes, memory_observation_error) =
            super::classify_windows_memory(child.query_memory_limits(), policy.commit_limit_bytes);

        Ok(ExecutionOutcome {
            exit,
            commit_limit_bytes,
            peak_commit_bytes,
            memory_observation_error,
        })
    }
}

/// 결함 79 — Windows `wait_then_exit_code()` 결과의 분류. 기다리기 실패는 실행 오류(자식이 살아 있을 수 있어 종료를 보고하지 않는다),
/// 기다린 뒤 코드 조회 실패는 NoCode, 코드는 그대로(259 포함 — 결함 77).
///
/// ★ 플랫폼 밖 순수 함수다 — OS 호출의 실패를 일으킬 수단이 없어, 실패 **결과**를 직접 넣어 이 분류만 시험한다(79 는 여기서 닫히지 않는다).
#[cfg_attr(not(windows), allow(dead_code))]
fn classify_wait_result<E1: std::fmt::Display, E2: std::fmt::Display>(
    wait: Result<Result<u32, E1>, E2>,
) -> Result<ExitObserved, ExecutionError> {
    match wait {
        Err(e) => Err(ExecutionError::WaitFailed {
            detail: e.to_string(),
        }),
        Ok(Ok(code)) => Ok(ExitObserved::Code(code)),
        Ok(Err(e)) => Ok(ExitObserved::NoCode {
            detail: format!("wait() 뒤 종료 코드 조회 실패: {e}"),
        }),
    }
}

/// 결함 79 — Windows 메모리 조회 결과의 분류. 실패하면 상한은 이 실행에 건 정책 값, peak 는 비우고 사유를 남긴다 — 실행 오류로 올리지 않는다(결함 69).
#[cfg_attr(not(windows), allow(dead_code))]
fn classify_windows_memory<E: std::fmt::Display>(
    query: Result<(usize, usize), E>,
    policy_commit_limit_bytes: u64,
) -> (u64, Option<u64>, Option<String>) {
    match query {
        Ok((peak, limit)) => (limit as u64, Some(peak as u64), None),
        Err(e) => (policy_commit_limit_bytes, None, Some(e.to_string())),
    }
}

/// 결함 81 — 리눅스 `memory.peak` 원문 읽기 결과의 분류. 값이면 peak, 아니면 peak 는 비우고 **사유를 남긴다** —
/// 부재(NotFound, 보통 memory.peak 가 없는 커널) · 읽기 실패 · 해석 실패를 문구로 가른다.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn classify_linux_memory_peak(read: std::io::Result<String>) -> (Option<u64>, Option<String>) {
    match read {
        Ok(text) => match text.trim().parse::<u64>() {
            Ok(peak) => (Some(peak), None),
            Err(_) => (
                None,
                Some(format!(
                    "memory.peak 값을 해석하지 못했다: {:?}",
                    text.trim()
                )),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
            None,
            Some(
                "memory.peak 가 없다 — 이 커널은 최댓값을 제공하지 않을 수 있다(관측 수단 없음)"
                    .to_string(),
            ),
        ),
        Err(e) => (None, Some(format!("memory.peak 읽기 실패: {e}"))),
    }
}

/// 결함 144 (검수 68) — 리눅스 `memory.max` 원문 읽기 결과의 분류. 숫자면 그 상한, 아니면 이 실행에 건 **정책 상한**으로 채우고 사유를 남긴다.
/// ★ 전에는 부재 · 읽기 실패 · 해석 실패가 조용히 정책 상한이 됐다 — `memory.peak` 만 정상이면 사유 칸이 비었다.
/// ★ 결함 173 (검수 68b) — 전에는 "max" 도 "해석하지 못했다" 로 적었다. `max` 는 cgroup v2 의 **유효한 "상한 없음" 표현**이라 틀린 말이다.
///   이제 "상한 없음이 관측돼 이 실행에 건 정책 상한과 어긋난다" 로 따로 적는다. 여전히 실행 오류로 올리지는 않는다(종료 보고 경로를 건너뛰게 된다).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn classify_linux_memory_limit(
    read: std::io::Result<String>,
    policy_commit_limit_bytes: u64,
) -> (u64, Option<String>) {
    match read {
        Ok(text) if text.trim() == "max" => (
            policy_commit_limit_bytes,
            Some("memory.max 가 max(상한 없음)로 관측됐다 — 이 실행에 건 정책 상한과 어긋난다 · 정책 상한으로 적었다".to_string()),
        ),
        Ok(text) => match text.trim().parse::<u64>() {
            Ok(limit) => (limit, None),
            Err(_) => (
                policy_commit_limit_bytes,
                Some(format!("memory.max 값을 해석하지 못했다: {:?} — 정책 상한으로 적었다", text.trim())),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
            policy_commit_limit_bytes,
            Some("memory.max 가 없다 — 정책 상한으로 적었다".to_string()),
        ),
        Err(e) => (policy_commit_limit_bytes, Some(format!("memory.max 읽기 실패: {e} — 정책 상한으로 적었다"))),
    }
}

/// 결함 144 — 상한 · peak 사유를 한 칸에 모은다. 둘 다 없으면 None, 하나만 있으면 그것, 둘이면 " · " 로 잇는다(어느 것도 버리지 않는다).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn join_observation_errors(limit: Option<String>, peak: Option<String>) -> Option<String> {
    match (limit, peak) {
        (None, None) => None,
        (Some(one), None) | (None, Some(one)) => Some(one),
        (Some(limit), Some(peak)) => Some(format!("{limit} · {peak}")),
    }
}

#[cfg(test)]
mod observation_classification_tests {
    //! 결함 79 · 81 — 어댑터 결과 -> 보고 칸 분류. 실패 결과를 직접 넣는다(OS 실패 주입이 아니다).
    use super::*;

    #[test]
    fn a_wait_failure_is_an_execution_error_and_a_code_lookup_failure_is_no_code() {
        let wait_failed = classify_wait_result::<String, String>(Err("wait 실패".into()));
        assert!(
            matches!(wait_failed, Err(ExecutionError::WaitFailed { .. })),
            "{wait_failed:?}"
        );
        let no_code = classify_wait_result::<String, String>(Ok(Err("코드 조회 실패".into())))
            .expect("종료는 관측했다");
        assert!(
            matches!(&no_code, ExitObserved::NoCode { detail } if detail.contains("코드 조회 실패")),
            "{no_code:?}"
        );
        let code = classify_wait_result::<String, String>(Ok(Ok(259))).expect("종료는 관측했다");
        assert!(matches!(code, ExitObserved::Code(259)), "{code:?}");
    }

    #[test]
    fn a_windows_memory_query_failure_keeps_the_policy_limit_and_the_reason() {
        let (limit, peak, error) =
            classify_windows_memory::<String>(Err("QueryInformationJobObject 실패".into()), 256);
        assert_eq!((limit, peak), (256, None));
        assert!(
            error
                .as_deref()
                .is_some_and(|e| e.contains("QueryInformationJobObject")),
            "{error:?}"
        );
        assert_eq!(
            classify_windows_memory::<String>(Ok((100, 200)), 256),
            (200, Some(100), None)
        );
    }

    #[test]
    fn a_linux_memory_peak_absence_read_failure_and_garbage_each_leave_a_distinct_reason() {
        assert_eq!(
            classify_linux_memory_peak(Ok("12345\n".into())),
            (Some(12345), None)
        );
        let (peak, absent) =
            classify_linux_memory_peak(Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
        assert!(
            peak.is_none() && absent.as_deref().is_some_and(|e| e.contains("없다")),
            "{absent:?}"
        );
        let (peak, denied) = classify_linux_memory_peak(Err(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )));
        assert!(
            peak.is_none() && denied.as_deref().is_some_and(|e| e.contains("읽기 실패")),
            "{denied:?}"
        );
        let (peak, garbage) = classify_linux_memory_peak(Ok("max\n".into()));
        assert!(
            peak.is_none()
                && garbage
                    .as_deref()
                    .is_some_and(|e| e.contains("해석하지 못했다")),
            "{garbage:?}"
        );
        assert_ne!(absent, denied, "부재와 읽기 실패의 사유가 같다");
    }

    /// 결함 144 — memory.max 실패는 정책 상한으로 채우되 사유를 남기고, peak 가 정상이어도 사유 칸이 비지 않는다.
    #[test]
    fn a_linux_memory_limit_failure_keeps_the_policy_limit_and_its_reason_survives_a_good_peak() {
        assert_eq!(
            classify_linux_memory_limit(Ok("268435456\n".into()), 1),
            (268435456, None)
        );
        // 결함 173 — max 는 "상한 없음" 으로, 진짜 해석 불가 값은 "해석하지 못했다" 로 가른다
        let (limit, unlimited) = classify_linux_memory_limit(Ok("max\n".into()), 256);
        assert!(
            limit == 256
                && unlimited
                    .as_deref()
                    .is_some_and(|e| e.contains("상한 없음") && e.contains("어긋난다")),
            "{unlimited:?}"
        );
        let (limit, garbage) = classify_linux_memory_limit(Ok("12k\n".into()), 256);
        assert!(
            limit == 256
                && garbage
                    .as_deref()
                    .is_some_and(|e| e.contains("memory.max") && e.contains("해석하지 못했다")),
            "{garbage:?}"
        );
        assert_ne!(unlimited, garbage, "상한 없음과 해석 불가의 사유가 같다");
        let (limit, absent) = classify_linux_memory_limit(
            Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            256,
        );
        assert!(
            limit == 256
                && absent
                    .as_deref()
                    .is_some_and(|e| e.contains("memory.max 가 없다")),
            "{absent:?}"
        );
        let (limit, denied) = classify_linux_memory_limit(
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            256,
        );
        assert!(
            limit == 256
                && denied
                    .as_deref()
                    .is_some_and(|e| e.contains("memory.max 읽기 실패")),
            "{denied:?}"
        );

        let (_, peak_ok) = classify_linux_memory_peak(Ok("12345\n".into()));
        let joined = join_observation_errors(denied.clone(), peak_ok);
        assert_eq!(
            joined, denied,
            "peak 가 정상이어도 memory.max 사유가 남아야 한다"
        );
        let (_, peak_absent) =
            classify_linux_memory_peak(Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
        let both = join_observation_errors(absent, peak_absent).expect("둘 다 사유");
        assert!(
            both.contains("memory.max 가 없다") && both.contains("memory.peak 가 없다"),
            "{both}"
        );
        assert_eq!(join_observation_errors(None, None), None);
        // 결함 172 (검수 68b) — memory.max 는 정상이고 peak 만 사유가 있는 조합. 이 분기만 None 으로 바꾸는 회귀를 잡는다
        let (_, peak_denied) = classify_linux_memory_peak(Err(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )));
        assert!(peak_denied.is_some(), "시험 전제: peak 사유가 있어야 한다");
        assert_eq!(
            join_observation_errors(None, peak_denied.clone()),
            peak_denied,
            "memory.max 가 정상이어도 peak 사유가 남아야 한다"
        );
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{ExecutionError, ExecutionOutcome, ExecutionPolicy};
    use gputeer_protocol::execution_spec::ExecutionSpec;

    use super::{STDERR_FILENAME, STDOUT_FILENAME};

    /// cgroup v2 로 상한을 걸고 실행한다.
    ///
    /// # ★ 이것이 무엇이고 무엇이 아닌가
    ///
    /// `runtime-linux` 가 실측으로 확인한 그대로다.
    ///
    /// ```text
    /// 막는다     실수로 메모리를 너무 먹는 작업
    ///            소유자가 즉시 끝내야 하는 작업(cgroup.kill 로 트리 전체)
    /// 못 막는다  빠져나가려고 작정한 코드 — 자기 pid 를 상위
    ///            cgroup.procs 에 써서 나갈 수 있다(테스트로 확인)
    /// ```
    ///
    /// Windows 의 Job Object 도 같은 성격이다(소프트 제한, 호스트 보호
    /// 아님). `CLAUDE.md` §0.4 대로, 여기서 그 이상을 주장하지 않는다.
    pub(super) fn execute(
        spec: &ExecutionSpec,
        policy: &ExecutionPolicy,
        on_started: impl FnOnce(super::WorkloadStopper),
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let create = gputeer_runtime_linux::SpawnSpec {
            program: spec.entrypoint.clone().into(),
            args: spec.args.iter().map(|a| a.clone().into()).collect(),
            current_dir: policy.capture_dir.clone(),
            stdout_path: policy
                .capture_dir
                .as_ref()
                .map(|dir| dir.join(STDOUT_FILENAME)),
            stderr_path: policy
                .capture_dir
                .as_ref()
                .map(|dir| dir.join(STDERR_FILENAME)),
        };

        // ★ cgroup 이름은 attempt 별로 갈라야 한다. 같은 이름을 쓰면
        //   두 작업이 같은 cgroup 을 공유해 한쪽을 멈출 때 다른 쪽도
        //   죽는다 — 소유자가 A 를 멈췄는데 B 가 사라진다.
        let cgroup_name = super::derive_cgroup_name(&policy.isolation);

        let mut child = gputeer_runtime_linux::create_constrained_child(
            &create,
            policy.commit_limit_bytes,
            &cgroup_name,
            // ★ 루트로 **자동으로** 내려가지 않는다. 그러면 운영자가
            //   상위에 걸어 둔 CPU·메모리·PID 상한 밖으로 나간다 —
            //   Agent 가 자기 판단으로 할 일이 아니다.
            //
            //   운영자가 위임받은 subtree 를 지정하면 그것을 쓰고,
            //   없으면 "내 cgroup" 을 쓴다(위임이 없으면 거부된다).
            &match &policy.cgroup_parent {
                Some(path) => gputeer_runtime_linux::CgroupParent::Explicit(path.clone()),
                None => gputeer_runtime_linux::CgroupParent::Current,
            },
        )
        .map_err(|error| match error {
            gputeer_runtime_linux::CgroupError::SpawnFailed { detail } => {
                ExecutionError::SpawnFailed { detail }
            }
            other => ExecutionError::LimitNotApplied {
                detail: other.to_string(),
            },
        })?;

        // ★ 손잡이를 **기다리기 전에** 넘긴다. 자식은 이미 돌고 있으므로,
        //   여기서 넘기지 않으면 `wait()` 에 붙잡힌 동안 소유자가 멈출
        //   방법이 없다.
        on_started(super::WorkloadStopper {
            inner: child.stopper(),
        });

        // ★ 결함 144 (검수 68) — 전에는 `memory_limit_bytes().unwrap_or(정책)` 이라 읽기 · 해석 실패 사유가 사라졌다. 원문을 받아 사유를 남긴다.
        let (limit, limit_observation_error) = super::classify_linux_memory_limit(
            child.read_memory_max_file(),
            policy.commit_limit_bytes,
        );
        // ★ 결함 69 — 신호 종료를 -1 로 합성하지 않는다(`wait_status`). 전에는 -1 을 u32 로 옮겨
        //   OBSERVED_WITH_CODE / 4294967295 로 보고될 수 있었다.
        let exit = match child
            .wait_status()
            .map_err(|error| ExecutionError::WaitFailed {
                detail: error.to_string(),
            })? {
            gputeer_runtime_linux::ChildExit::Code(code) => super::ExitObserved::Code(code as u32),
            gputeer_runtime_linux::ChildExit::Signaled(signal) => super::ExitObserved::NoCode {
                detail: match signal {
                    Some(signal) => format!("신호 {signal} 로 끝났다 — 종료 코드가 없다"),
                    None => "신호로 끝났다(번호 미상) — 종료 코드가 없다".to_string(),
                },
            },
        };
        // ★ peak 는 `wait()` **뒤에** 읽는다. 자식이 살아 있는 동안 읽으면
        //   최종값이 아니다. cgroup 은 프로세스가 끝나도 우리가 지울
        //   때까지 남아 있으므로 여기서 읽을 수 있다.
        // 모르면 None — 전에는 0 으로 채웠다(`CLAUDE.md` §1).
        // ★ 결함 81 — 전에는 `peak_memory_bytes()` 가 부재 · 읽기 실패 · 해석 실패를 모두 None 으로 접어 사유가 사라졌다
        //   (`memory_observation_error` 가 리눅스에서 늘 None 이었다). 원문 결과를 받아 사유를 남긴다.
        // ★ 이 두 줄은 Windows 개발 기계에서 타입 검사를 못 한다 — 리눅스 실행 전까지 미검증이다.
        let (peak, peak_observation_error) =
            super::classify_linux_memory_peak(child.read_memory_peak_file());
        let memory_observation_error =
            super::join_observation_errors(limit_observation_error, peak_observation_error);

        Ok(ExecutionOutcome {
            exit,
            commit_limit_bytes: limit,
            peak_commit_bytes: peak,
            memory_observation_error,
        })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::{ExecutionError, ExecutionOutcome, ExecutionPolicy};
    use gputeer_protocol::execution_spec::ExecutionSpec;

    /// ★ **무방비로 실행하느니 거부한다.**
    ///
    /// Windows 는 Job Object, Linux 는 cgroup v2 로 상한을 건다. 그
    /// 둘이 아닌 플랫폼에는 연결된 강제 수단이 없다 — 연결되지 않은
    /// 강제를 "있다" 고 취급해 프로세스를 띄우면 `CLAUDE.md` §0.4 위반이다.
    pub(super) fn execute(
        _spec: &ExecutionSpec,
        _policy: &ExecutionPolicy,
        _on_started: impl FnOnce(super::WorkloadStopper),
    ) -> Result<ExecutionOutcome, ExecutionError> {
        Err(ExecutionError::UnsupportedPlatform {
            detail: "이 플랫폼에는 자원 상한 강제가 연결돼 있지 않다 — 상한 없이 실행하지 않는다"
                .into(),
        })
    }
}

/// 이 실행을 다른 실행과 구분하는 신원.
///
/// ★ 두 성분을 **문자열로 미리 합치지 않는다**(2026-08-30 독립 검수
///   지적). 초안은 `format!("{grant}-{attempt}")` 로 합친 뒤 해시했는데,
///   그러면 구분자가 성분 경계를 못 지킨다.
///
/// ```text
/// grant="g-a", attempt="b"    ->  "g-a-b"
/// grant="g",   attempt="a-b"  ->  "g-a-b"   ← 같아진다
/// ```
///
/// 이 저장소가 이미 두 번 배운 것이다 — `derive_replay_nonce` 와
/// `start_checkpoint_id()` 둘 다 길이 접두사를 쓴다. 여기서만 안 썼다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IsolationIdentity {
    pub grant_id: String,
    pub attempt_id: String,
}

/// cgroup 디렉터리 이름을 만든다. **자르거나 치환하지 않고 해시한다.**
///
/// # 왜 다듬으면 안 되는가
///
/// ★ 2026-08-30 독립 검수가 실제 충돌 반례를 냈다. 초안은 허용 문자만
///   남기고 64자로 잘랐는데, 그러면 서로 다른 실행이 같은 이름이 된다.
///
/// ```text
/// grant=g, attempt=a.b   ->  g-a_b
/// grant=g, attempt=a_b   ->  g-a_b     ← 같아진다
/// 64자 뒤만 다른 두 값   ->  같아진다
/// ```
///
/// 그리고 그건 단순히 cgroup 을 공유하는 것으로 끝나지 않았다 —
/// `runtime-linux` 가 기존 cgroup 을 죽이고 다시 만들었으므로,
/// **B 를 시작하면 A 가 죽었다.** 소유자가 아닌 것이 남의 작업을
/// 끝내는 것은 `CLAUDE.md` §0.1 이 가장 앞에서 막는 일이다.
///
/// 그래서 다듬지 않는다. 각 성분에 길이 접두사를 붙여 BLAKE3 로
/// 해시하고 고정 길이 hex 를 쓴다 — 문자 집합·길이·성분 경계 문제가
/// 동시에 사라진다.
///
/// ★ `derive_replay_nonce` 와 같은 이유로 **단사 함수라고 주장하지
///   않는다.** 해시를 자르는 한 그건 사실이 아니다. 정확히는
///   "128비트 충돌 저항에 의존하는 결정적 이름" 이다.
///
/// # 읽기 어려워지는 것은 감수한다
///
/// 사람이 `/sys/fs/cgroup` 에서 어느 작업인지 바로 못 알아본다. 그
/// 대가로 남의 작업을 죽이지 않는다 — 바꿀 만한 거래다. 어느 attempt
/// 인지는 Agent 로그가 같이 남긴다.
#[cfg(target_os = "linux")]
fn derive_cgroup_name(isolation: &IsolationIdentity) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gputeer/v1/cgroup-name");
    for component in [&isolation.grant_id, &isolation.attempt_id] {
        hasher.update(&(component.len() as u64).to_be_bytes());
        hasher.update(component.as_bytes());
    }
    hasher.finalize().to_hex()[..32].to_string()
}

#[cfg(all(test, target_os = "linux"))]
mod cgroup_name_tests {
    use super::{derive_cgroup_name, IsolationIdentity};

    fn name(grant: &str, attempt: &str) -> String {
        derive_cgroup_name(&IsolationIdentity {
            grant_id: grant.to_string(),
            attempt_id: attempt.to_string(),
        })
    }

    /// ★ 검수가 든 실제 충돌 반례가 이제 안 나오는가.
    #[test]
    fn the_reported_name_collisions_are_gone() {
        // 1라운드 지적 — 문자 치환·절단
        assert_ne!(name("g", "a.b"), name("g", "a_b"));
        let long_a = format!("{}X", "z".repeat(70));
        let long_b = format!("{}Y", "z".repeat(70));
        assert_ne!(name("g", &long_a), name("g", &long_b));

        // 2라운드 지적 — 성분 경계
        assert_ne!(name("g-a", "b"), name("g", "a-b"));
        assert_ne!(name("ab", "c"), name("a", "bc"));
        assert_ne!(name("", "abc"), name("abc", ""));
    }

    /// 이름이 cgroup 디렉터리로 쓸 수 있는 모양인가.
    #[test]
    fn the_name_is_always_a_valid_directory_name() {
        for (g, a) in [
            ("", ""),
            ("a/b", "c"),
            ("한글", "x"),
            (&"x".repeat(500), "y"),
        ] {
            let n = name(g, a);
            assert_eq!(n.len(), 32, "길이가 고정이 아니다: {n}");
            assert!(
                n.chars().all(|c| c.is_ascii_hexdigit()),
                "hex 가 아닌 문자가 있다: {n}"
            );
        }
    }

    /// 같은 입력은 같은 이름을 낸다 — 재시도가 같은 자리를 쓴다.
    #[test]
    fn the_name_is_deterministic() {
        assert_eq!(name("g", "a"), name("g", "a"));
    }
}
