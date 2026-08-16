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

use gputeer_protocol::signing::{Lifetime, Signable};
use gputeer_protocol::pb;

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
// 10개 메시지 전부
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
    assert!(impls.len() >= 10, "impl 을 {}개만 찾았다 — 파서 결함", impls.len());

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
        Signable::replay_nonce(&no_nonce).map(|n| n.is_empty()).unwrap_or(true),
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
