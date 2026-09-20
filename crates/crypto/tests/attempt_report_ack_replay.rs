//! 결함 76 (재검수 56) — `AttemptReportAck` 의 수명 · replay nonce 를 **값으로** 고정한다.
//!
//! `lifetime_consistency.rs` 의 공통 `check()` 는 대상이 **선언한** 수명으로 분기한다. 그래서 Ack 의 `LIFETIME` 을
//! `Perpetual` 로 바꾸면 빈 분기를 지나 통과하고, `replay_nonce()` 를 `Some(&[])` 로 바꿔도 `Some` 이라 통과한다 —
//! 실제 검증기에서는 각각 시각 · replay 검사가 사라지거나 모든 Ack 가 거부된다.
//!
//! 여기서는 선언값 자체와, 실제 검증기(`Ed25519Verifier` + `InMemoryReplayGuard`)의 수락 · 중복 거부를 본다.

use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, InMemoryReplayGuard, SigningKey};
use gputeer_protocol::pb;
use gputeer_protocol::signing::{verify, Lifetime, Signable, VerifyOutcome};

const COORDINATOR: &str = "coordinator-1";
const ISSUED: u64 = 1_755_104_402_000;
const NOW: u64 = ISSUED + 1_000;

fn key() -> SigningKey {
    SigningKey::from_bytes(&[5u8; 32])
}

fn ack(key: &SigningKey, session_nonce: Vec<u8>) -> pb::AttemptReportAck {
    let mut ack = pb::AttemptReportAck {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        node_id: "node-1".into(),
        fence_epoch: 42,
        report_hash: Some(pb::Digest {
            algo: 1,
            value: vec![7; 32],
        }),
        created: true,
        coordinator_id: COORDINATOR.into(),
        issued_at_unix_ms: ISSUED,
        session_nonce,
        ..Default::default()
    };
    ack.coordinator_signature = sign(key, &ack).to_vec();
    ack
}

fn verifier(key: &SigningKey) -> Ed25519Verifier<InMemoryKeyring> {
    let mut keys = InMemoryKeyring::new();
    keys.insert(COORDINATOR, key.verifying_key());
    Ed25519Verifier::new(keys)
}

/// 선언값 자체 — ShortLived 이고, replay nonce 는 이 세션 Hello 의 nonce(`session_nonce`) 바이트 그대로다.
#[test]
fn ack_is_short_lived_and_its_replay_nonce_is_exactly_the_session_nonce() {
    assert_eq!(
        <pb::AttemptReportAck as Signable>::LIFETIME,
        Lifetime::ShortLived,
        "Ack 를 재생할 수 있으면 다른 세션의 보고를 받았다고 믿게 된다"
    );
    let nonce: Vec<u8> = (48u8..64).collect();
    let ack = ack(&key(), nonce.clone());
    assert_eq!(ack.replay_nonce(), Some(nonce.as_slice()));
    assert!(
        ack.expires_at_unix_ms() > ISSUED,
        "발급 시각보다 뒤에 만료돼야 한다 — 즉시 만료되면 모든 Ack 가 거부된다"
    );
}

/// 실제 검증기 — 정상 Ack 는 받고, **같은 Ack** 는 replay 로 거부하고, 다른 세션 nonce 는 받는다(guard 가 무조건
/// 거부하는 것이 아님을 확인 — 비공허성).
#[test]
fn a_fresh_ack_is_accepted_and_the_same_ack_again_is_refused_as_replay() {
    let key = key();
    let verifier = verifier(&key);
    let mut guard = InMemoryReplayGuard::new();

    let first = ack(&key, (48u8..64).collect());
    let verified = verify(&first, 1, &verifier, NOW, &mut guard).expect("정상 Ack 는 받는다");
    assert!(
        verified.replay_checked(),
        "실제 guard 로 검사했는데 미검사로 표시됐다"
    );

    assert_eq!(
        verify(&first, 1, &verifier, NOW, &mut guard)
            .unwrap_err()
            .outcome()
            .unwrap(),
        VerifyOutcome::Replay,
        "같은 Ack 의 재생이 통과했다"
    );

    let other_session = ack(&key, (64u8..80).collect());
    verify(&other_session, 1, &verifier, NOW, &mut guard)
        .expect("다른 세션 nonce 의 Ack 는 받는다");
}
