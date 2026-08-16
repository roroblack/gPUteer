//! `Signable` 구현 — 각 메시지의 **domain_tag 와 시각 정책**.
//!
//! # 왜 `to_fields.rs` 와 분리하는가
//!
//! ```text
//! to_fields.rs   어떤 **필드**가 서명에 들어가는가   (signing.md §3)
//! signable.rs    어떤 **domain·수명**이 붙는가        (signing.md §5 · §9)
//! ```
//!
//! 둘 다 손으로 쓴다(§13.1). 그러나 틀렸을 때의 증상이 다르다.
//!
//! ```text
//! 필드가 틀리면    다른 구현체와 붙을 때 서명이 안 맞는다
//! domain 이 틀리면 다른 문맥의 서명이 통과한다              (ADR-028)
//! 수명이 틀리면    정상 메시지가 전부 거부되거나
//!                  만료된 것이 영원히 유효해진다             (§9 경고)
//! ```
//!
//! 섞어 두면 리뷰할 때 어느 쪽을 보는지 흐려진다.
//!
//! # 시각 정책은 추측하지 않는다
//!
//! `signing.md` §9 표에 없는 메시지는 **ADR 로 결정한 뒤** 여기에 적는다.
//! `Lifetime` 을 잘못 고르면 큐를 통과한 정상 Job 이 100% 거부되거나(§9),
//! 만료된 증거가 영원히 유효해진다.
//!
//! - `Lifetime::Evidence` 6종 → **ADR-029**
//! - domain_tag 분리 → **ADR-028**

use crate::canonical::{Domain, Fields};
use crate::constants::GRANT_TTL_MS;
use crate::pb;
use crate::signing::{Lifetime, Signable};
use crate::ToCanonicalFields;

// ══════════════════════════════════════════════════════════════════
// 장수명 (§9 표) — JobManifest · Lease
// ══════════════════════════════════════════════════════════════════

impl Signable for pb::JobManifest {
    const DOMAIN: Domain = Domain::Manifest;
    /// §9 — Job 은 큐에서 수 시간 대기하는 것이 **정상 동작**이다
    /// (계획서 §13.6 aging queue). skew 규칙을 걸면 정상 Job 이 전부 거부된다.
    const LIFETIME: Lifetime = Lifetime::LongLived;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.submitter_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.submitter_device_id
    }
}

impl Signable for pb::Lease {
    const DOMAIN: Domain = Domain::Lease;
    /// §9 — skew 미적용, `expires_at` 만 검사. 기본 TTL 10분.
    const LIFETIME: Lifetime = Lifetime::LongLived;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.coordinator_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.issuing_coordinator_id
    }
}

// ══════════════════════════════════════════════════════════════════
// 단수명 (§9 표) — ExecutionGrant · RenewLeaseRequest
//
// ★ §9 의 ShortLived 경로가 **실메시지로 처음 검증되는 지점**이다.
//   DoD-04 에서는 테스트 전용 타입으로만 확인했다.
// ══════════════════════════════════════════════════════════════════

impl Signable for pb::ExecutionGrant {
    const DOMAIN: Domain = Domain::Grant;
    /// §9 — skew 적용 · `expires_at` 검사 · 기본 TTL 60초.
    const LIFETIME: Lifetime = Lifetime::ShortLived;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.coordinator_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.coordinator_device_id
    }
}

impl Signable for pb::RenewLeaseRequest {
    const DOMAIN: Domain = Domain::LeaseRenew;
    /// §9 — skew 적용 · `expires_at` 검사 · 기본 TTL 60초.
    const LIFETIME: Lifetime = Lifetime::ShortLived;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.node_signature
    }

    /// ★ `RenewLeaseRequest` 에는 `expires_at` 필드가 **없다.**
    ///
    /// §9 표는 단수명으로 분류하고 TTL 60초를 정하는데 필드가 없다.
    /// `issued_at + GRANT_TTL_MS` 로 **도출**한다.
    ///
    /// 이것은 값을 지어내는 것이 아니라(`CLAUDE.md` §1),
    /// **규범이 정한 TTL 을 적용**하는 것이다. 근거를 여기 적어 둔다.
    /// 필드를 추가하면 `schema_version` 상향이 필요하다(§7.3).
    fn expires_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.node_id
    }
}

// ══════════════════════════════════════════════════════════════════
// 증거 (ADR-029) — 6종
//
// ★ "권한" 이 아니라 "증거" 다. 시각으로 만료시키지 않는다.
//   `observed_at_unix_ms()` 로 **언제의 사실인가** 를 노출하고,
//   소비 측이 `fence_epoch` 와 함께 신선도를 판단한다.
//
// 매크로를 쓰지 않는다 — `signer_id` 와 `observed_at` 의 필드명이 메시지마다
// 다르고, **어떤 시각 정책이 붙는지 사람이 눈으로 확인할 수 있어야** 한다(§13.1).
//
// `expires_at_unix_ms()` 가 0 을 반환하는 것은 "만료 개념이 없다" 는 뜻이며,
// `Lifetime::Evidence` 에서는 `verify()` 가 이 값을 아예 읽지 않는다.
// ══════════════════════════════════════════════════════════════════

impl Signable for pb::CheckpointManifest {
    const DOMAIN: Domain = Domain::Checkpoint;
    const LIFETIME: Lifetime = Lifetime::Evidence;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.producer_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        0
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }
    fn observed_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.producer_node_id
    }
}

/// ★ 6종 중 **유일하게 `fence_epoch` 이 없다** (ADR-029 · `TODO_VISION` V-07).
///
/// `ReplicaAck` 는 `REPLICATED(n)` 을 세는 근거이므로 **durability 주장의 뿌리**인데,
/// 신선도 판단 근거가 `acked_at_unix_ms` 하나뿐이다.
///
/// **복제본이 삭제되어도 이 ACK 는 영원히 유효하다.**
/// 소비 측은 이것을 "지금 durable 하다" 가 아니라
/// "`acked_at` 시점에 durable 했다" 로만 읽어야 한다 (`CLAUDE.md` §0.3).
impl Signable for pb::ReplicaAck {
    const DOMAIN: Domain = Domain::ReplicaAck;
    const LIFETIME: Lifetime = Lifetime::Evidence;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.holder_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        0
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.acked_at_unix_ms
    }
    fn observed_at_unix_ms(&self) -> u64 {
        self.acked_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.holder_device_id
    }
}

impl Signable for pb::ArtifactRef {
    const DOMAIN: Domain = Domain::Artifact;
    const LIFETIME: Lifetime = Lifetime::Evidence;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.producer_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        0
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }
    fn observed_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }
    /// ★ 생산자 ID 필드가 **없다** — `attempt_id` 로 대신한다.
    ///   검증자가 attempt → node 매핑을 알아야 키를 찾을 수 있다.
    ///   `producer_node_id` 를 추가하는 것이 옳으나 `schema_version` 상향이
    ///   필요하다 (`TODO_VISION` V-08).
    fn signer_id(&self) -> &str {
        &self.attempt_id
    }
}

impl Signable for pb::AttemptReport {
    const DOMAIN: Domain = Domain::AttemptReport;
    const LIFETIME: Lifetime = Lifetime::Evidence;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.node_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        0
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn observed_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.node_id
    }
}

impl Signable for pb::CanonicalDecision {
    const DOMAIN: Domain = Domain::Canonical;
    const LIFETIME: Lifetime = Lifetime::Evidence;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.coordinator_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        0
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.decided_at_unix_ms
    }
    fn observed_at_unix_ms(&self) -> u64 {
        self.decided_at_unix_ms
    }
    /// ★ 서명자 ID 필드가 **없다** — 어느 Coordinator 가 결정했는지 메시지에 없다.
    ///   `job_id` 로 대신한다 (`TODO_VISION` V-08).
    fn signer_id(&self) -> &str {
        &self.job_id
    }
}

/// ★ 회수 통지는 **명령**이지만 `expires_at` 필드가 없다.
///
/// stale 한 회수는 `fence_epoch(3)` 이 거부한다 — 시각이 아니라 epoch 이 기준이다.
/// 단 **같은 `fence_epoch` 의 회수 통지를 반복 전송하는 것**은 막지 못한다.
/// 회수는 멱등하므로 무해하나, 그 판단을 여기 적어 둔다 (ADR-029).
impl Signable for pb::RevokeLeaseNotice {
    const DOMAIN: Domain = Domain::LeaseRevoke;
    const LIFETIME: Lifetime = Lifetime::Evidence;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.coordinator_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        0
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn observed_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    /// ★ 발급자 ID 필드가 **없다** — `lease_id` 로 대신한다 (`TODO_VISION` V-08).
    fn signer_id(&self) -> &str {
        &self.lease_id
    }
}
