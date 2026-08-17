//! gPUteer 암호 계층 — Ed25519 · 키 보관 · nonce 캐시.
//!
//! 이 크레이트는 Crypto 스트림이 소유한다 (`RULE.md` §4.1 · `docs/contracts/01_스트림_소유권.md`).
//!
//! # 경계
//!
//! ```text
//! crates/protocol   무엇을 서명하는가 · 어떤 순서로 검증하는가 (signing.md §3·§8)
//!                   -> Signable · SignatureVerifier · Verified<M> · VerifyOutcome
//!
//! crates/crypto     그 서명을 실제로 만들고 확인하는 법
//!                   -> Ed25519Verifier · sign()
//! ```
//!
//! **`crates/protocol` 은 암호 라이브러리에 의존하지 않는다.**
//! 그래야 서명 알고리즘을 바꿔도 프로토콜 규범이 흔들리지 않는다.
//!
//! # 아직 하지 않은 것
//!
//! `signing.md` §11(키 보관 K0~K2)은 미구현이다.
//! [`InMemoryKeyring`] 과 [`InMemoryReplayGuard`] 는 **테스트와 초기 통합용**이며
//! 운영에 쓰면 안 된다 — 둘 다 프로세스가 죽으면 사라진다.

pub mod replay;

pub use replay::{InMemoryReplayGuard, DEFAULT_CAPACITY};

use std::collections::HashMap;

use ed25519_dalek::{Signature, Signer, Verifier};

// 재수출 — 호출부가 ed25519-dalek 을 직접 의존하지 않아도 되게 한다.
pub use ed25519_dalek::{SigningKey, VerifyingKey};

use gputeer_protocol::signing::{signing_input, Signable, SignatureVerifier, VerifyOutcome};

/// 메시지에 서명한다. 반환값을 서명 필드(90)에 넣는다.
///
/// ★ `signing_input` 은 `crates/protocol` 것을 쓴다.
/// 서명 경로와 검증 경로가 갈라지면 **자기 자신과만 맞는 서명**이 만들어진다.
pub fn sign<M: Signable + ?Sized>(key: &SigningKey, msg: &M) -> [u8; 64] {
    key.sign(&signing_input(msg)).to_bytes()
}

/// `signer_id` 로 공개키를 찾는다.
///
/// **팀 멤버십 · 폐기 · quarantine 여부까지 여기서 판단한다.**
/// 키를 안다는 것과 그 키가 **지금** 유효하다는 것은 다르다.
/// `None` 을 반환하면 `UnknownSigner` 가 된다.
pub trait KeyDirectory {
    fn lookup(&self, signer_id: &str) -> Option<VerifyingKey>;
}

/// `signing.md` §8-5 · §8-6 의 Ed25519 구현.
pub struct Ed25519Verifier<D: KeyDirectory> {
    directory: D,
}

impl<D: KeyDirectory> Ed25519Verifier<D> {
    pub fn new(directory: D) -> Self {
        Self { directory }
    }
}

impl<D: KeyDirectory> SignatureVerifier for Ed25519Verifier<D> {
    fn verify_signature(
        &self,
        signer_id: &str,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), VerifyOutcome> {
        // 길이가 틀린 서명은 키를 찾기 전에 거른다.
        // 길이 오류를 UnknownSigner 로 보고하면 원인을 엉뚱한 곳에서 찾게 된다.
        let sig_array: [u8; 64] = signature
            .try_into()
            .map_err(|_| VerifyOutcome::InvalidSignature)?;

        let key = self
            .directory
            .lookup(signer_id)
            .ok_or(VerifyOutcome::UnknownSigner)?;

        key.verify(message, &Signature::from_bytes(&sig_array))
            .map_err(|_| VerifyOutcome::InvalidSignature)
    }
}

/// ★ **메모리 키링 — 운영에 쓰지 않는다.**
///
/// `signing.md` §11 이 요구하는 키 보관 등급(K0~K2)을 만족하지 않는다.
/// 프로세스가 죽으면 사라지고, 폐기·회전 개념이 없다.
///
/// **이름이 사실을 말한다.** `DefaultKeyring` 같은 이름을 쓰면
/// 누군가 운영에 그대로 쓴다.
#[derive(Debug, Default, Clone)]
pub struct InMemoryKeyring(HashMap<String, VerifyingKey>);

impl InMemoryKeyring {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, signer_id: impl Into<String>, key: VerifyingKey) -> &mut Self {
        self.0.insert(signer_id.into(), key);
        self
    }

    /// 키를 폐기한다. 이후 그 서명자는 `UnknownSigner` 가 된다.
    pub fn revoke(&mut self, signer_id: &str) -> bool {
        self.0.remove(signer_id).is_some()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl KeyDirectory for InMemoryKeyring {
    fn lookup(&self, signer_id: &str) -> Option<VerifyingKey> {
        self.0.get(signer_id).copied()
    }
}

/// `&InMemoryKeyring` 로도 쓸 수 있게 한다 (소유권을 넘기지 않는 호출부용).
impl<T: KeyDirectory + ?Sized> KeyDirectory for &T {
    fn lookup(&self, signer_id: &str) -> Option<VerifyingKey> {
        (**self).lookup(signer_id)
    }
}
