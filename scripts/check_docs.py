#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
문서 구조 · 파일명 · 중복 검사.

규범: ../RULE.md §5, ../docs/README.md

검사 항목
  1. 필수 폴더 존재
  2. 파일명 규칙 (폴더별)
  3. ★ proto 복제 검사 — contracts/ 가 proto 필드를 옮겨 적었는가
  4. ★ 상태 전이표 중복 검사
  5. history 는 추가만 (수정 감지는 git 이 담당. 여기서는 형식만)

사용법:
    python scripts/check_docs.py
"""
import io
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(ROOT, "docs")

REQUIRED_DIRS = [
    "docs/protocol", "docs/contracts", "docs/plans", "docs/history",
    "docs/reports", "docs/reports/debugs", "docs/evidence",
    "docs/decisions", "docs/runbooks", "docs/vision", "docs/manuals",
]

# 폴더별 파일명 패턴 (템플릿/README/인덱스는 면제)
NAME_RULES = {
    # DoD-NN 기능 검증 · P0-NNx 스파이크 · ENV-NN 환경 실측 · COMPAT-NN 호환성 매트릭스
    "docs/evidence": re.compile(
        r"^(DoD-\d{2,3}|P0-\d{2}[a-z]?|ENV-\d{2}|COMPAT-\d{2})_.+\.md$"),
    "docs/decisions": re.compile(r"^ADR-\d{3}_.+\.md$"),
    "docs/contracts": re.compile(r"^\d{2}_.+\.md$"),
    "docs/vision": re.compile(r"^VISION-\d{2}_.+\.md$"),
    "docs/plans": re.compile(r"^\d{4}-\d{2}-\d{2}_\d{4}_.+\.md$"),
    "docs/reports": re.compile(r"^\d{4}-\d{2}-\d{2}_\d{4}_.+\.md$"),
    "docs/reports/debugs": re.compile(r"^\d{4}-\d{2}-\d{2}_\d{4}_.+\.md$"),
}

EXEMPT = {"README.md", "_TEMPLATE.md", "HISTORY.md", "TODO_VISION.md",
          "ESTIMATION_BASELINE.md", ".gitkeep"}

# proto 에서만 정의되어야 하는 것들 — contracts/ 에 나타나면 복제 의심
PROTO_MARKERS = [
    (re.compile(r"^\s*(uint32|uint64|int32|int64|bytes|string|bool|repeated|map<)\s+\w+\s*=\s*\d+\s*;",
                re.M), "protobuf 필드 정의"),
    (re.compile(r"^\s*message\s+\w+\s*\{", re.M), "protobuf message 선언"),
    (re.compile(r"^\s*enum\s+\w+\s*\{", re.M), "protobuf enum 선언"),
]

STATE_TABLE = re.compile(r"^\s*```statetable", re.M)


def rel(p):
    return os.path.relpath(p, ROOT).replace("\\", "/")


def main():
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    errors, warns = [], []

    # 1. 필수 폴더
    for d in REQUIRED_DIRS:
        if not os.path.isdir(os.path.join(ROOT, d)):
            errors.append("필수 폴더 없음: %s" % d)

    # 2. 파일명 규칙
    for folder, pat in NAME_RULES.items():
        path = os.path.join(ROOT, folder)
        if not os.path.isdir(path):
            continue
        for name in os.listdir(path):
            full = os.path.join(path, name)
            if os.path.isdir(full) or name in EXEMPT or name.startswith("_"):
                continue
            if not name.endswith(".md"):
                continue
            if not pat.match(name):
                errors.append("파일명 규칙 위반: %s/%s  (기대: %s)"
                              % (folder, name, pat.pattern))

    # 3. proto 복제 검사
    contracts = os.path.join(DOCS, "contracts")
    if os.path.isdir(contracts):
        for name in os.listdir(contracts):
            if not name.endswith(".md"):
                continue
            text = io.open(os.path.join(contracts, name), encoding="utf-8").read()
            # 체크리스트/금지문구 안의 언급은 제외하기 위해 코드펜스만 검사
            fences = re.findall(r"```(?:protobuf|proto)?\n(.*?)```", text, re.S)
            body = "\n".join(fences)
            for pat, what in PROTO_MARKERS:
                if pat.search(body):
                    errors.append(
                        "proto 복제 의심: docs/contracts/%s 에 %s 이 있다. "
                        "원본은 proto/ 다 (docs/README.md '중복 금지')" % (name, what))
                    break

    # 4. 상태 전이표 중복
    holders = []
    for base, _dirs, files in os.walk(DOCS):
        for name in files:
            if not name.endswith(".md"):
                continue
            p = os.path.join(base, name)
            if STATE_TABLE.search(io.open(p, encoding="utf-8").read()):
                holders.append(rel(p))
    if len(holders) > 1:
        errors.append("상태 전이표가 여러 곳에 있다 (원본은 docs/protocol/state-machines.md 하나여야 한다): %s"
                      % ", ".join(holders))
    elif holders and holders[0] != "docs/protocol/state-machines.md":
        warns.append("상태 전이표가 예상 밖 위치에 있다: %s" % holders[0])

    # 5. history 형식
    hist = os.path.join(DOCS, "history", "HISTORY.md")
    if os.path.isfile(hist):
        t = io.open(hist, encoding="utf-8").read()
        entries = re.findall(r"^## (\d{4}-\d{2}-\d{2})", t, re.M)
        if not entries:
            warns.append("docs/history/HISTORY.md 에 기록이 없다")

    # 출력
    print("문서 구조 검사 — %s" % ROOT)
    print("=" * 62)
    if not errors and not warns:
        print("  이상 없음")
    for e in errors:
        print("  FAIL  %s" % e)
    for w in warns:
        print("  warn  %s" % w)
    print("-" * 62)
    print("오류 %d · 경고 %d" % (len(errors), len(warns)))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
