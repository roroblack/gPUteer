//! 서명된 산출물을 파일로 내보낸다 — **기존 파일을 조용히 망가뜨리지 않는다.**
//!
//! # 왜 이 모듈이 있나
//!
//! ★★ 2026-09-10 독립 검수가 `issue-grant` 에서 찾은 결함이다. 그 명령은
//!   `std::fs::write(out_path, ..)` 한 줄로 산출물을 내보냈고, 그것이 두
//!   가지를 한꺼번에 잘못했다:
//!
//!   1. **기존 파일을 말없이 덮었다.** 검수가 든 반례 — 운영자가 서명
//!      키를 잘못 주면 그 명령은 그것을 잡지 못하고(발급자 이름에 그 키의
//!      공개키를 등록해 자기 검증하므로 자기 검증도 못 잡는다) 성공을
//!      찍으면서 **멀쩡하던 산출물을 못 쓰는 것으로 바꿔 놓는다.**
//!   2. **먼저 자르고 쓴다.** 중간에 실패하면(디스크 참, 권한) 기존
//!      파일이 **잘린 채** 남는다. `CLAUDE.md` §0.3 이 경고하는 모양이다.
//!
//! ★★ **그리고 `submit` 에 똑같은 줄이 있었다.** 한쪽만 고치면 다음
//!   사람이 다른 쪽을 다시 발견한다 — 애초에 이 결함이 두 곳에 생긴
//!   방식이 그것이다. 그래서 도우미를 한 곳에 둔다.
//!
//! # 무엇을 하고 무엇을 안 하나
//!
//! ```text
//! 한다     기존 파일이 있으면 거부 (호출부가 명시적으로 허용할 때만 덮는다)
//! 한다     같은 디렉터리 임시 파일에 쓰고 rename 으로 확정
//! 안 한다  fsync — 이건 체크포인트가 아니다. 전원이 끊기면 다시 발급하면 된다
//!          (`crates/checkpoint` 의 `ADR-026` 절차는 그쪽에서 쓴다)
//! 안 한다  내용 검증 — 부르는 쪽이 이미 서명하고 자기 검증했다
//! ```

use std::path::Path;

/// 산출물을 원자적으로 내보낸다.
///
/// `code` 는 거부 메시지의 접두어다(`"SUBMIT_REFUSED"` 처럼). 명령마다
/// 다르므로 인자로 받는다 — 메시지 머리에 안정적인 코드를 두는 것은
/// 이 저장소의 관례이고, 테스트가 **줄 시작**으로 사유를 확인할 수
/// 있게 해 준다(2026-09-10 검수 지적).
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
    if target.exists() && !overwrite {
        return Err(format!(
            "{code}: OUT_EXISTS — {out_path} 에 파일이 이미 있다. 덮어쓰려면 명시적으로 허용해야 한다 (잘못된 인자로 멀쩡한 {what} 를 날리는 것을 막는다)"
        ));
    }
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        "{}.tmp.{}",
        target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "out".into()),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)
        .map_err(|e| format!("{code}: {what} 임시 파일 쓰기 실패({}): {e}", tmp.display()))?;
    if overwrite && target.exists() {
        // ★ Windows 는 대상이 있으면 rename 이 실패한다. 덮어쓰기를
        //   허용한 경우에만 먼저 지운다 — 그 순간의 창은 남지만 그건
        //   호출부가 명시적으로 요청한 덮어쓰기다.
        let _ = std::fs::remove_file(target);
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{code}: {what} 파일 확정 실패({out_path}): {e}")
    })?;
    Ok(())
}
