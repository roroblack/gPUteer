//! `crates/protocol/src/membership.rs` 순수 kernel 의 계약 테스트.
//!
//! 규범 근거는 `docs/protocol/state-machines.md` §5.1 이다.

use gputeer_protocol::membership::{
    resolve_device_authorization, DeviceBinding, MemberAuthorization, MemberFact, MemberState,
    MembershipResolutionError, ViewFreshness,
};

const FRESH: ViewFreshness = ViewFreshness::Linearizable { revision: 42 };

fn binding(device: &str, member: &str, generation: u64) -> DeviceBinding {
    DeviceBinding {
        device_id: device.to_owned(),
        member_id: member.to_owned(),
        generation,
    }
}

fn fact(member: &str, generation: u64, state: MemberState) -> MemberFact {
    MemberFact {
        member_id: member.to_owned(),
        generation,
        state,
    }
}

/// 모든 순열에서 결과가 같은지 확인한다. 입력 순서가 판정을 바꾸면 안 된다.
fn assert_order_independent(
    device: &str,
    bindings: &[DeviceBinding],
    members: &[MemberFact],
    expected: &MemberAuthorization,
) {
    for binding_order in permutations(bindings) {
        for member_order in permutations(members) {
            let actual = resolve_device_authorization(device, FRESH, &binding_order, &member_order)
                .expect("유효한 입력이다");
            assert_eq!(
                &actual, expected,
                "입력 순서가 결과를 바꿨다: bindings={binding_order:?} members={member_order:?}"
            );
        }
    }
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.is_empty() {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for index in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(index);
        for mut tail in permutations(&rest) {
            let mut one = vec![head.clone()];
            one.append(&mut tail);
            out.push(one);
        }
    }
    out
}

// ── 규범: ACTIVE 하나만 현재 권한이다 ──────────────────────────────

#[test]
fn active_member_is_authorized() {
    let bindings = [binding("dev-a", "mem-1", 1)];
    let members = [fact("mem-1", 1, MemberState::Active)];
    assert_order_independent(
        "dev-a",
        &bindings,
        &members,
        &MemberAuthorization::Authorized {
            member_id: "mem-1".to_owned(),
            generation: 1,
        },
    );
}

/// `state-machines.md` §5.1 — PENDING·SUSPENDED·REVOKED·REMOVED 는 과거
/// 기록으로 남지만 현재 권한이 아니다. 넷 다 개별로 고정한다.
#[test]
fn only_active_state_is_current_authority() {
    for state in [
        MemberState::Pending,
        MemberState::Suspended,
        MemberState::Revoked,
        MemberState::Removed,
    ] {
        let bindings = [binding("dev-a", "mem-1", 1)];
        let members = [fact("mem-1", 1, state)];
        let actual = resolve_device_authorization("dev-a", FRESH, &bindings, &members)
            .expect("유효한 입력이다");
        assert_eq!(
            actual,
            MemberAuthorization::NotAuthorized {
                member_id: "mem-1".to_owned(),
                generation: 1,
                state,
            },
            "{state:?} 가 현재 권한으로 취급됐다"
        );
        assert!(!state.is_current_authority(), "{state:?}");
    }
    assert!(MemberState::Active.is_current_authority());
}

// ── 규범: 조회 일관성 — 오래된 view 로는 권한을 판정하지 않는다 ──────

#[test]
fn stale_view_is_refused_not_silently_downgraded() {
    let bindings = [binding("dev-a", "mem-1", 1)];
    let members = [fact("mem-1", 1, MemberState::Active)];
    let actual =
        resolve_device_authorization("dev-a", ViewFreshness::BoundedStale, &bindings, &members);
    assert_eq!(
        actual,
        Err(MembershipResolutionError::StaleViewNotAllowedForAuthorization),
        "오래된 view 가 권한 판정에 통과했다"
    );
}

/// 오래된 view 는 `Unresolved` 로 조용히 내려가면 안 된다 — caller 가
/// "사실이 없다" 로 오해한다. 위 테스트가 오류임을 확인하지만, 그 오류가
/// 판정 결과와 **다른 종류**임을 여기서 못박는다.
#[test]
fn stale_view_error_is_not_an_authorization_outcome() {
    let actual = resolve_device_authorization("dev-a", ViewFreshness::BoundedStale, &[], &[]);
    assert!(actual.is_err(), "빈 입력이어도 신선도가 먼저 걸러져야 한다");
}

// ── 규범: "사실 없음" 과 "권한 없음" 을 구분한다 ────────────────────

#[test]
fn unknown_device_is_unresolved_not_denied() {
    let bindings = [binding("dev-a", "mem-1", 1)];
    let members = [fact("mem-1", 1, MemberState::Active)];
    let actual = resolve_device_authorization("dev-zzz", FRESH, &bindings, &members)
        .expect("유효한 입력이다");
    assert_eq!(actual, MemberAuthorization::Unresolved);
}

#[test]
fn binding_without_member_fact_is_unresolved() {
    let bindings = [binding("dev-a", "mem-1", 1)];
    let actual =
        resolve_device_authorization("dev-a", FRESH, &bindings, &[]).expect("유효한 입력이다");
    assert_eq!(actual, MemberAuthorization::Unresolved);
}

// ── 규범: tombstone 된 ID 재사용 금지 — 세대가 다르면 다른 주체다 ────

#[test]
fn generation_mismatch_is_unresolved_not_authorized() {
    // 옛 세대 1 에 묶인 device 인데, 권위에는 세대 2 의 ACTIVE 사실만 있다.
    // 같은 member_id 라도 세대가 다르면 다른 주체이므로 권한이 아니다.
    let bindings = [binding("dev-a", "mem-1", 1)];
    let members = [fact("mem-1", 2, MemberState::Active)];
    let actual =
        resolve_device_authorization("dev-a", FRESH, &bindings, &members).expect("유효한 입력이다");
    assert_eq!(
        actual,
        MemberAuthorization::Unresolved,
        "세대가 다른 ACTIVE 사실이 권한으로 승격됐다"
    );
}

#[test]
fn revoked_old_generation_does_not_block_new_generation() {
    // 반대 방향도 고정한다 — 옛 세대가 REVOKED 라도 새 세대 binding 은
    // 자기 세대의 사실로 판정된다.
    let bindings = [binding("dev-a", "mem-1", 2)];
    let members = [
        fact("mem-1", 1, MemberState::Revoked),
        fact("mem-1", 2, MemberState::Active),
    ];
    assert_order_independent(
        "dev-a",
        &bindings,
        &members,
        &MemberAuthorization::Authorized {
            member_id: "mem-1".to_owned(),
            generation: 2,
        },
    );
}

// ── 규범: 충돌은 fail closed ────────────────────────────────────────

#[test]
fn device_bound_to_two_subjects_is_ambiguous() {
    let bindings = [binding("dev-a", "mem-1", 1), binding("dev-a", "mem-2", 1)];
    let members = [
        fact("mem-1", 1, MemberState::Active),
        fact("mem-2", 1, MemberState::Active),
    ];
    assert_order_independent(
        "dev-a",
        &bindings,
        &members,
        &MemberAuthorization::Ambiguous,
    );
}

#[test]
fn same_subject_in_two_states_is_ambiguous() {
    let bindings = [binding("dev-a", "mem-1", 1)];
    let members = [
        fact("mem-1", 1, MemberState::Active),
        fact("mem-1", 1, MemberState::Revoked),
    ];
    assert_order_independent(
        "dev-a",
        &bindings,
        &members,
        &MemberAuthorization::Ambiguous,
    );
}

/// 충돌이 ACTIVE 를 이긴다 — 하나라도 권한이 있으면 통과시키는 식으로
/// 완화되지 않는다.
#[test]
fn ambiguity_is_not_resolved_in_favor_of_active() {
    let bindings = [binding("dev-a", "mem-1", 1), binding("dev-a", "mem-2", 1)];
    let members = [
        fact("mem-1", 1, MemberState::Active),
        fact("mem-2", 1, MemberState::Revoked),
    ];
    let actual =
        resolve_device_authorization("dev-a", FRESH, &bindings, &members).expect("유효한 입력이다");
    assert_eq!(
        actual,
        MemberAuthorization::Ambiguous,
        "충돌이 ACTIVE 쪽으로 해소됐다"
    );
}

// ── malformed 입력은 판정이 아니라 오류다 ───────────────────────────

#[test]
fn blank_ids_are_rejected() {
    let members = [fact("mem-1", 1, MemberState::Active)];

    assert_eq!(
        resolve_device_authorization("", FRESH, &[], &members),
        Err(MembershipResolutionError::BlankQueryDeviceId)
    );
    assert_eq!(
        resolve_device_authorization("   ", FRESH, &[], &members),
        Err(MembershipResolutionError::BlankQueryDeviceId),
        "공백만 있는 ID 가 통과했다"
    );
    assert_eq!(
        resolve_device_authorization("dev-a", FRESH, &[binding("", "mem-1", 1)], &members),
        Err(MembershipResolutionError::BlankBindingId { index: 0 })
    );
    assert_eq!(
        resolve_device_authorization("dev-a", FRESH, &[binding("dev-a", "  ", 1)], &members),
        Err(MembershipResolutionError::BlankBindingId { index: 0 })
    );
    assert_eq!(
        resolve_device_authorization(
            "dev-a",
            FRESH,
            &[binding("dev-a", "mem-1", 1)],
            &[fact("", 1, MemberState::Active)]
        ),
        Err(MembershipResolutionError::BlankMemberFactId { index: 0 })
    );
}

/// 완전히 같은 입력이 두 번 들어온 것은 **충돌이 아니라 malformed** 다.
/// 둘을 구분하지 않으면 caller 의 중복 제거 실수가 `Ambiguous` 로 뭉개져
/// 진짜 충돌과 섞인다.
#[test]
fn exact_duplicate_input_is_an_error_not_ambiguous() {
    let bindings = [binding("dev-a", "mem-1", 1), binding("dev-a", "mem-1", 1)];
    let members = [fact("mem-1", 1, MemberState::Active)];
    assert_eq!(
        resolve_device_authorization("dev-a", FRESH, &bindings, &members),
        Err(MembershipResolutionError::DuplicateDeviceBinding {
            device_id: "dev-a".to_owned()
        })
    );

    let bindings = [binding("dev-a", "mem-1", 1)];
    let members = [
        fact("mem-1", 1, MemberState::Active),
        fact("mem-1", 1, MemberState::Active),
    ];
    assert_eq!(
        resolve_device_authorization("dev-a", FRESH, &bindings, &members),
        Err(MembershipResolutionError::DuplicateMemberFact {
            member_id: "mem-1".to_owned(),
            generation: 1
        })
    );
}

/// 다른 device 의 사실이 판정에 새어 들어오지 않는다.
#[test]
fn other_devices_do_not_leak_into_the_verdict() {
    let bindings = [
        binding("dev-a", "mem-1", 1),
        binding("dev-b", "mem-2", 1),
        binding("dev-c", "mem-3", 7),
    ];
    let members = [
        fact("mem-1", 1, MemberState::Suspended),
        fact("mem-2", 1, MemberState::Active),
        fact("mem-3", 7, MemberState::Active),
    ];
    assert_order_independent(
        "dev-a",
        &bindings,
        &members,
        &MemberAuthorization::NotAuthorized {
            member_id: "mem-1".to_owned(),
            generation: 1,
            state: MemberState::Suspended,
        },
    );
}

/// 이 kernel 은 시계·I/O 를 읽지 않으므로, 같은 입력을 여러 번 불러도
/// 결과가 변하지 않는다.
#[test]
fn repeated_calls_are_identical() {
    let bindings = [binding("dev-a", "mem-1", 3)];
    let members = [fact("mem-1", 3, MemberState::Active)];
    let first = resolve_device_authorization("dev-a", FRESH, &bindings, &members);
    for _ in 0..8 {
        assert_eq!(
            resolve_device_authorization("dev-a", FRESH, &bindings, &members),
            first
        );
    }
}

/// revision 값 자체는 판정을 바꾸지 않는다 — kernel 은 revision 을
/// 해석하지 않고 "확정된 view 인가" 만 본다. 해석은 caller 몫이다.
#[test]
fn revision_value_does_not_change_the_verdict() {
    let bindings = [binding("dev-a", "mem-1", 1)];
    let members = [fact("mem-1", 1, MemberState::Active)];
    let expected = MemberAuthorization::Authorized {
        member_id: "mem-1".to_owned(),
        generation: 1,
    };
    for revision in [0, 1, 999, u64::MAX] {
        assert_eq!(
            resolve_device_authorization(
                "dev-a",
                ViewFreshness::Linearizable { revision },
                &bindings,
                &members
            )
            .expect("유효한 입력이다"),
            expected
        );
    }
}
