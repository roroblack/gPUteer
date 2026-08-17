//! VRAM quota 강제와 S1 호스트 보호 — `CLAUDE.md` §0.4 가 이미 고정했다.
//!
//! 이 모듈은 새 판단을 만들지 않는다. `CLAUDE.md` 가 실측(ADR-027, P0-06)
//! 으로 이미 내린 결론을 **타입으로 다시 우회할 수 없게** 만든다.
//!
//! ```text
//! 소비자 GPU VRAM quota   강제 수단 없음. MIG 데이터센터 전용,
//!                         MPS Linux 전용, cgroup 은 RAM 만.
//!                         -> 기본값 Exclusive.
//!
//! Windows Job Object      간접 제한 가능 (VRAM 최대 ≈ RAM 제한 − 2000MiB).
//!                         ★ quota 가 아니라 총 커밋 상한. 거칠고 Windows 전용.
//!                         Exclusive 기본값은 유지한다 (ADR-027).
//!
//! S1(Windows Restricted   임의 네이티브 코드로부터 호스트를 지키지 못한다.
//!  Native)                "S1 이상이면 안전" 이라는 표현을 쓰지 않는다.
//! ```

/// VRAM 을 실제로 제한할 수 있는가.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VramEnforcement {
    /// 강제 수단이 없다. 소비자 GPU 의 기본 상태.
    NoQuotaMechanism,
    /// Windows Job Object 의 간접 제한. **quota 가 아니라 총 커밋 상한**이다.
    WindowsCommitCap { approx_vram_max_bytes: u64 },
}

impl VramEnforcement {
    /// 이 등급에서 VRAM 초과를 **막을 수 있는가.**
    ///
    /// ★ `WindowsCommitCap` 도 `false` 다. 그것은 "간접적으로 제한된다"
    ///   이지 "quota 가 강제된다" 가 아니다. `CLAUDE.md` 원문 그대로다.
    pub fn guarantees_hard_limit(self) -> bool {
        false
    }
}

/// `WindowsCommitCap` 의 근사 계산. RAM 제한에서 예약분을 뺀다.
///
/// ★ 이 상수(2000MiB)는 P0-06 실측값이다. 재측정 없이 다른 값으로
///   바꾸지 않는다 — `CLAUDE.md` §1, "지어내지 않는다."
pub const WINDOWS_JOB_OBJECT_RESERVED_BYTES: u64 = 2000 * 1024 * 1024;

pub fn windows_commit_cap(ram_limit_bytes: u64) -> VramEnforcement {
    VramEnforcement::WindowsCommitCap {
        approx_vram_max_bytes: ram_limit_bytes.saturating_sub(WINDOWS_JOB_OBJECT_RESERVED_BYTES),
    }
}

/// S1(Windows Restricted Native)이 호스트를 보호한다는 주장.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostProtectionClaim {
    /// S1 이 임의 네이티브 코드로부터 호스트를 지킨다 — **거짓 주장.**
    S1ProtectsAgainstArbitraryNativeCode,
    /// S1 은 그런 보호를 제공하지 않는다는 정직한 주장.
    S1DoesNotProtectHost,
}

impl HostProtectionClaim {
    /// 이 주장이 `CLAUDE.md` §0.4 와 합치하는가.
    pub fn is_honest(self) -> bool {
        matches!(self, Self::S1DoesNotProtectHost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 통과가 곧 "아직 못 막는다" 는 뜻임을 이름과 주석에 남긴다.
    #[test]
    fn vram_quota_is_not_enforceable_on_consumer_gpu() {
        assert!(!VramEnforcement::NoQuotaMechanism.guarantees_hard_limit());
    }

    /// Windows 간접 제한도 "hard limit" 이라고 주장하면 안 된다.
    #[test]
    fn windows_commit_cap_is_not_a_hard_quota_either() {
        let cap = windows_commit_cap(8 * 1024 * 1024 * 1024);
        assert!(
            !cap.guarantees_hard_limit(),
            "★ Windows Job Object 간접 제한을 hard quota 로 주장했다 — ADR-027 위반"
        );
    }

    /// 근사 계산이 실측 상수를 실제로 쓰는가 — 비공허성.
    #[test]
    fn commit_cap_subtracts_measured_reservation() {
        let cap = windows_commit_cap(8 * 1024 * 1024 * 1024);
        match cap {
            VramEnforcement::WindowsCommitCap { approx_vram_max_bytes } => {
                assert_eq!(
                    approx_vram_max_bytes,
                    8 * 1024 * 1024 * 1024 - WINDOWS_JOB_OBJECT_RESERVED_BYTES
                );
            }
            other => panic!("잘못된 variant: {other:?}"),
        }
    }

    /// RAM 제한이 예약분보다 작으면 saturating_sub 로 0 이 되어야 한다
    /// (음수로 넘치지 않는다).
    #[test]
    fn commit_cap_does_not_underflow_when_ram_limit_is_tiny() {
        let cap = windows_commit_cap(100);
        match cap {
            VramEnforcement::WindowsCommitCap { approx_vram_max_bytes } => {
                assert_eq!(approx_vram_max_bytes, 0);
            }
            other => panic!("잘못된 variant: {other:?}"),
        }
    }

    #[test]
    fn restricted_native_does_not_protect_host() {
        assert!(HostProtectionClaim::S1DoesNotProtectHost.is_honest());
        assert!(
            !HostProtectionClaim::S1ProtectsAgainstArbitraryNativeCode.is_honest(),
            "★ S1 이 호스트를 보호한다는 주장은 정직하지 않다 (CLAUDE.md §0.4)"
        );
    }
}
