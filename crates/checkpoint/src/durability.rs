//! Durability Contract — 기준선 §18.2.
//!
//! v4 는 "Committed checkpoint 에서 복구한다" 고 하면서 **committed 의 정의가 없었다.**
//! fsync 인지, 복제 완료인지, Raft 커밋인지 불명이었다.
//!
//! 상태 전이는 `docs/protocol/state-machines.md` §4 가 규범이다.
//! **표에 없는 전이는 구현하지 않는다(MUST NOT).**

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

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
    /// **상태를 되돌리지 않는다** — 이미 이것을 근거로 다른 결정이 내려졌을 수 있다.
    /// canonical 후보 자격은 유지하고 복구 큐에 넣는다.
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
    /// `state-machines.md` §4 전이표. **표에 없는 전이는 거부한다.**
    pub fn can_transition_to(self, to: DurabilityState) -> bool {
        use DurabilityState::*;
        matches!(
            (self, to),
            (Writing, LocalWritten)
                | (Writing, Partial)
                | (LocalWritten, HashVerified)
                | (LocalWritten, Partial)          // 해시 불일치
                | (HashVerified, Replicating)
                | (HashVerified, Committed)        // durability == Local
                | (Replicating, Replicated)
                | (Replicating, HashVerified)      // 전송 실패 / ACK timeout
                | (Replicated, Committed)
                | (Replicated, Replicating)        // 요구치 미달로 하락
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
    /// `CommittedDegraded` 도 자격을 유지한다 — 기준선 §18.2.
    pub fn is_canonical_candidate(self) -> bool {
        matches!(self, DurabilityState::Committed | DurabilityState::CommittedDegraded)
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
///
/// **매니페스트는 데이터 파일이 모두 확정된 뒤 마지막에 쓴다.**
/// 이것이 PARTIAL 판정의 근거다 (기준선 §18.2 규칙 2·3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointManifest {
    pub schema_version: u32,
    pub checkpoint_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub step: u64,
    pub files: Vec<CheckpointFile>,
    /// files 전체에 대한 Merkle root (signing.md §6.3). hex.
    pub root_digest: String,
    pub total_bytes: u64,
    pub created_at_unix_ms: u64,
    pub producer_node_id: String,
    pub fence_epoch: u64,
}

pub const MANIFEST_FILENAME: &str = "manifest.json";

impl CheckpointManifest {
    /// 매니페스트에 적힌 모든 파일의 해시를 실제로 재계산해 대조한다.
    ///
    /// `LocalWritten -> HashVerified` 전이의 근거다.
    pub fn verify_files(&self, dir: &Path) -> Result<(), CheckpointError> {
        for f in &self.files {
            let path = dir.join(&f.path);
            let data = std::fs::read(&path)?;
            let actual = blake3::hash(&data).to_hex().to_string();
            if actual != f.digest {
                return Err(CheckpointError::HashMismatch {
                    path,
                    expected: f.digest.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<Vec<u8>, CheckpointError> {
        serde_json::to_vec_pretty(self).map_err(|e| CheckpointError::Manifest(e.to_string()))
    }

    pub fn from_json(data: &[u8]) -> Result<Self, CheckpointError> {
        serde_json::from_slice(data).map_err(|e| CheckpointError::Manifest(e.to_string()))
    }
}

/// 유효 replica 계수 (기준선 §18.2 규칙 1~4).
///
/// - 서명된 ACK 만 계산에 넣는다
/// - 같은 failure domain 여러 개는 1개로 센다
/// - ephemeral 노드의 로컬 복사본은 세지 않는다
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
        self.entries
            .insert(failure_domain.into(), (device_id.into(), is_ephemeral, signature_valid));
        self
    }

    /// 규칙 1~4 를 적용한 유효 replica 수.
    pub fn effective_count(&self) -> u32 {
        self.entries
            .values()
            .filter(|(_, is_ephemeral, sig_ok)| *sig_ok && !*is_ephemeral)
            .count() as u32
    }

    pub fn satisfies(&self, required: Durability) -> bool {
        self.effective_count() >= required.required_replicas()
    }
}
