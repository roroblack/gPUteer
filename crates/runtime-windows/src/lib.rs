//! Windows Job Object 로 자식 프로세스의 커밋 메모리 상한을 실제로 건다.
//!
//! `crates/runtime-policy/src/vram.rs` 는 강제 가능성을 **판정만** 한다 —
//! 그 모듈 문서가 스스로 이렇게 적어 뒀다:
//!
//! > `runtime-windows` 가 생기면 그쪽이 이 판정을 부르고 실제로
//! > `CreateJobObject`/`SetInformationJobObject` 를 호출해야 한다.
//!
//! 이 크레이트가 그 "그쪽"이다. `windows_commit_cap()` 이 계산한
//! `approx_vram_max_bytes` 를 실제 Win32 Job Object 커밋 상한으로
//! 연결한다.
//!
//! # 이것이 시스템 설정 변경이 아닌 이유
//!
//! `CreateJobObjectW(NULL, NULL)` 는 **이름 없는** Job Object 를 만든다.
//! Job 은 여기에 할당된 프로세스 그룹의 속성만 제어하고, 마지막 핸들이
//! 닫히고 연결된 프로세스가 모두 끝나면 Job 자체도 사라진다 — 방화벽
//! 규칙 추가나 OS 기능 활성화처럼 재부팅이나 시스템 전역 상태 변경을
//! 요구하지 않는다. 이 크레이트를 만드는 시점(2026-08-18)에는 이
//! 저장소가 실제로 Windows 에서 이 코드를 실행해 본 적이 없다 —
//! **"일반 사용자 권한으로 성공하는가" 는 확인 안 됨이다.**
//! (설계 검토 — 코덱스, `p58` 프롬프트.)
//!
//! # 이것이 실제로 보장하는 것 / 보장하지 않는 것
//!
//! `crates/runtime-policy/src/vram.rs::VramEnforcement::guarantees_hard_limit()`
//! 는 `WindowsCommitCap` 도 `false` 를 반환한다 — 이 크레이트가 그
//! 정직성을 뒤집지 않는다.
//!
//! ```text
//! 보장하는 것
//!   Job 에 성공적으로 할당된 프로세스들의 committed virtual memory
//!   합계에 상한을 건다. 상한을 넘는 commit 시도는 실패해야 한다.
//!   자식이 실제로 실행되기(주 스레드가 코드를 돌리기) 전에 이미
//!   Job 제한이 걸려 있다 — CREATE_SUSPENDED 로 경합을 없앤다.
//!
//! 보장하지 않는 것
//!   GPU VRAM quota 그 자체 — 이건 committed **RAM** 상한이다.
//!   CUDA allocator·WDDM 드라이버 내부 할당 상한.
//!   Job 에서 벗어난 프로세스(CREATE_BREAKAWAY_FROM_JOB 등).
//!   호스트 전체 메모리 사용량.
//!   VRAM 바이트 수와 committed RAM 의 정확한 1:1 대응
//!   (근사치다 — P0-06 실측 예약분을 뺀 값일 뿐이다).
//! ```

#[cfg(windows)]
mod beneath;
#[cfg(windows)]
pub use beneath::{open_artifact, open_beneath, open_beneath_read_only};

// ★ **완결된 짝으로 넣는다.** 위의 `#[cfg(windows)]` 는 `mod beneath` 의
//   것이고, 그 사이에 끼워 넣으면 소속이 조용히 바뀐다 — 이 저장소가
//   여덟 번 겪은 실수다(2026-09-01 커밋 040b5d0).
#[cfg(windows)]
pub mod appcontainer;

#[cfg(windows)]
mod windows_impl {
    use std::ffi::{OsStr, OsString};
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::ptr;

    use gputeer_runtime_policy::vram::{windows_commit_cap, VramEnforcement};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_JOB_MEMORY,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
        InitializeProcThreadAttributeList, ResumeThread, TerminateProcess,
        UpdateProcThreadAttribute, WaitForSingleObject, CREATE_SUSPENDED,
        CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
        LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
    };

    /// `CreateProcessW` 호출에 필요한 최소 입력.
    ///
    /// `std::process::Command` 는 안정 Rust 에서 프로세스를 즉시
    /// 시작하며, 시작된 자식의 주 스레드를 나중에 멈춰 둘 방법(안정
    /// API 로 스레드 핸들을 얻는 방법)이 없다 — `ChildExt::main_thread_handle()`
    /// 은 nightly 전용이다. `CREATE_SUSPENDED` 로 자식을 만들고 Job
    /// 할당이 끝난 뒤에야 재개해야 "자식이 상한 걸리기 전에 이미 메모리를
    /// 커밋했다" 는 경합을 없앨 수 있으므로, `CreateProcessW` 를 직접
    /// 부른다(설계 검토 — 코덱스, `p58` 프롬프트).
    #[derive(Debug, Clone)]
    pub struct CreateProcessSpec {
        /// 실행 파일의 절대 경로를 권장한다.
        pub application_name: OsString,
        /// Windows 명령줄 인용 규칙에 맞게 이미 구성된 전체 명령줄
        /// (`application_name` 자신도 첫 토큰으로 포함해야 한다).
        pub command_line: OsString,
        pub current_dir: Option<PathBuf>,
        /// 자식의 표준 출력을 받을 파일. `None` 이면 받지 않는다.
        ///
        /// ★ 파이프가 아니라 **파일**이다. 파이프로 받으면 부모가
        ///   계속 빨아내야 하고, 안 빨아내는 사이 버퍼가 차면 자식이
        ///   쓰기에서 멈춘다 — 부모는 종료를 기다리고 자식은 쓰기를
        ///   기다리는 고전적인 교착이다. 어차피 결과를 체크포인트에
        ///   파일로 남겨야 하므로 처음부터 파일로 받는다.
        pub stdout_path: Option<PathBuf>,
        /// 자식의 표준 오류를 받을 파일. `None` 이면 받지 않는다.
        pub stderr_path: Option<PathBuf>,
    }

    /// `std::process::Child` 대신 쓰는 최소 Windows 프로세스+Job 핸들.
    ///
    /// Job 핸들을 갖고 있는 것이 이 구조체의 목적이 아니다 — Job 은
    /// 마지막 핸들이 닫혀도 **연결된 프로세스가 살아 있는 동안** 유지된다.
    /// 그래도 핸들을 들고 있는 이유는 `Drop` 에서 확실히 정리하기 위해서다.
    pub struct ConstrainedChild {
        process: windows_sys::Win32::Foundation::HANDLE,
        job: windows_sys::Win32::Foundation::HANDLE,
    }

    impl ConstrainedChild {
        /// 자식이 끝날 때까지 기다린다.
        pub fn wait(&self) -> std::io::Result<()> {
            let result = unsafe { WaitForSingleObject(self.process, INFINITE) };
            if result == u32::MAX {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        /// 자식이 끝날 때까지 기다린 **뒤** 종료 코드를 읽는다 — 순서를 한 호출로 강제한다(결함 77, 재검수 57).
        ///
        /// 바깥 `Err` 는 기다리기 자체의 실패(종료를 관측하지 못했다), 안쪽 `Err` 는 종료는 관측했는데 코드를 못 읽은 것이다.
        ///
        /// ★ 기다리기가 끝났으면 259 도 **실제 종료 코드**다. [`Self::exit_code`] 의 STILL_ACTIVE 가드는 wait 전에 부른
        ///   실수를 막는 것이라, wait 뒤에 걸면 실제 값을 버린다 — 재검수 57 이 `cmd.exe /d /c exit 259` 로
        ///   WaitForSingleObject=0 · GetExitCodeProcess 성공 · 259 를 실측했고, 그 값이 "코드 없음" 으로 보고되고 있었다.
        pub fn wait_then_exit_code(&self) -> std::io::Result<std::io::Result<u32>> {
            self.wait()?;
            let mut code: u32 = 0;
            let ok = unsafe {
                windows_sys::Win32::System::Threading::GetExitCodeProcess(self.process, &mut code)
            };
            if ok == 0 {
                return Ok(Err(std::io::Error::last_os_error()));
            }
            Ok(Ok(code))
        }

        /// 자식의 종료 코드를 읽는다. **`wait()` 이 끝난 뒤에만 부른다.**
        ///
        /// ★ 종료 보고에는 [`Self::wait_then_exit_code`] 를 쓴다 — 이 함수는 실제 종료 코드 259 를 오류로 바꾼다(결함 77).
        ///
        /// ★ 아직 살아 있는 프로세스에 `GetExitCodeProcess` 를 부르면
        ///   `STILL_ACTIVE`(259)가 돌아온다. 그걸 진짜 종료 코드로 쓰면
        ///   "259 로 끝났다" 는 거짓 사실이 생기므로, 그 값을 만나면
        ///   종료 코드가 아니라 **오류**로 보고한다.
        ///
        ///   259 로 실제로 끝나는 프로세스와 구분되지 않는다는 한계가
        ///   있다 — Win32 API 자체의 한계이며 이 함수가 만든 것이 아니다.
        ///   그래서 `wait()` 뒤에만 부르라는 계약을 문서로 못박는다.
        pub fn exit_code(&self) -> std::io::Result<u32> {
            const STILL_ACTIVE: u32 = 259;
            let mut code: u32 = 0;
            let ok = unsafe {
                windows_sys::Win32::System::Threading::GetExitCodeProcess(self.process, &mut code)
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if code == STILL_ACTIVE {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "GetExitCodeProcess 가 STILL_ACTIVE(259) 를 반환했다 — wait() 뒤에 불러야 한다",
                ));
            }
            Ok(code)
        }

        /// 이 Job 에 연결된 프로세스들이 지금까지 커밋한 메모리의
        /// 최댓값(`PeakJobMemoryUsed`)과, 걸어 둔 상한(`JobMemoryLimit`)
        /// 을 함께 반환한다 — 실측 검증용(`peak <= limit` 을 확인한다).
        pub fn query_memory_limits(&self) -> std::io::Result<(usize, usize)> {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            let mut returned = 0u32;
            let ok = unsafe {
                windows_sys::Win32::System::JobObjects::QueryInformationJobObject(
                    self.job,
                    JobObjectExtendedLimitInformation,
                    &mut info as *mut _ as *mut _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    &mut returned,
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok((info.PeakJobMemoryUsed, info.JobMemoryLimit))
        }

        /// 이 Job 에 지금 속한 프로세스 ID 들.
        ///
        /// ★ 테스트가 "이 테스트가 만든 프로세스" 를 **정확히** 가리키게
        ///   하려고 만들었다(2026-08-29, 독립 검수 지적). 시스템 전체의
        ///   `PING.EXE` 개수를 세면 다른 사람이 돌린 ping 이 섞여
        ///   귀속이 엄밀하지 않다.
        pub fn process_ids(&self) -> std::io::Result<Vec<u32>> {
            // 목록은 가변 길이라 넉넉히 잡는다. 이 저장소의 작업은
            // 프로세스 트리가 작다 — 넘치면 오류로 알린다.
            const CAPACITY: usize = 256;
            #[repr(C)]
            struct ProcessIdList {
                number_of_assigned_processes: u32,
                number_of_process_ids_in_list: u32,
                process_id_list: [usize; CAPACITY],
            }
            let mut info: ProcessIdList = unsafe { std::mem::zeroed() };
            let mut returned = 0u32;
            let ok = unsafe {
                windows_sys::Win32::System::JobObjects::QueryInformationJobObject(
                    self.job,
                    windows_sys::Win32::System::JobObjects::JobObjectBasicProcessIdList,
                    &mut info as *mut _ as *mut _,
                    size_of::<ProcessIdList>() as u32,
                    &mut returned,
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let count = info.number_of_process_ids_in_list as usize;
            if count > CAPACITY {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::OutOfMemory,
                    "Job 의 프로세스 수가 이 버퍼보다 많다",
                ));
            }
            Ok(info.process_id_list[..count]
                .iter()
                .map(|id| *id as u32)
                .collect())
        }

        /// 이 자식을 **다른 스레드에서** 강제 종료할 수 있는 손잡이를 만든다.
        ///
        /// # 왜 이게 있어야 하는가
        ///
        /// `CLAUDE.md` §0.1 은 다른 어떤 규칙보다 앞에 이렇게 정한다 —
        /// "노드 소유자는 언제든 자기 GPU 를 즉시 비울 수 있어야 한다.
        /// 네트워크가 끊겨도, quorum 이 없어도, Coordinator 가 죽어도."
        ///
        /// 그런데 `wait()` 은 자식이 끝날 때까지 그 스레드를 붙잡는다.
        /// 붙잡힌 스레드에서는 아무것도 못 하므로, **멈추라고 말할 수 있는
        /// 다른 손잡이**가 없으면 그 규칙을 지킬 수단 자체가 없다.
        ///
        /// # 왜 프로세스가 아니라 Job 을 죽이는가
        ///
        /// 자식이 손자를 만들었을 수 있다(예: `cmd /c python train.py`).
        /// `TerminateProcess` 는 그 프로세스 하나만 죽여 손자를 고아로
        /// 남긴다 — GPU 를 쥔 채로. Job Object 는 이미 트리 전체를
        /// 담고 있으므로 `TerminateJobObject` 가 트리를 한 번에 끝낸다.
        /// `state-machines.md` §3 의 `WATCHDOG_KILLED` effect 도
        /// "process tree 종료" 라고 적혀 있다.
        ///
        /// # 핸들을 복제하는 이유
        ///
        /// ★ Job 핸들을 그대로 넘기면 `ConstrainedChild` 가 먼저 drop 될 때
        ///   `CloseHandle` 이 불려 손잡이가 이미 닫힌 핸들을 가리키게 된다.
        ///   그 핸들 값은 나중에 **다른 객체에 재사용될 수 있으므로**,
        ///   운 나쁘면 남의 Job 을 죽인다. `DuplicateHandle` 로 각자
        ///   자기 몫을 갖게 해서 수명을 분리한다.
        pub fn stopper(&self) -> std::io::Result<JobStopper> {
            let process = unsafe { GetCurrentProcess() };
            let mut duplicated: HANDLE = std::ptr::null_mut();
            let ok = unsafe {
                DuplicateHandle(
                    process,
                    self.job,
                    process,
                    &mut duplicated,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(JobStopper { job: duplicated })
        }
    }

    /// 실행 중인 작업을 **밖에서** 끝낼 수 있는 손잡이.
    ///
    /// `ConstrainedChild` 와 수명이 독립적이다 — 어느 쪽이 먼저 사라져도
    /// 다른 쪽이 닫힌 핸들을 쓰지 않는다.
    pub struct JobStopper {
        job: HANDLE,
    }

    // SAFETY: Win32 커널 핸들은 프로세스 전역이고 스레드에 묶이지
    // 않는다. 이 타입은 그 값을 옮기기만 하며, 실제 조작은
    // `TerminateJobObject` 한 번뿐이다(스레드 안전한 호출이다).
    unsafe impl Send for JobStopper {}
    unsafe impl Sync for JobStopper {}

    impl JobStopper {
        /// 테스트 전용 — 아무 Job 도 가리키지 않는 손잡이.
        ///
        /// ★ `terminate()` 는 반드시 **실패**한다. 성공을 돌려주면
        ///   이걸 쓰는 상위 테스트가 "멈췄다" 를 통과시켜 공허해진다.
        pub fn inert_for_test() -> Self {
            Self {
                job: std::ptr::null_mut(),
            }
        }

        /// Job 에 속한 **모든** 프로세스를 즉시 끝낸다.
        ///
        /// ★ 이건 정중한 요청이 아니라 강제 종료다. 자식은 정리할
        ///   기회를 얻지 못하며 쓰던 파일이 중간 상태로 남을 수 있다.
        ///   그래서 `CLAUDE.md` §0.1 은 "강제 종료 시 손실 범위를 미리
        ///   계산해 보여준다" 고 요구한다 — 얼마를 잃는지 모른 채
        ///   누르게 하지 않는다. 그 계산은 이 함수의 책임이 아니라
        ///   호출부의 책임이다.
        ///
        /// 이미 끝난 Job 에 불러도 성공한다(멱등).
        pub fn terminate(&self, exit_code: u32) -> std::io::Result<()> {
            if self.job.is_null() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "이 손잡이는 어떤 Job 도 가리키지 않는다",
                ));
            }
            let ok = unsafe { TerminateJobObject(self.job, exit_code) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for JobStopper {
        fn drop(&mut self) {
            unsafe {
                if !self.job.is_null() {
                    CloseHandle(self.job);
                }
            }
        }
    }

    impl Drop for ConstrainedChild {
        fn drop(&mut self) {
            unsafe {
                if !self.process.is_null() {
                    CloseHandle(self.process);
                }
                if !self.job.is_null() {
                    CloseHandle(self.job);
                }
            }
        }
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    /// `CreateProcessW` 의 `lpCommandLine` 에 넣을 명령줄을 MSVC 인자
    /// 분해 규칙에 맞게 조립한다(`exe` 가 첫 토큰). `std::process::Command`
    /// 는 이 조립을 내부적으로 해 주지만 공개 API 가 아니다 — 여기서는
    /// `CreateProcessW` 를 직접 부르므로 우리가 직접 만든다.
    ///
    /// ★ 2026-08-18 수정(코덱스 독립 검수 · `p59` 프롬프트).
    ///   초안은 문자당 백슬래시 개수를 `n+1`/`n`(각각 따옴표 앞·끝)로
    ///   출력했다 — MSVC 규칙은 **따옴표 바로 앞의 백슬래시를 2배로
    ///   만든 뒤 하나를 더** 붙여야 한다(`2n+1`), 문자열 끝에서 닫는
    ///   따옴표 앞이면 **그냥 2배**(`2n`)다. 예를 들어 인자가
    ///   `C:\foo\` 로 끝나면 초안은 백슬래시를 원래 개수 그대로
    ///   출력해 닫는 따옴표를 이스케이프해 버렸다(명령줄이 깨진다).
    ///   `OsStr::to_string_lossy()` 로 UTF-16 을 문자로 왕복하던 것도
    ///   비정상 서로게이트 페어가 있는 경로/인자를 손상시킬 수 있어
    ///   그만두고, UTF-16 코드 유닛 위에서 직접 조립한다.
    pub fn quote_command_line(exe: &OsStr, args: &[&OsStr]) -> OsString {
        use std::os::windows::ffi::OsStringExt;

        const SPACE: u16 = b' ' as u16;
        const TAB: u16 = b'\t' as u16;
        const QUOTE: u16 = b'"' as u16;
        const BACKSLASH: u16 = b'\\' as u16;

        let mut out: Vec<u16> = Vec::new();
        for (i, part) in std::iter::once(exe).chain(args.iter().copied()).enumerate() {
            if i > 0 {
                out.push(SPACE);
            }
            let units: Vec<u16> = part.encode_wide().collect();
            let needs_quotes =
                units.is_empty() || units.iter().any(|&c| c == SPACE || c == TAB || c == QUOTE);
            if !needs_quotes {
                out.extend_from_slice(&units);
                continue;
            }

            out.push(QUOTE);
            let mut backslashes = 0usize;
            for &c in &units {
                if c == BACKSLASH {
                    backslashes += 1;
                    continue;
                }
                if c == QUOTE {
                    // ★ 따옴표를 실제로 출력하기 직전 — 앞선 백슬래시를
                    //   2배로 만들어야(2n) 그 백슬래시들이 따옴표를 먹지
                    //   않고, 그 뒤에 이스케이프용 백슬래시 하나를 더
                    //   붙여야(+1) 이 따옴표 자체가 문자로 살아남는다.
                    out.extend(std::iter::repeat(BACKSLASH).take(backslashes * 2 + 1));
                    out.push(QUOTE);
                } else {
                    out.extend(std::iter::repeat(BACKSLASH).take(backslashes));
                    out.push(c);
                }
                backslashes = 0;
            }
            // ★ 인자 끝에 남은 백슬래시는 그 뒤에 우리가 붙일 **닫는**
            //   따옴표를 이스케이프하지 않도록 2배로 만든다(2n) —
            //   문자로서의 따옴표가 아니므로 +1 은 붙이지 않는다.
            out.extend(std::iter::repeat(BACKSLASH).take(backslashes * 2));
            out.push(QUOTE);
        }
        OsString::from_wide(&out)
    }

    /// `windows_commit_cap()` 의 RAM 정책 판정을 실제 Job Object 커밋
    /// 상한으로 그대로 연결한다. `VramEnforcement::NoQuotaMechanism` 이
    /// 나오면(이 함수는 항상 `WindowsCommitCap` 을 반환하므로 실제로는
    /// 도달하지 않지만, enum 이 커버된 채로 남도록) 오류로 거부한다.
    pub fn create_constrained_child_for_ram_limit(
        spec: &CreateProcessSpec,
        ram_limit_bytes: u64,
    ) -> std::io::Result<ConstrainedChild> {
        match windows_commit_cap(ram_limit_bytes) {
            VramEnforcement::WindowsCommitCap {
                approx_vram_max_bytes,
            } => create_constrained_child(spec, approx_vram_max_bytes),
            VramEnforcement::NoQuotaMechanism => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "windows_commit_cap 이 강제 수단 없음을 반환했다",
            )),
        }
    }

    /// 자식이 상속할 수 있는 쓰기 전용 파일 핸들을 만든다.
    ///
    /// ★ `bInheritHandle = TRUE` 로 만들지만 그것만으로는 안전하지
    ///   않다. `CreateProcessW` 에 `bInheritHandles = TRUE` 를 주면 이
    ///   프로세스의 **상속 가능한 모든 핸들**이 넘어간다 — 남의
    ///   코드를 돌리는 이 저장소에서는 받아들일 수 없는 위험이다.
    ///   그래서 호출부는 `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` 로 상속
    ///   대상을 **정확히 이 두 핸들로** 제한한다.
    fn create_inheritable_output_file(path: &std::path::Path) -> std::io::Result<HANDLE> {
        let wide_path = wide(path.as_os_str());
        let mut security: SECURITY_ATTRIBUTES = unsafe { std::mem::zeroed() };
        security.nLength = size_of::<SECURITY_ATTRIBUTES>() as u32;
        security.bInheritHandle = 1;

        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                FILE_GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &security,
                CREATE_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        Ok(handle)
    }

    /// `CREATE_SUSPENDED` 로 자식을 만들고, Job Object 커밋 상한을 건
    /// 뒤에야 재개한다. 순서를 바꾸면 안 된다 — 재개를 먼저 하면 자식이
    /// Job 에 할당되기 전에 이미 메모리를 커밋할 수 있다.
    ///
    /// ```text
    /// CreateProcessW(CREATE_SUSPENDED)
    ///   -> CreateJobObjectW
    ///   -> SetInformationJobObject(JobObjectExtendedLimitInformation)
    ///   -> AssignProcessToJobObject
    ///   -> ResumeThread
    /// ```
    pub fn create_constrained_child(
        spec: &CreateProcessSpec,
        commit_limit_bytes: u64,
    ) -> std::io::Result<ConstrainedChild> {
        create_constrained_child_suspended(spec, commit_limit_bytes)?.resume()
    }

    /// `create_constrained_child` 와 같지만 **재개하지 않고** 돌려준다.
    ///
    /// 호출부가 정지 손잡이를 등록한 뒤 `resume()` 을 부른다.
    pub fn create_constrained_child_suspended(
        spec: &CreateProcessSpec,
        commit_limit_bytes: u64,
    ) -> std::io::Result<SuspendedChild> {
        if commit_limit_bytes == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "커밋 상한이 0이다",
            ));
        }
        let commit_limit = usize::try_from(commit_limit_bytes).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "커밋 상한이 이 플랫폼의 usize 범위를 넘는다",
            )
        })?;

        let application_name = wide(&spec.application_name);
        let mut command_line = wide(&spec.command_line);
        let current_dir = spec.current_dir.as_ref().map(|p| wide(p.as_os_str()));

        // 출력 파일 핸들을 먼저 열어둔다. 여기서 실패하면 아직
        // 프로세스가 없으므로 정리할 것도 없다.
        let mut inherited: Vec<HANDLE> = Vec::new();
        let stdout_handle = match spec.stdout_path.as_ref() {
            Some(path) => {
                let handle = create_inheritable_output_file(path)?;
                inherited.push(handle);
                Some(handle)
            }
            None => None,
        };
        let stderr_handle = match spec.stderr_path.as_ref() {
            Some(path) => match create_inheritable_output_file(path) {
                Ok(handle) => {
                    inherited.push(handle);
                    Some(handle)
                }
                Err(error) => {
                    if let Some(handle) = stdout_handle {
                        unsafe { CloseHandle(handle) };
                    }
                    return Err(error);
                }
            },
            None => None,
        };
        // 자식이 생기기 전이든 뒤이든, 이 함수가 나갈 때 부모 쪽
        // 복사본은 반드시 닫는다. 안 닫으면 파일이 계속 열린 채로 남아
        // 나중에 읽는 쪽이 잘린 내용을 볼 수 있다.
        let close_inherited = |handles: &[HANDLE]| {
            for handle in handles {
                unsafe { CloseHandle(*handle) };
            }
        };

        let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;

        // ★ 상속 목록을 이 두 핸들로 제한한다. 이게 없으면
        //   `bInheritHandles = TRUE` 가 이 프로세스의 모든 상속 가능
        //   핸들(SQLite 파일·소켓·락 파일 등)을 남의 코드에게 넘긴다.
        let mut attribute_buffer: Vec<u8> = Vec::new();
        let mut attribute_list: LPPROC_THREAD_ATTRIBUTE_LIST = ptr::null_mut();
        if !inherited.is_empty() {
            let mut size: usize = 0;
            // 첫 호출은 반드시 실패하면서 필요한 크기를 채운다 —
            // 그게 이 API 의 계약이므로 반환값을 오류로 읽지 않는다.
            unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
            if size == 0 {
                close_inherited(&inherited);
                return Err(std::io::Error::last_os_error());
            }
            attribute_buffer.resize(size, 0);
            attribute_list = attribute_buffer.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            if unsafe { InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut size) } == 0 {
                close_inherited(&inherited);
                return Err(std::io::Error::last_os_error());
            }
            let updated = unsafe {
                UpdateProcThreadAttribute(
                    attribute_list,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    inherited.as_ptr() as *const std::ffi::c_void,
                    std::mem::size_of_val(&inherited[..]),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if updated == 0 {
                let error = std::io::Error::last_os_error();
                unsafe { DeleteProcThreadAttributeList(attribute_list) };
                close_inherited(&inherited);
                return Err(error);
            }
            startup.lpAttributeList = attribute_list;
            startup.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdOutput = stdout_handle.unwrap_or(INVALID_HANDLE_VALUE);
            startup.StartupInfo.hStdError = stderr_handle.unwrap_or(INVALID_HANDLE_VALUE);
            startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        }

        let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let mut flags = CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT;
        if !inherited.is_empty() {
            flags |= EXTENDED_STARTUPINFO_PRESENT;
        }

        let created = unsafe {
            CreateProcessW(
                application_name.as_ptr(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                i32::from(!inherited.is_empty()),
                flags,
                ptr::null(),
                current_dir.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
                &startup as *const STARTUPINFOEXW as *const STARTUPINFOW,
                &mut process_info,
            )
        };
        if !attribute_list.is_null() {
            unsafe { DeleteProcThreadAttributeList(attribute_list) };
        }
        // 자식이 자기 복사본을 가졌으므로 부모 쪽은 지금 닫는다.
        close_inherited(&inherited);
        if created == 0 {
            return Err(std::io::Error::last_os_error());
        }

        // ★ 이 지점부터는 프로세스가 이미 존재한다(정지 상태). 뒤이은
        //   단계 중 하나라도 실패하면 좀비로 남기지 않고 반드시 죽인다.
        //
        // ★ 2026-08-18 수정(코덱스 독립 검수 · `p59` 프롬프트) —
        //   초안은 `TerminateProcess` 의 반환값을 확인하지 않고 바로
        //   핸들을 닫았다. 그러면 종료가 실제로 실패해도(예: 다른
        //   프로세스가 이미 그 PID 에 대한 디버그 권한을 쥐고 있는
        //   드문 경우) **정지 상태 프로세스가 영구히 남는다** — 핸들을
        //   닫아도 프로세스 자체는 안 죽는다. 여기서는 실패를 최소한
        //   눈에 보이게(`eprintln!`) 남긴다 — `create_constrained_child`
        //   는 이미 다른 1차 오류를 반환하는 중이라 두 오류를 하나의
        //   `io::Error` 로 합칠 표준 방법이 없다.
        let kill_and_close = || unsafe {
            if TerminateProcess(process_info.hProcess, 1) == 0 {
                eprintln!(
                    "gputeer-runtime-windows: TerminateProcess 실패(pid={}, error={}) — \
                     정지 상태 프로세스가 남아 있을 수 있다",
                    process_info.dwProcessId,
                    std::io::Error::last_os_error()
                );
            }
            CloseHandle(process_info.hProcess);
        };

        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            let err = std::io::Error::last_os_error();
            unsafe { CloseHandle(process_info.hThread) };
            kill_and_close();
            return Err(err);
        }

        // ★ `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 를 반드시 같이 건다
        //   (2026-08-29, 독립 검수가 찾은 차단 결함).
        //
        //   이 플래그가 없으면 Windows 는 **마지막 Job 핸들을 닫아도
        //   소속 프로세스를 죽이지 않는다.** 그러면 기동 직후 어떤
        //   이유로든(정지 손잡이 생성 실패·호출부 panic·에이전트
        //   종료) 핸들만 사라지고 **남의 코드는 남의 PC 에서
        //   계속 돌며 GPU 를 잡고 있는** 상태가 된다 — 제어할
        //   핸들은 없으면서. `CLAUDE.md` §0.1 이 가장 앞에 금지하는
        //   상태다.
        //
        //   이 플래그로 정리가 **RAII 로** 보장된다 — 오류 경로마다
        //   손으로 죽이는 것을 기억할 필요가 없다. 그래도 명시적인
        //   종료를 병행한다(아래 `stopper()` 실패 경로) — 의도가 코드에
        //   드러나야 다음 사람이 지우지 않는다.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation = JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..unsafe { std::mem::zeroed() }
        };
        limits.JobMemoryLimit = commit_limit;

        let set_ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if set_ok == 0 {
            let err = std::io::Error::last_os_error();
            unsafe {
                CloseHandle(job);
                CloseHandle(process_info.hThread);
            }
            kill_and_close();
            return Err(err);
        }

        let assigned = unsafe { AssignProcessToJobObject(job, process_info.hProcess) };
        if assigned == 0 {
            let err = std::io::Error::last_os_error();
            unsafe {
                CloseHandle(job);
                CloseHandle(process_info.hThread);
            }
            kill_and_close();
            return Err(err);
        }

        Ok(SuspendedChild {
            child: ConstrainedChild {
                process: process_info.hProcess,
                job,
            },
            main_thread: process_info.hThread,
        })
    }

    /// 상한까지 걸렸지만 **아직 돌지 않는** 자식.
    ///
    /// # 왜 이 단계를 밖으로 노출하는가
    ///
    /// ★ 2026-08-29, 독립 검수 지적. 전에는 기동과 재개가 한 덩어리였고,
    ///   호출부는 이미 돌기 시작한 자식을 받아 그 다음에야 소유자 화면에
    ///   등록했다. 짧은 작업은 **등록 전에 이미 끝나** 화면에 한 번도
    ///   안 보이거나, 반대로 도는 작업이 잠깐 누락될 수 있었다.
    ///
    ///   정지 상태로 받아 **등록을 먼저 끝내고 재개**하면 그 구간이
    ///   사라진다 — 소유자가 보는 목록이 사실과 어긋나지 않는다.
    pub struct SuspendedChild {
        child: ConstrainedChild,
        main_thread: windows_sys::Win32::Foundation::HANDLE,
    }

    impl SuspendedChild {
        /// 재개 전에 정지 손잡이를 먼저 만든다.
        pub fn stopper(&self) -> std::io::Result<JobStopper> {
            self.child.stopper()
        }

        /// 자식을 돌리기 시작한다.
        ///
        /// 실패하면 자식은 정지 상태 그대로 남는데, 여기서 `child` 를
        /// drop 하므로 `KILL_ON_JOB_CLOSE` 가 정리한다.
        pub fn resume(self) -> std::io::Result<ConstrainedChild> {
            let resumed = unsafe { ResumeThread(self.main_thread) };
            unsafe { CloseHandle(self.main_thread) };
            // `self` 가 여기서 Drop 을 다시 돌리지 않도록 먼저 꺼낸다.
            let child = unsafe { std::ptr::read(&self.child) };
            std::mem::forget(self);
            if resumed == u32::MAX {
                let err = std::io::Error::last_os_error();
                drop(child); // KILL_ON_JOB_CLOSE 가 정지 상태 자식을 끝낸다
                return Err(err);
            }
            Ok(child)
        }
    }

    impl Drop for SuspendedChild {
        /// 재개 없이 버려지면 정지 상태 자식을 남기지 않는다.
        ///
        /// `child` 가 자기 Drop 에서 핸들을 닫고, `KILL_ON_JOB_CLOSE` 가
        /// 정지 상태 프로세스를 끝낸다.
        fn drop(&mut self) {
            unsafe {
                if !self.main_thread.is_null() {
                    CloseHandle(self.main_thread);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 공백 없는 짧은 인자는 그대로 통과한다 — 불필요한 따옴표를
        /// 붙이지 않는다.
        #[test]
        fn simple_args_are_not_quoted() {
            let out = quote_command_line(
                OsStr::new("C:\\bin\\gputeer.exe"),
                &[OsStr::new("selftest")],
            );
            assert_eq!(out.to_str().unwrap(), "C:\\bin\\gputeer.exe selftest");
        }

        /// ★ 코덱스 독립 검수(2026-08-18, `p59`)가 잡은 버그의 회귀
        /// 테스트 — 인자가 백슬래시로 끝나고 공백을 포함하면(따옴표가
        /// 필요해진다), 그 trailing 백슬래시를 **2배**로 만들어야 닫는
        /// 따옴표를 이스케이프하지 않는다. 초안은 원래 개수 그대로
        /// 출력해 명령줄이 깨졌다.
        #[test]
        fn trailing_backslash_before_closing_quote_is_doubled() {
            let out = quote_command_line(OsStr::new("exe"), &[OsStr::new("C:\\Program Files\\")]);
            // 기대: 여는 따옴표 + "C:\Program Files" + 백슬래시 2개 + 닫는 따옴표.
            let mut expected = String::from("exe \"C:\\Program Files");
            expected.push('\\');
            expected.push('\\');
            expected.push('"');
            assert_eq!(out.to_str().unwrap(), expected);
        }

        /// ★ 같은 버그의 두 번째 회귀 테스트 — 인자 **중간**에 있는
        /// 리터럴 따옴표 앞의 백슬래시는 `2n+1` 개여야 한다(그 백슬래시들
        /// 자체를 이스케이프하면서, 뒤따르는 따옴표도 문자로 살려야
        /// 하므로 하나를 더 붙인다). 초안은 `n+1` 개만 출력했다.
        #[test]
        fn backslash_before_embedded_quote_uses_2n_plus_1_rule() {
            // 리터럴 인자: a \ " b  (공백을 포함시켜 강제로 따옴표 처리시킨다)
            let mut arg = String::from("a ");
            arg.push('\\');
            arg.push('"');
            arg.push('b');
            let out = quote_command_line(OsStr::new("exe"), &[OsStr::new(&arg)]);

            let mut expected = String::from("exe \"a ");
            expected.push('\\'); // backslashes*2+1 = 1*2+1 = 3개
            expected.push('\\');
            expected.push('\\');
            expected.push('"'); // 이스케이프된 리터럴 따옴표
            expected.push('b');
            expected.push('"'); // 닫는 따옴표
            assert_eq!(out.to_str().unwrap(), expected);
        }

        /// 빈 문자열 인자는 빈 채로 사라지면 안 된다 — `""` 로 감싸야
        /// 자식이 "인자가 있지만 비어 있다"를 알 수 있다.
        #[test]
        fn empty_arg_is_wrapped_in_quotes() {
            let out = quote_command_line(OsStr::new("exe"), &[OsStr::new("")]);
            assert_eq!(out.to_str().unwrap(), "exe \"\"");
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{
    create_constrained_child, create_constrained_child_for_ram_limit,
    create_constrained_child_suspended, quote_command_line, ConstrainedChild, CreateProcessSpec,
    JobStopper, SuspendedChild,
};

#[cfg(not(windows))]
compile_error!("gputeer-runtime-windows 는 Windows 전용이다 — Job Object 는 Win32 개념이다");
