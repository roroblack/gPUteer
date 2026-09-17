동의가 아니라 **반박**을 원한다.

작업 위치: worktree `feat/agent-startup-gc`. 검수 대상은 95af3f5(REPORT 세션 · 결함 107) 위의 커밋 — `git show HEAD` · `git diff 95af3f5..HEAD`.

# 결함 85 — Agent 기동 GC 연결

```text
agent  run()(기본 lane) 맨 앞 — claim_checkpoint_root_and_collect: <root>.agent-lock try_lock -> startup_gc(root) -> CHECKPOINT_STARTUP_GC 출력.
       잠금 잡힘 CHECKPOINT_ROOT_BUSY · GC 실패 CHECKPOINT_STARTUP_GC_FAILED 로 시작하지 않는다. 잠금은 run() 이 끝날 때까지 쥔다
selftest 44  root 를 디렉터리로 두고 checkpoint 디렉터리 자리를 파일로 막는다(전에는 root 자체를 파일로)
테스트  startup_gc_tests 4 · 뮤테이션 G1(호출 제거) · G2(잠금 무시) · G3(GC 생략)
```

물을 것:
1. 기동 GC 가 **지우면 안 되는 것**을 지우는 경로가 있나 — 재개해야 할 체크포인트 · 다른 프로세스가 쓰는 중인 디렉터리 · outbox · 작업 출력 · 격리 증거.
   체크포인트 루트를 다른 용도 디렉터리(홈 · 기존 데이터)로 잘못 준 경우의 피해를 막는 장치가 필요한가.
2. 잠금 — 형제 파일 잠금이 같은 루트를 가리키는 다른 경로 표기(상대 · 후행 구분자 · 대소문자 · junction)에서 갈라지나. 잠금을 run() 끝까지 쥐는 것이
   재접속 · 실행 중 갱신 · 소유자 강제 종료 경로와 충돌하나. 잠금 파일을 지우지 않는 선택이 맞나.
3. GC 실패 시 시작하지 않는 것이 §0.1(소유자 주권)이나 가용성과 부딪치나 — 예: 권한 문제 하나로 Agent 가 영영 못 뜨는 경우.
4. selftest 44 의 입력 변경이 원래 증명하려던 것(marker 생성 실패의 fail-closed)을 약하게 만들었나.
5. 하지 않은 것(multi_agent lane · SENSITIVE 삭제 · 옛 Agent 경쟁 · 리눅스)을 했다고 적은 문장이 있나.

읽을 파일:

```text
crates/agent/src/lib.rs (claim_checkpoint_root_and_collect · run · startup_gc_tests · workload_run_root · report_outbox_dir)
crates/checkpoint/src/writer.rs (startup_gc · find_resume_point_for) · crates/checkpoint/src/atomic.rs (gc_partial)
crates/cli/src/coordinator_agent_selftest.rs (시나리오 39 · 43 · 44 · 83)
docs/plans/2026-09-17_0930_Agent_기동_GC_연결.md
docs/reports/debugs/2026-09-10_0900_검수가_찾은_결함_5건.md (82~86 · 107)
docs/evidence/_raw/결함85_Agent_기동_GC_시험_2026-09-17.txt
```

# 못 찾았으면

**못 찾았다고 말하고, 어디까지 확인했는지 적어라.** 반례를 못 만든 것과 결함이 없는 것은 다르다.

# 답의 형식

각 지적마다 `파일:줄` 을 인용하라.
마지막 줄에 `ACCEPTED` 또는 `CHANGES_REQUESTED` 중 하나를 써라.
