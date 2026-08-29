//! T1b — `grant` · `membership` · `policy` · `quarantine` domain.
//!
//! # 이 파일의 두 축
//!
//! ```text
//! 1. ExecutionGrant — 규칙 i 가 **두 번** 적용되는 유일한 메시지
//!    (a) manifest_hash(4)  도출 해시 필드      -> 제외
//!    (b) 중첩 manifest/lease 의 서명(90)        -> 재귀 제외
//!    => Agent 는 manifest 를 독립 검증하고 hash 를 재계산해야 한다(MUST)
//!
//! 2. 한 domain_tag 를 공유하는 메시지들 (§5.1)
//!    membership 6종 · quarantine 2종.
//!    domain 분리가 없으므로 **canonical 차이만이 서명 재사용을 막는다.**
//! ```

use std::path::PathBuf;

use gputeer_protocol::canonical::canonical_encode;
use gputeer_protocol::{pb, ToCanonicalFields, DERIVED_HASH_FIELDS};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn expect_hex(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/vectors/canonical_v1.json");
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p).expect("벡터 파일")).expect("JSON");
    doc["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("벡터 없음: {name}"))["canonical_hex"]
        .as_str()
        .unwrap()
        .to_string()
}

fn canon<T: ToCanonicalFields>(m: &T) -> Vec<u8> {
    canonical_encode(&m.to_canonical_fields(), &[])
}

// ══════════════════════════════════════════════════════════════════
// ExecutionGrant
// ══════════════════════════════════════════════════════════════════

fn minimal_manifest(sig: u8) -> pb::JobManifest {
    pb::JobManifest {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        team_id: "01JBXR7Q0000000000000000TT".into(),
        entrypoint: "train.py".into(),
        submitter_device_id: "01JBXR7Q0000000000000000DD".into(),
        issued_at_unix_ms: 1_755_100_800_000,
        expires_at_unix_ms: 1_755_705_600_000,
        submitter_signature: vec![sig; 64],
        ..Default::default()
    }
}

fn grant(manifest_sig: u8, manifest_hash: u8) -> pb::ExecutionGrant {
    pb::ExecutionGrant {
        schema_version: 2,
        grant_id: "01JBXGRANT0000000000000001".into(),
        manifest: Some(minimal_manifest(manifest_sig)),
        manifest_hash: Some(pb::Digest {
            algo: 1,
            value: vec![manifest_hash; 32],
        }),
        attempt_id: "01JBXATT00000000000000001".into(),
        lease: Some(pb::Lease {
            schema_version: 1,
            lease_id: "01JBXLEASE0000000000000001".into(),
            job_id: "01JBXR7Q0000000000000000AA".into(),
            attempt_id: "01JBXATT00000000000000001".into(),
            fence_epoch: 42,
            coordinator_term: 7,
            holder_node_id: "node-1".into(),
            issued_at_unix_ms: 1_755_100_800_000,
            expires_at_unix_ms: 1_755_100_860_000,
            coordinator_signature: vec![0xCD; 64],
            ..Default::default()
        }),
        peers: vec![pb::PeerHint {
            node_id: "node-2".into(),
            peer_id: "12D3KooWExample".into(),
            multiaddrs: vec!["/ip4/10.20.20.2/udp/4001/quic-v1".into()],
            known_digests: vec![],
        }],
        creds: Some(pb::EphemeralCredential {
            credential_id: "01JBXCRED0000000000000001".into(),
            token: b"\xDE\xAD\xBE\xEF".repeat(4),
            expires_at_unix_ms: 1_755_100_860_000,
            allowed_endpoints: vec!["hub.internal:443".into()],
        }),
        plan: Some(pb::GrantedExecutionPlan {
            mode: 1,
            gpu_allocation: 1,
            assigned_gpu_uuids: vec!["GPU-11111111-2222-3333-4444-555555555555".into()],
            remote_replication_interval_minutes: 15,
            effective_durability: 3,
            // ★ 배치 근거도 서명 대상이다 (감사 무결성)
            rationale: Some(pb::PlacementRationale {
                t_est_seconds: 10800,
                sigma_ln_ppm: 150_000,
                stage: 2,
                p_within_estimate_ppm: 900_000,
                p_survival_ppm: 979_700,
                p_success_ppm: 881_730,
                target_confidence_ppm: 800_000,
                is_exploration: false,
                rejected: vec![pb::RejectedCandidate {
                    node_id: "node-9".into(),
                    reason: 2,
                    detail: "insufficient VRAM".into(),
                }],
            }),
        }),
        coordinator_device_id: "01JBXR7Q0000000000000000CC".into(),
        coordinator_term: 7,
        issued_at_unix_ms: 1_755_100_800_000,
        expires_at_unix_ms: 1_755_100_860_000,
        nonce: (0u8..16).collect(),
        lease_from_durable_store: true,
        coordinator_signature: vec![0xFE; 64],
    }
}

#[test]
fn execution_grant_matches_reference() {
    assert_eq!(
        hex(&canon(&grant(0xAA, 0x01))),
        expect_hex("v25_execution_grant"),
        "ExecutionGrant canonical 이 참조 구현과 다르다"
    );
}

/// ★ 규칙 i 가 **두 곳**에 적용된다.
///
/// 도출 해시(`manifest_hash`)와 중첩 서명(`manifest.submitter_signature`)을
/// **둘 다** 바꿔도 Grant 의 canonical 은 같아야 한다.
///
/// 이것은 결함이 아니라 규칙 i 의 필연적 결과다. 그러나 결과를 알아야 한다:
/// **Agent 는 manifest 를 독립 검증하고 `manifest_hash` 를 재계산해야 한다(MUST).**
/// 하지 않으면 서명이 벗겨진 매니페스트로 Job 을 실행하게 된다.
#[test]
fn derived_hash_and_nested_signature_are_both_excluded() {
    let a = canon(&grant(0xAA, 0x01));
    let b = canon(&grant(0xBB, 0x02));
    assert_eq!(
        a, b,
        "도출 해시 또는 중첩 서명이 Grant canonical 에 들어갔다 (규칙 i)"
    );
    assert_eq!(
        hex(&b),
        expect_hex("v25b_execution_grant_derived_hash_and_nested_sig_swapped")
    );

    // 이 사실이 코드에 선언되어 있는가
    assert!(
        DERIVED_HASH_FIELDS
            .iter()
            .any(|(m, n, _)| *m == "ExecutionGrant" && *n == 4),
        "manifest_hash 가 DERIVED_HASH_FIELDS 에 선언되지 않았다 — \
         '의도적 제외'와 '실수로 누락'을 구분할 수 없다"
    );
}

/// 반대로 manifest 의 **내용**은 반영되어야 한다.
///
/// 위 테스트만 있으면 "중첩 manifest 전체가 무시되는" 결함과 구분되지 않는다.
#[test]
fn nested_manifest_content_does_affect_grant_canonical() {
    let base = canon(&grant(0xAA, 0x01));

    let mut g = grant(0xAA, 0x01);
    g.manifest.as_mut().unwrap().entrypoint = "evil.py".into();
    let changed = canon(&g);
    assert_ne!(changed, base, "중첩 manifest 의 내용이 반영되지 않았다");
    assert_eq!(
        hex(&changed),
        expect_hex("v25c_execution_grant_manifest_content_changed")
    );

    // manifest 를 통째로 빼도 달라져야 한다
    let mut g2 = grant(0xAA, 0x01);
    g2.manifest = None;
    assert_ne!(canon(&g2), base);

    // lease 도 마찬가지
    let mut g3 = grant(0xAA, 0x01);
    g3.lease.as_mut().unwrap().fence_epoch = 99;
    assert_ne!(canon(&g3), base, "중첩 lease 의 fence_epoch 이 서명 밖이다");
}

/// Grant 의 핵심 필드가 전부 서명에 반영되는가.
#[test]
fn every_grant_field_affects_canonical() {
    let base = canon(&grant(0xAA, 0x01));
    type Mut = Box<dyn Fn(&mut pb::ExecutionGrant)>;
    let cases: Vec<(&str, Mut)> = vec![
        (
            "grant_id(2)",
            Box::new(|g: &mut pb::ExecutionGrant| g.grant_id.clear()),
        ),
        (
            "attempt_id(5)",
            Box::new(|g: &mut pb::ExecutionGrant| g.attempt_id.clear()),
        ),
        (
            "peers(7)",
            Box::new(|g: &mut pb::ExecutionGrant| g.peers.clear()),
        ),
        (
            "creds(8)",
            Box::new(|g: &mut pb::ExecutionGrant| g.creds = None),
        ),
        (
            "plan(9)",
            Box::new(|g: &mut pb::ExecutionGrant| g.plan = None),
        ),
        (
            "coordinator_device_id(20)",
            Box::new(|g: &mut pb::ExecutionGrant| g.coordinator_device_id.clear()),
        ),
        (
            "coordinator_term(21)",
            Box::new(|g: &mut pb::ExecutionGrant| g.coordinator_term = 0),
        ),
        (
            "issued_at(22)",
            Box::new(|g: &mut pb::ExecutionGrant| g.issued_at_unix_ms = 0),
        ),
        (
            "expires_at(23)",
            Box::new(|g: &mut pb::ExecutionGrant| g.expires_at_unix_ms = 0),
        ),
        // ★ nonce 가 서명 밖이면 replay 캐시를 우회할 수 있다
        (
            "nonce(24)",
            Box::new(|g: &mut pb::ExecutionGrant| g.nonce.clear()),
        ),
        (
            "lease_from_durable_store(25)",
            Box::new(|g: &mut pb::ExecutionGrant| g.lease_from_durable_store = false),
        ),
    ];

    let mut unsigned = Vec::new();
    for (name, mutate) in &cases {
        let mut g = grant(0xAA, 0x01);
        mutate(&mut g);
        if canon(&g) == base {
            unsigned.push(*name);
        }
    }
    assert!(
        unsigned.is_empty(),
        "지워도 canonical 이 변하지 않는 필드 = 서명 밖 = 위조 가능:\n  {}",
        unsigned.join("\n  ")
    );

    // ★ 자격증명 토큰 — 서명 밖이면 다른 토큰으로 바꿔칠 수 있다
    let mut g = grant(0xAA, 0x01);
    g.creds.as_mut().unwrap().token = vec![0xFF; 16];
    assert_ne!(canon(&g), base, "EphemeralCredential.token 이 서명 밖이다");

    // 할당된 GPU — 서명 밖이면 다른 GPU 를 쓸 수 있다
    let mut g2 = grant(0xAA, 0x01);
    g2.plan.as_mut().unwrap().assigned_gpu_uuids = vec!["GPU-other".into()];
    assert_ne!(canon(&g2), base, "assigned_gpu_uuids 가 서명 밖이다");

    // ★ 배치 근거 — 서명 밖이면 Coordinator 가 "왜 이 노드를 골랐는가" 를
    //   사후에 조작할 수 있다. 분쟁 시 유일한 기록이다.
    let mut g3 = grant(0xAA, 0x01);
    g3.plan
        .as_mut()
        .unwrap()
        .rationale
        .as_mut()
        .unwrap()
        .p_success_ppm = 999_999;
    assert_ne!(canon(&g3), base, "PlacementRationale 이 서명 밖이다");

    let mut g4 = grant(0xAA, 0x01);
    g4.plan
        .as_mut()
        .unwrap()
        .rationale
        .as_mut()
        .unwrap()
        .rejected
        .clear();
    assert_ne!(canon(&g4), base, "탈락 후보 기록이 서명 밖이다");
}

// ══════════════════════════════════════════════════════════════════
// ★ 한 domain_tag 를 공유하는 메시지들 (§5.1)
// ══════════════════════════════════════════════════════════════════

/// `membership` domain 은 6개 메시지가 **하나의 tag 를 공유**한다.
/// domain 분리가 없으므로 **canonical 차이만이 서명 재사용을 막는다.**
///
/// 이 테스트가 실패하면 한 메시지의 서명을 다른 메시지로 재사용할 수 있다는 뜻이다.
#[test]
fn membership_messages_have_distinct_canonicals() {
    let id = "01JBXMEM00000000000000001";

    let add = pb::AddMember {
        member_id: id.into(),
        public_key: (0u8..32).collect(),
        role: "member".into(),
        owner_signature: vec![0x11; 64],
    };
    let remove = pb::RemoveMember {
        member_id: id.into(),
        owner_signature: vec![0x11; 64],
    };
    let approve = pb::ApproveDevice {
        device_id: id.into(),
        member_id: id.into(),
        public_key: (0u8..32).collect(),
        peer_id: "12D3KooW".into(),
        key_protection: 2,
        is_ephemeral: false,
        owner_signature: vec![0x11; 64],
    };
    let revoke = pb::RevokeDevice {
        device_id: id.into(),
        reason: "compromised".into(),
        signatures: vec![vec![0x11; 64]],
    };
    let rotate = pb::RotateOwnerKey {
        new_owner_public_key: (0u8..32).collect(),
        new_recovery_public_key: (32u8..64).collect(),
        authorizing_signature: vec![0x11; 64],
    };

    let encodings = vec![
        ("AddMember", canon(&add)),
        ("RemoveMember", canon(&remove)),
        ("ApproveDevice", canon(&approve)),
        ("RevokeDevice", canon(&revoke)),
        ("RotateOwnerKey", canon(&rotate)),
    ];

    for (i, (na, a)) in encodings.iter().enumerate() {
        for (nb, b) in encodings.iter().skip(i + 1) {
            assert_ne!(
                a, b,
                "★ {na} 와 {nb} 의 canonical 이 같다. 두 메시지는 같은 domain_tag 를 \
                 공유하므로(§5.1) 서명을 서로 재사용할 수 있다"
            );
        }
    }

    assert_eq!(hex(&encodings[0].1), expect_hex("v26_add_member"));
    assert_eq!(hex(&encodings[1].1), expect_hex("v26b_remove_member"));
}

/// ★ ADR-028 회귀 방지 — **서명이 메시지 간에 전이되지 않는가.**
///
/// # 왜 canonical 이 아니라 sig_input 을 보는가
///
/// 처음 이 테스트를 canonical 비교로 썼고 **실패했다.**
/// `QuarantineDevice{device_id}` 와 `ReleaseQuarantine{device_id}` 는
/// 최소 형태에서 canonical 이 **바이트 단위로 같다** — 규칙 b 로 나머지가 생략되므로.
///
/// 그때는 두 메시지가 `gputeer/v1/quarantine` 하나를 공유했으므로
/// **격리 판정 서명이 격리 해제 서명으로 그대로 통과했다.**
///
/// ADR-028 이 tag 를 분리해 이것을 막았다. 그런데 **canonical 은 여전히 같다** —
/// 그래서 검사 대상은 canonical 이 아니라 `sig_input` 이다.
/// canonical 을 검사하면 "우연히 필드가 달라서 통과" 하는 약한 보증밖에 못 얻는다.
#[test]
fn signatures_do_not_transfer_between_control_messages() {
    use gputeer_protocol::canonical::{sig_input, Domain};

    let dev = "01JBXR7Q0000000000000000DD";

    let q = pb::QuarantineDevice {
        device_id: dev.into(),
        signals: vec![],
        target_is_coordinator: false,
        verdict_signatures: vec![vec![0x33; 64]],
    };
    let r = pb::ReleaseQuarantine {
        device_id: dev.into(),
        reason: String::new(),
        owner_signature: vec![0x33; 64],
    };

    // ★ canonical 은 여전히 같다. 이것이 tag 분리가 필요했던 이유다.
    assert_eq!(
        canon(&q),
        canon(&r),
        "이 테스트의 전제가 바뀌었다 — 두 메시지의 canonical 이 달라졌다면 \
         ADR-028 의 근거를 재확인하라"
    );

    // 그러나 sig_input 은 달라야 한다 — domain_tag 가 다르므로.
    assert_ne!(
        sig_input(Domain::QuarantineDevice, 1, &canon(&q)),
        sig_input(Domain::QuarantineRelease, 1, &canon(&r)),
        "★ 격리 판정 서명이 격리 해제 서명으로 재사용된다 (ADR-028 회귀)"
    );

    // membership 6종도 마찬가지 — 최소 형태에서 canonical 이 겹친다
    let id = "01JBXMEM00000000000000001";
    let add = pb::AddMember {
        member_id: id.into(),
        ..Default::default()
    };
    let rem = pb::RemoveMember {
        member_id: id.into(),
        ..Default::default()
    };
    let rev = pb::RevokeDevice {
        device_id: id.into(),
        ..Default::default()
    };
    assert_eq!(
        canon(&add),
        canon(&rem),
        "전제 변경 — ADR-028 근거 재확인 필요"
    );
    assert_eq!(
        canon(&rem),
        canon(&rev),
        "전제 변경 — ADR-028 근거 재확인 필요"
    );

    let inputs = [
        ("AddMember", sig_input(Domain::MemberAdd, 1, &canon(&add))),
        (
            "RemoveMember",
            sig_input(Domain::MemberRemove, 1, &canon(&rem)),
        ),
        (
            "RevokeDevice",
            sig_input(Domain::DeviceRevoke, 1, &canon(&rev)),
        ),
    ];
    for (i, (na, a)) in inputs.iter().enumerate() {
        for (nb, b) in inputs.iter().skip(i + 1) {
            assert_ne!(
                a, b,
                "★ {na} 서명이 {nb} 로 재사용된다 (ADR-028 회귀). \
                 canonical 이 같으므로 domain_tag 만이 방어다"
            );
        }
    }
}

/// domain_tag 가 실제로 24종 전부 서로 다른가.
///
/// ADR-028 이 6종을 추가했다(17→23). `GrantAck` 가 그 뒤 7번째로
/// 추가됐다(23→24, 2026-08-18). 오타로 두 tag 가 같아지면
/// 그 두 메시지 사이의 방어가 조용히 사라진다.
///
/// ★ 이 배열은 `Domain` enum 을 순회하지 않고 손으로 쓴 목록이다 —
///   `domain_coverage_is_explicit`(`t1_signing_targets.rs`)와 같은
///   구조적 한계를 안고 있다. 새 variant 를 추가할 때 여기 갱신을
///   잊으면 그 variant 의 중복 여부가 이 테스트로는 검출되지
///   않는다(2026-08-18, DoD-06 schema v2 승격 재검수에서 stale
///   수치와 함께 지적됨).
#[test]
fn all_domain_tags_are_distinct() {
    use gputeer_protocol::canonical::Domain;
    // ★ 수동 배열을 쓰지 않는다. 이 저장소는 그 배열이 새 domain 을
    //   못 잡는 결함을 **세 번** 겪었다(DoD-02·DoD-06·2026-08-29).
    //   `Domain::ALL` 은 같은 파일의 exhaustive `match` 가 지키므로,
    //   variant 를 추가하면 컴파일이 깨져 잊을 수가 없다.
    let all = gputeer_protocol::canonical::Domain::ALL;
    let mut seen = std::collections::HashMap::new();
    for &d in all {
        if let Some(prev) = seen.insert(d.tag_bytes(), d) {
            panic!("domain_tag 중복: {prev:?} 와 {d:?} 가 같은 tag 를 쓴다");
        }
    }
    assert_eq!(
        seen.len(),
        Domain::ALL.len(),
        "domain 중 일부가 같은 tag 로 접혔다"
    );
}

// ══════════════════════════════════════════════════════════════════
// 다중 서명 메시지
// ══════════════════════════════════════════════════════════════════

/// `repeated bytes signatures = 90` 도 규칙 i 로 제외되는가.
#[test]
fn multi_signature_field_is_excluded() {
    let base = pb::UpdatePolicy {
        policy_hash: Some(pb::Digest {
            algo: 1,
            value: vec![0x66; 32],
        }),
        policy_content: b"max_egress_bps: 0\n".to_vec(),
        is_relaxation: true,
        signatures: vec![],
    };
    let c0 = canon(&base);

    let mut many = base.clone();
    many.signatures = vec![vec![0x22; 64], vec![0x33; 64], vec![0x44; 64]];
    assert_eq!(
        canon(&many),
        c0,
        "다중 서명 필드(90)가 canonical 에 들어갔다 — 서명 개수가 늘 때마다 canonical 이 바뀐다"
    );
    assert_eq!(hex(&c0), expect_hex("v27_update_policy"));

    // ★ is_relaxation 이 서명 밖이면 완화를 강화로 위장할 수 있다
    let mut tightened = base.clone();
    tightened.is_relaxation = false;
    assert_ne!(canon(&tightened), c0, "is_relaxation 이 서명 밖이다");
}

#[test]
fn quarantine_verdict_matches_reference() {
    let q = pb::QuarantineDevice {
        device_id: "01JBXR7Q0000000000000000DD".into(),
        signals: vec![
            pb::RiskSignal {
                kind: "hash_mismatch".into(),
                detail: "checkpoint digest differs".into(),
                observed_at_unix_ms: 1_755_103_000_000,
                observer_coordinator_id: "coord-a".into(),
            },
            pb::RiskSignal {
                kind: "lease_violation".into(),
                detail: "wrote after revoke".into(),
                observed_at_unix_ms: 1_755_103_100_000,
                observer_coordinator_id: "coord-b".into(),
            },
        ],
        target_is_coordinator: true,
        verdict_signatures: vec![vec![0x33; 64]],
    };
    assert_eq!(hex(&canon(&q)), expect_hex("v28_quarantine_device"));

    // ★ Coordinator 격리와 워커 격리는 위험도가 다르다. 서명 밖이면 위장 가능하다.
    let mut worker = q.clone();
    worker.target_is_coordinator = false;
    assert_ne!(
        canon(&worker),
        canon(&q),
        "target_is_coordinator 가 서명 밖이다"
    );

    // 근거 신호를 지워도 canonical 이 변해야 한다 — 근거 없는 격리를 막는다
    let mut no_signals = q.clone();
    no_signals.signals.clear();
    assert_ne!(canon(&no_signals), canon(&q));
}

#[test]
fn change_coordinator_set_matches_reference_and_preserves_order() {
    let entry = |id: &str, k: u8, dom: &str| pb::CoordinatorEntry {
        device_id: id.into(),
        public_key: vec![k; 32],
        endpoints: vec![format!("10.20.20.{k}:7000")],
        failure_domain: dom.into(),
    };
    let c = pb::ChangeCoordinatorSet {
        new_set: vec![entry("coord-a", 1, "rack-a"), entry("coord-b", 2, "rack-b")],
        added_id: "coord-b".into(),
        removed_id: String::new(),
        owner_signature: vec![0x44; 64],
    };
    assert_eq!(hex(&canon(&c)), expect_hex("v29_change_coordinator_set"));

    // 규칙 d — 순서 유지
    let mut rev = c.clone();
    rev.new_set.reverse();
    assert_ne!(
        canon(&rev),
        canon(&c),
        "coordinator 집합 순서가 반영되지 않았다"
    );

    // ★ failure_domain 이 서명 밖이면 quorum 이 같은 랙에 몰려도 알 수 없다
    let mut same_rack = c.clone();
    same_rack.new_set[1].failure_domain = "rack-a".into();
    assert_ne!(
        canon(&same_rack),
        canon(&c),
        "failure_domain 이 서명 밖이다"
    );
}

// ══════════════════════════════════════════════════════════════════
// 코덱스 지적 — §6.1 manifest_hash 의 **값 자체**는 검증되지 않았다
// ══════════════════════════════════════════════════════════════════

/// `signing.md` §6.1 — `manifest_hash = BLAKE3_256(sig_input_of(JobManifest))`
///
/// 기존 벡터(v25/v25b)는 "manifest_hash 를 바꿔도 canonical 이 같다" 만 확인했다.
/// 그것은 규칙 i 의 검증이지 **§6.1 공식의 검증이 아니다.**
///
/// ★ Agent 는 이 값을 신뢰하지 않고 **재계산해 대조해야 한다(MUST)**
///   — 계획서 §15.4 검증 13단계. 재계산 공식이 여기 고정된다.
#[test]
fn manifest_hash_formula_matches_spec() {
    use gputeer_protocol::canonical::{blake3_256, sig_input, Domain};
    use gputeer_protocol::signing::Signable;

    let m = minimal_manifest(0xAA);

    // §6.1 공식
    let canon = canonical_encode(&ToCanonicalFields::to_canonical_fields(&m), &[]);
    let si = sig_input(Domain::Manifest, Signable::schema_version(&m), &canon);
    let expected = blake3_256(&si);

    // signing_input() 이 같은 바이트를 내는가 — 서명 경로와 해시 경로가 갈리면 안 된다
    assert_eq!(
        gputeer_protocol::signing::signing_input(&m),
        si,
        "signing_input() 과 §6.1 의 sig_input 이 다르다"
    );

    // ★ 서명 필드를 채워도 manifest_hash 는 변하지 않아야 한다.
    //   변하면 "서명하려면 해시가 필요하고 해시하려면 서명이 필요한" 순환이 생긴다.
    let mut signed = minimal_manifest(0xAA);
    signed.submitter_signature = vec![0xFF; 64];
    let si2 = gputeer_protocol::signing::signing_input(&signed);
    assert_eq!(
        blake3_256(&si2),
        expected,
        "서명 필드가 manifest_hash 에 영향을 준다 — 자기참조 순환"
    );

    // 내용이 바뀌면 해시도 바뀌어야 한다 (비공허성)
    let mut other = minimal_manifest(0xAA);
    other.entrypoint = "other.py".into();
    assert_ne!(
        blake3_256(&gputeer_protocol::signing::signing_input(&other)),
        expected
    );

    println!("manifest_hash = {}", hex(&expected));
}

/// ★ Grant 안의 `manifest_hash` 가 **실제 매니페스트의 해시가 아닐 수 있다.**
///
/// 규칙 i 로 canonical 에서 제외되므로 Grant 서명이 이 값을 보증하지 않는다.
/// 그래서 §6.1 이 "Agent 는 재계산해 대조한다(MUST)" 를 요구한다.
///
/// 이 테스트는 **그 위험을 고정한다** — 통과한다는 것이
/// "프로토콜이 불일치를 막지 못한다" 는 뜻이다.
#[test]
fn grant_manifest_hash_can_be_wrong_without_breaking_signature() {
    use gputeer_protocol::canonical::blake3_256;

    let g = grant(0xAA, 0x01); // manifest_hash = [0x01; 32] — 명백히 틀린 값
    let real = blake3_256(&gputeer_protocol::signing::signing_input(
        g.manifest.as_ref().unwrap(),
    ));

    assert_ne!(
        g.manifest_hash.as_ref().unwrap().value.as_slice(),
        real.as_slice(),
        "이 테스트의 전제가 깨졌다 — 벡터의 manifest_hash 가 우연히 맞았다"
    );

    // 그런데 Grant 의 canonical 은 정상이다 — 서명이 이 불일치를 잡지 못한다
    let mut fixed = grant(0xAA, 0x01);
    fixed.manifest_hash = Some(pb::Digest {
        algo: 1,
        value: real.to_vec(),
    });
    assert_eq!(
        canon(&g),
        canon(&fixed),
        "★ manifest_hash 가 canonical 에 들어갔다면 이 테스트를 재검토하라"
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ §6.1 manifest_hash 대조 — verify() 가 실제로 하는가
// ══════════════════════════════════════════════════════════════════

/// `DERIVED_HASH_FIELDS` 에 있는 메시지가 `check_derived_consistency()` 를
/// **실제로 덮어썼는가.**
///
/// ★ 기본 구현은 no-op 이라 **덮어쓰지 않아도 컴파일된다.**
///   타입이 강제하지 못하므로 이 테스트가 강제한다.
#[test]
fn derived_hash_messages_override_consistency_check() {
    use gputeer_protocol::signing::Signable;

    // 목록에 있는 메시지는 ExecutionGrant 뿐이다
    let msgs: Vec<&str> = DERIVED_HASH_FIELDS.iter().map(|(m, _, _)| *m).collect();
    assert_eq!(
        msgs,
        vec!["ExecutionGrant"],
        "DERIVED_HASH_FIELDS 가 바뀌었다 — 새 메시지도 \
         check_derived_consistency() 를 덮어썼는지 이 테스트에 추가하라"
    );

    // ★ 덮어썼는지 확인 — 틀린 해시를 넣었을 때 오류가 나야 한다.
    //   기본 구현(no-op)이 쓰였다면 Ok 가 나온다.
    let g = grant(0xAA, 0x01); // manifest_hash = [0x01; 32] — 명백히 틀린 값
    assert!(
        Signable::check_derived_consistency(&g).is_err(),
        "★ ExecutionGrant 가 check_derived_consistency() 를 덮어쓰지 않았다 — \
         기본 no-op 이 조용히 쓰이고 있다"
    );
}
