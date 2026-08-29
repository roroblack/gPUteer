//! 원시 protobuf 바이트를 검증된 메시지로 바꾸는 단일 진입점.
//!
//! 이 모듈은 Protocol의 검증 규칙과 Crypto의 실제 구현을 조립한다.
//! `protocol`이 `crypto`에 의존하지 않도록 이 경계는 Crypto 스트림에 둔다.
//!
//! ```text
//! raw bytes
//!   -> prost decode
//!   -> Clock에서 현재 시각 획득
//!   -> Ed25519Verifier + KeyDirectory
//!   -> ReplayGuard
//!   -> Verified<M>
//! ```
//!
//! 디코드된 원시 메시지는 이 함수의 반환값으로 노출하지 않는다.
//! 호출자는 성공한 경우에만 `Verified<M>::get()`을 통해 메시지를 읽는다.
//!
//! 단, protobuf 생성 타입 자체가 공개되어 있으므로 호출자가 별도로
//! `prost::Message::decode()`를 직접 호출하는 것까지 이 모듈이 막는다고
//! 주장하지 않는다. 이 모듈이 제공하는 보장은 "이 진입점을 통과한 값만
//! 안전한 입력으로 취급한다"는 경계다.

use std::{
    error::Error,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use gputeer_protocol::signing::{
    verify, ReplayGuard, ReplayStoreError, Signable, Verified, VerifyError,
};
use prost::Message;

use crate::{Ed25519Verifier, KeyDirectory, KeyDirectoryView, PersistentKeyring};

/// 테스트 가능한 시각 공급자.
///
/// 검증 함수 내부에서 `SystemTime::now()`를 직접 호출하지 않는다.
/// 테스트는 고정 시각 구현을 주입하고, 운영 코드는 [`SystemClock`]을 사용한다.
pub trait Clock {
    fn now_unix_ms(&self) -> u64;
}

/// 운영 환경에서 사용하는 Unix 시각 공급자.
///
/// 이 타입 자체가 시계를 읽는 유일한 위치다. 검증 로직은 이 타입을
/// 직접 참조하지 않고 [`Clock`] trait만 사용한다.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();

        duration.as_millis().min(u64::MAX as u128) as u64
    }
}

/// 검증에 사용할 키 디렉터리의 공급 방식.
///
/// `PersistentKeyring`은 반드시 `Persistent` 변형으로 넘긴다.
/// 그러면 진입점이 Clock에서 얻은 동일한 시각으로 `keyring.at(now)`를
/// 만들어 키 유효기간 판단과 메시지 시각 검증이 같은 기준을 사용한다.
///
/// `Provided`는 이미 특정 시각에 맞춰 만든 `KeyDirectoryView`나
/// 시각과 무관한 메모리 디렉터리를 넘길 때 사용한다.
pub enum KeyDirectorySource<'a> {
    /// 영속 키링을 진입점 내부에서 동일한 시각으로 조회한다.
    Persistent(&'a PersistentKeyring),

    /// 호출자가 준비한 키 디렉터리다.
    Provided(&'a dyn KeyDirectory),
}

/// 원시 입력 진입점의 실패.
///
/// protobuf 디코드 실패는 프로토콜 검증 결과가 아니므로
/// [`VerifyError`]나 [`VerifyOutcome`]으로 변환하지 않는다.
#[derive(Debug)]
pub enum IngressError {
    /// wire bytes가 protobuf 메시지로 해석되지 않았다.
    Decode(prost::DecodeError),

    /// 메시지는 디코드되었지만 서명·시각·정책 검증에 실패했다.
    Verification(VerifyError),

    /// replay 저장소가 검증 결과를 확정하지 못했다.
    ///
    /// 특히 `LockTimeout`은 "이미 본 메시지"가 아니다.
    /// 따라서 `Replay`로 위장하지 않고 별도 오류로 반환한다.
    ReplayStoreUnavailable(ReplayStoreError),
}

impl fmt::Display for IngressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => {
                write!(formatter, "protobuf 디코드 실패: {error}")
            }
            Self::Verification(error) => {
                write!(formatter, "메시지 검증 실패: {error:?}")
            }
            Self::ReplayStoreUnavailable(error) => {
                write!(formatter, "replay 저장소를 사용할 수 없다: {error:?}")
            }
        }
    }
}

impl Error for IngressError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            Self::Verification(_) | Self::ReplayStoreUnavailable(_) => None,
        }
    }
}

/// 원시 protobuf 바이트를 `Verified<M>`으로 변환한다.
///
/// `M`을 제네릭으로 받기 때문에 domain tag 문자열을 런타임에서 분기하지
/// 않는다. 호출자가 `ExecutionGrant`를 요구하면 `ExecutionGrant`만 디코드되고,
/// 그 타입의 정적 `Signable::DOMAIN`과 `LIFETIME`이 검증에 사용된다.
///
/// 시각은 이 함수 안에서 한 번만 읽는다. `PersistentKeyring`을 사용하는
/// 경우에도 같은 시각으로 key directory view를 만든다.
///
/// replay 저장소의 `LockTimeout`은 자동 재시도하지 않는다. replay 검사는
/// "검사와 기록"이 한 원자적 연산이어야 하므로, 락 상태가 불명확한 뒤
/// 임의로 재시도하는 정책을 이 계층에 숨기지 않는다. 저장소 장애는 즉시
/// 거부하고, 부작용을 실행할 수 있는 `Verified<M>`을 반환하지 않는다.
/// # ★ 게이트를 우회할 수 없다 (컴파일러가 확인한다)
///
/// 반환값은 [`Verified<M>`] 이며 `Deref` 도 공개 필드도 없다.
/// 필드를 직접 읽으려 하면 **컴파일되지 않는다.**
///
/// ```compile_fail
/// use gputeer_protocol::pb;
/// use gputeer_protocol::signing::Verified;
///
/// fn read_without_gate(m: &Verified<pb::JobManifest>) -> &str {
///     &m.job_id      // ★ Verified 에는 그런 필드가 없다
/// }
/// ```
///
/// ★ **비공허성**: 위 예제가 import 오류로 실패하면 아무것도 증명하지 못한다.
///   같은 import 로 `get()` 을 쓰는 아래 예제는 **컴파일된다.**
///
/// ```
/// use gputeer_protocol::pb;
/// use gputeer_protocol::signing::Verified;
///
/// fn read_through_gate(m: &Verified<pb::JobManifest>) -> &str {
///     &m.get().job_id
/// }
/// ```

pub fn decode_and_verify<M>(
    raw: &[u8],
    max_supported_schema_version: u32,
    key_directory: KeyDirectorySource<'_>,
    replay: &mut dyn ReplayGuard,
    clock: &dyn Clock,
) -> Result<Verified<M>, IngressError>
where
    // ★ `Default` 가 필요한 이유: `prost::Message::decode` 는
    //   기본값 인스턴스를 만든 뒤 필드를 채운다. 초안에 이 bound 가 빠져 있었다.
    M: Message + Signable + Clone + Default,
{
    let message = M::decode(raw).map_err(IngressError::Decode)?;

    // 시각을 한 번만 얻어 키 유효기간과 메시지 수명 검증에 함께 사용한다.
    let now_unix_ms = clock.now_unix_ms();

    match key_directory {
        KeyDirectorySource::Persistent(keyring) => {
            let directory = keyring.at(now_unix_ms);

            verify_decoded(
                &message,
                max_supported_schema_version,
                &directory,
                now_unix_ms,
                replay,
            )
        }
        KeyDirectorySource::Provided(directory) => verify_decoded(
            &message,
            max_supported_schema_version,
            directory,
            now_unix_ms,
            replay,
        ),
    }
}

fn verify_decoded<M>(
    message: &M,
    max_supported_schema_version: u32,
    directory: &dyn KeyDirectory,
    now_unix_ms: u64,
    replay: &mut dyn ReplayGuard,
) -> Result<Verified<M>, IngressError>
where
    M: Signable + Clone,
{
    let verifier = Ed25519Verifier::new(directory);

    match verify(
        message,
        max_supported_schema_version,
        &verifier,
        now_unix_ms,
        replay,
    ) {
        Ok(verified) => Ok(verified),

        Err(VerifyError::ReplayStore(ReplayStoreError::LockTimeout)) => {
            // 락 타임아웃은 중복 메시지라는 뜻이 아니다.
            // 재시도하지 않고 저장소 장애로 명시해 fail-closed 한다.
            Err(IngressError::ReplayStoreUnavailable(
                ReplayStoreError::LockTimeout,
            ))
        }

        Err(VerifyError::ReplayStore(error)) => Err(IngressError::ReplayStoreUnavailable(error)),

        Err(error) => Err(IngressError::Verification(error)),
    }
}

/// `PersistentKeyring`과 [`Clock`]을 같은 시각으로 연결하는 보조 타입.
///
/// 실제 바이트 진입점은 [`decode_and_verify`] 하나다.
pub type PersistentDirectory<'a> = KeyDirectoryView<'a>;
