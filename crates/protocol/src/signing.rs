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
//!
//! # ★ 이 크레이트는 암호 라이브러리에 의존하지 않는다
//!
//! `RULE.md` §4.1 이 **Ed25519 를 Crypto 스트림(`crates/crypto/`) 소유**로 정한다.
//! 그래서 여기에는 [`SignatureVerifier`] **trait 만** 둔다 (§4.3 — 공용 trait 는
//! `crates/protocol` 에서 먼저 확정하고, 구현 크레이트는 구현만 한다).
//!
//! 실제 Ed25519 구현은 `gputeer-crypto` 의 `Ed25519Verifier` 다.
//!
//! ★ 처음에는 이 파일이 `ed25519-dalek` 을 직접 썼다. **소유권 위반이었다.**
//!   테스트가 통과한다고 규칙을 고치지 않고, 코드를 규칙에 맞췄다 (§4.3 마지막 줄).

use crate::canonical::{canonical_encode, sig_input, Domain, Fields};
use crate::constants::CLOCK_SKEW_TOLERANCE_MS;

/// §10 — nonce 는 CSPRNG **16바이트**여야 한다(MUST).
pub const NONCE_LEN: usize = 16;

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
    /// ★ 만료 검사 없음 — **증거(evidence)** 다 (ADR-029).
    ///
    /// CheckpointManifest · ReplicaAck · ArtifactRef ·
    /// AttemptReport · CanonicalDecision · RevokeLeaseNotice
    ///
    /// # `Perpetual` 과 무엇이 다른가
    ///
    /// 동작은 같다(만료 검사 없음). **의미가 다르다.**
    ///
    /// ```text
    /// Perpetual   시스템 상수에 가깝다. "지금도 참인가" 를 물을 필요가 없다
    /// Evidence    관측 시점의 사실이다. **소비 측이 신선도를 판단해야 한다**
    /// ```
    ///
    /// 과거의 사실은 만료되지 않는다 — 체크포인트 증거를 시각으로 만료시키면
    /// 오래된 체크포인트에서 재개할 수 없게 되고, 그것은 이 시스템의 존재 이유를 부순다.
    ///
    /// ★ 그러나 **"그 시점의 사실" 은 "지금의 사실" 이 아니다.**
    /// `ReplicaAck` 가 가장 뚜렷하다 — 복제본이 삭제되어도 ACK 는 영원히 유효하다.
    /// 그래서 [`Signable::observed_at_unix_ms`] 를 반드시 노출하게 했다.
    Evidence,
    /// 만료 검사 없음. Genesis · Release Manifest — 시스템 상수에 가깝다.
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
    /// §9. `Evidence`/`Perpetual` 이면 무시된다.
    fn expires_at_unix_ms(&self) -> u64;
    /// §9. `ShortLived` 일 때만 쓰인다.
    fn issued_at_unix_ms(&self) -> u64;

    /// ★ 이 증거가 **언제의 사실인가** (ADR-029).
    ///
    /// `Lifetime::Evidence` 메시지는 만료되지 않으므로, 소비 측이
    /// 신선도를 판단할 근거가 필요하다. 그 근거를 **타입이 강제**한다.
    ///
    /// 기본 구현은 `issued_at_unix_ms()` 다. 메시지마다 이름이 다르므로
    /// (`created_at` · `acked_at` · `decided_at`) 각자 덮어쓴다.
    ///
    /// ★ 0 을 반환하면 안 된다 — "언제인지 모르는 증거" 는 증거가 아니다.
    /// `evidence_must_expose_observation_time` 테스트가 검사한다.
    fn observed_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms()
    }
    /// §8-6. 이 값으로 공개키를 찾는다.
    fn signer_id(&self) -> &str;

    /// §8-8 · §10. replay 캐시에 쓸 nonce.
    ///
    /// ★ **반드시 메시지 안의 서명된 필드에서 온다.**
    ///
    /// 처음에는 `verify()` 가 nonce 를 **별도 인자**로 받았다.
    /// 독립 검수(2026-08-16)가 그것을 지적했다 —
    /// 호출자가 서명된 nonce 대신 아무 값이나 넘길 수 있고,
    /// 그러면 **서명은 통과하는데 replay 방어만 무력화**된다.
    /// 매번 새 값을 넘기면 같은 메시지를 몇 번이든 재생할 수 있다.
    ///
    /// 이제 nonce 는 메시지에서 나오므로 **호출자가 고를 수 없다.**
    ///
    /// `ShortLived` 메시지는 반드시 `Some` 을 반환해야 한다.
    /// 그렇지 않으면 `verify()` 가 `Replay` 로 거부한다.
    fn replay_nonce(&self) -> Option<&[u8]> {
        None
    }
}

// ══════════════════════════════════════════════════════════════════
// 호출자가 제공하는 두 가지 — 신원과 replay
// ══════════════════════════════════════════════════════════════════

/// §8-5 · §8-6. 서명자를 찾고 서명을 검증한다.
///
/// ★ **이 crate 는 암호 라이브러리를 알지 못한다** (`RULE.md` §4.1 — Ed25519 는
/// Crypto 스트림 소유). 그래서 공개키 타입을 노출하지 않고 **결과만** 주고받는다.
/// 구현은 `gputeer-crypto::Ed25519Verifier`.
///
/// # 두 단계를 하나의 trait 에 둔 이유
///
/// §8 은 5(서명 검증)를 6(신원 확인)보다 앞에 적지만, **실제로는 키를 찾아야
/// 서명을 검증할 수 있다.** 순서가 아니라 **결과의 구분**이 중요하다.
///
/// ```text
/// 서명자를 모른다        -> UnknownSigner    "팀 멤버가 아니거나 키가 폐기됐다"
/// 키는 아는데 안 맞는다  -> InvalidSignature "위조되었거나 변조되었다"
/// ```
///
/// 둘을 뭉뚱그려 하나로 보고하면 운영자가 원인을 구분할 수 없다.
pub trait SignatureVerifier {
    /// `signer_id` 의 키로 `message` 에 대한 `signature` 를 검증한다.
    ///
    /// **`UnknownSigner` 와 `InvalidSignature` 를 반드시 구분해 반환한다.**
    /// 그 외의 값을 반환하면 안 된다.
    fn verify_signature(
        &self,
        signer_id: &str,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), VerifyOutcome>;
}

/// §8-8. replay 검사.
///
/// ★ **이 trait 이 존재하는 이유는 구현을 강제하기 위해서다.**
/// 저장소 계층이 없어 지금은 [`NoReplayCheck`] 뿐이지만,
/// 타입에 남겨두면 "빠뜨린 것" 이 아니라 "아직 안 한 것" 으로 보인다.
pub trait ReplayGuard {
    /// 이 nonce 를 처음 보는가. 처음이면 **기록까지 확정**하고 `Fresh`.
    ///
    /// §10 — 검사와 삽입은 **단일 트랜잭션**이어야 한다.
    /// 나눠서 하면 그 사이에 창이 열린다.
    ///
    /// `retain_until_ms` 는 이 항목의 보존 시한이다(§10 —
    /// `expires_at + clock_skew_tolerance`). **미만료 항목을 축출하면 안 된다.**
    ///
    /// # 실패를 `Duplicate` 로 뭉뚱그리지 않는다
    ///
    /// 저장소 장애 · 락 타임아웃 · 캐시 포화는 "이미 봤다" 와 **다른 사실**이다.
    /// 뭉뚱그리면 운영자가 원인을 구분할 수 없고(`CLAUDE.md` §3),
    /// 더 나쁘게는 **장애를 정상 거부로 착각**해 넘어간다.
    fn check_and_record(
        &mut self,
        signer_id: &str,
        domain: Domain,
        nonce: &[u8],
        retain_until_ms: u64,
    ) -> Result<ReplayDecision, ReplayStoreError>;

    /// 이 guard 가 실제로 replay 를 막는가.
    ///
    /// `false` 면 [`Verified::replay_checked`] 가 `false` 가 되고,
    /// 그 사실이 값과 함께 전파된다.
    fn is_effective(&self) -> bool;
}

/// [`ReplayGuard::check_and_record`] 의 정상 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayDecision {
    /// 처음 보는 nonce. **기록이 확정되었다.**
    Fresh,
    /// 이미 본 nonce.
    Duplicate,
}

/// replay 저장소의 **로컬 장애.**
///
/// ★ 이것은 `VerifyOutcome` 이 **아니다.**
/// `VerifyOutcome` 은 상대에게 보고하는 프로토콜 결과이고,
/// 이것은 우리 쪽 저장소가 답을 못 준 것이다.
/// 섞으면 "상대가 재전송했다" 와 "우리 디스크가 죽었다" 를 구분할 수 없다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayStoreError {
    /// 디스크 I/O 실패.
    Io(String),
    /// 다른 프로세스가 잡은 락을 제한 시간 안에 얻지 못했다.
    LockTimeout,
    /// §10 상한에 도달했다. **축출이 아니라 거부**가 안전한 방향이다 —
    /// 미만료 nonce 를 밀어내면 replay 창이 열린다.
    CacheFull,
}

impl ReplayStoreError {
    /// 운영자에게 보여줄 설명.
    pub fn explain(&self) -> &'static str {
        match self {
            Self::Io(_) => "replay 저장소 I/O 실패 — 상대 문제가 아니라 우리 쪽 장애다",
            Self::LockTimeout => "replay 저장소 락 대기 초과 — 동시 검증이 몰렸거나 락이 걸렸다",
            Self::CacheFull => {
                "replay 캐시 포화 — 미만료 nonce 를 축출하지 않고 거부했다. 과부하 신호다"
            }
        }
    }
}

/// [`verify`] 의 실패.
///
/// ★ **프로토콜 결과와 로컬 장애를 분리한다.**
///
/// ```text
/// Outcome(..)      상대 메시지의 문제. common.proto VerifyOutcome 으로 보고 가능
/// ReplayStore(..)  우리 쪽 저장소가 답을 못 줬다. 보고할 값이 아니다
/// ```
///
/// 둘을 하나로 합치면 "재전송 공격" 과 "디스크 장애" 가 같은 값이 되고,
/// 운영자는 엉뚱한 곳을 본다(`CLAUDE.md` §3).
///
/// ★ 어느 쪽이든 **부작용을 실행하지 않는다.** fail closed 다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    Outcome(VerifyOutcome),
    ReplayStore(ReplayStoreError),
}

impl From<VerifyOutcome> for VerifyError {
    fn from(o: VerifyOutcome) -> Self {
        Self::Outcome(o)
    }
}

impl VerifyError {
    /// 프로토콜 결과라면 그 값. 로컬 장애면 `None`.
    pub fn outcome(&self) -> Option<VerifyOutcome> {
        match self {
            Self::Outcome(o) => Some(*o),
            Self::ReplayStore(_) => None,
        }
    }

    pub fn explain(&self) -> &'static str {
        match self {
            Self::Outcome(o) => o.explain(),
            Self::ReplayStore(e) => e.explain(),
        }
    }
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
    fn check_and_record(
        &mut self,
        _signer: &str,
        _domain: Domain,
        _nonce: &[u8],
        _retain_until_ms: u64,
    ) -> Result<ReplayDecision, ReplayStoreError> {
        // 검사하지 않으므로 통과시킨다. is_effective() 가 그 사실을 알리고,
        // Verified::require_replay_checked() 가 부작용 경로 사용을 막는다.
        Ok(ReplayDecision::Fresh)
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
/// 서명 쪽(`gputeer-crypto::sign`)도 반드시 이 함수를 통과한다.
pub fn signing_input<M: Signable + ?Sized>(msg: &M) -> Vec<u8> {
    let canonical = canonical_encode(&msg.to_canonical_fields(), &[]);
    sig_input(M::DOMAIN, msg.schema_version(), &canonical)
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
    verifier: &dyn SignatureVerifier,
    now_unix_ms: u64,
    replay: &mut dyn ReplayGuard,
) -> Result<Verified<M>, VerifyError> {
    // 1. domain_tag — M::DOMAIN 으로 정적 결정. 메시지에서 읽지 않는다.

    // 2. schema_version
    if msg.schema_version() > max_supported_schema_version {
        return Err(VerifyOutcome::SchemaTooNew.into());
    }

    // 3·4. canonical 재구성 + sig_input 조립
    let input = signing_input(msg);

    // 5·6. 서명 검증 + 서명자 신원 (Crypto 스트림에 위임)
    verifier.verify_signature(msg.signer_id(), &input, msg.signature_bytes())?;

    // 7. 시각 (§9)
    match M::LIFETIME {
        // 증거는 만료되지 않는다 (ADR-029). 신선도는 소비 측이 fence_epoch 로 판단한다.
        Lifetime::Evidence | Lifetime::Perpetual => {}
        Lifetime::LongLived => {
            if now_unix_ms >= msg.expires_at_unix_ms() {
                return Err(VerifyOutcome::Expired.into());
            }
        }
        Lifetime::ShortLived => {
            if now_unix_ms >= msg.expires_at_unix_ms() {
                return Err(VerifyOutcome::Expired.into());
            }
            let issued = msg.issued_at_unix_ms();
            let skew = now_unix_ms.abs_diff(issued);
            if skew > CLOCK_SKEW_TOLERANCE_MS {
                return Err(VerifyOutcome::ClockSkew.into());
            }
        }
    }

    // 8. replay (§10) — 단수명 메시지만 대상이다.
    //    JobManifest 는 하나로 여러 Attempt 를 만드는 것이 정상이므로 대상이 아니다.
    let replay_checked = match M::LIFETIME {
        Lifetime::ShortLived => {
            // ★ nonce 는 **메시지 안의 서명된 필드**에서 온다.
            //   호출자가 넘기던 예전 설계는 서명은 통과하고 replay 방어만
            //   무력화되는 구멍이었다 (독립 검수 2026-08-16).
            let n = msg.replay_nonce().ok_or(VerifyOutcome::Replay)?;
            // §10 — nonce 는 CSPRNG 16바이트여야 한다(MUST).
            if n.len() != NONCE_LEN {
                return Err(VerifyOutcome::Replay.into());
            }
            // §10 — 보존 시한. 미만료 항목은 축출되면 안 된다.
            let retain_until = msg
                .expires_at_unix_ms()
                .saturating_add(CLOCK_SKEW_TOLERANCE_MS);

            match replay.check_and_record(msg.signer_id(), M::DOMAIN, n, retain_until) {
                Ok(ReplayDecision::Fresh) => replay.is_effective(),
                Ok(ReplayDecision::Duplicate) => return Err(VerifyOutcome::Replay.into()),
                // ★ 저장소 장애를 Replay 로 뭉뚱그리지 않는다.
                //   어느 쪽이든 부작용은 실행하지 않지만(fail closed),
                //   운영자가 원인을 구분할 수 있어야 한다.
                Err(e) => return Err(VerifyError::ReplayStore(e)),
            }
        }
        // 장수명·증거·영구 메시지는 replay 대상이 아니므로 "검사됨" 으로 본다.
        _ => true,
    };

    // 9. 여기서부터 필드 값을 신뢰한다
    Ok(Verified {
        inner: msg.clone(),
        signer_id: msg.signer_id().to_string(),
        replay_checked,
    })
}
