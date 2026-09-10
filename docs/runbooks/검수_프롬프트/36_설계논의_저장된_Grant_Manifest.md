동의가 아니라 **반박**을 원한다. 이것은 판정이 아니라 **설계 논의**다 — 구현 전에 틀린
전제를 찾고 싶다.

# 대상

```text
docs/plans/2026-09-10_1854_저장된_예약_Grant_에_Manifest_싣기.md
```

저장된 예약에서 만드는 Grant 에 제출자 서명 Manifest 를 싣는 조각이다. 초안은 "싣기 전에
Coordinator 가 제출자 keyring 으로 다시 검증한다(B)" 를 제안한다.

# 읽을 파일 — 이것만 읽어라

```text
docs/plans/2026-09-10_1854_저장된_예약_Grant_에_Manifest_싣기.md
crates/coordinator/src/grant_from_stored.rs
crates/coordinator/src/lib.rs                  (load_signed_manifest · issue_grant · 870~915행)
crates/coordinator/src/job_store.rs            (StoredManifestBinding · get_manifest_binding · 손상 검사)
crates/agent/src/lib.rs                        (verify_nested_manifest)
crates/cli/src/scheduler_tick.rs               (저장된 Manifest 재검증 부분)
crates/cli/src/plan_job.rs                     (제출 시점 서명자와 재검증 서명자 비교)
crates/cli/src/issue_grant.rs
crates/cli/tests/grant_over_wire.rs            (덫 테스트와 그 주석)
```

# 물을 것 — 넷

1. **A 와 B 중 무엇이 맞나.** 초안의 "A 를 고르면 잃는 것" 이 코드로 사실인가.
2. **B 의 대조 목록에 빠진 것이 있나.** 특히 저장된 hash 와 재계산 hash 의 관계, 제출 시점
   서명자 비교, Manifest 만료와 Grant 수명의 관계.
3. **결함 ⑱(Agent 가 ACK 전에 워크로드를 끝까지 돌리고 Coordinator 는 10초만 기다린다)이
   이 조각의 정상 경로 테스트를 막나.** 덫 테스트의 워크로드는 `cmd /c exit 0` 이다.
4. **검증 계획의 뮤테이션이 판별력이 있나.** 되돌려도 통과하는 테스트가 끼어 있지 않은가.

# 답의 형식

각 지적마다 `파일:줄` 을 인용하라. 못 찾은 것은 못 찾았다고, 어디까지 봤는지 적어라.
마지막 줄에 `ACCEPTED`(이 설계로 구현을 시작해도 된다) 또는 `CHANGES_REQUESTED` 를 써라.
