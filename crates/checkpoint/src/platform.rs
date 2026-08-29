//! 체크포인트 루트 아래 파일을 경로 이탈 없이 읽기 위한 플랫폼 계층.
//!
//! Windows만 실제 읽기 경로에 링크 방어가 연결돼 있다. Linux용
//! `openat2` 구현은 아래에 함께 두지만, Windows 개발기에서 아직 실측하지
//! 못했으므로 의도적으로 호출하지 않는다. 이 상태는 공개 상수와 테스트로
//! 고정해 조용히 "지원됨"으로 오해되지 않게 한다.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path};

/// Windows 체크포인트 읽기 경로는 reparse point 방어가 활성화돼 있다.
pub const WINDOWS_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE: bool = true;

/// Linux 구현은 존재하지만 아직 검증·연결되지 않았으므로 반드시 `false`다.
pub const LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE: bool = false;

/// 현재 빌드 대상에서 체크포인트 읽기 링크 방어가 실제로 연결됐는지 나타낸다.
pub const CHECKPOINT_READ_LINK_DEFENSE_ACTIVE: bool = if cfg!(windows) {
    WINDOWS_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE
} else if cfg!(target_os = "linux") {
    LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE
} else {
    false
};

fn validate_relative(relative: &Path) -> io::Result<()> {
    let mut saw_component = false;
    for component in relative.components() {
        match component {
            Component::Normal(_) => saw_component = true,
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("허용되지 않는 상대 경로 구성요소: {other:?}"),
                ))
            }
        }
    }

    if !saw_component {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "상대 경로가 비어 있다",
        ));
    }

    Ok(())
}

/// `root` 아래의 기존 파일을 열되, Windows에서는 경로 구성요소의 reparse
/// point를 열기 시점에 거부한다.
///
/// 반환된 `File`만 사용해야 한다. 검증 뒤 경로 문자열을 다시 여는 순간
/// TOCTOU 방어가 사라진다.
#[cfg(windows)]
pub(crate) fn open_beneath_for_read(root: &Path, relative: &Path) -> io::Result<File> {
    validate_relative(relative)?;

    // 읽기 전용 변형을 쓴다. open_beneath 는 최종 대상에 OPEN_ALWAYS 를
    // 써서 없는 파일을 만들므로 읽기에 부적합하다. 존재 검사를 먼저 하는
    // 우회는 그 자체가 TOCTOU 라, 커널이 OPEN_EXISTING 으로 판정하게 한다.
    gputeer_runtime_windows::open_beneath_read_only(root, relative)
}

/// Linux는 고의로 평범한 열기를 유지한다. 아래 `openat2` 구현을 검증하고
/// 연결하기 전까지 symlink를 따라가므로 방어되지 않는다.
#[cfg(target_os = "linux")]
pub(crate) fn open_beneath_for_read(root: &Path, relative: &Path) -> io::Result<File> {
    validate_relative(relative)?;
    File::open(root.join(relative))
}

/// Windows/Linux 이외 플랫폼에는 커널 수준 beneath primitive가 연결돼 있지
/// 않다. 상대 경로 문법만 제한하고 링크는 따라간다.
#[cfg(not(any(windows, target_os = "linux")))]
pub(crate) fn open_beneath_for_read(root: &Path, relative: &Path) -> io::Result<File> {
    validate_relative(relative)?;
    File::open(root.join(relative))
}

pub(crate) fn read_beneath(root: &Path, relative: &Path) -> io::Result<Vec<u8>> {
    let mut file = open_beneath_for_read(root, relative)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    Ok(data)
}

/// 아직 호출하지 않는 Linux `openat2` 구현.
///
/// `RESOLVE_BENEATH`로 root 밖 탈출을 막고 `RESOLVE_NO_SYMLINKS`와
/// `RESOLVE_NO_MAGICLINKS`로 경로 전체의 링크 추적을 거부한다. 이 코드는
/// Windows 개발기에서 작성됐고 Linux에서 아직 실측되지 않았다. 검증 전에는
/// `open_beneath_for_read`에 연결하지 않는다.
///
/// ★ **타입 검사 이력(2026-08-27).** 이 모듈은 `#[cfg(target_os = "linux")]`
///   이라 Windows 기본 빌드에서는 컴파일 자체가 안 된다 — 작성 이후 이날까지
///   **어디서도 타입 검사를 받은 적이 없었다.** CI 워크플로가 ubuntu 에서
///   워크스페이스를 빌드하지만 이 저장소는 원격이 없어 그 워크플로가 실제로
///   실행된 적이 없다. 이날 아래 명령으로 처음 교차 검사했고 통과했다.
///
///   ```text
///   cargo check -p gputeer-checkpoint --all-targets ///       --target x86_64-unknown-linux-gnu
///   ```
///
///   비공허성 확인: 이 모듈 안에 타입 오류를 넣으면 Windows 기본 검사는
///   그대로 통과하고 위 명령만 실패한다(실측). 워크스페이스 전체를 같은
///   방식으로 교차 검사하지는 못한다 — `libsqlite3-sys` 가 Linux용 C
///   크로스 컴파일러를 요구한다. 이 crate 는 rusqlite 의존이 없어 가능하다.
///
///   **통과했다는 것은 "컴파일된다" 는 뜻이지 "동작한다" 는 뜻이 아니다.**
///   실제 `openat2` 거동·symlink 거부 실측은 여전히 Linux 기계가 필요하다.
#[cfg(target_os = "linux")]
mod linux_unverified {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
    const RESOLVE_NO_SYMLINKS: u64 = 0x04;
    const RESOLVE_BENEATH: u64 = 0x08;

    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }

    #[allow(dead_code)]
    pub(super) fn open_beneath_linux_unverified(root: &Path, relative: &Path) -> io::Result<File> {
        super::validate_relative(relative)?;

        let root_c = CString::new(root.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "root 경로에 NUL이 있다"))?;
        let relative_c = CString::new(relative.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "상대 경로에 NUL이 있다"))?;

        let root_fd = unsafe {
            libc::open(
                root_c.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if root_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let root_file = unsafe { File::from_raw_fd(root_fd) };

        let how = OpenHow {
            flags: (libc::O_RDONLY | libc::O_CLOEXEC) as u64,
            mode: 0,
            resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS,
        };
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                root_file.as_raw_fd(),
                relative_c.as_ptr(),
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(unsafe { File::from_raw_fd(fd as i32) })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHECKPOINT_READ_LINK_DEFENSE_ACTIVE, LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE,
        WINDOWS_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE,
    };

    #[test]
    fn rollout_status_is_windows_only() {
        assert!(WINDOWS_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE);
        assert!(
            !LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE,
            "Linux openat2 구현은 아직 검증·연결되지 않았다"
        );
        assert_eq!(CHECKPOINT_READ_LINK_DEFENSE_ACTIVE, cfg!(windows));
    }
}
