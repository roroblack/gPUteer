#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
docs/evidence/ 의 front-matter 를 기계 검사한다.

"파일이 있다" 가 아니라 "재현 가능한 실험 기록이 완전하다" 를 검사한다.
파일 존재만 세면 빈 파일도 통과하기 때문이다.

규범: ../RULE.md §7

사용법:
    python scripts/verify_evidence.py
    python scripts/verify_evidence.py --json
"""
import argparse
import hashlib
import io
import json
import os
import re
import subprocess
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EVIDENCE_DIR = os.path.join(REPO_ROOT, "docs", "evidence")
RAW_DIR_PREFIX = "docs/evidence/_raw/"
GRANDFATHER_LIST = os.path.join(EVIDENCE_DIR, "_schema_v1_grandfathered.txt")

# ★ 유예 목록의 **고정 digest** (독립 검수 2026-08-17 · 치명).
#
#   전에는 목록 파일에 "추가 금지" 라고 **적어만** 뒀다.
#   검사기는 주석 아닌 모든 줄을 그대로 믿었다 —
#   목록에 한 줄 추가하면 새 v1 evidence 가 검수 없이 통과했다.
#   검수자가 실제로 재현했다.
#
#   ★ 강제 장치가 없는 규범은 규범이 아니라 희망이다.
#
#   지금은 목록 파일이 바뀌면 이 상수도 바꿔야 한다.
#   그러면 유예 확대가 **코드 diff 에 드러난다.**
#   (완전한 위조 방지는 아니다 — 둘 다 고치면 통과한다.
#    목적은 '조용히 늘어나는 것' 을 막는 것이다.)
GRANDFATHER_DIGEST = "sha256:51a568cbc91cf0e1f34398f1eb3969cbfa900d6e7f70a0601961a2f3329e9053"

REQUIRED = [
    "id", "claim", "status", "commit", "binary_digests", "protocol_versions",
    "platform", "hardware", "network_profile", "command", "raw_output",
    "artifacts", "negative_tests", "limitations", "decision",
]

# ══════════════════════════════════════════════════════════════════════
# schema v2 — provenance · 독립 검수 · 원문 무결성
#   (2026-08-17. 독립 검수와의 설계 논의 결과. ADR-030)
# ══════════════════════════════════════════════════════════════════════
#
# ★ v2 가 **보장하는 것**과 **보장하지 않는 것**을 먼저 적는다.
#   보장하지 않는 것을 적지 않으면 "검수를 강제했다" 가 "주장이 참이다" 로 읽힌다.
#
#   보장한다    검수 기록이 빠졌는가 · 검수자와 실행자가 같은 것으로 적혔는가
#               · `_raw/` 원문이 digest 기록 이후 손으로 바뀌었는가
#
#   보장 못 한다 명령이 실제로 실행됐는가 · 원문이 그 명령의 출력인가
#               · 검수자가 정직했는가 · 두 모델이 정말 독립인가
#               · 작성자가 원문과 digest 를 **함께** 고쳤는가
#
#   위조를 정말 막으려면 Git 서명이나 외부 실행 시스템이 필요하다.
#   개발자 1명 · 로컬 실행 환경에는 과하다 — 그래서 여기까지다.

V2_FIELDS = [
    "executor_id", "executor_tool", "executed_at",
    "reviewer_id", "reviewer_tool", "review_context",
    "review_outcome", "review_scope", "review_artifact",
    "raw_output_artifact", "raw_output_digest", "raw_output_bytes",
]

# 독립 검수를 **강제**하는 대상. RULE.md §7.3
#   ENV-* 같은 단순 환경 기록까지 검수시키면 과하다 —
#   규칙이 과하면 우회하게 된다.
REVIEW_REQUIRED_PREFIX = ("P0-", "DoD-")

ID_PREFIXES = ("human:", "agent:", "service:")

# ★ digest 는 SHA-256 이다. BLAKE3 가 아니다.
#
#   내 최초 계획은 BLAKE3 였다 — 저장소 나머지가 BLAKE3 를 쓰기 때문이다.
#   **틀렸다.** Python 표준 라이브러리에 BLAKE3 가 없다.
#   이 검사기는 stdlib 만 쓴다는 조건이 있고, 그것을 깨면
#   검사기를 못 돌리는 환경이 생긴다 — 검사기를 못 돌리면 규칙도 없다.
#
#   증거 파일의 **수기 변조 검출**에는 SHA-256 이면 충분하다.
#   프로토콜 digest 는 Rust 에서 계속 BLAKE3 다. 용도가 다르다.
DIGEST_RE = re.compile(r"^sha256:([0-9a-f]{64})$")

# ★ 보이지 않는 문자 (독립 검수 2026-08-17 · 중대).
#
#   `reviewer_id: "agent:claude-code​"` 는 사람 눈에 executor_id 와
#   **똑같이 보이지만** 문자열 비교에서는 다르다.
#   자기 검수 검사가 그대로 뚫린다. 검수자가 실제로 재현했다.
#
#   제로폭·서식 문자·제어문자를 신원 필드에서 거부한다.
INVISIBLE_RE = re.compile("[" + "".join(chr(c) for c in list(range(0x00, 0x20)) + list(range(0x7f, 0xa0)) + list(range(0x200b, 0x2010)) + list(range(0x2028, 0x202f)) + list(range(0x2060, 0x2070)) + [0xfeff]) + "]")

# evidence id 는 ASCII 만 허용한다.
#   `DоD-09` 의 `о` 가 키릴 문자여도 사람은 `DoD-09` 로 읽는다.
#   그러면 검수 강제 대상 판정(`DoD-` 접두사)을 조용히 피한다.
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")

VALID_STATUS = {
    "PASS", "FAIL-ARCHITECTURE", "FAIL-SCOPE",
    "INCONCLUSIVE", "ENVIRONMENT-BLOCKED", "SUPERSEDED",
}

# PASS 로 계상되는 것은 PASS 뿐이다. RULE.md §7.1
COUNTS_AS_PASS = {"PASS"}

PLACEHOLDER = re.compile(
    r"^\s*(\(.*\)|TODO|TBD|-|N/A|0{8,}|blake3:0{3,}\.{0,3}\"?;?)\s*$", re.I)


def parse_front_matter(text):
    """--- 로 감싼 YAML 유사 블록을 최소 파싱한다 (외부 의존성 없이)."""
    if text.startswith("﻿"):
        # ★ BOM 은 명시적으로 거부한다 (독립 검수 2026-08-17).
        #   지금도 `---` 검사가 실패해 거부되지만, 이유가 "front-matter 없음" 으로
        #   나와 원인을 엉뚱한 곳에서 찾게 된다.
        return None, "파일이 BOM 으로 시작한다 — UTF-8(BOM 없음)으로 저장하라"
    if not text.startswith("---"):
        return None, "front-matter 없음 (--- 로 시작해야 한다)"
    # ★ 종료 구분자는 **한 줄 전체가 `---`** 여야 한다 (독립 검수 2026-08-17).
    #   전에는 `text.find("\n---", 3)` 이어서 본문의 `---not-a-delimiter` 도
    #   종료로 봤다. front-matter 가 조기 종료되면 뒤의 필드가 통째로 사라진다.
    m_end = re.search(r"(?m)^---[ \t]*$", text[3:])
    if not m_end:
        return None, "front-matter 가 닫히지 않았다"
    end = 3 + m_end.start()
    block = text[3:end]

    data, key, buf, mode = {}, None, [], None
    # ★ 중복 키를 모은다 (독립 검수 2026-08-17 · 중대).
    #   전에는 마지막 값이 이겼다. 그래서
    #     review_outcome: "CHANGES_REQUESTED"
    #     review_outcome: "ACCEPTED"
    #   가 통과했다 — **사람이 읽는 값과 검사기가 보는 값이 다르다.**
    seen_keys, dup_keys = set(), []
    for raw in block.split("\n"):
        if not raw.strip():
            if mode == "block":
                buf.append("")
            continue
        # 블록 스칼라(| 또는 >) 내부
        if mode == "block":
            if raw.startswith(("  ", "\t")):
                buf.append(raw.strip())
                continue
            data[key] = "\n".join(buf).strip()
            mode, buf = None, []
        # 리스트 항목
        if raw.lstrip().startswith("- ") and key is not None and mode == "list":
            data.setdefault(key, []).append(unquote(raw.lstrip()[2:].strip()))
            continue
        # 중첩 매핑 (한 단계만)
        if raw.startswith(("  ", "\t")) and key is not None and mode == "map":
            k2, _, v2 = raw.strip().partition(":")
            if k2:
                data.setdefault(key, {})[k2.strip()] = unquote(v2.strip())
            continue
        m = re.match(r"^([A-Za-z_][A-Za-z0-9_]*):\s*(.*)$", raw)
        if not m:
            continue
        key, val = m.group(1), m.group(2).strip()
        if key in seen_keys:
            dup_keys.append(key)
        seen_keys.add(key)
        if val in ("|", ">", "|-", ">-"):
            mode, buf = "block", []
        elif val == "":
            mode = "pending"      # list 인지 map 인지 다음 줄에서 결정
            data[key] = None
        else:
            mode = None
            data[key] = unquote(val)
        if mode == "pending":
            mode = None
        # 다음 줄 형태로 list/map 결정
    # 블록 스칼라가 끝에서 종료된 경우
    if mode == "block" and key:
        data[key] = "\n".join(buf).strip()

    # 2차 패스: 값이 None 인 키의 하위 항목을 리스트/맵으로 채운다
    lines = block.split("\n")
    for i, raw in enumerate(lines):
        m = re.match(r"^([A-Za-z_][A-Za-z0-9_]*):\s*$", raw)
        if not m:
            continue
        k = m.group(1)
        items, mapping = [], {}
        for nxt in lines[i + 1:]:
            if not nxt.startswith((" ", "\t")):
                break
            s = nxt.strip()
            if not s:
                continue
            if s.startswith("- "):
                items.append(unquote(s[2:].strip()))
            elif ":" in s:
                a, _, b = s.partition(":")
                mapping[a.strip()] = unquote(b.strip())
        if items:
            data[k] = items
        elif mapping:
            data[k] = mapping
    if dup_keys:
        return None, ("front-matter 에 중복 키가 있다: %s — "
                      "사람이 읽는 값과 검사기가 보는 값이 달라진다"
                      % ", ".join(sorted(set(dup_keys))))
    return data, None


def unquote(v):
    """YAML 스칼라의 감싼 따옴표를 벗긴다.

    ★ 2026-08-17 추가. 전에는 벗기지 않아서 `reviewer_id: "agent:x"` 의 값이
      `"agent:x"` (따옴표 포함)였다. 접두사 검사가 전부 오탐이었다.
      **검사기의 오탐은 규칙을 우회하게 만든다** — 그래서 검사기 자체를 고친다.
    """
    if not isinstance(v, str):
        return v
    t = v.strip()
    if len(t) >= 2 and t[0] == t[-1] and t[0] in ("'", '"'):
        return t[1:-1]
    return v


def is_empty(v):
    if v is None:
        return True
    if isinstance(v, str):
        return not v.strip() or bool(PLACEHOLDER.match(v))
    if isinstance(v, (list, dict)):
        if not v:
            return True
        if isinstance(v, list):
            return all(isinstance(x, str) and (not x.strip() or PLACEHOLDER.match(x))
                       for x in v)
        return all(is_empty(x) for x in v.values())
    return False


def commit_exists(h):
    """이 저장소에 실제로 존재하는 커밋인가.

    ★ 2026-08-16 추가. 전에는 40자 hex 형식만 봤다 —
      **존재하지 않는 커밋도 통과했다.**

    git 이 없거나 저장소가 아니면 검사를 건너뛴다(경고 없이 통과).
    검사기 자체가 환경 때문에 실패하면 안 되기 때문이다.
    """
    try:
        r = subprocess.run(
            ["git", "cat-file", "-t", h],
            cwd=REPO_ROOT, capture_output=True, text=True, timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return True  # git 없음 — 검사 불가이므로 통과시킨다
    if r.returncode != 0 and "not a git repository" in (r.stderr or "").lower():
        return True
    return r.stdout.strip() == "commit"


def as_list(v):
    """YAML 리스트/문자열/None 을 문자열 리스트로."""
    if v is None:
        return []
    if isinstance(v, list):
        return [str(x).strip() for x in v]
    return [str(v).strip()]

_SOURCE_CACHE = {}


def repo_sources():
    """`crates` · `tools` · `scripts` 의 소스 전문. `REPO_ROOT` 별로 캐시한다.

    ★ 테스트 하니스가 `REPO_ROOT` 를 임시 디렉터리로 바꾸므로
      캐시를 전역 하나로 두면 **엉뚱한 저장소를 스캔한 결과**를 재사용한다.
      (실제로 그렇게 만들었다가 부정 테스트가 전부 오탐이 됐다.)
    """
    if REPO_ROOT in _SOURCE_CACHE:
        return _SOURCE_CACHE[REPO_ROOT]
    blob = []
    for sub in ("crates", "tools", "scripts"):
        base = os.path.join(REPO_ROOT, sub)
        for root, _dirs, files in os.walk(base):
            if "target" in root or "__pycache__" in root:
                continue
            for f in files:
                if f.endswith((".rs", ".py")):
                    try:
                        blob.append(io.open(os.path.join(root, f),
                                            encoding="utf-8",
                                            errors="ignore").read())
                    except OSError:
                        pass
    _SOURCE_CACHE[REPO_ROOT] = chr(10).join(blob)
    return _SOURCE_CACHE[REPO_ROOT]


def load_grandfathered():
    """schema v1 로 남을 수 있는 파일 목록. `(집합, 오류목록)` 반환.

    ★ 목록이 없으면 **아무것도 유예하지 않는다** (빈 집합).
      "파일이 없으면 전부 통과" 로 만들면 목록을 지우는 것이 규칙 우회가 된다.

    ★ 목록 자체가 `GRANDFATHER_DIGEST` 와 일치해야 한다.
      일치하지 않으면 **아무것도 유예하지 않는다** — 안전한 실패 방향이다.
    """
    if not os.path.exists(GRANDFATHER_LIST):
        return set(), []
    raw = io.open(GRANDFATHER_LIST, "rb").read()
    actual = "sha256:" + hashlib.sha256(raw).hexdigest()
    errs = []
    if GRANDFATHER_DIGEST != actual:
        errs.append(
            "유예 목록이 변경됐다 — 기록 %s / 실제 %s. "
            "유예를 늘리려면 verify_evidence.py 의 GRANDFATHER_DIGEST 도 함께 고쳐야 하며, "
            "그 diff 가 검토 대상이다."
            % (GRANDFATHER_DIGEST, actual)
        )
        return set(), errs      # ★ 유예를 전부 취소한다
    out = set()
    for line in raw.decode("utf-8").splitlines():
        t = line.strip()
        if t and not t.startswith("#"):
            out.add(t)
    return out, errs


def safe_repo_path(rel):
    """저장소 안의 상대 경로인가. (ok, 이유) 반환.

    ★ 2026-08-17 추가 (독립 검수). 전에는 `os.path.exists` 만 봤다 —
      `../../../etc/passwd` 도 존재하면 통과했다.
      artifact 목록은 사람이 손으로 적는 필드다.

    ★ 2026-08-17 2차 강화 (독립 검수가 **실제로 우회를 재현**했다).

      ```text
      docs/evidence/_raw/x.txt::$DATA     NTFS ADS — 통과했다
      docs/evidence/_raw/DOD-09~2.TXT     8.3 단축이름 — 통과했다
      docs/evidence/_raw/link.txt -> 외부  symlink — 차단 로직이 없었다
      ```

      셋 다 "저장소 안의 그 파일" 이 아닌 것을 그 파일로 통과시킨다.
      `realpath` 로 실제 경로를 풀고 저장소 안인지 대조한다.
    """
    if not rel:
        return False, "빈 경로"
    p = rel.replace("\\", "/")
    if INVISIBLE_RE.search(p):
        return False, "경로에 보이지 않는 문자가 있다: %r" % rel
    if p.startswith("/") or re.match(r"^[A-Za-z]:", p):
        return False, "절대 경로는 허용하지 않는다: %s" % rel
    parts = [x for x in p.split("/") if x not in ("", ".")]
    if ".." in parts:
        return False, "경로 탈출(..)은 허용하지 않는다: %s" % rel
    # ★ NTFS ADS — `file.txt::$DATA` 는 같은 내용을 다른 이름으로 읽게 한다.
    if ":" in p:
        return False, "경로에 콜론을 쓸 수 없다 (NTFS ADS): %s" % rel

    full = os.path.join(REPO_ROOT, *parts)
    if not os.path.exists(full):
        return True, None          # 존재 여부는 호출자가 따로 보고한다

    # ★ symlink / junction / 8.3 단축이름을 실제 경로로 푼다.
    real = os.path.realpath(full)
    root = os.path.realpath(REPO_ROOT)
    try:
        if os.path.commonpath([real, root]) != root:
            return False, "저장소 밖을 가리킨다(symlink/junction): %s -> %s" % (rel, real)
    except ValueError:
        return False, "다른 드라이브를 가리킨다: %s -> %s" % (rel, real)

    # 적힌 경로와 실제 경로가 다르면(8.3 단축이름 등) 적힌 대로 쓰게 한다.
    declared = os.path.realpath(os.path.join(root, *parts))
    if os.path.normcase(real) != os.path.normcase(declared):
        return False, "실제 경로가 다르다: %s -> %s" % (rel, real)
    expected = os.path.normcase(os.path.join(root, *parts))
    if os.path.normcase(real) != expected:
        return False, (
            "적힌 경로와 실제 이름이 다르다(8.3 단축이름 등): %s -> %s" % (rel, real)
        )
    return True, None


def sha256_file(full):
    h = hashlib.sha256()
    n = 0
    # ★ 텍스트가 아니라 **바이트**로 읽는다.
    #   텍스트로 읽으면 Windows 개행 변환이 digest 를 바꾼다.
    with io.open(full, "rb") as f:
        while True:
            chunk = f.read(65536)
            if not chunk:
                break
            h.update(chunk)
            n += len(chunk)
    return h.hexdigest(), n


def check_schema_v2(fm, path, errors, warns, grandfathered):
    """schema v2 — provenance · 독립 검수 · 원문 digest.

    RULE.md §7.3 · ADR-030.
    """
    name = os.path.basename(path)
    raw_ver = str(fm.get("schema_version", "1")).strip()
    try:
        ver = int(raw_ver)
    except ValueError:
        errors.append("schema_version 이 정수가 아니다: %r" % raw_ver)
        return

    if ver < 2:
        # ★ 유예 목록에 없는 **새** evidence 는 v1 로 만들 수 없다.
        #   "새 evidence 부터 v2" 라고만 적으면 기계는 "새 것" 을 모른다.
        if name not in grandfathered:
            errors.append(
                "schema_version 1 이지만 유예 목록(docs/evidence/_schema_v1_grandfathered.txt)에 "
                "없다 — 새 evidence 는 v2 여야 한다 (RULE.md §7.3)"
            )
        return

    for f in V2_FIELDS:
        if f not in fm:
            errors.append("v2 필수 필드 누락: %s" % f)
        elif is_empty(fm[f]):
            errors.append("v2 필드가 비었거나 자리표시자: %s" % f)

    ex_id = str(fm.get("executor_id") or "").strip()
    rv_id = str(fm.get("reviewer_id") or "").strip()
    for label, val in (("executor_id", ex_id), ("reviewer_id", rv_id)):
        if not val:
            continue
        if not val.startswith(ID_PREFIXES):
            errors.append(
                "%s 는 %s 중 하나로 시작해야 한다: %r"
                % (label, "/".join(ID_PREFIXES), val)
            )
        # ★ 보이지 않는 문자 (독립 검수 2026-08-17 · 중대. 실제로 재현했다).
        #
        #   `agent:claude-code` 와 `agent:claude-code​` 는 눈으로 같다.
        #   문자열 비교로는 다르다 — **자기 검수 검사가 그대로 뚫린다.**
        if INVISIBLE_RE.search(val):
            errors.append(
                "%s 에 보이지 않는 문자가 있다 — 눈으로 같은 두 신원을 만들 수 있다: %r"
                % (label, val)
            )
        if not val.isascii():
            # 키릴 `о` 같은 동형 문자도 같은 문제를 만든다.
            errors.append("%s 는 ASCII 만 쓸 수 있다: %r" % (label, val))

    status = str(fm.get("status") or "").strip()

    # ★ 검수 강제 대상 판정을 **파일명만으로 하지 않는다** (독립 검수 2026-08-17 · 중대).
    #
    #   전에는 파일명 접두사만 봤다. 파일명을 `ENV-09.md` 로 바꾸고
    #   `id: DoD-09` 를 그대로 두면 검수 강제가 **사라졌다.**
    #   검수자가 실제로 재현했다.
    #
    #   지금은 id 와 파일명 **둘 다** 보고, 둘이 어긋나면 그것부터 오류다.
    ev_id = str(fm.get("id") or "").strip()
    if ev_id and not ID_RE.match(ev_id):
        errors.append(
            "id 에 쓸 수 없는 문자가 있다: %r — "
            "동형 문자(키릴 о 등)로 검수 강제를 피할 수 있다" % ev_id
        )
    if ev_id and not name.startswith(ev_id):
        errors.append(
            "파일명(%s)이 id(%s)로 시작하지 않는다 — "
            "파일명을 바꿔 검수 강제를 피할 수 있다" % (name, ev_id)
        )
    needs_review = status in COUNTS_AS_PASS and (
        name.startswith(REVIEW_REQUIRED_PREFIX)
        or ev_id.startswith(REVIEW_REQUIRED_PREFIX)
    )

    if ex_id and rv_id and ex_id == rv_id:
        # ★ 이것이 v2 의 핵심이다. 자기가 자기 결론을 승인하면 교차검증이 아니다.
        errors.append(
            "executor_id 와 reviewer_id 가 같다(%s) — 자기 검수는 독립 검수가 아니다" % ex_id
        )

    ctx = str(fm.get("review_context") or "").strip()
    if needs_review and ctx and ctx != "fresh-read-only":
        errors.append(
            "review_context 가 %r 이다 — P0/DoD 의 PASS 는 새 컨텍스트 · 읽기 전용 "
            "검수여야 한다 (fresh-read-only)" % ctx
        )

    outcome = str(fm.get("review_outcome") or "").strip()
    if needs_review and outcome != "ACCEPTED":
        errors.append(
            "review_outcome 이 %r 이다 — 검수가 수용되지 않은 것을 PASS 로 셀 수 없다"
            % outcome
        )

    arts = set(as_list(fm.get("artifacts")))

    # ── 검수 receipt ──────────────────────────────────────────────
    rv_art = str(fm.get("review_artifact") or "").strip()
    if rv_art:
        ok, why = safe_repo_path(rv_art)
        rv_norm = rv_art.replace("\\", "/")
        if not ok:
            errors.append("review_artifact: %s" % why)
        elif not rv_norm.startswith(RAW_DIR_PREFIX):
            # ★ 2026-08-17 추가 (독립 검수). 전에는 저장소 안이기만 하면 됐다 —
            #   `scripts/verify_evidence.py` 를 검수 receipt 로 적어도 통과했다.
            #   길이 200자와 `파일:줄` 정규식을 소스 파일이 자연히 만족하기 때문이다.
            errors.append(
                "review_artifact 는 %s 아래여야 한다: %s" % (RAW_DIR_PREFIX, rv_art)
            )
        elif not os.path.exists(os.path.join(REPO_ROOT, rv_norm)):
            errors.append("review_artifact 가 존재하지 않는다: %s" % rv_art)
        elif rv_art not in arts:
            errors.append("review_artifact 가 artifacts 목록에 없다: %s" % rv_art)
        else:
            body = io.open(os.path.join(REPO_ROOT, rv_norm),
                           encoding="utf-8", errors="ignore").read()
            # ★ 형식적 LGTM 을 거른다. 기계가 판정할 수 있는 것은
            #   "검수자가 무엇을 봤는지 적었는가" 까지다 — 정직성은 아니다.
            if len(body.strip()) < 200:
                errors.append(
                    "review_artifact 가 너무 짧다(%d자) — "
                    "'무엇을 공격했고 어디까지 확인했는지' 가 없으면 검수 기록이 아니다"
                    % len(body.strip())
                )
            else:
                # ★ 2026-08-17 강화 (독립 검수가 우회 payload 를 실제로 만들었다).
                #
                #   `("LGTM. " * 40) + "fake.py:1"` 이 통과했다 —
                #   길이도 넘고 `파일:줄` 정규식도 만족한다.
                #
                #   그래서 **가리키는 파일이 실제로 있는지**까지 본다.
                #   존재하지 않는 파일을 인용한 검수는 그 파일을 읽지 않았다.
                cited = re.findall(
                    r"([A-Za-z0-9_][A-Za-z0-9_./\\-]*\.(?:rs|py|md|proto|toml|json)):\d+",
                    body,
                )
                real = set()
                for c in cited:
                    c2 = c.replace("\\", "/").lstrip("./")
                    # 검수자는 `writer.rs:102` 처럼 파일명만 적기도 한다.
                    if os.path.exists(os.path.join(REPO_ROOT, c2)):
                        real.add(c2)
                    elif "/" not in c2:
                        for root, _dirs, files in os.walk(
                            os.path.join(REPO_ROOT, "crates")
                        ):
                            if c2 in files:
                                real.add(c2)
                                break
                if not cited:
                    errors.append(
                        "review_artifact 에 파일:줄 위치가 하나도 없다 — "
                        "구체적 반례 없는 검수는 형식적 승인이다"
                    )
                elif not real:
                    errors.append(
                        "review_artifact 가 인용한 파일이 저장소에 하나도 없다(%s) — "
                        "읽지 않은 검수다" % ", ".join(sorted(set(cited))[:5])
                    )

    # ── 원문 digest ───────────────────────────────────────────────
    raw_art = str(fm.get("raw_output_artifact") or "").strip()
    if raw_art:
        ok, why = safe_repo_path(raw_art)
        norm = raw_art.replace("\\", "/")
        if not ok:
            errors.append("raw_output_artifact: %s" % why)
        elif not norm.startswith(RAW_DIR_PREFIX):
            errors.append(
                "raw_output_artifact 는 %s 아래여야 한다: %s" % (RAW_DIR_PREFIX, raw_art)
            )
        elif not os.path.exists(os.path.join(REPO_ROOT, norm)):
            errors.append("raw_output_artifact 가 존재하지 않는다: %s" % raw_art)
        elif raw_art not in arts:
            errors.append("raw_output_artifact 가 artifacts 목록에 없다: %s" % raw_art)
        else:
            actual, nbytes = sha256_file(os.path.join(REPO_ROOT, norm))
            m = DIGEST_RE.match(str(fm.get("raw_output_digest") or "").strip())
            if not m:
                errors.append(
                    "raw_output_digest 형식이 잘못됐다 — 'sha256:' + 소문자 hex 64자여야 한다"
                )
            elif m.group(1) != actual:
                # ★ 이것이 잡는 것: digest 를 기록한 뒤 원문만 손으로 고친 경우.
                #   잡지 못하는 것: 원문과 digest 를 **함께** 고친 경우.
                errors.append(
                    "raw_output_digest 불일치 — 기록 %s / 실제 sha256:%s "
                    "(원문이 기록 이후 변경됐다)" % (m.group(0), actual)
                )
            try:
                declared = int(str(fm.get("raw_output_bytes")).strip())
            except (TypeError, ValueError):
                errors.append("raw_output_bytes 가 정수가 아니다")
            else:
                if declared != nbytes:
                    errors.append(
                        "raw_output_bytes 불일치 — 기록 %d / 실제 %d" % (declared, nbytes)
                    )


def check_file(path, grandfathered=frozenset()):
    """(errors, warnings, data) 반환."""
    text = io.open(path, encoding="utf-8").read()
    fm, err = parse_front_matter(text)
    if err:
        return [err], [], {}

    errors, warns = [], []

    for f in REQUIRED:
        if f not in fm:
            errors.append("필수 필드 누락: %s" % f)
        elif is_empty(fm[f]):
            errors.append("필드가 비었거나 자리표시자: %s" % f)

    st = fm.get("status")
    if isinstance(st, str) and st not in VALID_STATUS:
        errors.append("status 값이 잘못됨: %r (허용: %s)" % (st, ", ".join(sorted(VALID_STATUS))))

    # RULE.md §7 — limitations 가 비면 반려
    if is_empty(fm.get("limitations")):
        errors.append("limitations 가 비었다 — 무엇을 증명하지 '않는지' 없는 실험은 증거가 아니다")

    # RULE.md §6 — negative test 없으면 미완료
    if is_empty(fm.get("negative_tests")):
        errors.append("negative_tests 가 비었다 — 정상 경로만으로는 완료가 아니다")

    commit = fm.get("commit")
    if isinstance(commit, str):
        c = commit.strip()
        if not re.fullmatch(r"[0-9a-f]{7,40}", c):
            errors.append("commit 이 유효한 hash 가 아니다: %r" % commit)
        elif not commit_exists(c):
            # ★ 2026-08-16 추가 (독립 검수).
            #   전에는 **형식만** 봤다. 40자 hex 이면 존재하지 않는 커밋도 통과했다.
            #   "재현 가능한 실험 기록" 이 목적인데 커밋을 못 찾으면 재현할 수 없다.
            errors.append(
                "commit %s 이 이 저장소에 없다 — 재현할 수 없는 기록이다" % c
            )

    # ★ 2026-08-16 추가 (독립 검수) — artifact 가 실제로 존재하는가.
    #
    #   전에는 `artifacts` 가 **비어 있지 않은지만** 봤다.
    #   실제로 `DoD-04` 가 `crates/protocol/tests/ed25519_verify.rs` 를 적어 두었는데
    #   그 파일은 `crates/crypto/` 로 옮겨져 **존재하지 않았다.** 그래도 PASS 였다.
    #
    #   증거의 목적은 "재현 가능한 기록" 이다. 가리키는 파일이 없으면 재현할 수 없다.
    for rel in as_list(fm.get("artifacts")):
        if not rel or rel.startswith("("):
            continue
        ok, why = safe_repo_path(rel)
        if not ok:
            errors.append("artifact: %s" % why)
            continue
        if not os.path.exists(os.path.join(REPO_ROOT, rel.replace("\\", "/"))):
            errors.append("artifact 가 존재하지 않는다: %s" % rel)

    # ★ raw_output 은 **요약**이고 원문은 `_raw/` 에 있다.
    #   둘의 관계를 검사기가 강제하지 않으면 수기 편집과 실행 원문을 구분할 수 없다.
    #   최소한 `_raw/` artifact 를 하나는 갖도록 요구한다.
    raws = [a for a in as_list(fm.get("artifacts")) if "/_raw/" in a or a.startswith("docs/evidence/_raw/")]
    if not raws:
        warns.append(
            "artifacts 에 docs/evidence/_raw/ 원문이 없다 — "
            "frontmatter 의 raw_output 은 요약이므로 원문 없이는 대조할 수 없다"
        )

    # ★ 2026-08-16 추가 (독립 검수) — negative test 를 **실제로 실행했는가.**
    #
    #   전에는 `negative_tests` 가 비어 있지 않은지만 봤다.
    #   "없음" 한 줄만 적어도 통과했다.
    #
    #   이름이 테스트 함수처럼 생겼다면 그 이름이 `raw_output` 이나
    #   `_raw/` 원문에 나타나야 한다. 나타나지 않으면 **적기만 하고 안 돌렸을** 수 있다.
    #
    #   ★ 경고에 그친다 — negative test 를 산문으로 적는 것도 정당하다
    #     (예: "뮤테이션 2종으로 확인"). 오탐을 오류로 만들면 규칙을 우회하게 된다.
    haystack = str(fm.get("raw_output") or "")
    for rel in as_list(fm.get("artifacts")):
        full = os.path.join(REPO_ROOT, rel)
        if "/_raw/" in rel and os.path.exists(full):
            try:
                haystack += io.open(full, encoding="utf-8", errors="ignore").read()
            except OSError:
                pass
    for entry in as_list(fm.get("negative_tests")):
        e = entry.lstrip("★ ").strip().strip('"')
        m = re.match(r"([a-z][a-z0-9_]{6,})", e)
        if m and m.group(1) not in haystack:
            warns.append(
                "negative test %r 의 이름이 실행 원문에 없다 — "
                "적기만 하고 실행하지 않았을 수 있다" % m.group(1)
            )

    # ★ 2026-08-17 추가 — negative test 가 **아직 존재하는가.**
    #
    #   실제로 있었던 일: DoD-07 이 `evidence_is_not_replay_checked` 를 적어 뒀는데
    #   그 테스트는 이름이 바뀌었고 **동작도 뒤집혔다.**
    #   evidence 는 그대로 "이 테스트가 이것을 보장한다" 고 말하고 있었다.
    #
    #   ★ 스테일한 evidence 는 없는 것보다 나쁘다 —
    #     읽는 사람이 그것을 근거로 판단하기 때문이다.
    #
    #   경고에 그친다. 이름을 바꾸는 것은 정당하고, evidence 는 **그 시점의
    #   관측 기록**이므로 자동으로 틀렸다고 단정할 수 없다.
    #   다만 "확인해 보라" 는 신호는 있어야 한다.
    sources = repo_sources()

    for entry in as_list(fm.get("negative_tests")):
        e = entry.lstrip("★ ").strip().strip('"')
        # ★ `이름: 설명` 형태만 본다.
        #   evidence 는 `- "test_name: 무엇을 확인했는가"` 규약을 쓴다.
        #   그 형태가 아니면 산문이거나 여러 이름을 나열한 것이므로 건너뛴다 —
        #   오탐이 많으면 경고 전체를 무시하게 되고, 그러면 검사가 없는 것과 같다.
        m = re.match(r"([a-z][a-z0-9_]{6,})\s*(?::|$)", e)
        if not m:
            continue
        name = m.group(1)
        # ★ **함수 정의**로만 찾는다 (2026-08-17 자체 수정).
        #   처음엔 이름이 소스 어디에든 있으면 통과시켰다.
        #   그랬더니 **이 검사기 자신의 주석**에 적어 둔 예시 이름
        #   (`evidence_is_not_replay_checked`)과 매칭돼 오탐을 놓쳤다.
        #   검사기가 자기 주석 때문에 눈이 머는 것은 우스운 실패다.
        if not re.search(r"(?:fn|def)\s+" + re.escape(name) + r"\s*[(<]", sources):
            warns.append(
                "negative test %r 의 함수 정의를 찾지 못했다 — "
                "이름이 바뀌었거나 지워졌을 수 있다. "
                "테스트 함수가 아닌 것(벡터 이름 등)을 적었다면 무시해도 된다"
                % name
            )

    # 본문의 '증명하지 않는 것' 절 확인
    if "증명하지" not in text:
        warns.append("본문에 '이 실험이 증명하지 않는 것' 절이 보이지 않는다")

    check_schema_v2(fm, path, errors, warns, grandfathered)

    return errors, warns, fm


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    if not os.path.isdir(EVIDENCE_DIR):
        print("evidence 디렉터리 없음: %s" % EVIDENCE_DIR)
        return 1

    files = sorted(f for f in os.listdir(EVIDENCE_DIR)
                   if f.endswith(".md") and not f.startswith("_"))

    grandfathered, gf_errors = load_grandfathered()
    if gf_errors:
        # ★ 유예 목록이 흔들리면 **아무것도 유예하지 않는다.**
        #   이 오류를 조용히 넘기면 목록을 고치는 것이 규칙 우회가 된다.
        for e in gf_errors:
            print("★ %s" % e)
        print()

    results, bad = [], 0
    tally = {}
    unreviewed = []   # ★ 독립 검수 기록이 없는 PASS — 부채로 센다

    for name in files:
        path = os.path.join(EVIDENCE_DIR, name)
        errors, warns, fm = check_file(path, grandfathered)
        st = fm.get("status", "?")
        tally[st] = tally.get(st, 0) + 1
        ver = str(fm.get("schema_version", "1")).strip()
        if ver == "1" and st in COUNTS_AS_PASS and name.startswith(REVIEW_REQUIRED_PREFIX):
            unreviewed.append(name)
        results.append({"file": name, "id": fm.get("id"), "status": st,
                        "schema_version": ver,
                        "errors": errors, "warnings": warns})
        if errors:
            bad += 1

    if args.json:
        print(json.dumps({"results": results, "tally": tally,
                          "unreviewed_pass": unreviewed},
                         ensure_ascii=False, indent=2))
        return 1 if bad else 0

    print("evidence 검사 — %s" % EVIDENCE_DIR)
    print("=" * 62)
    if not files:
        print("  (evidence 파일 없음)")
    for r in results:
        mark = "FAIL" if r["errors"] else "ok  "
        print("  %s %-40s %s" % (mark, r["file"], r["status"]))
        for e in r["errors"]:
            print("        ! %s" % e)
        for w in r["warnings"]:
            print("        ~ %s" % w)

    print("-" * 62)
    print("파일 %d개" % len(files))
    for k in sorted(tally):
        note = "  <- PASS 로 계상" if k in COUNTS_AS_PASS else ""
        print("  %-22s %d%s" % (k, tally[k], note))

    passed = sum(v for k, v in tally.items() if k in COUNTS_AS_PASS)
    print("PASS 로 계상되는 항목: %d / %d" % (passed, len(files)))

    # ★ **부채를 조용히 두지 않는다.**
    #   유예 목록은 "괜찮다" 가 아니라 "아직 안 했다" 는 기록이다.
    #   숫자가 보이지 않으면 영원히 유예된다.
    if unreviewed:
        print()
        print("★ 독립 검수 기록이 없는 P0/DoD PASS: %d건 (schema v1 유예)"
              % len(unreviewed))
        for n in unreviewed:
            print("    - %s" % n)
        print("  이들은 '검수를 통과했다' 가 아니라 '검수하지 않았다' 이다.")
        print("  줄이는 방법: schema v2 로 올리고 실제 독립 검수를 받는다 (RULE.md §7.3).")
    if tally.get("ENVIRONMENT-BLOCKED"):
        print("  주의: ENVIRONMENT-BLOCKED %d건은 PASS 가 아니다 (RULE.md §7.1)"
              % tally["ENVIRONMENT-BLOCKED"])

    if bad:
        print("\n스키마 위반 %d건. 위 항목을 채워야 evidence 로 인정된다." % bad)
        return 1
    if gf_errors:
        print("\n유예 목록 오류 %d건 — 유예가 전부 취소됐다." % len(gf_errors))
        return 1
    print("\n스키마 위반 없음.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
