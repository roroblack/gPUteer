//! 체크포인트 쓰기/재개 — ADR-026 절차의 상위 API.
//!
//! 규범: 기준선 §18.2 · `docs/protocol/state-machines.md` §4
//!
//! 이 모듈이 보장하는 불변식 (카오스 테스트가 검증한다)
//!   1. 매니페스트가 존재하면 그 매니페스트가 가리키는 파일이 전부 존재하고 해시가 맞다
//!   2. 쓰기 중 강제 종료는 PARTIAL 만 남긴다. COMMITTED 로 승격되지 않는다
//!   3. 포인터는 항상 유효한 체크포인트를 가리키거나 존재하지 않는다
//!   4. 재개는 항상 마지막 유효 체크포인트에서 이뤄진다

use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic::{gc_partial, replace_with_retry, write_once, RetryPolicy};
use crate::durability::{CheckpointFile, CheckpointManifest, MANIFEST_FILENAME};
use crate::CheckpointError;

/// canonical/latest 포인터 파일 이름. 이 파일만 replace-over-existing 을 쓴다.
pub const POINTER_FILENAME: &str = "LATEST";

/// 체크포인트 하나를 확정한다.
///
/// 순서가 규범이다 (기준선 §18.2 규칙 2).
///   1. 데이터 파일을 전부 write-once 로 확정
///   2. **그 다음에** 매니페스트를 쓴다
///   3. 마지막에 포인터를 갱신
///
/// 2번 이전에 죽으면 매니페스트가 없으므로 PARTIAL 로 판정된다.
/// 3번 이전에 죽으면 체크포인트는 온전하나 포인터가 가리키지 않는다 (안전).
pub fn write_checkpoint(
    root: &Path,
    manifest: &CheckpointManifest,
    files: &[(String, Vec<u8>)],
    slow_ms: u64,
) -> Result<PathBuf, CheckpointError> {
    let dir = root.join(&manifest.checkpoint_id);
    fs::create_dir_all(&dir)?;

    // 1. 데이터 파일 (고유 이름, write-once)
    for (name, data) in files {
        write_once(&dir, name, data)?;
        // 카오스 테스트에서 kill 창을 만들기 위한 지연
        if slow_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(slow_ms));
        }
    }

    // 2. 매니페스트를 마지막에 (PARTIAL 판정의 근거)
    let json = manifest.to_json()?;
    write_once(&dir, MANIFEST_FILENAME, &json)?;

    // 3. 포인터 갱신 — 유일한 replace-over-existing
    replace_with_retry(
        root,
        POINTER_FILENAME,
        manifest.checkpoint_id.as_bytes(),
        RetryPolicy::default(),
    )?;

    Ok(dir)
}

/// 매니페스트를 기준으로 체크포인트 파일 목록을 만든다.
pub fn manifest_for(
    checkpoint_id: &str,
    job_id: &str,
    attempt_id: &str,
    step: u64,
    fence_epoch: u64,
    files: &[(String, Vec<u8>)],
) -> CheckpointManifest {
    let entries: Vec<CheckpointFile> = files
        .iter()
        .map(|(name, data)| CheckpointFile {
            path: name.clone(),
            digest: blake3::hash(data).to_hex().to_string(),
            size_bytes: data.len() as u64,
        })
        .collect();
    let total = entries.iter().map(|f| f.size_bytes).sum();

    let chunks: Vec<&[u8]> = files.iter().map(|(_, d)| d.as_slice()).collect();
    let root_digest = gputeer_protocol::merkle_root(&chunks)
        .map(|r| r.iter().map(|b| format!("{b:02x}")).collect::<String>())
        .unwrap_or_default();

    CheckpointManifest {
        schema_version: 1,
        checkpoint_id: checkpoint_id.to_string(),
        job_id: job_id.to_string(),
        attempt_id: attempt_id.to_string(),
        step,
        files: entries,
        root_digest,
        total_bytes: total,
        created_at_unix_ms: 0, // 결정론적 테스트를 위해 호출자가 채운다
        producer_node_id: "test-node".to_string(),
        fence_epoch,
    }
}

/// ★ 재개 지점을 찾는다 — **job/attempt 로 걸러서.**
///
/// # 왜 필터가 필요한가 (2026-08-16 신설)
///
/// 독립 검수가 지적했다. [`find_resume_point`] 는 `root` 아래 **모든**
/// 디렉터리를 후보로 삼고 해시 검증만 통과하면 가장 큰 `step` 을 고른다.
///
/// ```text
/// 내 job    step 10
/// 남의 job  step 100   <- 같은 root 에 있으면 이것을 고른다
/// ```
///
/// **해시가 맞으면 안전하다** 가 아니다 — 해시는 *그 파일이 그 매니페스트의 것*임을
/// 보장할 뿐, *그 매니페스트가 내 것*임은 보장하지 않는다.
/// 남의 체크포인트에서 재개하면 **완전히 다른 학습 상태를 로드한다.**
///
/// # 거르는 것
///
/// ```text
/// job_id / attempt_id 불일치   다른 실행의 상태다
/// files 가 빈 매니페스트       데이터가 없는데 "유효" 로 통과한다
/// checkpoint_id != 디렉터리명   반환된 id 로 파일을 찾으면 엉뚱한 곳을 본다
/// ```
///
/// `attempt_id` 는 특히 중요하다 — 같은 job 의 다른 attempt 는
/// **fencing 으로 무효화된 실행**일 수 있다.
pub fn find_resume_point_for(
    root: &Path,
    job_id: &str,
    attempt_id: &str,
) -> Result<Option<CheckpointManifest>, CheckpointError> {
    let mut candidates: Vec<CheckpointManifest> = Vec::new();
    if !root.is_dir() {
        return Ok(None);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(m) = load_valid_manifest(&dir)? else {
            continue;
        };

        // ★ 이 실행의 것인가
        if m.job_id != job_id || m.attempt_id != attempt_id {
            continue;
        }
        // ★ 데이터가 하나도 없는 매니페스트는 재개 근거가 아니다.
        //   verify_files 는 반복문을 0회 돌고 통과한다.
        if m.files.is_empty() {
            continue;
        }
        // ★ checkpoint_id 가 디렉터리 이름과 같은가.
        //   다르면 반환된 id 로 파일을 찾는 호출자가 엉뚱한 곳을 본다.
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if m.checkpoint_id != dir_name {
            continue;
        }
        candidates.push(m);
    }
    candidates.sort_by_key(|m| m.step);
    Ok(candidates.pop())
}

/// 디렉터리에서 **해시까지 검증된** 매니페스트를 읽는다.
///
/// 매니페스트가 없거나 깨졌거나 파일 해시가 어긋나면 `None`.
fn load_valid_manifest(dir: &Path) -> Result<Option<CheckpointManifest>, CheckpointError> {
    let mpath = dir.join(MANIFEST_FILENAME);
    if !mpath.exists() {
        return Ok(None); // PARTIAL — 매니페스트가 없다
    }
    let Ok(data) = fs::read(&mpath) else {
        return Ok(None);
    };
    let Ok(m) = CheckpointManifest::from_json(&data) else {
        return Ok(None); // 매니페스트 자체가 깨졌다
    };
    // 불변식 1: 매니페스트가 있으면 파일이 전부 온전해야 한다
    if m.verify_files(dir).is_err() {
        return Ok(None);
    }
    Ok(Some(m))
}

/// 재개 지점을 찾는다 — **필터 없음.**
///
/// ★ **호출자가 job/attempt 를 반드시 걸러야 한다.**
/// 이 함수는 `root` 아래 모든 유효 체크포인트 중 가장 큰 `step` 을 고른다.
/// 여러 job 이 같은 root 를 쓰면 **남의 것을 고른다** (독립 검수 2026-08-16).
///
/// 새 코드는 [`find_resume_point_for`] 를 쓴다.
/// 이 함수는 "root 에 한 실행의 체크포인트만 있다" 가 보장될 때만 안전하다.
pub fn find_resume_point(root: &Path) -> Result<Option<CheckpointManifest>, CheckpointError> {
    let mut candidates: Vec<CheckpointManifest> = Vec::new();

    if !root.is_dir() {
        return Ok(None);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let mpath = dir.join(MANIFEST_FILENAME);
        if !mpath.exists() {
            continue; // PARTIAL — 매니페스트가 없다
        }
        let data = match fs::read(&mpath) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let m = match CheckpointManifest::from_json(&data) {
            Ok(m) => m,
            Err(_) => continue, // 매니페스트 자체가 깨졌다
        };
        // 불변식 1: 매니페스트가 있으면 파일이 전부 온전해야 한다
        if m.verify_files(&dir).is_err() {
            continue;
        }
        candidates.push(m);
    }

    candidates.sort_by_key(|m| m.step);
    Ok(candidates.pop())
}

/// 포인터가 가리키는 체크포인트 id (있으면).
pub fn read_pointer(root: &Path) -> Option<String> {
    fs::read_to_string(root.join(POINTER_FILENAME))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 부팅 시 정리 — 모든 체크포인트 디렉터리에서 PARTIAL/tmp 를 제거한다.
///
/// 반환값: (검사한 디렉터리 수, 제거한 파일 수)
pub fn startup_gc(root: &Path) -> Result<(usize, usize), CheckpointError> {
    let mut dirs = 0;
    let mut removed = 0;
    if !root.is_dir() {
        return Ok((0, 0));
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        dirs += 1;
        removed += gc_partial(&dir, MANIFEST_FILENAME)?.len();
        // 비어버린 디렉터리는 제거
        if fs::read_dir(&dir)?.next().is_none() {
            let _ = fs::remove_dir(&dir);
        }
    }
    Ok((dirs, removed))
}
