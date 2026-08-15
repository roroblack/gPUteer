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
import io
import json
import os
import re
import sys

EVIDENCE_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                            "docs", "evidence")

REQUIRED = [
    "id", "claim", "status", "commit", "binary_digests", "protocol_versions",
    "platform", "hardware", "network_profile", "command", "raw_output",
    "artifacts", "negative_tests", "limitations", "decision",
]

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
    if not text.startswith("---"):
        return None, "front-matter 없음 (--- 로 시작해야 한다)"
    end = text.find("\n---", 3)
    if end == -1:
        return None, "front-matter 가 닫히지 않았다"
    block = text[3:end]

    data, key, buf, mode = {}, None, [], None
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
            data.setdefault(key, []).append(raw.lstrip()[2:].strip())
            continue
        # 중첩 매핑 (한 단계만)
        if raw.startswith(("  ", "\t")) and key is not None and mode == "map":
            k2, _, v2 = raw.strip().partition(":")
            if k2:
                data.setdefault(key, {})[k2.strip()] = v2.strip()
            continue
        m = re.match(r"^([A-Za-z_][A-Za-z0-9_]*):\s*(.*)$", raw)
        if not m:
            continue
        key, val = m.group(1), m.group(2).strip()
        if val in ("|", ">", "|-", ">-"):
            mode, buf = "block", []
        elif val == "":
            mode = "pending"      # list 인지 map 인지 다음 줄에서 결정
            data[key] = None
        else:
            mode = None
            data[key] = val
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
                items.append(s[2:].strip())
            elif ":" in s:
                a, _, b = s.partition(":")
                mapping[a.strip()] = b.strip()
        if items:
            data[k] = items
        elif mapping:
            data[k] = mapping
    return data, None


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


def check_file(path):
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
    if isinstance(commit, str) and not re.fullmatch(r"[0-9a-f]{7,40}", commit.strip()):
        errors.append("commit 이 유효한 hash 가 아니다: %r" % commit)

    # 본문의 '증명하지 않는 것' 절 확인
    if "증명하지" not in text:
        warns.append("본문에 '이 실험이 증명하지 않는 것' 절이 보이지 않는다")

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

    results, bad = [], 0
    tally = {}

    for name in files:
        path = os.path.join(EVIDENCE_DIR, name)
        errors, warns, fm = check_file(path)
        st = fm.get("status", "?")
        tally[st] = tally.get(st, 0) + 1
        results.append({"file": name, "id": fm.get("id"), "status": st,
                        "errors": errors, "warnings": warns})
        if errors:
            bad += 1

    if args.json:
        print(json.dumps({"results": results, "tally": tally}, ensure_ascii=False, indent=2))
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
    if tally.get("ENVIRONMENT-BLOCKED"):
        print("  주의: ENVIRONMENT-BLOCKED %d건은 PASS 가 아니다 (RULE.md §7.1)"
              % tally["ENVIRONMENT-BLOCKED"])

    if bad:
        print("\n스키마 위반 %d건. 위 항목을 채워야 evidence 로 인정된다." % bad)
        return 1
    print("\n스키마 위반 없음.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
