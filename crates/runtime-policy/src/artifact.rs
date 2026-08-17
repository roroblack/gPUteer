//! `ArtifactScope`(job.proto 필드 55) 강제 — Job 이 쓰기 가능한 경로.
//!
//! # 강제 가능성
//!
//! **Enforceable — 경로 문자열 검사** 수준에서.
//!
//! ★ **이것은 파일시스템 강제가 아니다.** `scripts/verify_evidence.py` 의
//! `safe_repo_path` 가 같은 문제를 다뤘고 거기서 배운 것을 재사용하되,
//! 차이를 분명히 한다.
//!
//! ```text
//! 이 모듈이 하는 것        요청된 경로 문자열이 허용 접두사 안에 있는가
//!                          (.. 탈출 · 절대경로 · NTFS ADS 문자열 검사)
//!
//! 이 모듈이 하지 못하는 것  검사와 실제 쓰기 사이의 TOCTOU.
//!                          검사 통과 직후 그 경로를 symlink 로 바꿔치기하면
//!                          이 검사는 무력하다.
//!                          진짜 강제는 openat2(RESOLVE_BENEATH|NO_SYMLINKS)
//!                          (Linux) 나 Windows 재분석 지점 차단 핸들이
//!                          필요하다 — 이 크레이트에는 없다.
//! ```
//!
//! `check()` 를 "안전하다" 의 증명으로 쓰지 않는다. **명백히 잘못된 요청을
//! 조기에 거부하는 문자열 필터**로만 쓴다.

use std::path::Path;

/// artifact_scope 위반.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactViolation {
    /// `..` 로 허용 범위를 벗어나려 했다.
    PathTraversal { requested: String },
    /// 절대 경로다. 허용 접두사는 항상 상대 경로다.
    AbsolutePath { requested: String },
    /// 어떤 허용 접두사와도 맞지 않는다.
    OutsideAllowedPrefixes { requested: String },
    /// NTFS Alternate Data Stream 문자열(`file.txt:stream`).
    ///
    /// ★ 이 검사는 **문자열 수준**이다. 이 프로세스가 실제로 Windows 에서
    ///   도는지, ADS 가 그 파일시스템에서 의미가 있는지는 보지 않는다 —
    ///   콜론을 포함한 경로는 어느 플랫폼에서든 거부한다. 보수적인
    ///   방향이 안전한 방향이다.
    NtfsAlternateDataStream { requested: String },
    /// 경로에 보이지 않는/제어 문자가 있다.
    InvisibleCharacters { requested: String },
    /// ★ ASCII 가 아닌 문자가 있다 (독립 검수 2026-08-17 · 중대).
    ///
    /// 검수자가 실제로 우회를 만들었다: `．．`(U+FF0E 전각 마침표 두 개)
    /// 는 ASCII `..` 와 눈으로 구분되지 않지만 이 코드의 `p == ".."`
    /// 비교(정확한 ASCII 매칭)를 통과한다. 키릴 `о` 같은 동형 문자도
    /// 같은 문제다.
    ///
    /// 경로 요소를 ASCII 로 제한하면 이 범주의 우회를 전부 닫는다 —
    /// 정당한 artifact 경로가 비 ASCII 파일명을 필요로 할 이유는 없다.
    NonAscii { requested: String },
    /// ★ Windows 예약 장치 이름 (`CON` · `NUL` · `AUX` · `PRN` ·
    /// `COM1`~`COM9` · `LPT1`~`LPT9`, 확장자가 붙어도 동일하게 취급).
    ///
    /// Windows 는 이 이름들을 일반 파일이 아니라 장치로 해석한다.
    /// `NUL` 에 쓰면 조용히 버려지고, `CON` 은 콘솔을 연다 — 검수자가
    /// 지적했다.
    WindowsReservedName { requested: String, component: String },
    /// ★ 끝에 점·공백이 있는 경로 요소.
    ///
    /// Win32 경로 처리는 trailing dot/space 를 **조용히 제거**한다.
    /// `model.bin. ` 로 쓰기 검사를 통과시키고 실제로는 `model.bin` 을
    /// 건드리면, 검사한 이름과 실제로 영향받는 이름이 달라진다.
    TrailingDotOrSpace { requested: String },
}

impl std::fmt::Display for ArtifactViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathTraversal { requested } => {
                write!(f, "경로 탈출(..): {requested}")
            }
            Self::AbsolutePath { requested } => write!(f, "절대 경로: {requested}"),
            Self::OutsideAllowedPrefixes { requested } => {
                write!(f, "허용 접두사 밖: {requested}")
            }
            Self::NtfsAlternateDataStream { requested } => {
                write!(f, "경로에 콜론(ADS 가능성): {requested}")
            }
            Self::InvisibleCharacters { requested } => {
                write!(f, "경로에 보이지 않는 문자: {requested}")
            }
            Self::NonAscii { requested } => {
                write!(f, "경로에 ASCII 가 아닌 문자가 있다(동형 문자 우회 방지): {requested}")
            }
            Self::WindowsReservedName { requested, component } => {
                write!(f, "Windows 예약 장치 이름({component}): {requested}")
            }
            Self::TrailingDotOrSpace { requested } => {
                write!(f, "경로 요소 끝에 점/공백이 있다(Win32 가 조용히 제거한다): {requested}")
            }
        }
    }
}

impl std::error::Error for ArtifactViolation {}

/// 제로폭·서식·제어 문자.
///
/// `scripts/verify_evidence.py` 의 `INVISIBLE_RE` 와 같은 범위를 쓴다 —
/// 신원/경로 문자열에서 반복해서 나오는 공격 범주라 기준을 통일한다.
fn has_invisible_chars(s: &str) -> bool {
    s.chars().any(|c| {
        let cp = c as u32;
        (0x00..=0x1f).contains(&cp)
            || (0x7f..=0x9f).contains(&cp)
            || (0x200b..=0x200f).contains(&cp)
            || (0x2028..=0x202e).contains(&cp)
            || (0x2060..=0x206f).contains(&cp)
            || cp == 0xfeff
    })
}

/// `Verified<pb::JobManifest>.artifact_scope` 에서 나온 허용 접두사.
///
/// ★ 이 타입은 `Verified<M>` 에서 나온 값을 받는다는 **전제**로 설계됐다.
///   검증 전 필드로 만들면 `CLAUDE.md` §0.2 를 어긴다. 이 타입 자체는
///   그 전제를 강제하지 않는다 — 호출자가 `Verified` 게이트를 통과한
///   값에서만 만들 책임이 있다.
pub struct ArtifactPolicy {
    writable_prefixes: Vec<String>,
}

/// Windows 예약 장치 이름. 대소문자 구분 없이, 확장자가 붙어도 매칭한다
/// (`nul`, `NUL.txt` 모두 예약어다).
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_windows_reserved_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    WINDOWS_RESERVED_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
}

impl ArtifactPolicy {
    /// ★ 2026-08-17 정정 (독립 검수). 전에는 접두사를 검증 없이 받았다 —
    ///   빈 문자열 접두사가 들어오면 `check("")` · `check(".")` 이
    ///   통과했다. 지금은 의미 없는 접두사(빈 문자열 · `.`)를 걸러낸다.
    ///   그런 접두사가 "허용 범위" 로 쓰이면 사실상 아무 제약이 없다.
    pub fn new(writable_prefixes: Vec<String>) -> Self {
        let writable_prefixes = writable_prefixes
            .into_iter()
            .filter(|p| {
                let trimmed = p.trim_matches('/');
                !trimmed.is_empty() && trimmed != "."
            })
            .collect();
        Self { writable_prefixes }
    }

    /// 요청된 상대 경로가 허용 범위 안인지 **문자열로만** 판정한다.
    ///
    /// TOCTOU 를 막지 못한다는 것은 모듈 문서를 참조한다.
    ///
    /// ★ 2026-08-17 강화 (독립 검수가 실제로 우회를 만들었다):
    ///   - ASCII 외 문자를 전부 거부한다 (전각 마침표 `．．` 가
    ///     `..` 로 안 보이지만 순회 목적으로 쓰일 수 있었다)
    ///   - Windows 예약 장치 이름(`NUL`·`CON` 등)을 거부한다
    ///   - 끝에 점/공백이 있는 요소를 거부한다 (Win32 가 조용히 없앤다)
    pub fn check(&self, requested: &str) -> Result<(), ArtifactViolation> {
        if has_invisible_chars(requested) {
            return Err(ArtifactViolation::InvisibleCharacters {
                requested: requested.to_string(),
            });
        }

        if !requested.is_ascii() {
            return Err(ArtifactViolation::NonAscii {
                requested: requested.to_string(),
            });
        }

        let normalized = requested.replace('\\', "/");

        if normalized.contains(':') {
            return Err(ArtifactViolation::NtfsAlternateDataStream {
                requested: requested.to_string(),
            });
        }

        if normalized.starts_with('/') || Path::new(&normalized).is_absolute() {
            return Err(ArtifactViolation::AbsolutePath {
                requested: requested.to_string(),
            });
        }

        for raw_component in normalized.split('/') {
            // ★ ".." 는 그 자체가 예약된 상대 경로 토큰이다 — Win32 의
            //   trailing-dot 제거 규칙 대상이 아니다(구조적으로 특별
            //   취급된다). 여기서 빼지 않으면 ".." 가 "끝에 점이 있다" 로
            //   먼저 걸려 PathTraversal 대신 TrailingDotOrSpace 로 오분류된다.
            if raw_component.is_empty() || raw_component == "." || raw_component == ".." {
                continue;
            }
            if raw_component.ends_with('.') || raw_component.ends_with(' ') {
                return Err(ArtifactViolation::TrailingDotOrSpace {
                    requested: requested.to_string(),
                });
            }
            if is_windows_reserved_name(raw_component) {
                return Err(ArtifactViolation::WindowsReservedName {
                    requested: requested.to_string(),
                    component: raw_component.to_string(),
                });
            }
        }

        let parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
        if parts.iter().any(|p| *p == "..") {
            return Err(ArtifactViolation::PathTraversal {
                requested: requested.to_string(),
            });
        }
        let rebuilt = parts.join("/");

        // ★ 빈 요청(""·"."만 남은 경우)은 어떤 접두사로도 정당화되지
        //   않는다 — write 대상은 항상 구체적인 이름이 있어야 한다.
        if rebuilt.is_empty() {
            return Err(ArtifactViolation::OutsideAllowedPrefixes {
                requested: requested.to_string(),
            });
        }

        let allowed = self
            .writable_prefixes
            .iter()
            .any(|prefix| {
                let prefix = prefix.trim_end_matches('/');
                rebuilt == prefix || rebuilt.starts_with(&format!("{prefix}/"))
            });

        if !allowed {
            return Err(ArtifactViolation::OutsideAllowedPrefixes {
                requested: requested.to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ArtifactPolicy {
        ArtifactPolicy::new(vec!["jobs/job-1/attempt-1".to_string()])
    }

    #[test]
    fn artifact_path_traversal_is_rejected() {
        let p = policy();
        let r = p.check("jobs/job-1/attempt-1/../../../etc/passwd");
        assert!(
            matches!(r, Err(ArtifactViolation::PathTraversal { .. })),
            "{r:?}"
        );
    }

    #[test]
    fn artifact_ntfs_ads_is_rejected() {
        let p = policy();
        let r = p.check("jobs/job-1/attempt-1/model.bin:hidden");
        assert!(
            matches!(r, Err(ArtifactViolation::NtfsAlternateDataStream { .. })),
            "{r:?}"
        );
    }

    #[test]
    fn artifact_absolute_path_is_rejected() {
        let p = policy();
        let r = p.check("/etc/passwd");
        assert!(matches!(r, Err(ArtifactViolation::AbsolutePath { .. })), "{r:?}");
    }

    #[test]
    fn artifact_outside_prefix_is_rejected() {
        let p = policy();
        let r = p.check("jobs/job-2/attempt-1/model.bin");
        assert!(
            matches!(r, Err(ArtifactViolation::OutsideAllowedPrefixes { .. })),
            "{r:?}"
        );
    }

    #[test]
    fn artifact_invisible_chars_are_rejected() {
        let p = policy();
        let r = p.check("jobs/job-1/attempt-1/model\u{200b}.bin");
        assert!(
            matches!(r, Err(ArtifactViolation::InvisibleCharacters { .. })),
            "{r:?}"
        );
    }

    /// 비공허성 — 실제로 허용 범위 안인 경로는 통과해야 한다.
    #[test]
    fn artifact_within_prefix_is_allowed() {
        let p = policy();
        assert!(p.check("jobs/job-1/attempt-1/model.bin").is_ok());
        assert!(p.check("jobs/job-1/attempt-1").is_ok());
    }

    /// ★ 이 검사가 **못 막는 것**을 스스로 고정한다.
    ///
    /// 접두사 검사를 통과한 경로가 실제로는 symlink 를 통해 밖을
    /// 가리킬 수 있다 — 이 모듈은 그것을 볼 방법이 없다(문자열만 본다).
    /// 이 테스트는 "문자열은 통과한다" 를 확인해 모듈의 한계를 코드로
    /// 남긴다. **통과가 안전을 뜻하지 않는다.**
    #[test]
    fn string_check_alone_cannot_see_symlink_targets() {
        let p = policy();
        // 이 크레이트에는 실제 파일시스템 접근이 없으므로 symlink 를
        // 만들 수도, 만들지 않을 수도 없다는 것 자체가 한계를 보여준다.
        // 문자열만으로는 "jobs/job-1/attempt-1/link" 가 symlink 인지
        // 일반 파일인지 구분할 수 없다.
        assert!(
            p.check("jobs/job-1/attempt-1/link").is_ok(),
            "★ symlink 여부와 무관하게 문자열 검사는 통과한다 — TOCTOU 를 막지 못한다"
        );
    }

    // ══════════════════════════════════════════════════════════════
    // ★ 독립 검수(2026-08-17)가 실제로 통과시킨 우회들 — 이제 거부한다
    // ══════════════════════════════════════════════════════════════

    #[test]
    fn artifact_fullwidth_dots_do_not_bypass_traversal_check() {
        let p = policy();
        // U+FF0E 전각 마침표 두 개 — 눈으로 ".." 처럼 보이지만
        // 예전 코드의 정확한 ASCII 비교(`p == ".."`)를 통과했다.
        let r = p.check("jobs/job-1/attempt-1/\u{ff0e}\u{ff0e}/model.bin");
        assert!(matches!(r, Err(ArtifactViolation::NonAscii { .. })), "{r:?}");
    }

    #[test]
    fn artifact_cyrillic_homoglyph_is_rejected() {
        let p = policy();
        // 키릴 'о'(U+043E) — 라틴 'o' 처럼 보인다.
        let r = p.check("jobs/job-1/attempt-1/m\u{043e}del.bin");
        assert!(matches!(r, Err(ArtifactViolation::NonAscii { .. })), "{r:?}");
    }

    #[test]
    fn artifact_windows_reserved_device_names_are_rejected() {
        let p = policy();
        for name in ["NUL", "CON.txt", "nul", "aux", "COM1", "lpt9"] {
            let full = format!("jobs/job-1/attempt-1/{name}");
            let r = p.check(&full);
            assert!(
                matches!(r, Err(ArtifactViolation::WindowsReservedName { .. })),
                "{name} 이 거부되지 않았다: {r:?}"
            );
        }
    }

    #[test]
    fn artifact_trailing_dot_or_space_is_rejected() {
        let p = policy();
        let r1 = p.check("jobs/job-1/attempt-1/model.bin.");
        assert!(matches!(r1, Err(ArtifactViolation::TrailingDotOrSpace { .. })), "{r1:?}");
        let r2 = p.check("jobs/job-1/attempt-1/model.bin ");
        assert!(matches!(r2, Err(ArtifactViolation::TrailingDotOrSpace { .. })), "{r2:?}");
    }

    #[test]
    fn empty_prefix_no_longer_allows_empty_or_dot_requests() {
        // ★ 전에는 빈 문자열 접두사가 check("") 와 check(".") 를 통과시켰다.
        let p = ArtifactPolicy::new(vec!["".to_string()]);
        assert!(p.check("").is_err(), "빈 요청이 통과했다");
        assert!(p.check(".").is_err(), "'.' 요청이 통과했다");
        assert!(p.check("./.").is_err(), "'./.'  요청이 통과했다");
    }

    #[test]
    fn dot_prefix_is_filtered_out_at_construction() {
        // "." 접두사는 생성자에서 걸러진다 — 남아 있으면 모든 상대 경로가
        // 통과하는 것과 다를 바 없다.
        let p = ArtifactPolicy::new(vec![".".to_string()]);
        assert!(
            p.check("anything/at/all").is_err(),
            "'.' 접두사가 살아남아 모든 경로를 허용했다"
        );
    }

    /// 비공허성 — 위 강화가 정상 파일명까지 막지 않는다.
    #[test]
    fn normal_ascii_filenames_with_dots_still_work() {
        let p = policy();
        assert!(p.check("jobs/job-1/attempt-1/model.v2.bin").is_ok());
        assert!(p.check("jobs/job-1/attempt-1/checkpoint-00010.ckpt").is_ok());
    }
}
