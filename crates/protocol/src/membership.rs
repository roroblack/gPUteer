//! 권위가 확정한 멤버십 사실에서 **현재 권한**을 판정하는 순수 kernel.
//!
//! `docs/protocol/state-machines.md` §5.1 이 정한 Member 상태기계를 읽기만
//! 한다. 상태를 바꾸지 않고, action 을 검증하지 않으며, 누가 서명할 자격이
//! 있는지도 판단하지 않는다 — 그것들은 전부 이 kernel 밖이다.
//!
//! # 이 kernel 이 닫는 것
//!
//! `ADR-031` 이 "이음매" 로 남긴 셋은 전부 같은 질문이었다 —
//! **"이 사실을 채울 자격이 누구에게 있는가."**
//!
//! ```text
//! 이음매 4  HolderValidation (crates/checkpoint/src/durability.rs)
//! 이음매 5  ProvenanceGate   (crates/scheduler/src/scope.rs)
//! 이음매 6  durable load 의 재검증 경로 (crates/coordinator)
//! ```
//!
//! 셋 다 값을 **caller 가 넣는 입력**이고, 지금까지 그 값을 프로덕션에서
//! 채우는 코드가 하나도 없었다(테스트에서만 만들어졌다). 이 kernel 이 그
//! 값을 계산한다. 다만 **매핑은 caller 가 한다** — 이 모듈은 `checkpoint`
//! 나 `scheduler` 의 타입을 알지 않는다.
//!
//! # 순수성
//!
//! 시계·I/O·DB·network·난수·전역 상태를 읽지 않는다. 같은 입력은 입력
//! 순서와 무관하게 항상 같은 결과를 낸다.
//!
//! # 규범 근거
//!
//! ```text
//! ACTIVE 만 현재 권한이다            state-machines.md §5.1
//! 과거 기록은 남기되 권한은 아니다     같은 절 "현재 권한 판정에 쓸 수 있는 상태"
//! 현재 권한 판정은 항상 최신 상태로     같은 절 "조회 일관성"
//! tombstone 된 ID 재사용 금지         같은 절 "항상 금지"
//! ```

use std::collections::BTreeMap;

/// `state-machines.md` §5.1 의 Member 상태.
///
/// 이 enum 에 `Default` 를 두지 않는다. 기본값이 있으면 "모르는 상태" 가
/// 조용히 어떤 값으로 흘러들어간다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberState {
    Pending,
    Active,
    Suspended,
    Revoked,
    Removed,
}

impl MemberState {
    /// 이 상태가 **현재 권한**인가.
    ///
    /// `state-machines.md` §5.1 — `ACTIVE` 하나만 현재 권한이다. 나머지는
    /// "그때 그랬다" 는 사실로만 남고 "지금 유효하다" 로 승격되지 않는다.
    pub fn is_current_authority(self) -> bool {
        matches!(self, MemberState::Active)
    }
}

/// 권위 view 의 신선도. **caller 가 채운다.**
///
/// `DoD-55` 의 `ProvenanceGate` 와 같은 성격이다 — kernel 이 만들어내는
/// 값이 아니라, 무엇이 이 값을 채울 자격이 있는지를 caller 가 책임진다.
///
/// `state-machines.md` §5.1 "조회 일관성" 이 정한 것을 타입으로 강제한다.
/// 현재 권한 판정에는 `Linearizable` 만 쓸 수 있고, `BoundedStale` 로
/// 판정을 시도하면 통과가 아니라 **typed error** 다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViewFreshness {
    /// 이 revision 시점의 확정된 view 다.
    Linearizable { revision: u64 },
    /// 오래됐을 수 있는 view. 표시·진단 전용이며 권한 판정에 쓸 수 없다.
    BoundedStale,
}

/// device 가 어느 member 의 어느 세대에 묶여 있는가.
///
/// `generation` 은 tombstone 된 ID 재사용을 막는다. 같은 `member_id` 라도
/// 세대가 다르면 **다른 주체**다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeviceBinding {
    pub device_id: String,
    pub member_id: String,
    pub generation: u64,
}

/// 권위가 확정한 log 를 투영한 member 사실 하나.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemberFact {
    pub member_id: String,
    pub generation: u64,
    pub state: MemberState,
}

/// 판정 결과.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemberAuthorization {
    /// 현재 권한이 있다.
    Authorized { member_id: String, generation: u64 },
    /// 주체는 특정됐으나 현재 권한이 없다.
    NotAuthorized {
        member_id: String,
        generation: u64,
        state: MemberState,
    },
    /// 이 device 에 대한 사실이 없다. **권한 없음과 구분한다** —
    /// "없다" 와 "있는데 권한이 아니다" 는 다른 신호다.
    Unresolved,
    /// 서로 충돌하는 사실이 있다. fail closed.
    Ambiguous,
}

/// 입력 자체가 잘못된 경우. 판정 결과가 아니라 오류다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipResolutionError {
    /// 권한 판정에 오래된 view 를 쓰려 했다.
    ///
    /// `state-machines.md` §5.1 "조회 일관성" — 조금 오래된 정보로
    /// 판정하면 방금 쫓아낸 멤버가 잠깐 유효해 보이는 구멍이 생긴다.
    StaleViewNotAllowedForAuthorization,
    /// 조회 대상 device ID 가 비었다.
    BlankQueryDeviceId,
    /// binding 의 어떤 ID 가 비었다.
    BlankBindingId { index: usize },
    /// member 사실의 ID 가 비었다.
    BlankMemberFactId { index: usize },
    /// 같은 device 에 대해 완전히 같은 binding 이 두 번 이상 들어왔다.
    ///
    /// 충돌(`Ambiguous`)과 구분한다 — 이건 caller 가 중복을 제거하지
    /// 않은 malformed 입력이다.
    DuplicateDeviceBinding { device_id: String },
    /// 같은 `(member_id, generation)` 에 대해 완전히 같은 사실이 두 번
    /// 이상 들어왔다.
    DuplicateMemberFact { member_id: String, generation: u64 },
}

/// device 하나의 현재 권한을 판정한다.
///
/// # 판정 순서
///
/// ```text
/// 1  view 가 Linearizable 인가          아니면 오류
/// 2  입력이 malformed 인가              맞으면 오류
/// 3  이 device 의 binding 이 몇 개인가   0 -> Unresolved, 충돌 -> Ambiguous
/// 4  그 (member, generation) 의 사실은   0 -> Unresolved, 충돌 -> Ambiguous
/// 5  그 상태가 ACTIVE 인가               맞으면 Authorized, 아니면 NotAuthorized
/// ```
///
/// # 이 kernel 이 하지 않는 것
///
/// - 서명을 검증하지 않는다. 입력은 이미 검증된 사실이어야 한다.
/// - 누가 그 사실을 만들 자격이 있는지 판단하지 않는다(모드마다 다르다 —
///   사설 팀은 방장, 공개 풀은 Broker. `state-machines.md` §5.1).
/// - 상태를 전이시키지 않는다.
/// - 신선도를 스스로 판단하지 않는다. `ViewFreshness` 는 caller 입력이다.
pub fn resolve_device_authorization(
    device_id: &str,
    freshness: ViewFreshness,
    bindings: &[DeviceBinding],
    members: &[MemberFact],
) -> Result<MemberAuthorization, MembershipResolutionError> {
    // 1) 오래된 view 로는 권한을 판정하지 않는다. 판정을 시도한 것 자체가
    //    오류다 — 조용히 `Unresolved` 로 내려보내면 caller 가 "사실이
    //    없다" 로 오해한다.
    let ViewFreshness::Linearizable { .. } = freshness else {
        return Err(MembershipResolutionError::StaleViewNotAllowedForAuthorization);
    };

    if device_id.trim().is_empty() {
        return Err(MembershipResolutionError::BlankQueryDeviceId);
    }

    validate_bindings(bindings)?;
    validate_members(members)?;

    // 3) 이 device 의 binding 을 모은다. 입력 순서와 무관해야 하므로
    //    정렬된 집합으로 중복을 제거한 뒤 개수를 센다.
    let matched: Vec<&DeviceBinding> = bindings
        .iter()
        .filter(|binding| binding.device_id == device_id)
        .collect();

    let subjects: std::collections::BTreeSet<(&str, u64)> = matched
        .iter()
        .map(|binding| (binding.member_id.as_str(), binding.generation))
        .collect();

    let (member_id, generation) = match subjects.len() {
        0 => return Ok(MemberAuthorization::Unresolved),
        1 => {
            let (member_id, generation) = subjects
                .into_iter()
                .next()
                .expect("len==1 이므로 원소가 있다");
            (member_id.to_owned(), generation)
        }
        // 같은 device 가 서로 다른 주체에 묶여 있다. 어느 쪽이 맞는지
        // 이 kernel 은 알 수 없다 — fail closed.
        _ => return Ok(MemberAuthorization::Ambiguous),
    };

    // 4) 그 (member_id, generation) 의 사실을 찾는다. 세대가 다르면 애초에
    //    다른 주체이므로 여기서 자연히 걸러진다(tombstone ID 재사용 금지).
    let states: std::collections::BTreeSet<MemberState> = members
        .iter()
        .filter(|fact| fact.member_id == member_id && fact.generation == generation)
        .map(|fact| fact.state)
        .collect();

    match states.len() {
        0 => Ok(MemberAuthorization::Unresolved),
        1 => {
            let state = states
                .into_iter()
                .next()
                .expect("len==1 이므로 원소가 있다");
            if state.is_current_authority() {
                Ok(MemberAuthorization::Authorized {
                    member_id,
                    generation,
                })
            } else {
                Ok(MemberAuthorization::NotAuthorized {
                    member_id,
                    generation,
                    state,
                })
            }
        }
        // 같은 주체가 한 revision 에서 두 상태를 동시에 갖는다. fail closed.
        _ => Ok(MemberAuthorization::Ambiguous),
    }
}

fn validate_bindings(bindings: &[DeviceBinding]) -> Result<(), MembershipResolutionError> {
    let mut seen: BTreeMap<(&str, &str, u64), ()> = BTreeMap::new();
    for (index, binding) in bindings.iter().enumerate() {
        if binding.device_id.trim().is_empty() || binding.member_id.trim().is_empty() {
            return Err(MembershipResolutionError::BlankBindingId { index });
        }
        let key = (
            binding.device_id.as_str(),
            binding.member_id.as_str(),
            binding.generation,
        );
        if seen.insert(key, ()).is_some() {
            return Err(MembershipResolutionError::DuplicateDeviceBinding {
                device_id: binding.device_id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_members(members: &[MemberFact]) -> Result<(), MembershipResolutionError> {
    let mut seen: BTreeMap<(&str, u64, MemberState), ()> = BTreeMap::new();
    for (index, fact) in members.iter().enumerate() {
        if fact.member_id.trim().is_empty() {
            return Err(MembershipResolutionError::BlankMemberFactId { index });
        }
        let key = (fact.member_id.as_str(), fact.generation, fact.state);
        if seen.insert(key, ()).is_some() {
            return Err(MembershipResolutionError::DuplicateMemberFact {
                member_id: fact.member_id.clone(),
                generation: fact.generation,
            });
        }
    }
    Ok(())
}
