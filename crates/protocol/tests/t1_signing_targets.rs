//! T1 — `artifact.proto` / `lease.proto` 서명 대상의 참조 구현 대조.
//!
//! `signing.md` §5 domain_tag 중 이 파일이 다루는 것:
//! (★ 여기 개수를 적지 않는다 — 적으면 목록이 늘어도 숫자만 낡는다.
//!  2026-08-30 독립 검수가 "23종" 이 30종이 된 뒤에도 남아 있음을 짚었다)
//! `checkpoint` · `replica-ack` · `artifact` · `attempt-report` · `canonical` ·
//! `lease-renew` · `lease-revoke`
//!
//! # 이 파일에서 가장 중요한 두 가지
//!
//! ```text
//! 1. 규칙 j (부호 있는 정수)
//!    스키마 전체에서 유일한 int64 가 하필 서명 대상 안에 있었다.
//!    규칙이 없으면 구현마다 다르게 인코딩한다 (2의 보수 vs zigzag).
//!
//! 2. 중첩 서명 메시지
//!    규칙 i 는 **재귀 적용**된다. 중첩 서명은 바깥 canonical 에 들어가지 않는다.
//!    -> 중첩 서명을 바꿔치기해도 바깥 서명은 깨지지 않는다.
//!    -> 검증자는 중첩 서명 메시지를 독립적으로 검증해야 한다(MUST).
//! ```

use std::path::PathBuf;

use gputeer_protocol::canonical::canonical_encode;
use gputeer_protocol::{pb, ToCanonicalFields};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn vectors() -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/vectors/canonical_v1.json");
    serde_json::from_slice(&std::fs::read(p).expect("벡터 파일")).expect("JSON")
}

fn expect_hex(name: &str) -> String {
    vectors()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("벡터 없음: {name}"))["canonical_hex"]
        .as_str()
        .unwrap()
        .to_string()
}

/// 벡터의 다른 hex 필드(`sig_input_hex` 등).
fn expect_vector_field(name: &str, field: &str) -> String {
    vectors()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("벡터 없음: {name}"))[field]
        .as_str()
        .unwrap_or_else(|| panic!("{name} 에 {field} 가 없다"))
        .to_string()
}

fn digest(b: u8) -> Option<pb::Digest> {
    Some(pb::Digest {
        algo: 1,
        value: vec![b; 32],
    })
}

// ══════════════════════════════════════════════════════════════════
// ★ 규칙 j — 부호 있는 정수
// ══════════════════════════════════════════════════════════════════

fn report_with_metrics(metrics: Vec<pb::ReportedMetric>) -> pb::AttemptReport {
    pb::AttemptReport {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        node_id: "node-1".into(),
        fence_epoch: 42,
        outcome: 1,
        final_step: 20000,
        started_at_unix_ms: 1_755_100_800_000,
        finished_at_unix_ms: 1_755_104_400_000,
        metrics,
        issued_at_unix_ms: 1_755_104_400_000,
        node_signature: vec![0xEF; 64],
        ..Default::default()
    }
}

fn metric(name: &str, v: i64, higher: bool) -> pb::ReportedMetric {
    pb::ReportedMetric {
        name: name.into(),
        value_micro: v,
        higher_is_better: higher,
    }
}

#[test]
fn rule_j_signed_integers_match_reference() {
    let m = report_with_metrics(vec![
        metric("loss", 1_234_567, false),
        metric("loss_delta", -456_789, false),
        metric("min_i64", i64::MIN, true),
        metric("max_i64", i64::MAX, true),
    ]);
    assert_eq!(
        hex(&canonical_encode(&m.to_canonical_fields(), &[])),
        expect_hex("v20_signed_integers_rule_j"),
        "부호 있는 정수 인코딩이 참조 구현과 다르다 (규칙 j)"
    );
}

/// ★ 부호가 canonical 에 반영되는가.
///
/// 반영되지 않으면 `-1000` 짜리 지표를 `+1000` 으로 바꿔도 서명이 통과한다.
#[test]
fn rule_j_sign_affects_canonical() {
    let pos = canonical_encode(
        &report_with_metrics(vec![metric("m", 1000, true)]).to_canonical_fields(),
        &[],
    );
    let neg = canonical_encode(
        &report_with_metrics(vec![metric("m", -1000, true)]).to_canonical_fields(),
        &[],
    );
    assert_ne!(pos, neg, "부호가 canonical 에 반영되지 않았다");

    assert_eq!(hex(&pos), expect_hex("v20a_int_positive"));
    assert_eq!(hex(&neg), expect_hex("v20b_int_negative"));

    // 규칙 j — 음수는 2의 보수 u64 재해석이므로 **항상 정확히 10바이트**다.
    // zigzag 였다면 -1000 은 2바이트가 되어 pos 와 길이가 같았을 것이다.
    assert_eq!(
        neg.len() - pos.len(),
        8,
        "음수 varint 가 10바이트가 아니다 — zigzag 로 인코딩됐을 수 있다"
    );
}

/// 0 은 규칙 b 로 생략된다. `-0` 은 정수에 존재하지 않으므로 §g 문제가 없다.
#[test]
fn rule_j_zero_is_omitted() {
    let zero = canonical_encode(
        &report_with_metrics(vec![metric("m", 0, true)]).to_canonical_fields(),
        &[],
    );
    let absent = canonical_encode(
        &report_with_metrics(vec![pb::ReportedMetric {
            name: "m".into(),
            higher_is_better: true,
            ..Default::default()
        }])
        .to_canonical_fields(),
        &[],
    );
    assert_eq!(zero, absent, "명시적 0 이 canonical 에 들어갔다");
}

/// 경계값 — `i64::MIN` 은 절대값을 취할 수 없다. 순진한 구현이 여기서 패닉한다.
#[test]
fn rule_j_handles_i64_min_without_panic() {
    for v in [i64::MIN, i64::MIN + 1, -1, 1, i64::MAX] {
        let c = canonical_encode(
            &report_with_metrics(vec![metric("m", v, true)]).to_canonical_fields(),
            &[],
        );
        assert!(!c.is_empty(), "{v} 인코딩 실패");
    }
}

// ══════════════════════════════════════════════════════════════════
// ★ 중첩 서명 메시지 — 규칙 i 의 재귀 적용
// ══════════════════════════════════════════════════════════════════

fn artifact_with_ack_signature(sig: u8) -> pb::ArtifactRef {
    pb::ArtifactRef {
        schema_version: 1,
        artifact_id: "01JBXARTF0000000000000001".into(),
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        kind: 1,
        digest: digest(0x55),
        size_bytes: 1_073_741_824,
        cas_path: "jobs/01JBXR7Q0000000000000000AA/attempt-3/model.tar".into(),
        replicas: vec![pb::ReplicaAck {
            schema_version: 1,
            checkpoint_id: "01JBXCKPT0000000000000001".into(),
            root_digest: digest(0x44),
            holder_device_id: "01JBXR7Q0000000000000000HH".into(),
            kind: 3,
            failure_domain: "rack-a".into(),
            fsynced: true,
            hash_verified: true,
            stored_bytes: 1_073_741_824,
            acked_at_unix_ms: 1_755_103_500_000,
            holder_signature: vec![sig; 64],
        }],
        created_at_unix_ms: 1_755_103_600_000,
        fence_epoch: 42,
        producer_signature: vec![0x77; 64],
    }
}

/// ★ 중첩 서명은 바깥 canonical 에 **들어가지 않는다** (규칙 i 재귀).
///
/// 이것은 결함이 아니라 규칙 i 의 필연적 결과다. 그러나 **결과를 알아야 한다** —
/// 검증자가 중첩 서명 메시지를 독립적으로 검증하지 않으면
/// 중간자가 `ReplicaAck` 의 서명만 갈아끼워도 바깥 검증이 통과한다.
///
/// `ReplicaAck` 는 "이 복제본이 실제로 durable 하다" 는 증거이므로,
/// 검증되지 않은 ACK 를 세면 **REPLICATED(n) 이 거짓이 된다.**
#[test]
fn nested_signature_is_excluded_from_outer_canonical() {
    let a = canonical_encode(
        &artifact_with_ack_signature(0xAA).to_canonical_fields(),
        &[],
    );
    let b = canonical_encode(
        &artifact_with_ack_signature(0xBB).to_canonical_fields(),
        &[],
    );

    assert_eq!(
        a, b,
        "중첩 서명이 바깥 canonical 에 들어갔다 — 규칙 i 가 재귀 적용되지 않았다"
    );
    assert_eq!(hex(&a), expect_hex("v22_artifact_ref_nested_signature"));
    assert_eq!(
        hex(&b),
        expect_hex("v22b_artifact_ref_swapped_nested_signature")
    );
}

/// 반면 중첩 메시지의 **내용**은 바깥 canonical 에 반영되어야 한다.
///
/// 위 테스트만 있으면 "중첩 전체가 무시되는" 결함과 구분되지 않는다.
#[test]
fn nested_message_content_does_affect_outer_canonical() {
    let base = canonical_encode(
        &artifact_with_ack_signature(0xAA).to_canonical_fields(),
        &[],
    );

    // failure_domain 을 바꾼다 — REPLICATED(n) 계산의 핵심 입력이다
    let mut tampered = artifact_with_ack_signature(0xAA);
    tampered.replicas[0].failure_domain = "rack-b".into();
    assert_ne!(
        canonical_encode(&tampered.to_canonical_fields(), &[]),
        base,
        "중첩 메시지의 failure_domain 이 서명 밖이다 — 같은 박스 2개를 REPLICATED(2) 로 위조 가능"
    );

    // fsynced 를 끈다 — durability 주장의 근거다
    let mut t2 = artifact_with_ack_signature(0xAA);
    t2.replicas[0].fsynced = false;
    assert_ne!(canonical_encode(&t2.to_canonical_fields(), &[]), base);

    // replica 를 통째로 뺀다
    let mut t3 = artifact_with_ack_signature(0xAA);
    t3.replicas.clear();
    assert_ne!(canonical_encode(&t3.to_canonical_fields(), &[]), base);
}

// ══════════════════════════════════════════════════════════════════
// 나머지 T1 메시지 — 참조 구현 대조
// ══════════════════════════════════════════════════════════════════

#[test]
fn checkpoint_manifest_matches_reference() {
    let m = pb::CheckpointManifest {
        schema_version: 1,
        checkpoint_id: "01JBXCKPT0000000000000001".into(),
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        step: 12000,
        epoch: 3,
        files: vec![
            pb::CheckpointFile {
                path: "model/weights.safetensors".into(),
                digest: digest(0x11),
                size_bytes: 5_368_709_120,
                chunk_digests: vec![
                    pb::Digest {
                        algo: 1,
                        value: vec![0x21; 32],
                    },
                    pb::Digest {
                        algo: 1,
                        value: vec![0x22; 32],
                    },
                ],
                chunk_size_bytes: 4 * 1024 * 1024,
            },
            pb::CheckpointFile {
                path: "optim/state.pt".into(),
                digest: digest(0x33),
                size_bytes: 10_737_418_240,
                ..Default::default()
            },
        ],
        root_digest: digest(0x44),
        total_bytes: 16_106_127_360,
        completeness: Some(pb::ResumeCompleteness {
            model_weights: true,
            optimizer_state: true,
            lr_scheduler_state: true,
            rng_state: true,
            sampler_position: true,
            dataloader_position: true,
            amp_scaler_state: true,
            full_resume_guaranteed: true,
        }),
        created_at_unix_ms: 1_755_103_000_000,
        producer_node_id: "node-1".into(),
        fence_epoch: 42,
        producer_signature: vec![0x99; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&m.to_canonical_fields(), &[])),
        expect_hex("v21_checkpoint_manifest"),
        "repeated message 안의 repeated message 인코딩이 참조 구현과 다르다"
    );
}

/// 규칙 d — 체크포인트 파일 순서는 유지된다 (proto 주석: "정렬하지 않는다").
#[test]
fn checkpoint_file_order_is_preserved() {
    let f = |p: &str, d: u8| pb::CheckpointFile {
        path: p.into(),
        digest: digest(d),
        size_bytes: 100,
        ..Default::default()
    };
    let mut a = pb::CheckpointManifest {
        schema_version: 1,
        checkpoint_id: "c".into(),
        files: vec![f("a.pt", 1), f("b.pt", 2)],
        ..Default::default()
    };
    let c1 = canonical_encode(&a.to_canonical_fields(), &[]);
    a.files.reverse();
    assert_ne!(
        canonical_encode(&a.to_canonical_fields(), &[]),
        c1,
        "파일 순서가 canonical 에 반영되지 않았다 (규칙 d)"
    );
}

#[test]
fn renew_lease_request_matches_reference() {
    let r = pb::RenewLeaseRequest {
        schema_version: 1,
        lease_id: "01JBXLEASE0000000000000001".into(),
        fence_epoch: 42,
        node_id: "node-1".into(),
        progress: Some(pb::ProgressReport {
            current_step: 12000,
            total_steps: 20000,
            eta_seconds: 3400,
            last_committed_step: 11500,
            replication_backlog_bytes: 2_147_483_648,
        }),
        issued_at_unix_ms: 1_755_103_700_000,
        nonce: (0u8..16).collect(),
        node_signature: vec![0x88; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&r.to_canonical_fields(), &[])),
        expect_hex("v23_renew_lease_request")
    );

    // ★ nonce 는 서명 대상이어야 한다. 아니면 재전송 시 nonce 만 갈아끼울 수 있다.
    let mut other = r.clone();
    other.nonce = vec![0xFF; 16];
    assert_ne!(
        canonical_encode(&other.to_canonical_fields(), &[]),
        canonical_encode(&r.to_canonical_fields(), &[]),
        "nonce 가 서명 밖이다 — replay 캐시를 우회할 수 있다"
    );
}

/// ★ `AgentGrantAck` 는 coordinator/agent 핸드셰이크(2026-08-18)가
/// 추가한 뒤로 참조 구현(Python) 대조를 한 번도 받은 적이 없었다 —
/// `DoD-05` schema v2 승격 재검수에서 발견된 공백(CLAUDE.md 백로그
/// 6번). `tools/canonical/reference_canonical.py` 에 `v32`·`v32b`
/// 벡터를 추가하고 여기서 대조한다.
#[test]
fn agent_grant_ack_matches_reference() {
    let a = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: "01JBXGRANT0000000000000001".into(),
        attempt_id: "01JBXATTEMPT000000000000001".into(),
        agent_device_id: "agent-1".into(),
        issued_at_unix_ms: 1_755_103_900_000,
        expires_at_unix_ms: 1_755_103_960_000,
        nonce: (0u8..16).collect(),
        accepted: true,
        agent_signature: vec![0x99; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&a.to_canonical_fields(), &[])),
        expect_hex("v32_agent_grant_ack")
    );

    // ★ nonce 는 서명 대상이어야 한다 — RenewLeaseRequest 와 같은 이유.
    let mut other = a.clone();
    other.nonce = (16u8..32).collect();
    assert_eq!(
        hex(&canonical_encode(&other.to_canonical_fields(), &[])),
        expect_hex("v32b_agent_grant_ack_different_nonce")
    );
    assert_ne!(
        canonical_encode(&other.to_canonical_fields(), &[]),
        canonical_encode(&a.to_canonical_fields(), &[]),
        "nonce 가 서명 밖이다 — replay 캐시를 우회할 수 있다"
    );
}

#[test]
fn renew_lease_result_matches_reference() {
    let lease = pb::Lease {
        schema_version: 1,
        lease_id: "01JBXLEASE0000000000000001".into(),
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        fence_epoch: 43,
        coordinator_term: 7,
        holder_node_id: "node-1".into(),
        issued_at_unix_ms: 1_755_103_900_000,
        expires_at_unix_ms: 1_755_103_960_000,
        coordinator_signature: vec![0xCD; 64],
        ..Default::default()
    };
    let r = pb::RenewLeaseResult {
        outcome: 1, // RENEW_OUTCOME_RENEWED
        lease: Some(lease.clone()),
        detail: "ok".into(),
        retry_after_ms: 0,
        schema_version: 1,
        coordinator_id: "coord-1".into(),
        issued_at_unix_ms: 1_755_103_900_000,
        request_nonce: (0u8..16).collect(),
        coordinator_signature: vec![0x77; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&r.to_canonical_fields(), &[])),
        expect_hex("v33_renew_lease_result")
    );

    // ★ request_nonce 는 서명 대상이어야 한다 — 재전송 시 갈아끼워 replay
    //   캐시를 우회하지 못하게 한다.
    let mut other_nonce = r.clone();
    other_nonce.request_nonce = (16u8..32).collect();
    assert_eq!(
        hex(&canonical_encode(&other_nonce.to_canonical_fields(), &[])),
        expect_hex("v33b_renew_lease_result_different_nonce")
    );
    assert_ne!(
        canonical_encode(&other_nonce.to_canonical_fields(), &[]),
        canonical_encode(&r.to_canonical_fields(), &[]),
        "request_nonce 가 서명 밖이다 — replay 캐시를 우회할 수 있다"
    );

    // ★ 규칙 i — nested Lease 의 서명은 outer canonical 에서 제외된다.
    //   outer 서명이 유효해도 nested Lease 서명은 독립적으로 위조될 수
    //   있으므로 검증자가 별도로 검증해야 한다(MUST).
    let mut other_lease_sig = r.clone();
    other_lease_sig.lease = Some(pb::Lease {
        coordinator_signature: vec![0xEE; 64],
        ..lease.clone()
    });
    assert_eq!(
        hex(&canonical_encode(
            &other_lease_sig.to_canonical_fields(),
            &[]
        )),
        expect_hex("v33c_renew_lease_result_nested_signature_excluded")
    );
    assert_eq!(
        canonical_encode(&other_lease_sig.to_canonical_fields(), &[]),
        canonical_encode(&r.to_canonical_fields(), &[]),
        "규칙 i 위반 — nested Lease 서명 필드가 outer canonical 에 새어나갔다"
    );
}

#[test]
fn resume_protocol_messages_match_reference() {
    let hello = pb::AgentSessionHello {
        schema_version: 1,
        mode: 2, // SESSION_MODE_RESUME
        session_id: "01JBXSESSION00000000000001".into(),
        node_id: "node-1".into(),
        connection_attempt: 2,
        issued_at_unix_ms: 1_755_103_900_000,
        nonce: (0u8..16).collect(),
        node_signature: vec![0x11; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&hello.to_canonical_fields(), &[])),
        expect_hex("v34_agent_session_hello"),
        "AgentSessionHello canonical 이 Python 참조 구현과 다르다"
    );

    let request = pb::ResumeLeaseRequest {
        schema_version: 1,
        lease_id: "01JBXLEASE0000000000000001".into(),
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        node_id: "node-1".into(),
        fence_epoch: 42,
        session_id: "01JBXSESSION00000000000001".into(),
        connection_attempt: 2,
        issued_at_unix_ms: 1_755_103_900_000,
        request_nonce: (16u8..32).collect(),
        node_signature: vec![0x22; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&request.to_canonical_fields(), &[])),
        expect_hex("v35_resume_lease_request"),
        "ResumeLeaseRequest canonical 이 Python 참조 구현과 다르다"
    );

    let lease = pb::Lease {
        schema_version: 1,
        lease_id: "01JBXLEASE0000000000000001".into(),
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        fence_epoch: 42,
        coordinator_term: 7,
        holder_node_id: "node-1".into(),
        member_node_ids: vec!["node-1".into(), "node-2".into()],
        issuing_coordinator_id: "coord-a".into(),
        issued_at_unix_ms: 1_755_100_800_000,
        expires_at_unix_ms: 1_755_100_860_000,
        renew_after_unix_ms: 1_755_100_830_000,
        max_total_duration_seconds: 86_400,
        scope: Some(pb::ResourceScope {
            gpu_uuids: vec!["GPU-11111111-2222-3333-4444-555555555555".into()],
            cpu_cores: 8,
            ram_bytes: 25_769_803_776,
            workspace_bytes: 85_899_345_920,
            writable_prefixes: vec!["jobs/01JBXR7Q0000000000000000AA/attempt-3/".into()],
        }),
        coordinator_signature: vec![0xCD; 64],
    };
    let result = pb::ResumeLeaseResult {
        outcome: 1, // RESUME_OUTCOME_RESUMED
        lease: Some(lease),
        detail: "resumed".into(),
        retry_after_ms: 0,
        schema_version: 1,
        coordinator_id: "coord-a".into(),
        issued_at_unix_ms: 1_755_103_900_000,
        request_nonce: (16u8..32).collect(),
        coordinator_signature: vec![0x33; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&result.to_canonical_fields(), &[])),
        expect_hex("v36_resume_lease_result"),
        "ResumeLeaseResult canonical 이 Python 참조 구현과 다르다"
    );
}

#[test]
fn revoke_lease_notice_matches_reference() {
    let n = pb::RevokeLeaseNotice {
        schema_version: 1,
        lease_id: "01JBXLEASE0000000000000001".into(),
        fence_epoch: 42,
        cause: 5, // OWNER_PREEMPT — 소유자 주권 (CLAUDE.md §0.1)
        issued_at_unix_ms: 1_755_103_800_000,
        coordinator_signature: vec![0xCC; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&n.to_canonical_fields(), &[])),
        expect_hex("v24_revoke_lease_notice")
    );

    // cause 가 서명 밖이면 OWNER_PREEMPT 를 다른 사유로 바꿔치기할 수 있다
    let mut other = n.clone();
    other.cause = 1;
    assert_ne!(
        canonical_encode(&other.to_canonical_fields(), &[]),
        canonical_encode(&n.to_canonical_fields(), &[]),
        "회수 사유가 서명 밖이다"
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 도메인 커버리지 감사
// ══════════════════════════════════════════════════════════════════

/// `signing.md` §5 의 domain_tag **전부**에 대해, 실제 proto 메시지가
/// 있는 것과 `ToCanonicalFields` 가 구현된 것을 대조한다.
///
/// ★ 이 주석은 "24종" 이라고 적혀 있었다 — 그 뒤 30종이 됐는데 숫자만
///   낡았다(2026-08-30 독립 검수 4라운드 지적). **숫자를 주석에 적지
///   않는다** — 코드는 `Domain::ALL` 을 쓰므로 자동으로 따라간다.
///
/// ★ **4종은 proto 메시지 자체가 없다** — 규범이 존재하지 않는 메시지의
///   domain_tag 를 등록해 두고 있다. 스펙 공백이며 이 테스트가 그것을 고정한다.
#[test]
fn domain_coverage_is_explicit() {
    use gputeer_protocol::canonical::Domain;

    // (domain, proto 메시지 이름 또는 None, ToCanonicalFields 구현 여부)
    let coverage: &[(Domain, Option<&str>, bool)] = &[
        (Domain::Manifest, Some("JobManifest"), true),
        (Domain::Lease, Some("Lease"), true),
        (Domain::LeaseRenew, Some("RenewLeaseRequest"), true),
        (Domain::LeaseRevoke, Some("RevokeLeaseNotice"), true),
        (Domain::Checkpoint, Some("CheckpointManifest"), true),
        (Domain::ReplicaAck, Some("ReplicaAck"), true),
        (Domain::Artifact, Some("ArtifactRef"), true),
        (Domain::AttemptReport, Some("AttemptReport"), true),
        (Domain::Canonical, Some("CanonicalDecision"), true),
        // ★ 한때 "아직 구현하지 않음" 이라고 적혀 있었다 — 그 뒤
        //   `ToCanonicalFields`·`Signable` 둘 다 구현됐는데 주석만
        //   남았다(2026-08-30 독립 검수 12라운드 지적).
        (Domain::Grant, Some("ExecutionGrant"), true),
        // ADR-028 — 메시지별 tag 분리. ToCanonicalFields 는 구현했으나
        // Signable(§9 시각 정책)이 없어 아직 verify() 는 통과하지 못한다.
        (Domain::MemberAdd, Some("AddMember"), true),
        (Domain::MemberRemove, Some("RemoveMember"), true),
        (Domain::DeviceApprove, Some("ApproveDevice"), true),
        (Domain::DeviceRevoke, Some("RevokeDevice"), true),
        (Domain::CoordinatorSet, Some("ChangeCoordinatorSet"), true),
        (Domain::OwnerKeyRotate, Some("RotateOwnerKey"), true),
        (Domain::PolicyUpdate, Some("UpdatePolicy"), true),
        (Domain::QuarantineDevice, Some("QuarantineDevice"), true),
        (Domain::QuarantineRelease, Some("ReleaseQuarantine"), true),
        // ★ proto 메시지 자체가 없다
        (Domain::Genesis, None, false),
        (Domain::Audit, None, false),
        (Domain::Release, None, false),
        (Domain::Invite, None, false),
        // coordinator/agent 최소 핸드셰이크 (2026-08-18) — 메시지도
        // 있고 ToCanonicalFields·Signable 둘 다 구현되어 있다.
        (Domain::GrantAck, Some("AgentGrantAck"), true),
        // Lease 갱신 최소 조각 (2026-08-19) — RenewLeaseResult 를
        // 서명 대상으로 승격. ToCanonicalFields·Signable 둘 다 구현됨.
        (Domain::LeaseRenewResult, Some("RenewLeaseResult"), true),
        (Domain::SessionHello, Some("AgentSessionHello"), true),
        (Domain::LeaseResume, Some("ResumeLeaseRequest"), true),
        (Domain::LeaseResumeResult, Some("ResumeLeaseResult"), true),
        // 노드 생존 보고 (2026-08-29, ADR-033 §7 앞 단계)
        (Domain::NodeHeartbeat, Some("NodeHeartbeat"), true),
        // 이웃 신고 (2026-08-30, ADR-033 §7 관측 층)
        (
            Domain::NeighborUnreachableReport,
            Some("NeighborUnreachableReport"),
            true,
        ),
        // B+E 계약 단계 1 (2026-09-14) — "받았다" 응답
        (Domain::AttemptReportAck, Some("AttemptReportAck"), true),
        // 결함 131 (2026-09-23) — ACK 수신 확인
        (Domain::GrantAckReceipt, Some("GrantAckReceipt"), true),
    ];

    // ★ 수동으로 적은 28 같은 숫자를 쓰지 않는다. 그 숫자를 두면
    //   새 domain 을 추가하고 이 목록을 안 고쳐도 숫자만 맞추면
    //   통과해 버린다 — 이 저장소가 세 번 겪은 결함이다.
    //   `Domain::ALL` 과 길이를 맞추면 문자 그대로 "빠진 domain 이 있다" 가 된다.
    let missing: Vec<&str> = Domain::ALL
        .iter()
        .filter(|d| !coverage.iter().any(|(c, _, _)| c == *d))
        .map(|d| d.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "★ domain 을 추가했는데 이 감사 목록에 없다: {missing:?}"
    );
    assert_eq!(
        coverage.len(),
        Domain::ALL.len(),
        "감사 목록에 Domain::ALL 에 없는 항목이 있다"
    );

    let implemented = coverage.iter().filter(|(_, _, i)| *i).count();
    let no_message = coverage.iter().filter(|(_, m, _)| m.is_none()).count();

    println!(
        "domain {}종 — 구현 {implemented} · proto 메시지 없음 {no_message}",
        coverage.len()
    );
    for (d, msg, impl_) in coverage {
        if !impl_ {
            println!(
                "  미구현 {:<28} {}",
                d.as_str(),
                msg.unwrap_or("★ proto 메시지 없음 — 스펙 공백")
            );
        }
    }

    // 이 숫자가 바뀌면 목록을 갱신하게 만든다.
    // **줄어드는(=후퇴하는) 것도 잡는다.**
    // 26 -> 27 (2026-09-14) — B+E 계약 단계 1 의 AttemptReportAck.
    // 27 -> 28 (2026-09-23) — 결함 131 의 GrantAckReceipt.
    assert_eq!(
        implemented, 28,
        "구현된 domain 수가 바뀌었다 — 목록을 갱신하라"
    );
    assert_eq!(
        no_message, 4,
        "proto 메시지 없는 domain 수가 바뀌었다 — 목록을 갱신하라"
    );
}

/// `NodeHeartbeat` 가 Python 참조 구현과 바이트 단위로 같은가.
///
/// ★ 이 대조가 없으면 "Rust 가 이렇게 인코딩한다" 는 말만 있고,
///   그것이 규범인지는 아무도 모른다 — `DoD-05`·`DoD-36` 이 정확히
///   그 공백을 뒤늦게 발견했다.
#[test]
fn node_heartbeat_matches_reference() {
    let heartbeat = pb::NodeHeartbeat {
        schema_version: 1,
        node_id: "node-1".into(),
        device_id: "01JBXDEV00000000000000001".into(),
        coordinator_device_id: "01JBXCOORD000000000000001".into(),
        issued_at_unix_ms: 1_755_103_900_000,
        fence_epoch: 42,
        running_attempts: 3,
        request_nonce: (16u8..32).collect(),
        node_signature: vec![0x44; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&heartbeat.to_canonical_fields(), &[])),
        expect_hex("v37_node_heartbeat"),
        "NodeHeartbeat canonical 이 Python 참조 구현과 다르다"
    );

    // ★ `fence_epoch` 하나만 바꾼 대조쌍. 값 하나만 검사하면 그 필드를
    //   빠뜨려도 통과하므로, 그 필드가 실제로 바이트에 닿는지 본다.
    let other = pb::NodeHeartbeat {
        fence_epoch: 43,
        ..heartbeat.clone()
    };
    let first = hex(&canonical_encode(&heartbeat.to_canonical_fields(), &[]));
    let second = hex(&canonical_encode(&other.to_canonical_fields(), &[]));
    assert_ne!(
        first, second,
        "fence_epoch 을 바꿨는데 canonical 바이트가 같다 — 그 필드가 인코딩에 안 들어간다"
    );
    assert_eq!(
        second,
        expect_hex("v37b_node_heartbeat_different_epoch"),
        "fence_epoch 이 다른 NodeHeartbeat 가 참조 구현과 다르다"
    );
}

/// 이웃 신고가 Python 참조 구현과 **바이트 단위로** 같은가.
///
/// ★ `DoD-05` 가 찾은 공백과 같은 종류다 — `AgentGrantAck` 는 서명·검증·
///   dispatch 테스트는 있었지만 참조 구현 대조가 없어, Rust 와 Python 이
///   갈라져도 아무도 몰랐다. 새 서명 대상 메시지는 반드시 이 대조를 받는다.
#[test]
fn neighbor_unreachable_report_matches_reference() {
    let report = pb::NeighborUnreachableReport {
        schema_version: 1,
        reporter_node_id: "node-reporter".into(),
        reporter_device_id: "device-reporter".into(),
        unreachable_node_id: "node-stranded".into(),
        coordinator_device_id: "coordinator-1".into(),
        observed_at_unix_ms: 1_700_000_000_000,
        request_nonce: (32u8..48).collect(),
        reporter_signature: vec![0x45; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&report.to_canonical_fields(), &[])),
        expect_hex("v38_neighbor_unreachable_report"),
        "NeighborUnreachableReport canonical 이 Python 참조 구현과 다르다"
    );

    // ★ **지목당한 노드**만 바꾼 대조쌍. 이 필드가 canonical 에 안 들어가면
    //   한 노드에 대한 신고 서명을 **다른 노드에 그대로 재사용**할 수 있다 —
    //   A 를 지목한 정당한 신고가 B 를 지목한 신고로 둔갑한다.
    //
    //   ★ 초안 주석은 여기에 "정족수를 한 건으로 채운다" 를 붙였는데
    //     **틀렸다**(독립 검수 2라운드 지적) — 정족수는 신고자 기계 수로
    //     세므로 대상을 바꿔도 한 신고자는 한 표다. 문제는 **대상 위조**
    //     자체다.
    let other = pb::NeighborUnreachableReport {
        unreachable_node_id: "node-other".into(),
        ..report.clone()
    };
    let first = hex(&canonical_encode(&report.to_canonical_fields(), &[]));
    let second = hex(&canonical_encode(&other.to_canonical_fields(), &[]));
    assert_ne!(
        first, second,
        "지목 노드를 바꿨는데 canonical 이 같다 — 신고 서명을 다른 노드에 재사용할 수 있다"
    );
    assert_eq!(
        second,
        expect_hex("v38b_neighbor_unreachable_different_target"),
        "지목 노드가 다른 신고가 참조 구현과 다르다"
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 결함 75 (재검수 56) — B+E 계약 단계 1 의 새 벡터 14개를 **Rust 메시지로** 대조한다
//
// Python `--verify` 는 Python 끼리의 재생성 대조다. 이 대조가 없으면 Rust 의
// `to_canonical_fields` 가 참조 구현과 갈라져도(예: Ack 의 `created` 를 뒤집어도)
// 아무것도 실패하지 않는다. canonical 과 sig_input(domain · 버전 포함) 을 둘 다 본다.
// ══════════════════════════════════════════════════════════════════

fn assert_matches_reference<M: gputeer_protocol::signing::Signable + ToCanonicalFields>(
    name: &str,
    msg: &M,
) {
    assert_eq!(
        hex(&canonical_encode(
            &ToCanonicalFields::to_canonical_fields(msg),
            &[]
        )),
        expect_hex(name),
        "{name} canonical 이 Python 참조 구현과 다르다"
    );
    assert_eq!(
        hex(&gputeer_protocol::signing::signing_input(msg)),
        expect_vector_field(name, "sig_input_hex"),
        "{name} sig_input 이 Python 참조 구현과 다르다"
    );
}

/// 참조 구현 `_report_base` 와 같은 값.
fn b_e_report(
    schema_version: u32,
    outcome: i32,
    exit_observation: i32,
    exit_code: u32,
    stage: i32,
) -> pb::AttemptReport {
    pb::AttemptReport {
        schema_version,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        node_id: "node-1".into(),
        fence_epoch: 42,
        outcome,
        started_at_unix_ms: 1_755_100_800_000,
        finished_at_unix_ms: 1_755_104_400_000,
        issued_at_unix_ms: 1_755_104_401_000,
        exit_observation,
        exit_code,
        finalization_failure_stage: stage,
        node_signature: vec![b'R'; 64],
        ..Default::default()
    }
}

#[test]
fn b_e_attempt_report_vectors_match_reference() {
    for (name, report) in [
        ("v39_attempt_report_v1_completed", b_e_report(1, 1, 0, 0, 0)),
        (
            "v39a_attempt_report_v2_completed_default_fields",
            b_e_report(2, 1, 0, 0, 0),
        ),
        (
            "v39b_attempt_report_v2_completed",
            b_e_report(2, 1, 2, 0, 0),
        ),
        (
            "v39c_attempt_report_v2_failed_exit7_read_outputs",
            b_e_report(2, 2, 2, 7, 1),
        ),
        (
            "v39d_attempt_report_v2_failed_exit8_read_outputs",
            b_e_report(2, 2, 2, 8, 1),
        ),
        (
            "v39e_attempt_report_v2_output_finalization_read_outputs",
            b_e_report(2, 6, 2, 0, 1),
        ),
        (
            "v39f_attempt_report_v2_output_finalization_encode_result",
            b_e_report(2, 6, 2, 0, 2),
        ),
        (
            "v39g_attempt_report_v2_output_finalization_commit_checkpoint",
            b_e_report(2, 6, 2, 0, 3),
        ),
        (
            "v39h_attempt_report_v2_failed_not_observed",
            b_e_report(2, 2, 1, 0, 0),
        ),
        (
            "v39i_attempt_report_v2_failed_observed_no_code",
            b_e_report(2, 2, 3, 0, 0),
        ),
    ] {
        assert_matches_reference(name, &report);
    }
}

#[test]
fn b_e_attempt_report_ack_vectors_match_reference() {
    let ack = pb::AttemptReportAck {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        node_id: "node-1".into(),
        fence_epoch: 42,
        report_hash: Some(pb::Digest {
            algo: 1,
            value: (0u8..32).collect(),
        }),
        created: true,
        coordinator_id: "coordinator-1".into(),
        issued_at_unix_ms: 1_755_104_402_000,
        session_nonce: (48u8..64).collect(),
        coordinator_signature: vec![b'A'; 64],
        ..Default::default()
    };
    assert_matches_reference("v40_attempt_report_ack", &ack);
    // `created` 하나만 바꾼 대조쌍 — 이 필드가 바이트에 닿지 않으면 재전송 응답과 첫 저장 응답을 서명으로 구별할 수 없다.
    let replay = pb::AttemptReportAck {
        created: false,
        ..ack.clone()
    };
    assert_ne!(
        hex(&canonical_encode(
            &ToCanonicalFields::to_canonical_fields(&ack),
            &[]
        )),
        hex(&canonical_encode(
            &ToCanonicalFields::to_canonical_fields(&replay),
            &[]
        )),
        "created 를 바꿨는데 canonical 이 같다"
    );
    assert_matches_reference("v40b_attempt_report_ack_replay_created_false", &replay);
}

#[test]
fn b_e_session_hello_mode_vectors_match_reference() {
    let hello = |mode: i32| pb::AgentSessionHello {
        schema_version: 1,
        mode,
        session_id: "01JBXSESSION00000000000001".into(),
        node_id: "node-1".into(),
        connection_attempt: 2,
        issued_at_unix_ms: 1_755_103_900_000,
        nonce: (0u8..16).collect(),
        node_signature: vec![0x11; 64],
        ..Default::default()
    };
    assert_matches_reference(
        "v41_agent_session_hello_renew",
        &hello(gputeer_protocol::constants::MODE_RENEW as i32),
    );
    assert_matches_reference(
        "v41b_agent_session_hello_report",
        &hello(gputeer_protocol::constants::MODE_REPORT as i32),
    );
}

/// ★ 결함 131 (2026-09-23) — ACK 수신 확인의 canonical 이 참조 구현과 같고, 어느 ACK 에 대한 확인인지(ack_nonce)가 서명에 닿는다.
#[test]
fn grant_ack_receipt_vectors_match_reference() {
    let receipt = pb::GrantAckReceipt {
        schema_version: 1,
        grant_id: "g0123456789abcdef01234567".into(),
        attempt_id: "01JBXATT00000000000000001".into(),
        agent_device_id: "node-1".into(),
        ack_nonce: (16u8..32).collect(),
        coordinator_device_id: "coordinator-1".into(),
        issued_at_unix_ms: 1_755_104_403_000,
        nonce: (64u8..80).collect(),
        coordinator_signature: vec![b'R'; 64],
    };
    assert_matches_reference("v42_grant_ack_receipt", &receipt);
    let other_ack = pb::GrantAckReceipt {
        ack_nonce: (32u8..48).collect(),
        ..receipt.clone()
    };
    assert_matches_reference("v42b_grant_ack_receipt_other_ack", &other_ack);
    assert_ne!(
        hex(&canonical_encode(
            &ToCanonicalFields::to_canonical_fields(&receipt),
            &[]
        )),
        hex(&canonical_encode(
            &ToCanonicalFields::to_canonical_fields(&other_ack),
            &[]
        )),
        "ack_nonce 를 바꿨는데 canonical 이 같다 — 다른 ACK 에 대한 확인을 구별하지 못한다"
    );
}
