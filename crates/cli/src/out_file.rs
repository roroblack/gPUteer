//! 서명된 산출물을 파일로 내보낸다 — **기존 파일을 조용히 망가뜨리지 않는다.**
//!
//! # 왜 이 모듈이 있나
//!
//! ★★ 2026-09-10 독립 검수가 `issue-grant` 에서 찾은 결함이다. 그 명령은
//!   `std::fs::write(out_path, ..)` 한 줄로 산출물을 내보냈고, 그것이 두
//!   가지를 한꺼번에 잘못했다:
//!
//!   1. **기존 파일을 말없이 덮었다.** 운영자가 서명 키를 잘못 주면 그
//!      명령은 그것을 잡지 못하고 성공을 찍으면서 **멀쩡하던 산출물을
//!      못 쓰는 것으로 바꿔 놓는다.**
//!   2. **먼저 자르고 쓴다.** 중간에 실패하면 기존 파일이 **잘린 채** 남는다.
//!      `CLAUDE.md` §0.3 이 경고하는 모양이다.
//!
//! ★★ **그리고 `submit` 에 똑같은 줄이 있었다.** 한쪽만 고치면 다음
//!   사람이 다른 쪽을 다시 발견한다 — 애초에 이 결함이 두 곳에 생긴
//!   방식이 그것이다. 그래서 도우미를 한 곳에 둔다.
//!
//! # ★★ 1차 수정이 모자랐다 — 재검수가 셋을 더 찾았다 (2026-09-10)
//!
//! ```text
//! ① 존재 확인과 확정 사이가 벌어져 있었다
//!    `target.exists()` 로 보고 나서 `rename` 했다. 그 사이에 다른
//!    프로세스가 만들면 **말없이 덮는다** — 막으려던 바로 그 일이다
//! ② 임시 파일을 배타적으로 안 만들었다
//!    `<이름>.tmp.<pid>` 를 `fs::write` 로 만들었다. 같은 이름이 이미
//!    있으면(앞 실행이 죽어 남긴 것) 그것을 덮었다
//! ③ 덮어쓰기에서 **원본을 먼저 지웠다**
//!    `remove_file` 뒤 `rename` 사이에 **아무것도 없는 창**이 생긴다.
//!    그 순간 프로세스가 죽으면 원본도 새것도 없다
//! ```
//!
//! # 지금 어떻게 하나
//!
//! ```text
//! 임시 파일   create_new(true) 로 **배타 생성**한다. 이미 있으면 이름을
//!             바꿔 다시 시도한다 — 남의 임시 파일을 덮지 않는다
//! 덮어쓰기 X  temp 를 target 에 **hard_link** 한다. 대상이 있으면
//!             링크가 실패한다 — **확인과 확정이 한 번의 원자적 연산**이다
//! 덮어쓰기 O  `fs::rename` 한 번으로 바꾼다. **먼저 지우지 않는다** —
//!             그래서 교체가 실패하면 기존 파일이 그대로 남는다
//! ```
//!
//! ★★ 2026-09-10 재검수 11 — 여기 "std 의 rename 은 두 플랫폼 다 대상을
//!   원자적으로 교체한다" 고 적었었다. **보장 범위를 넘었다.** std 문서는
//!   모든 파일시스템에서 원자성을 약속하지 않고, Windows 구현은
//!   `MoveFileExW` 말고 다른 경로도 쓴다. 이 모듈이 말할 수 있는 것은
//!   "먼저 지우지 않으므로 **실패하면 원본이 남는다**" 까지다.
//!
//! ★ 왜 hard_link 인가 — Rust 표준 라이브러리에 **덮어쓰지 않는 rename 이
//!   없다.** `rename_noreplace` 는 아직 제안 단계다(rust-lang/libs-team#131).
//!   플랫폼별로는 있다 — Linux `renameat2(RENAME_NOREPLACE)`, macOS
//!   `renameatx_np(RENAME_EXCL)`, Windows 는 `MOVEFILE_REPLACE_EXISTING` 을
//!   빼면 그게 기본 동작이다. 표준만으로 그 성질을 얻는 방법이 hard_link 다
//!   — `link()` 는 EEXIST 로, `CreateHardLinkW` 는 실패로 끝난다.
//!
//! ★ 한계를 적어 둔다 — hard_link 를 **지원하지 않는 파일시스템**(FAT32,
//!   일부 네트워크 마운트)에서는 이 경로가 실패한다. 그때는 오류가
//!   그대로 올라가고 산출물이 안 생긴다. **조용히 덮는 것보다 낫다.**
//!
//! ★ 한계 둘 더(재검수 11):
//!   ```text
//!   남은 링크   hard_link 뒤 임시 이름을 못 지우면 **산출물과 같은 파일을
//!               가리키는 링크**가 남는다. 그 링크에 누가 쓰면 산출물도
//!               바뀐다. 못 지운 사실을 **경고로 돌려준다**
//!   64 후보     같은 이름·같은 pid 의 잔여가 64개면 target 이 없어도
//!               실패한다. 가용성 한계이고 덮어쓰는 경로는 아니다
//!   ```
//!
//! ★★ **경고를 여기서 출력하지 않는다**(재검수 13). 처음엔 `eprintln!` 으로
//!   찍었는데, 그 매크로는 stderr 쓰기가 실패하면 **패닉**한다. 산출물이 이미
//!   확정됐는데 경고 때문에 실패로 끝나면 "못 지워도 실패로 돌리지 않는다"
//!   는 계약이 깨진다. 경고는 반환값에 담고, 부르는 쪽이 **자기 요약에**
//!   싣는다([`with_warning`]).
//!
//! ★ 테스트가 지키는 것과 못 지키는 것:
//!   ```text
//!   지킨다   임시 이름 배타 생성 — 1차 구현이 쓰던 이름(`.tmp.<pid>`)과
//!            지금 이름(`.tmp.<pid>.0`)의 남의 파일을 **둘 다** 안 건드린다
//!   지킨다   덮어쓰기 금지에서 동시 쓰기 16개 중 정확히 하나만 이긴다.
//!            ★ 확인과 확정이 갈라진 1차 구현은 여럿이 이길 **수 있다** —
//!              스케줄에 달려 있어 매번 잡는다는 보장은 없다
//!   지킨다   먼저 지우지 않는다 — 교체를 주입해 실패시키면 원본이 남는다
//!            (재검수 13 전에는 "코드로만 지킨다" 였다)
//!   지킨다   정리 실패를 말한다 — 성공 경로는 경고를 돌려주고, 실패
//!            경로는 오류에 덧붙인다
//!   ```
//!
//! ```text
//! 안 한다  fsync — 이건 체크포인트가 아니다. 전원이 끊기면 다시 발급하면 된다
//!          (`crates/checkpoint` 의 `ADR-026` 절차는 그쪽에서 쓴다)
//! 안 한다  내용 검증 — 부르는 쪽이 이미 서명하고 자기 검증했다
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};

/// 파일 교체·삭제. 테스트가 실패를 **주입**할 수 있게 한 벌로 묶는다.
///
/// ★ 교체와 삭제를 따로 주입할 수 있어야 "먼저 지우지 않는다" 를 잴 수 있다
///   — 교체만 실패시키고 삭제는 진짜로 두면, 대상을 먼저 지우는 구현은
///   원본을 잃는다.
struct FsOps {
    remove: fn(&Path) -> std::io::Result<()>,
    rename: fn(&Path, &Path) -> std::io::Result<()>,
}

fn real_remove(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

fn real_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

const REAL_FS: FsOps = FsOps {
    remove: real_remove,
    rename: real_rename,
};

/// 산출물을 원자적으로 내보낸다.
///
/// `code` 는 거부 메시지의 접두어다(`"SUBMIT_REFUSED"` 처럼). 명령마다
/// 다르므로 인자로 받는다 — 메시지 머리에 안정적인 코드를 두는 것은
/// 이 저장소의 관례이고, 테스트가 **줄 시작**으로 사유를 확인할 수
/// 있게 해 준다.
///
/// `what` 은 사람이 읽을 산출물 이름(`"Manifest"`, `"Grant"`).
///
/// 성공하면 `Ok(None)`, 성공했지만 알릴 것이 있으면 `Ok(Some(경고))` 다.
pub fn write_new(
    out_path: &str,
    bytes: &[u8],
    overwrite: bool,
    code: &str,
    what: &str,
) -> Result<Option<String>, String> {
    write_new_with(&REAL_FS, out_path, bytes, overwrite, code, what)
}

/// 경고가 있으면 요약 뒤에 붙인다 — 부르는 쪽마다 따로 출력하지 않게.
pub fn with_warning(summary: String, warning: Option<String>) -> String {
    match warning {
        Some(warning) => format!("{summary}\n{warning}"),
        None => summary,
    }
}

fn write_new_with(
    fs: &FsOps,
    out_path: &str,
    bytes: &[u8],
    overwrite: bool,
    code: &str,
    what: &str,
) -> Result<Option<String>, String> {
    let target = Path::new(out_path);
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let stem = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "out".into());

    let (tmp, mut file) = create_exclusive_temp(dir, &stem, code, what)?;

    let write_result = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .map_err(|e| format!("{code}: {what} 임시 파일 쓰기 실패({}): {e}", tmp.display()));
    drop(file);
    if let Err(e) = write_result {
        return Err(with_cleanup(fs, e, &tmp));
    }

    if overwrite {
        // ★ 먼저 지우지 않는다. rename 이 한 번에 교체한다 — 실패하면 원본이 남는다.
        return (fs.rename)(&tmp, target).map(|()| None).map_err(|e| {
            with_cleanup(
                fs,
                format!("{code}: {what} 파일 확정 실패({out_path}): {e}"),
                &tmp,
            )
        });
    }

    // ★ 대상이 있으면 여기서 실패한다 — 확인과 확정이 한 연산이다.
    match std::fs::hard_link(&tmp, target) {
        Ok(()) => match (fs.remove)(&tmp) {
            Ok(()) => Ok(None),
            // 산출물은 이미 완성됐다. 실패로 돌리지 않고 **말한다**.
            Err(e) => Ok(Some(format!(
                "{code}: 경고 — 임시 링크 {} 를 지우지 못했다: {e}. \
                 산출물과 같은 파일을 가리키므로 여기에 쓰면 산출물도 바뀐다. \
                 지워도 산출물은 남는다",
                tmp.display()
            ))),
        },
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(with_cleanup(
            fs,
            format!(
                "{code}: OUT_EXISTS — {out_path} 에 파일이 이미 있다. 덮어쓰려면 명시적으로 허용해야 한다 (잘못된 인자로 멀쩡한 {what} 를 날리는 것을 막는다)"
            ),
            &tmp,
        )),
        Err(e) => Err(with_cleanup(
            fs,
            format!(
                "{code}: {what} 파일 확정 실패({out_path}): {e} — 이 파일시스템이 hard link 를 지원하지 않을 수 있다"
            ),
            &tmp,
        )),
    }
}

/// 실패 경로의 정리. 정리까지 실패하면 **두 오류를 다** 보고한다 —
/// 한쪽을 묵으면 남은 임시 파일의 원인을 놓친다.
fn with_cleanup(fs: &FsOps, error: String, tmp: &Path) -> String {
    match (fs.remove)(tmp) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => error,
        Err(cleanup) => format!(
            "{error} / 그리고 임시 파일 {} 도 지우지 못했다: {cleanup}",
            tmp.display()
        ),
    }
}

/// 임시 파일을 **배타적으로** 만든다.
///
/// ★ 앞 실행이 죽어 남긴 같은 이름을 덮지 않는다. 이미 있으면 이름을
///   바꿔 다시 시도한다. 시도 횟수를 제한해 무한 루프를 만들지 않는다.
fn create_exclusive_temp(
    dir: &Path,
    stem: &str,
    code: &str,
    what: &str,
) -> Result<(PathBuf, std::fs::File), String> {
    let pid = std::process::id();
    for attempt in 0..64u32 {
        let candidate = dir.join(format!("{stem}.tmp.{pid}.{attempt}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "{code}: {what} 임시 파일을 만들지 못했다({}): {e}",
                    candidate.display()
                ))
            }
        }
    }
    Err(format!(
        "{code}: {what} 임시 파일 이름을 64번 시도해도 비어 있는 것을 못 찾았다 — 앞선 실행이 남긴 파일이 쌓였을 수 있다"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failing_rename(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("주입한 교체 실패"))
    }

    fn failing_remove(_: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("주입한 삭제 실패"))
    }

    fn temps_left(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .expect("목록")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp."))
            .collect()
    }

    /// ★ 결함 ⑭·⑰ — 임시 이름을 **배타적으로** 만든다.
    ///
    /// 1차 구현은 `<이름>.tmp.<pid>` 에 `fs::write` 로 썼다 — 같은 이름이
    /// 있으면 덮고, 확정하면서 치웠다. 지금은 `.tmp.<pid>.<n>` 을
    /// `create_new` 로 만든다. **두 이름 모두**에 남의 파일을 미리 두고 둘 다
    /// 남는지 본다.
    ///
    /// ★★ 재검수 13 — 처음엔 `.0` 하나만 뒀다. 1차 구현의 **실제 이름**을
    ///   안 둬서, 1차 구현으로 되돌리면 그 구현은 다른 이름에 쓰고 `.0` 을
    ///   안 건드려 **테스트가 통과했다.** 내 뮤테이션은 이름은 그대로 두고
    ///   생성 방식만 바꿔서 그 차이를 못 봤다.
    #[test]
    fn leftover_temps_under_both_the_old_and_new_names_are_kept() {
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let target = dir.path().join("x.pb");
        let pid = std::process::id();
        let old_name = dir.path().join(format!("x.pb.tmp.{pid}"));
        let new_name = dir.path().join(format!("x.pb.tmp.{pid}.0"));
        std::fs::write(&old_name, b"sentinel-old").expect("잔여 파일");
        std::fs::write(&new_name, b"sentinel-new").expect("잔여 파일");

        let warning = write_new(target.to_str().unwrap(), b"new", false, "T", "X").expect("쓰기");

        assert_eq!(warning, None);
        assert_eq!(std::fs::read(&target).expect("산출물"), b"new");
        assert_eq!(
            std::fs::read(&old_name).expect("★ 1차 구현 이름의 남의 파일이 사라졌다"),
            b"sentinel-old"
        );
        assert_eq!(
            std::fs::read(&new_name).expect("★ 지금 이름의 남의 파일이 사라졌다"),
            b"sentinel-new"
        );
    }

    /// ★ 결함 ⑰ — **먼저 지우지 않는다.** 교체가 실패하면 원본이 그대로 남는다.
    ///
    /// 1차 구현은 `remove_file(target)` 뒤 `rename` 했다. 교체가 실패하면
    /// 원본도 새것도 없다. 여기서는 **교체만** 주입해 실패시키고 삭제는
    /// 진짜로 둔다 — 대상을 먼저 지우는 구현은 이 테스트에서 원본을 잃는다.
    #[test]
    fn a_failed_replace_leaves_the_original_in_place() {
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let target = dir.path().join("x.pb");
        std::fs::write(&target, b"original").expect("원본");
        let fs = FsOps {
            remove: real_remove,
            rename: failing_rename,
        };

        let error = write_new_with(&fs, target.to_str().unwrap(), b"new", true, "T", "X")
            .expect_err("교체가 실패했는데 성공이라 했다");

        assert!(error.contains("주입한 교체 실패"), "{error}");
        assert_eq!(
            std::fs::read(&target).expect("★ 원본이 사라졌다"),
            b"original"
        );
        assert!(temps_left(dir.path()).is_empty(), "임시 파일이 남았다");
    }

    /// ★ 결함 ⑰ — hard link 뒤 임시 이름을 못 지우면 **성공이되 경고를
    /// 돌려준다.** 출력하지 않는다(`eprintln!` 은 패닉할 수 있다).
    #[test]
    fn a_leftover_link_is_reported_back_not_printed() {
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let target = dir.path().join("x.pb");
        let fs = FsOps {
            remove: failing_remove,
            rename: real_rename,
        };

        let warning = write_new_with(&fs, target.to_str().unwrap(), b"new", false, "T", "X")
            .expect("산출물은 확정됐다 — 실패로 돌리면 안 된다")
            .expect("못 지운 임시 링크를 말하지 않았다");

        assert!(
            warning.contains("임시 링크") && warning.contains("주입한 삭제 실패"),
            "{warning}"
        );
        assert_eq!(std::fs::read(&target).expect("산출물"), b"new");
        assert_eq!(
            with_warning("요약".into(), Some(warning.clone())),
            format!("요약\n{warning}")
        );
    }

    /// ★ 결함 ⑰ — 실패 경로에서 정리까지 실패하면 **두 오류를 다** 보고한다.
    #[test]
    fn a_failed_cleanup_is_added_to_the_error() {
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let target = dir.path().join("x.pb");
        std::fs::write(&target, b"original").expect("원본");
        let fs = FsOps {
            remove: failing_remove,
            rename: failing_rename,
        };

        let error = write_new_with(&fs, target.to_str().unwrap(), b"new", true, "T", "X")
            .expect_err("교체가 실패했는데 성공이라 했다");

        assert!(
            error.contains("주입한 교체 실패") && error.contains("주입한 삭제 실패"),
            "두 오류 중 하나를 묵었다: {error}"
        );
        assert_eq!(std::fs::read(&target).expect("원본"), b"original");
    }

    /// ★ 결함 ⑭ — 덮어쓰기 금지에서 **동시에** 쓰면 정확히 하나만 이긴다.
    ///
    /// 1차 구현은 `exists()` 로 보고 `rename` 했다. 둘 사이에 다른 쓰기가
    /// 끼면 **나중 것이 앞의 것을 말없이 덮는다.** 순차로 두 번 쓰는 기존
    /// 테스트는 그 구현도 통과시켰다. 여기서는 16개를 한꺼번에 풀어 놓는다.
    ///
    /// ★ 1차 구현은 여럿이 이길 **수 있다** — 스레드가 차례로 돌면 하나만
    ///   이긴다(재검수 13). 뮤테이션으로 3회 중 3회 잡았지만 "늘 잡는다" 의
    ///   증명은 아니다. 올바른 구현은 매번 통과해야 한다.
    #[test]
    fn concurrent_writers_without_overwrite_leave_exactly_one_winner() {
        const WRITERS: u8 = 16;
        for round in 0..20 {
            let dir = tempfile::tempdir().expect("임시 디렉터리");
            let target = dir.path().join("race.pb").to_str().unwrap().to_string();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS as usize));
            let handles: Vec<_> = (0..WRITERS)
                .map(|i| {
                    let barrier = barrier.clone();
                    let target = target.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        (i, write_new(&target, &[i; 64], false, "T", "X"))
                    })
                })
                .collect();
            let results: Vec<(u8, Result<Option<String>, String>)> =
                handles.into_iter().map(|h| h.join().expect("스레드")).collect();

            let winners: Vec<u8> = results
                .iter()
                .filter(|(_, r)| r.is_ok())
                .map(|(i, _)| *i)
                .collect();
            assert_eq!(winners.len(), 1, "round {round}: 이긴 쓰기가 {winners:?} 다");
            for (i, result) in &results {
                if let Err(e) = result {
                    assert!(e.contains("OUT_EXISTS"), "round {round} writer {i}: {e}");
                }
            }
            assert_eq!(
                std::fs::read(&target).expect("산출물"),
                vec![winners[0]; 64],
                "round {round}: 산출물이 이긴 쓰기의 것이 아니다"
            );
            let left = temps_left(dir.path());
            assert!(left.is_empty(), "round {round}: 임시 파일이 남았다 {left:?}");
        }
    }
}
