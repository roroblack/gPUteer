# 2026-08-21_1208_scheduler_inventory_v1

- 조사 대상: `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md`의
  조각 3 `다중 Agent inventory/session`
- 선행 완료: DoD-41(조각 1), DoD-42(조각 2a), DoD-43(조각 2b-1)
- 조사 성격: 설계 조사. 이 문서 외 파일은 변경하지 않는다.
- 상태: **오늘 착수 후보 있음 — 조각 3a, durable Agent inventory 저장소 kernel**

## 결론

조각 3 원안 전체를 하루에 구현하는 것은 불가능하다. 원안은 Agent별 key/identity,
동시 session, heartbeat/capability producer, freshness, per-Agent active-session owner,
live `PoolSnapshot`을 한 단계로 묶고 production 800~1,300줄 + 테스트
700~1,150줄, 4~6일로 추정한다
(`scheduler_전체_설계_v1.md:180-192`). 특히 실제 동시 session과 owner 선정은
DoD-40이 이미 판정한 Coordinator 아키텍처 변경이다.

그러나 **inventory의 로컬 저장과 결정적 snapshot 투영 자체는 그 변경을 기다릴
필요가 없다.** 마스터 플랜 §5.3은 heartbeat/telemetry 원본을 Raft log가 아니라
각 Coordinator의 로컬 SQLite + 인메모리에 유지하라고 정한다
(`../gputeer_master_plan_FINAL.md:542-560`). 현재 `crates/scheduler`도 이미
외부에서 고정된 `PoolSnapshot`을 받는 순수 소비자다
(`crates/scheduler/src/model.rs:92-137`). 따라서 오늘의 정직한 최소 조각은
다음과 같다.

> 서명 검증을 마친 것으로 간주한 정규화 inventory를 Agent별로 원자·영속 저장하고,
> 두 Agent 이상도 담을 수 있는 결정적 `PoolSnapshot`으로 읽는 **순수 저장소 API**.
> 실제 wire heartbeat, 동시 socket 처리, session owner 선정은 하지 않는다.

이 조각을 완료해도 Coordinator 제품 경로는 여전히 단일 Agent다. 이를 “다중 Agent
지원” 또는 조각 3 완료라고 부르지 않는다.

## 로드맵 조각 3 원문과 의존성

로드맵에는 조각 3만을 풀어 쓴 별도 절은 없고, 의존성 도식과 단계 표의 한 행이
전체 정의다.

- 의존성: 조각 1에서 조각 2와 3이 갈라져 병행 가능하고, 조각 4는 둘 다 필요하다
  (`scheduler_전체_설계_v1.md:155-178`).
- 원안: Agent별 key/identity, concurrent session, heartbeat/capability, freshness,
  per-Agent session owner, `PoolSnapshot` (`scheduler_전체_설계_v1.md:180-192`).
- 이유: scheduler 후보가 실제로 2대 이상 존재하려면 필요하며 DoD-40 이월분을
  포함한다.
- 금지된 지름길: 고정 `agent_device_id`를 반복해 후보 목록처럼 보이게 하면 안 된다
  (`scheduler_전체_설계_v1.md:245-255`).

따라서 inventory 저장소는 원안의 진짜 하위 조각이지만 concurrent session과
active-session owner를 빼면 원안 전체는 아니다.

## 확인한 현재 구현

### Coordinator는 여전히 단일 Agent 계약이다

- `CoordinatorConfig`는 `agent_verifying_key`와 `agent_device_id`를 각각 하나만
  가진다 (`crates/coordinator/src/lib.rs:40-48`). CLI도 `--peer-pubkey`와
  `--agent-device-id` 하나만 요구한다 (`lib.rs:1527-1537`).
- 시작 시 keyring에 그 한 쌍만 넣는다 (`lib.rs:323-327`).
- accept loop는 `serve_one_connection()`을 동기적으로 끝낸 뒤 다음 연결을 받는다
  (`lib.rs:330-380`). 공유 replay guard와 mutable lease store도 같은 순차 경로에
  묶여 있다.
- `connection_attempt`는 Agent별 값이 아니라 Coordinator의 전체 accept 횟수다
  (`lib.rs:333-357`). Resume hello의 값도 이 전역 순번과 같아야 한다
  (`lib.rs:843-886`).
- ACK, renew, resume 모두 config의 한 `agent_device_id`와 직접 대조한다
  (`lib.rs:551-558`, `670-678`, `876-886`).

즉 저장소를 추가하는 것만으로 실제 두 Agent가 접속할 수는 없다. 그 경로를 열려면
최소한 Agent별 key lookup, accept당 독립 task, 공유 저장소/재생 방어 동기화,
Agent별 connection generation, active-session ownership/fencing이 먼저 설계돼야 한다.
이는 DoD-40의 “최소 2~4일 아키텍처 변경” 판정과 같은 blocker다
(`docs/evidence/DoD-40_lease_store_동시_발급_안전성.md:119-127`).

### inventory producer와 충분한 wire schema가 없다

- 저장소 안에는 Agent registry, capability/heartbeat store가 없다.
- `AgentSessionHello`는 mode/session/node/attempt/time/nonce/signature만 가지며
  resource inventory는 없다 (`proto/lease.proto:122-137`).
- `NodeRecord`는 heartbeat 시각과 보안/신뢰성 일부만 있고 GPU 목록, 가용 VRAM,
  CPU/RAM/workspace, owner/workload policy가 없다
  (`proto/control.proto:504-522`).
- 반면 scheduler의 `CandidateSnapshot`은 그 값들을 명시적 `Option`으로 요구하고,
  unknown을 기본값으로 바꾸지 않는다 (`crates/scheduler/src/model.rs:83-111`).
- heartbeat/telemetry는 `ControlAction` 복제 로그 대상이 아니며 Node 상태 전이만
  control action이다 (`proto/control.proto:231-268`).

따라서 오늘 wire/proto를 임의로 발명해 Agent가 live snapshot을 보낸다고 주장하면
계약과 producer를 동시에 건드리는 원안 규모로 다시 커진다. 저장소 API는
**상위 계층에서 identity/signature/capability 검증을 끝낸 정규화 입력**만 받는
경계로 둔다. 그 검증 계층은 후속 조각이다.

## 오늘 착수할 최소 조각 — 3a

### 이름

**single-Coordinator durable Agent inventory repository kernel**

### In

- `crates/coordinator/src/inventory_store.rs`의 독립 SQLite 저장소 API.
- Agent registry row:
  - non-empty `node_id`, `device_id`, `owner_member_id`
  - 정확히 32-byte Ed25519 공개키
  - 정규화된 node/risk/security/isolation/key-protection 사실
- Agent별 최신 inventory revision:
  - caller가 주는 단조 `inventory_revision`
  - `observed_at_unix_ms`
  - GPU별 stable ID/model/health/available VRAM
  - available CPU/RAM/workspace, allowed workload class, third-party opt-in
- registry 등록은 동일 identity + byte-identical payload만 idempotent하다. 같은
  node/device ID에 다른 key나 owner를 쓰는 시도는 상태를 바꾸지 않고 거부한다.
- inventory 갱신은 한 `BEGIN IMMEDIATE`에서 parent + GPU/workload child row를
  전부 교체한다. 더 낮은 revision과 같은 revision의 다른 payload는 거부한다.
- `pool_snapshot(evaluated_at_unix_ms)`는 node ID 순으로 정렬된
  `gputeer_scheduler::PoolSnapshot`을 만든다. 저장하지 않은 사실은 `None`이고,
  stale 사실도 지우거나 fresh로 바꾸지 않고 원래 `observed_at_unix_ms`를 보존한다.
- `gputeer-coordinator`가 `gputeer-scheduler`를 의존해 저장된 사실을 기존
  scheduler domain model로 투영한다. proto 변경은 하지 않는다.
- 저장소는 시스템 clock, network, Agent process를 읽지 않는다.

여기서 registry row는 single-Coordinator v0.1의 로컬 durable truth다. 다중
Coordinator 합의나 Owner 승인 완료를 증명하지 않으며, 후속 ControlStore 통합 때
권위 경계를 다시 연결해야 한다.

### Out

- `CoordinatorConfig`/CLI 변경과 `run()` 연결.
- 실제 Agent 2대의 접속, 병렬 accept/task, wire heartbeat/capability message.
- 서명 검증, 승인/enrollment, capability probe, hardware/NVML 수집.
- `AgentSessionHello`를 registry 등록이나 inventory heartbeat로 재사용하는 것.
- active-session owner, 이전 session fencing, Agent별 reconnect generation,
  session takeover/cleanup.
- ONLINE→SUSPECT→UNREACHABLE→LOST 상태 판정 및 ControlStore commit.
- Raft, 다중 Coordinator 복제/합의, `COMMITTED` 주장.
- resource reservation, best-fit winner, Grant dispatch.
- 실제 telemetry 정확성이나 GPU hardware 실측.

### 필수 불변식과 negative tests

1. 서로 다른 key/device를 가진 Agent 2개 이상을 저장하고 reopen 뒤 동일하게 읽는다.
2. snapshot 후보는 삽입 순서와 SQLite row 순서에 무관하게 node ID 순으로 결정적이다.
3. 같은 registry payload 재시도는 idempotent하고, node/device/key/owner 충돌은
   기존 row를 한 필드도 바꾸지 않는다.
4. inventory parent 기록 뒤, GPU child 교체 중, workload child 교체 중 오류를 각각
   주입해 이전 revision 전체가 보존되는지 확인한다.
5. 낮은 revision과 같은 revision의 다른 payload는 fail closed한다. 같은 revision의
   byte-equivalent payload만 idempotent success다.
6. 중복/빈 GPU ID, 빈 필수 identity, 32-byte가 아닌 key, 범위를 벗어난 enum은
   전체 transaction을 rollback한다.
7. 관측 안 된 값은 `None`으로 투영된다. `0`, 빈 문자열, proto enum 0을 정상값처럼
   채우지 않는다.
8. 오래됐거나 평가 시각보다 미래인 `observed_at_unix_ms`는 보존되고, 기존
   `evaluate_eligibility()`에 넣으면 `SnapshotNotFresh`로 거부된다.
9. 같은 DB를 연 별도 connection 두 개가 같은 next revision을 경쟁하면 정확히
   하나의 완전한 payload만 남고 child row가 섞이지 않는다. 이것은 저장소 경쟁
   검증일 뿐 wire 동시 Agent 검증으로 이름 붙이지 않는다.
10. 손상된 enum/key/revision/GPU child row를 reopen/read할 때 default로 복구하지
    않고 명시적 storage corruption 오류를 낸다.

### 완료 조건과 정직한 규모

- production Rust 350~600줄, 테스트 450~750줄, **1일**.
- 구현 변경 후보는 `crates/coordinator/src/inventory_store.rs`,
  `crates/coordinator/src/lib.rs`의 module export,
  `crates/coordinator/Cargo.toml`의 scheduler dependency 연결뿐이다.
  wire/CLI/proto diff는 0이어야 한다.
- DoD 후보 claim은 “single-Coordinator SQLite에 복수 Agent normalized inventory를
  원자·영속 기록하고 결정적 PoolSnapshot으로 투영한다”까지만 쓴다.
- “live”, “concurrent session”, “active owner”, “multi-Coordinator”, “다중 Agent
  지원 완료”를 claim에 쓰지 않는다.

## 조각 4로 건너뛸 수 있는가

조각 4 원안 전체로는 건너뛸 수 없다. 로드맵은 조각 4의 placement +
reservation이 조각 2와 live inventory인 조각 3 모두에 의존한다고 명시한다.
현재 hard-filter는 복수 적격이면 의도적으로 `RankingRequired`만 반환하고,
실제 resource truth/CAS reservation owner도 없다. 합성 snapshot 위의 순수
best-fit 함수만 별도 연구하는 것은 가능하지만 실제 v0.1 placement + reservation
완료가 아니며, 오늘의 3a보다 핵심 의존성을 덜 해소한다.

따라서 순서는 다음이 정직하다.

```text
오늘: 3a durable inventory repository + deterministic projection
  ↓
후속: heartbeat/capability wire 계약 + 검증된 producer
  ↓
후속: per-Agent concurrent session owner/fencing (DoD-40 이월)
  ↓
조각 4: live snapshot 기반 best-fit + CAS reservation/admission
```

3a가 완료돼도 조각 3 원안의 완료율을 과장하지 않는다. 다만 조각 4가 필요로 하는
`PoolSnapshot`의 저장·투영 경계를 먼저 고정하므로 버리는 우회 구현은 아니다.

## 구현 결과 (2026-08-21)

### 구현한 범위

- `crates/coordinator/src/inventory_store.rs`에
  `CoordinatorInventoryStore`를 추가했다. 검증·정규화가 끝난 입력만 받는 독립
  SQLite API이며 시스템 clock, network, Agent process를 읽지 않는다.
- `AgentRegistry`는 non-empty `node_id`/`device_id`/`owner_member_id`, 정확히
  32-byte인 Ed25519 공개키, scheduler의 node/risk/security/isolation/
  key-protection 사실을 저장한다. 동일 정규화 payload만 멱등이고 node/device/key/
  owner 충돌은 기존 row를 바꾸지 않고 거부한다.
- `AgentInventory`는 caller-provided `inventory_revision`과
  `observed_at_unix_ms`, GPU/CPU/RAM/workspace/workload/third-party 사실을
  저장한다. u64는 기존 store와 같이 big-endian BLOB으로 보존한다.
- `update_inventory()`는 `BEGIN IMMEDIATE` 하나에서 parent upsert와 GPU/workload
  child 전량 교체를 수행한다. 낮은 revision과 동일 revision의 다른 payload는
  fail closed하고, 동일 revision의 정규화 동등 payload만 멱등 성공한다.
- `pool_snapshot(evaluated_at_unix_ms)`는 한 SQLite read snapshot에서 registry와
  inventory를 읽어 node ID 순으로 정렬된 기존
  `gputeer_scheduler::PoolSnapshot`을 반환한다. 미관측 사실은 `None`이며 저장된
  관측 시각은 stale/future 여부와 관계없이 그대로 보존한다.
- `crates/coordinator/src/lib.rs`는 새 모듈을 export하고,
  `crates/coordinator/Cargo.toml`/`Cargo.lock`은 기존 scheduler domain model을
  재사용하기 위한 path dependency만 추가했다. proto/CLI/`run()`은 바꾸지 않았다.

원래 3a 범위를 더 좁히지 않았다. 실제 다중 연결, heartbeat wire, enrollment/signature
검증, active-session owner/fencing, 상태 판정, Raft/ControlStore, reservation/dispatch는
계획의 Out 그대로 구현하지 않았다. 따라서 이 결과는 “다중 Agent 지원 완료”가 아니라
**single-Coordinator local durable inventory repository kernel**이다.

### negative·경계·경쟁 테스트

`inventory_store` 단위 테스트 11건이 다음을 검증한다.

1. 서로 다른 Agent 2개 저장, reopen 영속성, 삽입 순서와 무관한 node/GPU 결정 정렬.
2. registry 10회 재시도 멱등성과 같은 node의 device/key/owner 변경, 다른 node의
   device/key 재사용 거부 및 기존 row 무변경.
3. 동일 revision의 GPU 입력 순서 차이는 멱등, payload 차이와 낮은 revision은 거부.
4. parent 기록 직후, GPU child 부분 교체 중, workload child 부분 교체 중 주입 실패가
   이전 parent/GPU/workload 전체를 보존.
5. 공백 필수 identity, 31-byte key, 빈/중복 GPU ID, 공백 GPU model,
   미등록 node inventory가 상태를 만들거나 바꾸지 않음.
6. `None`과 관측된 `0`/빈 GPU·workload 집합/`false`가 투영에서 구분됨.
7. stale와 future 관측 시각이 보존되고 기존 `evaluate_eligibility()`에서 둘 다
   `SnapshotNotFresh`로 거부됨.
8. `u64::MAX` revision/time/resource와 `u32::MAX` CPU의 BLOB round-trip.
9. 별도 connection 두 개의 같은 next revision 경쟁에서 정확히 한 완전 payload만
   남고, 후보별 GPU/workload/opt-in까지 구별해 child 혼합을 탐지.
10. 손상된 enum/key/revision/GPU child row를 reopen 후 `CorruptData`로 거부.
11. 손상된 redundant payload와 관측 marker boolean을 default로 복구하지 않고 거부.

뮤테이션 테스트 2건도 원복까지 확인했다.

- `pool_snapshot()`의 유일한 node 정렬 제거: `multiple_agents_reopen_and_project_in_node_order`
  실패, 실제값 `[node-b, node-a]` 대 기대값 `[node-a, node-b]`.
- revision 하강 비교를 `<`에서 `<=`로 변경: 동일 revision 멱등 테스트가
  `LowerRevision { stored: 8, requested: 8 }`로 실패.

### 자체 재검토와 수정

- **fail-open/negative 경로:** 최초 구현은 공개키를 `[u8; 32]`로 받아 잘못된 길이의
  실제 API 입력 거부를 테스트할 수 없었다. `Vec<u8>` + 정확한 길이 검사로 바꾸고
  31-byte 입력이 무부작용으로 거부됨을 추가했다. DB read도 같은 검사를 거쳐 손상
  key를 `CorruptData`로 거부한다.
- **경쟁 테스트 판별력:** 최초 경쟁 후보의 workload 집합과 third-party opt-in이 같아
  일부 child 혼합을 식별하지 못했다. 두 후보의 해당 값도 모두 다르게 만들어 최종
  구조체 전체 동등성 검사가 parent와 두 child 집합을 구분하게 했다.
- **결정성 뮤테이션:** SQL `ORDER BY`와 Rust sort가 중복되어 한쪽 제거 뮤테이션이
  살아남을 수 있었다. registry read의 중복 정렬을 제거하고 projection의 단일 sort만
  남겨 테스트가 정렬 제거를 실제로 잡게 했다.
- **교착·경계·기존 API 회귀:** wire/session 대기 루프는 추가하지 않았다. write는
  `BEGIN IMMEDIATE`, snapshot은 upgrade 없는 read transaction이며 별도 connection
  경쟁이 완료됐다. revision/time/resource 최대값과 stale/future 경계를 테스트했고,
  전체 workspace 테스트에서 기존 Coordinator/Agent/Lease API 회귀가 없음을 확인했다.

### 검증 결과와 한계

```text
cargo build --workspace --exclude gputeer-runtime-windows
  PASS — exit 0

cargo test --workspace --exclude gputeer-runtime-windows
  PASS — 427 passed, 0 failed, 1 ignored

cargo test -p gputeer-coordinator inventory_store::tests::
  PASS — 11 passed, 0 failed

git diff --check
  PASS — whitespace error 0
```

이 PowerShell 세션의 `PATH`에는 cargo가 없어 실제 명령은
`C:\Users\playdata2\.cargo\bin\cargo.exe` 절대 경로로 같은 인자를 실행했다.
`cargo fmt -p gputeer-coordinator -- --check`는 설치된 rustup proxy가 현재
`stable-x86_64-pc-windows-msvc` toolchain의 rustfmt component 부재를 보고해 실행하지
못했다. 컴파일과 `git diff --check`는 통과했지만 rustfmt 미실행은 한계로 남긴다.

구현 파일은 총 1,514줄이며 test module 시작 전까지 약 1,028줄, 테스트 약 486줄이다.
테스트 규모는 조사 추정 안이지만 production은 350~600줄 추정을 초과했다. 주요 원인은
모든 scheduler enum의 명시적 저장 codec, `None`과 명시적 zero/empty를 구분하는 parent
marker, parent/child와 redundant normalized payload의 양방향 손상 대조다. 범위 밖 기능을
추가해서 늘어난 것은 아니지만, 코드량 추정은 맞지 않았음을 숨기지 않는다.
