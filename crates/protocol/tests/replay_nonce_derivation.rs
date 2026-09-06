//! `derive_replay_nonce` 의 **바이트 배치**를 고정한다 (`signing.md` §10).
//!
//! # 왜 이 파일이 생겼나 — 뮤테이션 감사에서 살아남았다 (2026-09-06)
//!
//! `crates/protocol/src/nonce.rs` 의 모듈 문서는 이렇게 적는다.
//!
//! > `connection_attempt` 는 값과 무관하게 **항상** 고정 8바이트로 들어간다 —
//! > 조건부로 넣으면 있고 없고가 길이 차이를 만들어 다시 모호해진다.
//!
//! 그런데 **그 규칙을 지우는 뮤테이션 두 개가 살아남았다.**
//!
//! ```text
//! attempt 를 0 이 아닐 때만 8바이트가 아닌 4바이트로 넣는다   -> 테스트 통과
//! tag 를 유도 입력에서 통째로 뺀다                            -> 테스트 통과
//! ```
//!
//! 기존 단위 테스트(`nonce.rs` 안의 `#[cfg(test)] mod tests`)는 **서로 다른
//! 입력이 서로 다른 값을 내는가**만 봤다. 길이 접두사가 살아 있는 한 그
//! 성질은 위 두 결함이 있어도 유지된다 — 그래서 아무것도 재지 못했다.
//! `tag` 를 빼도 통과한 것이 가장 뚜렷하다. `tag` 는 같은 id 를 쓰는
//! 서로 다른 용도(heartbeat · resume 등)를 갈라 주는 유일한 성분인데,
//! 그것이 없어도 테스트가 전부 초록이었다.
//!
//! 그래서 여기서는 성질이 아니라 **바이트열 자체**를 고정한다.
//!
//! ```text
//! input = u64_be(len(tag)) || tag
//!      || u64_be(len(id))  || id
//!      || u64_be(connection_attempt)
//! nonce = BLAKE3_256(input)[..16]
//! ```
//!
//! ★ 기대값은 이 저장소 밖에서 계산했다(Python `blake3`). 구현이 내는 값을
//!   받아쓰면 "지금 나오는 것" 을 재확인할 뿐 규범을 재지 못한다.

use gputeer_protocol::canonical::blake3_256;
use gputeer_protocol::nonce::derive_replay_nonce;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// signing.md §10 배치를 테스트 안에서 **독립적으로** 다시 쓴 것.
///
/// 구현과 같은 함수를 부르면 아무것도 대조하지 못한다.
fn spec_nonce(tag: &str, id: &str, connection_attempt: u32) -> Vec<u8> {
    let mut input = Vec::new();
    input.extend_from_slice(&(tag.len() as u64).to_be_bytes());
    input.extend_from_slice(tag.as_bytes());
    input.extend_from_slice(&(id.len() as u64).to_be_bytes());
    input.extend_from_slice(id.as_bytes());
    // ★ 값과 무관하게 **항상** 8바이트다. 조건부로 넣지 않는다.
    input.extend_from_slice(&(connection_attempt as u64).to_be_bytes());
    blake3_256(&input)[..16].to_vec()
}

/// 저장소 밖에서 계산한 고정 벡터.
///
/// 이 세 값이 맞으면 tag · id · attempt 세 성분이 모두, 그리고 각각의
/// 길이 접두사가 규범대로 들어간 것이다.
#[test]
fn derivation_matches_the_normative_byte_layout() {
    let cases: [(&str, &str, u32, &str); 3] = [
        // ★ attempt = 0 도 8바이트를 차지한다. 이 줄이 조건부 삽입을 잡는다.
        (
            "node-heartbeat",
            "L:51234",
            0,
            "2ddac002349d54a1157e49a58219e8aa",
        ),
        (
            "node-heartbeat",
            "L:51234",
            7,
            "aa78ba5522f56167499fc149a8c5161f",
        ),
        // ★ id 와 attempt 가 같고 tag 만 다르다. 이 줄이 tag 누락을 잡는다.
        (
            "lease-resume",
            "L:51234",
            0,
            "09c9d28183c938e18eccf8a5dca68f0d",
        ),
    ];

    for (tag, id, attempt, expected) in cases {
        let actual = derive_replay_nonce(tag, id, attempt);
        assert_eq!(
            hex(&actual),
            expected,
            "({tag}, {id}, {attempt}) 의 nonce 가 규범 배치와 다르다"
        );
        // 테스트 안의 참조 구현과도 맞는지 — 고정 벡터가 낡았을 때 어느 쪽이
        // 틀렸는지 구분할 수 있게 둘 다 본다.
        assert_eq!(
            actual,
            spec_nonce(tag, id, attempt),
            "({tag}, {id}, {attempt}) 이 테스트의 참조 구현과도 다르다"
        );
    }
}

/// ★ `tag` 가 실제로 결과를 가르는가.
///
/// `tag` 는 같은 id 를 쓰는 서로 다른 용도를 갈라 주는 유일한 성분이다.
/// 이것이 무시되면 heartbeat 의 nonce 와 resume 의 nonce 가 같아진다 —
/// 같은 domain 안에서라면 한쪽이 다른 쪽을 replay 로 밀어낸다.
#[test]
fn the_tag_separates_purposes_that_share_an_id() {
    let id = "L:51234";
    assert_ne!(
        derive_replay_nonce("node-heartbeat", id, 0),
        derive_replay_nonce("lease-resume", id, 0),
        "tag 가 nonce 유도에 반영되지 않는다"
    );
}

/// `connection_attempt` 는 0 과 1 을 가르고, 0 일 때도 자리를 차지한다.
///
/// 자리를 차지하지 않으면 성분 경계가 다시 모호해진다 (모듈 문서의 옛 결함).
#[test]
fn the_attempt_occupies_its_slot_even_when_zero() {
    let (tag, id) = ("node-heartbeat", "L:51234");
    assert_ne!(
        derive_replay_nonce(tag, id, 0),
        derive_replay_nonce(tag, id, 1)
    );
    // 0 일 때 아무것도 넣지 않는 구현은 이 단언에서 갈린다 —
    // 참조 구현은 항상 8바이트를 넣기 때문이다.
    assert_eq!(derive_replay_nonce(tag, id, 0), spec_nonce(tag, id, 0));
}

/// replay guard 가 기대하는 크기 (§10 — CSPRNG 16바이트 MUST).
#[test]
fn the_nonce_is_sixteen_bytes() {
    assert_eq!(derive_replay_nonce("t", "id", 0).len(), 16);
}
