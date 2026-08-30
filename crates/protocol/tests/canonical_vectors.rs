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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/vectors/canonical_v1.json")
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

    assert_eq!(
        hex(&si),
        v["sig_input_hex"].as_str().unwrap(),
        "sig_input 불일치"
    );
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
        find(&doc, "v04a_repeated_order_1")["canonical_hex"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        hex(&cb),
        find(&doc, "v04b_repeated_order_2")["canonical_hex"]
            .as_str()
            .unwrap()
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

    assert_ne!(
        as_manifest, as_lease,
        "domain 이 다르면 sig_input 도 달라야 한다"
    );
    assert_eq!(
        hex(&as_manifest),
        v["sig_input_as_manifest_hex"].as_str().unwrap()
    );
    assert_eq!(
        hex(&as_lease),
        v["sig_input_as_lease_hex"].as_str().unwrap()
    );
}

#[test]
fn v11_schema_version_separation() {
    let doc = load_vectors();
    let v = find(&doc, "v11_schema_version_separation");
    let canon = canonical_encode(&minimal_manifest(), &[]);

    assert_eq!(
        hex(&sig_input(Domain::Manifest, 1, &canon)),
        v["sig_input_v1_hex"].as_str().unwrap()
    );
    assert_eq!(
        hex(&sig_input(Domain::Manifest, 2, &canon)),
        v["sig_input_v2_hex"].as_str().unwrap()
    );
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
        assert_eq!(
            actual,
            roots[key].as_str().unwrap(),
            "{key} merkle root 불일치"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// 결정론성 — 100회 인코딩이 모두 동일
// ═══════════════════════════════════════════════════════════════════

#[test]
fn determinism_100_iterations() {
    let mut m = minimal_manifest();
    m.set(14, repeated(&["--epochs", "3", "--lr", "1e-4"]))
        .set(
            15,
            map_of(&[
                ("OMP_NUM_THREADS", "8"),
                ("HF_HOME", "/ws/hf"),
                ("AAA", "1"),
            ]),
        )
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
    assert_eq!(
        decode_varint(&bad, &mut pos),
        Err(CanonicalError::TruncatedVarint)
    );
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
        Domain::Manifest,
        Domain::Grant,
        Domain::Lease,
        Domain::Checkpoint,
        Domain::ReplicaAck,
        Domain::Artifact,
        Domain::Canonical,
        Domain::Release,
    ];
    let mut seen = std::collections::HashSet::new();
    for d in domains {
        assert!(
            seen.insert(sig_input(d, 1, &canon)),
            "domain {d:?} 의 sig_input 이 중복된다"
        );
    }
}

/// 모든 domain tag 가 32바이트이고 서로 다른가.
///
/// # ★ 손으로 쓴 배열을 없앴다 — 같은 결함이 **다섯 번째**였다
///
/// 이 테스트는 원래 `Domain` 값을 손으로 나열했다. 그 배열이 낡으면
/// **새 domain 의 tag 가 32바이트인지·중복인지 한 번도 검사되지 않는다.**
///
/// ```text
/// DoD-02      t1_signing_targets.rs 의 배열이 enum 크기 변화를 못 잡음
/// DoD-06      같은 이유로 GrantAck tag 중복을 한 번도 검사 안 함
/// 2026-08-19  코덱스 감사(p116)가 이 파일의 배열이 24종에 멈춰 있음을 발견
/// 2026-08-29  NodeHeartbeat 추가 시 canonical.rs 의 두 목록이 28 로 남음
/// 2026-08-30  ★ 그 p116 수정 이후에도 **이 배열은 여전히 손으로 쓰여 있어서**
///             NodeHeartbeat·NeighborUnreachableReport 두 종이 빠진 채
///             28종만 검사하고 있었다(독립 검수 4라운드 지적)
/// ```
///
/// 배열을 고치는 것으로는 여섯 번째가 온다. `canonical.rs` 의 매크로가
/// 이미 [`Domain::ALL`] 을 enum 과 함께 생성하므로 **그것을 쓴다** —
/// 빠뜨릴 자리 자체가 없어진다.
#[test]
fn domain_tags_are_32_bytes_and_unique() {
    let mut seen = std::collections::HashSet::new();
    for d in Domain::ALL {
        let t = d.tag_bytes();
        assert_eq!(t.len(), 32, "{d:?} 의 tag 가 32바이트가 아니다");
        assert!(seen.insert(t), "domain tag 중복: {d:?}");
    }
    assert_eq!(
        seen.len(),
        Domain::ALL.len(),
        "domain tag 가 서로 달라야 한다 — 같으면 다른 문맥의 서명을 재사용할 수 있다"
    );
    // ★ 숫자를 손으로 적지 않는다. 적으면 그 숫자만 맞추고 목록은 낡는다.
    assert!(
        !Domain::ALL.is_empty(),
        "Domain::ALL 이 비었다 — 매크로가 깨졌다"
    );
}


/// ★ **규범 문서·Python 참조 구현·Rust 구현의 "메시지 → domain tag" 대응이 같은가.**
///
/// # 왜 이 테스트가 필요한가
///
/// 이 저장소는 "손으로 쓴 domain 목록이 낡는" 결함을 **다섯 번** 겪었다
/// (바로 위 테스트의 표 참조). `canonical.rs` 의 매크로가 Rust 쪽 세
/// 자리를 한꺼번에 만들어 그 셋은 닫혔지만, **남은 두 자리는 여전히
/// 손으로 쓰여 있다**(2026-08-30 독립 검수 5라운드 지적).
///
/// ```text
/// docs/protocol/signing.md §5            규범 표 — 여기 없으면 등록 안 된 것이다
/// tools/canonical/reference_canonical.py Python 참조 구현의 DOMAIN_TAGS
/// ```
///
/// 둘 중 하나가 낡으면 **Rust 와 Python 이 갈라지거나**(교차검증이 그
/// 메시지를 아예 안 봄) **규범에 없는 domain 이 코드에만 생긴다**
/// (`CLAUDE.md` §2 — "새 서명 대상 메시지는 signing.md §5 에 반드시 등록").
///
/// # 집합이 아니라 **대응**을 비교한다
///
/// ★ 초안은 tag **집합**만 비교했다. 그러면 `AddMember` 와 `RemoveMember`
///   의 tag 를 **서로 맞바꿔도 통과한다** — 집합은 그대로이기 때문이다
///   (2026-08-30 독립 검수 7라운드 지적). tag 를 맞바꾸면 한쪽 메시지의
///   서명을 다른 쪽에 재사용할 수 있으므로 `ADR-028` 이 막으려던 바로
///   그 상황이 된다.
///
///   그래서 세 자료 전부에서 **메시지 이름 → tag** 를 뽑아 대조한다.
///   Rust 쪽 대응은 `signable.rs` 의 `impl Signable for pb::X { const
///   DOMAIN: Domain = Domain::Y }` 를 파싱해 만든다.
#[test]
fn domain_tags_match_the_norm_document_and_the_python_reference() {
    use std::collections::BTreeMap;

    /// `signing.md` **§5 절 안의** 표에서만 `메시지 → tag` 를 뽑는다.
    ///
    /// ★ 초안은 문서 전체 표를 훑었다 — 다른 표의 같은 tag 가 §5 행
    ///   삭제를 가릴 수 있었다(독립 검수 7라운드 지적). 이제 §5 로
    ///   시작해 다음 `## ` 절에서 멈춘다.
    fn map_from_norm_section(source: &str) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        let mut inside = false;
        for line in source.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("## ") {
                // §5 가 시작하면 켜고, 그 다음 최상위 절에서 끈다.
                inside = trimmed.starts_with("## 5.");
                continue;
            }
            if !inside || !trimmed.starts_with('|') {
                continue;
            }
            let columns: Vec<&str> = trimmed.split('|').collect();
            // ["", 메시지, tag, ""]
            if columns.len() < 4 {
                continue;
            }
            let name = columns[1].trim().trim_matches('`').trim().to_string();
            if let Some(tag) = extract_tags(columns[2]).into_iter().next() {
                map.insert(name, tag);
            }
        }
        map
    }

    /// Python 참조 구현의 `DOMAIN_TAGS = { ... }` **블록 안 실제 항목**만.
    ///
    /// ★ 주석 처리한 줄은 건너뛴다 — 초안은 `# "AddMember": ...` 로
    ///   사전에서 빼도 tag 를 계속 검출했다(독립 검수 7라운드 지적).
    fn map_from_python_dict(source: &str) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        let mut inside = false;
        for line in source.lines() {
            if line.starts_with("DOMAIN_TAGS") {
                inside = true;
                continue;
            }
            if !inside {
                continue;
            }
            if line.starts_with('}') {
                break;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            // `"Name": b"gputeer/...",` 형태만 받는다.
            //
            // ★ 초안은 키 뒤의 **아무 곳**에서나 첫 tag 를 뽑았다 — 그래서
            //   `"AddMember": b"x",  # gputeer/v1/member-add` 처럼 값이
            //   틀렸는데 주석의 tag 를 읽어 정상으로 봤다(2026-08-30 독립
            //   검수 8라운드 지적). 이제 **값 리터럴 안**만 읽는다.
            let Some(rest) = trimmed.strip_prefix('"') else {
                continue;
            };
            let Some(close) = rest.find('"') else {
                continue;
            };
            let name = rest[..close].to_string();
            // ★ `:` **바로 뒤**의 byte literal 만 읽는다. `find("b\"")` 는
            //   주석까지 뒤져서, 값이 틀려도 같은 줄 주석의 tag 를 읽어
            //   통과했다(2026-08-30 독립 검수 9라운드 지적).
            let after_key = rest[close + 1..].trim_start();
            let Some(after_colon) = after_key.strip_prefix(':') else {
                continue;
            };
            let value_part = after_colon.trim_start();
            let Some(literal) = value_part.strip_prefix("b\"") else {
                continue;
            };
            let Some(literal_end) = literal.find('"') else {
                continue;
            };
            let value = &literal[..literal_end];
            if value.starts_with("gputeer/v") {
                map.insert(name, value.to_string());
            }
        }
        map
    }

    /// `t1_signing_targets.rs` 의 `coverage` 배열에서 `Domain::변형 → 메시지`
    /// 를 뽑는다.
    ///
    /// ★ **왜 이 배열인가** — `signable.rs` 기반 대응은 `Signable` 을 구현한
    ///   메시지(17종)만 덮어서, membership 계열처럼 `ToCanonicalFields` 만
    ///   있는 것은 빠진다. 그래서 두 문서를 **같은 방향으로 함께** 맞바꾸면
    ///   아무도 못 잡았다(2026-08-30 독립 검수 8라운드 지적).
    ///
    ///   그 배열은 **`Domain::ALL` 과 길이·누락이 대조되므로**
    ///   (`domain_coverage_is_explicit`) domain 이 빠지지는 않는다.
    ///
    ///   ★ **이름 대응은 30종이 아니라 26종이다** — `Genesis`·`Audit`·
    ///     `Release`·`Invite` 는 proto 메시지가 없어 `None` 이다(2026-08-30
    ///     독립 검수 10라운드 지적).
    ///
    ///   ★ 그리고 **거기 적힌 메시지 이름의 정확성까지 검증되지는 않는다**
    ///     (같은 검수 9라운드). "권위" 는 "아무도 못 고친다" 가 아니라
    ///     **우회 비용을 올린다** 는 뜻이고, 그 비용은 경우에 따라 다르다 —
    ///
    ///     ```text
    ///     Signable 구현 메시지    네 자리 (canonical.rs 또는 signable.rs +
    ///                             coverage 이름 + signing.md + Python)
    ///     그 외(membership 계열)  세 자리 (coverage 이름 + signing.md + Python)
    ///                             — Rust 쪽 대응이 없어 한 자리가 빠진다
    ///     ```
    fn variant_to_message_from_coverage(source: &str) -> BTreeMap<String, String> {
        // 주석 줄을 먼저 걷어낸다 — 배열 뒤에 `// Domain::X, Some("Y")`
        // 같은 주석이 있으면 실제 대응을 덮어쓸 수 있다(2026-08-30 독립
        // 검수 9라운드 지적).
        //
        // ★ **다만 이것만으로 막히는 공격은 못 찾았다.** 그 주석 공격을
        //   "양쪽 문서 맞바꾸기" 와 결합해 재현해 봤더니, 이 걸러내기를
        //   빼도 다른 검사(같은 메시지에 대한 뒤쪽 항목이 덮어쓰기)가
        //   먼저 잡았다. 그래서 이것은 **증명된 방어가 아니라 위생**이다 —
        //   주석을 읽지 않는 편이 옳으므로 남기되, 없어서는 안 될
        //   방어라고 주장하지 않는다.
        let stripped: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = stripped.as_str();

        let mut map = BTreeMap::new();
        let bytes = source.as_bytes();
        let needle = b"Domain::";
        let mut i = 0;
        while i + needle.len() < bytes.len() {
            if &bytes[i..i + needle.len()] != needle {
                i += 1;
                continue;
            }
            let mut end = i + needle.len();
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
            {
                end += 1;
            }
            let variant = String::from_utf8_lossy(&bytes[i + needle.len()..end]).into_owned();
            // 변형 뒤 공백·쉼표·줄바꿈을 건너뛰고 `Some("...")` 를 기대한다.
            let mut j = end;
            while j < bytes.len() && (bytes[j].is_ascii_whitespace() || bytes[j] == b',') {
                j += 1;
            }
            let tail = &source[j.min(source.len())..];
            if let Some(inner) = tail.strip_prefix("Some(\"") {
                if let Some(q) = inner.find('"') {
                    map.insert(variant, inner[..q].to_string());
                }
            }
            i = end;
        }
        map
    }

    /// `signable.rs` 에서 `pb::메시지 → Domain::변형` 을 뽑는다.
    fn rust_message_to_variant(source: &str) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        let mut pending: Option<String> = None;
        for line in source.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("impl Signable for pb::") {
                pending = Some(rest.trim_end_matches(" {").trim().to_string());
                continue;
            }
            if let Some(name) = pending.clone() {
                if let Some(rest) = trimmed.strip_prefix("const DOMAIN: Domain = Domain::") {
                    map.insert(name, rest.trim_end_matches(';').trim().to_string());
                    pending = None;
                }
            }
        }
        map
    }

    fn extract_tags(source: &str) -> Vec<String> {
        let mut found = Vec::new();
        let bytes = source.as_bytes();
        let needle = b"gputeer/v";
        let mut i = 0;
        while i + needle.len() < bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                let mut end = i + needle.len();
                while end < bytes.len()
                    && (bytes[end].is_ascii_lowercase()
                        || bytes[end].is_ascii_digit()
                        || bytes[end] == b'-'
                        || bytes[end] == b'/')
                {
                    end += 1;
                }
                found.push(String::from_utf8_lossy(&bytes[i..end]).into_owned());
                i = end;
            } else {
                i += 1;
            }
        }
        found
    }

    // ── Rust 의 권위 있는 대응: 메시지 → tag ──────────────────────────
    let variant_to_tag: BTreeMap<String, String> = Domain::ALL
        .iter()
        .map(|d| (format!("{d:?}"), d.as_str().to_string()))
        .collect();
    let rust_map: BTreeMap<String, String> =
        rust_message_to_variant(include_str!("../src/signable.rs"))
            .into_iter()
            .map(|(message, variant)| {
                let tag = variant_to_tag
                    .get(&variant)
                    .unwrap_or_else(|| panic!("Domain::{variant} 이 Domain::ALL 에 없다"))
                    .clone();
                (message, tag)
            })
            .collect();

    let md_map = map_from_norm_section(include_str!("../../../docs/protocol/signing.md"));
    let py_map = map_from_python_dict(include_str!("../../../tools/canonical/reference_canonical.py"));

    // 파서가 공허하지 않은지 먼저 본다.
    assert!(
        rust_map.len() >= 15 && md_map.len() >= 20 && py_map.len() >= 20,
        "파서가 거의 못 찾았다 — 파서 결함이다 (rust={}, md={}, py={})",
        rust_map.len(),
        md_map.len(),
        py_map.len()
    );

    // ── tag 집합: 코드 ↔ 문서 ↔ Python ────────────────────────────────
    let expected: std::collections::BTreeSet<String> =
        Domain::ALL.iter().map(|d| d.as_str().to_string()).collect();
    let md_tags: std::collections::BTreeSet<String> = md_map.values().cloned().collect();
    let py_tags: std::collections::BTreeSet<String> = py_map.values().cloned().collect();

    let missing_in_md: Vec<_> = expected.difference(&md_tags).collect();
    assert!(
        missing_in_md.is_empty(),
        "★ 코드에는 있는데 `signing.md` §5 표에 없는 domain: {missing_in_md:?}\n\
         새 서명 대상 메시지는 규범 표에 반드시 등록한다 (CLAUDE.md §2)"
    );
    let extra_in_md: Vec<_> = md_tags.difference(&expected).collect();
    assert!(
        extra_in_md.is_empty(),
        "★ `signing.md` §5 표에는 있는데 코드에 없는 domain: {extra_in_md:?}"
    );
    let missing_in_py: Vec<_> = expected.difference(&py_tags).collect();
    assert!(
        missing_in_py.is_empty(),
        "★ 코드에는 있는데 Python `DOMAIN_TAGS` 에 없는 domain: {missing_in_py:?}\n\
         그 메시지는 canonical 바이트 교차검증을 한 번도 못 받는다 (DoD-05 와 같은 공백)"
    );
    let extra_in_py: Vec<_> = py_tags.difference(&expected).collect();
    assert!(
        extra_in_py.is_empty(),
        "★ Python `DOMAIN_TAGS` 에는 있는데 코드에 없는 domain: {extra_in_py:?}"
    );

    // ── ★ 메시지 → tag **대응**: 맞바꿔치기를 잡는다 ──────────────────
    let mut mismatches: Vec<String> = Vec::new();
    for (message, rust_tag) in &rust_map {
        if let Some(md_tag) = md_map.get(message) {
            if md_tag != rust_tag {
                mismatches.push(format!(
                    "signing.md: {message} -> {md_tag} (코드는 {rust_tag})"
                ));
            }
        }
        if let Some(py_tag) = py_map.get(message) {
            if py_tag != rust_tag {
                mismatches.push(format!(
                    "reference_canonical.py: {message} -> {py_tag} (코드는 {rust_tag})"
                ));
            }
        }
    }
    // ★ 규범 문서 ↔ Python 도 **서로** 대조한다.
    for (message, md_tag) in &md_map {
        if let Some(py_tag) = py_map.get(message) {
            if md_tag != py_tag {
                mismatches.push(format!(
                    "signing.md 와 reference_canonical.py 가 다르다: {message} -> {md_tag} vs {py_tag}"
                ));
            }
        }
    }

    // ★ **양쪽 문서를 같은 방향으로 맞바꾸는 것**까지 잡는다.
    //
    //   위 두 비교만으로는 못 잡았다 — Rust 기준 비교는 `Signable` 구현
    //   메시지만 덮고, 문서↔Python 비교는 둘이 똑같이 틀리면 통과한다
    //   (2026-08-30 독립 검수 8라운드 지적). `t1_signing_targets.rs` 의
    //   `coverage` 배열이 **proto 메시지가 있는 26종**에 대한 이름 대응을 갖고
    //   있으므로 그것을 기준으로 다시 본다.
    let coverage_map = variant_to_message_from_coverage(include_str!("t1_signing_targets.rs"));
    let mut authoritative: BTreeMap<String, String> = BTreeMap::new();
    for (variant, message) in &coverage_map {
        if let Some(tag) = variant_to_tag.get(variant) {
            authoritative.insert(message.clone(), tag.clone());
        }
    }
    assert!(
        authoritative.len() >= 20,
        "coverage 배열에서 뽑은 대응이 {}개뿐이다 — 파서 결함이다",
        authoritative.len()
    );
    // ★ **있으면 비교** 가 아니라 **반드시 있어야 한다**. 없으면 건너뛰는
    //   구현은 두 문서에서 이름을 함께 바꾸는 우회를 허용했다(2026-08-30
    //   독립 검수 9라운드 지적).
    for (message, tag) in &authoritative {
        match md_map.get(message) {
            None => mismatches.push(format!(
                "signing.md 에 {message} 행이 없다 (권위 대응은 {tag})"
            )),
            Some(md_tag) if md_tag != tag => mismatches.push(format!(
                "signing.md: {message} -> {md_tag} (권위 대응은 {tag})"
            )),
            Some(_) => {}
        }
        match py_map.get(message) {
            None => mismatches.push(format!(
                "reference_canonical.py 에 {message} 항목이 없다 (권위 대응은 {tag})"
            )),
            Some(py_tag) if py_tag != tag => mismatches.push(format!(
                "reference_canonical.py: {message} -> {py_tag} (권위 대응은 {tag})"
            )),
            Some(_) => {}
        }
    }

    assert!(
        mismatches.is_empty(),
        "★ 메시지 → domain tag 대응이 어긋난다 — 두 tag 를 맞바꾸면 한쪽 서명을\n\
         다른 쪽에 재사용할 수 있다 (ADR-028 이 막으려던 상황):\n  {}",
        mismatches.join("\n  ")
    );

    // 대응 비교가 공허하지 않은지 — 실제로 겹치는 이름이 충분히 있는가.
    let compared = rust_map
        .keys()
        .filter(|m| md_map.contains_key(*m) || py_map.contains_key(*m))
        .count();
    assert!(
        compared >= 15,
        "이름이 겹치는 메시지가 {compared}개뿐이다 — 대응 비교가 사실상 안 이뤄졌다"
    );
}
