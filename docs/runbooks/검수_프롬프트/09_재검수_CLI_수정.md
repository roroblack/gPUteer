동의가 아니라 **반박**을 원한다. 지적을 받고 고쳤는데, **그 수정이
지적을 실제로 닫았는지** 확인해 달라. 괜찮다는 답이 아니라 **무엇이
여전히 틀렸는지**를 찾아 달라.

# 배경 — 2026-09-10 에 세 지적을 받고 고쳤다

세 독립 검수가 CLI 세 명령에서 결함을 찾았고, 같은 세션이 고쳤다.
**고친 것이 지적을 닫았는지**가 이번 질문이다.

```text
지적 A (scheduler-tick)
  오류 메시지에 사용자 입력이 그대로 들어가서, 축 이름에 뒤 관문의
  사유 문구를 심으면 **축 파서에서 죽으면서도** 테스트의
  output.contains() 를 통과했다. 재려던 관문은 한 번도 안 돌았다.

지적 B (issue-grant)
  std::fs::write 한 줄이 (1) 기존 파일을 말없이 덮고 (2) 먼저 자르고
  쓰므로 중간에 실패하면 잘린 채 남는다.

지적 C (submit)
  (1) 만료 생략 시 "발급 + 7일" 덧셈이 오버플로해 **패닉**한다
  (2) 정상 경로가 "선언한 값이 실제로 서명됐는지" 를 안 본다
  (3) 대소문자 불변성을 고정하는 테스트가 없다
```

# 읽을 파일 — 이것만 읽어라

```text
crates/cli/src/out_file.rs
crates/cli/src/scheduler_tick.rs
crates/cli/src/submit.rs
crates/cli/tests/scheduler_tick.rs
crates/cli/tests/submit.rs
```

# 물을 것 — 네 가지

1. **지적 A 의 수정이 닫혔나.** 거부 사유에 `TICK_ARGS_REFUSED:` /
   `TICK_REFUSED:` 코드를 붙이고, 테스트가 `.contains()` 가 아니라
   **오류 줄의 시작**(`refused_with()`)으로 확인하게 바꿨다.
   - 사용자 입력으로 그 코드를 **여전히 흉내낼 수 있는 경로**가 있나?
     (줄바꿈 주입·다른 플래그의 echo·stdout/stderr 합쳐지는 형태 등)
   - `an_argument_error_cannot_impersonate_a_later_gate` 가 **정말**
     그것을 고정하나, 아니면 전제가 우연히 성립하는 것인가?

2. **지적 B 의 수정이 닫혔나.** `out_file::write_new()` 로 모았다.
   - 임시 파일 이름이 `<name>.tmp.<pid>` 다. **같은 pid 가 재사용되거나
     두 프로세스가 같은 디렉터리에 쓰면** 어떻게 되나?
   - `overwrite` 일 때 `remove_file` 후 `rename` 사이의 창은?
   - rename 이 실패하면 임시 파일을 지우는데, **그 지우기가 실패하면**?
   - fsync 를 안 한다고 주석에 적었다. 그 판단이 이 용도에 맞나?

3. **지적 C 의 수정이 닫혔나.**
   - `checked_add` 로 바꿨는데, **다른 산술에도 같은 위험**이 있나?
     이 파일 전체에서 넘칠 수 있는 덧셈·곱셈을 전부 찾아라.
   - `the_declared_values_are_the_ones_that_get_signed` 가 여섯 축과
     자원 셋을 대조한다. **빠진 서명 대상 필드**가 있나?
   - `declaration_case_does_not_change_the_signed_bytes` 가 바이트를
     통째로 비교한다. 그게 **너무 강해서** 관계없는 변경에도 깨지나?

4. **수정이 새로 만든 결함이 있나.** 특히:
   - `SUBMIT_REFUSED: ISSUED_AT_TOO_LARGE` 가 **정상 입력을 거부**하게
     되는 경계가 있나?
   - `UndeclaredAmount` 거부(별도 파일이라 여기서 안 읽지만) 때문에
     `submit` 이 만든 Manifest 가 뒤에서 거부될 조합이 있나?

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 없는 문제를
지어내지 마라. 반례를 못 만든 것과 결함이 없는 것은 다르다 —
그 둘을 구분해 써라.

# 답의 형식

각 지적마다 `파일:줄` 과 **구체적 입력**(이 인자로 부르면 이렇게 된다)을
붙여라.

마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
