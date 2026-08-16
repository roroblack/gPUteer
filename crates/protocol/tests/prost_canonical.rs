//! prost 메시지 → canonical 변환이 참조 구현과 일치하는가.
//!
//! **DoD-01 의 최대 공백을 닫는 테스트다.**
//!
//! DoD-01 은 손으로 만든 `Fields`/`Value` 로만 canonical 규칙을 검증했다.
//! 실제 protobuf 메시지에서 `Fields` 를 만드는 계층에서 규칙이 깨질 수 있고,
//! 그것이 미검증이었다. 이 테스트가 그 경로를 검증한다.
//!
//! 특히 중요한 것은 **`HashMap` → `BTreeMap` 정규화**다.
//! prost 는 map 필드를 `HashMap` 으로 생성하고, `HashMap` 순회 순서는
//! 실행마다 다르다. 정규화하지 않으면 서명이 랜덤하게 달라진다.

use std::collections::HashMap;
use std::path::PathBuf;

use gputeer_protocol::canonical::{canonical_encode, sig_input, Domain};
use gputeer_protocol::{blake3_256, pb, ToCanonicalFields, UNIMPLEMENTED_FIELDS};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn vectors() -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/vectors/canonical_v1.json");
    serde_json::from_slice(&std::fs::read(p).expect("벡터 파일")).expect("JSON")
}

fn find<'a>(doc: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    doc["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("벡터 없음: {name}"))
}

/// 참조 구현의 v01_minimal_manifest 와 같은 내용을 prost 타입으로 만든다.
fn minimal_manifest() -> pb::JobManifest {
    pb::JobManifest {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        team_id: "01JBXR7Q0000000000000000TT".into(),
        entrypoint: "train.py".into(),
        submitter_device_id: "01JBXR7Q0000000000000000DD".into(),
        issued_at_unix_ms: 1_755_100_800_000,
        expires_at_unix_ms: 1_755_705_600_000,
        ..Default::default()
    }
}

// ══════════════════════════════════════════════════════════════════
// 핵심 — prost 경로가 참조 구현과 바이트 단위로 일치하는가
// ══════════════════════════════════════════════════════════════════

#[test]
fn prost_message_produces_same_canonical_as_reference() {
    let doc = vectors();
    let expected = find(&doc, "v01_minimal_manifest")["canonical_hex"]
        .as_str()
        .unwrap();

    let m = minimal_manifest();
    let actual = hex(&canonical_encode(&m.to_canonical_fields(), &[]));

    assert_eq!(
        actual, expected,
        "prost 메시지 경로가 Python 참조 구현과 다른 canonical 을 냈다.\n\
         손으로 만든 Fields 로는 통과했으므로 to_fields 변환 계층의 결함이다."
    );
}

#[test]
fn prost_message_produces_same_sig_input_and_digest() {
    let doc = vectors();
    let v = find(&doc, "v01_minimal_manifest");

    let m = minimal_manifest();
    let canon = canonical_encode(&m.to_canonical_fields(), &[]);
    let si = sig_input(Domain::Manifest, m.schema_version(), &canon);

    assert_eq!(hex(&si), v["sig_input_hex"].as_str().unwrap());
    assert_eq!(
        hex(&blake3_256(&si)),
        v["sig_input_blake3_256"].as_str().unwrap()
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ HashMap 정규화 — 이 계층에서 가장 깨지기 쉬운 지점
// ══════════════════════════════════════════════════════════════════

#[test]
fn hashmap_insertion_order_does_not_affect_canonical() {
    // prost 는 map 을 HashMap 으로 생성한다. 순회 순서가 실행마다 다르므로
    // BTreeMap 정규화가 없으면 서명이 랜덤하게 달라진다.
    let mut a = minimal_manifest();
    let mut ma = HashMap::new();
    ma.insert("ZZZ".to_string(), "3".to_string());
    ma.insert("AAA".to_string(), "1".to_string());
    ma.insert("MMM".to_string(), "2".to_string());
    a.env_vars = ma;

    let mut b = minimal_manifest();
    let mut mb = HashMap::new();
    mb.insert("AAA".to_string(), "1".to_string());
    mb.insert("MMM".to_string(), "2".to_string());
    mb.insert("ZZZ".to_string(), "3".to_string());
    b.env_vars = mb;

    let ca = canonical_encode(&a.to_canonical_fields(), &[]);
    let cb = canonical_encode(&b.to_canonical_fields(), &[]);
    assert_eq!(ca, cb, "HashMap 삽입 순서가 canonical 에 영향을 줬다");

    // 참조 구현과도 일치해야 한다
    let doc = vectors();
    assert_eq!(
        hex(&ca),
        find(&doc, "v03a_map_insertion_order_1")["canonical_hex"]
            .as_str()
            .unwrap(),
        "map 정규화 결과가 참조 구현과 다르다"
    );
}

#[test]
fn hashmap_canonical_is_stable_across_many_rebuilds() {
    // HashMap 은 매 생성마다 다른 순회 순서를 가질 수 있다.
    // 200회 다시 만들어 전부 같은 canonical 이 나오는지 본다.
    let mut seen = std::collections::HashSet::new();
    for i in 0..200 {
        let mut m = minimal_manifest();
        let mut hm = HashMap::new();
        // 삽입 순서를 매번 바꾼다
        let keys = ["k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8"];
        let rot = i % keys.len();
        for k in keys.iter().cycle().skip(rot).take(keys.len()) {
            hm.insert((*k).to_string(), format!("v-{k}"));
        }
        m.env_vars = hm;
        seen.insert(canonical_encode(&m.to_canonical_fields(), &[]));
    }
    assert_eq!(seen.len(), 1, "200회 중 서로 다른 canonical 이 {}종 나왔다", seen.len());
}

// ══════════════════════════════════════════════════════════════════
// 서명 필드 제외 (규칙 i) — prost 경로에서도 지켜지는가
// ══════════════════════════════════════════════════════════════════

#[test]
fn signature_field_excluded_in_prost_path() {
    let plain = minimal_manifest();
    let mut signed = minimal_manifest();
    signed.submitter_signature = vec![0xFF; 64];

    assert_eq!(
        canonical_encode(&plain.to_canonical_fields(), &[]),
        canonical_encode(&signed.to_canonical_fields(), &[]),
        "서명 필드가 canonical 에 들어갔다 — 자기참조 순환이 생긴다"
    );
}

// ══════════════════════════════════════════════════════════════════
// 중첩 메시지 · repeated · enum
// ══════════════════════════════════════════════════════════════════

#[test]
fn nested_messages_and_enums_round_through_prost() {
    let mut m = minimal_manifest();
    m.args = vec!["--epochs".into(), "3".into()];
    m.deadline_minutes = 180;
    m.preference = pb::Preference::Balanced as i32;
    m.durability = pb::Durability::Replicated as i32;
    m.side_effect_class = pb::SideEffectClass::Pure as i32;
    m.resources = Some(pb::ResourceRequest {
        gpu: Some(pb::GpuRequest {
            min_vram_bytes: 19_327_352_832,
            min_count: 1,
            cuda: Some(pb::CudaRequirement {
                min_driver_version: 550,
                cuda_runtime_version: "12.4".into(),
                compute_capabilities: vec!["8.6".into(), "8.9".into()],
            }),
            allocation_mode: pb::GpuAllocationMode::Exclusive as i32,
            ..Default::default()
        }),
        cpu_cores: 8,
        ram_bytes: 25_769_803_776,
        workspace_bytes: 85_899_345_920,
        ..Default::default()
    });
    m.workload = Some(pb::WorkloadHint {
        class: pb::WorkloadClass::Training as i32,
        estimated_steps: 20000,
        ref_step_time_ms: 420,
        ref_gpu_model: "RTX4090".into(),
        model_params: 1_300_000_000,
        est_checkpoint_bytes: 18_200_000_000,
        ..Default::default()
    });

    let c1 = canonical_encode(&m.to_canonical_fields(), &[]);
    assert!(!c1.is_empty());

    // 결정론성
    for _ in 0..50 {
        assert_eq!(canonical_encode(&m.to_canonical_fields(), &[]), c1);
    }

    // 중첩 필드가 실제로 반영됐는지 — 빼면 달라져야 한다
    let mut m2 = m.clone();
    m2.resources = None;
    assert_ne!(
        canonical_encode(&m2.to_canonical_fields(), &[]),
        c1,
        "중첩 메시지가 canonical 에 반영되지 않았다"
    );
}

#[test]
fn default_valued_fields_are_omitted_in_prost_path() {
    let base = minimal_manifest();
    let mut explicit = minimal_manifest();
    explicit.deadline_minutes = 0;
    explicit.preference = 0;
    explicit.durability = 0;
    explicit.acknowledge_duplicate_risk = false;
    explicit.args = vec![];
    explicit.env_vars = HashMap::new();

    assert_eq!(
        canonical_encode(&base.to_canonical_fields(), &[]),
        canonical_encode(&explicit.to_canonical_fields(), &[]),
        "명시적 기본값이 canonical 에 들어갔다"
    );
}

// ══════════════════════════════════════════════════════════════════
// Lease
// ══════════════════════════════════════════════════════════════════

#[test]
fn lease_canonical_is_deterministic_and_excludes_signature() {
    let mut l = pb::Lease {
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
        max_total_duration_seconds: 86400,
        ..Default::default()
    };
    let c1 = canonical_encode(&l.to_canonical_fields(), &[]);

    l.coordinator_signature = vec![0xAB; 64];
    let c2 = canonical_encode(&l.to_canonical_fields(), &[]);
    assert_eq!(c1, c2, "Lease 서명 필드가 canonical 에 포함됐다");

    // domain 이 다르면 sig_input 이 달라야 한다
    let si_lease = sig_input(Domain::Lease, 1, &c1);
    let si_manifest = sig_input(Domain::Manifest, 1, &c1);
    assert_ne!(si_lease, si_manifest);
}

// ══════════════════════════════════════════════════════════════════
// ★ 미구현 필드가 문서화되어 있는가
//
// 서명 대상에서 빠진 필드는 "조용히 빠져 있으면" 안 된다.
// 구현자가 알아채지 못하면 그 필드는 위조 가능해진다.
// ══════════════════════════════════════════════════════════════════

#[test]
fn unimplemented_nested_fields_are_declared() {
    // 현재 미구현으로 선언된 필드가 실제로 canonical 에 없는지 확인한다.
    // (반대로, 선언되지 않은 필드가 빠져 있으면 이 테스트로는 못 잡는다 —
    //  그래서 to_fields.rs 를 수동 구현으로 유지한다)
    assert!(
        !UNIMPLEMENTED_FIELDS.is_empty(),
        "미구현 필드가 없다면 이 목록을 비우고 이 테스트를 제거해야 한다"
    );

    // JobManifest 의 dataset(12) 을 채워도 canonical 이 변하지 않아야 한다
    // (= 아직 서명 대상이 아니다)
    let base = minimal_manifest();
    let mut with_dataset = minimal_manifest();
    with_dataset.dataset = Some(pb::DatasetRef {
        total_bytes: 12345,
        display_name: "ds".into(),
        ..Default::default()
    });

    assert_eq!(
        canonical_encode(&base.to_canonical_fields(), &[]),
        canonical_encode(&with_dataset.to_canonical_fields(), &[]),
        "dataset 이 UNIMPLEMENTED_FIELDS 에 있는데 canonical 에 반영됐다 — 목록이 낡았다"
    );

    for (msg, num, desc) in UNIMPLEMENTED_FIELDS {
        println!("미구현 서명 필드: {msg} field {num} — {desc}");
    }
}

// ══════════════════════════════════════════════════════════════════
// ★ negative — prost 기본 인코더를 서명에 쓰면 안 된다
//
// 최초에 나는 이 테스트를 "prost 출력 != canonical" 로 썼고 **실패했다**.
// 단순 메시지(map 없음)에서는 prost 도 필드 번호 오름차순 · 기본값 생략 ·
// 최소 varint 를 쓰므로 **바이트가 우연히 같다**.
//
// 즉 signing.md §13.1 의 근거는 "항상 다르다" 가 아니라
// **"map 이 들어가는 순간 prost 는 결정론적이지 않다"** 이다.
// 아래 두 테스트가 그것을 실측으로 보인다.
// ══════════════════════════════════════════════════════════════════

/// map 없는 단순 메시지에서는 prost 와 canonical 이 일치할 수 있다.
///
/// **이것을 "prost 를 서명에 써도 된다" 로 읽으면 안 된다.**
/// 아래 `prost_encode_is_not_deterministic_for_maps` 를 함께 볼 것.
#[test]
fn prost_and_canonical_may_coincide_for_simple_messages() {
    use prost::Message;

    let m = minimal_manifest();
    let mut pb_bytes = Vec::new();
    m.encode(&mut pb_bytes).unwrap();
    let canon = canonical_encode(&m.to_canonical_fields(), &[]);

    println!(
        "map 없는 매니페스트: prost {}B, canonical {}B, 동일={}",
        pb_bytes.len(),
        canon.len(),
        pb_bytes == canon
    );
    // 같든 다르든 통과한다. 이 테스트는 사실을 기록할 뿐 규범을 주장하지 않는다.
}

/// ★ prost 인코딩은 map 이 있으면 결정론적이지 않다 — 서명에 쓸 수 없는 이유.
#[test]
fn prost_encode_is_not_deterministic_for_maps() {
    use prost::Message;

    let mut prost_variants = std::collections::HashSet::new();
    let mut canon_variants = std::collections::HashSet::new();

    for i in 0..500 {
        let mut m = minimal_manifest();
        let mut hm = HashMap::new();
        let keys = ["k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8"];
        let rot = i % keys.len();
        for k in keys.iter().cycle().skip(rot).take(keys.len()) {
            hm.insert((*k).to_string(), format!("v-{k}"));
        }
        m.env_vars = hm;

        let mut b = Vec::new();
        m.encode(&mut b).unwrap();
        prost_variants.insert(b);
        canon_variants.insert(canonical_encode(&m.to_canonical_fields(), &[]));
    }

    println!(
        "500회 재구축 — prost 서로 다른 인코딩 {}종 / canonical {}종",
        prost_variants.len(),
        canon_variants.len()
    );

    assert_eq!(
        canon_variants.len(),
        1,
        "canonical 이 결정론적이지 않다 — 서명이 깨진다"
    );

    // ★ 비공허성 검증.
    // prost 가 1종만 냈다면 이 테스트는 "canonical 이 안정적이다" 를
    // 보인 것일 뿐 "prost 는 불안정하다" 를 보인 것이 아니다.
    // 그 경우 signing.md §13.1 의 근거를 다시 세워야 하므로 실패시킨다.
    assert!(
        prost_variants.len() > 1,
        "prost 가 map 순서를 항상 같게 냈다 (500회 전부 동일). \n\
         이 러너에서는 HashMap 순회 순서가 안정적이라는 뜻이다.\n\
         signing.md §13.1 의 근거를 재확인해야 한다 — 조용히 통과시키지 않는다."
    );
}
