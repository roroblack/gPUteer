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
use crate::constants::{CLOCK_SKEW_TOLERANCE_MS, MAX_SHORTLIVED_TTL_MS, SCHEMA_VERSION};

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
    /// 신선도를 판단할 근거가 필요하다.
    ///
    /// # ★ 이것은 "타입 수준 강제" 가 **아니다** (2026-08-16 정정)
    ///
    /// 처음에는 "타입이 강제한다" 고 적었는데 **과장이었다.**
    /// 독립 검수가 지적했다 — 기본 구현이 있으므로
    /// **아무것도 덮어쓰지 않아도 컴파일된다.**
    ///
    /// ```text
    /// 타입이 실제로 보장하는 것   메서드가 존재한다 · 값이 반환된다
    /// 타입이 보장하지 **못하는** 것
    ///     - 반환값이 진짜 관측 시각인가
    ///     - 발행 시각과 구분되는가
    ///     - 신선도 판단에 충분한가
    /// ```
    ///
    /// 기본 구현이 `issued_at_unix_ms()` 인 이유는 대부분의 메시지에서
    /// 그 둘이 같기 때문이다. 다른 메시지(`created_at` · `acked_at` · `decided_at`)는
    /// **손으로 덮어써야 하며, 그것을 강제하는 것은 타입이 아니라 테스트다.**
    ///
    /// `evidence_must_expose_observation_time` 이 6종 각각의 값을 대조한다.
    /// 새 `Evidence` 메시지를 추가하면 **그 테스트에도 넣어야 한다** —
    /// 넣지 않으면 기본 구현이 조용히 쓰인다.
    fn observed_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms()
    }
    /// §8-6. 이 값으로 공개키를 찾는다.
    fn signer_id(&self) -> &str;

    /// ★ §6 — **서명만으로는 보장되지 않는 내부 일관성**을 검사한다.
    ///
    /// 도출 해시 필드(규칙 i)는 canonical 에서 제외되므로 **서명이 그 값을
    /// 보증하지 않는다.** `ExecutionGrant.manifest_hash` 가 유일한 예다.
    ///
    /// `signing.md` §6.1 —
    /// > `ExecutionGrant.manifest_hash` 는 참조용 사본이다.
    /// > **Agent 는 이 값을 신뢰하지 않고 반드시 재계산해 대조한다(MUST).**
    ///
    /// 독립 검수(2026-08-16)가 지적했다 — 규범은 MUST 라고 적었는데
    /// **그 코드가 어디에도 없었다.** `verify()` 성공만으로는
    /// Grant 가 올바른 manifest 를 가리킨다는 보장이 없었다.
    ///
    /// # ★ 기본 구현은 no-op 이다 — 타입이 강제하지 않는다
    ///
    /// 대부분의 메시지에는 도출 해시가 없으므로 기본은 통과다.
    /// **덮어쓰지 않아도 컴파일된다.** 그것을 잡는 것은 타입이 아니라
    /// `derived_hash_messages_override_consistency_check` 테스트다 —
    /// `DERIVED_HASH_FIELDS` 에 있는 메시지가 이 메서드를 덮어썼는지 대조한다.
    fn check_derived_consistency(&self) -> Result<(), DerivedMismatch> {
        Ok(())
    }

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

/// ★ replay 검사가 **실제로 무엇을 했는가.**
///
/// # 왜 bool 이 아닌가 (독립 검수 2026-08-17)
///
/// 전에는 `replay_checked: bool` 하나였고, 단수명이 아닌 메시지에는
/// `_ => true` 를 넣었다. 그래서 **"검사했다" 와 "검사 대상이 아니다" 가
/// 같은 값**이었다.
///
/// 결과: `require_replay_checked()` 가 `LongLived` 메시지를 통과시켰다.
/// 그 메시지는 만료 전까지 **무제한 재전송이 가능하다.**
/// 부작용 게이트로 쓰라고 문서에 적어 둔 함수가 방어를 안 한 것이다.
///
/// ```text
/// Checked        guard 에 물어봤고 Fresh 였다        -> 부작용 허용
/// NotApplicable  LIFETIME 이 ShortLived 가 아니다    -> ★ replay 방어가 **없다**
/// Ineffective    guard 가 NoReplayCheck 였다         -> 방어가 없다
/// ```
///
/// `NotApplicable` 은 "안전하다" 가 아니라 **"이 계층은 막지 않는다"** 이다.
/// 그 메시지로 부작용을 실행하려면 소비 측이 자기 멱등성을 갖춰야 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStatus {
    /// replay guard 에 물어봤고 처음 보는 nonce 였다.
    Checked,
    /// `LIFETIME` 이 `ShortLived` 가 아니다 — 이 계층에 replay 방어가 없다.
    NotApplicable,
    /// guard 가 [`NoReplayCheck`] 였다 — 물어봤지만 아무것도 기억하지 않는다.
    Ineffective,
}

impl ReplayStatus {
    /// 부작용을 실행해도 되는가. `Checked` 만 참이다.
    pub fn permits_side_effects(self) -> bool {
        matches!(self, Self::Checked)
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::Checked => "replay 저장소가 처음 보는 nonce 라고 답했다",
            Self::NotApplicable => {
                "단수명 메시지가 아니어서 replay 검사를 하지 않았다 —                  이 계층은 재전송을 막지 않는다"
            }
            Self::Ineffective => {
                "replay guard 가 아무것도 기억하지 않는 구현이다 — 방어가 없다"
            }
        }
    }
}

/// ★ 프로토콜 결과가 아니라 **로컬 정책 위반.**
///
/// `VerifyOutcome` 에 대응하는 값이 없다 — `common.proto` 의 enum 은
/// 서명·시각·replay 만 다룬다. `VerifyError::Derived` 와 같은 처지다.
///
/// ★ **알려진 공백**: 상대에게 이 사유를 그대로 보고할 수단이 없다.
///   proto enum 에 값을 추가하려면 `RULE.md` §3.5 의 5단계를 밟아야 한다.
///   지금은 로컬 거부 + 로그로만 다룬다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyViolation {
    /// 단수명 메시지의 TTL 이 상한을 넘었다.
    ///
    /// # 왜 막는가 (독립 검수 2026-08-17)
    ///
    /// `retain_until = expires_at + skew` 이므로, `expires_at` 이 아주 먼
    /// 미래면 그 nonce 는 **영원히 GC 되지 않는다.**
    /// 그런 nonce 를 capacity 만큼 만들면 캐시가 영구히 차서
    /// 다른 모든 검증이 `CacheFull` 로 거부된다.
    ///
    /// ★ `retain_until` 만 잘라내는 방법은 쓰지 않았다 —
    ///   메시지가 아직 유효한데 nonce 를 지우면 §10 이 금지한
    ///   "미만료 항목 축출" 이 되어 replay 창이 열린다.
    ///   DoS 를 replay 구멍으로 바꾸는 것은 거래가 아니다.
    ShortLivedTtlTooLong { ttl_ms: u64, max_ms: u64 },
}

impl PolicyViolation {
    pub fn explain(&self) -> String {
        match self {
            Self::ShortLivedTtlTooLong { ttl_ms, max_ms } => format!(
                "단수명 메시지의 TTL {}ms 가 상한 {}ms 를 넘었다 —                  replay 캐시를 영구 점유할 수 있어 거부한다",
                ttl_ms, max_ms
            ),
        }
    }
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
    /// ★ nonce 가 §10 이 요구하는 길이가 아니다 (독립 검수 2026-08-17).
    ///
    /// # 왜 `Io` 가 아닌가
    ///
    /// 초안은 이것을 `Io` 로 보고했다. `Io` 의 뜻은
    /// **"상대 문제가 아니라 우리 쪽 장애다"** 이다.
    ///
    /// 길이가 틀린 nonce 는 **디스크 장애가 아니라 입력 위반**이다.
    /// 둘을 섞으면 운영자가 malformed request 를 디스크 고장으로 읽는다 —
    /// `CLAUDE.md` §3, "오류 메시지가 사실을 잘못 전하지 않게 한다."
    InvalidNonce { len: usize },
    /// ★ 한 서명자가 자기 몫을 다 썼다 (독립 검수 2026-08-17).
    ///
    /// 전역 상한만 있으면 **한 device 가 캐시를 다 차지해 다른 모든
    /// device 를 차단**할 수 있다. 그것은 replay 방어가 아니라 DoS 통로다.
    /// 이 오류는 **그 서명자만** 막고 나머지는 계속 통과시킨다.
    SignerQuotaExceeded { signer_id: String, quota: usize },
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
            Self::InvalidNonce { .. } => {
                "nonce 길이가 §10 규정과 다르다 — 저장소 장애가 아니라 입력 위반이다"
            }
            Self::SignerQuotaExceeded { .. } => {
                "이 서명자가 replay 캐시 할당량을 다 썼다 — 다른 서명자는 영향받지 않는다"
            }
        }
    }
}

/// §6 도출 해시 불일치.
///
/// ★ `VerifyOutcome` 에 대응하는 값이 **없다.**
/// `common.proto` 의 `VerifyOutcome` 은 서명·시각·replay 만 다루고
/// 도출 해시 불일치를 위한 값이 없다.
///
/// **없는 값을 만들어 붙이지 않는다** — `InvalidSignature` 로 보고하면
/// "서명이 위조됐다" 로 읽히는데 실제로는 **서명은 정상이고 참조 해시가 틀린 것**이다
/// (`CLAUDE.md` §3). 그 둘은 원인도 대응도 다르다.
///
/// proto 에 값을 추가하려면 `schema_version` 상향이 필요하다(§7.3).
/// → `TODO_VISION` V-09.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedMismatch {
    /// 어느 필드가 어긋났는가 (예: `"ExecutionGrant.manifest_hash"`).
    pub field: &'static str,
    /// 사람이 읽을 설명.
    pub detail: String,
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
    /// 로컬 정책 위반 — 프로토콜 결과가 아니다. [`PolicyViolation`] 참조.
    Policy(PolicyViolation),
    /// §6 — 서명은 정상인데 도출 해시가 내용과 맞지 않는다.
    ///
    /// ★ `VerifyOutcome` 으로 보고할 수 없다 — 대응하는 proto 값이 없다.
    Derived(DerivedMismatch),
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
            // ★ 로컬 사유는 프로토콜 결과가 없다. 상대에게 보고할 값이 없다는 뜻이며,
            //   그것을 임의의 VerifyOutcome 으로 채우면 오류가 사실을 잘못 전한다.
            Self::ReplayStore(_) | Self::Derived(_) | Self::Policy(_) => None,
        }
    }

    pub fn explain(&self) -> &'static str {
        match self {
            Self::Outcome(o) => o.explain(),
            Self::ReplayStore(e) => e.explain(),
            Self::Derived(_) => {
                "도출 해시가 내용과 맞지 않는다 — 서명은 정상이므로 위조가 아니라                  발신자가 잘못된 참조 해시를 넣었거나 중첩 메시지가 바꿔치기됐다"
            }
            Self::Policy(PolicyViolation::ShortLivedTtlTooLong { .. }) => {
                "단수명 메시지의 TTL 이 상한을 넘었다 — replay 캐시를 영구 점유할 수 있어 거부했다"
            }
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
/// # ★ `M: Clone` 이 깊은 복사를 보장하지는 않는다 (2026-08-16 기록)
///
/// [`verify`] 는 `msg.clone()` 을 저장하므로, 호출자가 원본을 고쳐도
/// 이 값은 바뀌지 않는다 — **prost 생성 타입은 전부 깊은 복사**이므로 안전하다.
///
/// 그러나 누군가 `Arc<Mutex<_>>` 를 담은 타입에 [`Signable`] 을 구현하면
/// clone 이 상태를 공유해 **원본 수정이 `get()` 에 반영된다.**
/// 독립 검수가 지적했다. 현재 그런 타입은 없다.
///
/// [`Verified::into_inner`] 로 꺼낸 값은 검증 증명을 잃는다 —
/// 고쳐서 다시 `Verified` 로 감쌀 **공개 경로는 없다**(생성자가 비공개).
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
    replay_status: ReplayStatus,
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

    /// replay 검사가 **실제로 무엇을 했는가.**
    pub fn replay_status(&self) -> ReplayStatus {
        self.replay_status
    }

    /// replay 검사를 실제로 거쳤는가.
    ///
    /// ★ 2026-08-17 의미가 **좁아졌다** (독립 검수).
    /// 전에는 단수명이 아닌 메시지에도 `true` 였다 —
    /// "검사 대상이 아니다" 를 "검사했다" 로 보고한 것이다.
    /// 지금은 [`ReplayStatus::Checked`] 일 때만 참이다.
    pub fn replay_checked(&self) -> bool {
        self.replay_status.permits_side_effects()
    }

    /// replay 검사를 **실제로 거친** 경우에만 값을 내준다.
    ///
    /// **부작용이 있는 동작(외부 API 호출 · 과금 · Job 실행 시작)은 이것을 쓴다.**
    ///
    /// # 무엇이 막히는가
    ///
    /// ```text
    /// Checked        통과
    /// Ineffective    거부 — guard 가 아무것도 기억하지 않는다
    /// NotApplicable  거부 — ★ 이 계층에 replay 방어가 **없다**
    /// ```
    ///
    /// ★ `NotApplicable` 을 막는 것이 2026-08-17 의 변경이다.
    ///   `LongLived` 메시지는 만료 전까지 **무제한 재전송**된다.
    ///   그것으로 과금이나 Job 실행을 하면 중복 실행된다.
    ///
    /// 그 메시지로 부작용을 실행해야 한다면 [`Verified::get`] 을 쓰고
    /// **소비 측이 자기 멱등성을 갖춘다.** 그 선택을 눈에 보이게 만드는 것이
    /// 이 함수의 목적이다.
    pub fn require_replay_checked(&self) -> Result<&M, VerifyOutcome> {
        if self.replay_status.permits_side_effects() {
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

    // ★ 호출자가 `u32::MAX` 를 넘기면 SCHEMA_TOO_NEW 가 **영영 안 나온다**
    //   (독립 검수 2026-08-16). u32 값은 u32::MAX 보다 클 수 없기 때문이다.
    //   §7.2 는 "모르면 검증 불가를 선언한다" 를 MUST 로 정하는데,
    //   그 방어가 호출자 실수 한 번으로 통째로 꺼진다.
    //
    //   이 크레이트가 아는 최대 버전을 넘는 값은 **받지 않는다.**
    //   "무제한 허용" 을 표현할 방법을 남겨 두지 않는다.
    debug_assert!(
        max_supported_schema_version <= SCHEMA_VERSION,
        "max_supported({max_supported_schema_version}) 가 이 빌드가 아는          SCHEMA_VERSION({SCHEMA_VERSION}) 보다 크다"
    );
    let max_supported = max_supported_schema_version.min(SCHEMA_VERSION);

    // 2. schema_version
    if msg.schema_version() > max_supported {
        return Err(VerifyOutcome::SchemaTooNew.into());
    }

    // 3·4. canonical 재구성 + sig_input 조립
    let input = signing_input(msg);

    // 5·6. 서명 검증 + 서명자 신원 (Crypto 스트림에 위임)
    verifier.verify_signature(msg.signer_id(), &input, msg.signature_bytes())?;

    // 6.5 ★ 도출 해시 대조 (§6.1).
    //
    //   규칙 i 로 canonical 에서 제외되므로 **서명이 이 값을 보증하지 않는다.**
    //   §6.1 이 "Agent 는 반드시 재계산해 대조한다(MUST)" 고 적었는데
    //   그 코드가 어디에도 없었다 (독립 검수 2026-08-16).
    //
    //   ★ §8 의 9단계에는 없는 단계다. §6 의 요구를 §8 흐름에 넣은 것이며,
    //     서명 검증 **뒤**에 둔다 — 서명이 깨진 메시지의 내부 일관성을
    //     따지는 것은 의미가 없고, 오류 진단만 흐려진다.
    msg.check_derived_consistency().map_err(VerifyError::Derived)?;

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
    let replay_status = match M::LIFETIME {
        Lifetime::ShortLived => {
            // ★ nonce 는 **메시지 안의 서명된 필드**에서 온다.
            //   호출자가 넘기던 예전 설계는 서명은 통과하고 replay 방어만
            //   무력화되는 구멍이었다 (독립 검수 2026-08-16).
            let n = msg.replay_nonce().ok_or(VerifyOutcome::Replay)?;
            // §10 — nonce 는 CSPRNG 16바이트여야 한다(MUST).
            if n.len() != NONCE_LEN {
                return Err(VerifyOutcome::Replay.into());
            }
            // ★ TTL 상한 (독립 검수 2026-08-17).
            //   `retain_until` 이 아주 먼 미래면 그 nonce 는 **영원히 GC 되지 않는다.**
            //   그런 서명을 capacity 만큼 만들면 캐시가 영구히 찬다.
            //
            //   `retain_until` 만 잘라내는 방법은 쓰지 않았다 —
            //   미만료 nonce 를 지우면 §10 이 금지한 축출이 되어 replay 창이 열린다.
            //   **DoS 를 replay 구멍으로 바꾸는 것은 거래가 아니다.**
            let ttl = msg
                .expires_at_unix_ms()
                .saturating_sub(msg.issued_at_unix_ms());
            if ttl > MAX_SHORTLIVED_TTL_MS {
                return Err(VerifyError::Policy(PolicyViolation::ShortLivedTtlTooLong {
                    ttl_ms: ttl,
                    max_ms: MAX_SHORTLIVED_TTL_MS,
                }));
            }

            // §10 — 보존 시한. 미만료 항목은 축출되면 안 된다.
            let retain_until = msg
                .expires_at_unix_ms()
                .saturating_add(CLOCK_SKEW_TOLERANCE_MS);

            match replay.check_and_record(msg.signer_id(), M::DOMAIN, n, retain_until) {
                Ok(ReplayDecision::Fresh) => {
                    if replay.is_effective() {
                        ReplayStatus::Checked
                    } else {
                        ReplayStatus::Ineffective
                    }
                }
                Ok(ReplayDecision::Duplicate) => return Err(VerifyOutcome::Replay.into()),
                // ★ 저장소 장애를 Replay 로 뭉뚱그리지 않는다.
                //   어느 쪽이든 부작용은 실행하지 않지만(fail closed),
                //   운영자가 원인을 구분할 수 있어야 한다.
                Err(e) => return Err(VerifyError::ReplayStore(e)),
            }
        }
        // ★ 장수명·증거·영구 메시지는 replay 대상이 **아니다.**
        //   전에는 이것을 `true`(검사됨)로 보고했다 — 사실이 아니다.
        //   이 계층은 그 메시지들의 재전송을 막지 않는다.
        _ => ReplayStatus::NotApplicable,
    };

    // 9. 여기서부터 필드 값을 신뢰한다
    Ok(Verified {
        inner: msg.clone(),
        signer_id: msg.signer_id().to_string(),
        replay_status,
    })
}

// ══════════════════════════════════════════════════════════════════
// ★ 오류 타입을 `std::error::Error` 로 만든다 (2026-08-17)
//
//   `gputeer selftest` 바이너리를 처음 쓰면서 드러났다 —
//   `DurableReplayGuard::open(...)?` 가 컴파일되지 않았다.
//   `ReplayStoreError` 가 `std::error::Error` 를 구현하지 않아
//   `Box<dyn Error>` 로 올라가지 못한 것이다.
//
//   ★ **공개 오류 타입인데 호출자가 `?` 를 못 쓰는 것은 결함이다.**
//     테스트는 전부 `unwrap()`/`matches!` 를 써서 이 문제를 못 봤다.
//     실제로 쓰는 코드를 하나 쓰니 바로 나왔다.
// ══════════════════════════════════════════════════════════════════

impl core::fmt::Display for VerifyOutcome {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}: {}", self, self.explain())
    }
}

impl std::error::Error for VerifyOutcome {}

impl core::fmt::Display for ReplayStoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(detail) => write!(f, "{} ({detail})", self.explain()),
            Self::SignerQuotaExceeded { signer_id, quota } => {
                write!(f, "{} (signer={signer_id}, quota={quota})", self.explain())
            }
            Self::InvalidNonce { len } => write!(f, "{} (len={len})", self.explain()),
            _ => f.write_str(self.explain()),
        }
    }
}

impl std::error::Error for ReplayStoreError {}

impl core::fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.explain())
    }
}

impl std::error::Error for PolicyViolation {}

impl core::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.explain())
    }
}

impl std::error::Error for VerifyError {}
