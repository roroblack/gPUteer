동의가 아니라 **반박**을 원한다. 재검수 12 의 두 지적을 고쳤고, 그러다
검수가 아닌 곳에서 하나를 더 찾아 고쳤다. **또 과장인지** 봐 달라.

# 배경

```text
E  "CLI 로는 상태 관문에 못 닿는다" 가 과한 일반화였다
   -> 검수자가 준 입력 그대로 테스트를 넣었다: plan-job 을 건너뛰어
      SUBMITTED 로 둔 채 빈 노드에 stage-job. **닿았다** — Job is not QUEUED
   -> 덫 테스트는 이름을 "이 fixture 로 재예약하면 점유가 먼저" 로 좁혔다

F  scope.rs 의 "오늘 어느 플랫폼에서도 수단이 없다" 가 측정보다 강했다
   -> "이번에 잰 환경(x600 WSL2)과 방식에서는 입증하지 못했다.
      네이티브 Linux + MPS 는 재지 않았다" 로 좁혔다

⑯ (검수 밖에서 찾음) 저장된 예약 lane(--grant-from-control-db)이
   --manifest-file 을 받아 두고 **말없이 버렸다**. Manifest 부착은 레거시
   issue_grant() 안에만 있다
   -> 두 플래그를 같이 주면 bind 전에 STARTUP_REFUSED

§A1 1.5  종료 보고가 두 프로세스 사이를 건너는지 재려 했더니, 저장된
   예약 Grant 에 Manifest 가 없어 Agent 가 실행할 것이 없었다.
   -> 오늘의 사실을 고정하는 덫 테스트로 두었다. 문서에 "저장된 예약
      경로의 진짜 빈칸은 Manifest 싣기" 라고 적었다
```

# 읽을 파일 — 이것만 읽어라

```text
crates/cli/tests/stage_job.rs
crates/cli/src/stage_job.rs
crates/coordinator/src/staging_store.rs
crates/coordinator/src/grant_from_stored.rs
crates/coordinator/src/lib.rs
crates/cli/tests/grant_over_wire.rs
crates/agent/src/lib.rs
crates/scheduler/src/scope.rs
crates/scheduler/tests/gpu_scope_candidate.rs
docs/reports/debugs/2026-09-10_0900_검수가_찾은_결함_5건.md
docs/plans/_열린_작업.md
```

`coordinator/src/lib.rs`·`agent/src/lib.rs` 는 크다. 필요한 함수만 보라 —
시작 관문(`STARTUP_REFUSED`), `issue_grant`, 저장된 예약 분기, 설정 파싱,
Agent 의 워크로드 실행·종료 보고 부분.

# 물을 것 — 다섯 가지

1. **E 의 새 CLI 테스트가 정말 상태 관문을 재나.** `Job is not QUEUED` 가
   **다른 경로에서도** 나올 수 있나? `stage-job` 의 앞선 검사가 SUBMITTED
   Job 을 먼저 거르는데 우연히 같은 문구를 쓰는 것은 아닌가?

2. **F 의 새 문장이 측정 범위 안인가.** 아직 넓은 곳이 남았나?

3. **⑯ 이 닫혔나.** 저장된 예약 lane 에서 **받아 두고 말없이 버려지는
   플래그가 `--manifest-file` 말고 또 있나?** 설정 파싱과 저장된 예약 분기를
   대조해 전부 찾아라(예: Manifest·Lease·Grant 관련 테스트용 플래그).

4. **1.5 덫 테스트가 맞는 사실을 고정하나.** Manifest 싣기가 들어오면
   **정확히 그때** 깨지나? 다른 이유로 깨지거나, Manifest 싣기가 들어와도
   안 깨질 수 있나?

5. **"저장된 예약 경로의 진짜 빈칸은 Manifest 싣기" 가 과장인가.** Manifest
   가 실린다고 가정하고 Agent 실행 -> 종료 관측 -> 보고 -> Coordinator 결합·
   저장까지 따라가라. **그 사이에 또 막히는 곳**이 있나?

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 반례를 못 만든
것과 결함이 없는 것은 다르다 — 그 둘을 구분해 써라.

# 답의 형식

각 지적마다 `파일:줄` 과 구체적 입력을 붙여라.
마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
