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

## 5. 지금 상태 (2026-08-20)

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
| **Rust 구현** | 🟡 **진행 중** — **`cargo test --workspace` 348 passed / 0 failed**(1 ignored, 2026-08-20 실측), 빌드 경고 0 |
| ├ `crates/protocol` | canonical · prost 연동 · 서명 대상 완전성 · **Ed25519 + `Verified<M>`** · **`AgentGrantAck` 서명 대상 메시지**(coordinator/agent 핸드셰이크용, 2026-08-18) |
| ├ `crates/crypto` | Ed25519Verifier · DurableReplayGuard · PersistentKeyring · replay 계약 적합성 · `ingress` 진입점 · **`framed_ingress` 프레이밍·디스패치**(`FrameType::GrantAck` 포함) · **별도 OS 프로세스 8개로 replay 락 경합 실측**(2026-08-18) |
| ├ `crates/checkpoint` | ADR-026 원자적 쓰기 · kill 카오스 · 경로 탈출 차단 · 재개 job/attempt 필터 · 실패 마커 · 상태 사이드카 · 동시 GC 경합 · **`chaos-hooks`(비기본) self-kill 훅으로 HASH_VERIFIED~COMMITTED 결정적 kill** · **`write_once()` 동시 동일-이름 호출 명시적 거부(2026-08-19, `DoD-21`)** — 프로세스 간 파일 잠금 + 성공 시 자가 정리 + GC 의 죽은 락 회수 |
| ├ `crates/runtime-policy` | **정책 강제 판정** (V-06) — artifact_scope · network · Lease.scope · VRAM/S1 분류. 실제 연결은 `crates/runtime-windows` 가 시작함 |
| ├ `crates/runtime-windows` | **신규**(2026-08-18) — VRAM 판정을 실제 `CreateJobObjectW`/`SetInformationJobObject` 로 연결(소프트 제한 실측, 오버슈트 700~850KiB — `guarantees_hard_limit()==false` 재확인). **`open_beneath`/`open_artifact`** — `artifact_scope` TOCTOU 방어, reparse point(symlink·junction) 를 열기 시점에 실제로 거부(junction 으로 실측, 뮤테이션 테스트 포함). network(방화벽)만 미착수(시스템 설정 승인 필요) |
| ├ `crates/cli` | **`gputeer selftest`** — 계층을 끝에서 끝까지 25개 검사로 통과. **127.0.0.1 실제 TCP 소켓 왕복** 포함. **`gputeer coordinator-agent-selftest`**(신규, 2026-08-18) — 별도 프로세스 2개(coordinator-stub·agent-stub)가 실제 handshake + **거부 경로 3종(위조 Grant·위조 ACK·replay) 자동 검증** |
| ├ `crates/coordinator` | `ExecutionGrant` 서명 발급, `AgentGrantAck` 검증, `Lease` 를 Grant 에 실어 보낸다(2026-08-18). **Lease 갱신 왕복도 처리**(2026-08-19) — 같은 연결에 이어서 `RenewLeaseRequest` 를 받아 검증하고 서명된 `RenewLeaseResult` 로 응답한다. **같은 연결에서 N 회 반복 갱신 가능**(`--renew-rounds`, `derive_renew_nonce(lease_id, round)` 로 회차별 nonce 분리). **`CoordinatorLeaseStore` 로 발급 Lease 를 SQLite 에 영속화**(`--lease-db`, optional — 안 주면 기존 레거시 경로) — 재시작 후에도 자신이 발급한 Lease 의 신원·epoch 를 기억한다. **`max_total_duration_seconds` 갱신 차단**(2026-08-19, `--lease-db` 사용 시에만) — 저장된 `issued_at_unix_ms` 기준 누적 시간 초과 시 서명된 `MAX_DURATION_EXCEEDED` 반환, 만료시각 미연장. **Lease revoke 최소 경로**(2026-08-19, `DoD-22`) — `--revoke-after-round` 로 이미 발급한 Lease 를 대상으로 서명된 `RevokeLeaseNotice` 를 같은 연결로 보낸다. **실제 재발급 정책 — SUPERSEDED 부분**(2026-08-19, `DoD-23`) — 영속 저장소가 있을 때 요청 epoch 이 저장된 값보다 낮으면 연결을 끊는 대신 서명된 `RENEW_OUTCOME_SUPERSEDED` 로 응답한다(proto 가 이미 "정상적인 failover 경합"이라 선언했던 상황을 실제로 그렇게 처리). **재접속 최소 조각 — 프로세스 재시작 복원**(2026-08-19, `DoD-24`) — 테스트 전용 `--disconnect-after-ack` 로 연결 단절을 흉내내면, 완전히 새로운 프로세스 쌍이 같은 `--lease-db`/`--fence-db` 로 `get_or_issue()` 를 통해 저장된 활성 Lease(identity·epoch)를 복원해 Grant 로 돌려준다 — 새 proto 메시지 없음, 기존 handshake 재사용. 다중 Agent·QUARANTINED 실제 트리거(`TODO_VISION` V-11)·자동 재접속 루프는 미착수. **Lease revoke 영속화**(2026-08-19, `DoD-25`) — `CoordinatorLeaseStore` 에 `revoked_at_unix_ms` 추가(기존 SQLite 파일도 `open()` 시 `PRAGMA table_info`+`ALTER TABLE` 로 자동 보정), `send_revoke_notice()` 가 wire 전송 전에 `mark_revoked()`(idempotent) 로 커밋을 먼저 확정, `get_or_issue()`·갱신 경로 양쪽 다 revoked Lease 를 거부한다 — revoke 된 뒤 재접속해도 되살아나지 않는다. **만료 Lease 재접속 거부**(2026-08-20, `DoD-26`) — `get_or_issue()` 가 만료된 Lease 도 거부한다(`<=` 경계, `crates/protocol/src/signing.rs`·Agent 의 revoke 검사와 동일 규칙). **`REVOKED` signed outcome**(2026-08-20, `DoD-27`) — 갱신 경로의 revoked 거부가 raw error 대신 서명된 `RenewLeaseResult{outcome: RENEW_OUTCOME_REVOKED=8}` 로 응답한다(proto enum 순수 추가) |
| ├ `crates/agent` | `ExecutionGrant` 검증, `AgentGrantAck` 서명 응답, nested `Lease` 를 outer Grant 와 독립 검증(2026-08-18). **Lease 갱신도 처리**(2026-08-19) — 서명·`request_nonce` echo·nested Lease 독립 서명·epoch 단조성(낮은 epoch 거부 + 높은 epoch 명시적 거부)을 전부 확인한 뒤에만 보유 Lease 를 교체한다. **같은 연결에서 반복 갱신**(`--renew-rounds`) 지원. **`FenceWatermark` 가 SQLite 로 영속화됨**(`DurableFenceWatermark`) — 최초 Grant·갱신 검증 두 호출부 모두 재시작을 넘는다, `--fence-db :memory:` 는 fail closed. **`MAX_DURATION_EXCEEDED` 명시 거부**(2026-08-19). **`RevokeLeaseNotice` 검증·처리**(2026-08-19, `DoD-22`) — 서명·`lease_id`·`fence_epoch`·만료 상태를 확인한 뒤 보유 Lease 를 revoked 로 표시하고 이후 갱신 요청을 만들지 않는다. **Job 실행의 첫 걸음 — WRITING 시작 마커**(2026-08-20, `DoD-30`) — 유효 Grant/Lease 검증 성공 직후·`AgentGrantAck` 전송 전에 결정적 `checkpoint_id`(BLAKE3-256)로 checkpoint 디렉터리를 만들고 `WRITING` 마커를 `write_once()` 로 기록한다(위조/만료/revoked Lease 는 마커 미생성, 마커 생성 실패는 fail-closed). **갱신 직전 만료 자기 재확인**(2026-08-20, `DoD-34`) — `RenewLeaseRequest` 를 만들기 전에 보유 Lease 의 만료(`<=` 경계)를 스스로 재확인해, 만료됐으면 요청을 안 보내고 `RENEW_REFUSED:LOCAL_EXPIRED` 로 종료한다(Coordinator 의 `DoD-32` 방어를 보완하는 낭비/관측 공백 해소). 실제 entrypoint 프로세스 실행·GPU 확인·runtime 격리·데이터 파일·manifest.json 작성·Coordinator 보고 wire 메시지는 여전히 미착수 — `RESULT ok=true` 는 Job 완료를 뜻하지 않는다 |
| └ 미착수 | scheduler · UI · OS 방화벽 강제(network.rs) · 재접속의 나머지(자동 재접속 루프·`ResumeLeaseRequest`·동시 경쟁) · Coordinator 의 실제 재발급 정책 중 QUARANTINED 실제 트리거(`TODO_VISION` V-11)와 새 `lease_id` 발급 · 다중 Agent · 실제 entrypoint 프로세스 실행(WRITING 마커까지만 완료). **핸드셰이크**(2026-08-18) + **Lease 최소 조각**(2026-08-18) + **Lease 갱신 최소 조각**(2026-08-19) + **durable FenceWatermark**(2026-08-19) + **반복 Lease 갱신**(2026-08-19) + **Coordinator 영속 Lease 저장소**(2026-08-19) + **max_total_duration_seconds 갱신 차단**(2026-08-19) + **RevokeLeaseNotice framed ingress 커버리지**(2026-08-19) + **Lease revoke 최소 경로**(2026-08-19, `DoD-22`) + **실제 재발급 정책 SUPERSEDED 부분**(2026-08-19, `DoD-23`) + **재접속 최소 조각**(2026-08-19, `DoD-24`) + **Lease revoke 영속화**(2026-08-19, `DoD-25`) + **만료 Lease 재접속 거부**(2026-08-20, `DoD-26`) + **`REVOKED` signed outcome**(2026-08-20, `DoD-27`) + **레거시 lease 경로 명시적 opt-in**(2026-08-20, `DoD-29`) + **Job 시작 WRITING 마커**(2026-08-20, `DoD-30`) + **terminal outcome 다회차 교착 회귀 테스트**(2026-08-20, `DoD-31`) + **만료 Lease 갱신 fail-closed**(2026-08-20, `DoD-32`) + **marker-only checkpoint GC 회귀 테스트**(2026-08-20, `DoD-33`) + **Agent 쪽 갱신 직전 만료 재확인**(2026-08-20, `DoD-34`) 전부 완료, `coordinator-agent-selftest` **48/48 시나리오** 연속 통과, `docs/evidence/` schema v2 정식 기록(`DoD-11`~`34`)도 전부 완료 |
| **P0 스파이크** | 🟡 **5/9 완료** — 01 ✅ · 03 ✅ · 03a ✅ · 06 ⚠️FAIL-SCOPE · 07 ✅(2026-08-18 x600 재실측으로 σ=0.0213 재확인, INCONCLUSIVE→PASS 복원) · 08 ✅ / 02·04·04b·05 미실행 |
| **DoD** | 🟡 **evidence 42건** (PASS **42** · FAIL-SCOPE 1, 2026-08-20 `verify_evidence.py` 실측 — 43개 중). `DoD-17`(RevokeLeaseNotice 커버리지)·`DoD-18`(max_total_duration_seconds 갱신 차단)·`DoD-19`(오래된 테스트 공백 3건)·`DoD-20`(`tools/canonical/check_schema.py` 신규 구현, 코덱스 1라운드가 실행 환경 오류 exit 코드 계약 위반을 실제 실행으로 발견 후 2라운드 ACCEPTED)·**`DoD-21`(2026-08-19, `write_once()` 동시 호출 계약 — 아래 참조)**·**`DoD-22`(2026-08-19, Lease revoke 최소 경로 — 아래 참조)**·**`DoD-23`(2026-08-19, Coordinator 재발급 정책 SUPERSEDED — 아래 참조)**·**`DoD-24`(2026-08-19, Lease 재접속 최소 조각 — 아래 참조)**·**`DoD-25`(2026-08-19, Coordinator Lease revoke 영속화 — 아래 참조)**·**`DoD-26`(2026-08-20, 만료 Lease 재접속 거부 — 아래 참조)**·**`DoD-27`(2026-08-20, REVOKED signed outcome — 아래 참조)**·**`DoD-28`(2026-08-20, check_schema.py CI 연결 — 아래 참조)**·**`DoD-29`(2026-08-20, 레거시 lease 경로 명시적 opt-in — 아래 참조)**·**`DoD-30`(2026-08-20, Job 시작 WRITING 마커 — 아래 참조)**·**`DoD-31`(2026-08-20, terminal outcome 다회차 교착 회귀 테스트 — 아래 참조)**·**`DoD-32`(2026-08-20, 만료 Lease 갱신 fail-closed — 아래 참조)**·**`DoD-33`(2026-08-20, marker-only checkpoint GC 회귀 테스트 — 아래 참조)**·**`DoD-34`(2026-08-20, Agent 쪽 갱신 직전 만료 재확인 — 아래 참조)** 추가. 스키마 위반 0. ★ v1 evidence 전부(DoD-01~08·P0-01·03·03a·07·08 13건) schema v2 승격 완료, 독립 검수 없는 P0/DoD PASS 부채 **0건**. `DoD-09`~`34` 는 신규 작성부터 v2. **`DoD-13`(2026-08-19)** — Lease 갱신 최소 조각(`RenewLeaseRequest`/`RenewLeaseResult` 왕복, `RenewLeaseResult` 신규 서명화 포함). 코덱스 1라운드 검수(`p99`)가 완료 보고 전에 실제 설계 결함 2건을 찾아냈다 — Coordinator 가 요청의 `fence_epoch` 을 검증하지 않던 문제, `FenceWatermark` 가 `<` 만 거부하고 `>`(epoch 상승)는 통과시키는데 계획서는 상승을 이 조각 범위에서 정책상 거부하라고 명시했던 문제. 둘 다 코드로 고치고 2라운드 좁은 후속 검수(`p100`)에서 `ACCEPTED`. 신규 검증 게이트 4건(nested Lease 독립 검증·request_nonce 대조·epoch 상승 거부·Coordinator epoch 대조)을 뮤테이션 테스트로 비공허성 확인. **`DoD-21`(2026-08-19)** — `write_once()`(`crates/checkpoint`)가 같은 `(dir, name)` 동시 호출을 지원하지 않고 명시적으로 거부하는 계약을 `std::fs::File::try_lock()` 기반 프로세스 간 파일 잠금으로 강제했다(`DoD-08` 이 발견했으나 안 고치고 넘겼던 결함의 후속). 코덱스 독립 검수 **5라운드**(`p128`~`p132`) — 매 라운드가 실제 결함을 찾았다: MSRV 불일치(`Cargo.toml` 1.85 vs `try_lock` 요구 1.89)·`k1c` 결함 고정 테스트의 비결정적 주장·GC 의 죽은 락 영구 보존으로 PARTIAL 디렉터리 청소 불능(내 1차 수정이 만든 회귀)·이름공간 충돌로 등록 데이터 파일 삭제 가능성(락 경로와 데이터 경로가 우연히 같아질 수 있음, 1차 등록 검사→근본 원인은 접미사 자체 예약)·대소문자/Win32 후행 점공백 우회. 전부 코드로 고치고 5라운드에서 `ACCEPTED`. 신규 테스트 5건(`k1c` 재설계+`k1d`·`k1e`·`k1f`·`k4c`)과 뮤테이션 테스트 6건으로 비공허성 확인 — 그 중 하나(락 파일 자가 정리 무력화)는 **기존** `durability_chaos.rs` 테스트 2건("완결된 체크포인트는 GC 가 절대 안 건드려야 한다")을 실패시켜, 이 세션 안에서 스스로 만들었다가 스스로 고친 회귀였음을 확인했다. GC 대 활성 writer 의 디렉터리 단위 경쟁은 의도적으로 범위 밖으로 남겨 문서화만 함(다중 Agent 실행 시작이 트리거). **`DoD-22`(2026-08-19)** — `RevokeLeaseNotice`(서명 대상·framed_ingress dispatch 는 `DoD-17` 이 이미 갖춰뒀다)의 실제 Coordinator/Agent 업무 로직을 구현했다 — Coordinator 가 이미 발급한 Lease 를 대상으로 서명해 보내고, Agent 가 서명·`lease_id`·`fence_epoch`·만료 여부를 검증한 뒤 보유 Lease 를 revoked 로 표시해 갱신을 멈춘다. ★ 사용자 요청에 따라 **구현 자체를 코덱스 CLI(workspace-write 샌드박스)에 위임**하고, 이 세션은 독립 재검증(빌드·테스트·selftest 직접 재실행)과 대화 기록이 없는 새 코덱스 인스턴스의 독립 검수(read-only)만 맡는 방식으로 진행했다. 독립 검수 1라운드(`p134`)가 진짜 교착 결함 1건(`--revoke-after-round 0` + Coordinator/Agent 둘 다 `do_renew=true` 조합에서 Coordinator 가 오지 않을 프레임을 기다림 — 기존 selftest 시나리오들이 이 조합을 우연히 피해가 안 드러났었다)을 포함해 4건을 찾아 전부 코드로 고쳤다(`p135`). 이 세션이 코덱스의 자체 뮤테이션 보고와 별개로 교착 방지 가드를 직접 되돌려 재현해(정확히 시나리오 25 에서 exit=1) 결함의 실재를 독립 재확인했다. 2라운드(`p136`)는 문서 완결성만 지적, 문서 보강 뒤 3라운드(`p137`)에서 `ACCEPTED`. 재접속 시 revoke 유실·비동기 전송·실제 정책 엔진(SUPERSEDED/QUARANTINED 판단)은 의도적으로 범위 밖. **`DoD-23`(2026-08-19)** — Coordinator 의 실제 재발급 정책 중 SUPERSEDED 부분을 구현했다 — `lease_store` 가 있고 요청 epoch 이 저장된 값보다 낮으면 연결을 끊는 대신 서명된 `RENEW_OUTCOME_SUPERSEDED` 로 응답한다(proto 자체가 "정상적인 failover 경합"이라 선언했던 상황을 실제로 그렇게 처리). 이번에도 구현을 코덱스 CLI(workspace-write)에 위임. 이 코드를 읽던 이 세션이 먼저 의심스러운 지점(SUPERSEDED 응답 후 `continue`)을 포착해 미리 알리지 않고 블라인드로 독립 검수를 돌려 교차 확인했다 — 독립 검수 1라운드(`p139`)가 정확히 같은 지점을 지적: `renew_rounds > 1` 이고 SUPERSEDED 가 마지막이 아닌 회차에서 발생하면 Coordinator 가 오지 않을 프레임을 기다리는 교착(오늘 Lease revoke 조각에 이어 **두 번째로** 나온 같은 부류의 결함, 기존 시나리오들이 우연히 이 조합을 피해가 안 드러났었다). `continue` 를 `break` 로 고치면서, Agent 가 즉시 종료하는 다른 outcome(QUARANTINED·MAX_DURATION_EXCEEDED)도 같은 위험이 **이전 조각들부터** 있었음을 확인해 공통으로 일반화했다(`p140`). 이 세션이 그 수정을 직접 되돌려 뮤테이션을 재현해(정확히 새 시나리오 32 에서 exit=1, 스트림 끊김) 코덱스의 자체 보고와 별개로 재확인했다. 2라운드(`p141`)에서 `ACCEPTED`. QUARANTINED 실제 트리거는 위험도/신뢰도 인프라 부재로 `TODO_VISION` V-11 로 등록만 함. **`DoD-24`(2026-08-19)** — 재접속(failover)의 첫 최소 조각("Active Lease process-restart rehydration")을 구현했다 — 설계 조사(코덱스, `p142`)가 먼저 "전체 재연결 프로토콜은 오늘 조각들보다 크다" 고 정직하게 판단하고, "프로세스 재시작 후 활성 Lease 복원" 만으로 범위를 좁혔다. 새 proto 메시지 없이, 테스트 전용 `--disconnect-after-ack` 로 ACK 직후 연결을 끊고, 완전히 새로운 프로세스 쌍이 같은 `--lease-db`/`--fence-db` 로 시작해 `CoordinatorLeaseStore::get_or_issue()`(이미 있던 인프라)로 저장된 Lease 를 복원한다 — CLI 의 틀린 `--fence-epoch` 보다 저장된 값이 우선함을 확인했다. 오늘 이미 두 번 나온 "한쪽은 끝났는데 다른 쪽은 계속 기다리는" 교착 패턴이 세 번째로 있는지 특히 의심하며 독립 검수(`p144`)를 요청했으나, 이번 설계는 애초에 그 위험 구조를 피했음을 확인(연결이 끊기면 Agent 는 무한 대기가 아니라 즉시 EOF 오류로 종료) — **1라운드 만에 `ACCEPTED`**. holder identity 충돌 검사는 새로 만든 게 아니라 기존 코드였음을 확인만 하고 테스트를 추가했다. revoke 상태는 재접속에서 여전히 보존 안 됨(`DoD-22` 한계 그대로), 자동 재접속 루프·`ResumeLeaseRequest`는 의도적으로 범위 밖. **`DoD-25`(2026-08-19)** — `DoD-24` 가 남긴 그 revoke 미보존 안전 공백을 닫았다 — `CoordinatorLeaseStore` 에 `revoked_at_unix_ms` 필드를 추가하고, 기존 SQLite 파일도 `open()` 시점에 `PRAGMA table_info`+`ALTER TABLE` 로 자동 보정한다. 설계 조사(코덱스 `p145`)가 먼저 스키마·`get_or_issue()`·`send_revoke_notice()` 를 실측해 마이그레이션이 필요함을 짚었고, 구현(`p146`, 코덱스 workspace-write)이 `mark_revoked()`(idempotent, wire 전송 **전에** 커밋 확정)와 `get_or_issue()`·갱신 경로(정상 경로 + `renew_outcome_override` 읽기 전용 경로) **양쪽 다**의 revoked 거부를 만들었다. 이번엔 구현자가 evidence·CLAUDE.md·HISTORY 를 안 건드려 구현자/검수자 경계가 더 깔끔했다. 독립 검수(`p147`)가 마이그레이션 로직·두 갱신 경로·override 시에도 실제 lease_id 로 기록되는지까지 전부 확인하고 **1라운드 만에 `ACCEPTED`**. **`DoD-26`(2026-08-20)** — `DoD-24` 가 이월한 "만료된 Lease 재접속 거부" 를 구현했다 — `get_or_issue()` 에 만료 검사를 추가했다. 독립 검수 1라운드(`p149`)가 진짜 계층 간 결함을 찾았다 — 만료 판정이 엄격한 `<` 를 써서, 이미 이 저장소가 정착시킨 "경계 포함"(`<=`) 규칙(`crates/protocol/src/signing.rs` 의 Lease 서명 검증, Agent 의 revoke 검사)과 어긋났다 — 정확히 만료 시각과 같은 순간에 Coordinator 는 재발급을 허용하는데 Agent 는 같은 Lease 를 즉시 거부하는 모순이 생길 뻔했다. `<` 를 `<=` 로 고치고 경계값 테스트 2건을 추가한 뒤(`p150`) 2라운드(`p151`)에서 `ACCEPTED`. **`DoD-27`(2026-08-20)** — `DoD-25` 가 명시적으로 남긴 공백("새 signed outcome 이나 proto 변경은 하지 않았다 — Agent 쪽에서 이 거부와 다른 종류의 handshake 실패를 구분할 신호가 없다")을 닫았다 — **갱신(renew) 경로에 한정해** `proto/lease.proto` 에 `RENEW_OUTCOME_REVOKED = 8` 을 순수 추가하고, Coordinator 가 revoked Lease 에 대한 갱신 요청을 raw error 로 연결을 끊는 대신 서명된 `RenewLeaseResult{ outcome: 8 }` 로 응답하도록(override 읽기 전용 경로·정상 갱신 경로 양쪽 다) 고쳤다. 오늘 이미 두 번(`DoD-22`·`DoD-23`) 나온 "한쪽은 끝났는데 다른 쪽은 계속 기다리는" 교착 패턴이 세 번째로 있는지 특히 의심하며 독립 검수를 요청했다 — 결과를 전송·flush 한 **뒤에** outcome 8 을 기존 교착 방지 `break` 목록(`2 | 3 | 6`)에 정확히 추가했는지, Agent(`crates/agent/src/lib.rs`)도 outcome 8 을 만나면 즉시 종료하는지가 핵심 확인 대상이었다. 독립 검수(`p157`)가 proto enum 순수 추가·양쪽 경로의 signed outcome 변환·`break` 위치(전송 후)·Agent 즉시 종료·신규 테스트 전용 플래그 `revoke_before_renew`(ACK 후 저장소만 revoke, notice 는 안 보냄 — 기존 `revoke_after_round` 경로와 독립)·시나리오 37 이 실제 wire 서명/replay/nonce 검증을 통과해야 성공함·`lease_store.rs` 는 이번 조각에서 전혀 안 바뀌었음(설계대로)·초기 Grant 발급 시점의 revoked 거부는 여전히 범위 밖(안 건드림)까지 전부 확인하고 **1라운드 만에 `ACCEPTED`**. `coordinator-agent-selftest` 37/37 시나리오, 5회 연속 통과(각 60초 하드 타임아웃). 뮤테이션 테스트로 outcome 8 `break` 제거 시 시나리오 37 실패 및 교착 재현을 확인. 초기 Grant 발급의 revoked 거부·`lease_store.rs` 자체 변경은 의도적으로 범위 밖. **`DoD-28`(2026-08-20)** — `DoD-20`(`check_schema.py` 신규 구현)이 남긴 "CI 파이프라인에 실제로 연결하지 않았다" 공백을 닫았다 — `.github/workflows/canonical-schema-check.yml` 을 신설해 `main` 대상 `push`·`pull_request` 에서 canonical 참조 self-test·벡터 대조·`check_schema.py`·워크스페이스 build/test(`gputeer-runtime-windows` 제외)·`verify_evidence.py` 를 순서대로 실행한다. 이 저장소는 원격이 없어 워크플로가 실제 GitHub Actions 에서 돈 적은 없다 — 로컬 명령 순서 실행 성공으로 검증을 대신했다. 독립 검수(`p159`)가 YAML 문법·트리거·경로 정확성(`--exclude gputeer-runtime-windows` 이름이 `crates/runtime-windows/Cargo.toml` 의 실제 package name 과 일치)·Rust 버전 일치·apt `protobuf-compiler` 설치의 실제 필요성(`check_schema.py` 가 Cargo 의 `protoc-bin-vendored` 와 별개로 PATH 의 `protoc` 를 직접 호출함을 코드로 확인)·최소 권한·미커밋 상태까지 전부 확인하고 **1라운드 만에 `ACCEPTED`**. **`DoD-29`(2026-08-20)** — `--lease-db` 없는 레거시 경로가 운영에서 실수로 켜지는 것을 막는 명시적 opt-in 플래그(`--i-understand-legacy-mode-is-unsafe`)를 추가했다. 독립 검수 1라운드(`p161`)가 실제 코드 지적 2건(시나리오 27~31 의 legacy opt-in 자동 적용 의도 불명확, 시나리오 38 의 Agent 쪽 미검증)을 찾아 전부 고쳤다(`p162`). 2·3라운드(`p163`·`p164`)가 코드 자체는 문제없음을 확인했으나 코덱스 read-only 샌드박스 안에서만 재현되는 selftest 미완료(Coordinator 만 스폰되고 Agent 서브프로세스는 안 뜸)를 보고해 반려됐다 — 감독자(claude-code)가 같은 바이너리를 샌드박스 밖에서 16회 연속 실행해(전부 exit=0, 38개 시나리오, 약 9초/회) 재현되지 않음을 확인하고, `DoD-28` 에서 이미 관측된 것과 같은 종류의 샌드박스 프로세스 스폰 제약으로 결론지어 최종 `ACCEPTED`. **`DoD-30`(2026-08-20)** — `crates/agent` 에 Job 실행을 향한 가장 작은 첫 걸음(설계는 `p154`)을 구현했다 — 유효 Grant/Lease 검증 성공 직후·`AgentGrantAck` 전송 전에 결정적 `checkpoint_id`(BLAKE3-256, 길이-프리픽스된 job_id/attempt_id/grant_id)로 `WRITING` 마커를 `write_once()` 로 기록한다. 위조/만료/revoked Lease 는 마커 미생성·ACK 미전송, 마커 생성 실패는 fail-closed, 재시도는 `write_once()` 의 기존 idempotent 동작에 의존한다. 실제 entrypoint 실행·manifest·wire 메시지·scheduler 는 전부 범위 밖 — `RESULT ok=true` 는 여전히 Job 완료가 아니다. 독립 검수(`p166`)가 실행 순서·checkpoint_id 인코딩·거부 경로·fail-closed·멱등성·범위 제한까지 전부 코드로 확인하고 **1라운드 만에 `ACCEPTED`**(검수 환경의 selftest 30초 제한은 `DoD-28`·`DoD-29` 와 같은 샌드박스 제약으로 판단, 감독자가 5회 연속 재확인). **`DoD-31`(2026-08-20)** — 오늘 밤 세 번(`DoD-22`·`DoD-23`·`DoD-27`) 나온 교착 버그 패턴에 대한 회귀 테스트를 `QUARANTINED`·`MAX_DURATION_EXCEEDED` 까지 확장했다(프로덕션 코드 불변, 순수 테스트 추가). 독립 검수 1라운드(`p169`)가 `CHANGES_REQUESTED` 를 냈으나, 이는 검수가 진행되던 시간대에 감독자(claude-code)가 코덱스의 뮤테이션 보고를 독립 재확인하려고 `matches!` 를 직접 순차 뮤테이션(outcome=3·6 각각 제거→재현→원복)하던 중간 상태를 검수가 우연히 읽은 오탐이었다 — 감독자가 두 뮤테이션 모두 정확히 예측된 교착으로 재현·원복까지 확인한 뒤 안정 상태에서 2라운드(`p170`)를 요청해 **`ACCEPTED`**. 프로세스 교훈: 독립 검수 진행 중 감독자의 직접 뮤테이션 재현은 순차 진행하기로 함. **`DoD-32`(2026-08-20)** — 설계 조사(`p171`)가 확인한 실제 안전 공백을 닫았다 — `renew_existing_within_duration()` 이 revoke·max-duration 만 검사하고 저장된 `expires_at_unix_ms` 가 이미 지났는지는 검사 안 한 채 즉시 새 만료시각으로 갱신했다(정상 Agent 도 지연·시계 어긋남으로 도달 가능). `DoD-26` 과 동일한 `<=` 경계 규칙으로 `expires_at_unix_ms <= now_unix_ms` 검사를 revoke 뒤·max-duration 전에 추가하고, 기존 `LeaseStoreError::Expired` 를 재사용해 raw error 로 거부한다(signed outcome 없음, `DoD-27` 과 같은 범위 판단). override·일반 갱신 경로 양쪽 적용, 기존 fixture 보정(판정 조건 유지), 경계 단위 테스트 2건·selftest 시나리오 47 신설. 독립 검수(`p173`)가 경계·순서·`UPDATE` 미실행·fixture 타당성·범위 제한까지 전부 코드로 확인하고 **1라운드 만에 `ACCEPTED`**, 감독자가 검수 완료 후 순차로 재확인(5회 연속 exit=0, 47개 시나리오). **`DoD-33`(2026-08-20)** — `DoD-30` 이 문서로만 주장했던 "marker-only 디렉터리는 GC 대상" 이라는 안전 불변식을 실제 회귀 테스트로 고정했다 — `crates/checkpoint/tests/durability_chaos.rs` 에 신설한 테스트가 `.durability.writing` 마커만 있는 디렉터리는 `startup_gc()` 후 삭제되고 완결된(manifest+데이터) 디렉터리는 보존됨을 파일시스템 상태로 확인한다. GC 알고리즘·Agent 코드는 전혀 안 바꿨다. 독립 검수(`p175`)가 `gc_partial()` 판정식과 Agent 의 실제 마커 생성 코드가 정확히 일치하는지, assert 가 실질적인지, 뮤테이션 논리까지 전부 코드로 확인하고 **1라운드 만에 `ACCEPTED`**, 감독자가 직접 재현해 재확인. 이로써 백로그 재조사(`p167`)가 찾은 3개 후보(교착 회귀·만료 갱신 fail-closed·marker-only GC) 전부 완료. **`DoD-34`(2026-08-20)** — 마지막 재조사(`p176`)가 확인한 유일 후보 — Agent 가 갱신 루프에서 `revoked` 만 확인하고 만료는 재확인 안 해 이미 만료된 Lease 로 갱신 요청을 보낼 수 있던 공백(`DoD-32` 가 Coordinator 쪽에서 이미 방어)을 Agent 쪽에서도 닫았다. `RenewLeaseRequest` 생성 직전에 새 `lease_is_expired()`(`<=` 경계, `DoD-26`/`DoD-32` 와 일관)로 재확인해 만료 시 요청을 안 보내고 `RENEW_REFUSED:LOCAL_EXPIRED` 로 종료. Coordinator 는 전혀 안 건드림. ★ 오늘 밤 세 번 나온 교착의 **반대 방향**(Agent 가 안 보내면 Coordinator 가 기다릴 위험)이 최우선 검증 대상이었다 — 구현자·독립 검수(`p178`)·감독자 3단계 모두 `read_frame()` 의 EOF 즉시 전파를 코드로 확인하고 selftest 5회(매회 약 16초, 90초 타임아웃 근처 안 감)로 실측 확인, **1라운드 만에 `ACCEPTED`**. 이로써 오늘 밤 백로그 재조사가 찾은 모든 후보를 마쳤다 |
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

Linux 검증 — 부분 해소, 완전 해소 아님(2026-08-19, `ENV-03`)
  사용자가 임시로 빌려준 원격 기계(remote5090, Ubuntu 24.04 + RTX 5090)에서
  이 저장소가 처음으로 Linux 빌드·테스트를 통과했고(gputeer-runtime-windows
  제외 전부 ok, k1c 하나만 플랫폼 차이로 FAILED), sudo 없이 cgroup v2 로
  memory/CPU/PID/freezer 4종 강제를 확인했다. 그러나 이 기계는 사용자
  소유의 공유·비영구 기계라 "확보"가 아니라 "임시 접근"이다 — 반복
  가능한 접근성과 GPU VRAM 세분 할당(MPS) 검증은 여전히 없다.  -> D-3
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
```

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

- **트리거 자체가 없음**: scheduler·다중 Agent·Coordinator 다중
  HA·TLS·QUARANTINED 실제 판정(`TODO_VISION` V-11)·Job↔Agent
  자동 매칭(V-10).
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

### 환경 주의사항

- **Rust 1.97.1** 설치됨 (로컬 · x600 · remote5090 전부). `protoc` 는 `protoc-bin-vendored` 로 번들.
- 개발 기계는 Windows 11, GPU 없음(Intel Iris Xe).
  GPU 검증은 **x600**(RTX 4070 SUPER · driver 595.79 · CUDA 13.2, Windows). 작업 디스크 **F:**.
- `blake3` Python 패키지 설치 확인됨.
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
