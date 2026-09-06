# TODO_VISION — 지금은 하지 않는 것

> ★ **"지금은 안 한다"로 판정했으면 그 자리에서 여기에 등록한다. 등록 없이 폐기하지 않는다.**
>
> 이유 둘:
> 1. **MVP 제외와 영구 기각은 다르다.** 규모가 바뀌면 판단도 바뀐다.
> 2. 기록하지 않으면 **같은 논의를 반복**한다.

## 등록 규칙

| 필드 | 요구 |
|---|---|
| **도입 트리거** | **관측 가능한 수치**로. "규모가 커지면"은 트리거가 아니다 |
| 지금 안 하는 이유 | 측정 가능한 근거로. "복잡해서"는 이유가 아니다 |
| 예상 비용 | 생성 / 검증·통합 / 대기로 나누고 병목을 한 줄로 |
| 폐기 조건 | 트리거가 영영 오지 않을 조건 |

★ **트리거 없는 항목은 등록으로 치지 않는다.** 위시리스트이지 계획이 아니다.

내용이 한 줄 이상으로 커지면 `VISION-NN_<제목>.md` 로 분리하고 여기서 링크한다.

---

## 목록

기준선 계획서 §43.3 에 등록된 항목이 원본이다. 여기서는 **구현 중 새로 판정된 것**을 추가한다.

| # | 항목 | 도입 트리거 | 원본 |
|---|---|---|---|
| V-01 | Job 샌드박스 진단 세션 (`gputeer job inspect`) | 재현 불가 장애 분기당 3건 초과, 또는 진단 번들만으로 원인 규명 실패율 30% 초과 | 기준선 §43.3 TODO-01 |
| V-02 | 임계 서명 (threshold signature) | Coordinator 5개 이상, 또는 quarantine verdict 월 10건 초과 | 기준선 §43.3 TODO-02 |
| V-03 | 분산 Job 부분 STALE 처리 | Mode B 가 4노드 이상에서 실운용 | 기준선 §43.3 TODO-03 |
| V-04 | 모바일 뷰 | 팀원 5명 이상 요청 | 기준선 §43.3 TODO-06 |

**상세는 기준선 §43.3 을 본다. 여기서 복제하지 않는다.**

---

## 신규 등록

(구현 중 새로 판정한 항목을 아래에 추가한다)

### V-05 — 스키마 지문 파서를 `prost-reflect` 로 교체

> ★ **2026-08-16 정정.** 최초 등록 시 트리거를 "`oneof` 가 1건이라도 들어오는 시점" 으로
> 적고 "현 스키마에 `oneof` 0건" 이라고 썼는데 **둘 다 틀렸다.**
> `control.proto` 에 `oneof` 가 **5개** 있고, 파서는 그 안의 필드를 **전부 잡는다**
> (`ControlAction` 21개 필드가 지문에 있고, 뮤테이션으로 확인했다).
> **틀린 한계 서술은 없는 것만큼 나쁘다** — 불필요한 교체 작업을 유발하거나
> 멀쩡한 가드를 불신하게 만든다. 트리거를 실제 한계에 맞춰 다시 적는다.

| 필드 | 내용 |
|---|---|
| **도입 트리거** | 다음 중 하나 — (a) 중첩 `message` 선언이 1건이라도 들어온다, (b) `reserved` 가 1건이라도 들어온다, (c) 기존 필드를 `oneof` 안팎으로 **이동**해야 한다 |
| 지금 안 하는 이유 | 파서가 `oneof` 필드를 정확히 잡는다(실측). 중첩 선언 **0건** · `reserved` **0건**(grep 확인). 의존성이 적은 쪽이 낫다 |
| 예상 비용 | 생성 소(파서 교체 ~100줄) / 검증 중(지문 재생성 + 66개 메시지 대조) / 대기 없음 |
| 폐기 조건 | 스키마가 v1.0 에서 동결되고 (a)~(c) 가 끝내 오지 않는 경우 |

**실제로 남아 있는 구멍**

```text
(a) 중첩 message   Inner 필드가 Outer 것으로 기록되고 번호가 겹치면 덮어쓴다
(b) reserved       필드로 오인하는지 미확인
(c) oneof 이동     번호·타입·이름을 그대로 둔 채 oneof 안팎으로 옮기면 탐지되지 않는다.
                   상호 배타성이 바뀌므로 §7.3 대상인데 지문은 통과한다
```

★ **트리거가 오는 순간이 곧 구멍이 생기는 순간이다.**
→ `signing.md` §7.3 에서 이 항목을 링크한다.
근거: `docs/evidence/P0-08_스키마_진화.md` limitations 2번 (2026-08-16 정정됨).

### V-06 — 서명된 정책 필드의 **강제 계층**

| 필드 | 내용 |
|---|---|
| **도입 트리거** | ★★ **2026-09-06 충족됐다** — `crates/agent/src/exec.rs` 가 Windows·Linux 양쪽에서 자식을 띄운다. 원문: "Agent 가 Job 을 실제로 실행하기 시작하는 시점" (v0.1 워커 착수) |
| 지금 안 하는 이유 | ★ 2026-09-06 정정 — 실행 계층은 **생겼다**. 지금 남은 것은 축마다 다르다: 자원 상한은 실제로 걸고(소프트 제한), `artifact_scope` 는 자식에게 강제 못 하고(OS 격리 필요 = `P0-02`), 방화벽은 부르는 코드가 없다. 강제할 대상이 아직 존재하지 않는다 |
| 예상 비용 | 생성 중 / 검증 대(플랫폼별 네트워크·파일 접근 제어 실측 필요) / 대기 — Linux 환경(D-3) 확보에 의존 |
| 폐기 조건 | 없음. **v0.1 필수 항목이다** |

★ `DoD-03` 이 `network(54)` · `artifact_scope(55)` · `Lease.scope(40)` 을
서명 대상에 넣었다. 그러나 **"서명에 들어갔다" 와 "Agent 가 그 정책을 강제한다" 는 다르다.**
지금 상태는 *위조를 막은 것*이지 *정책을 시행한 것*이 아니다.

이것을 "보안 필드를 서명했으니 안전하다" 로 읽으면 `CLAUDE.md` §0.4 위반이다.
근거: `docs/evidence/DoD-03_서명대상_완전성.md` limitations 마지막 항목.

★ **2026-08-17 부분 착수.** `crates/runtime-policy/` — 정책 필드가
`Enforceable` / `Suppressible` / `Unenforceable` 중 어디에 속하는지
판정하는 **순수 함수 계층**을 먼저 만들었다. 시스템 호출(OS 방화벽 ·
커널 경로 강제)은 여전히 없다 — `runtime-windows`/`runtime-container`
가 생기면 그쪽이 이 크레이트의 판정을 받아 실제로 시스템을 조작한다.

```text
artifact_scope   Enforceable(문자열)  경로 접두사 검사. TOCTOU 는 못 막는다
network          Unenforceable        OS 방화벽 백엔드가 없어 실행을 거부한다
Lease.scope      Enforceable(소유 자원) / Suppressible(외부 API)
VRAM quota       Unenforceable         (ADR-027 실측 그대로)
S1 호스트 보호   Unenforceable         (CLAUDE.md §0.4 그대로)
```

Agent 실행 계층이 아직 없다는 트리거 조건은 **여전히 유효하다** —
이 크레이트는 판정 로직만이고, ★ 2026-09-06 정정 — 소비자는 생겼다(`runtime-windows`). 판정 결과를 받아 프로세스를 실제로
격리·차단하는 소비자가 없다. `gputeer selftest` §4c 에서 판정 로직만
실행해 본다.


### V-07 — `ReplicaAck` 에 `fence_epoch` 추가

| 필드 | 내용 |
|---|---|
| **도입 트리거** | `ReplicaAck` 신선도 오판으로 인한 데이터 손실 **1건**, 또는 `COMMITTED` 선언 후 복제본 부재가 확인된 사례 **1건** |
| 지금 안 하는 이유 | `.proto` 변경 → `schema_version` 상향 필요(§7.3). 복제 계층 자체가 미구현이라 오판 사례를 관측할 수 없다 |
| 예상 비용 | 생성 소(필드 1개) / 검증 대(**schema_version 2 도입 = 버전 협상 경로 전체가 처음으로 실행된다**) / 대기 없음 |
| 폐기 조건 | 복제 계층이 ACK 대신 **주기적 재확인**(challenge-response)으로 durability 를 판정하도록 설계가 바뀌는 경우 |

★ `ReplicaAck` 는 6종 증거 중 **유일하게 `fence_epoch` 이 없다**(ADR-029).
그런데 `REPLICATED(n)` 을 세는 근거이므로 **durability 주장의 뿌리**다.
가장 약한 곳이 가장 중요한 곳이다.

**복제본이 삭제되어도 ACK 는 영원히 유효하다.** 지금은 소비 측이
`acked_at` 만 보고 신선도를 판단해야 하며, 그 규약은 강제되지 않는다.

고정 테스트: `crates/crypto/tests/lifetime_policy.rs::replica_ack_stays_valid_forever_even_if_replica_is_gone`
— **통과한다는 것이 곧 "프로토콜이 이 상황을 막지 못한다" 는 뜻**이다.

### V-08 — 증거 메시지에 서명자 ID 필드 추가

| 필드 | 내용 |
|---|---|
| **도입 트리거** | 검증자가 `signer_id` 대체값으로 키를 찾지 못해 `UnknownSigner` 를 내는 사례 **1건**, 또는 Coordinator 가 2대 이상으로 늘어나는 시점 |
| 지금 안 하는 이유 | `.proto` 변경 → `schema_version` 상향(§7.3). 단일 Coordinator(v0.1 SingleNodeStore)에서는 대체값으로 충분하다 |
| 예상 비용 | 생성 소(필드 3개) / 검증 중 / 대기 없음. **V-07 과 함께 하면 schema_version 상향을 1회로 묶을 수 있다** |
| 폐기 조건 | 없음 — 다중 Coordinator 로 가면 반드시 필요하다 |

세 메시지가 **서명자 ID 필드를 갖지 않는다.**

```text
ArtifactRef         attempt_id 로 대신 (검증자가 attempt -> node 매핑을 알아야 한다)
CanonicalDecision   job_id 로 대신     (어느 Coordinator 가 결정했는지 메시지에 없다)
RevokeLeaseNotice   lease_id 로 대신
```

★ 대체값은 **키 조회 키로 쓰이므로**, 매핑을 모르는 검증자는 유효한 서명도
`UnknownSigner` 로 거부한다. 지금은 단일 Coordinator 라 무해하다.


### V-09 — `VerifyOutcome` 에 도출 해시 불일치 값 추가

| 필드 | 내용 |
|---|---|
| **도입 트리거** | 도출 해시 불일치를 **상대에게 보고**해야 하는 시점 — 즉 Coordinator↔Agent RPC 에 `VerifyOutcome` 을 실어 보내기 시작할 때 |
| 지금 안 하는 이유 | `.proto` 변경 → `schema_version` 상향(§7.3). 아직 RPC 계층이 없어 보고할 상대가 없다. `VerifyError::Derived` 로 **로컬에서는 구분된다** |
| 예상 비용 | 생성 소(enum 값 1개) / 검증 중 / 대기 없음. **V-07 · V-08 과 함께 하면 상향 1회로 묶인다** |
| 폐기 조건 | 없음 — RPC 가 생기면 반드시 필요하다 |

★ §6.1 의 `manifest_hash` 대조가 실패했을 때 상대에게 보고할 값이 없다.
`INVALID_SIGNATURE` 로 보고하면 **"서명 위조" 로 읽히는데 서명은 정상**이다 —
원인도 대응도 다르다(`CLAUDE.md` §3).

지금은 `VerifyError::Derived` 로 **로컬에서만** 구분한다.
`err.outcome()` 이 `None` 을 반환하는 것이 "보고할 proto 값이 없다" 는 신호다.

### V-10 — Job↔Agent 자동 매칭 (신뢰도 · 하드웨어 티어 반영)

| 필드 | 내용 |
|---|---|
| **도입 트리거** | 같은 Job 의 자격 조건(`GpuRequest`/`ResourceRequest`)을 동시에 만족하는 Agent 후보가 **2대 이상**인 상황이 처음 발생하는 시점 |
| 지금 안 하는 이유 | scheduler 자체가 미착수 — Agent 가 Job 을 아직 하나도 실행하지 않아 "여러 후보 중 고른다" 는 상황 자체가 없다. 신뢰도 입력으로 쓸 이력(`QuarantineDevice`/`DeviceRevoke` 이벤트)도 아직 쌓인 게 없다 |
| 예상 비용 | 생성 중(스코어링 함수 자체는 순수 함수로 작지만, 노드별 신뢰도 이력 저장소 + 하드웨어 프로파일 조회가 선행돼야 한다) / 검증 대(공정성·기아 방지·점수 조작 저항성은 실제 다중 노드 환경 없이는 실측 불가) / 대기 — scheduler 착수(§ "미착수" 목록)에 의존 |
| 폐기 조건 | 없음 — 여러 Agent 를 동시에 운용하기 시작하면 결국 필요해진다(신뢰도 낮거나 노후 하드웨어인 노드가 섞이면 임의 배정은 곧 문제가 된다) |

★ 재미있는 확장 아이디어로 제안됨(2026-08-20, 사용자). 지금 스키마에 이미
있는 조각들을 엮으면 된다 — `GpuRequest`(`min_vram_bytes`·`cuda_runtime_version`·
`allowed_gpu_models`)가 하드웨어 티어 쪽 자격 조건이고, `QuarantineDevice`/
`DeviceRevoke`(ADR-028)가 신뢰도 쪽 신호 후보다. **아직 결정 안 된 것**:
점수를 단일 스칼라로 합칠지 사전조건(hard filter)과 우선순위(soft rank)를
분리할지, 신뢰도 점수의 시간 감쇠(decay) 규칙, 점수 조작(자기 자신에게
유리하게 이력을 세탁) 방지 — 전부 scheduler 설계 착수 시점에 다시 판단한다.

### V-11 — Coordinator의 QUARANTINED 실제 판정 정책

| 필드 | 내용 |
|---|---|
| **도입 트리거** | Coordinator 입력에 서명된 기기 위험도/신뢰도 관측값과 분류 이유가 생기고, selftest가 그 입력으로 `QUARANTINED` 결과를 재현할 수 있는 시점. 최소 1개의 관측 가능한 위험 이벤트 종류와 그 발생 횟수/시각이 저장되어야 한다 |
| 지금 안 하는 이유 | 현재 저장소에는 기기 위험도·신뢰도 이력, 위험 이벤트 수집기, 다중 Agent 선택 계층이 없다. `QUARANTINED`를 임의 epoch 비교나 CLI override로 트리거하면 정상 failover 경합(`SUPERSEDED`)과 위험 판정을 혼동하고, 근거 없는 Agent 작업 중단을 만든다 |
| 예상 비용 | 생성: 위험 이벤트 입력·영속 이력·판정 함수 / 검증·통합: 서명된 reason·관측 시각·오탐/재현성·다중 Agent selftest / 대기: scheduler와 다중 Agent 착수. 병목은 신뢰도 입력의 provenance를 먼저 확보하는 것 |
| 폐기 조건 | 기기 격리/정책 설계에서 `QUARANTINED` outcome을 제거하고 다른 서명된 상태 전이로 대체하는 결정이 기준선과 proto에서 확정되는 경우 |

이번 Lease 재발급 정책 조각에서는 위 트리거가 아직 오지 않았으므로 실제
`QUARANTINED` 계산을 만들지 않는다.

### V-12 — Elastic 추론 노드를 위한 admission 확장 (VRAM 이진 판정 → 처리량 곡선 판정)

> ★ **2026-08-24 독립 검수 반영.** 최초 등록본은 코덱스 CLI 독립 검수(read-only,
> 대화 기록 없는 인스턴스) 1라운드에서 `CHANGES_REQUESTED` — 트리거 (b)가
> 수치가 아니었고, FreeToken 논문의 서로 다른 하드웨어 등급 결과("8GB 노트북
> → 35B"·"96GB 워크스테이션 → 753B")를 하나로 합쳐 "8GB에 753B가 들어간다"는
> 근거 없는 조합을 만들었으며, `GpuSnapshot`이 아니라 이미 `CandidateSnapshot`에
> 있는 `available_ram_bytes`를 놓치고 host RAM 필드를 중복 제안했고, §10.5를
> 일반 admission 규범인 것처럼 과대 일반화했고, V-10이 "아직 결정 안 함"이라
> 명시한 hard-filter/soft-rank 분리를 이미 확정된 것처럼 서술했다. 전부 아래
> 내용에 반영해 수정했다.

| 필드 | 내용 |
|---|---|
| **도입 트리거** | 다음 중 하나 — (a) inference-class Job이 `minimum_vram_bytes_per_gpu` 미충족만으로 hard-filter에서 거부된 사례가 1건 이상 있고, 사후 확인 결과 그 노드가 VRAM+host RAM+PCIe 대역폭 조합으로는 목표 SLO(지연·tok/s)를 실제로 만족할 수 있었던 경우, (b) elastic serving runtime(모델 일부를 GPU VRAM 밖으로 내보내 CPU/RAM에서 계산하거나 필요 시에만 전송하는 방식 — 예: expert/layer 단위로 GPU↔CPU를 오가는 MoE 런타임)을 노드 **1대 이상**에 실제로 설치하고, 그 노드에서 고정 프롬프트/모델 세트로 처리량을 **1회 이상 실측(calibration)** 한 기록이 남는 시점 |
| 지금 안 하는 이유 | `crates/scheduler`의 `JobRequirements`는 아직 실제 Job 제출 → Manifest → projection 경로에 연결되지 않았고(`CLAUDE.md` §5, "`JobRequirements` projection"이 scheduler `DoD-41`~`51` 전 조각에 걸쳐 반복적으로 후속으로 남아 있음), `crates/agent`도 아직 어떤 workload도 실행하지 않는다(entrypoint 실행 자체가 미착수 — `WRITING` 마커 생성까지만 완료, `DoD-30`). "몇 개 노드가 이 확장으로 실제 이득을 보는가"를 관측할 대상 자체가 없다. 또한 `WorkloadHint`(`proto/common.proto:223-236`, [gputeer_master_plan_FINAL.md:1783-1801](../../../gputeer_master_plan_FINAL.md:1783))에는 목표 처리량/지연 SLO 필드 자체가 없어 Manifest에서 값을 받을 계약이 아직 없다. 지금 시점에 처리량 곡선 판정을 hard-filter에 넣으면 실측 없는 추정 로직을 admission 결정에 박아 넣는 셈이라 `CLAUDE.md` §0.4("강제할 수 없는 것을 보장으로 선언하지 않는다")·§1("지어내지 않는다")과 같은 종류의 위험이다 |
| 예상 비용 | 생성: 대 — ① `proto/common.proto`의 `WorkloadHint`에 목표 처리량/지연 SLO 필드 추가(schema_version 상향, §7.3 전체 검증 경로 재실행 필요) ② `CandidateSnapshot`에 이미 있는 `available_ram_bytes`([model.rs:108](../../crates/scheduler/src/model.rs))를 노드 단위 자원으로 재사용하고 GPU별 실측 PCIe 대역폭 관측값(현재 없음)을 신설 ③ hard-filter의 `available_vram_bytes >= required_vram` 스칼라 비교([filter.rs:216](../../crates/scheduler/src/filter.rs:216))를 inference class에 한정해 "이 자원 조합이 SLO를 만족하는가" 판정 함수로 교체(training 등 다른 class는 기존 이진 판정 유지) / 검증·통합: 대 — 처리량 추정 자체가 모델 크기·양자화·실측 PCIe 대역폭에 의존하는 회귀 모델이라 실측 calibration 없이는 채울 수 없다 / 대기: scheduler production 연결(Job submit ingress·`JobRequirements` projection) + 최소 1개 elastic serving runtime 채택 결정. **병목**: proto 계약 변경(①)이 나머지 전부를 막는 선행 조건이다 — schema_version을 올리기 전에는 SLO 값을 담을 그릇 자체가 없다 |
| 폐기 조건 | gPUteer가 워커 노드에서 "모델 전체가 노드 VRAM에 들어가지 않으면 그 노드는 애초에 후보에서 제외"라는 고정 배치만 지원하기로 확정하고, CPU/RAM 오프로딩을 지원 대상에서 제외하는 결정이 기준선에 반영되는 경우 |

★ FreeToken(arXiv:2608.16157 — 노드 하나 안에서 GPU VRAM·CPU RAM·PCIe 대역폭을
하나의 elastic 자원 풀로 취급해 MoE 모델을 서빙하는 시스템 논문)을 검토하다
제안됨(2026-08-24, 사용자). 이 논문은 하드웨어 등급별로 **서로 다른** 모델
크기를 서빙한 결과를 보고한다(8GB 노트북 GPU급에서 더 작은 모델, 96GB
워크스테이션 GPU급에서 훨씬 큰 모델 — 같은 모델을 여러 등급에서 돌린 비교가
아니다). 여기서는 그 정확한 수치를 재인용하지 않고 **메커니즘만** 참고한다.
**레이어 관계**: FreeToken류 런타임은 노드 하나 **내부**에서 계산·모델 상태를
GPU/CPU에 계속 재매핑하는 실행 계층이고, gPUteer scheduler는 노드들 **사이**에서
어느 Job을 어느 노드에 배치할지 정하는 계층이다 — 그 자체는 자연스러운 분리다.

문제는 지금 admission이 그 경계를 "이 노드의 VRAM에 모델이 들어가는가"라는
**이진 판정**으로 굳혀 놓았다는 것이다([filter.rs:215-223](../../crates/scheduler/src/filter.rs:215) —
`CandidateSnapshot`에 `minimum_vram_bytes_per_gpu`를 대조해 부족하면 즉시 거부.
[gputeer_master_plan_FINAL.md:1133-1144](../../../gputeer_master_plan_FINAL.md:1133)의
§10.5 **Shared Admission**(조건부 opt-in 경로 — 전체 admission 규범이 아니라
그 경로 한정) 공식도 `new_job_peak_estimate`를 노드가 반드시 흡수해야 하는
고정값으로 취급하는 사례 중 하나다). Elastic 노드에서는 VRAM보다 훨씬 큰
모델도 host RAM+PCIe를 함께 쓰면 서빙 자체는 가능해지고, 문제는 그 조합에서
나오는 처리량이 SLO를 만족하느냐로 바뀐다.

**V-10(Job↔Agent 자동 매칭)과는 다른 질문이다** — 이 항목은 inference class에
한정해 hard-filter의 판정 기준 자체(이진 → SLO 곡선)를 바꾸는 문제다. 다만
V-10 자체가 "사전조건(hard filter)과 우선순위(soft rank)를 분리할지"를 **아직
결정하지 않았다**고 명시하므로, 이 항목도 그 경계를 이미 정해진 것처럼 서술하지
않는다 — 둘 다 scheduler가 production에 연결되기 전까지는 서로 순서를 정할
필요가 없다.
