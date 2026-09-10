동의가 아니라 **반박**을 원한다. 재검수 11 이 셋을 찾았고 고쳤다. 이번
수정이 **또 과장인지**, 그리고 **수정이 새로 만든 문제**를 찾아 달라.

# 배경 — 11번의 판정과 이번 수정

```text
⑬ issue-grant 에 submit 과 같은 hex 패닉이 남아 있었다
   -> grep 해 보니 production 에 여섯 곳(+ 이미 고친 submit = 일곱)
   -> 해석을 crates/crypto/src/hex.rs 한 곳으로 모았다. 문자열을 **자르지
      않고** 바이트를 하나씩 읽는다
   -> crates/cli/tests/hex_input_never_panics.rs — 일곱 진입점에
      "가"+"1"… 입력. 실패 · 패닉 흔적 없음 · 그 칸의 말로 거부, 셋을 본다

⑭ ⑧ 의 세 수정을 지키는 테스트가 없었다 + 원자성 주석이 과했다 +
   임시 파일 정리 실패를 `let _ =` 로 버렸다
   -> 잔여 임시 파일 보존 테스트, 동시 쓰기 16개 중 정확히 하나 테스트
   -> "먼저 지우지 않는다" 는 장애 주입 없이 테스트할 수 없어 **코드로만
      지킨다** 고 적었다
   -> 정리 실패는 경고(성공 경로) 또는 오류에 덧붙임(실패 경로)

⑮ ⑩ 의 경계 둘 — GPU 모델만 준 제출, 자원 플래그 전부 생략
```

수정 쪽이 잰 뮤테이션 결과는 결함 리포트 ⑬~⑮ 의 조치 칸에 있다.
**그 숫자를 믿지 말고** 구조로 판정하라.

# 읽을 파일 — 이것만 읽어라

```text
crates/crypto/src/hex.rs
crates/cli/src/issue_grant.rs
crates/cli/src/stage_job.rs
crates/cli/src/import_manifest.rs
crates/cli/src/import_inventory.rs
crates/cli/src/submit.rs
crates/cli/src/out_file.rs
crates/cli/tests/hex_input_never_panics.rs
crates/cli/tests/submit.rs
docs/reports/debugs/2026-09-10_0900_검수가_찾은_결함_5건.md
```

`crates/coordinator/src/lib.rs` 와 `crates/agent/src/lib.rs` 는 **`hex_decode`
함수와 그것을 부르는 곳만** 봐도 된다.

★ **저장소 전체 grep 은 허용한다** — 질문 1 이 그것을 요구한다.

# 물을 것 — 다섯 가지

1. **⑬ 이 정말 일곱 곳뿐이었나.** 수정 쪽은 `[i * 2..` 와
   `from_str_radix(&x[` 두 모양으로만 찾았다. **다른 모양으로 문자열을
   바이트 인덱스로 자르는 곳**, 사용자 입력이 닿는 곳이 저장소에 남았나?

2. **`hex.rs` 가 정말 패닉하지 않나.** 인덱스 경계·`N * 2` 계산·빈 입력을
   따라가라. 옛 코드가 받던 입력 중 **새 코드가 거부하게 된 정상 입력**이
   있나(부호 `+` 말고)?

3. **패닉 테스트 일곱이 정말 hex 해석 자리에 닿나.** 특히
   coordinator·agent 는 표지가 `"hex 가 아니다"` 다 — **다른 오류 메시지가
   같은 문구를 담아** 해석에 닿지 않고도 통과할 수 있나?

4. **⑭ 의 두 테스트가 1차 구현을 잡나.** 동시 쓰기 테스트는 경쟁 타이밍에
   기대는데, 그 한계를 정직하게 적었나? 잔여 파일 테스트가 **같은 프로세스의
   다른 테스트 스레드**와 이름이 겹칠 수 있나?
   `with_cleanup` 이 `NotFound` 를 삼키는 것이 옳은가?

5. **새로 만든 결함이 있나.** 각 수정마다 **되돌리면 실패하는 테스트**가
   실제로 있는지 구조로 확인하라.

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 반례를 못 만든
것과 결함이 없는 것은 다르다 — 그 둘을 구분해 써라.

# 답의 형식

각 지적마다 `파일:줄` 과 구체적 입력을 붙여라.
마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
