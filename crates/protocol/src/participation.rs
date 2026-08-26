//! 참여 모델 — 사설 팀 / 공개 풀 (ADR-031 · ADR-032).
//!
//! # 왜 타입인가
//!
//! 두 모델은 **위협 모델이 다르다.** 사설 팀은 참여자가 서로 아는 사이라고
//! 전제하지만, 공개 풀에서는 **모르는 사람의 코드가 내 GPU 에서 돈다.**
//! 그래서 "지금 어느 모드인가" 를 추론하거나 기본값으로 흘려보내면 안 된다 —
//! 팀 모드인 줄 알고 낯선 코드를 돌리는 것이 이 설계에서 가장 위험한 사고다.
//!
//! ```text
//! PrivateTeam   신뢰 앵커 = Genesis + Owner 키        (기준선 §7.2)
//! PublicPool    신뢰 앵커 = Broker 공개키              (ADR-032)
//! ```
//!
//! # 이 모듈이 하는 것과 하지 않는 것
//!
//! **하는 것**: 모드를 명시적 타입으로 표현하고, 문자열에서 읽을 때 알 수 없는
//! 값·빈 값·모호한 값을 **거부**한다. 기본값을 만들지 않는다.
//!
//! **하지 않는 것**: 모드에 따라 동작을 분기하는 배선을 넣지 않는다.
//!
//! ★ 이유가 있다. ADR-031 은 모드 의존점(이음매)이 membership 과 trust anchor
//!   **두 곳뿐**이라고 주장했으나 독립 검수가 이를 **반증**했다 —
//!   `owner_member_id` 의 의미론, `DoD-54` 의 membership 다치성
//!   (`Valid`/`NotApproved`/`Unresolved`/`Ambiguous`), `DoD-55` 의
//!   `ProvenanceGate`, durable load 의 재검증 경로까지 **최소 여섯 곳**이다.
//!   이음매가 어디인지 확정되기 전에 배선을 넣으면 **틀렸다고 판명난 구조를
//!   코드로 굳히게 된다.** 그래서 선택자만 먼저 둔다.
//!
//! # 왜 `Default` 를 구현하지 않는가
//!
//! 기본값이 있으면 "설정을 깜빡한" 상태가 조용히 한쪽 모드로 떨어진다.
//! 그 방향이 `PrivateTeam` 이면 공개 풀에서 격리가 낮아지고, `PublicPool`
//! 이면 팀이 불필요한 문턱에 막힌다. 어느 쪽이든 조용한 오작동이므로
//! **호출자가 반드시 명시**하게 한다.

use std::fmt;

/// 참여 모델. 추론하지 않고 명시된 값만 쓴다.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ParticipationModel {
    /// 기준선 §7 그대로. Owner 는 선출되지 않고 팀을 만든 사람이다.
    /// 참여자는 서로 아는 사이라고 전제한다.
    PrivateTeam,
    /// ADR-032. Broker 가 멤버십 authority 를 갖고 리더는 스케줄링만 한다.
    /// 참여자는 서로 모른다고 전제하므로 격리 요구를 낮출 수 없다.
    PublicPool,
}

/// 모드를 읽지 못한 이유. 어느 경우에도 기본값으로 대체하지 않는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParticipationModelError {
    /// 값이 아예 없다. 설정 누락이다.
    Missing,
    /// 아는 값이 아니다. 오타이거나 이 빌드가 모르는 새 모드다.
    ///
    /// ★ 모르는 모드를 조용히 무시하지 않는다. `SCHEMA_TOO_NEW` 를 `VALID`
    ///   로 취급하지 않는 것과 같은 이유다(`CLAUDE.md` §0.2) — 구버전이
    ///   새 안전 제약을 모른 채 통과시키면 안 된다.
    Unknown { value: String },
}

impl fmt::Display for ParticipationModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(
                f,
                "참여 모델이 지정되지 않았다 — 기본값은 없다. \
                 'private-team' 또는 'public-pool' 을 명시해야 한다"
            ),
            Self::Unknown { value } => write!(
                f,
                "알 수 없는 참여 모델 {value:?} — 이 빌드가 모르는 값이다. \
                 조용히 무시하지 않는다"
            ),
        }
    }
}

impl std::error::Error for ParticipationModelError {}

impl ParticipationModel {
    /// 설정 문자열에서 읽는다. 앞뒤 공백만 허용하고 그 외 변형은 거부한다.
    ///
    /// 대소문자를 접지 않는다. `Private-Team` 을 받아주면 어느 표기가
    /// 정본인지 흐려지고, 나중에 로그·설정 파일·문서가 서로 달라진다.
    pub fn parse(value: &str) -> Result<Self, ParticipationModelError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(ParticipationModelError::Missing);
        }
        match trimmed {
            "private-team" => Ok(Self::PrivateTeam),
            "public-pool" => Ok(Self::PublicPool),
            other => Err(ParticipationModelError::Unknown {
                value: other.to_string(),
            }),
        }
    }

    /// 설정에 쓰는 정본 표기. `parse` 와 왕복한다.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrivateTeam => "private-team",
            Self::PublicPool => "public-pool",
        }
    }

    /// 이 모드에서 참여자가 서로 신뢰할 수 있다고 전제해도 되는가.
    ///
    /// ★ 이 값이 거짓이면 낯선 코드가 도는 것이므로 격리 요구를 낮출 수
    ///   없다. 구체적인 격리 등급은 아직 정해지지 않았다(ADR-032 §검수 반영
    ///   (F) — "최고 등급" 의 구체 값·검증 지점 미정). 그래서 여기서
    ///   등급을 정하지 않고 **전제만** 노출한다. 등급을 지금 하드코딩하면
    ///   검증되지 않은 값이 굳는다.
    pub fn assumes_mutual_trust(self) -> bool {
        match self {
            Self::PrivateTeam => true,
            Self::PublicPool => false,
        }
    }
}

impl fmt::Display for ParticipationModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
