//! 이 프로세스가 **상승된(관리자) 토큰**으로 도는가 — `TokenElevation`.
//!
//! ★ 결함 562 "안 해본 것" — 운영 스크립트 여섯은 상승 창을 거부하지만, `gputeer agent-loop` 을 관리자 창에서 **직접** 부르면 그 검사를
//!   지나지 않는다. 그러면 호스트에서 도는 작업(S0 · RESTRICTED)이 관리자 토큰을 물려받는다(UAC 경계를 넘는다). Agent 가 호스트 작업을
//!   띄우기 전에 이 함수로 스스로 확인한다.
//!
//! ★ 리눅스에는 같은 규칙을 쓰지 않는다 — K1 키 보관 때문에 root Agent 가 정상 경로다.

use std::mem::size_of;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// 이 프로세스의 토큰이 상승돼 있으면 `Ok(true)`. 확인하지 못하면 `Err` — 부르는 쪽이 "모른다" 로 다룬다(조용히 `false` 로 바꾸지 않는다).
pub fn current_process_is_elevated() -> Result<bool, String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess 는 닫을 필요 없는 의사 핸들을 돌려준다. token 은 성공 시에만 채워지고 아래에서 닫는다.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(format!(
            "OpenProcessToken 실패: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    // SAFETY: elevation 은 TOKEN_ELEVATION 크기의 쓰기 가능한 버퍼이고, 크기를 정확히 넘긴다.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    let query_error = std::io::Error::last_os_error();
    // SAFETY: token 은 위 OpenProcessToken 이 연 핸들이다 — 한 번만 닫는다.
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return Err(format!(
            "GetTokenInformation(TokenElevation) 실패: {query_error}"
        ));
    }
    Ok(elevation.TokenIsElevated != 0)
}

#[cfg(test)]
mod tests {
    /// 확인 자체가 된다 — 값은 시험을 돌리는 창에 따라 다르므로 단정하지 않는다(개발 기계는 보통 상승 아님).
    #[test]
    fn the_elevation_of_this_process_can_be_read() {
        let elevated = super::current_process_is_elevated().expect("토큰 상승 여부를 읽지 못했다");
        eprintln!("TOKEN_ELEVATED={elevated}");
    }
}
