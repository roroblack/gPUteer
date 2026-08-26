//! 참여 모델 선택자 계약 (ADR-031 · ADR-032).
//!
//! 이 테스트가 고정하는 핵심은 **조용히 기본값으로 떨어지지 않는다** 는 것이다.
//! 팀 모드인 줄 알고 낯선 코드를 돌리는 것이 이 설계에서 가장 위험한 사고다.

use gputeer_protocol::{ParticipationModel, ParticipationModelError};

#[test]
fn the_two_canonical_spellings_round_trip() {
    for model in [
        ParticipationModel::PrivateTeam,
        ParticipationModel::PublicPool,
    ] {
        let text = model.as_str();
        assert_eq!(
            ParticipationModel::parse(text),
            Ok(model),
            "정본 표기 {text:?} 가 왕복하지 않는다"
        );
        assert_eq!(model.to_string(), text, "Display 가 정본 표기와 다르다");
    }
}

#[test]
fn surrounding_whitespace_is_tolerated() {
    assert_eq!(
        ParticipationModel::parse("  private-team\n"),
        Ok(ParticipationModel::PrivateTeam)
    );
}

/// ★ 빈 값은 기본값이 아니라 오류다. 설정을 깜빡한 상태가 조용히 한쪽
///   모드로 떨어지면 안 된다.
#[test]
fn an_absent_value_is_an_error_not_a_default() {
    for empty in ["", "   ", "\t\n"] {
        assert_eq!(
            ParticipationModel::parse(empty),
            Err(ParticipationModelError::Missing),
            "빈 값 {empty:?} 이 기본값으로 처리됐다"
        );
    }
}

/// ★ 모르는 값을 조용히 무시하지 않는다. `SCHEMA_TOO_NEW` 를 `VALID` 로
///   취급하지 않는 것과 같은 이유다 — 구버전이 새 안전 제약을 모른 채
///   통과시키면 안 된다.
#[test]
fn an_unknown_value_is_rejected_with_the_offending_text() {
    let err = ParticipationModel::parse("hybrid-pool").unwrap_err();
    assert_eq!(
        err,
        ParticipationModelError::Unknown {
            value: "hybrid-pool".to_string()
        }
    );
    assert!(
        err.to_string().contains("hybrid-pool"),
        "오류 메시지가 문제의 값을 보여주지 않는다: {err}"
    );
}

/// 표기 변형을 받아주지 않는다. 받아주면 어느 것이 정본인지 흐려지고
/// 로그·설정 파일·문서가 서로 달라진다.
#[test]
fn spelling_variants_are_not_silently_accepted() {
    for variant in [
        "Private-Team",
        "PRIVATE-TEAM",
        "private_team",
        "privateteam",
        "public pool",
        "publicPool",
    ] {
        assert!(
            matches!(
                ParticipationModel::parse(variant),
                Err(ParticipationModelError::Unknown { .. })
            ),
            "표기 변형 {variant:?} 이 조용히 받아들여졌다"
        );
    }
}

/// 위협 모델 전제가 두 모드에서 실제로 갈린다.
#[test]
fn only_the_private_team_assumes_mutual_trust() {
    assert!(ParticipationModel::PrivateTeam.assumes_mutual_trust());
    assert!(
        !ParticipationModel::PublicPool.assumes_mutual_trust(),
        "공개 풀이 상호 신뢰를 전제하면 낯선 코드에 격리를 낮추게 된다"
    );
}

/// 기본값을 만들지 않았음을 컴파일 시점에 고정한다.
///
/// `Default` 가 생기면 `ParticipationModel::default()` 가 컴파일되고,
/// 설정 누락이 조용히 한쪽 모드로 떨어진다. 이 doctest 가 그것을 막는다.
///
/// ```compile_fail
/// use gputeer_protocol::ParticipationModel;
/// let _silently_defaulted = ParticipationModel::default();
/// ```
#[test]
fn there_is_no_default_see_the_compile_fail_doctest() {
    // 본문은 비어 있다 — 계약은 위 doctest 가 강제한다.
}
