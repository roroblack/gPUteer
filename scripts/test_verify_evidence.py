#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""`verify_evidence.py` 의 부정 테스트 — **검사기가 실제로 잡는가.**

# 왜 이 파일이 있나

`RULE.md` §4.3 · `CLAUDE.md` §4 — 정상 경로 통과만으로 완료라 하지 않는다.

evidence 검사기는 **규칙을 강제하는 장치**다.
강제 장치가 실제로 강제하는지 증명하지 않으면,
"검사기가 통과했다" 는 "검사기가 아무것도 안 본다" 와 구분되지 않는다.

2026-08-17 실제로 그런 일이 있었다 — 파서가 따옴표를 벗기지 않아
`reviewer_id: "agent:x"` 의 값이 `"agent:x"` 였고 접두사 검사가 전부 오탐이었다.
정상 evidence 하나로만 확인했으면 못 봤다.

# 방법

정상 evidence 를 임시 디렉터리에 복사하고 **한 군데씩 망가뜨린 뒤**
검사기가 그 항목을 오류로 보고하는지 확인한다.
보고하지 않으면 그 검사는 존재하지 않는 것이다.

# 사용법

    python scripts/test_verify_evidence.py
"""
import io
import os
import re
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import verify_evidence as VE  # noqa: E402

REPO_ROOT = VE.REPO_ROOT
PILOT = "DoD-09_재개선택_필터.md"

FAILURES = []


def real_sources():
    """진짜 저장소의 소스 전문. `VE.REPO_ROOT` 가 바뀌어도 이것을 쓴다."""
    saved = VE.REPO_ROOT
    VE.REPO_ROOT = REPO_ROOT
    try:
        return VE.repo_sources()
    finally:
        VE.REPO_ROOT = saved


def check(text, raw_files, grandfathered=frozenset({PILOT}), filename=None):
    """망가뜨린 evidence 를 검사기에 통과시킨다. (errors, warnings) 반환.

    `_raw/` 원문은 실제 파일이어야 digest 검사가 의미를 갖는다 —
    임시 저장소를 통째로 만든다.
    """
    tmp = tempfile.mkdtemp(prefix="gputeer-ve-")
    try:
        ev = os.path.join(tmp, "docs", "evidence")
        raw = os.path.join(ev, "_raw")
        os.makedirs(raw)
        for name, data in raw_files.items():
            io.open(os.path.join(raw, name), "wb").write(data)
        path = os.path.join(ev, filename or PILOT)
        io.open(path, "w", encoding="utf-8", newline="\n").write(text)

        # `_raw/` 밖의 artifact(소스 파일 등)는 내용이 검사 대상이 아니다.
        # 임시 저장소에 **빈 파일로 세운다** — 그러지 않으면 기준선조차
        # "artifact 가 존재하지 않는다" 로 실패해 부정 테스트가 무의미해진다.
        for rel in re.findall(r"(?m)^  - (\S+)$", text):
            ok, _ = VE.safe_repo_path(rel)
            if not ok or "/_raw/" in rel:
                continue          # 경로 탈출 케이스는 세우지 않는다 — 그게 검사 대상이다
            full = os.path.join(tmp, rel.replace("/", os.sep))
            os.makedirs(os.path.dirname(full), exist_ok=True)
            io.open(full, "w").close()

        old_root, old_ev = VE.REPO_ROOT, VE.EVIDENCE_DIR
        VE.REPO_ROOT, VE.EVIDENCE_DIR = tmp, ev
        # ★ 소스 스캔은 **진짜 저장소**를 봐야 한다.
        #   임시 저장소에는 빈 스텁 파일뿐이라 모든 테스트 이름이
        #   "함수 정의를 찾지 못했다" 가 된다 — 실제로 그렇게 만들었다가
        #   부정 테스트가 전부 오탐이 됐다.
        VE._SOURCE_CACHE[tmp] = real_sources()
        try:
            # 임시 저장소는 git 저장소가 아니므로 commit 검사는 자동으로 건너뛴다.
            errors, warns, _ = VE.check_file(path, grandfathered)
        finally:
            VE.REPO_ROOT, VE.EVIDENCE_DIR = old_root, old_ev
        return errors, warns
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def run_main_on(text, raw_files, filename=None):
    """임시 저장소를 만들고 `VE.main()` 을 실제로 돌려 **종료 코드**를 얻는다.

    ★ 왜 필요한가 (독립 검수 2026-08-17 지적).

      다른 검사들은 `check_file()` 만 부른다. 그래서 **경고로만 보고되는 위반이
      종료 코드 0 으로 통과하는지** 알 수 없다.
      규칙을 강제하는 것은 함수 반환값이 아니라 **종료 코드**다.
    """
    tmp = tempfile.mkdtemp(prefix="gputeer-ve-main-")
    try:
        ev = os.path.join(tmp, "docs", "evidence")
        os.makedirs(os.path.join(ev, "_raw"))
        for name, data in raw_files.items():
            io.open(os.path.join(ev, "_raw", name), "wb").write(data)
        io.open(os.path.join(ev, filename or PILOT), "w",
                encoding="utf-8", newline="\n").write(text)
        for rel in re.findall(r"(?m)^  - (\S+)$", text):
            if "/_raw/" in rel:
                continue
            full = os.path.join(tmp, rel.replace("/", os.sep))
            os.makedirs(os.path.dirname(full), exist_ok=True)
            io.open(full, "w").close()
        # 유예 목록은 그대로 복사한다 — digest 검사가 통과해야 하기 때문이다.
        shutil.copyfile(VE.GRANDFATHER_LIST,
                        os.path.join(ev, "_schema_v1_grandfathered.txt"))

        saved = (VE.REPO_ROOT, VE.EVIDENCE_DIR, VE.GRANDFATHER_LIST, sys.argv,
                 sys.stdout)
        VE.REPO_ROOT, VE.EVIDENCE_DIR = tmp, ev
        VE.GRANDFATHER_LIST = os.path.join(ev, "_schema_v1_grandfathered.txt")
        sys.argv = ["verify_evidence.py"]
        sys.stdout = io.StringIO()          # main() 의 출력은 삼킨다
        try:
            return VE.main()
        finally:
            (VE.REPO_ROOT, VE.EVIDENCE_DIR, VE.GRANDFATHER_LIST, sys.argv,
             sys.stdout) = saved
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


# ★ 기준선(무손상)이 내는 오류. `expect()` 가 **다른 이유로 난 오류**를
#   목표 오류로 착각하지 않게 하려고 쓴다 (독립 검수 2026-08-17 지적).
BASELINE_ERRORS = []


def expect(label, errors, needle, want=True):
    """`needle` 을 포함한 오류가 **새로** 났는가.

    ★ 2026-08-17 강화 (독립 검수). 전에는 부분문자열 하나만 찾았다.
      망가뜨린 것과 **무관한** 오류가 같은 문자열을 담고 있어도 통과했다.
      지금은 기준선에도 있던 오류는 근거로 세지 않는다.
    """
    fresh = [e for e in errors if e not in BASELINE_ERRORS]
    hit = any(needle in e for e in fresh)
    if hit != want:
        FAILURES.append(
            "%s — %s: %r\n      새로 난 오류: %s"
            % (label,
               "잡아야 하는데 못 잡았다" if want else "잡으면 안 되는데 잡았다",
               needle, fresh or "(없음)"))
        print("  FAIL %s" % label)
    else:
        print("  ok   %s" % label)


def main():
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    src = os.path.join(REPO_ROOT, "docs", "evidence", PILOT)
    if not os.path.exists(src):
        print("시범 evidence 가 없다: %s" % src)
        return 1
    base = io.open(src, encoding="utf-8").read()

    raw_files = {}
    for n in ("DoD-09_resume_selection.txt", "DoD-09_review.txt"):
        p = os.path.join(REPO_ROOT, "docs", "evidence", "_raw", n)
        raw_files[n] = io.open(p, "rb").read()

    print("verify_evidence.py 부정 테스트")
    print("=" * 62)

    # ── 0. 기준선 — 손대지 않은 것은 통과해야 한다 ────────────────
    errors, warns = check(base, raw_files)
    BASELINE_ERRORS[:] = errors
    if errors:
        FAILURES.append("기준선이 통과하지 않는다: %s" % errors)
        print("  FAIL 기준선(무손상) 통과")
    else:
        print("  ok   기준선(무손상) 통과")

    # ── 1. 자기 검수 ──────────────────────────────────────────────
    #   가장 중요한 검사다. 이것이 없으면 v2 는 형식 절차일 뿐이다.
    m = base.replace('reviewer_id: "agent:codex-cli"',
                     'reviewer_id: "agent:claude-code"')
    assert m != base
    errors, _ = check(m, raw_files)
    expect("자기 검수(executor_id == reviewer_id)", errors, "자기 검수")

    # ── 2. 검수 결과가 ACCEPTED 가 아닌데 PASS ────────────────────
    m = base.replace('review_outcome: "ACCEPTED"',
                     'review_outcome: "CHANGES_REQUESTED"')
    assert m != base
    errors, _ = check(m, raw_files)
    expect("review_outcome != ACCEPTED 인데 PASS", errors, "review_outcome")

    # ── 3. 검수 컨텍스트가 새 세션이 아님 ─────────────────────────
    m = base.replace('review_context: "fresh-read-only"',
                     'review_context: "same-session"')
    assert m != base
    errors, _ = check(m, raw_files)
    expect("review_context != fresh-read-only", errors, "review_context")

    # ── 4. 원문이 digest 기록 이후 변조됨 ─────────────────────────
    tampered = dict(raw_files)
    tampered["DoD-09_resume_selection.txt"] = \
        raw_files["DoD-09_resume_selection.txt"].replace(b"4 failed", b"0 failed")
    assert tampered["DoD-09_resume_selection.txt"] != raw_files["DoD-09_resume_selection.txt"]
    errors, _ = check(base, tampered)
    expect("원문 수기 변조 (digest 불일치)", errors, "raw_output_digest 불일치")

    # ── 5. 바이트 수만 틀림 ───────────────────────────────────────
    m = re.sub(r"raw_output_bytes: \d+", "raw_output_bytes: 1", base)
    assert m != base
    errors, _ = check(m, raw_files)
    expect("raw_output_bytes 불일치", errors, "raw_output_bytes 불일치")

    # ── 5b. 줄바꿈만 다른 원문 — 체크아웃마다 다른 같은 원문이다 ──
    #   ★ 2026-09-17 — `core.autocrlf` 로 체크아웃마다 작업 트리 바이트가 다르다. 검사기는 LF 정규화 내용으로 잰다.
    #     줄바꿈만 다르면 통과하고, 내용을 바꾸면 **어느 형태로든** 실패해야 한다.
    key = "DoD-09_resume_selection.txt"
    lf_raw = {k: v.replace(b"\r\n", b"\n") for k, v in raw_files.items()}
    crlf_raw = {k: v.replace(b"\n", b"\r\n") for k, v in lf_raw.items()}
    for label, files in (("LF", lf_raw), ("CRLF", crlf_raw)):
        errors, _ = check(base, files)
        expect("줄바꿈만 다른 원문(%s 체크아웃)은 통과" % label, errors, "raw_output_", want=False)
        tampered = dict(files)
        tampered[key] = files[key].replace(b"4 failed", b"0 failed")
        assert tampered[key] != files[key]
        errors, _ = check(base, tampered)
        expect("변조한 원문(%s 체크아웃)은 실패" % label, errors, "raw_output_digest 불일치")
    parts = lf_raw[key].split(b"\n")
    mixed = dict(lf_raw)
    mixed[key] = b"\n".join(
        part + (b"\r" if i % 2 == 0 and i < len(parts) - 1 else b"") for i, part in enumerate(parts))
    assert b"\r\n" in mixed[key] and mixed[key] != lf_raw[key]
    errors, _ = check(base, mixed)
    expect("줄바꿈이 섞인 원문은 통과", errors, "raw_output_", want=False)
    lone = dict(lf_raw)
    lone[key] = lf_raw[key].replace(b"\n", b"\r", 1)
    assert lone[key] != lf_raw[key]
    errors, _ = check(base, lone)
    expect("줄바꿈 하나를 외로운 CR 로 바꾼 원문은 실패(내용이다)", errors, "raw_output_digest 불일치")

    # ── 6. 검수 receipt 가 형식적 LGTM ────────────────────────────
    lgtm = dict(raw_files)
    lgtm["DoD-09_review.txt"] = b"LGTM\n"
    errors, _ = check(base, lgtm)
    expect("형식적 LGTM (너무 짧은 receipt)", errors, "review_artifact 가 너무 짧다")

    # ── 6b. 길지만 파일:줄 위치가 없는 receipt ────────────────────
    #   ★ 2026-08-17 — 전에는 경고였다. 경고는 종료 코드에 반영되지 않아
    #     "검사기가 통과했다" 로 읽힌다. 오류로 승격했다.
    vague = dict(raw_files)
    vague["DoD-09_review.txt"] = ("전반적으로 좋아 보입니다. " * 40).encode("utf-8")
    errors, _ = check(base, vague)
    expect("구체적 반례 없는 receipt", errors, "파일:줄 위치가 하나도 없다")

    # ── 6c. 검수자가 실제로 만든 우회 payload ─────────────────────
    #   `("LGTM. " * 40) + "fake.py:1"` — 길이도 넘고 정규식도 만족한다.
    #   가리키는 파일이 저장소에 없다는 것으로 잡는다.
    bypass = dict(raw_files)
    bypass["DoD-09_review.txt"] = (("LGTM. " * 40) + "fake.py:1").encode("utf-8")
    errors, _ = check(base, bypass)
    expect("형식적 LGTM + 없는 파일 인용", errors, "인용한 파일이 저장소에 하나도 없다")

    # ── 6d. review_artifact 가 `_raw/` 밖 ─────────────────────────
    m = base.replace('review_artifact: "docs/evidence/_raw/DoD-09_review.txt"',
                     'review_artifact: "scripts/verify_evidence.py"')
    m = m.replace("  - docs/evidence/_raw/DoD-09_review.txt",
                  "  - scripts/verify_evidence.py")
    assert m != base
    errors, _ = check(m, raw_files)
    expect("review_artifact 가 _raw/ 밖", errors, "아래여야 한다")

    # ── 6e. 중복 키 ───────────────────────────────────────────────
    m = base.replace(
        'review_outcome: "ACCEPTED"',
        'review_outcome: "CHANGES_REQUESTED"\nreview_outcome: "ACCEPTED"')
    assert m != base
    errors, _ = check(m, raw_files)
    expect("중복 키", errors, "중복 키")

    # ── 6f. 보이지 않는 문자로 위장한 검수자 ──────────────────────
    m = base.replace('reviewer_id: "agent:codex-cli"',
                     'reviewer_id: "agent:claude-code​"')
    assert m != base
    errors, _ = check(m, raw_files)
    expect("제로폭 문자로 위장한 검수자", errors, "보이지 않는 문자")

    # ── 6g. id 와 파일명 불일치로 검수 강제 회피 ──────────────────
    m = base.replace('review_outcome: "ACCEPTED"',
                     'review_outcome: "CHANGES_REQUESTED"')
    errors, _ = check(m, raw_files, filename="ENV-99_disguised.md",
                      grandfathered=frozenset())
    expect("파일명 위장 — id 로도 강제된다", errors, "review_outcome")
    expect("파일명 위장 — 불일치 자체가 오류", errors, "시작하지 않는다")

    # ── 6h. 경로에 콜론 (NTFS ADS) ────────────────────────────────
    m = base.replace(
        'raw_output_artifact: "docs/evidence/_raw/DoD-09_resume_selection.txt"',
        'raw_output_artifact: "docs/evidence/_raw/DoD-09_resume_selection.txt::$DATA"')
    assert m != base
    errors, _ = check(m, raw_files)
    expect("NTFS ADS 경로", errors, "콜론")

    # ── 6i. BOM ───────────────────────────────────────────────────
    errors, _ = check("﻿" + base, raw_files)
    expect("BOM 으로 시작하는 파일", errors, "BOM")

    # ── 7. 검수 receipt 가 artifacts 목록에 없음 ──────────────────
    m = base.replace("  - docs/evidence/_raw/DoD-09_review.txt\n", "")
    assert m != base
    errors, _ = check(m, raw_files)
    expect("review_artifact 가 artifacts 에 없음", errors, "artifacts 목록에 없다")

    # ── 8. 경로 탈출 ──────────────────────────────────────────────
    m = base.replace('raw_output_artifact: "docs/evidence/_raw/DoD-09_resume_selection.txt"',
                     'raw_output_artifact: "docs/evidence/_raw/../../../etc/passwd"')
    assert m != base
    errors, _ = check(m, raw_files)
    expect("raw_output_artifact 경로 탈출", errors, "경로 탈출")

    m = base.replace("  - crates/checkpoint/src/writer.rs",
                     "  - ../../../etc/passwd")
    assert m != base
    errors, _ = check(m, raw_files)
    expect("artifact 경로 탈출", errors, "경로 탈출")

    # ── 9. 신원 형식 ──────────────────────────────────────────────
    m = base.replace('reviewer_id: "agent:codex-cli"', 'reviewer_id: "codex"')
    assert m != base
    errors, _ = check(m, raw_files)
    expect("reviewer_id 접두사 없음", errors, "reviewer_id 는")

    # ── 10. v2 필드 누락 ──────────────────────────────────────────
    for field in VE.V2_FIELDS:
        m = re.sub(r"(?m)^%s:.*(\n(?=\S|\s*$))?" % re.escape(field), "", base, count=1)
        if m == base:
            FAILURES.append("테스트 준비 실패 — %s 를 지우지 못했다" % field)
            print("  FAIL v2 필드 누락: %s (준비 실패)" % field)
            continue
        errors, _ = check(m, raw_files)
        expect("v2 필드 누락: %s" % field, errors, field)

    # ── 11. 유예 목록에 없는 v1 evidence ──────────────────────────
    m = base.replace("schema_version: 2", "schema_version: 1")
    assert m != base
    errors, _ = check(m, raw_files, grandfathered=frozenset())
    expect("유예 목록에 없는 새 v1 evidence", errors, "유예 목록")

    # ── 11b. 유예 목록에 있으면 v1 이어도 통과 ────────────────────
    errors, _ = check(m, raw_files, grandfathered=frozenset({PILOT}))
    expect("유예된 v1 evidence 는 v2 검사 면제", errors, "유예 목록", want=False)
    expect("유예된 v1 evidence 는 v2 필드도 면제", errors, "v2 필수 필드 누락", want=False)

    # ── 12. ENV-* 는 검수 강제 대상이 아니다 ──────────────────────
    #   규칙이 과하면 우회하게 된다. 범위를 실제로 좁혔는지 확인한다.
    saved = VE.REVIEW_REQUIRED_PREFIX
    try:
        m = base.replace('review_outcome: "ACCEPTED"',
                         'review_outcome: "CHANGES_REQUESTED"')
        # 파일명을 ENV-* 로 바꾸는 대신 접두사 판정을 그대로 쓰되,
        # PILOT 이 DoD- 로 시작하므로 강제 대상이다. 대조군으로 접두사를 비운다.
        VE.REVIEW_REQUIRED_PREFIX = ("ZZZ-",)
        errors, _ = check(m, raw_files)
        expect("강제 대상이 아니면 review_outcome 을 묻지 않는다",
               errors, "review_outcome", want=False)
    finally:
        VE.REVIEW_REQUIRED_PREFIX = saved

    # ── 12b. 사라진 negative test 이름 ────────────────────────────
    #   ★ 실제로 있었던 일: DoD-07 이 `evidence_is_not_replay_checked` 를
    #     적어 뒀는데 그 테스트는 이름이 바뀌었고 **동작도 뒤집혔다.**
    #     evidence 는 그대로 "이 테스트가 이것을 보장한다" 고 말하고 있었다.
    m = base.replace("  - w1_resume_must_not_pick_another_job",
                     "  - this_test_does_not_exist_anywhere: 없는 테스트")
    assert m != base
    _errors, warns = check(m, raw_files)
    if any("함수 정의를 찾지 못했다" in w for w in warns):
        print("  ok   사라진 negative test 이름 -> 경고")
    else:
        FAILURES.append("사라진 negative test 에 경고가 없다: %s" % warns)
        print("  FAIL 사라진 negative test 이름 -> 경고")

    # 비공허성 — 실재하는 이름은 경고하지 않는다
    _errors, warns = check(base, raw_files)
    if any("함수 정의를 찾지 못했다" in w for w in warns):
        FAILURES.append("실재하는 negative test 에 경고가 났다: %s" % warns)
        print("  FAIL 실재하는 이름은 경고하지 않는다")
    else:
        print("  ok   실재하는 이름은 경고하지 않는다")

    # ── 13. 유예 목록 자체를 손대는 경우 ──────────────────────────
    #   ★ 독립 검수(2026-08-17)가 **치명**으로 지적한 우회다.
    #     목록 파일에 "추가 금지" 라고 적어만 뒀고 검사기는 그냥 믿었다.
    #     한 줄 추가하면 새 v1 evidence 가 검수 없이 통과했다.
    tmp = tempfile.mkdtemp(prefix="gputeer-gf-")
    try:
        real = io.open(VE.GRANDFATHER_LIST, "rb").read()
        fake = os.path.join(tmp, "_schema_v1_grandfathered.txt")
        io.open(fake, "wb").write(real + b"ENV-99_fake.md\n")
        old_list = VE.GRANDFATHER_LIST
        VE.GRANDFATHER_LIST = fake
        try:
            got, errs = VE.load_grandfathered()
        finally:
            VE.GRANDFATHER_LIST = old_list
        if errs and not got:
            print("  ok   유예 목록에 한 줄 추가 -> 유예 전부 취소")
        else:
            FAILURES.append(
                "유예 목록을 늘렸는데 검사기가 통과시켰다 (오류=%s, 유예=%d건)"
                % (errs, len(got)))
            print("  FAIL 유예 목록에 한 줄 추가 -> 유예 전부 취소")

        # 비공허성 — 손대지 않은 목록은 정상 로드된다
        got, errs = VE.load_grandfathered()
        if not errs and got:
            print("  ok   손대지 않은 유예 목록은 정상 로드")
        else:
            FAILURES.append("무손상 유예 목록이 로드되지 않는다: %s" % errs)
            print("  FAIL 손대지 않은 유예 목록은 정상 로드")

        # ★ 2026-09-17 — 줄바꿈만 다른 목록(LF · CRLF 체크아웃)은 같은 목록이다. 한 줄 추가는 어느 형태로든 유예를 전부 취소한다.
        lf_list = real.replace(b"\r\n", b"\n")
        for label, data, extra in (
            ("LF", lf_list, b"ENV-99_fake.md\n"),
            ("CRLF", lf_list.replace(b"\n", b"\r\n"), b"ENV-99_fake.md\r\n"),
        ):
            for appended, want_loaded in ((b"", True), (extra, False)):
                io.open(fake, "wb").write(data + appended)
                VE.GRANDFATHER_LIST = fake
                try:
                    got, errs = VE.load_grandfathered()
                finally:
                    VE.GRANDFATHER_LIST = old_list
                name = "유예 목록(%s 체크아웃)%s" % (
                    label, " 정상 로드" if want_loaded else "에 한 줄 추가 -> 유예 전부 취소")
                if (not errs and bool(got)) == want_loaded and (bool(errs) != want_loaded):
                    print("  ok   %s" % name)
                else:
                    FAILURES.append("%s — 오류=%s, 유예=%d건" % (name, errs, len(got)))
                    print("  FAIL %s" % name)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    # ── 14. 종료 코드 — end-to-end ────────────────────────────────
    #   ★ 위 검사들은 `check_file()` 만 부른다. 독립 검수가 지적했듯
    #     그것만으로는 **경고만 나는 위반이 종료 코드 0 으로 통과하는지**
    #     알 수 없다. `main()` 을 실제로 돌려 확인한다.
    if run_main_on(base, raw_files) != 0:
        FAILURES.append("무손상 evidence 에서 main() 이 0 을 반환하지 않는다")
        print("  FAIL main() — 무손상은 0")
    else:
        print("  ok   main() — 무손상은 0")

    broken = base.replace('review_outcome: "ACCEPTED"',
                          'review_outcome: "CHANGES_REQUESTED"')
    if run_main_on(broken, raw_files) == 0:
        FAILURES.append("★ 위반이 있는데 main() 이 0 을 반환했다 — 종료 코드가 규칙을 강제하지 않는다")
        print("  FAIL main() — 위반은 1")
    else:
        print("  ok   main() — 위반은 1")

    print("-" * 62)
    if FAILURES:
        print("검사기 결함 %d건:" % len(FAILURES))
        for f in FAILURES:
            print("  ! %s" % f)
        print("\n★ 검사기가 잡지 못하는 규칙은 규칙이 아니다.")
        return 1
    print("검사기가 위 위반을 전부 잡는다.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
