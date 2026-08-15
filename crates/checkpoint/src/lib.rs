//! 체크포인트 durability contract — 기준선 §18.2 + ADR-026 의 구현체.
//!
//! 상태 전이는 `docs/protocol/state-machines.md` §4 를 따른다.
//! **표에 없는 전이는 구현하지 않는다.**

pub mod atomic;
pub mod durability;

pub use atomic::{gc_partial, replace_with_retry, sync_dir, write_once, RetryPolicy};
pub use durability::{DurabilityState, Durability, CheckpointManifest, CheckpointFile};

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("io: {0}")]
    Io(String),

    #[error("rename 실패 {from:?} -> {to:?}: {source}")]
    Rename {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// 포인터 replace 재시도 소진. 조용히 넘어가지 않는다 (RULE.md §3.2).
    #[error("replace 재시도 {attempts}회 소진 {path:?}: {source}")]
    ReplaceExhausted {
        path: PathBuf,
        attempts: u32,
        #[source]
        source: std::io::Error,
    },

    #[error("해시 불일치 {path:?}: 기대 {expected}, 실제 {actual}")]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("허용되지 않는 상태 전이: {from:?} -> {to:?} (state-machines.md §4)")]
    InvalidTransition {
        from: DurabilityState,
        to: DurabilityState,
    },

    #[error("매니페스트 파싱: {0}")]
    Manifest(String),
}
