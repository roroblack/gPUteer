---
schema_version: 2
id: DoD-57
claim: "`crates/runtime-linux` 가 cgroup v2 로 자원 상한을 실제로 강제하고, Agent 의 Linux 실행 경로가 그것을 통해 프로세스를 띄운다. x600 WSL2 에서 실측으로 확인했다 — 상한을 넘는 할당은 죽고, 상한 안의 작업은 살아남고, `cgroup.kill` 이 손자까지 정리하고, 붙잡힌 `wait()` 을 다른 스레드에서 풀 수 있다. 상한을 걸 수 없으면 프로세스를 띄우지 않는다(typed error). ★ 그러나 이것은 **협조하는 작업에 대한 상한이지 적대적 코드에 대한 격리가 아니다** — 자식이 자기 pid 를 상위 `cgroup.procs` 에 써서 실제로 빠져나가 32MiB 상한 밖에서 90MB 를 잡는 것을 테스트로 확인했다"
status: PASS
commit: 2eb3637a5eb73d91fb6a03fb69b82f2dfc222f63

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — crates/runtime-linux 신설, crates/agent 의 Linux platform::execute 구현, x600 WSL2 원격 실측"
executor_model: "claude-opus-5"
executed_at: "2026-08-30T16:40:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 3라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "파일:줄 위치로 지목된 지점 — `crates/agent/src/exec.rs:386`·`:402`(Agent Linux 경로가 UnsupportedPlatform 반환), `crates/runtime-linux/src/lib.rs:294`(pre_exec 투입), `:355`·`:363`(루트 자동 폴백), `:266`(memory.swap.max 부재 추정), `:297`(async-signal-safe 단정), `:214`·`:218`(Drop 정리 실패 무시), `:286`(stdout 열기 실패 시 cgroup 미정리), `:435`(기존 이름 정리의 오류 무시), `:240`~`:258`·`:527`~`:542`(빈 cgroup 회수 불가), `:569`~`:576`(마지막 이름만 검사), `crates/agent/src/lib.rs:798`·`exec.rs:574`~`:580`(성분 결합 충돌), `crates/runtime-linux/tests/cgroup_enforcement.rs:16`(테스트가 Agent 경로를 안 씀), `:220`·`:233`·`:244`~`:249`·`:261`~`:278`(탈출 테스트의 오통과 경로), `:767`(마지막 이름 틀린 경우만 검사). 3라운드에 걸쳐 진짜 결함을 연속으로 찾았다. 1라운드: (a) 새 크레이트가 독립 API 와 테스트만 있고 Agent 의 Linux 경로는 여전히 `UnsupportedPlatform` 이라 '운영 Job 에 상한을 강제한다' 는 주장이 코드로 뒷받침되지 않음, (b) 자식이 상위 `cgroup.procs` 에 자기를 써서 빠져나갈 수 있음, (c) `resolve_parent()` 의 v2 루트 자동 폴백이 systemd unit·slice·컨테이너 상한 밖으로 자식을 내보냄, (d) `memory.swap.max` 파일 부재를 '스왑 없음' 으로 추정, (e) `pre_exec` 안 `std::fs::write` 를 async-signal-safe 라고 단정. 2라운드: (f) 이름을 해시하면서 두 성분을 문자열로 먼저 합쳐 `(\"g-a\",\"b\")` 와 `(\"g\",\"a-b\")` 가 충돌, (g) 1라운드 수정('기존 cgroup 을 죽이지 않고 거부')이 새 문제를 만들어 비정상 종료 후 남은 빈 cgroup 이 같은 attempt 의 재시도를 영영 막음, (h) 탈출 테스트가 `;` 로만 이어져 관측 실패 시에도 통과, (i) `Explicit` 부모 검사가 `system.slice` 를 그대로 통과. 3라운드: (j) `gputeer-` 접두사를 마지막 이름에만 적용해 `system.slice/gputeer-agent.service` 가 통과. 전부 코드로 고쳤고, 마지막 라운드는 남은 한계(bind mount, systemd 위임 slice 미지원)를 문서에 정직하게 적었는지까지 확인했다"
review_artifact: "docs/evidence/_raw/DoD-57_review_rounds.txt"

decision: "cgroup 부모를 자동으로 고르지 않고 `CgroupParent{Current, Explicit, RootBypassingAncestorLimits}` 로 호출부에 넘긴다 — 자동 폴백은 운영자가 상위에 걸어 둔 CPU·메모리·PID 상한 밖으로 자식을 내보내는데, 그건 Agent 가 자기 판단으로 할 일이 아니다. `Explicit` 는 금지 목록이 아니라 **허용 조건**(루트의 직속 자식 + 이름이 `gputeer-` 로 시작)으로 막는다 — 금지 목록은 언제나 빠뜨린 항목이 생기지만 '우리 이름으로 만든 것만' 은 빠뜨릴 자리가 없다. 그 대가로 systemd 위임 slice 를 지원하지 않게 됐고 그것을 문서에 적었다. cgroup 이름은 다듬지 않고 성분별 길이 접두사와 함께 해시한다 — 치환·절단·성분 결합이 각각 충돌을 만들었고, 충돌하면 남의 작업을 죽인다. 기존 cgroup 은 **비어 있음이 증명될 때만**(`cgroup.procs` 비었고 `cgroup.events` 가 `populated 0`) 회수한다 — 무조건 죽이면 남의 작업을 끝내고, 무조건 거부하면 비정상 종료 후 재시도가 영영 막힌다."
raw_output_artifact: "docs/evidence/_raw/DoD-57_cgroup_enforcement_x600_2026-08-30.txt"
raw_output_digest: "sha256:1a829492e6a7dad72799099495df4c43d27adca64a902dcbe4b8be34010951a1"
raw_output_bytes: 35538

binary_digests:
  toolchain: "x600 WSL2 의 cargo 1.89.0 (c24e10642 2025-06-23) — /root/.cargo. 개발 기계 교차 타입 검사는 cargo 1.97.1 + --target x86_64-unknown-linux-gnu"
protocol_versions:
  schema_version: "proto 변경 없음 — 이 크레이트는 서명 대상 메시지를 만들지도 소비하지도 않는다"
  canonical_spec: "canonical/domain_tag 무관 — 서명·저장·상태 전이를 하지 않는다"
platform: "실측: x600 의 WSL2(커널 6.18.33.2-microsoft-standard-WSL2, Ubuntu, systemd 259, cgroup v2 단일 계층). 교차 타입 검사: Windows 11 개발 기계"
hardware: "x600 — 상한 강제는 시스템 RAM 만 다루므로 GPU 는 이 조각과 무관하다. 빌드·실행은 F: 드라이브(954G)에서 했다: WSL 의 /tmp 는 ext4 VHDX 이고 그 파일이 C:(여유 18G)에 있어 거기서 빌드하면 C: 를 먹는다"
network_profile: "x600 으로의 SSH/SCP 만 사용. 이 크레이트 자체는 네트워크를 쓰지 않으며, 자식 프로세스의 네트워크를 차단하지도 않는다"
command: |
  # x600 WSL2 — 작업 드라이브는 F: 다
  ssh x600 "wsl -e bash /mnt/f/gputeer-work/run3.sh"
  #   내부: cd /mnt/f/gputeer-work/build/gputeer
  #         cargo test -p gputeer-runtime-linux
  #         cargo test --workspace --exclude gputeer-runtime-windows --no-fail-fast
  # 뮤테이션
  ssh x600 "wsl -e bash /mnt/f/gputeer-work/mut4.sh"
  # 개발 기계 교차 타입 검사
  cargo check -p gputeer-runtime-linux --all-targets --target x86_64-unknown-linux-gnu
raw_output: |
  ★ 2026-08-30 독립 검수 5라운드가 이전 raw 를 반려했다 — 사람이 편집한
    요약이라 "running 5 tests" 아래 6개가 나열되는 불일치가 있었다.
    지금은 실제 명령 출력을 그대로 붙였다(35,538바이트).
    검수 원문도 판정 블록을 편집 없이 잘라 세 파일로 보존했다.

  (docs/evidence/_raw/DoD-57_cgroup_enforcement_x600_2026-08-30.txt 전문 참조)

  통합 7/7 · 단위 6/6 · Linux 워크스페이스 693 passed / 실패 suite 0

  탈출 실측: before 에 gputeer-escape 있음 -> after == "0::/" -> 90,655,836 바이트 할당
  ★ 뮤테이션 M1/M2/M3 의 실패 출력은 **이 raw 파일에 없다**(2026-08-30
    독립 검수 6라운드 지적). 뮤테이션은 기준선과 다른 실행이었고 그
    출력을 저장하지 않았다 — 아래는 그때 관측한 결과의 서술이지
    원문이 아니다.
    M1(swap 상한 제거): 2건 실패   M2(pre_exec 투입 제거): 4건 실패
    M3(이름 접두사 제거): system_slices_are_refused_by_name 실패

artifacts:
  - crates/runtime-linux/src/lib.rs
  - crates/runtime-linux/tests/cgroup_enforcement.rs
  - crates/runtime-linux/Cargo.toml
  - crates/agent/src/exec.rs
  - crates/agent/Cargo.toml
  - docs/evidence/_raw/DoD-57_cgroup_enforcement_x600_2026-08-30.txt
  - docs/evidence/_raw/DoD-57_review_rounds.txt
  - docs/evidence/_raw/DoD-57_review_round1_verbatim.txt
  - docs/evidence/_raw/DoD-57_review_round2_verbatim.txt
  - docs/evidence/_raw/DoD-57_review_round3_verbatim.txt
negative_tests:
  - "exceeding_the_limit_actually_kills_the_child: 32MiB 상한에 64MiB 를 잡으려 하면 자식이 0 이 아닌 코드로 끝남을 확인한다. ★ 이 검사가 **초안을 실제로 반증했다** — `memory.max` 만 걸었을 때 자식이 90MB 를 잡고 **정상 종료했다**. cgroup 은 상한 초과 시 먼저 회수를 시도하고 스왑으로 밀어내므로, `memory.swap.max=0` 을 같이 걸어야 실제로 끝난다"
  - "a_workload_within_the_limit_is_untouched: 상한 안의 작업이 살아남음을 확인한다 — 위 검사만 있으면 '전부 죽인다' 로도 통과한다"
  - "killing_the_cgroup_kills_grandchildren_too: `sh -c 'sleep & sleep'` 로 손자를 만든 뒤 `cgroup.kill` 이 트리 전체를 비움을 프로세스 목록으로 확인한다. 하나만 죽이면 손자가 GPU 를 쥔 채 남는다"
  - "a_blocked_wait_can_be_released_from_another_thread: `wait()` 에 붙잡힌 상태에서 다른 스레드의 정지 손잡이가 실제로 풀어냄을 확인한다(§0.1)"
  - "a_zero_limit_is_refused / a_name_with_a_separator_is_refused / error_kinds_are_distinguishable: 상한 0, 경로 탈출 이름, 오류 종류 혼동을 각각 거부·구분한다"
  - "system_slices_are_refused_by_name: `/sys/fs/cgroup/system.slice` 등을 `Explicit` 부모로 받지 않음을 확인한다"
  - "a_correct_name_under_a_system_ancestor_is_still_refused: `system.slice/gputeer-agent.service` 처럼 **마지막 이름만 맞는** 경로도 거부함을 확인한다 — 이 검사가 없을 때 3라운드 검수가 정확히 이 우회를 지적했다"
  - "the_parent_the_agent_actually_uses_is_exercised: Agent 가 실제로 쓰는 `Explicit`·`Current` 경로를 돌린다 — 그전까지 모든 테스트가 `RootBypassingAncestorLimits` 만 써서 Agent 의 실제 경로가 한 번도 실행된 적이 없었다"
  - "a_determined_child_can_still_escape_the_cgroup: ★ **탈출이 성공하기를 기대하는 테스트다.** 자식이 자기 pid 를 `/sys/fs/cgroup/cgroup.procs` 에 쓴 뒤 상한을 넘는 할당에 성공함을, 탈출 전후의 `/proc/self/cgroup`(정확히 `0::/`)과 실제 할당 크기(90,655,836 바이트)로 확인한다. 구멍이 닫히면 이 테스트가 실패하고, 그때 모듈 문서의 '적대적인 코드는 못 막는다' 도 같이 고치게 된다"
limitations:
  - "★ **적대적 코드를 막지 못한다.** 자식은 부모와 같은 권한으로 돌고, `exec` 뒤 자기 pid 를 상위 `cgroup.procs` 에 써서 나갈 수 있다 — 실측으로 확인했다. 닫으려면 cgroup namespace(`CLONE_NEWCGROUP`)와 권한 강등이 필요하고 둘 다 이 조각보다 크다. `runtime-windows` 가 'S1 은 호스트를 지키지 못한다' 고 적어 둔 것과 같은 자리다(§0.4)"
  - "memory 만 건다 — CPU·PID·io 상한, 네트워크 차단, 파일시스템 격리는 없다"
  - "VRAM 은 제한하지 않는다 — cgroup 은 시스템 RAM 만 본다. 그래서 GPU 할당 기본값이 Exclusive 다"
  - "위임 확인을 하지 못한다 — `CgroupParent::Explicit` 는 '루트의 직속 자식이고 이름이 `gputeer-` 로 시작' 만 요구한다. 운영자가 실제로 위임했는지 커널에 물을 방법이 없다. 명백히 위험한 값을 막을 뿐 적대적인 운영자를 막지 못한다"
  - "bind mount 를 막지 못한다 — cgroup 루트를 루트 아래 다른 이름에 bind mount 하면 겉보기 경로는 직속 자식인데 실제 대상은 루트일 수 있다. `canonicalize()` 로도 안 잡힌다"
  - "systemd 가 관리하는 위임 slice 를 지원하지 않는다 — 깊이 1 제한의 대가다. 운영자가 `/sys/fs/cgroup/gputeer-*` 를 직접 만들어야 한다"
  - "`pre_exec` 안의 `std::fs::write` 가 async-signal-safe 하다고 **보장되지 않는다** — 실제 하는 일은 open/write/close 뿐이지만 `std` 가 그 경로만 쓴다는 계약은 없다. 없애려면 raw syscall 이나 libc 의존이 필요하다"
  - "WSL2 한 대에서만 실측했다 — 네이티브 Linux, 컨테이너 안, systemd 위임 환경에서는 돌린 적이 없다"
  - "★ **`raw_output_artifact` 는 '실제 출력 그대로' 가 아니라 '필터링·수동 결합한 실제 출력 발췌' 다**(2026-08-30 독립 검수 6라운드 정정). 여러 실행을 사람이 합쳤고 헤더도 수작업이다. 특히 **뮤테이션 M1/M2/M3 의 실패 출력은 들어 있지 않다** — 서술로만 남았다"
  - "★ `raw_output_artifact` 는 실제 출력이지만 **워크스페이스 전체 출력은 `test result:`/`running`/`error` 줄만 남긴 것**이다(수천 줄이라 전량 보존하지 않았다). 남긴 줄 자체는 편집하지 않았다. 검수 원문도 판정 블록만 잘라 보존했고 도구 호출 로그(수십만 바이트)는 저장소에 없다"
  - "Windows 실행 경로는 이 조각에서 바뀌지 않았다 — 한 플랫폼 통과를 다른 플랫폼 통과로 세지 않는다(§4)"
---

# DoD-57 — Linux cgroup v2 자원 상한 강제

## 왜 이 조각이 생겼는가

`crates/agent/src/exec.rs` 의 non-Windows 경로는 이렇게 말하고 실행을
거부하고 있었다.

```text
이 플랫폼에는 자원 상한 강제가 연결돼 있지 않다(Linux cgroup 미착수)
— 상한 없이 실행하지 않는다.
```

거부 자체는 옳았다(§0.4). 없던 것은 **수단**이다. x600 의 WSL 이
응답하게 되면서 실측이 가능해져 그 수단을 만들었다.

## 순서가 핵심이다

```text
1  하위 cgroup 생성
2  memory.max + memory.swap.max
3  fork 후 exec **전에** 자기를 cgroup 에 넣는다 (pre_exec)
4  exec
```

3–4 순서를 바꾸면 남의 코드가 상한 밖에서 먼저 돈다 —
`runtime-windows` 가 `CREATE_SUSPENDED` 를 쓰는 것과 정확히 같은 이유다.

## 실측이 초안을 두 번 반증했다

### memory.max 하나만으로는 상한이 아니다

32MiB 상한에 90MB 를 할당했는데 자식이 **정상 종료했다.** cgroup 은
상한을 넘으면 먼저 회수를 시도하고, 스왑이 있으면 페이지를 거기로
밀어낸다 — 안 죽고 느려질 뿐이다. 남의 PC 에서 도는 작업이 스왑을
무한정 먹으면 소유자 기계가 기어간다. 그건 상한이 아니다.

`memory.swap.max = 0` 을 같이 걸어야 실제로 OOM 으로 끝난다.

★ 그리고 파일 부재를 "스왑 없음" 으로 **추정하지 않는다.** 파일이
없는 것은 스왑 계정이 꺼졌다는 뜻일 뿐이다 — `/proc/swaps` 를 실제로
읽고, 스왑이 있는데 상한을 못 걸면 실행을 거부한다.

### 부모 cgroup 을 "내 cgroup" 으로 고정하면 안 된다

첫 실행이 설계대로 실행을 거부했다 — `/proc/self/cgroup` 이
`/init.scope` 였는데, cgroup v2 의 **"내부 프로세스 금지"** 규칙 때문에
프로세스를 직접 담은 cgroup 은 컨트롤러를 하위에 위임할 수 없다.

자동으로 루트까지 내려가는 폴백을 넣었다가 검수가 반려했다 — 그러면
자식이 운영자가 상위에 걸어 둔 CPU·메모리·PID 상한 **밖**으로 나간다.
선택을 호출부에 넘기고 기본값을 거부로 뒀다.

## 못 막는 것을 실제로 재 봤다

검수가 "협조하는 작업에는 상한이지만 적대적인 코드에는 아니다" 를
짚었다. 모듈 문서에 그렇게 적었는데, **적어 두는 것만으로는 그게
사실인지 알 수 없다.**

§0.4 는 강제할 수 없는 것을 보장으로 선언하지 말라고 한다. 그러려면
무엇을 강제 못 하는지 정확히 알아야 하고, 아는 방법은 해 보는 것뿐이다.

```text
자식: 자기 pid 를 /sys/fs/cgroup/cgroup.procs 에 쓴 뒤 64MB 할당
상한: 32MiB
결과: 탈출 후 cgroup 이 정확히 "0::/", 실제 할당 90,655,836 바이트
```

**탈출이 성공하기를 기대하는 테스트**를 남겼다. 나중에 누가 cgroup
namespace 로 구멍을 닫으면 그 테스트가 실패하면서 모듈 문서도 같이
고치라고 알려준다.

## 이 실험이 증명하지 않는 것

```text
적대적 코드 격리     자식이 상위 cgroup.procs 로 나가는 것을 실측으로
                     확인했다. 이 조각은 그것을 막지 못한다
CPU·PID·io 상한      memory 만 건다
네트워크·파일시스템   차단하지 않는다
VRAM                 cgroup 은 시스템 RAM 만 본다
위임 여부            커널에 물을 방법이 없어 이름 규칙으로 대신한다 —
                     실수를 막는 것이지 적대적 운영자를 막는 것이 아니다
bind mount           겉보기 직속 자식이 실제로는 루트일 수 있다
네이티브 Linux       WSL2 한 대에서만 돌렸다
Windows 경로         이 조각에서 안 바뀌었다(§4 — 한 플랫폼 통과를
                     다른 플랫폼 통과로 세지 않는다)
```

## 남은 것

이 조각은 memory 만 건다. CPU·PID·io 상한, 네트워크 차단, 파일시스템
격리, VRAM 제한은 없다. 위임 확인도 못 한다 — 커널에 물을 방법이 없어
"루트의 직속 자식이고 이름이 `gputeer-` 로 시작" 만 요구하며, 그것은
실수를 막는 것이지 적대적인 운영자를 막는 것이 아니다.
