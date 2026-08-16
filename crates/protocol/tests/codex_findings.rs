//! 코덱스 독립 검토(2026-08-16)가 지적한 결함의 실측 확인.
//!
//! `CLAUDE.md` §4 — "중요한 판단은 독립 검수자와 교차검증한다.
//! 반박당하면 실측으로 가린다."
//!
//! ★ **지적을 액면 그대로 받지 않는다.** 각 지적이 실제로 성립하는지
//!   먼저 테스트로 확인하고, 성립하는 것만 고친다.
//!
//! # 지적 목록
//!
//! ```text
//! C-1  규칙 i 가 Fields 수준에서 새어나간다
//!      Message({90: sig}) 는 is_default() 가 false 라 빈 중첩 메시지로 출력된다.
//!      Message({}) 는 생략된다. => 서명 필드가 canonical 에 영향을 준다
//!
//! C-2  verify() 의 nonce 가 메시지 필드와 결속되지 않았다
//!      호출자가 서명된 nonce 대신 아무 값이나 넘길 수 있다
//!
//! C-3  map 값이 빈 문자열일 때 규칙 b 가 모호하다
//! ```

use gputeer_protocol::canonical::{canonical_encode, Fields, Value, SIGNATURE_FIELD};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ══════════════════════════════════════════════════════════════════
// C-1 ★ 규칙 i 가 Fields 수준에서 새어나간다
// ══════════════════════════════════════════════════════════════════

/// `signing.md` 규칙 i — "서명 필드(90)는 **제외**한다."
///
/// 그 뜻은 **서명 필드가 canonical 에 영향을 주지 않는다**는 것이다.
/// 그런데 중첩 메시지가 서명 필드만 가진 경우 그 규칙이 깨진다.
///
/// ```text
/// Message({})           is_default() == true   -> 생략된다
/// Message({90: sig})    is_default() == false  -> 빈 중첩 메시지로 **출력된다**
/// ```
///
/// 두 값은 서명 필드를 제외하면 **의미가 같다**(서명 대상 내용이 없다).
/// 그런데 canonical 이 다르다.
///
/// # 지금 악용 가능한가
///
/// 아니다 — `to_fields.rs` 가 field 90 을 `Fields` 에 애초에 넣지 않는다.
/// 그러나 **규칙이 자료구조 수준에서 지켜지지 않는다**는 것은 사실이며,
/// 자동 생성기나 새 mapper 가 field 90 을 넣는 순간 깨진다.
#[test]
fn c1_signature_only_nested_message_leaks_into_canonical() {
    let mut empty = Fields::new();
    empty.set(1, Value::Message(Fields::new()));

    let mut sig_only = Fields::new();
    let mut inner = Fields::new();
    inner.set(SIGNATURE_FIELD, Value::Bytes(vec![0xAA; 64]));
    sig_only.set(1, Value::Message(inner));

    let a = canonical_encode(&empty, &[]);
    let b = canonical_encode(&sig_only, &[]);

    println!("C-1  Message({{}})       -> {} ({}B)", hex(&a), a.len());
    println!("C-1  Message({{90:sig}}) -> {} ({}B)", hex(&b), b.len());

    assert_eq!(
        a, b,
        "★ 코덱스 지적 C-1 성립 — 서명 필드만 가진 중첩 메시지가 canonical 을 바꾼다.\n\
         규칙 i 는 '서명 필드가 canonical 에 영향을 주지 않는다' 를 뜻하는데,\n\
         is_default() 검사가 field 90 제외보다 **먼저** 일어나 그 규칙이 새어나간다."
    );
}

/// 최상위에서도 같은 문제가 있는가.
///
/// 최상위는 `canonical_encode` 가 90 을 건너뛰므로 출력에는 영향이 없다.
/// 중첩만 문제다 — 그 사실을 고정해 둔다.
#[test]
fn c1b_top_level_signature_field_is_correctly_excluded() {
    let mut plain = Fields::new();
    plain.set(1, Value::Uint(7));

    let mut signed = Fields::new();
    signed.set(1, Value::Uint(7));
    signed.set(SIGNATURE_FIELD, Value::Bytes(vec![0xFF; 64]));

    assert_eq!(
        canonical_encode(&plain, &[]),
        canonical_encode(&signed, &[]),
        "최상위 서명 필드 제외가 깨졌다"
    );
}

/// 도출 해시 필드도 같은 문제를 갖는가.
#[test]
fn c1c_derived_hash_only_nested_message_has_same_problem() {
    const DERIVED: u32 = 4;

    let mut empty = Fields::new();
    empty.set(1, Value::Message(Fields::new()));

    let mut derived_only = Fields::new();
    let mut inner = Fields::new();
    inner.set(DERIVED, Value::Bytes(vec![0xBB; 32]));
    derived_only.set(1, Value::Message(inner));

    assert_eq!(
        canonical_encode(&empty, &[DERIVED]),
        canonical_encode(&derived_only, &[DERIVED]),
        "도출 해시 필드만 가진 중첩 메시지도 canonical 을 바꾼다 (C-1 과 같은 원인)"
    );
}

// ══════════════════════════════════════════════════════════════════
// C-3  map 값이 빈 문자열일 때
// ══════════════════════════════════════════════════════════════════

/// 코덱스 지적 — `{"k": ""}` 가 규칙 b 를 어기는가.
///
/// **결론: 어기지 않는다.** 규칙 b 는 **필드**의 기본값을 생략하라는 것이지
/// map 엔트리 **안**의 값을 생략하라는 것이 아니다. map 엔트리는
/// `key=1, value=2` 를 가진 중첩 메시지이고, `{"k": ""}` 는
/// "키 k 가 존재하고 값이 빈 문자열" 이라는 **다른 정보**다.
///
/// 그러나 **규범이 이것을 명시하지 않는다.** 다른 구현자가 다르게 읽을 수 있다.
/// 이 테스트가 현재 동작을 고정하고, 규범에 명시하도록 만든다.
#[test]
fn c3_map_entry_with_empty_value_is_preserved() {
    use std::collections::BTreeMap;

    let mut with_empty = Fields::new();
    let mut m1 = BTreeMap::new();
    m1.insert("k".to_string(), String::new());
    with_empty.set(15, Value::MapStrStr(m1));

    let mut absent = Fields::new();
    absent.set(15, Value::MapStrStr(BTreeMap::new()));

    let a = canonical_encode(&with_empty, &[]);
    let b = canonical_encode(&absent, &[]);

    println!("C-3  {{\"k\": \"\"}} -> {} ({}B)", hex(&a), a.len());
    println!("C-3  {{}}          -> {} ({}B)", hex(&b), b.len());

    assert_ne!(
        a, b,
        "빈 값을 가진 map 엔트리가 사라졌다 — 키의 존재 자체가 정보인데 소실된다"
    );

    // 값이 빈 문자열이어도 엔트리 안의 field 2 는 출력되지 않는다(길이 0).
    // 즉 `{"k": ""}` 와 `{"k": "x"}` 는 다르고, `{}` 와도 다르다.
    let mut m2 = BTreeMap::new();
    m2.insert("k".to_string(), "x".to_string());
    let mut with_value = Fields::new();
    with_value.set(15, Value::MapStrStr(m2));
    assert_ne!(a, canonical_encode(&with_value, &[]));
}

// ══════════════════════════════════════════════════════════════════
// 규칙 e — 인코더가 항상 최단을 내는가 (코덱스 지적 3)
// ══════════════════════════════════════════════════════════════════

/// 디코더는 non-minimal 을 거부한다. **인코더가 항상 최단을 내는가**는 별개다.
#[test]
fn encoder_always_emits_minimal_varint() {
    use gputeer_protocol::canonical::encode_varint;

    // 경계값들 — 각 varint 길이의 첫/마지막 값
    let cases: &[(u64, usize)] = &[
        (0, 1),
        (127, 1),
        (128, 2),
        (16_383, 2),
        (16_384, 3),
        (u32::MAX as u64, 5),
        (u64::MAX, 10),
    ];
    for (v, expect_len) in cases {
        let mut out = Vec::new();
        encode_varint(*v, &mut out);
        assert_eq!(out.len(), *expect_len, "{v} 의 varint 길이가 최단이 아니다");
        // 마지막 바이트가 0이면서 길이 > 1 이면 non-minimal
        assert!(
            out.len() == 1 || *out.last().unwrap() != 0,
            "{v} 가 non-minimal 로 인코딩됐다: {}",
            hex(&out)
        );
    }

    // 규칙 j — 음수 int64 는 2의 보수 u64 재해석이므로 항상 10바이트다.
    // 그것이 그 u64 값의 최단이므로 규칙 e 와 모순되지 않는다.
    for v in [-1i64, i64::MIN, -1_000_000] {
        let mut out = Vec::new();
        encode_varint(v as u64, &mut out);
        assert_eq!(out.len(), 10, "{v} 의 varint 가 10바이트가 아니다");
        assert_ne!(*out.last().unwrap(), 0, "{v} 가 non-minimal 이다");
    }
}
