//! 체크포인트 쓰기/재개 — ADR-026 절차의 상위 API.

use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic::{
    gc_partial, replace_with_retry, retry_tolerating_race, write_once, RetryPolicy,
};
use crate::durability::{
    publication_failed, record_initial_state, record_publication_failure,
    record_state_transition, CheckpointFile, CheckpointManifest,
    DurabilityState, MANIFEST_FILENAME,
};
use crate::CheckpointError;

/// canonical/latest 포인터 파일 이름.
pub const POINTER_FILENAME: &str = "LATEST";

fn is_not_found(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
}

/// 실패 마커를 남긴 뒤 원래 오류를 반환한다.
///
/// 마커도 기록하지 못하면 그 사실을 오류에 포함한다.
/// 실패 마커를 조용히 무시하면 W-4를 다시 만들기 때문이다.
fn fail_after_materialization(
    dir: &Path,
    original: CheckpointError,
) -> CheckpointError {
    match record_publication_failure(dir) {
        Ok(()) => original,
        Err(marker_error) => CheckpointError::Io(format!(
            "체크포인트 공개 실패: {original}; \
             실패 마커 기록도 실패하여 재개 배제를 보장할 수 없다: {marker_error}"
        )),
    }
}

/// 체크포인트 하나를 확정한다.
///
/// 순서:
///
/// 1. 데이터 파일을 write-once 로 확정
/// 2. 매니페스트를 기록
/// 3. 매니페스트의 파일 해시를 검증
/// 4. 포인터를 갱신
/// 5. COMMITTED 상태 마커를 기록
///
/// 포인터 갱신에 실패하면 완전한 artifact를 삭제하지 않는다.
/// 대신 `.publication-failed` 를 write-once 로 기록하고 재개 후보에서 제외한다.
pub fn write_checkpoint(
    root: &Path,
    manifest: &CheckpointManifest,
    files: &[(String, Vec<u8>)],
    slow_ms: u64,
) -> Result<PathBuf, CheckpointError> {
    let dir = root.join(&manifest.checkpoint_id);
    fs::create_dir_all(&dir)?;

    record_initial_state(&dir)?;

    for (name, data) in files {
        write_once(&dir, name, data)?;

        if slow_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(slow_ms));
        }
    }

    record_state_transition(
        &dir,
        DurabilityState::Writing,
        DurabilityState::LocalWritten,
    )?;

    let json = manifest.to_json()?;
    write_once(&dir, MANIFEST_FILENAME, &json)?;

    if let Err(error) = manifest.verify_files(&dir) {
        return Err(fail_after_materialization(&dir, error));
    }

    if let Err(error) = record_state_transition(
        &dir,
        DurabilityState::LocalWritten,
        DurabilityState::HashVerified,
    ) {
        return Err(fail_after_materialization(&dir, error));
    }

    if let Err(error) = replace_with_retry(
        root,
        POINTER_FILENAME,
        manifest.checkpoint_id.as_bytes(),
        RetryPolicy::default(),
    ) {
        return Err(fail_after_materialization(&dir, error));
    }

    if let Err(error) = record_state_transition(
        &dir,
        DurabilityState::HashVerified,
        DurabilityState::Committed,
    ) {
        return Err(fail_after_materialization(&dir, error));
    }

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

    let total = entries.iter().map(|file| file.size_bytes).sum();
    let chunks: Vec<&[u8]> =
        files.iter().map(|(_, data)| data.as_slice()).collect();

    let root_digest = gputeer_protocol::merkle_root(&chunks)
        .map(|root| {
            root.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
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
        created_at_unix_ms: 0,
        producer_node_id: "test-node".to_string(),
        fence_epoch,
    }
}

/// 포인터 또는 COMMITTED 상태 마커로 공개된 체크포인트인지 확인한다.
///
/// `.publication-failed` 가 있으면 포인터가 우연히 새 체크포인트를 가리켜도
/// 호출자가 Err를 받은 쓰기를 재개 후보로 되살리지 않는다.
/// 재개 후보인가.
///
/// # ★ 왜 `COMMITTED` 마커를 **요구하지 않는가** (2026-08-17 설계 정정)
///
/// 초안은 "COMMITTED 마커가 있거나 지금 LATEST 가 가리키는 것" 만 후보로 삼았다.
/// **그것은 과하다.**
///
/// ```text
/// 기록 순서
///   데이터 -> 매니페스트 -> HASH_VERIFIED -> LATEST 교체 -> COMMITTED
///                                                          ^^^^^^^^^
///                          여기 직전에 kill 되면 마커가 없다
/// ```
///
/// 그 체크포인트는 **데이터도 매니페스트도 온전하고 해시도 맞다.**
/// 그런데 마커 하나가 없다는 이유로 버리면 **복구 가능한 상태를 잃는다** —
/// `CLAUDE.md` §0.3, "데이터 손실은 되돌릴 수 없다."
///
/// 실제로 카오스 테스트가 이것을 잡았다. 부하가 높을 때
/// 500ms 안에 COMMITTED 까지 간 체크포인트가 하나도 없으면
/// 재개 지점이 통째로 사라졌다.
///
/// 또한 기존 규범은 **매니페스트의 존재**를 완결 신호로 삼는다
/// (`CLAUDE.md` §0.3 — "매니페스트 없는 데이터 파일은 PARTIAL").
/// 완결 지점을 마커로 옮기면 그 규범과 어긋난다.
///
/// # 그러면 W-4(포인터 실패 잔여물)는 어떻게 막는가
///
/// **명시적 실패 마커로만** 막는다. 포인터 갱신이 실패하면
/// [`PUBLICATION_FAILED_MARKER`] 를 쓰고 `Err` 를 반환한다.
/// 그 마커가 있는 디렉터리는 후보에서 빠진다.
///
/// ★ 남는 위험: **마커 쓰기 자체가 실패하면** 배제되지 않는다.
///   그 경우 `write_checkpoint` 가 명시적 오류를 반환하지만,
///   디스크에는 선택 가능한 잔여물이 남는다. 완전히 막지는 못한다.
fn is_resume_candidate(
    dir: &Path,
    _dir_name: &str,
    _pointer: Option<&str>,
) -> Result<bool, CheckpointError> {
    Ok(!publication_failed(dir)?)
}

/// 유효 매니페스트를 읽는다.
fn load_valid_manifest(
    dir: &Path,
) -> Result<Option<CheckpointManifest>, CheckpointError> {
    let manifest_path = dir.join(MANIFEST_FILENAME);

    if !manifest_path.exists() {
        return Ok(None);
    }

    let data = match fs::read(&manifest_path) {
        Ok(data) => data,
        Err(_) => return Ok(None),
    };

    let manifest = match CheckpointManifest::from_json(&data) {
        Ok(manifest) => manifest,
        Err(_) => return Ok(None),
    };

    if manifest.verify_files(dir).is_err() {
        return Ok(None);
    }

    Ok(Some(manifest))
}

/// job/attempt로 제한한 재개 지점 검색.
pub fn find_resume_point_for(
    root: &Path,
    job_id: &str,
    attempt_id: &str,
) -> Result<Option<CheckpointManifest>, CheckpointError> {
    let mut candidates = Vec::new();
    let pointer = read_pointer(root);

    if !root.is_dir() {
        return Ok(None);
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if is_not_found(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_not_found(&error) => continue,
            Err(error) => return Err(error.into()),
        };

        let dir = entry.path();

        if !dir.is_dir() {
            continue;
        }

        let Some(manifest) = load_valid_manifest(&dir)? else {
            continue;
        };

        let dir_name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        if manifest.job_id != job_id
            || manifest.attempt_id != attempt_id
            || manifest.files.is_empty()
            || manifest.checkpoint_id != dir_name
        {
            continue;
        }

        if !is_resume_candidate(&dir, dir_name, pointer.as_deref())? {
            continue;
        }

        candidates.push(manifest);
    }

    candidates.sort_by_key(|manifest| manifest.step);
    Ok(candidates.pop())
}

/// job/attempt 필터가 없는 호환 API.
///
/// 이 API는 여전히 다른 job의 상태를 고를 수 있다.
pub fn find_resume_point(
    root: &Path,
) -> Result<Option<CheckpointManifest>, CheckpointError> {
    let mut candidates = Vec::new();
    let pointer = read_pointer(root);

    if !root.is_dir() {
        return Ok(None);
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if is_not_found(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_not_found(&error) => continue,
            Err(error) => return Err(error.into()),
        };

        let dir = entry.path();

        if !dir.is_dir() {
            continue;
        }

        let Some(manifest) = load_valid_manifest(&dir)? else {
            continue;
        };

        let dir_name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        if manifest.files.is_empty() || manifest.checkpoint_id != dir_name {
            continue;
        }

        if !is_resume_candidate(&dir, dir_name, pointer.as_deref())? {
            continue;
        }

        candidates.push(manifest);
    }

    candidates.sort_by_key(|manifest| manifest.step);
    Ok(candidates.pop())
}

/// 포인터가 가리키는 체크포인트 id.
pub fn read_pointer(root: &Path) -> Option<String> {
    fs::read_to_string(root.join(POINTER_FILENAME))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 부팅 시 정리.
///
/// `.tmp` 는 매니페스트에 등록되지 않은 경우에만 제거한다.
/// 등록된 `.tmp` 파일은 정상적인 데이터 파일일 수 있으므로 보존한다.
///
/// 디렉터리나 파일이 다른 프로세스에 의해 먼저 사라진 경우는 정상 경합이다.
/// 그 경우에만 건너뛰며, 권한 오류 등 다른 오류는 반환한다.
pub fn startup_gc(root: &Path) -> Result<(usize, usize), CheckpointError> {
    let mut dirs = 0;
    let mut removed = 0;

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if is_not_found(&error) => return Ok((0, 0)),
        Err(error) => return Err(error.into()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_not_found(&error) => continue,
            Err(error) => return Err(error.into()),
        };

        let dir = entry.path();

        // ★ 2026-08-17 — 이 세 지점 모두 atomic.rs 의 재시도 헬퍼를 쓴다.
        //   전에는 이 파일만 고쳤고 atomic.rs::gc_partial 은 못 고쳐서
        //   결함이 두 곳에 나뉘어 있었다. 부하 테스트에서 5회 중 3회
        //   실패하는 걸로 드러났다 — gc_partial 내부의 read_dir/remove_file
        //   이 여전히 NotFound 만 경합으로 봤다.
        let metadata = match retry_tolerating_race(|| fs::symlink_metadata(&dir))? {
            Some(m) => m,
            None => continue, // 경합 — 이미 사라졌다
        };

        if !metadata.is_dir() {
            continue;
        }

        dirs += 1;
        removed += gc_partial(&dir, MANIFEST_FILENAME)?.len();

        let mut children = match retry_tolerating_race(|| fs::read_dir(&dir))? {
            Some(children) => children,
            None => continue,
        };

        let empty = match children.next() {
            None => true,
            Some(Ok(_)) => false,
            Some(Err(error)) if is_not_found(&error) => continue,
            Some(Err(error)) => return Err(error.into()),
        };

        if empty {
            retry_tolerating_race(|| fs::remove_dir(&dir))?;
        }
    }

    Ok((dirs, removed))
}
