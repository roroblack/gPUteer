#!/usr/bin/env python3
"""
gPUteer canonical_encode — 참조 구현 (NORMATIVE companion)

규범 문서: docs/protocol/signing.md
목적:
  1. docs/protocol/signing.md §3 의 규칙을 실행 가능한 형태로 고정한다.
  2. Rust 구현(crates/protocol)이 대조할 테스트 벡터를 생성한다.

의존성 없음 (표준 라이브러리만). protobuf 라이브러리를 쓰지 않는 이유:
  - 라이브러리 버전에 따라 인코딩이 달라지는 것이 바로 우리가 막으려는 문제다.
  - 참조 구현은 바이트를 직접 만들어야 규범이 된다.

사용법:
    python reference_canonical.py --self-test
    python reference_canonical.py --emit-vectors > ../../tests/vectors/canonical_v1.json
    python reference_canonical.py --verify ../../tests/vectors/canonical_v1.json
"""

import argparse
import json
import sys

# ═══════════════════════════════════════════════════════════════════════
# Wire format primitives
# ═══════════════════════════════════════════════════════════════════════

WIRETYPE_VARINT = 0
WIRETYPE_LEN = 2


def encode_varint(n: int) -> bytes:
    """signing.md 규칙 e — 최단 인코딩만 생성한다."""
    if n < 0:
        raise ValueError("negative varint not allowed in canonical encoding")
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def decode_varint(buf: bytes, pos: int):
    """non-minimal varint를 거부한다 (규칙 e)."""
    result = 0
    shift = 0
    start = pos
    while True:
        if pos >= len(buf):
            raise ValueError("truncated varint")
        b = buf[pos]
        pos += 1
        result |= (b & 0x7F) << shift
        if not (b & 0x80):
            break
        shift += 7
        if shift > 63:
            raise ValueError("varint too long")
    # 최단성 검사: 마지막 바이트가 0이면서 길이가 1보다 크면 non-minimal
    if pos - start > 1 and buf[pos - 1] == 0:
        raise ValueError("non-minimal varint rejected")
    if encode_varint(result) != buf[start:pos]:
        raise ValueError("non-minimal varint rejected")
    return result, pos


def encode_tag(field_number: int, wiretype: int) -> bytes:
    return encode_varint((field_number << 3) | wiretype)


def encode_len_delimited(field_number: int, payload: bytes) -> bytes:
    return encode_tag(field_number, WIRETYPE_LEN) + encode_varint(len(payload)) + payload


# ═══════════════════════════════════════════════════════════════════════
# Schema description
#
# 실제 .proto 를 파싱하지 않고 필드 표를 손으로 둔다.
# 서명 대상 메시지는 17개뿐이므로 현실적이며, 표가 곧 명세가 된다.
# 표와 .proto 가 어긋나면 CI 가 잡도록 tools/canonical/check_schema.py 를 둔다.
# ═══════════════════════════════════════════════════════════════════════

# kind: uint | int | bool | enum | string | bytes | message | map_ss
#       repeated_string | repeated_message
#
# ★ "int" 는 규칙 j (2026-08-16 추가) — int32/int64.
#   2의 보수 u64 재해석. zigzag 가 아니다. 음수는 항상 10바이트.
Field = tuple  # (number, name, kind, nested_schema_or_None)

SCHEMAS = {
    "Digest": [
        (1, "algo", "enum", None),
        (2, "value", "bytes", None),
    ],
    "CudaRequirement": [
        (1, "min_driver_version", "uint", None),
        (2, "cuda_runtime_version", "string", None),
        (3, "compute_capabilities", "repeated_string", None),
    ],
    "GpuRequest": [
        (1, "min_vram_bytes", "uint", None),
        (2, "min_count", "uint", None),
        (3, "cuda", "message", "CudaRequirement"),
        (4, "allocation_mode", "enum", None),
        (5, "allowed_gpu_models", "repeated_string", None),
    ],
    "ResourceRequest": [
        (1, "gpu", "message", "GpuRequest"),
        (2, "cpu_cores", "uint", None),
        (3, "ram_bytes", "uint", None),
        (4, "workspace_bytes", "uint", None),
        (5, "max_egress_bps", "uint", None),
    ],
    "WorkloadHint": [
        (1, "class", "enum", None),
        (2, "estimated_steps", "uint", None),
        (3, "ref_step_time_ms", "uint", None),
        (4, "ref_gpu_model", "string", None),
        (10, "model_params", "uint", None),
        (11, "est_checkpoint_bytes", "uint", None),
        (12, "est_peak_vram_bytes", "uint", None),
    ],
    # --- 아래 5종은 2026-08-16 추가 ---
    # DoD-02 가 "서명 대상에서 빠진 필드 6건, 그 중 3건이 보안 필드" 를 찾았다.
    # 그 6건을 서명 안으로 넣기 위해 참조 구현을 먼저 확장한다 (RULE.md §3.5 계약 우선).
    "TarballPolicy": [
        (1, "reject_path_traversal", "bool", None),
        (2, "reject_links", "bool", None),
        (3, "max_extracted_bytes", "uint", None),
    ],
    "ExecutionEnvironment": [
        (1, "kind", "enum", None),
        (10, "image_ref", "string", None),
        (11, "image_digest", "message", "Digest"),
        (12, "oci_source_digest", "message", "Digest"),
        (20, "base_runtime", "string", None),
        (21, "lock_digest", "message", "Digest"),
        (22, "lock_content", "bytes", None),
        (23, "lock_cas_ref", "message", "Digest"),
        (30, "os", "string", None),
        (31, "arch", "string", None),
        (32, "min_libc_version", "string", None),
        (33, "cuda", "message", "CudaRequirement"),
        (34, "code_digest", "message", "Digest"),
        (35, "tarball_policy", "message", "TarballPolicy"),
    ],
    "DatasetRef": [
        (1, "root_digest", "message", "Digest"),
        (2, "total_bytes", "uint", None),
        (3, "sensitivity", "enum", None),
        (4, "retention", "enum", None),
        (5, "encrypted_at_rest", "bool", None),
        (6, "display_name", "string", None),
    ],
    "NetworkPolicy": [
        (1, "staging_allow_hosts", "repeated_string", None),
        (2, "runtime_allow_hosts", "repeated_string", None),
        (3, "mediated_dns", "bool", None),
    ],
    "ArtifactScope": [
        (1, "writable_prefixes", "repeated_string", None),
        (2, "readable_prefixes", "repeated_string", None),
    ],
    "ResourceScope": [
        (1, "gpu_uuids", "repeated_string", None),
        (2, "cpu_cores", "uint", None),
        (3, "ram_bytes", "uint", None),
        (4, "workspace_bytes", "uint", None),
        (5, "writable_prefixes", "repeated_string", None),
    ],
    # ★ proto/job.proto 의 **전 필드**. 부분집합이 아니다.
    #   부분집합이면 표에 없는 필드는 교차검증을 받지 않는다.
    "JobManifest": [
        (1, "schema_version", "uint", None),
        (2, "job_id", "string", None),
        (3, "team_id", "string", None),
        (10, "env", "message", "ExecutionEnvironment"),
        (11, "input_artifacts", "repeated_message", "Digest"),
        (12, "dataset", "message", "DatasetRef"),
        (13, "entrypoint", "string", None),
        (14, "args", "repeated_string", None),
        (15, "env_vars", "map_ss", None),
        (20, "resources", "message", "ResourceRequest"),
        (21, "workload", "message", "WorkloadHint"),
        (30, "deadline_minutes", "uint", None),
        (31, "preference", "enum", None),
        (32, "max_queue_minutes", "uint", None),
        (40, "checkpoint_interval_minutes", "uint", None),
        (41, "durability", "enum", None),
        (42, "on_partition", "enum", None),
        (43, "max_data_loss_minutes", "uint", None),
        (50, "minimum_isolation_class", "enum", None),
        (51, "minimum_security_tier", "enum", None),
        (52, "minimum_key_protection", "enum", None),
        (53, "side_effect_class", "enum", None),
        (54, "network", "message", "NetworkPolicy"),
        (55, "artifact_scope", "message", "ArtifactScope"),
        (56, "acknowledge_duplicate_risk", "bool", None),
        (60, "submitter_device_id", "string", None),
        (61, "issued_at_unix_ms", "uint", None),
        (62, "expires_at_unix_ms", "uint", None),
        (90, "submitter_signature", "bytes", None),  # 규칙 i — 항상 제외됨
    ],
    # ★ proto/lease.proto 의 전 필드.
    "Lease": [
        (1, "schema_version", "uint", None),
        (2, "lease_id", "string", None),
        (3, "job_id", "string", None),
        (4, "attempt_id", "string", None),
        (10, "fence_epoch", "uint", None),
        (11, "coordinator_term", "uint", None),
        (20, "holder_node_id", "string", None),
        (21, "member_node_ids", "repeated_string", None),
        (22, "issuing_coordinator_id", "string", None),
        (30, "issued_at_unix_ms", "uint", None),
        (31, "expires_at_unix_ms", "uint", None),
        (32, "renew_after_unix_ms", "uint", None),
        (33, "max_total_duration_seconds", "uint", None),
        (40, "scope", "message", "ResourceScope"),
        (90, "coordinator_signature", "bytes", None),  # 규칙 i
    ],
    # ══════════════════════════════════════════════════════════════
    # T1 (2026-08-16) — artifact.proto / lease.proto / job.proto 의 서명 대상.
    #
    # ★ ReportedMetric 이 규칙 j(부호 있는 정수)를 도입한 이유다.
    #   스키마 전체에서 유일한 int64 이고, 하필 서명 대상 안에 있었다.
    # ══════════════════════════════════════════════════════════════
    "ReportedMetric": [
        (1, "name", "string", None),
        (2, "value_micro", "int", None),   # ★ 규칙 j — 음수 가능
        (3, "higher_is_better", "bool", None),
    ],
    "CheckpointFile": [
        (1, "path", "string", None),
        (2, "digest", "message", "Digest"),
        (3, "size_bytes", "uint", None),
        (4, "chunk_digests", "repeated_message", "Digest"),
        (5, "chunk_size_bytes", "uint", None),
    ],
    "ResumeCompleteness": [
        (1, "model_weights", "bool", None),
        (2, "optimizer_state", "bool", None),
        (3, "lr_scheduler_state", "bool", None),
        (4, "rng_state", "bool", None),
        (5, "sampler_position", "bool", None),
        (6, "dataloader_position", "bool", None),
        (7, "amp_scaler_state", "bool", None),
        (10, "full_resume_guaranteed", "bool", None),
    ],
    "CheckpointManifest": [
        (1, "schema_version", "uint", None),
        (2, "checkpoint_id", "string", None),
        (3, "job_id", "string", None),
        (4, "attempt_id", "string", None),
        (5, "step", "uint", None),
        (6, "epoch", "uint", None),
        (10, "files", "repeated_message", "CheckpointFile"),
        (11, "root_digest", "message", "Digest"),
        (12, "total_bytes", "uint", None),
        (20, "completeness", "message", "ResumeCompleteness"),
        (30, "created_at_unix_ms", "uint", None),
        (31, "producer_node_id", "string", None),
        (32, "fence_epoch", "uint", None),
        (90, "producer_signature", "bytes", None),
    ],
    "ReplicaAck": [
        (1, "schema_version", "uint", None),
        (2, "checkpoint_id", "string", None),
        (3, "root_digest", "message", "Digest"),
        (10, "holder_device_id", "string", None),
        (11, "kind", "enum", None),
        (12, "failure_domain", "string", None),
        (20, "fsynced", "bool", None),
        (21, "hash_verified", "bool", None),
        (22, "stored_bytes", "uint", None),
        (30, "acked_at_unix_ms", "uint", None),
        (90, "holder_signature", "bytes", None),
    ],
    "ArtifactRef": [
        (1, "schema_version", "uint", None),
        (2, "artifact_id", "string", None),
        (3, "job_id", "string", None),
        (4, "attempt_id", "string", None),
        (5, "kind", "enum", None),
        (10, "digest", "message", "Digest"),
        (11, "size_bytes", "uint", None),
        (12, "cas_path", "string", None),
        (20, "replicas", "repeated_message", "ReplicaAck"),
        (21, "created_at_unix_ms", "uint", None),
        (22, "fence_epoch", "uint", None),
        (90, "producer_signature", "bytes", None),
    ],
    "AttemptReport": [
        (1, "schema_version", "uint", None),
        (2, "job_id", "string", None),
        (3, "attempt_id", "string", None),
        (4, "node_id", "string", None),
        (5, "fence_epoch", "uint", None),
        (10, "outcome", "enum", None),
        (11, "final_step", "uint", None),
        (12, "started_at_unix_ms", "uint", None),
        (13, "finished_at_unix_ms", "uint", None),
        (20, "artifacts", "repeated_message", "ArtifactRef"),
        (21, "final_checkpoint", "message", "CheckpointManifest"),
        (30, "metrics", "repeated_message", "ReportedMetric"),
        (40, "issued_at_unix_ms", "uint", None),
        (90, "node_signature", "bytes", None),
    ],
    "CanonicalDecision": [
        (1, "schema_version", "uint", None),
        (2, "job_id", "string", None),
        (3, "chosen_attempt_id", "string", None),
        (4, "superseded_attempt_ids", "repeated_string", None),
        (10, "deciding_criterion", "enum", None),
        (11, "rationale", "string", None),
        (12, "fence_epoch", "uint", None),
        (13, "decided_at_unix_ms", "uint", None),
        (90, "coordinator_signature", "bytes", None),
    ],
    "ProgressReport": [
        (1, "current_step", "uint", None),
        (2, "total_steps", "uint", None),
        (3, "eta_seconds", "uint", None),
        (4, "last_committed_step", "uint", None),
        (5, "replication_backlog_bytes", "uint", None),
    ],
    "RenewLeaseRequest": [
        (1, "schema_version", "uint", None),
        (2, "lease_id", "string", None),
        (3, "fence_epoch", "uint", None),
        (4, "node_id", "string", None),
        (10, "progress", "message", "ProgressReport"),
        (20, "issued_at_unix_ms", "uint", None),
        (21, "nonce", "bytes", None),
        (90, "node_signature", "bytes", None),
    ],
    "RevokeLeaseNotice": [
        (1, "schema_version", "uint", None),
        (2, "lease_id", "string", None),
        (3, "fence_epoch", "uint", None),
        (4, "cause", "enum", None),
        (5, "issued_at_unix_ms", "uint", None),
        (90, "coordinator_signature", "bytes", None),
    ],
    # ★ 2026-08-19 — coordinator/agent 핸드셰이크(2026-08-18)가 추가한
    #   AgentGrantAck 는 그동안 이 참조 구현(Python)과 canonical_v1.json
    #   양쪽에 없었다 — Rust 구현(crates/protocol/src/to_fields.rs)의
    #   canonical/sig_input 바이트가 독립 참조 구현과 대조된 적이
    #   없는 공백이었다(DoD-05 schema v2 승격 재검수에서 발견,
    #   CLAUDE.md 백로그 6번). 여기서 그 공백을 메운다.
    "AgentGrantAck": [
        (1, "schema_version", "uint", None),
        (2, "grant_id", "string", None),
        (3, "attempt_id", "string", None),
        (4, "agent_device_id", "string", None),
        (5, "issued_at_unix_ms", "uint", None),
        (6, "expires_at_unix_ms", "uint", None),
        (7, "nonce", "bytes", None),
        (8, "accepted", "bool", None),
        (90, "agent_signature", "bytes", None),
    ],
    # ★ 2026-08-19 — Lease 갱신 최소 조각(docs/plans/2026-08-19_0500_...)이
    #   RenewLeaseResult 를 서명 대상으로 승격했다. RenewLeaseRequest(agent
    #   가 서명하는 요청)와 domain_tag 를 공유하지 않는다 — 공유하면 요청
    #   서명이 응답 검증도 통과해 교차 재생이 가능해진다(§5.1 과 같은 이유).
    "RenewLeaseResult": [
        (1, "outcome", "enum", None),
        (2, "lease", "message", "Lease"),
        (3, "detail", "string", None),
        (4, "retry_after_ms", "uint", None),
        (5, "schema_version", "uint", None),
        (6, "coordinator_id", "string", None),
        (7, "issued_at_unix_ms", "uint", None),
        (8, "request_nonce", "bytes", None),
        (90, "coordinator_signature", "bytes", None),
    ],
    "AgentSessionHello": [
        (1, "schema_version", "uint", None),
        (2, "mode", "enum", None),
        (3, "session_id", "string", None),
        (4, "node_id", "string", None),
        (5, "connection_attempt", "uint", None),
        (6, "issued_at_unix_ms", "uint", None),
        (7, "nonce", "bytes", None),
        (90, "node_signature", "bytes", None),
    ],
    "NodeHeartbeat": [
        (1, "schema_version", "uint", None),
        (2, "node_id", "string", None),
        (3, "device_id", "string", None),
        (4, "coordinator_device_id", "string", None),
        (5, "issued_at_unix_ms", "uint", None),
        (6, "fence_epoch", "uint", None),
        (7, "running_attempts", "uint", None),
        (8, "request_nonce", "bytes", None),
        (90, "node_signature", "bytes", None),
    ],
    "ResumeLeaseRequest": [
        (1, "schema_version", "uint", None),
        (2, "lease_id", "string", None),
        (3, "job_id", "string", None),
        (4, "attempt_id", "string", None),
        (5, "node_id", "string", None),
        (6, "fence_epoch", "uint", None),
        (7, "session_id", "string", None),
        (8, "connection_attempt", "uint", None),
        (9, "issued_at_unix_ms", "uint", None),
        (10, "request_nonce", "bytes", None),
        (90, "node_signature", "bytes", None),
    ],
    "ResumeLeaseResult": [
        (1, "outcome", "enum", None),
        (2, "lease", "message", "Lease"),
        (3, "detail", "string", None),
        (4, "retry_after_ms", "uint", None),
        (5, "schema_version", "uint", None),
        (6, "coordinator_id", "string", None),
        (7, "issued_at_unix_ms", "uint", None),
        (8, "request_nonce", "bytes", None),
        (90, "coordinator_signature", "bytes", None),
    ],
    # ══════════════════════════════════════════════════════════════
    # T1b (2026-08-16) — grant · membership · policy · quarantine
    #
    # ★ RevokeDevice · UpdatePolicy · QuarantineDevice 는
    #   `repeated bytes signatures = 90` — **다중 서명(m-of-n)** 이다.
    #   규칙 i 는 그대로(90 제외)이나 검증 절차가 다르다.
    # ══════════════════════════════════════════════════════════════
    "PeerHint": [
        (1, "node_id", "string", None),
        (2, "peer_id", "string", None),
        (3, "multiaddrs", "repeated_string", None),
        (4, "known_digests", "repeated_message", "Digest"),
    ],
    "EphemeralCredential": [
        (1, "credential_id", "string", None),
        (2, "token", "bytes", None),
        (3, "expires_at_unix_ms", "uint", None),
        (4, "allowed_endpoints", "repeated_string", None),
    ],
    "RejectedCandidate": [
        (1, "node_id", "string", None),
        (2, "reason", "enum", None),
        (3, "detail", "string", None),
    ],
    # 배치 근거. ★ 감사 목적으로 서명 대상이다 —
    # 서명 밖이면 Coordinator 가 "왜 이 노드를 골랐는가" 를 사후에 조작할 수 있다.
    "PlacementRationale": [
        (1, "t_est_seconds", "uint", None),
        (2, "sigma_ln_ppm", "uint", None),
        (3, "stage", "enum", None),
        (10, "p_within_estimate_ppm", "uint", None),
        (11, "p_survival_ppm", "uint", None),
        (12, "p_success_ppm", "uint", None),
        (13, "target_confidence_ppm", "uint", None),
        (20, "is_exploration", "bool", None),
        (30, "rejected", "repeated_message", "RejectedCandidate"),
    ],
    "GrantedExecutionPlan": [
        (1, "mode", "enum", None),
        (2, "gpu_allocation", "enum", None),
        (3, "assigned_gpu_uuids", "repeated_string", None),
        (10, "remote_replication_interval_minutes", "uint", None),
        (11, "effective_durability", "enum", None),
        (20, "rationale", "message", "PlacementRationale"),
    ],
    "ExecutionGrant": [
        (1, "schema_version", "uint", None),
        (2, "grant_id", "string", None),
        (3, "manifest", "message", "JobManifest"),
        # ★ manifest_hash(4) 는 규칙 i 의 도출 해시 필드 — 목록에 두지 않는다
        (5, "attempt_id", "string", None),
        (6, "lease", "message", "Lease"),
        (7, "peers", "repeated_message", "PeerHint"),
        (8, "creds", "message", "EphemeralCredential"),
        (9, "plan", "message", "GrantedExecutionPlan"),
        (20, "coordinator_device_id", "string", None),
        (21, "coordinator_term", "uint", None),
        (22, "issued_at_unix_ms", "uint", None),
        (23, "expires_at_unix_ms", "uint", None),
        (24, "nonce", "bytes", None),
        (25, "lease_from_durable_store", "bool", None),
        (90, "coordinator_signature", "bytes", None),
    ],
    "CoordinatorEntry": [
        (1, "device_id", "string", None),
        (2, "public_key", "bytes", None),
        (3, "endpoints", "repeated_string", None),
        (4, "failure_domain", "string", None),
    ],
    "RiskSignal": [
        (1, "kind", "string", None),
        (2, "detail", "string", None),
        (3, "observed_at_unix_ms", "uint", None),
        (4, "observer_coordinator_id", "string", None),
    ],
    "AddMember": [
        (1, "member_id", "string", None),
        (2, "public_key", "bytes", None),
        (3, "role", "string", None),
        (90, "owner_signature", "bytes", None),
    ],
    "RemoveMember": [
        (1, "member_id", "string", None),
        (90, "owner_signature", "bytes", None),
    ],
    "ApproveDevice": [
        (1, "device_id", "string", None),
        (2, "member_id", "string", None),
        (3, "public_key", "bytes", None),
        (4, "peer_id", "string", None),
        (5, "key_protection", "enum", None),
        (6, "is_ephemeral", "bool", None),
        (90, "owner_signature", "bytes", None),
    ],
    "RevokeDevice": [
        (1, "device_id", "string", None),
        (2, "reason", "string", None),
        (90, "signatures", "bytes", None),   # ★ 다중 서명. 규칙 i 로 제외되므로 kind 무관
    ],
    "ChangeCoordinatorSet": [
        (1, "new_set", "repeated_message", "CoordinatorEntry"),
        (2, "added_id", "string", None),
        (3, "removed_id", "string", None),
        (90, "owner_signature", "bytes", None),
    ],
    "RotateOwnerKey": [
        (1, "new_owner_public_key", "bytes", None),
        (2, "new_recovery_public_key", "bytes", None),
        (90, "authorizing_signature", "bytes", None),
    ],
    "UpdatePolicy": [
        (1, "policy_hash", "message", "Digest"),
        (2, "policy_content", "bytes", None),
        (3, "is_relaxation", "bool", None),
        (90, "signatures", "bytes", None),   # ★ 다중 서명
    ],
    "QuarantineDevice": [
        (1, "device_id", "string", None),
        (2, "signals", "repeated_message", "RiskSignal"),
        (3, "target_is_coordinator", "bool", None),
        (90, "verdict_signatures", "bytes", None),   # ★ 다중 서명
    ],
    "ReleaseQuarantine": [
        (1, "device_id", "string", None),
        (2, "reason", "string", None),
        (90, "owner_signature", "bytes", None),
    ],
}

# 규칙 i: canonical 인코딩에서 항상 제외되는 필드 번호
SIGNATURE_FIELD_NUMBER = 90
# 규칙 i: 도출 해시 필드 (메시지 안에 있으면 안 되지만, 방어적으로 목록화)
DERIVED_HASH_FIELDS = {"manifest_hash"}


def is_default(kind: str, value) -> bool:
    """signing.md 규칙 b — 기본값은 출력하지 않는다."""
    if value is None:
        return True
    if kind in ("uint", "enum", "int"):
        return value == 0
    if kind == "bool":
        return value is False
    if kind == "string":
        return value == ""
    if kind == "bytes":
        return len(value) == 0
    if kind in ("repeated_string", "repeated_message"):
        return len(value) == 0
    if kind == "map_ss":
        return len(value) == 0
    if kind == "message":
        return len(value) == 0 if isinstance(value, dict) else value is None
    raise ValueError("unknown kind: %s" % kind)


def canonical_encode(schema_name: str, msg: dict) -> bytes:
    """
    docs/protocol/signing.md §3 의 규칙 a~i 를 그대로 구현한다.
    """
    schema = SCHEMAS[schema_name]
    out = bytearray()

    # 규칙 a — field number 오름차순
    for number, name, kind, nested in sorted(schema, key=lambda f: f[0]):
        # 규칙 i — 서명 필드와 도출 해시 필드 제외
        if number == SIGNATURE_FIELD_NUMBER:
            continue
        if name in DERIVED_HASH_FIELDS:
            continue

        value = msg.get(name)

        # 규칙 b — 기본값 생략
        if is_default(kind, value):
            continue

        if kind in ("uint", "enum"):
            out += encode_tag(number, WIRETYPE_VARINT)
            out += encode_varint(int(value))  # 규칙 e

        elif kind == "int":
            # 규칙 j — 2의 보수 u64 재해석. proto3 int64 의 wire format 과 같다.
            # 음수는 항상 정확히 10바이트가 되며, 그것이 u64 값의 최단 varint 이므로
            # 규칙 e 와 모순되지 않는다.
            out += encode_tag(number, WIRETYPE_VARINT)
            out += encode_varint(int(value) & 0xFFFFFFFFFFFFFFFF)

        elif kind == "bool":
            out += encode_tag(number, WIRETYPE_VARINT)
            out += encode_varint(1)

        elif kind == "string":
            out += encode_len_delimited(number, value.encode("utf-8"))

        elif kind == "bytes":
            out += encode_len_delimited(number, bytes(value))

        elif kind == "repeated_string":
            # 규칙 d — 순서 유지, 정렬하지 않는다. packed 미사용.
            for item in value:
                out += encode_len_delimited(number, item.encode("utf-8"))

        elif kind == "map_ss":
            # 규칙 c — key 바이트 오름차순 정렬
            entries = [(k.encode("utf-8"), v.encode("utf-8")) for k, v in value.items()]
            entries.sort(key=lambda kv: kv[0])
            for kb, vb in entries:
                # ★ 규칙 c-2 — 엔트리 안에서도 규칙 b 를 적용한다.
                #   proto3 map 시맨틱에서 "값 없음" 과 "빈 값" 은 같다.
                #   키의 존재 자체는 정보이므로 field 1 은 항상 출력한다.
                inner = encode_len_delimited(1, kb)
                if vb:
                    inner += encode_len_delimited(2, vb)
                out += encode_len_delimited(number, inner)

        elif kind == "message":
            # 규칙 f — 재귀 적용
            inner = canonical_encode(nested, value)
            # ★ 규칙 i-2 — 서명/도출해시 제외 후 비면 필드 자체를 생략한다.
            #   그렇지 않으면 Message({90: sig}) 가 빈 중첩 메시지로 출력되어
            #   "서명 필드가 canonical 에 영향을 주지 않는다" 가 깨진다.
            if not inner:
                continue
            out += encode_len_delimited(number, inner)

        elif kind == "repeated_message":
            # ★ 규칙 d — 원소를 버리지 않는다. 빈 원소도 길이 0으로 자리를 지킨다.
            #   (규칙 i-2 는 **단일** 중첩 메시지에만 적용된다)
            for item in value:
                inner = canonical_encode(nested, item)
                out += encode_len_delimited(number, inner)

        else:
            raise ValueError("unknown kind: %s" % kind)

    return bytes(out)


# ═══════════════════════════════════════════════════════════════════════
# sig_input — signing.md §4
# ═══════════════════════════════════════════════════════════════════════

DOMAIN_TAGS = {
    "JobManifest": b"gputeer/v1/manifest",
    "ExecutionGrant": b"gputeer/v2/grant",
    "Lease": b"gputeer/v1/lease",
    "RenewLeaseRequest": b"gputeer/v1/lease-renew",
    "RevokeLeaseNotice": b"gputeer/v1/lease-revoke",
    "AgentGrantAck": b"gputeer/v1/grant-ack",
    "RenewLeaseResult": b"gputeer/v1/lease-renew-result",
    "AgentSessionHello": b"gputeer/v1/session-hello",
    "NodeHeartbeat": b"gputeer/v1/node-heartbeat",
    "ResumeLeaseRequest": b"gputeer/v1/lease-resume",
    "ResumeLeaseResult": b"gputeer/v1/lease-resume-result",
    "CheckpointManifest": b"gputeer/v1/checkpoint",
    "ReplicaAck": b"gputeer/v1/replica-ack",
    "ArtifactRef": b"gputeer/v1/artifact",
    "AttemptReport": b"gputeer/v1/attempt-report",
    "CanonicalDecision": b"gputeer/v1/canonical",
    "Genesis": b"gputeer/v1/genesis",
    # ★ ADR-028 (2026-08-16) — membership/policy/quarantine 3종을 9종으로 분리.
    #   공유하면 RemoveMember{id} 와 RevokeDevice{id} 의 canonical 이 28바이트로
    #   동일해 서명이 재사용된다.
    "AddMember": b"gputeer/v1/member-add",
    "RemoveMember": b"gputeer/v1/member-remove",
    "ApproveDevice": b"gputeer/v1/device-approve",
    "RevokeDevice": b"gputeer/v1/device-revoke",
    "ChangeCoordinatorSet": b"gputeer/v1/coordinator-set",
    "RotateOwnerKey": b"gputeer/v1/owner-key-rotate",
    "UpdatePolicy": b"gputeer/v1/policy-update",
    "QuarantineDevice": b"gputeer/v1/quarantine-device",
    "ReleaseQuarantine": b"gputeer/v1/quarantine-release",
    "Audit": b"gputeer/v1/audit",
    "Release": b"gputeer/v1/release",
    "Invite": b"gputeer/v1/invite",
}

DOMAIN_TAG_LEN = 32


# ★ ADR-028 이후 메시지 이름 == domain 키 (1:1).
#
# 2026-08-16 이전에는 membership · policy · quarantine 을 여러 메시지가 공유했고,
# 그 때문에 서명 재사용이 가능했다. 이 표가 비어 있다는 것이 곧 "공유가 없다" 는 뜻이다.
MESSAGE_DOMAIN = {}


def domain_tag(name: str) -> bytes:
    """32바이트, 우측 0x00 패딩."""
    raw = DOMAIN_TAGS[MESSAGE_DOMAIN.get(name, name)]
    if len(raw) > DOMAIN_TAG_LEN:
        raise ValueError("domain tag too long: %s" % name)
    return raw + b"\x00" * (DOMAIN_TAG_LEN - len(raw))


def uint32_be(n: int) -> bytes:
    return n.to_bytes(4, "big")


def sig_input(msg_type: str, schema_version: int, canonical: bytes) -> bytes:
    """
    sig_input = domain_tag(32) || uint32_be(schema_version) || uint32_be(len) || canonical
    """
    return (
        domain_tag(msg_type)
        + uint32_be(schema_version)
        + uint32_be(len(canonical))
        + canonical
    )


# ═══════════════════════════════════════════════════════════════════════
# Hash — BLAKE3 가 있을 때만 다이제스트를 채운다
#
# canonical bytes 자체는 해시 라이브러리 없이 완전히 결정된다.
# 따라서 blake3 가 없어도 벡터의 핵심(canonical / sig_input)은 생성 가능하다.
# blake3 가 없으면 digest 필드를 null 로 두고, 절대로 다른 해시로 대체하지 않는다.
# ═══════════════════════════════════════════════════════════════════════

try:
    import blake3 as _blake3  # type: ignore

    def blake3_256(data: bytes):
        return _blake3.blake3(data).digest(length=32).hex()

    HAVE_BLAKE3 = True
except ImportError:  # pragma: no cover
    def blake3_256(data: bytes):
        return None

    HAVE_BLAKE3 = False


# ═══════════════════════════════════════════════════════════════════════
# Merkle — signing.md §6.3
# ═══════════════════════════════════════════════════════════════════════

def merkle_root_hex(chunks):
    """홀수 노드는 승격(promote)한다. 복제하지 않는다."""
    if not HAVE_BLAKE3:
        return None
    if not chunks:
        return None
    level = [_blake3.blake3(b"\x00" + c).digest(length=32) for c in chunks]
    while len(level) > 1:
        nxt = []
        for i in range(0, len(level) - 1, 2):
            nxt.append(_blake3.blake3(b"\x01" + level[i] + level[i + 1]).digest(length=32))
        if len(level) % 2 == 1:
            nxt.append(level[-1])  # 승격
        level = nxt
    return level[0].hex()


# ═══════════════════════════════════════════════════════════════════════
# Test vectors — signing.md §12.2
# ═══════════════════════════════════════════════════════════════════════

def _minimal_manifest():
    return {
        "schema_version": 1,
        "job_id": "01JBXR7Q0000000000000000AA",
        "team_id": "01JBXR7Q0000000000000000TT",
        "entrypoint": "train.py",
        "submitter_device_id": "01JBXR7Q0000000000000000DD",
        "issued_at_unix_ms": 1755100800000,
        "expires_at_unix_ms": 1755705600000,
    }


def _full_manifest():
    m = _minimal_manifest()
    m.update({
        "args": ["--epochs", "3", "--lr", "1e-4"],
        "env_vars": {"OMP_NUM_THREADS": "8", "HF_HOME": "/ws/hf", "AAA": "1"},
        "resources": {
            "gpu": {
                "min_vram_bytes": 19327352832,
                "min_count": 1,
                "cuda": {
                    "min_driver_version": 550,
                    "cuda_runtime_version": "12.4",
                    "compute_capabilities": ["8.6", "8.9", "9.0"],
                },
                "allocation_mode": 1,
            },
            "cpu_cores": 8,
            "ram_bytes": 25769803776,
            "workspace_bytes": 85899345920,
        },
        "workload": {
            "class": 1,
            "estimated_steps": 20000,
            "ref_step_time_ms": 420,
            "ref_gpu_model": "RTX4090",
            "model_params": 1300000000,
            "est_checkpoint_bytes": 18200000000,
        },
        "deadline_minutes": 180,
        "preference": 2,
        "max_queue_minutes": 60,
        "checkpoint_interval_minutes": 15,
        "durability": 3,
        "on_partition": 2,
        "max_data_loss_minutes": 30,
        "minimum_isolation_class": 2,
        "minimum_security_tier": 3,
        "minimum_key_protection": 2,
        "side_effect_class": 1,
        # --- 2026-08-16 추가: DoD-02 가 찾은 서명 밖 필드 ---
        "env": {
            "kind": 1,
            "image_ref": "registry.internal/torch:2.4-cu124",
            "image_digest": {"algo": 1, "value": bytes(range(32))},
            "oci_source_digest": {"algo": 2, "value": bytes(range(1, 33))},
            "base_runtime": "python-3.11-cu124",
            "lock_digest": {"algo": 1, "value": bytes(range(2, 34))},
            "lock_content": b"torch==2.4.0\n",
            "lock_cas_ref": {"algo": 1, "value": bytes(range(3, 35))},
            "os": "linux",
            "arch": "amd64",
            "min_libc_version": "2.31",
            "cuda": {
                "min_driver_version": 550,
                "cuda_runtime_version": "12.4",
                "compute_capabilities": ["8.9"],
            },
            "code_digest": {"algo": 1, "value": bytes(range(4, 36))},
            "tarball_policy": {
                "reject_path_traversal": True,
                "reject_links": True,
                "max_extracted_bytes": 10 * 1024 * 1024 * 1024,
            },
        },
        "input_artifacts": [
            {"algo": 1, "value": b"\x01" * 32},
            {"algo": 1, "value": b"\x02" * 32},
        ],
        "dataset": {
            "root_digest": {"algo": 1, "value": b"\xAB" * 32},
            "total_bytes": 42_949_672_960,
            "sensitivity": 3,
            "retention": 2,
            "encrypted_at_rest": True,
            "display_name": "internal-corpus-v3",
        },
        "network": {
            "staging_allow_hosts": ["pypi.internal", "mirror.internal"],
            "runtime_allow_hosts": ["metrics.internal"],
            "mediated_dns": True,
        },
        "artifact_scope": {
            "writable_prefixes": ["jobs/01JBXR7Q0000000000000000AA/"],
            "readable_prefixes": ["datasets/shared/"],
        },
        "acknowledge_duplicate_risk": True,
        "submitter_signature": b"\xAA" * 64,  # 규칙 i — canonical 에서 제외되어야 함
    })
    return m


def _full_lease():
    """Lease 의 전 필드."""
    return {
        "schema_version": 1,
        "lease_id": "01JBXLEASE0000000000000001",
        "job_id": "01JBXR7Q0000000000000000AA",
        "attempt_id": "01JBXATT00000000000000001",
        "fence_epoch": 42,
        "coordinator_term": 7,
        "holder_node_id": "node-1",
        "member_node_ids": ["node-1", "node-2"],
        "issuing_coordinator_id": "coord-a",
        "issued_at_unix_ms": 1_755_100_800_000,
        "expires_at_unix_ms": 1_755_100_860_000,
        "renew_after_unix_ms": 1_755_100_830_000,
        "max_total_duration_seconds": 86400,
        "scope": {
            "gpu_uuids": ["GPU-11111111-2222-3333-4444-555555555555"],
            "cpu_cores": 8,
            "ram_bytes": 25_769_803_776,
            "workspace_bytes": 85_899_345_920,
            "writable_prefixes": ["jobs/01JBXR7Q0000000000000000AA/attempt-3/"],
        },
        "coordinator_signature": b"\xCD" * 64,
    }


def missing_from_full(schema_name: str, msg: dict):
    """
    "전체 필드" 벡터가 실제로 전 필드를 채웠는지 검사한다.

    ★ v02 는 오랫동안 "모든 필드" 라고 적혀 있었지만 실제로는 부분집합이었다
      (2026-08-16 발견). 주장과 실제가 어긋나면 그 필드는 아무도 검증하지 않는다.
      그래서 주장을 사람이 아니라 코드가 검사하게 한다.

    서명 필드(90)는 규칙 i 로 제외되므로 대상이 아니다.
    """
    out = []
    for number, name, kind, _nested in SCHEMAS[schema_name]:
        if number == SIGNATURE_FIELD_NUMBER:
            continue
        if is_default(kind, msg.get(name)):
            out.append("%s(%d)" % (name, number))
    return out


def build_vectors():
    vectors = []

    def add(name, desc, schema, msg, checks=None):
        canon = canonical_encode(schema, msg)
        si = sig_input(schema, msg.get("schema_version", 1), canon)
        vectors.append({
            "name": name,
            "description": desc,
            "message_type": schema,
            "canonical_hex": canon.hex(),
            "canonical_len": len(canon),
            "sig_input_hex": si.hex(),
            "sig_input_blake3_256": blake3_256(si),
            "checks": checks or [],
        })
        return canon

    # 1. 최소 메시지 — 기본값 생략 (규칙 b)
    add("v01_minimal_manifest",
        "필수 필드만. 기본값 필드가 출력되지 않아야 한다 (규칙 b)",
        "JobManifest", _minimal_manifest())

    # 2. 전체 필드 — 필드 순서 (규칙 a)
    # ★ 정말로 전 필드인지 코드가 검사한다. 빠진 필드는 아무도 검증하지 않는다.
    _gap = missing_from_full("JobManifest", _full_manifest())
    assert not _gap, "v02 가 전 필드를 채우지 않았다: %s" % ", ".join(_gap)
    add("v02_full_manifest",
        "모든 필드. field number 오름차순으로 직렬화되어야 한다 (규칙 a)",
        "JobManifest", _full_manifest())

    # 2b. Lease 전체 필드
    _gap_l = missing_from_full("Lease", _full_lease())
    assert not _gap_l, "v02b 가 전 필드를 채우지 않았다: %s" % ", ".join(_gap_l)
    add("v02b_full_lease",
        "Lease 의 모든 필드. scope(40) 포함 (규칙 a·f)",
        "Lease", _full_lease())

    # 3. map 정렬 (규칙 c) — 삽입 순서가 달라도 canonical 이 같아야 한다
    m_a = _minimal_manifest()
    m_a["env_vars"] = {"ZZZ": "3", "AAA": "1", "MMM": "2"}
    m_b = _minimal_manifest()
    m_b["env_vars"] = {"AAA": "1", "MMM": "2", "ZZZ": "3"}
    c_a = add("v03a_map_insertion_order_1",
              "map 삽입 순서 A (규칙 c)", "JobManifest", m_a,
              ["MUST_EQUAL:v03b_map_insertion_order_2"])
    c_b = add("v03b_map_insertion_order_2",
              "map 삽입 순서 B — v03a 와 canonical 이 동일해야 한다 (규칙 c)",
              "JobManifest", m_b,
              ["MUST_EQUAL:v03a_map_insertion_order_1"])
    assert c_a == c_b, "map ordering rule violated"

    # 4. repeated 순서 유지 (규칙 d) — 순서가 다르면 canonical 이 달라야 한다
    r_a = _minimal_manifest()
    r_a["args"] = ["--a", "--b"]
    r_b = _minimal_manifest()
    r_b["args"] = ["--b", "--a"]
    cr_a = add("v04a_repeated_order_1",
               "repeated 순서 A (규칙 d)", "JobManifest", r_a,
               ["MUST_DIFFER:v04b_repeated_order_2"])
    cr_b = add("v04b_repeated_order_2",
               "repeated 순서 B — v04a 와 canonical 이 달라야 한다 (규칙 d)",
               "JobManifest", r_b,
               ["MUST_DIFFER:v04a_repeated_order_1"])
    assert cr_a != cr_b, "repeated order must be preserved"

    # 5. 중첩 재귀 (규칙 f)
    nested = _minimal_manifest()
    nested["resources"] = {
        "gpu": {"min_vram_bytes": 1, "cuda": {"min_driver_version": 550}}
    }
    add("v05_nested_recursion",
        "3단계 중첩. 규칙이 재귀 적용되어야 한다 (규칙 f)",
        "JobManifest", nested)

    # 6. 명시적 기본값 (규칙 b) — 최소 메시지와 canonical 이 같아야 한다
    zeros = _minimal_manifest()
    zeros.update({
        "deadline_minutes": 0,
        "preference": 0,
        "durability": 0,
        "acknowledge_duplicate_risk": False,
        "args": [],
        "env_vars": {},
    })
    c_zero = add("v06_explicit_defaults",
                 "0/빈값/UNSPECIFIED 를 명시. v01 과 canonical 이 동일해야 한다 (규칙 b)",
                 "JobManifest", zeros,
                 ["MUST_EQUAL:v01_minimal_manifest"])
    assert c_zero == canonical_encode("JobManifest", _minimal_manifest())

    # 8. 서명 필드 제외 (규칙 i)
    signed = _minimal_manifest()
    signed["submitter_signature"] = b"\xFF" * 64
    c_signed = add("v08_signature_excluded",
                   "서명 필드가 채워져도 v01 과 canonical 이 동일해야 한다 (규칙 i)",
                   "JobManifest", signed,
                   ["MUST_EQUAL:v01_minimal_manifest"])
    assert c_signed == canonical_encode("JobManifest", _minimal_manifest())

    # ══════════════════════════════════════════════════════════════
    # 14~19 — DoD-02 가 찾은 "서명 밖 필드 6건" 을 서명 안으로 넣는 벡터.
    #
    # ★ 이 벡터들의 요점은 "canonical 이 v01 과 달라야 한다" 는 것이다.
    #   같으면 그 필드는 서명에 반영되지 않은 것이고, 곧 위조 가능하다는 뜻이다.
    # ══════════════════════════════════════════════════════════════

    # 14. 보안 필드 — network(54) · artifact_scope(55)
    sec = _minimal_manifest()
    sec["network"] = {
        "staging_allow_hosts": ["pypi.internal", "mirror.internal"],
        "runtime_allow_hosts": [],
        "mediated_dns": True,
    }
    sec["artifact_scope"] = {
        "writable_prefixes": ["jobs/01JBXR7Q0000000000000000AA/"],
        "readable_prefixes": ["datasets/shared/"],
    }
    c_sec = add("v14_security_fields_are_signed",
                "network(54) · artifact_scope(55) 가 canonical 에 반영되어야 한다. "
                "빠지면 중간자가 네트워크 정책과 산출물 범위를 고쳐도 검증이 통과한다",
                "JobManifest", sec,
                ["MUST_DIFFER:v01_minimal_manifest"])
    assert c_sec != canonical_encode("JobManifest", _minimal_manifest()), \
        "보안 필드가 canonical 에 반영되지 않았다"

    # 15. 실행 환경 — env(10). 중첩 4단 (JobManifest→ExecutionEnvironment→Digest)
    envm = _minimal_manifest()
    envm["env"] = {
        "kind": 1,
        "image_ref": "registry.internal/torch:2.4-cu124",
        "image_digest": {"algo": 1, "value": bytes(range(32))},
        "os": "linux",
        "arch": "amd64",
        "min_libc_version": "2.31",
        "cuda": {"min_driver_version": 550, "cuda_runtime_version": "12.4"},
        "code_digest": {"algo": 1, "value": bytes(range(32, 64))},
        "tarball_policy": {
            "reject_path_traversal": True,
            "reject_links": True,
            "max_extracted_bytes": 10 * 1024 * 1024 * 1024,
        },
    }
    c_env = add("v15_execution_environment_is_signed",
                "env(10) 이 canonical 에 반영되어야 한다. 빠지면 실행 이미지를 교체할 수 있다",
                "JobManifest", envm,
                ["MUST_DIFFER:v01_minimal_manifest"])
    assert c_env != canonical_encode("JobManifest", _minimal_manifest())

    # 16. repeated message — input_artifacts(11). 순서 유지 (규칙 d)
    ia_a = _minimal_manifest()
    ia_a["input_artifacts"] = [
        {"algo": 1, "value": b"\x01" * 32},
        {"algo": 1, "value": b"\x02" * 32},
    ]
    ia_b = _minimal_manifest()
    ia_b["input_artifacts"] = [
        {"algo": 1, "value": b"\x02" * 32},
        {"algo": 1, "value": b"\x01" * 32},
    ]
    c_ia_a = add("v16a_input_artifacts_order_1",
                 "repeated message 순서 A (규칙 d). repeated 는 정렬하지 않는다",
                 "JobManifest", ia_a,
                 ["MUST_DIFFER:v16b_input_artifacts_order_2"])
    c_ia_b = add("v16b_input_artifacts_order_2",
                 "repeated message 순서 B — v16a 와 canonical 이 달라야 한다 (규칙 d)",
                 "JobManifest", ia_b,
                 ["MUST_DIFFER:v16a_input_artifacts_order_1"])
    assert c_ia_a != c_ia_b, "repeated message 순서가 유지되지 않았다"

    # 17. 데이터셋 — dataset(12)
    ds = _minimal_manifest()
    ds["dataset"] = {
        "root_digest": {"algo": 1, "value": b"\xAB" * 32},
        "total_bytes": 42_949_672_960,
        "sensitivity": 3,          # SENSITIVE
        "retention": 2,            # DELETE_ON_JOB_END
        "encrypted_at_rest": True,
        "display_name": "internal-corpus-v3",
    }
    c_ds = add("v17_dataset_is_signed",
               "dataset(12) 이 canonical 에 반영되어야 한다. "
               "빠지면 SENSITIVE 표시와 삭제 정책을 떼어낼 수 있다",
               "JobManifest", ds,
               ["MUST_DIFFER:v01_minimal_manifest"])
    assert c_ds != canonical_encode("JobManifest", _minimal_manifest())

    # 18/19. Lease — scope(40) 가 서명에 들어가는가
    lease_min = {
        "schema_version": 1,
        "lease_id": "01JBXLEASE0000000000000001",
        "job_id": "01JBXR7Q0000000000000000AA",
        "attempt_id": "01JBXATT00000000000000001",
        "fence_epoch": 42,
        "coordinator_term": 7,
        "holder_node_id": "node-1",
        "member_node_ids": ["node-1", "node-2"],
        "issuing_coordinator_id": "coord-a",
        "issued_at_unix_ms": 1_755_100_800_000,
        "expires_at_unix_ms": 1_755_100_860_000,
        "renew_after_unix_ms": 1_755_100_830_000,
        "max_total_duration_seconds": 86400,
        "coordinator_signature": b"\xCD" * 64,  # 규칙 i — 제외되어야 함
    }
    c_lease_min = add("v18_lease_minimal",
                      "Lease. 서명 필드(90)는 canonical 에서 제외된다 (규칙 i)",
                      "Lease", lease_min)

    lease_scoped = dict(lease_min)
    lease_scoped["scope"] = {
        "gpu_uuids": ["GPU-11111111-2222-3333-4444-555555555555"],
        "cpu_cores": 8,
        "ram_bytes": 25_769_803_776,
        "workspace_bytes": 85_899_345_920,
        "writable_prefixes": ["jobs/01JBXR7Q0000000000000000AA/attempt-3/"],
    }
    c_lease_scoped = add("v19_lease_scope_is_signed",
                         "Lease.scope(40) 가 canonical 에 반영되어야 한다. "
                         "빠지면 보유자가 자원 범위를 스스로 넓힐 수 있다",
                         "Lease", lease_scoped,
                         ["MUST_DIFFER:v18_lease_minimal"])
    assert c_lease_scoped != c_lease_min, "Lease.scope 가 canonical 에 반영되지 않았다"

    # ══════════════════════════════════════════════════════════════
    # 20~24 — T1. 규칙 j(부호 있는 정수) · 중첩 서명 메시지.
    # ══════════════════════════════════════════════════════════════

    # 20. ★ 규칙 j — 음수 int64.
    #     스키마 전체에서 유일한 부호 있는 필드이고, 하필 서명 대상 안에 있다.
    metrics_report = {
        "schema_version": 1,
        "job_id": "01JBXR7Q0000000000000000AA",
        "attempt_id": "01JBXATT00000000000000001",
        "node_id": "node-1",
        "fence_epoch": 42,
        "outcome": 1,
        "final_step": 20000,
        "started_at_unix_ms": 1_755_100_800_000,
        "finished_at_unix_ms": 1_755_104_400_000,
        "metrics": [
            {"name": "loss", "value_micro": 1_234_567, "higher_is_better": False},
            # ★ 음수 — 규칙 j 가 없으면 여기서 구현이 갈린다
            {"name": "loss_delta", "value_micro": -456_789, "higher_is_better": False},
            {"name": "min_i64", "value_micro": -(2**63), "higher_is_better": True},
            {"name": "max_i64", "value_micro": 2**63 - 1, "higher_is_better": True},
        ],
        "issued_at_unix_ms": 1_755_104_400_000,
        "node_signature": b"\xEF" * 64,
    }
    add("v20_signed_integers_rule_j",
        "int64 음수 (규칙 j). 2의 보수 u64 재해석 — zigzag 가 아니다. "
        "음수는 항상 정확히 10바이트",
        "AttemptReport", metrics_report)

    # 20b. 부호만 다른 두 값이 서로 다른 canonical 을 내야 한다.
    pos = dict(metrics_report)
    pos["metrics"] = [{"name": "m", "value_micro": 1000, "higher_is_better": True}]
    neg = dict(metrics_report)
    neg["metrics"] = [{"name": "m", "value_micro": -1000, "higher_is_better": True}]
    c_pos = add("v20a_int_positive", "int64 양수 (규칙 j)", "AttemptReport", pos,
                ["MUST_DIFFER:v20b_int_negative"])
    c_neg = add("v20b_int_negative",
                "int64 음수 — v20a 와 canonical 이 달라야 한다 (규칙 j)",
                "AttemptReport", neg,
                ["MUST_DIFFER:v20a_int_positive"])
    assert c_pos != c_neg, "부호가 canonical 에 반영되지 않았다"
    assert len(c_neg) > len(c_pos), "음수 varint 가 10바이트가 아니다"

    # 21. 체크포인트 매니페스트 — repeated message 3단 중첩
    ckpt = {
        "schema_version": 1,
        "checkpoint_id": "01JBXCKPT0000000000000001",
        "job_id": "01JBXR7Q0000000000000000AA",
        "attempt_id": "01JBXATT00000000000000001",
        "step": 12000,
        "epoch": 3,
        "files": [
            {
                "path": "model/weights.safetensors",
                "digest": {"algo": 1, "value": b"\x11" * 32},
                "size_bytes": 5_368_709_120,
                "chunk_digests": [
                    {"algo": 1, "value": b"\x21" * 32},
                    {"algo": 1, "value": b"\x22" * 32},
                ],
                "chunk_size_bytes": 4 * 1024 * 1024,
            },
            {
                "path": "optim/state.pt",
                "digest": {"algo": 1, "value": b"\x33" * 32},
                "size_bytes": 10_737_418_240,
            },
        ],
        "root_digest": {"algo": 1, "value": b"\x44" * 32},
        "total_bytes": 16_106_127_360,
        "completeness": {
            "model_weights": True,
            "optimizer_state": True,
            "lr_scheduler_state": True,
            "rng_state": True,
            "sampler_position": True,
            "dataloader_position": True,
            "amp_scaler_state": True,
            "full_resume_guaranteed": True,
        },
        "created_at_unix_ms": 1_755_103_000_000,
        "producer_node_id": "node-1",
        "fence_epoch": 42,
        "producer_signature": b"\x99" * 64,
    }
    add("v21_checkpoint_manifest",
        "CheckpointManifest. repeated message 안의 repeated message (규칙 d·f)",
        "CheckpointManifest", ckpt)

    # 22. ★ 중첩된 서명 메시지 — 규칙 i 는 재귀 적용된다.
    #
    #     ArtifactRef.replicas 는 **각자 서명된** ReplicaAck 들이다.
    #     규칙 i 가 재귀 적용되므로 **중첩 서명은 바깥 canonical 에 들어가지 않는다.**
    #     즉 중첩 서명을 바꿔치기해도 바깥 서명은 깨지지 않는다.
    #     -> 검증자는 중첩 서명 메시지를 **독립적으로 검증해야 한다(MUST).**
    def _artifact(ack_sig):
        return {
            "schema_version": 1,
            "artifact_id": "01JBXARTF0000000000000001",
            "job_id": "01JBXR7Q0000000000000000AA",
            "attempt_id": "01JBXATT00000000000000001",
            "kind": 1,
            "digest": {"algo": 1, "value": b"\x55" * 32},
            "size_bytes": 1_073_741_824,
            "cas_path": "jobs/01JBXR7Q0000000000000000AA/attempt-3/model.tar",
            "replicas": [{
                "schema_version": 1,
                "checkpoint_id": "01JBXCKPT0000000000000001",
                "root_digest": {"algo": 1, "value": b"\x44" * 32},
                "holder_device_id": "01JBXR7Q0000000000000000HH",
                "kind": 3,
                "failure_domain": "rack-a",
                "fsynced": True,
                "hash_verified": True,
                "stored_bytes": 1_073_741_824,
                "acked_at_unix_ms": 1_755_103_500_000,
                "holder_signature": ack_sig,   # ★ 중첩 서명
            }],
            "created_at_unix_ms": 1_755_103_600_000,
            "fence_epoch": 42,
            "producer_signature": b"\x77" * 64,
        }

    c_art = add("v22_artifact_ref_nested_signature",
                "ArtifactRef. 중첩된 ReplicaAck 의 서명(90)도 규칙 i 로 제외된다 — "
                "검증자는 중첩 서명 메시지를 독립적으로 검증해야 한다",
                "ArtifactRef", _artifact(b"\xAA" * 64),
                ["MUST_EQUAL:v22b_artifact_ref_swapped_nested_signature"])
    c_art2 = add("v22b_artifact_ref_swapped_nested_signature",
                 "중첩 서명만 바꾼 것 — v22 와 canonical 이 **같아야** 한다 (규칙 i 재귀). "
                 "이것이 중첩 독립 검증이 필수인 이유다",
                 "ArtifactRef", _artifact(b"\xBB" * 64),
                 ["MUST_EQUAL:v22_artifact_ref_nested_signature"])
    assert c_art == c_art2, "규칙 i 가 재귀 적용되지 않았다"

    # 23. Lease 갱신 (단수명, nonce 있음)
    add("v23_renew_lease_request",
        "RenewLeaseRequest — 단수명 메시지. nonce(21)는 canonical 에 포함된다",
        "RenewLeaseRequest", {
            "schema_version": 1,
            "lease_id": "01JBXLEASE0000000000000001",
            "fence_epoch": 42,
            "node_id": "node-1",
            "progress": {
                "current_step": 12000,
                "total_steps": 20000,
                "eta_seconds": 3400,
                "last_committed_step": 11500,
                "replication_backlog_bytes": 2_147_483_648,
            },
            "issued_at_unix_ms": 1_755_103_700_000,
            "nonce": bytes(range(16)),
            "node_signature": b"\x88" * 64,
        })

    # 24. Lease 회수
    add("v24_revoke_lease_notice",
        "RevokeLeaseNotice",
        "RevokeLeaseNotice", {
            "schema_version": 1,
            "lease_id": "01JBXLEASE0000000000000001",
            "fence_epoch": 42,
            "cause": 5,  # OWNER_PREEMPT — 소유자 주권 (CLAUDE.md §0.1)
            "issued_at_unix_ms": 1_755_103_800_000,
            "coordinator_signature": b"\xCC" * 64,
        })

    # ══════════════════════════════════════════════════════════════
    # 25~29 — T1b. grant · membership · policy · quarantine
    # ══════════════════════════════════════════════════════════════

    # 25. ★ ExecutionGrant — 서명된 메시지를 두 개 중첩하고,
    #     도출 해시 필드(manifest_hash)를 갖는 유일한 메시지.
    def _grant(manifest_sig, manifest_hash_val):
        m = _minimal_manifest()
        m["submitter_signature"] = manifest_sig
        return {
            "schema_version": 2,
            "grant_id": "01JBXGRANT0000000000000001",
            "manifest": m,
            # ★ 이 필드는 SCHEMAS["ExecutionGrant"] 에 없다 — 규칙 i 의 도출 해시 필드.
            #   여기 값을 넣어도 canonical 에 나타나지 않아야 한다.
            "manifest_hash": manifest_hash_val,
            "attempt_id": "01JBXATT00000000000000001",
            "lease": {
                "schema_version": 1,
                "lease_id": "01JBXLEASE0000000000000001",
                "job_id": "01JBXR7Q0000000000000000AA",
                "attempt_id": "01JBXATT00000000000000001",
                "fence_epoch": 42,
                "coordinator_term": 7,
                "holder_node_id": "node-1",
                "issued_at_unix_ms": 1_755_100_800_000,
                "expires_at_unix_ms": 1_755_100_860_000,
                "coordinator_signature": b"\xCD" * 64,
            },
            "peers": [{
                "node_id": "node-2",
                "peer_id": "12D3KooWExample",
                "multiaddrs": ["/ip4/10.20.20.2/udp/4001/quic-v1"],
            }],
            "creds": {
                "credential_id": "01JBXCRED0000000000000001",
                "token": b"\xDE\xAD\xBE\xEF" * 4,
                "expires_at_unix_ms": 1_755_100_860_000,
                "allowed_endpoints": ["hub.internal:443"],
            },
            "plan": {
                "mode": 1,
                "gpu_allocation": 1,
                "assigned_gpu_uuids": ["GPU-11111111-2222-3333-4444-555555555555"],
                "remote_replication_interval_minutes": 15,
                "effective_durability": 3,
                # ★ 배치 근거도 서명 대상이다 (감사 무결성)
                "rationale": {
                    "t_est_seconds": 10800,
                    "sigma_ln_ppm": 150000,
                    "stage": 2,
                    "p_within_estimate_ppm": 900000,
                    "p_survival_ppm": 979700,
                    "p_success_ppm": 881730,
                    "target_confidence_ppm": 800000,
                    "is_exploration": False,
                    "rejected": [
                        {"node_id": "node-9", "reason": 2, "detail": "insufficient VRAM"},
                    ],
                },
            },
            "coordinator_device_id": "01JBXR7Q0000000000000000CC",
            "coordinator_term": 7,
            "issued_at_unix_ms": 1_755_100_800_000,
            "expires_at_unix_ms": 1_755_100_860_000,
            "nonce": bytes(range(16)),
            "lease_from_durable_store": True,
            "coordinator_signature": b"\xFE" * 64,
        }

    c_g1 = add("v25_execution_grant",
               "ExecutionGrant. 서명된 메시지 2개(manifest·lease)를 중첩하고 "
               "도출 해시 필드(manifest_hash)를 갖는다",
               "ExecutionGrant", _grant(b"\xAA" * 64, {"algo": 1, "value": b"\x01" * 32}),
               ["MUST_EQUAL:v25b_execution_grant_derived_hash_and_nested_sig_swapped"])

    # 25b. ★ 도출 해시와 중첩 서명을 **둘 다** 바꿔도 canonical 이 같아야 한다.
    #      - manifest_hash: 규칙 i 의 도출 해시 필드 -> 제외
    #      - manifest.submitter_signature: 규칙 i 재귀 -> 제외
    #      => Agent 는 manifest 를 독립 검증하고 hash 를 재계산해야 한다(MUST).
    c_g2 = add("v25b_execution_grant_derived_hash_and_nested_sig_swapped",
               "manifest_hash 와 중첩 서명을 둘 다 바꾼 것 — v25 와 canonical 이 "
               "**같아야** 한다. 이것이 Agent 의 독립 검증·재계산이 필수인 이유다",
               "ExecutionGrant", _grant(b"\xBB" * 64, {"algo": 1, "value": b"\x02" * 32}),
               ["MUST_EQUAL:v25_execution_grant"])
    assert c_g1 == c_g2, "규칙 i(도출 해시 · 중첩 서명 제외)가 적용되지 않았다"

    # 25c. 반대로 manifest 의 **내용**은 반영되어야 한다.
    g3 = _grant(b"\xAA" * 64, {"algo": 1, "value": b"\x01" * 32})
    g3["manifest"] = dict(g3["manifest"])
    g3["manifest"]["entrypoint"] = "evil.py"
    c_g3 = add("v25c_execution_grant_manifest_content_changed",
               "manifest 의 내용을 바꾼 것 — v25 와 canonical 이 달라야 한다",
               "ExecutionGrant", g3,
               ["MUST_DIFFER:v25_execution_grant"])
    assert c_g3 != c_g1, "중첩 manifest 의 내용이 canonical 에 반영되지 않았다"

    # 26. 멤버십 — 소유자 서명
    add("v26_add_member",
        "AddMember (domain gputeer/v1/member-add)",
        "AddMember", {
            "member_id": "01JBXMEM00000000000000001",
            "public_key": bytes(range(32)),
            "role": "member",
            "owner_signature": b"\x11" * 64,
        })

    # 26b. ★ ADR-028 이전에는 membership 6종이 한 tag 를 공유했고 canonical 차이가
    #      유일한 방어였다. 지금은 tag 도 분리됐으므로(member-add / member-remove)
    #      방어가 이중이다 — 그래도 canonical 차이는 계속 검증한다.
    c_rm = add("v26b_remove_member",
               "RemoveMember — AddMember 와 **다른 domain_tag**(member-remove) 를 쓴다. "
               "ADR-028 이전의 tag 공유가 사라진 뒤에도 canonical 은 달라야 한다 (§5.1)",
               "RemoveMember", {
                   "member_id": "01JBXMEM00000000000000001",
                   "owner_signature": b"\x11" * 64,
               },
               ["MUST_DIFFER:v26_add_member"])

    # 27. 정책 변경 — 다중 서명
    add("v27_update_policy",
        "UpdatePolicy — 다중 서명(repeated bytes signatures = 90). "
        "is_relaxation 이 서명 대상이어야 완화를 강화로 위장할 수 없다",
        "UpdatePolicy", {
            "policy_hash": {"algo": 1, "value": b"\x66" * 32},
            "policy_content": b"max_egress_bps: 0\n",
            "is_relaxation": True,
            "signatures": b"\x22" * 64,
        })

    # 28. 격리 verdict — 다중 서명
    add("v28_quarantine_device",
        "QuarantineDevice — 다중 서명. target_is_coordinator 가 서명 대상이다",
        "QuarantineDevice", {
            "device_id": "01JBXR7Q0000000000000000DD",
            "signals": [
                {"kind": "hash_mismatch", "detail": "checkpoint digest differs",
                 "observed_at_unix_ms": 1_755_103_000_000,
                 "observer_coordinator_id": "coord-a"},
                {"kind": "lease_violation", "detail": "wrote after revoke",
                 "observed_at_unix_ms": 1_755_103_100_000,
                 "observer_coordinator_id": "coord-b"},
            ],
            "target_is_coordinator": True,
            "verdict_signatures": b"\x33" * 64,
        })

    # 29. Coordinator 집합 변경
    add("v29_change_coordinator_set",
        "ChangeCoordinatorSet. new_set 순서는 유지된다 (규칙 d)",
        "ChangeCoordinatorSet", {
            "new_set": [
                {"device_id": "coord-a", "public_key": b"\x01" * 32,
                 "endpoints": ["10.20.20.1:7000"], "failure_domain": "rack-a"},
                {"device_id": "coord-b", "public_key": b"\x02" * 32,
                 "endpoints": ["10.20.20.2:7000"], "failure_domain": "rack-b"},
            ],
            "added_id": "coord-b",
            "removed_id": "",
            "owner_signature": b"\x44" * 64,
        })

    # ==============================================================
    # 30~31 -- 독립 검수(2026-08-16)가 찾은 규칙 i-2 . c-2
    #
    # * 이 벡터들이 없으면 두 구현이 **똑같이 틀린 채로** 통과한다.
    #   실제로 그런 상태였다.
    # ==============================================================

    # 30. 규칙 i-2 -- 중첩 메시지가 서명 필드만 가지면 필드 자체를 생략한다
    g_base = _grant(bytes([0xAA]) * 64, {"algo": 1, "value": bytes([0x01]) * 32})
    g_empty_manifest = dict(g_base)
    g_empty_manifest["manifest"] = {}
    g_sig_only = dict(g_base)
    g_sig_only["manifest"] = {"submitter_signature": bytes([0xAA]) * 64}

    c_e = add("v30a_nested_empty_message_omitted",
              "중첩 메시지가 비면 필드를 생략한다 (규칙 b)",
              "ExecutionGrant", g_empty_manifest,
              ["MUST_EQUAL:v30b_nested_signature_only_message_omitted"])
    c_s = add("v30b_nested_signature_only_message_omitted",
              "* 규칙 i-2 -- 중첩 메시지가 **서명 필드만** 가져도 생략한다. "
              "v30a 와 canonical 이 같아야 한다. 다르면 서명 필드가 canonical 에 "
              "영향을 준다는 뜻이다",
              "ExecutionGrant", g_sig_only,
              ["MUST_EQUAL:v30a_nested_empty_message_omitted"])
    assert c_e == c_s, "규칙 i-2 위반 -- 서명 필드가 canonical 에 새어나갔다"

    # 31. 규칙 c-2 -- map 엔트리 안에서도 규칙 b 를 적용한다
    m_empty_val = _minimal_manifest()
    m_empty_val["env_vars"] = {"EMPTY": "", "SET": "v"}
    c_map = add("v31_map_entry_empty_value",
                "* 규칙 c-2 -- map 값이 빈 문자열이면 엔트리 안의 field 2 를 생략한다. "
                "키의 존재 자체는 정보이므로 엔트리는 남는다",
                "JobManifest", m_empty_val)

    m_absent = _minimal_manifest()
    m_absent["env_vars"] = {"SET": "v"}
    c_absent = add("v31b_map_key_absent",
                   "빈 값 엔트리와 키 부재는 다르다 -- v31 과 canonical 이 달라야 한다",
                   "JobManifest", m_absent,
                   ["MUST_DIFFER:v31_map_entry_empty_value"])
    assert c_map != c_absent, "빈 값 엔트리가 키 부재와 구분되지 않는다"

    # 32. AgentGrantAck — coordinator/agent 핸드셰이크(2026-08-18)가 추가한
    #   단수명 메시지. nonce(7)는 replay 캐시 대상이라 canonical 에 포함돼야
    #   한다. 이전까지 이 메시지는 참조 구현 대조를 한 번도 받은 적이 없었다
    #   (CLAUDE.md 백로그 6번, DoD-05 schema v2 승격 재검수에서 발견).
    _agent_grant_ack_full = {
        "schema_version": 1,
        "grant_id": "01JBXGRANT0000000000000001",
        "attempt_id": "01JBXATTEMPT000000000000001",
        "agent_device_id": "agent-1",
        "issued_at_unix_ms": 1_755_103_900_000,
        "expires_at_unix_ms": 1_755_103_960_000,
        "nonce": bytes(range(16)),
        "accepted": True,
        "agent_signature": b"\x99" * 64,
    }
    _gap_ack = missing_from_full("AgentGrantAck", _agent_grant_ack_full)
    assert not _gap_ack, "v32 가 전 필드를 채우지 않았다: %s" % ", ".join(_gap_ack)
    add("v32_agent_grant_ack",
        "AgentGrantAck. 모든 필드(서명 제외) — field number 오름차순 (규칙 a). "
        "nonce(7)는 canonical 에 포함된다. 서명 필드(90)는 규칙 i 로 제외된다",
        "AgentGrantAck", _agent_grant_ack_full)

    # 32b. nonce 를 바꾸면 canonical 이 달라야 한다 — 서명 밖이면
    #   재전송 시 nonce 만 갈아끼워 replay 캐시를 우회할 수 있다
    #   (RenewLeaseRequest 의 negative_tests 항목과 같은 이유).
    _agent_grant_ack_diff_nonce = dict(_agent_grant_ack_full, nonce=bytes(range(16, 32)))
    c_ack1 = add("v32b_agent_grant_ack_different_nonce",
                 "v32 와 nonce 만 다르다 — canonical 이 달라야 한다",
                 "AgentGrantAck", _agent_grant_ack_diff_nonce,
                 ["MUST_DIFFER:v32_agent_grant_ack"])
    c_ack0 = canonical_encode("AgentGrantAck", _agent_grant_ack_full)
    assert c_ack0 != c_ack1, "AgentGrantAck.nonce 가 canonical 에 반영되지 않는다"

    # 33. RenewLeaseResult — Lease 갱신 최소 조각(2026-08-19)이 추가한
    #   단수명 메시지. nested Lease 를 담는다 — outer 서명이 유효해도
    #   nested Lease 서명은 독립 검증 대상이다(규칙 i, signing.md §3).
    #   request_nonce(8)는 RenewLeaseRequest.nonce 를 echo 한다 — replay
    #   캐시 네임스페이스가 signer 별로 분리되므로(§10) 안전하게 재사용된다.
    _renew_result_lease = {
        "schema_version": 1,
        "lease_id": "01JBXLEASE0000000000000001",
        "job_id": "01JBXR7Q0000000000000000AA",
        "attempt_id": "01JBXATT00000000000000001",
        "fence_epoch": 43,
        "coordinator_term": 7,
        "holder_node_id": "node-1",
        "issued_at_unix_ms": 1_755_103_900_000,
        "expires_at_unix_ms": 1_755_103_960_000,
        "coordinator_signature": b"\xCD" * 64,
    }
    _renew_result_full = {
        "outcome": 1,  # RENEW_OUTCOME_RENEWED
        "lease": _renew_result_lease,
        "detail": "ok",
        "retry_after_ms": 0,
        "schema_version": 1,
        "coordinator_id": "coord-1",
        "issued_at_unix_ms": 1_755_103_900_000,
        "request_nonce": bytes(range(16)),
        "coordinator_signature": b"\x77" * 64,
    }
    # retry_after_ms(4) 는 규칙 b 로 0 이면 생략되므로 "전 필드" 검사에서
    # 제외한다 — outcome == RENEWED 에서는 실제로도 항상 0(§4 UNAVAILABLE 전용).
    _gap_renew = [g for g in missing_from_full("RenewLeaseResult", _renew_result_full)
                  if not g.startswith("retry_after_ms(")]
    assert not _gap_renew, "v33 이 전 필드를 채우지 않았다: %s" % ", ".join(_gap_renew)
    add("v33_renew_lease_result",
        "RenewLeaseResult. 모든 필드(서명·retry_after_ms=0 제외) — field number "
        "오름차순 (규칙 a). nested Lease(2)는 자신의 coordinator_signature 를 "
        "포함해 독립적으로 서명된다 — outer 서명과 무관하다(규칙 i)",
        "RenewLeaseResult", _renew_result_full)

    # 33b. request_nonce 를 바꾸면 canonical 이 달라야 한다 — 서명 밖이면
    #   재전송 시 nonce 만 갈아끼워 replay 캐시를 우회할 수 있다.
    _renew_result_diff_nonce = dict(_renew_result_full, request_nonce=bytes(range(16, 32)))
    c_rr1 = add("v33b_renew_lease_result_different_nonce",
                "v33 과 request_nonce 만 다르다 — canonical 이 달라야 한다",
                "RenewLeaseResult", _renew_result_diff_nonce,
                ["MUST_DIFFER:v33_renew_lease_result"])
    c_rr0 = canonical_encode("RenewLeaseResult", _renew_result_full)
    assert c_rr0 != c_rr1, "RenewLeaseResult.request_nonce 가 canonical 에 반영되지 않는다"

    # 33c. nested Lease 의 서명만 바꿔도 outer canonical 은 바뀌지 않는다
    #   (규칙 i — 서명 필드는 canonical 에서 제외되고, 그 배제는 재귀적으로
    #   nested message 에도 적용된다). outer RenewLeaseResult 서명이 유효
    #   해도 nested Lease 서명은 별도로 위조될 수 있다는 사실의 근거.
    _renew_result_diff_lease_sig = dict(
        _renew_result_full,
        lease=dict(_renew_result_lease, coordinator_signature=b"\xEE" * 64),
    )
    c_rr2 = add("v33c_renew_lease_result_nested_signature_excluded",
                "v33 과 nested Lease.coordinator_signature 만 다르다 — 규칙 i 로 "
                "제외되므로 outer canonical 은 v33 과 같아야 한다",
                "RenewLeaseResult", _renew_result_diff_lease_sig,
                ["MUST_EQUAL:v33_renew_lease_result"])
    assert c_rr0 == c_rr2, "규칙 i 위반 -- nested Lease 서명 필드가 outer canonical 에 새어나갔다"

    # 10. domain_tag 분리 — 같은 canonical, 다른 tag → 다른 sig_input
    _hello = {
        "schema_version": 1, "mode": 2,
        "session_id": "01JBXSESSION00000000000001", "node_id": "node-1",
        "connection_attempt": 2, "issued_at_unix_ms": 1_755_103_900_000,
        "nonce": bytes(range(16)), "node_signature": b"\x11" * 64,
    }
    add("v34_agent_session_hello", "AgentSessionHello RESUME canonical vector", "AgentSessionHello", _hello)

    _resume_request = {
        "schema_version": 1,
        "lease_id": "01JBXLEASE0000000000000001",
        "job_id": "01JBXR7Q0000000000000000AA",
        "attempt_id": "01JBXATT00000000000000001",
        "node_id": "node-1", "fence_epoch": 42,
        "session_id": "01JBXSESSION00000000000001", "connection_attempt": 2,
        "issued_at_unix_ms": 1_755_103_900_000,
        "request_nonce": bytes(range(16, 32)), "node_signature": b"\x22" * 64,
    }
    add("v35_resume_lease_request", "ResumeLeaseRequest canonical vector", "ResumeLeaseRequest", _resume_request)

    _resume_result = {
        "outcome": 1, "lease": _full_lease(), "detail": "resumed",
        "retry_after_ms": 0, "schema_version": 1, "coordinator_id": "coord-a",
        "issued_at_unix_ms": 1_755_103_900_000,
        "request_nonce": bytes(range(16, 32)), "coordinator_signature": b"\x33" * 64,
    }
    add("v36_resume_lease_result", "ResumeLeaseResult RESUMED canonical vector", "ResumeLeaseResult", _resume_result)

    # NodeHeartbeat (2026-08-29). 두 개를 만든다 — fence_epoch 만 다른
    # 대조쌍이다. 그 필드가 canonical 에 실제로 반영되는지를
    # 바이트로 확인하기 위해서다 — 값 하나만 넣으면 그 필드가
    # 빠져도 벍터가 통과한다.
    _heartbeat = {
        "schema_version": 1,
        "node_id": "node-1",
        "device_id": "01JBXDEV00000000000000001",
        "coordinator_device_id": "01JBXCOORD000000000000001",
        "issued_at_unix_ms": 1_755_103_900_000,
        "fence_epoch": 42,
        "running_attempts": 3,
        "request_nonce": bytes(range(16, 32)),
        "node_signature": b"D" * 64,
    }
    add("v37_node_heartbeat", "NodeHeartbeat canonical vector", "NodeHeartbeat", _heartbeat)

    _heartbeat_other_epoch = dict(_heartbeat)
    _heartbeat_other_epoch["fence_epoch"] = 43
    add(
        "v37b_node_heartbeat_different_epoch",
        "NodeHeartbeat with a different fence_epoch — proves the field reaches canonical bytes",
        "NodeHeartbeat",
        _heartbeat_other_epoch,
    )

    base = _minimal_manifest()
    canon = canonical_encode("JobManifest", base)
    si_manifest = sig_input("JobManifest", 1, canon)
    si_lease = sig_input("Lease", 1, canon)
    assert si_manifest != si_lease, "domain separation failed"
    vectors.append({
        "name": "v10_domain_separation",
        "description": "같은 canonical, 다른 domain_tag → sig_input 이 달라야 한다 (§5)",
        "message_type": "JobManifest",
        "canonical_hex": canon.hex(),
        "sig_input_as_manifest_hex": si_manifest.hex(),
        "sig_input_as_lease_hex": si_lease.hex(),
        "checks": ["MUST_DIFFER_INTERNAL"],
    })

    # 11. schema_version 분리
    si_v1 = sig_input("JobManifest", 1, canon)
    si_v2 = sig_input("JobManifest", 2, canon)
    assert si_v1 != si_v2, "schema_version separation failed"
    vectors.append({
        "name": "v11_schema_version_separation",
        "description": "같은 canonical, 다른 schema_version → sig_input 이 달라야 한다 (§4)",
        "message_type": "JobManifest",
        "canonical_hex": canon.hex(),
        "sig_input_v1_hex": si_v1.hex(),
        "sig_input_v2_hex": si_v2.hex(),
        "checks": ["MUST_DIFFER_INTERNAL"],
    })

    # 13. Merkle 홀수 노드 승격
    vectors.append({
        "name": "v13_merkle_promotion",
        "description": "청크 1/2/3개. 홀수 노드는 승격한다 (§6.3)",
        "message_type": "MerkleRoot",
        "roots": {
            "1_chunk": merkle_root_hex([b"a" * 16]),
            "2_chunks": merkle_root_hex([b"a" * 16, b"b" * 16]),
            "3_chunks": merkle_root_hex([b"a" * 16, b"b" * 16, b"c" * 16]),
        },
        "checks": ["REQUIRES_BLAKE3"],
    })

    return vectors


# ═══════════════════════════════════════════════════════════════════════
# Self-test — signing.md §12.2 의 규칙별 검증
# ═══════════════════════════════════════════════════════════════════════

def self_test():
    failures = []

    def check(name, cond, detail=""):
        if cond:
            print("  PASS  %s" % name)
        else:
            print("  FAIL  %s  %s" % (name, detail))
            failures.append(name)

    print("canonical_encode self-test (docs/protocol/signing.md §3)")

    # 규칙 e — 최단 varint
    check("rule_e_minimal_varint_encode",
          encode_varint(0) == b"\x00" and encode_varint(300) == b"\xac\x02")
    try:
        decode_varint(b"\x80\x00", 0)
        check("rule_e_reject_non_minimal", False, "should have raised")
    except ValueError:
        check("rule_e_reject_non_minimal", True)

    # 규칙 b — 기본값 생략
    c1 = canonical_encode("JobManifest", _minimal_manifest())
    z = _minimal_manifest()
    z.update({"deadline_minutes": 0, "preference": 0, "args": [], "env_vars": {}})
    check("rule_b_defaults_omitted", canonical_encode("JobManifest", z) == c1)

    # 규칙 a — 필드 순서
    m = _minimal_manifest()
    reordered = dict(reversed(list(m.items())))
    check("rule_a_field_order_independent",
          canonical_encode("JobManifest", reordered) == c1)

    # 규칙 c — map 정렬
    a = _minimal_manifest(); a["env_vars"] = {"z": "1", "a": "2"}
    b = _minimal_manifest(); b["env_vars"] = {"a": "2", "z": "1"}
    check("rule_c_map_sorted",
          canonical_encode("JobManifest", a) == canonical_encode("JobManifest", b))

    # 규칙 d — repeated 순서 유지
    a = _minimal_manifest(); a["args"] = ["x", "y"]
    b = _minimal_manifest(); b["args"] = ["y", "x"]
    check("rule_d_repeated_order_preserved",
          canonical_encode("JobManifest", a) != canonical_encode("JobManifest", b))

    # 규칙 i — 서명 필드 제외
    s = _minimal_manifest(); s["submitter_signature"] = b"\x01" * 64
    check("rule_i_signature_excluded", canonical_encode("JobManifest", s) == c1)

    # 결정론성 — 100회 반복
    full = _full_manifest()
    encs = {canonical_encode("JobManifest", full) for _ in range(100)}
    check("determinism_100_iterations", len(encs) == 1)

    # domain separation
    canon = canonical_encode("JobManifest", _minimal_manifest())
    check("domain_separation",
          sig_input("JobManifest", 1, canon) != sig_input("Lease", 1, canon))

    # schema_version separation
    check("schema_version_separation",
          sig_input("JobManifest", 1, canon) != sig_input("JobManifest", 2, canon))

    # domain_tag 길이
    check("domain_tag_len_32", all(len(domain_tag(k)) == 32 for k in DOMAIN_TAGS))

    # sig_input 구조
    si = sig_input("JobManifest", 1, canon)
    check("sig_input_layout",
          si[:32] == domain_tag("JobManifest")
          and si[32:36] == uint32_be(1)
          and si[36:40] == uint32_be(len(canon))
          and si[40:] == canon)

    print()
    if failures:
        print("FAILED: %d" % len(failures))
        return 1
    print("all checks passed")
    if not HAVE_BLAKE3:
        print("NOTE: blake3 not installed. digest fields will be null.")
        print("      Run `pip install blake3` and regenerate to complete the vectors.")
    return 0


def verify_vectors(path):
    """
    저장된 벡터가 **지금 이 구현이 내는 값과 같은지** 확인한다.

    ★ 2026-08-16 시정. 처음에는 MUST_EQUAL/MUST_DIFFER 관계만 검사했다.
      독립 검수가 지적했다 — 그러면 **저장된 값이 구현과 어긋나도 통과한다.**
      "cross-checks: OK" 가 실제보다 강한 보증처럼 읽혔다.

    이제 두 가지를 한다.
      1. 재생성 대조 — build_vectors() 를 다시 돌려 저장본과 바이트 비교
      2. 관계 검사 — MUST_EQUAL / MUST_DIFFER
    """
    with open(path, encoding="utf-8") as f:
        doc = json.load(f)
    ok = True

    # 1. ★ 재생성 대조
    fresh = {v["name"]: v for v in build_vectors()}
    stored = {v["name"]: v for v in doc["vectors"]}

    missing = sorted(set(fresh) - set(stored))
    extra = sorted(set(stored) - set(fresh))
    if missing:
        print("FAIL 저장본에 없는 벡터: %s" % ", ".join(missing))
        ok = False
    if extra:
        print("FAIL 구현이 더 이상 내지 않는 벡터: %s" % ", ".join(extra))
        ok = False

    drift = []
    for name in sorted(set(fresh) & set(stored)):
        for field in ("canonical_hex", "sig_input_hex", "sig_input_blake3_256", "roots"):
            if fresh[name].get(field) != stored[name].get(field):
                drift.append("%s.%s" % (name, field))
    if drift:
        print("FAIL 저장본이 구현과 어긋난다 (재생성 필요): %s" % ", ".join(drift))
        ok = False
    else:
        print("재생성 대조: %d개 벡터 일치" % len(fresh))

    # 2. 관계 검사
    by_name = stored
    for v in doc["vectors"]:
        for c in v.get("checks", []):
            if c.startswith("MUST_EQUAL:"):
                other = by_name[c.split(":", 1)[1]]
                if v.get("canonical_hex") != other.get("canonical_hex"):
                    print("FAIL %s MUST_EQUAL %s" % (v["name"], other["name"]))
                    ok = False
            elif c.startswith("MUST_DIFFER:"):
                other = by_name[c.split(":", 1)[1]]
                if v.get("canonical_hex") == other.get("canonical_hex"):
                    print("FAIL %s MUST_DIFFER %s" % (v["name"], other["name"]))
                    ok = False
    print("vector cross-checks: %s" % ("OK" if ok else "FAILED"))
    return 0 if ok else 1


def main():
    # Windows 콘솔(cp949)에서 비ASCII 출력이 죽지 않게 한다.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--emit-vectors", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--verify", metavar="PATH")
    args = ap.parse_args()

    if args.self_test:
        return self_test()
    if args.verify:
        return verify_vectors(args.verify)
    if args.emit_vectors:
        doc = {
            "spec": "docs/protocol/signing.md",
            "spec_version": "v1",
            "generator": "tools/canonical/reference_canonical.py",
            "blake3_available": HAVE_BLAKE3,
            "note": (
                "이 파일이 canonical bytes 의 유일한 규범 원본이다. "
                "손으로 수정하지 말 것. blake3_available=false 이면 "
                "digest 필드는 미완성이며 blake3 설치 후 재생성해야 한다."
            ),
            "vectors": build_vectors(),
        }
        json.dump(doc, sys.stdout, indent=2, ensure_ascii=False)
        sys.stdout.write("\n")
        return 0

    ap.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())
