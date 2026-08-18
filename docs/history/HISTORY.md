# 작업 이력

> ★ **추가만 한다. 기존 기록을 수정하지 않는다.**
> 최신이 위로 오도록 **역순**으로 쌓는다.

형식:

```markdown
## YYYY-MM-DD HH:mm — <작업 제목>
- 계획: <docs/plans/ 문서명> 의 <단계>
- 스트림: <RULE.md §4.1 의 스트림명>
- 수행: <핵심 변경 요약>
- 검증: <성공/실패 + 방법>
- 리포트: <docs/reports/ 파일명>
```

---

## 2026-08-18 15:05 — `runtime-windows` 수정 재검수 `ACCEPTED`

- 계획: `23e77fd`(quote_command_line·TerminateProcess 수정)에 대한
  코덱스 재검수(`p60` 프롬프트).
- 스트림: —
- 결과: **`ACCEPTED`.** `2n+1`(따옴표 직전)·`2n`(문자열 끝) 백슬래시
  규칙이 코드에 정확히 구현됐는지 손으로 재계산해 확인, 회귀
  테스트 2개의 기대값 자체가 옳은지도(테스트 통과 자체가 아니라)
  검증, `TerminateProcess` 수정이 1차 오류 반환 흐름을 깨지 않았는지,
  이전 라운드에서 `ACCEPTED` 받은 부분(핸들 정리 순서·`CREATE_SUSPENDED`
  경합 제거·`wide()` 쓰기 가능 버퍼·`guarantees_hard_limit()` 정직성·
  evidence append-only)이 이번 수정으로 회귀하지 않았는지 전부
  확인받았다. 발견 사항 없음.
- **이로써 `crates/runtime-windows`(VRAM Job Object 커밋 상한 연결)
  는 구현·실측·코덱스 독립 검수(2라운드: 1차 CHANGES_REQUESTED →
  수정 → 2차 ACCEPTED)까지 완전히 끝났다.** CLAUDE.md 백로그 2번의
  VRAM 부분은 완료 — network.rs(OS 방화벽, 사용자 승인 필요)와
  artifact.rs(TOCTOU, 추가 조사 필요)만 남는다.
- 검증: 코덱스 자신은 Windows 환경이 아니라 실제 빌드/테스트 재실행은
  하지 않았다(코드 대조로만 검증) — 실행 기반 확인은 이 세션이
  앞서 이미 여러 차례 했다(단위 테스트 4/4, 통합 테스트 3회 연속).
- 리포트: 이 이력 항목.

---

## 2026-08-18 14:50 — `runtime-windows` 코덱스 검수: 명령줄 인용 버그 2건 + `TerminateProcess` 미확인 수정

- 계획: `cb2d7c2`(VRAM Job Object 연결)에 대한 코덱스 독립 검수(`p59`
  프롬프트). 사용자 지시 — 자율 루프 계속 + "코덱스 최대한 쿼터
  써서" 재확인.
- 스트림: Runtime.
- 결과: `CHANGES_REQUESTED`(1라운드). unsafe FFI 코드라 특히 꼼꼼히
  봐 달라고 요청했는데, 실제로 진짜 결함 2건(P1)과 과장 주석 1건(P2)
  을 잡았다:
  1. **`quote_command_line` 의 백슬래시 이스케이프가 MSVC 규칙과
     다르다.** 따옴표 직전 백슬래시는 `2n+1` 개가 맞는데 초안은
     `n+1` 개만 출력했다(부족하거나 과다). 문자열 끝(닫는 따옴표
     직전) 백슬래시는 `2n` 개가 맞는데 초안은 `n` 개만 출력했다 —
     예를 들어 `C:\Program Files\` 처럼 공백을 포함하고 백슬래시로
     끝나는 인자는 닫는 따옴표를 이스케이프해 명령줄이 깨진다.
  2. **`TerminateProcess` 반환값을 확인하지 않고 바로 핸들을 닫았다.**
     종료가 실제로 실패하면 정지 상태 프로세스가 영구히 남을 수
     있는데 그 사실을 아무도 몰랐다.
  3. (P2) `alloc_fixture.rs` 의 주석이 "첫 바이트를 건드려 페이지를
     물리적으로 커밋시킨다"고 과장했다 — `JOB_OBJECT_LIMIT_JOB_MEMORY`
     는 애초에 물리 RSS 가 아니라 virtual commit 총량을 본다.
  검증 순서(성공/실패 각 분기의 핸들 정리), `CREATE_SUSPENDED` 경합
  제거, `wide()`/`lpCommandLine` 의 쓰기 가능 버퍼 요구사항, 뮤테이션
  테스트의 타당성, `guarantees_hard_limit()` 정직성, evidence
  append-only 준수는 전부 문제없음을 확인받았다.
- 수행: `quote_command_line` 을 UTF-16 코드 유닛 위에서 직접 조립하도록
  다시 짰다(`OsStr::to_string_lossy()` 를 거치던 것도 비정상 서로게이트
  손상 위험이 있어 제거) — 따옴표 앞은 `2n+1`, 문자열 끝은 `2n` 규칙을
  정확히 구현했다. 회귀 테스트 4개 추가(`simple_args_are_not_quoted`,
  `trailing_backslash_before_closing_quote_is_doubled`,
  `backslash_before_embedded_quote_uses_2n_plus_1_rule`,
  `empty_arg_is_wrapped_in_quotes`) — 뒤 두 개가 정확히 코덱스가
  잡은 버그 패턴이다. `kill_and_close` 클로저가 이제
  `TerminateProcess` 반환값을 확인하고 실패 시 `eprintln!` 으로
  PID·오류를 남긴다(두 개의 `io::Error` 를 표준 방법으로 합칠 수
  없어 최소한 눈에 보이게는 만들었다). `alloc_fixture.rs` 주석을
  정정해 "virtual commit 상한을 재는 것이지 물리 메모리 압박이
  아니다"라고 정확히 적었다.
- 검증: `cargo build --workspace` 경고 0. `cargo test -p
  gputeer-runtime-windows --lib` 4/4 통과. `commit_cap.rs` 통합
  테스트 3회 연속 통과(제약된 자식·negative control 둘 다). `cargo
  test --workspace` 303/0/1(ignored) — 이전 299 + 신규 단위 테스트 4건.
- 리포트: 이 이력 항목 + `crates/runtime-windows/src/lib.rs` 코드
  주석(수정 사유 명시).

---

## 2026-08-18 14:15 — `crates/runtime-windows` 신설 — VRAM 판정을 실제 Job Object 로 연결

- 계획: CLAUDE.md "다음에 할 일" 2번(runtime-policy 판정을 실제
  시스템 호출로 연결). 사용자 지시 — 자율 루프 계속 + "코덱스 최대한
  쿼터 써서" 재확인.
- 스트림: Runtime.
- 수행: `crates/runtime-policy/src/vram.rs` 모듈 문서가 스스로
  "`runtime-windows` 가 생기면 그쪽이 이 판정을 부르고 실제로
  `CreateJobObject`/`SetInformationJobObject` 를 호출해야 한다"고
  적어 둔 것을 그대로 이행했다. 착수 전 안전 판단: 이 작업은 시스템
  전역 설정(방화벽 규칙·OS 기능 활성화)이 아니라 **프로세스/세션
  범위** Win32 API(Job Object)라 사용자 명시적 승인 없이 자율 실행
  가능하다고 판단 — 코덱스에게도 이 전제를 검토시켰다(`p58`
  프롬프트, "동의" 판정, 근거: 이름 없는 Job 은 시스템 전역에 영향
  없고 마지막 프로세스가 끝나면 사라진다).
  - `crates/runtime-windows/src/lib.rs` — `create_constrained_child`:
    `CreateProcessW(CREATE_SUSPENDED)` → `CreateJobObjectW` →
    `SetInformationJobObject(JobMemoryLimit)` →
    `AssignProcessToJobObject` → `ResumeThread` 순서로 자식을 만든다.
    `std::process::Command` 대신 `CreateProcessW` 를 직접 부른 이유:
    안정 Rust 에는 자식의 주 스레드를 나중에 재개할 공개 API가 없다
    (`ChildExt::main_thread_handle()` 은 nightly 전용) — `CREATE_SUSPENDED`
    로 "자식이 Job 할당 전에 이미 메모리를 커밋하는" 경합을 없앤다.
  - `crates/runtime-windows/src/bin/alloc_fixture.rs` — 실측 테스트용
    fixture. `VirtualAlloc(MEM_COMMIT)` 를 청크 단위로 반복해 실패할
    때까지 커밋하고 결과를 파일에 적는다(stdout 파이프 상속을 신뢰할
    수 없어서 파일로 뺐다).
  - `crates/runtime-windows/tests/commit_cap.rs` — 제약된 자식과
    negative control(제약 없는 자식)을 비교하는 실측 테스트 2개.
- **실측으로 발견한 것**: `JOB_OBJECT_LIMIT_JOB_MEMORY` 는 딱딱한
  상한이 아니라 **소프트** 제한이다 — `PeakJobMemoryUsed` 가
  `JobMemoryLimit` 을 5회 연속 측정 모두에서 약 700~850KiB 만큼
  넘었다(64MiB 상한 기준, 4MiB 청크 크기보다 작은 오버슈트라 "청크
  하나가 더 통과했다"가 아니다). 이것이 바로
  `VramEnforcement::guarantees_hard_limit()` 가 `WindowsCommitCap`
  에도 미리 `false` 를 못박아 둔 판단을 실측으로 재확인한 것이다.
  테스트는 이 실측 여유(2MiB 허용치)를 문서화해 반영했다.
- 뮤테이션 테스트로 비공허성 증명: `AssignProcessToJobObject` 호출을
  일시 무력화 → 제약된 자식이 안전 상한(4096MiB)까지 아무 제약 없이
  전부 할당(negative control 과 동일 거동) → selftest 가 정확히 이
  결함을 잡음(할당 실패 없음을 감지) → 원복 후 재통과 확인.
- `docs/evidence/P0-06_vram_enforcement.md` 에 append-only addendum
  추가 — 이 evidence 가 이미 적어 둔 limitation("runtime-policy 는
  판정만 하고 실제 Job Object 를 생성·설정하지 않는다")이 부분적으로
  해소됐음을 기록. x600 실제 GPU 하드웨어 재실측은 아니다(로컬
  개발 기계는 GPU 가 없다) — "RAM 커밋 상한이 VRAM 에도 적용된다"는
  이 evidence 의 핵심 발견은 여전히 x600 실측에만 근거한다는 점을
  명시했다.
- `crates/protocol/tests/stream_ownership.rs::every_crate_is_covered_by_ownership_rules`
  의 `KNOWN` 목록에 `runtime-windows` 추가(이전 세션들과 같은 패턴 —
  안전망이 새 크레이트를 실제로 잡았다).
- 검증: `cargo build --workspace` 경고 0. `cargo test --workspace`
  299/0/1(ignored) — 이전 297 + `commit_cap.rs` 신규 2건.
  `python scripts/verify_evidence.py` 스키마 위반 0.
- 리포트: 이 이력 항목 + `crates/runtime-windows/src/lib.rs` 모듈
  문서 + P0-06 evidence addendum.

---

## 2026-08-18 13:05 — x600 WSL2 시도 — 시스템 설정 변경이라 자율 실행 보류

- 계획: CLAUDE.md "다음에 할 일" 3번(D-3 — Linux 검증 환경 확보).
- 스트림: —
- 결과: `ssh x600 "wsl --status"` 실행 — "Windows Subsystem for
  Linux 가 설치되어 있지 않다. `wsl.exe --install` 로 설치하라"는
  응답 확인. `wsl -l -v` 도 동일.
- **자율 실행하지 않고 보류했다.** `wsl --install` 은 Windows 선택적
  기능(가상화 플랫폼 등)을 활성화하고 통상 재부팅을 요구하는
  시스템 설정 변경이다. 이 세션의 안전 규칙은 "시스템/보안 설정
  변경"을 자율 실행 금지 항목이 아니라 **채팅에서 명시적 승인 필요**
  항목으로 분류하며, 이 규칙은 "사용자가 자고 있으니 질문하지 말고
  계속 진행"이라는 표준 루프 지시보다 우선한다. 원격 x600 은 사용자의
  실제 GPU 워크스테이션이라 재부팅이 다른 작업을 방해할 수도 있다.
- 남은 것: 사용자가 깨어나면 (1) 직접 `wsl --install` 실행 후 재부팅,
  또는 (2) 이 세션에 명시적으로 승인. 승인 전까지 D-3 은 계속 미해소
  상태로 CLAUDE.md 에 정직하게 남겨 둔다.
- 검증: 해당 없음(실행하지 않음).
- 리포트: 이 이력 항목 + CLAUDE.md "다음에 할 일" 3번 갱신.

---

## 2026-08-18 13:00 — coordinator/agent 핸드셰이크 단계 5 코덱스 검수 `ACCEPTED` — 계획 완전 종료

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  단계 6, 단계 5 커밋(`d7aacdc`) 대상. 사용자 지시 — 자율 루프 계속.
- 스트림: —
- 결과: **`ACCEPTED`**(1라운드, `p57` 프롬프트). 위조가 서명 **후**에
  일어나는지(오염된 데이터에 서명한 것이 아니라 서명 필드만 변조한
  것인지), replay 시나리오가 `grant.encode_to_vec()` 을 두 번 부르지
  않고 동일 `frame` 바이트를 재사용하는지, `signing.rs:826` 인용이
  정확한지, 위조 ACK 시나리오가 Agent 쪽 결과를 판정에 안 쓰는
  이유가 코드 주석에도 정직하게 반영됐는지, 뮤테이션 테스트 주장이
  논리적으로 타당한지, `run_handshake` 리팩터링이 기존(`ffe8d45`
  검수 통과) PID 구분·RESULT 상관관계·stdout/stderr 파이프 교착
  방지 로직을 그대로 보존했는지 — 전부 파일:줄 단위로 대조해
  확인받았다. 결함 없음.
- **이로써 `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  의 단계 1~6 전부 구현·검증·독립 검수까지 완전히 끝났다.** 남은
  것은 계획 DoD 의 마지막 항목(`docs/evidence/` 에 schema v2 형식
  정식 기록)뿐이며, 그 항목은 "구현이 안 됐다"가 아니라 "이 저장소의
  증거 문서화 관례(RULE.md §7.3)를 아직 안 따랐다"는 뜻이다.
- 검증: 코덱스 자신은 `cargo test`/selftest 재실행이나 뮤테이션
  재현은 하지 않았다고 명시했다(코드 대조로만 논리 검증) — 실행
  기반 확인은 이 세션이 앞서 이미 5회 연속 수행했다.
- 리포트: 이 이력 항목 + 계획 문서 "단계 6" 절.

---

## 2026-08-18 12:40 — coordinator/agent 핸드셰이크 단계 5: 거부 경로 3종 완료 — 계획 전 단계 종료

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  단계 5(마지막 남은 단계). 사용자 지시 — 자율 루프 계속 + "코덱스
  최대한 쿼터 써서" 재확인.
- 스트림: Coordinator, Agent, CLI.
- 수행: 코덱스에게 "정직한 프로세스는 자기 서명을 위조 못 한다" 는
  문제의 설계를 다시 맡겼다(`p56` 프롬프트) — TCP proxy 로 진짜
  중간자 변조를 만드는 안보다 **stub 자체에 테스트 전용
  self-corruption 플래그를 넣는 안**을 권장받아 채택했다.
  - `CoordinatorConfig`/`AgentConfig` 에 `corrupt_own_signature: bool`
    — 서명 직후 마지막 바이트를 뒤집는다.
  - `CoordinatorConfig::send_grant_twice`/`AgentConfig::expect_replay`
    — 같은 wire bytes(재인코딩 없이 동일 `frame`)를 한 TCP 연결에
    두 번 써서 실제 replay 를 재현한다. `signing.rs:826` 을 직접
    읽어 `verify()` 가 `Duplicate` 를 만나면 `read_frame` 단계에서
    이미 `Err` 를 반환함을 확인하고 그 성질에 기대 설계했다.
  - `coordinator_agent_selftest.rs` 를 `Fixture`/`HandshakeOutcome`/
    `run_handshake()` 로 리팩터링해 4개 시나리오(정상·위조 Grant·
    위조 ACK·replay)를 공통 오케스트레이션으로 돌린다.
- **replay DoD 문구를 정직하게 정정**: 원래 "`DurableReplayGuard` 가
  거부한다" 였으나, 두 stub 은 `InMemoryReplayGuard` 를 쓴다(실행
  간 replay 상태 비공유) — 이 selftest 가 증명하는 것은 "같은
  프로세스·같은 guard 수명 안에서 동일 wire bytes 두 번째가
  거부되는가" 이지 "재시작을 넘는 replay 방어" 가 아니다. 후자는
  이미 별도로 `durable_replay_process.rs`(같은 날 앞선 작업)가
  증명했다. 과장하지 않고 DoD 문구를 "replay guard 가 거부한다"로
  고쳤다.
- 뮤테이션 테스트로 비공허성 증명(대표 1건): 위조 Grant 시나리오의
  `corrupt_own_signature` 처리를 `if false && ...` 로 무력화 →
  selftest 가 예상대로 실패("Agent 가 ACK 를 발급했다") → 원복 →
  4개 시나리오 전부 재통과 확인. 이 세션 내내 쓴 패턴(백업→뮤테이션
  →실패 확인→원복) 그대로.
- 검증: 5회 연속 `coordinator-agent-selftest` 실행 — 4개 시나리오
  전부 매번 통과, 총 실행 시간 ~1.2초(위조 Grant 시나리오에서 Agent
  가 검증 실패로 즉시 종료 → TCP 연결도 즉시 닫혀 Coordinator 의
  ACK 대기가 10초 타임아웃을 다 기다리지 않는다 — 당초 "느릴 수
  있다"는 우려를 실측으로 기각). `cargo test --workspace`
  297/0/1(ignored) 유지. `cargo build --workspace` 경고 0.
- **이로써 이 계획의 단계 1~6 전부 완료됐다.** 남은 것은 계획
  문서 자체가 명시한 DoD 마지막 항목(`docs/evidence/` 에 schema v2
  형식으로 정식 기록) 뿐이며, 이는 이 계획이 처음부터 "완전한
  coordinator/agent 가 아니다" 라고 명시한 범위(lease·스케줄링·
  다중 Agent·운영용 key protection·TLS 등)와는 무관하다.
- 리포트: 이 이력 항목 + 계획 문서 "단계 5 수행 메모" 절.

---

## 2026-08-18 11:55 — coordinator-agent-selftest 코덱스 검수: stderr 파이프 교착 위험 수정

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  단계 6(코덱스 독립 검수). 사용자 지시 — 자율 루프 계속.
- 스트림: CLI, Coordinator, Agent.
- 결과: 1라운드 `CHANGES_REQUESTED`. 코덱스가
  `crates/cli/src/coordinator_agent_selftest.rs` 를 지적했다 —
  coordinator 의 stderr 를 메인 흐름과 **동시에** 비우지 않아서,
  coordinator 가 OS 파이프 버퍼를 채울 만큼 stderr 에 쓰면(에러 메시지가
  길어지는 경우 등) coordinator 가 쓰기에서 블로킹되고, agent 는
  coordinator 의 TCP 응답을 기다리느라 블로킹되어 이 selftest 전체가
  교착할 수 있다는 지적 — 정상 경로에서는 coordinator 가 stderr 에
  아무것도 안 쓰므로 지금까지 5회 연속 실행에서는 드러나지 않았지만,
  구조적으로는 진짜 결함이었다. 검증 순서·키 배분·PID 검사·
  `InMemoryReplayGuard` 사용·`derive_nonce` 안전성은 전부 문제없음을
  코드 대조로 확인받았다.
- 수행: coordinator 의 stderr 를 `thread::spawn` 으로 만든 별도
  스레드가 처음부터 끝까지 비우도록 고쳤다(`read_to_string`), 메인
  흐름은 `.join()` 으로 나중에 결과를 받는다. stdout 은 READY/RESULT
  줄을 순서대로 읽어야 하므로 메인 스레드에 남겼다 — stdout·stderr
  를 분리한 이유가 서로 다르다(하나는 순서 의존, 하나는 그냥 비우면
  됨). 부가로 `derive_nonce` 의 "운영 코드가 이 패턴을 쓰면 안 되는
  이유" 경고를 `coordinator`·`agent` 양쪽에 대칭적으로 명시했다.
- 검증: `cargo build --workspace` 경고 0. 수정 후 5회 연속
  `coordinator-agent-selftest` 재실행 — 매번 성공, 매번 다른 PID 3개.
  `cargo test --workspace` 297/0/1(ignored) 유지.
- 리포트: 이 이력 항목 + 계획 문서 "단계 6" 절.

---

## 2026-08-18 11:20 — coordinator/agent 핸드셰이크 단계 3·4: 별도 프로세스 실제 handshake 성공

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md` 단계 3·4.
  사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지" (자율 루프 계속).
- 스트림: Coordinator(신규), Agent(신규), CLI.
- 수행: `crates/coordinator`·`crates/agent` 신설, `gputeer
  coordinator-stub`/`agent-stub`/`coordinator-agent-selftest` 배선.
  단계 1·2 검증 결과를 그대로 따라 `PersistentKeyring` 대신
  `InMemoryKeyring`(검증용) + 호출자가 직접 쥔 `SigningKey`(서명용)
  패턴을 썼다 — `selftest.rs:540` 이미 쓰는 패턴, 새 keyring API
  불필요. 두 stub 은 서로 다른 OS 프로세스이므로 키를 hex 인자로
  주고받는다(`--own-seed`/`--peer-pubkey`) — 실제 키 프로비저닝은
  범위 밖(계획 "Out" 절). 자세한 내용은 계획 문서 "단계 3·4 수행 메모".
- 실제로 `gputeer coordinator-agent-selftest` 를 실행해 보고서야 잡은
  결함: `coordinator.stdout.take()` 로 READY 줄을 읽은 뒤
  `wait_with_output()` 을 또 부르면 이미 소비된 stdout 핸들 때문에
  RESULT 줄이 조용히 빈 문자열이 된다. `wait()` + 직접 읽기로 고쳤다.
  고친 뒤 5회 연속 실행해 매번 서로 다른 PID 3개(자기 자신·
  coordinator·agent)로 성공을 확인 — 타이밍 경합 없음.
- 구현 중 계획서에 없던 안전망(`stream_ownership.rs::
  every_crate_is_covered_by_ownership_rules`)이 새 크레이트를 감지해
  걸렸다 — `docs/contracts/01_스트림_소유권.md`·`RULE.md` §4.1 에는
  Coordinator/Agent 자리가 이미 예약돼 있었지만 이 테스트의 `KNOWN`
  목록엔 없었다. 추가해 해소.
- 이 단계가 실제로 증명하는 것: Coordinator·Agent 가 진짜 별도 PID다 ·
  127.0.0.1 실제 TCP 연결이 성립한다 · Coordinator 가 canonical
  `ExecutionGrant` 를 서명한다 · Agent 가 `framed_ingress::read_frame`
  으로 Grant 를 검증하고 `require_replay_checked()` 를 통과한 뒤에만
  ACK 를 만든다 · Coordinator 가 ACK 를 검증하고 grant_id/attempt_id/
  agent_device_id 를 대조한다. **증명하지 않는 것**: 거부 경로(위조
  Grant·위조 ACK·replay — 단계 5), Job 실행·스케줄링·lease·다중 Agent·
  운영용 key protection(계획 "Out" 절 그대로).
- 검증: `cargo build --workspace` 경고 0. `cargo test --workspace`
  297/0/1(ignored) — 이전과 동일(coordinator/agent 크레이트에는 아직
  자체 단위 테스트가 없다. 검증은 `coordinator-agent-selftest` 5회
  연속 실행으로 했다). `gputeer coordinator-agent-selftest` exit 0.
- 리포트: 이 이력 항목 + 계획 문서 "단계 3·4 수행 메모" 절. 남은
  단계(5: 거부 경로 3종, 6: 코덱스 독립 검수)는 계획서에 남겨 뒀다.

---

## 2026-08-18 10:05 — coordinator/agent 핸드셰이크 단계 1·2: `AgentGrantAck` 서명 대상 메시지 + framed_ingress 배선

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md` 단계 1·2.
  사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지" (자율 루프 계속).
- 스트림: Protocol, Crypto.
- 수행: 계획서 자신이 요구한 "확인 안 됨" 3건부터 실측 검증(계획 문서의
  "단계 1·2 수행 메모" 절 참조 — 요약: `PersistentKeyring` 은 서명키를
  나중에 다시 꺼내는 API가 없지만 `selftest.rs:198` 의 기존 패턴(호출자가
  `SigningKey` 를 별도 보관)으로 충분함을 확인, 새 message 필드 번호
  충돌 없음을 빌드로 확인, `canonical_vectors.rs:304-324` 가 domain_tag
  개수를 하드코딩하는 정확한 위치를 특정). 그 다음 실제 구현:
  - `proto/control.proto` — `AgentGrantAck`(필드 1~8 + 서명 90) 추가
  - `crates/protocol/src/canonical.rs` — `Domain::GrantAck`
    (`gputeer/v1/grant-ack`) 추가. `ReplicaAck` 재사용 안 함 — Evidence
    lifetime 이라 replay 를 검사하지 않으므로 재사용하면 ACK replay
    방어를 증명할 수 없다(계획서 "왜 ReplicaAck 를 재사용하지 않는가").
  - `crates/protocol/src/to_fields.rs` — `ToCanonicalFields for
    pb::AgentGrantAck` (필드 1~8, nonce=7 포함 — 서명 밖이면 replay
    캐시 우회 가능)
  - `crates/protocol/src/signable.rs` — `Signable for pb::AgentGrantAck`
    (`Lifetime::ShortLived`, replay_nonce = Some(&self.nonce))
  - `crates/crypto/src/framed_ingress.rs` — `FrameType::GrantAck = 10`,
    `IngressMessage::GrantAck`, dispatch 배선
  - `crates/crypto/tests/framed_ingress.rs` — `grant_ack()` 헬퍼 +
    `normal_grant_ack_frame_dispatches_to_the_right_variant` round-trip
  - `docs/protocol/signing.md` §5 — domain_tag 표 23→24종 갱신
- 구현 중 계획서가 예상 못 한 안전망 4개가 순서대로 걸렸다 — 이 저장소가
  스스로 만들어 둔 회귀 방지 그물이 실제로 동작함을 보여준다:
  `field_number_audit.rs::every_impl_is_audited`,
  `lifetime_consistency.rs::every_signable_is_covered`,
  `canonical_vectors.rs::domain_tags_are_32_bytes_and_unique`,
  `schema_fingerprint.rs::proto_schema_matches_recorded_fingerprint`(P0-08
  스키마 진화 가드 — `UPDATE_SCHEMA_FINGERPRINT=1` 로 정당하게 갱신. 필드
  추가가 아니라 **새 메시지 추가**라 schema_version 상향은 불필요).
- 검증: `cargo build --workspace` 성공. `cargo test -p gputeer-protocol`
  · `cargo test -p gputeer-crypto` 각각 전부 green. `cargo test
  --workspace` 297/0/1(ignored) — 이전 296 + GrantAck round-trip 1건.
  ★ `gputeer-checkpoint::write_failure::
  concurrent_startup_gc_treats_not_found_as_normal_race` 가 병렬 실행
  중 1회 우연히 실패 → `--test-threads=1` 단독 재실행 시 통과 확인 →
  이 작업과 무관한 기존 테스트의 타이밍 취약성으로 판단, 별도 조사
  과제로 남긴다(원인은 조사하지 않았다 — 추측하지 않는다).
- 리포트: 이 이력 항목 + 계획 문서 자체("단계 1·2 수행 메모" 절).
  남은 단계(3~6: coordinator/agent crate 신설·CLI 배선·거부 경로·
  독립 검수)는 계획서에 남긴 대로 별도 작업.

---

## 2026-08-18 09:10 — `DurableReplayGuard` 별도 프로세스 replay 경쟁 실측 추가

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). CLAUDE.md "다음에 할 일" 백로그 항목 — 별도
  **프로세스** replay 경쟁은 지금까지 스레드로만 측정했다는 공백.
- 스트림: 저장소 신뢰성 실측 (`crates/crypto`).
- 수행: 코덱스에게 설계를 시켰다(스크래치패드 `p53.md`) —
  `crates/crypto/tests/durable_replay_race.rs` 가 같은 프로세스
  내 스레드로만 경쟁을 만들던 공백을 지적하고, 별도 OS 프로세스로
  경쟁을 강제하는 fixture+테스트 구조를 설계받았다. 그 설계대로:
  - `crates/crypto/src/bin/durable_replay_process_fixture.rs`
    (신규) — `worker`/`holder` 두 서브커맨드. `holder` 는 별도
    `rusqlite::Connection` 으로 `BEGIN IMMEDIATE` 를 잡고, 모든
    worker 가 `check_and_record` 호출 직전 마커 파일을 남길 때까지
    기다린 뒤에만(파일 마커 barrier) 락을 놓거나(정상 경로) 1300ms
    쥐고 있다가(LockTimeout 경로, `BUSY_TIMEOUT`=1000ms 초과) 푼다.
  - `crates/crypto/tests/durable_replay_process.rs` (신규) — 8개
    worker 프로세스를 스폰해 (1) 같은 nonce → 정확히 1개만 Fresh,
    나머지 Duplicate, (2) 서로 다른 nonce → 전부 Fresh(비공허성),
    (3) holder 가 1300ms 락을 쥐면 → Duplicate 로 위장되지 않고
    LockTimeout 을 받는다, 3가지를 검증. 모든 worker 의
    `start_ns` 가 holder 의 `locked_ns`~`released_ns` 구간
    안이었는지 타임스탬프로 재확인해 "우연히 안 겹쳤을 수도
    있다"는 반례를 차단한다.
- 검증(뮤테이션 테스트로 비공허성 증명): `durable_replay.rs` 의
  중복 검사(`if duplicate.is_some() { ... Duplicate }`)를
  `if false && duplicate.is_some()` 로 무력화 → 예상대로
  `separate_processes_same_nonce_have_exactly_one_fresh` 가
  실패(`Duplicate` 개수 0, 기대 7) → 즉시 `.bak` 백업에서 원복 →
  재빌드 후 3개 테스트 재통과 확인. `cargo test --workspace`
  296/0/0(이전 293 + 신규 3), 스키마 위반 0.
- 리포트: 이 이력 항목. 별도 evidence 문서는 만들지 않았다 — 이
  테스트는 RULE.md §7.3 이 요구하는 "evidence 파일"이 아니라
  일반 회귀 테스트이며, Phase 3 `chaos-hooks` kill 테스트와 같은
  선례를 따라 코덱스 독립 리뷰는 선택 사항으로 남겨 뒀다.

---

## 2026-08-18 08:30 — ENV-02 도 ACCEPTED — 이 저장소의 v1 evidence 16건 전부 addendum 재검수 완료

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: —
- 결과: **`ACCEPTED`.** `:158` 이 정확히 D-1 문장을 가리킴을
  확인했고, 앞선 두 라운드 지적이 전부 해소됐다고 확인했다.
- **★ 이로써 이 저장소의 v1 evidence 16건(review-required 14건 +
  ENV-01·02) 전부가 addendum 독립 재검수 `ACCEPTED` 를 받았다.**
  `python scripts/verify_evidence.py` 는 17/18 을 PASS 로 계상하고
  (P0-06 은 FAIL-SCOPE 로 원래도 PASS 가 아니다), 스키마 위반은
  0건이다.
- 이 사이클 전체에서 반복적으로 확인된 것: (1) 코덱스는 정말
  형식적 승인을 하지 않고 매번 파일:줄을 직접 열어 검증했다 —
  라운드당 평균 2~4회. (2) 구현자(이 세션) 스스로도 정정하다가
  새 오류를 만든 사례가 여러 번 있었고(oneof 구현 개수 과장,
  HISTORY.md 줄 번호 자연 붕괴 2회, 통계량 계산 오류, 인용 줄
  번호 오류 다수) 전부 다음 라운드가 잡았다 — **재검수 사이클
  자체가 스스로를 검증하는 도구로 기능했다.**
- 남은 것: v1 → schema v2 실제 승격(frontmatter 전체 교체·정식
  executor/reviewer 메타데이터·raw_output digest)은 여전히 별도
  작업이다. addendum ACCEPTED 는 "정정 내용이 맞다"는 뜻이지
  "이 evidence 가 schema v2 다"가 아니다.
- 검증: `scripts/verify_evidence.py` 스키마 위반 0. `cargo test
  --workspace` 293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 08:20 — ENV-01 ACCEPTED, ENV-02 인용 오류 1건 더 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: —
- 결과: **`ENV-01` `ACCEPTED`.** `cargo.exe` 직접 경로 실행과 PATH
  미검출을 둘 다 재현해 확인했다. `ENV-02` 는 `CHANGES_REQUESTED`
  — 코덱스 샌드박스는 네트워크가 막혀 x600 접속을 재현하지 못했고
  (문서가 이미 그 사실을 명시), 남은 결함은 D-1 문장 인용이
  `:60`(엉뚱한 decision 필드)을 가리킨 것 — 실제는 `:158`
  (limitations 목록)이었다. 고쳤다.
- 검증: `scripts/verify_evidence.py` 재확인. `cargo test --workspace`
  293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 08:10 — ENV-01·ENV-02 재검수: "Rust 없음" 이 둘 다 stale — PATH 미등록과 미설치 혼동

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). review-required 대상은 아니지만 완결성을 위해
  남은 v1 evidence 2건 착수.
- 스트림: —(환경 기록)
- 결과: 둘 다 `CHANGES_REQUESTED` — 같은 패턴의 stale 이었다.
  둘 다 "Rust 툴체인이 없다"고 적었는데, 실제로는 **PATH 에
  없을 뿐 `.cargo\bin` 에 설치되어 있었다.** 이 개발 기계는
  `C:\Users\playdata2\.cargo\bin` 에, x600 은
  `C:\Users\<x600-user>\.cargo\bin` 에 각각 cargo 1.97.1 이 실재함을
  직접 확인했다 — 코덱스의 read-only 샌드박스는 네트워크가
  막혀 x600 재접속을 못 했지만, 이 세션은 이미 써 온 SSH 접속으로
  직접 재확인했다. D-1("Rust 설치를 사용자 결정으로 상신") 결정도
  둘 다 stale — 설치는 이미 되어 있었고, 남은 문제는 PATH
  등록이다. ENV-02 는 GPU/드라이버 스펙(RTX 4070 SUPER, driver
  595.79)도 다시 재서 evidence 기록과 일치함을 재확인했다.
  base64 전송 방식 limitation 도 그 뒤 `scp` 로 바뀌어 stale —
  이 세션의 P0-07 재실측이 실제로 `scp` 를 썼다.
- 검증: `scripts/verify_evidence.py` 재확인. `cargo test --workspace`
  293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 08:00 — coordinator/agent 최소 핸드셰이크 계획서 작성

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). evidence 재검수 사이클이 끝난 뒤 다음 작업
  단위로 CLAUDE.md 가 반복해서 지적한 가장 큰 공백(coordinator·
  agent 미착수)에 착수했다.
- 스트림: — (계획 문서, 아직 코드 없음)
- 수행: 코덱스(read-only)에게 "완전한 coordinator/agent 가 아니라
  다음 한 걸음만" 설계하도록 요청했다. 결과를
  `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  로 정리했다 — `gputeer coordinator-stub`/`agent-stub` 을
  `Command::current_exe()` 로 별도 PID 로 띄우고, 전용
  `AgentGrantAck` 서명 메시지(`ReplicaAck` 재사용 불가 — Evidence
  lifetime 이라 replay 검사가 없다)로 signed Grant 왕복 +
  위조/replay 거부를 증명하는 최소 설계다.
- ★ **이 계획은 아직 구현하지 않았다.** 새 crate(`crates/coordinator`,
  `crates/agent`) 신설과 proto 스키마 변경(`AgentGrantAck` 추가,
  domain_tag 23→24)은 이번 세션의 다른 작업들(문서 정정·기존
  코드에 대한 검증)보다 훨씬 큰 아키텍처 결정이라, 사용자가 깨어난
  뒤 방향을 확인받는 것이 맞다고 판단해 계획 문서로만 남겼다.
  설계 자체가 스스로 "확인 안 됨"이라 표시한 3가지(PersistentKeyring
  서명 핸들 API, AgentGrantAck 컴파일 여부, domain 개수 하드코딩
  갱신 필요)도 계획 문서에 그대로 옮겼다.
- 검증: 문서만 추가, 코드 변경 없음. `cargo test --workspace`
  293/0/0(불변).
- 리포트: 이 이력 항목

---

## 2026-08-18 07:55 — P0-07 재실측 addendum ACCEPTED (4라운드) — 이번 세션의 evidence 작업 마무리

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Runtime
- 결과: **`ACCEPTED`.** 제목·통계 수치를 PowerShell 로 독립
  재계산해 전부 일치함을 확인했고, `verify_evidence.py` 로
  `status: PASS`·스키마 위반 0 을 재확인했다.
- 이로써 P0-07 재실측 작업(x600 SSH 원격 실행 → 통계 정정 3라운드
  → 최종 ACCEPTED)이 끝났다. 4라운드에 걸쳐 이 addendum 자체에서
  스스로 만든 오류 3건을 순서대로 잡았다: (1) "재확인"과 "불일치
  해소"의 혼동, (2) 통계량 계산 오류(평균 대비 편차와 최솟값-최댓값
  상대차를 섞어 씀), (3) 절 제목이 본문 결론과 모순.
- ★ 이번 세션의 v1 evidence 재검수 작업은 여기서 일단락한다.
  `RULE.md` §7.3 review-required 14건 전부 addendum ACCEPTED,
  그 중 P0-07 은 실제 GPU 재실측까지 거쳐 원래 상태(PASS)로
  돌아왔다. 남은 것: `ENV-01`·`02`(review-required 아님, 미착수),
  v1 → schema v2 실제 승격(frontmatter 전체 교체) 작업.
- 검증: `scripts/verify_evidence.py` 스키마 위반 0. `cargo test
  --workspace` 293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 07:45 — P0-07 재실측 addendum 3라운드: 절 제목이 본문과 모순됐다

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Runtime
- 결과: `CHANGES_REQUESTED`. 통계 수치(평균 0.020167, 편차 +3.1%·
  −8.8%·+5.6%, 최솟값 대비 최댓값 15.76%)는 재검수가 직접 재계산해
  전부 정확하다고 확인했다. 다만 절 제목 "★ 재실측으로 해소"가
  본문 자신의 정직한 결론("원래 불일치의 원인은 해소되지 않았다")
  과 모순된다는 것을 지적받았다 — "재실측으로 claim 재확인 (원래
  불일치는 미해결)"로 고쳤다.
- 검증: `scripts/verify_evidence.py` 재확인. `cargo test --workspace`
  293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 07:35 — P0-07 재실측 addendum 2라운드: 통계량 자체도 잘못 계산했었다

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Runtime
- 결과: `CHANGES_REQUESTED`. 앞선 라운드의 핵심 지적(claim 재확인
  vs 원래 불일치 해소 구분, "정상 변동" 단정 제거)은 해소됐다고
  확인됐지만, 그 정정문 안에 있던 **수치 자체가 또 틀렸다** —
  "평균 대비 편차 14~16%"라고 썼는데, 재검수가 PowerShell 로 직접
  계산해 보니 실제 평균 대비 편차는 +3.1%·−8.8%·+5.6% 이고,
  "15.76%"는 최솟값 대비 최댓값의 상대 차이였다(평균 대비 편차가
  아니다) — 서로 다른 두 통계량을 섞어 쓴 것이었다. 정확한 값으로
  고쳤다.
- ★ 같은 addendum 안에서 "근거 없는 통계적 단정을 고친다"고 써
  놓고 그 정정문 자체에 또 다른 통계 계산 오류를 넣은 것 — 이
  세션 전체에서 반복된 패턴("정정하다가 새 오류를 만든다")의
  가장 미묘한 사례다.
- 검증: `scripts/verify_evidence.py` 재확인. `cargo test --workspace`
  293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 07:20 — P0-07 재실측 addendum 정정: "재확인" 과 "불일치 해소" 를 혼동했었다

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 방금 만든 재실측 addendum 도 곧바로 재검수에
  맡겼다.
- 스트림: Runtime
- 결과: `CHANGES_REQUESTED`. "세 σ 값이 15% 이내로 근접하고 정상적인
  실행 간 변동으로 설명 가능하다"는 문장이 **통계적 근거 없는
  단정**이었다 — 사전 정의된 허용 변동 범위가 이 evidence 에 없었고,
  "15% 이내"라는 표현도 기준(평균? 최솟값?)이 없어 값에 따라
  성립하기도 안 하기도 했다. 더 근본적으로 "재실측이 원래 불일치를
  해소했다"와 "재실측이 claim 을 재확인했다"를 하나로 뭉뚱그렸다 —
  후자만 참이고, 원래 두 기록이 왜 서로 달랐는지는 여전히 확인
  안 됨으로 남아야 했다. 이 구분을 명시하고 근거 없는 "정상 변동"
  단정을 제거했다 — `_raw/P0-07_probe_2026-08-18_rerun.txt` 에
  붙였던 같은 분석문도 raw 파일에서 빼고 .md 쪽 addendum 으로
  옮겼다(raw 파일은 원문만 남기는 것이 이 저장소의 관례다).
  `status: PASS` 자체(claim 재확인 근거)는 유지한다 — 재검수도
  그 점을 문제 삼지 않았다.
- 검증: `scripts/verify_evidence.py` 로 재확인. `cargo test
  --workspace` 293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 06:48 — P0-07 실제 재실측: x600 GPU 로 σ 재확인, status 를 PASS 로 복원

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 재검수 사이클이 끝난 뒤, 미뤄뒀던 실제 재측정을
  실행했다.
- 스트림: Runtime
- 수행: `~/.ssh/config` 의 `x600` 접속이 살아 있고 GPU(RTX 4070
  SUPER)·torch(2.13.0+cu126, CUDA)가 그대로 있음을 확인했다.
  `tools/probes/p0_07_runtime_estimation.py` 를 그대로 scp 로 복사해
  인자 없이(원래 RUN1 과 동일 설정) 실행했다.
- 결과: **σ=0.0213** (평균 1.9%, 최대 4.7%, DoD PASS). 기존 두 상충
  기록(evidence 본문 σ=0.0208, `_raw` 원문 σ=0.0184)과 비교하면
  세 값 모두 15% 이내로 근접하고 전부 DoD 를 여유 있게 통과한다 —
  정상적인 실행 간 변동으로 설명 가능하며 조작·계산 오류의 증거는
  없다. 원래 불일치의 정확한 원인(전사 오류 등)은 여전히 특정할
  수 없지만, **claim 자체는 독립적인 세 번째 실행으로 재확인됐다.**
- 조치: 새 raw artifact
  `docs/evidence/_raw/P0-07_probe_2026-08-18_rerun.txt` 로 원문을
  저장했다. **frontmatter `status` 를 `INCONCLUSIVE` 에서 다시
  `PASS` 로 정정했다** — 판정 불가 상태를 만들었던 근거 부재가
  실제 재측정으로 해소됐기 때문이다. `INCONCLUSIVE` 로 낮췄던
  판단 자체는 그 시점엔 옳았다(근거 없이 PASS 를 유지할 수
  없었다) — 지금은 근거가 생겼을 뿐이다.
- ★ 이 재실측·status 복원은 아직 독립 재검수를 거치지 않았다 —
  다음에 이 문서를 다루는 세션이 검수해야 한다.
- 검증: `scripts/verify_evidence.py` 로 `status: PASS` 재확인
  (파싱 오류 없음). `cargo test --workspace` 293/0/0(코드 변경 없음).
- 리포트: 이 이력 항목

---

## 2026-08-18 02:30 — P0-06·P0-07 도 ACCEPTED — RULE.md §7.3 review-required v1 evidence 전체(14건) 재검수 완료

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프). 이 사이클의 마지막 2건.
- 스트림: Runtime
- 결과: **둘 다 `ACCEPTED`.** `python scripts/verify_evidence.py --json`
  exit 0 과 PyYAML `safe_load` 로 `P0-07` 의 `status='INCONCLUSIVE'`
  파싱을 재검수가 직접 재현해 확인했다. "관측값을 고친 것이 아니라
  판정만 바꾼 것이므로 evidence 철학과 충돌하지 않는다."
- **★ 이로써 `RULE.md` §7.3 review-required v1 evidence(14건:
  `DoD-01`~`08`, `P0-01`·`03`·`03a`·`06`·`07`·`08`) 전부가 독립
  재검수 `ACCEPTED` 를 받았다.** 라운드 수 합계 34회(문서당
  1~4라운드). 그 과정에서:
  - 실질적 stale limitation·claim 범위 초과·negative_tests 이름
    오류를 수십 건 찾아 고쳤다.
  - **frontmatter 를 실제로 고친 것은 `P0-07` 의 `status` 필드
    단 하나** — 나머지는 전부 append-only 절로 처리했다.
  - 구현자(이 세션) 스스로 정정하다가 새 오류를 만든 사례가
    최소 3번 있었다(`ControlAction` oneof 구현 개수 과장,
    `HISTORY.md` 줄 번호 자연 붕괴 2회, "frontmatter 에 반영했다"
    는 거짓 문장) — 전부 다음 라운드가 잡았다.
  - `P0-01b`·`P0-07b` 등 "후속 스파이크로 분리한다"고 decision 에
    적었지만 실제로는 한 번도 실행되지 않은 약속이 최소 2건 있었다.
  - `docs/history/HISTORY.md` 처럼 계속 자라는 append-only 파일에
    줄 번호로 인용하면 그 인용이 세션 안에서도 저절로 틀려진다는
    것을 배웠다 — 이후 제목 기반 인용으로 전환했다.
  - **v1 → schema v2 승격(frontmatter 전체 교체·정식
    executor/reviewer 메타데이터·raw_output digest)은 여전히
    별도 작업으로 남아 있다** — addendum ACCEPTED 는 "정정 내용이
    맞다"는 뜻이지 "이 evidence 가 schema v2 다"가 아니다.
    `verify_evidence.py` 는 지금도 이 14건을 v1 로 계상한다.
  - `ENV-01`·`02` 는 `RULE.md` §7.3 의 강제 대상이 아니라 이번
    사이클에서 다루지 않았다.
- 검증: `scripts/verify_evidence.py` 스키마 위반 0. `cargo test
  --workspace` 293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 02:15 — P0-07 의 status 를 PASS → INCONCLUSIVE 로 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 2라운드 재검수 반영.
- 스트림: Runtime
- 결과: 둘 다 여전히 `CHANGES_REQUESTED` — 이번 라운드는 더
  근본적인 지적이었다.
  - **P0-07**: 재검수가 raw_output 불일치 표 자체는 정확하다고
    확인했지만, "두 수치 다 DoD 통과이니 PASS 유지"라는 판단이
    `RULE.md` §7.1 기준으로 틀렸다고 지적했다 — `INCONCLUSIVE`
    ("측정은 했으나 판정 불가")가 정확히 이 상황을 위한 상태값이다.
    ★ **`status` 필드를 `PASS` 에서 `INCONCLUSIVE` 로 정정했다** —
    이번 v1-evidence 재검수 사이클 전체에서 frontmatter 를 실제로
    고친 유일한 경우다. `claim`·`raw_output`·`decision` 등 나머지는
    당시 기록 그대로 보존했다 — `status` 는 관측이 아니라 그
    관측에 대한 판정이므로, 판정 근거(raw_output 신뢰성)가
    무너지면 판정도 정정하는 것이 옳다고 판단했다. YAML 안에
    inline 주석(`#`)을 달았다가 `verify_evidence.py` 의 최소
    파서가 그것까지 값으로 먹어버릴 뻔한 것을 스스로 잡아
    수정했다 — 설명은 frontmatter 밖(본문 addendum)으로 옮겼다.
    `scripts/verify_evidence.py` 로 재확인: PASS 계상 17→16건,
    파싱 오류 없음.
  - **P0-06**: "frontmatter claim 에도 반영했다"는 문장이 거짓이었다
    (재검수가 지적 — 실제로는 addendum 의 해석일 뿐 `claim` 필드는
    안 고쳤다). "이 addendum 이 명시적으로 해석해 보여주는 것"으로
    정정했다.
- 검증: `scripts/verify_evidence.py` 로 두 evidence 파일의 frontmatter
  가 여전히 유효 파싱됨을 확인. `cargo test --workspace` 293/0/0
  재확인(코드 변경 없음).
- 리포트: 이 이력 항목

---

## 2026-08-18 02:00 — P0-06·P0-07 1라운드 정정 — P0-07 에서 raw_output/artifact 수치 불일치 발견

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). `RULE.md` §7.3 review-required v1 evidence 의
  **마지막 2건**.
- 스트림: Runtime
- 결과: 둘 다 `CHANGES_REQUESTED`.
  - P0-06: claim("VRAM quota 강제 수단이 없다")이 evidence 본문
    자신이 찾은 사실(Windows Job Object 의 간접 총 커밋 상한)과
    표면적으로 충돌하는 것처럼 읽혔다 — "정밀한 hard VRAM quota
    는 없지만 코스한 간접 제한은 가능하다"로 명시했다.
    `runtime-policy` 크레이트가 VRAM 판정을 분류하지만 실제
    Job Object 를 생성·설정하지는 않는다는 limitation 을 추가했다.
  - P0-07: ★ **claim 범위 문제보다 심각한 것을 찾았다** — 이
    문서의 frontmatter `raw_output` 요약과 링크된
    `docs/evidence/_raw/P0-07_probe.txt` 원문의 **숫자가 서로
    다르다**(σ=0.0208 vs 0.0184, RUN2 최대오차 9.3% vs 6.1% 등).
    둘 다 DoD(σ<=0.20) 는 통과해 최종 판정(PASS)은 안 바뀌지만,
    이 문서에 적힌 구체적 수치를 그대로 신뢰할 수 없다는 뜻이다.
    재실측 없이는 어느 쪽이 맞는지 판별할 수 없어 **불일치 사실
    자체를 정직하게 기록**했다 — 임의로 하나를 골라 조용히
    통일하지 않았다. `P0-07b`(노드 간 외삽) 후속 검증도
    `P0-01b` 와 마찬가지로 저장소에 실제로 존재하지 않는다는
    것을 확인했다.
  - 둘 다 원본 YAML 은 당시 기록이므로 고치지 않고 append-only
    로 정정했다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:50 — P0-03a 도 3라운드 만에 ACCEPTED — v1 evidence 12건 addendum ACCEPTED 누적

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Checkpoint
- 결과: **`ACCEPTED`.** `write_failure.rs:5-20` 이 전부 모듈 문서
  주석일 뿐임을 재확인했고, `durability_chaos.rs:371-384` 가 공유
  모드와 무관하게 설계됨을, `write_failure.rs:52-66` 의 지금 실패
  주입이 디렉터리 rename 방식임을 재확인했다.
- 누적: 지금까지 `DoD-01`~`08`(8건 전부)·`P0-01`·`P0-03`·`P0-03a`·
  `P0-08` **12건**의 addendum 이 독립 재검수 `ACCEPTED` 를 받았다.
  남은 v1 evidence 는 `P0-06`·`P0-07` **2건**뿐이다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:40 — P0-01 ACCEPTED, P0-03a 는 근거 과장 1건 더 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Runtime · Checkpoint
- 결과: **`P0-01` `ACCEPTED`.** `P0-03a` 는 여전히
  `CHANGES_REQUESTED` — `FILE_SHARE_DELETE` 재확인 근거를 과장했다:
  "`durability_chaos.rs`/`write_failure.rs` 가 재확인 테스트"라고
  적었는데, 실제로는 `write_failure.rs` 모듈 문서에 **서술로만**
  남아 있고 독립 검증 테스트는 없었다. "서술로만 기록, 재검증
  테스트 없음"으로 좁혔다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:30 — P0-01·P0-03a 1라운드 정정 (P0-01 은 GPU 하드웨어라 재실측 불가)

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). `DoD-` 8건이 모두 끝나 `P0-` 계열로 넘어갔다.
- 스트림: Runtime · Checkpoint
- 결과: 둘 다 `CHANGES_REQUESTED`.
  - P0-01: ★ 이 evidence 는 실제 NVIDIA GPU 하드웨어 실측이라
    개발 기계(Intel Iris Xe)에서는 재실측할 수 없다 — 그 사실을
    명시하고 코드·문서의 내적 일관성만 확인했다. claim 을
    "`DISABLE_MAX_PRIVILEGE` 토큰 + 단일 CUDA 프로세스" 범위로
    좁혔다(관리자 SID 비활성·프로세스 트리는 미시험). negative_tests
    분류 정정(과거 오판 서술을 재실행 가능한 테스트처럼 나열했던
    항목 1건). **결정문이 약속한 P0-01b 후속 검증이 실제로는
    존재하지 않는다**는 것도 확인해 명시했다.
  - P0-03a: claim 을 "원래 절차가 Windows 에서 그대로 성립한다"가
    아니라 "원래 절차 실패를 실측하고 ADR-026 수정안을 도출했다"
    로 명시했다. stale limitation 1건 — "Rust std::fs 공유 모드
    별도 확인 필요"가 그 뒤 실제로 확인됐다(FILE_SHARE_DELETE 포함
    — 그 발견 경위 자체가 흥미롭다: 옛 실패 주입 테스트가 반대
    가정에 기대다가 스스로 넣은 "성공하면 무효" 단언에 걸려 잡혔다).
  - 둘 다 원본 YAML 은 당시 기록이므로 고치지 않고 append-only 로
    정정했다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:20 — DoD-01·P0-08 둘 다 3라운드 만에 ACCEPTED — v1 evidence 10건 addendum ACCEPTED 누적

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Protocol
- 결과: **둘 다 `ACCEPTED`.** `canonical_v1.json:7` 이 실제 `vectors`
  배열 시작임과 40건을 재확인했고, `HISTORY.md` 제목 인용도 실제
  내용과 일치함을 확인했다. `q6` 이 sig_input·canonical 결속을 각각
  직접 단언하는 두 assert 를 확인했다.
- 누적: 지금까지 `DoD-01`·`DoD-02`·`DoD-03`·`DoD-04`·`DoD-05`·`DoD-06`·
  `DoD-07`·`DoD-08`·`P0-03`·`P0-08` **10건**의 addendum 이 독립
  재검수 `ACCEPTED` 를 받았다 — **`DoD-` 접두사 evidence 는 이로써
  전부(8/8) 완료됐다.** 재확인해 보니 남은 것은 `P0-01`·`P0-03a`·
  `P0-06`·`P0-07` **4건**이다(`ENV-01`·`02` 는 `RULE.md` §7.3 의
  독립 검수 강제 대상 접두사 `DoD-`/`P0-` 에 포함되지 않는다 —
  CLAUDE.md 의 이전 "v1 13건" 수치가 이 구분을 정확히 반영하지
  않고 있었다는 것도 이번에 재확인하며 발견했다).
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:10 — DoD-01·P0-08 2라운드: 인용 정밀도 오류 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Protocol
- 결과: 둘 다 여전히 `CHANGES_REQUESTED` — 이번엔 사실관계가 아니라
  **인용의 정밀도** 문제였다.
  - DoD-01: 벡터 40건 인용이 `canonical_v1.json:2`(spec 선언)를
    가리켜 실제로 개수를 입증하지 못했다 — `:7`(vectors 배열 시작)
    로 고쳤다. `HISTORY.md` 제목 인용도 틀렸다 — "09:30 — prost
    연동 계층" 에는 12→20 기록이 없고, 실제로는 "10:40 — 서명 밖
    필드 6건 제거" 항목이었다.
  - P0-08: `schema_version` 의 canonical 결속 근거가 `canonical.rs:350,360`
    (sig_input 설명일 뿐)을 가리켰다 — 실제로는
    `schema_evolution.rs:249,254-256` 의 `q6` 이 canonical·sig_input
    결속을 **둘 다** 직접 단언한다. q6 을 q4·q5 와 같은 범위로
    뭉뚱그렸는데 q6 은 canonical 비교까지 포함해 범위가 더 넓었다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:00 — DoD-01·P0-08 1라운드 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 이 저장소에서 가장 오래된 evidence(DoD-01)와
  P0-08 착수.
- 스트림: Protocol
- 결과: 둘 다 `CHANGES_REQUESTED`.
  - DoD-01: claim("두 구현이 모든 범위에서 바이트 단위로 일치")이
    넓게 읽혔다 — `canonical_vectors.rs` 가 40개 벡터 전체가 아니라
    수동 구성한 부분집합만 순회한다고 좁혔다. domain 수치 17→23
    정정, stale limitation 4건(JobManifest 부분집합·prost 미구현·
    Ed25519 미검증·SCHEMA_TOO_NEW 미구현 — 전부 그 뒤 해소됨) 정정.
    이 evidence 당시 12건이던 벡터와 지금 40건을 명시적으로
    구분했다.
  - P0-08: claim 핵심은 유지. "SCHEMA_TOO_NEW 반환 경로 미구현"
    limitation 이 stale(지금 구현되어 있다). "지문 가드 뮤테이션"
    항목이 named test 가 아니라 수동 뮤테이션 실험이라는 분류
    정정. q4·q5·q6 이 실제 Ed25519 가 아니라 sig_input 비교라는
    범위 명시.
  - 둘 다 원본 YAML 은 당시 기록이므로 고치지 않고 append-only 로
    정정했다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:50 — DoD-07 도 2라운드 만에 ACCEPTED — v1 evidence 8건 addendum ACCEPTED 누적

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Crypto
- 결과: **`ACCEPTED`.** vectors 40건·`evidence_has_no_replay_defense_and_says_so`
  함수명·`DurableReplayGuard`/`PersistentKeyring` 인용을 전부 직접
  열어 재확인했다. `HISTORY.md` 인용도 제목 기반으로 바뀌어 문제
  없었다.
- 누적: 지금까지 `DoD-02`·`DoD-03`·`DoD-04`·`DoD-05`·`DoD-06`·`DoD-07`·
  `DoD-08`·`P0-03` **8건**의 addendum 이 독립 재검수 `ACCEPTED` 를
  받았다. 나머지 v1 evidence 는 **5건**.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:40 — DoD-08 1라운드 만에 ACCEPTED, DoD-07 1라운드 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 다음 v1 evidence 2건 착수.
- 스트림: Protocol · Crypto
- 결과: `DoD-08`(독립검수 시정)이 **1라운드 만에 `ACCEPTED`** — 이
  사이클에서 첫 즉시 통과 사례다. `DoD-07`(시각정책 실메시지)은
  `CHANGES_REQUESTED`: vectors 36→40 정정, negative_tests 이름 오류
  1건(`evidence_is_not_replay_checked` → 실제
  `evidence_has_no_replay_defense_and_says_so`), stale limitation
  2건(§10 replay·키 관리 — 둘 다 그 뒤 실제로 구현된 것을 "미구현"
  으로 남겨둔 채였다) 정정.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:30 — DoD-02 도 4라운드 만에 ACCEPTED — v1 evidence 6건 재검수 사이클 종료

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). DoD-02 의 마지막 남은 지적(HISTORY.md 인용
  방식)을 고친 버전을 네 번째로 재검수했다.
- 스트림: Protocol
- 결과: **`ACCEPTED`.** 제목 기반 인용으로 `HISTORY.md` 항목을
  다시 찾아 필드 편입 내용을 확인했고, 이 문서에 그동안 쌓인 모든
  정정(claim 범위·`ControlAction` 9/21·`Lease` 대조 인용·
  negative_tests 이름)을 처음부터 다시 훑어 "추가 수정 사항은
  확인되지 않았다"로 마무리했다.
- **이로써 이번 v1-evidence 재검수 사이클 `DoD-02`·`DoD-03`·`DoD-04`·
  `DoD-05`·`DoD-06`·`P0-03` 6건 전부가 addendum 독립 재검수
  `ACCEPTED` 를 받았다.** 라운드 수: `DoD-02`·`DoD-03` 각 4회,
  `P0-03` 2회, `DoD-04`·`DoD-05`·`DoD-06` 각 3회. 이 과정에서 확인된
  것: (1) 코덱스는 형식적 승인을 하지 않고 매번 file:line 을 직접
  열어 검증했다. (2) 구현자(이 세션) 스스로도 정정하다가 두 번
  새 오류를 만들었다 — `ControlAction` oneof 구현 개수 과장(9/21을
  "전부 구현"으로 잘못 정정)과 `HISTORY.md` 줄 번호 인용의 자연
  붕괴(append-at-top 파일에 줄 번호를 인용하면 세션이 진행될수록
  스스로 틀려진다) — 둘 다 재검수가 잡았다.
- **문서 전체를 schema v2 로 승격하는 것은 여전히 별도 작업**이다
  (frontmatter 교체 · 정식 executor/reviewer 메타데이터 ·
  raw_output digest). addendum ACCEPTED 와 혼동하지 않는다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:20 — DoD-05 ACCEPTED, DoD-02 는 HISTORY.md 줄 번호 불안정성 문제로 4라운드째

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Protocol
- 결과: **`DoD-05` `ACCEPTED`.** `DoD-02` 는 여전히
  `CHANGES_REQUESTED` — 그런데 이번엔 **이 세션 스스로가 만든
  구조적 문제**가 원인이었다: 직전 라운드에서 `docs/history/HISTORY.md:629-645`
  로 정확히 인용했는데, 그 사이 이 세션이 `HISTORY.md` 에 새 항목을
  더 추가하면서(최신 항목을 맨 위에 쌓는 append 방식) 대상 항목이
  `:651-667` 로 밀려나 인용이 **다시 틀린 상태**가 됐다. 줄 번호를
  또 맞추는 대신 **제목 기반 인용**으로 바꿨다 — append-only 로
  계속 자라는 파일에 줄 번호 인용을 쓰는 것 자체가 이 재검수
  사이클에서 두 번째로 문제를 일으켰다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:10 — DoD-02·DoD-05 2라운드 재검수 — 자체 만든 과장 하나를 발견

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Protocol
- 결과: 둘 다 여전히 `CHANGES_REQUESTED`. 가장 중요한 지적은 **직전
  라운드에서 구현자 자신이 새로 만든 과장**이었다: `ControlAction`
  의 oneof 하위 메시지 21개 중 9개만 `ToCanonicalFields` 가 구현되어
  있는데, "각각 구현되어 있다"고 적었다. 재검수가 직접 세어 지적했고
  grep 으로 재확인했다(나머지 12개 = 0건). ★ 원래 evidence 의
  limitation("개별 구현이 필요하다")이 이 정정보다 오히려 더 정확한
  상태였다 — 정정하다가 새 과장을 만든 사례. "9/21 구현, 12개
  미구현"으로 다시 고쳤다.
- 그 외: `Lease` 전 필드 바이트 대조 테스트 인용 누락
  (`prost_canonical.rs:700-735` 추가), `HISTORY.md` 인용에 줄 번호
  없음(`:629-645` 추가), DoD-05 의 벡터 증가 이력 주장을 "40건
  이라는 사실만 확인됨, 증가 과정은 확인 안 됨"으로 낮췄다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-17 23:55 — 다음 v1 evidence 2건(DoD-02·DoD-05) 재검수 착수

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속, dynamic 모드). 앞선 4건 사이클이 끝나 다음 v1
  evidence 로 넘어갔다.
- 스트림: Protocol
- 수행: 코덱스(read-only) 1개 태스크로 `DoD-02`(prost 연동 계층)와
  `DoD-05`(T1 서명대상 확장)를 함께 재검수시켰다. 둘 다 첫 라운드에서
  `CHANGES_REQUESTED`.
- 발견과 조치:
  - DoD-02: claim("Python 참조 구현과 바이트 단위로 일치한다")이
    전수 비교가 있는 것처럼 넓게 읽혔다 — 실제 바이트 대조는
    `JobManifest` 중심이고 나머지는 필드 번호·이름 감사뿐이라고
    좁혔다. negative_tests 이름 오류(`common_` → 실제
    `common_message_field_numbers_match_proto`) 1건, stale
    limitation 5건(필드 6개 미구현·17종 중 7종·oneof 없음·SCHEMA_TOO_NEW
    미구현·Ed25519 미구현 — 전부 코드가 이미 해소했거나 범위를
    좁혀야 함) 정정.
  - DoD-05: claim 의 "17종 중 9종" 자체가 stale — 지금은 §5 domain
    이 23종이고 그 중 19종이 구현되어 있다(ADR-028 이 17→23으로
    늘렸다). negative_tests 이름 오류 2건(`renew_lease_request`,
    `revoke_lease_notice` — 실제로는 `_matches_reference` 접미사가
    붙는다), stale limitation 5건(6종 Signable 미구현·domain tag
    공유·ControlAction oneof 서술·17종 중 9종·Ed25519 미구현) 정정.
    vectors 메타데이터도 28건에서 지금 40건으로 정정.
  - 두 문서 모두 원본 YAML(claim/status/limitations)은 당시 기록이므로
    고치지 않고 append-only "이후 변경" 절로 정정했다. **재검수는
    아직 진행 중** — 다음 라운드 대상.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-17 23:45 — DoD-03 도 4라운드 만에 ACCEPTED — evidence 4건 재검수 사이클 종료

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). DoD-03 의 마지막 남은 지적(인용 모호함)을 고친
  버전을 네 번째로 재검수했다.
- 스트림: Protocol
- 결과: **`ACCEPTED`.** `field_number_audit.rs:249/259/269`(번호-이름
  대조)와 `:282`(`every_impl_is_audited`)의 구분이 정확한지, claim
  범위·vectors 40건·Ed25519 등 구분까지 문서 전체를 다시 훑어 "확인
  안 됨: 없음" 으로 마무리했다.
- **이로써 이번 v1-evidence 재검수 사이클(`DoD-03`·`DoD-04`·`DoD-06`·
  `P0-03`) 4건 모두 addendum 이 `ACCEPTED` 를 받았다** — `DoD-03` 은
  4라운드, `P0-03` 은 2라운드, `DoD-04`·`DoD-06` 은 3라운드 만에.
  문서 전체를 schema v2(frontmatter 승격 · executor/reviewer 정식
  메타데이터)로 올리는 것은 여전히 별도 작업으로 남아 있다 — addendum
  ACCEPTED 와 frontmatter v2 승격을 혼동하지 않는다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-17 23:35 — v1 evidence 3라운드 재검수 완료 — DoD-04·DoD-06 ACCEPTED, DoD-03 만 잔류

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프, 코덱스 위주). 직전 라운드에서 남은 인용 오류를 고친
  버전을 세 번째로 재검수했다.
- 스트림: Protocol
- 결과: **`DoD-04`·`DoD-06` 이 이번 라운드에서 `ACCEPTED`.** 인용
  교정(각각 K2/Linux limitation 인용, vectors 40건 대조)이 정확했다고
  확인됐다. `DoD-03` 은 세 라운드째 `CHANGES_REQUESTED` — 이번엔
  실질적 결함이 아니라 **표현의 모호함**이었다: "claim 을 좁혀 읽는다"
  절의 `field_number_audit.rs:249-274` 인용과 "그 외 확인" 절의
  `field_number_audit.rs:282` 인용이 나란히 있어 **같은 결함이 또
  남은 것처럼 읽혔다** — 실제로는 둘 다 진짜인, 서로 다른 함수
  (`common_message_field_numbers_match_proto` 류 vs
  `every_impl_is_audited`) 를 가리키고 있었다. 함수명을 명시해
  구분했다.
- 세 evidence 모두 문서 전체를 schema v2 로 승격하는 것은 별도
  판단이 필요하다고 검수자가 남겼다 — frontmatter 가 여전히 v1 이고,
  ACCEPTED 는 이번 addendum(정정 절)에 한정된다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-17 23:20 — v1 evidence 4건 재검수 완료 라운드 — P0-03 addendum 최초 ACCEPTED

- 계획: 사용자 지시 — "코덱스로 다음 작업들 진행해" 의 연장. 직전 정정본
  (DoD-03·04·06, P0-03)이 실제로 지적을 해소했는지 코덱스(read-only)에게
  최종 재검수를 맡겼다.
- 스트림: Checkpoint · Protocol
- 결과: **P0-03 의 새 addendum(HASH_VERIFIED~COMMITTED 결정적 kill
  테스트) 은 `ACCEPTED`** — 이 저장소에서 독립 검수가 명시적으로
  ACCEPTED 를 준 첫 v1-evidence 정정이다. 나머지 셋은 다시
  `CHANGES_REQUESTED`(이번엔 훨씬 작은 흠):
  - DoD-04: K2/Linux limitation 의 파일:줄 인용이 틀렸다
    (`lib.rs:11-16,47-48` → 실제는 `lib.rs:28-32`). 고쳤다.
  - DoD-06: frontmatter 의 vectors 수(36)가 지금 파일(40)과
    불일치한다는 것을 짚었다 — DoD-03 이 같은 파일을 20→40 으로 이미
    정정한 사실과 연결해 명시했다.
  - DoD-03: (a) `field_number_audit.rs:282` 인용이 실제 필드번호
    대조(`:249-274`)가 아니라 감사망 등록 테스트를 가리켰다. (b)
    Ed25519/SCHEMA_TOO_NEW/runtime-policy limitation 정정이 "구현이
    존재한다"와 "이 evidence 가 직접 실행해 확인했다"를 충분히
    구분하지 않아 과장으로 읽힐 수 있었다 — 세 항목 모두 그 구분을
    명시하도록 다시 썼다.
  - P0-03 의 addendum 자체는 코드에서 직접 확인됐다: `chaos-hooks` 가
    `default` feature 밖에 있고, self-kill 훅이 `replace_with_retry`
    성공 직후·`Committed` 기록 이전에 정확히 있고, 새 테스트 단언이
    필요한 사후 상태를 전부 검사한다는 것. 다만 "8회 연속 실행"
    결과 자체는 검수자가 재실행하지 않아 그 수치는 확인 안 됨으로
    남았다.
- 세 건(DoD-03·04·06)은 이번 라운드에서 지적된 것만 다시 고쳤고,
  **재재검수는 아직 하지 않았다** — CLAUDE.md 에 명시.
- 검증: 이번 라운드는 문서 전용 수정이라 `cargo test --workspace`
  293/0/0 재확인만 했다(코드 변경 없음).
- 리포트: 이 이력 항목

---

## 2026-08-17 23:10 — v1 evidence 4건 독립 재검수(코덱스), HASH_VERIFIED~COMMITTED 결정적 kill 테스트 신설

- 계획: 사용자 지시 — "코덱스로 다음 작업들 진행해" (v1 evidence 부채
  축소를 계속한다).
- 스트림: Checkpoint · Protocol
- 수행: 코덱스(read-only) 3개 태스크 — (1) DoD-04·P0-03 수정본 재검수,
  (2) DoD-03·DoD-06 신규 검수, (3) P0-03 이 지적한 "HASH_VERIFIED~
  COMMITTED 구간을 직접 겨냥한 kill 테스트가 없다"는 공백을 메우는
  설계. 넷 다 최초 라운드에서 `CHANGES_REQUESTED`.
- 발견과 조치:
  1. [실제 결함, 자체 재현] DoD-04 재검수가 지적: 앞선 수정에서 붙인
     raw receipt 가 재현되지 않은 옛 실행 그대로였다. 새로 실행해
     `docs/evidence/_raw/DoD-04_replay_status_doctest_2026-08-17.txt` 로
     교체(sha256 digest 포함). `HISTORY.md` 인용에 전체 경로·줄
     번호가 없었던 것도 소스 파일:줄 직접 인용으로 바꾸고, "단수명
     검증 경로가 매 실행된다"를 `ExecutionGrant` 로 좁혔다.
  2. [설계+구현] P0-03 재검수가 지적한 공백 — `write_checkpoint()` 의
     `LATEST` 교체 직후·`COMMITTED` 마커 기록 직전이라는 좁은 구간을
     기존 8개 시간 기반 kill 시점이 겨냥한 적이 없었다. 코덱스 설계를
     받아 `chaos-hooks` feature(비기본)로 `writer.rs::chaos_kill_after_latest()`
     self-kill 훅을 추가했다 — `replace_with_retry` 성공 뒤 `abort()`
     로 그 자리에서 프로세스를 끝내 코드 순서로 그 구간을 결정적으로
     겨냥한다(race 없음). 새 테스트
     `kill_after_latest_before_committed_is_resume_candidate` 가
     "COMMITTED 마커 없이도 재개된다"를 8/8 연속 직접 관측했고,
     훅 위치를 Committed 뒤로 옮기는 뮤테이션으로 비공허성을 확인했다.
     기본 빌드·`cargo test --workspace` 에는 포함되지 않는다.
  3. `writer.rs:157` 의 stale한 주석("포인터 또는 COMMITTED 마커로
     공개된") 도 함께 고쳤다 — 실제로는 COMMITTED 를 요구하지 않는다.
  4. [claim 범위 정정] DoD-03: vectors 메타데이터가 20 인데 실제는
     40(`python -c "..."` 로 재현). claim 의 "각 필드가 실제로
     영향을 준다"는 `JobManifest` 최상위 필드 전부와 `Lease.scope`
     로 좁혀 읽어야 한다(나머지는 field_number_audit 의 번호-이름
     대조만 있다). Ed25519/SchemaTooNew/runtime-policy 관련 stale
     limitation 3건도 정정.
  5. [claim 범위 정정] DoD-06: claim 자체는 유지되나 "framed ingress
     의 모든 타입 혼동을 domain_tag 가 막는다"로 확장해 읽으면 안
     된다 — Lease→Grant 위장은 nested-message decode 단계에서
     먼저 실패한다(domain_tag 이전). negative_tests 의
     `change_coordinator_set` 은 실제로는
     `change_coordinator_set_matches_reference_and_preserves_order`.
     stale limitation 3건 정정.
- 네 evidence 모두 원본 YAML(claim/status/limitations)은 고치지 않고
  append-only "이후 변경" 절로 정정했다. **네 건 다 이 정정 자체는
  아직 재검수를 거치지 않은 상태로 남아 있다** — 다음 라운드 대상.
- 검증: `cargo test --workspace` 293/0/0 (변화 없음). `cargo test -p
  gputeer-checkpoint --features chaos-hooks --test kill_chaos` 8/8.
  새 kill 테스트 단독 8회 연속 실행 8/8. 뮤테이션 2건(훅 위치 이동,
  DoD-04 doctest 필드명)으로 비공허성 확인 후 원복.
- 리포트: 이 이력 항목

---

## 2026-08-17 22:20 — gputeer selftest 에 127.0.0.1 루프백 TCP 왕복 추가, 종료 코드 결함 자체 발견·수정 (293 tests green, selftest 25개 검사)

- 계획: 사용자 지시 — "코덱스로 다음 작업들 진행해". 코덱스(read-only)에게
  전송 계층 설계를 맡겼다(§4c까지는 프로세스 안에서만 꿰어져 있었다 —
  CLAUDE.md 가 스스로 적어 둔 공백).
- 스트림: CLI · Crypto
- 수행:
  1. 코덱스 설계를 받아 `selftest.rs` §5 로 구현: `TcpListener::bind(("127.0.0.1", 0))`
     로 커널이 고른 포트를 얻고, 서버 스레드가 `read_frame` 으로 검증,
     클라이언트가 `write_frame` 으로 전송. 서버는 **개인키 없이 공개키만
     가진 InMemoryKeyring** 을 쓴다 — 검증자 역할을 흉내낸다.
  2. 정상 Grant 는 실제 소켓을 왕복해도 검증 통과(grant_id 에코 확인),
     위조 서명은 실제 소켓을 왕복해도 거부됨을 확인 — 양쪽 모두
     `set_read_timeout`/`set_write_timeout` 을 걸어, framed_ingress 모듈
     문서가 명시한 "타임아웃은 호출자 책임" 경고를 실제 코드로 재확인했다.
  3. [자체 발견, 코덱스 아님] `main.rs` 가 `report.contains("실패")` 로
     종료 코드를 정하고 있었는데, 요약 줄이 항상 `"통과 X · 실패 Y"` 를
     적기 때문에 **Y=0 이어도 그 문자열이 항상 존재해서 정상 실행도
     종료 코드 1이었다.** `cargo run -- selftest` 를 직접 실행해 exit
     code 1 을 재현하고서야 발견했다 — 이전까지는 사람이 눈으로 "실패 0"
     을 읽고 통과로 판단했을 뿐, 자동화가 실제로 그 신호를 쓴 적이 없었다.
     `SelftestReport { text, failed, blocked }` 로 리팩터해 사람이 읽는
     텍스트와 기계가 읽는 상태를 분리했다.
  4. `RULE.md` §7.1 ENVIRONMENT-BLOCKED != FAIL 을 selftest 에도 반영 —
     `Report::blocked()` 신설(루프백 바인드 자체가 막힌 환경을 실패와
     구분). 지금 실행 환경에서는 0건.
- 검증:
  - `cargo test --workspace` 293 passed / 0 failed (변화 없음 — 새 검사는
    `#[test]` 가 아니라 selftest 런타임 체크라 이 카운트에 안 잡힌다).
  - `gputeer selftest` 8회 연속 25/0/0, exit code 0.
  - 뮤테이션 테스트 2건으로 새 검사의 비공허성 증명: (a) 위조 코드를
    빼면 "위조 서명 거부" 검사가 정확히 실패로 뒤집힘(exit 1). (b) 서버
    keyring 에 엉뚱한 공개키를 넣으면 "정상 Grant 검증" 이 정확히
    실패로 뒤집힘(exit 1, InvalidSignature 로 보고됨). 두 뮤테이션 모두
    되돌린 뒤 25/0/0 재확인.
  - 종료 코드 수정 자체도 위 뮤테이션 실행에서 함께 검증됐다 — `failed>0`
    일 때 실제로 `ExitCode::FAILURE` 가 나오는지 그 실행들이 증명한다.
- 이것이 증명하지 않는 것: coordinator·agent·scheduler 는 여전히 없다.
  서버 쪽은 같은 프로세스 안 스레드 하나가 여는 소켓이다 — 별도 프로세스
  간, 하물며 별도 기계 간 통신은 실측하지 않았다.
- 리포트: 이 이력 항목

## 2026-08-17 21:30 — runtime-policy 독립 검수 반영 (293 tests green)

- 계획: 사용자 지시 — "코덱스로 ㄱ" (앞 세션에서 중단된 runtime-policy 검수 확인)
- 스트림: Runtime
- 수행: crates/runtime-policy 5개 파일을 독립 검수(코덱스, read-only)에 맡겼다.
  "정책 강제 계층이라기보다 일부 문자열 판정과 메모리상 상태 판정" 이라는
  총평과 함께 **중대 6건**을 찾았다.
- 발견과 조치:
  1. [중대] ArtifactPolicy::check() 실제 우회 5종을 검수자가 직접 만들었다 —
     전각 마침표(U+FF0E) 두 개로 ".." 위장, 키릴 동형 문자, Windows 예약
     장치명(NUL·CON 등), trailing dot/space. ASCII 전용 강제 + 예약어
     차단 + trailing dot/space 차단을 추가했다. 뮤테이션(ASCII 검사 제거)
     이 정확히 그 2건에서만 실패해 공허하지 않음을 확인했다.
  2. [중대] 빈 문자열/"." 접두사가 check("")·check(".") 를 통과시켰다.
     생성자에서 의미 없는 접두사를 걸러내고, 빈 요청은 어떤 접두사로도
     정당화되지 않게 했다.
  3. [중대] NetworkDecision 의 catch-all 매치가 NoEnforcementBackend 를
     조용히 "실행 허용" 으로 흘려보낼 수 있었다. permits_execution() 을
     추가해 Allowed 일 때만 true 를 반환하게 했다 — 이름이 아니라 타입이
     실수를 막게 한다.
  4. [중대] FenceWatermark 가 재시작 후 stale epoch 를 통과시키는 정확한
     시나리오를 검수자가 재현했다. 이미 문서화된 한계였지만 재현 테스트가
     없었다 — restart_resets_watermark_and_lets_stale_epoch_through 로
     "이 위험이 아직 존재한다" 를 통과가 곧 그 뜻이 되도록 고정했다.
     같은 epoch 재사용(<vs<=) 이 의도적임도 별도 테스트로 명시했다.
  5. [중대] VramEnforcement::guarantees_hard_limit() 을 부르는 곳이
     테스트 말고 없다 — 죽은 API 라는 지적. 모듈 문서에 "판정만 하고
     아무데도 연결 안 됐다" 를 명시했다(수정 아님, 정직한 표시).
  6. [중대] EnforcementClass 를 어떤 모듈도 실제로 안 썼다(각자 자기
     enum 을 씀) — 문서화되지 않은 사문화. YAGNI 원칙에 따라 **삭제**하고
     lib.rs 에 삭제 이유를 남겼다.
- gputeer selftest 4c 절도 permits_execution() 사용으로 갱신. 23개 검사.
- 검증: `cargo test --workspace` **293 passed / 0 failed**, 빌드 경고 0
  (3회 연속). runtime-policy 자체 29건(11건 신설).
- 리포트: 이 이력 항목

## 2026-08-17 20:10 — framed_ingress 독립 검수 반영 (283 tests green)

- 계획: 사용자 지시 — "코덱스로 ㄱㄱ"
- 스트림: Crypto
- 수행: framed_ingress.rs 를 독립 검수에 맡겼다(코덱스, read-only). 실제
  검증 우회는 못 찾았지만 문서·테스트의 과장과 진짜 결함 3건을 찾았다.
- 발견과 조치:
  1. [중대] AttemptReport·ArtifactRef 는 필드 1·2·4·90 의 와이어 타입이
     겹쳐 서명된 바이트가 양쪽으로 유효 디코드되고 canonical 도 같아질
     수 있다. domain_tag 로는 막히지만, 기존 테스트(Lease→Grant 위장)는
     사실 **nested-message decode 실패**로 막힌 것이라 이 방어를 실제로
     시험하지 못했다. distinct_types_with_colliding_wire_fields_are_
     rejected_by_domain_tag 를 신설해 진짜 시나리오로 domain_tag 방어를
     시험한다. 기존 테스트는 frame_type_mismatch_fails_at_nested_message_
     decode 로 이름을 바로잡았다.
  2. [중대] FrameTooLarge/UnknownFrameType 이 몸통을 안 읽어 다음
     read_frame 호출이 잔여 바이트를 헤더로 오인했다. UnknownFrameType 은
     길이가 이미 상한 이내로 확인된 뒤라 안전하게 비울 수 있어 그렇게
     고쳤다. FrameTooLarge 는 상한을 넘는 길이를 실제로 읽는 것 자체가
     DoS 이므로 비우지 않는다 — 대신 "이 오류 뒤 스트림은 못 쓴다" 를
     문서화하고 그 위험을 재현하는 테스트로 고정했다.
  3. [경미] write_frame 이 자기 상한을 검사하지 않고 usize->u32 를
     무검사 캐스팅했다 — 4GiB 넘는 body 는 길이 필드가 잘려 다른 프레임이
     됐다. Result 를 반환하도록 바꾸고 모든 호출부를 고쳤다.
  4. [문서] 모듈 문서가 "타입 위조 실패는 domain_tag 때문" 이라고
     뭉뚱그렸다 — 실제로는 nested-decode 실패와 domain_tag 실패 두
     경로가 있다는 것을 검수자가 지적해 정정했다.
  5. [알려진 한계, 미수정] claimed_len 이 상한 이내면 read_exact 에
     타임아웃이 없어 상대가 몸통을 안 보내면 무기한 블로킹한다.
     std::io::Read 에는 타임아웃이 없어 이 계층에서 못 막는다 —
     호출자가 소켓 타임아웃을 걸어야 한다. 전송 계층이 없어 아직
     아무도 그 책임을 안 진다.
- 검증: `cargo test --workspace` **283 passed / 0 failed**, 빌드 경고 0
  (4회 연속). framed_ingress 자체 16건(6건 신설).
- DoD-09 도 재발 경위를 반영해 재정정했다 (GC 경합 수정이 처음엔
  writer.rs 만 고쳐 8회 중 5회 재발했던 것).
- 리포트: 이 이력 항목 · CLAUDE.md 공백 목록에 DoS 한계 추가

## 2026-08-17 18:40 — 프레이밍·디스패치 · 정책 강제 계층 (278 tests green)

- 계획: 사용자 지시 — "코덱스로 이어서 작업"
- 스트림: Crypto · Runtime(신설)
- 수행:
  1. `crates/crypto/src/framed_ingress.rs` — [type][len][body] 프레이밍,
     헤더 타입으로 decode_and_verify<M> 디스패치. 헤더는 서명 대상이
     아니므로 위조 가능하다는 전제로 다룬다 — 타입을 속이면 실제 서명의
     domain_tag 가 달라 검증이 반드시 실패한다는 성질에 기댄다.
  2. `crates/runtime-policy` (신설, Runtime 스트림) — 서명된 정책 필드가
     Enforceable/Suppressible/Unenforceable 중 어디인지 판정하는 순수
     함수 계층(V-06). artifact_scope(문자열 검사, TOCTOU는 못 막음) ·
     network(OS 백엔드 없으면 항상 거부) · Lease.scope(소유 자원은
     watermark, 외부 API는 억제뿐) · VRAM/S1(CLAUDE.md §0.4 그대로 고정).
- ★ 코덱스에 이 두 작업을 설계로 맡겼는데 **둘 다 read-only 샌드박스라
  파일을 못 썼다** — 설계 논의만 돌아왔다. 설계 자체는 타당해서 그대로
  구현했다(코덱스 원안: [u32_be len][body] 프레이밍 · YAGNI로 tokio 배제 ·
  3분류 강제성 체계 · 8개 negative test 이름).
- 검증: `cargo test --workspace` **278 passed / 0 failed**, 빌드 경고 0
  뮤테이션(상한 검사 제거 · 경로 탈출 검사 제거 · stale epoch 검사 제거)
  모두 의도한 테스트에서만 실패.
- `gputeer selftest` 에 4b(프레이밍) · 4c(정책) 단계 추가. 통과 18 -> 22.
- 안 남은 것: OS 방화벽 호출 · 커널 경로 강제(openat2 등) · 실제 시스템
  조작 — runtime-policy 는 판정만 하고 아무것도 강제로 실행하지 않는다.
- 리포트: 이 이력 항목 · TODO_VISION V-06 갱신 · 소유권 표에 runtime-policy 등록

## 2026-08-17 16:20 — 검증 진입점 · 체크포인트 실패 경로 (248 tests green)

- 계획: 사용자 지시 — "코덱스로 ㄱ"
- 스트림: Crypto · Checkpoint
- 수행:
  1. **`crates/crypto/src/ingress.rs`** — raw bytes -> `Verified<M>` 단일 진입점.
     ★ 만든 방어(verify · keyring · replay 저장소)를 **처음으로 실제 연결**했다.
     Clock 주입 · decode 실패 분리 · LockTimeout 은 재시도 없이 거부(fail-open 금지).
  2. **체크포인트 실패 경로 4건** — `.publication-failed` 불변 마커로 배제,
     등록된 `.tmp` 보존, 동시 GC 경합 판별, DurabilityState 사이드카 기록.
  3. **스테일 evidence 탐지** — `verify_evidence.py` 가 negative test 의
     함수 정의가 아직 있는지 본다.
- 검증: `cargo test --workspace` **248 passed / 0 failed**, 빌드 경고 0 (3회 반복 동일)
- ★ 코덱스 설계를 그대로 받지 않은 것:
  초안은 "COMMITTED 마커 또는 현재 LATEST" 만 재개 후보로 삼았다. **과하다** —
  마커 직전에 kill 된 온전한 체크포인트를 버린다. 카오스 테스트가
  `--workspace` 부하에서 이것을 잡았다(단독 실행은 통과해 부하 의존이었다).
  완결 신호는 **매니페스트의 존재**로 남기고, 실패는 마커로만 배제한다.
- ★ 코덱스 자신의 테스트 2건이 실패했고 둘 다 진짜 발견이었다:
  실패 주입이 무효했다(Rust `File::open` 은 Windows 에서 `FILE_SHARE_DELETE` 를
  포함해 연다) · 동시 GC 가 Windows 에서 `Access Denied` 를 낸다.
- ★ 내가 만든 검사에서 세 번 틀렸다: 자기 주석과 매칭 · raw string 아님(`` 가
  백스페이스) · 소스 캐시가 임시 저장소를 스캔. 셋 다 부정 테스트가 잡았다.
- 안 고친 것: 네트워크 전송·coordinator 없음(진입점을 부르는 것이 없다) ·
  별도 프로세스 replay 경쟁 미측정 · LATEST 포인터 미사용(의도적)
- evidence: `DoD-09` · `DoD-10` 에 '이후 변경' 절 추가 (스테일 방지)
- 리포트: 이 이력 항목으로 갈음

## 2026-08-17 14:10 — 영속 replay 저장소 · 키 관리 · 두 구현의 계약 일치 (DoD-10, 229 tests green)

- 계획: 사용자 지시 — "같이 할 수 있는 코드 작업을 코덱스에 의뢰해서 진행"
- 스트림: Crypto
- 수행:
  1. **DurableReplayGuard** (§10 3단계) — 코덱스에 초안 의뢰, SQLite(rusqlite bundled) 채택.
     ★ 의존성을 **먼저 측정**했다 — 14.55초, SQLite 3.46.0. 추정으로 고르지 않았다.
  2. **PersistentKeyring** (§11 K0/K1) — K1 은 Windows DPAPI.
     Linux 는 조용히 K0 로 내려가지 않고 `UnsupportedPlatform` 으로 실패한다.
  3. **replay_contract.rs** — 두 구현이 같은 답을 내는지 검사하는 계약 테스트 9건.
- 검증: `cargo test --workspace` **229 passed / 0 failed**, 빌드 경고 0
- 코덱스 코드에서 내가 찾은 것:
  - `is_durable()` 이 **없었다.** 만들고 나서 `true` 를 하드코딩했더니
    영속성 제거 뮤테이션에도 `true` 였다 — **거짓말을 했다.**
    `Connection::path()` 에서 도출하도록 고쳤다.
  - "동시 프로세스 지원" 을 문서가 주장했는데 테스트가 없었다.
  - `let _ = now_sql;` 로 미사용 경고를 눌러 놨다 (`CLAUDE.md` §3 위반).
  - 회전 grace 종료 후 구 키를 `InvalidSignature`(위조)로 보고했다 —
    서명은 진짜다. `lookup_retired()` 로 구분한다.
  - 개인키 유출 테스트가 hex 한 가지만 봤다. Debug 는 10진수 배열로 찍는다.
- 검수(codex15)가 찾은 것 — **두 구현이 같은 계약을 만족하지 않았다** (4건).
  각 구현을 따로 시험하면 영원히 안 보인다. 계약 테스트로 닫았다.
  ★ 그 계약 테스트가 **내 기댓값의 오류도 잡았다.**
- 안 고친 것 (`DoD-10` limitations):
  소비 측 미착수(아무도 안 쓴다) · 실제 다중 프로세스 경쟁 미측정 ·
  torn write 미검증 · LockTimeout 재시도 정책 없음 · WAL 비교 근거 없음 ·
  ★ 검수자가 이 코드의 초안을 썼다 (완전한 독립 검수가 아니다)
- evidence: `docs/evidence/DoD-10_영속_replay_저장소.md` (schema v2)
- 리포트: 이 이력 항목으로 갈음

## 2026-08-17 12:40 — ★ 독립 검수 강제(schema v2) · replay 방어 4건 · 재개 선택 필터 (203 tests green)

- 계획: 사용자 지시 — "해결 안 된 4가지를 코덱스와 논의해" + 코덱스 쿼터 소진
- 스트림: Crypto · Checkpoint · 프로세스(공용)
- 수행:
  1. **재개 지점 필터** — 검수자가 `writer.rs:102-133` 에서 반례 4건 제시.
     `find_resume_point` 가 job/attempt 를 안 걸러 **남의 체크포인트에서 재개**할 수 있었다.
     `find_resume_point_for()` 신설 (job/attempt · 빈 매니페스트 · id≠디렉터리명 제외).
  2. **replay 방어 4건** — `require_replay_checked()` 가 replay 검사를 **안 한** 메시지를
     통과시켰다(`_ => true`). `ReplayStatus` 3상태로 갈랐다.
     `MAX_SHORTLIVED_TTL_MS`(15분) · 서명자별 quota · `MAX_GC_ADVANCE_MS`(5분) 추가.
  3. **evidence schema v2** (ADR-030 · `RULE.md` §7.3) — 미해결 4항목을 검수자와 논의해
     기계가 막을 것과 사람 책임을 갈랐다. `scripts/test_verify_evidence.py` 40건 신설.
- 검증:
  - `cargo test --workspace` **203 passed / 0 failed**, 빌드 경고 0
  - 뮤테이션 M1+M2+M3 -> 부정 테스트 4건 FAILED (공허하지 않음)
  - `MAX_GC_ADVANCE_MS` 를 1시간으로 잡았다가 **테스트가 잡아냈다** —
    최대 보존 시한(16분)보다 길면 아무것도 못 막는다. 5분으로 고쳤다.
  - 구현 직후 **2차 검수**에서 우회 7건(치명 1 · 중대 6)을 실제로 통과당했다. 전부 고쳤다.
  - 검수자가 내 부정 테스트의 **공허성 5건**도 지적했다. 전부 고쳤다.
- 안 고친 것 (`DoD-09` limitations):
  `LATEST` 포인터 미사용 · `write_checkpoint` 실패 후 잔여물 ·
  `startup_gc` 가 등록된 `.tmp` 삭제 · `DurabilityState` 미연결
- evidence: `docs/evidence/DoD-09_재개선택_필터.md` (**schema v2 최초 적용**)
- 리포트: 이 이력 항목과 ADR-030 · `docs/runbooks/ai-workflow.md` 갱신으로 갈음

## 2026-08-16 18:20 — ★ 독립 검수(Codex) 지적 7건 시정 (DoD-08 PASS, 167 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T4 착수 전 계약 정비
- 스트림: Protocol · Crypto · Checkpoint · QA
- 배경: 이 세션의 결정(ADR-028 · ADR-029 · canonical 규칙)은 **전부 내가 혼자 판단하고
  내가 만든 테스트로 검증한 것**이다. 그 테스트가 놓친 것은 그 테스트로 못 찾는다.
  `CLAUDE.md` §4 대로 Codex CLI 에 **적대적 검토**를 맡겼다 — 동의가 아니라 반박을 요청
- ★★ **두 구현이 똑같이 틀린 곳 3건 발견** — 벡터 대조로는 원리적으로 못 잡던 것들
  1. **규칙 i-2** 중첩 메시지가 서명 필드만 가지면 `0a00`(빈 중첩)으로 **출력**됐다.
     `is_default()` 검사가 field 90 제외보다 먼저 일어나 규칙 i 가 새어나갔다.
     Python 도 동일 (`manifest={}` → `0801` vs `{90:sig}` → `08011a00`)
  2. **규칙 c-2** map 엔트리 안의 규칙 b 가 규범에 없었다
  3. **규칙 i-3** 도출 해시 제외가 **재귀 적용**돼
     `DatasetRef.retention`(field 4)이 `manifest_hash`(field 4)로 오인돼
     **데이터셋 삭제 정책이 서명에서 지워졌다**
- ★★ **검증 도구 자체가 검증하지 않고 있었다.**
  `--verify` 가 저장된 hex 끼리 관계만 봤다 — **저장본이 구현과 어긋나도 통과**한다.
  나는 `DoD-01` 이래 "vector cross-checks: OK" 를 검증 근거로 인용해 왔다.
  -> `build_vectors()` 재실행 대조로 고쳤다. 관계 없는 벡터 변조 뮤테이션으로 실효성 확인.
  -> **`DoD-01` 에 후속 정정을 추가**했다 (교차검증 자체는 Rust 테스트가 했으므로 유효)
- ★★ **replay nonce 가 메시지와 결속되지 않았다** (보안 영향 최대)
  `verify(msg, ..., nonce, ...)` 로 **호출자가 nonce 를 골랐다.**
  => 서명은 통과하는데 replay 방어만 무력화. 매번 새 값이면 무한 재생 가능
  -> `Signable::replay_nonce()` — **서명된 메시지 필드**에서 가져온다
  -> 검수자 조언대로 **SQLite 착수 전에 계약부터 고쳤다** ("지금 SQLite 를 추가하면
     잘못된 외부 nonce 를 영속 기록하는 구현이 된다")
- checkpoint (첫 독립 검토):
  - **K-1** `write_once` 가 "content-addressed 이름" 을 전제했는데 실제는 `shard-0.bin`.
    같은 이름·다른 내용을 조용히 수락 → **writer 가 "확정했다"고 거짓 보고**
  - **K-2** `RetryPolicy{max_attempts:0}` 이 `atomic.rs:139` 에서 **panic**.
    ADR-026 의 "최종 실패는 명시적 오류" 계약 위반 — panic 은 오류가 아니다
- 부수: `ReplayGuard` → `Result<ReplayDecision, ReplayStoreError>`,
  `VerifyError` 로 프로토콜 결과와 로컬 장애 분리,
  §6.1 `manifest_hash` **공식 자체**를 처음으로 검증
- 검증: **cargo test --workspace = 167 passed / 0 failed** (143 → 167). 빌드 경고 0.
  ★ **기존 벡터 canonical 변경 0건** — 회귀 없이 규범 구멍만 메웠다.
  `SCHEMA_FINGERPRINT` 불변(`.proto` 미변경)
- ★ **시정하지 않은 지적을 숨기지 않았다** — `RevokeLeaseNotice` 반복 전송(미실측),
  증거 3종 서명자 ID(V-08), `ReplicaAck.fence_epoch`(V-07),
  §8 5·6단계 순서(규범 수정 여부 별도 판단), `write_once` 메모리 사용(미측정)
- evidence: `DoD-08_독립검수_시정.md`

## 2026-08-16 16:40 — T2 증거 메시지 시각 정책 (ADR-029 · DoD-07 PASS, 143 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T2
- 스트림: Protocol · Crypto
- ★ **결정: `signing.md` §9 표에 없던 6종은 "권한" 이 아니라 "증거" 다.
  시각으로 만료시키지 않는다.** `Lifetime::Evidence` 신설 (ADR-029)
  - 과거의 사실은 만료되지 않는다. `CheckpointManifest` 를 만료시키면
    **오래된 체크포인트에서 재개할 수 없고**, 그것은 시스템의 존재 이유를 부순다
  - 그러나 "그 시점의 사실" != "지금의 사실" -> `Perpetual` 과 구분한다.
    `Evidence` 는 **`observed_at` 노출을 타입으로 강제**한다.
    "언제인지 모르는 증거" 는 증거가 아니다
  - 신선도는 시각이 아니라 `fence_epoch` 이 판단한다.
    **시계는 어긋나지만 epoch 은 어긋나지 않는다**
- ★ **`ReplicaAck` 만 `fence_epoch` 이 없다** — 6종 중 유일하다.
  그런데 `REPLICATED(n)` 을 세는 근거이므로 **durability 주장의 뿌리**다.
  **복제본이 삭제되어도 ACK 는 영원히 유효하다.**
  `replica_ack_stays_valid_forever_even_if_replica_is_gone` 이 이 결함을 고정한다 —
  **통과한다는 것이 곧 "프로토콜이 막지 못한다" 는 뜻이다.** -> V-07
- 수행: `Signable` 2종 -> **10종**. `ExecutionGrant` · `RenewLeaseRequest` 를
  `ShortLived` 로 구현 — **§9 단수명 경로가 실메시지로 처음 검증**되었다
  (`DoD-04` 는 테스트 전용 타입뿐이었다)
  - `RenewLeaseRequest` 는 `expires_at` 필드가 없어 `issued_at + GRANT_TTL_MS` 로 도출.
    값을 지어내는 게 아니라 §9 가 정한 TTL 적용이며 근거를 코드에 적었다
  - `Signable` 을 `signable.rs` 로 분리 — `to_fields` 는 "어떤 필드",
    `signable` 은 "어떤 domain·수명". **틀렸을 때의 증상이 달라** 섞으면 리뷰가 흐려진다
- 검증: **cargo test --workspace = 143 passed / 0 failed** (129 -> 143). 빌드 경고 0
  ★ `the_three_lifetimes_actually_behave_differently` — 세 정책이 실제로 다른
  동작을 하는지 확인. 전부 같으면 `Lifetime` 구분이 의미가 없다
- ★ TTL==skew 문제: **TTL 을 늘려 미래 방향 skew 를 "살리는" 것은 하지 않았다.**
  단수명 수명을 늘리면 replay 창이 커진다 —
  보안 매개변수를 코드 경로 도달성 때문에 바꾸지 않는다.
  대신 두 테스트로 사실을 고정(기본 TTL 에서 가려짐 + TTL 1시간이면 발동)
- ★ `.proto` 를 건드리지 않았으므로 `schema_version` 상향·벡터 재생성 불필요.
  `SCHEMA_FINGERPRINT` 불변
- 신규 등록: **V-07**(`ReplicaAck.fence_epoch`) · **V-08**(증거 메시지 서명자 ID).
  둘 다 `schema_version` 상향이 필요해 **함께 처리하는 것이 싸다**
- evidence: `DoD-07_시각정책_실메시지.md` · ADR: `ADR-029`

## 2026-08-16 15:40 — ★ 서명 재사용 취약점 발견·시정 (ADR-028 · DoD-06 PASS, 129 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T1b
- 스트림: Protocol · QA(벡터)
- ★★ **스펙 취약점 발견 — `signing.md` §5 가 자기 MUST 를 어기고 있었다.**
  §5 는 "메시지마다 새 domain_tag 를 등록해야 한다(MUST)" 라고 적어 놓고
  membership(6종) · policy · quarantine(2종) 을 **공유**하게 두었다.
  공유하면 §5 의 방어("tag 가 달라 반드시 실패한다")가 사라지고
  canonical 차이만 남는데, 규칙 b(기본값 생략) 때문에 **공격자가 필드를 비우면
  서로 다른 메시지가 같은 바이트가 된다.**
- 실측 (참조 구현 전수 대조) — **충돌 5쌍**:
  ```
  AddMember        == RemoveMember       공통필드[1]    28바이트 동일
  ApproveDevice    == RemoveMember       공통필드[1]    28바이트 동일
  ApproveDevice    == RevokeDevice       공통필드[1,2]  56바이트 동일
  RemoveMember     == RevokeDevice       공통필드[1]    28바이트 동일
  QuarantineDevice == ReleaseQuarantine  공통필드[1]    동일
  ```
  → 소유자의 `RemoveMember` 서명이 `RevokeDevice` 로 통과한다.
  → ★ **격리 판정 m-of-n 서명이 격리 해제로 재사용된다**
- 조치: **ADR-028** — 메시지별 domain_tag 분리 (17 → 23종).
  ★ **canonical bytes 는 하나도 바뀌지 않았다** (tag 는 sig_input 에만 들어간다).
  `schema_version` 상향 불필요, `SCHEMA_FINGERPRINT` 불변, 기존 벡터 회귀 0건
- ★ 회귀 방지 테스트는 **canonical 이 아니라 `sig_input`** 을 본다 —
  ADR-028 이후에도 canonical 은 여전히 같기 때문이다.
  canonical 을 검사하면 "우연히 필드가 달라서 통과"하는 약한 보증만 얻는다.
  전제(canonical 동일)도 `assert_eq!` 로 고정해 전제가 바뀌면 근거를 재확인하게 했다
- 수행: T1b — grant · membership · policy · quarantine. domain 9 → **19/23**.
  벡터 28 → 36건
- 부수:
  - **`DERIVED_HASH_FIELDS` 신설** — 규칙 i 의 의도적 제외(`manifest_hash`)와
    실수 누락(`UNIMPLEMENTED`)을 분리. 뜻이 정반대인데 섞으면 구분할 수 없다
  - `ExecutionGrant` 는 규칙 i 가 **두 번** 적용되는 유일한 메시지
    (도출 해시 + 중첩 manifest/lease 서명) → Agent 의 독립 검증·재계산이 필수
  - `PlacementRationale` 을 서명 대상에 넣었다. 처음엔 "설명용" 이라며 미뤘는데
    가드가 "위조 가능" 으로 실패시켰다 — **약화하지 않고 구현했다.**
    서명 밖이면 Coordinator 가 배치 근거를 사후 조작할 수 있다
  - `every_impl_is_audited` 가 신규 impl 15종을 잡아 등록을 강제했다
- 검증: **cargo test --workspace = 129 passed / 0 failed**. 빌드 경고 0
- evidence: `DoD-06_domain_tag_충돌.md` · ADR: `ADR-028`

## 2026-08-16 14:30 — T1 서명 대상 확장 + 규칙 j 신설 (DoD-05 PASS, 117 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T1
- 스트림: Protocol · QA(벡터)
- 수행: **계약 우선** — 참조 구현 확장 -> 벡터 생성 -> Rust 구현 -> 대조.
  `artifact.proto` 8종 + `lease.proto` 3종의 `ToCanonicalFields`.
  domain 커버리지 **2 -> 9 / 17**. 벡터 20 -> 28건
- 검증: **cargo test --workspace = 117 passed / 0 failed** (105 -> 117). 빌드 경고 0
- ★ **규범 공백 3건**을 찾아 전부 규범 문서에 기록:
  1. **규칙 j 신설** — `int64 value_micro` 가 스키마 전체에서 **유일한 부호 있는 필드**인데
     하필 서명 대상 안에 있었다. 규칙 없이는 구현마다 갈린다(2의 보수 10B vs zigzag 2B).
     canonical 은 유효한 protobuf 인코딩의 부분집합이어야 하므로 **2의 보수** 채택.
     ★ **기존 벡터 20건이 하나도 바뀌지 않았다** — 회귀 없음
  2. **§5 의 4종(genesis·audit·release·invite)은 proto 메시지가 아예 없다.**
     membership·policy·quarantine 은 여러 메시지가 한 tag 를 공유 -> §5.1 기록
  3. **§9 시각 정책 표에 6종이 없고 `expires_at` 필드조차 없다.**
     ★ **추측으로 채우지 않았다**(`CLAUDE.md` §1) — `Signable` 을 구현하지 않고
     §9.1 에 결정할 질문 3개를 적어 T2 로 미뤘다.
     따라서 그 6종은 **아직 `verify()` 를 통과할 수 없다**
- ★ 중첩 서명의 귀결 고정: 규칙 i 재귀로 중첩 서명은 바깥 canonical 에 들어가지 않는다.
  **검증자는 중첩 서명 메시지를 독립 검증해야 한다(MUST)** — 안 하면 REPLICATED(n) 이 거짓이 된다.
  "중첩 전체가 무시되는" 결함과 구분하려고 반대 방향 테스트도 함께 넣었다
- evidence: `DoD-05_T1_서명대상_확장.md`

## 2026-08-16 13:30 — 스트림 소유권 위반 시정 + 실행계획 v2 (105 tests green)

- 계획: 이 커밋으로 `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` 착수
- 스트림: Protocol · Crypto · QA
- ★ **내가 `RULE.md` §4.1 을 어겼다.** `crates/protocol/src/signing.rs` 가
  `ed25519-dalek` 을 직접 썼는데 소유권 표는 Ed25519 를 Crypto 스트림 소유로 정한다.
  **테스트 100건이 전부 통과했고 아무도 알아채지 못했다.**
  테스트가 통과한다고 규칙을 고치지 않고 **코드를 규칙에 맞췄다** (§4.3 마지막 줄)
- 수행:
  - `crates/protocol`: `SignatureVerifier` trait 신설. **암호 라이브러리 의존 0**
    (blake3 만 예외 — canonical 의 일부)
  - `crates/crypto` 신설: `Ed25519Verifier` · `sign()` · `InMemoryKeyring`
    (이름이 "운영에 쓰면 안 됨"을 드러낸다 — §11 K0~K2 미구현)
  - `crates/protocol/tests/stream_ownership.rs` 5건 — 경계를 **코드로 강제**.
    뮤테이션(ed25519 재추가)으로 실효성 확인
  - **실행계획 v2 작성** — v1 범위가 소진됐는데 작업이 계획 밖에서 이어지고 있었다.
    T1~T6 확정. D-5 를 4건으로 갱신
  - `CLAUDE.md` §5 상태표를 디스크·빌드 실측으로 갱신
- 검증: **cargo test --workspace = 105 passed / 0 failed**. 빌드 경고 0. 문서 검사 통과
- 부수 정정: `UnknownSigner` 와 `InvalidSignature` 를 구분해 반환하도록 trait 계약에 명시.
  뭉뚱그리면 운영자가 "팀 멤버가 아니다"와 "위조되었다"를 구분할 수 없다
- 리포트: `docs/reports/2026-08-16_1330_프로토콜_계층_완주_자율세션.md` (세션 종합)

## 2026-08-16 12:40 — Ed25519 서명·검증 + Verified<M> (DoD-04 PASS, 100 tests green)

- 계획: 계획 밖 — `DoD-01`~`DoD-03` 이 모두 limitations 에 남긴 "Ed25519 미구현"
- 스트림: Protocol
- 수행: `crates/protocol/src/signing.rs` — `signing.md` §8 검증 순서 · §9 시각 정책 · §13.2.
  `tests/ed25519_verify.rs` 20건 + 독테스트 2건
- 검증: **cargo test --workspace = 100 passed / 0 failed** (78 -> 100). 빌드 경고 0건
  - §8 의 9단계가 **각각 실제로 발동**함을 확인. 발동하지 않는 단계는 없는 것과 같다
  - 보안 필드 변조 **9종 전부 거부** (DoD-03 이 서명에 넣은 것들이 실제로 지켜지는가)
  - ★ `Verified<M>` 우회 생성 차단을 `compile_fail` 독테스트로 검증.
    **필드를 pub 으로 바꾸는 뮤테이션**에서 FAILED, 원복 시 통과
- 설계 결정: `VerifyOutcome` 에 **`VALID` 를 두지 않았다.** 성공은 다른 타입이다.
  열거형에 VALID 를 두면 새 실패 값이 조용히 통과한다
- ★ 발견 2건 (둘 다 테스트가 처음에 실패해서 드러났다):
  1. `minimum_security_tier = 0` 변조가 **no-op** 이었다 (기준값이 이미 0).
     -> 모든 변조 케이스에 `assert_ne!(변조본, 원본)` 비공허성 단언 추가
  2. Grant 기본 TTL(60초) == skew 허용치(60초) 라서
     **미래 방향 skew 경로가 만료 검사에 가려져 도달 불가능**하다.
     안전성 문제는 아니나 "skew 검사가 동작한다"고 잘못 믿게 된다
  3. `compile_fail` 독테스트를 처음에 `tests/` 에 두었는데 **실행되지 않는다.**
     검증하지 않는 것을 주장하고 있었다 -> `src/` 로 이동
- 미구현 명시: **§8-8 replay 는 저장소 계층이 없어 실제 방어가 없다.**
  `NoReplayCheck` 라는 이름으로 사실을 드러내고 `require_replay_checked()` 가
  미검사 값의 부작용 경로 사용을 막는다. 조용히 빠뜨리지 않았다
- evidence: `DoD-04_ed25519_검증순서.md`

## 2026-08-16 11:30 — P0-08 스키마 진화 (PASS, 78 tests green)

- 계획: 계획 밖 — `DoD-02` 가 제기한 "CLAUDE.md §0.2 vs prost 기본 동작 충돌"
- 스트림: Protocol
- 수행: `tests/schema_evolution.rs` 6건(q1~q6) + `tests/schema_fingerprint.rs` 2건 +
  `proto/SCHEMA_FINGERPRINT.txt`(66 메시지 · 389 필드)
- 검증: **cargo test --workspace = 78 passed / 0 failed** (70 -> 78)
  - **prost 는 미지 필드를 조용히 버린다** — 118B -> 148B(주입) -> 118B(재인코딩).
    오류도 경고도 없다. canonical 에도 흔적이 없다
  - => 구버전은 본문만 보고는 새 필드의 존재를 알 수 없다. **유일한 신호는 schema_version**
  - `schema_version` 은 canonical(필드 1)과 sig_input **양쪽**에 묶여 강등은 서명을 깬다
  - => **§7.2 SCHEMA_TOO_NEW 는 구현 가능하다. 아키텍처 재검토 불필요**
- ★ 새 발견: **§7.3 "schema_version 증가 없는 필드 추가 금지" 를 프로토콜은 강제하지 못한다.**
  한 줄짜리 실수가 조용한 보안 우회가 된다 (구버전이 새 보안 제약을 무시한 채 통과)
  -> `SCHEMA_FINGERPRINT.txt` + 대조 테스트로 **빌드 시점 강제**.
  뮤테이션(`bool require_attestation = 63` 추가)으로 실효성 확인 —
  "추가된 줄: 63 bool require_attestation" 을 출력하며 실패
- 부수 발견: 버전 검사를 서명 검사보다 먼저 해야 하는 이유는 **안전성이 아니라 진단 정확성**.
  순서를 뒤집으면 "업그레이드 필요" 를 "서명 위조" 로 보고한다
- 결정: `signing.md` §7.2 변경 없음. §7.3 에 강제 장치 규범 추가. §8 근거 정정.
  **V-05 등록** — `oneof` 도입 시 지문 파서가 무력해진다.
  **V-06 등록** — 서명된 정책 필드의 강제 계층 (v0.1 필수)
- evidence: `P0-08_스키마_진화.md`

## 2026-08-16 10:40 — 서명 밖 필드 6건 제거 (DoD-03 PASS, 70 tests green)

- 계획: 계획 밖 — `DoD-02` 가 찾은 "위조 가능한 보안 필드 3건" 을 닫는 작업
- 스트림: Protocol · QA(벡터)
- 수행: **계약 우선 순서 준수** — 참조 구현 확장 -> 벡터 생성 -> Rust 구현 -> 대조.
  `reference_canonical.py` SCHEMAS 에 6종 추가 + JobManifest 를 전 필드로 + Lease 신설.
  벡터 12 -> 20건. `to_fields.rs` 에 6종 impl + JobManifest 10/11/12/54/55 · Lease 40 편입
- 검증: **cargo test --workspace = 70 passed / 0 failed** (55 -> 70).
  `v02_full_manifest` **896바이트가 Python 참조 구현과 바이트 일치**(BLAKE3 까지).
  ★ `every_field_in_full_manifest_affects_canonical` — 27개 필드를 하나씩 지워
  canonical 이 반드시 변하는지 확인. **27/27 전부 서명 반영**.
  벡터 대조만으로는 "두 구현이 사이좋게 같은 필드를 빠뜨린" 경우를 못 잡는다
- 발견: **`v02_full_manifest` 는 "모든 필드" 라고 적혀 있었지만 16개 부분집합이었다.**
  주장과 실제가 어긋난 만큼은 아무도 검증하지 않는다.
  -> `missing_from_full()` 로 벡터 생성 시점에 코드가 검사하게 했다
- 결정: `UNIMPLEMENTED_FIELDS` **비었다**. DoD-02 의 "위조 가능한 보안 필드 3건" 해소.
  단 **"서명에 들어갔다"와 "Agent 가 그 정책을 강제한다"는 다르다** — 강제 계층 미구현
- 리포트: `docs/reports/2026-08-16_0930_prost_연동계층_자율세션.md` (§12 갱신)
- evidence: `DoD-03_서명대상_완전성.md`

## 2026-08-16 09:30 — prost 연동 계층 (DoD-02 PASS, 55 tests green)

- 계획: 계획 밖 — `DoD-01` limitations 1·2번을 닫는 작업
- 스트림: Protocol
- 수행: `build.rs`(protoc-bin-vendored) · `src/to_fields.rs`(ToCanonicalFields 수동 구현) ·
  `tests/prost_canonical.rs` 11건 · `tests/field_number_audit.rs` 6건
- 검증: **cargo test --workspace = 55 passed / 0 failed** (38 -> 55).
  실제 prost 메시지가 Python 참조 구현과 바이트 일치(BLAKE3 까지).
  ★ map 500회 재구축에서 **prost 495종 vs canonical 1종** — 비공허성 단언 포함.
  field_number_audit 은 **뮤테이션 2종**(중복 경로·이름 불일치 경로)으로 실효성 확인
- 발견:
  1. `job.proto` 에 `import "lease.proto"` 누락 — **5개 proto 를 한 번도 컴파일한 적이 없었다**
  2. 내 negative test 주장이 틀렸다. map 없으면 prost==canonical(113B/113B).
     테스트를 느슨하게 고치지 않고 **근거를 다시 세웠다**
  3. ★ **서명에서 빠진 필드 6건, 그 중 3건이 보안 필드**
     (JobManifest 54 network · 55 artifact_scope · Lease 40 scope). 현재 **위조 가능**
  4. 계획서 §15.2 `bytes submitter_device_id` vs proto `string` 드리프트
- 결정: `signing.md` §3 규칙 변경 없음. §13.1 **근거 문구 정정**(규범 아님) +
  수동 구현 채택 명시 + `UNIMPLEMENTED_FIELDS` 선언 의무화.
  **P0-08 신규 등록** — `SCHEMA_TOO_NEW` × prost unknown-field.
  `CLAUDE.md` §0.2 와 prost 기본 동작이 정면 충돌한다.
  D-5 기준선 수정 요청 **3건 -> 4건**
- 리포트: `docs/reports/2026-08-16_0930_prost_연동계층_자율세션.md`
- evidence: `DoD-02_prost_연동_계층.md`

## 2026-08-16 08:10 — P0-03 카오스 테스트 완주 (38 tests green)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S3
- 스트림: Checkpoint
- 수행: `crates/checkpoint/src/writer.rs` (write_checkpoint / find_resume_point / startup_gc),
  `src/bin/ckpt_writer.rs` 카오스용 바이너리, `tests/kill_chaos.rs` 7건.
  별도 프로세스를 띄워 8개 고정 시점(40~700ms)에 실제로 kill
- 검증: **cargo test --workspace = 38 passed / 0 failed** (기존 31 + kill_chaos 7)
  불변식 4개 전부 통과. ★ 테스트가 공허하지 않음을 별도 검증 —
  8회 중 7회에서 PARTIAL 발생, `kill@560ms` 에서 **manifest.json.tmp**(매니페스트 쓰는 도중) 포착
- 결정: **P0-03 PASS.** local-first 원칙(§18.1) 재검토 안 함.
  단 COMMITTED durability 주장은 **HASH_VERIFIED 까지만 입증** — 복제 계층 미구현.
  P0-03b(복제) · P0-03c(전원 차단) 신규 등록
- 리포트: `docs/reports/2026-08-16_0700_P0스파이크_3건_자율세션.md` (§7 갱신)
- evidence: `P0-03_checkpoint_durability.md`

## 2026-08-16 07:00 — P0 스파이크 3건 (P0-01 PASS · P0-07 PASS · P0-06 FAIL-SCOPE)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md`
- 스트림: QA · Runtime
- 수행: x600 에 Rust 1.97.1 설치. SSH 전달을 base64 -> scp+`-File` 로 교체.
  P0-01(Windows S1+CUDA) · P0-07(추정 정확도) · P0-06(VRAM 강제) 실측
- 검증:
  **P0-01 PASS** — Restricted Token 에서 CUDA 완전 동작. Job Object 종료 시 VRAM 168->489->168 반환
  **P0-07 PASS** — sigma=0.021 (DoD 0.20). warmup 10->300 으로 오차 9.3%->1.2%
  **P0-06 FAIL-SCOPE** — 기준선 §10.3 의 "Job Object 는 VRAM 무관" 이 Windows 에서 **틀렸다**.
  5x5 스윕으로 `VRAM 최대 ~= RAM 제한 - 2000MiB` 확인
- 결정: ADR-005 유지 · ADR-007 유지 · **ADR-015 유지** · **ADR-027 신설**.
  기준선 수정 3건 승인 대기 (ADR-026 · ADR-027 · §12.3 warmup)
- ★ 오판 5건 정정 기록. 특히 "빈 출력 -> CUDA 실패" 오판을 잡지 못했으면
  ADR-005 를 뒤집고 Windows S1 을 로드맵에서 제거했을 것
- 리포트: `docs/reports/2026-08-16_0700_P0스파이크_3건_자율세션.md`
- evidence: `P0-01` · `P0-07` · `P0-06`

## 2026-08-16 06:20 — Rust 구현 착수: protocol + checkpoint (31 tests green)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S2 · S4
- 스트림: Protocol · Checkpoint
- 수행: Rust 1.97.1 설치(로컬). Cargo workspace + 크레이트 2종 구현.
  `crates/protocol` — canonical_encode(규칙 a~i) · sig_input · Domain 17종 · merkle · constants
  `crates/checkpoint` — ADR-026 write_once/replace_with_retry · sync_dir · 상태전이 · ReplicaSet
- 검증: **cargo test --workspace = 31 passed / 0 failed**
  canonical 15건이 Python 참조 구현과 **바이트 단위 일치**(BLAKE3 다이제스트까지).
  checkpoint 16건 중 `adr026_write_once_succeeds_while_readers_hold_files_open` 이
  P0-03a 에서 313/3000 실패하던 조건에서 **500/500 성공**
- 결정: signing.md §3 규칙 변경 없음. 다음 공백은 **prost 연동 계층**
- 리포트: `docs/reports/2026-08-16_0620_Rust구현_protocol_checkpoint.md`
- evidence: `DoD-01_canonical_encode_교차검증.md`

## 2026-08-16 05:30 — 원격 GPU 기계(x600) 실측, BLOCKED 7건 중 4건 해제

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S0 (재실측)
- 스트림: —
- 수행: `~/.ssh/config` 의 x600 · runpod-gpu 두 호스트 조사.
  x600 = **RTX 4070 SUPER 12GB · driver 595.79 · CUDA 13.2 · Windows 11 · 가상화 ON**.
  runpod-gpu 는 Connection refused (인스턴스 종료)
- 검증: `nvidia-smi` + `Win32_VideoController` 교차 확인. `wsl --list` 로 배포판 0개 확인.
  **P0-01·02·07 해제 · P0-06 부분 해제 · P0-04/04b/05 BLOCKED 유지**
- 결정: x600 을 GPU 검증 기계로 지정. 작업 디스크 **F:** (C: 는 8.1GB 뿐).
  실행계획 D-2 해소, **D-1(Rust)이 유일한 착수 차단 요인**으로 남음
- 리포트: (S0 재실측이므로 `docs/evidence/ENV-02_원격_GPU_기계_실측.md` 로 갈음)

## 2026-08-15 14:10 — P0-03a Windows 파일시스템 원자성 조사

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S1
- 스트림: QA
- 수행: `tools/probes/windows_fs_atomicity.py` 작성(프로브 8종).
  기준선 §18.2 의 `rename -> fsync(dir)` 절차가 Windows/NTFS 에서 성립하는지 실측
- 검증: **FAIL 2 · PARTIAL 1 · PASS 5.**
  Windows `MoveFileEx` 가 열린 파일을 대체하지 못함(313/3000). POSIX 시맨틱 API 로도 실패(87/1000).
  디렉터리 fsync 는 쓰기 권한을 주면 가능. 부분 내용 관측은 전 시나리오 0건
- 결정: **ADR-026 신설** — 데이터 파일은 write-once 로 rename-over-existing 회피,
  포인터 파일만 재시도 replace, `sync_dir` 플랫폼별 정의
- 리포트: `docs/reports/2026-08-15_1410_P0-03a_파일시스템_조사.md`

## 2026-08-15 13:45 — 환경 실측 (ENV-01)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S0
- 스트림: —
- 수행: 툴체인·GPU·파일시스템 실측. `Get-Command` + 표준경로 + WMI 3중 교차 확인
- 검증: **NVIDIA GPU 없음(Intel Iris Xe) · Rust 툴체인 없음.**
  P0 스파이크 8종 중 **7종이 ENVIRONMENT-BLOCKED**. P0-03 만 실행 가능
- 리포트: (S0/S1 을 묶어 위 리포트에 기록)

## 2026-08-15 — 저장소 골격 수립

- 계획: (기준선 통합 직후. 실행계획서 이전)
- 스트림: —
- 수행: `RULE.md`·`CLAUDE.md`·`docs/` 10개 폴더·템플릿 5종·`scripts/verify_evidence.py` 생성.
  `proto/`·`docs/protocol/`·`tools/`·`tests/vectors/` 를 저장소 안으로 이동
- 검증: `verify_evidence.py` 가 템플릿의 자리표시자를 정상 검출(commit·raw_output 2건).
  `reference_canonical.py --self-test` 12/12 통과
- 리포트: (골격 수립이라 리포트 생략. 다음 세션부터 필수)
