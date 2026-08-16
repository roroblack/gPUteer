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

/// 재개 지점을 찾는다.
///
/// 포인터가 가리키는 체크포인트를 검증하고, 유효하지 않으면
/// **더 낮은 step 의 유효한 체크포인트로 내려간다.**
/// 유효한 것이 없으면 `None`.
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
