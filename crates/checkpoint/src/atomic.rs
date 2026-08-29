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
///   않지만, 링크를 통해 **읽는** 것은 막지 못한다.
///
///   ★ 그 "별도 스파이크" 는 이미 있다 — `crates/runtime-windows/src/beneath.rs` 의
///   `open_beneath`/`open_artifact` 가 reparse point(symlink·junction)를 **열기
///   시점에** 거부한다(TOCTOU 방어). 다만 이 크레이트는 아직 그것을 쓰지 않는다.
///   연결은 checkpoint 가 플랫폼 크레이트에 의존하게 만드는 계층 결정이라
///   별도 조각이다. 스파이크를 다시 만들지 마라.
fn validate_relative_name(name: &str) -> Result<(), CheckpointError> {
    use std::path::Component;

    if name.is_empty() {
        return Err(CheckpointError::UnsafePath {
            name: name.to_string(),
            reason: "빈 이름",
        });
    }

    // ★ `.write_once.lock` 접미사는 예약돼 있다(2026-08-19, 코덱스
    //   독립 검수 `p130` 이 지적한 근본 원인). `write_once(dir, "foo", ..)`
    //   의 락 파일은 `dir/foo.write_once.lock` 이다 — 이 이름 자체를
    //   **데이터 파일 이름**으로 허용하면, `write_once(dir,
    //   "foo.write_once.lock", data)` 로 만든 진짜 데이터 파일과
    //   `write_once(dir, "foo", ..)` 의 락 파일이 **정확히 같은
    //   경로**를 가리키게 된다. 그러면 "foo" 쓰기가 성공할 때 자기
    //   락 파일을 정리하는 코드가 그 데이터 파일을 그대로 지워버린다
    //   — 이름공간이 진짜로 충돌한다. `.tmp` 접미사는 GC 쪽에서
    //   "등록됐으면 보존" 으로 방어하지만, 이건 애초에 경로 자체가
    //   같아지는 문제라 등록 여부로는 못 막는다 — 이름을 아예
    //   금지한다.
    //
    //   ★ 대소문자 · 후행 점/공백 무시(2026-08-19, 코덱스 독립 검수
    //   `p131` 이 지적) — NTFS 는 대소문자를 구분하지 않고
    //   (`foo.write_once.LOCK` 도 같은 파일), Win32 파일 API 는 레거시
    //   DOS 호환을 위해 마지막 경로 성분의 **후행 점·공백을 자동으로
    //   잘라낸다**(`foo.write_once.lock.` 이나 `foo.write_once.lock `
    //   도 결국 `foo.write_once.lock` 을 가리킨다). 둘 다 문자열
    //   비교만으로는 안 보이므로, 검사 전에 정규화한다 — Linux(둘 다
    //   구분하는 파일시스템)에서는 과잉 차단이지만, 그쪽에서 이 이름을
    //   추가로 예약하는 비용은 거의 0이다.
    let normalized = name.trim_end_matches(['.', ' ']).to_ascii_lowercase();
    if normalized.ends_with(".write_once.lock") {
        return Err(CheckpointError::UnsafePath {
            name: name.to_string(),
            reason: "'.write_once.lock' 접미사(대소문자·후행 점/공백 무관)는 write_once() \
                     자신의 락 파일 전용으로 예약돼 있다",
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
///
/// ★ **동시 동일-이름 호출은 지원하지 않는다** — 명시적으로 거부한다
///   (2026-08-19, `docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md`).
///   과거에 이 계약이 없어 여러 호출자가 같은 `{name}.tmp` 를 공유해
///   경쟁했다(`DoD-08` 의 "발견했으나 고치지 않은 더 넓은 결함" 절,
///   `ENV-03` 이 Linux 에서 증상만 다르게 재확인). 실제 in-tree
///   호출부(`writer.rs`)는 전부 순차 호출이고, 이 프로젝트의
///   Lease/fencing 은 파일 쓰기 상호 배제를 보장하지 않는다
///   (`proto/lease.proto:12-18` 가 이미 그렇게 명시한다) — 그래서
///   "지원 안 함을 명시하고 강제"하는 쪽을 택했다. `flock`/`LockFileEx`
///   기반 프로세스 간 파일 잠금(`std::fs::File::try_lock`, Rust
///   1.89+ 안정화)으로 강제한다 — 잠금은 프로세스가 죽으면 OS 가
///   자동 해제하므로 crash 후에도 영구히 막히지 않는다. 인메모리
///   뮤텍스는 다중 프로세스 시나리오를 못 막으므로 쓰지 않았다.
pub fn write_once(dir: &Path, name: &str, data: &[u8]) -> Result<bool, CheckpointError> {
    // ★ 무엇보다 먼저 — 경로 탈출을 막는다. tmp 파일을 만들기 전에 거른다.
    validate_relative_name(name)?;
    let final_path = dir.join(name);

    // ★ 락은 첫 존재 검사보다 먼저 잡는다 — 그래야 동시 호출자
    //   전체(존재 검사 -> tmp 쓰기 -> rename)가 직렬화된다.
    //   `try_lock()` 은 즉시 실패한다 — 무기한 대기는 장애를 숨긴다.
    let lock_path = dir.join(format!("{name}.write_once.lock"));
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    if let Err(e) = lock_file.try_lock() {
        return match e {
            fs::TryLockError::WouldBlock => {
                Err(CheckpointError::WriteInProgress { path: final_path })
            }
            fs::TryLockError::Error(source) => Err(CheckpointError::Io(source.to_string())),
        };
    }
    // `lock_file` 은 함수가 반환할 때(모든 경로에서) drop 되며 OS 가
    // 잠금을 자동 해제한다 — 명시적 unlock 이 필요 없다.
    //
    // ★ **성공** 경로에서는 락 파일 자체도 정리한다(아래 `cleanup_lock`
    //   클로저, 2026-08-19 — 첫 구현은 락 파일을 영원히 남겼는데,
    //   코덱스 독립 검수 `p128` 이 그 결과로 완결된(매니페스트까지
    //   있는) 체크포인트 디렉터리에도 락 파일이 계속 쌓인다는 걸
    //   지적했다. **실패**(`ContentMismatch`, `?` 로 전파되는 I/O
    //   오류) 경로에서는 정리하지 않는다 — 그 경우 이 디렉터리는
    //   매니페스트를 절대 못 받아 PARTIAL 로 남고, `gc_partial` 이
    //   나중에(아무도 안 쥔 락만) 정리한다. 성공 시 여기서 먼저
    //   지우면 완결된 체크포인트에는 애초에 락 파일이 남지 않는다.
    let cleanup_lock = |lock_file: File| {
        drop(lock_file);
        let _ = fs::remove_file(&lock_path);
    };

    // 이미 존재하면 **내용을 대조한다.** 이름만으로 같다고 가정하지 않는다.
    if final_path.exists() {
        let existing = fs::read(&final_path)?;
        if existing == data {
            cleanup_lock(lock_file);
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
    //
    // ★ 여기도 내용을 대조한다(2026-08-18, DoD-08 schema v2 승격 재검수
    //   에서 발견 — 위 "이미 존재할 때" 분기만 고치고 이 분기는
    //   그대로 뒀었다. 같은 위치 기반 이름 문제가 경쟁 경로에서
    //   그대로 재현된다: 승자의 내용을 확인하지 않고 Ok(false) 를
    //   반환하면, 패자가 다른 내용을 썼다는 사실이 조용히 사라진다).
    if final_path.exists() {
        let winner = fs::read(&final_path)?;
        let _ = fs::remove_file(&tmp_path);
        if winner == data {
            cleanup_lock(lock_file);
            return Ok(false);
        }
        return Err(CheckpointError::ContentMismatch {
            path: final_path,
            existing_len: winner.len(),
            incoming_len: data.len(),
        });
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
    cleanup_lock(lock_file);
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

/// 매니페스트에 등록되지 않은 `.tmp` 와
/// 매니페스트가 없는 체크포인트의 파일을 정리한다.
///
/// 매니페스트에 등록된 `weights.tmp` 는 정상 데이터 파일일 수 있다.
/// 따라서 `.tmp`라는 접미사만으로 삭제하지 않는다.
/// 이 오류가 **정상적인 동시 실행 경합**의 흔적인가.
///
/// # ★ Windows 는 `NotFound` 만 내지 않는다 (2026-08-17, 두 번 실측으로 정정)
///
/// 두 프로세스(또는 `--workspace` 부하에서의 두 테스트)가 같은 파일/디렉터리를
/// 동시에 건드리면 Windows 는 `NotFound` 가 아니라
/// **`액세스가 거부되었습니다`(os error 5)** 를 낼 수 있다. 다른 쪽이
/// 마지막 핸들을 닫을 때까지 대상이 "삭제 중" 으로 여전히 보이기 때문이다.
///
/// ```text
/// 1차  NotFound 만 경합으로 봤다                    -> 부하에서 실패
/// 2차  오류 뒤 path.exists() 로 판별 (한 번만)       -> 5회 중 1회 여전히 실패
///      (writer.rs 의 startup_gc 루프만 고치고,
///       atomic.rs 의 gc_partial 은 못 고쳤다 — 결함이 두 곳에 나뉘어 있었다)
/// 3차  이 함수로 **재시도**하고, 두 파일이 공유한다 (지금)
/// ```
///
/// **한 번의 확인은 추측이고, 재시도는 사실이다.** 예산(약 200ms)을 넘기면
/// `CLAUDE.md` §3 에 따라 오류를 조용히 삼키지 않고 그대로 올린다.
pub(crate) fn is_windows_delete_race(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(5) // ERROR_ACCESS_DENIED
}

/// I/O 연산 하나를 **경합을 견디며** 재시도한다.
///
/// `NotFound` 는 즉시 "없다" 로 본다(더 기다릴 이유가 없다).
/// 그 외 오류는 [`is_windows_delete_race`] 로 보이면 짧게 재시도하고,
/// 예산을 다 쓰면 마지막 오류를 그대로 올린다.
pub(crate) fn retry_tolerating_race<T>(
    mut op: impl FnMut() -> io::Result<T>,
) -> io::Result<Option<T>> {
    const ATTEMPTS: usize = 10;
    const WAIT: Duration = Duration::from_millis(20);

    let mut last = None;
    for attempt in 0..ATTEMPTS {
        match op() {
            Ok(v) => return Ok(Some(v)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) if is_windows_delete_race(&e) => {
                last = Some(e);
                if attempt + 1 < ATTEMPTS {
                    std::thread::sleep(WAIT);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.expect("ATTEMPTS 가 0이 아니면 마지막 오류가 있다"))
}

/// ★ 알려진 한계 — 이 함수는 **디렉터리 전체를 보호하지 않는다**
/// (2026-08-19, 코덱스 독립 검수 `p129` 이 지적, 의도적으로 고치지
/// 않음).
///
/// `write_once()` 의 락은 그 호출이 쓰고 있는 **한 파일**만 보호한다.
/// 그런데 매니페스트가 없는 디렉터리(PARTIAL)에서는 이 함수가
/// "락이 걸려 있지 않은" 다른 모든 파일 — 활성 writer 가 지금 막
/// 쓰고 있는 `.tmp` 파일까지 포함해서 — 을 무조건 지운다. 즉 GC 가
/// 활성 writer 와 같은 디렉터리에서 동시에 돌면, writer 가 쓰고 있는
/// 중인 `.tmp` 를 GC 가 지워버릴 수 있다 — writer 의 최종 파일 락은
/// 안전하지만, **그 writer 의 다른 파일까지는 안전하지 않다.**
///
/// 이건 이번 조각(`write_once()` 자체의 동시-동일-이름 계약)의 범위
/// 밖이다 — 계획 문서(`docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md`)
/// 가 이미 "`write_checkpoint()` 전체를 하나의 락으로 감싸는 것은
/// 범위 밖" 이라고 명시했는데, 이 finding 은 정확히 그 더 넓은
/// 문제의 한 증상이다. 진짜 해법은 checkpoint 디렉터리 단위 락(GC
/// 시작 시 그 락을 잡고, `write_checkpoint()` 도 시작 시 잡는 것)
/// 이지 `write_once()` 하나만 고쳐서 될 일이 아니다.
///
/// 지금 실제 호출부(`startup_gc`)는 **프로세스 부팅 시 한 번만**
/// 돈다 — 이 프로세스가 아직 아무것도 쓰기 전이다. 다른 프로세스가
/// 같은 루트에 동시에 쓰는 시나리오는 다중 Agent 실행이 아직
/// 없어 실제 호출 경로가 없다(같은 논리로 이 계획 문서가 "진짜
/// 동시 쓰기 지원" 도 범위 밖으로 미뤘다). 다중 Agent/다중 프로세스가
/// 같은 checkpoint 루트에 동시 접근하기 시작하는 시점이 이 문제를
/// 다시 열어야 할 트리거다.
pub fn gc_partial(
    checkpoint_dir: &Path,
    manifest_name: &str,
) -> Result<Vec<PathBuf>, CheckpointError> {
    let metadata = match retry_tolerating_race(|| fs::symlink_metadata(checkpoint_dir))? {
        Some(m) => m,
        None => return Ok(Vec::new()), // 경합 — 이미 사라졌다
    };

    if !metadata.is_dir() {
        return Ok(Vec::new());
    }

    // 이름 기반 사전 존재 검사를 두지 않는다 — 검사와 열기 사이에 대상이
    // 바뀔 수 있다(TOCTOU). 존재 여부와 내용을 **한 번의 읽기**에서 함께
    // 얻는다. `retry_tolerating_race` 의 `Ok(None)` 은 정확히 NotFound 이므로
    // "매니페스트 없음" 과 "있는데 손상" 을 여전히 구분할 수 있다.
    //
    // ★ 이 구분은 아래 GC 판정에 그대로 쓰인다. `manifest_exists` 가 거짓이면
    //   `should_remove` 가 전부 참이 되어 **데이터 파일까지 지운다.** 손상된
    //   매니페스트를 "없음" 으로 접으면 데이터 유실이 되므로 접어서는 안 된다.
    //
    // 매니페스트가 reparse point 이거나 디렉터리면 read_beneath 가 NotFound 가
    // 아닌 오류를 내고 여기서 전파된다. 조용히 "없음" 으로 떨어져 전체 삭제로
    // 가는 것보다 시끄럽게 실패하는 편이 안전하다.
    let manifest_data = retry_tolerating_race(|| {
        crate::platform::read_beneath(checkpoint_dir, Path::new(manifest_name))
    })?;
    let manifest_exists = manifest_data.is_some();

    let registered_tmp = match manifest_data {
        Some(data) => match crate::durability::CheckpointManifest::from_json(&data) {
            Ok(manifest) => manifest
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(), // 있지만 손상 — 등록 목록은 비지만 존재는 참이다
        },
        None => Vec::new(),
    };

    let mut removed = Vec::new();

    let entries = match retry_tolerating_race(|| fs::read_dir(checkpoint_dir))? {
        Some(entries) => entries,
        None => return Ok(Vec::new()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };

        let path = entry.path();

        let metadata = match retry_tolerating_race(|| fs::symlink_metadata(&path))? {
            Some(m) => m,
            None => continue, // 경합 — 그 사이 사라졌다
        };

        if !metadata.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // ★ 락 파일은 무조건 지우지 않는다 — 그러나 **영원히** 남기지도
        //   않는다(2026-08-19, 코덱스 독립 검수 `p128` 이 지적).
        //
        //   처음에는 이 분기에서 무조건 `continue` 했다. 이유:
        //   매니페스트가 없는 디렉터리(PARTIAL)는 아래에서 내용물을
        //   전부 지우는데, 그 순간에도 다른 프로세스가 `write_once()`
        //   로 같은 디렉터리에 쓰는 중이라 락을 쥐고 있을 수 있다.
        //   락 파일을 지우면(inode unlink) 그 파일을 쥔 잠금은 그대로
        //   남지만 경로는 사라지고, 다음 호출자가 같은 경로를 다시
        //   `create` 하면 **새 inode** 에 새로 락을 걸 수 있어 상호
        //   배제가 깨진다.
        //
        //   그런데 그렇게 하면 **죽은 프로세스가 남긴 락 파일**(쓰다가
        //   죽어 매니페스트도 데이터도 없이 락 파일 하나만 남은 경우)
        //   때문에 그 디렉터리가 절대 청소되지 않는다 — `startup_gc`
        //   가 "디렉터리가 비었으면 지운다" 를 검사하는데, 락 파일이
        //   영원히 남아 있으니 절대 비지 않는다.
        //
        //   해법: GC 자신이 먼저 `try_lock` 을 시도해 "지금 아무도
        //   쥐고 있지 않다" 를 **직접 확인한 뒤에만** 지운다. GC 가
        //   락을 쥔 채로 지우므로, 그 사이 진짜 writer 가 같은 경로를
        //   열어 잠그려 해도 (아직 존재하는 같은 inode 라서)
        //   `WouldBlock` 으로 정상 거부된다 — 새 inode 로 도망칠 틈이
        //   없다. 반대로 누군가 이미 쥐고 있으면 GC 의 `try_lock` 도
        //   실패하므로 그 락은 건드리지 않는다.
        //
        //   ★ 이름 충돌 방어(2026-08-19, 코덱스 독립 검수 `p129` 이
        //   지적) — `validate_relative_name` 은 `.write_once.lock`
        //   접미사를 예약하지 않는다. 매니페스트가 **실제로 이
        //   접미사로 끝나는 이름을 등록**했다면(파일 이름은
        //   매니페스트에서 오는 외부 입력이다 — `CLAUDE.md` §0), 그건
        //   가짜 락 파일이 아니라 **진짜 데이터**다. `.tmp` 접미사도
        //   같은 문제가 있어서 이미 `registered_tmp` 로 "등록됐으면
        //   보존" 을 검사한다 — 여기도 같은 검사를 먼저 한다.
        if name.ends_with(".write_once.lock") {
            let is_registered = registered_tmp.iter().any(|registered| registered == &name);
            if manifest_exists && is_registered {
                continue; // 이름만 겹치는 등록된 데이터 파일 — 보존한다.
            }
            let opened = retry_tolerating_race(|| OpenOptions::new().write(true).open(&path))?;
            if let Some(lock_file) = opened {
                if lock_file.try_lock().is_ok() {
                    retry_tolerating_race(|| fs::remove_file(&path))?;
                    removed.push(path);
                }
                // try_lock 실패 = 누군가 지금 쥐고 있다 -> 건드리지 않는다.
            }
            continue;
        }

        let is_tmp = name.ends_with(".tmp");

        let is_registered = registered_tmp.iter().any(|registered| registered == &name);

        let should_remove = !manifest_exists || (is_tmp && !is_registered);

        if !should_remove {
            continue;
        }

        // ★ 삭제도 재시도한다 — 다른 쪽이 같은 파일을 동시에 지우면
        //   Windows 가 NotFound 대신 Access Denied 를 낼 수 있다.
        retry_tolerating_race(|| fs::remove_file(&path))?;
        removed.push(path);
    }

    Ok(removed)
}

/// io::Error 를 그대로 노출하기 위한 변환.
impl From<io::Error> for CheckpointError {
    fn from(e: io::Error) -> Self {
        CheckpointError::Io(e.to_string())
    }
}
