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
    pub(super) fn open_beneath_linux_unverified(
        root: &Path,
        relative: &Path,
    ) -> io::Result<File> {
        super::validate_relative(relative)?;

        let root_c = CString::new(root.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "root 경로에 NUL이 있다")
        })?;
        let relative_c = CString::new(relative.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "상대 경로에 NUL이 있다")
        })?;

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
