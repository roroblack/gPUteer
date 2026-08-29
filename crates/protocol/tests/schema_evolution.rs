//! P0-08 — `SCHEMA_TOO_NEW` × prost unknown-field.
//!
//! # 왜 이것이 P0 인가
//!
//! `CLAUDE.md` §0.2 와 `signing.md` §7.2 는 이렇게 못박는다.
//!
//! > **"모르는 필드는 무시하고 통과" 가 아니라 "모르면 검증 불가를 선언" 한다.**
//! > 보안 필드가 추가됐는데 구버전이 그것을 무시한 채 통과시키는 상황을 막기 위해서다.
//!
//! 그런데 **prost 의 기본 동작은 모르는 필드를 조용히 버리는 것**이다.
//! 규범이 요구하는 "모르는 필드가 있음을 안다" 를 prost 는 제공하지 않을 수 있다.
//!
//! 규범이 구현 불가능한 절차를 규정하고 있다면 §7.2 를 다시 써야 한다.
//! **그것이 이 스파이크가 답할 질문이다.**
//!
//! # 실험 방법
//!
//! 신버전 메시지를 흉내내기 위해 **와이어 바이트에 직접 미지 필드를 덧붙인다.**
//! 실제 v2 `.proto` 를 만들지 않는 이유는, 필요한 것이 "v1 디코더가 미지 필드를
//! 만났을 때의 행동" 이지 v2 스키마 자체가 아니기 때문이다.

use prost::Message;

use gputeer_protocol::canonical::{canonical_encode, sig_input, Domain};
use gputeer_protocol::{pb, ToCanonicalFields};

fn base_manifest() -> pb::JobManifest {
    pb::JobManifest {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000000AA".into(),
        team_id: "01JBXR7Q0000000000000000TT".into(),
        entrypoint: "train.py".into(),
        submitter_device_id: "01JBXR7Q0000000000000000DD".into(),
        issued_at_unix_ms: 1_755_100_800_000,
        expires_at_unix_ms: 1_755_705_600_000,
        network: Some(pb::NetworkPolicy {
            mediated_dns: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn varint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

/// 아직 존재하지 않는 field number 를 varint 로 덧붙인다.
/// = "v2 가 추가한 새 필드" 를 흉내낸다.
fn append_unknown_varint_field(bytes: &mut Vec<u8>, field: u32, value: u64) {
    bytes.extend(varint(((field as u64) << 3) | 0)); // wiretype 0 = varint
    bytes.extend(varint(value));
}

/// 아직 존재하지 않는 field number 에 length-delimited 값을 덧붙인다.
/// = "v2 가 추가한 새 중첩 메시지 / 문자열" 을 흉내낸다.
fn append_unknown_len_field(bytes: &mut Vec<u8>, field: u32, payload: &[u8]) {
    bytes.extend(varint(((field as u64) << 3) | 2)); // wiretype 2
    bytes.extend(varint(payload.len() as u64));
    bytes.extend(payload);
}

// ══════════════════════════════════════════════════════════════════
// 질문 1 — prost 는 미지 필드를 만나면 어떻게 하는가?
// ══════════════════════════════════════════════════════════════════

#[test]
fn q1_prost_accepts_and_drops_unknown_fields() {
    let m = base_manifest();
    let mut wire = m.encode_to_vec();
    let original_len = wire.len();

    // v2 가 추가했다고 가정하는 필드들. 현재 job.proto 에 63·70 은 없다.
    append_unknown_varint_field(&mut wire, 63, 999);
    append_unknown_len_field(&mut wire, 70, b"new-security-constraint");

    let decoded = pb::JobManifest::decode(&wire[..])
        .expect("prost 가 미지 필드를 오류로 처리하지 않았다면 여기까지 온다");

    let reencoded = decoded.encode_to_vec();

    println!(
        "원본 {original_len}B -> 미지 필드 추가 {}B -> 재인코딩 {}B",
        wire.len(),
        reencoded.len()
    );

    // ★ 이 단언들이 P0-08 의 실측 결과다.
    assert_eq!(
        reencoded.len(),
        original_len,
        "prost 가 미지 필드를 보존했다. 이 경우 §7.2 의 전제가 달라진다"
    );
    assert_eq!(
        decoded, m,
        "미지 필드를 버린 뒤 디코드 결과가 원본과 같다 = 구버전은 차이를 볼 수 없다"
    );
}

/// ★ 가장 중요한 결과 — 미지 필드는 canonical 에 **아무 흔적도 남기지 않는다.**
#[test]
fn q2_unknown_fields_are_invisible_to_canonical() {
    let m = base_manifest();
    let canon_before = canonical_encode(&m.to_canonical_fields(), &[]);

    let mut wire = m.encode_to_vec();
    append_unknown_len_field(&mut wire, 70, b"runtime_allow_hosts: evil.example");
    let decoded = pb::JobManifest::decode(&wire[..]).unwrap();

    let canon_after = canonical_encode(&decoded.to_canonical_fields(), &[]);

    assert_eq!(
        canon_before, canon_after,
        "미지 필드가 canonical 에 반영됐다면 이 스파이크의 전제가 틀렸다"
    );

    // 즉 — 구버전 검증자는 **메시지 본문만 보고는 미지 필드의 존재를 알 수 없다.**
    // 유일한 신호는 schema_version 이다.
}

// ══════════════════════════════════════════════════════════════════
// 질문 2 — 그렇다면 §7.2 의 SCHEMA_TOO_NEW 는 구현 가능한가?
// ══════════════════════════════════════════════════════════════════

/// 안전한 경로 — 신버전 발신자가 규칙대로 `schema_version` 을 올린 경우.
#[test]
fn q3_version_bump_is_detectable_and_signature_bound() {
    let mut v2 = base_manifest();
    v2.schema_version = 2;

    let canon = canonical_encode(&v2.to_canonical_fields(), &[]);

    // 발신자는 version 2 로 서명한다
    let signed_as_v2 = sig_input(Domain::Manifest, 2, &canon);
    // 구버전 검증자가 version 1 로 재구성하면 sig_input 이 다르다
    let rebuilt_as_v1 = sig_input(Domain::Manifest, 1, &canon);

    assert_ne!(
        signed_as_v2, rebuilt_as_v1,
        "schema_version 이 sig_input 에 묶여 있지 않다면 버전 강등이 가능하다"
    );

    // → 구버전은 schema_version=2 를 보고 SCHEMA_TOO_NEW 를 반환할 수 있다.
    //   설령 그 검사를 빠뜨려도 서명 검증이 실패한다 (이중 방어).
    println!("schema_version 상향: 탐지 가능 + 서명에 묶임");
}

/// ★ 위험한 경로 — 신버전 발신자가 **`schema_version` 을 올리지 않고** 필드를 추가한 경우.
///
/// `signing.md` §7.3 이 "schema_version 증가 없는 필드 추가" 를 **금지**하는 이유가
/// 바로 이것이다. 그런데 **금지는 프로토콜이 강제하지 못한다.** 프로세스 규칙이다.
#[test]
fn q4_field_addition_without_version_bump_is_undetectable() {
    let m = base_manifest(); // schema_version = 1 그대로

    let mut wire = m.encode_to_vec();
    // v2 가 "새 보안 제약" 을 field 70 에 넣고 schema_version 은 올리지 않았다
    append_unknown_len_field(&mut wire, 70, b"require_attestation: true");

    let decoded = pb::JobManifest::decode(&wire[..]).unwrap();

    // 구버전 검증자 입장:
    assert_eq!(decoded.schema_version, 1, "버전은 1이라고 적혀 있다");
    let canon = canonical_encode(&decoded.to_canonical_fields(), &[]);
    let si = sig_input(Domain::Manifest, 1, &canon);

    // 발신자도 같은 canonical(구버전 필드만)로 서명했다면 — 검증은 **통과한다.**
    let sender_si = sig_input(
        Domain::Manifest,
        1,
        &canonical_encode(&m.to_canonical_fields(), &[]),
    );
    assert_eq!(
        si, sender_si,
        "구버전이 새 보안 제약을 무시한 채 서명 검증을 통과시킨다"
    );

    // ★ 이것이 P0-08 의 결론이다.
    //   프로토콜은 이 시나리오를 막지 못한다. §7.3 의 "금지" 는 프로세스 규칙이며,
    //   `field_number_audit` 같은 **빌드 시점 검사**로만 강제할 수 있다.
    println!(
        "★ schema_version 을 올리지 않은 필드 추가는 프로토콜 수준에서 탐지 불가. \
         빌드 시점 검사로 막아야 한다"
    );
}

// ══════════════════════════════════════════════════════════════════
// 질문 3 — 실패 모드가 사실을 정확히 전하는가?
//
// CLAUDE.md §3: "오류 메시지가 사실을 잘못 전하지 않게 한다."
// stale lease 를 "서명 실패" 로 보고하면 한참 헤맨다는 그 규칙이다.
// ══════════════════════════════════════════════════════════════════

#[test]
fn q5_version_check_must_precede_signature_check() {
    // 신버전 메시지. 구버전은 이것을 두 가지 방식으로 거부할 수 있다.
    let mut v2 = base_manifest();
    v2.schema_version = 2;

    const MAX_SUPPORTED: u32 = 1;

    // (a) 규범대로 — 버전을 먼저 본다
    let outcome_a = if v2.schema_version > MAX_SUPPORTED {
        "SCHEMA_TOO_NEW"
    } else {
        "검증 진행"
    };

    // (b) 버전 검사를 빠뜨리고 서명부터 본다
    let canon = canonical_encode(&v2.to_canonical_fields(), &[]);
    let rebuilt = sig_input(Domain::Manifest, MAX_SUPPORTED, &canon);
    let actual = sig_input(Domain::Manifest, v2.schema_version, &canon);
    let outcome_b = if rebuilt != actual {
        "INVALID_SIGNATURE"
    } else {
        "VALID"
    };

    assert_eq!(outcome_a, "SCHEMA_TOO_NEW");
    assert_eq!(outcome_b, "INVALID_SIGNATURE");

    // ★ 둘 다 "거부" 지만 **운영자에게 전하는 사실이 다르다.**
    //   (b) 는 "서명이 위조됐다" 로 읽힌다 — 실제로는 "업그레이드가 필요하다" 인데.
    //   그래서 §8 의 검증 순서(버전 -> 서명)는 안전성이 아니라
    //   **진단 정확성** 때문에 지켜야 한다. 안전성은 둘 다 확보된다.
    println!("버전 먼저: {outcome_a} / 서명 먼저: {outcome_b} — 둘 다 거부하나 진단이 다르다");
}

/// 버전 강등 공격 — 공격자가 schema_version 2 -> 1 로 낮추면?
#[test]
fn q6_version_downgrade_breaks_signature() {
    let mut v2 = base_manifest();
    v2.schema_version = 2;
    let canon_v2 = canonical_encode(&v2.to_canonical_fields(), &[]);
    let signed = sig_input(Domain::Manifest, 2, &canon_v2);

    // 공격자가 버전만 1로 내린다
    let mut tampered = v2.clone();
    tampered.schema_version = 1;
    let canon_t = canonical_encode(&tampered.to_canonical_fields(), &[]);
    let rebuilt = sig_input(Domain::Manifest, 1, &canon_t);

    assert_ne!(
        signed, rebuilt,
        "버전 강등이 서명을 깨지 않는다면 SCHEMA_TOO_NEW 를 우회할 수 있다"
    );

    // schema_version 은 canonical(필드 1) 과 sig_input 양쪽에 들어간다.
    // 이중으로 묶여 있어 강등은 반드시 서명을 깬다.
    assert_ne!(
        canon_v2, canon_t,
        "schema_version 이 canonical 에도 들어가야 한다"
    );
}
