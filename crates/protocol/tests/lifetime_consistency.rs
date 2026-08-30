//! `Lifetime` 오지정 가드 — 독립 검수(2026-08-16) 지적 3번.
//!
//! # 무엇이 위험한가
//!
//! `Lifetime` 은 `Signable` 의 **연관 상수**일 뿐이다.
//! 한 글자만 바꿔도 컴파일되고, **검사 전체가 조용히 사라진다.**
//!
//! ```text
//! ShortLived -> Evidence
//!     만료 검사 사라짐 · skew 검사 사라짐 · replay 검사 사라짐
//!     그런데 replay_checked() 는 true 가 된다 (Evidence 는 대상이 아니므로)
//!     => 만료된 Grant 를 무한히 재생할 수 있다
//!
//! Evidence -> LongLived
//!     expires_at() 이 0 이므로 **모든 메시지가 즉시 만료**된다
//!     => 체크포인트 증거를 하나도 못 읽는다
//! ```
//!
//! 검수자: "이를 막는 코드는 못 찾았다. 개발자가 `const LIFETIME` 을
//! 잘못 수정해도 컴파일된다."
//!
//! # 이 파일의 접근
//!
//! 정책을 **메시지의 실제 능력**과 대조한다. 손으로 적은 목록이 아니라
//! 구조적 불변식이므로, 새 메시지를 추가해도 자동으로 검사된다.
//!
//! ```text
//! ShortLived  replay_nonce() 가 Some 이어야 한다 (§10 replay 대상)
//!             expires_at() 이 0 이 아니어야 한다  (즉시 만료 방지)
//!
//! Evidence    expires_at() 이 0 이어야 한다        (만료 개념 없음)
//!             replay_nonce() 가 None 이어야 한다   (§10 대상 아님)
//!
//! LongLived   expires_at() 이 0 이 아니어야 한다
//! ```

use gputeer_protocol::pb;
use gputeer_protocol::signing::{Lifetime, Signable};

const T: u64 = 1_755_200_000_000;

/// 한 메시지에 대해 `LIFETIME` 과 실제 능력이 맞는지 검사한다.
fn check<M: Signable>(name: &str, msg: &M) {
    let expires = msg.expires_at_unix_ms();
    let nonce = msg.replay_nonce().map(|n| n.len());

    match M::LIFETIME {
        Lifetime::ShortLived => {
            assert!(
                nonce.is_some(),
                "★ {name} 은 ShortLived 인데 replay_nonce() 가 None 이다 — \n\
                 verify() 가 항상 Replay 로 거부한다. §10 대상이면 nonce 필드가 있어야 한다"
            );
            assert_ne!(
                expires, 0,
                "★ {name} 은 ShortLived 인데 expires_at() 이 0 이다 — 즉시 만료된다"
            );
        }
        Lifetime::Evidence => {
            assert_eq!(
                expires, 0,
                "★ {name} 은 Evidence 인데 expires_at() 이 0 이 아니다.\n\
                 Evidence 는 만료 개념이 없다 (ADR-029). \n\
                 만료가 필요하면 LongLived 로 바꿔야 하고, 그것은 설계 변경이다"
            );
            assert!(
                nonce.is_none(),
                "★ {name} 은 Evidence 인데 replay_nonce() 가 Some 이다 — \n\
                 Evidence 는 §10 replay 대상이 아니므로 그 nonce 는 **검사되지 않는다.**\n\
                 nonce 가 필요하면 ShortLived 여야 한다"
            );
            assert_ne!(
                msg.observed_at_unix_ms(),
                0,
                "★ {name} 은 Evidence 인데 observed_at() 이 0 이다 — \n\
                 '언제인지 모르는 증거' 는 증거가 아니다 (ADR-029)"
            );
        }
        Lifetime::LongLived => {
            assert_ne!(
                expires, 0,
                "★ {name} 은 LongLived 인데 expires_at() 이 0 이다 — 즉시 만료된다"
            );
        }
        Lifetime::Perpetual => {}
    }
}

// ══════════════════════════════════════════════════════════════════
// `Signable` 을 구현한 메시지 전부
//
// ★ 여기 개수를 적지 않는다 — "10개" 라고 적혀 있었는데 실제로는
//   17개였다(2026-08-30 독립 검수 5라운드 지적). 빠짐은 아래
//   `every_signable_is_covered` 가 잡으므로 숫자는 불필요하다.
// ══════════════════════════════════════════════════════════════════

/// ★ `Signable` 을 구현한 **모든** 메시지가 여기 있어야 한다.
///
/// 새 메시지를 추가하고 여기 안 넣으면 `every_signable_is_covered` 가 잡는다.
#[test]
fn declared_lifetime_matches_message_capability() {
    check(
        "JobManifest",
        &pb::JobManifest {
            schema_version: 1,
            submitter_device_id: "d".into(),
            issued_at_unix_ms: T,
            expires_at_unix_ms: T + 1000,
            ..Default::default()
        },
    );
    check(
        "Lease",
        &pb::Lease {
            schema_version: 1,
            issued_at_unix_ms: T,
            expires_at_unix_ms: T + 1000,
            ..Default::default()
        },
    );
    check(
        "ExecutionGrant",
        &pb::ExecutionGrant {
            schema_version: 1,
            issued_at_unix_ms: T,
            expires_at_unix_ms: T + 60_000,
            nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
    check(
        "RenewLeaseRequest",
        &pb::RenewLeaseRequest {
            schema_version: 1,
            issued_at_unix_ms: T,
            nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
    check(
        "CheckpointManifest",
        &pb::CheckpointManifest {
            schema_version: 1,
            created_at_unix_ms: T,
            ..Default::default()
        },
    );
    check(
        "ReplicaAck",
        &pb::ReplicaAck {
            schema_version: 1,
            acked_at_unix_ms: T,
            ..Default::default()
        },
    );
    check(
        "ArtifactRef",
        &pb::ArtifactRef {
            schema_version: 1,
            created_at_unix_ms: T,
            ..Default::default()
        },
    );
    check(
        "AttemptReport",
        &pb::AttemptReport {
            schema_version: 1,
            issued_at_unix_ms: T,
            ..Default::default()
        },
    );
    check(
        "CanonicalDecision",
        &pb::CanonicalDecision {
            schema_version: 1,
            decided_at_unix_ms: T,
            ..Default::default()
        },
    );
    check(
        "RevokeLeaseNotice",
        &pb::RevokeLeaseNotice {
            schema_version: 1,
            issued_at_unix_ms: T,
            ..Default::default()
        },
    );
    check(
        "AgentGrantAck",
        &pb::AgentGrantAck {
            schema_version: 1,
            issued_at_unix_ms: T,
            expires_at_unix_ms: T + 60_000,
            nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
    check(
        "RenewLeaseResult",
        &pb::RenewLeaseResult {
            schema_version: 1,
            issued_at_unix_ms: T,
            request_nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
    check(
        "AgentSessionHello",
        &pb::AgentSessionHello {
            schema_version: 1,
            issued_at_unix_ms: T,
            nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
    check(
        "ResumeLeaseRequest",
        &pb::ResumeLeaseRequest {
            schema_version: 1,
            issued_at_unix_ms: T,
            request_nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
    check(
        "ResumeLeaseResult",
        &pb::ResumeLeaseResult {
            schema_version: 1,
            issued_at_unix_ms: T,
            request_nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
    // 노드 생존 보고(2026-08-29). ShortLived 이어야 한다 —
    // heartbeat 를 재생할 수 있으면 이미 죽은 노드를 살아 있는
    // 것처럼 보이게 만들 수 있다.
    check(
        "NodeHeartbeat",
        &pb::NodeHeartbeat {
            schema_version: 1,
            issued_at_unix_ms: T,
            request_nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
    // 이웃 신고(2026-08-30). ShortLived 이어야 한다 — heartbeat 와 방향이
    // 반대다. 신고를 재생할 수 있으면 **어제의 "연락이 안 된다" 가 오늘
    // 관측으로 되살아나** 지금 멀쩡한 노드가 연락 두절로 보인다.
    //
    // ★ 초안 주석은 "정족수를 한 건으로 채울 수 있다" 고 썼는데 **틀렸다**
    //   (2026-08-30 독립 검수 지적). `reassignment.rs` 가 `reporter_node_id`
    //   로 중복 제거하므로 같은 신고를 N 번 넣어도 한 표다. 재생 방어가
    //   막는 것은 **신선도 위조와 중복 부작용**이다.
    check(
        "NeighborUnreachableReport",
        &pb::NeighborUnreachableReport {
            schema_version: 1,
            observed_at_unix_ms: T,
            request_nonce: vec![0u8; 16],
            ..Default::default()
        },
    );
}

/// ★ `Signable` 을 구현한 메시지가 위 테스트에 **전부** 있는가.
///
/// 빠뜨리면 그 메시지의 `Lifetime` 은 아무도 검사하지 않는다.
#[test]
fn every_signable_is_covered() {
    let src = include_str!("../src/signable.rs");
    let mut impls: Vec<String> = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("impl Signable for pb::") {
            impls.push(rest.trim_end_matches(" {").to_string());
        }
    }
    assert!(
        impls.len() >= 10,
        "impl 을 {}개만 찾았다 — 파서 결함",
        impls.len()
    );

    let covered = include_str!("lifetime_consistency.rs");
    let missing: Vec<_> = impls
        .iter()
        .filter(|m| !covered.contains(&format!("\"{m}\",")))
        .collect();
    assert!(
        missing.is_empty(),
        "★ Signable 을 구현했는데 Lifetime 일관성 검사에 없는 메시지:\n  {missing:?}\n\
         그 메시지의 Lifetime 은 아무도 검사하지 않는다."
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 정책을 바꾸면 실제로 무엇이 사라지는가 — 위험을 고정한다
// ══════════════════════════════════════════════════════════════════

/// `Evidence` 와 `ShortLived` 가 **검사하는 것이 다르다**는 사실.
///
/// 이 테스트가 통과한다는 것은 곧 "정책을 바꾸면 검사가 사라진다" 는 뜻이다.
/// 위 `check()` 가 그 실수를 잡는 유일한 장치다.
#[test]
fn evidence_and_shortlived_differ_in_what_they_check() {
    // ShortLived 는 nonce 를 요구한다
    assert!(
        pb::ExecutionGrant::LIFETIME == Lifetime::ShortLived,
        "ExecutionGrant 의 Lifetime 이 바뀌었다"
    );
    let no_nonce = pb::ExecutionGrant {
        schema_version: 1,
        issued_at_unix_ms: T,
        expires_at_unix_ms: T + 60_000,
        nonce: vec![],
        ..Default::default()
    };
    assert!(
        Signable::replay_nonce(&no_nonce)
            .map(|n| n.is_empty())
            .unwrap_or(true),
        "nonce 가 비어 있어야 하는 테스트 전제"
    );

    // Evidence 는 nonce 를 아예 안 본다
    assert!(pb::CheckpointManifest::LIFETIME == Lifetime::Evidence);
    let c = pb::CheckpointManifest {
        schema_version: 1,
        created_at_unix_ms: T,
        ..Default::default()
    };
    assert!(
        Signable::replay_nonce(&c).is_none(),
        "Evidence 메시지가 nonce 를 노출한다 — §10 대상이 아닌데 검사되지 않는다"
    );
    assert_eq!(
        Signable::expires_at_unix_ms(&c),
        0,
        "Evidence 의 expires_at 이 0 이 아니다"
    );
}


/// ★ **재생이 사실을 조작하는 메시지**의 `Lifetime` 을 값으로 고정한다.
///
/// # 왜 이 테스트가 따로 필요한가
///
/// `declared_lifetime_matches_message_capability` 는 선언한 lifetime 의
/// **내부 일관성**만 본다 — `ShortLived` 를 `LongLived` 로 바꿔도
/// `expires_at != 0` 이면 그대로 통과한다. 즉 **강등을 아무도 못 잡는다.**
///
/// 2026-08-30 이웃 신고를 추가하며 뮤테이션으로 발견했다. 새 메시지만의
/// 문제가 아니라 `NodeHeartbeat` 도 같은 구멍이었으므로 둘 다 고정한다.
///
/// # 강등되면 무슨 일이 생기는가
///
/// ```text
/// NodeHeartbeat              재생하면 이미 죽은 노드가 살아 있어 보인다
/// NeighborUnreachableReport  재생하면 어제의 관측이 오늘 관측으로
///                            되살아나 살아 있는 노드가 연락 두절로 보인다
/// ```
#[test]
fn messages_whose_replay_would_forge_facts_must_stay_shortlived() {
    assert_eq!(
        <pb::NodeHeartbeat as Signable>::LIFETIME,
        Lifetime::ShortLived,
        "★ NodeHeartbeat 의 lifetime 이 강등됐다 — 재생하면 죽은 노드가 살아 보인다"
    );
    assert_eq!(
        <pb::NeighborUnreachableReport as Signable>::LIFETIME,
        Lifetime::ShortLived,
        "★ NeighborUnreachableReport 의 lifetime 이 강등됐다 — 재생하면 어제의 관측이 오늘 관측으로 되살아난다"
    );
}
