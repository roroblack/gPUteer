//! 정책 강제 계층 — `TODO_VISION` V-06.
//!
//! # 왜 이 크레이트가 있나
//!
//! `CLAUDE.md` 가 스스로 적어 뒀다:
//!
//! > 서명은 되지만 강제는 없다
//! >   network(54) · artifact_scope(55) · Lease.scope(40) 이 서명 대상에
//! >   들어갔다. 그러나 Agent 가 그 정책을 강제하는 계층은 없다.
//! >   = 위조를 막은 것이지 정책을 시행한 것이 아니다.
//!
//! `Verified<M>` 는 서명·시각·replay 검증을 통과했다는 것만 보장한다.
//! **그 안의 정책 필드가 실제로 지켜지는지는 아무도 보지 않았다.**
//!
//! # ★ 이 크레이트가 하는 일과 하지 않는 일
//!
//! `CLAUDE.md` §0.4 — 강제할 수 없는 것을 보장으로 선언하지 않는다.
//! 그래서 각 정책 필드를 셋 중 하나로 분류하고, **분류가 정직한지**를
//! 이 크레이트가 강제한다.
//!
//! ```text
//! Enforceable    이 프로세스가 스스로 판정하고 실제로 막을 수 있다
//!                (예: 경로 문자열이 허용 접두사 밖인지)
//!
//! Suppressible   완전히 막지는 못하지만 정직한 경로에서는 억제한다
//!                (예: fence_epoch — 우리가 소유한 자원에는 통하지만
//!                 외부 API 는 epoch 를 모른다)
//!
//! Unenforceable  이 계층이 판정할 수 없다. 판정하는 척하지 않는다
//!                (예: VRAM quota, S1 이 임의 네이티브 코드를 막는가)
//! ```
//!
//! **`Unenforceable` 로 분류된 것은 항상 거부하거나, 항상 "모른다" 로
//! 응답한다.** 실행을 허용해 놓고 안전한 척하지 않는다.
//!
//! # 무엇을 만들지 않았나
//!
//! - OS 방화벽 호출 (Windows `netsh`/COM, Linux `iptables`) — 이 크레이트는
//!   순수 판정 로직이다. 실제 시스템 호출은 `runtime-windows`/`runtime-container`
//!   가 생기면 그쪽이 이 크레이트의 판정을 받아 시스템을 조작한다.
//! - `openat2(RESOLVE_BENEATH)` 같은 커널 수준 경로 강제 — 이 crate 는
//!   **문자열 검사**만 한다. TOCTOU(검사 후 symlink 교체)는 막지 못한다.
//!   그 사실을 [`ArtifactPolicy::check`] 문서에 명시한다.

pub mod artifact;
pub mod lease_scope;
pub mod network;
pub mod vram;

pub use artifact::{ArtifactPolicy, ArtifactViolation};
pub use lease_scope::{FenceWatermark, LeaseScopeViolation};
pub use network::{NetworkDecision, NetworkPolicyCheck, NoFirewallBackend, OsFirewallBackend};
pub use vram::{HostProtectionClaim, VramEnforcement};

/// 이 계층이 한 정책 필드에 대해 낼 수 있는 세 가지 판정.
///
/// ★ 이름 자체가 진실을 말해야 한다 — `CLAUDE.md` §0.4.
///   "허용" 이 아니라 "이 계층이 그것을 강제할 수 있는가" 를 답한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementClass {
    /// 이 프로세스가 스스로 판정하고 실제로 막을 수 있다.
    Enforceable,
    /// 정직한 경로에서는 억제하지만 완전히 막지는 못한다.
    Suppressible,
    /// 이 계층은 판정할 수 없다.
    Unenforceable,
}
