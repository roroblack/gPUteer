//! 저장소의 `AttemptState` 가 **규범 정본에서 조용히 갈라지지 않게** 한다.
//!
//! # 왜 이 파일이 생겼나 (2026-09-07)
//!
//! 같은 이름의 타입이 **두 곳에** 있다.
//!
//! ```text
//! gputeer_protocol::attempt_state::AttemptState   11개 상태 · transition() 있음
//!                                                 state_table_parity.rs 가
//!                                                 규범 표와 양방향 대조한다
//!
//! gputeer_coordinator::staging_store::AttemptState  1개(Created) · 설명 없음
//! ```
//!
//! ★★ **위험한 것은 개수 차이가 아니라 "설명이 없다" 는 점이었다.**
//!   저장소 쪽 enum 에 누가 `Running` 을 추가하면, 그 값은 규범 표를
//!   거치지 않고 생긴다. 그러면 `state-machines.md` 가 강제한다고 믿는
//!   전이 규칙 **밖에서** 상태가 늘어난다.
//!
//!   이 저장소는 같은 부류의 사고를 이미 겪었다 — "막는 문 하나와 안
//!   막는 문 하나" 가 있으면, 막힌다고 믿으면서 안 막힌 쪽으로 들어간다.
//!
//! # 이 테스트가 하는 것 / 하지 않는 것
//!
//! ```text
//! 한다      저장소의 모든 상태가 규범 정본에 **이름으로 대응**되는지
//!           저장소가 상태를 늘리면 컴파일이 깨지게 만드는지
//!
//! 안 한다   두 enum 을 하나로 합치는 것.
//!           저장소 쪽은 SQLite 에 적히는 값이라 합치려면 마이그레이션이
//!           필요하고, 그건 이 테스트의 범위가 아니다.
//! ```

use gputeer_coordinator::staging_store::AttemptState as StoredAttemptState;
use gputeer_protocol::attempt_state::AttemptState as NormativeAttemptState;

/// 저장소 상태 → 규범 정본 상태.
///
/// ★★ **`match` 를 exhaustive 로 둔다.** 저장소 enum 에 변형이 추가되면
///   이 함수가 **컴파일되지 않는다.** 그것이 이 파일의 핵심 장치다 —
///   추가하는 사람이 "이 새 상태는 규범 표의 어느 것인가" 를 반드시
///   답하게 만든다.
///
///   `_ => ...` 를 절대 쓰지 마라. 쓰는 순간 이 검사가 사라진다.
fn to_normative(stored: StoredAttemptState) -> NormativeAttemptState {
    match stored {
        StoredAttemptState::Created => NormativeAttemptState::Created,
    }
}

/// 저장소가 오늘 아는 모든 상태.
///
/// ★ 손으로 적는다. 저장소 enum 이 늘면 위 `to_normative` 가 먼저
///   컴파일 오류를 내므로, 그때 여기도 같이 늘리게 된다.
const STORED_STATES: &[StoredAttemptState] = &[StoredAttemptState::Created];

#[test]
fn every_stored_state_maps_to_a_normative_state() {
    for stored in STORED_STATES {
        let normative = to_normative(*stored);
        // 이름으로 대응되는지까지 본다 — 값만 맞고 이름이 다르면
        // 나중에 읽는 사람이 두 개념을 헷갈린다.
        assert_eq!(
            format!("{stored:?}"),
            format!("{normative:?}"),
            "저장소 상태 {stored:?} 가 규범의 다른 이름에 대응된다 — \
             이름이 다르면 두 문서가 같은 것을 말하는지 알 수 없다"
        );
    }
}

#[test]
fn the_stored_enum_is_a_strict_subset_of_the_normative_one() {
    // ★ 저장소가 규범보다 **많아지면** 그것은 규범 밖 상태다.
    //   `state-machines.md` 표를 안 거치고 생긴 상태가 있다는 뜻이다.
    let normative_count = gputeer_protocol::attempt_state::ALL_ATTEMPT_STATES.len();
    assert!(
        STORED_STATES.len() <= normative_count,
        "저장소 상태({})가 규범 정본({})보다 많다 — 규범 표를 거치지 않은 \
         상태가 생겼다는 뜻이다",
        STORED_STATES.len(),
        normative_count
    );
}

#[test]
fn the_normative_enum_still_has_the_states_this_crate_expects() {
    // ★★ **반대 방향 대조군.** 위 둘만 있으면, 규범 쪽에서 `Created` 를
    //   지워도 이 테스트들이 통과한다(저장소가 0개가 되면 되니까).
    //   규범이 이 crate 가 기대하는 상태를 계속 갖고 있는지 본다.
    assert!(
        gputeer_protocol::attempt_state::ALL_ATTEMPT_STATES
            .contains(&NormativeAttemptState::Created),
        "규범 정본에서 Created 가 사라졌다 — 저장소가 쓰는 상태다"
    );
    // 오늘 저장소가 아는 것은 하나뿐이다. 그 사실 자체를 고정해,
    // 늘어날 때 이 파일을 반드시 다시 보게 만든다.
    assert_eq!(
        STORED_STATES.len(),
        1,
        "저장소 상태가 늘었다 — to_normative() 와 이 파일 머리말을 함께 갱신하라"
    );
}
