//! Rust 구현 ↔ Python 참조 구현 교차 검증.
//!
//! `tests/vectors/canonical_v1.json` 은 `tools/canonical/reference_canonical.py`
//! 가 생성한다. **손으로 고치지 않는다** (QA 스트림 소유).
//!
//! signing.md §12.3 — 두 구현이 같은 입력에 같은 canonical bytes 를 내야 한다.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gputeer_protocol::canonical::{
    blake3_256, canonical_encode, decode_varint, encode_varint, merkle_root, sig_input,
    CanonicalError, Domain, Fields, Value,
};

fn vectors_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/vectors/canonical_v1.json")
}

fn load_vectors() -> serde_json::Value {
    let raw = std::fs::read(vectors_path()).expect("테스트 벡터를 읽을 수 없다");
    serde_json::from_slice(&raw).expect("벡터 JSON 파싱 실패")
}

fn find<'a>(doc: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    doc["vectors"]
        .as_array()
        .expect("vectors 배열")
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("벡터 없음: {name}"))
}

// ═══════════════════════════════════════════════════════════════════
// 참조 구현의 JobManifest 부분집합을 Rust 로 재구성한다.
// 필드 번호는 proto/job.proto 와 일치해야 한다.
// ═══════════════════════════════════════════════════════════════════

fn minimal_manifest() -> Fields {
    let mut m = Fields::new();
    m.set(1, Value::Uint(1)) // schema_version
        .set(2, Value::Str("01JBXR7Q0000000000000000AA".into())) // job_id
        .set(3, Value::Str("01JBXR7Q0000000000000000TT".into())) // team_id
        .set(13, Value::Str("train.py".into())) // entrypoint
        .set(60, Value::Str("01JBXR7Q0000000000000000DD".into())) // submitter_device_id
        .set(61, Value::Uint(1_755_100_800_000)) // issued_at
        .set(62, Value::Uint(1_755_705_600_000)); // expires_at
    m
}

fn map_of(pairs: &[(&str, &str)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), (*v).to_string());
    }
    Value::MapStrStr(m)
}

fn repeated(items: &[&str]) -> Value {
    Value::RepeatedStr(items.iter().map(|s| (*s).to_string()).collect())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ═══════════════════════════════════════════════════════════════════
// v01 — 최소 메시지. 기본값 생략 (규칙 b)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn v01_minimal_manifest_matches_reference() {
    let doc = load_vectors();
    let expected = find(&doc, "v01_minimal_manifest")["canonical_hex"]
        .as_str()
        .unwrap();
    let actual = hex(&canonical_encode(&minimal_manifest(), &[]));
    assert_eq!(actual, expected, "참조 구현과 canonical bytes 불일치");
}

#[test]
fn v01_sig_input_and_digest_match_reference() {
    let doc = load_vectors();
    let v = find(&doc, "v01_minimal_manifest");
    let canon = canonical_encode(&minimal_manifest(), &[]);
    let si = sig_input(Domain::Manifest, 1, &canon);

    assert_eq!(hex(&si), v["sig_input_hex"].as_str().unwrap(), "sig_input 불일치");
    assert_eq!(
        hex(&blake3_256(&si)),
        v["sig_input_blake3_256"].as_str().unwrap(),
        "BLAKE3 다이제스트 불일치"
    );
}

// ═══════════════════════════════════════════════════════════════════
// v03 — map 정렬 (규칙 c). 삽입 순서가 달라도 같아야 한다
// ═══════════════════════════════════════════════════════════════════

#[test]
fn v03_map_insertion_order_is_irrelevant() {
    let doc = load_vectors();

    let mut a = minimal_manifest();
    a.set(15, map_of(&[("ZZZ", "3"), ("AAA", "1"), ("MMM", "2")]));
    let mut b = minimal_manifest();
    b.set(15, map_of(&[("AAA", "1"), ("MMM", "2"), ("ZZZ", "3")]));

    let ca = canonical_encode(&a, &[]);
    let cb = canonical_encode(&b, &[]);
    assert_eq!(ca, cb, "map 삽입 순서가 canonical 에 영향을 주면 안 된다");

    let expected = find(&doc, "v03a_map_insertion_order_1")["canonical_hex"]
        .as_str()
        .unwrap();
    assert_eq!(hex(&ca), expected, "참조 구현과 불일치");
}

// ═══════════════════════════════════════════════════════════════════
// v04 — repeated 순서 유지 (규칙 d). 순서가 다르면 달라야 한다
// ═══════════════════════════════════════════════════════════════════

#[test]
fn v04_repeated_order_is_preserved() {
    let doc = load_vectors();

    let mut a = minimal_manifest();
    a.set(14, repeated(&["--a", "--b"]));
    let mut b = minimal_manifest();
    b.set(14, repeated(&["--b", "--a"]));

    let ca = canonical_encode(&a, &[]);
    let cb = canonical_encode(&b, &[]);
    assert_ne!(ca, cb, "repeated 순서는 보존되어야 한다");

    assert_eq!(
        hex(&ca),
        find(&doc, "v04a_repeated_order_1")["canonical_hex"].as_str().unwrap()
    );
    assert_eq!(
        hex(&cb),
        find(&doc, "v04b_repeated_order_2")["canonical_hex"].as_str().unwrap()
    );
}

// ═══════════════════════════════════════════════════════════════════
// v06 — 명시적 기본값은 생략된다 (규칙 b)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn v06_explicit_defaults_are_omitted() {
    let mut z = minimal_manifest();
    z.set(30, Value::Uint(0)) // deadline_minutes = 0
        .set(31, Value::Uint(0)) // preference = UNSPECIFIED
        .set(41, Value::Uint(0)) // durability = UNSPECIFIED
        .set(56, Value::Bool(false)) // acknowledge_duplicate_risk
        .set(14, Value::RepeatedStr(vec![]))
        .set(15, Value::MapStrStr(BTreeMap::new()));

    assert_eq!(
        canonical_encode(&z, &[]),
        canonical_encode(&minimal_manifest(), &[]),
        "명시적 기본값은 미설정과 같은 canonical 을 내야 한다"
    );
}

// ═══════════════════════════════════════════════════════════════════
// v08 — 서명 필드는 제외된다 (규칙 i)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn v08_signature_field_is_excluded() {
    let mut signed = minimal_manifest();
    signed.set(90, Value::Bytes(vec![0xFF; 64]));

    assert_eq!(
        canonical_encode(&signed, &[]),
        canonical_encode(&minimal_manifest(), &[]),
        "서명 필드가 canonical 에 포함되면 순환이 생긴다"
    );
}

// ═══════════════════════════════════════════════════════════════════
// v10 / v11 — domain_tag 와 schema_version 분리 (§4, §5)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn v10_domain_separation() {
    let doc = load_vectors();
    let v = find(&doc, "v10_domain_separation");
    let canon = canonical_encode(&minimal_manifest(), &[]);

    let as_manifest = sig_input(Domain::Manifest, 1, &canon);
    let as_lease = sig_input(Domain::Lease, 1, &canon);

    assert_ne!(as_manifest, as_lease, "domain 이 다르면 sig_input 도 달라야 한다");
    assert_eq!(hex(&as_manifest), v["sig_input_as_manifest_hex"].as_str().unwrap());
    assert_eq!(hex(&as_lease), v["sig_input_as_lease_hex"].as_str().unwrap());
}

#[test]
fn v11_schema_version_separation() {
    let doc = load_vectors();
    let v = find(&doc, "v11_schema_version_separation");
    let canon = canonical_encode(&minimal_manifest(), &[]);

    assert_eq!(hex(&sig_input(Domain::Manifest, 1, &canon)), v["sig_input_v1_hex"].as_str().unwrap());
    assert_eq!(hex(&sig_input(Domain::Manifest, 2, &canon)), v["sig_input_v2_hex"].as_str().unwrap());
}

// ═══════════════════════════════════════════════════════════════════
// v13 — Merkle 홀수 노드 승격 (§6.3)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn v13_merkle_promotion_matches_reference() {
    let doc = load_vectors();
    let roots = &find(&doc, "v13_merkle_promotion")["roots"];

    let a = vec![b'a'; 16];
    let b = vec![b'b'; 16];
    let c = vec![b'c'; 16];

    let cases: [(&str, Vec<&[u8]>); 3] = [
        ("1_chunk", vec![a.as_slice()]),
        ("2_chunks", vec![a.as_slice(), b.as_slice()]),
        ("3_chunks", vec![a.as_slice(), b.as_slice(), c.as_slice()]),
    ];

    for (key, chunks) in cases {
        let actual = hex(&merkle_root(&chunks).expect("루트가 있어야 한다"));
        assert_eq!(actual, roots[key].as_str().unwrap(), "{key} merkle root 불일치");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 결정론성 — 100회 인코딩이 모두 동일
// ═══════════════════════════════════════════════════════════════════

#[test]
fn determinism_100_iterations() {
    let mut m = minimal_manifest();
    m.set(14, repeated(&["--epochs", "3", "--lr", "1e-4"]))
        .set(15, map_of(&[("OMP_NUM_THREADS", "8"), ("HF_HOME", "/ws/hf"), ("AAA", "1")]))
        .set(30, Value::Uint(180));

    let first = canonical_encode(&m, &[]);
    for i in 0..100 {
        assert_eq!(canonical_encode(&m, &[]), first, "{i}회차에서 달라졌다");
    }
}

// ═══════════════════════════════════════════════════════════════════
// negative test — RULE.md §6
// ═══════════════════════════════════════════════════════════════════

#[test]
fn negative_non_minimal_varint_is_rejected() {
    // 0 을 2바이트로 인코딩한 non-minimal 형태
    let bad = [0x80u8, 0x00];
    let mut pos = 0;
    assert_eq!(
        decode_varint(&bad, &mut pos),
        Err(CanonicalError::NonMinimalVarint),
        "non-minimal varint 는 거부되어야 한다 (서명 우회 방지)"
    );
}

#[test]
fn negative_truncated_varint_is_rejected() {
    let bad = [0x80u8]; // 연속 비트가 켜졌는데 다음 바이트가 없다
    let mut pos = 0;
    assert_eq!(decode_varint(&bad, &mut pos), Err(CanonicalError::TruncatedVarint));
}

#[test]
fn minimal_varint_roundtrip() {
    for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
        let mut buf = Vec::new();
        encode_varint(v, &mut buf);
        let mut pos = 0;
        assert_eq!(decode_varint(&buf, &mut pos).unwrap(), v);
        assert_eq!(pos, buf.len());
    }
}

#[test]
fn negative_cross_domain_signature_input_differs() {
    // 같은 canonical 이라도 domain 이 다르면 sig_input 이 달라야 한다.
    // 이것이 없으면 Lease 서명을 Manifest 서명으로 재사용할 수 있다.
    let canon = canonical_encode(&minimal_manifest(), &[]);
    let domains = [
        Domain::Manifest, Domain::Grant, Domain::Lease, Domain::Checkpoint,
        Domain::ReplicaAck, Domain::Artifact, Domain::Canonical, Domain::Release,
    ];
    let mut seen = std::collections::HashSet::new();
    for d in domains {
        assert!(seen.insert(sig_input(d, 1, &canon)), "domain {d:?} 의 sig_input 이 중복된다");
    }
}

#[test]
fn domain_tags_are_32_bytes_and_unique() {
    let domains = [
        Domain::Manifest, Domain::Grant, Domain::Lease, Domain::LeaseRenew,
        Domain::LeaseRevoke, Domain::Checkpoint, Domain::ReplicaAck, Domain::Artifact,
        Domain::AttemptReport, Domain::Canonical, Domain::Genesis,
        // ADR-028 — membership/policy/quarantine 3종 -> 9종 분리
        Domain::MemberAdd, Domain::MemberRemove, Domain::DeviceApprove, Domain::DeviceRevoke,
        Domain::CoordinatorSet, Domain::OwnerKeyRotate, Domain::PolicyUpdate,
        Domain::QuarantineDevice, Domain::QuarantineRelease,
        Domain::Audit, Domain::Release, Domain::Invite,
    ];
    let mut seen = std::collections::HashSet::new();
    for d in domains {
        let t = d.tag_bytes();
        assert_eq!(t.len(), 32);
        assert!(seen.insert(t), "domain tag 중복: {d:?}");
    }
    assert_eq!(
        seen.len(),
        23,
        "signing.md §5 의 domain_tag 23종과 일치해야 한다 (ADR-028)"
    );
}
