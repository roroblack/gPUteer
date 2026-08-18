//! Canonical 인코딩 — `docs/protocol/signing.md` §3 의 구현체.
//!
//! **`prost::Message::encode()` 를 서명에 쓰지 않는다.** proto3 는 deterministic
//! serialization 을 보장하지 않으므로 서명 검증이 랜덤하게 실패한다.
//!
//! 규범 규칙 (signing.md §3.1):
//!   a. 필드는 field number 오름차순
//!   b. 기본값 필드는 출력하지 않는다
//!   c. map 은 key 바이트 오름차순 정렬
//!   d. repeated 는 선언 순서 유지 (정렬하지 않는다)
//!   e. varint 는 최단 인코딩만
//!   f. 중첩 메시지에 재귀 적용
//!   g. 부동소수점 금지 (ppm 정수 / 밀리초 정수)
//!   h. 알 수 없는 필드는 포함하지 않는다
//!   i. 서명 필드(90)와 도출 해시 필드는 제외
//!
//! 검증: `tests/vectors/canonical_v1.json` 의 벡터 12건과 대조한다.

use std::collections::BTreeMap;

/// 서명 필드 번호. 모든 서명 대상 메시지에서 고정이다 (signing.md §3.1-i).
pub const SIGNATURE_FIELD: u32 = 90;

const WIRETYPE_VARINT: u32 = 0;
const WIRETYPE_LEN: u32 = 2;

/// 최단 varint 인코딩 (규칙 e).
pub fn encode_varint(mut n: u64, out: &mut Vec<u8>) {
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// non-minimal varint 를 거부하는 디코더 (규칙 e).
///
/// 같은 값을 여러 바이트열로 표현할 수 있으면 서명 우회 여지가 생긴다.
pub fn decode_varint(buf: &[u8], pos: &mut usize) -> Result<u64, CanonicalError> {
    let start = *pos;
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(*pos).ok_or(CanonicalError::TruncatedVarint)?;
        *pos += 1;
        result |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return Err(CanonicalError::VarintTooLong);
        }
    }
    // 최단성 검사: 같은 값을 다시 인코딩해 바이트가 일치해야 한다
    let mut re = Vec::new();
    encode_varint(result, &mut re);
    if re.as_slice() != &buf[start..*pos] {
        return Err(CanonicalError::NonMinimalVarint);
    }
    Ok(result)
}

fn encode_tag(field: u32, wiretype: u32, out: &mut Vec<u8>) {
    encode_varint(((field as u64) << 3) | wiretype as u64, out);
}

fn encode_len_delimited(field: u32, payload: &[u8], out: &mut Vec<u8>) {
    encode_tag(field, WIRETYPE_LEN, out);
    encode_varint(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalError {
    #[error("varint 가 잘렸다")]
    TruncatedVarint,
    #[error("varint 가 64비트를 넘는다")]
    VarintTooLong,
    #[error("non-minimal varint 는 거부된다 (signing.md §3.1-e)")]
    NonMinimalVarint,
    #[error("float/double 은 canonical 인코딩에서 금지된다 (signing.md §3.1-g)")]
    FloatNotAllowed,
}

/// canonical 인코딩이 가능한 값.
///
/// **float/double 이 없다는 점이 핵심이다.** IEEE-754 는 `-0.0`·NaN 페이로드·
/// 비정규수 표현이 플랫폼마다 달라 canonical 인코딩을 깬다.
/// 비율은 ppm 정수, 시각은 밀리초 정수로 표현한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// uint32 / uint64 / enum
    Uint(u64),
    /// int32 / int64 (규칙 j).
    ///
    /// ★ 2의 보수 u64 로 재해석해 varint 인코딩한다. zigzag 가 **아니다.**
    /// 음수는 항상 정확히 10바이트다.
    ///
    /// `int32` 는 이 variant 에 담기 전에 **반드시 64비트로 부호 확장**한다
    /// (`v as i64`). protobuf 의 유명한 함정이라 `to_fields` 에서 실수하기 쉽다.
    Int(i64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    Message(Fields),
    /// 선언 순서를 유지한다 (규칙 d)
    RepeatedStr(Vec<String>),
    RepeatedMessage(Vec<Fields>),
    /// key 바이트 오름차순으로 정렬된다 (규칙 c).
    /// BTreeMap 을 쓰므로 삽입 순서와 무관하게 항상 같은 순서가 나온다.
    MapStrStr(BTreeMap<String, String>),
}

impl Value {
    /// 기본값 판정 (규칙 b).
    ///
    /// proto3 는 "설정된 0" 과 "미설정" 을 구분하지 않는다. 둘 다 생략해 일치시킨다.
    fn is_default(&self) -> bool {
        match self {
            Value::Uint(v) => *v == 0,
            Value::Int(v) => *v == 0,
            Value::Bool(b) => !*b,
            Value::Str(s) => s.is_empty(),
            Value::Bytes(b) => b.is_empty(),
            Value::Message(f) => f.0.is_empty(),
            Value::RepeatedStr(v) => v.is_empty(),
            Value::RepeatedMessage(v) => v.is_empty(),
            Value::MapStrStr(m) => m.is_empty(),
        }
    }
}

/// field number -> 값. BTreeMap 이므로 항상 오름차순으로 순회된다 (규칙 a).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields(BTreeMap<u32, Value>);

impl Fields {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// 필드를 설정한다. 삽입 순서는 인코딩에 영향을 주지 않는다.
    pub fn set(&mut self, field: u32, value: Value) -> &mut Self {
        self.0.insert(field, value);
        self
    }

    pub fn get(&self, field: u32) -> Option<&Value> {
        self.0.get(&field)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// signing.md §3 의 canonical_encode.
///
/// 서명 필드(90)와 `derived_hash_fields` 는 제외된다 (규칙 i).
///
/// # ★ `derived_hash_fields` 는 **최상위에만** 적용된다 (2026-08-16 시정)
///
/// 처음에는 이 목록을 재귀로 물려주었다. 독립 검수가 반례를 냈다.
///
/// ```text
/// ExecutionGrant.manifest_hash        = field 4   -> 제외해야 한다
/// ExecutionGrant.manifest.dataset.retention
///                     (DatasetRef 의) field 4     -> 제외하면 **안 된다**
///
/// canonical_encode(&fields, &[4]) 는 둘 다 지웠다.
/// => DatasetRef.retention(삭제 정책)이 서명에서 조용히 빠진다.
/// ```
///
/// field number 는 **메시지마다 의미가 다르다.** 번호만으로 재귀 제외하면
/// 우연히 같은 번호를 쓰는 중첩 필드가 함께 사라진다.
///
/// 도출 해시 필드는 **정의상 최상위 메시지의 것**이므로(§6.1 —
/// `ExecutionGrant.manifest_hash`), 최상위에만 적용하는 것이 옳다.
/// 중첩 메시지가 자기 도출 해시를 갖게 되면 그때 `(타입, 번호)` 쌍으로 확장한다.
pub fn canonical_encode(fields: &Fields, derived_hash_fields: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    // BTreeMap 순회 = field number 오름차순 (규칙 a)
    for (&num, value) in fields.0.iter() {
        // 규칙 i
        if num == SIGNATURE_FIELD || derived_hash_fields.contains(&num) {
            continue;
        }
        // 규칙 b
        if value.is_default() {
            continue;
        }
        // ★ 중첩에는 derived 목록을 물려주지 않는다 (위 설명).
        //   서명 필드(90)는 모든 메시지에서 같은 의미이므로 재귀 적용된다.
        encode_value(num, value, &[], &mut out);
    }
    out
}

fn encode_value(num: u32, value: &Value, derived: &[u32], out: &mut Vec<u8>) {
    match value {
        Value::Uint(v) => {
            encode_tag(num, WIRETYPE_VARINT, out);
            encode_varint(*v, out); // 규칙 e
        }
        Value::Int(v) => {
            // 규칙 j — 2의 보수 u64 재해석. proto3 int64 의 wire format 과 같다.
            encode_tag(num, WIRETYPE_VARINT, out);
            encode_varint(*v as u64, out);
        }
        Value::Bool(_) => {
            // is_default 로 false 는 이미 걸러졌으므로 여기서는 항상 true
            encode_tag(num, WIRETYPE_VARINT, out);
            encode_varint(1, out);
        }
        Value::Str(s) => encode_len_delimited(num, s.as_bytes(), out),
        Value::Bytes(b) => encode_len_delimited(num, b, out),
        Value::Message(f) => {
            // 규칙 f — 재귀
            let inner = canonical_encode(f, derived);
            // ★ 규칙 i-2 — 서명/도출해시를 제외한 결과가 비면 **필드 자체를 생략**한다.
            //
            //   이 검사가 없으면 `Message({90: sig})` 가 빈 중첩 메시지(`0a 00`)로
            //   출력되고, `Message({})` 는 생략되어 canonical 이 달라진다.
            //   즉 "서명 필드가 canonical 에 영향을 주지 않는다" 가 깨진다.
            //
            //   ★ 2026-08-16 독립 검수에서 발견. Rust 와 Python 참조 구현이
            //     **똑같이 틀리고 있어** 벡터 대조로는 잡히지 않았다.
            //     벡터 대조는 "두 구현이 같은가" 를 증명하지 "옳은가" 를 증명하지 않는다.
            if inner.is_empty() {
                return;
            }
            encode_len_delimited(num, &inner, out);
        }
        Value::RepeatedStr(items) => {
            // 규칙 d — 순서 유지, packed 미사용
            for item in items {
                encode_len_delimited(num, item.as_bytes(), out);
            }
        }
        Value::RepeatedMessage(items) => {
            // ★ 규칙 d — 원소를 버리지 않는다. 순서가 의미를 가지므로
            //   빈 원소도 길이 0으로 자리를 지킨다.
            //   (규칙 i-2 는 **단일** 중첩 메시지에만 적용된다)
            for item in items {
                let inner = canonical_encode(item, derived);
                encode_len_delimited(num, &inner, out);
            }
        }
        Value::MapStrStr(map) => {
            // 규칙 c — BTreeMap 이므로 이미 key 오름차순
            for (k, v) in map.iter() {
                let mut entry = Vec::new();
                // 키의 존재 자체가 정보이므로 field 1 은 항상 출력한다.
                encode_len_delimited(1, k.as_bytes(), &mut entry);
                // ★ 규칙 c-2 — 엔트리 안에서도 규칙 b 를 적용한다.
                //   proto3 map 시맨틱에서 "값 없음" 과 "빈 값" 은 같다.
                if !v.is_empty() {
                    encode_len_delimited(2, v.as_bytes(), &mut entry);
                }
                encode_len_delimited(num, &entry, out);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// sig_input — signing.md §4
// ═══════════════════════════════════════════════════════════════════════

pub const DOMAIN_TAG_LEN: usize = 32;

/// 서명 용도를 고정하는 도메인 태그 (signing.md §5).
///
/// 한 문맥의 서명을 다른 문맥에서 검증하면 반드시 실패한다.
/// 새 서명 대상 메시지를 추가하면 여기에 등록해야 한다(MUST).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Manifest,
    Grant,
    Lease,
    LeaseRenew,
    LeaseRevoke,
    Checkpoint,
    ReplicaAck,
    Artifact,
    AttemptReport,
    Canonical,
    Genesis,
    // ★ ADR-028 (2026-08-16) — membership · policy · quarantine 3종을 9종으로 분리했다.
    //   공유하면 서명 재사용이 가능하다: RemoveMember{id} 와 RevokeDevice{id} 의
    //   canonical 이 28바이트로 동일해, 탈퇴 서명이 기기 폐기로 재사용된다.
    MemberAdd,
    MemberRemove,
    DeviceApprove,
    DeviceRevoke,
    CoordinatorSet,
    OwnerKeyRotate,
    PolicyUpdate,
    QuarantineDevice,
    QuarantineRelease,
    Audit,
    Release,
    Invite,
    // ★ 2026-08-18 — coordinator/agent 최소 핸드셰이크
    //   (docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md).
    //   ReplicaAck 의 domain 을 공유하지 않는다 — ReplicaAck 는
    //   Lifetime::Evidence 라 replay nonce 를 검사하지 않으므로,
    //   공유하면 AgentGrantAck 의 replay 방어를 증명할 수 없다.
    GrantAck,
}

impl Domain {
    pub fn as_str(self) -> &'static str {
        match self {
            Domain::Manifest => "gputeer/v1/manifest",
            Domain::Grant => "gputeer/v1/grant",
            Domain::Lease => "gputeer/v1/lease",
            Domain::LeaseRenew => "gputeer/v1/lease-renew",
            Domain::LeaseRevoke => "gputeer/v1/lease-revoke",
            Domain::Checkpoint => "gputeer/v1/checkpoint",
            Domain::ReplicaAck => "gputeer/v1/replica-ack",
            Domain::Artifact => "gputeer/v1/artifact",
            Domain::AttemptReport => "gputeer/v1/attempt-report",
            Domain::Canonical => "gputeer/v1/canonical",
            Domain::Genesis => "gputeer/v1/genesis",
            Domain::MemberAdd => "gputeer/v1/member-add",
            Domain::MemberRemove => "gputeer/v1/member-remove",
            Domain::DeviceApprove => "gputeer/v1/device-approve",
            Domain::DeviceRevoke => "gputeer/v1/device-revoke",
            Domain::CoordinatorSet => "gputeer/v1/coordinator-set",
            Domain::OwnerKeyRotate => "gputeer/v1/owner-key-rotate",
            Domain::PolicyUpdate => "gputeer/v1/policy-update",
            Domain::QuarantineDevice => "gputeer/v1/quarantine-device",
            Domain::QuarantineRelease => "gputeer/v1/quarantine-release",
            Domain::Audit => "gputeer/v1/audit",
            Domain::Release => "gputeer/v1/release",
            Domain::Invite => "gputeer/v1/invite",
            Domain::GrantAck => "gputeer/v1/grant-ack",
        }
    }

    /// 32바이트, 우측 0x00 패딩.
    pub fn tag_bytes(self) -> [u8; DOMAIN_TAG_LEN] {
        let s = self.as_str().as_bytes();
        debug_assert!(s.len() <= DOMAIN_TAG_LEN, "domain tag 가 32바이트를 넘는다");
        let mut buf = [0u8; DOMAIN_TAG_LEN];
        buf[..s.len()].copy_from_slice(s);
        buf
    }
}

/// signing.md §4
///
/// ```text
/// sig_input = domain_tag(32) || uint32_be(schema_version) || uint32_be(len) || canonical
/// ```
///
/// `schema_version` 을 길이 앞에 넣는 이유: 버전이 다르면 같은 canonical 이라도
/// 다른 서명이 된다. 구버전 필드 집합으로 만든 서명이 신버전으로 통과하는 것을 막는다.
///
/// `len` 을 넣는 이유: 길이 확장 공격과 연접 모호성 차단.
pub fn sig_input(domain: Domain, schema_version: u32, canonical: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(DOMAIN_TAG_LEN + 8 + canonical.len());
    out.extend_from_slice(&domain.tag_bytes());
    out.extend_from_slice(&schema_version.to_be_bytes());
    out.extend_from_slice(&(canonical.len() as u32).to_be_bytes());
    out.extend_from_slice(canonical);
    out
}

/// signing.md §6.1 — manifest_hash 는 메시지 안에 저장하지 않고 여기서 도출한다.
pub fn blake3_256(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

// ═══════════════════════════════════════════════════════════════════════
// Merkle — signing.md §6.3
// ═══════════════════════════════════════════════════════════════════════

/// leaf/inner 도메인을 분리해 두 번째 원상 공격을 막는다.
/// 홀수 노드는 **승격**한다 (복제하지 않는다).
pub fn merkle_root(chunks: &[&[u8]]) -> Option<[u8; 32]> {
    if chunks.is_empty() {
        return None;
    }
    let mut level: Vec<[u8; 32]> = chunks
        .iter()
        .map(|c| {
            let mut h = blake3::Hasher::new();
            h.update(&[0x00]);
            h.update(c);
            *h.finalize().as_bytes()
        })
        .collect();

    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i + 1 < level.len() {
            let mut h = blake3::Hasher::new();
            h.update(&[0x01]);
            h.update(&level[i]);
            h.update(&level[i + 1]);
            next.push(*h.finalize().as_bytes());
            i += 2;
        }
        if level.len() % 2 == 1 {
            next.push(level[level.len() - 1]); // 승격
        }
        level = next;
    }
    Some(level[0])
}
