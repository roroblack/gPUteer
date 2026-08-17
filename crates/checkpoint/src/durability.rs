//! Durability Contract — 기준선 §18.2.
//!
//! v4 는 "Committed checkpoint 에서 복구한다" 고 하면서 committed 의 정의가 없었다.
//! fsync 인지, 복제 완료인지, Raft 커밋인지 불명이었다.
//!
//! 상태 전이는 `docs/protocol/state-machines.md` §4 가 규범이다.
//! 표에 없는 전이는 구현하지 않는다(MUST NOT).
//!
//! 상태는 매니페스트를 덮어써서 기록하지 않는다.
//! ADR-026 의 write-once 원칙을 지키기 위해 상태별 불변 사이드카를 기록한다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic::write_once;
use crate::CheckpointError;

/// `docs/protocol/state-machines.md` §4 의 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurabilityState {
    Writing,

    /// 매니페스트 없이 데이터 파일만 존재. 부팅 시 GC 대상.
    Partial,

    LocalWritten,
    HashVerified,
    Replicating,
    Replicated,
    Committed,

    /// COMMITTED 이후 replica 가 유실됨.
    ///
    /// 상태를 되돌리지 않는다. 이미 이 체크포인트를 근거로
    /// 다른 결정이 내려졌을 수 있기 때문이다.
    CommittedDegraded,
}

/// Job 이 요구하는 COMMITTED 조건. `proto/common.proto` 의 `Durability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Durability {
    /// HASH_VERIFIED 로 충분. 워커 디스크 고장 시 유실.
    Local,

    /// REPLICATED(1). 기본값.
    Mirrored,

    /// REPLICATED(2), 서로 다른 failure domain.
    Replicated,
}

impl Durability {
    /// COMMITTED 도달에 필요한 유효 replica 수.
    pub fn required_replicas(self) -> u32 {
        match self {
            Durability::Local => 0,
            Durability::Mirrored => 1,
            Durability::Replicated => 2,
        }
    }
}

impl DurabilityState {
    /// `state-machines.md` §4 전이표.
    ///
    /// 표에 없는 전이는 거부한다.
    pub fn can_transition_to(self, to: DurabilityState) -> bool {
        use DurabilityState::*;

        matches!(
            (self, to),
            (Writing, LocalWritten)
                | (Writing, Partial)
                | (LocalWritten, HashVerified)
                | (LocalWritten, Partial)
                | (HashVerified, Replicating)
                | (HashVerified, Committed)
                | (Replicating, Replicated)
                | (Replicating, HashVerified)
                | (Replicated, Committed)
                | (Replicated, Replicating)
                | (Committed, CommittedDegraded)
                | (CommittedDegraded, Committed)
        )
    }

    pub fn transition(self, to: DurabilityState) -> Result<DurabilityState, CheckpointError> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(CheckpointError::InvalidTransition { from: self, to })
        }
    }

    /// canonical 후보 자격이 있는가.
    ///
    /// `CommittedDegraded` 도 자격을 유지한다.
    pub fn is_canonical_candidate(self) -> bool {
        matches!(
            self,
            DurabilityState::Committed | DurabilityState::CommittedDegraded
        )
    }
}

/// 상태 기록 파일 이름.
///
/// 상태 파일도 write-once artifact 이다. 매니페스트를 덮어쓰지 않는 이유는
/// ADR-026 의 immutable artifact 원칙과 매니페스트 마지막 기록 규칙을
/// 동시에 지키기 위해서다.
pub fn state_marker_name(state: DurabilityState) -> &'static str {
    match state {
        DurabilityState::Writing => ".durability.writing",
        DurabilityState::Partial => ".durability.partial",
        DurabilityState::LocalWritten => ".durability.local-written",
        DurabilityState::HashVerified => ".durability.hash-verified",
        DurabilityState::Replicating => ".durability.replicating",
        DurabilityState::Replicated => ".durability.replicated",
        DurabilityState::Committed => ".durability.committed",
        DurabilityState::CommittedDegraded => ".durability.committed-degraded",
    }
}

pub fn state_marker_path(dir: &Path, state: DurabilityState) -> PathBuf {
    dir.join(state_marker_name(state))
}

/// 포인터 갱신 실패 또는 포인터 이후 상태 기록 실패를 표시한다.
///
/// 완전한 파일과 매니페스트를 삭제하지 않는다. 이 마커가 있는 디렉터리는
/// `find_resume_point*` 에서 제외한다.
pub const PUBLICATION_FAILED_MARKER: &str = ".publication-failed";

pub fn publication_failed_marker_path(dir: &Path) -> PathBuf {
    dir.join(PUBLICATION_FAILED_MARKER)
}

/// WRITING 시작을 디스크에 기록한다.
pub fn record_initial_state(dir: &Path) -> Result<(), CheckpointError> {
    let marker = state_marker_name(DurabilityState::Writing);
    let data = b"Writing\n";
    write_once(dir, marker, data)?;
    Ok(())
}

/// 규범 표의 전이를 검증한 뒤 목적지 상태를 write-once 로 기록한다.
///
/// 매니페스트를 다시 쓰지 않으므로 상태 전이가 원자성을 깨지 않는다.
pub fn record_state_transition(
    dir: &Path,
    from: DurabilityState,
    to: DurabilityState,
) -> Result<(), CheckpointError> {
    from.transition(to)?;

    let marker = state_marker_name(to);
    let data = format!("{to:?}\n");
    write_once(dir, marker, data.as_bytes())?;
    Ok(())
}

/// 포인터 공개 실패를 불변 마커로 기록한다.
pub fn record_publication_failure(dir: &Path) -> Result<(), CheckpointError> {
    write_once(
        dir,
        PUBLICATION_FAILED_MARKER,
        b"checkpoint publication failed\n",
    )?;
    Ok(())
}

/// 상태 마커의 내용까지 확인한다.
///
/// 파일이 없으면 아직 해당 상태에 도달하지 않은 것이다.
pub fn state_recorded(
    dir: &Path,
    state: DurabilityState,
) -> Result<bool, CheckpointError> {
    let path = state_marker_path(dir, state);
    let expected = format!("{state:?}\n");

    match std::fs::read(path) {
        Ok(actual) => Ok(actual == expected.as_bytes()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn publication_failed(dir: &Path) -> Result<bool, CheckpointError> {
    match std::fs::read(publication_failed_marker_path(dir)) {
        Ok(actual) => Ok(actual == b"checkpoint publication failed\n"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// 체크포인트를 구성하는 파일 하나.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointFile {
    /// 체크포인트 루트 기준 상대 경로. `..` · 절대경로 · 심볼릭 링크 금지.
    pub path: String,

    /// BLAKE3-256 hex.
    pub digest: String,

    pub size_bytes: u64,
}

/// `proto/artifact.proto` 의 `CheckpointManifest` 에 대응.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointManifest {
    pub schema_version: u32,
    pub checkpoint_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub step: u64,
    pub files: Vec<CheckpointFile>,
    pub root_digest: String,
    pub total_bytes: u64,
    pub created_at_unix_ms: u64,
    pub producer_node_id: String,
    pub fence_epoch: u64,
}

pub const MANIFEST_FILENAME: &str = "manifest.json";

impl CheckpointManifest {
    /// 매니페스트에 적힌 모든 파일의 해시를 재계산해 대조한다.
    pub fn verify_files(&self, dir: &Path) -> Result<(), CheckpointError> {
        for file in &self.files {
            let path = dir.join(&file.path);
            let data = std::fs::read(&path)?;
            let actual = blake3::hash(&data).to_hex().to_string();

            if actual != file.digest {
                return Err(CheckpointError::HashMismatch {
                    path,
                    expected: file.digest.clone(),
                    actual,
                });
            }
        }

        Ok(())
    }

    pub fn to_json(&self) -> Result<Vec<u8>, CheckpointError> {
        serde_json::to_vec_pretty(self)
            .map_err(|error| CheckpointError::Manifest(error.to_string()))
    }

    pub fn from_json(data: &[u8]) -> Result<Self, CheckpointError> {
        serde_json::from_slice(data)
            .map_err(|error| CheckpointError::Manifest(error.to_string()))
    }
}

/// 유효 replica 계수.
#[derive(Debug, Clone, Default)]
pub struct ReplicaSet {
    /// failure_domain -> (device_id, is_ephemeral, signature_valid)
    entries: BTreeMap<String, (String, bool, bool)>,
}

impl ReplicaSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(
        &mut self,
        failure_domain: impl Into<String>,
        device_id: impl Into<String>,
        is_ephemeral: bool,
        signature_valid: bool,
    ) -> &mut Self {
        self.entries.insert(
            failure_domain.into(),
            (device_id.into(), is_ephemeral, signature_valid),
        );
        self
    }

    /// 규칙 1~4 를 적용한 유효 replica 수.
    pub fn effective_count(&self) -> u32 {
        self.entries
            .values()
            .filter(|(_, is_ephemeral, signature_valid)| {
                *signature_valid && !*is_ephemeral
            })
            .count() as u32
    }

    pub fn satisfies(&self, required: Durability) -> bool {
        self.effective_count() >= required.required_replicas()
    }
}
