//! `InMemoryReplayGuard` — `signing.md` §10 의 참조 구현 검증.
//!
//! 독립 검수 설계 검토(2026-08-16)가 준 순서의 **2단계**다.
//!
//! ```text
//! 1. 계약 · API 수정   ✅ nonce 결속 · Result API · retain_until
//! 2. 메모리 참조 구현  ← 이 파일이 검증한다
//! 3. 영속 저장소       ⬜
//! ```
//!
//! # 검수자가 지목한 negative test 를 그대로 구현했다
//!
//! ```text
//! - 동시 검증 두 건 중 정확히 하나만 성공
//! - invalid signature 는 캐시를 채우지 않음
//! - expired / clock-skew 메시지도 캐시를 채우지 않음
//! - cache-full 시 기존 유효 nonce 가 삭제되지 않음
//! - namespace 별 quota 가 다른 sender 를 막지 않음
//! ```
//!
//! 여기에 **시계 되감김**을 추가했다 — 검수자가 "확신 없음" 으로 남긴 부분이다.

use gputeer_crypto::replay::MAX_GC_ADVANCE_MS;
use gputeer_crypto::{InMemoryReplayGuard, DEFAULT_CAPACITY};
use gputeer_protocol::canonical::Domain;
use gputeer_protocol::constants::{CLOCK_SKEW_TOLERANCE_MS, MAX_SHORTLIVED_TTL_MS};
use gputeer_protocol::signing::{ReplayDecision, ReplayGuard, ReplayStoreError};

const T: u64 = 1_755_200_000_000;
const RETAIN: u64 = T + 120_000;

fn n(b: u8) -> Vec<u8> {
    let mut v = vec![0u8; 16];
    v[0] = b;
    v
}

fn rec(
    g: &mut InMemoryReplayGuard,
    who: &str,
    nonce: u8,
) -> Result<ReplayDecision, ReplayStoreError> {
    g.check_and_record(who, Domain::Grant, &n(nonce), RETAIN)
}

// ══════════════════════════════════════════════════════════════════
// 기본
// ══════════════════════════════════════════════════════════════════

#[test]
fn first_use_is_fresh_and_reuse_is_duplicate() {
    let mut g = InMemoryReplayGuard::new();
    assert_eq!(rec(&mut g, "a", 1).unwrap(), ReplayDecision::Fresh);
    assert_eq!(rec(&mut g, "a", 1).unwrap(), ReplayDecision::Duplicate);
    // 다른 nonce 는 통과 — 무조건 거부하는 게 아님을 확인 (비공허성)
    assert_eq!(rec(&mut g, "a", 2).unwrap(), ReplayDecision::Fresh);
}

/// §10 — 키는 `(sender_device_id, domain_tag, nonce)` 다.
#[test]
fn key_is_namespaced_by_device_and_domain() {
    let mut g = InMemoryReplayGuard::new();
    assert_eq!(rec(&mut g, "a", 1).unwrap(), ReplayDecision::Fresh);

    // 다른 device — 통과해야 한다
    assert_eq!(
        rec(&mut g, "b", 1).unwrap(),
        ReplayDecision::Fresh,
        "device 별 namespace 가 없어 다른 device 의 nonce 가 충돌했다"
    );

    // 다른 domain — 통과해야 한다
    assert_eq!(
        g.check_and_record("a", Domain::LeaseRenew, &n(1), RETAIN)
            .unwrap(),
        ReplayDecision::Fresh,
        "domain 별 namespace 가 없다"
    );
}

/// ★ 한 device 가 다른 device 의 nonce 공간을 소진시킬 수 **없다.**
///
/// # 이력
///
/// 2026-08-16 까지 이 테스트는 **반대**를 고정했다 —
/// `global_capacity_lets_one_device_starve_others`.
/// 전역 상한만 있었고, 시끄러운 device 하나가 캐시를 채우면
/// 조용한 device 도 전부 `CacheFull` 로 거부됐다.
///
/// 그 테스트는 스스로 이렇게 적어 뒀다:
/// "이 테스트가 실패했다면 device 별 quota 가 생겼다는 뜻이다 —
///  그 사실을 §10 과 이 주석에 반영하라."
///
/// 독립 검수(2026-08-17)가 같은 결함을 **중대**로 지적했고 quota 를 만들었다.
/// 지금은 그 지시대로 이 테스트가 반대 방향을 고정한다.
#[test]
fn one_device_cannot_starve_others() {
    // 전역 6, 서명자별 3
    let mut g = InMemoryReplayGuard::with_capacities(6, 3);

    for i in 0..3 {
        assert_eq!(rec(&mut g, "noisy", i).unwrap(), ReplayDecision::Fresh);
    }

    // 시끄러운 쪽은 **자기 몫에서** 막힌다 — 전역이 아직 남았는데도.
    match rec(&mut g, "noisy", 99).unwrap_err() {
        ReplayStoreError::SignerQuotaExceeded { signer_id, quota } => {
            assert_eq!(signer_id, "noisy");
            assert_eq!(quota, 3);
        }
        other => panic!("서명자 몫이 아니라 {other:?} 로 거부됐다"),
    }

    // ★ 조용한 device 는 영향받지 않는다. 이것이 이 변경의 전부다.
    assert_eq!(
        rec(&mut g, "quiet", 1).unwrap(),
        ReplayDecision::Fresh,
        "★ 한 device 가 다른 device 를 굶겼다 — replay 방어가 DoS 통로가 됐다"
    );
}

/// 만료로 항목이 빠지면 그 서명자의 몫도 돌아와야 한다.
///
/// ★ 이것을 빠뜨리면 **한 번 몫을 채운 device 는 영원히 막힌다.**
/// quota 를 만들면서 같이 만들기 쉬운 결함이다.
#[test]
fn signer_quota_is_released_by_gc() {
    let mut g = InMemoryReplayGuard::with_capacities(10, 2);
    g.check_and_record("a", Domain::Grant, &n(1), 5_000)
        .unwrap();
    g.check_and_record("a", Domain::Grant, &n(2), 5_000)
        .unwrap();
    assert_eq!(g.signer_usage("a"), 2);

    assert!(matches!(
        g.check_and_record("a", Domain::Grant, &n(3), 5_000),
        Err(ReplayStoreError::SignerQuotaExceeded { .. })
    ));

    // 첫 GC 가 기준선을 세운다 (last_seen_ms == 0 이므로 자르지 않는다)
    assert_eq!(g.gc(6_000), 2, "만료 항목이 지워지지 않았다");
    assert_eq!(
        g.signer_usage("a"),
        0,
        "★ 몫이 반환되지 않아 서명자가 영구히 막힌다"
    );

    assert_eq!(
        g.check_and_record("a", Domain::Grant, &n(3), 20_000)
            .unwrap(),
        ReplayDecision::Fresh
    );
}

/// ★ 시각이 **앞으로 튀어도** 캐시를 통째로 비우지 못한다.
///
/// 되감김만 막는 것으로는 부족하다는 독립 검수(2026-08-17) 지적:
///
/// ```text
/// 1. retain_until = 20_000 으로 기록
/// 2. GC 가 잘못된 미래 시각으로 호출된다 -> 지워진다
/// 3. 시각이 돌아오면 같은 메시지가 Fresh 가 된다  <- replay 창
/// ```
#[test]
fn gc_clamps_forward_clock_jumps() {
    // 실제 값에 맞춘다: 보존 시한은 now + (TTL 상한 15분 + skew 1분).
    let now = T;
    let retain = now + MAX_SHORTLIVED_TTL_MS + CLOCK_SKEW_TOLERANCE_MS;

    let mut g = InMemoryReplayGuard::with_capacities(10, 10);
    g.check_and_record("a", Domain::Grant, &n(1), retain)
        .unwrap();

    // 기준선을 세운다
    assert_eq!(g.gc(now), 0);
    assert_eq!(g.len(), 1);

    // ★ 상한을 훨씬 넘는 미래 시각 — 잘라내야 한다
    let removed = g.gc(now + MAX_GC_ADVANCE_MS * 100);
    assert_eq!(removed, 0, "★ 엉뚱한 미래 시각 한 번으로 캐시가 비었다");
    assert_eq!(g.clock_jumps(), 1, "앞으로 튄 것을 세지 않았다");

    // 그 nonce 는 여전히 막힌다 — replay 창이 열리지 않았다
    assert_eq!(
        g.check_and_record("a", Domain::Grant, &n(1), retain)
            .unwrap(),
        ReplayDecision::Duplicate,
        "★ replay 창이 열렸다"
    );

    // ★ 상한이 최대 보존 시한보다 짧아야 의미가 있다.
    //   길면 한 번의 잘못된 GC 가 여전히 전부 지운다 —
    //   자르는 시늉만 하고 아무것도 막지 못한다.
    assert!(
        MAX_GC_ADVANCE_MS < MAX_SHORTLIVED_TTL_MS + CLOCK_SKEW_TOLERANCE_MS,
        "★ GC 전진 상한({MAX_GC_ADVANCE_MS})이 최대 보존 시한보다 길다 — 방어가 무의미하다"
    );

    // 비공허성 — 상한 안의 정상적인 전진은 실제로 지운다
    let mut h = InMemoryReplayGuard::with_capacities(10, 10);
    h.check_and_record("a", Domain::Grant, &n(1), now + 10_000)
        .unwrap();
    assert_eq!(h.gc(now), 0);
    assert_eq!(h.gc(now + 20_000), 1, "정상 전진에서도 GC 가 안 돈다");
    assert_eq!(h.clock_jumps(), 0);
}

/// ★ 이 방어가 **막지 못하는 것**을 고정한다.
///
/// 지속적으로 틀린 시계는 막지 못한다. 잘못된 GC 를 여러 번 부르면
/// `last_seen_ms` 가 상한씩 전진해 결국 캐시가 빈다.
///
/// 이 테스트가 통과한다는 것은 **취약점이 남아 있다**는 뜻이다.
/// 통과를 성과로 읽지 않기 위해 이름과 주석에 남긴다.
#[test]
fn repeated_bogus_gc_still_drains_the_cache() {
    let now = T;
    let retain = now + MAX_SHORTLIVED_TTL_MS + CLOCK_SKEW_TOLERANCE_MS;
    let mut g = InMemoryReplayGuard::with_capacities(10, 10);
    g.check_and_record("a", Domain::Grant, &n(1), retain)
        .unwrap();
    assert_eq!(g.gc(now), 0);

    // 상한씩 전진하며 반복 호출
    let mut total = 0;
    for _ in 0..8 {
        total += g.gc(now + MAX_GC_ADVANCE_MS * 100);
    }
    assert_eq!(
        total, 1,
        "★ 반복 호출로 캐시가 비지 않았다면 단조 시계가 도입된 것이다 —          replay.rs 의 MAX_GC_ADVANCE_MS 문서와 이 테스트를 갱신하라"
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ §10 — 상한 도달 시 축출이 아니라 거부
// ══════════════════════════════════════════════════════════════════

#[test]
fn cache_full_never_evicts_unexpired_entries() {
    // ★ 서명자 몫을 전역과 같게 둬서 **전역 상한만** 검사한다.
    //   서명자 몫을 전역보다 크게 둬야 SignerQuotaExceeded 가 먼저 나지 않는다.
    let mut g = InMemoryReplayGuard::with_capacities(2, 10);
    rec(&mut g, "a", 1).unwrap();
    rec(&mut g, "a", 2).unwrap();

    assert_eq!(
        rec(&mut g, "a", 3).unwrap_err(),
        ReplayStoreError::CacheFull
    );
    assert_eq!(g.len(), 2, "거부하면서 항목을 지웠다");

    // ★ 기존 nonce 가 살아 있어야 한다 — 축출됐다면 Fresh 가 나온다
    assert_eq!(
        rec(&mut g, "a", 1).unwrap(),
        ReplayDecision::Duplicate,
        "★ 상한 도달 시 기존 유효 nonce 가 축출됐다 — replay 창이 열렸다"
    );
    assert_eq!(rec(&mut g, "a", 2).unwrap(), ReplayDecision::Duplicate);
}

#[test]
fn default_capacity_matches_spec() {
    assert_eq!(DEFAULT_CAPACITY, 100_000, "§10 의 기본 상한과 다르다");
}

// ══════════════════════════════════════════════════════════════════
// GC — 만료 기준으로만 지운다
// ══════════════════════════════════════════════════════════════════

#[test]
fn gc_removes_only_expired_entries() {
    let mut g = InMemoryReplayGuard::new();
    g.check_and_record("a", Domain::Grant, &n(1), T + 1_000)
        .unwrap();
    g.check_and_record("a", Domain::Grant, &n(2), T + 100_000)
        .unwrap();

    // 아직 둘 다 유효
    assert_eq!(g.gc(T), 0);
    assert_eq!(g.len(), 2);

    // 첫 번째만 만료
    assert_eq!(g.gc(T + 2_000), 1);
    assert_eq!(g.len(), 1);

    // 지워진 것은 다시 Fresh — 정상이다(만료됐으므로 재사용 불가 시한이 지났다)
    assert_eq!(rec(&mut g, "a", 1).unwrap(), ReplayDecision::Fresh);
    // 안 지워진 것은 여전히 Duplicate
    assert_eq!(
        g.check_and_record("a", Domain::Grant, &n(2), T + 100_000)
            .unwrap(),
        ReplayDecision::Duplicate
    );
}

/// ★★ 시계 되감김 — 검수자가 "확신 없음" 으로 남긴 부분.
///
/// ```text
/// 1. 시계가 앞으로 튄다 (NTP 보정 등)
/// 2. GC 가 "만료됐다" 며 항목을 지운다
/// 3. 시계가 다시 뒤로 돌아온다
/// 4. 지워진 nonce 가 "처음 보는 것" 이 된다  -> replay 창
/// ```
///
/// 되감김 중에는 **아무것도 지우지 않는다.** 캐시가 커지는 것은
/// `CacheFull` 로 드러나지만, 지워진 nonce 는 **조용히** 통과한다.
#[test]
fn gc_refuses_to_delete_while_clock_goes_backwards() {
    let mut g = InMemoryReplayGuard::new();
    g.check_and_record("a", Domain::Grant, &n(1), T + 1_000)
        .unwrap();

    // 시계가 1시간 앞으로 튄다 — 정상 GC
    assert_eq!(g.gc(T + 3_600_000), 1);
    assert_eq!(g.clock_rollbacks(), 0);

    // 새 항목
    g.check_and_record("a", Domain::Grant, &n(2), T + 3_700_000)
        .unwrap();

    // ★ 시계가 뒤로 간다 — 지우면 안 된다
    let removed = g.gc(T);
    assert_eq!(
        removed, 0,
        "★ 되감김 중에 GC 가 항목을 지웠다 — replay 창이 열린다"
    );
    assert_eq!(g.clock_rollbacks(), 1, "되감김을 세지 않았다");
    assert_eq!(g.len(), 1);

    // 그 nonce 는 여전히 Duplicate 여야 한다
    assert_eq!(
        g.check_and_record("a", Domain::Grant, &n(2), T + 3_700_000)
            .unwrap(),
        ReplayDecision::Duplicate,
        "★ 되감김 뒤 nonce 가 재사용 가능해졌다"
    );
}

/// 되감김이 끝나면 GC 가 다시 동작해야 한다 (영구 정지하면 캐시가 무한히 큰다).
#[test]
fn gc_resumes_after_clock_recovers() {
    let mut g = InMemoryReplayGuard::new();
    g.check_and_record("a", Domain::Grant, &n(1), T + 1_000)
        .unwrap();
    g.gc(T + 3_600_000); // 시각 기준을 올린다
    g.gc(T); // 되감김 — 무시
    assert_eq!(g.clock_rollbacks(), 1);

    // 다시 앞으로 — GC 가 동작한다
    g.check_and_record("a", Domain::Grant, &n(2), T + 3_700_000)
        .unwrap();
    assert_eq!(
        g.gc(T + 7_200_000),
        1,
        "되감김 뒤 GC 가 영구히 멈췄다 — 캐시가 무한히 커진다"
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 영속되지 않는다는 사실을 드러내는가
// ══════════════════════════════════════════════════════════════════

/// 프로세스 재시작을 흉내낸다 — 새 guard 는 아무것도 기억하지 못한다.
///
/// **이 테스트가 통과한다는 것이 곧 "재시작 직후 replay 창이 열린다" 는 뜻이다.**
/// 영속 저장소(3단계)가 생기기 전까지 운영에 쓰면 안 된다.
#[test]
fn restart_opens_a_replay_window() {
    let mut g1 = InMemoryReplayGuard::new();
    assert_eq!(rec(&mut g1, "a", 1).unwrap(), ReplayDecision::Fresh);
    assert_eq!(rec(&mut g1, "a", 1).unwrap(), ReplayDecision::Duplicate);
    assert!(!g1.is_durable(), "메모리 구현이 durable 이라고 주장한다");

    // 재시작
    let mut g2 = InMemoryReplayGuard::new();
    assert_eq!(
        rec(&mut g2, "a", 1).unwrap(),
        ReplayDecision::Fresh,
        "★ 이 테스트가 실패했다면 영속 저장소가 생겼다는 뜻이다 — \
         is_durable() 과 이 주석을 갱신하라"
    );
}

// ══════════════════════════════════════════════════════════════════
// verify() 와 붙였을 때 — 실패한 검증이 캐시를 채우지 않는가
// ══════════════════════════════════════════════════════════════════

/// 검수자 지목: "invalid signature 는 캐시를 채우지 않음",
/// "expired / clock-skew 메시지도 캐시를 채우지 않음".
///
/// 채우면 공격자가 **유효하지 않은 메시지로 정상 nonce 를 소진**시킬 수 있다.
#[test]
fn failed_verification_does_not_touch_the_guard() {
    use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
    use gputeer_protocol::constants::{CLOCK_SKEW_TOLERANCE_MS, GRANT_TTL_MS};
    use gputeer_protocol::pb;
    use gputeer_protocol::signing::verify;

    const COORD: &str = "01JBXR7Q0000000000000000CC";
    let k = SigningKey::from_bytes(&[1u8; 32]);
    let mut kr = InMemoryKeyring::new();
    kr.insert(COORD, k.verifying_key());
    let ring = Ed25519Verifier::new(kr);

    let mk = |nonce: u8| {
        let mut g = pb::ExecutionGrant {
            schema_version: 1,
            grant_id: "g".into(),
            coordinator_device_id: COORD.into(),
            issued_at_unix_ms: T,
            expires_at_unix_ms: T + GRANT_TTL_MS,
            nonce: n(nonce),
            ..Default::default()
        };
        g.coordinator_signature = sign(&k, &g).to_vec();
        g
    };

    let mut guard = InMemoryReplayGuard::new();

    // 서명 깨짐
    let mut bad = mk(1);
    bad.coordinator_signature = vec![0u8; 64];
    assert!(verify(&bad, 1, &ring, T, &mut guard).is_err());

    // 만료
    assert!(verify(&mk(2), 1, &ring, T + GRANT_TTL_MS, &mut guard).is_err());

    // skew 위반
    assert!(verify(
        &mk(3),
        1,
        &ring,
        T - CLOCK_SKEW_TOLERANCE_MS - 1,
        &mut guard
    )
    .is_err());

    assert!(
        guard.is_empty(),
        "★ 실패한 검증이 캐시를 채웠다 — 공격자가 정상 nonce 를 소진시킬 수 있다"
    );

    // 같은 nonce 로 정상 메시지를 보내면 통과해야 한다
    verify(&mk(1), 1, &ring, T, &mut guard).expect("실패한 검증이 nonce 를 소비했다");
    assert_eq!(guard.len(), 1);

    // 재전송은 거부
    assert!(verify(&mk(1), 1, &ring, T, &mut guard).is_err());
}
