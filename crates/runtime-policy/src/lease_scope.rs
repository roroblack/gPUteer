//! `Lease.scope`(fence_epoch) 강제 — `CLAUDE.md` §0.4 가 이미 원칙을 정했다.
//!
//! > 외부 API 호출은 fencing 으로 막을 수 없다. 상대가 `fence_epoch` 를
//! > 모른다. `side_effecting` Job 의 중복 실행을 "막는다"고 쓰지 않는다.
//! > 억제할 뿐이다.
//!
//! # 강제 가능성
//!
//! ```text
//! gPUteer 가 소유한 자원        Enforceable — CAS 경로 · canonical pointer ·
//! (write-once, 우리가 게이트)   Hub 는 우리 코드가 쓰기 경로를 갖고 있다.
//!                              쓰기 전에 epoch 를 대조해 거부할 수 있다.
//!
//! 외부 HTTP/API · DB           Suppressible (사실상 Unenforceable 에 가깝다).
//!                              상대가 fence_epoch 개념을 모른다.
//!                              우리가 "보내지 않는다" 로 억제할 뿐,
//!                              이미 보낸 요청은 막을 수 없다.
//! ```
//!
//! 이 모듈은 **우리가 소유한 자원**에 대한 watermark 판정만 한다.
//! 외부 API 판정은 [`FenceWatermark::classify_external`] 이 항상
//! `Suppressible` 을 반환해 그 한계를 코드로 드러낸다.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseScopeViolation {
    /// 들어온 epoch 가 이미 기록된 watermark 보다 낮다 — stale lease.
    StaleEpoch { resource: String, incoming: u64, watermark: u64 },
}

impl std::fmt::Display for LeaseScopeViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleEpoch {
                resource,
                incoming,
                watermark,
            } => write!(
                f,
                "{resource}: fence_epoch {incoming} 은 기록된 watermark {watermark} 보다 낮다 — stale lease"
            ),
        }
    }
}

impl std::error::Error for LeaseScopeViolation {}

/// gPUteer 가 소유한 자원(CAS 경로 · canonical pointer 등)의 fence_epoch
/// watermark. **영속화되지 않는다** — 이 구조체 자체는 메모리 상태다.
/// 영속 저장은 CAS/Hub 가 생기면 그쪽 책임이다.
///
/// ★ 이름이 "영속" 이 아니라 "메모리" 임을 감추지 않는다. 재시작하면
///   watermark 를 잃고, 그 순간부터 재시작 전 epoch 보다 낮은 lease 도
///   통과할 수 있다 — replay guard 와 같은 처지의 결함이다.
///   `is_durable()` 관례를 여기도 따른다.
#[derive(Default)]
pub struct FenceWatermark {
    seen: HashMap<String, u64>,
}

impl FenceWatermark {
    pub fn new() -> Self {
        Self::default()
    }

    /// **이 계층은 영속되지 않는다.**
    pub fn is_durable(&self) -> bool {
        false
    }

    /// `resource` 에 대해 들어온 `epoch` 가 유효한가.
    ///
    /// 통과하면 watermark 를 그 값으로 올린다 — 더 높은 epoch 가 낮은
    /// epoch 의 재사용을 막는다. **단조 증가만 허용**한다.
    pub fn check_and_advance(
        &mut self,
        resource: &str,
        epoch: u64,
    ) -> Result<(), LeaseScopeViolation> {
        let watermark = self.seen.get(resource).copied().unwrap_or(0);
        if epoch < watermark {
            return Err(LeaseScopeViolation::StaleEpoch {
                resource: resource.to_string(),
                incoming: epoch,
                watermark,
            });
        }
        self.seen.insert(resource.to_string(), epoch);
        Ok(())
    }

    /// 외부 자원(HTTP API 등)에 대한 fencing 시도.
    ///
    /// ★ 이 함수는 **항상 억제 등급을 반환한다.** 강제할 방법이 없다는
    ///   사실을 함수 서명으로 표현한다 — 실수로 이 값을 "강제됨" 처럼
    ///   쓰는 코드를 컴파일 단계에서 만들지 않는다(반환 타입 자체가
    ///   "억제만" 이라는 뜻이다).
    pub fn classify_external(&self, _resource_url: &str) -> ExternalFencingClass {
        ExternalFencingClass::SuppressedNotEnforced
    }
}

/// 외부 자원에 대한 fencing 은 **이것 하나뿐이다.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalFencingClass {
    /// 우리가 그 요청을 "보내지 않는" 방식으로 억제할 뿐, 상대는 epoch 를
    /// 모른다. 이미 발송된 요청은 막을 수 없다.
    SuppressedNotEnforced,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_lease_is_rejected() {
        let mut w = FenceWatermark::new();
        w.check_and_advance("cas://jobs/1", 5).unwrap();

        let r = w.check_and_advance("cas://jobs/1", 3);
        assert!(
            matches!(r, Err(LeaseScopeViolation::StaleEpoch { incoming: 3, watermark: 5, .. })),
            "{r:?}"
        );
    }

    /// 비공허성 — 더 높은 epoch 는 통과하고 watermark 를 올린다.
    #[test]
    fn higher_epoch_advances_watermark() {
        let mut w = FenceWatermark::new();
        w.check_and_advance("cas://jobs/1", 5).unwrap();
        w.check_and_advance("cas://jobs/1", 10).unwrap();

        let r = w.check_and_advance("cas://jobs/1", 7);
        assert!(matches!(r, Err(LeaseScopeViolation::StaleEpoch { watermark: 10, .. })));
    }

    /// 자원별로 독립적인가 — 한 자원의 epoch 가 다른 자원을 막지 않는다.
    #[test]
    fn watermark_is_per_resource() {
        let mut w = FenceWatermark::new();
        w.check_and_advance("cas://jobs/1", 100).unwrap();
        assert!(w.check_and_advance("cas://jobs/2", 1).is_ok());
    }

    /// ★ 외부 API 는 **강제할 수 없다는 것 자체를 고정한다.**
    ///
    /// 통과가 곧 "여전히 억제뿐이다" 라는 뜻이다. 반환 타입에 다른
    /// variant 가 생기면 그것이 실제 fencing 을 뜻하는지 재검토해야 한다.
    #[test]
    fn external_api_is_not_fenced_by_epoch() {
        let w = FenceWatermark::new();
        assert_eq!(
            w.classify_external("https://api.example.com/webhook"),
            ExternalFencingClass::SuppressedNotEnforced,
            "★ 외부 자원에 실제 fencing 이 생겼다면 CLAUDE.md §0.4 문구부터 갱신하라"
        );
    }

    #[test]
    fn watermark_is_not_durable() {
        assert!(!FenceWatermark::new().is_durable());
    }
}
