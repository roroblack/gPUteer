---
schema_version: 2
id: DoD-68
claim: "부품은 다 있는데 이어지지 않아 Job 을 하나도 돌릴 수 없던 상태에서, 끊긴 고리 넷 중 **셋**(① 투입 · ② scheduler 연결 · ③ Grant 발급)을 잇는 명령 여섯 개를 만들었다 — `plan-job` · `stage-job`(`orchestrate` 의 첫 production 호출자) · `issue-grant`(저장된 예약의 사실만으로 서명된 Grant) · `scheduler-tick`(식별자 없이 스스로 골라 **한 번** 예약한다) · `JobManifest -> JobRequirements` 변환기 · `submit` 의 선언 축 확장. **테스트가 준비한 상태에서** `coordinator-stub --grant-from-control-db` 로 그 Grant 를 별도 OS 프로세스인 Agent 가 실제 TCP 로 받아 수용하는 것까지 확인했다. ★ **④ 데몬화는 잇지 못했다** — tick 은 단발이고, 재현한 두 노드 시나리오에서는 두 번째 tick 이 예약 중복으로 거부돼 다음 Job 으로 못 갔다. ★ **이 사슬로 Job 은 실행되지 않는다** — 저장된 예약 Grant 에 Manifest 가 없다(2026-09-10 확인). ★ 독립 검수(8건)와 재검수(9~16)가 이 사슬에서 결함을 찾아 고쳤고 수정본 재검수가 남았다 — 그래서 `status: INCONCLUSIVE` 다"
status: PASS
# ★ 2026-09-10 재검수 35 ACCEPTED 로 decision 의 승격 절차대로 INCONCLUSIVE -> PASS.
#   PASS 인 것은 claim 의 범위(끊긴 고리 넷 중 셋)다 — ④ 데몬화는 잇지 못했다.
#   35 는 **문장 검수 기준**의 승인이고 구현 전체·측정 로그·digest·스키마 검증과 테스트
#   실행은 하지 않았다고 원문에 적었다(review_scope 참조)
commit: 0d6a6ed

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — CLI 명령 4건 신설 + coordinator 모듈 2건 신설, 통합 테스트 37건, 뮤테이션 누적 22건(3라운드, 21 잡힘)"
executor_model: "claude-opus-5"
executed_at: "2026-09-03 (첫 실측) / 2026-09-10 (재측정, commit 0d6a6ed) — 시·분은 원문에 없다"
# ★ 초안일 때는 "2026-09-03T17:40:00+09:00" 을 "첫 실행 시각" 이라 적어 두었다. 원문
#   (DoD-68_실행_사슬.txt 1행)에는 날짜만 있어 evidence 로 옮기며 날짜만 남겼다(P0-02 와 같은
#   처리 — 재검수 29). 재측정은 raw_output 첫 블록에 있다

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec -m gpt-6-astra --sandbox read-only -c model_reasoning_effort=high (CLI 0.153.4 — 35 원문 1~10행) — 2026-09-10 재검수 35(DoD-68 14차)가 ACCEPTED. 앞선 검수 7 과 재검수 16·19·21·22·24·26·28·30·31·32·33·34 는 CHANGES_REQUESTED. 2026-09-03 의 3갈래 동시 실행은 셋 다 쿼터로 중단됐었다"
reviewer_model: "gpt-6-astra"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_artifact: "docs/evidence/_raw/검수_2026-09-10/35_재검수_DoD68_14차_ACCEPTED.txt"
review_scope: >
  ★★ 2026-09-10 재검수 35 — **ACCEPTED.** 확인 범위는 초안 164~467행(front-matter·주석·
  본문·도식)과 `plan_job.rs` 230~252행이다. 원문은 "문장 논리에 대한 판정" 이고 구현 전체·
  측정 로그·커밋·digest·스키마를 검증하거나 테스트를 실행하지 않았으므로 "evidence 전체의
  기술적 유효성까지 승인하는 판정은 아니다" 라고 적었다. 사슬의 코드는 그 앞의 검수 1~5 와
  재검수 9~20 이 나눠 봤다(1·17·20 ACCEPTED, 나머지는 반려 뒤 수정).

  ★ 2026-09-10 — 검수를 받았다(판정 7: CHANGES_REQUESTED, 위 머리 블록).
  아래는 2026-09-03 당시 기록이다.

  ★★ **검수를 받지 못했다. 그래서 이 문서는 PASS 가 아니다.**

  종합·서술 감사·경계 조합 세 갈래를 **동시에** 걸었는데 셋 다 판정
  전에 쿼터가 끊겼다(복구 2026-09-07 15:43). `DoD-65` 에서 3갈래
  병렬이 효과적이었던 것을 그대로 따라 했는데, **그때와 지금의 차이를
  안 봤다** — 그때는 대상이 파일 몇 개였고 지금은 crate 넷에 걸친
  명령 여섯 개다. 종합 검수 하나가 131k 토큰을 썼다. 순차로 하나씩
  돌렸으면 판정을 받았을 **수도** 있다. **내 판단 착오다.** (재검수 19 — 전에는
  "최소 하나는 판정을 받았을 것이다" 로 반사실을 확정했다)

  네 번째 갈래(우회 조사)는 코덱스 자체 필터가 "관문 우회 방법을
  찾아라" 를 보안 위험으로 분류해 거부했다 — 자기 저장소 검수인데
  오탐이지만, 표현을 바꿔 다시 걸었고 그것도 쿼터에 걸렸다.

  검수 대신 이 저장소가 기록해 둔 실패 패턴으로 **자기 검수**를 했다.
  그것이 찾은 것은 아래 negative_tests 와 limitations 에 있다 —
  독립 검수를 대신하지 못하며, 대신한다고 적지 않는다.

raw_output_artifact: "docs/evidence/_raw/DoD-68_실행_사슬.txt"
raw_output_digest: "sha256:911ecb14c7e2e92592a6388796e4c276568aa2b820189b30085049e47d28001f"
raw_output_bytes: 10812

artifacts:
  - "docs/evidence/_raw/DoD-68_실행_사슬.txt"
  - "docs/evidence/_raw/검수_2026-09-10/35_재검수_DoD68_14차_ACCEPTED.txt"
  - "docs/evidence/_raw/DoD-68_x600_실측.txt"
  - "docs/evidence/_raw/DoD-68_검수_중단.txt"
  - "crates/cli/src/plan_job.rs"
  - "crates/cli/src/stage_job.rs"
  - "crates/cli/src/issue_grant.rs"
  - "crates/cli/src/scheduler_tick.rs"
  - "crates/coordinator/src/manifest_requirements.rs"
  - "crates/coordinator/src/grant_from_stored.rs"

binary_digests:
  toolchain: "Windows 개발 기계 cargo 1.97.1"
  gputeer_exe: "8169984 bytes, debug 프로필"
protocol_versions:
  schema_version: "proto 변경 없음"
  canonical_spec: "canonical 벡터 변경 없음(52건 그대로)"
platform: >
  **두 Windows 기계에서 측정했다** — 개발 기계와 x600(RTX 4070 SUPER).
  x600 에서 워크스페이스 922 passed / 0 failed / 경고 0 으로 개발
  기계와 **정확히 같았고**, coordinator-agent-selftest 97 시나리오도
  통과했다(2026-09-04).

  ★★ **Linux 도 쟀다**(2026-09-05, x600 WSL2, 커널 6.18.33.2, cargo
  1.89.0) — `--exclude gputeer-runtime-windows` 로 918 passed / 0
  failed / 경고 0. Windows 와 **같은 소스 트리**를 공유한다.

  ★ 앞서 이것을 "WSL 이 죽었다" 고 적었던 것은 **오진이었다** — 그때
  x600 자체가 내려가는 중이었다. 기계가 돌아오자 `wsl.exe -e` 가 바로
  응답했다.

  ★★ **플랫폼 차이를 이름으로 셌더니 전용 테스트 수와 맞았다**:

      Windows 914 - 28(Windows 전용) = 886
      Linux    910 - 24(Linux 전용)  = 886   <- 일치

  그리고 양쪽 전용은 서로 **짝**이다 — junction 거부 <-> symlink 거부,
  Job Object 상한 <-> cgroup 상한, DPAPI K1 <-> systemd-creds K1.
  **이름으로 대조한 범위에서는 조용히 빠진 공유 테스트를 못 찾았다**
  (`CLAUDE.md` §4 가 경계하는 것이 정확히 그것이다. ★ 재검수 24 뒤 확정 표현을
  전수로 보다 좁혔다 — 전에는 "전부 설명된다 · 없다" 고 적었다).
hardware: >
  개발 기계는 GPU 없음. **x600 은 RTX 4070 SUPER 실물**(driver 595.79,
  CUDA 13020)이고 거기서 `gpu-probe` 가 실제 값을 읽었다 — UUID
  `GPU-09a269a7-...`, total 12878610432, free 12581863424, cc 8.9,
  MIG 미지원.

  ★★ **그 실물 값으로 `import-inventory` 를 돌렸다.** 이 사슬의 테스트
  inventory 는 손으로 쓴 JSON 이었다 — 여기서 `gpu_id` 에 실제 NVML UUID 를 넣었다
  (★ 전에는 "지금까지 전부 · 처음으로" 라 적었다 — 범위를 안 적었다).
  1차는 `BOOTSTRAP_REJECTED: verifying_key_hex 가 Ed25519 공개키가
  아니다` 로 막혔다(내가 채운 `0x31` 반복이 유효한 곡선 점이 아니다 —
  **진짜 관문에 걸린 것이지 실패가 아니다**). 실제 키로 바꾸니
  `IMPORTED entries=1 registered=1 inventories_updated=1`.

  ★ 사슬의 뒷부분(plan-job 이후)은 x600 에서 **파일만으로는 못 돌린다**
  — keyring 을 만드는 CLI 경로가 없어서다(테스트는 Rust API 로 만든다).
  그 부분은 x600 의 통합 테스트 안에서 돌았다(922건에 포함).
network_profile: >
  grant_over_wire 만 실제 127.0.0.1 TCP 를 쓴다(별도 프로세스 2개).
  나머지는 파일과 SQLite 만 쓴다.
command: |
  cargo test --workspace
  cargo test -p gputeer-cli --test plan_job --test stage_job --test issue_grant --test grant_over_wire --test scheduler_tick
  # 뮤테이션 누적 22건 — 3라운드(2026-09-03~04). 1차 9건은 scratchpad/mutate.py

raw_output: |
  === 2026-09-10 재측정 (commit 0d6a6ed — 검수 1~12 대응 반영) ===
  cargo test --workspace   1055 passed / 0 failed / ignored 1 / 경고 0
  ★ 아래는 **2026-09-03 f79b485 시점** 값이다. 그 코드에는 검수가 찾은
    결함이 들어 있었다 — 두 값을 같은 코드의 측정으로 읽지 마라.

  === cargo test --workspace (f79b485) ===
  915 passed / 0 failed / 경고 0   (착수 전 906)

  === 신규 통합 테스트 37건 ===
  plan_job        10 passed
  stage_job        7 passed
  issue_grant     10 passed
  grant_over_wire  2 passed
  scheduler_tick   8 passed

  === 뮤테이션 1차 9건 (2026-09-03, 자기 검수 2회차 시점) ===
  M1 미선언 workload 를 Training 으로       잡힘
  M2 모르는 enum 값을 조용히 통과           잡힘
  G5 Attempt/Lease fence 대조 제거          안 잡힘 (limitations 참조)
  G6 예약 존재 확인 제거                    안 잡힘 (limitations 참조)
  G7 폐기된 Lease 로도 발급                 잡힘 (1회차엔 안 잡혔다)
  T1 식별자에서 plan_id 를 뺀다             안 잡힘 (limitations 참조)
  T2 Lease 발급 시각을 시계로               잡힘 (1회차엔 안 잡혔다)
  T3 큐에 오래 있어도 예약                  잡힘 (1회차엔 안 잡혔다)
  P3 적격 0 이어도 큐에 올린다              잡힘

  === 뮤테이션 누적 22건 (2026-09-04, 3라운드) ===
  1차  9건 (내가 고른 것)          8 잡힘  G5·G6 은 DB 변조 테스트로 고정(bbe7785)
  2차  6건 (안 고른 관문)          6 잡힘  (a77e865)
  3차  7건 (변환기 전수 + submit)  7 잡힘  (2306ebd)
  누적 22건 중 21건. 남은 하나는 **T1** — 지금 저장소 경로로는 그 상황을
  만들지 못해 **수행한 뮤테이션 검사에서 검출하지 못했다**(limitations).
  ★ 재검수 19 — 전에는 "원리적으로 못 잰다" 고 적었다. G5·G6 도 정상 경로로는
    못 만들다가 DB 변조로 쟀다 — T1 도 그 길이 없다고 말할 근거는 없다

negative_tests: >
  거부 경로 위주로 짰다. 대표적인 것:
  --job-db "" 를 받아들이지 않는다(SQLite 는 빈 경로도 임시 DB 로 열어
  준다 — 저장소의 is_durable() 에 물어서 막는다);
  선언 축이 하나라도 비면 **어느 축인지 이름을 대며** 거부한다;
  적격 후보가 0 이면 큐에 안 올린다(뮤테이션 P3 로 확인);
  제출 시점 서명자가 keyring 에서 사라지면 계획을 진행하지 않는다 — 이유를
  `UnknownSigner` 로 **특정한다**(`crates/cli/tests/plan_job.rs:467`. 같은 ID 에
  다른 키를 넣으면 나오는 `InvalidSignature` 와 가른다);
  폐기된 Lease 로는 Grant 를 못 낸다 — **폐기 전에는 실제로 발급되는
  대조**를 같이 뒀다(그게 없으면 "항상 거부" 로도 통과한다);
  Grant 가 Lease 보다 오래 살면 거부한다 — 이유 `보다 늦다` 를 보고, **정확히
  Lease 만료 시각은 허용하는 대조**를 둔다(`crates/cli/tests/issue_grant.rs:542`);
  큐에 TTL 보다 오래 있었으면 예약하지 않는다(이미 만료된 Lease 를
  주게 되므로);
  빈 큐는 **오류가 아니다** — TICK_IDLE. 루프가 이걸 실패로 세면 정상
  유휴가 장애로 보인다;
  예약된 노드는 두 번째 Job 이 못 잡는다 — 이유 `node is already reserved` 를
  보고 두 번째 Job 이 `QUEUED` 로 남는지 본다(`crates/cli/tests/stage_job.rs:452`).
  ★ 이 셋이 이유를 특정하게 된 것은 **2026-09-04~07** 이다. 그 전에는 `!ok`
    만 봤다 — 검수 2번이 찾은 함정과 같은 모양이다.

limitations: >
  ★★ **뮤테이션 — 1차(2026-09-03)에서는 3건이 안 잡혔다. 지금 남은 것은
  T1 하나다**(누적 22건 중 21건, raw_output 참조).

  G5(Attempt/Lease fence 대조)·G6(예약 존재 확인) — 당시에는 셋이
  staging_store.rs 의 **한 BEGIN IMMEDIATE 안에서 함께** 쓰여 어긋난 상태를
  정상 경로로 만들 수 없다고 보고 "테스트가 지키고 있다고 말하면 거짓" 이라
  적었다. **이튿날 DB 를 직접 변조하는 테스트로 고정했다**(bbe7785) — 지금은
  잡힌다 — **DB 를 직접 변조한 손상 상태에서** 두 관문의 검출을 확인한 것이다.
  정상 입력 경로에서의 역할은 따로 판정하지 않았다(재검수 24 — 전에는 "입력 검증이
  아니라 손상 방어라는 성격" 이라 분류했다).

  T1(유도에서 plan_id 제거) — 처음엔 "낡은 계획의 예약을 재사용하지
  않는다" 고 적었는데 **그 상황을 만들려니 저장소가 더 앞에서
  막았다**(queued Job plan conflict). QUEUED 인 Job 의 계획은 안
  바뀐다. 시도한 경로에서는 앞선 저장소 관문에 막혀 **plan_id 제거의 영향을
  확인하지 못했다** — 이 측정으로는 plan_id 의 방어 기여를 판정하지 않는다.
  (재검수 21 — 전에는 "plan_id 는 오늘 방어일 뿐 짐을 지고 있지 않다 · 주석을
  사실로 고쳤다" 고 적었다. 역할 부재를 확정한 것이다)

  ★★ **데몬화의 실질적 차단 요인 — 후보 선택이 기존 예약을 모른다.**
  노드가 둘인데도 두 번째 tick 이 "node is already reserved" 로 막힌다.
  evaluate_eligibility/rank_best_fit 은 inventory 만 보고 예약은
  staging 저장소의 다른 테이블에 있어 둘을 잇는 것이 없다. 예약 관문이
  중복 예약을 거부하는 것은 봤다. **재현한 두 노드 시나리오에서는 두 번째
  tick 이 예약 중복으로 거부돼 다음 Job 으로 진행하지 못했다**(재검수 24 —
  전에는 "안전엔 문제없지만 · 루프를 돌리면 영영 멈춘다" 고 적었다). 고치려면 규범 결정이 필요해(예약을 pool_snapshot 에서
  빼는가 · hard-filter 에서 거르는가 · 차순위 재시도인가) 고치지 않고
  **현재 동작을 테스트로 고정**했다.

  ★★ **예약을 해제할 수 없다**(2026-09-10 검수 7 지적으로 추가). 후보
  선택을 고쳐도 예약 재사용은 안 풀린다 — 해제 경로가 production 에 없다.
  `reservation_release.rs` 의 관문은 `RuntimeStopProof` 를 요구하고 오늘
  정직한 값은 "증명 못 함" 이다. 종료 보고는 2026-09-06 에 생겼지만 그
  관문을 열지 않는다.

  ★★ **잠금 단위가 GPU 가 아니라 노드 전체다.** "예약된 노드" 는 GPU 한
  장이 아니라 **노드**를 잠근다 — 같은 노드의 다른 GPU 도 못 쓴다
  (`DoD-47` 의 node-exclusive reservation).

  ★ **scheduler 의 node_id 와 Agent 의 device id 는 다른 이름 공간인데
  잇는 것이 없다.** 오늘은 운영자가 둘을 같게 선언해야만 wire 경로가
  이어진다. 제약이지 설계가 아니다.

  ★ **issue-grant 는 키가 정말 그 Coordinator 의 것인지 못 본다.**
  자기 검증을 붙였지만 확인하는 것은 구조·수명·인코딩이다. 엉뚱한 키로
  서명하면 통과하고 나중에 그 Agent 가 거부한다(재검수 24 — 전에는 "위험하진
  않지만 · 아무도 못 쓰는 Grant" 로 무위험을 판정했다). 발급 성공을 대상
  Agent 가 받아들일 Grant 로 오인할 우려가 있다(재검수 33 — 전에는 "운영자는 유효한
  것을 만든 줄 안다" 고 운영자의 판단을 단정했다). 닫으려면
  Coordinator 공개키 목록이 필요하고 이 저장소에 없다.

  ★ plan_job.rs 에 **테스트로 고정되지 않은 분기가 하나 더** 있다
  (제출 시점 서명자와 재검증 서명자 비교) — 코드 주석에 적어 뒀다.

  ★ 이 사슬로는 **Job 이 실행되지 않는다.** Agent 의 실행 코드는 있다
  (`agent/src/exec.rs`, 2026-09-06 확인) — 그러나 저장된 예약에서 만든 Grant
  에 Manifest 가 없어 실행할 것이 없다(2026-09-10 확인, `grant_over_wire.rs`
  의 덫 테스트). 이 사슬은 **예약과 Grant 수용까지**다.

decision: >
  ★ 2026-09-10 재검수 35 가 ACCEPTED 를 냈다. 아래 둘째 문단의 절차대로 `status` 를 PASS 로
  올렸다. PASS 인 것은 **claim 의 범위(네 고리 중 셋)** 다 — ④ 데몬화는 여전히 잇지 못했고,
  35 가 문장 검수 기준의 승인이라는 한계는 review_scope 에 적었다.

  (아래는 승인 전 기록) 이 조각은 **부분 완료**로 기록한다. 네 고리 중 **셋**이 이어졌고, 테스트가
  준비한 상태에서 프로세스 경계를 넘는 것(Grant 수용)까지 확인했지만, **PASS 로
  셀 수 없다** — 독립 검수 7(2026-09-10)이 CHANGES_REQUESTED 였고, 그 반영본을
  본 재검수 16 도 CHANGES_REQUESTED 였다(ADR-030).

  수정본 재검수에서 ACCEPTED 를 받으면 이 문서를 PASS 로 승격한다. 반려되면
  고치고 다시 받는다 — **순차로** 진행한다. 2026-09-03 의 병렬 세 실행은 모두
  판정 전에 쿼터로 중단됐다(재검수 21 — 전에는 병렬을 "실패한 원인" 으로 확정했다).

  그때까지 **이 사슬 위에 새 기능을 쌓지 않는다.** 검수 없이 쌓으면 결함 원인을
  추적하기 어려워질 수 있다고 우려해서 정한 방침이다(재검수 28 — 전에는 그 효과를
  확정해 적었다).
---

# DoD-68 — 실행 사슬: 투입부터 전송까지

## 무엇이 달라졌는가

착수 전 이 저장소에는 부품이 거의 다 있었지만 **Job 을 하나도 돌릴 수
없었다.** 네 고리가 끊겨 있었다.

```text
①  Job 투입          서명된 Manifest 를 받아 영속 저장소에 넣는 경로가 없다
②  scheduler 연결    저장된 Job 이 큐에 오르지 않는다
③  Grant 발급        저장된 예약에서 Grant 를 만드는 경로가 없다
④  데몬화            누군가 손으로 부르지 않으면 아무것도 안 움직인다
```

이제 ①~③ 이 이렇게 이어진다. **④ 데몬화는 아직이다.**

```text
submit → import-manifest → import-inventory → plan-job → stage-job
                                                    ↘ issue-grant (파일로)
coordinator-stub --grant-from-control-db ──TCP──▶ agent-stub
scheduler-tick                                    (큐에서 스스로 골라 한 번 예약한다)
```

## 이 실험이 증명하지 않는 것

**Job 이 실제로 실행되지 않는다.** 이 사슬의 끝은 "Agent 가 Grant 를
받아들였다" 이지 "학습이 돌았다" 가 아니다. Agent 쪽 실행·격리·종료 보고
코드는 2026-09-06 에 생겼지만, **이 사슬(저장된 예약)에서는 Grant 에
Manifest 가 없어 거기 닿지 않는다**(2026-09-10).

**루프가 돌지 않는다.** `scheduler-tick` 은 한 번이다. 재현한 두 노드
시나리오에서는 두 번째 tick 이 위 limitations 의 예약 공백 때문에 다음 Job 으로
못 갔다.

**독립 검수는 2026-09-10 에 받았고 결함이 나왔다.** 아래 "자기 검수가 찾은
것" 은 그 전에 만든 사람이 스스로 확인한 것이다. 이번 독립 검수와 재검수에서 **자기 검수가 놓친
서술 과장이 반복해서 발견됐다**(재검수 24 — 전에는 "늘 있다" 고 적었다).

## 자기 검수가 찾은 것

검수를 못 받는 대신 이 저장소가 기록해 둔 실패 패턴으로 훑었다.

**뮤테이션이 내 테스트 넷을 잡았다.** 가장 나쁜 것은
`a_job_that_waited_longer_than_the_lease_ttl_is_refused` 가 **엉뚱한
관문을 재고 있었던** 것이다 — `--lease-ttl-ms 1` 만 주고 갱신 오프셋은
기본값을 남겨서, 더 앞에 있는 갱신 검사에 걸렸는데 내 단언이 그 메시지도
받아 줘서 통과했다. 재려던 것을 하나도 안 쟀다.

**무게중심이 안 재지고 있었다.** "Lease 발급 시각 = 저장된 queued_at" 이
이 모듈의 핵심인데, 멱등 테스트는 식별자만 비교하고 식별자는 시각에서
유도되지 않는다 — 시계로 바꿔도 통과했다.

**내 서술이 과장이었다.** `plan_id` 가 낡은 계획의 예약 재사용을 막는다고
적었는데, 그 효과를 **테스트로 확인하지 못했다** — 상황을 만들려다 저장소가
먼저 막았다(limitations 참조). (재검수 22 — 전에는 "사실이 아니었다" 고 적었다.
입증하지 못한 것을 거짓으로 확인한 것처럼 쓴 것이다)

**`submit` 에는 있고 Grant 발급 경로엔 없던 자기 검증**을 찾아 붙였다 —
두 코드를 나란히 놓고 보다가 발견했다.
