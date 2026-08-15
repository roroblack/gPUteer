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

pub use canonical::{
    blake3_256, canonical_encode, merkle_root, sig_input, CanonicalError, Domain, Fields, Value,
    SIGNATURE_FIELD,
};
