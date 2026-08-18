#!/usr/bin/env python3
"""
gPUteer canonical schema 정합성 검사기.

`reference_canonical.py` 의 `SCHEMAS` 딕셔너리는 각 서명 대상
메시지의 필드 표를 **손으로** 옮겨 적은 것이다("실제 .proto 를
파싱하지 않고 필드 표를 손으로 둔다", `reference_canonical.py:81`) —
`.proto` 가 바뀌어도 자동으로 동기화되지 않는다. 이 스크립트는
`protoc --descriptor_set_out` 으로 뽑은 `FileDescriptorSet` 을
구조적으로 파싱해 `SCHEMAS` 와 대조한다.

정규식으로 `.proto` 를 직접 파싱하지 않는 이유: 주석·문자열·nested
message·map synthetic entry·oneof 에 취약하고, protobuf 문법이
확장될 때마다 파서를 고쳐야 한다. `descriptor_pb2` 는 protoc 자신이
만드는 정규 출력이라 이런 문제가 없다
(설계: 코덱스 `p119`, `docs/plans/2026-08-20_0000_check_schema_py_v1.md`).

사용법:
    python tools/canonical/check_schema.py
    python tools/canonical/check_schema.py --json

종료 코드:
    0 — schema 오류 없음 (경고는 허용)
    1 — schema mismatch
    2 — 실행 환경 오류 (protoc 없음, google.protobuf 없음, 컴파일 실패 등)
"""

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    from google.protobuf import descriptor_pb2, message
except ImportError:
    print(
        "google.protobuf 가 설치돼 있지 않다 — pip install protobuf",
        file=sys.stderr,
    )
    sys.exit(2)

REPO_ROOT = Path(__file__).resolve().parents[2]
PROTO_DIR = REPO_ROOT / "proto"
PROTO_FILES = [
    "common.proto",
    "job.proto",
    "lease.proto",
    "artifact.proto",
    "control.proto",
]
PACKAGE_PREFIX = "gputeer.v1."

# ★ canonical_encode() 가 field number 로만 무조건 이 필드를 건너뛴다
#   (reference_canonical.py:546, :877) — 서명 필드의 선언된 kind(예:
#   `bytes` vs 실제 `repeated bytes`)는 실제 인코딩에 전혀 영향을 주지
#   않으므로 타입 비교에서 제외한다(SCHEMAS 를 건드리지 않고 이
#   사실을 검사기 쪽에서 명시한다). 필드 번호·이름 일치는 그대로
#   검사한다 — 번호가 같은데 이름이 다르면 그건 여전히 진짜 결함이다.
SIGNATURE_FIELD_NUMBER = 90

sys.path.insert(0, str(Path(__file__).resolve().parent))
import reference_canonical as ref  # noqa: E402

TYPE_ENUM = descriptor_pb2.FieldDescriptorProto
FIELD_TYPE_MAP = {
    TYPE_ENUM.TYPE_UINT32: "uint",
    TYPE_ENUM.TYPE_UINT64: "uint",
    TYPE_ENUM.TYPE_INT32: "int",
    TYPE_ENUM.TYPE_INT64: "int",
    TYPE_ENUM.TYPE_BOOL: "bool",
    TYPE_ENUM.TYPE_STRING: "string",
    TYPE_ENUM.TYPE_BYTES: "bytes",
    TYPE_ENUM.TYPE_ENUM: "enum",
    TYPE_ENUM.TYPE_MESSAGE: "message",
}


def find_protoc() -> str:
    protoc = shutil.which("protoc")
    if not protoc:
        print("protoc 를 PATH 에서 찾지 못했다", file=sys.stderr)
        sys.exit(2)
    return protoc


def build_descriptor_set(protoc: str) -> descriptor_pb2.FileDescriptorSet:
    """`protoc --descriptor_set_out` 으로 실제 .proto 를 컴파일해
    구조적 표현을 얻는다. 실패하면 exit(2) — schema mismatch(exit 1)
    와 실행 환경 오류를 구분한다.

    ★ 코덱스 독립 검수(2026-08-20, p120) 지적 — 처음 구현은 임시
      파일 생성(`NamedTemporaryFile`)이 바깥의 `try` 밖에 있었고,
      `read_bytes()`/`MergeFromString()` 의 `OSError`·디코드 오류도
      전혀 잡지 않았다. 이 경로들이 실패하면(예: 샌드박스가 임시
      디렉터리 쓰기를 막는 경우) 처리되지 않은 예외가 그대로 새어나가
      **exit(1)** 로 끝났다 — schema mismatch(정상적으로 exit 1) 와
      환경 오류를 구분할 수 없게 되는, 이 함수 docstring 이 약속한
      계약 위반이었다. descriptor 생성·읽기·파싱 전 구간을 감싸
      `OSError`/`message.DecodeError` 를 모두 exit(2) 로 통일했다.
    """
    try:
        with tempfile.NamedTemporaryFile(suffix=".pb", delete=False) as tmp:
            tmp_path = Path(tmp.name)
    except OSError as e:
        print(f"임시 descriptor 파일을 만들 수 없다: {e}", file=sys.stderr)
        sys.exit(2)

    try:
        try:
            result = subprocess.run(
                [
                    protoc,
                    f"--proto_path={PROTO_DIR}",
                    "--include_imports",
                    f"--descriptor_set_out={tmp_path}",
                    *PROTO_FILES,
                ],
                capture_output=True,
                text=True,
            )
        except OSError as e:
            print(f"protoc 실행 실패: {protoc} — {e}", file=sys.stderr)
            sys.exit(2)

        if result.returncode != 0:
            print(f"protoc 컴파일 실패:\n{result.stderr}", file=sys.stderr)
            sys.exit(2)

        try:
            data = tmp_path.read_bytes()
        except OSError as e:
            print(f"descriptor 파일을 읽을 수 없다: {e}", file=sys.stderr)
            sys.exit(2)

        fds = descriptor_pb2.FileDescriptorSet()
        try:
            fds.MergeFromString(data)
        except message.DecodeError as e:
            print(f"descriptor 파싱 실패 — protoc 출력이 손상됐다: {e}", file=sys.stderr)
            sys.exit(2)
    finally:
        tmp_path.unlink(missing_ok=True)

    return fds


def short_name(full_name: str) -> str:
    """`.gputeer.v1.Foo.Bar` -> `Bar`."""
    return full_name.rsplit(".", 1)[-1]


def index_messages(fds: descriptor_pb2.FileDescriptorSet):
    """짧은 메시지 이름 -> (DescriptorProto, 정의된 .proto 파일 이름).

    `nested_type` 도 재귀적으로 포함한다 — `map<string, string>` 필드는
    protoc 가 `map_entry` 옵션이 켜진 synthetic nested message(예:
    `JobManifest.EnvVarsEntry`)를 만들고, 필드의 `type_name` 은 그
    nested message 를 가리킨다. 최상위만 인덱싱하면 map 필드를
    `message`/`repeated_message` 로 오판한다.

    이 저장소의 최상위 메시지는 전부 non-nested 라 짧은 이름으로
    충분하다. 혹시 이름이 겹치면 나중 것이 앞선 것을 덮어쓴다 — 이
    저장소 규모에서는 발생하지 않는다.
    """
    index = {}

    def walk(msg, file_name):
        index[msg.name] = (msg, file_name)
        for nested in msg.nested_type:
            walk(nested, file_name)

    for file_proto in fds.file:
        for msg in file_proto.message_type:
            walk(msg, file_proto.name)
    return index


def normalize_field(field: descriptor_pb2.FieldDescriptorProto, msg_index: dict):
    """디스크립터 필드 하나를 SCHEMAS 표기 규칙(`kind`, `nested_ref`)으로
    정규화한다."""
    base_kind = FIELD_TYPE_MAP.get(field.type)
    is_repeated = field.label == descriptor_pb2.FieldDescriptorProto.LABEL_REPEATED

    if base_kind == "message":
        nested_short = short_name(field.type_name.lstrip("."))
        nested_msg, _ = msg_index.get(nested_short, (None, None))
        is_map_entry = (
            nested_msg is not None
            and nested_msg.options is not None
            and nested_msg.options.map_entry
        )
        if is_map_entry:
            key_field = next((f for f in nested_msg.field if f.name == "key"), None)
            value_field = next((f for f in nested_msg.field if f.name == "value"), None)
            if (
                key_field is not None
                and value_field is not None
                and key_field.type == TYPE_ENUM.TYPE_STRING
                and value_field.type == TYPE_ENUM.TYPE_STRING
            ):
                return "map_ss", None
            return "map_?", None
        if is_repeated:
            return "repeated_message", nested_short
        return "message", nested_short

    if base_kind == "string" and is_repeated:
        return "repeated_string", None

    if is_repeated:
        # 이 저장소는 지금까지 repeated_string/repeated_message 외의
        # repeated 스칼라를 SCHEMAS 에 선언한 적이 없다 — 정직하게
        # "알려지지 않은 kind" 로 보고해 대조 단계에서 불일치로
        # 잡히게 한다(단, SIGNATURE_FIELD_NUMBER 는 타입 비교 자체를
        # 건너뛴다).
        return f"repeated_{base_kind}", None

    return base_kind, None


def normalize_message_fields(msg: descriptor_pb2.DescriptorProto, msg_index: dict):
    """field_number -> (name, kind, nested_ref)."""
    out = {}
    for field in msg.field:
        kind, nested = normalize_field(field, msg_index)
        out[field.number] = (field.name, kind, nested)
    return out


def compare_message(msg_name: str, schema_fields, proto_fields: dict):
    errors = []
    infos = []
    schema_numbers = {f[0] for f in schema_fields}

    for number, name, kind, nested in schema_fields:
        if number not in proto_fields:
            errors.append(
                {
                    "kind": "missing-in-proto",
                    "message": msg_name,
                    "field": name,
                    "number": number,
                    "detail": "SCHEMAS 에는 있으나 .proto 에 없다",
                }
            )
            continue

        p_name, p_kind, p_nested = proto_fields[number]
        if name != p_name:
            errors.append(
                {
                    "kind": "field-name",
                    "message": msg_name,
                    "field": name,
                    "number": number,
                    "detail": f"이름 불일치: schema={name!r} proto={p_name!r}",
                }
            )
            continue

        if number == SIGNATURE_FIELD_NUMBER:
            # ★ 서명 필드 — canonical_encode() 가 번호로만 건너뛴다.
            #   선언된 kind 가 실제와 달라도 인코딩에 영향 없다.
            continue

        if kind != p_kind:
            errors.append(
                {
                    "kind": "field-type",
                    "message": msg_name,
                    "field": name,
                    "number": number,
                    "detail": f"타입 불일치: schema={kind} proto={p_kind}",
                }
            )
            continue

        if kind in ("message", "repeated_message") and nested != p_nested:
            errors.append(
                {
                    "kind": "nested-ref",
                    "message": msg_name,
                    "field": name,
                    "number": number,
                    "detail": f"nested 참조 불일치: schema={nested} proto={p_nested}",
                }
            )

    for number, (p_name, p_kind, p_nested) in proto_fields.items():
        if number in schema_numbers:
            continue
        if p_name in ref.DERIVED_HASH_FIELDS:
            infos.append(
                {
                    "kind": "derived-field",
                    "message": msg_name,
                    "field": p_name,
                    "number": number,
                    "detail": "derived hash field — SCHEMAS 의도적 제외",
                }
            )
        else:
            errors.append(
                {
                    "kind": "missing-in-schema",
                    "message": msg_name,
                    "field": p_name,
                    "number": number,
                    "detail": ".proto 에는 있으나 SCHEMAS 에 없다",
                }
            )

    return errors, infos


def run_check():
    protoc = find_protoc()
    fds = build_descriptor_set(protoc)
    msg_index = index_messages(fds)

    errors = []
    infos = []
    warnings = []

    for msg_name, schema_fields in ref.SCHEMAS.items():
        if msg_name not in msg_index:
            errors.append(
                {
                    "kind": "missing-message",
                    "message": msg_name,
                    "field": None,
                    "number": None,
                    "detail": "SCHEMAS 메시지가 .proto 에 없다",
                }
            )
            continue
        descriptor, _file_name = msg_index[msg_name]
        proto_fields = normalize_message_fields(descriptor, msg_index)
        e, i = compare_message(msg_name, schema_fields, proto_fields)
        errors.extend(e)
        infos.extend(i)

    # ★ map<string,string> 같은 필드가 만드는 synthetic map-entry
    #   nested message(예: JobManifest.EnvVarsEntry)는 사용자가 쓰는
    #   실제 메시지가 아니다 — proto-only 경고 목록에서 제외한다.
    real_messages = {
        name
        for name, (descriptor, _file) in msg_index.items()
        if not (descriptor.options is not None and descriptor.options.map_entry)
    }
    proto_only = sorted(real_messages - set(ref.SCHEMAS))
    for name in proto_only:
        warnings.append(
            {
                "kind": "proto-only-message",
                "message": name,
                "detail": ".proto 메시지가 SCHEMAS 에 없다 (canonical 대상이 아닐 수 있다)",
            }
        )

    return {
        "ok": len(errors) == 0,
        "errors": errors,
        "infos": infos,
        "warnings": warnings,
        "counts": {
            "schema_messages": len(ref.SCHEMAS),
            "proto_messages": len(real_messages),
        },
    }


def print_human(result: dict) -> None:
    print(f"schema 검사 — {PROTO_DIR}")
    print("=" * 64)
    for e in result["errors"]:
        loc = f"{e['message']}.{e['field']}" if e["field"] else e["message"]
        num = f" #{e['number']}" if e["number"] is not None else ""
        print(f"  FAIL {e['kind']:<20} {loc}{num}")
        print(f"        {e['detail']}")
    for i in result["infos"]:
        loc = f"{i['message']}.{i['field']}"
        print(f"  INFO {i['kind']:<20} {loc} #{i['number']}")
        print(f"        {i['detail']}")
    for w in result["warnings"]:
        print(f"  ~ WARN {w['kind']:<18} {w['message']}")
    print("-" * 64)
    print(
        f"오류 {len(result['errors'])}건, 경고 {len(result['warnings'])}건 "
        f"(SCHEMAS {result['counts']['schema_messages']}개 메시지 / "
        f".proto {result['counts']['proto_messages']}개 메시지)"
    )
    print("schema 검사 " + ("통과" if result["ok"] else "실패"))


def main() -> int:
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="JSON 으로 출력한다")
    args = parser.parse_args()

    result = run_check()

    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        print_human(result)

    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
