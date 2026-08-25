# 2026-08-24 15:28 — Elastic 추론 노드 admission 확장 vision 등록 (V-12)

- **계획:** (없음 — `RULE.md` §5.4 "지금은 안 한다"로 판정한 항목의 직접 vision 등록. `docs/plans/` 실행계획서를 거치지 않음)
- **스트림:** — (문서 전용, `docs/vision/`)
- **커밋:** 미커밋. 베이스라인 `3a0d4f5a36ec9fbe0ebed2f0defcec244a54dadf`. 같은 작업 트리에 이 작업과 무관한 다른 세션의 미커밋 변경(`crates/coordinator/src/lib.rs`, `crates/coordinator/src/checkpoint_manifest_store.rs`, `docs/plans/2026-08-24_1513_verified_checkpoint_manifest_durable_binding_v1.md`)이 있어 손대지 않았다.

## 1. 목표

별도 대화 세션에서 FreeToken(arXiv:2608.16157 — 노드 하나 안에서 GPU VRAM·CPU RAM·PCIe
대역폭을 elastic 자원 풀로 취급해 MoE 모델을 서빙하는 시스템 논문)을 검토하던 중,
gPUteer의 scheduler admission이 GPU VRAM을 이진(scalar `>=`) 판정하는 방식이 이런
elastic serving 노드에는 맞지 않을 수 있다는 아이디어가 나왔다. 이 아이디어를
"당장 구현할 계획"이 아니라 `RULE.md` §5.4가 요구하는 형식(관측 가능한 도입 트리거·
측정 가능한 보류 이유·비용 분해·폐기 조건)을 갖춰 `docs/vision/TODO_VISION.md`에
정식 등록하고, 이 저장소의 관행대로 코덱스 CLI 독립 검수를 받는 것이 목표다.
무엇이 되면 완료인가: vision 항목이 등록 규칙을 충족한다고 독립 검수가 `ACCEPTED`
판정을 내리고, `docs/history/`에 이력이 남는 것.

## 2. 수행 내용

변경 파일:

```text
docs/vision/TODO_VISION.md   (수정 — V-12 신규 등록)
```

`docs/vision/TODO_VISION.md`에 V-12(Elastic 추론 노드를 위한 admission 확장)를
V-11 뒤에 추가했다. 구성:

- 4필드 등록 표(도입 트리거 / 지금 안 하는 이유 / 예상 비용 / 폐기 조건) — 기존
  V-10·V-11과 같은 형식.
- FreeToken 배경과 gPUteer scheduler와의 레이어 관계(노드 내부 실행 계층 vs 노드 간
  배치 계층) 설명.
- 현재 admission이 `crates/scheduler/src/filter.rs`의 스칼라 VRAM 비교와
  기준선 §10.5 Shared Admission 공식으로 굳어 있다는 근거를 파일:줄 인용으로 명시.
- V-10(Job↔Agent 자동 매칭)과의 관계 — 서로 다른 축이며 V-10의 미결정 사항을
  V-12가 임의로 확정하지 않도록 명시.

이 저장소의 "구현자와 검수자가 달라야 한다"(ADR-030) 원칙을 문서 작업에도
적용하기 위해, 등록 초안 작성 후 곧바로 코덱스 CLI(`codex exec -s read-only`,
대화 기록 없는 새 인스턴스)로 독립 검수를 받았다.

## 3. 검증

```bash
codex exec -s read-only --skip-git-repo-check "<검수 프롬프트>"
```

4라운드 진행:

```text
1라운드  CHANGES_REQUESTED — 실질 결함 6건
  (1) 도입 트리거 (b)가 "런타임 채택 결정이 내려지는 시점"으로 비수치였음
  (2) FreeToken의 서로 다른 하드웨어 등급 결과("8GB 노트북→35B", "96GB 워크스테이션→753B")를
      하나로 합쳐 "8GB에 753B가 들어간다"는 근거 없는 조합을 만듦
  (3) GpuSnapshot에 host RAM 필드를 추가하자고 제안 — 실제로는 이미
      CandidateSnapshot.available_ram_bytes(model.rs:92-114)로 존재해 귀속 오류
  (4) WorkloadHint(proto/common.proto:223-236)에 SLO 필드가 없다는 계약 공백이
      예상 비용에 반영 안 됨
  (5) 기준선 §10.5(Shared Admission, 조건부 opt-in 경로)를 전체 admission
      규범인 것처럼 과대 일반화
  (6) V-10이 "아직 결정 안 함"이라 명시한 hard-filter/soft-rank 분리를
      V-12가 이미 확정된 것처럼 서술 — 두 vision 항목이 서로 모순

2라운드  CHANGES_REQUESTED — 위 6건 수정 확인, 조사 앞 띄어쓰기 오류 3곳 신규 지적
3라운드  CHANGES_REQUESTED — 3곳 수정 확인, 같은 유형의 띄어쓰기 오류 2곳 추가 발견
4라운드  ACCEPTED — 나머지 2곳 수정 확인, 신규 문제 없음
```

각 라운드에서 코덱스가 인용한 근거를 실제 파일과 대조: `filter.rs:215-229`의
`available >= required_vram` 비교, `model.rs:83-114`의 `GpuSnapshot`/`CandidateSnapshot`
필드 구분, `proto/common.proto:223-236`의 `WorkloadHint` 필드 목록,
`gputeer_master_plan_FINAL.md:1133-1144`(§10.5)·`1783-1801`(§15.3) — 전부 실재함을
1라운드 로그에서 코덱스가 직접 읽어 확인했다. 상위 기준선(`gputeer_master_plan_FINAL.md`)
무변경도 1라운드에서 코덱스가 mtime(2026-08-15 13:06:58, V-12 등록일 이전)과
SHA-256으로 대조해 `RULE.md` §11 "상위 기준선 수정 금지" 위반이 없음을 확인했다.

- evidence: 없음(vision 등록은 `RULE.md` §7 evidence 대상 — DoD/P0 — 이 아니다)
- **negative test:** 해당 없음(코드 변경이 아니라 문서 등록)

## 4. 미해결 · 다음 작업

- V-12는 vision **등록**일 뿐 실행계획이 아니다. 트리거(관측 가능한 admission
  거부 사례 또는 elastic runtime 실제 설치+calibration)가 오기 전까지는 착수하지
  않는다.
- 1라운드 검수 중 부수적으로 확인된 것: V-10의 "scheduler 자체가 미착수"라는
  서술이 `CLAUDE.md` §5의 최신 상태(scheduler `DoD-41`~`51` 완료)와 더 이상
  맞지 않는다. **이번 작업 범위 밖**이라 고치지 않았다 — V-10을 만지는 별도
  세션에서 정정이 필요하다.
- `docs/vision/TODO_VISION.md`가 자체적으로 명시한 "내용이 한 줄 이상으로
  커지면 `VISION-NN_<제목>.md`로 분리" 규칙을 V-12(및 이미 존재하는 V-06·V-10·V-11)가
  문자 그대로는 따르지 않고 있다 — 기존 항목 전부가 같은 방식으로 인라인이라
  실제 관행과 문서화된 규칙이 어긋나 있다. 이번 세션은 기존 관행(인라인 유지)을
  따랐다. 규칙 자체를 실제 관행에 맞게 고칠지, 전체 항목을 분리할지는 별도 판단이
  필요하다.

## 5. 검수 기록

1라운드가 실질 오류 6건(위 §3)을 찾아 `CHANGES_REQUESTED`. 전부 본문에 반영해
수정했다 — FreeToken 수치 조합 제거, `GpuSnapshot`→`CandidateSnapshot` 정정,
proto 계약 변경 비용 명시, §10.5 적용 범위 한정, V-10과의 모순 해소, 트리거를
수치화. 2·3라운드는 조사 앞 띄어쓰기(총 5곳)만 지적 — 조용히 고치지 않고 매
라운드 코덱스에게 재확인시켜 4라운드에서 `ACCEPTED`를 받았다.
