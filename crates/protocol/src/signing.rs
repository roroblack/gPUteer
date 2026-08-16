//! Ed25519 서명·검증 — `docs/protocol/signing.md` §8 · §13.2 구현.
//!
//! # 이 모듈의 목적은 암호가 아니라 **순서 강제**다
//!
//! Ed25519 자체는 `ed25519-dalek` 이 한다. 이 모듈이 하는 일은 다르다.
//!
//! > **서명 검증 전에는 어떤 필드 값도 로직에 사용하지 않는다(MUST NOT).** — §8
//!
//! 이것은 규율로 지킬 수 없다. 급할 때 `msg.job_id` 를 한 번 읽는 것을
//! 코드 리뷰가 항상 잡지는 못한다. 그래서 **타입으로 막는다.**
//!
//! ```text
//! verify(...) -> Result<Verified<M>, VerifyOutcome>
//!
//! Verified<M> 을 거치지 않으면 M 의 필드에 접근할 수 없다.
//! ```
//!
//! # §8 의 9단계
//!
//! ```text
//! 1. domain_tag 결정 (메시지 타입에서 정적으로)   <- Signable::DOMAIN
//! 2. schema_version 확인    -> SCHEMA_TOO_NEW      <- 이 모듈
//! 3. canonical 재구성                              <- 이 모듈
//! 4. sig_input 조립                                <- 이 모듈
//! 5. Ed25519 검증           -> INVALID_SIGNATURE   <- 이 모듈
//! 6. 서명자 신원 확인       -> UNKNOWN_SIGNER      <- KeyResolver (호출자 제공)
//! 7. 시각 검증              -> EXPIRED/CLOCK_SKEW  <- 이 모듈 (§9)
//! 8. replay 검증            -> REPLAY              <- ★ 미구현. 아래 참조
//! 9. 여기서부터 필드 값을 신뢰한다                 <- Verified<M>
//! ```
//!
//! ## ★ 8단계(replay)를 구현하지 않았다
//!
//! `signing.md` §10 은 replay 캐시에 **로컬 SQLite 와 원자적 트랜잭션**을 요구한다.
//! 저장소 계층이 아직 없다. 그래서 [`verify`] 는 replay 검사를 **하지 않는다.**
//!
//! **조용히 빠뜨리지 않기 위해** 단수명 메시지는 [`ReplayGuard`] 를 반드시 받도록
//! 타입으로 강제하고, 미구현 구현체 [`NoReplayCheck`] 는 이름으로 그 사실을 드러낸다.
//! `Verified<M>` 은 어떤 guard 를 거쳤는지 기억한다.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::canonical::{canonical_encode, sig_input, Domain, Fields};
use crate::constants::CLOCK_SKEW_TOLERANCE_MS;

/// `common.proto` 의 `VerifyOutcome` 과 1:1 대응한다.
///
/// ★ `VALID` 는 여기 없다. 성공은 [`Verified`] 라는 **다른 타입**으로 표현한다.
/// 열거형에 `VALID` 를 두면 `if outcome != INVALID` 같은 코드가 생기고,
/// 새 실패 값이 추가될 때 조용히 통과한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// §8-5. 서명이 맞지 않는다.
    InvalidSignature,
    /// §8-6. 서명자를 모른다 / 팀 멤버가 아니다 / 폐기됨.
    UnknownSigner,
    /// §8-7. `expires_at` 을 지났다.
    Expired,
    /// §8-7. 단수명 메시지의 `issued_at` 이 허용 skew 를 벗어났다.
    ClockSkew,
    /// §8-8. 같은 nonce 를 이미 봤다.
    Replay,
    /// §8-1. 다른 domain 의 서명을 재사용하려 했다.
    WrongDomain,
    /// §8-2. 검증자가 아는 버전보다 높다. **절대 VALID 로 취급하지 않는다.**
    SchemaTooNew,
}

impl VerifyOutcome {
    /// `common.proto` `VerifyOutcome` enum 값.
    pub fn proto_value(self) -> i32 {
        match self {
            Self::InvalidSignature => 2,
            Self::UnknownSigner => 3,
            Self::Expired => 4,
            Self::ClockSkew => 5,
            Self::Replay => 6,
            Self::WrongDomain => 7,
            Self::SchemaTooNew => 8,
        }
    }

    /// 운영자에게 보여줄 설명.
    ///
    /// `CLAUDE.md` §3 — "오류 메시지가 사실을 잘못 전하지 않게 한다."
    /// P0-08 이 확인했듯 `SCHEMA_TOO_NEW` 를 "서명 실패" 로 보고하면 며칠 헤맨다.
    pub fn explain(self) -> &'static str {
        match self {
            Self::InvalidSignature => "서명이 내용과 맞지 않는다 — 위조되었거나 전송 중 변조되었다",
            Self::UnknownSigner => "서명자를 모른다 — 팀 멤버가 아니거나 키가 폐기되었다",
            Self::Expired => "메시지가 만료되었다 — 재발급이 필요하다",
            Self::ClockSkew => "발급 시각이 허용 오차를 벗어났다 — 시계 동기화를 확인하라",
            Self::Replay => "이미 처리한 메시지다 — 재전송 공격이거나 중복 전송이다",
            Self::WrongDomain => "다른 용도의 서명이다 — 서명 재사용 시도다",
            Self::SchemaTooNew => {
                "발신자가 더 새로운 스키마를 쓴다 — 서명 문제가 아니라 업그레이드가 필요하다"
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// 서명 가능한 메시지
// ══════════════════════════════════════════════════════════════════

/// 시각 검증 정책 (`signing.md` §9).
///
/// ★ 이 구분을 지키지 않으면 **큐를 통과한 정상 Job 이 100% 거부된다.**
/// Job 이 큐에서 수 시간 대기하는 것은 정상 동작이므로(계획서 §13.6 aging queue),
/// Manifest 에 skew 규칙을 걸면 안 된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifetime {
    /// `|now - issued_at| <= skew` AND `now < expires_at`.
    /// ExecutionGrant · RenewLeaseRequest · Heartbeat · RPC
    ShortLived,
    /// `now < expires_at` 만. `issued_at` 은 검사하지 않는다.
    /// JobManifest · Lease · Invite Bundle
    LongLived,
    /// 만료 검사 없음. Genesis · Release Manifest
    Perpetual,
}

/// 서명 대상 메시지가 구현한다.
///
/// `to_canonical_fields` 는 [`crate::ToCanonicalFields`] 가 제공하므로,
/// 여기서는 **서명에만 필요한 메타데이터**를 요구한다.
pub trait Signable {
    /// §8-1. 메시지 타입에서 **정적으로** 결정된다. 메시지 내용에서 읽지 않는다.
    /// 내용에서 읽으면 공격자가 domain 을 고를 수 있다.
    const DOMAIN: Domain;
    /// §9 시각 검증 정책.
    const LIFETIME: Lifetime;

    fn schema_version(&self) -> u32;
    fn to_canonical_fields(&self) -> Fields;
    /// 서명 필드(90). 아직 서명하지 않았으면 빈 슬라이스.
    fn signature_bytes(&self) -> &[u8];
    /// §9. `Perpetual` 이면 무시된다.
    fn expires_at_unix_ms(&self) -> u64;
    /// §9. `ShortLived` 일 때만 쓰인다.
    fn issued_at_unix_ms(&self) -> u64;
    /// §8-6. 이 값으로 공개키를 찾는다.
    fn signer_id(&self) -> &str;
}

// ══════════════════════════════════════════════════════════════════
// 호출자가 제공하는 두 가지 — 신원과 replay
// ══════════════════════════════════════════════════════════════════

/// §8-6. `signer_id` 로 공개키를 찾는다.
///
/// **팀 멤버십 · 폐기 여부까지 여기서 판단한다.** 키를 안다는 것과
/// 그 키가 지금 유효하다는 것은 다르다.
pub trait KeyResolver {
    fn resolve(&self, signer_id: &str) -> Option<VerifyingKey>;
}

/// §8-8. replay 검사.
///
/// ★ **이 trait 이 존재하는 이유는 구현을 강제하기 위해서다.**
/// 저장소 계층이 없어 지금은 [`NoReplayCheck`] 뿐이지만,
/// 타입에 남겨두면 "빠뜨린 것" 이 아니라 "아직 안 한 것" 으로 보인다.
pub trait ReplayGuard {
    /// 이 nonce 를 처음 보는가. 처음이면 기록하고 `true`.
    ///
    /// §10 — 검사와 삽입은 **단일 트랜잭션**이어야 한다.
    /// 나눠서 하면 그 사이에 창이 열린다.
    fn check_and_record(&mut self, signer_id: &str, domain: Domain, nonce: &[u8]) -> bool;

    /// 이 guard 가 실제로 replay 를 막는가.
    ///
    /// `false` 면 [`Verified::replay_checked`] 가 `false` 가 되고,
    /// 그 사실이 값과 함께 전파된다.
    fn is_effective(&self) -> bool;
}

/// ★ **replay 를 검사하지 않는 구현체.**
///
/// `signing.md` §10 은 로컬 SQLite 와 원자적 트랜잭션을 요구하는데
/// 저장소 계층이 아직 없다.
///
/// **이름이 사실을 말한다.** `DefaultReplayGuard` 같은 이름을 쓰면
/// 아무도 미구현임을 알아채지 못한다.
///
/// 단수명 메시지에 이것을 쓰면 [`Verified::replay_checked`] 가 `false` 이며,
/// [`Verified::require_replay_checked`] 가 그 값의 사용을 막는다.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoReplayCheck;

impl ReplayGuard for NoReplayCheck {
    fn check_and_record(&mut self, _signer: &str, _domain: Domain, _nonce: &[u8]) -> bool {
        true // 검사하지 않으므로 통과시킨다. is_effective() 가 그 사실을 알린다.
    }
    fn is_effective(&self) -> bool {
        false
    }
}

// ══════════════════════════════════════════════════════════════════
// Verified<M> — §13.2 타입 수준 강제
// ══════════════════════════════════════════════════════════════════

/// **서명 검증을 통과한 메시지만 이 타입이 될 수 있다.**
///
/// 생성자가 [`verify`] 하나뿐이므로 우회 경로가 없다.
/// `Deref` 를 구현하지 않는 것도 의도적이다 — `&*msg` 로 새어나가지 않게 한다.
///
/// # 우회 경로가 없음을 컴파일러가 확인한다
///
/// 필드가 비공개이므로 구조체 리터럴로 만들 수 없다.
/// 누군가 필드를 `pub` 으로 열거나 `pub fn new()` 를 추가하면 **이 doctest 가 통과해
/// 실패한다** (`compile_fail` 이므로 컴파일에 성공하면 테스트 실패다).
///
/// ```compile_fail
/// use gputeer_protocol::Verified;
/// let v: Verified<u32> = Verified {
///     inner: 1,
///     signer_id: String::new(),
///     replay_checked: true,
/// };
/// ```
///
/// 정상 경로는 [`verify`] 뿐이다.
///
/// ```
/// use gputeer_protocol::Verified;
/// // Verified<T> 는 값을 읽을 수만 있다. 만들 수는 없다.
/// fn use_it(v: &Verified<u32>) -> u32 { *v.get() }
/// ```
#[derive(Debug, Clone)]
pub struct Verified<M> {
    inner: M,
    signer_id: String,
    replay_checked: bool,
}

impl<M> Verified<M> {
    /// 검증된 메시지. **여기서부터 필드 값을 신뢰한다** (§8-9).
    pub fn get(&self) -> &M {
        &self.inner
    }

    pub fn into_inner(self) -> M {
        self.inner
    }

    /// 검증된 서명자. 메시지 안의 `signer_id` 가 아니라 **검증에 실제로 쓴 키의 주인**이다.
    pub fn signer_id(&self) -> &str {
        &self.signer_id
    }

    /// replay 검사를 실제로 거쳤는가.
    ///
    /// [`NoReplayCheck`] 를 썼다면 `false` 다.
    pub fn replay_checked(&self) -> bool {
        self.replay_checked
    }

    /// replay 검사를 거친 경우에만 값을 내준다.
    ///
    /// **부작용이 있는 동작(외부 API 호출 · 과금 · Job 실행 시작)은 이것을 쓴다.**
    /// `get()` 을 쓰면 replay 미검사 상태로 실행될 수 있다.
    pub fn require_replay_checked(&self) -> Result<&M, VerifyOutcome> {
        if self.replay_checked {
            Ok(&self.inner)
        } else {
            // replay 를 확인하지 못했으므로 "안 봤다" 가 아니라 "막는다".
            // 안전한 실패 방향은 거부다 (§10 축출 정책과 같은 정신).
            Err(VerifyOutcome::Replay)
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// 서명
// ══════════════════════════════════════════════════════════════════

/// `sig_input` 을 조립한다. 서명·검증 양쪽이 이 함수를 쓴다.
///
/// **두 경로가 갈라지면 자기 자신과만 맞는 서명이 만들어진다.**
pub fn signing_input<M: Signable + ?Sized>(msg: &M) -> Vec<u8> {
    let canonical = canonical_encode(&msg.to_canonical_fields(), &[]);
    sig_input(M::DOMAIN, msg.schema_version(), &canonical)
}

/// 메시지에 서명한다. 반환값을 서명 필드(90)에 넣는다.
pub fn sign<M: Signable + ?Sized>(key: &ed25519_dalek::SigningKey, msg: &M) -> [u8; 64] {
    use ed25519_dalek::Signer;
    key.sign(&signing_input(msg)).to_bytes()
}

// ══════════════════════════════════════════════════════════════════
// 검증 — §8 의 순서를 그대로 따른다
// ══════════════════════════════════════════════════════════════════

/// `signing.md` §8 의 9단계를 순서대로 수행한다.
///
/// # 순서를 바꾸지 않는다
///
/// P0-08 이 확인했듯, 2단계(버전)와 5단계(서명)를 바꿔도 **둘 다 거부한다.**
/// 안전성은 순서와 무관하다. 그런데도 순서를 지키는 이유는 **진단 정확성**이다.
/// 뒤집으면 "업그레이드가 필요하다" 를 "서명이 위조됐다" 로 보고하게 된다.
///
/// # `now_unix_ms` 를 인자로 받는 이유
///
/// 테스트가 시각을 통제할 수 있어야 한다. 함수 안에서 시계를 읽으면
/// 만료·skew 경계를 테스트할 수 없고, 그러면 그 경로는 영영 검증되지 않는다.
pub fn verify<M: Signable + Clone>(
    msg: &M,
    max_supported_schema_version: u32,
    keys: &dyn KeyResolver,
    now_unix_ms: u64,
    nonce: Option<&[u8]>,
    replay: &mut dyn ReplayGuard,
) -> Result<Verified<M>, VerifyOutcome> {
    // 1. domain_tag — M::DOMAIN 으로 정적 결정. 메시지에서 읽지 않는다.

    // 2. schema_version
    if msg.schema_version() > max_supported_schema_version {
        return Err(VerifyOutcome::SchemaTooNew);
    }

    // 3·4. canonical 재구성 + sig_input 조립
    let input = signing_input(msg);

    // 5. Ed25519
    let sig_bytes = msg.signature_bytes();
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| VerifyOutcome::InvalidSignature)?;
    let signature = Signature::from_bytes(&sig_array);

    // 6. 서명자 신원 — 키 조회가 곧 멤버십·폐기 확인이다
    let key = keys
        .resolve(msg.signer_id())
        .ok_or(VerifyOutcome::UnknownSigner)?;

    key.verify(&input, &signature)
        .map_err(|_| VerifyOutcome::InvalidSignature)?;

    // 7. 시각 (§9)
    match M::LIFETIME {
        Lifetime::Perpetual => {}
        Lifetime::LongLived => {
            if now_unix_ms >= msg.expires_at_unix_ms() {
                return Err(VerifyOutcome::Expired);
            }
        }
        Lifetime::ShortLived => {
            if now_unix_ms >= msg.expires_at_unix_ms() {
                return Err(VerifyOutcome::Expired);
            }
            let issued = msg.issued_at_unix_ms();
            let skew = now_unix_ms.abs_diff(issued);
            if skew > CLOCK_SKEW_TOLERANCE_MS {
                return Err(VerifyOutcome::ClockSkew);
            }
        }
    }

    // 8. replay (§10) — 단수명 메시지만 대상이다.
    //    JobManifest 는 하나로 여러 Attempt 를 만드는 것이 정상이므로 대상이 아니다.
    let replay_checked = match M::LIFETIME {
        Lifetime::ShortLived => {
            let n = nonce.ok_or(VerifyOutcome::Replay)?;
            // §10 — nonce 는 CSPRNG 16바이트여야 한다(MUST).
            if n.len() != 16 {
                return Err(VerifyOutcome::Replay);
            }
            if !replay.check_and_record(msg.signer_id(), M::DOMAIN, n) {
                return Err(VerifyOutcome::Replay);
            }
            replay.is_effective()
        }
        // 장수명·영구 메시지는 replay 대상이 아니므로 "검사됨" 으로 본다.
        _ => true,
    };

    // 9. 여기서부터 필드 값을 신뢰한다
    Ok(Verified {
        inner: msg.clone(),
        signer_id: msg.signer_id().to_string(),
        replay_checked,
    })
}
