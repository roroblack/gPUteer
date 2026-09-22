//! 저장소의 Attempt 상태가 **규범 정본에서 갈라지지 않게** 한다.
//!
//! # 이 파일의 역사 (읽고 나서 고쳐라)
//!
//! **2026-09-07 — 같은 이름의 타입이 두 곳에 있었다.**
//!
//! ```text
//! gputeer_protocol::attempt_state::AttemptState     11개 상태 · transition() 있음
//! gputeer_coordinator::staging_store::AttemptState   1개(Created) · 설명 없음
//! ```
//!
//! 위험한 것은 개수 차이가 아니라 **저장소 쪽에 상태를 추가하면 규범 표를
//! 거치지 않고 생긴다**는 점이었다. 그래서 이 파일이 "저장소 enum 이 늘면
//! 컴파일이 깨지게" 만들어 두었다.
//!
//! **2026-09-22 (§A1 4c) — 둘을 하나로 합쳤다.** 저장소가 규범 타입을 그대로
//! 재수출한다(`pub use`). 갈라질 여지 자체가 사라졌으므로, 이 파일이 지키는
//! 대상도 바뀐다:
//!
//! ```text
//! 옛 역할   저장소 enum ⊆ 규범 enum 인가 (두 타입이 있을 때의 방어)
//! 새 역할   규범의 **모든** 상태가 SQLite 문자열로 **왕복**되는가
//!           -> 그래야 종료 상태를 쓴 직후 다시 읽을 수 있다.
//!              4c 이전의 저장소는 'CREATED' 아닌 값을 전부 손상으로 거부했다
//! ```
//!
//! ★ 왜 왕복이 중요한가 — 디스크에 남는 값이다. 쓰기만 되고 읽기가 안 되면
//!   다음 재시작에서 그 Attempt 는 **손상으로 보인다.**

use gputeer_coordinator::staging_store::{state_from_db, state_to_db, AttemptState};
use gputeer_protocol::attempt_state::{AttemptState as NormativeAttemptState, ALL_ATTEMPT_STATES};

/// 저장소 타입과 규범 타입이 **같은 타입**인지 컴파일 시점에 고정한다.
///
/// ★ 누가 저장소에 별도 enum 을 다시 만들면 이 함수가 컴파일되지 않는다 —
///   그것이 2026-09-07 에 막으려던 바로 그 사고다.
fn _same_type(state: AttemptState) -> NormativeAttemptState {
    state
}

#[test]
fn every_normative_state_round_trips_through_the_database_string() {
    for state in ALL_ATTEMPT_STATES {
        let raw = state_to_db(*state);
        let back = state_from_db(raw).unwrap_or_else(|error| {
            panic!("{state:?} 를 {raw:?} 로 적었는데 다시 못 읽는다 — 쓰기만 되고 읽기가 안 된다: {error}")
        });
        assert_eq!(
            back, *state,
            "{raw:?} 를 읽었더니 다른 상태가 나왔다 — 디스크 값과 코드가 어긋난다"
        );
    }
}

#[test]
fn the_database_strings_are_all_distinct() {
    // ★ 두 상태가 같은 문자열을 쓰면 왕복 시험은 통과하면서도 하나가 다른
    //   하나로 읽힌다. 개수로 직접 확인한다.
    let mut seen: Vec<&str> = ALL_ATTEMPT_STATES.iter().map(|s| state_to_db(*s)).collect();
    seen.sort_unstable();
    let before = seen.len();
    seen.dedup();
    assert_eq!(
        before,
        seen.len(),
        "두 상태가 같은 DB 문자열을 쓴다 — 하나가 다른 하나로 읽힌다"
    );
}

#[test]
fn an_unknown_database_string_is_refused_instead_of_defaulting() {
    // ★ 조용히 Created 로 읽으면, 손상된 행이 "이제 막 만들어진 시도" 로
    //   되살아난다. 그건 끝난 작업을 다시 돌리는 길이다.
    let error = state_from_db("NOT_A_STATE").expect_err("모르는 값이 통과했다");
    let message = error.to_string();
    assert!(
        message.contains("NOT_A_STATE"),
        "거부는 했는데 어떤 값이 문제인지 안 알려준다: {message}"
    );
}
