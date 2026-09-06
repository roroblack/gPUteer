//! 체크포인트 **확정 절차** — `CLAUDE.md` §0.3 · ADR-026 §수정안 의 구현체.
//!
//! # 이 모듈이 채우는 공백
//!
//! `atomic.rs` 는 파일 **하나**를 원자적으로 확정하는 방법을 안다
//! (`write_once` · `replace_with_retry` · `gc_partial`).
//! `writer.rs::write_checkpoint` 는 "전부 메모리에 있는 파일 목록"을 한 번에
//! 받아 포인터 교체와 `COMMITTED` 마커까지 한 호출로 처리한다.
//!
//! **없던 것은 그 사이다** — 데이터 파일을 하나씩 확정해 나가다가
//! 마지막에 매니페스트로 완결시키는 절차. 이 모듈이 그것이다.
//!
//! ```text
//! 1  데이터 파일을 write-once 로 쓴다 (고유 이름 = content-addressed)
//! 2  전부 확정된 뒤에 매니페스트를 마지막에 쓴다
//! 3  매니페스트 없는 데이터 파일은 PARTIAL 이며 부팅 시 GC 대상이다
//! ```
//!
//! # 왜 content-addressed 이름인가 (ADR-026)
//!
//! ADR-026 은 Windows 실측으로 `rename-over-existing` 이 신뢰할 수 없음을
//! 확인했다(독자가 열고 있으면 313/3000). 해법은 "대상이 애초에 존재하지
//! 않게 만드는 것" — 즉 **이름이 내용을 결정**하게 만드는 것이다.
//! 그러면 같은 이름이 이미 있다는 것은 곧 내용이 같다는 뜻이므로
//! rename 을 시도할 필요조차 없다(P1a 조건 = 3000/3000).
//!
//! ★ 그런데 `writer.rs` 는 그 전제를 지키지 않았다 — `shard-0.bin` 같은
//!   **위치 기반 이름**을 그대로 쓴다. `atomic.rs` 의
//!   [`CheckpointError::ContentMismatch`] 주석이 그 사실을 이미 적어 뒀다
//!   ("가정이 지켜지지 않았다"). 이 모듈은 그 전제를 **이름 생성 규칙으로
//!   강제**한다:
//!
//! ```text
//! 저장 이름 = <논리 이름> + ".b3-" + <내용의 BLAKE3-256 hex 64자>
//!   model.bin  ->  model.bin.b3-af19...(64자)
//! ```
//!
//! 논리 이름에는 `".b3-"` 를 금지하므로 마지막 `".b3-"` 하나로 되돌릴 수
//! 있다([`logical_name_of`]). 매니페스트의 `CheckpointFile.path` 에는
//! **저장 이름**이 들어간다 — `proto/artifact.proto` 의 `CheckpointFile`
//! 에 논리 이름 칸이 따로 없기 때문이다(스키마는 이 조각에서 안 바꾼다).
//!
//! # 무엇을 보장하지 않는가
//!
//! - **포인터(`LATEST`) 갱신과 `COMMITTED` 마커는 이 절차의 범위가 아니다.**
//!   여기서 끝난 체크포인트는 `HASH_VERIFIED` 까지다.
//!   `writer.rs::find_resume_point*` 는 `COMMITTED` 마커를 요구하지 않으므로
//!   그것만으로 재개 후보가 된다(그 함수의 주석이 이유를 적어 뒀다).
//! - **PARTIAL 은 마커로 표시하지 않는다.** 판정 근거는 **매니페스트의
//!   부재**다 — 프로세스가 죽는 순간에는 아무 마커도 쓸 수 없으므로,
//!   마커에 의존하는 판정은 정확히 필요한 순간에 없다.
//! - **디렉터리 단위 동시 쓰기는 지원하지 않는다.** `write_once` 의 파일
//!   단위 락만 있다(`atomic.rs::gc_partial` 의 "알려진 한계" 참조).
//! - [`StagedCheckpoint::commit`] 는 매니페스트 직전 재검증과 Merkle root
//!   계산을 위해 **확정된 데이터 파일을 전부 다시 읽는다.** 스트리밍이
//!   아니다 — 피크 메모리는 체크포인트 전체 크기다. `stage()` 단계는
//!   파일 하나씩이다.

use std::path::{Path, PathBuf};

use crate::atomic::write_once;
use crate::durability::{
    record_initial_state, record_state_transition, CheckpointFile, CheckpointManifest,
    DurabilityState, MANIFEST_FILENAME,
};
use crate::CheckpointError;

/// 저장 이름에서 논리 이름과 내용 해시를 가르는 표식.
///
/// 논리 이름에는 이 문자열을 금지한다 — 그래야 되돌리기가 유일하다.
pub const CONTENT_ADDRESS_MARKER: &str = ".b3-";

/// 논리 이름 / 체크포인트 id 의 길이 상한.
///
/// 저장 이름은 여기에 `".b3-" + 64자` 가 더 붙는다. Windows 의 경로 길이
/// 제한(기본 `MAX_PATH`)에 여유를 두기 위한 값이지 규범 값이 아니다.
pub const MAX_LOGICAL_NAME_LEN: usize = 120;

/// 확정 절차에서 나올 수 있는 실패.
///
/// **모든 변형이 "왜 실패했는지"를 값으로 들고 있다.** `!ok` 만 보는
/// 테스트가 이 저장소에서 반복해 사고를 냈기 때문이다(`RULE.md` §6).
#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error("체크포인트 id {id:?} 를 쓸 수 없다: {reason}")]
    UnsafeCheckpointId { id: String, reason: &'static str },

    #[error("논리 파일 이름 {name:?} 을 쓸 수 없다: {reason}")]
    UnsafeLogicalName { name: String, reason: &'static str },

    /// 같은 논리 이름을 한 체크포인트 안에서 두 번 쓰려 했다.
    ///
    /// 내용이 같아도 거부한다. 재사용을 허용하면 "매니페스트에 같은 논리
    /// 이름이 두 번" 인 상태가 만들어지고, 재개할 때 어느 쪽이 진짜인지
    /// 알 수 없다.
    #[error("논리 이름 {name:?} 재사용 — 이미 {stored:?} 로 확정됐다. 한 체크포인트 안에서 같은 이름을 두 번 쓰지 않는다")]
    DuplicateLogicalName { name: String, stored: String },

    #[error("데이터 파일 {stored:?} 확정 실패: {source}")]
    DataFile {
        stored: String,
        #[source]
        source: CheckpointError,
    },

    /// 매니페스트 직전 재검증 — 확정했던 파일을 읽지 못했다.
    #[error("매니페스트 직전 재검증 실패: 확정된 데이터 파일 {stored:?} 을 읽을 수 없다 ({kind:?}): {source}")]
    StagedFileUnreadable {
        stored: String,
        kind: std::io::ErrorKind,
        #[source]
        source: std::io::Error,
    },

    /// 매니페스트 직전 재검증 — 확정했던 파일의 내용이 달라졌다.
    ///
    /// 이 경우 매니페스트를 **쓰지 않는다.** 쓰면 부분 상태가 완결로 보인다.
    #[error("매니페스트 직전 재검증 실패: 확정된 데이터 파일 {stored:?} 의 내용이 달라졌다 (기대 {expected}, 실제 {actual}) — 매니페스트를 쓰지 않는다")]
    StagedFileChanged {
        stored: String,
        expected: String,
        actual: String,
    },

    #[error("체크포인트 {checkpoint_id:?} 에 확정할 데이터 파일이 하나도 없다 — 빈 매니페스트는 재개 후보가 될 수 없다")]
    NothingStaged { checkpoint_id: String },

    /// 이미 매니페스트가 있는 디렉터리에 다시 쓰려 했다.
    ///
    /// 매니페스트는 완결 신호다. 완결된 것에 데이터를 더하는 절차는 없다.
    #[error("체크포인트 id {checkpoint_id:?} 재사용 — 이미 {MANIFEST_FILENAME} 가 있다(완결됨). 확정된 체크포인트에 덧쓰지 않는다")]
    AlreadyCommitted { checkpoint_id: String },

    /// 매니페스트 기록 자체가 실패했다. **이 체크포인트는 완결되지 않았다.**
    #[error(
        "매니페스트 기록 실패 — 데이터 파일은 확정됐으나 이 체크포인트는 PARTIAL 이다: {source}"
    )]
    ManifestWrite {
        #[source]
        source: CheckpointError,
    },

    #[error("상태 마커 기록 실패: {source}")]
    Marker {
        #[source]
        source: CheckpointError,
    },

    #[error("io: {0}")]
    Io(String),
}

/// 확정된 데이터 파일 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFile {
    /// 호출자가 부른 이름. 매니페스트에는 들어가지 않는다.
    pub logical_name: String,
    /// 디스크에 있는 이름 = `logical_name` + `.b3-` + digest.
    pub stored_name: String,
    /// 내용의 BLAKE3-256 hex.
    pub digest: String,
    pub size_bytes: u64,
    /// `false` = 같은 이름·같은 내용이 이미 있어 새로 쓰지 않았다(멱등).
    pub newly_written: bool,
}

/// 매니페스트에 들어갈, 파일 목록 이외의 값들.
///
/// **지어내지 않는다**(`CLAUDE.md` §1) — 호출자가 아는 값만 받는다.
#[derive(Debug, Clone, Default)]
pub struct ManifestMeta {
    pub job_id: String,
    pub attempt_id: String,
    pub step: u64,
    pub fence_epoch: u64,
    pub producer_node_id: String,
    pub created_at_unix_ms: u64,
}

/// 완결된 체크포인트.
#[derive(Debug, Clone)]
pub struct CommittedCheckpoint {
    pub dir: PathBuf,
    pub manifest: CheckpointManifest,
    /// `false` = 같은 내용의 매니페스트가 이미 있었다(재실행 멱등).
    pub manifest_newly_written: bool,
}

/// 저장 이름을 만든다. 이름이 내용을 결정한다(ADR-026).
pub fn stored_name_for(logical_name: &str, data: &[u8]) -> Result<String, CommitError> {
    validate_logical_name(logical_name)?;
    Ok(format!(
        "{logical_name}{CONTENT_ADDRESS_MARKER}{}",
        blake3::hash(data).to_hex()
    ))
}

/// 저장 이름에서 논리 이름을 되돌린다.
///
/// content-address 접미사가 규칙에 맞지 않으면 `None` — 즉 이 함수는
/// "이 이름이 이 절차가 만든 데이터 파일인가" 의 판정이기도 하다.
pub fn logical_name_of(stored_name: &str) -> Option<&str> {
    let (logical, digest) = stored_name.rsplit_once(CONTENT_ADDRESS_MARKER)?;
    if logical.is_empty() || digest.len() != 64 {
        return None;
    }
    if !digest
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    Some(logical)
}

/// ★ `writer.rs` 도 이것을 쓴다 (2026-09-06).
///
///   `write_checkpoint()` 가 `root.join(&manifest.checkpoint_id)` 를 **검증
///   없이** 하고 있었다. 매니페스트는 바깥에서 온 값이므로 `checkpoint_id`
///   가 `"../evil"` 이면 **체크포인트 루트 밖에 디렉터리가 생긴다.**
///
///   이 함수를 새로 만든 절차(`StagedCheckpoint`)에만 쓰고 옛 경로를
///   안 막으면, 같은 저장소에 **막는 문 하나와 안 막는 문 하나**가 남는다.
pub(crate) fn validate_component(value: &str, len_limit: usize) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("빈 이름");
    }
    if value.len() > len_limit {
        return Err("이름이 너무 길다 — 저장 이름에 해시 접미사가 더 붙는다");
    }
    if value.contains('/') || value.contains('\\') {
        return Err("경로 구분자 — 이 절차는 체크포인트 디렉터리 바로 아래 평평한 이름만 쓴다");
    }
    if value == "." || value == ".." {
        return Err("상위/현재 디렉터리 성분 — 체크포인트 밖으로 나간다");
    }
    if value.starts_with('.') {
        return Err("'.' 로 시작하는 이름은 상태 사이드카(.durability.* · .publication-failed) 전용으로 예약돼 있다");
    }
    if value.ends_with('.') || value.ends_with(' ') {
        return Err("후행 점/공백 — Win32 가 조용히 잘라내 다른 파일을 가리킨다");
    }
    if value.contains('\0') {
        return Err("NUL 문자");
    }
    Ok(())
}

fn validate_logical_name(name: &str) -> Result<(), CommitError> {
    if let Err(reason) = validate_component(name, MAX_LOGICAL_NAME_LEN) {
        return Err(CommitError::UnsafeLogicalName {
            name: name.to_string(),
            reason,
        });
    }
    if name.contains(CONTENT_ADDRESS_MARKER) {
        return Err(CommitError::UnsafeLogicalName {
            name: name.to_string(),
            reason: "'.b3-' 는 content-address 접미사 표식으로 예약돼 있다 — 논리 이름에 쓰면 저장 이름을 되돌릴 수 없다",
        });
    }
    Ok(())
}

fn validate_checkpoint_id(id: &str) -> Result<(), CommitError> {
    validate_component(id, MAX_LOGICAL_NAME_LEN).map_err(|reason| CommitError::UnsafeCheckpointId {
        id: id.to_string(),
        reason,
    })
}

/// 매니페스트 **파일이 있는가**. 내용이 유효한지는 보지 않는다.
///
/// ★ `gc_partial` 이 쓰는 판정과 같은 기준이어야 한다 — 그쪽도 "손상됐지만
///   존재함" 을 "없음" 으로 접지 않는다(접으면 데이터 유실이 된다).
fn manifest_present(dir: &Path) -> Result<bool, CommitError> {
    match crate::platform::read_beneath(dir, Path::new(MANIFEST_FILENAME)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CommitError::Io(error.to_string())),
    }
}

/// 데이터 파일을 하나씩 확정하고, 마지막에 매니페스트로 완결시키는 절차.
///
/// 이 타입은 `Drop` 에서 **아무것도 쓰지 않는다.** 프로세스가 죽는 순간에는
/// 어차피 Drop 이 돌지 않으므로, Drop 에 의존하는 정리는 정확히 필요한
/// 순간에 없다. 미완결의 유일한 신호는 **매니페스트의 부재**다.
#[derive(Debug)]
pub struct StagedCheckpoint {
    dir: PathBuf,
    checkpoint_id: String,
    staged: Vec<StagedFile>,
}

impl StagedCheckpoint {
    /// 체크포인트 디렉터리를 만들고 `WRITING` 을 기록한다.
    ///
    /// 이미 매니페스트가 있으면 [`CommitError::AlreadyCommitted`] 로 거부한다.
    pub fn begin(root: &Path, checkpoint_id: &str) -> Result<Self, CommitError> {
        validate_checkpoint_id(checkpoint_id)?;

        let dir = root.join(checkpoint_id);
        std::fs::create_dir_all(&dir).map_err(|error| CommitError::Io(error.to_string()))?;

        if manifest_present(&dir)? {
            return Err(CommitError::AlreadyCommitted {
                checkpoint_id: checkpoint_id.to_string(),
            });
        }

        record_initial_state(&dir).map_err(|source| CommitError::Marker { source })?;

        Ok(Self {
            dir,
            checkpoint_id: checkpoint_id.to_string(),
            staged: Vec::new(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn checkpoint_id(&self) -> &str {
        &self.checkpoint_id
    }

    pub fn staged(&self) -> &[StagedFile] {
        &self.staged
    }

    /// 데이터 파일 하나를 write-once 로 확정한다.
    ///
    /// 반환 시점에 그 파일은 **디스크에 확정돼 있다**(fsync + rename +
    /// `sync_dir`). 이 호출 직후에 프로세스가 죽어도 그 파일은 온전하고,
    /// 매니페스트가 없으므로 전체는 PARTIAL 로 남아 GC 대상이 된다.
    pub fn stage(&mut self, logical_name: &str, data: &[u8]) -> Result<&StagedFile, CommitError> {
        validate_logical_name(logical_name)?;

        if let Some(previous) = self
            .staged
            .iter()
            .find(|file| file.logical_name == logical_name)
        {
            return Err(CommitError::DuplicateLogicalName {
                name: logical_name.to_string(),
                stored: previous.stored_name.clone(),
            });
        }

        let digest = blake3::hash(data).to_hex().to_string();
        let stored_name = format!("{logical_name}{CONTENT_ADDRESS_MARKER}{digest}");

        let newly_written =
            write_once(&self.dir, &stored_name, data).map_err(|source| CommitError::DataFile {
                stored: stored_name.clone(),
                source,
            })?;

        self.staged.push(StagedFile {
            logical_name: logical_name.to_string(),
            stored_name,
            digest,
            size_bytes: data.len() as u64,
            newly_written,
        });

        Ok(self
            .staged
            .last()
            .expect("방금 push 했으므로 마지막 원소가 있다"))
    }

    /// **매니페스트를 마지막에** 써서 체크포인트를 완결시킨다.
    ///
    /// 순서를 바꾸면 부분 상태가 완결로 보인다. 그래서 이 함수의 순서가
    /// 계약이다:
    ///
    /// ```text
    /// 1  WRITING -> LOCAL_WRITTEN          (데이터 파일이 전부 확정됨)
    /// 2  확정된 데이터 파일 재검증          <- 매니페스트보다 먼저
    /// 3  manifest.json 을 write-once       <- 마지막
    /// 4  LOCAL_WRITTEN -> HASH_VERIFIED
    /// ```
    ///
    /// 2번이 3번보다 앞이라는 것이 핵심이다. 뒤집으면 내용이 달라진
    /// 파일에도 매니페스트가 붙는다 — 그 순간 그 디렉터리는 GC 대상에서
    /// 빠지고 재개 후보로 보인다.
    pub fn commit(self, meta: &ManifestMeta) -> Result<CommittedCheckpoint, CommitError> {
        if self.staged.is_empty() {
            return Err(CommitError::NothingStaged {
                checkpoint_id: self.checkpoint_id,
            });
        }

        record_state_transition(
            &self.dir,
            DurabilityState::Writing,
            DurabilityState::LocalWritten,
        )
        .map_err(|source| CommitError::Marker { source })?;

        let contents = self.verify_staged_files()?;

        let manifest = self.build_manifest(meta, &contents);
        let json = manifest
            .to_json()
            .map_err(|source| CommitError::ManifestWrite { source })?;

        let manifest_newly_written = write_once(&self.dir, MANIFEST_FILENAME, &json)
            .map_err(|source| CommitError::ManifestWrite { source })?;

        record_state_transition(
            &self.dir,
            DurabilityState::LocalWritten,
            DurabilityState::HashVerified,
        )
        .map_err(|source| CommitError::Marker { source })?;

        Ok(CommittedCheckpoint {
            dir: self.dir,
            manifest,
            manifest_newly_written,
        })
    }

    /// 확정된 데이터 파일을 **디스크에서 다시 읽어** 대조한다.
    ///
    /// `stage()` 가 기억한 값이 아니라 지금 디스크에 있는 바이트를 본다 —
    /// 그 사이에 무슨 일이 있었는지는 메모리가 알려주지 않는다.
    /// 읽기는 `read_beneath` 를 쓴다(경로 이탈·reparse point 방어).
    fn verify_staged_files(&self) -> Result<Vec<Vec<u8>>, CommitError> {
        let mut contents = Vec::with_capacity(self.staged.len());

        for file in &self.staged {
            let data = crate::platform::read_beneath(&self.dir, Path::new(&file.stored_name))
                .map_err(|source| CommitError::StagedFileUnreadable {
                    stored: file.stored_name.clone(),
                    kind: source.kind(),
                    source,
                })?;

            let actual = blake3::hash(&data).to_hex().to_string();
            if actual != file.digest {
                return Err(CommitError::StagedFileChanged {
                    stored: file.stored_name.clone(),
                    expected: file.digest.clone(),
                    actual,
                });
            }

            contents.push(data);
        }

        Ok(contents)
    }

    fn build_manifest(&self, meta: &ManifestMeta, contents: &[Vec<u8>]) -> CheckpointManifest {
        let files: Vec<CheckpointFile> = self
            .staged
            .iter()
            .map(|file| CheckpointFile {
                path: file.stored_name.clone(),
                digest: file.digest.clone(),
                size_bytes: file.size_bytes,
            })
            .collect();

        let total_bytes = files.iter().map(|file| file.size_bytes).sum();

        let chunks: Vec<&[u8]> = contents.iter().map(|data| data.as_slice()).collect();
        let root_digest = gputeer_protocol::merkle_root(&chunks)
            .map(|root| {
                root.iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            })
            .unwrap_or_default();

        CheckpointManifest {
            schema_version: 1,
            checkpoint_id: self.checkpoint_id.clone(),
            job_id: meta.job_id.clone(),
            attempt_id: meta.attempt_id.clone(),
            step: meta.step,
            files,
            root_digest,
            total_bytes,
            created_at_unix_ms: meta.created_at_unix_ms,
            producer_node_id: meta.producer_node_id.clone(),
            fence_epoch: meta.fence_epoch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_name_round_trips_through_logical_name_of() {
        let stored = stored_name_for("model.bin", b"weights").unwrap();
        assert!(stored.starts_with("model.bin.b3-"));
        assert_eq!(logical_name_of(&stored), Some("model.bin"));
    }

    #[test]
    fn logical_name_of_rejects_names_this_procedure_did_not_make() {
        assert_eq!(logical_name_of("model.bin"), None, "접미사 없음");
        assert_eq!(logical_name_of("model.bin.b3-short"), None, "길이 불일치");
        assert_eq!(
            logical_name_of(&format!("model.bin.b3-{}", "Z".repeat(64))),
            None,
            "hex 가 아님"
        );
        assert_eq!(
            logical_name_of(&format!(".b3-{}", "a".repeat(64))),
            None,
            "논리 이름이 비었다"
        );
    }

    #[test]
    fn content_addressed_names_cannot_collide_with_reserved_suffixes() {
        // 저장 이름은 항상 hex 로 끝난다 — 그래서 write_once 가 예약한
        // 이름공간(.tmp · .write_once.lock)과 구조적으로 겹칠 수 없다.
        let stored = stored_name_for("a", b"x").unwrap();
        assert!(!stored.ends_with(".tmp"));
        assert!(!stored.ends_with(".write_once.lock"));
        assert!(!stored.starts_with('.'));
    }
}
