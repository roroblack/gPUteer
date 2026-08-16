//! prost 메시지 → canonical `Fields` 변환.
//!
//! **이 계층이 DoD-01 의 최대 공백이었다.**
//! canonical 규칙은 검증됐지만, 실제 protobuf 메시지에서 `Fields` 를 만드는
//! 과정에서 규칙이 깨질 수 있고 그것은 미검증이었다.
//!
//! # 왜 자동 생성이 아니라 수동 구현인가
//!
//! `signing.md` §13.1 이 정한 방침이다. 서명 대상 메시지는 17개뿐이고,
//! **어떤 필드가 서명에 들어가는지를 사람이 눈으로 확인할 수 있어야** 한다.
//! 자동 파생은 새 필드가 조용히 서명 대상에 들어가거나 빠지는 것을 숨긴다.
//!
//! # 규칙
//!
//! - field number 는 `.proto` 와 **정확히** 일치해야 한다 (테스트가 검증)
//! - 서명 필드(90)는 넣지 않는다
//! - 도출 해시 필드(`manifest_hash`)는 넣지 않는다
//! - 기본값은 `Fields` 에 넣어도 `canonical_encode` 가 생략하지만,
//!   **넣지 않는 것을 권장한다** (의도가 드러난다)

use std::collections::BTreeMap;

use crate::canonical::{Fields, Value};
use crate::pb;

/// canonical 서명 대상이 될 수 있는 메시지.
pub trait ToCanonicalFields {
    /// 이 메시지의 서명 대상 필드 집합.
    fn to_canonical_fields(&self) -> Fields;

    /// `schema_version`. sig_input 에 들어간다 (signing.md §4).
    fn schema_version(&self) -> u32;
}

// ── 헬퍼 ──────────────────────────────────────────────────────────

fn put_uint(f: &mut Fields, n: u32, v: u64) {
    if v != 0 {
        f.set(n, Value::Uint(v));
    }
}

fn put_bool(f: &mut Fields, n: u32, v: bool) {
    if v {
        f.set(n, Value::Bool(true));
    }
}

fn put_str(f: &mut Fields, n: u32, v: &str) {
    if !v.is_empty() {
        f.set(n, Value::Str(v.to_string()));
    }
}

fn put_bytes(f: &mut Fields, n: u32, v: &[u8]) {
    if !v.is_empty() {
        f.set(n, Value::Bytes(v.to_vec()));
    }
}

fn put_repeated_str(f: &mut Fields, n: u32, v: &[String]) {
    if !v.is_empty() {
        f.set(n, Value::RepeatedStr(v.to_vec()));
    }
}

fn put_map(f: &mut Fields, n: u32, m: &BTreeMap<String, String>) {
    if !m.is_empty() {
        f.set(n, Value::MapStrStr(m.clone()));
    }
}

/// prost 의 `HashMap` 을 `BTreeMap` 으로 옮긴다.
///
/// ★ 이 변환이 signing.md 규칙 c(map key 정렬)를 보장하는 지점이다.
/// `HashMap` 순회 순서는 실행마다 다르므로 반드시 `BTreeMap` 으로 정규화해야 한다.
fn norm_map(m: &std::collections::HashMap<String, String>) -> BTreeMap<String, String> {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn put_msg<T: ToCanonicalFields>(f: &mut Fields, n: u32, v: &Option<T>) {
    if let Some(inner) = v {
        let sub = inner.to_canonical_fields();
        if !sub.is_empty() {
            f.set(n, Value::Message(sub));
        }
    }
}

/// repeated message. **정렬하지 않는다** (규칙 d — 선언 순서가 의미를 갖는다).
fn put_repeated_msg<T: ToCanonicalFields>(f: &mut Fields, n: u32, v: &[T]) {
    if !v.is_empty() {
        f.set(
            n,
            Value::RepeatedMessage(v.iter().map(|x| x.to_canonical_fields()).collect()),
        );
    }
}

// ══════════════════════════════════════════════════════════════════
// common.proto
// ══════════════════════════════════════════════════════════════════

impl ToCanonicalFields for pb::Digest {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_uint(&mut f, 1, self.algo as u64);
        put_bytes(&mut f, 2, &self.value);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::CudaRequirement {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_uint(&mut f, 1, self.min_driver_version as u64);
        put_str(&mut f, 2, &self.cuda_runtime_version);
        put_repeated_str(&mut f, 3, &self.compute_capabilities);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::GpuRequest {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_uint(&mut f, 1, self.min_vram_bytes);
        put_uint(&mut f, 2, self.min_count as u64);
        put_msg(&mut f, 3, &self.cuda);
        put_uint(&mut f, 4, self.allocation_mode as u64);
        put_repeated_str(&mut f, 5, &self.allowed_gpu_models);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::ResourceRequest {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_msg(&mut f, 1, &self.gpu);
        put_uint(&mut f, 2, self.cpu_cores as u64);
        put_uint(&mut f, 3, self.ram_bytes);
        put_uint(&mut f, 4, self.workspace_bytes);
        put_uint(&mut f, 5, self.max_egress_bps);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::WorkloadHint {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_uint(&mut f, 1, self.class as u64);
        put_uint(&mut f, 2, self.estimated_steps);
        put_uint(&mut f, 3, self.ref_step_time_ms as u64);
        put_str(&mut f, 4, &self.ref_gpu_model);
        put_uint(&mut f, 10, self.model_params);
        put_uint(&mut f, 11, self.est_checkpoint_bytes);
        put_uint(&mut f, 12, self.est_peak_vram_bytes);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

// ══════════════════════════════════════════════════════════════════
// common.proto — 실행 환경 · 데이터셋 · 보안 범위
//
// ★ 2026-08-16 추가. DoD-02 가 "서명 대상에서 빠진 필드 6건, 그 중 3건이
//   보안 필드" 를 찾았고, 그 6건을 서명 안으로 넣기 위한 구현이다.
//   빠진 채로 두면 중간자가 네트워크 정책 · 산출물 범위 · 자원 범위를
//   고쳐도 서명 검증이 통과한다.
// ══════════════════════════════════════════════════════════════════

impl ToCanonicalFields for pb::TarballPolicy {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_bool(&mut f, 1, self.reject_path_traversal);
        put_bool(&mut f, 2, self.reject_links);
        put_uint(&mut f, 3, self.max_extracted_bytes);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::ExecutionEnvironment {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_uint(&mut f, 1, self.kind as u64);

        // kind == OCI_IMAGE
        put_str(&mut f, 10, &self.image_ref);
        put_msg(&mut f, 11, &self.image_digest);
        put_msg(&mut f, 12, &self.oci_source_digest);

        // kind == PYTHON_LOCK
        put_str(&mut f, 20, &self.base_runtime);
        put_msg(&mut f, 21, &self.lock_digest);
        put_bytes(&mut f, 22, &self.lock_content);
        put_msg(&mut f, 23, &self.lock_cas_ref);

        // 공통
        put_str(&mut f, 30, &self.os);
        put_str(&mut f, 31, &self.arch);
        put_str(&mut f, 32, &self.min_libc_version);
        put_msg(&mut f, 33, &self.cuda);
        put_msg(&mut f, 34, &self.code_digest);
        put_msg(&mut f, 35, &self.tarball_policy);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::DatasetRef {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_msg(&mut f, 1, &self.root_digest);
        put_uint(&mut f, 2, self.total_bytes);
        put_uint(&mut f, 3, self.sensitivity as u64);
        put_uint(&mut f, 4, self.retention as u64);
        put_bool(&mut f, 5, self.encrypted_at_rest);
        put_str(&mut f, 6, &self.display_name);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::NetworkPolicy {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_repeated_str(&mut f, 1, &self.staging_allow_hosts);
        put_repeated_str(&mut f, 2, &self.runtime_allow_hosts);
        put_bool(&mut f, 3, self.mediated_dns);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::ArtifactScope {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_repeated_str(&mut f, 1, &self.writable_prefixes);
        put_repeated_str(&mut f, 2, &self.readable_prefixes);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

impl ToCanonicalFields for pb::ResourceScope {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_repeated_str(&mut f, 1, &self.gpu_uuids);
        put_uint(&mut f, 2, self.cpu_cores as u64);
        put_uint(&mut f, 3, self.ram_bytes);
        put_uint(&mut f, 4, self.workspace_bytes);
        put_repeated_str(&mut f, 5, &self.writable_prefixes);
        f
    }
    fn schema_version(&self) -> u32 {
        1
    }
}

// ══════════════════════════════════════════════════════════════════
// job.proto — JobManifest
//
// ★ field number 는 proto/job.proto 와 정확히 일치해야 한다.
//   테스트 `field_numbers_match_proto_definition` 이 검증한다.
// ══════════════════════════════════════════════════════════════════

impl ToCanonicalFields for pb::JobManifest {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();

        // 식별
        put_uint(&mut f, 1, self.schema_version as u64);
        put_str(&mut f, 2, &self.job_id);
        put_str(&mut f, 3, &self.team_id);

        // 실행 대상
        put_msg(&mut f, 10, &self.env);
        put_repeated_msg(&mut f, 11, &self.input_artifacts);
        put_msg(&mut f, 12, &self.dataset);
        put_str(&mut f, 13, &self.entrypoint);
        put_repeated_str(&mut f, 14, &self.args);
        put_map(&mut f, 15, &norm_map(&self.env_vars));

        // 자원 요구
        put_msg(&mut f, 20, &self.resources);
        put_msg(&mut f, 21, &self.workload);

        // 성능
        put_uint(&mut f, 30, self.deadline_minutes as u64);
        put_uint(&mut f, 31, self.preference as u64);
        put_uint(&mut f, 32, self.max_queue_minutes as u64);

        // 복구
        put_uint(&mut f, 40, self.checkpoint_interval_minutes as u64);
        put_uint(&mut f, 41, self.durability as u64);
        put_uint(&mut f, 42, self.on_partition as u64);
        put_uint(&mut f, 43, self.max_data_loss_minutes as u64);

        // 보안
        put_uint(&mut f, 50, self.minimum_isolation_class as u64);
        put_uint(&mut f, 51, self.minimum_security_tier as u64);
        put_uint(&mut f, 52, self.minimum_key_protection as u64);
        put_uint(&mut f, 53, self.side_effect_class as u64);
        // ★ 54·55 는 보안 필드다. 서명 밖에 있으면 중간자가 고쳐도 검증이 통과한다.
        put_msg(&mut f, 54, &self.network);
        put_msg(&mut f, 55, &self.artifact_scope);
        put_bool(&mut f, 56, self.acknowledge_duplicate_risk);

        // 발급
        // ★ proto 는 `string submitter_device_id = 60` 이다 (common.proto 의 ULID 규약).
        //   기준선 §15.2 예시는 `bytes` 로 적혀 있으나 규범 문서인 proto 가 이긴다
        //   (`docs/README.md` 우선순위). 문서 드리프트로 기록해 두었다.
        put_str(&mut f, 60, &self.submitter_device_id);
        put_uint(&mut f, 61, self.issued_at_unix_ms);
        put_uint(&mut f, 62, self.expires_at_unix_ms);

        // ★ field 90 (submitter_signature) 은 넣지 않는다 (signing.md 규칙 i)
        f
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

// ══════════════════════════════════════════════════════════════════
// lease.proto — Lease
// ══════════════════════════════════════════════════════════════════

impl ToCanonicalFields for pb::Lease {
    fn to_canonical_fields(&self) -> Fields {
        let mut f = Fields::new();
        put_uint(&mut f, 1, self.schema_version as u64);
        put_str(&mut f, 2, &self.lease_id);
        put_str(&mut f, 3, &self.job_id);
        put_str(&mut f, 4, &self.attempt_id);

        put_uint(&mut f, 10, self.fence_epoch);
        put_uint(&mut f, 11, self.coordinator_term);

        put_str(&mut f, 20, &self.holder_node_id);
        put_repeated_str(&mut f, 21, &self.member_node_ids);
        put_str(&mut f, 22, &self.issuing_coordinator_id);

        put_uint(&mut f, 30, self.issued_at_unix_ms);
        put_uint(&mut f, 31, self.expires_at_unix_ms);
        put_uint(&mut f, 32, self.renew_after_unix_ms);
        put_uint(&mut f, 33, self.max_total_duration_seconds as u64);

        // ★ scope 는 보안 필드다. 서명 밖에 있으면 보유자가 스스로 자원 범위를 넓힐 수 있다.
        put_msg(&mut f, 40, &self.scope);

        // field 90 (coordinator_signature) 제외
        f
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

// ══════════════════════════════════════════════════════════════════
// 미구현 필드 안내
//
// 아래 필드들은 아직 `to_canonical_fields` 에 포함되지 않았다.
// **서명 대상에서 빠져 있다는 뜻이므로 구현 전에 반드시 채워야 한다.**
// 테스트 `unimplemented_nested_fields_are_documented` 가 이 목록과
// 실제 구현 상태를 대조한다.
// ══════════════════════════════════════════════════════════════════

/// 아직 canonical 변환에 포함되지 않은 필드 (메시지, field number).
///
/// **이 목록이 비어야 서명이 완전해진다.**
///
/// 2026-08-16: JobManifest 와 Lease 의 6건을 전부 구현해 **비웠다**.
/// 남은 미구현은 목록이 아니라 `ToCanonicalFields` 미구현 메시지 쪽이다
/// (`artifact.proto` · `control.proto` 의 서명 대상 — DoD-02 limitations 참조).
///
/// 여기에 항목을 추가할 때는 **왜 지금 구현하지 않는지**를 설명에 적는다.
/// 서명 밖 필드는 위조 가능하다는 뜻이므로 "나중에" 는 사유가 되지 않는다.
pub const UNIMPLEMENTED_FIELDS: &[(&str, u32, &str)] = &[];
