# 저장된 예약 Grant 에 제출자 서명 Manifest 싣기 — §A1 1.5 의 선행 조각

> **작성** 2026-09-10 18:54 · **성격** 설계 초안(코덱스 논의 전) ·
> **상위** `docs/plans/_열린_작업.md` §A1 1.5 · 결함 ⑯

## 1. 왜 지금

저장된 예약 경로(`--grant-from-control-db` · `issue-grant`)의 Grant 에는 Manifest 가 없다.
그래서 Agent 가 실행할 것이 없고, 보고할 종료도 없다 — §A1 1.5 의 **첫** 차단점이다.
오늘의 사실은 덫 테스트
`crates/cli/tests/grant_over_wire.rs::today_a_stored_grant_carries_no_manifest_so_there_is_no_exit_to_report`
가 고정한다. DoD-68 사슬의 검수가 2026-09-10 에 끝나(재검수 35) 착수할 수 있게 됐다.

★ Manifest 는 첫 차단점일 **뿐**이다. 부착 뒤에도 ⑱(ACK 전 실행 · 10초 시한) · ⑲ · ⑳ 이
남는다 — 이 조각의 범위가 아니다.

## 2. 지금 코드가 하는 것 (읽어서 확인한 것)

```text
기본 lane    crates/coordinator/src/lib.rs  load_signed_manifest · issue_grant
             Manifest 를 **검증하지 않고** 실어 나른다. manifest_hash 를 outer Grant 서명
             **전에** 채운다(BLAKE3_256(signing_input(manifest)))
Agent        crates/agent/src/lib.rs  verify_nested_manifest
             제출자 공개키 하나로 **독립** 검증 · Manifest.job_id == Lease.job_id ·
             manifest_hash 재계산 대조는 protocol 의 check_derived_consistency 가 한다
저장소       crates/coordinator/src/job_store.rs  StoredManifestBinding
             { manifest, manifest_hash, signer_id_at_submission }
             DoD-50: raw binding 은 key directory 재검증 전 scheduler/Grant 에 쓰지 않는다
scheduler    crates/cli/src/scheduler_tick.rs  저장된 Manifest 를 제출자 keyring
             (PersistentKeyring, 평문 K0 는 opt-in)으로 **다시 검증한 뒤** 요구사항을 뽑는다
Grant 조립   crates/coordinator/src/grant_from_stored.rs  "안 한다: Manifest 싣기"
             호출부 둘 — crates/cli/src/issue_grant.rs · crates/coordinator/src/lib.rs
```

## 3. 선택지

```text
A  싣기만 한다        기본 lane 과 같은 모양 · keyring 인자 불필요
                      잃는 것: DoD-50 의 "재검증 전에 Grant 에 쓰지 않는다" 와 어긋난다.
                      저장소가 손상·변조돼도 Coordinator 는 모른 채 서명해 보낸다
                      (Agent 가 거부하긴 한다 — 그러나 서명은 이미 한 뒤다)
B  싣기 전에 재검증   Coordinator 가 제출자 keyring 으로 저장된 Manifest 를 다시 검증한다
                      (scheduler_tick 과 같은 방식). DoD-50 을 지킨다
                      잃는 것: 호출부 둘에 keyring 인자(K0 opt-in 포함)가 늘어난다
```

**제안: B.** Agent 의 독립 검증은 그대로다 — Coordinator 의 재검증 결과를 Grant 에 싣지
않는다("검증했다" 비트 없음). 그래야 Agent 가 Coordinator 의 말을 믿는 구조가 생기지 않는다.

B 에서 싣기 전에 대조할 것(초안):

```text
1  binding 이 있다                       없으면 GRANT_REFUSED
2  제출자 keyring 으로 서명 검증         실패면 GRANT_REFUSED (만료 포함)
3  Manifest.job_id == request.job_id
4  제출 시점 서명자 == 지금 검증한 서명자  plan_job.rs 에 같은 비교가 있다 — 재사용 여부 확인
5  manifest_hash 를 재계산해 채운다       저장된 hash 와도 대조하는가? (job_store 가 load 시
                                          HashMismatch 를 검사하는지 확인 필요)
```

## 4. 범위 밖

⑱⑲⑳ · ResourceScope · K1(DPAPI·systemd-creds) keyring 에서 꺼내기 · 데몬화

## 5. 검증 계획

```text
덫 테스트를 **뒤집는다** — 그 테스트가 적어 둔 정상 경로 목록 그대로:
  Agent       WORKLOAD_RESULT ok=true · ATTEMPT_REPORT_SENT
  Coordinator ATTEMPT_REPORT_STORED
  DB          get_report_binding 이 Some 이고 저장값이 보낸 줄과 같다
  대조군      보고만 끄면 행이 없다
  그리고 issue-grant 로 만든 저장된 Grant 자체에 manifest 가 있는지 직접 본다
부정 경로   저장소의 Manifest 를 변조 -> GRANT_REFUSED (Agent 까지 가지 않는다)
            keyring 에 없는 제출자 -> GRANT_REFUSED
뮤테이션   재검증 제거 -> 변조 테스트 실패 · hash 채우기 제거 -> Agent 가 DerivedMismatch 로 거부
```

## 6. 코덱스에게 물을 것

`docs/runbooks/검수_프롬프트/` 의 설계 논의 프롬프트에 적는다.
