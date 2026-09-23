//! 공유 저장소 체크포인트 — **다른 기계에서 이어가기** 위한 게시 · 검증 · 복원 (신뢰망 남은 일 E).
//!
//! # 왜 생겼나 (2026-09-23)
//!
//! 조사 결과 체크포인트는 **만든 기계 밖으로 한 번도 나가지 않았다** — 작업이 끝난 뒤 로컬에 한 번,
//! 서명 없이. 그래서 노드가 죽으면 이어갈 것이 없었다. 신뢰망(서로 믿는 몇 사람)에서는 운영자가 정한
//! **공유 저장소**(NAS · 동기화 폴더 · 네트워크 드라이브)를 복제 수단으로 쓴다.
//!
//! ```text
//! <공유 루트>/<job_id>/<checkpoint_id>/            데이터 파일(내용 주소 이름) + manifest.json   ← StagedCheckpoint 그대로
//! <공유 루트>/<job_id>/<checkpoint_id>.checkpoint.pb   생산 노드가 서명한 CheckpointManifest(proto)
//! ```
//!
//! ★ 데이터 쓰기는 이미 있는 확정 절차(`StagedCheckpoint` — write-once · fsync · 매니페스트 마지막)를 그대로 쓴다.
//!   새 쓰기 규칙을 만들지 않는다(ADR-026).
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 서명 · 서명 검증   crypto 에 묶이지 않는다. 호출부(Agent · Coordinator)가 한다 — 여기는 바이트와 해시만 본다
//! 복제본 수 판정     공유 저장소 하나를 holder 하나로 본다는 것은 **운영 선언**이다(계획 문서 "기준선과 다른 점").
//!                    이 모듈은 "파일이 매니페스트와 맞는가" 만 답한다
//! 하위 폴더          체크포인트 폴더 안의 **파일만** 받는다. 하위 폴더가 있으면 거부한다(지어내지 않는다 —
//!                    경로를 이름으로 접는 규칙을 아직 정하지 않았다)
//! 큰 파일 스트리밍   파일을 통째로 메모리에 읽는다. 수 GB 체크포인트는 그만큼 메모리를 쓴다 — 알려진 한계다
//! ```

use std::path::{Path, PathBuf};

use gputeer_protocol::pb;

use crate::commit::{logical_name_of, CommittedCheckpoint, ManifestMeta, StagedCheckpoint};
use crate::durability::CheckpointManifest;

/// 서명된 proto 매니페스트 파일의 접미사.
pub const SIGNED_MANIFEST_SUFFIX: &str = ".checkpoint.pb";

/// 이 Job 의 체크포인트가 모이는 곳.
pub fn job_root(shared_root: &Path, job_id: &str) -> Result<PathBuf, String> {
    safe_component(job_id, "job_id")?;
    Ok(shared_root.join(job_id))
}

fn safe_component(value: &str, what: &str) -> Result<(), String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains(['/', '\\', ':'])
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "SHARED_CHECKPOINT_UNSAFE_NAME: {what} 가 경로 한 칸으로 안전하지 않다: {value:?}"
        ));
    }
    Ok(())
}

/// `source_dir` 안의 **파일만** 체크포인트 하나로 공유 저장소에 확정한다.
///
/// 이름 순으로 스테이징한다 — 같은 폴더에서 같은 매니페스트가 나오게(머클 루트가 순서에 달려 있다).
pub fn publish_directory(
    shared_root: &Path,
    job_id: &str,
    checkpoint_id: &str,
    source_dir: &Path,
    meta: &ManifestMeta,
) -> Result<CommittedCheckpoint, String> {
    let root = job_root(shared_root, job_id)?;
    let mut names: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(source_dir)
        .map_err(|e| format!("SHARED_CHECKPOINT_SOURCE: {source_dir:?} 를 읽지 못했다: {e}"))?
    {
        let entry = entry.map_err(|e| format!("SHARED_CHECKPOINT_SOURCE: {e}"))?;
        let kind = entry
            .file_type()
            .map_err(|e| format!("SHARED_CHECKPOINT_SOURCE: {e}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if kind.is_symlink() {
            return Err(format!(
                "SHARED_CHECKPOINT_SYMLINK: {name} 는 심볼릭 링크다 — 체크포인트에 넣지 않는다"
            ));
        }
        if kind.is_dir() {
            return Err(format!(
                "SHARED_CHECKPOINT_SUBDIR: {name} 는 폴더다 — 체크포인트 폴더에는 파일만 둔다"
            ));
        }
        names.push((name, entry.path()));
    }
    if names.is_empty() {
        return Err(format!(
            "SHARED_CHECKPOINT_EMPTY: {source_dir:?} 에 파일이 없다"
        ));
    }
    names.sort();
    let mut staged = StagedCheckpoint::begin(&root, checkpoint_id)
        .map_err(|e| format!("SHARED_CHECKPOINT_BEGIN: {e:?}"))?;
    for (name, path) in &names {
        let data =
            std::fs::read(path).map_err(|e| format!("SHARED_CHECKPOINT_READ: {path:?}: {e}"))?;
        staged
            .stage(name, &data)
            .map_err(|e| format!("SHARED_CHECKPOINT_STAGE: {name}: {e:?}"))?;
    }
    staged
        .commit(meta)
        .map_err(|e| format!("SHARED_CHECKPOINT_COMMIT: {e:?}"))
}

fn hex32(hex: &str, what: &str) -> Result<Vec<u8>, String> {
    if hex.len() != 64 {
        return Err(format!("{what} 가 32바이트 hex 가 아니다"));
    }
    (0..32)
        .map(|i| {
            u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| format!("{what} 가 hex 가 아니다"))
        })
        .collect()
}

/// 확정된 매니페스트(JSON)를 **서명할** proto 로 옮긴다. 서명 칸(90)은 비어 있다 — 호출부가 채운다.
///
/// ★ `completeness` 는 비운다 — 생산자(Agent)는 작업이 무엇을 담았는지 모른다. 지어내지 않는다(§1).
pub fn to_unsigned_pb(manifest: &CheckpointManifest) -> Result<pb::CheckpointManifest, String> {
    let mut files = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        files.push(pb::CheckpointFile {
            path: file.path.clone(),
            digest: Some(pb::Digest {
                algo: 1,
                value: hex32(&file.digest, "file digest")?,
            }),
            size_bytes: file.size_bytes,
            chunk_digests: Vec::new(),
            chunk_size_bytes: 0,
        });
    }
    Ok(pb::CheckpointManifest {
        schema_version: 1,
        checkpoint_id: manifest.checkpoint_id.clone(),
        job_id: manifest.job_id.clone(),
        attempt_id: manifest.attempt_id.clone(),
        step: manifest.step,
        epoch: 0,
        files,
        root_digest: Some(pb::Digest {
            algo: 1,
            value: hex32(&manifest.root_digest, "root digest")?,
        }),
        total_bytes: manifest.total_bytes,
        completeness: None,
        created_at_unix_ms: manifest.created_at_unix_ms,
        producer_node_id: manifest.producer_node_id.clone(),
        fence_epoch: manifest.fence_epoch,
        producer_signature: Vec::new(),
    })
}

/// 서명된 proto 매니페스트를 쓴다 — write-once(같은 바이트면 멱등, 다르면 거부).
pub fn write_signed_manifest(
    shared_root: &Path,
    job_id: &str,
    checkpoint_id: &str,
    signed_body: &[u8],
) -> Result<(), String> {
    safe_component(checkpoint_id, "checkpoint_id")?;
    let root = job_root(shared_root, job_id)?;
    std::fs::create_dir_all(&root).map_err(|e| format!("SHARED_CHECKPOINT_ROOT: {e}"))?;
    crate::write_once(
        &root,
        &format!("{checkpoint_id}{SIGNED_MANIFEST_SUFFIX}"),
        signed_body,
    )
    .map(|_| ())
    .map_err(|e| format!("SHARED_CHECKPOINT_SIGNED_MANIFEST: {e:?}"))
}

/// 이 Job 의 서명된 매니페스트를 **전부** 읽는다 — `(checkpoint_id, 원본 바이트)`.
///
/// ★ **검증하지 않은 바이트다.** 호출부가 서명을 검증하고 [`verify_on_disk`] 로 파일을 대조하기 전에는 아무것도
///   믿지 않는다(§0.2). 폴더가 없으면 빈 목록이다(아직 아무도 게시하지 않았다).
pub fn list_signed_manifests(
    shared_root: &Path,
    job_id: &str,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let root = job_root(shared_root, job_id)?;
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("SHARED_CHECKPOINT_LIST: {root:?}: {error}")),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("SHARED_CHECKPOINT_LIST: {e}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(checkpoint_id) = name.strip_suffix(SIGNED_MANIFEST_SUFFIX) else {
            continue;
        };
        if !entry
            .file_type()
            .map_err(|e| format!("SHARED_CHECKPOINT_LIST: {e}"))?
            .is_file()
        {
            continue;
        }
        let body = std::fs::read(entry.path())
            .map_err(|e| format!("SHARED_CHECKPOINT_LIST: {name}: {e}"))?;
        out.push((checkpoint_id.to_string(), body));
    }
    out.sort();
    Ok(out)
}

/// 공유 저장소의 파일을 **다시 읽고 해시해** 매니페스트와 대조한다. 맞으면 파일 내용을 매니페스트 순서로 돌려준다.
///
/// 보는 것: 파일마다 크기 · BLAKE3 · 저장 이름의 내용 주소 표식, 전체 크기, 머클 루트(§6.3).
pub fn verify_on_disk(
    shared_root: &Path,
    manifest: &pb::CheckpointManifest,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    safe_component(&manifest.checkpoint_id, "checkpoint_id")?;
    let dir = job_root(shared_root, &manifest.job_id)?.join(&manifest.checkpoint_id);
    if manifest.files.is_empty() {
        return Err("SHARED_CHECKPOINT_VERIFY: 파일이 없는 매니페스트다".to_string());
    }
    let mut contents: Vec<(String, Vec<u8>)> = Vec::with_capacity(manifest.files.len());
    let mut total: u64 = 0;
    for file in &manifest.files {
        let logical = logical_name_of(&file.path).ok_or_else(|| {
            format!(
                "SHARED_CHECKPOINT_VERIFY: {} 는 내용 주소 이름이 아니다",
                file.path
            )
        })?;
        let data = crate::platform::read_beneath(&dir, Path::new(&file.path)).map_err(|e| {
            format!(
                "SHARED_CHECKPOINT_VERIFY: {} 를 읽지 못했다: {e}",
                file.path
            )
        })?;
        if data.len() as u64 != file.size_bytes {
            return Err(format!(
                "SHARED_CHECKPOINT_VERIFY: {} 크기가 다르다(매니페스트 {} · 디스크 {})",
                file.path,
                file.size_bytes,
                data.len()
            ));
        }
        let digest = file
            .digest
            .as_ref()
            .filter(|digest| digest.algo == 1)
            .ok_or_else(|| {
                format!(
                    "SHARED_CHECKPOINT_VERIFY: {} 의 digest 가 BLAKE3-256 이 아니다",
                    file.path
                )
            })?;
        if blake3::hash(&data).as_bytes().as_slice() != digest.value.as_slice() {
            return Err(format!(
                "SHARED_CHECKPOINT_VERIFY: {} 내용이 매니페스트와 다르다",
                file.path
            ));
        }
        total = total.saturating_add(data.len() as u64);
        contents.push((logical.to_string(), data));
    }
    if total != manifest.total_bytes {
        return Err(format!(
            "SHARED_CHECKPOINT_VERIFY: 전체 크기가 다르다(매니페스트 {} · 디스크 {total})",
            manifest.total_bytes
        ));
    }
    let chunks: Vec<&[u8]> = contents.iter().map(|(_, data)| data.as_slice()).collect();
    let root = gputeer_protocol::merkle_root(&chunks)
        .ok_or("SHARED_CHECKPOINT_VERIFY: 머클 루트를 계산할 수 없다")?;
    let expected = manifest
        .root_digest
        .as_ref()
        .filter(|digest| digest.algo == 1)
        .ok_or("SHARED_CHECKPOINT_VERIFY: root_digest 가 BLAKE3-256 이 아니다")?;
    if root.as_slice() != expected.value.as_slice() {
        return Err("SHARED_CHECKPOINT_VERIFY: 머클 루트가 다르다".to_string());
    }
    Ok(contents)
}

/// 검증한 뒤 **논리 이름으로** `dest` 에 되살린다. `dest` 는 비어 있어야 한다(섞지 않는다).
pub fn restore(
    shared_root: &Path,
    manifest: &pb::CheckpointManifest,
    dest: &Path,
) -> Result<(), String> {
    let contents = verify_on_disk(shared_root, manifest)?;
    std::fs::create_dir_all(dest).map_err(|e| format!("SHARED_CHECKPOINT_RESTORE: {e}"))?;
    if std::fs::read_dir(dest)
        .map_err(|e| format!("SHARED_CHECKPOINT_RESTORE: {e}"))?
        .next()
        .is_some()
    {
        return Err(format!(
            "SHARED_CHECKPOINT_RESTORE: {dest:?} 가 비어 있지 않다 — 섞지 않는다"
        ));
    }
    for (logical, data) in contents {
        safe_component(&logical, "file name")?;
        std::fs::write(dest.join(&logical), data)
            .map_err(|e| format!("SHARED_CHECKPOINT_RESTORE: {logical}: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(step: u64) -> ManifestMeta {
        ManifestMeta {
            job_id: "job-1".into(),
            attempt_id: "attempt-1".into(),
            step,
            fence_epoch: 3,
            producer_node_id: "node-a".into(),
            created_at_unix_ms: 1_000,
        }
    }

    fn source_with(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
        let source = dir.join("out");
        std::fs::create_dir_all(&source).unwrap();
        for (name, data) in files {
            std::fs::write(source.join(name), data).unwrap();
        }
        source
    }

    /// 게시 -> 서명할 proto -> 다른 곳에서 검증 · 복원까지 한 바퀴. 복원된 파일이 원본과 같다.
    #[test]
    fn a_published_checkpoint_verifies_and_restores_elsewhere() {
        let temp = tempfile::tempdir().unwrap();
        let shared = temp.path().join("shared");
        let source = source_with(
            temp.path(),
            &[("state.bin", b"counter=2"), ("rng", b"seed")],
        );
        let committed = publish_directory(&shared, "job-1", "ckpt-2", &source, &meta(2)).unwrap();
        let pb = to_unsigned_pb(&committed.manifest).unwrap();
        assert_eq!(pb.step, 2);
        assert_eq!(pb.files.len(), 2);

        let dest = temp.path().join("restored");
        restore(&shared, &pb, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("state.bin")).unwrap(), b"counter=2");
        assert_eq!(std::fs::read(dest.join("rng")).unwrap(), b"seed");
    }

    /// ★ 공유 저장소의 파일이 **바뀌면** 검증에서 떨어진다 — 바뀐 체크포인트에서 이어가지 않는다(§0.2).
    #[test]
    fn a_tampered_file_or_manifest_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let shared = temp.path().join("shared");
        let source = source_with(temp.path(), &[("state.bin", b"counter=2")]);
        let committed = publish_directory(&shared, "job-1", "ckpt-2", &source, &meta(2)).unwrap();
        let pb = to_unsigned_pb(&committed.manifest).unwrap();

        // 매니페스트의 루트를 바꾸면
        let mut bad_root = pb.clone();
        bad_root.root_digest.as_mut().unwrap().value[0] ^= 0xFF;
        assert!(verify_on_disk(&shared, &bad_root)
            .unwrap_err()
            .contains("머클 루트"));

        // 디스크의 파일을 바꾸면
        let stored = shared.join("job-1").join("ckpt-2").join(&pb.files[0].path);
        std::fs::write(&stored, b"counter=9").unwrap();
        assert!(verify_on_disk(&shared, &pb)
            .unwrap_err()
            .contains("내용이 매니페스트와 다르다"));
        let dest = temp.path().join("restored");
        assert!(restore(&shared, &pb, &dest).is_err());
        assert!(!dest.join("state.bin").exists(), "검증 실패인데 복원했다");
    }

    /// 하위 폴더 · 빈 폴더는 거부한다 — 이름 규칙을 지어내지 않는다.
    #[test]
    fn subdirectories_and_empty_sources_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let shared = temp.path().join("shared");
        let source = source_with(temp.path(), &[("a", b"1")]);
        std::fs::create_dir_all(source.join("nested")).unwrap();
        assert!(publish_directory(&shared, "job-1", "c1", &source, &meta(1))
            .unwrap_err()
            .contains("SHARED_CHECKPOINT_SUBDIR"));
        let empty = temp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(publish_directory(&shared, "job-1", "c2", &empty, &meta(1))
            .unwrap_err()
            .contains("SHARED_CHECKPOINT_EMPTY"));
    }

    /// 서명된 매니페스트는 write-once 다 — 같은 바이트는 멱등, 다른 바이트는 거부. 목록은 이름 순이다.
    #[test]
    fn signed_manifests_are_write_once_and_listed() {
        let temp = tempfile::tempdir().unwrap();
        let shared = temp.path().join("shared");
        assert!(list_signed_manifests(&shared, "job-1").unwrap().is_empty());
        write_signed_manifest(&shared, "job-1", "ckpt-2", b"body-2").unwrap();
        write_signed_manifest(&shared, "job-1", "ckpt-2", b"body-2").unwrap();
        assert!(write_signed_manifest(&shared, "job-1", "ckpt-2", b"other").is_err());
        write_signed_manifest(&shared, "job-1", "ckpt-1", b"body-1").unwrap();
        let listed = list_signed_manifests(&shared, "job-1").unwrap();
        assert_eq!(
            listed,
            vec![
                ("ckpt-1".to_string(), b"body-1".to_vec()),
                ("ckpt-2".to_string(), b"body-2".to_vec())
            ]
        );
        assert!(job_root(&shared, "../escape").is_err());
    }
}
