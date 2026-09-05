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
  6. ★ 문서가 가리키는 저장소 경로가 실제로 존재하는가 (2026-08-16 추가)
  7. ★ HISTORY 가 실제로 append-only 인가 — git 으로 확인 (2026-08-16 추가)

사용법:
    python scripts/check_docs.py
"""
import glob
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


from claims_check import check_claims  # noqa: E402


def rel(p):
    return os.path.relpath(p, ROOT).replace("\\", "/")


# ══════════════════════════════════════════════════════════════════
# ★ 6. 문서가 가리키는 저장소 경로가 실제로 존재하는가 (2026-08-16 추가)
#
# 독립 검수 지적 — 규범 문서가 **없는 파일을 가리키고 있었다.**
#
#   state-machines.md   "테스트가 파싱한다" 며 tests/unit/state_machine_test.rs 를 지목
#                       -> 그 디렉터리 자체가 없었다
#   RULE.md · contracts tests/chaos/checkpoint_kill.rs
#                       -> 실제는 crates/checkpoint/tests/kill_chaos.rs
#
# 규칙대로 따라간 사람이 **없는 파일을 찾게 된다.**
# ══════════════════════════════════════════════════════════════════

# 백틱 안의 저장소 상대 경로처럼 보이는 것
PATHLIKE = re.compile(
    r"`((?:crates|tools|scripts|proto|tests|docs|apps|python)/[A-Za-z0-9_./-]+)`"
)

# 검사 대상 문서 — 규범·계약만 본다.
# 리포트·evidence 는 과거 시점의 기록이므로 경로가 낡을 수 있다(그때는 맞았다).
PATH_CHECKED = [
    "RULE.md", "CLAUDE.md",
    "docs/README.md",
    "docs/protocol/signing.md", "docs/protocol/state-machines.md",
    "docs/contracts/01_스트림_소유권.md", "docs/contracts/02_변경_제안_절차.md",
    # ★ 런북은 **그대로 실행하는** 문서다. 여기 적힌 경로가 틀리면
    #   실행하는 사람이 없는 파일을 먹인다. 리포트보다 더 엄하게 본다.
    "docs/runbooks/검수_대기열.md",
]
# 검수 프롬프트도 같은 이유로 전부 본다(파일이 늘어나므로 glob 으로 모은다).
PATH_CHECKED_GLOBS = ["docs/runbooks/검수_프롬프트/*.md"]

# 문서가 `파일:줄` 로 지목한 줄이 **파일에 실제로 있는가**.
#
# ★ 줄 번호는 편집하면 어긋난다 — 그래서 "가리키는 줄이 맞는가" 는 기계가
#   못 본다. 그러나 **파일이 그 줄보다 짧아졌다면** 그 인용은 확실히
#   죽었다. 거짓 양성이 없는 만큼만 검사한다.
PATH_WITH_LINE = re.compile(
    r"`((?:crates|tools|scripts|proto|tests|docs)/[A-Za-z0-9_./-]+):(\d+)`"
)


def _path_checked_docs():
    """검사 대상 문서 목록. glob 으로 모으는 것도 포함한다."""
    docs = list(PATH_CHECKED)
    for pattern in PATH_CHECKED_GLOBS:
        for path in sorted(glob.glob(os.path.join(ROOT, pattern))):
            docs.append(rel(path))
    return docs


def check_referenced_paths():
    errs = []
    for doc in _path_checked_docs():
        full = os.path.join(ROOT, doc)
        if os.path.exists(full):
            text = io.open(full, encoding="utf-8").read()
            # `파일:줄` 인용 — 파일이 그 줄보다 짧으면 인용이 죽은 것이다.
            for m in PATH_WITH_LINE.finditer(text):
                ref, lineno = m.group(1), int(m.group(2))
                target = os.path.join(ROOT, ref)
                if not os.path.isfile(target):
                    continue  # 아래 경로 검사가 따로 잡는다
                total = len(io.open(target, encoding="utf-8",
                                    errors="ignore").read().splitlines())
                if lineno > total:
                    errs.append(
                        "%s 가 없는 줄을 가리킨다: %s:%d (그 파일은 %d줄뿐)"
                        % (doc, ref, lineno, total))
        if not os.path.exists(full):
            continue
        text = io.open(full, encoding="utf-8").read()
        seen = set()
        for m in PATHLIKE.finditer(text):
            ref = m.group(1)
            if ref in seen:
                continue
            seen.add(ref)
            # 와일드카드·설명용 표기는 건너뛴다
            if any(c in ref for c in "*?<>") or ref.endswith("/"):
                continue
            if not os.path.exists(os.path.join(ROOT, ref)):
                errs.append("%s 가 없는 경로를 가리킨다: %s" % (doc, ref))
    return errs


# ══════════════════════════════════════════════════════════════════
# ★ 7. HISTORY 가 실제로 append-only 인가 (2026-08-16 추가)
#
# HISTORY.md 는 "추가만 한다. 기존 기록을 수정하지 않는다" 고 스스로 적어 두었다.
# 그런데 **아무도 검사하지 않았다.** git 으로 확인한다.
# ══════════════════════════════════════════════════════════════════

def check_history_append_only():
    """직전 커밋 대비 HISTORY 에서 **삭제된 줄**이 있는지 본다."""
    import subprocess
    hist = "docs/history/HISTORY.md"
    try:
        r = subprocess.run(
            ["git", "diff", "HEAD", "--unified=0", "--", hist],
            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=15,
        )
    except (OSError, subprocess.SubprocessError):
        return []  # git 없음 — 검사 불가
    if r.returncode != 0:
        return []
    removed = [
        ln[1:].strip()
        for ln in (r.stdout or "").splitlines()
        if ln.startswith("-") and not ln.startswith("---") and ln[1:].strip()
    ]
    if removed:
        return [
            "HISTORY.md 에서 %d줄이 **삭제**됐다 — 추가만 해야 한다 (RULE.md 5). 예: %s"
            % (len(removed), removed[0][:80])
        ]
    return []


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
    # ★ 2026-08-16 추가 — 독립 검수 지적
    errors.extend(check_referenced_paths())
    warns.extend(check_history_append_only())
    errors.extend(check_claims(ROOT, rel))

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
