---
schema_version: 2
id: DoD-66
claim: "`DoD-56` 이 **스스로** 남긴 검증 부채 셋을 닫는다 — 그 evidence 는 '이 조각의 NVML 쪽 주장들은 뮤테이션으로 검증하지 못했다. 다음 세 주장은 실측 1회로 받침될 뿐 자동 검사로 고정되지 않았다' 고 적었다(UUID 정렬의 열거 순서 무관성 · 96바이트 버퍼 상한 · MIG 를 `current` 로 판정). ★ 셋 다 **실물 GPU 없이 재는 방법**으로 고정했고, 핵심은 **떼어냈다**는 것이다 — 전에는 FFI 호출과 관측 경로 안에 묻혀 있어 GPU 없이는 잴 수 없었고, 그래서 정렬 테스트는 **관측 결과를 다시 정렬해 자기와 비교**하는(정렬을 통째로 지워도 통과하는) 공허한 것이었다. 이제 `normalize_gpu_order()` 는 합성 GPU 4장의 **서로 다른 24개 순열 전부**로, `mig_enabled_from_modes()` 는 두 값을 어긋나게 준 입력으로, `read_c_string()` 은 NUL 없는 96바이트 합성 버퍼로 잰다. ★ **실물 GPU 실측은 이 조각에 없다** — 여기서 고정한 것은 규칙이 순서·경계·모드 선택에 대해 어떻게 행동하는가이지 x600 의 GPU 가 무엇을 돌려주는가가 아니다"
status: PASS
commit: PENDING

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — 순수 함수 추출 2건, 테스트 7건, 뮤테이션 7건, 독립 검수 3라운드"
executor_model: "claude-opus-5"
executed_at: "2026-09-01T18:20:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 3라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: >
  3라운드. 매 라운드가 **테스트를 무력화하는 자리**를 찾았고 3라운드에서
  지적 없이 `ACCEPTED`.

  1R: **실제 결함 1건** — 24개 순열 테스트가 기대 행을 `normalize_gpu_order()`
  로 만들었다. UUID 와 index 는 리터럴로 대조했지만 `name`·VRAM·
  capability·MIG 는 **검수 대상 함수의 출력에서** 왔다 — 그 함수가 그
  필드들을 일관되게 훼손하면 기대값도 함께 훼손돼 통과한다. 서술 1건도
  같은 것("행 전체를 비교하므로 다른 필드를 뒤섞어도 잡는다" 가 사실보다
  강했다).
  2R: **실제 결함 1건** — 순열 생성기가 **서로 다른** 24개를 돌려주는지
  확인하지 않았다. 개수만 세므로 생성기가 정렬된 같은 순열을 24번
  돌려줘도 전부 통과하고, 그러면 정렬 제거 뮤테이션조차 안 잡힌다.
  ★ 이건 새로운 종류다 — 지금까지는 "본 테스트가 느슨하다" 였는데
  이번은 **테스트 헬퍼가 거짓말하면 본 테스트가 통째로 무의미해진다**.
  서술 2건 — `CUDA_DEVICE_ORDER` 가 NVML index 를 바꾼다고 적었는데 그건
  **CUDA** 의 열거 순서를 바꾸는 변수다(정렬이 필요한 진짜 근거는 NVML
  열거 순서가 재부팅 사이에 안정적이지 않다는 것이다), 그리고 "GPU 한
  장으로도" → 실제로는 **실물 GPU 없이** 합성 4장으로 잰다.
  3R: **지적 없음 — `ACCEPTED`.** 실제 결함 0 · 서술 0 · 기대값 자기참조
  0 · 헬퍼가 검증을 무력화하는 자리 0.
review_artifact: "docs/evidence/_raw/DoD-66_review_all_rounds_verbatim.txt"

decision: >
  **묻혀 있으면 잴 수 없고, 못 재면 테스트가 공허해진다.** 이 조각의
  전부가 그 문장이다. 정렬·MIG 판정은 FFI 호출과 관측 경로 안에 있었고,
  둘 다 실물 GPU 를 요구한다. 그래서 `DoD-56` 은 "실측 1회" 로만 받쳤고
  정렬 테스트는 **관측 결과를 다시 정렬해 자기와 비교**하는 공허한
  것이었다 — 정렬을 통째로 지워도 통과한다.

  떼어내니 **GPU 한 장도 필요 없어졌다.** 합성 GPU 4장의 서로 다른 24개
  순열 전부를 넣을 수 있고, MIG 의 두 값을 어긋나게 줄 수 있고, NUL 없는
  96바이트 버퍼를 만들 수 있다.

  ★ **`index` 는 정렬이 건드리지 않는다.** 그건 NVML 이 준 관측 사실이고
  정렬은 보는 순서일 뿐이다. 둘을 섞으면 관측과 표현이 뒤엉킨다.

  ★ **`pending` 은 의도적으로 안 쓴다.** 재부팅 뒤에 적용될 값이라 그걸로
  지금을 판단하면 아직 분할되지 않은 GPU 를 분할됐다고 보거나 그 반대가
  된다. 함수 안에 `let _ = pending;` 을 남겨 두는 이유는 그 선택이 실수가
  아니라 결정임을 코드로 보이기 위해서다.

  ★ **이 조각은 실물 GPU 실측이 아니다.** 규칙이 순서·경계·모드 선택에
  대해 어떻게 행동하는가를 고정할 뿐, x600 의 RTX 4070 SUPER 가 무엇을
  돌려주는가는 `DoD-56` 의 1회 실측 그대로다. 그 둘을 같은 것으로 세지
  않는다(`CLAUDE.md` §4).
raw_output_artifact: "docs/evidence/_raw/DoD-66_nvml_invariants_2026-09-01.txt"
raw_output_digest: "sha256:19765c4e668017cc97f06137ba232f082f4b71324b3a571d60d68275bc4ab3e0"
raw_output_bytes: 4819

artifacts:
  - "docs/evidence/_raw/DoD-66_nvml_invariants_2026-09-01.txt"
  - "docs/evidence/_raw/DoD-66_review_all_rounds_verbatim.txt"

binary_digests:
  toolchain: "Windows 개발 기계 cargo 1.97.1"
protocol_versions:
  schema_version: "proto 변경 없음"
  canonical_spec: "canonical 벡터 변경 없음(52건 그대로)"
platform: >
  Windows 개발 기계(NVIDIA GPU 없음)에서만 측정했다 — **그것이 이 조각의
  요점이다.** 신규 테스트 7건은 전부 합성값으로 돌므로 GPU 유무와 무관하며,
  Linux 에서도 같은 crate 테스트로 돈다.
hardware: "GPU 없음 — 합성값만 쓴다"
network_profile: "없음"
command: |
  cargo test -p gputeer-runtime-nvml
  cargo test --workspace
  # 뮤테이션 7건 (raw 4절)

raw_output: |
  === cargo test -p gputeer-runtime-nvml ===
  9 passed / 0 failed

  === cargo test --workspace ===
  passed=833  failed=0  errors=0
    (이 조각 착수 전 826 — 신규 7건이 늘어난 전부이고 회귀 0)

  === 뮤테이션 7건 — 전부 지정 테스트를 동작 수준에서 실패시켰다 ===
  N1  정렬을 통째로 제거          N5  NUL 없으면 마지막을 잘라 씀
  N2  정렬 기준을 index 로         N6  정렬이 name 을 지움
  N3  MIG 를 pending 으로 판정     N7  생성기가 같은 순열 24번
  N4  버퍼를 64 로 되돌림

  ★ N6 은 1라운드 수정 뒤에야, N7 은 2라운드 수정 뒤에야 잡힌다.
    고치기 전에는 둘 다 통과했다.

negative_tests:
  - "정렬을 지우면 24개 순열 테스트가 실패한다(N1)"
  - "정렬 기준을 index 로 바꾸면 실패한다(N2)"
  - "정렬이 name 을 훼손하면 실패한다(N6) — 기대값을 입력에서 만든 뒤에야 잡힌다"
  - "순열 생성기가 서로 다른 24개를 안 돌려주면 실패한다(N7)"
  - "합성 입력이 이미 정렬돼 있지 않음을 대조한다 — 아니면 정렬을 재지 못한다"
  - "MIG 를 pending 으로 판정하면 실패한다(N3) — 두 값을 어긋나게 준다"
  - "버퍼 상한을 64 로 되돌리면 실패한다(N4) — 값을 리터럴 96 으로 고정"
  - "NUL 없는 96바이트 이름은 오류다 — 잘린 문자열을 값으로 쓰지 않는다"
  - "95바이트+NUL 은 정상으로 읽힌다 — '전부 거부' 로 바꿔도 통과하는 것을 배제"
  - "UUID 버퍼도 같은 경계를 갖는다 — 한쪽만 고친 회귀 방지"

limitations: >
  - **실물 GPU 실측이 아니다.** 규칙의 행동만 고정한다. x600 의 RTX 4070
    SUPER 가 실제로 무엇을 돌려주는가는 `DoD-56` 의 1회 실측 그대로다.
  - **NVML 이 96바이트를 넘는 이름을 줄 수 있는가는 여전히 모른다.**
    이 조각이 고정한 것은 "버퍼 상한이 NVML v2 값과 같다" 와 "NUL 이 없으면
    잘린 값을 쓰지 않고 오류로 낸다" 뿐이다.
  - **MIG 장치를 실제로 본 적이 없다.** `current` 로 판정한다는 계약만
    고정했고, 실제 MIG GPU 에서 NVML 이 무엇을 돌려주는지는 미측정이다.
  - `normalize_gpu_order()` 는 **공개 함수**지만 production 소비자는
    `observe_with()` 하나다. 다른 곳에서 부를 이유가 생기기 전까지는 그렇다.
---

# DoD-66 — NVML 불변식을 실물 GPU 없이 고정

`DoD-56` 이 스스로 남긴 검증 부채 셋을 닫는다. 자세한 내용은 위
frontmatter 의 `claim` · `decision` · `limitations` 를 본다.

## 이 조각에서 배운 것

**"재려는 대상으로 기대값을 만들지 않는다" 가 세 번째로 나왔다.**
이번엔 두 번, 서로 다른 모양으로:

```text
버퍼 경계 테스트가 버퍼를 NAME_BUFFER 상수로 만듦
    -> 상수를 64 로 줄이면 테스트 버퍼도 같이 줄어 통과
    -> 내 뮤테이션이 잡았다

순열 테스트가 기대 행을 normalize_gpu_order() 로 만듦
    -> 그 함수가 다른 필드를 훼손하면 기대값도 함께 훼손돼 통과
    -> 독립 검수 1라운드가 잡았다
```

이 실수가 잘 안 보이는 이유가 있다 — 테스트를 쓸 때 "정답이 뭐지?" 를
물으면 **가장 손쉬운 답이 대상을 한 번 돌려 보는 것**이다. 그게 곧
순환이다. 두 번째 것은 특히 교묘했다: UUID 와 index 는 리터럴로 대조해
놓고 **나머지 필드만** 함수 출력에서 왔다.

**그리고 새로운 종류가 하나 나왔다 — 헬퍼의 공허성.** 2라운드가 찾은
것은 본 테스트가 아니라 **순열 생성기**였다. 개수만 세면 생성기가 정렬된
같은 순열을 24번 돌려줘도 전부 통과하고, 그러면 순서 독립성을 재려고 만든
테스트가 순서를 하나도 안 재게 된다.
