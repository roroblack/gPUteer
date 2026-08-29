//! replay nonce 결속 — 독립 검수(2026-08-16)가 지적한 결함의 시정 확인.
//!
//! # 무엇이 잘못됐었나
//!
//! ```text
//! 예전:  verify(msg, ..., nonce: Option<&[u8]>, replay)
//!        호출자가 nonce 를 **골라서** 넘겼다.
//!
//!        => 서명은 통과하는데 replay 방어만 무력화된다.
//!           매번 새 값을 넘기면 같은 메시지를 몇 번이든 재생할 수 있다.
//!
//! 지금:  verify(msg, ..., replay)
//!        nonce 는 msg.replay_nonce() — **메시지 안의 서명된 필드**에서 온다.
//!        호출자가 고를 수 없다.
//! ```
//!
//! # 이 파일이 확인하는 것
//!
//! 코덱스가 제안한 negative test 목록을 그대로 구현했다.

use std::collections::{HashMap, HashSet};

use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
use gputeer_protocol::canonical::Domain;
use gputeer_protocol::constants::{CLOCK_SKEW_TOLERANCE_MS, GRANT_TTL_MS};
use gputeer_protocol::pb;
use gputeer_protocol::signing::{
    verify, NoReplayCheck, ReplayDecision, ReplayGuard, ReplayStoreError, Signable, VerifyError,
    VerifyOutcome, NONCE_LEN,
};

const NOW: u64 = 1_755_200_000_000;
const COORD: &str = "01JBXR7Q0000000000000000CC";

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

fn grant(k: &SigningKey, signer: &str, nonce: Vec<u8>) -> pb::ExecutionGrant {
    let mut g = pb::ExecutionGrant {
        schema_version: 1,
        grant_id: "01JBXGRANT0000000000000001".into(),
        coordinator_device_id: signer.into(),
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + GRANT_TTL_MS,
        nonce,
        ..Default::default()
    };
    g.coordinator_signature = sign(k, &g).to_vec();
    g
}

fn n16(first: u8) -> Vec<u8> {
    let mut v = vec![0u8; NONCE_LEN];
    v[0] = first;
    v
}

// ══════════════════════════════════════════════════════════════════
// 참조 in-memory guard
//
// `RULE.md` §4.1 — replay 캐시는 Crypto 스트림 소유이므로 여기에 둔다.
// ★ 아직 **영속 저장소가 아니다.** 프로세스가 죽으면 사라진다.
// ══════════════════════════════════════════════════════════════════

#[derive(Default)]
struct MemGuard {
    seen: HashSet<(String, u32, Vec<u8>)>,
    /// nonce -> 보존 시한. GC 검증용.
    retain: HashMap<Vec<u8>, u64>,
    /// §10 상한. 도달하면 **축출이 아니라 거부**한다.
    cap: Option<usize>,
    /// 저장소 장애를 흉내낸다.
    fail: Option<ReplayStoreError>,
}

impl ReplayGuard for MemGuard {
    fn check_and_record(
        &mut self,
        signer_id: &str,
        domain: Domain,
        nonce: &[u8],
        retain_until_ms: u64,
    ) -> Result<ReplayDecision, ReplayStoreError> {
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        let k = (signer_id.to_string(), domain as u32, nonce.to_vec());
        if self.seen.contains(&k) {
            return Ok(ReplayDecision::Duplicate);
        }
        // ★ §10 — 상한 도달 시 **축출하지 않는다.** 미만료 nonce 를 밀어내면
        //   replay 창이 열린다. 거부가 안전한 실패 방향이다.
        if let Some(cap) = self.cap {
            if self.seen.len() >= cap {
                return Err(ReplayStoreError::CacheFull);
            }
        }
        self.retain.insert(nonce.to_vec(), retain_until_ms);
        self.seen.insert(k);
        Ok(ReplayDecision::Fresh)
    }
    fn is_effective(&self) -> bool {
        true
    }
}

// ══════════════════════════════════════════════════════════════════
// ★ 핵심 — nonce 가 메시지에 결속되었는가
// ══════════════════════════════════════════════════════════════════

/// 같은 메시지를 두 번 검증하면 거부된다.
#[test]
fn same_message_verified_twice_is_rejected() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let g = grant(&k, COORD, n16(1));
    let mut guard = MemGuard::default();

    let v = verify(&g, 1, &r, NOW, &mut guard).expect("첫 번째는 통과");
    assert!(v.replay_checked());
    assert!(v.require_replay_checked().is_ok());

    assert_eq!(
        verify(&g, 1, &r, NOW, &mut guard)
            .unwrap_err()
            .outcome()
            .unwrap(),
        VerifyOutcome::Replay,
        "★ 같은 메시지가 두 번 통과했다 — nonce 가 결속되지 않았다"
    );
}

/// ★ **호출자가 nonce 를 고를 수 없다.**
///
/// 예전 API 라면 호출자가 매번 새 nonce 를 넘겨 무한히 재생할 수 있었다.
/// 지금은 nonce 가 메시지에서 나오므로, 바꾸려면 메시지를 바꿔야 하고
/// 그러면 **서명이 깨진다.**
#[test]
fn caller_cannot_choose_a_different_nonce() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let g = grant(&k, COORD, n16(1));

    // 서명 후 nonce 만 바꾼다
    let mut tampered = g.clone();
    tampered.nonce = n16(2);

    assert_eq!(
        verify(&tampered, 1, &r, NOW, &mut MemGuard::default())
            .unwrap_err()
            .outcome()
            .unwrap(),
        VerifyOutcome::InvalidSignature,
        "nonce 가 서명 대상이 아니다 — 재전송 시 nonce 만 갈아끼울 수 있다"
    );
}

/// nonce 를 바꾸려면 **다시 서명**해야 하고, 그건 서명자만 할 수 있다.
/// 서명자가 새 nonce 로 새 Grant 를 만드는 것은 정상 동작이다.
#[test]
fn signer_can_issue_a_new_grant_with_a_fresh_nonce() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let mut guard = MemGuard::default();

    verify(&grant(&k, COORD, n16(1)), 1, &r, NOW, &mut guard).expect("첫 Grant");
    verify(&grant(&k, COORD, n16(2)), 1, &r, NOW, &mut guard)
        .expect("★ 새 nonce 로 발급한 Grant 는 통과해야 한다 — guard 가 무조건 거부하면 안 된다");
}

/// §10 — 키는 `(sender_device_id, domain_tag, nonce)` 다.
/// 같은 nonce 값이라도 **device 가 다르면** 충돌하지 않는다.
#[test]
fn nonce_namespace_is_per_device() {
    const OTHER: &str = "01JBXR7Q0000000000000000EE";
    let k1 = key(1);
    let k2 = key(2);
    let r = ring(&[(COORD, &k1), (OTHER, &k2)]);
    let mut guard = MemGuard::default();

    verify(&grant(&k1, COORD, n16(1)), 1, &r, NOW, &mut guard).expect("device A");
    verify(&grant(&k2, OTHER, n16(1)), 1, &r, NOW, &mut guard)
        .expect("★ device 별 namespace 가 없어 다른 device 의 nonce 가 충돌했다");
}

// ══════════════════════════════════════════════════════════════════
// ★ 실패한 검증은 캐시를 채우지 않는다
//
// 채우면 공격자가 유효하지 않은 메시지로 캐시를 채워
// 정상 nonce 를 못 쓰게 만들 수 있다 (DoS).
// ══════════════════════════════════════════════════════════════════

#[test]
fn failed_verification_does_not_consume_the_nonce() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let mut guard = MemGuard::default();

    // 서명이 깨진 메시지 — nonce 는 정상값
    let mut bad = grant(&k, COORD, n16(1));
    bad.coordinator_signature = vec![0u8; 64];
    assert!(verify(&bad, 1, &r, NOW, &mut guard).is_err());

    // 만료된 메시지
    let expired = grant(&k, COORD, n16(2));
    assert!(verify(&expired, 1, &r, NOW + GRANT_TTL_MS, &mut guard).is_err());

    // skew 위반
    let skewed = grant(&k, COORD, n16(3));
    assert!(verify(
        &skewed,
        1,
        &r,
        NOW - CLOCK_SKEW_TOLERANCE_MS - 1,
        &mut guard
    )
    .is_err());

    assert!(
        guard.seen.is_empty(),
        "★ 실패한 검증이 캐시를 채웠다 — 공격자가 정상 nonce 를 소진시킬 수 있다"
    );

    // 같은 nonce 로 정상 메시지를 보내면 통과해야 한다
    verify(&grant(&k, COORD, n16(1)), 1, &r, NOW, &mut guard)
        .expect("실패한 검증이 nonce 를 소비했다");
}

// ══════════════════════════════════════════════════════════════════
// ★ 저장소 장애는 Replay 가 아니다
// ══════════════════════════════════════════════════════════════════

/// 저장소 장애를 `Replay` 로 뭉뚱그리면 운영자가 원인을 못 찾는다.
///
/// `CLAUDE.md` §3 — "오류 메시지가 사실을 잘못 전하지 않게 한다."
#[test]
fn storage_failure_is_distinguishable_from_replay() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let g = grant(&k, COORD, n16(1));

    for e in [
        ReplayStoreError::Io("disk full".into()),
        ReplayStoreError::LockTimeout,
        ReplayStoreError::CacheFull,
    ] {
        let mut guard = MemGuard {
            fail: Some(e.clone()),
            ..Default::default()
        };
        let err = verify(&g, 1, &r, NOW, &mut guard).unwrap_err();

        // ★ 프로토콜 결과가 아니다 — 상대에게 보고할 값이 아니다
        assert_eq!(
            err.outcome(),
            None,
            "저장소 장애가 VerifyOutcome 으로 보고됐다 — \
             '상대가 재전송했다' 와 '우리 디스크가 죽었다' 가 구분되지 않는다"
        );
        assert_eq!(err, VerifyError::ReplayStore(e.clone()));
        assert!(!err.explain().is_empty());
    }
}

/// ★ 장애가 나도 **부작용은 실행되지 않는다** (fail closed).
#[test]
fn storage_failure_fails_closed() {
    let k = key(1);
    let g = grant(&k, COORD, n16(1));
    let mut guard = MemGuard {
        fail: Some(ReplayStoreError::LockTimeout),
        ..Default::default()
    };
    // Verified 자체가 만들어지지 않는다 — 부작용 경로에 값이 도달할 수 없다
    assert!(verify(&g, 1, &ring(&[(COORD, &k)]), NOW, &mut guard).is_err());
}

// ══════════════════════════════════════════════════════════════════
// ★ §10 — 상한 도달 시 축출이 아니라 거부
// ══════════════════════════════════════════════════════════════════

/// v5 초안은 "오래된 것부터 제거" 였다.
/// **아직 유효한 nonce 가 밀려나면 replay 창이 열린다.**
/// 축출이 아니라 **거부**가 안전한 실패 방향이다 (§10).
#[test]
fn cache_full_rejects_instead_of_evicting() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let mut guard = MemGuard {
        cap: Some(2),
        ..Default::default()
    };

    verify(&grant(&k, COORD, n16(1)), 1, &r, NOW, &mut guard).expect("1");
    verify(&grant(&k, COORD, n16(2)), 1, &r, NOW, &mut guard).expect("2");

    // 3번째는 거부된다
    let err = verify(&grant(&k, COORD, n16(3)), 1, &r, NOW, &mut guard).unwrap_err();
    assert_eq!(err, VerifyError::ReplayStore(ReplayStoreError::CacheFull));

    // ★ 기존 nonce 가 살아 있어야 한다 — 축출되지 않았다
    assert_eq!(guard.seen.len(), 2);
    assert_eq!(
        verify(&grant(&k, COORD, n16(1)), 1, &r, NOW, &mut guard)
            .unwrap_err()
            .outcome()
            .unwrap(),
        VerifyOutcome::Replay,
        "★ 상한 도달 시 기존 유효 nonce 가 축출됐다 — replay 창이 열렸다"
    );
}

// ══════════════════════════════════════════════════════════════════
// 보존 시한 (§10)
// ══════════════════════════════════════════════════════════════════

/// §10 — 보존 기간은 `expires_at + clock_skew_tolerance` 다.
///
/// `RenewLeaseRequest` 는 `expires_at` 필드가 없어 도출된 값을 쓴다.
#[test]
fn retain_until_is_expiry_plus_skew() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let mut guard = MemGuard::default();

    let g = grant(&k, COORD, n16(1));
    verify(&g, 1, &r, NOW, &mut guard).unwrap();
    assert_eq!(
        guard.retain.get(&n16(1)).copied(),
        Some(NOW + GRANT_TTL_MS + CLOCK_SKEW_TOLERANCE_MS),
        "보존 시한이 §10 대로 계산되지 않았다"
    );

    // RenewLeaseRequest — expires_at 이 도출된다
    const NODE: &str = "node-1";
    let k2 = key(2);
    let mut rn = pb::RenewLeaseRequest {
        schema_version: 1,
        lease_id: "l".into(),
        node_id: NODE.into(),
        issued_at_unix_ms: NOW,
        nonce: n16(9),
        ..Default::default()
    };
    rn.node_signature = sign(&k2, &rn).to_vec();

    let mut g2 = MemGuard::default();
    verify(&rn, 1, &ring(&[(NODE, &k2)]), NOW, &mut g2).unwrap();
    assert_eq!(
        g2.retain.get(&n16(9)).copied(),
        Some(Signable::expires_at_unix_ms(&rn) + CLOCK_SKEW_TOLERANCE_MS),
        "도출 만료를 쓰는 메시지의 보존 시한이 틀렸다"
    );
}

// ══════════════════════════════════════════════════════════════════
// 비공허성 — NoReplayCheck 와 실제 guard 가 다르게 동작하는가
// ══════════════════════════════════════════════════════════════════

#[test]
fn no_replay_check_and_working_guard_differ() {
    let k = key(1);
    let r = ring(&[(COORD, &k)]);
    let g = grant(&k, COORD, n16(1));

    // NoReplayCheck — 두 번 다 통과하지만 replay_checked() 가 false
    let v1 = verify(&g, 1, &r, NOW, &mut NoReplayCheck).unwrap();
    let v2 = verify(&g, 1, &r, NOW, &mut NoReplayCheck).unwrap();
    assert!(!v1.replay_checked() && !v2.replay_checked());
    assert!(v1.require_replay_checked().is_err());

    // 실제 guard — 두 번째가 거부된다
    let mut guard = MemGuard::default();
    let v3 = verify(&g, 1, &r, NOW, &mut guard).unwrap();
    assert!(v3.replay_checked());
    assert!(v3.require_replay_checked().is_ok());
    assert!(verify(&g, 1, &r, NOW, &mut guard).is_err());
}
