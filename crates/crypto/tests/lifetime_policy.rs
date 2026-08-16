//! T2 — `signing.md` §9 시각 정책이 **실메시지**에서 동작하는가.
//!
//! `DoD-04` 는 §9 의 단수명 경로를 **테스트 전용 타입**으로만 검증했다.
//! 이 파일이 그 공백을 닫는다.
//!
//! # 세 갈래
//!
//! ```text
//! ShortLived   ExecutionGrant · RenewLeaseRequest
//!              skew 검사 + 만료 검사 + nonce 필수
//!
//! LongLived    JobManifest · Lease
//!              만료만. 큐 대기가 정상이므로 skew 를 걸면 안 된다
//!
//! Evidence     6종 (ADR-029)
//!              만료 검사 없음. observed_at 을 노출해 소비 측이 신선도를 판단
//! ```
//!
//! ★ 가장 중요한 것은 **"거부해야 할 때 거부하는가"** 와
//!   **"거부하면 안 될 때 거부하지 않는가"** 를 둘 다 보는 것이다.
//!   전자만 보면 "전부 거부" 하는 구현이 통과한다.

use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
use gputeer_protocol::constants::{CLOCK_SKEW_TOLERANCE_MS, GRANT_TTL_MS};
use gputeer_protocol::pb;
use gputeer_protocol::signing::{verify, Lifetime, NoReplayCheck, Signable, VerifyOutcome};

const NOW: u64 = 1_755_200_000_000;
const COORD: &str = "01JBXR7Q0000000000000000CC";
const NODE: &str = "node-1";

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn ring(pairs: &[(&str, &SigningKey)]) -> Ed25519Verifier<InMemoryKeyring> {
    let mut kr = InMemoryKeyring::new();
    for (id, k) in pairs {
        kr.insert(*id, k.verifying_key());
    }
    Ed25519Verifier::new(kr)
}

fn nonce16() -> Vec<u8> {
    (0u8..16).collect()
}

// ══════════════════════════════════════════════════════════════════
// ShortLived — ExecutionGrant
// ══════════════════════════════════════════════════════════════════

fn grant(k: &SigningKey, issued: u64, ttl: u64) -> pb::ExecutionGrant {
    let mut g = pb::ExecutionGrant {
        schema_version: 1,
        grant_id: "01JBXGRANT0000000000000001".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        coordinator_device_id: COORD.into(),
        coordinator_term: 7,
        issued_at_unix_ms: issued,
        expires_at_unix_ms: issued + ttl,
        nonce: nonce16(),
        ..Default::default()
    };
    g.coordinator_signature = sign(k, &g).to_vec();
    g
}

#[test]
fn execution_grant_is_short_lived() {
    assert_eq!(pb::ExecutionGrant::LIFETIME, Lifetime::ShortLived);
    assert_eq!(pb::RenewLeaseRequest::LIFETIME, Lifetime::ShortLived);
}

#[test]
fn valid_grant_verifies() {
    let k = key(1);
    let g = grant(&k, NOW, GRANT_TTL_MS);
    let v = verify(
        &g,
        1,
        &ring(&[(COORD, &k)]),
        NOW,
        &mut NoReplayCheck,
    )
    .expect("정상 Grant 가 검증을 통과해야 한다");
    assert_eq!(v.signer_id(), COORD);
    // ★ 단수명이므로 replay 검사가 필요하고, NoReplayCheck 는 그것을 하지 않는다
    assert!(!v.replay_checked());
    assert_eq!(
        v.require_replay_checked().unwrap_err(),
        VerifyOutcome::Replay,
        "replay 미검사 Grant 로 Job 을 실행하면 안 된다"
    );
}

/// ★ 과거 방향 skew — 검증자 시계가 **빠른** 경우.
///
/// `DoD-04` 에서 확인했듯 기본 TTL(60초) == skew 허용치(60초)라
/// **미래 방향 skew 는 만료 검사에 가려진다.** 과거 방향만 도달 가능하다.
#[test]
fn grant_rejects_message_from_the_future() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let g = grant(&k, NOW, GRANT_TTL_MS);

    // 경계 안쪽 — 통과해야 한다 (거부하면 안 될 때 거부하지 않는가)
    verify(&g, 1, &r, NOW - CLOCK_SKEW_TOLERANCE_MS, &mut NoReplayCheck)
        .expect("skew 경계값은 허용된다");

    // 경계 밖 — 거부
    assert_eq!(
        verify(&g, 1, &r, NOW - CLOCK_SKEW_TOLERANCE_MS - 1, &mut NoReplayCheck)
            .unwrap_err().outcome().unwrap(),
        VerifyOutcome::ClockSkew
    );
}

/// ★ 기본 TTL 에서 미래 방향 skew 는 만료 검사에 가려진다 (DoD-04 발견 고정).
///
/// 안전성 문제는 아니다 — 둘 다 거부한다. 그러나 이 사실을 모르면
/// "skew 검사가 양방향으로 동작한다" 고 잘못 믿게 된다.
#[test]
fn default_ttl_masks_forward_skew_for_real_grant() {
    assert_eq!(
        GRANT_TTL_MS, CLOCK_SKEW_TOLERANCE_MS,
        "§9 의 Grant TTL 과 skew 허용치가 더 이상 같지 않다 — 아래 단언을 재검토하라"
    );
    let k = key(1);
    let g = grant(&k, NOW, GRANT_TTL_MS);
    assert_eq!(
        verify(&g, 1, &ring(&[(COORD, &k)]), NOW + GRANT_TTL_MS, &mut NoReplayCheck)
            .unwrap_err().outcome().unwrap(),
        VerifyOutcome::Expired,
        "기본 TTL 에서는 미래 방향 skew 경로에 도달할 수 없다"
    );
}

/// TTL 을 늘리면 미래 방향 skew 경로가 실제로 살아나는가.
///
/// 위 테스트만 있으면 "미래 방향 검사가 아예 없는" 구현과 구분되지 않는다.
#[test]
fn forward_skew_is_reachable_with_longer_ttl() {
    let k = key(1);
    let g = grant(&k, NOW, 3_600_000); // 1시간 TTL
    assert_eq!(
        verify(&g, 1, &ring(&[(COORD, &k)]), NOW + CLOCK_SKEW_TOLERANCE_MS + 1, &mut NoReplayCheck)
            .unwrap_err().outcome().unwrap(),
        VerifyOutcome::ClockSkew,
        "미래 방향 skew 검사가 동작하지 않는다"
    );
}

/// ★ nonce 는 **메시지 안의 서명된 필드**에서 온다 (독립 검수 2026-08-16).
///
/// 예전에는 `verify()` 가 nonce 를 별도 인자로 받았다. 그러면 호출자가
/// 서명된 nonce 대신 아무 값이나 넘길 수 있고, **서명은 통과하는데 replay
/// 방어만 무력화**된다. 매번 새 값을 넘기면 같은 메시지를 몇 번이든 재생할 수 있다.
///
/// 이제 nonce 는 메시지에서 나오므로 **호출자가 고를 수 없다.**
/// 따라서 nonce 를 바꾸려면 메시지를 바꿔야 하고, 그러면 서명이 깨진다.
#[test]
fn grant_nonce_comes_from_signed_message_field() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);

    // nonce 가 없는 Grant — 서명은 정상이지만 replay 대상 nonce 가 없다
    let mut no_nonce = pb::ExecutionGrant {
        schema_version: 1,
        grant_id: "g".into(),
        coordinator_device_id: COORD.into(),
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + GRANT_TTL_MS,
        nonce: vec![],
        ..Default::default()
    };
    no_nonce.coordinator_signature = sign(&k, &no_nonce).to_vec();
    assert_eq!(
        verify(&no_nonce, 1, &r, NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::Replay,
        "nonce 없는 단수명 메시지가 통과했다"
    );

    // 길이가 틀린 nonce — §10 은 CSPRNG 16바이트를 요구한다(MUST)
    for len in [1usize, 8, 15, 17, 32] {
        let mut g = pb::ExecutionGrant {
            nonce: vec![7u8; len],
            ..no_nonce.clone()
        };
        g.coordinator_signature = sign(&k, &g).to_vec(); // 서명은 정상으로 다시 만든다
        assert_eq!(
            verify(&g, 1, &r, NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
            VerifyOutcome::Replay,
            "{len}바이트 nonce 가 허용됐다"
        );
    }

    // ★ nonce 를 서명 후에 바꾸면 **서명이 깨진다** — 이것이 결속의 증거다
    let good = grant(&k, NOW, GRANT_TTL_MS);
    let mut swapped = good.clone();
    swapped.nonce = vec![0xFF; 16];
    assert_eq!(
        verify(&swapped, 1, &r, NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::InvalidSignature,
        "nonce 가 서명 대상이 아니다 — 재전송 시 nonce 만 갈아끼울 수 있다"
    );
}

// ══════════════════════════════════════════════════════════════════
// ShortLived — RenewLeaseRequest (expires_at 이 도출되는 유일한 메시지)
// ══════════════════════════════════════════════════════════════════

fn renew(k: &SigningKey, issued: u64) -> pb::RenewLeaseRequest {
    let mut r = pb::RenewLeaseRequest {
        schema_version: 1,
        lease_id: "01JBXLEASE0000000000000001".into(),
        fence_epoch: 42,
        node_id: NODE.into(),
        issued_at_unix_ms: issued,
        nonce: nonce16(),
        ..Default::default()
    };
    r.node_signature = sign(k, &r).to_vec();
    r
}

/// ★ `RenewLeaseRequest` 에는 `expires_at` 필드가 없다.
/// `issued_at + GRANT_TTL_MS` 로 도출한다 — 값을 지어내는 게 아니라 §9 의 TTL 적용이다.
#[test]
fn renew_lease_expiry_is_derived_from_ttl() {
    let k = key(2);
    let r = renew(&k, NOW);
    assert_eq!(
        Signable::expires_at_unix_ms(&r),
        NOW + GRANT_TTL_MS,
        "만료 시각이 §9 의 TTL 로 도출되지 않았다"
    );

    let ring = ring(&[(NODE, &k)]);
    // 만료 직전 통과
    verify(&r, 1, &ring, NOW + GRANT_TTL_MS - 1, &mut NoReplayCheck)
        .expect("만료 1ms 전은 유효");
    // 만료 시점 거부
    assert_eq!(
        verify(&r, 1, &ring, NOW + GRANT_TTL_MS, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::Expired
    );
}

/// `issued_at` 이 0이면 도출된 만료도 작아진다 — 즉시 만료된다.
/// 이것은 결함이 아니라 **"시각 없는 갱신 요청은 무효"** 라는 올바른 동작이다.
#[test]
fn renew_lease_without_issued_at_is_immediately_expired() {
    let k = key(2);
    let r = renew(&k, 0);
    assert_eq!(
        verify(&r, 1, &ring(&[(NODE, &k)]), NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::Expired
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ Evidence (ADR-029) — 6종
// ══════════════════════════════════════════════════════════════════

fn ckpt(k: &SigningKey, created: u64) -> pb::CheckpointManifest {
    let mut c = pb::CheckpointManifest {
        schema_version: 1,
        checkpoint_id: "01JBXCKPT0000000000000001".into(),
        job_id: "01JBXR7Q0000000000000000AA".into(),
        step: 12000,
        created_at_unix_ms: created,
        producer_node_id: NODE.into(),
        fence_epoch: 42,
        ..Default::default()
    };
    c.producer_signature = sign(k, &c).to_vec();
    c
}

#[test]
fn all_six_evidence_messages_are_marked_evidence() {
    assert_eq!(pb::CheckpointManifest::LIFETIME, Lifetime::Evidence);
    assert_eq!(pb::ReplicaAck::LIFETIME, Lifetime::Evidence);
    assert_eq!(pb::ArtifactRef::LIFETIME, Lifetime::Evidence);
    assert_eq!(pb::AttemptReport::LIFETIME, Lifetime::Evidence);
    assert_eq!(pb::CanonicalDecision::LIFETIME, Lifetime::Evidence);
    assert_eq!(pb::RevokeLeaseNotice::LIFETIME, Lifetime::Evidence);
}

/// ★ 증거는 만료되지 않는다.
///
/// 만료시키면 **오래된 체크포인트에서 재개할 수 없게 되고**,
/// 그것은 이 시스템의 존재 이유(다른 GPU 에서 작업을 이어간다)를 부순다.
#[test]
fn evidence_does_not_expire() {
    let k = key(3);
    let created = NOW;
    let c = ckpt(&k, created);
    let r = ring(&[(NODE, &k)]);

    // 3년 뒤에도 통과해야 한다
    for years in [0u64, 1, 3, 10] {
        let later = created + years * 365 * 24 * 3_600_000;
        verify(&c, 1, &r, later, &mut NoReplayCheck).unwrap_or_else(|e| {
            panic!("{years}년 뒤 체크포인트 증거가 거부됐다: {e:?} — 재개가 불가능해진다")
        });
    }

    // 발급 시각보다 **이전**에 검증해도 통과한다 (시계가 어긋나도 증거는 증거다)
    verify(&c, 1, &r, created - 3_600_000, &mut NoReplayCheck)
        .expect("증거에 skew 규칙을 걸면 안 된다");
}

/// 증거는 replay 대상이 아니다 — `nonce` 없이도 `require_replay_checked` 가 통과한다.
#[test]
fn evidence_is_not_replay_checked() {
    let k = key(3);
    let c = ckpt(&k, NOW);
    let v = verify(&c, 1, &ring(&[(NODE, &k)]), NOW, &mut NoReplayCheck).unwrap();
    assert!(v.replay_checked(), "증거는 replay 대상이 아니므로 검사된 것으로 본다");
    assert!(v.require_replay_checked().is_ok());
}

/// ★ 증거는 **언제의 사실인가** 를 반드시 노출해야 한다 (ADR-029).
///
/// 0 을 반환하면 "언제인지 모르는 증거" 이고, 그것은 증거가 아니다.
/// 소비 측이 신선도를 판단할 수 없다.
#[test]
fn evidence_must_expose_observation_time() {
    let k = key(3);

    let c = ckpt(&k, NOW);
    assert_eq!(Signable::observed_at_unix_ms(&c), NOW);

    let ack = pb::ReplicaAck {
        schema_version: 1,
        checkpoint_id: "c".into(),
        holder_device_id: "h".into(),
        acked_at_unix_ms: NOW + 1,
        ..Default::default()
    };
    assert_eq!(Signable::observed_at_unix_ms(&ack), NOW + 1);

    let art = pb::ArtifactRef {
        schema_version: 1,
        artifact_id: "a".into(),
        attempt_id: "at".into(),
        created_at_unix_ms: NOW + 2,
        ..Default::default()
    };
    assert_eq!(Signable::observed_at_unix_ms(&art), NOW + 2);

    let rep = pb::AttemptReport {
        schema_version: 1,
        node_id: NODE.into(),
        issued_at_unix_ms: NOW + 3,
        ..Default::default()
    };
    assert_eq!(Signable::observed_at_unix_ms(&rep), NOW + 3);

    let dec = pb::CanonicalDecision {
        schema_version: 1,
        job_id: "j".into(),
        decided_at_unix_ms: NOW + 4,
        ..Default::default()
    };
    assert_eq!(Signable::observed_at_unix_ms(&dec), NOW + 4);

    let rev = pb::RevokeLeaseNotice {
        schema_version: 1,
        lease_id: "l".into(),
        issued_at_unix_ms: NOW + 5,
        ..Default::default()
    };
    assert_eq!(Signable::observed_at_unix_ms(&rev), NOW + 5);
}

/// ★ `ReplicaAck` 의 위험 — 복제본이 삭제되어도 ACK 는 영원히 유효하다.
///
/// 이 테스트는 **결함을 고정한다.** 통과한다는 것이 곧
/// "프로토콜이 이 상황을 막지 못한다" 는 뜻이다 (ADR-029 · `TODO_VISION` V-07).
#[test]
fn replica_ack_stays_valid_forever_even_if_replica_is_gone() {
    let k = key(4);
    let mut ack = pb::ReplicaAck {
        schema_version: 1,
        checkpoint_id: "01JBXCKPT0000000000000001".into(),
        holder_device_id: "01JBXR7Q0000000000000000HH".into(),
        failure_domain: "rack-a".into(),
        fsynced: true,
        hash_verified: true,
        stored_bytes: 1_073_741_824,
        acked_at_unix_ms: NOW,
        ..Default::default()
    };
    ack.holder_signature = sign(&k, &ack).to_vec();

    // 10년 뒤 — 복제본이 오래전에 삭제되었더라도 서명은 유효하다
    let v = verify(
        &ack,
        1,
        &ring(&[("01JBXR7Q0000000000000000HH", &k)]),
        NOW + 10 * 365 * 24 * 3_600_000,
        &mut NoReplayCheck,
    )
    .expect("증거이므로 만료되지 않는다");

    // ★ 프로토콜이 줄 수 있는 것은 "언제의 사실인가" 뿐이다.
    //   "지금도 durable 한가" 는 소비 측이 별도로 확인해야 한다.
    assert_eq!(
        v.get().acked_at_unix_ms,
        NOW,
        "소비 측이 신선도를 판단할 유일한 근거"
    );
    // ReplicaAck 에는 fence_epoch 이 없다 — 이것이 V-07 의 근거다.
    assert!(
        !format!("{:?}", v.get()).contains("fence_epoch"),
        "ReplicaAck 에 fence_epoch 이 생겼다면 V-07 을 해소하고 이 테스트를 갱신하라"
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 세 정책이 서로 다른 동작을 하는가 (비공허성)
//
// 전부 같은 동작이면 Lifetime 구분이 의미가 없다.
// ══════════════════════════════════════════════════════════════════

#[test]
fn the_three_lifetimes_actually_behave_differently() {
    let k = key(5);

    // 1) LongLived — 오래 대기해도 통과, 만료되면 거부
    let mut m = pb::JobManifest {
        schema_version: 1,
        job_id: "j".into(),
        submitter_device_id: COORD.into(),
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + 7 * 24 * 3_600_000,
        ..Default::default()
    };
    m.submitter_signature = sign(&k, &m).to_vec();
    let r = ring(&[(COORD, &k), (NODE, &k)]);

    // 6시간 대기 — skew 를 훨씬 넘지만 통과해야 한다
    verify(&m, 1, &r, NOW + 6 * 3_600_000, &mut NoReplayCheck)
        .expect("LongLived 는 skew 를 검사하지 않는다");
    // 만료 후 거부
    assert_eq!(
        verify(&m, 1, &r, m.expires_at_unix_ms, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::Expired
    );

    // 2) ShortLived — 같은 6시간이면 거부된다
    let g = grant(&k, NOW, GRANT_TTL_MS);
    assert!(
        verify(&g, 1, &r, NOW + 6 * 3_600_000, &mut NoReplayCheck).is_err(),
        "ShortLived 가 6시간 뒤에도 통과했다 — LongLived 와 구분되지 않는다"
    );

    // 3) Evidence — 6시간이든 10년이든 통과한다
    let c = ckpt(&k, NOW);
    verify(&c, 1, &r, NOW + 10 * 365 * 24 * 3_600_000, &mut NoReplayCheck)
        .expect("Evidence 가 만료됐다 — LongLived 와 구분되지 않는다");
}
