# ADR-034 · 신뢰망 — 단일 Coordinator 의 확정 등급(`COORDINATOR_DURABLE`)

- **상태:** **채택**(2026-09-28 · 사용자 위임 「최고의 규칙 코덱스랑 찾아서 적용해」 · 코덱스 e2e ACCEPTED — v5). ★ **강제 코드는 없다**(아래 "결과" 의 미구현 목록 · state-machines.md §6 검사 9) — 규범만 채택했다(사용자 위임: 「최고의 규칙 코덱스랑 찾아서 적용해」 · 2026-09-28). v1 은 코덱스 e2 가 CHANGES_REQUESTED(높음 2)
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
운영자가 서명한 풀 확정 프로필(PoolCommitProfile — 아래 6 의 선행 작업)이 지정한 **논리 Coordinator 인스턴스** 하나 · 단일 control DB generation 에서,
다음을 모두 만족한 로컬 확정.

★ "논리 인스턴스" 는 **같은 control DB 파일을 함께 쓰는 협력 프로세스 집합**이다 — 지금 신뢰망은 `coordinator-stub`(풀 모드) · `scheduler-loop` ·
  운영자 CLI(import-inventory · release-lost-node · release-held-job 등)가 같은 SQLite 파일을 **따로** 연다(런북 §0 · gputeer-coordinator.service ·
  gputeer-scheduler.service). 프로세스 사이의 직렬화는 SQLite 파일 잠금(`BEGIN IMMEDIATE`)이 준다. 한 프로세스만 쓰게 하는 잠금은 두지 않는다 —
  정상 scheduler · CLI 를 막는다(코덱스 e2 ①). generation · control_db_id 결합과 Agent 의 watermark 는 **허가된 복구 · 전환과 관측된 되감기를
  알아볼 뿐**이다 — 같은 시점에 복제한 DB 사본은 control_db_id · generation 이 같고, watermark 는 낮은 commit_seq · fence 만 거부하므로 **동시에 도는
  복제본(split-brain)은 막지 못한다**(아래 §2 "제공하지 않는다" 와 같다 · 코덱스 e2d ②). 실제로 막으려면 호스트 밖의 lease/fencing 이 따로 필요하다(미정)

  모든 writer 에 대해:
  1 파일 SQLite control DB 다(메모리 DB 가 아니다)
  2 모든 상태 변경을 BEGIN IMMEDIATE 트랜잭션 하나에서 한다
  3 PRAGMA synchronous=FULL 과 명시한 journal mode 가 **연결마다** 확인된다
  4 fence epoch · Attempt · Lease · 예약 · idempotency · 보류와 상태 전이가 **한 트랜잭션**에서 함께 확정된다
  5 DB 안의 단조 증가 commit_seq(프로세스와 무관한 전역 카운터 행)와 직전 감사 해시 · **쓴 프로세스의 신원**(coordinator · scheduler · cli 명령)을
    같은 트랜잭션에 기록한다
  6 SQLite COMMIT 성공 **전에는** ACK · Grant · 재배치 승인 · 성공 응답을 보내지 않는다
  7 결과에 commit_provenance=COORDINATOR_DURABLE · coordinator_id · control_db_id · generation · commit_seq 를 드러낸다
```

★ **replay 원장은 control DB 와 다른 파일이다**(`--replay-db` — 런북 70 · 79 · `crypto/src/durable_replay.rs`). 두 파일의 쓰기는 **원자적이지 않다** —
  조건 4 의 "idempotency" 는 control DB 안의 멱등 키(operation · 제출 · 알림 기본키)를 뜻하고, 서명 메시지 재생 방어(nonce)는 포함하지 않는다.
  두 원장을 한 파일로 모을지, 순서 규칙(재생 기록 → 상태 전이 · 상태 전이 실패는 nonce 소비만 남아 재시도가 거부되는 fail-closed)으로 둘지는
  강제 코드 조각에서 정한다 — 그 전까지 이것은 한계다

모델을 추론하거나 기본값으로 고르지 않는다(ADR-031 · `participation.rs` 와 같은 원칙). `DURABLE` · `LOCAL` 의 뜻은 세 모델에서 같다.

### 2. ★ 이것은 과반 합의가 아니다 — 잃는 것

```text
제공한다      단일 Coordinator 프로세스 안의 원자적 순서 · 정상 재시작과 SQLite 저널 복구 · 같은 DB 를 쓰는 경로 사이의 직렬화
제공하지 않는다  Coordinator 또는 디스크를 잃은 뒤의 상태 생존 · 복제 합의 · 두 Coordinator 가 다른 DB 사본으로 도는 split-brain 방지 ·
              악의적 Coordinator 의 이중 답(equivocation) 방지 · 끊긴 Agent 프로세스나 외부 부작용의 강제 정지
```

**상위 규칙 — 마지막 복구 지점 이후의 모든 권위 상태가 사라지거나 되감길 수 있다.** 저장소별로:

```text
★ 아래는 **예시이지 전부가 아니다**(코덱스 e2b ③). 복구 단위는 "control DB 파일 전체" · "replay DB 파일 전체" · "호스트의 키 · 설정 파일" · "공유 저장소" 넷이고,
  한 단위를 잃으면 그 안의 **모든 것**을 잃는다.
control DB    예: 제출 · 큐 상태와 제출 멱등 · 검증된 Manifest · pool-mode 표식(job_store) · Agent 등록 · 인벤토리 · GPU 관측 · 불일치 기억(inventory_store) ·
              생존 관측 · 재결합 대기 · 세션(node_liveness_store) · 이웃 신고(neighbor_report_store) · 시도 · Lease · fence · 예약 · 보류 · RUN_UNKNOWN(staging · lease) ·
              종료 보고(attempt_report_store) · 체크포인트 매니페스트 결합(checkpoint_manifest_store) · replica ACK(replica_ack_store) · 해제 기록 · 운영자 해제 ·
              멤버십 · 폐기 · 완료 · canonical 결정 · ACK 멱등 기록
호스트 파일    Coordinator 시드(서명 키) · 제출자 keyring · 서비스 설정 · (들어오면) 풀 확정 프로필 — DB 밖이지만 같은 호스트에 있다(런북). 시드를 잃으면
              Coordinator 신원이 바뀐다 — 모든 Agent 의 pin 을 바꿔야 한다
replay DB     이미 본 서명 메시지의 nonce — 잃으면 **옛 메시지가 다시 받아들여질** 수 있다(인사 · ACK · 갱신)
공유 저장소    체크포인트 파일 · 서명 매니페스트 — control DB 와 따로 산다. control DB 만 잃으면 매니페스트는 남지만 그것을 "확정" 으로 읽은 기록은 사라진다
```

그 결과 Job 이 사라지거나 다시 제출되고, 옛 인벤토리 · GPU 관측이 되살아나고, 옛 Agent 가 다시 유효해 보이거나, 완료한 Job 이 다시 돌거나,
폐기된 주체가 되살아난 것처럼 보일 수 있다. **그래서 복구 뒤 자동 재개하지 않는다**(아래 5).

**제품 문구 · UI · 보고서에서 세 모델의 `COMMITTED` 를 같은 것으로 보이지 않는다.** 신뢰망의 확정은 "운영자 Coordinator 한 대가 디스크에 적은 확정" 이다.

### 3. 보호 전이 — 권한과 증거는 완화하지 않는다

`COORDINATOR_DURABLE` 은 아래 전이의 **저장 등급만** 정한다. 필요한 서명 · 증거가 없으면 `Unsupported` 로 거부한다 — Coordinator 가 자기 DB 에 행을
썼다는 사실만으로 이 guard 를 채울 수 없다.

```text
멤버십 · 승인 · 정지 · 복귀 · 폐기 · 제거(§5.1 · §1 * -> REVOKED)   **운영자 루트 키**(설치 · 초대 때 따로 pin 한 공개키 — ADR-034 §3.1 · Coordinator 장치 키와
                                                              **다른** 키)의 전이별 서명과 현재 generation
canonical 결정 — 두 표의 세 전이                                   선택된 Attempt `RECONCILING -> CANONICAL | SELECTED`(§3) · 결정에 열거된 **모든 탈락**
                                                              Attempt `RECONCILING -> SUPERSEDED | NOT_SELECTED`(§3 — CanonicalDecision 의
                                                              superseded_attempt_ids · artifact.proto) · Job `RECONCILING -> COMPLETED |
                                                              CANONICAL_CHOSEN`(§2)는 **같은 서명된 CanonicalDecision 하나**에 결합되어 한 트랜잭션에서
                                                              함께 확정된다(탈락 전이만 따로 · 서명 없이 하지 않는다).
                                                              ★ 완전성 — 그 Job 의 RECONCILING 후보 집합을 C 라 하면, 트랜잭션 안에서 chosen ∈ C ·
                                                              superseded = C − {chosen} · 중복 없음 · 다른 Job · 후보 아닌 ID 없음을 검사한다.
                                                              하나라도 어긋나면 전체를 되돌린다(탈락 하나를 빠뜨린 결정으로 Job 을 완료하지 않는다) — 검증된 시도 증거 · 결정적 선택 입력이 필요하다. Job 전이의 지금 guard
                                                              ("유효 attempt 1개 이상")만으로는 신뢰망에서 이 전이를 확정하지 못한다
RUNNING -> COMPLETED · 최종 산출물 확정(§2 · §3)                   산출물 · 체크포인트의 독립 내구성 정책과 서명 · 해시 검증
```

### 3.1 운영자 루트 키의 신뢰 부트스트랩 (코덱스 e2b ① — v2 는 프로필이 자기 안의 키로 자기를 서명하는 모양이라 권위를 증명하지 못했다)

```text
pin        운영자 루트 **공개키**(또는 그 지문)를 풀 확정 프로필과 **따로** 둔다 — 초대 파일 · 설치 설정(`GPUTEER_OPERATOR_ROOT_PUBKEY`)과 Coordinator 설정에.
           모든 검증자(Agent · Coordinator · scheduler · 운영자 CLI)는 **pin 한 키**로 프로필과 멤버십 전이 서명을 검증한다 — 프로필 안에 적힌 키를 믿지 않는다
최초 설치   운영자가 루트 키를 **Coordinator 장치 키와 다른 곳**(운영자 개인 기기 · 오프라인)에서 만든다. 초대 파일에 공개키를 싣고, 팀원은 지문을 대면 · 전화로
           한 번 맞춰 본다(지금 admit-node 의 공개키 확인과 같은 절차 — 런북). 지문이 다르면 설치하지 않는다
회전       옛 루트 키가 "새 루트 공개키 · 새 generation · 발급 시각" 을 서명한 회전 증서를 낸다. 검증자는 pin 한 옛 키로 증서를 검증한 뒤에만 새 키로 pin 을
           바꾼다(연쇄는 한 단계씩 — 건너뛰지 않는다). 옛 키를 잃었으면 회전이 아니라 **새 풀**이다(모든 노드 재설치 — 자동 복구 없음)
generation  프로필의 generation 이 바뀌면(모델 전환 · control DB 복구 · 루트 회전) 검증자는 새 프로필을 pin 한 키로 검증하고, 옛 generation 의 확정 영수증을 새 것으로
           받아들이지 않는다(§4 · 시험 4 · 12)
```

★ 지금 신뢰망의 신뢰 앵커는 Coordinator 공개키 · 풀 노드 공개키 목록뿐이다(런북 — Agent 에 그 둘만 배포). 운영자 루트 키는 **아직 없다** — 위 pin · 절차가 들어오기
  전에는 아래처럼 멤버십 guard 를 채우지 못한다.

★ **지금 신뢰망은 이 guard 를 채우지 못한다** — `admit-node` 는 가입 파일의 서명을 검증하지 않고 운영자의 대면 확인에 기댄다(런북 577).
  §5.1 Member 상태기계가 배선되고 운영자 루트 키 서명이 들어오기 전까지, 신뢰망의 멤버십 변경은 `COORDINATOR_DURABLE` 확정이 아니라
  **운영자 진술(서명 없음)** 등급이다(state-machines.md §5.1 "신뢰망" 행).
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

### 6. 선행 작업 — 서명 메시지 (정의 전에는 `COORDINATOR_DURABLE` 이라 부르지 않는다)

```text
PoolCommitProfile          운영자 루트 키가 서명 — pool_id · participation=trusted-network · coordinator_id · control_db_id · generation ·
                           운영자 루트 공개키 · 허용 writer 신원 · 발급 시각. Lifetime Perpetual 에 가깝다(generation 이 바뀌면 새 프로필).
                           ★ 검증은 프로필 안의 키가 아니라 **pin 한 운영자 루트 키**로 한다(§3.1)
OperatorRootRotation       옛 루트 키가 서명 — 새 루트 공개키 · 새 generation · 발급 시각(§3.1 회전)
CoordinatorCommitReceipt   Coordinator 가 서명 — 응답에 싣는 확정 영수증: commit_provenance · coordinator_id · control_db_id · generation · commit_seq ·
                           전이 요약 해시. Lifetime Evidence(관측 시각 노출 · fence 로 신선도)
```

세 메시지 모두 **아직 정의하지 않았다**(signing.md §5 domain 표 · canonical 벡터 · 참조 구현 · 지문 등록 필요 — signing.md 의 신규 서명 메시지 원칙).
정의 · 등록 · 응답 결합이 끝나기 전에는 신뢰망의 로컬 확정을 `COORDINATOR_DURABLE` 이라 **부르지 않는다**(state-machines.md §0.1 · §6 검사 9) —
시험 11 · 14 는 PoolCommitProfile 과 CoordinatorCommitReceipt 가 정의된 뒤에, 루트 회전 시험은 OperatorRootRotation 이 정의된 뒤에 구현할 수 있다.

## 근거

- 전이마다 모델별 값을 따로 두면 표가 두 배가 되고 누락을 찾기 어렵다 — §0.1 이 `BROKER_ATTESTED` 를 한 곳에 둔 이유와 같다.
- 그러나 일률 완화는 §7 미해결 4 가 걱정한 전이(canonical · 폐기 · 완료)까지 로컬 판단으로 확정하게 만든다. 저장 등급과 **결정 권한 · 증거**를
  떼어 놓으면, 일률 해석을 유지하면서 그 전이를 Coordinator 단독 판단으로 줄이지 않는다.
- 신뢰망의 신뢰 앵커(지금: 운영자가 배포한 Coordinator 공개키 · 풀 노드 공개키 목록 — §3.1 이 들어오면 pin 한 운영자 루트 키가 더해진다)는 사설 팀
  (Genesis + Owner 키)과도 공개 풀(Broker 키)과도 다르다 — 세 번째 모델로 두는 것이 사실과 맞다.

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
문서(이 ADR 과 함께)   state-machines.md §0 · §0.1(세 모델 해석 · 보호 전이) · §5.1(신뢰망 권위 행) · §6(검사 9) · §7(7행 추가 — 4행 공개 풀 · 5행 모드 전환은 미해결 그대로)
코드 — 이 ADR 과 함께   participation.rs: TrustedNetwork("trusted-network") · 확정 등급 판정(commit_profile). ★ 배선 없음 — 선택자만(ADR-031 원칙).
                        scheduler/src/reassignment.rs 의 복제 enum 은 두 모델만 갖는 **제한된 내부 타입**으로 남긴다(신뢰망 요청은 그 커널로 오지 않는다 —
                        배선 때 variant · 정족수 정책 · 시험을 함께 넣는다)
코드 — 미구현(강제 전 필수 · 단계 2 활성화 전 선행)
  control DB 연결을 open_control_db_durable() 하나로 모은다 — foreign_keys · synchronous=FULL · journal mode · 파일 DB 여부를 연결마다 확인
    (지금 failover.rs 는 새 연결을 직접 열고 busy timeout 만 건다 — 같은 설정을 증명할 수 없다)
  control_db_id / generation / coordinator_id 결합(모든 writer 가 확인) · 빈 새 DB 를 같은 generation 으로 자동 초기화하지 않음 · DB 안 전역 commit_seq ·
    writer 신원 기록 · replay 원장과의 순서 규칙(또는 통합) · PoolCommitProfile · OperatorRootRotation · CoordinatorCommitReceipt 정의(6 — 셋 다 proto · domain tag · canonical 벡터 · 참조 구현 · 지문 등록)
  Agent 가 (pool_id, generation, fence_epoch, commit_seq) watermark 를 영속하고 되감긴 Coordinator 응답을 거부
  fence 카운터가 Attempt · Lease · 예약의 최대값보다 작으면 fail closed
  보호 전이의 서명 · 증거 guard(없으면 Unsupported) · commit_seq · 감사 해시 체인 · provenance 노출
  재시작 때 무결성 · 프로필 · 감사 머리 검증 · fence 복구 · 활성 Lease · 보류 · 예약 대조가 끝날 때까지 스케줄링하지 않음
```

## 시험 (negative test — 강제 코드와 함께)

```text
1 모델 누락 · 오타 · 구버전 바이너리의 trusted-network → 시작 거부        9 (물리 저장소 fencing 을 넣으면) 옛 토큰의 파일 확정 거부
2 메모리 DB · synchronous != FULL · 예상 밖 journal mode → 시작 거부       10 멤버십 · 폐기 · canonical · 완료를 서명 · 증거 없이 → Unsupported
3 같은 DB 를 두 **논리 인스턴스**(다른 coordinator_id · generation)가 쓰려 하면 거부 · 같은 인스턴스의 scheduler · CLI 는 받아들임                        11 COORDINATOR_DURABLE 기록을 PublicPool 의 BROKER_ATTESTED 로 소비 → 거부(반대도)
4 같은 generation 의 복사된 DB 두 개 → Agent watermark 가 **낮은** commit_seq · fence 를 거부(더 높은 독립 분기는 못 막는다 — 한계 확인용)   12 오래된 백업 복원이 같은 generation 으로 조용히 시작하지 못함
5 COMMIT 전 crash → ACK 없음 · 상태 없음 / COMMIT 뒤 ACK 전 crash → 재시작 뒤 같은 결과를 멱등 반환   13 일관된 백업 복원 뒤 보류 · Lease 폐기 · 예약 · fence · 감사 머리 보존
6 디스크 가득 · fsync · COMMIT 실패 · 저널 복구 실패 → 성공 응답 없음       14 UI · API · evidence 에 provenance 없이 "COMMITTED" 만 보이면 실패
16 canonical 결정의 탈락 목록이 후보 하나를 빠뜨림 · 중복 · 다른 Job · 후보 아닌 ID → 선택 · 탈락 · Job 완료 모두 되돌림
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
| 2026-09-28 | 채택 — 코덱스 e2e(v5 대상) ACCEPTED · 결함 없음 |
| 2026-09-28 | v5 — 코덱스 e2d(중간 2 · 낮음 1) 반영: canonical 탈락 목록 완전성 검사 · split-brain 을 "가린다" 에서 "알아볼 뿐 · 막지 못한다" 로 낮춤 · 선행 메시지 이름 명시 · 시험 16 |
| 2026-09-28 | v4 — 코덱스 e2c(중간 2 · 낮음 1) 반영: §5.1 권위 문구를 pin 한 키로 · canonical 결정에 탈락 Attempt SUPERSEDED 까지 · 선행 메시지 셋 |
| 2026-09-28 | v3 — 코덱스 e2b(높음 2 · 중간 2 · 낮음 1) 반영: 운영자 루트 키 신뢰 부트스트랩(§3.1 — pin · 최초 설치 · 회전 · generation) · canonical 보호를 Job `CANONICAL_CHOSEN` 까지 두 전이 · 한 서명 결정으로 · 손실 목록은 예시임을 밝히고 복구 단위 · 빠진 표 · 호스트 파일 · state-machines §0.1 요약을 ADR 참조로 · scheduler 주석 정리 |
| 2026-09-28 | v2 — 코덱스 e2(높음 2 · 중간 3 · 낮음 2) 반영: "단일 Coordinator" → 같은 DB 를 쓰는 협력 프로세스의 논리 인스턴스(프로세스 잠금 철회) · replay 원장 비원자 한계 · 신뢰망 멤버십 권위(운영자 루트 키 · 지금 admit-node 는 못 채움) · 서명 메시지 둘을 선행 작업으로 · 손실 목록을 저장소별로 · scheduler 복제 enum 제한 · 변경 목록 정정 |
