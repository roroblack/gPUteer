//! `decode_and_verify` 를 **부르는** 계층 — 프레이밍 + 타입 판별 + 소비 경계.
//!
//! # 왜 이 파일이 있나 (2026-08-17)
//!
//! `ingress.rs` 는 raw bytes -> `Verified<M>` 를 조립한다.
//! 그런데 그것을 부르는 것이 아무것도 없었다 — `decode_and_verify<M>` 은
//! **제네릭**이라 호출자가 `M` 을 미리 알아야 한다.
//!
//! 실제 통신은 하나의 바이트 스트림 위에서 **여러 메시지 타입**을 주고받는다.
//! 그래서 "이 프레임이 어떤 타입인가" 를 정하는 계층이 필요하다.
//!
//! ```text
//! 바이트 스트림 -> 프레임 경계 -> 헤더의 타입 힌트로 처리기 선택
//!               -> decode_and_verify::<그 타입>() -> Verified<M> -> 처리기
//! ```
//!
//! # ★ "검증 전에는 아무것도 신뢰하지 않는다" 와의 모순을 어떻게 푸는가
//!
//! `CLAUDE.md` §0.2 — 검증 전 필드 값을 로직에 쓰지 않는다.
//! 그런데 타입을 알아야 어떤 `decode_and_verify::<M>()` 을 부를지 정할 수 있고,
//! 그 타입 정보는 **서명 대상이 아닌 헤더**에서 온다 — 위조 가능하다.
//!
//! **답: 헤더의 타입 태그는 "무엇을 시도할지" 를 정할 뿐, "무엇이 진짜인지"
//! 를 결정하지 않는다.** 공격자가 태그를 속이면:
//!
//! ```text
//! 태그: Grant 라고 주장   실제 바이트: Lease 메시지
//!   -> decode_and_verify::<ExecutionGrant>() 를 시도한다
//!   -> prost decode 단계에서 필드 형태가 안 맞아 실패하거나,
//!      우연히 decode 되더라도 서명은 Lease::DOMAIN 으로 만들어졌으므로
//!      sig_input 의 domain_tag 가 달라 서명 검증이 반드시 실패한다.
//! ```
//!
//! `M::DOMAIN` 은 **타입에서 정적으로** 나온다 (메시지 필드가 아니다).
//! 그래서 헤더 태그가 무엇이든 검증 결과는 "그 타입으로 실제로 서명됐는가"
//! 만 따진다. **헤더 태그를 신뢰하는 것이 아니라, 헤더 태그가 틀렸을 때
//! 검증이 반드시 실패한다는 성질에 기댄다.**
//!
//! 이것이 안전한 이유: 헤더 태그를 신뢰해서 하는 일은 "어느 파서를 시도할지"
//! 뿐이고, 그 결과로 나온 `Verified<M>` 는 여전히 독립적으로 검증된 값이다.
//! 잘못된 파서를 골랐다고 해서 검증되지 않은 값이 새어 나가지 않는다.
//!
//! ★ **정정 (독립 검수 2026-08-17).** 위 문단이 "타입을 속이면 실패하는
//! 이유는 domain_tag 불일치 때문이다" 라고 뭉뚱그렸는데, 실패 이유는
//! **두 가지**로 갈린다. `AttemptReport` 와 `ArtifactRef` 는 필드 1·2·4·90
//! (스칼라 타입까지)이 겹쳐서 **한쪽으로 서명된 바이트가 다른 쪽으로도
//! prost 디코드되고 canonical 필드까지 같아진다** — 그래도 `domain_tag`
//! 는 타입에서 정적으로 오므로 서명 검증은 그 지점에서 실패한다.
//! 반면 `Lease` 를 `Grant` 라고 주장하는 경우는 `Grant` 의 필드 3 이
//! 메시지 타입(`JobManifest`)이라 애초에 **prost decode 단계에서** 실패할
//! 수 있다 — 그 경우는 domain_tag 방어를 시험하지 못한다. 두 시나리오를
//! `tests/framed_ingress.rs` 에 각각 이름 붙여 분리했다.
//!
//! # 전송 계층은 만들지 않는다
//!
//! TCP · Unix socket · Windows named pipe 중 무엇을 쓸지는 여기서 정하지
//! 않는다. `std::io::Read` 하나만 요구한다. 비동기 런타임(tokio)도 넣지
//! 않는다 — coordinator/agent 가 아직 없어서 **동시 연결을 몇 개나
//! 감당해야 하는지 모른다.** 모르는 채로 tokio 를 넣으면 그 선택 자체가
//! 근거 없는 것이 된다(`CLAUDE.md` §3.3 YAGNI). 필요해지면 그때 잰다.
//!
//! # 소유자 주권과의 관계
//!
//! `CLAUDE.md` §0.1 — Owner Panel 은 `127.0.0.1` 고정이다.
//! 이 모듈은 **소켓을 열지 않는다.** `Read` 를 받을 뿐이다.
//! 바인딩 주소를 정하는 것은 이 모듈을 부르는 쪽(agent)의 책임이며,
//! 그 결정을 여기로 가져오지 않는다 — 그러면 이 모듈이 "왜 127.0.0.1 이
//! 아닌지" 를 설명해야 하는 처지가 된다.

use std::io::Read;

use gputeer_protocol::constants::MAX_INGRESS_FRAME_BYTES;
use gputeer_protocol::pb;
use gputeer_protocol::signing::{ReplayGuard, Verified};

use crate::ingress::{decode_and_verify, Clock, IngressError, KeyDirectorySource};

/// 프레임 헤더의 타입 힌트.
///
/// ★ 이 값은 **서명 대상이 아니다.** 위조될 수 있다는 전제로 다룬다.
/// 모듈 문서의 "모순을 어떻게 푸는가" 절 참조.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    Manifest = 1,
    Grant = 2,
    Lease = 3,
    LeaseRenew = 4,
    LeaseRevoke = 5,
    AttemptReport = 6,
    Checkpoint = 7,
    Artifact = 8,
    ReplicaAck = 9,
    GrantAck = 10,
    LeaseRenewResult = 11,
    SessionHello = 12,
    LeaseResume = 13,
    LeaseResumeResult = 14,
}

impl FrameType {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Manifest,
            2 => Self::Grant,
            3 => Self::Lease,
            4 => Self::LeaseRenew,
            5 => Self::LeaseRevoke,
            6 => Self::AttemptReport,
            7 => Self::Checkpoint,
            8 => Self::Artifact,
            9 => Self::ReplicaAck,
            10 => Self::GrantAck,
            11 => Self::LeaseRenewResult,
            12 => Self::SessionHello,
            13 => Self::LeaseResume,
            14 => Self::LeaseResumeResult,
            _ => return None,
        })
    }
}

/// 프레이밍 · 디코드 실패.
///
/// ★ [`IngressError`] 와 분리한다. 이것은 "프레임을 못 읽었다" 이고
/// `IngressError` 는 "프레임은 읽었는데 검증에 실패했다" 다.
/// 섞으면 전송 오류와 정책 위반이 같은 값으로 보인다.
#[derive(Debug)]
pub enum FramingError {
    /// 스트림이 헤더를 다 보내기 전에 끊겼다.
    Truncated,
    /// 프레임 길이가 [`MAX_INGRESS_FRAME_BYTES`] 를 넘는다고 주장했다.
    ///
    /// 그 크기를 실제로 읽지 않는다 — **주장된 길이만으로 즉시 거부한다.**
    /// 그러지 않으면 상한 검사 자체가 무의미해진다(어차피 다 읽어야 하므로).
    FrameTooLarge { claimed: u32, max: u32 },
    /// 헤더의 타입 태그가 알려진 값이 아니다.
    UnknownFrameType(u8),
    /// 프레임은 읽었지만 그 타입으로 검증할 수 없었다.
    Verify(IngressError),
    /// 스트림 읽기 자체가 실패했다 (연결 끊김 등).
    Io(std::io::Error),
}

impl std::fmt::Display for FramingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(f, "프레임이 완결되기 전에 스트림이 끊겼다"),
            Self::FrameTooLarge { claimed, max } => {
                write!(
                    f,
                    "프레임 크기 {claimed} 바이트가 상한 {max} 바이트를 넘는다"
                )
            }
            Self::UnknownFrameType(t) => write!(f, "알 수 없는 프레임 타입: {t}"),
            Self::Verify(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "스트림 읽기 실패: {e}"),
        }
    }
}

impl std::error::Error for FramingError {}

impl From<std::io::Error> for FramingError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// **검증을 통과한** 메시지. 처리기는 이 값만 받는다 — 원본 바이트나
/// 미검증 필드는 노출하지 않는다.
///
/// `Debug` 를 파생한다 — `Verified<M>` 는 내부 필드를 노출하지 않는
/// 자체 `Debug` 만 갖고 있어(§13.1 타입 게이트), 여기 담긴 값을 그대로
/// 찍어도 검증 우회 경로가 되지 않는다. 테스트 실패 메시지에 쓴다.
#[derive(Debug)]
pub enum IngressMessage {
    Manifest(Verified<pb::JobManifest>),
    Grant(Verified<pb::ExecutionGrant>),
    Lease(Verified<pb::Lease>),
    LeaseRenew(Verified<pb::RenewLeaseRequest>),
    LeaseRevoke(Verified<pb::RevokeLeaseNotice>),
    AttemptReport(Verified<pb::AttemptReport>),
    Checkpoint(Verified<pb::CheckpointManifest>),
    Artifact(Verified<pb::ArtifactRef>),
    ReplicaAck(Verified<pb::ReplicaAck>),
    GrantAck(Verified<pb::AgentGrantAck>),
    LeaseRenewResult(Verified<pb::RenewLeaseResult>),
    SessionHello(Verified<pb::AgentSessionHello>),
    LeaseResume(Verified<pb::ResumeLeaseRequest>),
    LeaseResumeResult(Verified<pb::ResumeLeaseResult>),
}

/// 헤더(5바이트: type 1 + len 4)를 읽고 본문을 읽어, 헤더가 가리키는
/// 타입으로 검증까지 마친 [`IngressMessage`] 하나를 반환한다.
///
/// # 처리기가 실패하면
///
/// 이 함수는 **처리기를 부르지 않는다** — 검증까지만 한다.
/// 처리기 실행은 호출자의 책임이다. 이렇게 가른 이유:
///
/// ```text
/// 검증 실패     상대가 신뢰할 수 없는 데이터를 보냈다는 프로토콜 사실
/// 처리기 실패   우리 쪽 로직·자원 문제(디스크 가득 참 등)
/// ```
///
/// 검증은 통과했는데 처리기가 실패하면, 그 메시지는 **다시 시도할 가치가
/// 있다** — 서명은 진짜이기 때문이다. 이 함수 안에서 처리까지 묶으면
/// 그 구분이 사라진다.
///
/// ★ 오류 응답을 서명해서 돌려주는 것은 이 함수의 책임이 아니다.
///   에러 하나를 서명하려면 그 자체가 새 `Signable` 대상이 되어야 하고
///   (`RULE.md` §3.5 의 5단계), 지금 소비자가 없는 상태에서 그 계약을
///   먼저 만드는 것은 범위를 넘는다.
///
/// # ★ 오류 뒤 스트림을 계속 읽어도 되는가 (독립 검수 2026-08-17 · 정정)
///
/// **`FrameTooLarge` 는 스트림을 끝낸다.** `claimed_len` 이 상한을 넘으면
/// 그 바이트를 실제로 읽지 않는다 — 공격자가 주장한 길이(최대 4GiB 근처)
/// 를 그대로 소비하려 드는 것 자체가 새로운 DoS 경로이기 때문이다.
/// 그 결과 스트림에는 다 못 읽은 몸통이 그대로 남고, **다음 `read_frame`
/// 호출은 그 잔여 바이트를 새 헤더로 오해한다.** 이 오류를 받은 호출자는
/// 연결을 닫아야 한다 — `unusable_stream_state_after_frame_too_large` 가
/// 그 위험을 실제로 재현해 고정한다.
///
/// **`UnknownFrameType` 은 스트림을 끝내지 않는다.** 길이는 이미 상한
/// 이내로 확인했으므로(8MiB 까지) 몸통을 안전하게 마저 읽어 버릴 수 있다
/// — 그러면 스트림 위치가 다음 프레임 헤더와 맞아떨어진다.
///
/// # DoS — 응답 없는 상대 (독립 검수 2026-08-17 · 알려진 한계)
///
/// `claimed_len` 이 상한 이내면 그만큼 즉시 `Vec` 를 할당하고
/// `read_exact` 로 **타임아웃 없이** 기다린다. 상대가 헤더만 보내고
/// 몸통을 안 보내면 이 호출은 무기한 블로킹한다.
///
/// 이 모듈은 `std::io::Read` 만 요구하고 타임아웃 개념이 없다(모듈
/// 문서 "전송 계층은 만들지 않는다" 참조) — 그래서 여기서 막을 수 없다.
/// **호출자가 소켓 수준에서 읽기 타임아웃을 걸어야 한다**
/// (예: `TcpStream::set_read_timeout`). 이 함수는 그 책임을 대신하지
/// 않는다는 사실을 감추지 않는다.
pub fn read_frame<R: Read>(
    stream: &mut R,
    max_supported_schema_version: u32,
    key_directory: KeyDirectorySource<'_>,
    replay: &mut dyn ReplayGuard,
    clock: &dyn Clock,
) -> Result<IngressMessage, FramingError> {
    let mut header = [0u8; 5];
    read_exact_or_truncated(stream, &mut header)?;

    let claimed_len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]);

    // ★ 타입 유효성보다 **길이 상한을 먼저** 본다 (독립 검수 2026-08-17).
    //   순서를 바꾼 이유: 타입이 알 수 없어도 길이가 상한 이내면 몸통을
    //   안전하게 비워 스트림을 맞출 수 있다. 반대로 길이가 상한을 넘으면
    //   타입이 무엇이든 그 몸통은 읽지 않는다 — 그것이 DoS 경로다.
    if claimed_len > MAX_INGRESS_FRAME_BYTES {
        return Err(FramingError::FrameTooLarge {
            claimed: claimed_len,
            max: MAX_INGRESS_FRAME_BYTES,
        });
    }

    let frame_type = match FrameType::from_u8(header[0]) {
        Some(t) => t,
        None => {
            // ★ 길이는 이미 상한 이내로 확인됐다 — 안전하게 비운다.
            //   그러지 않으면 이 오류 뒤 스트림이 다음 헤더와 어긋난다.
            let mut discard = vec![0u8; claimed_len as usize];
            read_exact_or_truncated(stream, &mut discard)?;
            return Err(FramingError::UnknownFrameType(header[0]));
        }
    };

    let mut body = vec![0u8; claimed_len as usize];
    read_exact_or_truncated(stream, &mut body)?;

    macro_rules! verify_as {
        ($variant:ident, $ty:ty) => {
            decode_and_verify::<$ty>(
                &body,
                max_supported_schema_version,
                key_directory,
                replay,
                clock,
            )
            .map(IngressMessage::$variant)
            .map_err(FramingError::Verify)
        };
    }

    match frame_type {
        FrameType::Manifest => verify_as!(Manifest, pb::JobManifest),
        FrameType::Grant => verify_as!(Grant, pb::ExecutionGrant),
        FrameType::Lease => verify_as!(Lease, pb::Lease),
        FrameType::LeaseRenew => verify_as!(LeaseRenew, pb::RenewLeaseRequest),
        FrameType::LeaseRevoke => verify_as!(LeaseRevoke, pb::RevokeLeaseNotice),
        FrameType::AttemptReport => verify_as!(AttemptReport, pb::AttemptReport),
        FrameType::Checkpoint => verify_as!(Checkpoint, pb::CheckpointManifest),
        FrameType::Artifact => verify_as!(Artifact, pb::ArtifactRef),
        FrameType::ReplicaAck => verify_as!(ReplicaAck, pb::ReplicaAck),
        FrameType::GrantAck => verify_as!(GrantAck, pb::AgentGrantAck),
        FrameType::LeaseRenewResult => verify_as!(LeaseRenewResult, pb::RenewLeaseResult),
        FrameType::SessionHello => verify_as!(SessionHello, pb::AgentSessionHello),
        FrameType::LeaseResume => verify_as!(LeaseResume, pb::ResumeLeaseRequest),
        FrameType::LeaseResumeResult => verify_as!(LeaseResumeResult, pb::ResumeLeaseResult),
    }
}

fn read_exact_or_truncated<R: Read>(stream: &mut R, buf: &mut [u8]) -> Result<(), FramingError> {
    match stream.read_exact(buf) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Err(FramingError::Truncated),
        Err(e) => Err(FramingError::Io(e)),
    }
}

/// 프레임 하나를 인코딩한다 (테스트 · 발신 측에서 재사용).
///
/// ★ 2026-08-17 정정 (독립 검수). 전에는 `body.len() as u32` 로 **무검사
/// 캐스팅**했다 — `body` 가 4GiB 를 넘으면 길이 필드가 조용히 잘려
/// **다른 프레임을 만들었다.** 그리고 [`MAX_INGRESS_FRAME_BYTES`] 도
/// 검사하지 않아서, 이 함수로 만든 프레임을 `read_frame` 이 그대로
/// `FrameTooLarge` 로 거부하는 비대칭이 있었다 — 쓰기와 읽기가 같은
/// 규칙을 안 지켰다.
pub fn write_frame(frame_type: FrameType, body: &[u8]) -> Result<Vec<u8>, FramingError> {
    let len: u32 = body
        .len()
        .try_into()
        .map_err(|_| FramingError::FrameTooLarge {
            claimed: u32::MAX,
            max: MAX_INGRESS_FRAME_BYTES,
        })?;
    if len > MAX_INGRESS_FRAME_BYTES {
        return Err(FramingError::FrameTooLarge {
            claimed: len,
            max: MAX_INGRESS_FRAME_BYTES,
        });
    }
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(frame_type as u8);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    Ok(out)
}
