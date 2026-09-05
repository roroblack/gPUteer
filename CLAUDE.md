# gPUteer — 작업 규칙 (도메인)

**gPUteer** 는 팀원 개인 PC · 연구실 서버 · 클라우드 GPU 를 하나의 사설 Compute Pool 로 묶고,
각 노드의 성능 · 자원 · 가용시간 · 신뢰성 · 보안 등급을 기준으로 작업을 자동 배치하며,
노드 장애 시 지속 보존된 상태로 **다른 GPU 에서 작업을 이어가는** GPU 오케스트레이션 플랫폼이다.

> **최종 갱신** 2026-09-05 · **성격** 규범(이 저장소에서 가장 강한 규칙) ·
> **기준선** `../gputeer_master_plan_FINAL.md`(**읽기 전용 · 수정 금지**) ·
> **검사** `scripts/check_docs.py` · `scripts/verify_evidence.py`

## 이 문서를 어떻게 읽나

**§0~§4 가 규칙이고, §5 는 지금 상태다.** 규칙은 바뀌는 일이 드물고
상태는 매번 바뀐다 — 둘을 같은 무게로 읽지 않는다.

| 알고 싶은 것 | 볼 곳 |
|---|---|
| 무엇을 절대 하면 안 되나 | **§0** — 다른 어떤 규칙보다 앞선다 |
| 값·계약·코드·검증의 원칙 | §1 · §2 · §3 · §4 |
| 지금 무엇이 되고 무엇이 위험한가 | §5 상태표와 "가장 위험한 공백" |
| 앞으로 무엇을 할 것인가 | §5 "다음에 할 일" |
| **어떤 순서로 여기까지 왔나** | [`docs/history/조각_이력.md`](docs/history/조각_이력.md) |
| 절차 · 검증 · 분업 | [`RULE.md`](RULE.md) |
| 문서가 어디 있나 | [`docs/README.md`](docs/README.md) |

★ **이 문서는 매 세션 자동으로 읽힌다.** 그래서 규칙이 아닌 것을 여기
  두지 않는다 — 2026-09-05 에 완료 기록 1733줄을 `docs/history/` 로
  옮긴 이유다(2157줄 -> 508줄).

## 작업 시작 진입 규칙

**모든 작업은 파일을 변경하기 전에 루트 `RULE.md` 전체를 반드시 읽고 따른다.**
이 문서는 **도메인 안전 원칙**을, `RULE.md` 는 **프로세스 · 검증 · 분업 절차**를 정한다.
작업 대상과 관련된 `docs/protocol/` 과 `docs/contracts/` 도 함께 확인하며,
적용 문서를 확인하지 못하면 변경을 시작하지 않는다.

## ★ 세션 진행 규칙 (도메인 규칙 아님 — AI 어시스턴트 자신의 행동 규칙)

- **응답 언어: 사용자가 다른 언어로 답하라고 명시적으로 요청하지 않는 한
  한국어로 응답한다.** 코드·커밋 메시지·기술 식별자는 이 저장소의
  기존 관례(이미 한국어 위주)를 따르면 되고 언어 충돌이 없다.
  ★ 2026-08-18~19 사이 자율 `/loop` 세션에서 **두 번** 영어로
  드리프트해 사용자가 직접 정정했다 — 도구 호출이 많은 긴 턴에서
  직접적인 한글 채팅 입력이 없으면 영어로 되돌아가는 패턴이
  반복됐다. 판단 기준은 "지금 이 메시지가 한글인가" 가 아니라
  "이 턴을 촉발한 입력(사용자의 직접 메시지, 또는 그것이 남긴
  `/loop` 반복 프롬프트)이 한글인가" 다 — 자동 발화된
  `<task-notification>` 만 있고 사용자 발화가 없는 턴도 마찬가지로
  한국어를 유지한다.

- **사람이 읽어서 바로 이해되는 문장으로 설명한다.** 정확성을 위해
  전문 용어가 필요하면 쓰되, **처음 나올 때 한 줄로 풀어 준다.**
  ★ 2026-08-24 사용자가 직접 정정했다 — 설계 논의 중 "선형화 지점",
  "이음매", "provenance gate" 같은 말을 풀이 없이 계속 써서 읽기
  어려웠다.

  ```text
  나쁨   공개 풀에 전역 선형화 지점이 정의되지 않았다
  좋음   여러 노드가 같은 GPU 를 동시에 잡으려 할 때, 누가 먼저인지
         정해 줄 심판이 공개 풀에는 없다
  ```

  판단 기준은 **"이 분야를 모르는 사람이 이 문장만 읽고 무슨 일이
  일어나는지 그릴 수 있는가"** 다. 못 그리면 다시 쓴다.

  - 결론을 먼저 쓰고 근거를 뒤에 붙인다. 사용자가 첫 문장만 읽고도
    무엇을 해야 하는지 알 수 있어야 한다.
  - 선택지를 줄 때는 **각각이 무엇을 얻고 무엇을 잃는지**를 같이
    적는다. 이름만 나열하면 고를 수 없다.
  - 축약어·영문 용어를 문장의 주어로 쓰지 않는다.
    "`COMMITTED` 가 요구된다" 보다 "이 전이는 과반 합의로 확정해야
    한다(규범 용어로는 `COMMITTED`)" 가 낫다.
  - **이 규칙은 채팅 응답에 적용된다.** 코드 주석·커밋 메시지·
    `docs/` 문서는 기존 관례(정확한 용어 사용)를 따른다 — 거기서는
    독자가 이 저장소 맥락을 이미 아는 사람이다.

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

## 5. 지금 상태 (2026-09-05)

> ★ 상태표의 숫자는 **문서가 아니라 디스크·빌드 결과를 세어** 갱신한다.
> 아래 숫자는 `cargo test --workspace` · `ls docs/evidence` · `git rev-list --count` 실측이다.

| 항목 | 상태 |
|---|---|
| 기준선 계획서 | **완료** — `../gputeer_master_plan_FINAL.md` (§1~§44, 5,491줄) |
| proto 스키마 | **완료** — 5개 파일. `cargo build` 가 매번 `protoc` 로 검사한다 |
| 서명 규범 | **완료** — `docs/protocol/signing.md`. §7.2·§7.3·§8·§13.1 은 실측 근거 반영됨 |
| 상태 전이 규범 | **완료** — `docs/protocol/state-machines.md` 5종 |
| canonical 참조 구현 | **완료** — self-test 12/12. JobManifest·Lease **전 필드** |
| 테스트 벡터 | **완료** — `tests/vectors/canonical_v1.json` **52건**(2026-08-31 실측). `--verify` 가 재생성 대조 |
| 저장소 골격 | **완료** |
| **Rust 구현** | 🟡 **진행 중** — **세 환경에서 실측**(2026-09-05) — 개발 기계 Windows **922 passed**, x600 Windows **922 passed**, x600 WSL2 Linux **918 passed**(`--exclude gputeer-runtime-windows`). 전부 0 failed · 경고 0. `coordinator-agent-selftest` **97/97**(x600 재실행). ★★ **플랫폼 차이가 전부 설명된다** — 이름 기준 Windows 914-28=886, Linux 910-24=886 으로 일치하고 양쪽 전용은 서로 짝이다(junction<->symlink, Job Object<->cgroup, DPAPI<->systemd-creds). 조용히 빠진 공유 테스트는 없다. canonical 벡터 **52건** 은 2026-09-01 값 그대로(오늘 안 쟀다). 커밋 276개 |
| ├ `crates/protocol` | canonical · prost 연동 · 서명 대상 완전성 · **Ed25519 + `Verified<M>`** · **`AgentGrantAck` 서명 대상 메시지**(coordinator/agent 핸드셰이크용, 2026-08-18) |
| ├ `crates/crypto` | Ed25519Verifier · DurableReplayGuard · PersistentKeyring · replay 계약 적합성 · `ingress` 진입점 · **`framed_ingress` 프레이밍·디스패치**(`FrameType::GrantAck` 포함) · **별도 OS 프로세스 8개로 replay 락 경합 실측**(2026-08-18) |
| ├ `crates/checkpoint` | ADR-026 원자적 쓰기 · kill 카오스 · 경로 탈출 차단 · 재개 job/attempt 필터 · 실패 마커 · 상태 사이드카 · 동시 GC 경합 · **`chaos-hooks`(비기본) self-kill 훅으로 HASH_VERIFIED~COMMITTED 결정적 kill** · **`write_once()` 동시 동일-이름 호출 명시적 거부(2026-08-19, `DoD-21`)** — 프로세스 간 파일 잠금 + 성공 시 자가 정리 + GC 의 죽은 락 회수 |
| ├ `crates/runtime-policy` | **정책 강제 판정** (V-06) — artifact_scope · network · Lease.scope · VRAM/S1 분류. 실제 연결은 `crates/runtime-windows` 가 시작함 |
| ├ `crates/runtime-windows` | **신규**(2026-08-18) — VRAM 판정을 실제 `CreateJobObjectW`/`SetInformationJobObject` 로 연결(소프트 제한 실측, 오버슈트 700~850KiB — `guarantees_hard_limit()==false` 재확인). **`open_beneath`/`open_artifact`** — `artifact_scope` TOCTOU 방어, reparse point(symlink·junction) 를 열기 시점에 실제로 거부(junction 으로 실측, 뮤테이션 테스트 포함). network(방화벽)만 미착수(시스템 설정 승인 필요) |
| ├ `crates/cli` | **`gputeer selftest`** — 계층을 끝에서 끝까지 25개 검사로 통과. **127.0.0.1 실제 TCP 소켓 왕복** 포함. **`gputeer coordinator-agent-selftest`**(신규, 2026-08-18) — 별도 프로세스 2개(coordinator-stub·agent-stub)가 실제 handshake + **거부 경로 3종(위조 Grant·위조 ACK·replay) 자동 검증** |
| ├ `crates/coordinator` | `ExecutionGrant` 서명 발급, `AgentGrantAck` 검증, `Lease` 를 Grant 에 실어 보낸다(2026-08-18). **Lease 갱신 왕복도 처리**(2026-08-19) — 같은 연결에 이어서 `RenewLeaseRequest` 를 받아 검증하고 서명된 `RenewLeaseResult` 로 응답한다. **같은 연결에서 N 회 반복 갱신 가능**(`--renew-rounds`, `derive_renew_nonce(lease_id, round)` 로 회차별 nonce 분리). **`CoordinatorLeaseStore` 로 발급 Lease 를 SQLite 에 영속화**(`--lease-db`, optional — 안 주면 기존 레거시 경로) — 재시작 후에도 자신이 발급한 Lease 의 신원·epoch 를 기억한다. **`max_total_duration_seconds` 갱신 차단**(2026-08-19, `--lease-db` 사용 시에만) — 저장된 `issued_at_unix_ms` 기준 누적 시간 초과 시 서명된 `MAX_DURATION_EXCEEDED` 반환, 만료시각 미연장. **Lease revoke 최소 경로**(2026-08-19, `DoD-22`) — `--revoke-after-round` 로 이미 발급한 Lease 를 대상으로 서명된 `RevokeLeaseNotice` 를 같은 연결로 보낸다. **실제 재발급 정책 — SUPERSEDED 부분**(2026-08-19, `DoD-23`) — 영속 저장소가 있을 때 요청 epoch 이 저장된 값보다 낮으면 연결을 끊는 대신 서명된 `RENEW_OUTCOME_SUPERSEDED` 로 응답한다(proto 가 이미 "정상적인 failover 경합"이라 선언했던 상황을 실제로 그렇게 처리). **재접속 최소 조각 — 프로세스 재시작 복원**(2026-08-19, `DoD-24`) — 테스트 전용 `--disconnect-after-ack` 로 연결 단절을 흉내내면, 완전히 새로운 프로세스 쌍이 같은 `--lease-db`/`--fence-db` 로 `get_or_issue()` 를 통해 저장된 활성 Lease(identity·epoch)를 복원해 Grant 로 돌려준다 — 새 proto 메시지 없음, 기존 handshake 재사용. 다중 Agent·QUARANTINED 실제 트리거(`TODO_VISION` V-11)·자동 재접속 루프는 미착수. **Lease revoke 영속화**(2026-08-19, `DoD-25`) — `CoordinatorLeaseStore` 에 `revoked_at_unix_ms` 추가(기존 SQLite 파일도 `open()` 시 `PRAGMA table_info`+`ALTER TABLE` 로 자동 보정), `send_revoke_notice()` 가 wire 전송 전에 `mark_revoked()`(idempotent) 로 커밋을 먼저 확정, `get_or_issue()`·갱신 경로 양쪽 다 revoked Lease 를 거부한다 — revoke 된 뒤 재접속해도 되살아나지 않는다. **만료 Lease 재접속 거부**(2026-08-20, `DoD-26`) — `get_or_issue()` 가 만료된 Lease 도 거부한다(`<=` 경계, `crates/protocol/src/signing.rs`·Agent 의 revoke 검사와 동일 규칙). **`REVOKED` signed outcome**(2026-08-20, `DoD-27`) — 갱신 경로의 revoked 거부가 raw error 대신 서명된 `RenewLeaseResult{outcome: RENEW_OUTCOME_REVOKED=8}` 로 응답한다(proto enum 순수 추가). **자동 재접속 최소 경로**(2026-08-20, `DoD-35`) — `listener.accept()` 를 반복하는 루프로 바뀌었다(`--max-connections`·`--accept-timeout-ms`·`--drop-connection-after-ack-once`) — 같은 Agent 프로세스가 연결만 끊긴 뒤 재연결하면 기존 Grant/ACK handshake 를 다시 받아준다. Grant nonce 계산에 `connection_attempt` 반영(재접속 시 nonce 충돌 방지). **Resume 프로토콜**(2026-08-20, `DoD-36`) — 새 읽기 전용 `classify_resume()`(identity→revoke→만료(`<=`)→epoch 순 판정, Lease 만료시각 비연장)로 `ResumeLeaseRequest` 를 처리한다 — `AgentSessionHello(mode=RESUME)` opt-in 시에만 쓰이고 기본 경로는 안 바뀐다. **dispatcher 정교화**(2026-08-20, `DoD-37`) — 인라인 Grant 처리를 `serve_one_connection()` 으로 분리하고 `CoordinatorSessionError{Transport, Protocol, Storage}` 로 오류를 분류한다 — transport/protocol 오류는 로그 후 다음 accept, storage 오류(SQLite I/O·lease-store 장애)는 즉시 fail-closed 종료. Resume 경로도 포함 전체 lease-store 오류 지점이 이 원칙을 따른다. **`ExecutionGrant` schema v2**(2026-08-20, `DoD-39`) — 서명 대상 필드 25 `lease_from_durable_store`(bool) 순수 추가(domain_tag `gputeer/v2/grant`), `lease_store.is_some()` 이고 실제 SQLite transaction commit 이 성공했을 때만 true 로 서명한다. Agent 의 Ambiguous Renew durable 복구가 legacy Coordinator 오조합에서도 안전하도록 이 비트로 검증한다(아래 agent 행 참조). **durable Job/Queue truth**(2026-08-21, `DoD-42`) — 신규 SQLite `CoordinatorJobStore`가 `BEGIN IMMEDIATE`로 accepted submit 멱등 저장, `SUBMITTED→PLANNING→QUEUED`, 결정적 queue 조회와 queue 실패 사유를 보존한다. **single-node local atomic STAGING kernel**(2026-08-21, `DoD-43`) — `CoordinatorStagingStore::stage_queued_with_lease()`가 한 `BEGIN IMMEDIATE`에서 fence epoch 채번·Attempt/node/Lease 삽입·`QUEUED→STAGING`·operation idempotency를 원자 처리한다. 조각 2b-1의 로컬 `DURABLE`만 제공하며 다중 노드 결합·Raft `COMMITTED`는 후속이다. **durable Agent inventory 저장소 kernel**(2026-08-21, `DoD-44`) — 신규 `CoordinatorInventoryStore`가 복수 Agent registry를 충돌 방지·멱등 등록하고, 한 `BEGIN IMMEDIATE`에서 parent/GPU/workload inventory를 원자 교체해 node ID 순의 기존 scheduler `PoolSnapshot`으로 결정 투영한다. 조각 3a 저장소 경계만 제공하며 실제 다중 연결·heartbeat wire·session owner/fencing은 후속이다. **로컬 placement-to-staging orchestration kernel**(2026-08-21, `DoD-46`) — private `orchestrate` module이 snapshot→hard-filter→0/1/N→N에서만 best-fit→durable staging을 조합한다. inventory revision/CAS reservation이 없어 서로 다른 Job의 같은 GPU 중복 선택을 막지 못하므로 test fixture 외 production `run()`/accept-loop에는 연결하지 않았다. **inventory revision 기반 node-exclusive CAS reservation**(2026-08-21, `DoD-47`) — 선택 snapshot revision 비교, `node_id` PRIMARY KEY reservation, fence epoch·Attempt·Lease, `QUEUED→STAGING`, operation 기록을 한 `BEGIN IMMEDIATE` transaction에서 원자 처리한다. CAS·점유 충돌은 자동 재시도 없이 실패하며 기존 DoD-43 API는 그대로다. node-exclusive라 같은 node의 다른 GPU도 막히고 release가 없으며 private orchestration은 여전히 production `run()`/accept-loop에 미연결이다 **deterministic selected GPU assignment 선행 kernel**(2026-08-24, `DoD-48`) — single/N 후보가 scheduler의 같은 순수 `resource_fit()`을 사용하고 `Staged` outcome이 canonical `selected_gpu_ids`를 보존하며 STAGING 전에 요구 개수를 재검증한다. **selected GPU durable reservation binding**(2026-08-24, `DoD-49`) — canonical ID를 reservation child rows와 operation payload에 넣어 inventory CAS·node reservation·STAGING과 같은 transaction에 영속화하고 restart/replay 복원, node-scoped 존재 검사와 손상 fail-closed를 보장한다. 반환 ID는 snapshot 식별자이고 Grant/Lease scope·reservation release·production wire는 여전히 범위 밖이다. **verified signed JobManifest durable binding**(2026-08-24, `DoD-50`) — `submit_verified_manifest()`가 `&Verified<pb::JobManifest>`만 받아 accepted Job·원본 body·submission-time signer·재계산 hash·idempotency를 한 `BEGIN IMMEDIATE` transaction에 저장한다. load는 의도적으로 raw `StoredManifestBinding`이며 authoritative key directory 재검증 전 scheduler/Grant에 사용할 수 없다. membership validity·`JobRequirements` projection·기본 `MIRRORED` 소비·Grant/Lease scope·`COMMITTED`·production wire는 후속이다. **verified terminal AttemptReport durable binding**(2026-08-24, `DoD-51`) — `store_verified_terminal_report()`가 `&Verified<pb::AttemptReport>`만 받고, 한 `BEGIN IMMEDIATE` 안에서 report와 durable Attempt·현재 reservation을 job/attempt·single node·verified signer·fence·owner로 5중 대조한 뒤 signature 포함 body와 직접 계산한 hash를 저장한다. raw load는 재검증 전 terminal/release에 사용할 수 없고 Job/Attempt/Lease/reservation 상태는 바꾸지 않는다. **verified CheckpointManifest durable binding**(2026-08-24, `DoD-52`) — `store_verified_manifest()`가 `&Verified<pb::CheckpointManifest>`만 받고 write lock 뒤 같은 transaction의 Attempt/reservation owner를 대조하며 BLAKE3-256/32-byte root와 signature 포함 body를 first-write fact로 저장한다. raw load는 재검증 전 durability 판단에 쓸 수 없고 checkpoint/control state는 바꾸지 않는다. |
| ├ `crates/agent` | `ExecutionGrant` 검증, `AgentGrantAck` 서명 응답, nested `Lease` 를 outer Grant 와 독립 검증(2026-08-18). **Lease 갱신도 처리**(2026-08-19) — 서명·`request_nonce` echo·nested Lease 독립 서명·epoch 단조성(낮은 epoch 거부 + 높은 epoch 명시적 거부)을 전부 확인한 뒤에만 보유 Lease 를 교체한다. **같은 연결에서 반복 갱신**(`--renew-rounds`) 지원. **`FenceWatermark` 가 SQLite 로 영속화됨**(`DurableFenceWatermark`) — 최초 Grant·갱신 검증 두 호출부 모두 재시작을 넘는다, `--fence-db :memory:` 는 fail closed. **`MAX_DURATION_EXCEEDED` 명시 거부**(2026-08-19). **`RevokeLeaseNotice` 검증·처리**(2026-08-19, `DoD-22`) — 서명·`lease_id`·`fence_epoch`·만료 상태를 확인한 뒤 보유 Lease 를 revoked 로 표시하고 이후 갱신 요청을 만들지 않는다. **Job 실행의 첫 걸음 — WRITING 시작 마커**(2026-08-20, `DoD-30`) — 유효 Grant/Lease 검증 성공 직후·`AgentGrantAck` 전송 전에 결정적 `checkpoint_id`(BLAKE3-256)로 checkpoint 디렉터리를 만들고 `WRITING` 마커를 `write_once()` 로 기록한다(위조/만료/revoked Lease 는 마커 미생성, 마커 생성 실패는 fail-closed). **갱신 직전 만료 자기 재확인**(2026-08-20, `DoD-34`) — `RenewLeaseRequest` 를 만들기 전에 보유 Lease 의 만료(`<=` 경계)를 스스로 재확인해, 만료됐으면 요청을 안 보내고 `RENEW_REFUSED:LOCAL_EXPIRED` 로 종료한다(Coordinator 의 `DoD-32` 방어를 보완하는 낭비/관측 공백 해소). **자동 재접속 최소 경로**(2026-08-20, `DoD-35`) — TCP 연결이 끊기면 bounded retry(최대 8회·총 60초·exponential backoff+full jitter)로 재연결해 기존 Grant/ACK handshake 를 처음부터 재수행한다. `RenewLeaseRequest` 전송 뒤 결과를 받기 전에 끊기면(`AmbiguousRenew`) 재시도하지 않고 즉시 종료한다(durable request ledger 없이는 안전하지 않다). **Resume 프로토콜**(2026-08-20, `DoD-36`) — `--resume-protocol` opt-in 시 `AgentSessionHello`/`ResumeLeaseRequest` 로 명시적 재접속을 시도할 수 있다(기본값은 기존 handshake 재수행 그대로). **Resume outcome 별 명시적 처리**(2026-08-20, `DoD-38`) — `RESUMED`/`UNAVAILABLE` 를 뺀 6개 terminal outcome(`REVOKED`·`EXPIRED`·`SUPERSEDED`·`UNKNOWN_LEASE`·`IDENTITY_CONFLICT`·`EPOCH_AHEAD`) 각각이 `RESUME_REFUSED:<OUTCOME>` 구분된 오류 문자열로 즉시 종료한다(재시도 안전 동작 자체는 이미 올바르던 것을 그대로 유지, 문자열만 구분). **Ambiguous Renew 의 durable Lease 상태 기반 복구**(2026-08-20, `DoD-39`) — `--recover-ambiguous-renew-from-durable-lease`(기본 false) opt-in 시, `RenewLeaseRequest` 전송 뒤 결과를 못 받으면(`AmbiguousRenew`) 즉시 fatal 종료하는 대신 bounded reconnect 후 기존 Grant/ACK 경로(`get_or_issue()`)로 최신 저장 Lease 를 재조회하고 새 nonce 로 새 Renew 를 보낸다. 재조회한 Grant 가 서명된 `lease_from_durable_store=true` 가 아니면(legacy Coordinator 오조합 등) ACK·checkpoint·Renew 전송 **전에** `DURABLE_LEASE_RECOVERY_REFUSED` 로 fatal 거부한다 — legacy 상태에서 조작된 Lease 를 "재조회"로 착각하지 않는다. 실제 entrypoint 프로세스 실행·GPU 확인·runtime 격리·데이터 파일·manifest.json 작성·Coordinator 보고 wire 메시지는 여전히 미착수 — `RESULT ok=true` 는 Job 완료를 뜻하지 않는다 |
| ├ `crates/scheduler` + checkpoint durability kernel | **신규**(2026-08-21~24, `DoD-41`~`DoD-55`) — hard-filter부터 durable Job/Queue·STAGING·inventory·best-fit·reservation·selected GPU binding까지의 선행 kernel과 verified JobManifest/AttemptReport/CheckpointManifest 저장을 갖췄다. **verified ReplicaAck durable checkpoint/root binding**(`DoD-53`)은 signed observation을 validated DoD-52 anchor와 exact root에 묶어 immutable history로 저장한다. **resolved-input effective replica count 순수 kernel**(`DoD-54`)은 외부 resolver가 선택·검증한 holder observation만 받아 holder/domain 중복 제거와 `MIRRORED=1`/`REPLICATED=2` 요구치 충족 여부를 입력 순서와 무관한 report로 계산한다. **GPU ScopeCandidate 순수 kernel**(`DoD-55`)은 실물 RTX 4070 SUPER NVML 실측 뒤 full scope를 재판정해 계산만 떼어냈다. 관측·요구·명시 자원·caller provenance gate에서 `(available_vram, gpu_id)` 동점까지 결정적인 후보를 만들고 PARTITIONED·CUDA 미해소·파생 VRAM을 typed error로 닫는다. 두 kernel 모두 시계·I/O·DB·network·crypto·상태 전이가 없다. **로드맵 `DoD-41`~`DoD-55` 완료** — 하드웨어 값 부재는 해소됐지만 authoritative GPU observation provenance와 membership/ControlStore, full Grant/Lease scope는 별도 과제다 |
| └ 미착수 | scheduler(`DoD-41`~`DoD-55` 완료, orchestration과 ReplicaAck consumer는 production 미연결; reservation release, authoritative device→member·membership/failure-domain resolver·projection, signed GPU observation provenance·freshness/revision binding, `MIRRORED` 판정 적용/전이, durable count 저장, holder freshness 선택과 ACK retention 정책이 없고 모든 raw signed binding은 key directory 재검증 필요) · full Grant/Lease wire `ResourceScope`·GPU allocation/release·shared MPS/동일-owner·MIG partitioned allocation · 다중 노드/Raft COMMITTED · 다중 Agent/session owner/fencing · Job ingress·routing/outbox/wire · UI · OS 방화벽 · 실제 entrypoint 실행. `coordinator-agent-selftest` **97/97**(2026-09-05 x600 재실행)과 schema v2 `DoD-11`~`67` 기록 완료. ★ 실행 사슬 6조각(`DoD-68` 예정)은 **커밋됐으나 검수 대기** — 위 "DoD 최신 집계" 참조 |
| **P0 스파이크** | 🟡 **5/9 완료** — 01 ✅ · 03 ✅ · 03a ✅ · 06 ⚠️FAIL-SCOPE · 07 ✅(2026-08-18 x600 재실측으로 σ=0.0213 재확인, INCONCLUSIVE→PASS 복원) · 08 ✅ / 02·04·04b·05 미실행 |
| **DoD 최신 집계** | 🟡 **evidence 77건** (PASS **76** · FAIL-SCOPE 1, 2026-09-03 `verify_evidence.py` 실측). `DoD-56` NVML 관측 계층, `DoD-57` Linux cgroup 자원 상한, `DoD-58` 다중 Agent lane, `DoD-59` Linux K1 키 보관, `DoD-60` 노드 생존 판정·관측 영속화, `DoD-61` `ADR-033` §8 재배정 관문, `DoD-62` 예약 해제 경로와 증명 관문, `DoD-63` 이웃 신고 wire 메시지, `DoD-64` 이웃 신고 관측 저장소, `DoD-65` 이웃 신고 wire 연결, `DoD-66` NVML 불변식 고정, `DoD-67` 제출 manifest 운영자 반입 추가. 스키마 위반 0, 독립 검수 없는 P0/DoD PASS 부채 0건. ★★ **그러나 evidence 미작성 부채가 다시 생겼다 — 0 이 아니다.** 실행 사슬 6조각(`plan-job`·`stage-job`·`issue-grant`·`scheduler-tick`·Manifest 변환기·`submit` 축 확장)이 커밋됐지만 **독립 검수를 못 받아** evidence 를 못 썼다(코덱스 쿼터 소진, 복구 2026-09-07 15:43). 초안은 `docs/plans/2026-09-03_1740_DoD-68_evidence_초안_검수대기.md` 에 있다 — schema v2 에 "측정은 끝났고 검수만 없다" 상태가 없어 `docs/evidence/` 에 두지 않았다 |
| **DoD 상세 이력** (수치 아님) | ★★ **이 행의 숫자를 현재값으로 읽지 마라.** 현재 집계는 **바로 위 행**이다 — 여기 있던 "evidence 60건(2026-08-24 실측)" 은 그 시점의 값이고, 이 행이 남아 있는 이유는 **조각별 경위**(어떤 검수가 무엇을 찾았는지)를 잃지 않기 위해서다. 2026-09-05 에 머리말만 이렇게 고쳤고 아래 서술은 당시 기록 그대로다. `DoD-17`(RevokeLeaseNotice 커버리지)·`DoD-18`(max_total_duration_seconds 갱신 차단)·`DoD-19`(오래된 테스트 공백 3건)·`DoD-20`(`tools/canonical/check_schema.py` 신규 구현, 코덱스 1라운드가 실행 환경 오류 exit 코드 계약 위반을 실제 실행으로 발견 후 2라운드 ACCEPTED)·**`DoD-21`(2026-08-19, `write_once()` 동시 호출 계약 — 아래 참조)**·**`DoD-22`(2026-08-19, Lease revoke 최소 경로 — 아래 참조)**·**`DoD-23`(2026-08-19, Coordinator 재발급 정책 SUPERSEDED — 아래 참조)**·**`DoD-24`(2026-08-19, Lease 재접속 최소 조각 — 아래 참조)**·**`DoD-25`(2026-08-19, Coordinator Lease revoke 영속화 — 아래 참조)**·**`DoD-26`(2026-08-20, 만료 Lease 재접속 거부 — 아래 참조)**·**`DoD-27`(2026-08-20, REVOKED signed outcome — 아래 참조)**·**`DoD-28`(2026-08-20, check_schema.py CI 연결 — 아래 참조)**·**`DoD-29`(2026-08-20, 레거시 lease 경로 명시적 opt-in — 아래 참조)**·**`DoD-30`(2026-08-20, Job 시작 WRITING 마커 — 아래 참조)**·**`DoD-31`(2026-08-20, terminal outcome 다회차 교착 회귀 테스트 — 아래 참조)**·**`DoD-32`(2026-08-20, 만료 Lease 갱신 fail-closed — 아래 참조)**·**`DoD-33`(2026-08-20, marker-only checkpoint GC 회귀 테스트 — 아래 참조)**·**`DoD-34`(2026-08-20, Agent 쪽 갱신 직전 만료 재확인 — 아래 참조)**·**`DoD-35`(2026-08-20, 자동 재접속 최소 경로 — 아래 참조)**·**`DoD-36`(2026-08-20, Resume 프로토콜 — 아래 참조)**·**`DoD-37`(2026-08-20, Coordinator dispatcher 정교화 — 아래 참조)**·**`DoD-38`(2026-08-20, Agent Resume outcome 별 명시적 처리 — 아래 참조)**·**`DoD-39`(2026-08-20, Ambiguous Renew 복구 — 아래 참조)**·**`DoD-40`(2026-08-20, Lease store 동시 최초 발급 안전성, 로드맵 조각 7 재정의·자동 재접속 루프 로드맵 마무리 — 아래 참조)**·**`DoD-41`(2026-08-21, scheduler 순수 hard-filter kernel, 로드맵 조각 1 — 아래 참조)**·**`DoD-42`(2026-08-21, scheduler durable Job/Queue truth, 로드맵 조각 2a — 아래 참조)**·**`DoD-43`(2026-08-21, scheduler single-node local atomic STAGING kernel, 로드맵 조각 2b-1 — 아래 참조)**·**`DoD-44`(2026-08-21, scheduler durable Agent inventory 저장소 kernel, 로드맵 조각 3a — 아래 참조)**·**`DoD-45`(2026-08-21, scheduler 순수 deterministic resource best-fit kernel, 로드맵 조각 4 — 아래 참조)**·**`DoD-46`(2026-08-21, scheduler 로컬 placement-to-staging orchestration kernel, 로드맵 조각 5 — 아래 참조)**·**`DoD-47`(2026-08-21, scheduler inventory revision 기반 CAS reservation, 로드맵 조각 5b — 아래 참조)**·**`DoD-48`(2026-08-24, deterministic selected GPU assignment 순수 kernel — 아래 참조)**·**`DoD-49`(2026-08-24, selected GPU durable reservation binding — 아래 참조)**·**`DoD-50`(2026-08-24, verified signed JobManifest durable binding — 아래 참조)**·**`DoD-51`(2026-08-24, verified terminal AttemptReport durable binding — 아래 참조)**·**`DoD-52`(2026-08-24, verified CheckpointManifest durable binding — 아래 참조)** 추가. 스키마 위반 0. ★ v1 evidence 전부(DoD-01~08·P0-01·03·03a·07·08 13건) schema v2 승격 완료, 독립 검수 없는 P0/DoD PASS 부채 **0건**. `DoD-09`~`52` 는 신규 작성부터 v2. **`DoD-13`(2026-08-19)** — Lease 갱신 최소 조각(`RenewLeaseRequest`/`RenewLeaseResult` 왕복, `RenewLeaseResult` 신규 서명화 포함). 코덱스 1라운드 검수(`p99`)가 완료 보고 전에 실제 설계 결함 2건을 찾아냈다 — Coordinator 가 요청의 `fence_epoch` 을 검증하지 않던 문제, `FenceWatermark` 가 `<` 만 거부하고 `>`(epoch 상승)는 통과시키는데 계획서는 상승을 이 조각 범위에서 정책상 거부하라고 명시했던 문제. 둘 다 코드로 고치고 2라운드 좁은 후속 검수(`p100`)에서 `ACCEPTED`. 신규 검증 게이트 4건(nested Lease 독립 검증·request_nonce 대조·epoch 상승 거부·Coordinator epoch 대조)을 뮤테이션 테스트로 비공허성 확인. **`DoD-21`(2026-08-19)** — `write_once()`(`crates/checkpoint`)가 같은 `(dir, name)` 동시 호출을 지원하지 않고 명시적으로 거부하는 계약을 `std::fs::File::try_lock()` 기반 프로세스 간 파일 잠금으로 강제했다(`DoD-08` 이 발견했으나 안 고치고 넘겼던 결함의 후속). 코덱스 독립 검수 **5라운드**(`p128`~`p132`) — 매 라운드가 실제 결함을 찾았다: MSRV 불일치(`Cargo.toml` 1.85 vs `try_lock` 요구 1.89)·`k1c` 결함 고정 테스트의 비결정적 주장·GC 의 죽은 락 영구 보존으로 PARTIAL 디렉터리 청소 불능(내 1차 수정이 만든 회귀)·이름공간 충돌로 등록 데이터 파일 삭제 가능성(락 경로와 데이터 경로가 우연히 같아질 수 있음, 1차 등록 검사→근본 원인은 접미사 자체 예약)·대소문자/Win32 후행 점공백 우회. 전부 코드로 고치고 5라운드에서 `ACCEPTED`. 신규 테스트 5건(`k1c` 재설계+`k1d`·`k1e`·`k1f`·`k4c`)과 뮤테이션 테스트 6건으로 비공허성 확인 — 그 중 하나(락 파일 자가 정리 무력화)는 **기존** `durability_chaos.rs` 테스트 2건("완결된 체크포인트는 GC 가 절대 안 건드려야 한다")을 실패시켜, 이 세션 안에서 스스로 만들었다가 스스로 고친 회귀였음을 확인했다. GC 대 활성 writer 의 디렉터리 단위 경쟁은 의도적으로 범위 밖으로 남겨 문서화만 함(다중 Agent 실행 시작이 트리거). **`DoD-22`(2026-08-19)** — `RevokeLeaseNotice`(서명 대상·framed_ingress dispatch 는 `DoD-17` 이 이미 갖춰뒀다)의 실제 Coordinator/Agent 업무 로직을 구현했다 — Coordinator 가 이미 발급한 Lease 를 대상으로 서명해 보내고, Agent 가 서명·`lease_id`·`fence_epoch`·만료 여부를 검증한 뒤 보유 Lease 를 revoked 로 표시해 갱신을 멈춘다. ★ 사용자 요청에 따라 **구현 자체를 코덱스 CLI(workspace-write 샌드박스)에 위임**하고, 이 세션은 독립 재검증(빌드·테스트·selftest 직접 재실행)과 대화 기록이 없는 새 코덱스 인스턴스의 독립 검수(read-only)만 맡는 방식으로 진행했다. 독립 검수 1라운드(`p134`)가 진짜 교착 결함 1건(`--revoke-after-round 0` + Coordinator/Agent 둘 다 `do_renew=true` 조합에서 Coordinator 가 오지 않을 프레임을 기다림 — 기존 selftest 시나리오들이 이 조합을 우연히 피해가 안 드러났었다)을 포함해 4건을 찾아 전부 코드로 고쳤다(`p135`). 이 세션이 코덱스의 자체 뮤테이션 보고와 별개로 교착 방지 가드를 직접 되돌려 재현해(정확히 시나리오 25 에서 exit=1) 결함의 실재를 독립 재확인했다. 2라운드(`p136`)는 문서 완결성만 지적, 문서 보강 뒤 3라운드(`p137`)에서 `ACCEPTED`. 재접속 시 revoke 유실·비동기 전송·실제 정책 엔진(SUPERSEDED/QUARANTINED 판단)은 의도적으로 범위 밖. **`DoD-23`(2026-08-19)** — Coordinator 의 실제 재발급 정책 중 SUPERSEDED 부분을 구현했다 — `lease_store` 가 있고 요청 epoch 이 저장된 값보다 낮으면 연결을 끊는 대신 서명된 `RENEW_OUTCOME_SUPERSEDED` 로 응답한다(proto 자체가 "정상적인 failover 경합"이라 선언했던 상황을 실제로 그렇게 처리). 이번에도 구현을 코덱스 CLI(workspace-write)에 위임. 이 코드를 읽던 이 세션이 먼저 의심스러운 지점(SUPERSEDED 응답 후 `continue`)을 포착해 미리 알리지 않고 블라인드로 독립 검수를 돌려 교차 확인했다 — 독립 검수 1라운드(`p139`)가 정확히 같은 지점을 지적: `renew_rounds > 1` 이고 SUPERSEDED 가 마지막이 아닌 회차에서 발생하면 Coordinator 가 오지 않을 프레임을 기다리는 교착(오늘 Lease revoke 조각에 이어 **두 번째로** 나온 같은 부류의 결함, 기존 시나리오들이 우연히 이 조합을 피해가 안 드러났었다). `continue` 를 `break` 로 고치면서, Agent 가 즉시 종료하는 다른 outcome(QUARANTINED·MAX_DURATION_EXCEEDED)도 같은 위험이 **이전 조각들부터** 있었음을 확인해 공통으로 일반화했다(`p140`). 이 세션이 그 수정을 직접 되돌려 뮤테이션을 재현해(정확히 새 시나리오 32 에서 exit=1, 스트림 끊김) 코덱스의 자체 보고와 별개로 재확인했다. 2라운드(`p141`)에서 `ACCEPTED`. QUARANTINED 실제 트리거는 위험도/신뢰도 인프라 부재로 `TODO_VISION` V-11 로 등록만 함. **`DoD-24`(2026-08-19)** — 재접속(failover)의 첫 최소 조각("Active Lease process-restart rehydration")을 구현했다 — 설계 조사(코덱스, `p142`)가 먼저 "전체 재연결 프로토콜은 오늘 조각들보다 크다" 고 정직하게 판단하고, "프로세스 재시작 후 활성 Lease 복원" 만으로 범위를 좁혔다. 새 proto 메시지 없이, 테스트 전용 `--disconnect-after-ack` 로 ACK 직후 연결을 끊고, 완전히 새로운 프로세스 쌍이 같은 `--lease-db`/`--fence-db` 로 시작해 `CoordinatorLeaseStore::get_or_issue()`(이미 있던 인프라)로 저장된 Lease 를 복원한다 — CLI 의 틀린 `--fence-epoch` 보다 저장된 값이 우선함을 확인했다. 오늘 이미 두 번 나온 "한쪽은 끝났는데 다른 쪽은 계속 기다리는" 교착 패턴이 세 번째로 있는지 특히 의심하며 독립 검수(`p144`)를 요청했으나, 이번 설계는 애초에 그 위험 구조를 피했음을 확인(연결이 끊기면 Agent 는 무한 대기가 아니라 즉시 EOF 오류로 종료) — **1라운드 만에 `ACCEPTED`**. holder identity 충돌 검사는 새로 만든 게 아니라 기존 코드였음을 확인만 하고 테스트를 추가했다. revoke 상태는 재접속에서 여전히 보존 안 됨(`DoD-22` 한계 그대로), 자동 재접속 루프·`ResumeLeaseRequest`는 의도적으로 범위 밖. **`DoD-25`(2026-08-19)** — `DoD-24` 가 남긴 그 revoke 미보존 안전 공백을 닫았다 — `CoordinatorLeaseStore` 에 `revoked_at_unix_ms` 필드를 추가하고, 기존 SQLite 파일도 `open()` 시점에 `PRAGMA table_info`+`ALTER TABLE` 로 자동 보정한다. 설계 조사(코덱스 `p145`)가 먼저 스키마·`get_or_issue()`·`send_revoke_notice()` 를 실측해 마이그레이션이 필요함을 짚었고, 구현(`p146`, 코덱스 workspace-write)이 `mark_revoked()`(idempotent, wire 전송 **전에** 커밋 확정)와 `get_or_issue()`·갱신 경로(정상 경로 + `renew_outcome_override` 읽기 전용 경로) **양쪽 다**의 revoked 거부를 만들었다. 이번엔 구현자가 evidence·CLAUDE.md·HISTORY 를 안 건드려 구현자/검수자 경계가 더 깔끔했다. 독립 검수(`p147`)가 마이그레이션 로직·두 갱신 경로·override 시에도 실제 lease_id 로 기록되는지까지 전부 확인하고 **1라운드 만에 `ACCEPTED`**. **`DoD-26`(2026-08-20)** — `DoD-24` 가 이월한 "만료된 Lease 재접속 거부" 를 구현했다 — `get_or_issue()` 에 만료 검사를 추가했다. 독립 검수 1라운드(`p149`)가 진짜 계층 간 결함을 찾았다 — 만료 판정이 엄격한 `<` 를 써서, 이미 이 저장소가 정착시킨 "경계 포함"(`<=`) 규칙(`crates/protocol/src/signing.rs` 의 Lease 서명 검증, Agent 의 revoke 검사)과 어긋났다 — 정확히 만료 시각과 같은 순간에 Coordinator 는 재발급을 허용하는데 Agent 는 같은 Lease 를 즉시 거부하는 모순이 생길 뻔했다. `<` 를 `<=` 로 고치고 경계값 테스트 2건을 추가한 뒤(`p150`) 2라운드(`p151`)에서 `ACCEPTED`. **`DoD-27`(2026-08-20)** — `DoD-25` 가 명시적으로 남긴 공백("새 signed outcome 이나 proto 변경은 하지 않았다 — Agent 쪽에서 이 거부와 다른 종류의 handshake 실패를 구분할 신호가 없다")을 닫았다 — **갱신(renew) 경로에 한정해** `proto/lease.proto` 에 `RENEW_OUTCOME_REVOKED = 8` 을 순수 추가하고, Coordinator 가 revoked Lease 에 대한 갱신 요청을 raw error 로 연결을 끊는 대신 서명된 `RenewLeaseResult{ outcome: 8 }` 로 응답하도록(override 읽기 전용 경로·정상 갱신 경로 양쪽 다) 고쳤다. 오늘 이미 두 번(`DoD-22`·`DoD-23`) 나온 "한쪽은 끝났는데 다른 쪽은 계속 기다리는" 교착 패턴이 세 번째로 있는지 특히 의심하며 독립 검수를 요청했다 — 결과를 전송·flush 한 **뒤에** outcome 8 을 기존 교착 방지 `break` 목록(`2 | 3 | 6`)에 정확히 추가했는지, Agent(`crates/agent/src/lib.rs`)도 outcome 8 을 만나면 즉시 종료하는지가 핵심 확인 대상이었다. 독립 검수(`p157`)가 proto enum 순수 추가·양쪽 경로의 signed outcome 변환·`break` 위치(전송 후)·Agent 즉시 종료·신규 테스트 전용 플래그 `revoke_before_renew`(ACK 후 저장소만 revoke, notice 는 안 보냄 — 기존 `revoke_after_round` 경로와 독립)·시나리오 37 이 실제 wire 서명/replay/nonce 검증을 통과해야 성공함·`lease_store.rs` 는 이번 조각에서 전혀 안 바뀌었음(설계대로)·초기 Grant 발급 시점의 revoked 거부는 여전히 범위 밖(안 건드림)까지 전부 확인하고 **1라운드 만에 `ACCEPTED`**. `coordinator-agent-selftest` 37/37 시나리오, 5회 연속 통과(각 60초 하드 타임아웃). 뮤테이션 테스트로 outcome 8 `break` 제거 시 시나리오 37 실패 및 교착 재현을 확인. 초기 Grant 발급의 revoked 거부·`lease_store.rs` 자체 변경은 의도적으로 범위 밖. **`DoD-28`(2026-08-20)** — `DoD-20`(`check_schema.py` 신규 구현)이 남긴 "CI 파이프라인에 실제로 연결하지 않았다" 공백을 닫았다 — `.github/workflows/canonical-schema-check.yml` 을 신설해 `main` 대상 `push`·`pull_request` 에서 canonical 참조 self-test·벡터 대조·`check_schema.py`·워크스페이스 build/test(`gputeer-runtime-windows` 제외)·`verify_evidence.py` 를 순서대로 실행한다. 이 저장소는 원격이 없어 워크플로가 실제 GitHub Actions 에서 돈 적은 없다 — 로컬 명령 순서 실행 성공으로 검증을 대신했다. 독립 검수(`p159`)가 YAML 문법·트리거·경로 정확성(`--exclude gputeer-runtime-windows` 이름이 `crates/runtime-windows/Cargo.toml` 의 실제 package name 과 일치)·Rust 버전 일치·apt `protobuf-compiler` 설치의 실제 필요성(`check_schema.py` 가 Cargo 의 `protoc-bin-vendored` 와 별개로 PATH 의 `protoc` 를 직접 호출함을 코드로 확인)·최소 권한·미커밋 상태까지 전부 확인하고 **1라운드 만에 `ACCEPTED`**. **`DoD-29`(2026-08-20)** — `--lease-db` 없는 레거시 경로가 운영에서 실수로 켜지는 것을 막는 명시적 opt-in 플래그(`--i-understand-legacy-mode-is-unsafe`)를 추가했다. 독립 검수 1라운드(`p161`)가 실제 코드 지적 2건(시나리오 27~31 의 legacy opt-in 자동 적용 의도 불명확, 시나리오 38 의 Agent 쪽 미검증)을 찾아 전부 고쳤다(`p162`). 2·3라운드(`p163`·`p164`)가 코드 자체는 문제없음을 확인했으나 코덱스 read-only 샌드박스 안에서만 재현되는 selftest 미완료(Coordinator 만 스폰되고 Agent 서브프로세스는 안 뜸)를 보고해 반려됐다 — 감독자(claude-code)가 같은 바이너리를 샌드박스 밖에서 16회 연속 실행해(전부 exit=0, 38개 시나리오, 약 9초/회) 재현되지 않음을 확인하고, `DoD-28` 에서 이미 관측된 것과 같은 종류의 샌드박스 프로세스 스폰 제약으로 결론지어 최종 `ACCEPTED`. **`DoD-30`(2026-08-20)** — `crates/agent` 에 Job 실행을 향한 가장 작은 첫 걸음(설계는 `p154`)을 구현했다 — 유효 Grant/Lease 검증 성공 직후·`AgentGrantAck` 전송 전에 결정적 `checkpoint_id`(BLAKE3-256, 길이-프리픽스된 job_id/attempt_id/grant_id)로 `WRITING` 마커를 `write_once()` 로 기록한다. 위조/만료/revoked Lease 는 마커 미생성·ACK 미전송, 마커 생성 실패는 fail-closed, 재시도는 `write_once()` 의 기존 idempotent 동작에 의존한다. 실제 entrypoint 실행·manifest·wire 메시지·scheduler 는 전부 범위 밖 — `RESULT ok=true` 는 여전히 Job 완료가 아니다. 독립 검수(`p166`)가 실행 순서·checkpoint_id 인코딩·거부 경로·fail-closed·멱등성·범위 제한까지 전부 코드로 확인하고 **1라운드 만에 `ACCEPTED`**(검수 환경의 selftest 30초 제한은 `DoD-28`·`DoD-29` 와 같은 샌드박스 제약으로 판단, 감독자가 5회 연속 재확인). **`DoD-31`(2026-08-20)** — 오늘 밤 세 번(`DoD-22`·`DoD-23`·`DoD-27`) 나온 교착 버그 패턴에 대한 회귀 테스트를 `QUARANTINED`·`MAX_DURATION_EXCEEDED` 까지 확장했다(프로덕션 코드 불변, 순수 테스트 추가). 독립 검수 1라운드(`p169`)가 `CHANGES_REQUESTED` 를 냈으나, 이는 검수가 진행되던 시간대에 감독자(claude-code)가 코덱스의 뮤테이션 보고를 독립 재확인하려고 `matches!` 를 직접 순차 뮤테이션(outcome=3·6 각각 제거→재현→원복)하던 중간 상태를 검수가 우연히 읽은 오탐이었다 — 감독자가 두 뮤테이션 모두 정확히 예측된 교착으로 재현·원복까지 확인한 뒤 안정 상태에서 2라운드(`p170`)를 요청해 **`ACCEPTED`**. 프로세스 교훈: 독립 검수 진행 중 감독자의 직접 뮤테이션 재현은 순차 진행하기로 함. **`DoD-32`(2026-08-20)** — 설계 조사(`p171`)가 확인한 실제 안전 공백을 닫았다 — `renew_existing_within_duration()` 이 revoke·max-duration 만 검사하고 저장된 `expires_at_unix_ms` 가 이미 지났는지는 검사 안 한 채 즉시 새 만료시각으로 갱신했다(정상 Agent 도 지연·시계 어긋남으로 도달 가능). `DoD-26` 과 동일한 `<=` 경계 규칙으로 `expires_at_unix_ms <= now_unix_ms` 검사를 revoke 뒤·max-duration 전에 추가하고, 기존 `LeaseStoreError::Expired` 를 재사용해 raw error 로 거부한다(signed outcome 없음, `DoD-27` 과 같은 범위 판단). override·일반 갱신 경로 양쪽 적용, 기존 fixture 보정(판정 조건 유지), 경계 단위 테스트 2건·selftest 시나리오 47 신설. 독립 검수(`p173`)가 경계·순서·`UPDATE` 미실행·fixture 타당성·범위 제한까지 전부 코드로 확인하고 **1라운드 만에 `ACCEPTED`**, 감독자가 검수 완료 후 순차로 재확인(5회 연속 exit=0, 47개 시나리오). **`DoD-33`(2026-08-20)** — `DoD-30` 이 문서로만 주장했던 "marker-only 디렉터리는 GC 대상" 이라는 안전 불변식을 실제 회귀 테스트로 고정했다 — `crates/checkpoint/tests/durability_chaos.rs` 에 신설한 테스트가 `.durability.writing` 마커만 있는 디렉터리는 `startup_gc()` 후 삭제되고 완결된(manifest+데이터) 디렉터리는 보존됨을 파일시스템 상태로 확인한다. GC 알고리즘·Agent 코드는 전혀 안 바꿨다. 독립 검수(`p175`)가 `gc_partial()` 판정식과 Agent 의 실제 마커 생성 코드가 정확히 일치하는지, assert 가 실질적인지, 뮤테이션 논리까지 전부 코드로 확인하고 **1라운드 만에 `ACCEPTED`**, 감독자가 직접 재현해 재확인. 이로써 백로그 재조사(`p167`)가 찾은 3개 후보(교착 회귀·만료 갱신 fail-closed·marker-only GC) 전부 완료. **`DoD-34`(2026-08-20)** — 마지막 재조사(`p176`)가 확인한 유일 후보 — Agent 가 갱신 루프에서 `revoked` 만 확인하고 만료는 재확인 안 해 이미 만료된 Lease 로 갱신 요청을 보낼 수 있던 공백(`DoD-32` 가 Coordinator 쪽에서 이미 방어)을 Agent 쪽에서도 닫았다. `RenewLeaseRequest` 생성 직전에 새 `lease_is_expired()`(`<=` 경계, `DoD-26`/`DoD-32` 와 일관)로 재확인해 만료 시 요청을 안 보내고 `RENEW_REFUSED:LOCAL_EXPIRED` 로 종료. Coordinator 는 전혀 안 건드림. ★ 오늘 밤 세 번 나온 교착의 **반대 방향**(Agent 가 안 보내면 Coordinator 가 기다릴 위험)이 최우선 검증 대상이었다 — 구현자·독립 검수(`p178`)·감독자 3단계 모두 `read_frame()` 의 EOF 즉시 전파를 코드로 확인하고 selftest 5회(매회 약 16초, 90초 타임아웃 근처 안 감)로 실측 확인, **1라운드 만에 `ACCEPTED`**. 이로써 오늘 밤 백로그 재조사가 찾은 모든 후보를 마쳤다. **`DoD-35`(2026-08-20)** — 사용자가 기상 후 직접 지시해 자동 재접속 루프 로드맵(7조각·6~8일)의 "1+2 축소판"(proto 변경 없이 Agent bounded retry + Coordinator 반복 accept)을 구현했다. 독립 검수 1라운드(`p182`)가 진짜 결함 2건을 찾았다 — Agent/Coordinator 간 nonce attempt 카운터 불일치(TCP `connect()` 레벨 실패 후 재접속이 `GRANT_REJECTED` 로 실패)와 selftest 하드 타임아웃이 Coordinator 반복 accept 상황에서 무력화될 수 있음. 2라운드(`p183`→`p184`)가 두 결함의 프로덕션 수정 자체는 올바르다고 확인했으나 새 회귀 테스트가 실제 경로를 안 타 증명력이 없다고 지적, 3라운드(`p185`→`p186`)가 실제 TCP 연결 거부→`run()` 재시도→성공까지 타는 통합 테스트로 재작성해 뮤테이션으로 원래 버그 재현까지 확인하고 **`ACCEPTED`**. Windows `WSAEWOULDBLOCK`(10035) 플랫폼 버그도 발견해 고쳤다. `coordinator-agent-selftest` 48→52개 시나리오, 기존 48개는 전부 회귀 없음. Resume proto·durable request ledger·다중 Agent 경쟁은 로드맵 후속 조각(3~7)으로 명시적으로 남음. **`DoD-36`(2026-08-20)** — 로드맵 조각 3(Resume 프로토콜)을 구현했다 — `proto/lease.proto` 에 `SessionMode`·`AgentSessionHello`·`ResumeLeaseRequest`·`ResumeOutcome`·`ResumeLeaseResult` 를 순수 추가(기존 필드 번호 불변)하고 canonical/signing 체인 전체(domain 25→28)를 갱신했다. `classify_resume()`(읽기 전용, `get_or_issue()`/`renew_existing_within_duration()` 재사용 안 함)이 identity→revoke→만료(`<=`)→epoch 순으로 판정한다. Agent 는 `--resume-protocol` opt-in 시에만 새 경로를 쓰고 기본값은 기존 handshake 그대로 — 기존 52개 시나리오 전부 회귀 없음. 독립 검수 1라운드(`p189`)가 대부분 통과시키면서도 새 canonical 벡터가 Rust 쪽에서 대조 테스트가 없는 진짜 공백(`DoD-05` 와 같은 종류)을 찾아 반려, 수정 뒤 2라운드(`p191`)가 내용은 확인했으나 아직 커밋 전인 조각 전체의 `git diff` 범위를 오해해(`DoD-31` 과 같은 종류) 다시 반려, 3라운드(`p192`)에서 오해 해소 후 **`ACCEPTED`**. `coordinator-agent-selftest` 52→60개 시나리오. 로드맵 7조각 중 1~3 완료 — 남은 4(dispatcher 정교화)·5(durable ledger)·6(Agent Resume 통합)·7(다중 Agent selftest)은 후속 조각. **`DoD-37`(2026-08-20)** — 로드맵 조각 4(session dispatcher + transport 오류 격리)를 완료했다 — 인라인 Grant 처리를 `serve_one_connection()` 으로 추출하고 `CoordinatorSessionError{Transport, Protocol, Storage}` 로 오류를 분류(transport/protocol 은 로그 후 다음 accept, storage 는 fail-closed 즉시 종료). 독립 검수 1라운드(`p195`)가 Resume 경로(`classify_resume()`)의 SQLite 오류가 서명된 `UNAVAILABLE` 로 흡수돼 fail-closed 분기를 우회하는 진짜 안전 결함을 찾아 반려 — 감독자가 코드로 직접 재확인해 실재함을 확인. 수정(`p196`)이 `LeaseStoreError` 를 정책 판정(정상 서명 응답)과 진짜 저장소 장애(`Storage` fail-closed)로 명확히 구분해 닫았다. ★ 이 과정에서 `DoD-36` 이 기록한 시나리오 60(durable store 없이 Resume → 서명된 `UNAVAILABLE`)의 기대 동작이 "구성 오류로 보고 fail-closed 즉시 종료" 로 의도적으로 재정의됐다 — 독립 검수 2라운드(`p197`)가 이 변경 방향이 fail-closed 원칙과 일관됨을 확인하고 최종 `ACCEPTED`. `coordinator-agent-selftest` 60→64개 시나리오. 로드맵 7조각 중 1~4 완료 — 남은 5(durable request ledger)·6(Agent Resume 통합)·7(다중 Agent selftest)은 후속 조각. **`DoD-38`(2026-08-20)** — 로드맵 조각 6(Agent Resume 통합)을 완료했다. 설계 조사(`p198`)가 "부분 완료" — Agent 는 이미 실제로 Resume 요청을 보내고 재시도 여부(안전 동작)는 이미 올바르지만 outcome 별 명시적 구분이 없다 — 로 정직하게 판정했다. `RESUMED`/`UNAVAILABLE` 를 뺀 6개 terminal outcome 각각에 `RESUME_REFUSED:<OUTCOME>` 구분된 오류 문자열을 추가하고(`DoD-27` 의 `RENEW_REFUSED:REVOKED` 패턴 재사용), selftest 시나리오 53~59 가 이 문자열·연결 1회·`ReconnectExhausted` 미발생을 실제로 assert 하도록 보강했다. 독립 검수(`p200`)가 재시도 안전 동작(`Retryable`/`Fatal` 분류) 불변을 코드로 직접 추적해 확인하고 **1라운드 만에 `ACCEPTED`**. Coordinator·proto 는 전혀 안 건드렸다. `coordinator-agent-selftest` 64개 시나리오(개수 불변, 기존 시나리오 강화만). 로드맵 7조각 중 1·2·3·4·6 완료 — 남은 5(durable request ledger)·7(다중 Agent selftest, 조각 5 이후 유의미)은 후속 조각. **`DoD-39`(2026-08-20)** — 로드맵 조각 5(원안 "durable request ledger")를 완료했다. 설계 조사(`p201`)가 실제 공백은 "정확히 한 번 처리"가 아니라 가용성 공백임을 코드로 확인해 "Ambiguous Renew 의 durable Lease 상태 기반 복구"로 재범위했다 — Agent 가 `RenewLeaseRequest` 전송 뒤 결과를 못 받으면(`AmbiguousRenew`) 즉시 fatal 종료하던 것을, 새 게이트(`--recover-ambiguous-renew-from-durable-lease`)가 켜졌을 때만 bounded reconnect 후 기존 Grant/ACK 경로(`get_or_issue()`)로 최신 저장 Lease 를 재조회하고 새 nonce 로 새 Renew 를 보내도록 구현(`p202`). 독립 검수 1라운드(`p203`)가 진짜 안전 결함을 찾았다 — durable 복구 게이트가 Coordinator 의 실제 `--lease-db` 설정과 검증 가능하게 결합되지 않아, legacy Coordinator + 게이트 오조합 시 재접속이 "상태 재조회"가 아니라 "그 순간 새로 조작된 Lease 발급"이 되는 결함. 구현 2라운드(`p204`)가 `proto/job.proto` 의 `ExecutionGrant` 를 schema v2 로 승격해 서명 대상 필드 `lease_from_durable_store` 를 순수 추가, Coordinator 는 `lease_store.is_some()` 일 때만 true 로 서명, Agent 는 이 비트가 true 가 아니면 ACK·checkpoint·Renew 전에 fatal 거부하도록 근본 수정. 독립 검수 2라운드(`p205`)가 서명 결합·Coordinator 정직성·Agent 거부 순서·기존 경로 무회귀까지 확인하고 최종 `ACCEPTED`. `coordinator-agent-selftest` 71→72개 시나리오. **로드맵 7조각 중 1·2·3·4·5·6 완료** — 남은 7(다중 Agent selftest)만 후속 조각. **`DoD-40`(2026-08-20)** — 로드맵 조각 7(원안 "다중 Agent selftest")을 완료했다. 설계 조사(`p206`)가 진짜 "다중 Agent 동시 경쟁"은 지금 Coordinator 아키텍처(의도적 순차 처리, Agent identity/key 1개만 등록)로는 표현 자체가 안 되고 가능하게 하려면 최소 2~4일 아키텍처 변경이 필요하다고 정직하게 판정, 대신 검증 안 된 진짜 위험(`get_or_issue()` 의 `BEGIN IMMEDIATE` 동시성 안전성이 실측된 적 없음)을 새 통합 테스트로 좁혔다(구현 `p207`). 독립 검수 1라운드(`p208`)가 경쟁 후보가 `holder_node_id` 외 모든 필드가 같아 부분 덮어쓰기를 못 잡는 테스트 판별력 결함을 찾음, 구현 2라운드(`p209`)가 후보 8개 필드를 전부 구별되게 만들고 self-check 로 판별력을 직접 증명, 독립 검수 2라운드(`p210`)에서 `ACCEPTED`. 프로덕션 코드 무변경(순수 테스트 추가). **로드맵 조각 7 원안은 scheduler/다중 Agent 아키텍처 도입 단계로 명시적으로 이월** — 이 조각은 "Lease store 동시 최초 발급 안전성"으로 기록. **이로써 2026-08-20 자동 재접속 루프 로드맵(7조각) 작업을 마무리한다**. **`DoD-41`(2026-08-21)** — scheduler 9단계 로드맵의 조각 1인 순수 hard-filter kernel을 완료했다. 1차 독립 검수가 실제 보안 결함 2건(`isolation_class` 축 오류·빈 identity `MissingFact` 우회)을 찾아 `CHANGES_REQUESTED`, 2라운드에서 Restricted isolation 축과 빈 문자열 fail-closed로 근본 수정하고 회귀 테스트 5건·뮤테이션 2건으로 고정한 뒤 2차 검수 `ACCEPTED`. 감독자가 `cargo test -p gputeer-scheduler` 33/33을 직접 확인했다. Coordinator 연결·실제 자동 매칭은 없으며, **scheduler 로드맵 조각 1 완료 — 남은 8단계는 후속 조각**. **`DoD-42`(2026-08-21)** — scheduler 로드맵 조각 2 원안(3~5일 규모 durable Job/Attempt/Queue·Lease/Grant 결합·전체 ControlStore)을 하루에 완료했다고 과장하지 않고 **조각 2a: durable Job/Queue truth**로 축소했다. 신규 `CoordinatorJobStore`가 SQLite `BEGIN IMMEDIATE` read-check-write로 accepted submit의 멱등 저장, `SUBMITTED→PLANNING→QUEUED`, 결정적 queue 조회, deadline/queue-timeout/영구 불가능 실패를 보존한다. 자체 재검토로 규범에 없는 all-zero key 거부를 제거하고 상태별 전체 row-shape 손상 검사로 강화했다. 독립 검수가 transaction 경계·실제 Barrier 경쟁·멱등성·전이·경계·뮤테이션 2건·자체 수정 2건·범위를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 테스트를 직접 재확인했다. Attempt 생성은 `STAGING` 진입과 fence epoch·Lease 발급에 함께 묶어야 split authority를 피하므로 조각 2b로 이월했다. **scheduler 로드맵 9단계 중 조각 1·2a 완료 — 남은 durable Attempt/Lease 결합(2b)과 이후 7단계는 후속**. **`DoD-43`(2026-08-21)** — 조각 2b 전체를 완료했다고 과장하지 않고 **조각 2b-1: single-node local atomic STAGING kernel**로 제한했다. 신규 `CoordinatorStagingStore::stage_queued_with_lease()`가 한 `BEGIN IMMEDIATE` transaction에서 fence epoch 채번·Attempt/node/Lease 삽입·`QUEUED→STAGING` 전이·operation idempotency를 전부-or-none 처리한다. 자체 재검토로 renew/revoke 뒤 retry가 정상 가변 Lease 필드를 손상으로 오인하던 버그를 고쳐 최초 결과 반환과 불변 identity/epoch 대조를 분리하고, 공백 plan row-shape·조각 2a 이전 migration·부분 commit 경로를 보강했다. 독립 검수가 rollback epoch 미소비·기존 Lease API 무회귀·실제 Barrier 경쟁·뮤테이션 2건·프로덕션 4개 파일 범위·`staging_store.rs` 862줄의 계획 상한 360줄 초과 자기 보고를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 테스트를 직접 재확인했다. **scheduler 로드맵 9단계 중 조각 1·2a·2b-1 완료 — 남은 조각 2의 다중 노드 결합·Raft `COMMITTED`와 조각 3~9는 후속**. **`DoD-44`(2026-08-21)** — scheduler 로드맵 조각 3 전체를 끝냈다고 과장하지 않고 **조각 3a: durable Agent inventory 저장소 kernel**로 제한했다. 신규 `CoordinatorInventoryStore`가 복수 Agent registry를 충돌 방지·멱등 등록하고, 한 `BEGIN IMMEDIATE` transaction에서 parent/GPU/workload inventory를 원자 교체해 node ID 순의 기존 scheduler `PoolSnapshot`으로 결정 투영한다. 자체 재검토로 key를 `Vec<u8>`+명시적 32-byte 검사로 바꾸고 경쟁 후보 전 필드 판별력을 보강했으며 SQL/Rust 이중 정렬을 Rust sort 한 곳으로 통일했다. 독립 검수가 원자성·register 멱등/충돌·revision 뮤테이션·결정성·수정 5건·제한된 변경 범위·1,514줄 자기 보고를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 테스트 71건을 직접 재확인했다. **scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a 완료 — 남은 조각 3의 실제 다중 연결·heartbeat wire·session owner/fencing과 조각 4~9는 후속**. **`DoD-45`(2026-08-21)** — scheduler 로드맵 조각 4를 전체 placement/reservation으로 과장하지 않고 **순수 deterministic resource best-fit kernel**로 완료했다. 신규 `rank_best_fit()`이 hard-filter 적격 후보에서 가장 tight한 GPU 요구 개수를 선택하고, `BestFitPolicy`가 명시한 VRAM 잔여 합·GPU 수 잔여·CPU/RAM/workspace 잔여를 lexicographic 비교한 뒤 완전 동점은 `node_id` 오름차순으로 해소한다. 독립 검수 1라운드는 GPU vector 자체의 순열 동등성 검증 공백을 찾아 `CHANGES_REQUESTED`, reverse된 GPU vector의 `BestFitRanking` 전체 비교와 VRAM 정렬 제거 시 `node-b`/`node-a` winner 분기 뮤테이션으로 보강했다. 2라운드는 조각 전체 미커밋 diff의 `lib.rs`/`model.rs` 1차 산출물을 후속 수정으로 오인한 git-diff-scope 오탐이었고, HEAD가 DoD-44의 `1877760`임을 명확히 한 3라운드에서 최종 `ACCEPTED`. 감독자가 scheduler 테스트 48/48을 직접 재확인했다. Coordinator 배선·inventory revision/CAS·allocation·reservation·Grant는 후속이며, **scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4 완료 — 남은 조각 3 나머지·5~9는 후속** **`DoD-46`(2026-08-21)** — private `orchestrate` module이 `pool_snapshot()`→hard-filter→0/1/N 분기→N에서만 best-fit→durable staging을 조합하고 caller-supplied issuance 값을 사용한다. 독립 검수가 private module·test-only 유일 호출, 기존 `run()`/accept-loop의 별도 `issue_grant()` 유지, 0/1/N 분기·뮤테이션 2건·unchanged-inventory replay 의미와 범위를 확인해 **1라운드 만에 `ACCEPTED`**했고 감독자가 coordinator 테스트 78/78을 재확인했다. inventory revision/CAS reservation이 없어 서로 다른 Job의 같은 GPU 중복 선택을 막지 못하므로 production에는 연결하지 않았다. **scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4·5 완료(5는 production 미연결 kernel만) — 조각 3 나머지·inventory CAS reservation·실제 wire 연결·조각 6~9는 후속** **`DoD-47`(2026-08-21)** — `CandidateSnapshot.inventory_revision`을 inventory projection부터 선택까지 보존하고, 새 reservation-aware staging API가 operation replay→revision 비교→`node_id` PRIMARY KEY reservation→Attempt/Lease/fence→`QUEUED→STAGING`→operation 기록을 하나의 `BEGIN IMMEDIATE` transaction에서 원자 처리한다. CAS·점유 충돌은 자동 재시도 없이 즉시 실패한다. 독립 검수가 rollback fence 미소비·같은 transaction의 CAS/점유 강제·실제 `Barrier` 경쟁에서 정확히 1건 성공과 loser QUEUED·뮤테이션 2건·DoD-43 API 무회귀·범위를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 테스트 86/86을 직접 재확인했다. node-exclusive라 같은 node의 다른 GPU도 동시에 쓸 수 없고 release가 없으며 private orchestration은 production `run()`에 미연결이다. **scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4·5·5b 완료 — GPU별 allocation/release·조각 3 나머지·실제 wire 연결·조각 6~9는 후속** **`DoD-48`(2026-08-24)** — 설계 조사에서 실제 production 연결은 선택 GPU 식별자/Grant scope·원본 Manifest adapter·node/device/session routing의 남은 계약 3건과 Job submit ingress·durable outbox·reservation release가 모두 없어 하루 규모를 넘는다고 판정했다. 대신 순수 `resource_fit()`이 적격 GPU를 `(available_vram_bytes, gpu_id)` 순으로 요구 개수만 선택하고 canonical ID를 `ResourceFit`·`RankedCandidate`·`Staged` outcome에 보존하도록 구현한 선행 조각을 기록했다. 자체 재검토에서 부적격 GPU 제외 직접 증명 공백을 찾아 테스트를 추가했고, 독립 검수가 순수성·single/N 공용 helper·reverse 전체 동등성·뮤테이션 2건·STAGING 전 개수 재검증·범위를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 scheduler 53/coordinator 87 passed를 직접 재확인했다. 반환 ID는 scheduler snapshot 식별자일 뿐 NVML UUID provenance가 아니며 Grant/Lease scope·GPU별 reservation/release·production wire는 후속이다. **`DoD-49`(2026-08-24)** — `DoD-48`의 canonical `selected_gpu_ids`를 inventory CAS·node reservation·Attempt/Lease/fence·`QUEUED→STAGING`·operation idempotency와 같은 `BEGIN IMMEDIATE` transaction에 durable child rows로 결합했다. 자체 재검토에서 같은 ID가 다른 node에만 있는 fixture를 보강했고, 독립 검수가 node-scoped SQL·binding 직후 전체 rollback·손상 fail-closed·replay/`OperationConflict`·DoD-43 무회귀·Barrier 경쟁·single/N 전달·뮤테이션 2건을 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 92 passed를 직접 재확인했다. release는 실행 종료 증명 없이 구현하면 중복 실행 위험이 생겨 후순위이며 Grant/Lease scope·NVML UUID provenance·GPU별 capacity accounting·production wire도 후속이다. **`DoD-50`(2026-08-24)** — `submit_verified_manifest()`가 `&Verified<pb::JobManifest>`만 받아 서명 검증 뒤에만 Manifest identity를 관찰하고 accepted device/signer를 대조한다. caller hash는 canonical signing input에서 재계산해 대조·저장하며 Job/body/signer/idempotency는 한 `BEGIN IMMEDIATE` transaction에 묶인다. load는 authoritative key directory 재검증 전 사용할 수 없는 raw `StoredManifestBinding`이다. 자체 재검토가 legacy exact-error와 body device identity 손상 fixture를 보강했고 독립 검수는 원자성·rollback·replay/corruption·뮤테이션 2건·범위를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 99 passed를 직접 재확인했다. membership validity·authoritative device→member·`JobRequirements` projection·기본 `MIRRORED` 소비·Grant/Lease scope·`COMMITTED`·production wire는 후속이다. **`DoD-51`(2026-08-24)** — verified terminal `AttemptReport`를 signature 포함 body와 저장소 직접 계산 hash로 보존하고, 하나의 `BEGIN IMMEDIATE` 안에서 report와 durable Attempt·현재 reservation을 job/attempt·single node·verified signer·fence·owner로 5중 대조한다. reservation 부재/불일치와 non-terminal outcome은 row 없이 거부하고 exact replay는 전체 protobuf 의미와 signer가 같은 기존 first-write fact만 반환한다. load는 의도적으로 raw binding이며 재검증 없이 쓰는 production consumer가 없다. 독립 검수는 rollback·상태 무변경·corruption fail-closed·staging helper 무회귀까지 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 108 passed를 직접 재확인했다. Job/Attempt terminal 전이, artifact/runtime-stop guard, Lease revoke/reservation release와 production wire는 후속이다. **`DoD-52`(2026-08-24)** — 직전 조사들의 질문을 "앞으로 나갈 조각"에서 "선행 조건의 첫 슬라이스"로 바꿔 verified `CheckpointManifest` durable binding을 찾았다. `&Verified<pb::CheckpointManifest>` 전용 API가 write lock 뒤 같은 transaction에서 current Attempt/reservation의 job/attempt/producer/signer/fence/owner를 대조하고, BLAKE3-256/32-byte root와 signature 포함 complete body·저장소 계산 hash를 first-write fact로 저장한다. 자체 재검토에서 SHA-256 root 허용 결함을 찾아 수정하고 negative case로 고정했다. 독립 검수는 유일 API/INSERT·검증 순서·TOCTOU 부재·root 제한·replay/load·rollback/state 무변경·production guard 뮤테이션을 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 118 passed를 직접 재확인했다. `ReplicaAck` 저장과 `MIRRORED`/checkpoint durability 전이, control state 전이는 후속이다. |
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
    **그래도 여전히 최소 조각이다** — scheduler의 hard-filter·best-fit과
    durable Job/Queue·STAGING·inventory 저장소를 묶는 로컬 orchestration
    kernel까지 생겼지만, inventory revision/CAS reservation이 없어 같은 GPU의
    중복 선택을 막지 못하므로 private test fixture 외 production 경로에는
    연결하지 않았다. 실제 자동 매칭·wire dispatch 경로는 여전히 없으며,
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

키 보관 — Windows·Linux 둘 다 있지만 **막는 경계가 다르다**(2026-08-30)
  §11 K1 이 두 플랫폼에 다 생겼다. 그러나 같은 이름이 같은 보호를 뜻하지 않는다.

             막는다                              못 막는다
  Windows    같은 기계의 **다른 사용자**         같은 사용자, 관리자
  (DPAPI)
  Linux      같은 기계의 **비-root 사용자**      root
  (systemd-  키링 파일만 훔쳐 다른 기계에서
   creds      여는 것
   --with-
   key=host)

  ★ Linux 쪽은 /var/lib/systemd/credential.secret(0600 root)로 봉인한다.
    systemd 자신이 경고하듯, **그 파일이 암호화 안 된 디스크에 있으면
    디스크를 통째로 가져가는 것은 막지 못한다** — x600 실측에서 정확히
    그 경고가 나왔다. TPM 봉인(--with-key=tpm2)이 그것까지 막지만 그건
    K2 이고 미구현이다.

  ★ Linux K1 은 **Agent 가 root 여야 성립한다** — credential.secret 이
    0600 root 다. 비-root Agent 는 systemd-creds 가 설치돼 있어도 실패한다.
    결함이 아니라 이 등급의 조건이지만, "Linux 에 K1 이 있다" 가 "아무
    Agent 나 쓸 수 있다" 로 읽히면 안 된다.

  둘 다 공통: 복호된 뒤의 프로세스 메모리·크래시 덤프는 보호하지 않는다.
  Linux 는 평문이 커널 파이프와 systemd-creds 프로세스 메모리에도 한 번
  더 존재한다 — 명령줄·임시 파일 누출은 피했지만 "누출 없음" 은 아니다.
  systemd-creds 가 없는 Linux 는 조용히 K0 로 내려가지 않고 실패한다.

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

자원 상한은 **협조하는 작업**에만 상한이다(2026-08-30, `DoD-57`)
  Linux cgroup v2 로 memory 상한을 실제로 걸고 Agent 실행 경로에
  연결했다. 그러나 자식은 부모와 같은 권한으로 돌고, exec 뒤 자기
  pid 를 상위 `cgroup.procs` 에 써서 **실제로 빠져나간다** — 32MiB
  상한 밖에서 90MB 를 잡는 것을 테스트로 확인했다.

    막는다     실수로 메모리를 너무 먹는 작업, 소유자의 강제 종료
    못 막는다  빠져나가려고 작정한 코드

  Windows Job Object 도 같은 성격이다(소프트 제한, 호스트 보호 아님).
  닫으려면 cgroup namespace + 권한 강등이 필요하고 둘 다 미착수다.
  memory 만 건다 — CPU·PID·io·네트워크·파일시스템·VRAM 은 제한 안 한다.

Linux 검증 — 반복 가능해졌다(2026-08-30) / remote5090 는 여전히 임시(`ENV-03`)
  사용자가 임시로 빌려준 원격 기계(remote5090, Ubuntu 24.04 + RTX 5090)에서
  이 저장소가 처음으로 Linux 빌드·테스트를 통과했고(gputeer-runtime-windows
  제외 전부 ok, k1c 하나만 플랫폼 차이로 FAILED), sudo 없이 cgroup v2 로
  memory/CPU/PID/freezer 4종 강제를 확인했다. 그러나 이 기계는 사용자
  소유의 공유·비영구 기계라 "확보"가 아니라 "임시 접근"이다 — 반복
  가능한 접근성과 GPU VRAM 세분 할당(MPS) 검증은 여전히 없다.  -> D-3

  ★ 그러나 **x600 의 WSL2 가 동작하면서 반복 가능한 Linux 환경이
    생겼다**(2026-08-30). 워크스페이스 693 passed / 실패 0 을 그 위에서
    확인했고, cgroup·systemd-creds 실측도 거기서 했다. GPU 는 WSL 에
    노출되지 않으므로 GPU 관련 Linux 검증은 여전히 없다.
```

### 다음에 할 일

★★ **여기 있던 1733줄을 [`docs/history/조각_이력.md`](docs/history/조각_이력.md)
  로 옮겼다**(2026-09-05). 1~55번이 거의 다 **완료 기록**이라 절 제목이
  사실과 달랐고, 그 분량이 이 파일의 80% 를 차지해 **매 세션 자동으로
  읽히는 규칙 문서를 규칙이 아닌 것이 뒤덮고** 있었다. 본문은 한 글자도
  안 고치고 그대로 옮겼다.

  아래는 **진짜로 남은 일**이다. 끝난 것은 여기 안 적는다.

#### 1. 검수만 기다리는 것 — 코드는 끝났다

```text
독립 검수 7건   실행 사슬 6조각 + DoD-68 evidence 확정
                복구 2026-09-07 15:43. ★ 반드시 **순차로** 돌린다 —
                병렬 3갈래가 판정 전에 쿼터를 태운 것이 이번 실패다
                초안: docs/plans/2026-09-03_1740_DoD-68_evidence_초안_검수대기.md
```

★ **검수 전까지 이 사슬 위에 새 기능을 쌓지 않는다.** 쌓으면 나중에 어느
  층이 틀렸는지 가려내기 어려워진다.

#### 2. 사용자만 할 수 있는 것

```text
OS 방화벽            ★ 2026-09-05 사용자가 직접 실측 — **경로 단위 차단은 된다**
                       (before=200 -> after=000, 규칙 삭제까지 확인).
                       docs/evidence/_raw/방화벽_경로단위_차단_실측.txt
                     즉 OS 백엔드가 **원리적으로 가능함은 확인됐다.**
                     아직 못 잰 것 넷 — 그 파일에 적었다:
                       1. 프로세스 **한 건**만 막기(program= 은 경로 단위다.
                          python.exe 를 통째로 막으면 소유자 작업이 끊긴다,
                          §0.1) -> 전용 계정/AppContainer SID, 즉 P0-02 동반
                       2. 규칙이 자식보다 **먼저** 있어야 하는 순서
                          (CREATE_SUSPENDED · cgroup pre_exec 과 같은 문제)
                       3. 이름 기반 허용 목록(규칙은 IP 단위 — mediated_dns)
                       4. Agent 가 죽었을 때 규칙 정리 보장
                     ★ 규칙 추가·삭제 자체는 여전히 이 세션이 못 한다 —
                       시스템/보안 설정 변경은 금지 카테고리다
P0-02 AppContainer   ★ 2026-09-05 x600 실물 GPU 로 1차 실측 —
                       docs/evidence/_raw/P0-02_appcontainer_cuda_1차.txt
                     된 것: 프로파일 생성 · 고유 SID · 컨테이너 안 실행 ·
                       **가둠 확인**(호스트 파일을 못 읽는다) ·
                       바깥 기준선(cuda True, matmul 계산까지 ok)
                     ★★ **CUDA 가 되는지는 아직 모른다** — 더 앞에서
                       막혔다: 0xC0000135 STATUS_DLL_NOT_FOUND.
                       CUDA 가 아니라 **Python 이 자기 DLL 을 못 읽어
                       시작조차 못 한 것**이다. 종료 코드만 보고
                       "AppContainer 에서 CUDA 실패" 로 적었으면 거짓을
                       기록할 뻔했다.
                     다음: Python 설치·site-packages 폴더에 컨테이너 SID
                       읽기·실행 권한(= P0-02 의 filesystem allowlist).
                       ★ 사용자 소유 디렉터리의 ACL 변경이라 이 세션이
                         실행하지 않는다
ADR-026 · ADR-027    구현은 이미 그 결정을 따른다. 기준선 수정 승인만 남음
```

#### 3. 규범 결정이 먼저인 것 — 코드가 아니라 판단이 막고 있다

```text
멤버십(Stage 2·11)   사용자 결정 4건(root key rotation · 권한 주체 ·
                     상태 전이 채택 · mutation TTL) + 과반 합의(COMMITTED)
                     부재 -> 다중 노드/Raft 가 선행
TLS(Stage 2·16)      인증서 신원 방식 미정(기준선 §42.7.3).
                     지금도 Ed25519 서명·replay 방어가 있으므로 TLS 가
                     더하는 것은 **기밀성**이다 — 필요하지만, 신원 체계를
                     둘로 만들면 어긋나는 것이 진짜 위험이다
예약 <-> 후보 선택    A/B/C 택일. 얻고 잃는 것은
                     docs/plans/2026-09-03_1830_예약과_후보선택_공백_조사.md
```

#### 4. ★★ 진짜 뿌리 — 이것부터 풀려야 나머지가 도미노로 풀린다

```text
④ 데몬화(루프)
  └ 후보 선택이 예약을 안다        <- 3 의 규범 결정
      └ 예약 해제                   <- 종료 증명이 없어 막힘
          └ Agent 가 실제로 실행     <- 미착수 · 진짜 뿌리
```

`reservation_release.rs:37` 이 스스로 적어 뒀다 — **"아직 증명 못 함" 이라
이 API 는 오늘 아무 예약도 풀지 못한다.** `DoD-62` 가 일부러 그렇게
만들었다(실행 종료 증명 없이 풀면 중복 실행 위험).

그리고 `agent/lib.rs:344` — **"이 stub 은 entrypoint 를 실행하지 않는다."**
프로세스를 안 띄우니 **끝났다는 증명도 만들 수 없다.**

★ 즉 "데몬화" 는 스케줄러 문제가 아니라 **실행 문제**다. 루프는 열 줄이고,
  지금 붙이면 큐 맨 앞에서 멈추는 것을 보게 된다 — 버그가 아니라 아래층이
  비어 있다는 신호다.

★ Agent 실행은 **하루 규모를 넘는다** — runtime 격리·GPU 확인·결과 보고가
  딸려 온다.

#### 5. 그 뒤에 남는 큰 덩어리

```text
다중 노드 / Raft COMMITTED          멤버십의 선행 조건이기도 하다
Grant/Lease full ResourceScope      GPU 할당/해제 · MPS · MIG
Job ingress · routing · outbox      wire 경로
UI (Owner Panel 확장)
TODO_VISION V-05~V-12 (8건)         트리거 없음 — 자동 매칭 · QUARANTINED 판정 등
```


### 환경 주의사항

- **Rust 1.97.1** 설치됨 (로컬 · x600 · remote5090 전부). `protoc` 는 `protoc-bin-vendored` 로 번들.
- 개발 기계는 Windows 11, GPU 없음(Intel Iris Xe).
  GPU 검증은 **x600**(RTX 4070 SUPER · driver 595.79 · CUDA 13.2, Windows). 작업 디스크 **F:**.
- **x600 의 WSL2 가 이제 동작한다**(2026-08-30). 커널 6.18.33.2, systemd 259 가
  PID 1, cgroup v2 단일 계층, `systemd-creds` 사용 가능. Rust 1.89 설치돼 있다
  (`/root/.cargo`). WSL 버전 2.7.12.0, 배포판은 `Ubuntu` 하나다.

- ★★ **x600 의 C: 에서는 WSL·Docker 작업을 하지 않는다**(2026-08-31,
  사용자 지시). Docker 컨테이너 생성도 `wsl` 작업도 **사용자가 명령하기
  전까지** 하지 않는다.

  WSL 배포판 전체가 **E: 로 이전됐다**(사용자가 직접 수행) — 등록 BasePath
  는 `\\?\E:`, 파일은 `E:\WSL-Ubuntu.vhdx`(41.63GB)이고 C: 에 잔재는
  없다. 이전 후 실측 여유: **C: 51.7GB · D: 11.1GB · E: 93.7GB · F: 81GB**
  ★ **Linux 빌드는 이제 `/mnt/e/gputeer-work/build/gputeer` 에서 한다**
    (2026-08-31, 사용자 지시로 F: 에서 이전). 소스만 옮겼고(28M, `target/`
    제외) 거기서 `cargo test -p gputeer-coordinator --test
    neighbor_report_store` **33 passed** 로 동작을 확인했다. 위 1844줄의
    `/mnt/f/...` 는 그 이전 기록이다.
  (이전 전 C: 는 9.3GB 까지 내려가 있었다).

  ★ 그 VHDX 가 41.6GB 였던 이유는 **우리 빌드가 아니다** — 안쪽 39G 중
    `/var/lib/containerd` 20G + `/var/lib/docker` 12G 로, 사용자의 **살아
    있는** Docker 환경이다(실행 중 컨테이너 4개·사용 중 볼륨 3개).
    **건드리지 않는다.** 껍데기만 큰 게 아니라 실제로 차 있어서 압축으로는
    2.6GB 밖에 못 되찾는다 — "안은 비었는데 껍데기만 크다" 는 추측이
    실측으로 반증된 사례다.
- `blake3` Python 패키지 설치 확인됨 — **개발 기계만**.
  ★ **x600 의 WSL python3(3.14.4)에는 없다**(2026-08-31 확인). 그래서
  `reference_canonical.py --verify` 를 x600 에서 돌리면 다이제스트를
  만들지 못해 **모든** 벡터가 불일치로 보고된다(신규분만이 아니라 v01
  부터 전부) — 코드 결함으로 오해하기 쉽다. Rust 테스트는 x600 에서
  정상이므로, **벡터 대조는 개발 기계에서만** 한다. 설치하려면 사용자가
  직접 `pip install blake3` 해야 한다.
- **Linux 검증 — 부분 해소(2026-08-19, `ENV-03`).** 사용자 소유의 원격 기계
  **remote5090**(Ubuntu 24.04.3, RTX 5090 32GB, sudo 불가)를 임시로 빌려 이
  저장소를 처음으로 Linux 에서 빌드·테스트했다. **하지만 이 기계는
  "확보"가 아니라 "임시 접근"이다** — 사용자 소유의 공유·비영구
  기계이고, 다른 사용자·서비스가 이미 돌고 있다. 반복 가능한 접근성이
  보장되지 않으므로, Linux 대상 DoD 를 이 기계 하나에 의존해 정기적으로
  검증할 수는 없다 — 필요할 때마다 접근 가능 여부를 다시 확인해야 한다.
  `ENV-03_remote5090_리눅스_GPU_기계_실측.md` 참조.

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

# Linux 전용 코드 교차 타입 검사 (이 개발기는 Windows 다)
#   crates/checkpoint/src/platform.rs 의 openat2 모듈은
#   #[cfg(target_os = "linux")] 이라 Windows 기본 빌드에서
#   컴파일되지 않는다 — 이 명령만이 그것을 검사한다.
#   워크스페이스 전체는 불가하다(libsqlite3-sys 가 Linux용 C
#   크로스 컴파일러를 요구). 통과는 "컴파일된다" 일 뿐
#   "동작한다" 가 아니다 — 실측은 여전히 Linux 기계가 필요하다.
cargo check -p gputeer-checkpoint --all-targets --target x86_64-unknown-linux-gnu
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
