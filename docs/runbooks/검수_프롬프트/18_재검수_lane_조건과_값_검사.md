동의가 아니라 **반박**을 원한다. 재검수 15 가 ⑯ 확장 관문이 **lane 조건을
틀리게 그렸다**고 짚었다(결함 ㉑). 이번에는 필드마다 실제 사용처를 먼저 보고
고쳤다. 그래도 **또 틀린 곳**이 있는지 찾아 달라.

# 배경 — 15 의 지적과 이번 수정

```text
지적                                       수정
Resume + control DB 인데 식별자를 요구      Resume 이면 파서의 저장된 예약 검사를 건너뛴다.
                                           그 조합 자체는 시작 관문(run)이 거부한다
                                           — Resume 은 control DB 를 아무도 안 연다
--renewed-fence-epoch 등을 무조건 거부      필드마다 사용처를 봤다:
                                             항상 무시 6  manifest·변조·만료·TTL 플래그
                                                          — 레거시 발급에서만 읽힌다
                                             --lease-db 있을 때만 무시 3
                                                          fence-epoch · max-total-duration ·
                                                          renewed-fence-epoch
잘못된 불리언(tru)이 조용히 false            bool_flag 가 true/false 가 아닌 값을 따로 적고
                                           설정을 다 만든 뒤 INVALID_BOOL (Coordinator · Agent)
--neighbor-report-db 만 준 경우             기대값 0 이면 NEEDS_EXPECT
레거시 lane 의 --stored-grant-*             STORED_LANE_ONLY
STORED_LANE_ID_MISSING 테스트 없음          추가
"기존 호출을 하나도 안 깼다"                 틀린 기록 — 고친 selftest 인자로 통과한 것이고
                                           --session-id 는 이제 거부된다고 정정
```

수정 쪽이 잰 것: coordinator 256 · Agent 60 · selftest 97/97 · 뮤테이션. 믿지 말고
구조로 판정하라.

# 읽을 파일 — 이것만 읽어라

```text
crates/coordinator/src/lib.rs   (Flags · parse_flags · parse_config_from_args ·
                                 run 의 시작 관문들 · 저장된 예약 분기 ·
                                 serve_one_connection_impl 의 갱신 경로 ·
                                 build_renew_result · issue_grant · issue_lease ·
                                 load_signed_manifest)
crates/agent/src/lib.rs         (Flags · parse_flags · parse_config_from_args)
crates/coordinator/tests/stored_lane_flags.rs
crates/agent/tests/unknown_flags.rs
docs/reports/debugs/2026-09-10_0900_검수가_찾은_결함_5건.md   (⑯ · ㉑)
```

# 물을 것 — 다섯 가지

1. **"항상 무시 6" 이 정말 세션 어느 단계에서도 안 쓰이나.** 그 여섯 필드를
   읽는 곳을 전부 따라가, 저장된 예약 lane 의 갱신·revoke·재접속·Resume 에서
   닿는 곳이 있나?

2. **"`--lease-db` 있을 때만 무시 3" 의 조건이 맞나.** `--lease-db` 가 있어도
   그 값이 쓰이는 경로(예: `renew_outcome_override`, 저장소 조회 실패 뒤의 대체값)가
   있나? 없을 때 쓰인다는 쪽도 확인하라.

3. **Resume 과 control DB 를 같이 거부하는 것이 정상 호출을 막나.** Resume
   lane 이 control DB 를 쓰는 곳이 정말 없나(보고·이웃 신고·heartbeat 저장소 포함)?

4. **여전히 조용히 사라지는 값이 있나.** 이름 검사 · 불리언 검사 · 몇 가지 조건
   규칙이 들어갔지만, **숫자 플래그의 조건부 적용**(예: `--renew-rounds` 를
   갱신을 안 하는 lane 에서, `--revoke-delay-ms` 를 revoke 가 없을 때)처럼 같은
   유형이 더 있나? 전수 목록을 요구하는 것이 아니다 — **구체적 반례 몇 개**와,
   이 방식이 그 유형 전체를 막지 못한다면 그 사실을 정확히 적은 문장인지를 보라.

5. **새 테스트가 각 수정을 고정하나** — 되돌리면 실패하는 구조인가. 그리고
   수정이 새로 만든 결함이 있나.

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 반례를 못 만든
것과 결함이 없는 것은 다르다 — 그 둘을 구분해 써라.

# 답의 형식

각 지적마다 `파일:줄` 과 구체적 입력을 붙여라.
마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
