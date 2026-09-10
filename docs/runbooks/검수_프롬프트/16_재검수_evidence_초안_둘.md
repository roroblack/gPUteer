동의가 아니라 **반박**을 원한다. 검수 7·8 이 두 evidence 초안의 과장을
짚었고, 본문을 고쳤다. 고친 본문이 **또 측정보다 강한지** 봐 달라.

# 배경

```text
DoD-68 (7번)  claim 을 "네 고리" -> "셋" 으로. 데몬화와 실행이 아직이라는
              것을 claim 안에 뒀다. commit 을 0d6a6ed 로 바꾸고 옛 측정
              (f79b485)과 새 측정을 raw_output 에서 갈랐다. 뮤테이션을
              1차 9건 -> 누적 22건(21 잡힘)으로. 부정 테스트 셋에 파일:줄과
              특정하는 이유. 예약 해제 불가 · 노드 단위 잠금을 한계에 넣었다
P0-02 (8번)   "동작한다" -> "시험한 환경에서 나열한 동작 확인". ole32 인과 ·
              combase 기능 주장을 낮췄다. "소거한 가설" -> "시험한 네 변경".
              "여섯 번 같은 자리" -> 여섯 단계의 탐색. decision 에서 범위를
              갈라 좁은 구성은 FAIL-SCOPE 근거가 있다고 적었다.
              가둠은 문장을 낮추는 대신 **대조를 다시 돌려 원문을 만들었다**
              (개발 기계)
```

두 초안 머리의 판정 요약 블록은 **일부러 남겼다** — 무엇이 과장이었는지의
기록이다. 그 블록은 판정 대상이 아니다. **본문과 front-matter** 를 봐라.

# 읽을 파일 — 이것만 읽어라

```text
docs/plans/2026-09-03_1740_DoD-68_evidence_초안_검수대기.md
docs/plans/2026-09-06_0020_P0-02_evidence_초안_검수대기.md
docs/evidence/_raw/P0-02_가둠_대조_재실측_2026-09-10.txt
docs/evidence/_raw/검수_2026-09-10/07_DoD-68_evidence_CHANGES_REQUESTED.txt
docs/evidence/_raw/검수_2026-09-10/08_P0-02_evidence_CHANGES_REQUESTED.txt
crates/cli/tests/plan_job.rs          (서명자 테스트만)
crates/cli/tests/issue_grant.rs       (Grant 가 Lease 보다 오래 테스트만)
crates/cli/tests/stage_job.rs         (두 번째 Job 예약 테스트만)
crates/runtime-windows/src/appcontainer.rs   (가둠 테스트만)
```

# 물을 것 — 네 가지

1. **7·8 의 지적이 본문에서 닫혔나.** 지적마다 대조하라. 빠진 것이 있나?

2. **고친 문장이 또 강한가.** 특히:
   - DoD-68 의 새 claim 이 raw_output 과 limitations 가 받치는 범위 안인가
   - P0-02 의 "이 범위로는 FAIL-SCOPE 를 뒷받침한다" 가 정당한가, 아니면 다른
     방향의 과장인가(status 는 여전히 INCONCLUSIVE 다)

3. **인용이 맞나.** DoD-68 이 든 `파일:줄` 세 곳에 그 테스트가 실제로 있고,
   적힌 이유 문자열을 **특정해서** 단언하나? P0-02 의 새 원문이 주장(바깥 exit 0 ·
   안쪽 exit 1 · 뮤테이션 시 안쪽 exit 0)과 맞나? 그 원문이 **개발 기계**에서
   나왔다는 한계가 본문에 적혔나?

4. **이 두 문서를 evidence 로 옮겨도 되나** — 안 되면 무엇이 남았나.

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 반례를 못 만든
것과 결함이 없는 것은 다르다 — 그 둘을 구분해 써라.

# 답의 형식

각 지적마다 문서의 해당 줄을 인용하고, 왜 측정을 넘어서는지 쓰라.
마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
