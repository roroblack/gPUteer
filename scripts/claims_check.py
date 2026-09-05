# -*- coding: utf-8 -*-
"""docs/_주장_검사.md 의 "아직 없다" 주장을 실제 코드에 대고 확인한다.

★ 왜 이 검사가 있는가 (2026-09-06)

2026-09-06 하루에 낡은 서술을 다섯 개 찾았다. 전부 같은 병이었다 —
한때 참이었고, 그 항목이 끝났을 때 **그것을 가리키던 다른 문서를 같이
안 고쳤다.**

    _열린_작업.md §A      네 행 중 세 행이 이미 끝난 일이었다
    CLAUDE.md "진짜 뿌리"  "Agent 가 실행 안 함" — 실행은 이미 된다
    CLAUDE.md schema v1    이관이 끝난 뒤의 중간 상태가 그대로 남아 있었다
    CLAUDE.md 정책 강제    "소비자가 아직 없다" — 생겼다
    CLAUDE.md 멤버십 4건   가운데 둘은 이미 답을 받은 항목이었다

다섯 개가 전부 **부재 주장**이다. 부재 주장은 **저절로 거짓이 된다** —
누가 그것을 만들면 그 순간 틀리는데, 만든 사람은 자기가 만든 것만 보지
그것을 "없다" 고 적어 둔 다른 문서를 보지 않는다.

그래서 규칙을 하나 더 적는 대신 **부재 주장을 반증 가능하게** 만든다.
`docs/_주장_검사.md` 의 표를 **실제로 파싱해** 대조한다 —
`state-machines.md` 를 `state_table_parity.rs` 가 파싱하는 것과 같은 방식이다.

★ 이 검사가 실패하는 것은 **결함이 아니라 진전일 수 있다.** 할 일은
  검사를 지우는 게 아니라 그 주장을 쓴 문서를 먼저 고치는 것이다.
"""

import io
import os
import re

CLAIMS_DOC = "docs/_주장_검사.md"
CLAIMS_HEADING = "## 지금 검사하는 것"
MODES = ("없어야 한다", "있어야 한다")

# 폴더를 대상으로 줬을 때 훑을 확장자. 문서·빌드 산출물은 안 본다.
CODE_EXT = (".rs", ".proto", ".toml", ".py")


def _iter_targets(target_abs):
    """대상이 파일이면 그것만, 폴더면 코드 파일을 재귀로 훑는다."""
    if os.path.isfile(target_abs):
        yield target_abs
        return
    for base, dirs, files in os.walk(target_abs):
        # 빌드 산출물은 건너뛴다 — 여기까지 훑으면 몇 분씩 걸린다.
        dirs[:] = [d for d in dirs if d != "target"]
        for name in files:
            if name.endswith(CODE_EXT):
                yield os.path.join(base, name)


# 마크다운 표에서 칸을 가르는 `|` 는 **앞에 백슬래시가 없는 것**뿐이다.
#
# ★★ 2026-09-06 — 처음엔 그냥 `line.split("|")` 했다가 **두 줄을 조용히
#   건너뛰었다.** 패턴 자체에 `|`(정규식 OR)가 들어 있으면 칸이 더 많이
#   쪼개지고, 그때 `len(cells) != 4` 로 그냥 넘겼기 때문이다.
#
#   뮤테이션 테스트(모든 줄의 검사 방향을 뒤집어 실패하는지 본다)가
#   그것을 잡았다 — 그 두 줄만 뒤집어도 통과했다. 검사기가 검사하지
#   않고 있었던 것이다. `CLAUDE.md` §3 "조용한 스킵을 만들지 않는다".
CELL_SPLIT = re.compile(r"(?<!\\)\|")


def parse_table(text):
    """표를 (주장, 검사, 대상, 패턴) 튜플 목록으로 읽는다.

    돌려주는 것은 `(rows, malformed)` 다. 표 머리와 구분선은 `검사` 칸이
    MODES 에 없으므로 자연히 걸러진다 — 별도 규칙을 두지 않는다(규칙이
    둘이면 둘이 어긋난다). 그러나 **칸 수가 안 맞는 줄은 조용히 넘기지
    않고 돌려준다.**
    """
    if CLAIMS_HEADING not in text:
        return None, []
    body = text.split(CLAIMS_HEADING, 1)[1]
    # 다음 절이 시작되면 표는 끝난다.
    body = re.split(r"\n## ", body, maxsplit=1)[0]

    rows, malformed = [], []
    for raw in body.splitlines():
        line = raw.strip()
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in CELL_SPLIT.split(line.strip("|"))]
        if len(cells) != 4:
            malformed.append(line)
            continue
        claim, mode, target, pattern = cells
        if mode not in MODES:
            continue
        # 표 안에서 `\|` 로 escape 했던 것을 정규식으로 돌려준다.
        pattern = pattern.strip("`").replace("\\|", "|")
        rows.append((claim, mode, target.strip("`"), pattern))
    return rows, malformed


def check_claims(root, rel):
    """표의 각 줄을 실제 파일에 대고 확인한다. 오류 문자열 목록을 돌려준다."""
    errs = []
    full = os.path.join(root, CLAIMS_DOC)
    if not os.path.exists(full):
        return ["%s 가 없다 — 주장 검사를 돌릴 수 없다" % CLAIMS_DOC]

    rows, malformed = parse_table(io.open(full, encoding="utf-8").read())
    if rows is None:
        return ["%s 에 '%s' 절이 없다 — 표를 찾지 못했다"
                % (CLAIMS_DOC, CLAIMS_HEADING)]
    if not rows:
        return ["%s 에서 검사할 줄을 하나도 못 읽었다 — 표 형식이 깨졌다"
                % CLAIMS_DOC]

    # ★ 칸 수가 안 맞는 줄을 **넘기지 않고 신고한다.** 넘기면 검사기가
    #   검사하지 않으면서 통과한다 — 그게 없는 것보다 나쁘다.
    for line in malformed:
        errs.append(
            "%s 의 표 줄에서 칸이 4개가 아니다 — 이 줄은 검사되지 않는다"
            "\n        %s"
            "\n        패턴에 `|`(정규식 OR)를 쓰려면 `\\|` 로 적는다."
            % (CLAIMS_DOC, line[:100])
        )

    how_to_fix = (
        "\n        고칠 것: 이 주장을 쓴 문서(CLAUDE.md · docs/plans/_열린_작업.md 등)를"
        "\n        먼저 고치고, 그다음 %s 의 해당 줄을 옮기거나 지운다." % CLAIMS_DOC
    )

    for claim, mode, target, pattern in rows:
        target_abs = os.path.join(root, target)
        if not os.path.exists(target_abs):
            errs.append("주장 검사: 대상 경로가 없다 (%s) — 주장: %s"
                        % (target, claim))
            continue
        try:
            rx = re.compile(pattern)
        except re.error as exc:
            errs.append("주장 검사: 정규식이 잘못됐다 (%s): %s" % (pattern, exc))
            continue

        hits = []
        for path in _iter_targets(target_abs):
            try:
                content = io.open(path, encoding="utf-8", errors="ignore").read()
            except OSError:
                continue
            if rx.search(content):
                hits.append(rel(path))

        if mode == "없어야 한다" and hits:
            # ★ 이건 결함이 아니라 **진전**일 수 있다. 그래서 문구를 그렇게 쓴다.
            errs.append(
                "주장이 더 이상 참이 아니다 — %s"
                "\n        %s 가 %s 에서 발견됐다 (예: %s)%s"
                % (claim, pattern, target, ", ".join(hits[:3]), how_to_fix)
            )
        elif mode == "있어야 한다" and not hits:
            errs.append(
                "주장이 더 이상 참이 아니다 — %s"
                "\n        %s 를 %s 에서 찾지 못했다%s"
                % (claim, pattern, target, how_to_fix)
            )

    return errs
