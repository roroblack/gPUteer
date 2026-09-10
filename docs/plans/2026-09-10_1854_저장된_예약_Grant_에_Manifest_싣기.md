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

## 3. 선택지 (★ 36 이전 초안 — 7 절이 대체한다)

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

## 5. 검증 계획 (★ 36 이전 초안 — 7 절이 대체한다)

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

## 7. 설계 논의 36 반영 (코덱스 CHANGES_REQUESTED — B 는 맞고 근거·검증을 고치라고 했다)

원문 `docs/evidence/_raw/검수_2026-09-10/36_설계논의_저장된_Grant_Manifest_CHANGES_REQUESTED.txt`

### 7.1 초안이 틀린 곳

```text
A 의 손실      "저장소가 손상·변조돼도 Coordinator 는 모른다" -> 과장이다. get_manifest_binding()
               이 본문 디코딩 · job_id · 제출자 id · 제출 당시 서명자 id · 저장 hash 대 재계산
               hash 를 이미 거부한다(job_store.rs fetch_manifest_binding). A 가 빠뜨리는 것은
               **지금 신뢰하는 제출자 키와 지금 시각으로 다시 검증하는 단계**다
"Agent 가 거부한다"  조건부다 — Agent 는 자기에게 설정된 공개키 하나로 검증한다. Coordinator
               쪽 신뢰 목록에서 빠진 제출자도 Agent 설정이 그대로면 통과할 수 있다
서명자 비교    plan_job.rs 에 재사용할 비교는 **없다** — 도달 불가라 이미 지웠다. 저장소가
               보장하는 불변조건이다
hash           저장소가 같은 식(BLAKE3_256(signing_input))으로 재계산해 대조한 값을 돌려준다.
               그 값을 그대로 쓴다. 새로 계산해 덮어쓰지 않는다
⑱              짧은 워크로드(cmd /c exit 0)는 10초 안에 끝날 수 있어 **이 테스트를 반드시
               막지는 않는다.** 반대로 이 테스트의 통과로 ⑱ 이 풀렸다고 말하지 않는다
```

### 7.2 B 의 절차 (구현할 것)

```text
1  get_manifest_binding(job_id)
     Ok(None) · LegacyManifestMissing · ManifestCorrupt -> GRANT_REFUSED
2  제출자 keyring 으로 verify(manifest, 1, 검증기, request.issued_at_unix_ms, NoReplayCheck)
     실패(서명 · 만료 · 모르는 제출자) -> GRANT_REFUSED
3  ★ 새 정책: Grant 만료 <= min(Lease 만료, Manifest 만료). 넘으면 GRANT_REFUSED
     (Agent 는 수신 시각으로 Manifest 를 검증하므로, Grant 는 유효한데 Manifest 가 만료된
      구간이 생길 수 있다 — 36 의 지적. 기존 코드에 이 검사는 없다)
4  grant.manifest = binding.manifest · grant.manifest_hash = {algo 1, binding.manifest_hash}
     outer 서명 **전에** 채운다. verify_own_output 의 protocol 검증이 hash 일치를 다시 본다
호출부 둘   issue-grant 와 coordinator-stub --grant-from-control-db 에 --submitter-keyring
            (평문 K0 는 --i-understand-plaintext-keyring-is-unsafe true 로만) — 없으면 거부
```

### 7.3 검증 계획 (판별력을 따져 다시 짰다)

```text
정상      덫 테스트를 뒤집는다. Agent WORKLOAD_RESULT ok=true · ATTEMPT_REPORT_SENT,
          Coordinator ATTEMPT_REPORT_STORED, DB 보고 행 = 보낸 값
          대조군(보고 끔)에서도 WORKLOAD_RESULT ok=true 를 확인한다 — 실행 실패로 행이 없는
          경우와 가른다
Grant     issue-grant 산출물에 manifest 와 manifest_hash 가 **각각** 있고, algo=1 · 값이
          저장된 hash 와 같은지 직접 단언한다(Agent 는 hash 없음을 통과시키므로 Agent 성공만
          으로는 hash 채우기 제거를 못 잡는다)
부정 1    본문과 저장 hash 를 **함께** 바꿔 저장소 검사는 통과시키고 서명만 무효로 만든다 ->
          get_manifest_binding() 성공을 먼저 확인한 뒤 GRANT_REFUSED
부정 2    정상 DB 를 만든 **뒤** 발급용 keyring 에서 제출자를 뺀다 -> GRANT_REFUSED
부정 3    Grant 만료 > Manifest 만료 -> GRANT_REFUSED · 같으면 통과(경계)
뮤테이션  재검증 제거 -> 부정 1·2 가 실패 · 수명 검사 제거 -> 부정 3 이 실패 ·
          hash 채우기 제거 -> Grant 단언이 실패
```

## 8. 구현 (2026-09-10)

```text
grant_from_stored.rs  signed_grant_from_stored 가 제출자 키 디렉터리를 받는다. 서명 전에
                      binding 읽기 -> 발급 시각 기준 재검증 -> Grant 만료 <= Manifest 만료 ->
                      저장된 hash 와 Manifest 를 싣는다
issue-grant           --submitter-keyring 필수(평문은 --i-understand-plaintext-keyring-is-unsafe)
coordinator-stub      저장된 예약 lane 에서 --submitter-keyring 필수(STORED_LANE_KEYRING_MISSING),
                      두 인자는 STORED_LANE_ONLY 에 넣었다(레거시 lane 은 거부)
테스트                덫을 뒤집은 정상 경로 + 보고 끈 대조군(grant_over_wire, Windows) ·
                      Grant 단언 · 서명만 무효 · 제출자 제외 · 수명 경계(issue_grant) ·
                      keyring 누락(stored_lane_flags). 기존 fixture 는 서명된 Manifest 를 묶거나
                      keyring 인자를 더했다
```

★ `tests/issue_grant.rs` 의 Grant 시각은 고정 상수(2027년 무렵)라, 준비 코드의 Manifest 가
  "지금 + 7일" 에 만료되면 **발급 시각 기준 재검증**에서 거부된다 — 새 수명 비교(Grant 만료 <=
  Manifest 만료)까지는 가지 않는다. 그 비교는 경계 테스트가 따로 잰다. JobManifest 는
  LongLived("지금 < 만료" 만 본다)라 만료를 Lease 만료 뒤로 옮겼다(구현 검수 37 — 전에는
  "새 규칙이 실제로 작동한 결과" 라 적어 두 관문을 섞었다).
