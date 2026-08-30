//! replay nonce 유도 — **단일 출처**.
//!
//! # 왜 여기로 옮겼는가
//!
//! ★ 2026-08-29. 같은 계산이 **세 군데**에 복사돼 있었다.
//!
//! ```text
//! crates/agent/src/lib.rs                  Agent 가 보낼 nonce
//! crates/coordinator/src/lib.rs            Coordinator 가 기대할 nonce
//! crates/cli/src/coordinator_agent_selftest.rs   selftest 가 대조할 nonce
//! ```
//!
//! 독립 검수 지적으로 이 계산을 고쳤을 때, 앞의 둘만 고치고 셋째를
//! 놓쳐 selftest 가 "nonce mismatch" 로 실패했다 — 세 군데에 흩어진
//! 계산은 언젠가 갈라진다는 것이 그 자리에서 증명됐다.
//! `CLAUDE.md` §3 이 "프로토콜 상수는 한 곳에만 둔다" 고 정한 것과
//! 같은 이유다.
//!
//! # 왜 길이 접두사가 필요한가
//!
//! ★ 초안은 `tag || 0x00 || id` 뒤에 `connection_attempt` 를 **0 이
//!   아닐 때만** 붙였다. 그래서 서로 다른 입력이 같은 바이트가 됐다.
//!
//! ```text
//! id = "L:51234", attempt = 0            -> "...\0L:51234"
//! id = "L:5",     attempt = 0x31323334   -> "...\0L:5" + "1234"
//! ```
//!
//! 두 번째의 4바이트가 ASCII `"1234"` 라 첫 번째와 완전히 같아진다.
//! 지금 bounded reconnect 범위에서는 도달하지 않지만, **해시 입력이
//! 다른 두 요청을 구분하지 못하면** 언젠가 서로 다른 두 요청이 같은
//! nonce 를 갖고 하나가 replay 로 거부된다 — 그때 원인을 찾기가 매우
//! 어렵다.
//!
//! # 이 함수는 단사가 아니다 — 그렇게 주장하지 않는다
//!
//! ★ 2026-08-30 독립 검수가 표현을 정정했다. 고친 것은 **해시 입력
//!   인코딩**이고, 그건 이제 단사다 — 서로 다른 `(tag, id, attempt)`
//!   는 서로 다른 바이트열이 된다.
//!
//!   그러나 마지막에 BLAKE3 결과를 16바이트로 자른다. 입력 공간은
//!   무한하고 출력은 2^128 개이므로 **함수 자체는 반드시 충돌한다.**
//!   정확한 표현은 이것이다.
//!
//! ```text
//! 맞다   해시 입력 인코딩이 단사다 (서로 다른 입력 -> 서로 다른 바이트)
//! 틀리다 derive_replay_nonce 가 단사다
//! 정확   128비트 충돌 저항에 의존하는 결정적 nonce
//! ```
//!
//!   실제 BLAKE3-128 충돌은 계산상 찾을 수 없고, replay guard 가
//!   기대하는 크기가 16바이트라 자르는 것 자체는 바꾸지 않는다.
//!   바꾼 것은 **무엇을 보장한다고 말하는가** 다.
//!
//! 이 저장소는 같은 교훈을 이미 배웠다. `start_checkpoint_id()` 는
//! "길이-프리픽스된 job_id/attempt_id/grant_id — canonical encoding
//! 결함 방지" 라고 주석까지 달아 뒀는데, 이 함수는 그러지 않았다.

/// nonce 를 유도한다. 모든 성분이 길이 접두사를 갖는다.
///
/// 128비트 충돌 저항에 의존한다 — 단사 함수가 아니다(모듈 문서 참조).
///
/// `connection_attempt` 는 값과 무관하게 **항상** 고정 8바이트로
/// 들어간다 — 조건부로 넣으면 있고 없고가 길이 차이를 만들어 다시
/// 모호해진다.
pub fn derive_replay_nonce(tag: &str, id: &str, connection_attempt: u32) -> Vec<u8> {
    let mut input = Vec::with_capacity(tag.len() + id.len() + 24);
    push_len_prefixed(&mut input, tag.as_bytes());
    push_len_prefixed(&mut input, id.as_bytes());
    input.extend_from_slice(&(connection_attempt as u64).to_be_bytes());
    crate::canonical::blake3_256(&input)[..16].to_vec()
}

/// 바이트열을 `길이(u64 big-endian) || 내용` 으로 넣는다.
fn push_len_prefixed(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 독립 검수가 든 실제 **인코딩** 충돌 반례가 이제 안 나오는가.
    ///
    ///   이건 "이 함수가 단사다" 를 증명하지 않는다 — 그건 128비트로
    ///   자르는 한 불가능하다. 서로 다른 입력이 같은 해시 **입력
    ///   바이트열**이 되던 결함이 사라졌는지만 본다.
    #[test]
    fn the_reported_collision_is_gone() {
        let a = derive_replay_nonce("node-heartbeat", "L:51234", 0);
        // 0x31323334 == ASCII "1234"
        let b = derive_replay_nonce("node-heartbeat", "L:5", 0x3132_3334);
        assert_ne!(a, b, "검수가 든 충돌 반례가 여전히 같은 nonce 를 만든다");
    }

    /// 성분 경계가 흐려지지 않는가.
    ///
    /// 길이 접두사가 없으면 `("ab","c")` 와 `("a","bc")` 가 같아진다.
    #[test]
    fn component_boundaries_do_not_blur() {
        assert_ne!(
            derive_replay_nonce("ab", "c", 0),
            derive_replay_nonce("a", "bc", 0)
        );
        assert_ne!(
            derive_replay_nonce("", "abc", 0),
            derive_replay_nonce("abc", "", 0)
        );
    }

    /// `connection_attempt` 가 0 이든 아니든 항상 반영되는가.
    #[test]
    fn the_attempt_number_always_matters() {
        assert_ne!(
            derive_replay_nonce("t", "id", 0),
            derive_replay_nonce("t", "id", 1)
        );
        assert_ne!(
            derive_replay_nonce("t", "id", 1),
            derive_replay_nonce("t", "id", 2)
        );
    }

    /// 같은 입력은 항상 같은 값을 낸다.
    #[test]
    fn derivation_is_deterministic() {
        assert_eq!(
            derive_replay_nonce("t", "id", 7),
            derive_replay_nonce("t", "id", 7)
        );
    }

    /// 길이가 16바이트인가 — replay guard 가 그 크기를 기대한다.
    #[test]
    fn the_nonce_is_sixteen_bytes() {
        assert_eq!(derive_replay_nonce("t", "id", 0).len(), 16);
    }
}
