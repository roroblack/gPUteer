# docs/ — 문서 지도

> **최종 갱신** 2026-09-05 · **성격** 지도(폴더 배치 규칙의 소유 문서) ·
> **검사** `scripts/check_docs.py` 가 폴더·파일명·중복을 강제한다

## 계층 — 각 폴더가 답하는 질문

문서를 종류가 아니라 **답하는 질문**으로 나눈다.
**같은 질문에 두 폴더가 답하면 그것이 결함이다.**

```text
../proto/*.proto        무엇을 주고받는가            (규범 · 와이어 스키마)
protocol/               어떻게 검증하고 전이하는가    (규범 · 서명 · 상태 전이)
contracts/              누가 무엇을 구현·검증하는가   (소유권 · 변경 절차)
../../gputeer_master_plan_FINAL.md
                        왜 그렇게 결정했는가          (아키텍처 근거 · 읽기 전용)
plans/                  무엇을 언제 할 것인가
evidence/               실제로 입증되었는가           (재현 명령 + 실제 출력)
reports/                무엇을 수행했는가
reports/debugs/         어떤 결함이 있었는가
decisions/              왜 그 결정을 바꿨는가         (ADR)
history/                언제 무엇을 했는가            (추가만, 수정 금지)
                          HISTORY.md      날짜 순 일지
                          조각_이력.md    조각 번호 순 — 검수가 무엇을 찾았나
runbooks/               운영·복구는 어떻게 하는가
vision/                 지금 안 하는 것과 그 트리거
manuals/                환경 구축 절차
```

## 우선순위 — 충돌하면 무엇이 이기나

```text
1. ../CLAUDE.md §0          안전 원칙 (소유자 주권 · 서명 검증 · 데이터 손실 방지)
2. ../RULE.md               프로세스 규칙
3. ../proto/ · protocol/    규범 스키마 · 규범 절차
4. 기준선 계획서            아키텍처 근거
5. plans/                   실행 계획
```

**위가 아래를 이긴다.** 예외 둘:

- 계획서와 규범 문서가 충돌하면 **규범 문서가 이긴다** (계획서는 근거, 규범은 계약)
- 안전 원칙은 어떤 프로세스 규칙보다 앞선다

## ★ 중복 금지

`contracts/` 는 **`proto/` 를 복제하지 않는다.**

```text
금지   필드 목록을 Markdown 으로 옮겨 적기
금지   상태 전이표를 두 곳에 두기
금지   서명 알고리즘을 다시 설명하기

허용   원본 링크 · 소유 스트림 · 변경 절차 · 테스트 위치
```

복제하면 반드시 drift 가 생기고, **어느 쪽이 진짜인지 알 수 없게 된다.**

## ★ 문서 머리말 (2026-09-05 추가)

규범·지도 문서는 **첫 제목 바로 아래에 한 문단**으로 이 넷을 적는다.
낡았는지 열어 보지 않고 알 수 있어야 하기 때문이다.

```text
최종 갱신   YYYY-MM-DD — 실제로 손댄 날. 자동이 아니므로 고칠 때 같이 고친다
성격        규범 / 지도 / 일지 / 기록 — 무게가 다르다는 것을 먼저 말한다
짝          같은 주제를 다른 각도로 보는 문서 (있으면)
검사        무엇이 이 문서를 기계로 강제하는가 (없으면 "없음" 이라 적는다)
```

★ `evidence/` 는 예외다 — 이미 front-matter 27개 필드가 그 일을 한다.

## 파일명

```text
기본          YYYY-MM-DD_HHmm_<제목>.md
evidence/     DoD-NN_<항목>.md  또는  P0-NN_<항목>.md
decisions/    ADR-NNN_<제목>.md
contracts/    NN_<제목>.md        (시점이 아니라 순서로 읽는다)
vision/       VISION-NN_<제목>.md (시점이 아니라 주제로 읽는다)
```

`contracts/` · `decisions/` · `vision/` 은 갱신 시 **새 파일을 만들지 않고 같은 번호를 고치고**
문서 안에 개정 이력을 남긴다.

## 템플릿

각 폴더의 `_TEMPLATE.md` 를 복사해서 쓴다. 템플릿의 필수 항목을 비우지 않는다.

| 폴더 | 템플릿 |
|---|---|
| `evidence/` | `_TEMPLATE.md` — front-matter **필수 15 + schema v2 12 = 27개**. `scripts/verify_evidence.py` 가 검사 |
| `decisions/` | `_TEMPLATE.md` — ADR 형식 |
| `reports/` | `_TEMPLATE.md` |
| `reports/debugs/` | `_TEMPLATE.md` |
| `plans/` | `_TEMPLATE.md` |

## 검사

```bash
python scripts/verify_evidence.py      # evidence front-matter 스키마 검사
python scripts/check_docs.py           # 문서 구조·파일명·중복 검사
```

**`scripts/verify_evidence.py` 는 "파일이 있다"가 아니라 "재현 가능한 기록이 완전하다"를 검사한다.**
`limitations` 가 비어 있으면 반려된다.

## ★ evidence 에 **넣을 수 없는** 상태가 있다 (2026-09-05 추가)

schema v2(`ADR-030`)에는 **"측정은 끝났고 독립 검수만 없다" 는 상태가 없다.**
`review_artifact` 는 필수인데 그 내용 검사가 이렇게 요구한다:

```text
! review_artifact 에 파일:줄 위치가 하나도 없다 —
  구체적 반례 없는 검수는 형식적 승인이다
```

즉 **검수를 못 받았으면 `docs/evidence/` 에 둘 수 없다.** 이것은 결함이
아니라 설계다 — 그 칸을 아무 로그로나 채우면 "검수를 강제한다" 가
"주장이 참이다" 로 읽히기 시작한다.

★ **그때는 `plans/` 에 초안으로 둔다.** 파일 이름에 `_검수대기` 를 붙이고,
  머리에 HTML 주석으로 "이것은 아직 evidence 가 아니다" 와 **왜 그런지**를
  적는다. 검수가 `ACCEPTED` 를 내면 `review_*` 를 채워 `evidence/` 로 옮기고
  `status` 를 `PASS` 로 올린다.

  ★ `INCONCLUSIVE` 로 우회하지 않는다 — 그건 "측정이 안 끝났다" 는 뜻이라
    **다른 상태**다. 둘을 같은 이름으로 부르면 나중에 구분이 사라진다.

  실제 사례: `plans/2026-09-03_1740_DoD-68_evidence_초안_검수대기.md`
