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
use crate::constants::{GRANT_TTL_MS, NEIGHBOR_REPORT_TTL_MS};
use crate::pb;
use crate::signing::{signing_input, DerivedMismatch, Lifetime, Signable};
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
    /// §10 — 서명된 nonce 필드(24). 호출자가 고를 수 없다.
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.nonce)
    }

    /// ★ §6.1 — `manifest_hash` 를 **재계산해 대조한다(MUST).**
    ///
    /// 이 필드는 규칙 i 로 canonical 에서 제외되므로 **Grant 서명이 보증하지 않는다.**
    /// 규범은 "Agent 는 이 값을 신뢰하지 않고 반드시 재계산해 대조한다" 고 적었는데
    /// **그 코드가 어디에도 없었다** (독립 검수 2026-08-16).
    ///
    /// ```text
    /// manifest_hash = BLAKE3_256( sig_input_of(JobManifest) )
    /// ```
    ///
    /// # 판정 규칙
    ///
    /// ```text
    /// manifest 있음 + hash 있음   -> 대조. 다르면 거부
    /// manifest 있음 + hash 없음   -> 통과 (주장을 안 했으므로)
    /// manifest 없음 + hash 있음   -> 거부 (없는 것의 해시를 주장한다)
    /// 둘 다 없음                  -> 통과
    /// ```
    fn check_derived_consistency(&self) -> Result<(), DerivedMismatch> {
        const FIELD: &str = "ExecutionGrant.manifest_hash";
        match (&self.manifest, &self.manifest_hash) {
            (Some(m), Some(h)) => {
                let expected = crate::canonical::blake3_256(&signing_input(m));
                if h.value.as_slice() == expected.as_slice() {
                    Ok(())
                } else {
                    Err(DerivedMismatch {
                        field: FIELD,
                        detail: format!("재계산 {} != 기재 {}", hex32(&expected), hex32(&h.value),),
                    })
                }
            }
            (None, Some(_)) => Err(DerivedMismatch {
                field: FIELD,
                detail: "manifest 가 없는데 manifest_hash 만 있다".into(),
            }),
            _ => Ok(()),
        }
    }
}

/// Coordinator 가 발급한 `ExecutionGrant` 에 대한 Agent 의 서명된 응답
/// (docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md).
///
/// `ReplicaAck` 를 재사용하지 않는다 — `ReplicaAck` 는
/// `Lifetime::Evidence` 라 replay nonce 를 검사하지 않으므로
/// (§262-264 `impl Signable for pb::ReplicaAck` 참조), 빌려 쓰면
/// "ACK replay 를 `DurableReplayGuard` 가 거부하는가" 를 증명할 수 없다.
impl Signable for pb::AgentGrantAck {
    const DOMAIN: Domain = Domain::GrantAck;
    /// Grant 와 대칭 — 정상 handshake 창 안에서만 유효한 응답이다.
    const LIFETIME: Lifetime = Lifetime::ShortLived;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.agent_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.agent_device_id
    }
    /// 서명된 nonce 필드(7). 호출자가 고를 수 없다.
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.nonce)
    }
}

fn hex32(b: &[u8]) -> String {
    b.iter()
        .take(8)
        .map(|x| format!("{x:02x}"))
        .collect::<String>()
        + ".."
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
    /// §10 — 서명된 nonce 필드(21). 호출자가 고를 수 없다.
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.nonce)
    }
}

/// ★ 2026-08-19 — Lease 갱신 최소 조각
/// (docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md).
///
/// 원래 `RenewLeaseResult` 는 서명 대상이 아니었다 — Coordinator 가
/// 낸 `SUPERSEDED`/`QUARANTINED` 같은 정책 거부를 아무나 위조해
/// 정당한 Agent 의 작업을 강제 중단시킬 수 있는 상태였다.
impl Signable for pb::RenewLeaseResult {
    const DOMAIN: Domain = Domain::LeaseRenewResult;
    /// §9 — `RenewLeaseRequest` 와 같은 이유로 단수명이다. 결과는
    /// 발급 즉시 소비되고 오래 보관될 이유가 없다.
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

    /// `RenewLeaseRequest` 와 같은 이유로 `expires_at` 필드가 없다 —
    /// `issued_at + GRANT_TTL_MS` 로 도출한다.
    fn expires_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.coordinator_id
    }
    /// §10 — 요청의 `nonce`(필드 21)를 그대로 echo 한 필드(8)를
    /// 그대로 replay nonce 로 쓴다. `RenewLeaseRequest` 는 Agent 가
    /// 서명해 `signer_id = node_id` 이고 이 결과는 Coordinator 가
    /// 서명해 `signer_id = coordinator_id` 다 — signer 가 다르므로
    /// replay guard 네임스페이스가 자동으로 분리된다
    /// (`nonce_namespace_is_per_device`). 같은 값을 재사용해도
    /// 요청 쪽 replay 기록과 충돌하지 않는다.
    ///
    /// 오래된(유효했던) 결과를 나중에 재전송해 Agent 의 상태를
    /// 되돌리려는 시도를 이 replay 검사가 막는다 — 예: 이미 처리한
    /// `RENEWED` 결과를 다시 보내 낮은 epoch 의 Lease 로 되돌리려는
    /// 시도.
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.request_nonce)
    }
}

impl Signable for pb::AgentSessionHello {
    const DOMAIN: Domain = Domain::SessionHello;
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
    fn expires_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.node_id
    }
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.nonce)
    }
}

impl Signable for pb::ResumeLeaseRequest {
    const DOMAIN: Domain = Domain::LeaseResume;
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
    fn expires_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.node_id
    }
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.request_nonce)
    }
}

impl Signable for pb::ResumeLeaseResult {
    const DOMAIN: Domain = Domain::LeaseResumeResult;
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
        self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.coordinator_id
    }
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.request_nonce)
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

/// B+E 계약 단계 1 — Coordinator 가 서명하는 "받았다" 응답. `RenewLeaseResult` 와 같은 모양이다 — 요청 쪽(REPORT 세션
/// Hello)의 nonce 를 echo 한 `session_nonce` 를 replay nonce 로 쓰고, 발급 즉시 소비되므로 단수명이다.
impl Signable for pb::AttemptReportAck {
    const DOMAIN: Domain = Domain::AttemptReportAck;
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
        self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.coordinator_id
    }
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.session_nonce)
    }
}

/// ★ 2026-09-23 (결함 131) — Coordinator 가 서명하는 ACK 수신 확인. 발급 즉시 소비되는 단수명이다.
impl Signable for pb::GrantAckReceipt {
    const DOMAIN: Domain = Domain::GrantAckReceipt;
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
        self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.coordinator_device_id
    }
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.nonce)
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

/// 이웃 신고 — `ADR-033` §7 의 관측 층.
///
/// ★ `Lifetime::ShortLived` 다 — **오래된 관측이 지금 관측으로 재사용되는
///   것**을 막기 위해서다. 어제의 "연락이 안 된다" 를 오늘 다시 보내면
///   지금 멀쩡한 노드가 연락 두절로 보인다.
///
///   ★ 초안 주석은 "재생하면 이웃 하나가 `ADR-033` §8 조건 3 의 정족수를
///     혼자 채울 수 있다" 고 썼는데 **틀렸다**(2026-08-30 독립 검수 지적).
///     `crates/scheduler/src/reassignment.rs` 는 `reporter_node_id` 를
///     집합으로 중복 제거하므로 같은 신고를 N 번 넣어도 한 표다. replay
///     방어가 막는 것은 정족수 조작이 아니라 **신선도 위조와 중복
///     부작용**이다.
///
/// ★ 서명자는 **신고자의 장치**다. "이 장치가 이 관측을 보냈다" 를
///   증명할 뿐이다. 증명하지 **않는** 것 —
///
///   - 그 관측이 사실인지
///   - 그 장치가 정당한 풀 이웃인지 (멤버십 해소의 몫, 아직 없다)
///   - 그 장치가 주장한 `reporter_node_id` 의 실제 장치인지
///     ★ 유효한 장치 키 하나가 서로 다른 node ID N 개를 서명할 수 있다
///       (같은 검수 지적). 정족수를 세기 전에 소비자가 authoritative
///       device→node 결합을 해소해야 한다 —
///       `crates/scheduler/src/reassignment.rs` 가 그것을 호출부 진술로
///       요구하는 이유이고, wire→커널 어댑터는 아직 없다.
impl Signable for pb::NeighborUnreachableReport {
    const DOMAIN: Domain = Domain::NeighborUnreachableReport;
    const LIFETIME: Lifetime = Lifetime::ShortLived;
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn to_canonical_fields(&self) -> Fields {
        <Self as ToCanonicalFields>::to_canonical_fields(self)
    }
    fn signature_bytes(&self) -> &[u8] {
        &self.reporter_signature
    }
    fn expires_at_unix_ms(&self) -> u64 {
        // ★ `GRANT_TTL_MS` 를 쓰지 않는다 — 그건 "ExecutionGrant 기본
        //   수명"(기준선 §15.4)이고 신고에 적용할 규범적 근거가 없다
        //   (2026-08-30 독립 검수 지적). 전용 상수를 쓴다.
        self.observed_at_unix_ms
            .saturating_add(NEIGHBOR_REPORT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.observed_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.reporter_device_id
    }
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.request_nonce)
    }
}

/// 노드 생존 보고.
///
/// ★ `Lifetime::ShortLived` 다 — replay nonce 를 반드시 검사한다.
///   heartbeat 를 재생할 수 있으면 이미 죽은 노드를 살아 있는 것처럼
///   보이게 만들 수 있고, 그러면 ADR-033 §7 의 판정이 통째로 무의미해진다.
///
/// ★ 이 주석은 2026-08-30 이웃 신고를 그 **위에** 끼워 넣으면서 잠시
///   남의 impl 에 붙어 있었다(독립 검수 2라운드가 짚었다). 문서 주석은
///   바로 아래 항목에 붙는다 — 새 impl 을 위에 넣을 때 딸려 올라간다.
impl Signable for pb::NodeHeartbeat {
    const DOMAIN: Domain = Domain::NodeHeartbeat;
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
    fn expires_at_unix_ms(&self) -> u64 {
        // heartbeat 는 짧게 산다. 오래 유효하면 옛 보고가 지금 상태로
        // 오인될 수 있다.
        //
        // ★ 60_000 을 직접 쓰지 않는다(2026-08-29 독립 검수 지적) —
        //   다른 ShortLived 메시지는 전부 `GRANT_TTL_MS` 를 쓴다. 직접
        //   쓰면 그 상수가 바뀌었을 때 heartbeat 만 조용히 갈라진다
        //   (`RULE.md` 의 프로토콜 상수 단일 출처 규칙).
        self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
    }
    fn issued_at_unix_ms(&self) -> u64 {
        self.issued_at_unix_ms
    }
    fn signer_id(&self) -> &str {
        &self.device_id
    }
    fn replay_nonce(&self) -> Option<&[u8]> {
        Some(&self.request_nonce)
    }
}
