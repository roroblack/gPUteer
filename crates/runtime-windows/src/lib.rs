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
mod windows_impl {
    use std::ffi::{OsStr, OsString};
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::ptr;

    use gputeer_runtime_policy::vram::{windows_commit_cap, VramEnforcement};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_JOB_MEMORY,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, ResumeThread, TerminateProcess, WaitForSingleObject,
        CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, INFINITE, PROCESS_INFORMATION,
        STARTUPINFOW,
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
    /// `CreateProcessW` 를 직접 부르므로 우리가 직접 만든다. 공백·따옴표가
    /// 없는 인자만 다루는 테스트 픽스처 호출이 대상이라 규칙을 전부
    /// 구현하지는 않지만, 공백/빈 문자열은 안전하게 감싼다.
    pub fn quote_command_line(exe: &OsStr, args: &[&OsStr]) -> OsString {
        let mut out = OsString::new();
        for (i, part) in std::iter::once(exe).chain(args.iter().copied()).enumerate() {
            if i > 0 {
                out.push(" ");
            }
            let needs_quotes = part.is_empty()
                || part.to_string_lossy().chars().any(|c| c == ' ' || c == '\t' || c == '"');
            if !needs_quotes {
                out.push(part);
                continue;
            }
            out.push("\"");
            let text = part.to_string_lossy();
            let mut backslashes = 0usize;
            for ch in text.chars() {
                match ch {
                    '\\' => backslashes += 1,
                    '"' => {
                        for _ in 0..=backslashes {
                            out.push("\\");
                        }
                        backslashes = 0;
                        out.push("\\\"");
                    }
                    _ => {
                        for _ in 0..backslashes {
                            out.push("\\");
                        }
                        backslashes = 0;
                        out.push(ch.to_string());
                    }
                }
            }
            for _ in 0..backslashes {
                out.push("\\");
            }
            out.push("\"");
        }
        out
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

        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

        let created = unsafe {
            CreateProcessW(
                application_name.as_ptr(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
                ptr::null(),
                current_dir.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
                &startup,
                &mut process_info,
            )
        };
        if created == 0 {
            return Err(std::io::Error::last_os_error());
        }

        // ★ 이 지점부터는 프로세스가 이미 존재한다(정지 상태). 뒤이은
        //   단계 중 하나라도 실패하면 좀비로 남기지 않고 반드시 죽인다.
        let kill_and_close = || unsafe {
            TerminateProcess(process_info.hProcess, 1);
            CloseHandle(process_info.hProcess);
        };

        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            let err = std::io::Error::last_os_error();
            unsafe { CloseHandle(process_info.hThread) };
            kill_and_close();
            return Err(err);
        }

        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation = JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_JOB_MEMORY,
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

        let resumed = unsafe { ResumeThread(process_info.hThread) };
        unsafe { CloseHandle(process_info.hThread) };
        if resumed == u32::MAX {
            let err = std::io::Error::last_os_error();
            unsafe { CloseHandle(job) };
            kill_and_close();
            return Err(err);
        }

        Ok(ConstrainedChild {
            process: process_info.hProcess,
            job,
        })
    }
}

#[cfg(windows)]
pub use windows_impl::{
    create_constrained_child, create_constrained_child_for_ram_limit, quote_command_line,
    ConstrainedChild, CreateProcessSpec,
};

#[cfg(not(windows))]
compile_error!("gputeer-runtime-windows 는 Windows 전용이다 — Job Object 는 Win32 개념이다");
