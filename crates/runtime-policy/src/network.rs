//! `NetworkPolicy`(job.proto 필드 54) 강제 — Job 이 접속할 수 있는 호스트.
//!
//! # 강제 가능성 (실측 기반)
//!
//! `EnforcementClass::Enforceable` — **OS 방화벽 백엔드가 실제로 있을 때만.**
//! 백엔드가 없으면 `Unenforceable` 로 떨어지고 **실행을 거부한다.**
//!
//! ```text
//! Windows   프로세스별 방화벽 규칙은 별도 OS 백엔드(WFP/netsh)가 필요하다.
//!           P0-01 evidence 는 방화벽을 실측하지 않았다 — S1(Restricted Native)
//!           검증은 CUDA·프로세스 격리를 봤을 뿐 네트워크 필터링은 아니었다.
//! Linux     D-3 로 미검증. 한 번도 안 돌려봤다.
//! ```
//!
//! ★ **`P0-01` 이 방화벽을 재본 적이 없다는 사실 자체가 이 모듈의 존재
//!   이유다.** "서명 대상에 들어갔다" 를 "강제된다" 로 읽으면 안 된다.
//!
//! `mediated_dns` 는 이 모듈이 판정하지 않는다 — DNS 가로채기는
//! 프로세스 네트워크 네임스페이스나 시스템 리졸버 후킹이 필요하고,
//! 그것도 OS 백엔드의 일이다.

/// OS 방화벽 강제의 실제 구현. 이 크레이트에는 **구현체가 없다** —
/// `runtime-windows`/`runtime-container` 가 생기면 그쪽이 구현한다.
///
/// ★ trait 을 여기 두는 이유: 정책 판정(`NetworkPolicyCheck`)과 시스템
///   호출을 분리해야 이 크레이트를 시스템 콜 없이 테스트할 수 있다.
pub trait OsFirewallBackend {
    /// 이 프로세스(또는 그 자식)에 대해 `allowed_hosts` 외 아웃바운드를
    /// 실제로 차단할 수 있는가.
    ///
    /// `false` 를 반환하는 것이 **정직한 기본값**이다 — 구현하지 않은
    /// 백엔드가 "된다" 고 답하면 그 자체가 거짓 보장이다.
    fn can_enforce(&self) -> bool;
}

/// 백엔드가 없음을 **명시하는** 자리표시자.
///
/// `Option<&dyn OsFirewallBackend>` 대신 이 타입을 쓰는 이유: 호출부에서
/// `None` 을 실수로 "아직 안 정했다" 와 "구현이 없다" 사이에서 헷갈리지
/// 않게 한다. 이름이 사실을 말한다.
pub struct NoFirewallBackend;

impl OsFirewallBackend for NoFirewallBackend {
    fn can_enforce(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkDecision {
    /// 백엔드가 실제로 강제할 수 있고, 요청이 허용 목록 안이다.
    Allowed,
    /// 백엔드가 실제로 강제할 수 있지만, 요청이 허용 목록 밖이다.
    Denied { host: String },
    /// ★ OS 백엔드가 없어 **강제 자체가 불가능하다.**
    ///   "일단 허용" 이 아니라 실행을 거부하는 것이 안전한 방향이다 —
    ///   CLAUDE.md §0.4, 강제 못 하는 것을 강제한다고 쓰지 않는다.
    NoEnforcementBackend,
}

pub struct NetworkPolicyCheck<'a> {
    runtime_allow_hosts: &'a [String],
    backend: &'a dyn OsFirewallBackend,
}

impl<'a> NetworkPolicyCheck<'a> {
    pub fn new(runtime_allow_hosts: &'a [String], backend: &'a dyn OsFirewallBackend) -> Self {
        Self {
            runtime_allow_hosts,
            backend,
        }
    }

    pub fn decide(&self, requested_host: &str) -> NetworkDecision {
        if !self.backend.can_enforce() {
            return NetworkDecision::NoEnforcementBackend;
        }
        if self
            .runtime_allow_hosts
            .iter()
            .any(|h| h == requested_host)
        {
            NetworkDecision::Allowed
        } else {
            NetworkDecision::Denied {
                host: requested_host.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeEnforcingBackend;
    impl OsFirewallBackend for FakeEnforcingBackend {
        fn can_enforce(&self) -> bool {
            true
        }
    }

    /// ★ 이 저장소가 실제로 갖고 있는 상태를 고정한다 — 백엔드가 없다.
    ///
    /// 통과가 곧 "아직 강제 못 한다" 는 뜻이다. 백엔드가 생기면 이 테스트가
    /// 실패해야 하고, 그때 이 파일과 모듈 문서를 함께 갱신한다.
    #[test]
    fn network_without_os_backend_is_rejected() {
        let backend = NoFirewallBackend;
        let allow = vec!["pypi.internal.example".to_string()];
        let check = NetworkPolicyCheck::new(&allow, &backend);

        let decision = check.decide("pypi.internal.example");
        assert_eq!(
            decision,
            NetworkDecision::NoEnforcementBackend,
            "★ 이 테스트가 실패했다면 OS 방화벽 백엔드가 생겼다는 뜻이다 — \
             모듈 문서와 이 주석을 갱신하라"
        );
    }

    /// 비공허성 — 백엔드가 있다고 가정하면 허용 목록이 실제로 갈린다.
    #[test]
    fn with_enforcing_backend_allow_list_actually_filters() {
        let backend = FakeEnforcingBackend;
        let allow = vec!["pypi.internal.example".to_string()];
        let check = NetworkPolicyCheck::new(&allow, &backend);

        assert_eq!(
            check.decide("pypi.internal.example"),
            NetworkDecision::Allowed
        );
        assert_eq!(
            check.decide("evil.example"),
            NetworkDecision::Denied {
                host: "evil.example".to_string()
            }
        );
    }
}
