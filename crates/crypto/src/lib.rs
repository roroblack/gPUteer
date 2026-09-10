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
//! # 영속 구현과 메모리 구현
//!
//! ```text
//! InMemoryReplayGuard   프로세스가 죽으면 캐시가 빈다 — is_durable() == false
//! DurableReplayGuard    SQLite. 재시작을 견딘다 — is_durable() 이 실제 경로에서 도출된다
//!
//! InMemoryKeyring       프로세스가 죽으면 사라진다. 폐기·회전 개념이 없다
//! FileKeyring           §11 K0/K1. ★ K1(DPAPI)은 **Windows 전용**이다
//! ```
//!
//! # ★ 아직 하지 않은 것
//!
//! - §11 K2 (TPM 2.0 / Secure Enclave 비수출 키) — 미구현
//! - Linux 의 OS 보호 저장소 — 미구현. `UnsupportedPlatform` 으로 **명시적으로 실패**한다
//!   (조용히 K0 로 내려가면 보호 등급이 낮아진 것을 아무도 모른다)

pub mod durable_replay;
pub mod framed_ingress;
pub mod hex;
pub mod ingress;
pub mod keyring;
pub mod replay;

pub use durable_replay::DurableReplayGuard;
pub use framed_ingress::{read_frame, write_frame, FrameType, FramingError, IngressMessage};
pub use ingress::{decode_and_verify, Clock, IngressError, KeyDirectorySource, SystemClock};
pub use keyring::{
    KeyDirectoryStatus, KeyDirectoryView, KeyProtection, KeyringError, PersistentKeyring,
    PlaintextPolicy, SecretSigningKey,
};
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

    /// ★ 회전 grace period 처럼 **한 signer_id 에 여러 키가 동시에 유효**할 때 쓴다.
    ///
    /// `lookup()` 하나로는 회전을 표현할 수 없다 —
    /// 옛 키로 서명된 메시지가 아직 도착 중일 수 있기 때문이다.
    ///
    /// 기본 구현은 `lookup()` 결과 하나만 반환한다.
    /// 회전을 모르는 구현체는 그대로 동작한다.
    fn lookup_candidates(&self, signer_id: &str) -> Vec<VerifyingKey> {
        self.lookup(signer_id).into_iter().collect()
    }

    /// ★ **더 이상 유효하지 않은** 키들 (폐기됨 · 회전으로 물러남 · 만료).
    ///
    /// # 왜 필요한가 (2026-08-17)
    ///
    /// 회전 grace period 가 끝난 뒤 구 키로 서명된 메시지가 도착하면,
    /// 활성 키로는 검증이 안 되므로 `InvalidSignature` 가 나온다.
    /// 그 오류의 뜻은 **"위조되었거나 전송 중 변조되었다"** 이다.
    ///
    /// **사실이 아니다.** 서명은 진짜고, 키가 물러났을 뿐이다.
    /// `CLAUDE.md` §3 — "오류 메시지가 사실을 잘못 전하지 않게 한다."
    /// (stale lease 를 "서명 실패" 로 보고해 한참 헤맨 전례가 있다.)
    ///
    /// 이 목록에서 검증되면 `UnknownSigner` 로 보고한다 —
    /// 그 설명이 "팀 멤버가 아니거나 **키가 폐기되었다**" 이기 때문이다.
    ///
    /// ★ **여전히 정확하지 않다.** "이 키는 물러났다" 를 그대로 말하는
    ///   `VerifyOutcome` 값이 없다. 추가하려면 `RULE.md` §3.5 의 5단계
    ///   (proto 수정 -> schema_version 증가 -> 벡터 재생성 -> 구현 -> negative test)를
    ///   밟아야 한다. 지금은 가장 덜 틀린 값을 쓰고 이 사실을 적어 둔다.
    fn lookup_retired(&self, _signer_id: &str) -> Vec<VerifyingKey> {
        Vec::new()
    }
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

        // ★ 회전 중에는 후보가 둘일 수 있다 (구 키 · 신 키).
        //   하나만 보면 회전 중 도착한 옛 서명을 InvalidSignature 로 오보한다 —
        //   `CLAUDE.md` §3, "오류 메시지가 사실을 잘못 전하지 않게 한다."
        let keys = self.directory.lookup_candidates(signer_id);
        if keys.is_empty() {
            return Err(VerifyOutcome::UnknownSigner);
        }

        let signature = Signature::from_bytes(&sig_array);
        if keys.iter().any(|k| k.verify(message, &signature).is_ok()) {
            return Ok(());
        }

        // ★ 물러난 키로 서명된 것인가 — 위조와 구분한다.
        //   구분하지 않으면 정상적인 회전 지연을 "위조" 로 보고하게 된다.
        if self
            .directory
            .lookup_retired(signer_id)
            .iter()
            .any(|k| k.verify(message, &signature).is_ok())
        {
            return Err(VerifyOutcome::UnknownSigner);
        }

        Err(VerifyOutcome::InvalidSignature)
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

    fn lookup_candidates(&self, signer_id: &str) -> Vec<VerifyingKey> {
        (**self).lookup_candidates(signer_id)
    }

    fn lookup_retired(&self, signer_id: &str) -> Vec<VerifyingKey> {
        (**self).lookup_retired(signer_id)
    }
}
