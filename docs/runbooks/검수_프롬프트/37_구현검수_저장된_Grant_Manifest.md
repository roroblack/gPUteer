동의가 아니라 **반박**을 원한다. 설계 논의 36 의 결론(B — 싣기 전에 발급 시각 기준으로 다시
검증한다)대로 **구현**했다. 이번에는 코드와 테스트를 본다.

# 무엇을 했나

```text
crates/coordinator/src/grant_from_stored.rs
  signed_grant_from_stored 가 제출자 키 디렉터리(K: KeyDirectory)를 받는다. 서명 전에
    1 get_manifest_binding — None · LegacyManifestMissing · ManifestCorrupt 는 GRANT_REFUSED
    2 verify(manifest, 1, 제출자 검증기, request.issued_at_unix_ms, NoReplayCheck)
    3 Grant 만료 > Manifest 만료 면 GRANT_REFUSED (새 정책 — 36 의 제안)
    4 grant.manifest = binding.manifest · manifest_hash = {1, binding.manifest_hash}
crates/cli/src/issue_grant.rs          --submitter-keyring 필수(평문은 opt-in). 키·저장소 검사 뒤에 연다
crates/coordinator/src/lib.rs          저장된 예약 lane: --submitter-keyring 필수
                                       (STORED_LANE_KEYRING_MISSING), 두 인자는 STORED_LANE_ONLY
```

테스트:

```text
crates/cli/tests/grant_over_wire.rs    덫을 뒤집은 정상 경로(Windows) — 보낸 줄과 DB 행 대조,
                                       issue-grant 산출물의 manifest·hash 단언 · 보고 끈 대조군
crates/cli/tests/issue_grant.rs        Grant 단언 · 본문+hash 함께 변조(서명만 무효) · 제출자 제외 ·
                                       Grant 만료 == / +1ms 경계
crates/coordinator/tests/stored_lane_flags.rs  keyring 누락 · 레거시 lane 의 두 인자 거부
기존 fixture                            attempt_report_ingress 는 서명된 Manifest 를 묶어 저장,
                                       lane_guard · grant_over_wire 는 keyring 인자
```

뮤테이션 넷(재검증 제거 · 수명 검사 제거 · hash 제거 · Manifest 싣기 전체 제거)을 돌렸다 —
결과는 `docs/history/HISTORY.md` 맨 위 항목에 있다.

# 읽을 파일 — 이것만 읽어라

```text
docs/plans/2026-09-10_1854_저장된_예약_Grant_에_Manifest_싣기.md   (§7 · §8)
crates/coordinator/src/grant_from_stored.rs
crates/coordinator/src/lib.rs          (저장된 예약 lane 호출부 · parse_config_from_args 의 lane 관문)
crates/cli/src/issue_grant.rs
crates/cli/tests/grant_over_wire.rs
crates/cli/tests/issue_grant.rs        (staged_until · issue_full · 끝의 새 테스트 넷)
crates/coordinator/tests/stored_lane_flags.rs
crates/coordinator/tests/attempt_report_ingress.rs   (prepare_control_db · fixture)
docs/history/HISTORY.md                (맨 위 항목만)
```

# 물을 것 — 넷

1. **구현이 설계 §7 과 맞나.** 순서(재검증이 서명 전) · 검증 시각 · 수명 경계(`>` 로 거부, 같으면 통과) ·
   hash 를 새로 계산해 덮지 않는지.
2. **새 테스트가 판별력이 있나.** 특히 "서명만 무효" 테스트가 저장소 검사를 정말 통과시키는지,
   경계 테스트의 거부 사유가 Lease 가 아니라 Manifest 인지.
3. **새 단정이 끼어들지 않았나.** 코드 주석과 설계 §8 의 문장 — 관측을 효과·필연·보편으로 넓힌 곳.
4. **놓친 호출부나 lane 이 있나.** 저장된 예약에서 Grant 를 만드는 다른 경로, Resume 경로와의 관계.

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 반례를 못 만든
것과 결함이 없는 것은 다르다 — 그 둘을 구분해 써라.

# 답의 형식

각 지적마다 `파일:줄` 을 인용하라.
마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
