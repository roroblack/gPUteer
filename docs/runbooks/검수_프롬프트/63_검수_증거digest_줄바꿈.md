동의가 아니라 **반박**을 원한다.

작업 위치: worktree `fix/evidence-digest-eol`. 검수 대상은 ce880d5 다음 커밋 — `git show HEAD` · `git diff ce880d5..HEAD`.

# 증거 digest 를 체크아웃 줄바꿈과 무관하게

```text
scripts/verify_evidence.py       원문 · 유예 목록 digest 를 LF 정규화(CRLF -> LF, 외로운 CR 유지) 내용으로 잰다
docs/evidence/DoD-20 61 62 63    digest · bytes 새 정의로 재기록(섞인 줄바꿈으로 기록돼 있었다)
GRANDFATHER_DIGEST               새 정의 값(목록 내용 불변)
scripts/test_verify_evidence.py  줄바꿈만 다른 원문 통과 · 변조 실패 · 유예 목록 LF · CRLF
```

물을 것:
1. **변조 탐지가 줄지 않았나** — LF 정규화 때문에 새로 통과하는 변조가 줄바꿈 말고 있나(예: 내용을 CRLF 로 감싸 의미를 바꾸는 경우 ·
   `raw_output` 요약 대조 · receipt 인용 검사와의 상호작용).
2. 4건 재기록의 근거가 충분한가 — "메인 체크아웃의 원본 바이트 digest = 옛 기록 · LF 정규화 = 커밋된 blob" 이 내용 불변의 증명인가.
   재기록이 "digest 를 원문에 맞춰 고친다"(RULE.md §7.3 이 막으려는 것)와 어떻게 다른가.
3. 대안 A(git blob) · C(.gitattributes) 를 버린 판단 — 반례가 있나.
4. 부정 테스트가 공허하지 않은가 — E1 에서 "CRLF 원문 통과" 줄이 CRLF worktree 에서는 실패하지 않은 이유(기준선 오류 필터)가 테스트의 약점인가.
5. CLAUDE.md · RULE.md 에 "이제 어느 체크아웃에서도 같다" 를 증명한 범위보다 넓게 적은 문장이 있나(잰 것: 이 worktree CRLF · LF 내보내기 · 메인의 섞인 원본 4건).

읽을 파일:

```text
docs/reports/debugs/2026-09-17_0513_증거_digest_가_체크아웃_줄바꿈에_따라_갈린다.md
docs/plans/2026-09-17_0513_증거_digest_줄바꿈_독립.md
scripts/verify_evidence.py · scripts/test_verify_evidence.py · RULE.md §7.3 · CLAUDE.md §5(DoD 최신 집계 행)
docs/evidence/DoD-20_*.md · DoD-61_*.md · DoD-62_*.md · DoD-63_*.md · docs/evidence/_schema_v1_grandfathered.txt
docs/evidence/_raw/증거_digest_줄바꿈_실측_2026-09-17.txt
```

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 반례를 못 만든 것과 결함이 없는 것은 다르다.

# 답의 형식

각 지적마다 `파일:줄` 을 인용하라.
마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
