//! gPUteer 프로토콜 — 스키마·canonical 인코딩·프로토콜 상수.
//!
//! 규범 문서:
//!   - `docs/protocol/signing.md`        서명 대상 · canonical 규칙
//!   - `docs/protocol/state-machines.md` 상태 전이
//!   - `proto/*.proto`                   와이어 스키마
//!
//! 이 크레이트는 Protocol 스트림이 소유한다 (`docs/contracts/01_스트림_소유권.md`).

pub mod canonical;
pub mod constants;
pub mod to_fields;

/// proto/*.proto 에서 생성된 타입.
///
/// ★ 생성 코드를 직접 수정하지 않는다 (RULE.md §4.3).
/// ★ 서명에는 prost 인코더를 쓰지 않는다. `to_fields` + `canonical` 을 쓴다.
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/gputeer.v1.rs"));
}

pub use to_fields::{ToCanonicalFields, UNIMPLEMENTED_FIELDS};
pub use canonical::{
    blake3_256, canonical_encode, merkle_root, sig_input, CanonicalError, Domain, Fields, Value,
    SIGNATURE_FIELD,
};
