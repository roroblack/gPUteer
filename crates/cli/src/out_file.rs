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
//!             std 의 rename 은 두 플랫폼 다 대상을 원자적으로 교체한다
//!             (Windows 는 `MOVEFILE_REPLACE_EXISTING`)
//! ```
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
//! ```text
//! 안 한다  fsync — 이건 체크포인트가 아니다. 전원이 끊기면 다시 발급하면 된다
//!          (`crates/checkpoint` 의 `ADR-026` 절차는 그쪽에서 쓴다)
//! 안 한다  내용 검증 — 부르는 쪽이 이미 서명하고 자기 검증했다
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};

/// 산출물을 원자적으로 내보낸다.
///
/// `code` 는 거부 메시지의 접두어다(`"SUBMIT_REFUSED"` 처럼). 명령마다
/// 다르므로 인자로 받는다 — 메시지 머리에 안정적인 코드를 두는 것은
/// 이 저장소의 관례이고, 테스트가 **줄 시작**으로 사유를 확인할 수
/// 있게 해 준다.
///
/// `what` 은 사람이 읽을 산출물 이름(`"Manifest"`, `"Grant"`).
pub fn write_new(
    out_path: &str,
    bytes: &[u8],
    overwrite: bool,
    code: &str,
    what: &str,
) -> Result<(), String> {
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
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    let finalize = if overwrite {
        // ★ 먼저 지우지 않는다. rename 이 한 번에 교체한다.
        std::fs::rename(&tmp, target)
            .map_err(|e| format!("{code}: {what} 파일 확정 실패({out_path}): {e}"))
    } else {
        // ★ 대상이 있으면 여기서 실패한다 — 확인과 확정이 한 연산이다.
        match std::fs::hard_link(&tmp, target) {
            Ok(()) => {
                let _ = std::fs::remove_file(&tmp);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(format!(
                "{code}: OUT_EXISTS — {out_path} 에 파일이 이미 있다. 덮어쓰려면 명시적으로 허용해야 한다 (잘못된 인자로 멀쩡한 {what} 를 날리는 것을 막는다)"
            )),
            Err(e) => Err(format!(
                "{code}: {what} 파일 확정 실패({out_path}): {e} — 이 파일시스템이 hard link 를 지원하지 않을 수 있다"
            )),
        }
    };

    if finalize.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    finalize
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
