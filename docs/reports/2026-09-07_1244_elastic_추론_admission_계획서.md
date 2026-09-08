# 2026-09-07 12:44 — V-12(elastic 추론 admission) 실행계획서 작성

- **계획:** `docs/plans/2026-09-07_1244_elastic_추론_admission_v1.md` — 이 세션이 만든 것. 어느 단계도 착수하지 않았다
- **스트림:** 문서 (`docs/plans/` · `docs/vision/` · `docs/history/`)
- **커밋:** 미커밋

## 1. 목표

사용자 질문 "FreeToken 기술을 우리 프로젝트에 적용하기로 했는데 얼만큼 적용돼
있어? 계획서만 짜인 상태야?" 에 답하고, 이어진 지시 "계획서는 작성해놔" 를
수행한다. 무엇이 되면 완료인가: `RULE.md` 템플릿을 따르는 실행계획서가
`docs/plans/` 에 있고, 그것을 가리켜야 하는 문서(vision · 열린 작업 · 이력)가
같이 고쳐져 있는 것(`RULE.md` §6.5).

## 2. 조사 결과 — 적용 정도

**코드 0줄, 계획서 0건. vision 등록만 있었다.**

```text
docs/vision/TODO_VISION.md V-12          2026-08-24 등록, 코덱스 4라운드 ACCEPTED
docs/plans/                              V-12 를 다루는 계획서 없음
crates/scheduler/src/filter.rs:215-229   available_vram_bytes >= required_vram  스칼라 그대로
proto/common.proto:223-237               WorkloadHint 에 SLO/처리량/지연 필드 없음
crates/ · proto/ grep                    slo | tokens_per_sec | throughput | pcie_bandwidth | offload  → 0건
docs/evidence/                           calibration · elastic 관련 기록 없음
_열린_작업.md A1-15                      "트리거가 안 왔다. 착수 가능한 일이 아니다"
```

부수 확인 — V-12 "지금 안 하는 이유" 세 줄 중 둘이 낡았다.

```text
"JobRequirements 가 projection 경로에 안 연결됨"  → crates/coordinator/src/manifest_requirements.rs 있음.
                                                    import-manifest(DoD-67) · plan-job · scheduler-tick 이어짐
"crates/agent 가 workload 실행 안 함"              → 2026-09-06 감사에서 이미 낡음 판정. exec.rs 가 실행
"WorkloadHint 에 SLO 필드 없음"                    → 여전히 참. 유일한 병목
```

## 3. 수행 내용

변경 파일:

```text
docs/plans/2026-09-07_1244_elastic_추론_admission_v1.md          (신규 — 실행계획서)
docs/vision/TODO_VISION.md                                       (수정 — V-12 머리에 "실행계획으로 올라감" 주석. 원문 보존)
docs/plans/_열린_작업.md                                         (수정 — A1-15 에서 V-12 분리해 16 신설 · §B 에 S0 계약 승인 행)
docs/history/HISTORY.md                                          (추가)
docs/reports/2026-09-07_1244_elastic_추론_admission_계획서.md    (신규 — 이 문서)
```

계획서의 핵심 결정과 그 이유:

| 결정 | 이유 |
|---|---|
| v1 은 처리량을 **추정하지 않는다.** 운영자가 반입한 calibration 실측 점이 요구를 8조건으로 지배하는지만 본다 | V-12 검수 1라운드가 잡은 결함이 정확히 "실측 없는 수치 조합" 이었다. 점 하나로 곡선을 그리면 같은 병이다 |
| 새 필드 없는 Job 은 새 관문에 진입하지 않는다 | 기존 scheduler 테스트 전부가 무변경 통과해야 한다는 S3 첫 완료 기준으로 고정 |
| `rank_best_fit` 무변경 | V-10 이 hard/soft 분리를 결정하지 않았다. elastic 통과 후보도 `minimum_vram_bytes_per_gpu` 는 만족하므로 rank 가 깨질 이유가 없다(계획 §4.4) |
| `minimum_vram_bytes_per_gpu` 의미 전환을 계약에 명시 | "모델 크기" → "상주 하한". 옛 방식으로 적으면 elastic 경로가 안 열릴 뿐 오동작하지 않는다 — 안전한 쪽으로 낡는다 |
| proto 변경은 `WorkloadHint` 13~16 네 필드 · `SCHEMA_VERSION` 2→3 · S0 제안서 승인 뒤 | `RULE.md` §3.5 · §4.2. 벡터 52건 재생성과 스큐 창이 걸려 사용자(통합 책임자) 결정 |
| calibration 은 proto 아님 — bootstrap 문서 schema 1→2 | `AgentInventory` 가 Rust 구조체+JSON 이라 서명·벡터에 영향 없음(`import_inventory.rs` 주석) |
| 트리거 (b) 를 S1 에서 x600 으로 직접 만든다 | 트리거가 곧 입력 데이터다. E: 규칙 · WSL 은 사용자 명령 뒤 · 3회 최솟값 |
| PCIe 는 출처 보존만 | `runtime-nvml` 에 PCIe 조회 없음(grep 0건). 같은 노드에서 잰 calibration 에 이미 반영돼 있다 |

## 4. 검증

코드 변경이 없다. 문서 검사기만 돌렸다.

```bash
python scripts/check_docs.py
python scripts/claims_check.py
```

```text
문서 구조 검사 — C:\Users\playdata2\Documents\test_workspace\gPUteer\gputeer
==============================================================
  이상 없음
--------------------------------------------------------------
오류 0 · 경고 0

claims_check.py   출력 없음 · exit=0   (부재 주장 표 전부 통과)
```

★ HISTORY 에 항목을 넣는 사이 다른 세션이 `2026-09-07 03:30` 항목을 먼저
추가해 삽입 위치가 한 번 어긋났다. 재확인 후 그 위에 넣었고, 기존 기록은
건드리지 않았다(append-only 유지 — `check_docs.py` 통과가 그 증거다).

- evidence: 없음 (계획서 작성은 `RULE.md` §7 evidence 대상이 아니다)
- **negative test:** 해당 없음 (코드 변경 아님)

## 5. 미해결 · 다음 작업

- **독립 검수 미실시.** 코덱스 쿼터 복구 2026-09-07 15:43 이후, `docs/runbooks/검수_대기열.md` 순번 **뒤에** 이 계획서를 검수받는다. V-12 등록본이 1라운드에 결함 6건을 받았던 전례가 있어 이 계획서도 같은 종류의 오류(수치 근거 · 필드 귀속 · 규범 과대 일반화)를 검수 초점으로 준다.
- **사용자 결정 3건**(계획 §11): S0 승인 · S1 WSL 사용 여부 · calibration 모델 선택.
- 계획 §7 의 `DoD-NN` 번호는 착수 시 `ls docs/evidence` 로 확정한다(지금 최신 DoD-67, DoD-68 초안 검수 대기).
- `CLAUDE.md` §5 는 이번에 안 고쳤다 — 계획서 작성은 상태표 항목이 아니다. S2 착수 시 "진행 중" 행을 추가한다.

## 6. 검수 기록

이 세션 안에서 다른 스트림 산출물을 받지 않았다. 독립 검수는 위 §5 대로 대기.
