//! `root` 아래에서만 파일을 여는 TOCTOU 방어 — reparse point(symlink·
//! junction·mount point)를 만나면 거부한다.
//!
//! `crates/runtime-policy/src/artifact.rs` 의 `ArtifactPolicy::check()`
//! 는 **경로 문자열만** 본다 — 검사를 통과한 직후 그 경로(또는 그
//! 경로 위의 중간 디렉터리)를 symlink/junction 으로 바꿔치기하면
//! 그 검사는 무력하다(같은 파일 모듈 문서의 "이 모듈이 하지 못하는
//! 것" 절). 이 모듈이 그 다음 방어선이다 — 문자열 검사를 통과한
//! 뒤에도, **실제로 여는 순간** 경로 위의 모든 구성요소가
//! reparse point 가 아님을 확인한다.
//!
//! # 이것이 완전한 TOCTOU 방어가 아닌 이유
//!
//! Linux 의 `openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS)` 는 커널이
//! 원자적으로 보장한다. 이 구현은 일반 Win32 `CreateFileW` 를 경로
//! 컴포넌트 단위로 반복 호출한다 — 한 컴포넌트를 확인한 시점과 다음
//! 컴포넌트를 여는 시점 사이에 그 디렉터리가 다른 것으로 교체되는
//! 경합은 막지 못한다(설계 검토 — 코덱스, `p61` 프롬프트. 더 강한
//! 보장이 필요해지면 `NtCreateFile` 의 `RootDirectory` 상대 open 으로
//! 승격해야 한다 — 미구현). 그래도 이 구현은 가장 흔한 공격("검사
//! 통과 후 심볼릭 링크로 통째로 바꿔치기")은 실제로 막는다 — reparse
//! point 자체를 열기 시점에 발견해 거부하기 때문이다.
//!
//! # ★ 이 모듈은 아직 아무 실제 쓰기 경로에도 연결되지 않았다 (독립 검수 2026-08-18 지적)
//!
//! `crates/runtime-policy/src/vram.rs` 가 `runtime-windows` 신설
//! 전까지 같은 처지였던 것과 정확히 같은 이유다 — 이 저장소는
//! **아직 Job 을 실행하지 않는다**(`CLAUDE.md` §5 "미착수" —
//! scheduler·`crates/agent` 의 Job 실행). 그래서 "artifact_scope
//! 로 통제되는 임의 경로에 실제로 쓰는" 코드 경로 자체가 이 저장소에
//! 없다 — `ArtifactPolicy::check()` 를 **운영 코드에서** 호출하는
//! 곳도 `crates/cli/src/selftest.rs` 의 합성 문자열 검사(파일시스템을
//! 건드리지 않는다) 하나뿐이다(이 크레이트 자신의 테스트도 검증용
//! 으로 부르지만, 그것은 이 문장이 말하는 "실제 쓰기 경로"가 아니다
//! — 코덱스 독립 검수 2026-08-18 이 부정확한 표현을 지적했다).
//! `open_beneath`/`open_artifact` 는 **미래의 실행 계층이 쓸 준비가
//! 된 primitive** 이지, 지금 당장 어떤 실제 쓰기를 대체한 것이
//! 아니다 — 이 사실을 감추지 않는다.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS, OPEN_EXISTING,
};

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

/// reparse point 를 절대 따라가지 않고 연다(`FILE_FLAG_OPEN_REPARSE_POINT`).
/// 연 핸들 자체가 reparse point 를 가리키면 즉시 닫고 거부한다.
fn open_no_reparse(path: &Path, access: u32, creation: u32, directory: bool) -> io::Result<File> {
    let path_w = wide(path.as_os_str());

    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS;
    }

    let handle = unsafe {
        CreateFileW(
            path_w.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            creation,
            flags,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    if ok == 0 {
        let err = io::Error::last_os_error();
        unsafe { CloseHandle(handle) };
        return Err(err);
    }

    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        unsafe { CloseHandle(handle) };
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} 는 reparse point(symlink/junction/mount point)다 — 허용되지 않는다",
                path.display()
            ),
        ));
    }

    Ok(unsafe { File::from_raw_handle(handle as _) })
}

/// `root` 아래 `relative` 를 연다.
///
/// `relative` 는 절대 경로·`.`·`..` 를 포함할 수 없다(이미
/// `ArtifactPolicy::check()` 가 걸러야 하지만, 이 함수 자체도
/// 독립적으로 다시 거부한다 — 방어를 한 곳에만 의존하지 않는다).
/// 경로 위의 **모든** 중간 구성요소와 최종 대상이 reparse point 가
/// 아님을 열기 시점에 직접 확인한다.
///
/// ★ 반환된 `File` 로만 써야 한다. 이 함수가 검증한 뒤 경로 문자열을
/// 다시 만들어 이름 기반 API(`std::fs::write` 등)를 부르면, 그 사이의
/// 어떤 변경도 검증을 우회한다 — 검증과 사용이 같은 핸들이어야
/// 의미가 있다.
pub fn open_beneath(root: &Path, relative: &Path) -> io::Result<File> {
    if relative.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative 는 절대 경로일 수 없다",
        ));
    }

    let components: Vec<Component<'_>> = relative.components().collect();
    if components.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "relative 가 비어 있다"));
    }

    // root 자신도 reparse point 가 아님을 확인한다 — 호출자가 이미
    // 오염된 root 를 넘기면 그 아래 전부가 무의미해진다.
    drop(open_no_reparse(root, GENERIC_READ, OPEN_EXISTING, true)?);

    let mut current: PathBuf = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let name = match component {
            Component::Normal(name) => name,
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("허용되지 않는 경로 구성요소: {other:?}"),
                ))
            }
        };
        current.push(name);

        let is_final = index + 1 == components.len();
        if is_final {
            return open_no_reparse(&current, GENERIC_READ | GENERIC_WRITE, OPEN_ALWAYS, false);
        }

        // ★ 중간 디렉터리는 지금 확인한 뒤 곧바로 그 경로 문자열로
        //   다음 컴포넌트를 연다 — 일반 Win32 공개 API 만으로는 "확인한
        //   바로 그 핸들을 기준으로 한 상대 열기"를 표현할 수 없다
        //   (모듈 문서의 한계 절 참조). 확인/사용 사이에 짧은 창이 남는다.
        drop(open_no_reparse(&current, GENERIC_READ, OPEN_EXISTING, true)?);
    }

    unreachable!("components 가 비어 있지 않음을 위에서 이미 확인했다")
}

/// `ArtifactPolicy::check()`(문자열 검사) 와 [`open_beneath`](TOCTOU
/// 방어)를 순서대로 적용하는 편의 함수. 호출자는 이 함수가 반환한
/// `File` 로만 써야 한다.
pub fn open_artifact(
    policy: &gputeer_runtime_policy::ArtifactPolicy,
    root: &Path,
    requested: &str,
) -> io::Result<File> {
    policy
        .check(requested)
        .map_err(|violation| io::Error::new(io::ErrorKind::PermissionDenied, format!("{violation:?}")))?;
    open_beneath(root, Path::new(requested))
}
