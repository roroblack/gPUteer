# ADR-034 · 신뢰망 — 단일 Coordinator 의 확정 등급(`COORDINATOR_DURABLE`)

- **상태:** 제안 — 독립 검수 중(사용자 위임: 「최고의 규칙 코덱스랑 찾아서 적용해」 · 2026-09-28)
- **날짜:** 2026-09-28
- **관련:** ADR-031(참여 모델) · ADR-032 · ADR-033 §5(`BROKER_ATTESTED`) · `docs/protocol/state-machines.md` §0 · §0.1 · §7 ·
  `crates/protocol/src/participation.rs` · 단계 2 계약 제안 `docs/contracts/proposals/2026-09-28_1034_실행여부불명_재배치보류_Lease_Attempt.md` 결정 D9 ·
  `CLAUDE.md` §0.1 · §0.3 · §0.4

## 배경

`state-machines.md` §0 은 `COMMITTED` 를 **과반 합의**로 정의하고 "SingleNodeStore 에서는 불가능" 이라 적는다. §0.1 은 공개 풀에서 그것을
`BROKER_ATTESTED` 로 읽는다고 정했다. 그런데 이 브랜치가 운영하는 **신뢰망 풀**(운영자가 띄운 Coordinator 하나 · SQLite control DB 하나 ·
`coordinator-stub --pool-mode true`)은 둘 다 아니다. 지금 신뢰망은 규범상 `COMMITTED` 인 전이(Job · Attempt · Lease)를 **이미** 로컬 SQLite
트랜잭션으로 확정하고 있다 — 규범이 그 사실을 말하지 않는 **조용한 완화**다(§6 검사 3 ❌).

단계 2 계약(실행 여부 불명 → 재배치 보류)은 보류 · 해제 · 알림 저장을 `COMMITTED` 로 요구한다. 이 ADR 이 없으면 그 계약은 운영에 연결할 수
없다(제안서 "활성화 선행 조건"). 사용자가 2026-09-28 결정 D9 를 코덱스와 찾아 적용하라고 위임했다(코덱스 논의 d1 — `READY_TO_APPLY`).

## 결정

**신뢰망을 세 번째 참여 모델(`trusted-network`)로 두고, 그 모델에서 표의 `COMMITTED` 를 `COORDINATOR_DURABLE` 로 읽는다.** 해석은 모델별로
`state-machines.md` §0.1 한 곳에서 **모든 `COMMITTED` 전이에 일률로** 정한다(전이마다 뜻을 바꾸지 않는다). 다만 **보호 전이**(아래)의
**권한과 증거** guard 는 Coordinator 의 로컬 쓰기로 대체하지 않는다 — 저장 등급만 완화하고, 누가 결정할 수 있는지와 산출물 내구성은 완화하지 않는다.

### 1. `COORDINATOR_DURABLE` 의 정의

```text
운영자가 서명한 풀 확정 프로필(PoolCommitProfile)이 지정한 단일 Coordinator 와 단일 control DB generation 에서, 다음을 모두 만족한 로컬 확정:
  1 파일 SQLite control DB 다(메모리 DB 가 아니다)
  2 모든 상태 변경을 BEGIN IMMEDIATE 트랜잭션 하나에서 한다
  3 PRAGMA synchronous=FULL 과 명시한 journal mode 가 **연결마다** 확인된다
  4 fence epoch · Attempt · Lease · 예약 · idempotency · 보류와 상태 전이가 **한 트랜잭션**에서 함께 확정된다
  5 단조 증가 commit_seq 와 직전 감사 해시를 같은 트랜잭션에 기록한다
  6 SQLite COMMIT 성공 **전에는** ACK · Grant · 재배치 승인 · 성공 응답을 보내지 않는다
  7 결과에 commit_provenance=COORDINATOR_DURABLE · coordinator_id · control_db_id · generation · commit_seq 를 드러낸다
```

모델을 추론하거나 기본값으로 고르지 않는다(ADR-031 · `participation.rs` 와 같은 원칙). `DURABLE` · `LOCAL` 의 뜻은 세 모델에서 같다.

### 2. ★ 이것은 과반 합의가 아니다 — 잃는 것

```text
제공한다      단일 Coordinator 프로세스 안의 원자적 순서 · 정상 재시작과 SQLite 저널 복구 · 같은 DB 를 쓰는 경로 사이의 직렬화
제공하지 않는다  Coordinator 또는 디스크를 잃은 뒤의 상태 생존 · 복제 합의 · 두 Coordinator 가 다른 DB 사본으로 도는 split-brain 방지 ·
              악의적 Coordinator 의 이중 답(equivocation) 방지 · 끊긴 Agent 프로세스나 외부 부작용의 강제 정지
```

디스크를 (반출되지 않은 백업 · 감사 꼬리와 함께) 잃으면 RUN_UNKNOWN · D6 보류 · Lease 폐기와 최신 fence · 예약 소유 · 완료 · canonical 결정 ·
멤버십 · 폐기 최신 상태 · ACK 멱등 기록을 잃는다. 옛 Agent 가 다시 유효해 보이거나, 완료한 Job 이 다시 돌거나, 폐기된 주체가 되살아난 것처럼
보일 수 있다. **그래서 복구 뒤 자동 재개하지 않는다**(아래 5).

**제품 문구 · UI · 보고서에서 세 모델의 `COMMITTED` 를 같은 것으로 보이지 않는다.** 신뢰망의 확정은 "운영자 Coordinator 한 대가 디스크에 적은 확정" 이다.

### 3. 보호 전이 — 권한과 증거는 완화하지 않는다

`COORDINATOR_DURABLE` 은 아래 전이의 **저장 등급만** 정한다. 필요한 서명 · 증거가 없으면 `Unsupported` 로 거부한다 — Coordinator 가 자기 DB 에 행을
썼다는 사실만으로 이 guard 를 채울 수 없다.

```text
멤버십 · 승인 · 정지 · 복귀 · 폐기 · 제거(§5.1 · §1 * -> REVOKED)   구성된 Owner/운영 권위의 전이별 서명과 현재 generation
RECONCILING -> CANONICAL(§3)                                   검증된 시도 증거 · 결정적 선택 입력 · 서명된 CanonicalDecision
RUNNING -> COMPLETED · 최종 산출물 확정(§2 · §3)                   산출물 · 체크포인트의 독립 내구성 정책과 서명 · 해시 검증
```

★ 체크포인트 **상태 이름** `COMMITTED`(§4 — replica 요구)와 표의 durability 열 `COMMITTED` 는 다르다. 이 ADR 은 체크포인트 replica 요구를 낮추지 않는다.

### 4. `BROKER_ATTESTED` 와의 관계 · 모델 전환

```text
한 풀 generation 에는 한 모델만     PublicPool 에서는 로컬 SQLite 확정만으로 COMMITTED 가 아니다(Broker 서명 · serial 이 먼저)
                                  TrustedNetwork 에서는 Broker 서명을 요구하지 않지만, Broker 기록을 COORDINATOR_DURABLE 로 다시 읽지 않는다
모델 전환                         활성 Lease · RUN_UNKNOWN · 보류가 모두 풀리고, 새 generation 과 기존 watermark 보다 높은 fence 기준점을 서명한 뒤에만.
                                  실행 중 플래그 변경 · 같은 generation 의 혼합은 거부(ADR-033 의 "권위 전환" 과 같은 원칙)
```

### 5. 백업 · 감사 · 복구

지금 journal mode 는 `DELETE` 다(`job_store.rs` · `lease_store.rs` 의 초기화) — "WAL shipping 을 한다" 고 쓰지 않는다. 초기 구현은:

```text
SQLite online backup API 로 일관된 스냅샷 · 다른 호스트의 서명된 해시 체인 감사 로그 · 주기적 복원 시험 ·
백업 시점의 control_db_id · generation · commit_seq · audit_head · fence_epoch 기록 · 백업 · 감사 지연을 UI 에 표시 ·
설정한 최대 복구 지연을 넘으면 새 COMMITTED 전이를 fail closed
```

오래된 백업을 복원하면 **같은 generation 으로 조용히 시작하지 못한다** — 운영자 복구 절차에서 멈춘다. 백업 · 비동기 감사는 이 등급을 과반 합의로
올리지 않는다(마지막 백업 뒤의 꼬리는 디스크와 함께 사라질 수 있다).

## 근거

- 전이마다 모델별 값을 따로 두면 표가 두 배가 되고 누락을 찾기 어렵다 — §0.1 이 `BROKER_ATTESTED` 를 한 곳에 둔 이유와 같다.
- 그러나 일률 완화는 §7 미해결 4 가 걱정한 전이(canonical · 폐기 · 완료)까지 로컬 판단으로 확정하게 만든다. 저장 등급과 **결정 권한 · 증거**를
  떼어 놓으면, 일률 해석을 유지하면서 그 전이를 Coordinator 단독 판단으로 줄이지 않는다.
- 신뢰망의 신뢰 앵커(운영자가 배포한 Coordinator 공개키 · 풀 노드 공개키 목록)는 사설 팀(Genesis + Owner 키)과도 공개 풀(Broker 키)과도 다르다 —
  세 번째 모델로 두는 것이 사실과 맞다.

## 대안과 기각 사유

| 대안 | 기각 사유 |
|---|---|
| (나) 단계 2 계약의 전이만 완화 | 규범이 전이마다 달라져 읽기 어렵다(§0.1 이 행별 모드 값을 피한 이유). 신뢰망이 이미 다른 COMMITTED 전이를 로컬로 확정하는 사실은 그대로 숨는다 |
| (다) Raft 를 기다림 | 단계 2 계약을 운영에 연결하지 못한다. 지금의 조용한 완화도 그대로 남는다 |
| 일률 완화(보호 전이 없이) | canonical · 폐기 · 완료가 운영자 Coordinator 한 대의 로컬 판단으로 확정된다 — 사후 탐지로는 되돌릴 수 없는 전이다 |

## 결과

- 가능해지는 것: 신뢰망에서 `COMMITTED` 전이를 규범대로 부를 수 있다 · 단계 2 계약의 운영 활성화 조건 하나가 채워진다(아래 강제 코드가 들어온 뒤).
- 포기하는 것: 디스크 유실 생존 · 복제 합의 — 위 2 에 적었다.
- 바꿀 문서 · 코드:

```text
문서(이 ADR 과 함께)   state-machines.md §0 · §0.1(세 모델 해석 · 보호 전이) · §6(검사 항목) · §7(4 · 5 행 갱신)
코드 — 이 ADR 과 함께   participation.rs: TrustedNetwork("trusted-network") · 확정 등급 판정(commit_profile). ★ 배선 없음 — 선택자만(ADR-031 원칙)
코드 — 미구현(강제 전 필수 · 단계 2 활성화 전 선행)
  control DB 연결을 open_control_db_durable() 하나로 모은다 — foreign_keys · synchronous=FULL · journal mode · 파일 DB 여부를 연결마다 확인
    (지금 failover.rs 는 새 연결을 직접 열고 busy timeout 만 건다 — 같은 설정을 증명할 수 없다)
  control DB 별 OS 프로세스 잠금 · control_db_id / generation / coordinator_id 결합 · 빈 새 DB 를 같은 generation 으로 자동 초기화하지 않음
  Agent 가 (pool_id, generation, fence_epoch, commit_seq) watermark 를 영속하고 되감긴 Coordinator 응답을 거부
  fence 카운터가 Attempt · Lease · 예약의 최대값보다 작으면 fail closed
  보호 전이의 서명 · 증거 guard(없으면 Unsupported) · commit_seq · 감사 해시 체인 · provenance 노출
  재시작 때 무결성 · 프로필 · 감사 머리 검증 · fence 복구 · 활성 Lease · 보류 · 예약 대조가 끝날 때까지 스케줄링하지 않음
```

## 시험 (negative test — 강제 코드와 함께)

```text
1 모델 누락 · 오타 · 구버전 바이너리의 trusted-network → 시작 거부        9 (물리 저장소 fencing 을 넣으면) 옛 토큰의 파일 확정 거부
2 메모리 DB · synchronous != FULL · 예상 밖 journal mode → 시작 거부       10 멤버십 · 폐기 · canonical · 완료를 서명 · 증거 없이 → Unsupported
3 같은 DB 를 두 Coordinator 가 열면 두 번째 거부                        11 COORDINATOR_DURABLE 기록을 PublicPool 의 BROKER_ATTESTED 로 소비 → 거부(반대도)
4 같은 generation 의 복사된 DB 두 개 → Agent watermark 가 낮은 commit_seq · fence 거부   12 오래된 백업 복원이 같은 generation 으로 조용히 시작하지 못함
5 COMMIT 전 crash → ACK 없음 · 상태 없음 / COMMIT 뒤 ACK 전 crash → 재시작 뒤 같은 결과를 멱등 반환   13 일관된 백업 복원 뒤 보류 · Lease 폐기 · 예약 · fence · 감사 머리 보존
6 디스크 가득 · fsync · COMMIT 실패 · 저널 복구 실패 → 성공 응답 없음       14 UI · API · evidence 에 provenance 없이 "COMMITTED" 만 보이면 실패
7 fence 카운터 < Attempt · Lease 최대값 → 스케줄링 거부                   15 전이와 감사 행 사이 실패 주입 → 한쪽만 성공하지 않음
8 더 높은 fence 뒤 옛 시도의 체크포인트 매니페스트 → 중앙 수락 · 재개 · canonical 후보 등록 거부
```

## 되돌리는 조건

신뢰망에 과반 ControlStore(Raft 등)가 들어와 `QUORUM_COMMITTED` 를 채울 수 있으면 신뢰망 모델의 해석을 사설 팀과 같게 바꾼다 — 그때 이 ADR 은
"대체됨" 이 된다. 디스크 유실로 확정이 되돌려진 사고가 한 번이라도 관측되면 백업 · 감사의 동기 요구(off-host archive ACK 전 성공 응답 금지)를 올린다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-09-28 | 최초 작성 — 코덱스 논의 d1 의 C 절을 규범 형태로 옮김. 독립 검수 전 |
