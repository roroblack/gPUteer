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
fn unimplemented_field_list_is_empty() {
    // 2026-08-16 에 6건을 전부 구현해 비웠다.
    // 비어 있지 않다면 **서명 밖에 있는 위조 가능한 필드가 있다**는 뜻이다.
    for (msg, num, desc) in UNIMPLEMENTED_FIELDS {
        println!("★ 서명 밖 필드: {msg} field {num} — {desc}");
    }
    assert!(
        UNIMPLEMENTED_FIELDS.is_empty(),
        "서명 대상에서 빠진 필드가 {}건 있다. 위조 가능하다",
        UNIMPLEMENTED_FIELDS.len()
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 보안 필드가 실제로 서명에 들어가는가
//
// 이 테스트들의 요점은 "canonical 이 달라져야 한다" 는 것이다.
// 같으면 그 필드는 서명에 반영되지 않은 것이고, 곧 위조 가능하다는 뜻이다.
// ══════════════════════════════════════════════════════════════════

fn changes_canonical(mutate: impl FnOnce(&mut pb::JobManifest)) -> bool {
    let base = canonical_encode(&minimal_manifest().to_canonical_fields(), &[]);
    let mut m = minimal_manifest();
    mutate(&mut m);
    canonical_encode(&m.to_canonical_fields(), &[]) != base
}

#[test]
fn network_policy_is_signed() {
    assert!(
        changes_canonical(|m| {
            m.network = Some(pb::NetworkPolicy {
                runtime_allow_hosts: vec!["evil.example".into()],
                mediated_dns: false,
                ..Default::default()
            });
        }),
        "network(54) 가 서명 밖이다 — 중간자가 네트워크 정책을 고쳐도 검증이 통과한다"
    );
}

#[test]
fn artifact_scope_is_signed() {
    assert!(
        changes_canonical(|m| {
            m.artifact_scope = Some(pb::ArtifactScope {
                writable_prefixes: vec!["/".into()],
                ..Default::default()
            });
        }),
        "artifact_scope(55) 가 서명 밖이다 — 산출물 쓰기 범위를 넓힐 수 있다"
    );
}

#[test]
fn execution_environment_is_signed() {
    assert!(
        changes_canonical(|m| {
            m.env = Some(pb::ExecutionEnvironment {
                image_ref: "registry.evil/backdoor:1".into(),
                image_digest: Some(pb::Digest {
                    algo: 1,
                    value: vec![0xEE; 32],
                }),
                ..Default::default()
            });
        }),
        "env(10) 이 서명 밖이다 — 실행 이미지를 통째로 교체할 수 있다"
    );
}

#[test]
fn dataset_ref_is_signed() {
    assert!(
        changes_canonical(|m| {
            m.dataset = Some(pb::DatasetRef {
                total_bytes: 12345,
                sensitivity: 3,
                display_name: "ds".into(),
                ..Default::default()
            });
        }),
        "dataset(12) 이 서명 밖이다 — SENSITIVE 표시와 삭제 정책을 떼어낼 수 있다"
    );
}

#[test]
fn input_artifacts_are_signed_and_order_is_preserved() {
    let d = |b: u8| pb::Digest {
        algo: 1,
        value: vec![b; 32],
    };
    assert!(
        changes_canonical(|m| m.input_artifacts = vec![d(1), d(2)]),
        "input_artifacts(11) 가 서명 밖이다 — 입력 산출물을 바꿔칠 수 있다"
    );

    // 규칙 d — repeated 는 정렬하지 않는다
    let mut a = minimal_manifest();
    a.input_artifacts = vec![d(1), d(2)];
    let mut b = minimal_manifest();
    b.input_artifacts = vec![d(2), d(1)];
    assert_ne!(
        canonical_encode(&a.to_canonical_fields(), &[]),
        canonical_encode(&b.to_canonical_fields(), &[]),
        "repeated message 순서가 canonical 에 반영되지 않았다 (규칙 d)"
    );
}

#[test]
fn lease_scope_is_signed() {
    let base = pb::Lease {
        schema_version: 1,
        lease_id: "01JBXLEASE0000000000000001".into(),
        fence_epoch: 42,
        ..Default::default()
    };
    let c0 = canonical_encode(&base.to_canonical_fields(), &[]);

    let mut scoped = base.clone();
    scoped.scope = Some(pb::ResourceScope {
        gpu_uuids: vec!["GPU-aaaa".into()],
        cpu_cores: 64,
        writable_prefixes: vec!["/".into()],
        ..Default::default()
    });
    assert_ne!(
        canonical_encode(&scoped.to_canonical_fields(), &[]),
        c0,
        "Lease.scope(40) 이 서명 밖이다 — 보유자가 스스로 자원 범위를 넓힐 수 있다"
    );
}

// ══════════════════════════════════════════════════════════════════
// 신규 벡터 — 참조 구현과 바이트 대조
// ══════════════════════════════════════════════════════════════════

#[test]
fn security_field_vectors_match_reference() {
    let doc = vectors();

    let mut m = minimal_manifest();
    m.network = Some(pb::NetworkPolicy {
        staging_allow_hosts: vec!["pypi.internal".into(), "mirror.internal".into()],
        runtime_allow_hosts: vec![],
        mediated_dns: true,
    });
    m.artifact_scope = Some(pb::ArtifactScope {
        writable_prefixes: vec!["jobs/01JBXR7Q0000000000000000AA/".into()],
        readable_prefixes: vec!["datasets/shared/".into()],
    });

    assert_eq!(
        hex(&canonical_encode(&m.to_canonical_fields(), &[])),
        find(&doc, "v14_security_fields_are_signed")["canonical_hex"]
            .as_str()
            .unwrap(),
        "보안 필드 canonical 이 참조 구현과 다르다"
    );
}

#[test]
fn dataset_vector_matches_reference() {
    let doc = vectors();
    let mut m = minimal_manifest();
    m.dataset = Some(pb::DatasetRef {
        root_digest: Some(pb::Digest {
            algo: 1,
            value: vec![0xAB; 32],
        }),
        total_bytes: 42_949_672_960,
        sensitivity: 3,
        retention: 2,
        encrypted_at_rest: true,
        display_name: "internal-corpus-v3".into(),
    });
    assert_eq!(
        hex(&canonical_encode(&m.to_canonical_fields(), &[])),
        find(&doc, "v17_dataset_is_signed")["canonical_hex"]
            .as_str()
            .unwrap()
    );
}

#[test]
fn input_artifacts_vector_matches_reference() {
    let doc = vectors();
    let d = |b: u8| pb::Digest {
        algo: 1,
        value: vec![b; 32],
    };
    let mut m = minimal_manifest();
    m.input_artifacts = vec![d(1), d(2)];
    assert_eq!(
        hex(&canonical_encode(&m.to_canonical_fields(), &[])),
        find(&doc, "v16a_input_artifacts_order_1")["canonical_hex"]
            .as_str()
            .unwrap()
    );
}

#[test]
fn execution_environment_vector_matches_reference() {
    let doc = vectors();
    let mut m = minimal_manifest();
    m.env = Some(pb::ExecutionEnvironment {
        kind: 1,
        image_ref: "registry.internal/torch:2.4-cu124".into(),
        image_digest: Some(pb::Digest {
            algo: 1,
            value: (0u8..32).collect(),
        }),
        os: "linux".into(),
        arch: "amd64".into(),
        min_libc_version: "2.31".into(),
        cuda: Some(pb::CudaRequirement {
            min_driver_version: 550,
            cuda_runtime_version: "12.4".into(),
            compute_capabilities: vec![],
        }),
        code_digest: Some(pb::Digest {
            algo: 1,
            value: (32u8..64).collect(),
        }),
        tarball_policy: Some(pb::TarballPolicy {
            reject_path_traversal: true,
            reject_links: true,
            max_extracted_bytes: 10 * 1024 * 1024 * 1024,
        }),
        ..Default::default()
    });
    assert_eq!(
        hex(&canonical_encode(&m.to_canonical_fields(), &[])),
        find(&doc, "v15_execution_environment_is_signed")["canonical_hex"]
            .as_str()
            .unwrap(),
        "4단 중첩(JobManifest→ExecutionEnvironment→Digest) canonical 이 참조 구현과 다르다"
    );
}

#[test]
fn lease_scope_vector_matches_reference() {
    let doc = vectors();
    let l = pb::Lease {
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
        scope: Some(pb::ResourceScope {
            gpu_uuids: vec!["GPU-11111111-2222-3333-4444-555555555555".into()],
            cpu_cores: 8,
            ram_bytes: 25_769_803_776,
            workspace_bytes: 85_899_345_920,
            writable_prefixes: vec!["jobs/01JBXR7Q0000000000000000AA/attempt-3/".into()],
        }),
        coordinator_signature: vec![0xCD; 64],
    };
    assert_eq!(
        hex(&canonical_encode(&l.to_canonical_fields(), &[])),
        find(&doc, "v19_lease_scope_is_signed")["canonical_hex"]
            .as_str()
            .unwrap()
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 전 필드 대조 — 가장 강한 교차검증
//
// v02 는 JobManifest 의 **모든 필드**를 채운 896바이트 벡터다.
// 부분 벡터는 "채우지 않은 필드" 를 검증하지 못한다. 이 테스트가 그 구멍을 메운다.
// 참조 구현 쪽에서도 `missing_from_full()` 이 "정말 전 필드인가" 를 자동 검사한다.
// ══════════════════════════════════════════════════════════════════

fn full_manifest() -> pb::JobManifest {
    let dg = |b: Vec<u8>| Some(pb::Digest { algo: 1, value: b });
    pb::JobManifest {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        team_id: "01JBXR7Q0000000000000000TT".into(),
        env: Some(pb::ExecutionEnvironment {
            kind: 1,
            image_ref: "registry.internal/torch:2.4-cu124".into(),
            image_digest: dg((0u8..32).collect()),
            oci_source_digest: Some(pb::Digest {
                algo: 2,
                value: (1u8..33).collect(),
            }),
            base_runtime: "python-3.11-cu124".into(),
            lock_digest: dg((2u8..34).collect()),
            lock_content: b"torch==2.4.0\n".to_vec(),
            lock_cas_ref: dg((3u8..35).collect()),
            os: "linux".into(),
            arch: "amd64".into(),
            min_libc_version: "2.31".into(),
            cuda: Some(pb::CudaRequirement {
                min_driver_version: 550,
                cuda_runtime_version: "12.4".into(),
                compute_capabilities: vec!["8.9".into()],
            }),
            code_digest: dg((4u8..36).collect()),
            tarball_policy: Some(pb::TarballPolicy {
                reject_path_traversal: true,
                reject_links: true,
                max_extracted_bytes: 10 * 1024 * 1024 * 1024,
            }),
        }),
        input_artifacts: vec![
            pb::Digest {
                algo: 1,
                value: vec![1u8; 32],
            },
            pb::Digest {
                algo: 1,
                value: vec![2u8; 32],
            },
        ],
        dataset: Some(pb::DatasetRef {
            root_digest: dg(vec![0xABu8; 32]),
            total_bytes: 42_949_672_960,
            sensitivity: 3,
            retention: 2,
            encrypted_at_rest: true,
            display_name: "internal-corpus-v3".into(),
        }),
        entrypoint: "train.py".into(),
        args: vec!["--epochs".into(), "3".into(), "--lr".into(), "1e-4".into()],
        env_vars: [
            ("OMP_NUM_THREADS", "8"),
            ("HF_HOME", "/ws/hf"),
            ("AAA", "1"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
        resources: Some(pb::ResourceRequest {
            gpu: Some(pb::GpuRequest {
                min_vram_bytes: 19_327_352_832,
                min_count: 1,
                cuda: Some(pb::CudaRequirement {
                    min_driver_version: 550,
                    cuda_runtime_version: "12.4".into(),
                    compute_capabilities: vec!["8.6".into(), "8.9".into(), "9.0".into()],
                }),
                allocation_mode: 1,
                allowed_gpu_models: vec![],
            }),
            cpu_cores: 8,
            ram_bytes: 25_769_803_776,
            workspace_bytes: 85_899_345_920,
            max_egress_bps: 0,
        }),
        workload: Some(pb::WorkloadHint {
            class: 1,
            estimated_steps: 20000,
            ref_step_time_ms: 420,
            ref_gpu_model: "RTX4090".into(),
            model_params: 1_300_000_000,
            est_checkpoint_bytes: 18_200_000_000,
            est_peak_vram_bytes: 0,
        }),
        deadline_minutes: 180,
        preference: 2,
        max_queue_minutes: 60,
        checkpoint_interval_minutes: 15,
        durability: 3,
        on_partition: 2,
        max_data_loss_minutes: 30,
        minimum_isolation_class: 2,
        minimum_security_tier: 3,
        minimum_key_protection: 2,
        side_effect_class: 1,
        network: Some(pb::NetworkPolicy {
            staging_allow_hosts: vec!["pypi.internal".into(), "mirror.internal".into()],
            runtime_allow_hosts: vec!["metrics.internal".into()],
            mediated_dns: true,
        }),
        artifact_scope: Some(pb::ArtifactScope {
            writable_prefixes: vec!["jobs/01JBXR7Q0000000000000000AA/".into()],
            readable_prefixes: vec!["datasets/shared/".into()],
        }),
        acknowledge_duplicate_risk: true,
        submitter_device_id: "01JBXR7Q0000000000000000DD".into(),
        issued_at_unix_ms: 1_755_100_800_000,
        expires_at_unix_ms: 1_755_705_600_000,
        submitter_signature: vec![0xAA; 64],
    }
}

#[test]
fn v02_full_manifest_matches_reference() {
    let doc = vectors();
    let v = find(&doc, "v02_full_manifest");
    let m = full_manifest();
    let canon = canonical_encode(&m.to_canonical_fields(), &[]);

    assert_eq!(
        hex(&canon),
        v["canonical_hex"].as_str().unwrap(),
        "전 필드 매니페스트가 참조 구현과 다르다"
    );
    assert_eq!(canon.len() as u64, v["canonical_len"].as_u64().unwrap());

    let si = sig_input(Domain::Manifest, 1, &canon);
    assert_eq!(hex(&si), v["sig_input_hex"].as_str().unwrap());
    assert_eq!(
        hex(&blake3_256(&si)),
        v["sig_input_blake3_256"].as_str().unwrap()
    );
    println!("v02 전 필드: {} bytes 일치", canon.len());
}

#[test]
fn v02b_full_lease_matches_reference() {
    let doc = vectors();
    let v = find(&doc, "v02b_full_lease");
    let l = pb::Lease {
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
        scope: Some(pb::ResourceScope {
            gpu_uuids: vec!["GPU-11111111-2222-3333-4444-555555555555".into()],
            cpu_cores: 8,
            ram_bytes: 25_769_803_776,
            workspace_bytes: 85_899_345_920,
            writable_prefixes: vec!["jobs/01JBXR7Q0000000000000000AA/attempt-3/".into()],
        }),
        coordinator_signature: vec![0xCD; 64],
    };
    let canon = canonical_encode(&l.to_canonical_fields(), &[]);
    assert_eq!(hex(&canon), v["canonical_hex"].as_str().unwrap());

    let si = sig_input(Domain::Lease, 1, &canon);
    assert_eq!(hex(&si), v["sig_input_hex"].as_str().unwrap());
    assert_eq!(
        hex(&blake3_256(&si)),
        v["sig_input_blake3_256"].as_str().unwrap()
    );
}

/// ★ 전 필드 벡터의 **모든 필드**가 실제로 서명에 영향을 주는가.
///
/// 어떤 필드든 하나씩 기본값으로 되돌리면 canonical 이 반드시 달라져야 한다.
/// 달라지지 않는 필드가 있다면 그 필드는 **서명 밖**이고 위조 가능하다.
///
/// 벡터 대조만으로는 이것을 못 잡는다 — 참조 구현도 같은 필드를 빠뜨렸다면
/// 두 구현이 사이좋게 틀린 채로 일치한다.
#[test]
fn every_field_in_full_manifest_affects_canonical() {
    let base = canonical_encode(&full_manifest().to_canonical_fields(), &[]);

    type Mut = Box<dyn Fn(&mut pb::JobManifest)>;
    let mutations: Vec<(&str, Mut)> = vec![
        ("job_id(2)", Box::new(|m: &mut pb::JobManifest| m.job_id.clear())),
        ("team_id(3)", Box::new(|m: &mut pb::JobManifest| m.team_id.clear())),
        ("env(10)", Box::new(|m: &mut pb::JobManifest| m.env = None)),
        ("input_artifacts(11)", Box::new(|m: &mut pb::JobManifest| m.input_artifacts.clear())),
        ("dataset(12)", Box::new(|m: &mut pb::JobManifest| m.dataset = None)),
        ("entrypoint(13)", Box::new(|m: &mut pb::JobManifest| m.entrypoint.clear())),
        ("args(14)", Box::new(|m: &mut pb::JobManifest| m.args.clear())),
        ("env_vars(15)", Box::new(|m: &mut pb::JobManifest| m.env_vars.clear())),
        ("resources(20)", Box::new(|m: &mut pb::JobManifest| m.resources = None)),
        ("workload(21)", Box::new(|m: &mut pb::JobManifest| m.workload = None)),
        ("deadline_minutes(30)", Box::new(|m: &mut pb::JobManifest| m.deadline_minutes = 0)),
        ("preference(31)", Box::new(|m: &mut pb::JobManifest| m.preference = 0)),
        ("max_queue_minutes(32)", Box::new(|m: &mut pb::JobManifest| m.max_queue_minutes = 0)),
        ("checkpoint_interval_minutes(40)", Box::new(|m: &mut pb::JobManifest| m.checkpoint_interval_minutes = 0)),
        ("durability(41)", Box::new(|m: &mut pb::JobManifest| m.durability = 0)),
        ("on_partition(42)", Box::new(|m: &mut pb::JobManifest| m.on_partition = 0)),
        ("max_data_loss_minutes(43)", Box::new(|m: &mut pb::JobManifest| m.max_data_loss_minutes = 0)),
        ("minimum_isolation_class(50)", Box::new(|m: &mut pb::JobManifest| m.minimum_isolation_class = 0)),
        ("minimum_security_tier(51)", Box::new(|m: &mut pb::JobManifest| m.minimum_security_tier = 0)),
        ("minimum_key_protection(52)", Box::new(|m: &mut pb::JobManifest| m.minimum_key_protection = 0)),
        ("side_effect_class(53)", Box::new(|m: &mut pb::JobManifest| m.side_effect_class = 0)),
        ("network(54)", Box::new(|m: &mut pb::JobManifest| m.network = None)),
        ("artifact_scope(55)", Box::new(|m: &mut pb::JobManifest| m.artifact_scope = None)),
        ("acknowledge_duplicate_risk(56)", Box::new(|m: &mut pb::JobManifest| m.acknowledge_duplicate_risk = false)),
        ("submitter_device_id(60)", Box::new(|m: &mut pb::JobManifest| m.submitter_device_id.clear())),
        ("issued_at_unix_ms(61)", Box::new(|m: &mut pb::JobManifest| m.issued_at_unix_ms = 0)),
        ("expires_at_unix_ms(62)", Box::new(|m: &mut pb::JobManifest| m.expires_at_unix_ms = 0)),
    ];

    let mut unsigned = Vec::new();
    for (name, mutate) in &mutations {
        let mut m = full_manifest();
        mutate(&mut m);
        if canonical_encode(&m.to_canonical_fields(), &[]) == base {
            unsigned.push(*name);
        }
    }
    assert!(
        unsigned.is_empty(),
        "지워도 canonical 이 변하지 않는 필드가 있다 = 서명 밖 = 위조 가능:\n  {}",
        unsigned.join("\n  ")
    );

    // schema_version(1) 은 canonical 이 아니라 sig_input 에 들어간다 (signing.md §4).
    // canonical 만 보면 놓치므로 여기서 별도로 확인한다.
    assert_ne!(
        sig_input(Domain::Manifest, 1, &base),
        sig_input(Domain::Manifest, 2, &base),
        "schema_version 이 sig_input 에 반영되지 않았다"
    );

    // 서명 필드(90)는 반대로 **변하면 안 된다**
    let mut sig_mut = full_manifest();
    sig_mut.submitter_signature = vec![0x11; 64];
    assert_eq!(
        canonical_encode(&sig_mut.to_canonical_fields(), &[]),
        base,
        "서명 필드(90)가 canonical 에 들어갔다"
    );

    println!(
        "전 필드 {}개 전부 서명에 반영됨 (+ schema_version 은 sig_input)",
        mutations.len()
    );
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
