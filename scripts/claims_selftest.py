# -*- coding: utf-8 -*-
"""주장 검사표가 **공허하지 않은지** 스스로 확인한다.

★ 왜 필요한가

`docs/_주장_검사.md` 의 줄이 늘어나면 안심하기 쉽다. 그런데 검사기가
그 줄을 **실제로 검사하고 있는지**는 별개다. 실제로 2026-09-06 에
두 줄이 **조용히 건너뛰어지고 있었다** — 패턴에 `|`(정규식 OR)가
들어 있어서 표 칸 나누기가 깨졌고, 칸 수가 안 맞으면 넘기게 짜여
있었기 때문이다. 표에는 있는데 검사는 안 되고 있었다.

이 저장소가 다른 곳에서 쓰는 방법을 그대로 쓴다 — **뮤테이션**.
각 줄의 `검사` 칸(없어야 한다 / 있어야 한다)을 **하나씩 뒤집어** 보고,
뒤집었을 때 `check_docs.py` 가 **반드시 실패해야 한다.**

    뒤집었는데 통과한다 = 그 줄은 아무것도 재고 있지 않다

★ 이 시험은 파일을 잠깐 고쳤다가 되돌린다. `finally` 로 원복하므로
  중간에 죽어도 원본이 남는다 — 초안은 그게 없어서 첫 실행이 죽었을 때
  뒤집힌 상태가 그대로 남았고, 다음 실행이 **그 뒤집힌 것을 원본으로
  삼아** 결과가 거꾸로 나왔다.

```text
python scripts/claims_selftest.py
```
"""

import io
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)

from claims_check import CLAIMS_DOC, CLAIMS_HEADING, MODES, CELL_SPLIT  # noqa: E402


def rows_of(text):
    body = text.split(CLAIMS_HEADING, 1)[1]
    body = body.split("\n## ", 1)[0]
    out = []
    for raw in body.splitlines():
        line = raw.strip()
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in CELL_SPLIT.split(line.strip("|"))]
        if len(cells) != 4 or cells[1] not in MODES:
            continue
        out.append((raw, cells[0], cells[1]))
    return out


def main():
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    path = os.path.join(ROOT, CLAIMS_DOC)
    original = io.open(path, encoding="utf-8").read()
    rows = rows_of(original)
    if not rows:
        print("검사할 줄을 못 읽었다 — 표 형식을 확인하라")
        return 1

    # 기준선: 손대기 전에는 통과해야 한다.
    base = subprocess.run([sys.executable, os.path.join(HERE, "check_docs.py")],
                          capture_output=True, text=True, encoding="utf-8")
    if base.returncode != 0:
        print("기준선이 이미 실패한다 — 뒤집기 시험을 할 수 없다.")
        print(base.stdout[-1500:])
        return 1

    vacuous = []
    try:
        for raw, claim, mode in rows:
            flipped = "없어야 한다" if mode == "있어야 한다" else "있어야 한다"
            mutated = original.replace(
                raw, raw.replace("| %s |" % mode, "| %s |" % flipped, 1), 1)
            if mutated == original:
                vacuous.append((claim, "줄을 못 바꿨다 — 표 형식 확인"))
                continue
            io.open(path, "w", encoding="utf-8").write(mutated)
            run = subprocess.run(
                [sys.executable, os.path.join(HERE, "check_docs.py")],
                capture_output=True, text=True, encoding="utf-8")
            if run.returncode == 0:
                vacuous.append((claim, "뒤집었는데도 통과했다"))
    finally:
        # ★ 무슨 일이 있어도 되돌린다.
        io.open(path, "w", encoding="utf-8").write(original)

    print("주장 검사표 뒤집기 시험 — %d줄" % len(rows))
    print("=" * 62)
    if not vacuous:
        print("  %d/%d 줄 전부 뒤집었을 때 실패했다 — 공허한 줄 없음"
              % (len(rows), len(rows)))
        return 0

    for claim, why in vacuous:
        print("  공허함  %s" % claim)
        print("          %s" % why)
    print("-" * 62)
    print("%d줄이 아무것도 재고 있지 않다." % len(vacuous))
    return 1


if __name__ == "__main__":
    sys.exit(main())
