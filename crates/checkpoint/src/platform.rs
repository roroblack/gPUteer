//! 체크포인트 루트 아래 파일을 경로 이탈 없이 읽기 위한 플랫폼 계층.
//!
//! Windows 와 Linux 둘 다 실제 읽기 경로에 링크 방어가 연결돼 있다.
//! Windows 는 `CreateFileW` 의 reparse point 거부로, Linux 는 `openat2` 의
//! `RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS` 로 막는다. 그 외 플랫폼은
//! 여전히 상대 경로 문법만 제한하고 링크를 따라간다 — 이 상태는 공개
//! 상수와 테스트로 고정해 조용히 "지원됨"으로 오해되지 않게 한다.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path};

/// Windows 체크포인트 읽기 경로는 reparse point 방어가 활성화돼 있다.
pub const WINDOWS_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE: bool = true;

/// Linux 체크포인트 읽기 경로도 `openat2` 링크 방어가 활성화돼 있다.
///
/// ★ **2026-08-29 실측 후 `false` -> `true`.** 그 전까지는 구현만 있고
///   호출되지 않아 `false` 였다. x600 의 WSL2(kernel 6.18)를 확보해
///   `crates/checkpoint/tests/symlink_defense_linux.rs` 를 실제 Linux 에서
///   돌린 뒤에 뒤집었다 — 배선만 하고 미리 켜면 `CLAUDE.md` §0.4 가
///   금지하는 "강제할 수 없는 것을 보장으로 선언" 이 된다.
pub const LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE: bool = true;

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

/// Linux는 `openat2`의 `RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS`로 경로
/// 이탈과 링크 추적을 커널에서 거부한다.
///
/// ★ **배선 시점(2026-08-29).** 이 함수는 그 전까지 `File::open()`으로
///   symlink를 그대로 따라갔다. x600의 WSL2(kernel 6.18)를 확보해 실측이
///   가능해진 뒤 연결했다 — `openat2`는 커널 5.6+가 필요하다.
///
///   Windows 경로와 달리 여기서는 존재하지 않는 파일을 만들 위험이 없다.
///   `flags`가 `O_RDONLY`뿐이고 `O_CREAT`가 없기 때문이다.
#[cfg(target_os = "linux")]
pub(crate) fn open_beneath_for_read(root: &Path, relative: &Path) -> io::Result<File> {
    linux_openat2::open_beneath_linux(root, relative)
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

/// `read_beneath` 와 같되 **`max_bytes` 까지만** 읽는다 — 넘으면 `FileTooLarge` 로 거부한다(결함 544 · 558).
///
/// ★ 크기 검사와 읽기를 **둘 다** 묶는다. 연 핸들의 크기가 이미 크면 읽기 전에 거부하고, 읽는 동안 커지면(검사 뒤 작업이 더 씀) `max_bytes + 1`
///   바이트째에서 멈추고 거부한다 — 크기만 보고 통째로 읽으면 그 사이에 뚫린다. 성긴 파일도 겉보기 크기로 걸린다(읽으면 0 이 그만큼 나온다).
pub(crate) fn read_beneath_capped(
    root: &Path,
    relative: &Path,
    max_bytes: u64,
) -> io::Result<Vec<u8>> {
    let file = open_beneath_for_read(root, relative)?;
    let len = file.metadata()?.len();
    read_capped(file, len, max_bytes, relative)
}

/// 파일을 **흘려 읽으며** BLAKE3 해시한다 — 내용을 메모리에 올리지 않는다(버퍼만). 링크 방어는 `read_beneath` 와 같다.
pub(crate) fn hash_beneath(root: &Path, relative: &Path) -> io::Result<String> {
    let mut file = open_beneath_for_read(root, relative)?;
    let mut hasher = blake3::Hasher::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// 크기 검사와 묶인 읽기의 본체 — 읽는 쪽을 받는다(시험이 "선언보다 더 내주는" 읽기로 부른다).
pub(crate) fn read_capped(
    reader: impl Read,
    declared_len: u64,
    max_bytes: u64,
    what: &Path,
) -> io::Result<Vec<u8>> {
    if declared_len > max_bytes {
        return Err(too_large(what, declared_len, max_bytes));
    }
    let mut data = Vec::with_capacity(declared_len as usize);
    reader
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut data)?;
    if data.len() as u64 > max_bytes {
        return Err(too_large(what, data.len() as u64, max_bytes));
    }
    Ok(data)
}

fn too_large(relative: &Path, seen: u64, max_bytes: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        format!(
            "{relative:?} 가 {seen} 바이트 이상이라 상한({max_bytes})을 넘어 메모리에 올리지 않는다"
        ),
    )
}

/// Linux `openat2` 구현 — `open_beneath_for_read`가 실제로 호출한다.
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
mod linux_openat2 {
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

    pub(super) fn open_beneath_linux(root: &Path, relative: &Path) -> io::Result<File> {
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
        read_capped, CHECKPOINT_READ_LINK_DEFENSE_ACTIVE,
        LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE, WINDOWS_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE,
    };
    use std::io::{self, Read};
    use std::path::Path;

    /// 읽히면 안 되는 읽기 — 크기만 보고 거부해야 할 때 실제로 읽지 않았는지 본다.
    struct MustNotRead;
    impl Read for MustNotRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("선언 크기가 상한을 넘으면 읽지 않아야 한다");
        }
    }

    /// 결함 558 — 선언 크기가 상한을 넘으면 **읽기 전에** 거부한다(성긴 1TiB 파일처럼).
    #[test]
    fn a_declared_size_over_the_cap_is_refused_without_reading() {
        let error = read_capped(MustNotRead, 1 << 40, 1024, Path::new("model.bin")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
    }

    /// 끝없이 커지는 파일 — 상한보다 한참 더(4배) 읽히면 멈춘다. 읽기가 상한에서 묶이지 않았다는 뜻이다.
    struct EndlessGrowth {
        served: u64,
        panic_after: u64,
    }
    impl Read for EndlessGrowth {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            assert!(
                self.served <= self.panic_after,
                "상한 + 1 에서 멈추지 않고 계속 읽었다({} 바이트)",
                self.served
            );
            buf.fill(7);
            self.served += buf.len() as u64;
            Ok(buf.len())
        }
    }

    /// 결함 558 — 크기를 본 뒤 파일이 커져도(작업이 계속 씀) 상한 + 1 바이트째에서 멈추고 거부한다. 선언은 작고 실제 내용은 끝이 없다.
    #[test]
    fn a_file_that_grows_after_the_size_check_is_cut_at_the_cap() {
        let grown = EndlessGrowth {
            served: 0,
            panic_after: 4 * 1024,
        };
        let error = read_capped(grown, 100, 1024, Path::new("model.bin")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
        // 대조군 — 상한 안쪽이면 그대로 읽는다(정확히 상한인 경우 포함).
        assert_eq!(
            read_capped(io::repeat(7).take(1024), 1024, 1024, Path::new("x"))
                .unwrap()
                .len(),
            1024
        );
    }

    /// 어느 플랫폼에서 링크 방어가 실제로 연결돼 있는지를 고정한다.
    ///
    /// ★ **2026-08-29 개정.** 이 테스트는 원래 `rollout_status_is_windows_only`
    ///   였고 "Linux 는 아직 꺼져 있다" 를 고정했다 — 구현만 있고 미배선인
    ///   상태가 조용히 "지원됨" 으로 오해되는 것을 막는 가드였다.
    ///   `openat2` 를 실제 Linux(x600 WSL2)에서 실측하고 배선한 뒤
    ///   그 가드를 새 사실로 갱신한다. **가드를 지우지 않고 뒤집는다** —
    ///   지우면 다음에 누가 꺼도 아무도 모른다.
    #[test]
    fn rollout_status_covers_windows_and_linux() {
        assert!(WINDOWS_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE);
        assert!(
            LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE,
            "Linux openat2 방어가 꺼졌다 — 실측 후 배선된 상태여야 한다"
        );
        // Windows·Linux 는 켜져 있고, 그 외 플랫폼은 여전히 꺼져 있다.
        assert_eq!(
            CHECKPOINT_READ_LINK_DEFENSE_ACTIVE,
            cfg!(any(windows, target_os = "linux"))
        );
    }
}
