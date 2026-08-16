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

# kind: uint | bool | enum | string | bytes | message | map_ss
#       repeated_string | repeated_message
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
}

# 규칙 i: canonical 인코딩에서 항상 제외되는 필드 번호
SIGNATURE_FIELD_NUMBER = 90
# 규칙 i: 도출 해시 필드 (메시지 안에 있으면 안 되지만, 방어적으로 목록화)
DERIVED_HASH_FIELDS = {"manifest_hash"}


def is_default(kind: str, value) -> bool:
    """signing.md 규칙 b — 기본값은 출력하지 않는다."""
    if value is None:
        return True
    if kind in ("uint", "enum"):
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
                inner = encode_len_delimited(1, kb) + encode_len_delimited(2, vb)
                out += encode_len_delimited(number, inner)

        elif kind == "message":
            # 규칙 f — 재귀 적용
            inner = canonical_encode(nested, value)
            out += encode_len_delimited(number, inner)

        elif kind == "repeated_message":
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
    "ExecutionGrant": b"gputeer/v1/grant",
    "Lease": b"gputeer/v1/lease",
    "RenewLeaseRequest": b"gputeer/v1/lease-renew",
    "RevokeLeaseNotice": b"gputeer/v1/lease-revoke",
    "CheckpointManifest": b"gputeer/v1/checkpoint",
    "ReplicaAck": b"gputeer/v1/replica-ack",
    "ArtifactRef": b"gputeer/v1/artifact",
    "AttemptReport": b"gputeer/v1/attempt-report",
    "CanonicalDecision": b"gputeer/v1/canonical",
    "Genesis": b"gputeer/v1/genesis",
    "Membership": b"gputeer/v1/membership",
    "Policy": b"gputeer/v1/policy",
    "Quarantine": b"gputeer/v1/quarantine",
    "Audit": b"gputeer/v1/audit",
    "Release": b"gputeer/v1/release",
    "Invite": b"gputeer/v1/invite",
}

DOMAIN_TAG_LEN = 32


def domain_tag(name: str) -> bytes:
    """32바이트, 우측 0x00 패딩."""
    raw = DOMAIN_TAGS[name]
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

    # 10. domain_tag 분리 — 같은 canonical, 다른 tag → 다른 sig_input
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
    with open(path, encoding="utf-8") as f:
        doc = json.load(f)
    ok = True
    by_name = {v["name"]: v for v in doc["vectors"]}
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
