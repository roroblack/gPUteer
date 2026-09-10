동의가 아니라 **반박**을 원한다. 재검수 21 이 DoD-68 초안에 **같은 부류의
문장 둘**을 짚었다 — 한 곳을 낮추고 다른 곳에서 다시 강하게 말한 것이다.
고쳤다. 다섯 번째다. 또 남았는지 봐 달라.

# 배경

```text
limitations  "plan_id 는 오늘 방어일 뿐 짐을 지고 있지 않다"
             -> "시도한 경로에서는 앞선 저장소 관문에 막혀 plan_id 제거의 영향을
                확인하지 못했다 — 이 측정으로는 방어 기여를 판정하지 않는다"
decision     "병렬 3갈래가 2026-09-03 에 실패한 원인이다"
             -> "순차로 진행한다. 병렬 세 실행은 모두 판정 전에 쿼터로 중단됐다"
★ 코드 주석  crates/cli/src/scheduler_tick.rs 의 모듈 주석에도 **같은 단정**이
             있었다(초안이 "주석을 사실로 고쳤다" 고 적은 그 주석). 같이 좁혔다
덤          재검수 20 의 단서 — Agent 에 "올바른 값의 중복은 받는다" 대조군,
             Coordinator 에 true -> false 대조군을 더했다
```

두 초안 머리의 판정 요약 블록은 과장의 기록으로 일부러 남겼다. 판정 대상이
아니다. **본문과 front-matter** 를 봐라.

# 읽을 파일 — 이것만 읽어라

```text
docs/plans/2026-09-03_1740_DoD-68_evidence_초안_검수대기.md
docs/plans/2026-09-06_0020_P0-02_evidence_초안_검수대기.md
crates/cli/src/scheduler_tick.rs           (모듈 주석과 derive_* 함수)
crates/agent/tests/unknown_flags.rs
crates/coordinator/tests/stored_lane_flags.rs   (중복 불리언 테스트만)
docs/evidence/_raw/검수_2026-09-10/21_재검수_evidence_초안_4차_CHANGES_REQUESTED.txt
```

# 물을 것 — 네 가지

1. **21 의 두 지적이 닫혔나** — 그리고 **같은 부류가 다른 자리에 또** 있나.
   DoD-68 의 claim · raw_output · negative_tests · limitations · decision · 본문,
   그리고 `scheduler_tick.rs` 주석을 서로 대조하라. 특히 "관측한 것(시도한 경로에서
   못 만들었다)" 에서 "역할·원인·불가능" 으로 넘어가는 문장을 찾아라.

2. **P0-02 는 21 이 "추가 수정 근거 없음" 이라 했다.** 머리에 그 기록만 붙였다 —
   본문이 바뀌지 않았는지, 여전히 그 판정이 유지되는지.

3. **대조 테스트 둘이 "중복이면 전부 거부" 회귀를 잡는 구조인가.**

4. **두 초안을 evidence 로 옮겨도 되나.** 안 되면 무엇이 남았나 — 문장 문제인지,
   측정이 더 필요한지 구분해 적어라.

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 반례를 못 만든
것과 결함이 없는 것은 다르다 — 그 둘을 구분해 써라.

# 답의 형식

각 지적마다 문서의 해당 줄 또는 `파일:줄` 을 인용하라.
마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
