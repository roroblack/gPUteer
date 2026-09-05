//! `P0-02` 를 실제로 재기 위한 AppContainer 실행 primitive.
//!
//! 기준선 §32 의 `P0-02` 는 일곱 항목을 요구한다 — 프로파일 생성 ·
//! AppContainer 에서 Python 실행 · `torch.cuda.is_available()` · 작은 CUDA
//! 연산 · filesystem allowlist / outbound 제한 · GPU 드라이버 capability
//! 식별 · Create Process in Sandbox API 상태 재확인.
//!
//! ★ **이 모듈은 그중 "띄우는 것" 만 한다.** CUDA 가 되는지·capability 가
//!   무엇인지는 **띄워 보고 나서야** 알 수 있는 사실이고, 이 저장소는
//!   모르는 것을 적지 않는다.
//!
//! # 왜 Job Object 가 아니라 AppContainer 인가
//!
//! 2026-09-05 사용자가 방화벽을 실측했다 — `netsh advfirewall` 의
//! `program=` **경로 단위** 아웃바운드 차단은 실제로 동작한다
//! (`docs/evidence/_raw/방화벽_경로단위_차단_실측.txt`).
//!
//! 그런데 그것으로는 `NetworkPolicy` 를 강제할 수 없다. 경로 단위로 막으면
//! 남의 PC 에서 `python.exe` 를 **통째로** 막게 되고, 그러면 소유자의 다른
//! 작업까지 끊긴다 — `CLAUDE.md` §0.1(남의 하드웨어를 인질로 잡지 않는다)
//! 위반이다.
//!
//! AppContainer 는 프로세스마다 **고유한 SID** 를 준다. 그 SID 에 규칙을
//! 걸면 **그 Job 하나만** 막힌다. 그래서 방화벽 강제는 이것이 선행이다.
//!
//! # 이 모듈이 **하지 않는** 것
//!
//! ★ **capability 를 지어내지 않는다.** GPU 드라이버 접근에 무엇이
//!   필요한지는 아직 아무도 모른다(`P0-02` 가 그걸 식별하라고 요구한다).
//!   호출부가 준 것만 넣고, 안 주면 **빈 목록**으로 띄운다 — 그 상태에서
//!   무엇이 실패하는지가 곧 답이다.
//!
//! ★ **네트워크를 막지 않는다.** AppContainer 는 기본적으로 네트워크
//!   capability 가 없어 아웃바운드가 막히지만, 그것을 이 모듈이 "강제한다"
//!   고 말하지 않는다 — 실측 전이다.
//!
//! ★ **정리를 보장하지 않는다.** [`AppContainerProfile`] 은 `Drop` 에서
//!   프로파일을 지우려 시도하지만, 프로세스가 죽으면 `Drop` 이 안 돈다.
//!   프로파일은 사용자 레지스트리에 남고 다음 실행에서 재사용된다
//!   (같은 이름이면 `ERROR_ALREADY_EXISTS` 이고 SID 는 같다).

#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

/// AppContainer 를 만들거나 그 안에서 프로세스를 띄우다 실패한 이유.
///
/// ★ Win32 오류 코드를 그대로 실어 보낸다 — `P0-02` 는 "실패했다" 가
///   아니라 **무엇이 왜 실패했는가**를 기록해야 한다.
#[derive(Debug, thiserror::Error)]
pub enum AppContainerError {
    #[error("AppContainer 프로파일을 만들지 못했다({name:?}): HRESULT 0x{hresult:08X}")]
    CreateProfile { name: String, hresult: i32 },

    #[error("AppContainer SID 를 얻지 못했다({name:?}): HRESULT 0x{hresult:08X}")]
    DeriveSid { name: String, hresult: i32 },

    #[error("프로세스 속성 목록을 준비하지 못했다: Win32 오류 {code}")]
    AttributeList { code: u32 },

    #[error("AppContainer 안에서 프로세스를 띄우지 못했다: Win32 오류 {code}")]
    Spawn { code: u32 },

    #[error("자식을 기다리지 못했다: Win32 오류 {code}")]
    Wait { code: u32 },
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// 살아 있는 AppContainer 프로파일. `Drop` 에서 지우려 **시도**한다.
///
/// ★ 이름을 호출부가 정한다. 같은 이름을 다시 만들면 Windows 는
///   `ERROR_ALREADY_EXISTS` 를 주고 **SID 는 같다** — 그 경우를 실패로
///   보지 않고 기존 SID 를 조회해 쓴다.
pub struct AppContainerProfile {
    name: String,
    sid: *mut core::ffi::c_void,
}

impl AppContainerProfile {
    /// 프로파일을 만들고(또는 이미 있으면 그 SID 를 찾아) 연다.
    ///
    /// `capabilities` 는 **호출부가 준 것만** 들어간다. 빈 목록이 정직한
    /// 기본값이다 — GPU 드라이버가 무엇을 요구하는지 아직 모르기 때문이다.
    pub fn create(name: &str, display: &str, description: &str) -> Result<Self, AppContainerError> {
        use windows_sys::Win32::Security::Isolation::{
            CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
        };

        let wname = wide(name);
        let wdisplay = wide(display);
        let wdesc = wide(description);
        let mut sid: *mut core::ffi::c_void = std::ptr::null_mut();

        // ★ `S_OK` 가 아니어도 끝이 아니다 — 이미 있으면 SID 를 따로 얻는다.
        let hr = unsafe {
            CreateAppContainerProfile(
                wname.as_ptr(),
                wdisplay.as_ptr(),
                wdesc.as_ptr(),
                std::ptr::null(),
                0,
                &mut sid,
            )
        };
        if hr < 0 {
            // HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS) == 0x800700B7
            if hr != 0x8007_00B7u32 as i32 {
                return Err(AppContainerError::CreateProfile {
                    name: name.to_string(),
                    hresult: hr,
                });
            }
            let hr2 =
                unsafe { DeriveAppContainerSidFromAppContainerName(wname.as_ptr(), &mut sid) };
            if hr2 < 0 {
                return Err(AppContainerError::DeriveSid {
                    name: name.to_string(),
                    hresult: hr2,
                });
            }
        }

        Ok(Self {
            name: name.to_string(),
            sid,
        })
    }

    /// 이 컨테이너의 SID. 방화벽 규칙을 **이 Job 하나에만** 걸 때 쓴다.
    pub fn sid(&self) -> *mut core::ffi::c_void {
        self.sid
    }

    /// 사람이 읽는 SID 문자열(`S-1-15-2-...`).
    ///
    /// ★ `netsh advfirewall` 규칙에 넣을 수 있는 형태다 — 다만 **이 모듈은
    ///   규칙을 만들지 않는다**(시스템 설정 변경).
    pub fn sid_string(&self) -> Option<String> {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;

        let mut raw: *mut u16 = std::ptr::null_mut();
        let ok = unsafe { ConvertSidToStringSidW(self.sid, &mut raw) };
        if ok == 0 || raw.is_null() {
            return None;
        }
        let mut len = 0usize;
        while unsafe { *raw.add(len) } != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw, len) });
        unsafe { LocalFree(raw as *mut core::ffi::c_void) };
        Some(text)
    }
}

impl Drop for AppContainerProfile {
    fn drop(&mut self) {
        use windows_sys::Win32::Security::FreeSid;
        use windows_sys::Win32::Security::Isolation::DeleteAppContainerProfile;

        let wname = wide(&self.name);
        // ★ 실패해도 할 수 있는 게 없다. 다만 **조용히 넘기지는 않는다** —
        //   stderr 로 남겨 운영자가 남은 프로파일을 알 수 있게 한다.
        let hr = unsafe { DeleteAppContainerProfile(wname.as_ptr()) };
        if hr < 0 {
            eprintln!(
                "APPCONTAINER_CLEANUP_FAILED name={} hresult=0x{:08X}",
                self.name, hr
            );
        }
        if !self.sid.is_null() {
            unsafe { FreeSid(self.sid) };
        }
    }
}

/// AppContainer 안에서 명령을 띄우고 종료 코드를 돌려준다.
///
/// ★ **stdout 을 캡처하지 않는다.** `alloc_fixture` 가 적어 둔 것과 같은
///   이유다 — `CreateProcessW` 를 직접 부르면 파이프 상속을 따로 설정해야
///   하고, 그러지 않은 채 stdout 을 신뢰하면 조용히 빈 결과를 얻는다.
///   자식이 **파일에 적게** 하고 그 파일을 읽는다.
pub fn run_in_container(
    profile: &AppContainerProfile,
    command_line: &str,
    working_dir: Option<&str>,
) -> Result<u32, AppContainerError> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_FAILED};
    use windows_sys::Win32::Security::SECURITY_CAPABILITIES;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject,
        EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTUPINFOEXW,
    };

    let mut caps = SECURITY_CAPABILITIES {
        AppContainerSid: profile.sid(),
        // ★ **빈 목록이다.** 무엇이 필요한지 모르는 상태에서 지어내지
        //   않는다 — 이 상태로 띄워 무엇이 실패하는지가 P0-02 의 답이다.
        Capabilities: std::ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };

    // 1) 속성 목록 크기를 물어본다(첫 호출은 반드시 실패하며 크기를 준다).
    let mut size: usize = 0;
    unsafe {
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
    }
    if size == 0 {
        return Err(AppContainerError::AttributeList {
            code: unsafe { GetLastError() },
        });
    }
    let mut buffer = vec![0u8; size];
    let attrs = buffer.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;

    if unsafe { InitializeProcThreadAttributeList(attrs, 1, 0, &mut size) } == 0 {
        return Err(AppContainerError::AttributeList {
            code: unsafe { GetLastError() },
        });
    }

    let updated = unsafe {
        UpdateProcThreadAttribute(
            attrs,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &mut caps as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if updated == 0 {
        let code = unsafe { GetLastError() };
        unsafe { DeleteProcThreadAttributeList(attrs) };
        return Err(AppContainerError::AttributeList { code });
    }

    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attrs;

    let mut cmd = wide(command_line);
    let cwd = working_dir.map(wide);
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let spawned = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT,
            std::ptr::null(),
            cwd.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            &mut startup.StartupInfo,
            &mut info,
        )
    };
    let spawn_error = unsafe { GetLastError() };
    unsafe { DeleteProcThreadAttributeList(attrs) };

    if spawned == 0 {
        return Err(AppContainerError::Spawn { code: spawn_error });
    }

    let waited = unsafe { WaitForSingleObject(info.hProcess, INFINITE) };
    if waited == WAIT_FAILED {
        let code = unsafe { GetLastError() };
        unsafe {
            CloseHandle(info.hProcess);
            CloseHandle(info.hThread);
        }
        return Err(AppContainerError::Wait { code });
    }

    let mut exit_code: u32 = 0;
    unsafe {
        GetExitCodeProcess(info.hProcess, &mut exit_code);
        CloseHandle(info.hProcess);
        CloseHandle(info.hThread);
    }
    Ok(exit_code)
}

/// AppContainer 안에서 명령을 띄우고 **종료 코드와 stdout 을 함께** 준다.
///
/// ★★ **파일을 안 쓴다.** [`run_in_container`] 는 자식이 파일에 적게
///   하는데, 그러면 **그 폴더에도 권한을 줘야 한다** — 2026-09-05 x600
///   실측에서 정확히 거기서 막혔다(`[Errno 13] Permission denied`).
///   운영자가 손으로 할 일이 하나 더 느는 것이라, 파이프로 직접 받는다.
///
/// ★ 파이프의 쓰기 끝만 상속시키고, 부모 쪽 사본은 `CreateProcessW`
///   **직후에 닫는다.** 안 닫으면 자식이 끝나도 파이프가 안 닫혀
///   `read_to_end` 가 영원히 기다린다 — 흔한 함정이라 적어 둔다.
pub fn run_in_container_capture(
    profile: &AppContainerProfile,
    command_line: &str,
    working_dir: Option<&str>,
) -> Result<(u32, String), AppContainerError> {
    use std::io::Read;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, DUPLICATE_SAME_ACCESS, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_FAILED,
    };
    use windows_sys::Win32::Foundation::DuplicateHandle;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    use windows_sys::Win32::Security::{SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES};
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject,
        EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTF_USESTDHANDLES,
        STARTUPINFOEXW,
    };

    // ── 파이프 ────────────────────────────────────────────────────────
    let mut sa: SECURITY_ATTRIBUTES = unsafe { std::mem::zeroed() };
    sa.nLength = std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
    sa.bInheritHandle = 1; // 자식이 물려받아야 한다
    let mut read_end: HANDLE = INVALID_HANDLE_VALUE;
    let mut write_end: HANDLE = INVALID_HANDLE_VALUE;
    if unsafe { CreatePipe(&mut read_end, &mut write_end, &sa, 0) } == 0 {
        return Err(AppContainerError::AttributeList {
            code: unsafe { GetLastError() },
        });
    }
    // ★ 읽기 끝은 **상속시키지 않는다.** 자식이 그것까지 들고 있으면
    //   자식이 죽어도 파이프가 안 닫힌다.
    let mut private_read: HANDLE = INVALID_HANDLE_VALUE;
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            read_end,
            GetCurrentProcess(),
            &mut private_read,
            0,
            0, // bInheritHandle = FALSE
            DUPLICATE_SAME_ACCESS,
        );
        CloseHandle(read_end);
    }

    let mut caps = SECURITY_CAPABILITIES {
        AppContainerSid: profile.sid(),
        Capabilities: std::ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };

    let mut size: usize = 0;
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size) };
    if size == 0 {
        return Err(AppContainerError::AttributeList {
            code: unsafe { GetLastError() },
        });
    }
    let mut buffer = vec![0u8; size];
    let attrs = buffer.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
    if unsafe { InitializeProcThreadAttributeList(attrs, 1, 0, &mut size) } == 0 {
        return Err(AppContainerError::AttributeList {
            code: unsafe { GetLastError() },
        });
    }
    if unsafe {
        UpdateProcThreadAttribute(
            attrs,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &mut caps as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        let code = unsafe { GetLastError() };
        unsafe { DeleteProcThreadAttributeList(attrs) };
        return Err(AppContainerError::AttributeList { code });
    }

    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attrs;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdOutput = write_end;
    startup.StartupInfo.hStdError = write_end;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;

    let mut cmd = wide(command_line);
    let cwd = working_dir.map(wide);
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let spawned = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // ★ bInheritHandles — 파이프를 물려주려면 반드시 TRUE
            EXTENDED_STARTUPINFO_PRESENT,
            std::ptr::null(),
            cwd.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            &mut startup.StartupInfo,
            &mut info,
        )
    };
    let spawn_error = unsafe { GetLastError() };
    unsafe { DeleteProcThreadAttributeList(attrs) };
    // ★ 부모 쪽 쓰기 끝을 **여기서** 닫는다. 안 닫으면 읽기가 안 끝난다.
    unsafe { CloseHandle(write_end) };

    if spawned == 0 {
        unsafe { CloseHandle(private_read) };
        return Err(AppContainerError::Spawn { code: spawn_error });
    }

    let out = {
        let mut file = unsafe { std::fs::File::from_raw_handle(private_read as *mut _) };
        let mut raw = Vec::new();
        let _ = file.read_to_end(&mut raw);
        String::from_utf8_lossy(&raw).into_owned()
    };

    let waited = unsafe { WaitForSingleObject(info.hProcess, INFINITE) };
    if waited == WAIT_FAILED {
        let code = unsafe { GetLastError() };
        unsafe {
            CloseHandle(info.hProcess);
            CloseHandle(info.hThread);
        }
        return Err(AppContainerError::Wait { code });
    }
    let mut exit_code: u32 = 0;
    unsafe {
        GetExitCodeProcess(info.hProcess, &mut exit_code);
        CloseHandle(info.hProcess);
        CloseHandle(info.hThread);
    }
    Ok((exit_code, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 프로파일을 **실제로 만들고 지운다.** 사용자 레지스트리에 쓰지만
    ///   `Drop` 이 지우고, 이름에 테스트 표시를 넣어 남아도 알아볼 수 있게
    ///   한다.
    #[test]
    fn a_profile_can_be_created_and_yields_an_appcontainer_sid() {
        let profile = AppContainerProfile::create(
            "gputeer-test-probe",
            "gputeer test probe",
            "P0-02 자동 테스트. 남아 있으면 지워도 된다.",
        )
        .expect("프로파일 생성");

        let sid = profile.sid_string().expect("SID 문자열");
        // AppContainer SID 는 반드시 S-1-15-2- 로 시작한다.
        assert!(
            sid.starts_with("S-1-15-2-"),
            "AppContainer SID 가 아니다: {sid}"
        );
    }

    /// ★★ **정말 AppContainer 안에 **갇혔는가**.**
    ///
    /// ★ 처음에 쓴 테스트는 `cmd /c exit 42` 의 종료 코드만 봤다. 그건
    ///   **컨테이너 밖에서도 42 를 준다** — 보안 속성을 통째로 안 붙이는
    ///   뮤테이션(A1)을 걸어도 통과했다. **가둬진 것을 하나도 안 재고
    ///   있었다.**
    ///
    ///   갇혔다는 것을 재려면 **컨테이너 안에서만 실패하는 것**을 시켜야
    ///   한다. 임시 디렉터리는 `ALL APPLICATION PACKAGES` 에 열려 있지
    ///   않으므로, 그 안의 파일을 읽는 것이 정확히 그런 일이다.
    ///
    /// ★ **대조를 같이 둔다** — 같은 명령을 컨테이너 **밖에서** 돌리면
    ///   성공해야 한다. 없으면 "경로가 틀려서 실패" 로도 통과한다.
    #[test]
    fn the_container_cannot_read_a_file_the_host_can() {
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let secret = dir.path().join("host-only.txt");
        std::fs::write(&secret, b"host only").expect("파일 쓰기");
        let command = format!("cmd.exe /c type \"{}\"", secret.display());

        // 대조 — 컨테이너 **밖**에서는 읽힌다.
        let outside = std::process::Command::new("cmd.exe")
            .args(["/c", "type", secret.to_str().unwrap()])
            .output()
            .expect("바깥에서 실행");
        assert!(
            outside.status.success(),
            "컨테이너 밖에서도 못 읽었다 — 이 테스트는 아무것도 재지 못한다: {:?}",
            String::from_utf8_lossy(&outside.stderr)
        );

        let profile = AppContainerProfile::create("gputeer-test-confine", "c", "테스트")
            .expect("프로파일 생성");
        let exit = run_in_container(&profile, &command, None).expect("컨테이너 안에서 실행");

        assert_ne!(
            exit, 0,
            "AppContainer 안에서 호스트 파일을 읽었다 — 가둬지지 않았다"
        );
    }

    /// ★★ **컨테이너가 쓸 수 있는 폴더가 생기는가.**
    ///
    /// 이게 있으면 **파일 ACL 코드를 안 써도 된다** — Windows 가
    /// `%LOCALAPPDATA%\Packages\<이름>\` 을 만들고 그 컨테이너에 권한을
    /// 준다. Python 스크립트와 결과 파일을 거기 두면 `P0-02` 의 나머지를
    /// ACL 없이 잴 수 있다.
    ///
    /// ★★ **이 테스트의 원래 이름은 거짓말이었다**(`..._the_container_can_use`).
    ///   폴더가 **있는지만** 재면서 **쓸 수 있는지** 재는 것처럼 이름을
    ///   붙였다. 2026-09-05 x600 실측이 그 차이를 드러냈다 — 폴더는
    ///   있는데 컨테이너가 그 안의 파일을 **못 읽었다**
    ///   (`[Errno 13] Permission denied`).
    ///
    ///   이름을 사실로 바꿨다 — **폴더가 생긴다는 것까지만** 잰다.
    ///   그래서 프로브는 이제 그 폴더를 안 쓰고 stdout 으로 받는다.
    #[test]
    fn the_profile_creates_a_folder_but_that_does_not_mean_it_is_usable() {
        let profile = AppContainerProfile::create("gputeer-test-folder", "f", "테스트")
            .expect("프로파일 생성");
        let _ = &profile;

        let local = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA");
        let dir = std::path::Path::new(&local)
            .join("Packages")
            .join("gputeer-test-folder");

        assert!(
            dir.is_dir(),
            "프로파일 폴더가 없다({}) — ACL 을 직접 줘야 한다",
            dir.display()
        );
    }

    /// **같은 이름을 두 번 만들어도 같은 SID** 여야 한다.
    ///
    /// ★ 이게 깨지면 재시작 뒤 방화벽 규칙이 **엉뚱한 컨테이너**를 가리킨다.
    #[test]
    fn the_same_name_yields_the_same_sid() {
        let first = AppContainerProfile::create("gputeer-test-stable", "s", "테스트")
            .expect("첫 생성");
        let sid_first = first.sid_string().expect("SID");
        // 아직 안 지운 상태에서 같은 이름을 다시 — ERROR_ALREADY_EXISTS 경로.
        let second = AppContainerProfile::create("gputeer-test-stable", "s", "테스트")
            .expect("재생성(기존 SID 조회)");
        let sid_second = second.sid_string().expect("SID");

        assert_eq!(sid_first, sid_second, "같은 이름인데 SID 가 다르다");
    }
}
