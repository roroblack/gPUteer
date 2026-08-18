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
pub use beneath::{open_artifact, open_beneath};

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
            let out = quote_command_line(
                OsStr::new("exe"),
                &[OsStr::new("C:\\Program Files\\")],
            );
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
    create_constrained_child, create_constrained_child_for_ram_limit, quote_command_line,
    ConstrainedChild, CreateProcessSpec,
};

#[cfg(not(windows))]
compile_error!("gputeer-runtime-windows 는 Windows 전용이다 — Job Object 는 Win32 개념이다");
