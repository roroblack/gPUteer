# RULE.md — gPUteer 작업 규칙 (프로세스)

> 이 문서는 이 저장소에서 수행되는 **모든 작업(사람 · AI 에이전트 공통)** 에 적용되는 **프로세스 규칙**이다.
> 계획서 · 리포트 · 이력이 이 문서와 충돌하면 이 문서가 우선한다.

## ★ 이 문서와 `CLAUDE.md` 의 관계

**둘은 다루는 것이 다르다. 하나로 합치지 않는다.**

| | 무엇 | 자동 로드 |
|---|---|---|
| **`RULE.md`** (이 문서) | **프로세스** — 작업 루프 · 문서 폴더 · evidence · 분업 · 동결 | ❌ 사람이 링크해야 읽힌다 |
| **`CLAUDE.md`** | **도메인** — 안전 원칙 · 현재 상태 | ✅ 매 세션 자동 |

### 충돌하면 무엇이 이기나

★ **`CLAUDE.md` §0(안전 원칙)이 이 문서의 어떤 조항보다 앞선다.**
남의 PC를 망가뜨리거나 서명 검증을 우회하거나 사용자 데이터를 잃는 것이 절차 위반보다 무겁다.
그 밖의 충돌은 **이 문서가 우선**한다.

★ **중복 조항을 새로 만들지 않는다.** 도메인 규칙은 `CLAUDE.md` 에만 두고 여기서는 링크한다.

### 상위 기준선과의 관계

`../gputeer_master_plan_FINAL.md` 는 **아키텍처 기준선**이다. **읽기 전용이며 이 저장소에서 수정하지 않는다.**
구현이 기준선과 어긋나면 `docs/plans/` 에 사유를 적고, 기준선 자체는 건드리지 않는다.
기준선을 바꿔야 하는 결정(P0 실패 등)은 **§8 절차**를 따른다.

---

## 1. 할루시네이션 방지

### 1.1 근거 없는 주장 금지
- 코드 · 파일 · API · 라이브러리 동작을 서술할 때는 **실제 파일을 읽거나 실행해서 확인한 후** 서술한다.
- 확인하지 않은 내용은 "확인 필요"라고 명시한다. 추측을 사실처럼 쓰지 않는다.
- 파일 경로 · 함수명 · 타입명을 인용할 때는 실제 존재를 확인하고 `경로:줄번호` 로 남긴다.

### 1.2 검증 우선
- 코드 변경 후 **실행 또는 테스트로 동작을 검증**하고, 결과(성공/실패 로그)를 리포트에 그대로 기록한다.
- 테스트가 실패하면 실패했다고 정직하게 기록한다. "아마 될 것이다"류 금지.
- 크레이트 버전 · API 시그니처는 `Cargo.lock` 또는 실제 빌드에서 확인한다.

### 1.3 수치는 조건과 함께 적는다
- "지연 50ms" 는 금지. **"write latency p99 = 47ms (3-node, 동일 리전, 24h 연속, netem 없음, n=12,400)"** 로 적는다.
- 벤치마크는 **하드웨어 · 드라이버 · CUDA 버전 · 플랫폼**을 함께 적지 않으면 리포트에 싣지 않는다.
- 평균만 적지 않는다. 분산 시스템 지표는 **p50/p99 와 표본 수**를 함께 낸다.

---

## 2. 작업 루프

```text
[작업 시작]
 0. 루트 RULE.md 와 CLAUDE.md, 대상과 관련된 docs/contracts/ 를 먼저 읽는다
    (어떤 문서가 적용되는지 확인하지 못하면 파일 변경을 시작하지 않는다)
 1. docs/plans/ 에서 현재 유효한 실행계획서를 읽는다
 2. docs/history/ 최신 이력으로 직전 상태를 파악한다
[작업 수행]
 3. 계획서의 해당 단계만 수행한다 (계획에 없는 작업은 계획서 갱신 먼저)
 4. 기존 코드를 대체·삭제하면 legacy/ 에 보존한다
 5. 변경 후 실행/테스트로 검증하고 재현 명령과 출력을 docs/evidence/ 에 남긴다
[작업 종료]
 6. docs/reports/ 에 리포트를 제출한다 (필수, 생략 불가)
 7. docs/history/ 에 이력을 추가한다
 8. 진행 상태가 바뀌면 docs/plans/ 를 갱신한다
```

- 한 세션에서 **계획서 범위를 초과하는 작업을 하지 않는다.** 범위 변경은 계획서 수정이 먼저다.
- 작업 단위는 "검증 가능한 최소 단위"로 쪼갠다. 여러 기능을 섞지 않는다.

---

## 3. 구현 원칙

### 3.1 하드코딩 금지
- 엔드포인트 · 타임아웃 · 포트 · 경로 · 매직 넘버를 소스에 직접 쓰지 않는다.
- ★ **프로토콜 상수는 한 곳에서만 정의한다.**
  `clock_skew_tolerance = 60s`, `election_timeout`, `lease_duration = 10m`,
  `fragmentation_factor = 1.20`, `chunk_size = 4MiB` 등이 코드 두 곳에 나타나면 그 자체가 결함이다.
- 상수의 단일 출처는 `crates/protocol/src/constants.rs` 다. 기준선의 수치와 일치해야 한다.

### 3.2 폴백 금지
- 오류를 삼키고 임의 기본 동작으로 대체하지 않는다.
- 서명 검증 실패 · 스키마 불일치 · 시계 skew 초과는 **명시적 오류로 실패**시킨다.
- ★ **`VERIFY_OUTCOME_SCHEMA_TOO_NEW` 를 `VALID` 로 취급하는 코드는 즉시 결함이다.**
- 예외 ① 사용자에게 오류를 알리는 응답은 폴백이 아니다.
- 예외 ② **degraded mode 는 폴백이 아니다** — 단 `degraded=true` 와 사유를 남겨야 한다.
  **신호 없는 자동 축소는 폴백이다.**

### 3.3 YAGNI
- 기준선 §33 의 **현재 단계 항목만** 구현한다. 다음 단계 코드가 들어오면 되돌린다.
- 같은 기능의 두 구현이 생기면 즉시 하나로 통합하고 나머지는 `legacy/` 로 보낸다.

### 3.4 리포트 제출 의무
- **모든 작업 세션은 `docs/reports/` 리포트 제출로 종료된다.** 리포트 없는 작업은 완료가 아니다.
- 최소 포함 항목: ① 목표(어느 계획서 어느 단계) ② 수행 내용(변경 파일 목록)
  ③ 검증 방법과 결과(+`docs/evidence/` 링크) ④ 미해결 이슈 · 다음 작업

### 3.5 ★ 계약을 코드보다 먼저 고친다

gPUteer 의 계약은 **`proto/*.proto` 와 `docs/protocol/*.md`** 다.

```text
계약을 바꾸는 변경은 다음 순서를 지킨다. 뒤집지 않는다.

  1. proto / protocol 문서 수정
  2. schema_version 증가
  3. 테스트 벡터 재생성
  4. 구현 수정
  5. negative test 추가
```

- `crates/protocol` 은 `proto/*.proto` 의 구현체다. 둘이 어긋나면 **결함**이다.
- ★ **`docs/contracts/` 는 proto 를 복제하지 않는다.** 소유권 · 변경 절차 · 테스트 위치만 담는다.
  필드 목록 · 상태표 · 서명 알고리즘을 Markdown 으로 옮겨 적으면 곧 drift 가 생긴다.

### 3.6 ★ 역할 분리 (도구 중립)

작업은 **역할**로 나눈다. 어떤 AI 도구를 쓰는지는 `docs/runbooks/ai-workflow.md` 에 둔다.

| 역할 | 책임 |
|---|---|
| **설계자** | 계약(proto/protocol) 확정. 스트림 경계 정의 |
| **구현자** | 확정된 계약 안에서 구현. 계약을 수정하지 않는다 |
| **독립 검수자** | 산출물 검사. **구현자와 달라야 한다** |
| **통합 책임자** | 공용 파일 변경 승인. 최종 통합 |

**넘기기 전에 순서대로 한다.**

1. **계약을 먼저 확정한다.** `proto/` 에 타입이 없는 상태로 구현을 시작하지 않는다.
   계약 없이 돌리면 구현자가 **범위를 임의로 줄인다.**
2. **스트림을 겹치지 않게 쪼갠다.** §4 의 소유권 표를 따른다.
3. **산출물을 그대로 신뢰하지 않는다.** 검수자가 받는 즉시 검사할 것:
   - 계약 위반 (필드 누락 · 타입 불일치 · `schema_version` 누락)
   - **범위 임의 삭감** (기준선의 현재 단계 항목이 빠졌는지)
   - **negative test 누락** (§6)
   - 잘못된 근거로 쓴 주석/문서
4. **검수 결과를 기록한다.** 무엇을 되돌렸는지 리포트에 남긴다. 조용히 고치면 다음에 또 나온다.
5. ★ **구현자가 쓴 테스트로 구현자가 통과시키지 않는다.**
   계약·카오스·보안 테스트는 QA 스트림이 소유한다(§4).

---

## 4. 스트림 소유권

Rust workspace 에서는 **디렉터리 소유권만으로는 충돌을 막지 못한다.**
`Cargo.toml`, 생성 코드, 공용 trait 가 모든 스트림의 충돌점이기 때문이다.

### 4.1 스트림별 소유 영역

| 스트림 | 소유 | 책임 |
|---|---|---|
| Protocol | `proto/`, `crates/protocol/` | 스키마 · canonical 인코딩 · 상수 |
| Crypto | `crates/crypto/` | Ed25519 · BLAKE3 · nonce · 키 보관 |
| Control | `crates/control-store/` | ControlStore trait · Raft 어댑터 |
| State | `crates/state-machine/` | Node/Job/Attempt/Checkpoint/Lease 전이 |
| Runtime | `crates/runtime-*/` | Job 실행 · 샌드박스 · 체크포인트 |
| Network | `crates/p2p/`, `crates/relay/`, `crates/hub/` | direct / hole-punch / relay / hub |
| Coordinator | `crates/coordinator/` | 스케줄링 · lease · reconciliation |
| Agent | `crates/agent/` | 워커 에이전트 · 텔레메트리 |
| UI | `apps/console/` | React · Tauri 셸 |
| Python | `python/gputeer_ml/` | ML 어댑터만 |
| **QA** | `tests/vectors/`, `tools/` | **독립 검증. 다른 스트림이 수정하지 않는다** |

★ **2026-08-16 정정.** 원래 "`tests/` 전체" 라고 적었으나, 실제 테스트는
`crates/<이름>/tests/` 에 있다 (Rust 관례). 저장소 루트의 `tests/` 에는
**벡터만** 있다. 독립 검수가 이 불일치를 지적했다 — 규칙대로 따라간 사람이
테스트 위치를 잘못 찾게 된다.

```text
tests/vectors/        QA 소유. 구현자가 자기 구현에 맞춰 고치면 검증이 무의미해진다
tools/canonical/      QA 소유. 참조 구현
crates/*/tests/       각 스트림이 자기 테스트를 쓴다. QA 는 벡터로 교차검증한다
```

### 4.2 ★ 공용 파일 — 어느 스트림도 임의 수정 금지

```text
Cargo.toml (workspace)      Cargo.lock
proto/*.proto               생성된 protobuf 코드
build.rs                    docs/protocol/*
tests/vectors/*             crates/protocol/src/constants.rs
릴리스 매니페스트 스키마
```

변경이 필요하면 **`docs/contracts/` 에 변경 제안을 먼저 만들고 통합 책임자가 승인**한다.

### 4.3 Rust 충돌 방지

- 공용 trait 는 `crates/protocol` 에서 **먼저 확정**한다. 구현 크레이트는 trait 를 수정하지 않고 구현만 한다.
- 생성된 protobuf 코드를 직접 수정하지 않는다.
- `Cargo.lock` 변경은 통합 책임자가 수행한다.
- `cargo fmt` 는 변경 크레이트 범위로 실행하고, 최종 통합에서만 workspace 전체를 실행한다.
- ★ **다른 스트림의 테스트를 통과시키려고 계약이나 assertion 을 완화하지 않는다.**

### 4.4 작업 단위

"Coordinator 구현" 처럼 넓게 주면 `control-store` · `scheduler` · `lease` · 상태기계를 동시에 건드려 충돌한다.

```text
좋은 단위 예시 — P0-03 checkpoint durability
  Contract:       checkpoint 상태표 · durability 의미 · 해시 형식
  Implementation: crates/checkpoint/
  Test:           crates/checkpoint/tests/kill_chaos.rs
  Evidence:       docs/evidence/P0-03_checkpoint_durability.md
```

---

## 5. 문서 폴더 규칙

**모든 문서는 `docs/` 아래에 둔다.** 최상위에 흩뿌리지 않는다.
(`legacy/` 는 문서가 아니라 **대체된 코드**의 보존소라 최상위에 남긴다.)

각 폴더는 **답하는 질문**으로 구분한다. 같은 질문에 두 폴더가 답하면 그것이 결함이다.

| 폴더 | 답하는 질문 | 갱신 시점 |
|---|---|---|
| `docs/protocol/` | **어떻게 검증하고 전이하는가** (규범) | 계약 변경 시. **코드보다 먼저** |
| `docs/contracts/` | **누가 무엇을 구현·검증하는가** | 소유권·절차가 바뀔 때 |
| `docs/plans/` | 무엇을 언제 할 것인가 | 착수 전 / 범위 변경 / 단계 완료 |
| `docs/history/` | 언제 무엇을 했는가 | 매 세션 종료 (**수정 금지, 추가만**) |
| `docs/reports/` | 무엇을 수행했는가 | 매 세션 종료 (필수) |
| `docs/reports/debugs/` | 어떤 결함이 있었는가 | 결함 **발견 즉시** (고치기 전에도) |
| `docs/evidence/` | **실제로 입증되었는가** | DoD·P0 를 통과시켰다고 주장할 때 (필수) |
| `docs/decisions/` | **왜 그렇게 결정했는가** (ADR) | 아키텍처 결정 · P0 실패 시 |
| `docs/runbooks/` | 운영·복구는 어떻게 하는가 | 절차가 바뀔 때 |
| `docs/vision/` | 지금 안 하는 것과 그 트리거 | "지금은 안 한다"로 판정한 **그 자리에서** |
| `docs/manuals/` | 환경 구축 절차 | 절차가 바뀔 때 |
| `legacy/` | 대체된 코드 | 대체·삭제 **직전** |

### 5.1 파일명 규칙

```text
기본            YYYY-MM-DD_HHmm_<제목>.md
docs/decisions/ ADR-NNN_<제목>.md
docs/contracts/ NN_<제목>.md          (시점이 아니라 순서로 읽는다)
docs/vision/    VISION-NN_<제목>.md   (시점이 아니라 주제로 읽는다)

docs/evidence/  DoD-NN_<항목>.md      기능 DoD 검증
                P0-NNx_<항목>.md      P0 스파이크 (x 는 하위 조사. 예: P0-03a)
                ENV-NN_<항목>.md      환경 실측
                COMPAT-NN_<항목>.md   버전 스큐 · 호환성 매트릭스 (§9.3)
```

★ `contracts/` · `decisions/` · `vision/` 은 **갱신 시 새 파일을 만들지 않고 같은 번호를 고치고**
문서 안에 개정 이력을 남긴다.

### 5.2 history 기록 형식

```markdown
## YYYY-MM-DD HH:mm — <작업 제목>
- 계획: <docs/plans/ 문서명> 의 <단계>
- 스트림: <§4.1 의 스트림명>
- 수행: <핵심 변경 요약>
- 검증: <성공/실패 + 방법>
- 리포트: <docs/reports/ 파일명>
```

### 5.3 결함을 찾으면 리포트부터 쓴다

★ **고쳤는지와 무관하게 쓴다.** 못 고치는 결함일수록 기록이 남아야 한다.

① 위치(`파일:줄번호`) ② 재현 명령 ③ 실측(추정이면 추정이라고) ④ 위험도
⑤ ★ **이 결함 때문에 내가 잘못 보고했던 수치의 정정**

### 5.4 "지금은 안 한다"로 끝내지 않는다

MVP 에서 하지 않기로 판정했다면 그 자리에서 `docs/vision/TODO_VISION.md` 에 등록한다.
**등록 없이 폐기하지 않는다.**

| 필드 | 요구 |
|---|---|
| **도입 트리거** | **관측 가능한 수치**로. "규모가 커지면"은 트리거가 아니다 |
| 지금 안 하는 이유 | 측정 가능한 근거로. "복잡해서"는 이유가 아니다 |
| 예상 비용 | 생성 / 검증·통합 / 대기로 나누고 병목을 한 줄로 |
| 폐기 조건 | 트리거가 영영 오지 않을 조건 |

★ **트리거 없는 항목은 등록으로 치지 않는다.** 위시리스트이지 계획이 아니다.

---

## 6. ★ 정상 경로만으로는 완료가 아니다

**gPUteer 의 진짜 실패는 정상 경로가 아니라 장애·공격 경로에서 일어난다.**

```text
기능 DoD 는 정상 경로 테스트만으로 통과할 수 없다.
해당 기능에 가능한 장애·공격 모델이 하나라도 있는데
negative test 가 없으면 미완료다.
```

| 기능 | 필수 negative test |
|---|---|
| Lease | stale lease · 낮은 fence_epoch · 중복 renew · 재시작 후 watermark |
| Checkpoint | 쓰기 중 kill · partial file · 해시 불일치 · COMMITTED 후 replica 유실 |
| Protocol | replay · 위조 서명 · SCHEMA_TOO_NEW · non-minimal varint · map 순서 · domain_tag 교차 |
| Network | 파티션 · relay 장애 · stale 재접속 · 한 방향 단절 |
| Security | revoked 노드 재접속 · 미허가 RPC · path traversal · `pure` Job egress 시도 |
| Release | 바이너리 변조 · 롤백 · 혼합 버전 클러스터 |

테스트 위치: `tests/chaos/`, `tests/security/`, `tests/compatibility/` — **QA 스트림 소유**.

---

## 7. evidence — "파일이 있다"가 아니라 "재현 가능하다"

파일 존재만 검사하면 빈 파일도 통과한다. `scripts/verify_evidence.py` 가 **스키마를 기계 검사**한다.

`docs/evidence/_TEMPLATE.md` 의 front-matter 를 채운다. 필수 필드:

```yaml
id / claim / status / commit / binary_digests / protocol_versions
platform / hardware / network_profile / command / raw_output
artifacts / negative_tests / limitations / decision
```

★ **`limitations` 가 비어 있으면 반려한다.** 무엇을 증명하지 "않는지" 모르는 실험은 증거가 아니다.

### 7.1 판정은 이진이 아니다

| 판정 | 의미 |
|---|---|
| `PASS` | 입증됨 |
| `FAIL-ARCHITECTURE` | 실패했고 **아키텍처를 바꿔야 한다** |
| `FAIL-SCOPE` | 실패했고 **범위를 줄이면 진행 가능** |
| `INCONCLUSIVE` | 측정은 했으나 판정 불가 |
| `ENVIRONMENT-BLOCKED` | 하드웨어·환경이 없어 미실행 |
| `SUPERSEDED` | 설계 변경으로 이 실험이 무의미해짐 |

★ **`ENVIRONMENT-BLOCKED` 를 `PASS` 로 세지 않는다.** 가장 흔한 자기기만이다.

### 7.2 플랫폼 매트릭스

**한 플랫폼에서 통과했다고 다른 플랫폼을 통과로 세지 않는다.**
evidence 에 `platform` · `hardware` 를 반드시 적는다.

| 조합 | Tier | 검증 필수 |
|---|---|---|
| Windows native | S1 | CUDA · process tree kill · NTFS ACL · 프로세스별 방화벽 · fsync/rename |
| Windows AppContainer | S2 | 위 + AppContainer capability |
| Windows WSL2 | S3 | 컨테이너 격리 · GPU passthrough |
| Linux container | S3 | cgroup quota · seccomp · GPU allowlist |
| Linux gVisor | S4 | nvproxy · syscall 인터셉트 |

---

## 8. ★ P0 스파이크 실패 시 절차

**계획서만 조용히 고치지 않는다.** 실패 증거가 남아야 설계 변경 이유를 추적할 수 있다.

```text
1. docs/evidence/ 에 결과 기록 (재현 명령 + 실제 출력 + 실행 환경)
2. §7.1 판정
3. 영향 범위 식별 (기준선 §43.6 매핑표)
4. docs/decisions/ 에 ADR 신규 작성 또는 기존 ADR 갱신
5. proto -> protocol -> 계획서 -> acceptance 순서로 수정
6. 관련 contract test 수정
7. 이전 evidence 를 삭제하지 않고 SUPERSEDED 로 표시
8. 수정된 가설로 P0 재실행
9. 다음 게이트 진입 여부 재판정
```

★ **이전 evidence 를 지우지 않는다.** 실패 기록이 사라지면 같은 실패를 반복한다.

---

## 9. 프로토콜 호환성

### 9.1 스키마 불변식

```text
schema_version 없는 서명 대상 메시지 금지
field number 재사용 금지
기존 field 의 타입·의미 변경 금지
삭제한 필드는 reserved 로 표시
서명 필드는 항상 90
float / double 금지 (비율은 ppm, 시각은 밀리초 정수)
unknown field 를 조용히 무시하지 않는다
```

### 9.2 서명 검증 순서 (CI 계약)

```text
domain_tag -> schema_version -> canonical 재구성 -> sig_input 조립
 -> Ed25519 검증 -> 그 다음에야 필드 사용
```

**서명 검증 전에 어떤 필드 값도 로직에 쓰지 않는다.**
`Verified<M>` 래퍼로 타입 수준에서 강제한다(`signing.md` §13.2).

### 9.3 멀티 바이너리 버전 스큐

agent · coordinator · relay · hub 가 따로 배포된다. 모든 바이너리는 다음을 보고한다.

```text
binary_version · binary_digest · protocol_major · protocol_minor
supported_schema_range · platform · build provenance
```

```text
protocol major 불일치  -> 연결 거부
minor 불일치           -> capability negotiation, N-2 minor 까지 호환
호환 창 밖             -> UPGRADE_REQUIRED (연결은 되나 신규 Job 배치 금지)
```

CI 는 **연결 조합별 compatibility matrix** 를 생성한다:
`agent↔coordinator` · `agent↔relay` · `coordinator↔hub` · `UI↔coordinator`.

---

## 10. 동결(freeze)은 네 종류다

단일 서비스의 "기능 동결"은 분산 시스템에 맞지 않는다.

```text
schema freeze       field number / semantic 변경 금지
protocol freeze     기존 버전의 검증 의미 변경 금지
architecture freeze 관련 P0 와 ADR 이 PASS 또는 명시적 FAIL-SCOPE 된 뒤 적용
release freeze      보안 패치 · 데이터 손실 방지 · 호환성 버그만 허용
```

★ **schema freeze 이후에도 프로토콜 호환성 패치는 허용된다.**

모든 merge 는 다음을 통과해야 한다.

```bash
cargo test --workspace
python scripts/verify_evidence.py
python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
```

---

## 11. 금지 사항 요약

- ❌ 확인 안 한 사실을 단정적으로 서술
- ❌ 검증(실행/테스트) 없이 "완료" 선언
- ❌ **evidence 없이 DoD·P0 통과 주장**
- ❌ **`ENVIRONMENT-BLOCKED` 를 `PASS` 로 계상**
- ❌ **`limitations` 가 빈 evidence 제출**
- ❌ 프로토콜 상수·엔드포인트·타임아웃 하드코딩
- ❌ 오류 삼키는 폴백 / 신호 없는 degraded
- ❌ **`SCHEMA_TOO_NEW` 를 `VALID` 로 취급**
- ❌ **서명 검증 전에 필드 값 사용**
- ❌ **negative test 없이 기능 완료 선언** (§6)
- ❌ 계획서 밖의 임의 작업 / 다음 단계 항목 선제 구현
- ❌ 리포트 없이 작업 종료
- ❌ `docs/history/` 기존 기록 수정 (추가만 허용)
- ❌ **`docs/contracts/` 없이 구현 시작** (§3.6)
- ❌ **두 스트림이 같은 디렉터리를 소유** (§4.1)
- ❌ **공용 파일 임의 수정** (§4.2)
- ❌ **다른 스트림의 assertion 을 완화해서 테스트 통과**
- ❌ **`proto/` 필드·상태표·서명 알고리즘을 Markdown 으로 복제** (§3.5)
- ❌ **상위 기준선 수정** — `../gputeer_master_plan_FINAL.md` 및 원본 문서 전부
- ❌ **P0 실패 후 evidence 를 지우고 계획서만 수정** (§8)
- ❌ **"지금은 안 한다"로 판정하고 `docs/vision/` 에 미등록** (§5.4)
- ❌ **트리거 없는 비전 항목 등록**
