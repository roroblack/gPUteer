//! 원자적 파일 확정 — **ADR-026** 의 구현체.
//!
//! # 왜 플랫폼별로 나뉘는가
//!
//! P0-03a 실측 결과 Windows 는 POSIX 와 다르게 동작한다.
//!
//! | 시나리오 | rename 성공률 (Windows/NTFS) |
//! |---|---|
//! | 동시 독자 없음 | 3000 / 3000 |
//! | 독자가 `FILE_SHARE_DELETE` 없이 열기 | **313 / 3000** |
//! | POSIX 시맨틱 API (`FileRenameInfoEx`) | **87 / 1000** |
//!
//! `MoveFileEx` 는 열려 있는 대상 파일을 대체하지 못한다.
//! `FILE_RENAME_FLAG_POSIX_SEMANTICS` 로도 해결되지 않았다.
//!
//! **다만 `부분 내용` 관측은 전 시나리오 0건이었다.** 성공하면 원자적이고,
//! 실패하면 아무 일도 일어나지 않는다. 즉 무결성 위험이 아니라 가용성 위험이다.
//!
//! # ADR-026 의 해법
//!
//! 기준선 §18.6 은 "모든 artifact 는 immutable" 이라고 이미 선언했다.
//! 불변 파일은 덮어쓸 일이 없으므로 **고유 이름 write-once** 로 문제를 회피한다.
//! `replace-over-existing` 이 실제로 필요한 곳은 포인터 파일 하나뿐이며,
//! 작아서 재시도 비용이 무시할 만하다.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::CheckpointError;

/// 포인터 replace 재시도 정책 (ADR-026).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(50),
            max_backoff: Duration::from_secs(2),
        }
    }
}

/// ★ 체크포인트 상대 경로 검증 (2026-08-16 신설).
///
/// `proto/common.proto` 의 `CheckpointFile.path` 는 이렇게 적혀 있다.
///
/// > 체크포인트 루트 기준 상대 경로. `".."`, 절대경로, 심볼릭 링크 금지.
///
/// **그런데 아무도 검사하지 않았다.** 독립 검수(2026-08-16)가 찾았다.
///
/// ```text
/// files = [("../../outside.bin", attacker_data)]
/// => write_once 가 dir.join(name) 을 그대로 해서
///    체크포인트 디렉터리 **밖**에 파일을 만든다
/// ```
///
/// 파일 이름은 **매니페스트에서 오는 외부 입력**이다.
/// 서명된 매니페스트라도 서명자가 악의적일 수 있고,
/// `CLAUDE.md` §0 — "이 시스템은 **남의 개인 PC 에서** 코드를 돌린다."
///
/// # 허용하는 것
///
/// 하위 디렉터리는 허용한다 — `model/weights.safetensors` 같은 경로가
/// 실제 체크포인트에 있다.
///
/// # 막는 것
///
/// ```text
/// ..              어떤 위치에서든 상위로 올라가는 성분
/// 절대 경로       /foo  ·  C:oo  ·  \server\share
/// 루트 성분       Windows 의 드라이브 접두사 포함
/// 빈 이름
/// ```
///
/// ★ 심볼릭 링크는 **여기서 막지 못한다** — 경로 문자열만으로는 알 수 없다.
///   `write_once` 는 대상이 이미 존재하면 쓰지 않으므로 링크를 따라가 덮어쓰지는
///   않지만, 링크를 통해 **읽는** 것은 막지 못한다. 별도 스파이크가 필요하다.
fn validate_relative_name(name: &str) -> Result<(), CheckpointError> {
    use std::path::Component;

    if name.is_empty() {
        return Err(CheckpointError::UnsafePath {
            name: name.to_string(),
            reason: "빈 이름",
        });
    }

    let p = Path::new(name);
    for c in p.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(CheckpointError::UnsafePath {
                    name: name.to_string(),
                    reason: "상위 디렉터리 성분(..) — 체크포인트 밖으로 나간다",
                })
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(CheckpointError::UnsafePath {
                    name: name.to_string(),
                    reason: "절대 경로 — 상대 경로여야 한다",
                })
            }
        }
    }
    Ok(())
}

/// 임시 파일에 쓰고 fsync 한 뒤 고유 이름으로 확정한다.
///
/// **대상이 이미 존재하면 rename 을 시도하지 않는다.**
/// 이것이 ADR-026 의 핵심 — Windows 의 rename 실패를 원천 회피한다.
///
/// # ★ 이미 존재할 때 내용을 대조한다 (2026-08-16 시정)
///
/// 처음에는 이렇게 적혀 있었다.
///
/// > content-addressed 이름이므로 존재한다는 것은 내용이 같다는 뜻이다.
///
/// **그 전제가 지켜지지 않았다.** `writer.rs` 가 넘기는 이름은
/// `shard-0.bin` 같은 **위치 기반 이름**이지 content-addressed 가 아니다.
/// 독립 검수(2026-08-16)가 지적했다.
///
/// ```text
/// 1. writer-A 가 ckpt-100/shard-0.bin 을 쓴다
/// 2. 매니페스트 쓰기 전에 프로세스가 죽는다
/// 3. writer-B 가 같은 checkpoint_id 로 **다른 내용**을 쓴다
/// 4. 내용 비교 없이 Ok(false) -> writer-B 의 매니페스트에는 B 의 해시,
///    디스크에는 A 의 데이터
/// 5. write_checkpoint 가 **성공을 반환한다**
/// ```
///
/// `find_resume_point` 가 나중에 해시 불일치로 제외하므로 데이터 손상은 아니다.
/// 그러나 **writer 가 "확정했다" 고 거짓 보고한다** — `CLAUDE.md` §3 위반이다.
///
/// 이제 기존 파일을 읽어 대조하고, 다르면 [`CheckpointError::ContentMismatch`] 를 낸다.
/// **기존 파일은 덮어쓰지 않는다** (write-once 원칙).
///
/// 반환값: `true` = 새로 썼음, `false` = 같은 내용이 이미 있어 tmp 를 정리함
pub fn write_once(dir: &Path, name: &str, data: &[u8]) -> Result<bool, CheckpointError> {
    // ★ 무엇보다 먼저 — 경로 탈출을 막는다. tmp 파일을 만들기 전에 거른다.
    validate_relative_name(name)?;
    let final_path = dir.join(name);

    // 이미 존재하면 **내용을 대조한다.** 이름만으로 같다고 가정하지 않는다.
    if final_path.exists() {
        let existing = fs::read(&final_path)?;
        if existing == data {
            return Ok(false);
        }
        return Err(CheckpointError::ContentMismatch {
            path: final_path,
            existing_len: existing.len(),
            incoming_len: data.len(),
        });
    }

    let tmp_path = dir.join(format!("{name}.tmp"));

    {
        let mut f = File::create(&tmp_path)?;
        f.write_all(data)?;
        f.flush()?;
        f.sync_all()?; // fsync(file)
    }

    // 경쟁: 우리가 쓰는 사이에 다른 쪽이 확정했을 수 있다.
    if final_path.exists() {
        let _ = fs::remove_file(&tmp_path);
        return Ok(false);
    }

    fs::rename(&tmp_path, &final_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        CheckpointError::Rename {
            from: tmp_path.clone(),
            to: final_path.clone(),
            source: e,
        }
    })?;

    sync_dir(dir)?;
    Ok(true)
}

/// 기존 파일을 대체한다. **포인터 파일 전용.**
///
/// 데이터 파일에는 쓰지 않는다 — `write_once` 를 쓴다.
/// Windows 에서 외부 프로세스(백신·인덱서)가 대상을 열고 있으면 실패할 수 있으므로
/// 지수 백오프로 재시도한다.
pub fn replace_with_retry(
    dir: &Path,
    name: &str,
    data: &[u8],
    policy: RetryPolicy,
) -> Result<(), CheckpointError> {
    validate_relative_name(name)?;

    // ★ 정책 검증을 **tmp 파일 생성 전에** 한다 (독립 검수 2026-08-16 2차).
    //   나중에 하면 디렉터리가 없거나 권한이 없을 때
    //   InvalidRetryPolicy 대신 I/O 오류가 나와 진단이 흐려진다.
    if policy.max_attempts == 0 {
        return Err(CheckpointError::InvalidRetryPolicy);
    }

    let final_path = dir.join(name);
    let tmp_path = dir.join(format!("{name}.tmp"));

    {
        let mut f = File::create(&tmp_path)?;
        f.write_all(data)?;
        f.flush()?;
        f.sync_all()?;
    }

    // (max_attempts == 0 검사는 위에서 이미 했다 — tmp 생성 전에 거른다)
    let mut backoff = policy.initial_backoff;
    let mut last_err = None;

    for attempt in 0..policy.max_attempts {
        match fs::rename(&tmp_path, &final_path) {
            Ok(()) => {
                sync_dir(dir)?;
                return Ok(());
            }
            Err(e) => {
                last_err = Some(e);
                if attempt + 1 < policy.max_attempts {
                    std::thread::sleep(backoff);
                    // ★ Duration 곱셈은 overflow 시 panic 한다. RetryPolicy 가 공개 구조체이므로
                    //   Duration::MAX 를 넣을 수 있다 (독립 검수 2026-08-16 2차).
                    backoff = backoff
                        .checked_mul(2)
                        .unwrap_or(policy.max_backoff)
                        .min(policy.max_backoff);
                }
            }
        }
    }

    let _ = fs::remove_file(&tmp_path);
    // 재시도 소진은 명시적 오류다. 조용히 넘어가지 않는다 (RULE.md §3.2).
    Err(CheckpointError::ReplaceExhausted {
        path: final_path,
        attempts: policy.max_attempts,
        source: last_err.expect("최소 1회는 시도한다"),
    })
}

/// 디렉터리 엔트리를 디스크에 확정한다.
///
/// P0-03a 발견: **Windows 는 쓰기 권한이 있어야 한다.**
/// `GENERIC_READ` 로만 열면 `FlushFileBuffers` 가 `ACCESS_DENIED(5)` 로 실패한다.
/// 1차 조사에서 이 함정에 빠져 "Windows 에는 디렉터리 fsync 가 없다" 로
/// 오판했다가 권한 조합을 늘려 재측정해 정정했다.
#[cfg(unix)]
pub fn sync_dir(dir: &Path) -> Result<(), CheckpointError> {
    let f = File::open(dir)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(windows)]
pub fn sync_dir(dir: &Path) -> Result<(), CheckpointError> {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_FLAG_BACKUP_SEMANTICS — 디렉터리를 열기 위해 필수
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    let f = OpenOptions::new()
        .read(true)
        .write(true) // ← 필수. 없으면 ACCESS_DENIED(5)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub fn sync_dir(_dir: &Path) -> Result<(), CheckpointError> {
    Ok(())
}

/// 매니페스트 없이 남은 데이터 파일과 `.tmp` 를 정리한다.
///
/// 기준선 §18.2 규칙 3 — 매니페스트가 마지막에 쓰이므로,
/// 매니페스트가 없으면 그 체크포인트는 PARTIAL 이다.
///
/// 반환값: 삭제한 파일 경로
pub fn gc_partial(checkpoint_dir: &Path, manifest_name: &str) -> Result<Vec<PathBuf>, CheckpointError> {
    let mut removed = Vec::new();
    if !checkpoint_dir.is_dir() {
        return Ok(removed);
    }

    let manifest_exists = checkpoint_dir.join(manifest_name).exists();

    for entry in fs::read_dir(checkpoint_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();

        // .tmp 는 항상 미완성이다
        let is_tmp = name.ends_with(".tmp");
        // 매니페스트가 없으면 전체가 PARTIAL
        let orphan = !manifest_exists;

        if is_tmp || orphan {
            fs::remove_file(&path)?;
            removed.push(path);
        }
    }
    Ok(removed)
}

/// io::Error 를 그대로 노출하기 위한 변환.
impl From<io::Error> for CheckpointError {
    fn from(e: io::Error) -> Self {
        CheckpointError::Io(e.to_string())
    }
}
