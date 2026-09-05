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

```text
1. 네트워크 전송 · coordinator 골격             ★ 핸드셰이크+Lease 최소 조각+Lease 갱신 최소 조각 전부 완료 + evidence 정식 기록 완료(2026-08-19)
   docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md
   docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md
   docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md
   `gputeer coordinator-agent-selftest` 가 별도 PID 2개(coordinator-stub·
   agent-stub)로 실제 handshake 에 성공하고, 이제 **16개 시나리오**
   (정상 1 + 핸드셰이크/Lease 거부 5 + 갱신 정상 1 + 갱신 거부 9)를
   프로세스 경계에서 자동 검증한다(5회+ 연속 확인, 뮤테이션 테스트로
   비공허성 증명 — 2026-08-19 오늘 재현). `docs/evidence/DoD-11_coordinator_agent_핸드셰이크.md`·
   `DoD-12_coordinator_agent_lease_최소_조각.md`·
   `DoD-13_coordinator_agent_lease_갱신_최소_조각.md` 로 schema v2
   정식 기록 완료(각 2~3라운드 재검수 끝에 `ACCEPTED`).

   **Lease 갱신 최소 조각**(2026-08-19) — 설계가 찾은 공백대로
   `RenewLeaseResult` 를 새 서명 대상 메시지로 승격(domain_tag
   `gputeer/v1/lease-renew-result`, Python 참조 구현 교차검증 포함)
   하고, 같은 TCP 연결에 `RenewLeaseRequest`/`RenewLeaseResult` 왕복을
   추가했다. Agent 는 결과 서명·`request_nonce` echo·nested 새 Lease
   독립 서명·epoch 단조성(낮은 epoch 거부 + 높은 epoch 도 이 조각
   범위에서는 명시적 정책 거부)을 전부 확인한 뒤에만 보유 Lease 를
   교체한다. ★ 코덱스 1라운드 검수(`p99`)가 완료 보고 전에 **실제
   설계 결함 2건**을 잡아냈다 — Coordinator 가 요청의 `fence_epoch`
   을 검증하지 않던 문제, `FenceWatermark` 가 `<` 만 거부하고 `>`
   (epoch 상승)는 통과시키는데 계획서는 상승을 이 조각에서 정책상
   거부하라고 명시했던 문제(watermark 하나만으로는 그 계약을 강제
   못 한다). 둘 다 코드로 고치고(Coordinator 에 요청 epoch 대조,
   Agent 에 epoch 상승 명시적 거부 추가) 2라운드 좁은 후속
   검수(`p100`)에서 `ACCEPTED`. 신규 검증 게이트 4건을 뮤테이션
   테스트로 비공허성 확인. "완전한 coordinator" 아님 — 반복 갱신·
   재접속(failover)·durable `FenceWatermark`·Coordinator 의 실제
   Lease 재발급 정책·영속 저장소·스케줄링·다중 Agent·운영용 key
   protection·TLS 는 여전히 범위 밖(각각 새 계획 문서 필요).

   **이후 순서대로 완료**(2026-08-19, 각각 `docs/plans/` 계획 문서 +
   `docs/evidence/` schema v2 기록): Agent 쪽 durable `FenceWatermark`
   (`DoD-14`) → 같은 연결 반복 갱신(`DoD-15`, 코덱스 1라운드 만에
   `ACCEPTED`) → Coordinator 영속 Lease 저장소(`DoD-16`,
   `CoordinatorLeaseStore`) → `RevokeLeaseNotice` framed ingress
   커버리지(`DoD-17`, 오래된 테스트 공백을 코덱스 감사 `p110` 이
   발견) → `max_total_duration_seconds` 갱신 차단(`DoD-18`, 코덱스
   1라운드가 `lease_store=Some`+override 조합의 저장소 상태 불일치를
   찾아내 수정). `coordinator-agent-selftest` 는 이제 **24개
   시나리오**를 5회 연속 통과한다. 여전히 범위 밖: 새 `lease_id`
   재발급 정책(`SUPERSEDED`/`QUARANTINED` 를 언제 내릴지)·Lease
   revoke·스케줄링·다중 Agent·다중 Coordinator HA·TLS.
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
5. v1 evidence — ★ **v1→v2 승격 사이클 완료**(2026-08-19) — DoD-01~08·P0-01·P0-03·P0-03a·P0-07·P0-08 총 13건 전부 schema v2 승격 + 독립 검수 ACCEPTED
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
   필요하다**(현재 18GB 여유로 안정적, 당장 급하지는 않다). `P0-03`
   승격 중에는 `kill_chaos.rs` 의 카오스 메커니즘이 이 evidence 를
   쓴 뒤 **시간 기반에서 이벤트 기반으로 리팩터**됐다는 것을
   발견했다 — 원본 raw_output 의 "PARTIAL 7/8(88%)" 통계는 지금
   구현이 재현하는 수치가 아니다(결함은 아니고, 다른 세션이 카오스
   테스트의 부하 아래 재개 지점 유실 문제를 잡으려고 바꾼 것).
   addendum 으로 정밀화했다. `P0-03a` 는 로컬에서 재실행 가능한
   순수 파일시스템 조사라 실제로 다시 돌렸다 — 다만 원본 기본값
   (`--iterations 3000`, 약 1만회 파일 연산)은 디스크 사고를
   감안해 축소 규모(`--iterations 300`)로, 정확한 카운트가 아니라
   정성적 패턴 재현을 목적으로 재실행했다. 1라운드 만에
   `ACCEPTED`. `P0-07` 은 이미 이 세션 안에서 두 번 addendum
   시퀀스(raw_output 수치 불일치 → status PASS→INCONCLUSIVE 정정
   → 실제 x600 SSH 재실측 → claim 재확인 → status 다시 PASS)를
   거쳤으므로, 그 기존 재실측을 근거로 재사용하고 문서 전체를
   세 번째로 재검수만 시켰다 — 1라운드 만에 `ACCEPTED`.
   `P0-08`(이 저장소의 마지막 v1 evidence)도 완료 — prost 버전
   (0.13→실제 0.14.4)·schema fingerprint(66/389→실제 67/398, 이
   세션 중 `AgentGrantAck` 등 추가로 자연 성장) 재정정, 1차
   `CHANGES_REQUESTED` → 정정 → `ACCEPTED`. ★ **독립 검수 없는
   P0/DoD PASS 부채가 13→...→1→0건이 됐다** — `verify_evidence.py`
   가 이제 그 목록 자체를 출력하지 않는다. 남은 v1 은
   `ENV-01·02`(review 비강제)와 `P0-06`(`status: FAIL-SCOPE`, 애초에
   §7.3 대상 아님)뿐이다. 진짜 코드 결함 3건(`DoD-02` domain
   coverage 테스트·`DoD-06` `all_domain_tags_are_distinct`·`DoD-08`
   `write_once` 경쟁 분기)을 이 과정에서 찾아 고쳤다.
   ★ **디스크 여유가 다시 줄었다**(18GB→8.7GB, 97% 사용) —
   `cargo clean` 으로 1.4GiB 추가 정리했으나 repo 밖 근본 원인은
   세션 범위 밖이다. 이후 무거운 빌드/테스트는 자제하고 디스크를
   계속 관찰한다.
6. `AgentGrantAck` 의 Python 참조 구현 교차검증 공백           ★ 완료(2026-08-19)
   `tools/canonical/reference_canonical.py` 의 `SCHEMAS`·`DOMAIN_TAGS`
   에 `AgentGrantAck` 를 추가하고 벡터 2건(`v32_agent_grant_ack`·
   `v32b_agent_grant_ack_different_nonce`, nonce 가 canonical 에
   반영됨을 확인하는 대조쌍)을 생성해 `tests/vectors/canonical_v1.json`
   에 편입했다(40→42건). `crates/protocol/tests/t1_signing_targets.rs::agent_grant_ack_matches_reference`
   가 Rust 인코딩을 그 벡터와 바이트 단위로 대조 — **통과**, Rust와
   Python 참조 구현이 `AgentGrantAck` 에서도 일치함을 처음으로
   확인했다(코드 결함 없었음이 확인됨).
   부수적으로 `crates/checkpoint/tests/codex_findings.rs::k1c_concurrent_same_name_writers_are_not_actually_safe`
   (P0-08 승격 때 추가한 결함 고정 테스트)가 단일 라운드 진짜
   스레드 경쟁에 의존해 시스템 부하가 높을 때(디스크 여유 부족 등)
   가끔 `ok_true==1` 로 우연히 실패하는 것을 발견 — 10라운드 중
   한 번이라도 경합이 관측되면 통과하도록 고쳐 안정화했다(경합
   자체가 사라진 게 아니라 재현 신뢰도만 올린 것). ★ 이 결함 고정
   테스트 자체는 7번 항목에서 완전히 재설계됐다 — 더 이상 타이밍에
   기대지 않는다.
7. `write_once()` 동시 호출 계약                              ★ **완료**(2026-08-19, `DoD-21`)
   `docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md`.
   `DoD-08` 이 찾았으나 안 고치고 "결함 고정 테스트" 로만 등록해뒀던
   `write_once()` 의 동시 동일-이름 호출 결함을, 정책 A(동시 호출
   미지원 + 명시적 거부)로 확정하고 `std::fs::File::try_lock()`
   기반 파일 잠금으로 강제했다. 코덱스 독립 검수 **5라운드**
   (`p128`~`p132`)에서 실제 결함 5건(MSRV 불일치·비결정적 테스트
   주장·GC 죽은 락 영구 보존 회귀·이름공간 충돌 2단계)을 순차로
   찾아내 전부 코드로 고쳤고, 5라운드에서 `ACCEPTED`. `k1c` 를
   결정론적 테스트로 재설계하고 `k1d`·`k1e`·`k1f`·`k4c` 4건을
   신설, 뮤테이션 테스트 6건으로 비공허성 확인. GC 대 활성 writer
   의 디렉터리 단위 경쟁은 의도적으로 범위 밖(다중 Agent 실행
   시작이 재검토 트리거).
8. Lease revoke 최소 경로                                     ★ **완료**(2026-08-19, `DoD-22`)
   `docs/plans/2026-08-19_1517_lease_revoke_최소_조각_v1.md`.
   `RevokeLeaseNotice`(서명 대상·`framed_ingress` dispatch 는
   `DoD-17` 이 이미 갖춰뒀다)의 실제 Coordinator/Agent 업무 로직을
   구현했다 — Coordinator 가 `--revoke-after-round` 로 이미 발급한
   Lease 를 대상으로 서명해 같은 연결로 보내고, Agent 가 서명·
   `lease_id`·`fence_epoch`·만료 여부를 검증한 뒤 보유 Lease 를
   revoked 로 표시해 이후 갱신 요청을 만들지 않는다. ★ 사용자
   요청("코덱스 cli 에 5.6 솔로 작업")에 따라 **구현 자체를 코덱스
   CLI(workspace-write 샌드박스)에 위임**하고, 이 세션은 독립
   재검증(빌드·테스트·`coordinator-agent-selftest` 직접 재실행)과
   대화 기록을 공유하지 않는 새 코덱스 인스턴스의 독립 검수
   (read-only)만 맡았다 — 구현자와 검수자가 달라야 한다는 원칙
   (ADR-030)을 구현 자체를 외주 준 상황에서도 지키기 위해서다.
   독립 검수 1라운드(`p134`)가 진짜 교착 결함 1건(`--revoke-after-round
   0` + Coordinator/Agent 둘 다 `do_renew=true` 조합에서
   Coordinator 가 오지 않을 `RenewLeaseRequest` 를 기다림 — 기존
   selftest 시나리오들이 이 정확한 조합을 우연히 피해가서 안
   드러났던 진짜 결함)을 포함해 4건을 찾아 전부 코드로 고쳤다
   (`p135`). 이 세션이 코덱스의 자체 뮤테이션 보고와 별개로 교착
   방지 가드를 직접 되돌려 재현해(정확히 시나리오 25 에서
   `exit=1`) 결함의 실재를 독립적으로 재확인했다. 2라운드(`p136`)
   는 계획 문서 개정 이력 누락만 지적, 문서 보강 뒤 3라운드
   (`p137`)에서 `ACCEPTED`. `coordinator-agent-selftest` 시나리오
   25~29 신설(29개 시나리오 전부), 뮤테이션 테스트 3건으로
   비공허성 확인. 재접속 시 revoke 유실·비동기 이벤트 전송·실제
   정책 엔진(`SUPERSEDED`/`QUARANTINED` 판단)·다중 Agent·HA·TLS 는
   의도적으로 범위 밖.
9. Coordinator 실제 재발급 정책 — SUPERSEDED 부분                ★ **완료**(2026-08-19, `DoD-23`)
   `docs/plans/2026-08-19_1725_lease_재발급_정책_superseded_v1.md`.
   `proto/lease.proto` 의 `RENEW_OUTCOME_SUPERSEDED` 는 "낮은 epoch
   는 정상적인 failover 경합" 이라고 이미 선언했는데, 코드는 지금까지
   `fence_epoch` 불일치를 방향과 무관하게 전부 raw error 로 연결을
   끊었다 — proto 가 "정상" 이라 부른 상황을 코드는 "오류" 로 다뤘다.
   이제 영속 저장소(`lease_store.is_some()`)가 있고 요청 epoch 이
   저장된 값보다 **낮으면** 연결을 끊는 대신 서명된
   `RenewLeaseResult{ outcome: SUPERSEDED }` 로 정상 응답한다 —
   레거시(`lease_store=None`)와 더 높은 epoch 는 기존 hard error
   유지. 이번에도 구현을 코덱스 CLI(workspace-write)에 위임. 이
   코드를 검토하던 이 세션이 먼저 의심스러운 지점(SUPERSEDED 응답
   뒤 `continue`)을 포착해, 미리 알리지 않고 블라인드로 독립 검수를
   돌려 교차 확인했다 — 독립 검수 1라운드(`p139`)가 정확히 같은
   지점을 지적했다: `renew_rounds > 1` 이고 SUPERSEDED 가 마지막이
   아닌 회차에서 발생하면, Agent 는 outcome=2 를 받는 즉시 `Err` 로
   종료해 다음 요청을 안 보내는데 Coordinator 는 `continue` 로 계속
   기다려 교착이 생긴다 — **오늘 Lease revoke 조각(8번)에 이어 두
   번째로 나온 같은 부류의 결함**, 기존 시나리오들이 우연히 이
   조합을 피해가 안 드러났었다. `continue` 를 `break` 로 고치면서,
   Agent 가 즉시 종료하는 다른 outcome(QUARANTINED=3·
   MAX_DURATION_EXCEEDED=6)도 같은 위험이 **이전 조각들(`DoD-13`·
   `DoD-18`)부터 잠재해 있었을 수 있음**을 확인해 세 outcome 전부
   공통으로 처리하도록 일반화했다(`p140`). 이 세션이 그 수정을
   직접 되돌려 뮤테이션을 재현해(정확히 새 시나리오 32 에서
   `exit=1`, "스트림이 끊겼다" 오류) 코덱스의 자체 보고와 별개로
   결함의 실재를 재확인했다. 2라운드(`p141`)에서 `ACCEPTED`.
   `coordinator-agent-selftest` 시나리오 25·26·32 신설(32개 시나리오
   전부), 뮤테이션 테스트 2건으로 비공허성 확인. QUARANTINED 실제
   트리거(위험도/신뢰도 판정)는 인프라가 아직 없어 `docs/vision/TODO_VISION.md`
   V-11 로 등록만 하고 미뤘다. 요청 epoch 이 더 높은 경우의 정책과
   새 `lease_id` 재발급은 여전히 범위 밖.
10. 재접속(failover) 최소 조각 — 프로세스 재시작 복원         ★ **완료**(2026-08-19, `DoD-24`)
    `docs/plans/2026-08-19_1814_lease_재접속_최소_조각_v1.md`.
    설계 조사(코덱스 `p142`, read-only)가 먼저 "완전한 재연결
    프로토콜(자동 재접속 루프·`ResumeLeaseRequest`·revoke/만료/다중
    Agent 경쟁까지 포함)은 오늘 조각들보다 크다" 고 정직하게
    판단하고, "프로세스 재시작 후 활성 Lease 복원" 만으로 첫 걸음을
    좁혔다 — `CoordinatorLeaseStore::get_or_issue()` 가 이미 같은
    `lease_id` 재발급 요청에 저장된 레코드를 그대로 돌려주는 걸
    실측으로 확인했고, `DurableFenceWatermark`(Agent 쪽)도 이미
    재시작을 넘는 fencing 방어를 하고 있어 "절반은 이미 풀려 있다"
    고 판단했다. 새 proto 메시지는 만들지 않았다. 구현(`p143`,
    코덱스 workspace-write)은 테스트 전용 `--disconnect-after-ack`
    로 Coordinator 가 ACK 검증 직후 연결을 끊게 하고, 완전히
    새로운 프로세스 쌍이 같은 `--lease-db`/`--fence-db` 로 시작해
    CLI 의 틀린 `--fence-epoch` 보다 저장된 값이 우선해 Grant/ACK
    가 성공하는지 확인하는 시나리오 33, 같은 `lease_id` 를 다른
    `holder_node_id` 로 재접속 주장하면 거부되는지 확인하는 시나리오
    34(이 검사는 새로 만든 게 아니라 이미 있던 코드임을 코드
    조사로 확인만 하고 테스트를 추가했다)를 신설했다. 오늘 이미
    두 번(8번·9번 항목) "한쪽은 끝났는데 다른 쪽은 계속 기다리는"
    교착 버그가 나왔던 걸 감안해, 이번에도 같은 패턴이 있는지 특히
    의심하며 독립 검수(`p144`)를 요청했으나, 이번 설계는 애초에 그
    위험 구조 자체를 피했음을 확인했다 — Coordinator 가 연결을
    끊으면 Agent 의 쓰기/읽기는 무한 대기가 아니라 즉시 EOF 오류로
    끝난다(소켓 타임아웃도 걸려 있다). **1라운드 만에 `ACCEPTED`.**
    `coordinator-agent-selftest` 34/34 시나리오, 5회 연속 통과(각
    60초 하드 타임아웃). revoke 상태는 재접속에서 여전히 보존되지
    않는다(`DoD-22` 한계 그대로 유지) — 자동 재접속 루프·
    `ResumeLeaseRequest`·reconnect token·만료 Lease 복원 거부·다중
    Agent 경쟁은 다음 조각으로 명시적으로 미뤘다.
11. Coordinator Lease revoke 영속화                            ★ **완료**(2026-08-19, `DoD-25`)
    `docs/plans/2026-08-19_0110_coordinator_lease_revoke_영속화_v1.md`.
    `DoD-24` 가 명시적으로 남긴 안전 공백 — "revoke 상태는 재접속에서
    보존되지 않는다. `revoked` 는 Agent 메모리 상태이고
    `CoordinatorLeaseStore` 에는 그 필드가 없어서, revoke 된 뒤
    프로세스가 끊기면 새 Agent 가 같은 Lease 를 다시 받을 수 있다"
    — 를 닫았다. 설계 조사(코덱스 `p145`, read-only)가 먼저
    `StoredLease` 스키마(11개 필드, revoked 없음)와 `CREATE TABLE
    IF NOT EXISTS` 만 있고 마이그레이션 헬퍼가 없다는 걸 실측해,
    기존 DB 파일 호환을 위해 `PRAGMA table_info`+`ALTER TABLE ADD
    COLUMN` 보정이 필요함을 짚었다. 구현(`p146`, 코덱스
    workspace-write)이 `revoked_at_unix_ms Option<u64>` 컬럼 추가,
    `open()` 시점 자동 마이그레이션, `mark_revoked()`(idempotent —
    이미 revoked 면 최초 timestamp 유지) 신설, `send_revoke_notice()`
    가 **wire 전송보다 먼저** 커밋을 확정하도록 순서를 정하고(테스트용
    `lease_id` override 가 있어도 저장소에는 Grant 의 실제 `lease_id`
    를 기록), `get_or_issue()` 와 갱신 경로 **양쪽 다**(정상 경로 +
    `renew_outcome_override` 읽기 전용 경로까지) revoked Lease 를
    거부하도록 구현했다. 이번엔 구현자가 evidence·CLAUDE.md·HISTORY
    를 스스로 건드리지 않아 "구현자와 검수자가 달라야 한다"
    (ADR-030) 경계를 이전 조각들보다 더 깔끔하게 지켰다. 독립 검수
    (`p147`)가 마이그레이션의 트랜잭션 안전성·listener bind 와의
    순서·두 갱신 경로 전부의 revoked 검사·override 시에도 실제
    lease_id 로 기록되는지·마이그레이션 단위 테스트가 진짜로 컬럼
    없는 옛 스키마를 수동 생성해서 검증하는지까지 전부 확인하고
    **1라운드 만에 `ACCEPTED`**(유일한 지적은 코드가 아니라 검수
    프롬프트 자체의 시나리오 번호 오기였다). `coordinator-agent-selftest`
    35/35 시나리오, 5회 연속 통과(각 90초 하드 타임아웃, coordinator
    유닛 테스트 20→24개). 새 signed outcome/proto 변경은 하지 않고
    기존 raw error 거부를 그대로 썼다. revoke 해제 API·Agent 쪽
    영속화·자동 재접속 루프는 여전히 범위 밖.
12. 만료된 Lease 재접속 거부                                    ★ **완료**(2026-08-20, `DoD-26`)
    `docs/plans/2026-08-20_0136_만료_lease_재접속_거부_v1.md`.
    `DoD-24` 가 명시적으로 이월했던 "만료된 Lease 의 재접속 복원
    거부 시나리오" 를 채웠다. `get_or_issue()` 가 지금까지 identity
    충돌과(`DoD-25` 이후) revoked 여부만 확인하고 **만료 여부는
    확인하지 않았다** — 이미 `expires_at_unix_ms` 를 지난 Lease 도
    재접속하면 그대로 다시 발급됐다. 새 `LeaseStoreError::Expired`
    를 추가하고, 짧은 TTL 로 Lease 를 실제로 만료시킨 뒤(250ms
    TTL + 400ms 실제 sleep) 재접속하면 거부되는 selftest 시나리오
    36 을 신설했다(구현 `p148`, 코덱스 workspace-write). 독립 검수
    1라운드(`p149`)가 진짜 계층 간 결함을 찾았다 — 만료 판정이
    엄격한 `<` 를 써서 정확히 만료 시각과 같은 순간을 아직 유효로
    취급했는데, 같은 저장소가 다루는 Lease 의 서명 검증 계층
    (`crates/protocol/src/signing.rs`, `now >= expires_at` 거부)과
    Agent 의 revoke 검사(`crates/agent/src/lib.rs`, `<=`)는 이미
    "경계 포함" 규칙을 쓰고 있었다 — 그대로 두면 같은 Lease 를
    Coordinator 는 재발급하는데 Agent 는 같은 순간 즉시 만료로
    거부하는 계층 간 모순이 생길 뻔했다. `<` 를 `<=` 로 고치고
    경계값 테스트 2건(정확히 경계에서 `Expired`, 경계 바로 전은
    정상)을 추가한 뒤(`p150`) 2라운드(`p151`)에서 `ACCEPTED`.
    `coordinator-agent-selftest` 36/36 시나리오, 2세트 x 5회 연속
    통과(각 60초 하드 타임아웃, coordinator 유닛 테스트 24→27개).
    만료된 Lease 의 자동 갱신·재발급 정책은 범위 밖 — 거부만 한다.
13. `REVOKED` signed outcome                                    ★ **완료**(2026-08-20, `DoD-27`)
    `docs/plans/2026-08-20_0357_revoked_signed_outcome_v1.md`.
    `DoD-25` 가 명시적으로 남긴 공백 — "새 signed `RENEW_OUTCOME`
    (예: `REVOKED`)이나 proto 변경은 하지 않았다 — Agent 쪽에서
    이 거부와 다른 종류의 handshake 실패를 구분할 신호가 없다" —
    를 채웠다. `proto/lease.proto` 의 `RenewOutcome` enum 에 기존
    번호를 하나도 바꾸지 않고 `RENEW_OUTCOME_REVOKED = 8` 을 순수
    추가하고, Coordinator 의 `build_renew_result()` 가 override
    읽기 전용 경로(`store.get()`)와 `renew_existing_within_duration()`
    를 쓰는 정상 갱신 경로 **양쪽 다**에서 `LeaseStoreError::Revoked`
    /`revoked_at_unix_ms` 를 만나면 raw error 대신 서명된
    `RenewLeaseResult{ outcome: 8 }` 를 반환하도록 고쳤다(구현
    `p155`, 코덱스 workspace-write). 오늘 이미 두 번(8번·9번 항목,
    `DoD-22`·`DoD-23`) 나온 "한쪽은 끝났는데 다른 쪽은 계속
    기다리는" 교착 패턴이 세 번째로 있는지 특히 의심하며 독립
    검수(`p157`)를 요청했다 — 기존 교착 방지 `break` 목록
    (`2 | 3 | 6`)에 outcome 8 이 정확히 추가됐는지, 그 `break` 가
    결과를 전송·flush 한 **뒤에** 실행되는지(전송 전에 break 하면
    Agent 가 응답을 못 받는다), Agent(`crates/agent/src/lib.rs`)도
    outcome 8 을 만나면 즉시 `Err("RENEW_REFUSED:REVOKED")` 로
    종료하는지가 핵심 확인 대상이었다. 검수가 이 전부를 파일:줄
    수준으로 확인했고, 추가로 신규 테스트 전용 플래그
    `revoke_before_renew`(ACK 직후 저장소에만 revoke 를 확정하고
    wire 로 notice 는 안 보냄 — 기존 `revoke_after_round` 경로와
    독립적으로 "Agent 가 revoke 통지를 받기 전에 이미 revoke 된
    Lease 로 갱신을 시도하는" 현실적 시나리오를 재현)와 시나리오
    37 이 실제 Agent 의 wire 서명·replay·nonce 검증을 통과해야만
    성공하는 실질 검증임(단순 exit code 검사가 아님)까지 확인하고,
    `lease_store.rs` 는 설계대로 이번 조각에서 전혀 안 바뀌었음과
    초기 Grant 발급 시점의 revoked 거부(`issue_lease()` ->
    `get_or_issue()`)는 여전히 범위 밖으로 안 건드렸음도 확인했다
    — **1라운드 만에 `ACCEPTED`**. `coordinator-agent-selftest`
    37/37 시나리오, 5회 연속 통과(각 60초 하드 타임아웃). 뮤테이션
    테스트로 outcome 8 `break` 제거 시 시나리오 37 실패 및
    Coordinator 교착 재현을 확인, 원복 후 재검증 PASS. 초기 Grant
    발급의 revoked 거부는 여전히 범위 밖 — 다음 조각으로 미룸.
14. `check_schema.py` CI 연결                                    ★ **완료**(2026-08-20, `DoD-28`)
    `docs/plans/2026-08-20_0424_check_schema_ci_연결_v1.md`.
    `DoD-20`(`tools/canonical/check_schema.py` 신규 구현)이 명시적으로
    남긴 공백 — "CI 파이프라인에 실제로 연결하지 않았다" — 를 닫았다.
    `.github/workflows/canonical-schema-check.yml` 신설(`p156`, 코덱스
    workspace-write) — `main` 대상 `push`·`pull_request` 트리거로
    canonical 참조 self-test·벡터 대조·`check_schema.py`·워크스페이스
    build/test(`gputeer-runtime-windows` 제외)·`verify_evidence.py`
    를 순서대로 실행한다. Rust 1.89(`Cargo.toml` 의 `rust-version`
    과 일치), apt `protobuf-compiler` 설치가 Cargo 의
    `protoc-bin-vendored` 와 별개로 `check_schema.py` 가 PATH 의
    `protoc` 를 직접 subprocess 호출하기 때문에 실제로 필요함을
    소스로 확인 후 포함시켰다. 이 저장소는 원격이 없어 워크플로
    자체가 GitHub Actions 에서 실제로 돈 적은 없다 — 로컬에서 동일
    명령을 순서대로 실행해 성공을 확인하는 것으로 검증을 대신했다.
    독립 검수(`p159`, 대화 기록 없는 새 코덱스 인스턴스, read-only)가
    YAML 문법·트리거 대상·각 스텝의 경로 정확성(`--exclude` 이름이
    `crates/runtime-windows/Cargo.toml` 의 실제 package name 과
    일치하는지)·Rust 버전 일치·apt 설치의 실제 필요성(코드로 확인)·
    최소 권한(`contents: read`)·워크플로가 아직 커밋·push 되지
    않았는지·evidence/CLAUDE.md/HISTORY 무변경까지 전부 확인하고
    핵심 명령(`check_schema.py`·`verify_evidence.py`) 일부는 직접
    재실행까지 해서 **1라운드 만에 `ACCEPTED`**. 실제 GitHub Actions
    러너에서의 첫 실행 결과는 원격이 생기기 전까지는 이 세션 범위
    밖으로 남는다.
15. 레거시 `--lease-db` 없는 경로 명시적 opt-in                  ★ **완료**(2026-08-20, `DoD-29`)
    `docs/plans/2026-08-20_0437_레거시_lease_경로_명시적_opt_in_v1.md`.
    `--lease-db` 없이(레거시 경로) Coordinator 를 시작하면 revoke·
    만료·`max_total_duration_seconds` 보호가 전부 조용히 우회되던
    위험한 기본값을, 새 플래그 `--i-understand-legacy-mode-is-unsafe`
    (기본값 false)로 명시적 opt-in 을 요구하도록 막았다 — 둘 다
    없으면 TCP bind 전에 즉시 종료한다. 구현 1라운드(`p160`, 코덱스
    workspace-write) 뒤 독립 검수 1라운드(`p161`)가
    `CHANGES_REQUESTED` — 시나리오 27~31(`DoD-22` 저장소 무관 revoke
    notice 계약)이 자동으로 opt-in 플래그를 받는 의도가 주석에 없고,
    시나리오 38 이 Agent 쪽 종료를 검증 안 함. 구현 2라운드(`p162`,
    `coordinator_agent_selftest.rs` 만 수정)가 두 지적 반영(`--lease-db`
    는 27~31에 추가하지 않음 — 검증 대상이 바뀌므로). 독립 검수
    2·3라운드(`p163`·`p164`)가 코드 지적은 해소를 확인했으나 검수
    환경(코덱스 read-only 샌드박스) 안에서만 재현되는 selftest 미완료
    (Coordinator 는 스폰되고 Agent 서브프로세스는 안 뜸)를 보고해
    거듭 `CHANGES_REQUESTED`. 이 세션(감독자, claude-code)이 같은
    바이너리를 샌드박스 **밖**에서 16회 연속 실행해 전부 exit=0·
    38개 시나리오·약 9초/회로 단 한 번도 재현되지 않음을 확인 —
    `DoD-28` 에서 이미 관측된 것과 같은 종류의 코덱스 read-only
    샌드박스 프로세스 스폰 제약으로 결론짓고 최종 `ACCEPTED`.
    `coordinator-agent-selftest` 38/38 시나리오. 레거시 경로 자체의
    동작(opt-in 뒤)은 바뀌지 않았다 — 여전히 revoke/만료/max-duration
    보호가 없다는 근본 한계는 유지, 이번 조각은 실수로 그 상태에
    빠지는 것만 막는다.
16. Job 시작 WRITING 마커                                        ★ **완료**(2026-08-20, `DoD-30`)
    `docs/plans/2026-08-20_0310_job_시작_마커_최소_조각_v1.md`
    (설계는 `p154`, 오늘 새벽 완료 — 오늘 실제 구현). `crates/agent`
    가 Grant/Lease 검증 → `AgentGrantAck` → `RESULT ok=true` 출력
    후 종료, 여기서 멈추던 것에 Job 실행을 향한 가장 작은 첫
    걸음을 붙였다 — 유효 검증 성공 직후·ACK 전송 전에 결정적
    `checkpoint_id`(BLAKE3-256, domain + 길이-프리픽스된
    job_id/attempt_id/grant_id — canonical encoding 결함 방지)로
    checkpoint 디렉터리를 만들고 `WRITING` 마커를 `write_once()`
    (`DoD-21` 계약 상속)로 기록한다. 위조/만료/revoked Lease 는
    마커 미생성·ACK 미전송, 마커 생성 실패는 fail-closed, 재시도는
    `write_once()` 의 기존 idempotent 동작에 의존한다. 실제
    entrypoint 프로세스 실행·GPU 확인·runtime 격리·데이터 파일·
    `manifest.json`·Coordinator 보고 wire 메시지·scheduler 는 전부
    범위 밖 — `RESULT ok=true` 가 Job 완료를 뜻하지 않는다는 것을
    코드 주석으로 명시했다. `coordinator-agent-selftest` 시나리오
    39~44 신설(38→44개). 독립 검수(`p166`, 대화 기록 없는 새 코덱스
    인스턴스, read-only)가 실행 순서·`checkpoint_id` 인코딩·거부
    경로의 파일시스템 수준 확인·fail-closed·멱등성·범위 제한까지
    전부 코드로 확인하고 **1라운드 만에 `ACCEPTED`**. 검수 환경
    (read-only 샌드박스)의 selftest 30초 제한은 `DoD-28`·`DoD-29`
    와 같은 종류의 프로세스 스폰 제약으로 판단, 감독자가 샌드박스
    밖에서 5회 연속 재확인(전부 exit=0, 44개 시나리오, 약 10초/회).
17. terminal outcome 다회차 교착 회귀 테스트 보강                ★ **완료**(2026-08-20, `DoD-31`)
    백로그 재조사(`p167`) 1순위. 오늘 밤 세 번(`DoD-22`·`DoD-23`·
    `DoD-27`) 나온 교착 버그 패턴 — Coordinator 가 다회차 갱신 루프
    중 정책 거부 outcome(`SUPERSEDED=2`·`QUARANTINED=3`·
    `MAX_DURATION_EXCEEDED=6`·`REVOKED=8`)을 마지막이 아닌 회차에서
    보내면 Agent 는 즉시 종료하는데 Coordinator 는 계속 기다리는
    교착 — 에 대해 `SUPERSEDED`·`REVOKED` 만 다회차 회귀 시나리오가
    있고 `QUARANTINED`·`MAX_DURATION_EXCEEDED` 는 단일 회차뿐이던
    공백을 닫았다. `coordinator_agent_selftest.rs` 에 시나리오
    45·46 신설(`p168`, 프로덕션 코드는 전혀 안 바꿈 — 순수 테스트
    추가). 독립 검수 1라운드(`p169`)가 `CHANGES_REQUESTED` 를
    냈으나, 조사 결과 검수가 진행되던 시간대에 감독자(claude-code)
    가 코덱스의 뮤테이션 보고를 독립 재확인하려고 `matches!` 를
    직접 순차 뮤테이션(outcome=3·6 각각 제거→재현→원복)하던 중간
    상태를 검수가 읽은 오탐이었다 — 감독자가 두 뮤테이션 모두
    정확히 예측된 교착으로 재현하고 완전히 원복(`git diff --stat`
    무변경)한 뒤 안정 상태로 2라운드(`p170`)를 요청 — `git diff
    --stat` 단일 파일·`matches!` 4개 값 전부 존재·시나리오 45·46
    의 정확한 assert·신규 CLI 플래그 없음까지 전부 코드로 확인하고
    **`ACCEPTED`**. 프로세스 교훈: 독립 검수 진행 중 감독자의 직접
    뮤테이션 재현은 순차 진행하기로 함. `coordinator-agent-selftest`
    46/46 시나리오, 5회 연속 통과.
18. 만료된 Lease 갱신 경로 fail-closed                           ★ **완료**(2026-08-20, `DoD-32`)
    백로그 재조사(`p167`) 2순위. 설계 조사(`p171`, read-only)가
    실제 안전 공백을 확인했다 — `renew_existing_within_duration()`
    이 revoke·`max_total_duration_seconds` 만 검사하고 저장된
    `expires_at_unix_ms` 가 이미 지났는지는 검사 안 한 채 즉시 새
    만료시각으로 갱신했다(정상 Agent 도 checkpoint/ACK 지연·시계
    어긋남으로 도달 가능한 공백이지, 악의적 Agent 만의 문제가
    아니었다). 구현(`p172`, 코덱스 workspace-write)이 `DoD-26` 과
    동일한 `<=` 경계 규칙으로 revoke 검사 뒤·max-duration 검사
    전에 `expires_at_unix_ms <= now_unix_ms` 를 추가하고, 기존
    `LeaseStoreError::Expired` 를 재사용해 raw error 로 거부한다
    (signed outcome 새로 안 만듦 — `DoD-27` 이 REVOKED 만 다뤘던
    것과 같은 범위 판단). override 읽기 경로·일반 갱신 경로 양쪽
    다 적용, 기존 정상 갱신·`MaxDurationExceeded`·경계 단위 테스트
    fixture 의 만료시각을 미래로 보정(원래 판정 조건은 유지), 새
    경계 단위 테스트 2건(`expires_at==now`→`Expired`·DB 레코드
    완전 불변, `expires_at==now+1`→정상 갱신)과 selftest 시나리오
    47(짧은 TTL 로 실제 만료 뒤 갱신 거부) 신설. 독립 검수(`p173`,
    대화 기록 없는 새 코덱스 인스턴스, read-only)가 경계 규칙·검사
    순서·`Expired` 반환 시 `UPDATE` 미실행·fixture 보정의 타당성·
    범위 제한(`renew_existing()`·Agent·proto 무변경)까지 전부
    코드로 확인하고 **1라운드 만에 `ACCEPTED`**. 감독자가 검수
    완료 **후**(`DoD-31` 의 동시 조작 오탐 교훈을 반영해 순차
    진행) `cargo build`/`test`·selftest 5회 연속으로 독립
    재확인(전부 exit=0, 47개 시나리오, coordinator 유닛 테스트
    27→29건). 새 signed `RENEW_OUTCOME_EXPIRED` 는 범위 밖 —
    raw error 거부만 한다.
19. marker-only checkpoint GC 회귀 테스트                        ★ **완료**(2026-08-20, `DoD-33`)
    백로그 재조사(`p167`) 3순위(마지막 후보). `DoD-30`(Job 시작
    WRITING 마커) evidence 문서가 "WRITING 마커만 있는 디렉터리는
    기존 `gc_partial()` 규칙상 PARTIAL 로 취급돼 GC 대상"이라고
    문서로만 주장했던 것을 실제 회귀 테스트로 고정했다(`p174`,
    코덱스 workspace-write) — `crates/checkpoint/tests/durability_chaos.rs`
    에 `startup_gc_removes_marker_only_checkpoint_but_preserves_manifest_checkpoint`
    신설, `.durability.writing` 마커만 있는 디렉터리가
    `startup_gc()` 후 실제로 삭제되고 완결된(manifest+데이터)
    디렉터리는 보존됨을 파일시스템 상태(`exists()`/`is_dir()`/
    `is_file()`)로 확인. GC 알고리즘 자체(`atomic.rs`·`writer.rs`)
    와 `crates/agent/src/lib.rs` 는 전혀 안 바꿨다(순수 테스트
    파일 1건만 변경). 독립 검수(`p175`, 대화 기록 없는 새 코덱스
    인스턴스, read-only)가 `gc_partial()` 판정식(`atomic.rs:589`)
    과 Agent 의 실제 마커 생성 코드(`agent/lib.rs:582-602`)가
    정확히 일치하는지, 새 assert 가 실제 파일시스템 상태를
    확인하는지, 뮤테이션 논리 타당성까지 전부 코드로 확인하고
    **1라운드 만에 `ACCEPTED`**. 감독자가 검수 완료 후 `cargo
    build`·`cargo test -p gputeer-checkpoint --test
    durability_chaos`(17개 전부 통과)·워크스페이스 전체 테스트로
    독립 재확인. **이로써 오늘 새벽 백로그 재조사(`p167`)가 찾은
    3개 후보(교착 회귀·만료 갱신 fail-closed·marker-only GC)
    전부 완료됐다.**
20. Agent 쪽 갱신 직전 만료 재확인                                ★ **완료**(2026-08-20, `DoD-34`)
    마지막 백로그 재조사(`p176`)가 찾은 유일 후보. Agent
    (`crates/agent/src/lib.rs`)가 갱신 루프에서 `revoked` 여부만
    확인하고 만료는 재확인 안 해 이미 만료된 Lease 로 갱신 요청을
    보낼 수 있었다(`DoD-32` 가 Coordinator 쪽에서 이미 raw error
    로 방어해 Lease 부활 결함은 아니었지만, Agent 쪽 낭비/관측
    공백이었다). `RenewLeaseRequest` 를 만들기 직전에 새
    `lease_is_expired()` 헬퍼(`DoD-26`/`DoD-32` 와 동일한 `<=`
    경계, 최초 Grant 검증·revoke 경로와 일관)로 재확인해, 만료
    시 요청 자체를 안 보내고 `RENEW_REFUSED:LOCAL_EXPIRED` 로
    종료하도록 고쳤다(`p177`, 코덱스 workspace-write). Coordinator
    코드는 전혀 안 건드렸다. ★ 오늘 밤 이미 세 번(`DoD-22`·
    `DoD-23`·`DoD-27`) 나온 교착 패턴의 **반대 방향**(Agent 가
    요청을 안 보내면 Coordinator 가 무한 대기할 위험)이 최우선
    검증 대상이었다 — 구현자·독립 검수(`p178`, 대화 기록 없는
    새 코덱스 인스턴스)·감독자(claude-code) 3단계 모두
    `read_frame()` 이 TCP EOF 를 즉시 오류로 전파함을 코드로
    확인하고, `coordinator-agent-selftest` 5회 연속(매회 약 16초,
    90초 하드 타임아웃 근처에도 안 감)으로 실측 재확인했다. 독립
    검수 **1라운드 만에 `ACCEPTED`**. `coordinator-agent-selftest`
    48/48 시나리오. **이로써 오늘 밤 백로그 재조사(`p167`·`p176`)
    가 찾은 모든 하루 규모 후보를 마쳤다** — 남은 항목은 트리거가
    없거나(scheduler·자동 재접속 루프·다중 Agent·QUARANTINED
    실제 판정) 시스템/보안 설정 변경이 필요한 것(OS 방화벽·WSL2)
    뿐이다.
21. 자동 재접속 최소 경로                                        ★ **완료**(2026-08-20, `DoD-35`)
    `docs/plans/2026-08-20_1001_자동_재접속_최소_경로_v1.md`
    (기준 로드맵: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`,
    7조각·6~8일 규모). 사용자가 기상 후 "코덱스로 더 할 거
    체크해서 작업 이어가" 라고 직접 지시했다 — 재조사(`p179`)가
    자동 재접속 루프를 최우선 후보로 꼽았고, 설계 조사(`p180`)가
    "proto 변경 없이 Agent bounded retry + Coordinator 반복
    accept" 로 범위를 좁히며 정직한 규모를 1.5~2일로 재산정했다
    (오늘 밤 조각들보다 훨씬 크다) — 사용자에게 직접 확인받고
    "그대로 한 번에 진행" 을 선택했다.

    같은 Agent **프로세스**가 TCP 연결이 끊겼을 때(재시작이 아니라
    연결만 끊긴 경우) bounded retry(최대 8회·총 60초·exponential
    backoff+full jitter·연결별 3초 timeout)로 재연결해 **기존
    Grant/ACK handshake 를 처음부터 재수행**한다 — Resume proto
    는 만들지 않았다(로드맵 조각 3 이후로 명시적으로 미룸).
    `CoordinatorLeaseStore::get_or_issue()`(`DoD-24`)가 이미 같은
    `lease_id` 재요청에 저장된 레코드를 그대로 돌려주므로 저장소
    쪽 추가 변경은 불필요했다. Coordinator 는 `listener.accept()`
    를 반복하는 루프로 바뀌었다(`--max-connections`·
    `--accept-timeout-ms`·`--drop-connection-after-ack-once`
    신설, 기존 `--disconnect-after-ack` 의미는 불변). Grant/ACK/
    Renew nonce 계산에 `connection_attempt` 를 반영해 재접속 시
    nonce 충돌(replay 오인)을 피했다.

    구현 1라운드(`p181`, 코덱스 workspace-write)가 Windows
    `WSAEWOULDBLOCK`(10035) 플랫폼 버그(nonblocking listener 설정이
    accept 된 stream 에도 전파되던 문제)도 발견해 고쳤다. 독립
    검수 1라운드(`p182`)가 **진짜 결함 2건**을 찾았다 — (1) Agent
    가 TCP `connect()` 레벨 실패(재시도 가능한 오류)마다 nonce
    계산용 카운터를 증가시키는데 Coordinator 는 실제 `accept()`
    성공 횟수만 증가시켜, 연결 실패가 한 번이라도 있으면 두 값이
    어긋나 재접속이 `GRANT_REJECTED` 로 실패하는 결함. (2) selftest
    헬퍼가 Coordinator stdout 을 무제한 blocking read 로 읽어
    Coordinator 가 반복 accept 로 멈추면 하드 타임아웃이 무력화될
    수 있는 결함. 감독자(claude-code)가 코드로 직접 재확인해 둘
    다 실재함을 확인.

    구현 2라운드(`p183`)가 nonce 계산용 카운터를 TCP 연결이 **실제
    성공한 뒤에만** 증가하도록 재시도 루프 반복 횟수와 분리하고,
    selftest 의 Coordinator stdout 읽기를 reader thread + deadline
    감시로 바꿨다. 독립 검수 2라운드(`p184`)가 **프로덕션 로직
    자체는 코드로 직접 추적해 올바르다** 고 확인했으나, 새 회귀
    테스트가 `run()` 의 실제 재접속 루프를 안 타고 헬퍼를 손으로
    두 번 호출할 뿐이라 증명력이 없다고 지적. 구현 3라운드
    (`p185`)가 실제 loopback 포트를 닫아 진짜 `connect()` 거부를
    만들고 실제 `Agent::run()` 을 스레드로 실행해 재시도를 태운
    뒤 연결을 성공시키는 통합 테스트로 재작성 — 뮤테이션으로
    원래 버그 상태(`attempt_config.connection_attempt = attempt`)
    를 재현해 새 테스트가 실제로 `GRANT_REJECTED` 로 실패함을
    확인한 뒤 원복. 독립 검수 3라운드(`p186`)에서 **`ACCEPTED`**.

    감독자가 3라운드 각각 `coordinator-agent-selftest` 5회 연속
    (전부 exit=0, 52개 시나리오, 약 25.4~26.7초/회, 120초 하드
    타임아웃 대비 여유)으로 독립 재확인했다 — 기존 48개 시나리오는
    전부 회귀 없이 그대로 통과(nonce 값이 `connection_attempt=0`
    일 때 기존과 바이트 단위로 동일). 신규 시나리오 49~52(정상
    재접속 성공·bounded retry 소진·재접속 중 revoke·재접속 중
    만료). `AmbiguousRenew`(갱신 결과 유실)는 재시도 안 하고 즉시
    종료 — durable request ledger 없이는 안전하지 않다. Resume
    proto·durable request ledger·다중 Agent 경쟁·Coordinator HA
    는 로드맵 후속 조각(3~7)으로 명시적으로 남는다.
22. Resume 프로토콜                                              ★ **완료**(2026-08-20, `DoD-36`)
    `docs/plans/2026-08-20_1200_resume_프로토콜_v1.md`(로드맵
    조각 3). 사용자가 "코덱스 쿼터만 써서 다 진행 최대한 시켜봐"
    라고 지시해 `DoD-35`(조각 1+2)에 이어 곧바로 시작했다. 설계
    조사(`p187`)가 범위를 확정한 뒤, 구현 1라운드(`p188`, 코덱스
    workspace-write, 22개 파일·1156줄)가 `proto/lease.proto` 에
    `SessionMode`·`AgentSessionHello`·`ResumeLeaseRequest`·
    `ResumeOutcome`·`ResumeLeaseResult` 를 순수 추가(기존 필드
    번호 불변)하고 canonical/signing 체인 전체(`docs/protocol/signing.md`
    §5 domain_tag 3종·`crates/protocol` 의 `canonical.rs`/
    `to_fields.rs`/`signable.rs`/5개 테스트 파일·
    `tools/canonical/reference_canonical.py`·
    `tests/vectors/canonical_v1.json` 45→48개 벡터·
    `proto/SCHEMA_FINGERPRINT.txt`·`crates/crypto/src/framed_ingress.rs`
    의 `FrameType` 3종)를 갱신했다 — domain count 25→28. 새 읽기
    전용 `classify_resume()`(`crates/coordinator/src/lease_store.rs`,
    `get_or_issue()`/`renew_existing_within_duration()` 재사용
    안 함)이 identity→revoke→만료(`<=`)→epoch(낮으면 SUPERSEDED,
    높으면 `EPOCH_AHEAD`, 같으면 `RESUMED`, 만료시각 비연장) 순으로
    판정한다. Agent 는 `--resume-protocol` opt-in 시에만 새 경로를
    쓰고 기본값은 기존 handshake 그대로 — 기존 52개 시나리오
    전부 안 바뀜. selftest 시나리오 53~60(`RESUMED`·
    `UNKNOWN_LEASE`·`IDENTITY_CONFLICT`·`REVOKED`·`EXPIRED`·
    `SUPERSEDED`·`EPOCH_AHEAD`·`UNAVAILABLE`) 신설.

    독립 검수 1라운드(`p189`)가 대부분(proto 순수성·기존 handshake
    보존·domain 고유성·`classify_resume()` 판정 순서 등) 통과시키면서도
    새 canonical 벡터 `v34`/`v35`/`v36` 이 Rust 쪽에서 실제로
    대조되는 테스트가 없다는 진짜 공백(`DoD-05` 와 같은 종류)을
    찾아 `CHANGES_REQUESTED` — 구현 2라운드(`p190`)가
    `t1_signing_targets.rs` 에 교차검증 테스트를 추가하고 뮤테이션
    (필드 번호 10→11)으로 검증. 독립 검수 2라운드(`p191`)가 그
    내용은 이미 문제없다고 확인하면서도 아직 커밋 전인 조각
    전체의 `git diff --stat` 범위를 오해해(`DoD-31` 과 같은 종류의
    오탐) 다시 `CHANGES_REQUESTED` — 감독자가 나머지 21개 파일의
    변경이 1라운드 이후 불변임을 직접 확인한 뒤 3라운드(`p192`)를
    요청해 그 경위를 설명받고 최종 **`ACCEPTED`**. 감독자가 매
    라운드 canonical self-test/verify·check_schema·cargo build/test·
    selftest 5~8회 연속(전부 exit=0, 60개 시나리오, 약 30초/회)
    으로 독립 재확인했다. **로드맵 7조각 중 1~3 완료** — 남은
    4(dispatcher 정교화)·5(durable request ledger)·6(Agent Resume
    통합)·7(다중 Agent selftest)은 후속 조각으로 남는다.
23. Coordinator dispatcher 정교화                                ★ **완료**(2026-08-20, `DoD-37`)
    `docs/plans/2026-08-20_1310_coordinator_dispatcher_정교화_v1.md`
    (로드맵 조각 4). 설계 조사(`p193`)가 "조각 4 의 반복 accept
    기반은 `DoD-35` 가 이미 만들었지만, `serve_one_connection()`
    함수 분리와 transport/protocol/storage 오류 차등 처리 계약은
    진짜 남은 하루 규모 작업"이라고 정직하게 판정했다. 구현
    1라운드(`p194`, 코덱스 workspace-write)가 인라인 Grant 처리를
    함수로 추출하고 `CoordinatorSessionError{Transport, Protocol,
    Storage}` 를 신설 — transport/protocol 오류는 로그 후 다음
    accept, storage 오류는 fail-closed. `--max-connections` 는
    성공이 아니라 accept 성공한 연결 시도 수를 세어 무한 재시도를
    막는다.

    독립 검수 1라운드(`p195`)가 진짜 안전 결함을 찾았다 — Resume
    처리 경로(`classify_resume()`)에서 SQLite 조회 오류가 서명된
    `UNAVAILABLE` 로 응답된 뒤 함수가 정상 종료해 dispatcher 의
    fail-closed 분기에 절대 도달 못했다. 감독자가 코드로 직접
    재확인해 실재함을 확인. 구현 2라운드(`p196`)가 `LeaseStoreError`
    를 정책 판정(`NotFound`·`IdentityConflict`·`Revoked`·`Expired`
    — 정상 서명 응답)과 진짜 저장소 장애(`Io`·`LockTimeout` →
    `Storage` fail-closed)로 명확히 구분해 닫았다. Grant 발급·
    renew 조회/갱신·revoke 저장 경로는 이미 올바르게 fail-closed
    돼 있음을 재점검만 하고 안 건드렸다.

    ★ 이 수정 과정에서 `DoD-36` 이 기록한 시나리오 60(durable
    store 없이 Resume 시 서명된 `UNAVAILABLE`)의 기대 동작이
    "구성 오류로 보고 fail-closed 즉시 종료" 로 의도적으로
    재정의됐다 — 독립 검수 2라운드(`p197`)가 이 방향이 "durable
    store 없이는 Resume 이 복구할 권위 있는 상태 자체가 없는
    구성 오류이므로 재시도 가능한 `UNAVAILABLE` 보다 즉시 종료가
    fail-closed 원칙과 일관된다"고 판단하고 최종 **`ACCEPTED`**.
    감독자가 두 라운드 모두 `coordinator-agent-selftest` 5회
    연속(전부 exit=0, 63→64개 시나리오, 약 30.5~34.3초/회)으로
    독립 재확인했다. **로드맵 7조각 중 1~4 완료** — 남은
    5(durable request ledger)·6(Agent Resume 통합)·7(다중 Agent
    selftest)은 후속 조각으로 남는다.
24. Agent Resume 통합                                            ★ **완료**(2026-08-20, `DoD-38`)
    `docs/plans/2026-08-20_1410_agent_resume_통합_v1.md`
    (로드맵 조각 6). 사용자 지시("코덱스 쿼터만 써서 다 진행 최대한
    시켜봐")에 따라 `DoD-37`(조각 4)에 곧바로 이어서 시작했다.
    설계 조사(`p198`, read-only)가 조각 6 의 상태를 "부분 완료"로
    정직하게 판정했다 — Agent(`crates/agent/src/lib.rs:297` 부근
    `run_resume_connection()`)는 이미 실제로
    `AgentSessionHello`/`ResumeLeaseRequest` 를 보내고
    `ResumeLeaseResult` 를 검증하며, 재시도 여부(안전 동작)는 이미
    올바르다 — `REVOKED`·`EXPIRED`·`SUPERSEDED`·`UNKNOWN_LEASE`·
    `IDENTITY_CONFLICT`·`EPOCH_AHEAD` 는 전부 재시도 없이 즉시
    종료, `UNAVAILABLE` 만 재시도 가능. 부족했던 건 **명시적
    구분**(오류 문자열이 outcome 별로 안 나뉘고, selftest 도 이를
    검증 안 함)뿐이었다.

    구현(`p199`, 코덱스 workspace-write)이 `agent/lib.rs:411`
    부근에 `DoD-27` 의 `RENEW_REFUSED:REVOKED` 패턴을 본떠
    `RESUME_REFUSED:REVOKED`·`EXPIRED`·`SUPERSEDED`·
    `UNKNOWN_LEASE`·`IDENTITY_CONFLICT`·`EPOCH_AHEAD` 6종을
    추가했다(`RESUMED`·`UNAVAILABLE`/`RETRYABLE_RESUME` 경로는 안
    건드림). `coordinator_agent_selftest.rs` 의 기존 시나리오
    53~59 를 보강해 — outcome 별 정확한 오류 문자열·
    `CONNECTION_ATTEMPT` 정확히 1회(재시도 안 함)·retryable
    오분류 시 나타나는 `ReconnectExhausted` 미발생까지 실제로
    assert 하도록 고쳤다. 뮤테이션(`REVOKED` 문자열을 임시로
    generic 으로 바꾸자 시나리오 56 실패, 원복 후 재검증)으로
    새 assert 의 실효성을 확인.

    ★ 재시도 안전 동작이 정말 하나도 안 바뀌었는가가 최우선 검증
    대상이었다 — 독립 검수(`p200`, 대화 기록 없는 새 코덱스
    인스턴스, read-only)가 `SessionError` 분류에서
    `RETRYABLE_CONNECTION`·`RETRYABLE_RESUME` 만 `Retryable` 이고
    새 6개 `RESUME_REFUSED:*` 는 전부 `Fatal` 로 남아있음을,
    `UNAVAILABLE` 은 여전히 `RETRYABLE_RESUME` 경로를 유지함을
    코드로 직접 추적해 확인했다. selftest 보강이 실제로 assert
    하는지, 뮤테이션이 타당한지, 범위(agent/lib.rs·
    coordinator_agent_selftest.rs 두 파일 + 계획 문서만,
    Coordinator/proto 무변경)까지 전부 확인하고 **1라운드 만에
    `ACCEPTED`**. 감독자가 검수 완료 후 `cargo build`/`test`·
    `coordinator-agent-selftest` 5회 연속(전부 exit=0, 64개
    시나리오, 약 30.5~31.4초/회, 시나리오 개수는 불변 — 기존
    시나리오 강화만)으로 독립 재확인했다. **로드맵 7조각 중
    1·2·3·4·6 완료** — 남은 5(durable request ledger, 원래 추정
    Rust 300~550줄+테스트 250~400줄)·7(다중 Agent selftest, 원래
    추정 selftest 300~500줄+검증 150~250줄, 조각 5 이후 유의미)만
    후속 조각으로 남는다.
25. Ambiguous Renew 복구 (로드맵 조각 5 재정의)                  ★ **완료**(2026-08-20, `DoD-39`)
    `docs/plans/2026-08-20_1615_ambiguous_renew_복구_v1.md`.
    사용자가 "코덱스한테 ㄱ" 로 조각 5 진행을 직접 지시. 설계
    조사(`p201`, read-only)가 로드맵 원안("durable request
    ledger", 300~550줄 규모)을 정직하게 재범위했다 — 실제 공백은
    "정확히 한 번 처리"가 아니라 **가용성** 공백이다. Agent 가
    `RenewLeaseRequest` 전송 뒤 결과를 받기 전에 연결이 끊기면
    `SessionError::AmbiguousRenew` 로 분류돼 지금까지는 재접속
    루프가 즉시 `Err` 로 끝났다(`agent/lib.rs:280`,`290`) —
    Coordinator 는 실제로 SQLite 커밋을 결과 전송 **전에** 이미
    확정하므로(`lease_store.rs:455`,`501`,`514` → `coordinator/lib.rs:740`)
    Agent 가 무조건 종료하는 건 안전하되 과도했다. Resume
    프로토콜(`DoD-36`~`38`)이 "현재 권위 있는 상태"를 이미
    제공하므로, 별도 request ledger 없이도 기존 Grant/ACK 경로의
    `get_or_issue()` 로 안전하게 복구할 수 있다는 게 조사의
    결론 — 정직한 규모 재산정: production Rust 약 70~130줄,
    테스트 약 140~230줄(원안의 1/3 이하).

    구현 1라운드(`p202`, 코덱스 workspace-write)가 새 게이트
    `--recover-ambiguous-renew-from-durable-lease`(기본 false)
    가 켜졌을 때만 `AmbiguousRenew` 를 bounded reconnect 대상으로
    바꾸고, 재접속 성공 후 **기존 Grant/ACK 경로**(`get_or_issue()`)
    를 그대로 재실행해 최신 저장 Lease 를 재조회한 뒤 새 nonce 로
    새 Renew 를 보내도록 구현했다. Coordinator 에 테스트 전용
    `--drop-after-renew-commit-before-result-once` hook 을 추가해
    "커밋과 결과 전송 사이" 애매한 구간을 재현했다. selftest
    시나리오 65~71 신설, 71/71 시나리오 5회 연속 통과. 감독자가
    직접 재확인하던 중 10회 중 1회(시나리오 51, 오늘 변경과 무관한
    `DoD-35` 이래의 기존 시나리오) `accept timeout exceeded` 로
    실패했으나, 배경 빌드 직후의 시스템 부하 없는 상태에서 추가
    5회 전부 통과·재현 안 됨을 확인해 타이밍 플레이키로 결론지었다.

    독립 검수 1라운드(`p203`)가 **진짜 안전 결함**을 찾았다 —
    durable 복구 게이트가 Coordinator 의 실제 `--lease-db` 설정과
    **검증 가능하게 결합되지 않아**, 사용자가 legacy Coordinator
    (`--i-understand-legacy-mode-is-unsafe`, `DoD-29`)와 이 Agent
    게이트를 잘못 조합하면, 재접속 Coordinator 는 durable
    `get_or_issue()` 가 아니라 `None => StoredLease` 경로로 **그
    순간 현재 시각 기준 Lease 를 새로 조작**한다
    (`coordinator/lib.rs:1408`,`1417`) — 이건 "커밋된 상태
    재조회"가 아니라 legacy 상태에서 `issued_at`·만료 권위를 새로
    만들어내는 것이었다. 시나리오 70 은 게이트를 생략한(기본값
    false) 경우만 검증했지 이 실제 오조합은 검증하지 않았다.

    구현 2라운드(`p204`, 코덱스 workspace-write)가 옵션 B(서명된
    durable 비트)로 근본 수정했다 — `proto/job.proto` 의
    `ExecutionGrant` 를 schema v2 로 승격해 서명 대상 필드 25
    `lease_from_durable_store`(bool) 를 순수 추가(기존 필드 번호
    불변, domain_tag `gputeer/v2/grant` 신설, `DoD-27`·`DoD-36`
    과 같은 순수 추가 패턴). Coordinator 는 `lease_store.is_some()`
    이고 실제 SQLite transaction commit 이 성공했을 때만 이
    필드를 true 로 서명한다(`coordinator/lib.rs:1354`,`1374`).
    Agent 는 재접속 복구 시 받은 Grant 가 서명된 durable=true 가
    **아니면** ACK·checkpoint·새 Renew 전송 **전에**
    `DURABLE_LEASE_RECOVERY_REFUSED` 로 fatal 종료한다
    (`agent/lib.rs:530`). canonical/signing 체인 전체(48개 벡터)
    를 갱신하고, `t1b_grant_and_control.rs` 에 이 비트가 실제로
    canonical 서명 입력에 영향을 주는지 확인하는 교차검증 테스트를
    추가했다. 새 selftest 시나리오 72 가 legacy+recovery=true
    오조합을 검증 — 방어 조건을 임시로 제거하는 뮤테이션으로
    원래 결함(legacy 가 조작 Lease 를 발급, 두 번째 Renew 까지
    성공)이 재현됨을 확인한 뒤 원복.

    독립 검수 2라운드(`p205`)가 새 필드가 실제로 canonical 서명
    입력에 포함되는지(값 변조 시 서명 실패), Coordinator 가
    "store 는 Some 이지만 특정 Lease 는 미커밋"인 경로 없이
    정직하게 이 필드를 채우는지(`coordinator/lib.rs:288`,`1354`,
    `lease_store.rs:287`), Agent 의 거부가 ACK·checkpoint·Renew
    전송 **전**에 있고 `Fatal` 분류로 재시도 안 되는지, durable/
    legacy 정상 경로(시나리오 65~69·71)에 회귀가 없는지, proto
    v2 승격이 배포 전 변경(원격 없음, `ADR-028`)이라 호환성
    문제가 없는지까지 전부 코드로 확인하고 최종 **`ACCEPTED`**.
    감독자가 두 라운드 모두 `cargo build`/`test`·canonical
    self-test/`--verify`(48개 벡터 일치)·`coordinator-agent-selftest`
    5회 연속(전부 exit=0, 72개 시나리오, 약 39.0~39.9초/회)으로
    독립 재확인했다. **로드맵 7조각 중 1·2·3·4·5·6 완료** — 남은
    7(다중 Agent selftest, 원래 추정 selftest 300~500줄+검증
    150~250줄, 조각 5 완료로 이제 착수 가능)만 후속 조각으로
    남는다.
26. Lease store 동시 최초 발급 안전성 (로드맵 조각 7 재정의 · 자동 재접속 루프 로드맵 마무리)   ★ **완료**(2026-08-20, `DoD-40`)
    `docs/plans/2026-08-20_1732_lease_store_동시_발급_안전성_v1.md`.
    사용자가 "코덱스로 다음 작업 ㄱ" 로 조각 7 진행을 지시. 설계
    조사(`p206`, read-only)가 정직한 결론을 냈다 — 진짜 "다중
    Agent 동시 경쟁"은 지금 Coordinator 아키텍처로는 표현 자체가
    안 된다. `run()` 은 `accept_with_deadline()` → 동기
    `serve_one_connection()` → 완료 후 다음 `accept()` 로
    **의도적으로 순차 처리**다(코드 주석에 "deliberately
    sequential" 명시, `coordinator/lib.rs:315`,`331`,`419`).
    Coordinator 설정에는 Agent identity/key 가 각각 하나뿐이라
    (`coordinator/lib.rs:38`,`321`) 두 번째 독립 Agent 는 그
    계약에서 먼저 막힌다. `connection_attempt` 도 Agent 별이
    아니라 Coordinator 전체 accept 순번이다
    (`coordinator/lib.rs:343`). 이걸 가능하게 하려면 최소 2~4일
    (production 500~900줄+테스트 350~600줄) 규모의 아키텍처
    변경이 필요하다고 판단했다 — `DoD-24` 시나리오 34
    (`IdentityConflict`)는 "순차 실행된 별도 프로세스 쌍의 뒤늦은
    holder 충돌 거부"만 증명했지 "비어 있는 DB 를 두 연결이 거의
    동시에 보고 INSERT 경쟁"은 검증한 적이 없다는 것도 확인했다.

    대신 조사가 찾은 **진짜 검증 안 된 위험**으로 범위를 좁혔다
    — `CoordinatorLeaseStore::get_or_issue()` 가 `BEGIN IMMEDIATE`
    로 check-then-insert TOCTOU 를 막는다고 코드는 주장하는데,
    이게 실제 동시 호출로 측정된 적이 한 번도 없었다. 구현
    (`p207`, 코덱스 workspace-write)이 `crates/coordinator/tests/lease_store_concurrent_issue.rs`
    를 신설했다 — `durable_replay_race.rs` 의 "스레드마다 별도
    SQLite 연결 + `Barrier`" 패턴을 따라, 16개 라운드에서 같은
    `lease_id` 를 다른 `holder_node_id` 로 동시에 최초 발급
    시도해 정확히 하나만 성공함을 확인. 프로덕션 코드는 전혀 안
    건드렸다.

    독립 검수 1라운드(`p208`)가 테스트 자체의 **판별력 결함**을
    찾았다 — 두 경쟁 후보가 `holder_node_id` 외 다른 모든 필드
    (`fence_epoch`·시각 필드들·`coordinator_term`·
    `max_total_duration_seconds`)가 같아서, 최종 `assert_eq!` 가
    패자 값의 부분 덮어쓰기를 실제로 구별해내지 못했다 — "부분
    덮어쓰기 없음" 이라는 핵심 주장이 실제로는 증명되지 않은
    상태였다. 구현 2라운드(`p209`)가 경쟁 후보 A/B 를 identity
    필드(`lease_id`·`job_id`·`attempt_id`)는 동일하게 유지하되
    나머지 8개 필드는 전부 서로 다른 값으로 구별하고, **self-check**
    (승자 필드를 하나씩 패자 값으로 바꿔가며 최종 assert 헬퍼가
    8번 모두 실제로 panic 하는지 직접 실행해 증명)를 추가했다.
    독립 검수 2라운드(`p210`)가 identity 비교 순서(`job_id →
    attempt_id → holder_node_id → issuing_coordinator_id`,
    `lease_store.rs:538`)가 실제 코드와 일치하는지, self-check
    가 dead code 없이 `#[test]` 실행 경로에 포함돼 있는지,
    프로덕션 코드가 여전히 안 바뀌었는지(객체 해시 `df9c9716...`
    가 `HEAD` 와 동일)까지 전부 확인하고 잔여 지적 없이
    `ACCEPTED`. 감독자가 두 라운드 모두 `cargo build`/`test`·
    신규 테스트 반복(각 라운드 5회 이상, flake 없음)·
    `coordinator-agent-selftest`(72개 시나리오, 회귀 없음)로
    독립 재확인했다.

    ★ **로드맵 조각 7 원안("다중 Agent selftest" 전체)은
    완료가 아니라 scheduler/다중 Agent 아키텍처 도입 단계로
    명시적으로 이월된다** — 진짜 다중 Agent 병렬 처리·wire-level
    경쟁·동일 identity 복제 Agent 의 active-session owner 선정·
    다중 Coordinator HA 는 전부 범위 밖으로 남았다. 이 조각은
    "다중 Agent selftest 완료" 가 아니라 훨씬 좁은 "Lease store
    동시 최초 발급 안전성"으로 정직하게 기록한다.

    **이로써 2026-08-20 자동 재접속 루프 로드맵(7조각) 작업을
    마무리한다** — 조각 1~6 은 원안 그대로 완료(`DoD-35`~`39`),
    조각 7 은 정직하게 재범위된 하위 조각만 완료(`DoD-40`), 원안의
    다중 Agent 아키텍처 부분은 후속 로드맵(scheduler 단계)으로
    이월한다.
27. scheduler 로드맵 조각 1 — 순수 hard-filter kernel             ★ **완료**(2026-08-21, `DoD-41`)
    `docs/plans/2026-08-21_1002_scheduler_hard_filter_v1.md`.
    `crates/scheduler`를 신설하고 고정 `PoolSnapshot`·
    `JobRequirements`·`Policy`만 받는 `evaluate_eligibility()`로
    Node/Risk/freshness·서로 독립적인 tier/isolation/key protection·
    GPU/CPU/RAM/workspace·owner 정책을 fail-closed 판정한다. 복수
    적격은 winner를 고르지 않고 `RankingRequired`로 남긴다.
    독립 검수 1라운드가 제3자 정책을 `security_tier==S1`에 잘못
    결합한 isolation 축 오류와 빈 owner/submitter `Some("")`의
    `MissingFact` 우회라는 실제 보안 결함 2건을 찾아
    `CHANGES_REQUESTED`. 2라운드에서 `IsolationClass::Restricted`
    축과 빈 문자열 fail-closed로 근본 수정하고 회귀 테스트 5건을
    추가(총 33개), 뮤테이션 2건으로 비공허성을 확인한 뒤 독립 검수
    `ACCEPTED`. 감독자가 scheduler 테스트 33/33을 직접 확인했다.
    **scheduler 로드맵 9단계 중 조각 1 완료 — 남은 8단계
    (durable Job/Attempt/Queue·다중 Agent inventory·v0.1 best-fit·
    실제 Grant dispatch·Agent entrypoint 실행·실패 감지/복구·
    chance-constrained/fairness·quarantine/E2E)는 후속 조각**.
28. scheduler 로드맵 조각 2a — durable Job/Queue truth            ★ **완료**(2026-08-21, `DoD-42`)
    `docs/plans/2026-08-21_1049_scheduler_durable_job_v1.md`.
    상위 로드맵 조각 2 원안은 durable Job/Attempt/Queue·Lease/Grant
    결합·전체 ControlStore를 포함한 3~5일 규모이므로, 하루에 전부
    완료했다고 주장하지 않고 **조각 2a: durable Job/Queue truth**로
    축소했다. `crates/coordinator/src/job_store.rs`의 신규 SQLite
    `CoordinatorJobStore`가 `BEGIN IMMEDIATE` transaction으로 accepted
    submit의 멱등 저장, `SUBMITTED→PLANNING→QUEUED` 전이, 결정적 queue
    조회와 deadline/queue-timeout/영구 불가능 실패를 보존한다. 별도
    connection과 실제 `Barrier` 경쟁 테스트로 동시 최초 submit의 단일
    생성·replay/conflict를 확인했다. 자체 재검토에서 규범에 없는
    all-zero idempotency key/manifest digest 거부를 제거하고 상태별
    전체 row-shape 손상 검사로 강화했다. 독립 검수는 transaction·
    멱등성·전이·경계·뮤테이션 2건·자체 수정 2건·변경 범위를 코드로
    확인해 **1라운드 만에 `ACCEPTED`**, 감독자가
    `cargo test -p gputeer-coordinator`를 직접 재확인했다. Attempt는
    `STAGING` 진입·fence epoch 증가·Lease 발급과 한 권위 있는 연산으로
    묶어야 split authority를 피하므로 조각 2b로 이월했다.
    **scheduler 로드맵 9단계 중 조각 1·2a 완료 — 남은 durable
    Attempt/Lease 결합(2b)과 이후 7단계는 후속 조각**.
29. scheduler 로드맵 조각 2b-1 — single-node local atomic STAGING kernel   ★ **완료**(2026-08-21, `DoD-43`)
    `docs/plans/2026-08-21_1123_scheduler_attempt_lease_v1.md`.
    조각 2b 전체의 multi-node·reservation·Grant dispatch·Raft
    `COMMITTED`를 하루에 끝냈다고 주장하지 않고, 한 control DB 파일의
    한 connection에서만 보장하는 **single-node local atomic STAGING
    kernel**로 축소했다. 신규 `CoordinatorStagingStore`의
    `stage_queued_with_lease()`가 한 `BEGIN IMMEDIATE` transaction에서
    fence epoch 채번·Attempt/node/Lease 삽입·`QUEUED→STAGING` 전이·
    operation idempotency를 한 번만 commit한다. 세 오류 주입은 모든
    부분 상태를 rollback하고 epoch를 소비하지 않았으며, 별도 connection
    두 개의 실제 `Barrier` 경쟁은 정확히 한 staging만 성공했다.
    자체 재검토에서 renew/revoke 뒤 retry가 정상 가변 Lease 필드를
    손상으로 오인하던 버그를 고쳐 최초 결과 반환과 불변 identity/epoch
    대조를 분리하고, 공백 plan row-shape·조각 2a 이전 migration·부분
    commit 경로를 보강했다. 독립 검수는 공개 Lease API 무회귀·뮤테이션
    2건·Coordinator 프로덕션 4개 파일 범위·`staging_store.rs` 862줄의
    계획 상한 360줄 초과 자기 보고까지 확인해 **1라운드 만에
    `ACCEPTED`**, 감독자가 `cargo test -p gputeer-coordinator`를 직접
    재확인했다. **scheduler 로드맵 9단계 중 조각 1·2a·2b-1 완료 —
    남은 조각 2의 다중 노드 결합·Raft `COMMITTED`와 조각 3~9는 후속**.
30. scheduler 로드맵 조각 3a — durable Agent inventory 저장소 kernel   ★ **완료**(2026-08-21, `DoD-44`)
    `docs/plans/2026-08-21_1208_scheduler_inventory_v1.md`.
    조각 3 전체의 실제 다중 연결·heartbeat wire·active-session
    owner/fencing을 완료했다고 과장하지 않고, 검증·정규화된 Agent inventory를
    저장·투영하는 **single-Coordinator durable repository kernel**로 축소했다.
    신규 `CoordinatorInventoryStore`의 `register_agent()`는 동일 normalized
    registry만 멱등 처리하고 node/device/key/owner 충돌을 무변경으로 거부한다.
    `update_inventory()`는 한 `BEGIN IMMEDIATE` transaction에서 parent/GPU/workload
    전체를 revision 규칙과 함께 원자 교체하고, `pool_snapshot()`은 저장된 사실을
    node ID 순의 기존 scheduler `PoolSnapshot`으로 결정 투영한다. 자체 재검토로
    공개키를 `Vec<u8>`+명시적 32-byte 검사로 바꾸고 경쟁 후보를 전 필드로
    구별하며 SQL/Rust 이중 정렬을 Rust sort 한 곳으로 통일했고, fail-closed
    경계·기존 API·새 교착 경로 부재를 재확인했다. 독립 검수는 원자성,
    register 멱등/충돌, revision 비교 뮤테이션, 결정성, 수정 5건, 제한된 변경
    범위와 1,514줄 자기 보고를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가
    `cargo test -p gputeer-coordinator` 71건을 직접 재확인했다. **scheduler
    로드맵 9단계 중 조각 1·2a·2b-1·3a 완료 — 남은 조각 3의 실제 다중
    연결·heartbeat wire·session owner/fencing과 조각 4~9는 후속**.
31. scheduler 로드맵 조각 4 — 순수 deterministic resource best-fit kernel   ★ **완료**(2026-08-21, `DoD-45`)
    `docs/plans/2026-08-21_1253_scheduler_best_fit_v1.md`.
    전체 placement/reservation을 완료했다고 과장하지 않고 고정 snapshot의 복수
    hard-filter 적격 후보를 정렬하는 순수 kernel로 제한했다. 신규
    `rank_best_fit()`은 healthy·model·최소 VRAM을 만족하는 GPU를 정렬해 요구
    개수만큼 가장 tight한 subset을 고르고, `BestFitPolicy`가 명시한 VRAM 잔여 합·
    GPU 수 잔여·CPU/RAM/workspace 잔여를 lexicographic 비교한 뒤 완전 동점은
    `node_id` 오름차순으로 해소한다. 독립 검수 1라운드는 후보/report 순서만 뒤집은
    기존 테스트가 GPU vector 자체의 순열 독립성을 증명하지 못한다고
    `CHANGES_REQUESTED`; 구현 2라운드에서 reverse된 GPU vector의
    `BestFitRanking` 전체 비교와 VRAM 정렬 제거 시 `node-b`/`node-a`로 winner가
    갈리는 뮤테이션을 추가했다. 검수 2라운드는 조각 전체가 미커밋인 `git diff`의
    `lib.rs`/`model.rs` 1차 산출물을 후속 수정으로 오인한 scope 오탐이었고,
    HEAD가 DoD-44의 `1877760`임을 감독자가 명확히 한 3라운드에서 최종
    `ACCEPTED`. 감독자가 `cargo test -p gputeer-scheduler` 48/48을 직접
    재확인했다. Coordinator 배선·inventory revision/CAS·allocation·reservation·
    Grant는 범위 밖이다. **scheduler 로드맵 9단계 중 조각
    1·2a·2b-1·3a·4 완료 — 남은 조각 3 나머지·5~9는 후속**.
32. scheduler 로드맵 조각 5 — 로컬 placement-to-staging orchestration kernel   ★ **완료**(2026-08-21, `DoD-46`, production 미연결)
    `docs/plans/2026-08-21_1502_scheduler_grant_dispatch_v1.md`.
    설계 조사에서 실제 계약 불일치 7건을 미리 찾고, private
    `crates/coordinator/src/orchestrate.rs`의 `pub(crate)` 함수가 한 번의
    `pool_snapshot()`을 `evaluate_eligibility()`에 넣어 0/1/N을 분기하고,
    N에서만 `rank_best_fit()`을 호출한 뒤 caller-supplied ID·coordinator term·
    시각·Lease 수명으로 `stage_queued_with_lease()`를 호출하도록 구현했다.
    selected GPU UUID/Grant scope·Manifest adapter·node/device/session routing·
    wire/ACK/rollback은 범위 밖이다. 특히 inventory revision/CAS reservation이
    없어 서로 다른 Job의 순차 호출도 unchanged inventory에서 같은 GPU를 중복
    선택할 수 있으므로 module은 비공개이고 유일한 호출은 `#[cfg(test)]` fixture다.
    production `run()`/accept-loop는 기존 별도 `issue_grant()`를 유지한다. 독립
    검수가 격리·0/1/N 분기·뮤테이션 2건·unchanged-inventory에 한정된 replay
    의미·변경 범위를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가
    `cargo test -p gputeer-coordinator` 78/78을 직접 재확인했다. **scheduler
    로드맵 9단계 중 조각 1·2a·2b-1·3a·4·5 완료(5는 production 미연결
    kernel만) — 남은 조각 3 나머지·inventory CAS reservation(6번 불일치 해결)·
    실제 wire 연결·조각 6~9는 후속**.
33. scheduler 로드맵 조각 5b — inventory revision 기반 CAS reservation   ★ **완료**(2026-08-21, `DoD-47`, production 미연결)
    `docs/plans/2026-08-21_1537_scheduler_inventory_cas_v1.md`.
    `CandidateSnapshot.inventory_revision`을 inventory projection부터 선택까지
    보존하고, `reserve_node_and_stage_queued_with_lease()`가 operation replay →
    revision 비교 → `node_id` PRIMARY KEY reservation → Attempt/Lease/fence →
    `QUEUED→STAGING` → operation 기록을 하나의 `BEGIN IMMEDIATE` transaction에서
    원자 처리한다. CAS·점유 충돌은 자동 재시도 없이 즉시 실패하며 기존 DoD-43
    `stage_queued_with_lease()` 시그니처와 reservation 없는 동작은 유지한다. 독립
    검수가 transaction 원자성·rollback fence 미소비·CAS/점유 경쟁·실제 Barrier
    경쟁의 정확히 한 성공과 loser QUEUED·뮤테이션 2건·DoD-43 무회귀·범위를 확인해
    **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 테스트 86/86을 직접
    재확인했다. node-exclusive라 같은 node의 다른 GPU도 동시에 쓸 수 없고 release가
    없으며, private `orchestrate` module은 여전히 production `run()`에 연결되지 않았다.
    **scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a·4·5·5b 완료 — GPU별 세부
    allocation/release·조각 3 나머지·실제 wire 연결·조각 6~9는 후속**.
34. deterministic selected GPU assignment 순수 kernel   ★ **완료**(2026-08-24, `DoD-48`, `DoD-46` 계약 불일치 3번의 선행 작업)
    `docs/plans/2026-08-21_1036_scheduler_selected_gpu_assignment_v1.md`.
    설계 조사는 production 연결에 선택 GPU 식별자/Grant scope·원본 Manifest adapter·
    node/device/session routing의 남은 계약 3건과 Job submit ingress·durable outbox·
    reservation release가 모두 필요해 하루 규모를 넘는다고 판정했다. 대신 순수
    `resource_fit()`이 health/model/VRAM 적격 GPU를 `(available_vram_bytes, gpu_id)`
    순으로 요구 개수만 선택하고 반환 ID를 `gpu_id` 순으로 정규화하도록 구현한
    선행 조각을 완료했다. `ResourceFit`·`RankedCandidate`·`Staged` outcome이 ID를
    보존하고 single/N 후보는 같은 helper를 쓰며 coordinator는 STAGING 전에 개수를
    재검증한다. 자체 재검토에서 부적격 GPU 제외 직접 증명 공백을 찾아 테스트를
    추가했다. 독립 검수가 순수성·공용 경로·reverse 전체 동등성·뮤테이션 2건·
    제한된 범위를 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 scheduler 53/
    coordinator 87 passed를 직접 재확인했다. 반환 ID는 scheduler snapshot 식별자일
    뿐 실제 NVML UUID provenance가 아니며 Grant/Lease scope 생성·GPU별 reservation/
    release·production wire는 범위 밖이다. **scheduler 로드맵 진행: 조각
    1·2a·2b-1·3a·4·5·5b와 이번 GPU assignment 선행 조각 완료 — 실제 production
    연결은 Job submit ingress·session routing·durable outbox·reservation release가
    모두 없어 아직 하루 규모를 넘는다(설계 조사 판정)**.
35. selected GPU durable reservation binding   ★ **완료**(2026-08-24, `DoD-49`, production 미연결)
    `docs/plans/2026-08-24_1103_scheduler_selected_gpu_reservation_binding_v1.md`.
    `DoD-48`의 canonical `selected_gpu_ids`를 inventory revision CAS, node reservation,
    Attempt/Lease/fence, `QUEUED→STAGING`, operation idempotency와 같은
    `BEGIN IMMEDIATE` transaction에 durable child rows로 저장한다. 빈/공백/중복/비정렬
    요청, 선택 node에 없는 GPU, child 부재·ordinal gap·blank/duplicate/비정렬 저장 손상을
    fail closed로 거부하고 exact replay는 최초 binding을 복원하며 changed payload는
    `OperationConflict`다. 자체 재검토에서 같은 ID가 다른 node에만 있는 fixture를
    보강했고 독립 검수가 실제 node-scoped SQL·binding 직후 전체 rollback·Barrier 경쟁·
    DoD-43 무회귀·뮤테이션 2건·single/N 전달을 확인해 **1라운드 만에 `ACCEPTED`**,
    감독자가 coordinator 92 passed를 직접 재확인했다. reservation release는 실행 종료
    증명 없이 구현하면 중복 실행 위험이 생기므로 후순위다. **scheduler 로드맵 진행:
    조각 1·2a·2b-1·3a·4·5·5b·GPU assignment·GPU reservation binding 완료 —
    Manifest→`JobRequirements` 변환기, reservation release, Grant scope 생성,
    production 연결은 후속**.
36. verified signed `JobManifest` durable binding   ★ **완료**(2026-08-24, `DoD-50`, production 미연결)
    `docs/plans/2026-08-24_1142_scheduler_verified_manifest_durable_binding_v1.md`.
    strict 변환기 자체는 하루에 만들 수 있지만 authoritative device→member 해석과 기본
    `MIRRORED` durability 소비 경로가 없어 orchestration 연결은 안전하지 않다고 판정하고,
    먼저 verified signed Manifest 원본을 accepted Job에 durable하게 묶었다.
    `submit_verified_manifest()`는 `&Verified<pb::JobManifest>`만 받아
    `Verified::get()` 이후에만 identity를 읽고 accepted device와 signer를 대조한다.
    caller hash는 `blake3_256(signing_input(manifest))`로 재계산해 대조·저장하며
    Job/body/signer/idempotency는 한 `BEGIN IMMEDIATE` transaction에 기록된다. load는
    의도적으로 raw `StoredManifestBinding`이어서 authoritative key directory 재검증 전
    scheduler/Grant에 사용할 수 없다. 자체 재검토에서 legacy exact-error assertion과 body
    device identity 손상 fixture를 보강했고 독립 검수는 API type gate·검증 순서·원자성·
    rollback·replay/corruption·뮤테이션 2건·범위를 확인해 **1라운드 만에 `ACCEPTED`**,
    감독자가 coordinator 99 passed를 직접 재확인했다. 첫 workspace 실행의 기존 crypto
    lock-timeout timing test 1회 실패는 알려진 flaky로 재실행 통과했고 판정 근거가 아니다.
    **scheduler 로드맵 진행: `DoD-41`~`DoD-50` 완료 — authoritative device→member 해석,
    `JobRequirements` projection, reservation release(실행 종료 증명 필요), Grant scope 생성,
    production 연결은 후속**.
37. verified terminal `AttemptReport` durable Attempt/reservation binding   ★ **완료**(2026-08-24, `DoD-51`, production 미연결)
    `docs/plans/2026-08-24_1238_terminal_attempt_report_binding_v1.md`.
    authoritative device→member 해석과 기본 `MIRRORED` 소비는 필요한 committed authority와
    replica/failure-domain producer가 없어 1일을 넘고, Job terminal 전이는 규범상
    `RUNNING`·canonical Attempt·final artifact `COMMITTED` guard를 건너뛸 수 없어 먼저
    추가하면 상태기계를 우회한다고 판정했다. 대신 신규 `CoordinatorAttemptReportStore`가
    `&Verified<pb::AttemptReport>`만 받아 signature 포함 body와 저장소 직접 계산 BLAKE3
    hash를 durable first-write fact로 보존한다. 하나의 `BEGIN IMMEDIATE` 안에서 report와
    durable Attempt·현재 reservation을 job/attempt·single node·verified signer·fence·owner로
    5중 대조하며 reservation 부재·owner 불일치·non-terminal outcome은 row 없이 거부한다.
    exact replay는 전체 protobuf 의미와 signer가 같은 기존 행만 변경 없이 반환하고 load는
    authoritative key directory 재검증 전 terminal/release에 사용할 수 없는 raw binding이다.
    자체 재검토에서 미사용 fixture field warning과 오류 문자열을 정리했다. 독립 검수가
    type gate·검증 순서·5중 대조·replay 비우회성·outcome 5종·전체 rollback·control state
    무변경·corruption fail-closed·staging helper 무회귀를 확인해 **1라운드 만에
    `ACCEPTED`**, 감독자가 coordinator 108 passed를 직접 재확인했다. **scheduler 로드맵
    진행: `DoD-41`~`DoD-51` 완료 — authoritative device→member 해석, `JobRequirements`
    projection, `MIRRORED` 소비, Job terminal 전이(`RUNNING`·artifact guard 선행),
    reservation release, production 연결은 후속**.
38. verified `CheckpointManifest` durable Attempt/reservation binding   ★ **완료**(2026-08-24, `DoD-52`, production 미연결)
    `docs/plans/2026-08-24_1513_verified_checkpoint_manifest_durable_binding_v1.md`.
    직전 조사들은 "앞으로 나갈 조각이 하루 규모인가"를 물어 계속 없다고 판정했지만,
    이번에는 "선행 조건의 첫 슬라이스가 하루 규모인가"로 질문을 바꿔 이 anchor 조각을
    찾았다. 신규 `CoordinatorCheckpointManifestStore`는
    `&Verified<pb::CheckpointManifest>`만 받고 `Verified::get()` 뒤에만 필드를 읽는다.
    `BEGIN IMMEDIATE` 획득 뒤 같은 transaction에서 current durable Attempt와 reservation의
    job/attempt/producer/verified signer/fence/owner를 대조한 다음 signature 포함 complete
    body와 저장소 계산 BLAKE3 hash를 first-write fact로 저장한다. `root_digest`는
    BLAKE3-256/정확히 32바이트만 허용한다. 자체 재검토에서 초기 구현이 SHA-256 root도
    허용하던 실제 결함을 발견해 production 검사를 고치고 negative case를 추가했다.
    exact replay는 최초 1행을 유지하고 changed replay는 conflict이며, raw load는 `Verified`를
    재구성하지 않고 전체 binding 손상을 재검사한다. 독립 검수가 유일 API/INSERT·검증 순서·
    transaction 안의 대조와 TOCTOU 부재·root 제한·replay/load·rollback·상태 무변경·실제
    production guard 뮤테이션 2건을 확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator
    118 passed를 직접 재확인했다. **scheduler 로드맵 진행: `DoD-41`~`DoD-52` 완료. 이
    anchor 다음은 verified `ReplicaAck`의 checkpoint/root binding과 `MIRRORED` 판정이다.
    membership/ControlStore 계열은 `Signable`·signer identity·lifetime·member 상태 규범 확정
    약 2일, durable 저장까지 누적 약 3일인 별도 과제다**.
39. verified `ReplicaAck` durable checkpoint/root binding   ★ **완료**(2026-08-24, `DoD-53`, production 미연결)
    `docs/plans/2026-08-24_1556_verified_replica_ack_durable_binding_v1.md`.
    신규 `CoordinatorReplicaAckStore`는 `&Verified<pb::ReplicaAck>`만 받고 raw 호출은
    compile-fail doctest로 막는다. 실제 순서는 `Verified::get()`·signature 포함 body encode·
    저장소 BLAKE3 hash 계산 뒤 `BEGIN IMMEDIATE`를 획득하며, 구조 검증·signer↔holder·validated
    DoD-52 anchor·exact root·replay 대조와 INSERT는 같은 transaction 안이다. anchor helper는
    manifest body/hash와 현재 Attempt job/node/fence binding까지 재검사한다. root는 BLAKE3-256/
    정확히 32바이트만 허용한다. PK `(checkpoint_id, holder_device_id, acked_at_unix_ms)`는
    immutable observation history를 보존해 exact replay는 최초 행을 유지하고 later observation은
    별도 행이다. 자체 재검토에서 PK time BLOB 손상 행을 기존 key로 단건 조회할 수 없는 test
    설계 결함을 찾아 list 경로의 fail-closed 검사로 바꿨다. 독립 검수는 정확한 lock 전·후 순서,
    raw load·corruption·rollback·상태 무변경과 production anchor-root/body-hash guard 뮤테이션을
    확인해 **1라운드 만에 `ACCEPTED`**, 감독자가 coordinator 121+5 passed를 직접 재확인했다.
    **scheduler 로드맵 진행: `DoD-41`~`DoD-53` 완료. `MIRRORED` 판정 입력은 durable해졌지만
    holder별 dedup/freshness·retention, effective count, membership/failure-domain 판정과 production
    소비자는 후속이다. membership/ControlStore 규범 확정 약 2일, durable 저장까지 누적 약 3일인
     별도 과제다**.
40. holder/freshness/membership 해석 입력 기반 effective replica count 순수 kernel   ★ **완료**(2026-08-24, `DoD-54`, production 미연결)
    `docs/plans/2026-08-24_1634_effective_replica_count_kernel_v1.md`.
    `crates/checkpoint/src/durability.rs:178`의 `evaluate_effective_replicas()`는 외부 resolver가
    holder별 freshness와 current signature/membership·ephemeral·failure-domain을 해소한
    observation만 받아 입력 검증·정렬·`BTreeMap`/`BTreeSet` 집계로 effective count와
    counted/excluded/superseded report 전체를 결정적으로 계산한다. 시계·TTL·I/O·DB·network·
    난수·membership 조회·전역 상태를 쓰지 않는다. 4개 observation의 실제 4! = 24개 순열에서
    report 전체를 비교하고, 같은 holder의 복수 selected는 후보 계산 전에 fail closed한다.
    오래된 관측을 selected, 최신 관측을 superseded로 둔 반례로 timestamp 최대값/TTL을 만들지
    않음을 고정했으며 replica 규범에 없는 `ONLINE` 조건도 추가하지 않았다. 자체 재검토에서
    미해석 ephemeral 사실을 모든 kind에 요구하던 과잉 조건을 찾아 `WORKER_LOCAL`에만 적용하도록
    고치고 양방향 회귀 테스트를 추가했다. duplicate-selected와 `WORKER_LOCAL` guard production
    뮤테이션 2건이 각각 지정 테스트를 실패시켰고 원복 뒤 통과했다. 독립 검수는 순수성·순열·
    fail-closed·TTL/ONLINE 부재·ephemeral 양방향·범위와 hard-filter 33/33, best-fit 20/20,
    상태표 parity 5/5, legacy `ReplicaSet` 2/2 회귀를 확인해 **1라운드 `ACCEPTED`**했다.
    감독자는 checkpoint 핵심 suite 13+7+7+5+5 passed를 직접 재확인했다. 기존 `ReplicaSet`은
    실행 코드가 같고 legacy 한계 주석만 보강됐으며 public API는 additive re-export뿐이다.
    **scheduler 로드맵 진행: `DoD-41`~`DoD-54` 완료. `DoD-52` anchor → `DoD-53` ACK 저장 →
    `DoD-54` count kernel로 `MIRRORED` 판정의 입력과 계산이 갖춰졌다. 남은 것은 authoritative
    membership/failure-domain resolver, durable ACK consumer와 전이 적용이다. membership/
    ControlStore 규범 확정 약 2일, durable 저장까지 누적 약 3일인 별도 과제다**.
41. 서명·저장·전이와 분리된 GPU `ScopeCandidate` 순수 kernel   ★ **완료**(2026-08-24, `DoD-55`, production 미연결)
    `docs/plans/2026-08-24_1700_gpu_scope_first_slice_v1.md`.
    Grant/Lease scope를 막던 원인을 실물 RTX 4070 SUPER NVML 실측 뒤 재판정했다. UUID·PCI·
    compute capability·VRAM 등 하드웨어 값 부재는 해소됐지만 그 관측의 authoritative
    provenance 차단은 그대로여서 full scope 대신 계산만 분리했다. `crates/scheduler/src/scope.rs:130`의
    `gpu_scope_candidate()`는 관측·GPU 요구·명시 CPU/RAM/workspace/prefix와 caller가 판정한
    `ProvenanceGate`만 읽고 I/O·clock·환경변수·난수·crypto·전역 상태 없이 typed validation,
    정렬, `BTreeMap`/`BTreeSet` 계산으로 `ScopeCandidate`를 만든다. GPU를
    `(available_vram, gpu_id)` 전체 튜플로 선택하고 결과 ID를 재정렬하며, 동점 GPU를 포함한
    4개 입력의 실제 4! = 24개 순열에서 candidate 전체를 비교한다. `Verified`는 caller 입력일
    뿐 kernel이 signature/membership을 검증하지 않으며 Unverified는 즉시 typed error다.
    PARTITIONED는 mode claim과 무관하게 항상 거부하고, CUDA runtime은 규범 없는 호환 규칙을
    발명하지 않으며, `DerivedFromTotalAndReserved`는 authoritative VRAM으로 쓰지 않는다.
    자체 재검토에서 proto CUDA runtime 요구가 타입에서 소실될 결함을 고쳐 필드·typed unresolved·
    negative test를 추가하고 model/driver/compute의 irrelevant fact 과잉 요구를 제거했다.
    provenance gate와 PARTITIONED 거부 production 조기 반환 뮤테이션 2건은 각각 지정 테스트를
    실패시켰고 원복 뒤 통과했다. 독립 검수는 순수성·동점/24개 순열/전체 결과·provenance 경계·
    PARTITIONED 상시 거부·CUDA 규칙 부재·파생 VRAM 미사용과 신규 14 + hard-filter 33 + best-fit
    20 회귀를 직접 확인해 **1라운드 `ACCEPTED`**, 감독자도 scheduler 67 passed를 재확인했다.
    **scheduler 로드맵 진행: `DoD-41`~`DoD-55` 완료. 하드웨어 값 부재 차단은 해소됐고 계산
    부분은 순수 kernel로 분리됐지만 authoritative provenance 차단은 그대로여서 full Grant/Lease
    scope는 여전히 막혀 있다. membership/ControlStore 계열은 규범 확정 포함 누적 약 3일의
    별도 과제다**.
42. Stage 1 조각 7 — Linux cgroup v2 자원 상한 강제   ★ **완료**(2026-08-30, `DoD-57`)
    x600 의 WSL 이 응답하게 되면서 실측이 가능해져 `crates/runtime-linux`
    를 신설하고 Agent 의 Linux 실행 경로에 연결했다. 순서가 핵심이다 —
    하위 cgroup 생성 -> memory.max + memory.swap.max -> fork 후 exec
    **전에** pre_exec 로 자기를 cgroup 에 투입 -> exec. 3-4 를 바꾸면
    남의 코드가 상한 밖에서 먼저 돈다(runtime-windows 의
    CREATE_SUSPENDED 와 같은 이유).

    ★ **실측이 초안을 두 번 반증했다.** (1) memory.max 만 걸었을 때
    32MiB 상한에 90MB 를 할당했는데 자식이 **정상 종료했다** — cgroup 은
    스왑으로 밀어낼 뿐이다. (2) 부모 cgroup 을 "내 cgroup" 으로 고정하니
    /init.scope 에서 설계대로 실행을 거부했다(내부 프로세스 금지 규칙).

    ★ **못 막는 것을 실제로 재 봤다.** 검수가 "적대적 코드는 못 막는다"
    고 지적했을 때, 문서에 적기만 하지 않고 자식이 상위 cgroup.procs 로
    나가는 것을 실행해 확인했다. **탈출이 성공하기를 기대하는 테스트**를
    남겨, 구멍이 닫히면 그 테스트가 실패하며 문서도 같이 고치라고 알린다.

    독립 검수 3라운드가 12개 지적(운영 경로 미연결·탈출·루트 폴백·스왑
    추정·이름 충돌·회수 불가·탈출 테스트 오통과·system.slice 우회·조상
    우회 등)을 파일:줄로 냈고 전부 코드로 고쳤다. 남은 한계(bind mount,
    systemd 위임 slice 미지원, 적대적 코드 탈출)는 문서에 명시했다.

43. Stage 2 조각 12 — 다중 Agent 동시 처리 lane   ★ **완료**(2026-08-30)
    `DoD-40` 이 "2~4일 아키텍처 변경" 으로 이월했던 것을, 순차 `run()` 을
    건드리지 않고 별도 lane 으로 열었다 — 기존 시나리오 무회귀.

    ★ 검수가 시나리오의 **공허성**을 지적했다: 두 Agent 를 연달아 띄우고
    둘 다 성공했는지만 보면 **순차 서버로도 통과한다.** 겹칠 때까지
    붙잡아 두는 방법은 확률이라 느린 기계에서 무의미해진다. 대신 관문을
    뒀다 — 각 세션이 2개 세션이 동시에 열릴 때까지 기다린다. 순차 서버는
    두 번째 연결을 아예 안 받으므로 **구조적으로 통과할 수 없다.**
    뮤테이션(순차 처리로 바꾸기)으로 시나리오 88 이 실제로 실패함을 확인.

    식별자 분리도 검수가 32비트 해시 충돌 반례를 실제로 만들어 반려했다
    (`agent-47131` 과 `agent-71872` 가 둘 다 `e7525a3b`). 축약을 없앴다.

44. Stage 2 조각 14 첫 조각 — 노드 생존 판정 + 관측 영속화   🟡 **진행 중**(2026-08-30)
    `ADR-033` §7 이 이미 답을 정해 뒀다 — 관측(신고)과 판정(결정)을
    나누고, **"연락이 안 된다" 는 "죽었다" 가 아니다**. 네트워크가
    갈라졌으면 대상 노드는 멀쩡히 실행 중이고, 그 상태에서 다른 GPU 에
    다시 띄우면 두 번 돈다.

    `classify_node_liveness()` 순수 커널에는 `Dead` 값이 **없다** — 가장
    나쁜 판정이 `Silent` 이고 그건 사실 진술이지 결정이 아니다.
    ★ 다만 검수가 정정했듯 **`Dead` 부재 자체는 강제 장치가 아니다** —
    지금 안전한 이유는 재배정 소비자가 아직 하나도 없어서다.

    ★ 이 커널은 `ADR-033` §7 의 **구현이 아니라 선행 조건**이다. §7 의
    입력은 이웃의 서명된 신고인데 이 커널은 노드 자기보고를 받는다 —
    검수가 이 차이를 짚어 문서를 정정했다.

    `CoordinatorNodeLivenessStore` 로 `NodeRecord.last_heartbeat_unix_ms`
    공백(§7 이 "값을 채우는 관측자가 없다" 고 지목)을 채우고 wire 에
    연결했다(시나리오 92). 노드당 한 행, 뒤로 안 감, 읽을 때 전체 재대조.

    ★ **재배정 결정(§8 의 여섯 조건)은 `DoD-61`, reservation release 는
    `DoD-62`, 이웃 신고 wire 메시지는 `DoD-63` 으로 완료**(아래 48·49·50번).
    ⇒ **조각 14 의 조각들이 전부 끝났다.** 관측을 **모아 두는 곳**까지
       `DoD-64` 로 채웠다(아래 51번). 남은 것은 이 재료들을 실제로 소비하는
       production 경로인데, 그건 `ADR-033` §8 조건 2 의 강제 수단이 생기기
       전에는 켜면 안 된다(`DoD-62` 참조).

45. Stage 2 조각 17 — Linux 운영용 키 보관   ★ **완료**(2026-08-30)
    x600 WSL 에 systemd 259 가 PID 1 로 돌아 실측이 가능해졌다. Linux K1
    을 `systemd-creds --with-key=host` 로 구현했다(새 의존성 없음).

    ★ **같은 "K1" 이 두 플랫폼에서 다른 것을 막는다** — Windows(DPAPI)는
    다른 **사용자**를, Linux(host key)는 **비-root** 를 막는다. 같다고
    쓰면 안 되므로 표로 나눠 적었다. Linux 는 Agent 가 root 여야 성립하고,
    평문이 커널 파이프와 systemd-creds 프로세스 메모리에도 존재한다 —
    "명령줄·임시 파일 누출을 피했다" 가 정확하지 "누출 없음" 이 아니다.

    ★ **검수와 내가 독립적으로 같은 결함을 찾았다** — 봉인이 signer 에
    안 묶여 있어 남의 키쌍을 이식할 수 있었다. 고친 뒤 검수가 **더 깊은
    우회**를 냈다: 개인키 blob 을 **비워** 공개키 전용 엔트리로 만들면
    복호를 건너뛰어 그 방어가 무의미해진다. 근본 원인은 파일 체크섬이
    **키 없는** BLAKE3 였다는 것 — 체크섬을 봉인해 위조에 OS 비밀이
    필요하게 만들고 파일 버전을 2 로 올렸다(v1 은 못 읽는다 — legacy
    fallback 은 원래 공격을 되살린다).

    ★ 이 테스트를 **두 번 공허하게** 썼고 둘 다 뮤테이션이 잡았다.
    1차는 봉인 blob 만 교환해 기존 "개인키·공개키 불일치" 검사가 잡았고,
    2차는 공개키까지 교환했지만 **파일 체크섬이 먼저** 막았다. 체크섬도
    다시 계산해서야 봉인을 실제로 쟀다.

46. **막힌 항목 — 지어내지 않고 그대로 둔다**
    - **Stage 2 조각 11(멤버십)**: `docs/plans/2026-08-24_1830_membership_norm_draft_v1.md`
      이 사용자 결정 4건(root key rotation·권한 주체·상태 전이 채택·
      mutation TTL)을 명시적으로 남겨 뒀고, 더 근본적으로 membership
      authorization 이 `COMMITTED`(과반 합의)를 필수 조건으로 요구하는데
      SingleNodeStore 로는 제공할 수 없다. 다중 노드/Raft 가 선행이다.
    - **Stage 2 조각 16(TLS)**: 기준선 §25.2 는 QUIC + TLS 1.3 을 정했고
      §42.7.3 은 인증서 신원 방식을 **미정**으로 남겼다(Device
      Certificate mTLS vs 단기 세션 토큰). 지금 모든 메시지는 이미
      Ed25519 로 서명·replay 방어되므로 TLS 가 더하는 것은 **기밀성**
      이다 — 그건 실제로 필요하지만, 신원 체계를 하나 더 만들면 두
      체계가 어긋나는 것이 진짜 위험이다. 규범 결정이 선행이다.

47. **evidence 부채 해소** — 42·43·44·45 전부 `DoD-57`~`DoD-60` 으로
    정식 기록 완료. evidence 71건(PASS 70), 스키마 위반 0.

48. Stage 2 조각 14 둘째 조각 — `ADR-033` §8 재배정 관문   ★ **완료**(2026-08-30, `DoD-61`)
    `liveness.rs` 가 남긴 문장("재배정을 만들 때는 §8 의 여섯 조건을
    타입으로 요구하는 별도 관문이 필요하다")을 이행했다.

    ★ **ADR 의 금지를 문서가 아니라 값으로 옮겼다.** §8 은 조건 2 에
    "이 규범을 강제하는 코드가 아직 없으니 그 전까지 이 경로를 켜면 안
    된다" 고 적어 뒀다. 문서에만 적으면 누군가는 켠다. 그래서 조건 2 는
    만료 시각만 보지 않고 `PartitionPauseEnforcement` 를 함께 요구하고,
    기본값이 없으므로 호출부가 반드시 진술해야 한다 — 오늘 정직한 값은
    `NotEnforcedYet` 하나뿐이라 **나머지가 완벽해도 거부된다.**

    ★ 다만 이건 **강제가 아니라 요구**다. 순수 커널은 서명을 검증할 수
    없다(검증하려면 crypto 에 묶여 순수성이 깨진다). 값어치는 "막는다"
    가 아니라 **"거짓말 없이는 통과 못 하고 그 거짓말이 호출 지점에
    남는다"** 다. production 소비자는 하나도 없다.

    **독립 검수 10라운드.** 무거운 것 셋:
    - 이미 쓴 105 보다 낮은 104 를 허용했다(범위 시작만 보고 개별 번호의
      단조성을 안 봤다). `fenced_operation.rs` 가 `StaleFence` 로 거부하는
      바로 그 상황이었고, 정직하게 현재 번호를 넣으면 오히려 입력 오류가
      나서 "범위를 다 쓸 때까지 재할당" 과 정반대였다.
    - "이웃 N **대**" 를 멤버 수로 셌다. 한 멤버의 기계 두 대가 한 표,
      한 기계의 멤버 ID 둘이 두 표가 됐다.
    - 내가 "어느 계층도 ULID 를 강제 안 한다" 고 썼는데 **거짓**이었다 —
      `fenced_operation.rs` 가 job/attempt 를 26자로 거부한다.

    ★ **10라운드 중 6개가 코드가 아니라 내 서술의 과장을 지적했다** —
    "타입으로 강제한다"(못 한다·요구할 뿐), "아무것도 허용 못 한다"(테스트는
    오늘도 Allowed 를 받는다), "정본과 같은 규칙"(다른 상황이다), "다른
    기록을 못 붙인다"(값이 어긋난 것만 거른다), "담합한 다수와 같은
    종류"(다른 공격 모델이다), "같은 멤버는 한 표"(같은 기계가 한 표다).
    **한계를 적었다고 정확한 것은 아니다** — 한계를 적으면서도 그 위
    문장에서 보장을 부풀리고 있었다.

    뮤테이션 29건 전부 지정 테스트를 동작 수준에서 실패시켰다.

49. Stage 2 조각 14 셋째 조각 — 예약 해제 경로와 증명 관문   ★ **완료**(2026-08-30, `DoD-62`)
    `DoD-49` 가 "실행 종료 증명 없이 구현하면 중복 실행 위험이 생긴다" 며
    미뤄 둔 reservation release 를 만들었다 — **오늘 쓸 수 없는 상태로.**

    ★ **내 전제가 틀렸고 검수가 그걸 반박했다.** 초안은 "`DoD-51` 의
    검증된 terminal `AttemptReport` 가 실행 종료의 증명" 이라고 전제했다.
    그런데 `DoD-51` evidence 가 정반대를 적어 뒀다 — "저장 성공은 프로세스
    종료를 증명하지 않으며 reservation release 의 충분조건이 아니다".
    terminal 보고서는 노드 **자기보고**다. 정상 키로 "끝났다" 를 서명해
    놓고 계속 돌면 위조도 DB 조작도 없이 GPU 가 남에게 넘어간다.

    그래서 계획서의 "안전한 release proof 최소 형태" 4조건 중 1번만 코드로
    확인하고, 2·4번은 값으로 요구한다 — 기본값이 없으므로 호출부가 반드시
    쓰고 오늘 정직한 값은 전부 "아직 증명 못 함" 이라 **아무 예약도 풀지
    못한다.**

    ★ **만족시킬 수 없는 것은 요구하지 않는다.** 초안은 조건 3(전이를 같은
    트랜잭션에 결합)도 진술로 요구했는데, 이 메서드가 자기 트랜잭션을
    소유하므로 호출부가 **정직하게 만족시킬 방법이 없었다**(검수 2라운드).
    요구하면 통과하려는 사람은 거짓말밖에 할 수 없다 — 그 진술을 없애고
    사실을 문서에 적었다.

    ★ **내 뮤테이션이 내 테스트 2건과 죽은 코드 1건을 잡았다** — GPU 목록을
    반환값으로만 확인해 자식 행을 안 써도 통과했고, 옛 fence 테스트가
    `matches!` 로 두 오류를 다 받아 어느 관문이 막았는지 몰랐다. 그리고
    내가 넣은 Attempt fence 재대조는 `fetch_report_binding` 이 이미 하므로
    **도달 불가능한 죽은 코드**였다 — 지웠다.

    뮤테이션 14건 전부 동작 수준에서 실패. 독립 검수 3라운드 `ACCEPTED`.

50. Stage 2 조각 14 마지막 조각 — 이웃 신고 wire 메시지   ★ **완료**(2026-08-31, `DoD-63`)
    `lease.proto` 주석이 "이건 pool membership 이 먼저 있어야 한다" 며
    미뤄 뒀던 `ADR-033` §7 의 관측 층을 넣었다 — 정확히는 **메시지 정의와
    멤버십 해소가 다른 일**이라 앞의 것만 넣는다.

    ★ **판정 필드가 하나도 없다.** §7 이 "관측을 판정 결과로 승격하지
    않는다" 고 못박았으므로 `is_dead` 같은 값을 넣지 않았다. 초안에 있던
    `last_contact_at`·`failed_attempt_count` 도 **뺐다** — §7·§8 이 요구
    하지 않고 소비자도 없는데 모순된 값(`last_contact > observed`)도 서명만
    맞으면 통과해 공격 입력면만 넓혔다.

    ★ **강제 못 하는 것은 테스트로 열어 둔 채 고정했다** — 한 장치 키가
    여러 기계 ID 를 서명할 수 있다는 것, 프레이밍 계층이 수신 Coordinator
    를 대조하지 않는다는 것. 둘 다 **통과하는 테스트**로 남겨, 나중에
    닫히면 실패하며 문서도 같이 고치라고 알린다(`runtime-linux` 의 "탈출이
    성공하기를 기대하는 테스트" 와 같은 장치).

    **독립 검수 12라운드.** ★ 그중 **절반이 코드가 아니라 내 서술의 오류**
    였고, 가장 무거운 것은 재생 방어의 이유를 **반대로** 적은 것이다 —
    "재생하면 이웃 하나가 정족수를 혼자 채운다" 고 세 곳에 썼는데,
    `reassignment.rs` 가 기계 ID 로 중복 제거하므로 N 번 넣어도 한 표다.
    재생이 막는 것은 **신선도 위조**다.

    ★ **부수 소득 — 같은 결함의 다섯·여섯 번째 자리를 닫았다.**
    `canonical_vectors.rs` 의 수동 배열이 30종 중 28종만 검사하고 있었고
    (2026-08-19 에 한 번 고쳤는데 **배열로** 고쳐서 또 낡았다), 규범 문서와
    Python 참조 구현은 대조 장치가 아예 없었다. `Domain::ALL` 순회로 바꾸고
    **메시지 → tag 대응**을 세 자료에서 파싱해 대조하는 테스트를 만들어
    우회 9가지를 재현·확인했다.

    뮤테이션 8건 + 우회 재현 9건 전부 동작 수준에서 잡혔다.

51. `ADR-033` §7 의 관측을 **모아 두는 곳** — 이웃 신고 저장소   ★ **완료**(2026-08-31, `DoD-64`)
    §7 이 층을 둘로 나눴는데(이웃이 **관측**을 서명해 보고 / Broker 가 그 보고를
    **모아** 판정), 신고 메시지는 `DoD-63` 이, 판정 관문은 `DoD-61` 이 만들었지만
    **그 사이가 비어 있었다** — 신고가 도착해도 남지 않았다.

    ★ **정족수를 세지 않는다.** 행을 돌려줄 뿐이고 각 행에 서명 장치를 실어
    보낸다 — 장치 하나가 여러 기계 ID 를 주장할 수 있다는(이 저장소가 막지
    못하는) 사실을 호출부가 **값으로 보게** 하기 위해서다.

    ★ **`DoD-63` 이 남긴 수신자 대조를 닫았다** — 이 저장소가 첫 소비자다.
    쓰기만으로는 부족해서 **읽기 경로에서도** 대조한다(이미 행이 든 파일을 다른
    Coordinator 가 열면 남에게 보낸 신고를 자기 것으로 읽는다).

    ★★ **독립 검수 12라운드. 이 저장소에서 가장 긴 검수였고, 그럴 만했다.**

    **방어 하나가 여섯 개의 새 표면을 열었다.** §0.5 를 지키려고 넣은 저장
    상한이 이렇게 번졌다 — 상한 → **정당한 이웃이 새 노드를 영영 신고 못 함**
    → 축출로 해결 → **여러 행이 함께 삭제**(삭제 키에 신고자 누락) → 키 수정
    → **손상된 행이 조용히 사라짐** → 지우기 전 검증 → **후보 고르는 단계가
    손상에 속음**(시각 컬럼을 크게 만들면 정렬 뒤로 숨는다) → 전수 검증 →
    **이미 상한 넘은 DB 를 복구 못 함**. 프로덕션 결함 여럿이 **직전 라운드
    수정의 산물**이다.

    ★ **내 테스트가 여덟 라운드 내리 같은 방식으로 느슨했다** — 개수만 보기,
    `is_some()` 만 보기, `..` 로 오류 필드 버리기, 그리고 가장 나쁜 것:
    **DB 에서 읽은 값을 그 DB 를 검사할 기대값으로 쓰기**(저장과 조회가 함께
    틀리면 통과한다). 기대값을 입력에서 만들도록 바꾸고, 프로덕션과 테스트가
    같은 인코더를 쓰는 남은 위험은 **골든 벡터**(저장된 몸통 바이트·BLAKE3 를
    고정)로 못박았다 — 검수가 제안한 것을 받아들였다.

    ★ **서술 지적이 전부 한 방향이었다** — 코드보다 강한 주장. 실제보다 약하게
    쓴 것은 한 건도 없다. "두 축을 함께 묶는다"(안 묶는다)·"세지 않는다"(상한용
    으론 센다)·"하나의 트랜잭션 안에서"(수신자 대조는 밖이다)·"검증된
    서명자"(읽을 때 재검증 안 한다)·"어떤 행이든"(`LIMIT 1` 이었다)·"다음 기록
    때"(**새 행** 기록 때만), 그리고 **테스트 이름 자체가 거짓말**이던 것
    (`eviction_never_removes_more_than_one_row` — 초과 복구는 의도적으로 7행을
    지운다). `DoD-61`·`DoD-63` 에 이어 **세 번째** 같은 패턴이다.

    ★ 그 밖에 잡힌 것 — `lib.rs` 에 모듈 한 줄을 끼워 넣으면서
    **`#[allow(dead_code)]` 를 가로채** 원래 대상에서 떼어냈고, 거부할 때
    **판단에 쓴 값과 다른 값을 보고**했으며(`CLAUDE.md` §3 위반),
    **손상을 장치 충돌로 가렸다**(원인이 다르면 대응도 다르다).

    테스트 33건, 뮤테이션 **26건 전부 동작 수준에서 실패**. Windows·x600 Linux
    양쪽 실측. ★ **production 소비자는 만들지 않았다** — `ADR-033` §8 조건 2 의
    강제 수단이 없는 한 재배정 경로를 켜면 안 된다(`DoD-62` 참조).
52. `DoD-64` 저장소를 **실제 wire 경로에 연결** — 이웃 신고 송수신·저장   ★ **완료**(2026-09-01, `DoD-65`)
    `DoD-64` 자신이 "wire 수신부에서 이 저장소를 호출하는 경로를 만들지 않았다" 고
    적어 둔 한계를 닫는다. Agent 가 서명한 신고를 보내고, Coordinator 가 받아
    검증한 뒤 영속 저장소에 남긴다.

    ★ **무게중심은 송수신이 아니라 거부다.** 이웃 신고를 **실제로 다루지 않는
      실행 경로**가 그 옵션을 받아들이면 아무 일도 안 일어나는데 아무 오류도
      안 난다 — 운영자는 신고가 모이는 줄 안다. **조용한 무시는 실패보다 나쁘다.**

    ★★ **관문이 자기가 어디 있는지를 남에게 물었다**(11라운드 우회 조사).
      `run_multi_agent()` 는 **그 함수 자체가 multi-agent lane** 인데 관문은
      `config.multi_agent` 를 읽어 판정했다 — 플래그를 끄고 부르면 "순차 lane
      이구나" 하고 통과했다. 이제 `NeighborReportLane` 을 **인자로 받아**
      진입점이 스스로 말한다.

    ★ **거부 대상은 lane 만이 아니다.** 순차 lane 안에도 ACK 직후 세션을 끝낼
      수 있는 test hook 이 셋 있다. 그중 하나는 특정 조건에서만 실제로 끊지만,
      **닿는다고 보장할 수 없는 구성**을 받아 주지 않는다.

    ★ **원인이 다르면 이름도 달라야 한다** — lane 충돌을 `kind=storage` 로 찍고
      있었고 **테스트가 그 잘못된 분류를 고정**하고 있었다. `STARTUP_REFUSED` 로
      나눴다(`CLAUDE.md` §3).

    ★ **순서를 문구가 아니라 값으로 드러낸다.** `READY` 부재는 "bind 전에
      막았다" 를 증명하지 못한다(bind 뒤 READY 전에 죽어도 같아 보인다):
        · 점유된 포트를 준다 — 관문이 먼저면 lane 오류, 나중이면 bind 실패
        · 테스트가 listener 를 소유한다 — `accept()` 가 `WouldBlock` 이면
          **연결 시도 자체가 없었다**
        · 대조 테스트는 반대로 **연결이 실제로 일어났음**을 요구한다

    **독립 검수 14라운드.** ★ **11라운드는 세 갈래를 동시에 돌렸다**(종합 ·
    서술 감사 · 우회 조사) — 사용자가 "코덱스랑 병렬 작업 되는 건 다 걸고
    돌려라" 고 지시해서다. 같은 코드를 같은 시각에 셋이 봤는데 **넓게 본
    종합은 `ACCEPTED` 였고 좁게 판 둘이 가장 무거운 결함을 찾았다.** 넓게
    물으면 "관문이 있나" 에, 좁게 물으면 "그 관문이 옳은가" 에 답한다 —
    순차로 돌렸으면 종합의 `ACCEPTED` 에서 멈췄을 것이다.

    ★★ **9·10·12·13라운드가 연속으로 "정정 자체가 만든 오류" 를 잡았고 매번
      형태가 달랐다** — 문서를 고치다 **엉뚱한 함수 위**를 고침(파싱을 떼어내며
      옛 문서 블록이 밀려난 걸 못 봄), 가드 함수를 끼워 넣다 **`run()` 의
      rustdoc 를 가로챔**, 과장을 고치며 **다른 과장을 새로 씀**, 정정을 아래에
      덧붙이고 **위의 낡은 문장을 남겨** 한 주석 안에서 앞뒤가 모순됨.
      `DoD-64` 에서 모듈 한 줄을 끼워 넣다 `#[allow(dead_code)]` 를 가로챈 것과
      같은 종류가 **두 번째**다 — **삽입 위치가 위쪽 속성·문서의 소속을 바꾼다.**

    ★ 그 밖에 잡힌 것 — 관문을 네 자리에 뒀는데 **셋이 한 번도 실행되지
      않았다**(모든 시나리오가 CLI 를 거쳐 CLI 관문이 먼저 걸렸다), `run()` 으로
      옮긴 lane 분기를 **아무 테스트도 고정하지 못함**, `127.0.0.1:1` 이
      거부된다는 걸 **가정**했다(점유하지도 확인하지도 않았다), `:memory:` 가
      "관측을 모아 둘 수 없다"(사실이 아니다 — 잃는 것은 재시작 내구성이다).

    Windows selftest 97 시나리오 3회 연속 · 워크스페이스 826 passed · Linux
    242 passed · 뮤테이션 18건 전부 동작 수준 실패.
    ★ 시나리오 93~97 은 **Windows 에서만** 측정됐다(x600 은 시나리오 80 의
      cgroup 위임 부재로 selftest 전체가 멈춘다 — `DoD-57` 기록). 신규 crate
      테스트 10건은 양쪽 플랫폼에서 측정됐다.
    ★ **production 소비자는 만들지 않았다** — `ADR-033` §8 조건 2 의 강제
      수단이 없는 한 재배정 경로를 켜면 안 된다(`DoD-62`).
53. `DoD-56` 이 스스로 남긴 검증 부채 — NVML 불변식 셋   ★ **완료**(2026-09-01, `DoD-66`)
    그 evidence 가 적어 둔 문장을 그대로 옮기면: "다음 세 주장은 **실측 1회로
    받침될 뿐 자동 검사로 고정되지 않았다**" — UUID 정렬의 열거 순서 무관성,
    96바이트 버퍼 상한, MIG 를 `current` 로 판정.

    ★★ **핵심은 "떼어냈다" 는 것이다.** 셋 다 FFI 호출과 관측 경로 **안에**
      묻혀 있었고, 묻혀 있으면 GPU 없이는 못 잰다. 못 재니까 테스트가
      공허해졌다 — 정렬 테스트는 **관측 결과를 다시 정렬해 자기와 비교**했다.
      **정렬을 통째로 지워도 통과한다.**

      떼어내니 GPU 한 장도 필요 없어졌다 — 합성 4장의 **서로 다른 24개 순열
      전부**, MIG 두 값을 어긋나게, NUL 없는 96바이트 버퍼.

    ★ `index` 는 정렬이 안 건드린다(관측 사실이다). `pending` 은 의도적으로
      안 쓴다(재부팅 뒤 값이다) — `let _ = pending;` 을 남겨 그 선택이 실수가
      아니라 결정임을 코드로 보인다.

    ★★ **"재려는 대상으로 기대값을 만들지 않는다" 가 이 조각에서 두 번 나왔다**
      (`DoD-64` 포함 통산 세 번째):
        · 버퍼 경계 테스트가 버퍼를 `NAME_BUFFER` **상수로** 만듦 — 상수를 64 로
          줄이면 테스트 버퍼도 같이 줄어 통과(**내 뮤테이션이 잡았다**)
        · 순열 테스트가 기대 행을 `normalize_gpu_order()` 로 만듦 — UUID·index 는
          리터럴로 봤지만 나머지 필드가 함수 출력에서 왔다(**검수 1라운드가 잡았다**)
      왜 잘 안 보이는지도 알겠다 — 테스트를 쓸 때 "정답이 뭐지?" 를 물으면
      **가장 손쉬운 답이 대상을 한 번 돌려 보는 것**이고, 그게 곧 순환이다.

    ★ **새로운 종류가 하나 나왔다 — 헬퍼의 공허성.** 검수 2라운드가 찾은 것은
      본 테스트가 아니라 **순열 생성기**였다. 개수만 세면 생성기가 정렬된 같은
      순열을 24번 돌려줘도 전부 통과하고, 그러면 순서 독립성을 재려던 테스트가
      순서를 하나도 안 재게 된다.

    ★ 서술 정정 — `CUDA_DEVICE_ORDER` 가 NVML index 를 바꾼다고 적었는데 그건
      **CUDA** 의 열거 순서를 바꾸는 변수다. 정렬이 필요한 진짜 근거는 NVML
      열거 순서가 재부팅 사이에 안정적이지 않다는 것이다.

    뮤테이션 7건 전부 동작 수준에서 실패(N6 은 1라운드 수정 뒤에야, N7 은
    2라운드 수정 뒤에야 잡힌다). 독립 검수 3라운드 `ACCEPTED`.
    ★ **실물 GPU 실측이 아니다** — 규칙의 행동만 고정한다.

54. **가로챈 속성·문서 8건 복구**(2026-09-01, 커밋 `040b5d0`)
    `DoD-65` 에서 같은 실수를 두 번 겪은 뒤 저장소 전체를 훑었다.

    ★★ **`#[allow(dead_code)]` 가 세 번 연속 도둑맞았다.**

        원래     #[allow(dead_code)] + mod orchestrate
        eba4114  pub mod multi_agent 삽입          -> 가로챔
        8134afa  pub mod node_liveness_store 삽입  -> 또 가로챔
        9c0239e  `DoD-64` 에서 "고침"              -> 직전의 틀린 상태로 되돌림

      결과가 정확히 반대였다 — 허용은 `pub mod` 라 경고가 날 일도 없는 모듈에
      붙어 있었고, 정작 필요한 `orchestrate` 는 **경고 5개를 계속 냈다.**
      원래 주인에게 돌려주니 0 이 됐다.
      ★ `DoD-64` 의 수정은 "원래 누구 것인지" 를 안 보고 **직전 상태로만**
        되돌린 것이었다.

    문서 소속 오류 7건 — 전부 같은 모양이다(상수·테스트 헬퍼를 함수 앞에
    끼워 넣으면서 그 함수의 문서를 가로챘다). `const NEWLINE` 위에
    "Coordinator 만 띄워 시작 전에 죽는지 본다" 가 붙어 있던 것이 가장 명백하다.

    ★ 근본 원인은 하나다 — **Rust 에서 속성과 문서는 바로 아래 항목에 붙는다.**
      사이에 무언가를 끼워 넣으면 소속이 조용히 바뀌고, **컴파일도 되고
      테스트도 통과한다.** 새 모듈 선언은 `orchestrate` 의 허용 **위**에 넣는다.

55. **실행 사슬 — 투입부터 전송까지**(2026-09-03, 커밋 `9a8950b`~`8fa2a2b`)
    ★★ **검수 대기 중이다. evidence 는 아직 못 썼다.**

    부품은 거의 다 있었는데 이어지지 않아 **Job 을 하나도 못 돌렸다.**
    끊긴 고리 넷 중 셋을 이었다.

    submit → import-manifest → import-inventory → plan-job → stage-job
                                                        ↘ issue-grant (파일로)
    coordinator-stub --grant-from-control-db ──TCP──▶ agent-stub
    scheduler-tick                                    (①~③ 을 스스로 한 번)

    ★ **무게중심은 "이어졌다" 가 아니라 "별도 프로세스가 받아들였다"** 다.
      `issue-grant` 는 우리가 만든 것을 우리가 검증한 것이라 같은 코드의
      두 방향일 뿐이다. `grant_over_wire` 는 **별도 OS 프로세스인 Agent**
      가 자기 규칙(서명·nested Lease 독립 검증·fence·만료)으로 판정한다.

    ★★ **`scheduler-tick` 의 핵심은 식별자를 저장된 사실에서 유도하는 것.**
      사람이 부르면 운영자가 식별자를 주지만 스스로 도는 것은 만들어야
      하고, 시계나 난수로 만들면 저장소의 operation key 가 약속하는 멱등이
      **거짓말**이 된다. 전부 `(job_id, plan_id)` 와 `queued_at` 에서 낸다.

    ★ 몰랐던 계약 셋을 실측으로 알았다

    · **Lease 발급 시각은 큐 진입보다 앞설 수 없다**
      (`staging clock moved backwards`)
    · **nonce 는 저장된 사실이 아니라 전송 계약이다** — Agent 는 연결 시도
      번호에서 유도한 값을 기대한다. 파일 경로에는 "연결" 개념이 없다
    · **scheduler 의 `node_id` 와 Agent 의 device id 는 다른 이름 공간**인데
      잇는 것이 없다 — 오늘은 운영자가 같게 선언해야만 이어진다

    ★ ★★ ④ 데몬화의 실질적 차단 요인 — 내 테스트가 찾았다

    노드가 **둘**인데 두 번째 tick 이 막힌다:

        TICK_REFUSED: node is already reserved: node=node-tick-a, ...

    `evaluate_eligibility`/`rank_best_fit` 은 **inventory 만** 본다. 예약은
    staging 저장소의 다른 테이블에 있고 둘을 잇는 것이 없다 — best-fit 이
    늘 같은 노드를 골라 예약 관문에서 막힌다. **비어 있는 노드가 있어도.**
    안전엔 문제없지만(관문이 중복을 막는다) **루프를 돌리면 큐 맨 앞에서
    영영 멈춘다.**

    고치려면 규범 결정이 필요하다 — 예약된 노드를 `pool_snapshot()` 에서
    빼는가, hard-filter 에서 거르는가, orchestrate 가 차순위로 재시도하는가.
    셋이 의미가 다르다. **그래서 고치지 않고 현재 동작을 테스트로 고정했다.**

    ★ 자기 검수가 찾은 것 — 뮤테이션 9건 중 6건이 안 잡혔다

    ★ **테스트가 엉뚱한 관문을 재고 있었다.** `--lease-ttl-ms 1` 만 주고
      갱신 오프셋은 기본값을 남겨서, 더 앞의 검사에 걸렸는데 내 단언이 그
      메시지도 받아 줘서 통과했다 — 재려던 것을 하나도 안 쟀다.

    ★ **무게중심이 안 재지고 있었다.** "발급 시각 = queued_at" 이 이 모듈의
      핵심인데 멱등 테스트는 **식별자만** 비교했고 식별자는 시각에서
      유도되지 않는다 — 시계로 바꿔도 통과했다.

    ★ **내 서술이 과장이었고 테스트를 만들려다 알았다.** "`plan_id` 가
      낡은 계획의 예약을 막는다" 고 썼는데, 그 상황을 만들려니 저장소가 더
      앞에서 막았다(`queued Job plan conflict`) — QUEUED 인 Job 의 계획은
      안 바뀐다. 지어낼 수 없는 테스트는 지우고 주석을 사실로 고쳤다.

    ★ **`submit` 에는 있고 Grant 발급엔 없던 자기 검증**을 붙였다. 다만
      확인하는 것은 구조·수명·인코딩이지 **키의 주인이 아니다** — 엉뚱한
      키로 서명하면 통과하고 나중에 Agent 가 거부한다.

    안 잡히는 셋은 **이유가 서로 다르고** 코드에 적었다 — 둘(G5·G6)은 셋이
    한 트랜잭션에 함께 쓰여 어긋난 상태를 정상 경로로 못 만드는 **손상
    방어**이고, 하나(T1)는 위의 `plan_id` 건이다.

    ★ 검수 — 내 판단 착오로 하나도 못 받았다

    종합·서술 감사·경계 조합 **세 갈래를 동시에** 걸었다가 셋 다 판정 전에
    쿼터가 끊겼다(각 131k·95k·60k 토큰). `DoD-65` 에서 3갈래 병렬이
    효과적이었던 것을 따라 했는데 **그때와 지금의 차이를 안 봤다** — 그때는
    파일 몇 개였고 지금은 crate 넷에 걸친 명령 여섯 개다. **순차로 하나씩
    돌렸으면 최소 하나는 받았다.** 복구는 2026-09-07 15:43.

    ★ 네 번째 갈래(우회 조사)는 코덱스 자체 필터가 "관문 우회 방법을
      찾아라" 를 보안 위험으로 분류해 거부했다 — 자기 저장소 검수인데
      오탐이다. 표현을 바꿔 다시 걸었고 그것도 쿼터에 걸렸다.

    ★ evidence — 스키마가 옳게 거부했다

        ! review_artifact 에 파일:줄 위치가 하나도 없다 —
          구체적 반례 없는 검수는 형식적 승인이다

    schema v2(`ADR-030`)에는 **"측정은 끝났고 검수만 없다" 는 상태가 없다.**
    쿼터 소진 로그를 그 칸에 끼워 넣으면 형식만 맞추는 짓이라, 초안을
    `docs/plans/2026-09-03_1740_DoD-68_evidence_초안_검수대기.md` 에 뒀다.
    `INCONCLUSIVE` 로 우회하는 것도 부정확하다 — 측정이 안 끝난 게 아니라
    **검수가 없는 것**이고, 두 상태를 같은 이름으로 부르면 구분이 사라진다.

    ★ 다음 사람에게

    1. 9/7 이후 **순차로** 검수를 돌린다(병렬 3갈래가 이번 실패 원인).
    2. `ACCEPTED` 가 나오면 초안을 `docs/evidence/` 로 옮기고 `PASS` 로.
    3. **그 전까지 이 사슬 위에 새 기능을 쌓지 않는다** — 검수 없이 쌓으면
       나중에 어느 층이 틀렸는지 가려내기 어려워진다.
    4. 예약↔후보선택 공백은 검수 뒤 규범 결정부터.

`RULE.md` §8 에 따라 각 스파이크는 **결과와 무관하게** `docs/evidence/` 에 기록한다.

### ★ 2026-08-19 밤 ~ 2026-08-20 새벽 자율 작업 세션 요약

사용자가 취침 중 "코덱스 쿼터를 최대한 태워서 자율로 진행하라"는
지시에 따라, 이 세션(감독자, `agent:claude-code`)이 코덱스 CLI
(`agent:codex-cli`, gpt-5.6-luna)에 구현을 위임하고 대화 기록이
없는 새 코덱스 인스턴스로 독립 검수를 받는 사이클을 반복해
`DoD-21`부터 `DoD-34`까지 **14개 조각**을 완료했다(전부 위 표와
"다음에 할 일" 7~20번에 상세 기록). `coordinator-agent-selftest`
가 24개 → **48개 시나리오**로, `docs/evidence/` 의 schema 검사
대상 문서가 29건 → **43건(PASS 42)**으로, `cargo test --workspace`
가 310개 → **348개**로 늘었다. 전 조각이 로컬 커밋됐다(원격 push 없음, 최신 커밋
`b3cba58`).

**패턴으로 남길 만한 것들**:

- 같은 부류의 교착 버그(Coordinator 가 정책 거부 outcome 을 보낸
  뒤 Agent 는 즉시 종료하는데 Coordinator 는 다음 프레임을 계속
  기다림)가 오늘 밤 **세 번**(`DoD-22`·`DoD-23`·`DoD-27`) 독립적
  으로 발견됐다 — 이후 조각(`DoD-31`·`DoD-34`)마다 이 패턴을
  명시적으로 경계하며 검증했다.
- 코덱스 read-only 샌드박스 안에서 `coordinator-agent-selftest`
  가 프로세스 스폰 문제로 완주하지 못하는 현상이 여러 조각
  (`DoD-28`~`DoD-33`)에서 반복 관측됐다 — 감독자가 매번 샌드박스
  밖 실제 환경에서 재현해 코드 결함이 아님을 확인했다. 이건 이
  세션의 검수 인프라 자체의 알려진 한계로 남는다.
- 독립 검수가 진행 중일 때 감독자가 같은 파일을 직접 조작(뮤테이션
  재현 등)하면 오탐이 날 수 있다(`DoD-31` 에서 실제로 발생) —
  이후 감독자의 직접 파일 조작은 검수와 시점이 겹치지 않도록
  순차 진행했다.
- 백로그 재조사를 세 번(`p152`·`p158`류 → `p167` → `p176`) 반복
  하며 매번 "억지로 후보를 만들지 말고 정직하게 판단하라"고
  지시했다 — 세 번째 재조사(`p176`)는 진짜 후보 1개(`DoD-34`)만
  찾고 나머지는 전부 "후보 아님"으로 명시적으로 분류했다.

**남은 것 — 이 세션이 자율로 진행할 수 없는 것들**:

- **실행 트리거·연결 경로가 없음**: scheduler 조각 1·2a는 생겼지만
  다중 Agent·Coordinator 다중 HA·TLS·QUARANTINED 실제 판정
  (`TODO_VISION` V-11)·Job↔Agent 자동 매칭(V-10)은 아직 없다.
- **설계상 하루 규모를 넘음**: 자동 재접속 루프 전체(설계 문서가
  6~8일 규모로 명시, `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`)·
  실제 Job 실행(entrypoint·GPU 확인·runtime 격리·scheduler 필요).
- **시스템/보안 설정 변경이라 이 세션이 자율 실행 불가**: OS
  방화벽 강제(network.rs)·x600 의 WSL2 설치 — 둘 다 사용자가
  직접 실행해야 한다.
- **사용자 승인만 남음(구현은 이미 그 결정을 따름)**: `ADR-026`
  (체크포인트 확정 절차 플랫폼 차이)·`ADR-027`(Windows Job Object
  VRAM 상한) — 둘 다 상태는 "제안" 이지만 기준선 문서 수정 승인
  만 남았다.

사용자가 깨어나면 위 "남은 것" 중 시스템 설정 변경 항목(WSL2·
방화벽)부터 직접 처리하거나, `crates/scheduler` 같은 새 서브시스템
착수 여부를 판단하면 된다.

### ★ 2026-08-30 자율 작업 세션 요약

x600 의 WSL 이 응답하게 되면서 **Linux 가 처음으로 반복 검증 가능한
환경이 됐다.** 그 결과 오래 막혀 있던 두 항목이 풀렸다.

| 로드맵 항목 | 결과 |
|---|---|
| Stage 1 · 7 격리 적용 | ✅ `crates/runtime-linux` 신설, Agent 실행 경로 연결, x600 실측 (`DoD-57`) |
| Stage 2 · 12 다중 Agent | ✅ 동시성을 관문으로 **증명**(순차 서버는 통과 불가) |
| Stage 2 · 14 실패 감지 | 🟡 생존 판정 순수 커널 + 관측 영속화 + wire 연결 |
| Stage 2 · 17 운영용 키 보관 | ✅ Linux K1 을 `systemd-creds` 로 구현, x600 실측 |
| Stage 2 · 11 멤버십 | ⛔ 사용자 결정 4건 + `COMMITTED`(과반 합의) 부재로 막힘 |
| Stage 2 · 16 TLS | ⛔ 인증서 신원 규범 미정(기준선 §42.7.3) |

`coordinator-agent-selftest` 72 → **92개 시나리오**.

**독립 검수를 5라운드 돌렸고, 매 라운드가 진짜 결함을 찾았다.**
그중 무거운 것들:

- **`memory.max` 하나만으로는 상한이 아니다.** 32MiB 상한에 90MB 를
  할당했는데 자식이 정상 종료했다 — cgroup 은 스왑으로 밀어낼 뿐이다.
  `memory.swap.max=0` 을 같이 걸어야 실제로 끝난다.
- **cgroup 이름 충돌이 남의 작업을 죽였다.** `a.b` 와 `a_b` 가 같은
  이름이 되고, 기존 cgroup 을 죽인 뒤 다시 만들었다 — B 를 시작하면
  A 가 죽는다(§0.1 위반). 해시 + 성분별 길이 접두사로 고쳤다.
- **키 봉인을 signer 에 묶어도 이식이 가능했다.** 개인키 blob 을 비워
  공개키 전용 엔트리로 만들면 복호를 건너뛴다. 근본 원인은 파일
  체크섬이 **키 없는** BLAKE3 였다는 것 — 봉인해서 위조에 OS 비밀이
  필요하게 만들고 파일 버전을 2 로 올렸다.
- **`rebind_device()` 가 새 장치에 묶이지 않았다.** 행만 지워서 A→B
  교체를 승인한 순간 C 가 먼저 보고하면 C 가 인수했다.

**★ 못 막는 것을 실제로 재 봤다.** 검수가 "적대적 코드는 못 막는다" 고
지적했을 때 문서에 적기만 하지 않고 실행했다 — 자식이 자기 pid 를 상위
`cgroup.procs` 에 쓰고 나가 32MiB 상한 밖에서 90MB 를 잡는 데 성공했다
(`escaped=90655836`). **탈출이 성공하기를 기대하는 테스트**를 남겨,
나중에 구멍이 닫히면 그 테스트가 실패하며 문서도 같이 고치라고 알린다.

**패턴으로 남길 것 — 내 테스트가 세 번 공허했고 전부 뮤테이션이 잡았다:**

```text
시나리오 88   두 Agent 를 연달아 띄우고 성공만 확인 -> 순차 서버로도 통과
              고침: 동시 세션 관문(순차 서버는 구조적으로 통과 불가)
키 이식 검사   봉인 blob 만 교환 -> 기존 "개인키·공개키 불일치" 가 잡음
              고침: 공개키까지 교환 + 체크섬 재계산해야 봉인을 실제로 잼
깊이 검사      존재하지 않는 경로 -> is_dir() 이 먼저 거부
              고침: 실제 중첩 디렉터리 생성 + "전부 거부" 배제 대조 케이스
```

**작업 드라이브 — F: 다.** WSL 의 `/tmp` 는 ext4 VHDX 이고 그 파일이
C: 의 `Users\<x600-user>\AppData\Local\wsl\...\ext4.vhdx`(25.4GB)에 있다.
거기서 빌드하면 여유 18G 인 x600 의 C: 를 먹는다. 지금은
`/mnt/f/gputeer-work/build` 에서 빌드한다(drvfs 라 전체 빌드 약 3분 36초).
★ 이미 VHDX 가 잡은 공간은 파일을 지워도 안 줄어든다 — `wsl --shutdown`
후 압축이 필요하며 사용자가 직접 해야 한다.

★★ **이 문단은 2026-08-31 에 낡았다** — 아래 `환경 주의사항` 의 최신 항목을
따른다. WSL 배포판이 **E: 로 이전**됐고, C: 에서의 WSL·Docker 작업은
사용자 지시로 **금지**됐다. 그리고 VHDX 가 컸던 이유는 우리 빌드가
아니라 사용자의 Docker 환경이었다 — 압축으로 되찾을 수 있는 양도
25GB 가 아니라 2.6GB 였다(실측).

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
