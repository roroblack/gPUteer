//! `verify()` 의 관문이 **실제로 발동하는가** — 뮤테이션 감사 후속 (2026-09-06).
//!
//! # 이 파일이 생긴 이유
//!
//! `crates/protocol` 과 `crates/crypto` 의 검증 관문을 하나씩 무력화하고
//! `cargo test -p gputeer-protocol -p gputeer-crypto` 를 돌렸다.
//! 66개 중 56개는 테스트가 잡았다. 여기 있는 것들은 **잡히지 않은 것들**이다.
//!
//! ```text
//! 살아남은 뮤테이션                                       이 파일의 테스트
//! ────────────────────────────────────────────────────────────────────────
//! 서명 길이 검사를 0 패딩으로 바꾼다                       a_valid_signature_with_a_trailing_byte_is_rejected
//! 단수명인데 nonce 가 없는 메시지를 통과시킨다             a_short_lived_message_without_a_nonce_is_rejected
//! 단수명 TTL 상한(MAX_SHORTLIVED_TTL_MS) 검사를 없앤다     an_over_long_short_lived_ttl_is_refused_by_policy
//! max_supported 를 SCHEMA_VERSION 으로 클램프하지 않는다   a_caller_cannot_switch_off_the_schema_gate
//! 아직 유효기간이 시작되지 않은 키를 유효로 본다           a_key_whose_validity_has_not_started_is_not_accepted
//! ```
//!
//! 각각 "왜 기존 테스트가 못 잡았는가" 를 테스트 위에 적어 둔다 —
//! 같은 종류의 공백을 다시 만들지 않기 위해서다.

use gputeer_crypto::{
    sign, Ed25519Verifier, InMemoryKeyring, KeyProtection, PersistentKeyring, PlaintextPolicy,
    SecretSigningKey, SigningKey,
};
use gputeer_protocol::canonical::{Domain, Fields, Value};
use gputeer_protocol::constants::{MAX_SHORTLIVED_TTL_MS, SCHEMA_VERSION};
use gputeer_protocol::pb;
use gputeer_protocol::signing::{
    verify, Lifetime, NoReplayCheck, PolicyViolation, Signable, VerifyError, VerifyOutcome,
};
use tempfile::tempdir;

const NOW: u64 = 1_755_200_000_000;
const DEVICE: &str = "01JBXR7Q0000000000000000DD";

type Ring = Ed25519Verifier<InMemoryKeyring>;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn ring_with(id: &str, k: &SigningKey) -> Ring {
    let mut kr = InMemoryKeyring::new();
    kr.insert(id, k.verifying_key());
    Ed25519Verifier::new(kr)
}

fn manifest() -> pb::JobManifest {
    pb::JobManifest {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        team_id: "01JBXR7Q0000000000000000TT".into(),
        entrypoint: "train.py".into(),
        submitter_device_id: DEVICE.into(),
        issued_at_unix_ms: NOW - 3_600_000,
        expires_at_unix_ms: NOW + 7 * 24 * 3_600_000,
        ..Default::default()
    }
}

fn signed_manifest(k: &SigningKey) -> pb::JobManifest {
    let mut m = manifest();
    m.submitter_signature = sign(k, &m).to_vec();
    m
}

// ══════════════════════════════════════════════════════════════════
// §8-5 — 서명 **길이** 검사
//
// ★ 기존 `wrong_length_signature_is_rejected` 는 길이가 틀린 서명을
//   **전부 0으로** 채웠다. 그래서 길이 검사를 지우고 64바이트로 0 패딩
//   하도록 바꿔도 Ed25519 가 어차피 실패해 테스트가 통과했다 —
//   테스트가 잰 것은 "길이 검사" 가 아니라 "0 서명은 안 맞는다" 였다.
//
//   구멍은 **유효한 서명 뒤에 바이트를 붙이는** 경우다. 앞 64바이트가
//   진짜이므로 패딩/절단 구현은 그것을 통과시킨다.
// ══════════════════════════════════════════════════════════════════

#[test]
fn a_valid_signature_with_a_trailing_byte_is_rejected() {
    let k = key(1);
    let ring = ring_with(DEVICE, &k);
    let good = signed_manifest(&k);

    // 대조군 — 손대지 않은 서명은 통과한다 (아래 실패가 길이 때문임을 보인다).
    verify(&good, 1, &ring, NOW, &mut NoReplayCheck).expect("원본 서명은 검증되어야 한다");

    for extra in [1usize, 2, 8, 64] {
        let mut m = good.clone();
        m.submitter_signature.extend(std::iter::repeat(0u8).take(extra));
        assert_eq!(
            verify(&m, 1, &ring, NOW, &mut NoReplayCheck)
                .unwrap_err()
                .outcome()
                .unwrap(),
            VerifyOutcome::InvalidSignature,
            "유효한 서명 뒤에 {extra}바이트를 붙였는데 통과했다 — \
             서명 길이가 고정 64바이트가 아니면 같은 메시지에 여러 서명 표현이 생긴다"
        );
    }

    // 앞을 잘라낸 경우도 마찬가지다 — 0 으로 되채우는 구현이 통과시키면 안 된다.
    let mut truncated = good.clone();
    truncated.submitter_signature.truncate(63);
    assert_eq!(
        verify(&truncated, 1, &ring, NOW, &mut NoReplayCheck)
            .unwrap_err()
            .outcome()
            .unwrap(),
        VerifyOutcome::InvalidSignature
    );
}

// ══════════════════════════════════════════════════════════════════
// §8-8 · §10 — 단수명 메시지는 **서명된 nonce 를 반드시 가져야 한다**
//
// ★ `Signable::replay_nonce()` 의 기본 구현은 `None` 이다. 즉 새 단수명
//   메시지를 추가하면서 이 메서드를 **덮어쓰지 않으면 컴파일은 된다.**
//   `verify()` 는 그 경우 `Replay` 로 거부하는데, 그 경로를 타는 테스트가
//   하나도 없었다 — 지금 존재하는 단수명 메시지는 전부 `Some` 을 주기
//   때문이다. 실제 위험은 **다음에 추가될 메시지**다.
// ══════════════════════════════════════════════════════════════════

/// 단수명 테스트 메시지. `nonce` 가 `None` 이면 `replay_nonce()` 도 `None` 이다.
#[derive(Clone, Debug)]
struct ShortLivedProbe {
    issued_at: u64,
    expires_at: u64,
    nonce: Option<Vec<u8>>,
    sig: Vec<u8>,
}

impl ShortLivedProbe {
    fn new(issued_at: u64, expires_at: u64, nonce: Option<Vec<u8>>) -> Self {
        Self {
            issued_at,
            expires_at,
            nonce,
            sig: Vec::new(),
        }
    }

    fn signed(mut self, k: &SigningKey) -> Self {
        self.sig = sign(k, &self).to_vec();
        self
    }
}

impl Signable for ShortLivedProbe {
    const DOMAIN: Domain = Domain::Grant;
    const LIFETIME: Lifetime = Lifetime::ShortLived;

    fn schema_version(&self) -> u32 {
        1
    }
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        f.set(1, Value::Uint(1));
        f.set(30, Value::Uint(self.issued_at));
        f.set(31, Value::Uint(self.expires_at));
        f.set(60, Value::Str(DEVICE.to_string()));
        // nonce 는 서명 대상이다 — 서명 뒤에 갈아끼울 수 없어야 한다.
        if let Some(n) = &self.nonce {
            f.set(70, Value::Bytes(n.clone()));
        }
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
        DEVICE
    }
    /// ★ `nonce` 가 `None` 이면 기본 구현과 같은 값을 낸다 — 그 경우가 이 절의 표적이다.
    fn replay_nonce(&self) -> Option<&[u8]> {
        self.nonce.as_deref()
    }
}

fn nonce16() -> Vec<u8> {
    (0u8..16).collect()
}

#[test]
fn a_short_lived_message_without_a_nonce_is_rejected() {
    let k = key(1);
    let ring = ring_with(DEVICE, &k);

    let without = ShortLivedProbe::new(NOW, NOW + 60_000, None).signed(&k);
    assert_eq!(
        verify(&without, 1, &ring, NOW, &mut NoReplayCheck)
            .unwrap_err()
            .outcome()
            .unwrap(),
        VerifyOutcome::Replay,
        "★ nonce 가 없는 단수명 메시지가 통과했다 — replay 캐시에 넣을 키가 없으므로 \
         그 메시지는 만료 전까지 무제한 재전송된다"
    );

    // 비공허성 — nonce 만 채우면 같은 메시지가 통과한다.
    // 이것이 없으면 위 실패가 서명·시각 때문일 수도 있다.
    let with = ShortLivedProbe::new(NOW, NOW + 60_000, Some(nonce16())).signed(&k);
    verify(&with, 1, &ring, NOW, &mut NoReplayCheck)
        .expect("nonce 만 채운 같은 메시지는 통과해야 한다");
}

// ══════════════════════════════════════════════════════════════════
// §10 — 단수명 TTL 상한 (로컬 정책)
//
// ★ `retain_until = expires_at + skew` 이므로 `expires_at` 이 먼 미래면
//   그 nonce 는 영원히 GC 되지 않는다. capacity 만큼 만들면 캐시가 영구히
//   차서 **다른 모든 검증이 CacheFull 로 거부된다.**
//   2026-08-17 독립 검수가 넣은 방어인데 고정 테스트가 없었다.
// ══════════════════════════════════════════════════════════════════

#[test]
fn an_over_long_short_lived_ttl_is_refused_by_policy() {
    let k = key(1);
    let ring = ring_with(DEVICE, &k);

    let too_long = ShortLivedProbe::new(
        NOW,
        NOW + MAX_SHORTLIVED_TTL_MS + 1,
        Some(nonce16()),
    )
    .signed(&k);

    match verify(&too_long, 1, &ring, NOW, &mut NoReplayCheck).unwrap_err() {
        VerifyError::Policy(PolicyViolation::ShortLivedTtlTooLong { ttl_ms, max_ms }) => {
            assert_eq!(ttl_ms, MAX_SHORTLIVED_TTL_MS + 1);
            assert_eq!(max_ms, MAX_SHORTLIVED_TTL_MS);
        }
        other => panic!(
            "★ TTL 상한을 넘은 단수명 메시지가 정책 위반으로 거부되지 않았다: {other:?}"
        ),
    }

    // ★ 이것은 프로토콜 결과가 아니라 **로컬 정책**이다 — 상대에게 보고할
    //   VerifyOutcome 이 없다는 사실도 함께 고정한다 (CLAUDE.md §3).
    assert!(
        verify(&too_long, 1, &ring, NOW, &mut NoReplayCheck)
            .unwrap_err()
            .outcome()
            .is_none(),
        "로컬 정책 위반이 프로토콜 결과로 보고되면 원인을 상대 쪽에서 찾게 된다"
    );

    // 경계 — 상한 **정각**은 허용된다. 이것이 없으면 "전부 거부" 하는
    // 구현과 구분되지 않는다.
    let at_limit =
        ShortLivedProbe::new(NOW, NOW + MAX_SHORTLIVED_TTL_MS, Some(nonce16())).signed(&k);
    verify(&at_limit, 1, &ring, NOW, &mut NoReplayCheck)
        .expect("TTL 상한 정각은 허용되어야 한다");
}

// ══════════════════════════════════════════════════════════════════
// §7.2 — 호출자가 `SCHEMA_TOO_NEW` 방어를 끌 수 있으면 안 된다
//
// ★ `verify()` 는 `max_supported_schema_version` 을 이 빌드가 아는
//   `SCHEMA_VERSION` 으로 클램프한다. 그 코드에는 "호출자가 u32::MAX 를
//   넘기면 SCHEMA_TOO_NEW 가 영영 안 나온다" 는 주석까지 달려 있는데,
//   **그 방어를 지워도 아무 테스트도 깨지지 않았다.**
//   범위를 벗어난 값을 넘기는 테스트가 하나도 없었기 때문이다.
//
// ★ 방어는 두 겹이고 **빌드 프로파일마다 발동하는 쪽이 다르다.**
//
//   ```text
//   debug   (cargo test)            debug_assert! 가 먼저 패닉한다
//   release (cargo test --release)  debug_assert 가 사라지고 클램프가 막는다
//   ```
//
//   그래서 테스트도 두 개다. 한쪽만 두면 다른 프로파일에서는 아무것도
//   재지 못한다 — `CLAUDE.md` §4, "한 플랫폼 통과를 다른 플랫폼 통과로
//   세지 않는다" 와 같은 이유다.
// ══════════════════════════════════════════════════════════════════

fn too_new_manifest(k: &SigningKey) -> pb::JobManifest {
    let mut m = manifest();
    m.schema_version = SCHEMA_VERSION + 1;
    m.submitter_signature = sign(k, &m).to_vec();
    m
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "SCHEMA_VERSION")]
fn a_caller_cannot_switch_off_the_schema_gate() {
    let k = key(1);
    let m = too_new_manifest(&k);
    // 이 빌드가 아는 것보다 큰 값을 넘긴다 — 받아들이면 안 된다.
    let _ = verify(
        &m,
        u32::MAX,
        &ring_with(DEVICE, &k),
        NOW,
        &mut NoReplayCheck,
    );
}

#[cfg(not(debug_assertions))]
#[test]
fn a_caller_cannot_switch_off_the_schema_gate() {
    let k = key(1);
    let m = too_new_manifest(&k);
    assert_eq!(
        verify(
            &m,
            u32::MAX,
            &ring_with(DEVICE, &k),
            NOW,
            &mut NoReplayCheck
        )
        .unwrap_err()
        .outcome()
        .unwrap(),
        VerifyOutcome::SchemaTooNew,
        "★ 호출자가 넘긴 u32::MAX 가 SCHEMA_TOO_NEW 방어를 통째로 껐다"
    );
}

/// 정상 범위에서는 방어가 그대로 동작한다 (위 두 테스트의 대조군).
#[test]
fn the_schema_gate_still_fires_within_range() {
    let k = key(1);
    let m = too_new_manifest(&k);
    assert_eq!(
        verify(
            &m,
            SCHEMA_VERSION,
            &ring_with(DEVICE, &k),
            NOW,
            &mut NoReplayCheck
        )
        .unwrap_err()
        .outcome()
        .unwrap(),
        VerifyOutcome::SchemaTooNew
    );
}

// ══════════════════════════════════════════════════════════════════
// §11 — 아직 유효기간이 **시작되지 않은** 키는 쓰지 않는다
//
// ★ `version_is_valid()` 의 세 조건 중 `state == Active` 와 `valid_until`
//   은 테스트가 잡았는데 `valid_from <= now` 만 아무도 재지 않았다.
//   회전으로 등록된 새 키는 `valid_from = 회전 시각` 이므로, 이 조건이
//   없으면 **회전 전에 만들어 둔 서명이 회전 전 시점에도 통과한다.**
// ══════════════════════════════════════════════════════════════════

#[test]
fn a_key_whose_validity_has_not_started_is_not_accepted() {
    let old = key(1);
    let new = key(2);
    let rotation_at = NOW + 10_000;

    let mut keyring = PersistentKeyring::new(
        tempdir().unwrap().path().join("keys.bin"),
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .unwrap();
    keyring
        .insert_private(DEVICE, SecretSigningKey::from_signing_key(old.clone()))
        .unwrap();
    keyring
        .rotate(
            DEVICE,
            SecretSigningKey::from_signing_key(new.clone()),
            rotation_at,
        )
        .unwrap();

    // 새 키로 서명한 메시지를 **회전 1ms 전** 시점에 검증한다.
    let mut m = manifest();
    m.submitter_signature = sign(&new, &m).to_vec();

    // ★ 결과가 `UnknownSigner` 인 것이 옳다 — `InvalidSignature` 가 아니다.
    //   아직 유효하지 않은 키는 `lookup_candidates` 에서 빠지고
    //   `lookup_retired` 에서 잡히므로, "서명이 위조됐다" 가 아니라
    //   "그 키는 지금 쓸 수 없다" 로 보고된다 (`CLAUDE.md` §3).
    //   `signer_id` 는 알려진 값이므로 사람이 읽기에는 여전히 덜 정확하지만,
    //   `VerifyOutcome` 에 "아직 유효하지 않은 키" 값이 없다는 것이
    //   `crates/crypto/src/lib.rs` 에 기록된 알려진 공백이다.
    assert_eq!(
        verify(
            &m,
            1,
            &Ed25519Verifier::new(keyring.at(rotation_at - 1)),
            rotation_at - 1,
            &mut NoReplayCheck
        )
        .unwrap_err()
        .outcome()
        .unwrap(),
        VerifyOutcome::UnknownSigner,
        "★ 아직 유효기간이 시작되지 않은 키로 검증이 통과했다"
    );

    // 비공허성 — 회전 시점부터는 같은 메시지가 통과한다.
    verify(
        &m,
        1,
        &Ed25519Verifier::new(keyring.at(rotation_at)),
        rotation_at,
        &mut NoReplayCheck,
    )
    .expect("회전 시점부터는 새 키가 유효해야 한다");
}
