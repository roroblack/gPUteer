//! DLL 하나를 `LoadLibraryW` 로 열어 보고 결과만 말한다.
//!
//! ★★ **`P0-02` 를 한 칸 더 좁히려고 만들었다.** 2026-09-05 감별에서
//!   AppContainer 안에서 `zlib`·`_socket` 같은 C 확장은 되는데
//!   `_ctypes` 만 실패한다는 것까지 알아냈다. 그런데 `_ctypes.pyd` 는
//!   `libffi-8.dll` 에 의존하므로 **둘 중 누가 실패하는지 아직 모른다.**
//!
//!   Python 을 거치면 그 구분이 안 된다 — `import _ctypes` 는 둘을 묶어
//!   한 번에 시도하기 때문이다. 그래서 **DLL 을 직접 하나씩** 연다.
//!
//! ★ 이 프로브는 **아무것도 해석하지 않는다.** 성공/실패와 Win32 오류
//!   코드만 찍는다 — 해석은 기록하는 사람이 한다.
//!
//! ```text
//! dll_probe <dll 경로> [<dll 경로> ...]
//! ```

#[cfg(windows)]
fn main() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;

    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("사용법: dll_probe <dll 경로> [...]");
        std::process::exit(2);
    }

    let mut failures = 0;
    for path in &paths {
        let wide: Vec<u16> = OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
        if handle.is_null() {
            let code = unsafe { GetLastError() };
            // ★ 오류 코드를 그대로 남긴다. 1114 는 DLL_INIT_FAILED,
            //   126 은 MOD_NOT_FOUND, 5 는 ACCESS_DENIED 다 — 이 셋이
            //   가리키는 원인이 서로 완전히 다르다.
            println!("DLL_PROBE path={path} ok=false error={code}");
            failures += 1;
        } else {
            println!("DLL_PROBE path={path} ok=true");
        }
    }
    // 하나라도 실패하면 0 이 아닌 코드로 끝낸다 — 호출부가 종료 코드만
    // 봐도 알 수 있게.
    std::process::exit(if failures == 0 { 0 } else { 1 });
}

#[cfg(not(windows))]
fn main() {
    eprintln!("이 프로브는 Windows 전용이다");
    std::process::exit(2);
}
