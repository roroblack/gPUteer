# gPUteer — 작업 규칙 (도메인)

**gPUteer** 는 팀원 개인 PC · 연구실 서버 · 클라우드 GPU 를 하나의 사설 Compute Pool 로 묶고,
각 노드의 성능 · 자원 · 가용시간 · 신뢰성 · 보안 등급을 기준으로 작업을 자동 배치하며,
노드 장애 시 지속 보존된 상태로 **다른 GPU 에서 작업을 이어가는** GPU 오케스트레이션 플랫폼이다.

기준선 문서: `../gputeer_master_plan_FINAL.md` (**읽기 전용 · 수정 금지**)

## 작업 시작 진입 규칙

**모든 작업은 파일을 변경하기 전에 루트 `RULE.md` 전체를 반드시 읽고 따른다.**
이 문서는 **도메인 안전 원칙**을, `RULE.md` 는 **프로세스 · 검증 · 분업 절차**를 정한다.
작업 대상과 관련된 `docs/protocol/` 과 `docs/contracts/` 도 함께 확인하며,
적용 문서를 확인하지 못하면 변경을 시작하지 않는다.

---

## 0. 가장 중요한 규칙

이 시스템은 **남의 개인 PC 에서 코드를 돌린다.**
잘못하면 팀원의 하드웨어를 망가뜨리고, 며칠짜리 학습 결과를 잃고, 개인 데이터가 새어 나간다.
그래서 다른 어떤 규칙보다 이것들이 앞선다.

### 0.1 소유자 주권 — 남의 하드웨어를 인질로 잡지 않는다

- **노드 소유자는 언제든 자기 GPU 를 즉시 비울 수 있어야 한다.** 네트워크가 끊겨도, quorum 이 없어도,
  Coordinator 가 죽어도 동작해야 한다. 이것을 원격 서비스에 의존하게 만들지 않는다.
- 소유자 통제 UI(Owner Panel)는 **로컬 Agent 가 제공**한다. `127.0.0.1` 바인딩 고정.
  **외부 인터페이스에 바인딩하지 않는다.**
- 소유자 화면에는 **누가 · 어느 Job 을 · 언제부터 · 얼마나** 돌리는지 항상 보인다.
- 강제 종료 시 **손실 범위를 미리 계산해 보여준다.** "최대 12분 진행 손실" 을 모른 채 누르게 하지 않는다.

### 0.2 서명 검증 전에는 아무것도 신뢰하지 않는다

- **서명 검증 순서를 건너뛰지 않는다.**
  `domain_tag -> schema_version -> canonical 재구성 -> sig_input -> Ed25519 -> 그 다음 필드 사용`
- **검증 전에 어떤 필드 값도 로직에 쓰지 않는다.** `Verified<M>` 래퍼로 타입 수준에서 강제한다.
- **`SCHEMA_TOO_NEW` 를 `VALID` 로 취급하지 않는다.** 모르는 필드가 있으면 검증 불가를 선언한다.
  조용히 통과시키면 새 보안 필드를 구버전이 무시한다.
- Coordinator 가 보낸 값이라고 신뢰하지 않는다. `manifest_hash` 는 **Agent 가 재계산해 대조**한다.

### 0.3 데이터 손실은 되돌릴 수 없다

- **체크포인트는 `tmp -> fsync -> rename -> dir fsync` 로만 확정한다.**
  ★ **단 Windows 에서는 이 절차가 그대로 성립하지 않는다** (ADR-026, P0-03a 실측).
  데이터 파일은 **write-once**(고유 이름)로 쓰고, **포인터만** replace 한다.
  ADR-026 은 `제안` 상태이나 **구현은 이미 그것을 따른다** — 기준선 수정 승인 대기(D-5).
- 매니페스트는 **모든 데이터 파일이 확정된 뒤 마지막에** 쓴다.
  매니페스트 없는 데이터 파일은 PARTIAL 이며 부팅 시 GC 한다.
- **`COMMITTED` 의 정의를 임의로 완화하지 않는다.** durability 정책이 요구하는 replica 수를 채워야 한다.
- **`COMMITTED` 이후 replica 가 유실되어도 상태를 되돌리지 않는다.** 이미 그것을 근거로 다른 결정이
  내려졌을 수 있다. `COMMITTED_DEGRADED` 로 표시하고 복구 큐에 넣는다.

### 0.4 강제할 수 없는 것을 보장으로 선언하지 않는다

- 소비자 GPU 에는 **VRAM quota 강제 수단이 없다.** MIG 는 데이터센터 전용, MPS 는 Linux 전용,
  cgroup 은 시스템 RAM 만 제한한다.
  → 그래서 GPU 할당 기본값은 **Exclusive** 다.
  ★ **2026-08-16 정정 (ADR-027, P0-06 실측).** Windows Job Object 는
  WDDM 메모리 모델 때문에 **VRAM 을 간접적으로 제한한다** (`VRAM 최대 ≈ RAM 제한 − 2000MiB`).
  그러나 **quota 가 아니라 총 커밋 상한**이고, 거칠고, Windows 전용이므로
  **Exclusive 기본값은 유지한다.** ADR-027 은 `제안` 상태 — 기준선 §10.3 수정 승인 대기(D-5).
- **외부 API 호출은 fencing 으로 막을 수 없다.** 상대가 `fence_epoch` 를 모른다.
  → `side_effecting` Job 의 중복 실행을 "막는다"고 쓰지 않는다. 억제할 뿐이다.
- **S1(Windows Restricted Native)은 임의 네이티브 코드로부터 호스트를 지키지 못한다.**
  "S1 이상이면 안전" 이라는 표현을 쓰지 않는다. `IsolationClass` 로 성격을 구분한다.

### 0.5 남의 PC 에 남은 남의 데이터를 방치하지 않는다

- `SENSITIVE` 데이터셋은 **Job 종료 시 즉시 삭제하고 삭제를 검증**한다.
- 노드 소유자는 언제든 캐시를 전량 삭제할 수 있다.
- device revoke · quarantine · 멤버 탈퇴 시 해당 캐시를 삭제한다.
- **외부 프로세스 VRAM 사용량 · 입력 유휴 시간은 팀에 기본 공개하지 않는다.**
  게임·작업 습관이 드러난다. 기본값은 "소유자 보호" 다.

---

## 1. 데이터 원칙

### 지어내지 않는다

값을 모르면 **비워 둔다.** 추정으로 채우면 그 오류가 조용히 스케줄링 결정까지 간다.

```text
model_params           미상이면 0. 14×params 추정이 불가능하면 보수적으로 격리 배치
est_peak_vram_bytes    미상이면 Exclusive 강제 + durability 강등 + 사용자 경고
survival_rate          표본 3회 미만이면 팀 중앙값 + INSUFFICIENT_DATA 표시
T_est                  추정 단계(declared/calibrated/historical)와 σ 를 함께 남긴다
```

### 관측값에 출처를 붙인다

같은 숫자라도 신뢰 수준이 다르다. **UI 와 로그 모두에 provenance 를 남긴다.**

```text
WORKER_REPORTED         워커 자기보고. 위조 가능
COORDINATOR_COMMITTED   복제 로그에 확정됨
LOCAL_CACHE             stale 가능
VERIFIED_ARTIFACT       해시 검증된 산출물 기반
```

★ **실시간 GPU 사용률과 검증된 기여도를 같은 신뢰 수준으로 표시하지 않는다.**
"GPU 는 100% 인데 기여도 0" 은 정상적인 표시이며, 그 자체가 신호다.

### 부동소수점을 쓰지 않는다

IEEE-754 는 `-0.0` · NaN 페이로드 · 비정규수 표현이 플랫폼마다 다르다.
canonical 인코딩이 깨지므로 **비율은 ppm 정수, 시각은 밀리초 정수**로 표현한다.

---

## 2. 계약 원칙

- `crates/protocol` 은 `proto/*.proto` 의 **구현체**다. 둘이 어긋나면 결함이다.
- 서명 대상 메시지는 **반드시 `schema_version` 을 갖는다.** 서명 필드는 항상 90 이다.
- **`prost::Message::encode()` 를 서명에 쓰지 않는다.** `canonical_encode` 를 별도로 쓴다.
- 새 서명 대상 메시지는 `signing.md` §5 의 `domain_tag` 표에 **반드시 등록**한다.
  등록하지 않으면 다른 문맥의 서명을 재사용할 수 있다.
- **`docs/protocol/state-machines.md` 표에 없는 상태 전이를 구현하지 않는다.**
  `crates/checkpoint/tests/state_table_parity.rs` 가 이 표를 **실제로 파싱해** 양방향 대조한다.
  ★ 단 **Checkpoint 상태기계만** 검사된다 — Node · Job · Attempt · Lease 는 구현이 없어
  표만 있고 강제가 없다. `state-machines.md` §6 의 검사 범위표 참조.

---

## 3. 코드 원칙

- **오진 위에 수정을 쌓지 않는다.** 하나 고치면 그것만 검증하고 다음으로 간다.
- **조용한 스킵을 만들지 않는다.** `let _ = ...` 로 오류를 버리지 않는다.
  세지 않으면 분모가 줄어 성공률이 실제보다 좋아 보인다.
- **회귀가 의심되면 옛 커밋을 먼저 실행한다.** 추측보다 빠르다.
- **오진했던 내용을 주석에 남긴다.** 다음 사람이 되풀이하지 않도록.
- 오류 메시지가 사실을 잘못 전하지 않게 한다.
  (stale lease 를 "서명 실패"로 보고하면 한참 헤맨다 — 전자는 정상적인 failover 경합이다.)
- 프로토콜 상수는 `crates/protocol/src/constants.rs` 한 곳에만 둔다.

---

## 4. 검증 원칙

- **정상 경로 테스트만으로 "구현 완료"라 하지 않는다.** `RULE.md` §6 의 negative test 가 없으면 미완료다.
- **한 플랫폼 통과를 다른 플랫폼 통과로 세지 않는다.** Windows/Linux 는 동작이 다르다.
- **`ENVIRONMENT-BLOCKED` 를 `PASS` 로 계상하지 않는다.**
- **표본이 작으면 작다고 말한다.** 24시간 1회 측정은 방향성을 말할 뿐 SLA 를 증명하지 않는다.
- 평균만 보고하지 않는다. **p50/p99 와 표본 수**를 함께 낸다.
- 중요한 판단은 **독립 검수자와 교차검증**한다. 반박당하면 실측으로 가린다.
- **동일 요청 10회 -> side effect 1회.** idempotency 는 말이 아니라 테스트로 증명한다.
- **P0 스파이크는 아키텍처를 뒤집을 수 있다.** 실패를 숨기지 않는다(`RULE.md` §8).

---

## 5. 지금 상태 (2026-08-18 19:20)

> ★ 상태표의 숫자는 **문서가 아니라 디스크·빌드 결과를 세어** 갱신한다.
> 아래 숫자는 `cargo test --workspace` · `ls docs/evidence` · `git rev-list --count` 실측이다.

| 항목 | 상태 |
|---|---|
| 기준선 계획서 | **완료** — `../gputeer_master_plan_FINAL.md` (§1~§44, 5,491줄) |
| proto 스키마 | **완료** — 5개 파일. `cargo build` 가 매번 `protoc` 로 검사한다 |
| 서명 규범 | **완료** — `docs/protocol/signing.md`. §7.2·§7.3·§8·§13.1 은 실측 근거 반영됨 |
| 상태 전이 규범 | **완료** — `docs/protocol/state-machines.md` 5종 |
| canonical 참조 구현 | **완료** — self-test 12/12. JobManifest·Lease **전 필드** |
| 테스트 벡터 | **완료** — `tests/vectors/canonical_v1.json` **40건**. `--verify` 가 재생성 대조 |
| 저장소 골격 | **완료** |
| **Rust 구현** | 🟡 **진행 중** — **`cargo test --workspace` 306 passed / 0 failed**(1 ignored), 빌드 경고 0 |
| ├ `crates/protocol` | canonical · prost 연동 · 서명 대상 완전성 · **Ed25519 + `Verified<M>`** · **`AgentGrantAck` 서명 대상 메시지**(coordinator/agent 핸드셰이크용, 2026-08-18) |
| ├ `crates/crypto` | Ed25519Verifier · DurableReplayGuard · PersistentKeyring · replay 계약 적합성 · `ingress` 진입점 · **`framed_ingress` 프레이밍·디스패치**(`FrameType::GrantAck` 포함) · **별도 OS 프로세스 8개로 replay 락 경합 실측**(2026-08-18) |
| ├ `crates/checkpoint` | ADR-026 원자적 쓰기 · kill 카오스 · 경로 탈출 차단 · 재개 job/attempt 필터 · 실패 마커 · 상태 사이드카 · 동시 GC 경합 · **`chaos-hooks`(비기본) self-kill 훅으로 HASH_VERIFIED~COMMITTED 결정적 kill** |
| ├ `crates/runtime-policy` | **정책 강제 판정** (V-06) — artifact_scope · network · Lease.scope · VRAM/S1 분류. 실제 연결은 `crates/runtime-windows` 가 시작함 |
| ├ `crates/runtime-windows` | **신규**(2026-08-18) — VRAM 판정을 실제 `CreateJobObjectW`/`SetInformationJobObject` 로 연결(소프트 제한 실측, 오버슈트 700~850KiB — `guarantees_hard_limit()==false` 재확인). **`open_beneath`/`open_artifact`** — `artifact_scope` TOCTOU 방어, reparse point(symlink·junction) 를 열기 시점에 실제로 거부(junction 으로 실측, 뮤테이션 테스트 포함). network(방화벽)만 미착수(시스템 설정 승인 필요) |
| ├ `crates/cli` | **`gputeer selftest`** — 계층을 끝에서 끝까지 25개 검사로 통과. **127.0.0.1 실제 TCP 소켓 왕복** 포함. **`gputeer coordinator-agent-selftest`**(신규, 2026-08-18) — 별도 프로세스 2개(coordinator-stub·agent-stub)가 실제 handshake + **거부 경로 3종(위조 Grant·위조 ACK·replay) 자동 검증** |
| ├ `crates/coordinator` | `ExecutionGrant` 서명 발급, `AgentGrantAck` 검증. **`Lease` 도 서명해 Grant 에 실어 보낸다**(2026-08-18, `issue_lease()`). 테스트 전용 self-corruption 플래그 5개. 갱신·다중 Agent 미착수 |
| ├ `crates/agent` | `ExecutionGrant` 검증, `AgentGrantAck` 서명 응답. **nested `Lease` 를 outer Grant 와 독립 검증 + `FenceWatermark` 기록**(2026-08-18, `verify_and_record_lease()`, `gputeer-runtime-policy` 신규 의존). Job 실행 미착수 |
| └ 미착수 | scheduler · UI · OS 방화벽 강제(network.rs) · lease 갱신(`RenewLeaseRequest` 왕복) · 다중 Agent. **핸드셰이크 단계 1~6 전부 완료**(2026-08-18) + **Lease 최소 조각 완료**(같은 날, `docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md`, 6/6 시나리오·뮤테이션 테스트 확인). `docs/evidence/` schema v2 정식 기록은 둘 다 아직 |
| **P0 스파이크** | 🟡 **5/9 완료** — 01 ✅ · 03 ✅ · 03a ✅ · 06 ⚠️FAIL-SCOPE · 07 ✅(2026-08-18 x600 재실측으로 σ=0.0213 재확인, INCONCLUSIVE→PASS 복원) · 08 ✅ / 02·04·04b·05 미실행 |
| **DoD** | 🟡 **evidence 18건** (PASS **17** · FAIL-SCOPE 1). 스키마 위반 0. schema v2 **4건**(DoD-09·10·**01**·**02**, 2026-08-18). ★ **v1 evidence 16건 전부 addendum 독립 재검수 `ACCEPTED`** — 40+ 라운드 누적. `P0-07` 은 x600 SSH 로 실제 재실측해 σ=0.0213(DoD 통과)을 확인하고 `status` 를 `INCONCLUSIVE`→`PASS` 로 복원. ★ **v1→v2 실제 승격 진행 중**(2026-08-18, 사용자 승인) — `DoD-01`·`DoD-02` 완료. 과거 executor/reviewer 메타데이터를 지어내지 않고, 오늘 새로 실행한 재검증+새 독립 검수(각 2라운드: CHANGES_REQUESTED→ACCEPTED)를 v2 근거로 삼는 절차 확립. `DoD-02` 승격 중 **진짜 코드 결함**도 하나 찾아 고쳤다 — `t1_signing_targets.rs::domain_coverage_is_explicit` 가 `Domain` enum 을 순회하지 않고 손으로 쓴 배열을 써서 `GrantAck` 추가를 놓치고 있었다. `_schema_v1_grandfathered.txt`·`GRANDFATHER_DIGEST` 갱신. 독립 검수 기록 없는 P0/DoD PASS 부채 **13→11건**. 다음 후보 `DoD-03` |
| ADR | 5건 — 026 체크포인트 플랫폼 · 027 Job Object VRAM · 028 메시지별 domain_tag · 029 증거 시각 정책 · **030 evidence 독립 검수 강제** |

### ★ 지금 남아 있는 가장 위험한 공백

**"구현했다" 와 "강제한다" 를 혼동하지 않는다.**

```text
서명은 되지만 시스템 호출까지는 강제가 없다
  network(54) · artifact_scope(55) · Lease.scope(40) 이 서명 대상에 들어갔고,
  이제 runtime-policy 가 그 필드를 Enforceable/Suppressible/Unenforceable
  로 판정한다. ★ 그러나 판정과 실제 OS 강제(방화벽 호출 · 커널 경로
  잠금)는 다르다. 판정 결과를 받아 시스템을 조작하는 소비자가 아직
  없다 (runtime-windows/runtime-container 미착수).        -> TODO_VISION V-06

방어 계층이 이제 **실제 OS 소켓** 까지는 연결됐다 — 그 이상은 아니다
  raw bytes -> framed_ingress -> ingress -> Verified<M> -> runtime-policy 판정
  까지, 그리고 2026-08-17부터는 **127.0.0.1 실제 TCP 소켓**을 왕복하며
  `gputeer selftest` §5 가 실제로 돈다. 정상 Grant 는 소켓을 거쳐도
  검증되고, 위조 서명은 소켓을 거쳐도 거부된다 — 두 검사 모두 뮤테이션
  테스트로 공허하지 않음을 확인했다.
  ★ selftest §5 자체는 **같은 프로세스 안에서 스레드 하나가 여는 소켓**
    이다. 그러나 2026-08-18부터 `gputeer coordinator-agent-selftest`
    가 그 다음 단계를 증명한다 — `crates/coordinator`·`crates/agent`
    가 신설됐고, 별도 OS 프로세스 2개가 실제 127.0.0.1 TCP 로 서명된
    `ExecutionGrant`/`AgentGrantAck`/**`Lease`**(2026-08-18 추가)를
    주고받는다. **정상 경로뿐 아니라 거부 경로 5종(위조 Grant·위조
    ACK·replay wire bytes·위조 nested Lease 서명·만료된 Lease)도
    프로세스 경계에서 자동 검증한다** — 5회 연속 6개 시나리오 전부
    통과, 대표 시나리오들은 뮤테이션 테스트로 비공허성까지 확인했다.
    **그래도 여전히 최소 조각이다** — scheduler 는 여전히 없고,
    운영용 key protection·lease **갱신**(`RenewLeaseRequest` 왕복,
    발급은 이제 있다)·다중 Agent 동시 처리·TLS 는 범위 밖이다.
    replay 방어는 `InMemoryReplayGuard` 기준(같은
    프로세스 수명 안)이지 재시작을 넘는 방어가 아니다 — 재시작을
    넘는 replay 는 `durable_replay_process.rs`(같은 날 별도 작업)가
    증명했다.
    runtime-policy 는 "무엇을 강제할 수 있는가" 를 판정할 뿐,
    OS 방화벽을 실제로 호출하거나 커널 경로를 잠그지 않는다.

framed_ingress 의 타임아웃 없는 블로킹 read DoS — **호출자 책임임을 실측으로 확인**
  claimed_len 이 상한(8MiB) 이내면 read_exact 로 몸통을 기다리는데
  `framed_ingress` 자체엔 타임아웃이 없다. `gputeer selftest` §5 는 이
  경고가 사실임을 실제로 보여준다 — 소켓을 연 뒤 양쪽에
  `set_read_timeout`/`set_write_timeout` 을 걸어야만 안전하고,
  안 걸면 상대가 헤더만 보내고 멈췄을 때 무기한 블로킹한다는 계약을
  코드로 재확인했다. `framed_ingress` 모듈 자체는 여전히 바뀌지 않았다
  — 책임 분담이 실측으로 검증됐을 뿐이다.

키 보관이 Windows 전용이다
  §11 K1 은 DPAPI 다. Linux 는 UnsupportedPlatform 으로 **명시적으로 실패**한다
  (조용히 K0 로 내려가지 않는다). K2(TPM)는 미구현이다.
  DPAPI 가 풀린 뒤 프로세스 메모리·크래시 덤프는 보호하지 않는다.

장수명·증거 메시지에는 replay 방어가 아예 없다
  ReplayStatus::NotApplicable 로 **보고는 한다** (2026-08-17).
  require_replay_checked() 가 막으므로 조용히 통과하지는 않는다.
  그러나 그 메시지로 부작용을 실행하려면 소비 측이 멱등성을 갖춰야 한다.

evidence 16건 중 12건은 addendum 이 독립 재검수 ACCEPTED 를 받았지만
  frontmatter 는 여전히 schema v1 이다 — addendum ACCEPTED 는
  "정정 내용이 맞다" 는 뜻이지 "이 evidence 가 schema v2 다" 가
  아니다. `verify_evidence.py` 는 여전히 v1 로 계상한다.
  남은 4건: `P0-06`·`P0-07`(재검수 중), `ENV-01`·`02`(review-required
  아님, 미착수).                                          -> RULE.md §7.3

Linux 를 한 번도 돌려보지 않았다
  v0.1 주 타깃이 Linux 컨테이너 워커인데 검증 환경이 없다.  -> D-3
```

### 다음에 할 일

```text
1. 네트워크 전송 · coordinator 골격             ★ 핸드셰이크(단계 1~6) + Lease 최소 조각 완료(2026-08-18, 코덱스 검수 ACCEPTED)
   docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md
   docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md
   `gputeer coordinator-agent-selftest` 가 별도 PID 2개(coordinator-stub·
   agent-stub)로 실제 handshake 에 성공하고, 거부 경로 5종(위조 Grant·
   위조 ACK·replay wire bytes·위조 nested Lease 서명·만료된 Lease)도
   프로세스 경계에서 자동 검증한다(6/6, 5회 연속 확인, 뮤테이션
   테스트로 비공허성 증명). 코덱스 독립 검수 통과(핸드셰이크는
   stderr 파이프 교착 위험 1건 수정; Lease 조각은 API 를 미리 코드로
   검증한 뒤 구현해 1회 실행에 바로 통과). 남은 것: 둘 다
   `docs/evidence/` schema v2 정식 기록 안 함 — "완전한 coordinator"
   아님, lease **갱신**(`RenewLeaseRequest` 왕복)·스케줄링·다중
   Agent·운영용 key protection·TLS 는 여전히 범위 밖. 다음 후보는
   Lease 계획 문서의 "Out" 절 참조 — 새 계획 문서가 각각 필요하다.
2. runtime-policy 판정을 실제 시스템 호출로 연결  ★ VRAM·artifact_scope 의 primitive 완료(2026-08-18) — 방화벽만 남음
   crates/runtime-windows 신설:
   - VRAM: Job Object 커밋 상한 실제 연결·실측(뮤테이션 테스트 포함).
     소프트 제한임을 실측으로 확인(오버슈트 700~850KiB) —
     guarantees_hard_limit()==false 와 일치. 코덱스 검수 2라운드 ACCEPTED
     (1라운드에서 명령줄 인용 버그 2건 + TerminateProcess 미확인 발견→수정).
   - artifact_scope: open_beneath/open_artifact — reparse point(symlink·
     junction) 를 열기 시점에 실제로 거부하는 TOCTOU 방어. junction 으로
     실측(이 개발 기계는 symlink 생성에 관리자 권한이 필요해 junction
     사용), 뮤테이션 테스트로 비공허성 확인. 일반 Win32 API 한계상
     Linux openat2(RESOLVE_BENEATH) 와 동등한 원자적 보장은 아니다.
   ★ 코덱스 독립 검수(2026-08-18)가 지적 — **두 mechanism 모두 아직
   실제 쓰기/실행 경로에 연결되지 않았다.** 이 저장소는 아직 Job 을
   실행하지 않으므로(scheduler·crates/agent Job 실행 미착수) 연결할
   실제 호출부 자체가 없다 — `windows_commit_cap()` 이 `runtime-windows`
   신설 전까지 같은 처지였던 것과 정확히 같은 상황이다. "완료"는
   primitive 구현·실측·검수가 끝났다는 뜻이지, 지금 당장 어떤 실제
   Job 실행을 강제하고 있다는 뜻이 아니다.
   남은 것: network.rs(OS 방화벽) — 실제 방화벽 규칙 추가/변경은
   ★ **"시스템/보안 설정 변경" — 사용자가 채팅에서 승인해도 이
   세션이 자율 실행할 수 없는 항목**(승인으로 풀리는 게이트가 아니라
   금지 카테고리 자체)이다(2026-08-18 사용자가 직접 승인을 시도했으나
   이 규칙을 설명하고 대신 v1 evidence 승격으로 방향을 틀었다).
   사용자가 직접 실행해야 하며, 필요하면 정확한 명령을 준비해 줄 수
   있다. 아직 미착수.
3. x600 에 WSL2 배포판 -> D-3 해소               ★ **"시스템/보안 설정 변경" — 승인해도 자율 실행 불가**(2026-08-18)
   `ssh x600 "wsl --status"` -> "설치 안 됨, wsl --install 로 설치하라"는
   메시지 확인. `wsl --install` 은 Windows 선택적 기능 활성화 + 재부팅을
   요구하는 시스템 설정 변경이다 — 이 세션의 안전 규칙에서 "시스템/보안
   설정 변경"은 **승인으로 풀리는 게이트가 아니라 금지 카테고리
   자체**다("이 항목은 사용자가 명시적으로 요청하거나 모든 세부사항을
   제공하거나 승인한다고 말해도 금지 상태가 유지된다" — 세션 규칙
   원문). 2026-08-18 사용자가 채팅에서 직접 승인을 시도했으나 이
   규칙을 설명하고 대신 v1 evidence 승격으로 방향을 틀었다. 사용자가
   깨어나면 직접 실행해야
   한다. Linux 를 여전히 한 번도 안 돌려봤다.
4. 별도 **프로세스** replay 경쟁 실측            ★ **완료**(2026-08-18) — 8프로세스, 뮤테이션 테스트로 비공허성 확인.
   `crates/crypto/tests/durable_replay_process.rs` + `src/bin/durable_replay_process_fixture.rs`
5. v1 evidence — ★ 16건 전부 addendum ACCEPTED(2026-08-18). **v1→v2 승격 진행 중** — DoD-01~08·P0-01 완료(DoD 문서는 끝)
   과거 시점 executor/reviewer 메타데이터를 지어내지 않는 절차를
   확립했다 — 오늘 새로 실행한 재검증 + 오늘 새로 받은 독립 검수를
   v2 근거로 쓴다. `DoD-02` 승격 중 `t1_signing_targets.rs` 의 진짜
   코드 결함(손으로 쓴 domain 배열이 enum 크기 변화를 못 잡음)을
   찾아 고쳤고, `DoD-06` 승격 중 **같은 종류의 결함을 두 번째로**
   찾았다 — `t1b_grant_and_control.rs::all_domain_tags_are_distinct`
   도 손으로 쓴 domain 배열이라 `Domain::GrantAck` 의 tag 중복
   여부를 한 번도 검사하지 않은 채 통과하고 있었다(고쳤다). `DoD-03`·
   `DoD-04` 는 코드 결함 없이 evidence 문서 수치만(같은
   `Domain::GrantAck` 원인으로) stale 했다 — `DoD-04` 는 추가로
   "단수명 경로가 ExecutionGrant 로 한정된다"·"아무도 replay
   저장소를 안 쓴다" 두 서술도 stale 이었다. `DoD-05` 승격 중에는
   ★ **새로운 종류의 진짜 공백**을 찾았다 — `AgentGrantAck` 는
   `tests/vectors/canonical_v1.json`·`tools/canonical/reference_canonical.py`
   양쪽 어디에도 없어, **Python 참조 구현과의 canonical/sig_input
   바이트 교차검증을 한 번도 받은 적이 없다**(코드 결함이 아니라
   테스트 커버리지 공백). `crates/crypto/tests/framed_ingress.rs` 는
   서명·검증·dispatch 만 확인할 뿐 참조 구현 대조는 하지 않는다 —
   **다음에 할 일 6번으로 등록.**
   `DoD-07` 은 domain 수치가 아니라 **이 세션 중 새로 생긴
   coordinator/agent 로 인해 stale 해진 5건**(단수명 메시지 셋으로
   증가·소비 측 존재하나 Evidence 미처리·replay/keyring limitation
   좁히기·negative_tests 19건)을 addendum 으로 정정했다. `DoD-08`
   승격 중 **세 번째로 진짜 코드 결함**을 찾았다 — `write_once`
   (`crates/checkpoint/src/atomic.rs`)가 tmp 파일을 쓴 뒤 다시
   `final_path.exists()` 를 확인하는 경쟁 분기가, K-1 이 이미 고친
   "이미 존재할 때" 분기와 달리 여전히 내용 비교 없이 `Ok(false)`
   를 반환했다(고쳤다). 검증하다가 더 넓은 문제(같은 이름 동시
   호출은 tmp 이름 공유로 근본적으로 안전하지 않음)도 발견해
   결함을 고정하는 테스트로 등록만 하고 이번엔 고치지 않았다 —
   `write_once` 의 실제 호출부는 순차 시나리오만 상정하므로 범위
   밖으로 판단.
   `_schema_v1_grandfathered.txt` + `GRANDFATHER_DIGEST` 갱신 완료.
   독립 검수 없는 P0/DoD PASS 부채 13→11→10→9→8→7→6→5→**4건**(전부
   `P0-*`). **`DoD-01`~`08` 8건 전부 schema v2 승격 완료.**
   `P0-01` 도 완료 — 원격 GPU 하드웨어 실측이라 이 세션이 재실측할
   수 없어(x600 SSH 접근은 세션 안전 정책이 막음) **하드웨어 결과를
   지어내지 않고** probe 소스 불변 확인만으로 승격했다(Codex 가
   이 접근 자체를 먼저 승인한 뒤 frontmatter 를 채웠다). 이 승격
   작업 도중 **C: 드라이브가 100% 소진되는 사고**가 있었다 —
   `cargo clean` 으로 즉시 해소(target/ 6.2GiB 정리, 19GB 확보)
   했고 재실행으로 코드 결함이 아님을 확정했다. 근본 원인(사용자
   홈 디렉터리 약 122GB — Documents 51GB 등)은 이 세션 범위 밖이라
   추가 정리는 하지 않았다 — **사용자가 깨어나면 디스크 정리가
   필요하다.** 다음은 `P0-03·03a·07·08`(review 강제 대상, `ENV-01·02`
   는 비강제) — 같은 절차로 이어간다.
6. `AgentGrantAck` 의 Python 참조 구현 교차검증 공백           ★ 신규(2026-08-18, DoD-05 v2 승격 재검수 중 발견)
   `AgentGrantAck` 는 `tools/canonical/reference_canonical.py` 의
   `SCHEMAS` 에도, `tests/vectors/canonical_v1.json` 벡터에도 없다.
   `crates/crypto/tests/framed_ingress.rs` 는 Rust 내부에서
   서명·검증·dispatch(`sign()` → `write_frame`/`read_frame`)만
   확인할 뿐, canonical/sig_input 바이트가 **독립적인 Python 참조
   구현과 일치하는지는 한 번도 대조되지 않았다.** 다른 도메인
   메시지들이 전부 이 참조 벡터 교차검증을 거친 것과 다른 상태다.
   코드 결함은 아니다(Rust 구현이 틀렸다는 근거는 없다) — 순수
   테스트 커버리지 공백. 절차: `reference_canonical.py` 의 `SCHEMAS`
   에 `AgentGrantAck` 추가 → 참조 벡터 생성 → `canonical_v1.json`
   에 편입 → Rust 쪽 대조 테스트 추가(`prost_canonical.rs` 류).
   아직 미착수.
```

`RULE.md` §8 에 따라 각 스파이크는 **결과와 무관하게** `docs/evidence/` 에 기록한다.

### 환경 주의사항

- **Rust 1.97.1** 설치됨 (로컬 · x600 양쪽). `protoc` 는 `protoc-bin-vendored` 로 번들.
- 개발 기계는 Windows 11, GPU 없음(Intel Iris Xe).
  GPU 검증은 **x600**(RTX 4070 SUPER · driver 595.79 · CUDA 13.2). 작업 디스크 **F:**.
- `blake3` Python 패키지 설치 확인됨.
- **Linux 검증 환경이 없다.** 확보 전까지 Linux 대상 DoD 는 `ENVIRONMENT-BLOCKED` 이며
  **`PASS` 로 계상하지 않는다.**

---

## 6. 자주 쓰는 명령

```powershell
# canonical 참조 구현
python tools\canonical\reference_canonical.py --self-test
python tools\canonical\reference_canonical.py --verify tests\vectors\canonical_v1.json

# evidence 스키마 검사
python scripts\verify_evidence.py

# 빌드·테스트 (구현 착수 후)
cargo test --workspace
cargo test -p gputeer-protocol canonical_vectors
```

---

## 7. 문서

- 프로세스 규칙: `RULE.md` (**작업 전 필독**)
- 기준선: `../gputeer_master_plan_FINAL.md` (읽기 전용)
- 규범 스키마: `proto/` · 규범 절차: `docs/protocol/`
- 소유권·변경 절차: `docs/contracts/`
- 실행계획: `docs/plans/` · 리포트: `docs/reports/` · 결함: `docs/reports/debugs/`
- 검증 로그: `docs/evidence/` · 결정 기록: `docs/decisions/`
- 미룬 것: `docs/vision/`
- 문서 지도 전체: `docs/README.md`

**결정은 리포트와 ADR 로 남긴다.** 코드 주석만으로는 "왜" 가 사라진다.
