//! Ed25519 서명·검증 — `signing.md` §8 검증 순서 · §9 시각 정책 · §13.2 타입 강제.
//!
//! # 이 테스트 파일의 관점
//!
//! **정상 경로 하나에 실패 경로 여럿.** `RULE.md` §6.
//! 서명 검증은 "통과시키는 일" 이 아니라 **"거부하는 일"** 이므로,
//! 거부해야 할 때 거부하는지가 본질이다.
//!
//! §8 의 각 단계가 **실제로 발동하는지** 하나씩 확인한다.
//! 발동하지 않는 단계는 없는 것과 같다.

use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
use gputeer_protocol::canonical::Domain;
use gputeer_protocol::constants::CLOCK_SKEW_TOLERANCE_MS;
use gputeer_protocol::pb;
use gputeer_protocol::signing::{ReplayStatus, 
    signing_input, verify, Lifetime, NoReplayCheck, ReplayDecision, ReplayGuard, ReplayStoreError,
    Signable, VerifyOutcome,
};

const NOW: u64 = 1_755_200_000_000;

// ── 테스트용 키 저장소 ────────────────────────────────────────────

type Ring = Ed25519Verifier<InMemoryKeyring>;

fn ring_with(id: &str, k: &SigningKey) -> Ring {
    let mut kr = InMemoryKeyring::new();
    kr.insert(id, k.verifying_key());
    Ed25519Verifier::new(kr)
}

fn empty_ring() -> Ring {
    Ed25519Verifier::new(InMemoryKeyring::new())
}

/// 결정론적 키. `rand` 를 쓰면 실패 재현이 어려워진다.
fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

const DEVICE: &str = "01JBXR7Q0000000000000000DD";

fn manifest() -> pb::JobManifest {
    pb::JobManifest {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        team_id: "01JBXR7Q0000000000000000TT".into(),
        entrypoint: "train.py".into(),
        submitter_device_id: DEVICE.into(),
        issued_at_unix_ms: NOW - 3_600_000, // 1시간 전 — 큐 대기는 정상이다
        expires_at_unix_ms: NOW + 7 * 24 * 3_600_000,
        network: Some(pb::NetworkPolicy {
            mediated_dns: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn signed_manifest(k: &SigningKey) -> pb::JobManifest {
    let mut m = manifest();
    m.submitter_signature = sign(k, &m).to_vec();
    m
}

// ══════════════════════════════════════════════════════════════════
// 정상 경로
// ══════════════════════════════════════════════════════════════════

#[test]
fn valid_manifest_verifies() {
    let k = key(1);
    let m = signed_manifest(&k);
    let v = verify(
        &m,
        1,
        &ring_with(DEVICE, &k),
        NOW,
        &mut NoReplayCheck,
    )
    .expect("정상 매니페스트가 검증을 통과해야 한다");

    assert_eq!(v.signer_id(), DEVICE);
    assert_eq!(v.get().job_id, "01JBXR7Q0000000000000000AA");
    // ★ 2026-08-17 정정 (독립 검수).
    //
    //   전에는 여기서 `assert!(v.replay_checked())` 를 했다 —
    //   "장수명은 replay 대상이 아니므로 검사된 것으로 본다" 는 이유였다.
    //   **그 테스트가 틀렸다.** JobManifest 는 만료 전까지 무제한 재전송된다.
    //   그것을 "검사됨" 으로 보고하면 부작용 게이트가 열린다.
    assert_eq!(
        v.replay_status(),
        ReplayStatus::NotApplicable,
        "장수명 메시지에 replay 방어가 있는 것처럼 보고하면 안 된다"
    );
    assert!(!v.replay_checked());
    assert_eq!(
        v.require_replay_checked().unwrap_err(),
        VerifyOutcome::Replay,
        "★ replay 방어가 없는 메시지로 부작용을 실행하게 두면 안 된다"
    );
    // 값 자체는 읽을 수 있다 — 부작용 없는 사용까지 막지는 않는다.
    assert_eq!(v.get().job_id, "01JBXR7Q0000000000000000AA");
}

/// 서명 필드가 canonical 에 들어가지 않으므로, 서명 전후의 sig_input 이 같아야 한다.
/// 다르면 **자기 자신도 검증하지 못한다.**
#[test]
fn signing_input_is_stable_before_and_after_signing() {
    let k = key(1);
    let unsigned = manifest();
    let before = signing_input(&unsigned);
    let after = signing_input(&signed_manifest(&k));
    assert_eq!(before, after, "서명 필드가 sig_input 에 영향을 줬다");
}

// ══════════════════════════════════════════════════════════════════
// §8-2  SCHEMA_TOO_NEW
// ══════════════════════════════════════════════════════════════════

#[test]
fn schema_too_new_is_rejected_and_not_reported_as_signature_failure() {
    let k = key(1);
    let mut m = manifest();
    m.schema_version = 2;
    m.submitter_signature = sign(&k, &m).to_vec(); // 서명 자체는 완전히 정상이다

    let out = verify(&m, 1, &ring_with(DEVICE, &k), NOW, &mut NoReplayCheck)
        .expect_err("구버전은 신버전 메시지를 거부해야 한다").outcome().unwrap();

    // ★ P0-08 의 결론 — 이것을 INVALID_SIGNATURE 로 보고하면 며칠 헤맨다
    assert_eq!(out, VerifyOutcome::SchemaTooNew);
    assert_ne!(out, VerifyOutcome::InvalidSignature);
    assert!(out.explain().contains("업그레이드"));
}

/// 버전 검사가 서명 검사보다 **먼저** 일어나는가.
///
/// 순서를 확인하는 방법: 서명을 일부러 망가뜨린 신버전 메시지를 넣는다.
/// 서명을 먼저 봤다면 `InvalidSignature`, 버전을 먼저 봤다면 `SchemaTooNew`.
#[test]
fn version_check_precedes_signature_check() {
    let k = key(1);
    let mut m = manifest();
    m.schema_version = 2;
    m.submitter_signature = vec![0u8; 64]; // 명백히 틀린 서명

    let out = verify(&m, 1, &ring_with(DEVICE, &k), NOW, &mut NoReplayCheck)
        .expect_err("거부되어야 한다").outcome().unwrap();
    assert_eq!(
        out,
        VerifyOutcome::SchemaTooNew,
        "서명을 먼저 봤다 — §8 순서 위반. 안전성은 같으나 진단이 틀린다"
    );
}

// ══════════════════════════════════════════════════════════════════
// §8-5  INVALID_SIGNATURE
// ══════════════════════════════════════════════════════════════════

#[test]
fn tampered_field_breaks_signature() {
    let k = key(1);
    let mut m = signed_manifest(&k);
    m.entrypoint = "evil.py".into(); // 서명 후 변조

    assert_eq!(
        verify(&m, 1, &ring_with(DEVICE, &k), NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::InvalidSignature
    );
}

/// ★ 보안 필드 변조 — DoD-03 이 서명 안으로 넣은 것들이 실제로 지켜지는가.
#[test]
fn tampering_security_fields_breaks_signature() {
    let k = key(1);
    let ring = ring_with(DEVICE, &k);

    let cases: Vec<(&str, Box<dyn Fn(&mut pb::JobManifest)>)> = vec![
        (
            "network(54): runtime_allow_hosts 추가",
            Box::new(|m: &mut pb::JobManifest| {
                m.network.as_mut().unwrap().runtime_allow_hosts.push("evil.example".into());
            }),
        ),
        (
            "network(54): mediated_dns 끄기",
            Box::new(|m: &mut pb::JobManifest| {
                m.network.as_mut().unwrap().mediated_dns = false;
            }),
        ),
        (
            "artifact_scope(55): 쓰기 범위 확대",
            Box::new(|m: &mut pb::JobManifest| {
                m.artifact_scope = Some(pb::ArtifactScope {
                    writable_prefixes: vec!["/".into()],
                    ..Default::default()
                });
            }),
        ),
        (
            "env(10): 실행 이미지 교체",
            Box::new(|m: &mut pb::JobManifest| {
                m.env = Some(pb::ExecutionEnvironment {
                    image_ref: "registry.evil/backdoor:1".into(),
                    ..Default::default()
                });
            }),
        ),
        (
            "minimum_security_tier(51) 낮추기",
            Box::new(|m: &mut pb::JobManifest| m.minimum_security_tier = 1),
        ),
        (
            "minimum_isolation_class(50) 낮추기",
            Box::new(|m: &mut pb::JobManifest| m.minimum_isolation_class = 1),
        ),
        (
            "side_effect_class(53) 바꾸기",
            Box::new(|m: &mut pb::JobManifest| m.side_effect_class = 2),
        ),
        (
            "expires_at(62) 연장",
            Box::new(|m: &mut pb::JobManifest| m.expires_at_unix_ms += 86_400_000),
        ),
        (
            "submitter_device_id(60) 바꿔치기",
            Box::new(|m: &mut pb::JobManifest| {
                m.submitter_device_id = "01JBXR7Q0000000000000000EE".into()
            }),
        ),
    ];

    for (desc, tamper) in &cases {
        let original = signed_manifest(&k);
        let mut m = original.clone();
        tamper(&mut m);

        // ★ 비공허성 — 변조하지 않는 변조 테스트는 통과해도 의미가 없다.
        //   실제로 이 실수를 했다: `minimum_security_tier = 0` 은 기준값이 이미 0이라
        //   아무것도 바꾸지 않았고, 테스트는 "통과할 리 없는데 통과" 했다.
        assert_ne!(
            m, original,
            "이 변조는 메시지를 바꾸지 않는다 — 테스트가 공허하다: {desc}"
        );

        let out = verify(&m, 1, &ring, NOW, &mut NoReplayCheck);
        // signer_id 를 바꾼 경우는 키 조회가 먼저 실패한다 (§8-6 이 §8-5 보다 앞이 아니라,
        // 키를 찾아야 서명을 검증할 수 있기 때문이다). 둘 다 "거부" 이므로 함께 허용한다.
        let err = out.unwrap_err().outcome().expect("프로토콜 결과여야 한다");
        assert!(
            matches!(
                err,
                VerifyOutcome::InvalidSignature | VerifyOutcome::UnknownSigner
            ),
            "변조가 검증을 통과했다: {desc} -> {err:?}"
        );
    }
    println!("보안 필드 변조 {}종 전부 거부됨", cases.len());
}

#[test]
fn wrong_length_signature_is_rejected() {
    let k = key(1);
    let ring = ring_with(DEVICE, &k);
    for len in [0usize, 1, 63, 65, 128] {
        let mut m = manifest();
        m.submitter_signature = vec![0u8; len];
        assert_eq!(
            verify(&m, 1, &ring, NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
            VerifyOutcome::InvalidSignature,
            "{len}바이트 서명이 거부되지 않았다"
        );
    }
}

// ══════════════════════════════════════════════════════════════════
// §8-1  WRONG_DOMAIN — 도메인 교차 재사용
// ══════════════════════════════════════════════════════════════════

/// ★ Lease 서명을 Manifest 서명으로 재사용할 수 없는가.
///
/// `Domain` 이 `sig_input` 에 들어가므로 **자동으로 막힌다.**
/// 결과는 `InvalidSignature` 다 — `WrongDomain` 을 별도로 반환할 필요가 없다.
/// (별도 반환은 도메인을 메시지에서 읽을 때만 의미가 있는데, 그렇게 하지 않는다)
#[test]
fn signature_from_another_domain_does_not_verify() {
    let k = key(1);

    // Lease 에 서명한다
    let mut lease = pb::Lease {
        schema_version: 1,
        lease_id: "01JBXLEASE0000000000000001".into(),
        issuing_coordinator_id: DEVICE.into(),
        issued_at_unix_ms: NOW - 1000,
        expires_at_unix_ms: NOW + 600_000,
        fence_epoch: 42,
        ..Default::default()
    };
    lease.coordinator_signature = sign(&k, &lease).to_vec();

    // 그 서명을 Manifest 에 붙인다
    let mut m = manifest();
    m.submitter_signature = lease.coordinator_signature.clone();

    assert_eq!(
        verify(&m, 1, &ring_with(DEVICE, &k), NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::InvalidSignature,
        "도메인 간 서명 재사용이 가능하다"
    );

    // Lease 자체는 정상 검증된다 (대조군 — 위 실패가 서명 자체의 문제가 아님을 보인다)
    verify(&lease, 1, &ring_with(DEVICE, &k), NOW, &mut NoReplayCheck)
        .expect("Lease 자체는 검증되어야 한다");
}

// ══════════════════════════════════════════════════════════════════
// §8-6  UNKNOWN_SIGNER
// ══════════════════════════════════════════════════════════════════

#[test]
fn unknown_signer_is_rejected() {
    let k = key(1);
    let m = signed_manifest(&k);
    // 키링이 비어 있다 = 팀 멤버가 아니거나 폐기됨
    assert_eq!(
        verify(&m, 1, &empty_ring(), NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::UnknownSigner
    );
}

/// 다른 사람의 키로 서명한 경우 — 서명자 ID 는 맞는데 키가 다르다.
#[test]
fn signature_by_different_key_is_rejected() {
    let attacker = key(9);
    let mut m = manifest();
    m.submitter_signature = sign(&attacker, &m).to_vec();

    // 키링은 진짜 소유자의 키를 갖고 있다
    assert_eq!(
        verify(&m, 1, &ring_with(DEVICE, &key(1)), NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::InvalidSignature
    );
}

// ══════════════════════════════════════════════════════════════════
// §8-7 · §9  시각 검증 — 장수명/단수명 분리
// ══════════════════════════════════════════════════════════════════

#[test]
fn expired_manifest_is_rejected() {
    let k = key(1);
    let m = signed_manifest(&k);
    let expiry = m.expires_at_unix_ms;
    let ring = ring_with(DEVICE, &k);

    // 만료 직전 통과
    verify(&m, 1, &ring, expiry - 1, &mut NoReplayCheck).expect("만료 1ms 전은 유효");
    // 만료 시점부터 거부
    assert_eq!(
        verify(&m, 1, &ring, expiry, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
        VerifyOutcome::Expired
    );
}

/// ★ **가장 중요한 회귀 방지 테스트.**
///
/// `signing.md` §9 — "이 구분을 지키지 않으면 **큐를 통과한 정상 Job 이 100% 거부된다.**"
/// Job 이 큐에서 수 시간 대기하는 것은 정상 동작이다(계획서 §13.6 aging queue).
#[test]
fn long_lived_manifest_ignores_clock_skew() {
    assert_eq!(pb::JobManifest::LIFETIME, Lifetime::LongLived);

    let k = key(1);
    let ring = ring_with(DEVICE, &k);
    let m = signed_manifest(&k); // issued_at = NOW - 1시간

    // 60초 skew 허용치를 훨씬 넘는 시간이 흘렀다
    for hours in [1u64, 6, 24, 24 * 6] {
        let now = m.issued_at_unix_ms + hours * 3_600_000;
        if now >= m.expires_at_unix_ms {
            continue;
        }
        verify(&m, 1, &ring, now, &mut NoReplayCheck).unwrap_or_else(|e| {
            panic!(
                "{hours}시간 큐 대기 후 매니페스트가 거부됐다: {e:?}. \
                 §9 의 장수명/단수명 분리가 깨졌다 — 정상 Job 이 전부 거부된다"
            )
        });
    }
}

/// 단수명 메시지에는 skew 규칙이 **적용되어야** 한다.
///
/// 현재 `Signable` 을 구현한 메시지가 전부 장수명이라, 단수명 경로가
/// 검증되지 않은 채 남는다. 테스트용 타입으로 그 경로를 검증한다.
mod short_lived {
    use super::*;
    use gputeer_protocol::canonical::{Fields, Value};

    #[derive(Clone, Debug)]
    struct Grant {
        schema_version: u32,
        issued_at: u64,
        expires_at: u64,
        signer: String,
        /// ★ nonce 는 **서명 대상 필드**다 (독립 검수 2026-08-16).
        ///   호출자가 넘기던 예전 설계는 replay 방어만 무력화되는 구멍이었다.
        nonce: Vec<u8>,
        sig: Vec<u8>,
    }

    impl Signable for Grant {
        const DOMAIN: Domain = Domain::Grant;
        const LIFETIME: Lifetime = Lifetime::ShortLived;

        fn schema_version(&self) -> u32 {
            self.schema_version
        }
        fn to_canonical_fields(&self) -> Fields {
            let mut f = Fields::new();
            f.set(1, Value::Uint(self.schema_version as u64));
            f.set(30, Value::Uint(self.issued_at));
            f.set(31, Value::Uint(self.expires_at));
            f.set(60, Value::Str(self.signer.clone()));
            // nonce 를 canonical 에 넣는다 — 서명 후 갈아끼울 수 없게 한다
            f.set(70, Value::Bytes(self.nonce.clone()));
            f
        }
        fn signature_bytes(&self) -> &[u8] {
            &self.sig
        }
        fn expires_at_unix_ms(&self) -> u64 {
            self.expires_at
        }
        fn issued_at_unix_ms(&self) -> u64 {
            self.issued_at
        }
        fn signer_id(&self) -> &str {
            &self.signer
        }
        fn replay_nonce(&self) -> Option<&[u8]> {
            Some(&self.nonce)
        }
    }

    fn nonce16() -> Vec<u8> {
        (0u8..16).collect()
    }

    fn grant_with_nonce(k: &SigningKey, nonce: Vec<u8>) -> Grant {
        let mut g = Grant {
            schema_version: 1,
            issued_at: NOW,
            expires_at: NOW + 60_000,
            signer: DEVICE.into(),
            nonce,
            sig: vec![],
        };
        g.sig = sign(k, &g).to_vec();
        g
    }

    fn grant(k: &SigningKey) -> Grant {
        grant_with_nonce(k, nonce16())
    }

    /// ★ 발견 — 기본 TTL(60초)과 skew 허용치(60초)가 같아서
    /// **미래 방향 skew 경계는 만료 검사에 가려진다.**
    ///
    /// `signing.md` §9 는 두 값을 모두 60초로 정한다. 그러면
    /// `now > issued_at + 60s` 인 시점은 이미 `now >= expires_at` 이므로
    /// `ClockSkew` 가 아니라 `Expired` 가 나온다.
    ///
    /// 안전성 문제는 아니다(둘 다 거부). 그러나 **미래 방향 skew 경로는
    /// 기본 TTL 에서 도달 불가능한 죽은 코드**이며, 그 사실을 모르면
    /// "skew 검사가 동작한다" 고 잘못 믿게 된다.
    /// 그래서 이 테스트는 TTL 을 길게 잡아 skew 경로를 **분리해서** 검증한다.
    #[test]
    fn short_lived_enforces_clock_skew() {
        let k = key(1);
        let ring = ring_with(DEVICE, &k);

        // TTL 을 길게 잡아 만료 검사와 skew 검사를 분리한다.
        //
        // ★ 2026-08-17 — 전에는 1시간(3_600_000)이었다.
        //   MAX_SHORTLIVED_TTL_MS(15분)가 생기면서 그 값 자체가 거부된다.
        //   10분은 상한 안이면서 skew 허용치(60초)보다 충분히 길다.
        let mut g = grant(&k);
        g.expires_at = NOW + 600_000;
        g.sig = sign(&k, &g).to_vec();

        // 경계값 — 허용
        verify(&g, 1, &ring, NOW + CLOCK_SKEW_TOLERANCE_MS, &mut NoReplayCheck)
            .expect("skew 경계값은 허용된다");

        // 경계 바로 밖 — 거부
        assert_eq!(
            verify(&g, 1, &ring, NOW + CLOCK_SKEW_TOLERANCE_MS + 1, &mut NoReplayCheck)
                .unwrap_err().outcome().unwrap(),
            VerifyOutcome::ClockSkew
        );

        // 과거 방향도 대칭으로 거부 (검증자 시계가 빠른 경우)
        assert_eq!(
            verify(&g, 1, &ring, NOW - CLOCK_SKEW_TOLERANCE_MS - 1, &mut NoReplayCheck)
                .unwrap_err().outcome().unwrap(),
            VerifyOutcome::ClockSkew
        );
    }

    /// 위 발견을 명시적으로 고정한다 — 기본 TTL 에서는 `Expired` 가 먼저 나온다.
    ///
    /// §9 표의 TTL 을 바꾸면 이 테스트가 실패해 그 사실을 알린다.
    #[test]
    fn default_ttl_masks_forward_skew_with_expiry() {
        use gputeer_protocol::constants::GRANT_TTL_MS;
        assert_eq!(
            GRANT_TTL_MS, CLOCK_SKEW_TOLERANCE_MS,
            "§9 의 Grant TTL 과 skew 허용치가 더 이상 같지 않다 — 아래 단언을 재검토하라"
        );

        let k = key(1);
        let ring = ring_with(DEVICE, &k);
        let g = grant(&k); // expires_at = NOW + 60_000

        // skew 경계와 만료 시각이 같은 지점. 만료가 이긴다.
        assert_eq!(
            verify(&g, 1, &ring, NOW + CLOCK_SKEW_TOLERANCE_MS, &mut NoReplayCheck)
                .unwrap_err().outcome().unwrap(),
            VerifyOutcome::Expired,
            "기본 TTL 에서는 미래 방향 skew 경로에 도달할 수 없다"
        );

        // 과거 방향은 여전히 도달 가능하다
        assert_eq!(
            verify(&g, 1, &ring, NOW - CLOCK_SKEW_TOLERANCE_MS - 1, &mut NoReplayCheck)
                .unwrap_err().outcome().unwrap(),
            VerifyOutcome::ClockSkew
        );
    }

    /// §10 — nonce 는 CSPRNG **16바이트**여야 한다(MUST).
    #[test]
    fn short_lived_requires_16_byte_nonce() {
        let k = key(1);
        let ring = ring_with(DEVICE, &k);
        let g = grant(&k);

        let _ = &g;
        // ★ nonce 는 메시지에서 오므로, 길이를 바꾸려면 **메시지를 바꿔 다시 서명**해야 한다.
        //   호출자가 임의로 넘길 수 없다는 것이 이 설계의 핵심이다.
        for len in [0usize, 8, 15, 17, 32] {
            let bad = grant_with_nonce(&k, vec![9u8; len]);
            assert_eq!(
                verify(&bad, 1, &ring, NOW, &mut NoReplayCheck).unwrap_err().outcome().unwrap(),
                VerifyOutcome::Replay,
                "{len}바이트 nonce 가 허용됐다 (§10 은 16바이트 MUST)"
            );
        }
    }

    /// ★ replay 미검사 상태가 값에 따라다니는가.
    ///
    /// `NoReplayCheck` 로 검증하면 `Verified` 는 통과하지만
    /// `require_replay_checked()` 는 거부해야 한다.
    /// **부작용 있는 동작이 replay 미검사 상태로 실행되면 안 된다.**
    #[test]
    fn no_replay_check_taints_the_verified_value() {
        let k = key(1);
        let g = grant(&k);
        let v = verify(&g, 1, &ring_with(DEVICE, &k), NOW, &mut NoReplayCheck)
            .expect("서명 자체는 정상이다");

        assert!(!v.replay_checked(), "NoReplayCheck 인데 검사됨으로 표시됐다");
        assert_eq!(
            v.require_replay_checked().unwrap_err(),
            VerifyOutcome::Replay,
            "replay 미검사 값이 부작용 경로에 흘러갈 수 있다"
        );
        // get() 은 여전히 읽을 수 있다 — 로깅·표시 용도
        assert_eq!(v.get().signer, DEVICE);
    }

    /// 실제로 동작하는 guard 를 끼우면 재사용이 막히는가.
    #[test]
    fn working_replay_guard_rejects_reuse() {
        use std::collections::HashSet;

        struct MemGuard(HashSet<(String, u32, Vec<u8>)>);
        impl ReplayGuard for MemGuard {
            fn check_and_record(&mut self, s: &str, d: Domain, n: &[u8], _r: u64)
                -> Result<ReplayDecision, ReplayStoreError> {
                Ok(if self.0.insert((s.to_string(), d as u32, n.to_vec())) {
                    ReplayDecision::Fresh
                } else {
                    ReplayDecision::Duplicate
                })
            }
            fn is_effective(&self) -> bool {
                true
            }
        }

        let k = key(1);
        let ring = ring_with(DEVICE, &k);
        let g = grant(&k);
        let mut guard = MemGuard(HashSet::new());

        let v = verify(&g, 1, &ring, NOW, &mut guard).expect("첫 번째는 통과");
        assert!(v.replay_checked());
        assert!(v.require_replay_checked().is_ok());

        assert_eq!(
            verify(&g, 1, &ring, NOW, &mut guard).unwrap_err().outcome().unwrap(),
            VerifyOutcome::Replay,
            "같은 nonce 재사용이 통과했다"
        );

        // 다른 nonce 면 통과 — guard 가 무조건 거부하는 게 아님을 확인 (비공허성)
        let mut other = nonce16();
        other[0] = 0xFF;
        let g2 = grant_with_nonce(&k, other);
        verify(&g2, 1, &ring, NOW, &mut guard).expect("다른 nonce 는 통과해야 한다");
    }

    /// §10 — nonce 는 **device 별 namespace** 를 가져야 한다.
    /// 그렇지 않으면 한 device 가 다른 device 의 nonce 공간을 소진시킬 수 있다.
    #[test]
    fn replay_key_is_namespaced_by_device() {
        use std::collections::HashSet;
        struct MemGuard(HashSet<(String, u32, Vec<u8>)>);
        impl ReplayGuard for MemGuard {
            fn check_and_record(&mut self, s: &str, d: Domain, n: &[u8], _r: u64)
                -> Result<ReplayDecision, ReplayStoreError> {
                Ok(if self.0.insert((s.to_string(), d as u32, n.to_vec())) {
                    ReplayDecision::Fresh
                } else {
                    ReplayDecision::Duplicate
                })
            }
            fn is_effective(&self) -> bool {
                true
            }
        }

        let k1 = key(1);
        let k2 = key(2);
        const OTHER: &str = "01JBXR7Q0000000000000000EE";

        let g1 = grant(&k1);
        let mut g2 = Grant {
            signer: OTHER.into(),
            sig: vec![],
            ..g1.clone()
        };
        g2.sig = sign(&k2, &g2).to_vec(); // nonce 는 g1 과 같다 — namespace 검증이 목적

        let mut kr = InMemoryKeyring::new();
        kr.insert(DEVICE, k1.verifying_key());
        kr.insert(OTHER, k2.verifying_key());
        let ring = Ed25519Verifier::new(kr);
        let mut guard = MemGuard(HashSet::new());

        verify(&g1, 1, &ring, NOW, &mut guard).expect("device A");
        // 같은 nonce 값이지만 다른 device — 통과해야 한다
        verify(&g2, 1, &ring, NOW, &mut guard)
            .expect("device 별 namespace 가 없어 다른 device 의 nonce 가 충돌했다");
    }
}

// ══════════════════════════════════════════════════════════════════
// §13.2 타입 강제 — 우회 경로가 없는가
// ══════════════════════════════════════════════════════════════════

/// `Verified<M>` 의 우회 생성 차단은 **`src/signing.rs` 의 `compile_fail` 독테스트**가
/// 검증한다. 통합 테스트 파일의 독테스트는 실행되지 않으므로 여기 두면 안 된다.
///
/// 여기서는 `VerifyOutcome` 의 표면을 확인한다.
#[test]
fn verify_outcome_surface_is_complete() {
    for o in [
        VerifyOutcome::InvalidSignature,
        VerifyOutcome::UnknownSigner,
        VerifyOutcome::Expired,
        VerifyOutcome::ClockSkew,
        VerifyOutcome::Replay,
        VerifyOutcome::WrongDomain,
        VerifyOutcome::SchemaTooNew,
    ] {
        assert!(!o.explain().is_empty(), "{o:?} 의 설명이 비었다");
        assert!(o.proto_value() >= 2, "{o:?} 의 proto 값이 잘못됐다");
    }
}

/// `VerifyOutcome` 이 `common.proto` 의 값과 어긋나지 않는가.
#[test]
fn verify_outcome_matches_proto_enum() {
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../proto/common.proto"),
    )
    .expect("common.proto");

    let expect = [
        (VerifyOutcome::InvalidSignature, "VERIFY_OUTCOME_INVALID_SIGNATURE"),
        (VerifyOutcome::UnknownSigner, "VERIFY_OUTCOME_UNKNOWN_SIGNER"),
        (VerifyOutcome::Expired, "VERIFY_OUTCOME_EXPIRED"),
        (VerifyOutcome::ClockSkew, "VERIFY_OUTCOME_CLOCK_SKEW"),
        (VerifyOutcome::Replay, "VERIFY_OUTCOME_REPLAY"),
        (VerifyOutcome::WrongDomain, "VERIFY_OUTCOME_WRONG_DOMAIN"),
        (VerifyOutcome::SchemaTooNew, "VERIFY_OUTCOME_SCHEMA_TOO_NEW"),
    ];

    for (outcome, name) in expect {
        let needle = format!("{name} = {};", outcome.proto_value());
        assert!(
            src.contains(&needle),
            "common.proto 에 `{needle}` 가 없다 — Rust 열거형과 proto 가 어긋났다"
        );
    }
}
