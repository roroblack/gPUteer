//! 체크포인트 durability contract — 기준선 §18.2 + ADR-026 의 구현체.
//!
//! 상태 전이는 `docs/protocol/state-machines.md` §4 를 따른다.
//! **표에 없는 전이는 구현하지 않는다.**

pub mod atomic;
pub mod commit;
pub mod durability;
pub mod platform;
pub mod writer;

pub use atomic::{gc_partial, replace_with_retry, sync_dir, write_once, RetryPolicy};
pub use commit::{
    logical_name_of, stored_name_for, CommitError, CommittedCheckpoint, ManifestMeta,
    StagedCheckpoint, StagedFile, CONTENT_ADDRESS_MARKER,
};
pub use durability::{
    evaluate_effective_replicas, CheckpointFile, CheckpointManifest, CountedReplica, Durability,
    DurabilityState, EffectiveReplicaReport, ExcludedReplica, FactResolution, HolderValidation,
    ReplicaEvaluationError, ReplicaEvaluationScope, ReplicaExclusionReason, ReplicaKind,
    ResolvedHolderObservation,
};
pub use writer::{find_resume_point, manifest_for, read_pointer, startup_gc, write_checkpoint};

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

    /// ★ write-once 대상이 이미 존재하는데 **내용이 다르다** (2026-08-16 신설).
    ///
    /// `write_once` 는 이름이 같으면 내용도 같다고 **가정**했었다.
    /// 그러나 `writer.rs` 의 이름은 `shard-0.bin` 같은 위치 기반이지
    /// content-addressed 가 아니다. 가정이 지켜지지 않았다.
    ///
    /// 이것을 조용히 통과시키면 **writer 가 "확정했다" 고 거짓 보고한다** —
    /// 매니페스트에는 새 해시가, 디스크에는 옛 데이터가 남는다.
    #[error("write-once 내용 불일치 {path:?}: 기존 {existing_len}B, 새 {incoming_len}B —              같은 이름에 다른 내용을 쓰려 했다. 앞선 writer 가 중단됐을 수 있다")]
    ContentMismatch {
        path: PathBuf,
        existing_len: usize,
        incoming_len: usize,
    },

    /// ★ 동일 `(dir, name)` 에 대한 동시 `write_once` 호출 (2026-08-19 신설).
    ///
    /// `write_once` 는 동시 동일-이름 호출을 지원하지 않는다는 계약을
    /// 명시적으로 강제한다(`docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md`).
    /// 다른 호출자가 이미 같은 파일을 쓰는 중이면 대기하지 않고
    /// 즉시 이 오류를 반환한다 — 무기한 대기는 장애를 숨긴다.
    #[error("write-once 진행 중 {path:?}: 다른 호출자가 이미 같은 파일을 쓰고 있다")]
    WriteInProgress { path: PathBuf },

    /// ★ `RetryPolicy::max_attempts == 0` (2026-08-16 신설).
    ///
    /// ADR-026 은 "최종 실패는 명시적 오류" 를 계약으로 정한다.
    /// 예전에는 이 경우 `expect` 에서 **패닉**했다 — panic 은 오류가 아니다.
    #[error("RetryPolicy.max_attempts 가 0이다 — 최소 1회는 시도해야 한다 (ADR-026)")]
    InvalidRetryPolicy,

    /// ★ 체크포인트 밖을 가리키는 경로 (2026-08-16 신설).
    ///
    /// `proto/common.proto` 의 `CheckpointFile.path` 는 `".."` · 절대경로 ·
    /// 심볼릭 링크를 금지한다고 적어 놓고 **아무도 검사하지 않았다.**
    ///
    /// 파일 이름은 매니페스트에서 오는 **외부 입력**이고,
    /// 이 시스템은 **남의 개인 PC 에서** 돌아간다 (`CLAUDE.md` §0).
    ///
    /// ★★ **2026-09-06 — `checkpoint_id`(디렉터리 이름)도 여기로 온다.**
    ///
    ///   `write_checkpoint()` 가 `root.join(&manifest.checkpoint_id)` 를
    ///   검증 없이 하고 있었다. 즉 이 변형이 **파일 이름은 지키는데
    ///   디렉터리 이름은 안 지키고** 있었다 — `atomic.rs` 의
    ///   `validate_relative_name()` 이 파일에만 걸렸기 때문이다.
    ///   `checkpoint_id` 가 `"../evil"` 이면 루트 밖에 디렉터리가 생겼다.
    ///
    ///   ★ 새 변형을 만들지 않고 이것을 쓴다. 같은 성격의 거부를 두
    ///     이름으로 나누면 호출부가 **둘 중 하나만** 처리하게 된다.
    ///     `name` 칸에 파일 이름이든 `checkpoint_id` 든 거부된 값이 온다.
    #[error("안전하지 않은 경로 {name:?}: {reason}")]
    UnsafePath { name: String, reason: &'static str },

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
