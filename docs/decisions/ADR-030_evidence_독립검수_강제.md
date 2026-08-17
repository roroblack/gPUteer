# ADR-030 — evidence schema v2: 독립 검수 강제 · provenance · 원문 무결성

- 상태: **채택**
- 날짜: 2026-08-17
- 관련: `RULE.md` §7 · `CLAUDE.md` §4 · `docs/runbooks/ai-workflow.md` · ADR-029

---

## 배경 — 무엇이 문제였나

`docs/runbooks/ai-workflow.md` 는 AI 도구 분업 절차를 적으면서
**해결되지 않은 4가지**를 스스로 나열했다.

```text
1. 검수자가 한 모델뿐이다        다른 검수자는 다른 것을 본다
2. evidence 에 검증자 신원이 없다  누가 실행했는지 기록되지 않는다
3. raw_output 이 요약이다         _raw/ 원문과 byte 대조가 강제되지 않는다
4. 검수 자체가 선택적이다         "독립 검수를 받아라" 를 강제하는 검사가 없다
```

★ 네 가지 모두 같은 병이다 — **규범은 있는데 강제 장치가 없다.**
이 저장소가 반복해서 겪은 실패 유형이다.

## 논의

독립 검수(`agent:codex-cli`, 새 세션 · 읽기 전용)와 설계를 논의했다.
검수자가 각 항목을 "기계로 보장 가능한 것" 과 "절차로만 완화 가능한 것" 으로 갈랐다.

### 검수자가 정정한 것

**BLAKE3 를 쓰려던 최초 계획이 틀렸다.**

저장소 나머지가 BLAKE3 를 쓰므로 evidence digest 도 BLAKE3 로 하려 했다.
그러나 `verify_evidence.py` 는 **Python 표준 라이브러리만 쓴다**는 조건이 있고,
표준 라이브러리에 BLAKE3 가 없다. `cargo` 나 외부 패키지에 의존시키면
검사기를 못 돌리는 환경이 생긴다 — **검사기를 못 돌리면 규칙도 없다.**

→ evidence digest 는 `hashlib.sha256` 을 쓴다.
   프로토콜 digest 는 Rust 에서 계속 BLAKE3 다. **용도가 다르다.**

## 결정

evidence front-matter 에 `schema_version: 2` 를 도입하고, 다음을 강제한다.

```yaml
schema_version: 2

executor_id:   "agent:claude-code"     # 누가 실행했는가
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-opus-5"
executed_at:   "2026-08-17T11:05:00+09:00"

review_required: true
reviewer_id:     "agent:codex-cli"     # ★ executor_id 와 달라야 한다
reviewer_tool:   "codex exec --sandbox read-only ..."
reviewer_model:  "gpt-5.6-luna"
review_context:  "fresh-read-only"     # 새 세션 · 저장소 쓰기 권한 없음
review_outcome:  "ACCEPTED"
review_scope:    "무엇을 검수했는가"
review_artifact: "docs/evidence/_raw/DoD-09_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-09_resume_selection.txt"
raw_output_digest:   "sha256:e525d3d3..."
raw_output_bytes:    5138
```

### 강제 범위

```text
P0-* · DoD-* 의 status: PASS   -> 독립 검수 필수
FAIL-* · INCONCLUSIVE          -> 필수 아님 (권장)
ENV-*                          -> 검수 생략 허용
```

★ **범위를 좁힌 이유**: 단순 환경 기록까지 검수시키면 과하다.
규칙이 과하면 우회하게 된다 — 그러면 규칙이 없는 것보다 나쁘다.

### 유예 — 기존 16건

기존 evidence 16건은 v1 이다. "새 evidence 부터 v2" 라고 적는 것만으로는
기계가 **"새 것"** 을 모른다.

→ `docs/evidence/_schema_v1_grandfathered.txt` 에 정확히 그 16개를 적고,
  **목록에 없는 v1 evidence 는 오류**로 만든다.

★ 그리고 목록 파일의 SHA-256 을 `verify_evidence.py` 에 **상수로 박았다.**
  목록을 늘리려면 코드도 고쳐야 하고, 그러면 **유예 확대가 diff 에 드러난다.**

  (독립 검수가 이 구멍을 **치명**으로 지적했다 — 처음에는 목록 파일에
   "추가 금지" 라고 적어만 뒀고 검사기는 그냥 믿었다.
   검수자가 실제로 `ENV-99_fake.md` 를 한 줄 추가해 통과시켰다.)

## 검사기가 실제로 막는 것 — 그리고 막지 못하는 것

★ **이 절이 이 ADR 에서 가장 중요하다.**
막지 못하는 것을 적지 않으면 "검수를 강제했다" 가 "주장이 참이다" 로 읽힌다.

### 막는다 (기계가 판정한다)

```text
검수 기록 누락                     v2 필수 필드 12개
자기 검수                          executor_id == reviewer_id -> 오류
눈으로 같은 두 신원                제로폭 · 서식 · 제어문자 · 비ASCII 거부
검수 미수용인데 PASS               review_outcome != ACCEPTED -> 오류
같은 세션에서의 검수               review_context != fresh-read-only -> 오류
형식적 LGTM                        200자 미만 · 파일:줄 없음 · 인용 파일 부재 -> 오류
검수 receipt 위치 위장             _raw/ 밖이면 오류 (소스 파일을 receipt 로 쓰던 우회)
원문 사후 변조                     sha256 · byte 수 대조
파일명 위장으로 검수 회피          id 와 파일명이 불일치하면 오류
중복 키                            사람이 읽는 값 != 검사기가 보는 값 -> 오류
경로 탈출 · symlink · NTFS ADS     realpath 로 저장소 안인지 대조
유예 목록 확대                     목록 digest 불일치 -> 유예 전부 취소
```

### 막지 못한다 (사람의 책임이다)

```text
명령이 실제로 실행됐는가            원문은 손으로 만들 수 있다
원문이 그 명령의 출력인가           digest 는 무결성이지 진실성이 아니다
raw_output 요약이 정직한가          요약과 원문의 의미 일치는 기계가 못 본다
검수자가 정직했는가                 반례를 알고도 숨기면 알 수 없다
두 모델이 정말 독립인가             같은 학습 편향을 공유할 수 있다
원문과 digest 를 함께 고친 경우     둘 다 고치면 통과한다
```

**진짜 위조 방지는 Git 서명이나 외부 실행 시스템이 필요하다.**
개발자 1명 · 로컬 실행 환경에는 과하다 — 그래서 여기까지다.

## 대안과 그것을 고르지 않은 이유

| 대안 | 왜 안 골랐나 |
|---|---|
| Ed25519 로 evidence 자체를 서명 | 키 관리(§11)가 아직 없다. 없는 것 위에 규칙을 쌓지 않는다 |
| 모든 evidence 에 검수 강제 | 과하면 우회한다. ENV-* 까지 검수시킬 이유가 없다 |
| v1 을 전부 v2 로 소급 | 지금 없는 검수 기록을 만들어 내는 것은 위조다 |
| 유예 없이 즉시 전환 | 기존 16건이 전부 오류가 되어 검사기를 끄게 된다 |
| digest 없이 검수만 강제 | 같은 모델이 자기 결론을 승인하는 형식 절차가 된다 |

## 남은 부채

`verify_evidence.py` 가 매번 출력한다.

```text
★ 독립 검수 기록이 없는 P0/DoD PASS: 13건 (schema v1 유예)
  이들은 '검수를 통과했다' 가 아니라 '검수하지 않았다' 이다.
```

★ **부채를 조용히 두지 않는다.** 숫자가 보이지 않으면 영원히 유예된다.
줄어드는 것이 진전이고, 늘어나는 것은 규칙 위반이다.

## 검증

```text
scripts/test_verify_evidence.py   40건 — 검사기의 부정 테스트
docs/evidence/DoD-09_*.md         v2 최초 적용 (실제 독립 검수 기록 포함)
```

★ 부정 테스트 자체도 독립 검수를 받았고, **공허한 것 5건**을 지적받아 고쳤다.
  (`check_file()` 만 부르고 종료 코드를 안 봤다 · 부분문자열 하나로 판정했다 ·
   파일명 우회를 실제로 시험하지 않고 전역 상수를 바꿨다 등.)
