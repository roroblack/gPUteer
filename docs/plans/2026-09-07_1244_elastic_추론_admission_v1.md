# Elastic 추론 노드 admission 확장 (V-12 실행계획) v1

- **기준선:** `../gputeer_master_plan_FINAL.md` §10.5(Shared Admission — 조건부 경로), §13.2(scheduler), §15.3(`WorkloadHint`)
- **출처:** `docs/vision/TODO_VISION.md` **V-12**. 2026-08-24 등록, 코덱스 독립 검수 4라운드 `ACCEPTED`
- **대상 단계:** v0.2 (scheduler production 연결 뒤)
- **선행 게이트:** S0(계약 제안 승인) · S1(calibration 실측 1회) — 아래 참조. 둘 다 끝나기 전에 S2 이후 코드를 쓰지 않는다
- **작성:** 2026-09-07 · **상태:** 초안 — 독립 검수 전
- **추정 규모:** production Rust 300~500줄 · 테스트 400~700줄 · proto 4필드 · calibration 실측 1일 · **3~5일**. 산정이지 실측이 아니다

## 0. 왜 지금 계획서를 쓰는가 — 정직하게 적는다

V-12 는 `RULE.md` §5.4 형식으로 등록된 **보류 항목**이다. 등록된 도입 트리거는
둘 중 하나였다.

```text
(a) inference Job 이 VRAM 부족만으로 hard-filter 에서 거부됐는데, 사후 확인 결과
    그 노드가 VRAM+host RAM+PCIe 조합으로는 SLO 를 만족할 수 있었던 사례 1건
(b) elastic serving runtime 을 노드 1대 이상에 실제 설치하고, 고정 프롬프트/모델
    세트로 처리량을 1회 이상 실측(calibration)한 기록
```

**2026-09-07 현재 둘 다 오지 않았다.** `docs/evidence/` 에 해당 기록이 없고,
2026-09-06 문서 감사([`2026-09-06_0800_문서_감사.md`](../reports/2026-09-06_0800_문서_감사.md))도
"둘 다 저장소 밖의 운영 사실이라 코드로 확인할 수 없다"로 판정했다.

이 계획서는 **사용자 결정(2026-09-07, "FreeToken 기술을 우리 프로젝트에 적용하기로
했다")**으로 착수한다. 트리거를 기다리는 대신 **트리거 (b)를 S1 에서 직접 만든다.**
이것은 규칙 회피가 아니다 — (b)는 어차피 이 기능의 **입력 데이터** 자체라서,
calibration 없이는 S2 이후를 만들 수 없다. 트리거가 곧 선행 조건이다.

★ **V-12 의 "지금 안 하는 이유" 세 줄 중 둘은 이미 낡았다**(2026-09-06 감사에서
하나, 이번 조사에서 하나 더).

```text
"JobRequirements 가 Manifest → projection 경로에 연결되지 않았다"
    → 낡았다. crates/coordinator/src/manifest_requirements.rs 가 그 변환기다.
      gputeer import-manifest(DoD-67) · plan-job · scheduler-tick 이 이어져 있다.
"crates/agent 가 어떤 workload 도 실행하지 않는다"
    → 낡았다(2026-09-06 감사). crates/agent/src/exec.rs 가 실행한다.
"WorkloadHint 에 SLO 필드가 없다"
    → 여전히 참. proto/common.proto:223-237. 이것이 병목이다.
```

세 번째만 남았고, 그것이 이 계획의 S0·S2 다.

## 1. 무엇을 바꾸는가 — 한 문장

**inference class Job 에 한해**, hard-filter 의 "모델이 이 노드 VRAM 에 들어가는가"
이진 판정 옆에 **"이 노드가 이 크기의 모델을 이 SLO 로 서빙할 수 있다고 실측된
기록이 있는가"** 판정을 하나 더 두고, 그 기록이 있으면 VRAM 보다 큰 모델도 통과시킨다.

## 2. FreeToken 과의 관계 — 무엇을 가져오고 무엇을 안 가져오는가

FreeToken(arXiv:2608.16157)은 **노드 하나 안에서** GPU VRAM·CPU RAM·PCIe 대역폭을
하나의 elastic 자원 풀로 다뤄 MoE 모델을 서빙하는 런타임이다. gPUteer scheduler 는
**노드들 사이에서** Job 을 어느 노드에 놓을지 정한다. 계층이 다르다.

```text
가져온다     "VRAM 이진 판정은 elastic 노드를 부당하게 거부한다" 는 문제 인식
가져온다     판정 기준을 "들어가는가" 에서 "SLO 를 만족하는가" 로 바꾸는 방향

안 가져온다  논문의 수치. 하드웨어 등급별로 서로 다른 모델을 서빙한 결과라
             여기 대입할 수 없다(V-12 검수 1라운드 결함 (2))
안 가져온다  논문의 런타임 자체. 이 저장소는 런타임을 구현하지 않는다.
             S1 에서 고르는 런타임은 "설치해서 실측할 수 있는 것" 이면 된다
안 가져온다  처리량 회귀 모델. v1 은 추정하지 않는다(§4 참조)
```

★ **gPUteer 가 elastic 런타임을 실행하는 것은 이 계획 밖이다.** Agent 가 모델
서버를 띄우는 것은 별개의 Runtime 스트림 조각이다. 이 계획은 **"어느 노드에
놓을지"** 만 다룬다.

## 3. 범위

### In

- **S0** `docs/contracts/proposals/` 에 `WorkloadHint` 변경 제안 (Protocol 스트림 · 통합 책임자 승인)
- **S1** x600 에서 elastic 런타임 1종 설치 + calibration 1회 실측 → `docs/evidence/ENV-03_*`
- **S2** `proto/common.proto` `WorkloadHint` 에 inference SLO 필드 4개 추가 · `SCHEMA_VERSION` 2→3 · 테스트 벡터 재생성 · `SCHEMA_TOO_NEW` 테스트
- **S3** scheduler 순수 커널: `ElasticCalibration` 모델 · calibration **지배(dominance) 판정** · `evaluate_gpus()` 분기 · 새 `MissingFact`/`RejectionReason`
- **S4** inventory 쪽: `AgentInventory`/bootstrap 문서(`import-inventory`) schema v2 에 calibration 기록 반입 · `inventory_store` 영속화 · `manifest_requirements.rs` 가 SLO 필드를 `JobRequirements` 로 옮김
- **S5** negative test (QA) · evidence `DoD-NN` · 문서 정리(V-12 → 착수 상태, `_열린_작업.md`, `CLAUDE.md` §5, `_주장_검사.md`)

### Out — 명시하지 않으면 범위가 샌다

| 하지 않는 것 | 이유 |
|---|---|
| **처리량 추정·보간·회귀 모델** | 실측 점 하나로 곡선을 만들면 지어내는 것이다(`CLAUDE.md` §1). v1 은 **실측 점이 요구를 지배하는지만** 본다(§4). 보간은 실측 점이 여러 개 쌓인 뒤 별도 vision 으로 |
| `rank_best_fit` / soft-rank 변경 | V-10 이 hard-filter/soft-rank 분리를 **아직 결정하지 않았다.** 이 계획은 hard-filter 안에서만 움직이고 rank 는 건드리지 않는다. §4.4 가 그래도 rank 가 깨지지 않는 이유를 설명한다 |
| §10.5 Shared Admission 공식 수정 | 조건부 opt-in 경로이고 `SHARED` allocation 전용. 이 계획은 `EXCLUSIVE` 기본값 위에서 동작한다 |
| training·preprocessing 등 다른 class | 이진 판정 그대로. 분기는 `WorkloadClass::Inference` 에서만 열린다 |
| Agent 가 calibration 을 **스스로** 측정·서명해 보고 | 운영자 반입(`import-inventory` 와 같은 신뢰 경계)부터. 자동 측정은 `RecordWorkloadProfile`(`control.proto:440`) 이 이미 그 모양을 갖고 있으니 그때 그 메시지로 |
| PCIe 대역폭을 inventory 의 **독립 필드**로 측정·비교 | v1 의 calibration 은 **그 노드·그 GPU 에서** 잰 것이라 PCIe 가 결과에 이미 들어 있다. calibration 기록에 링크 세대·폭을 **출처로 보존**만 하고, 현재 링크와 대조하는 것은 `runtime-nvml` 에 PCIe 조회가 없어(2026-09-07 grep 0건) 미룬다 |
| Agent 가 elastic 런타임 프로세스를 실행 | Runtime 스트림 별개 조각 |
| scheduler 데몬 루프 | `_열린_작업.md` §A1 5번. 이 계획과 무관하게 진행 |

## 4. 설계 — 판정 규칙

### 4.1 원칙: 추정하지 않는다. 실측이 요구를 지배하면 통과다

```text
calibration 기록 C 가 요구 R 을 지배한다  ⇔  아래 전부 참

  C.node_id == 후보.node_id                          같은 노드에서 잰 것
  C.gpu_id  ∈ 후보.gpus (healthy == Some(true))      그 GPU 가 지금 있고 건강함
  C.model_weight_bytes            >= R.model_weight_bytes
  C.measured_decode_tps_milli     >= R.target_decode_tps_milli
  C.measured_first_token_ms       <= R.target_first_token_ms
  C.resident_vram_bytes_used      <= 후보.gpu.available_vram_bytes
  C.host_ram_bytes_used           <= 후보.available_ram_bytes      ← 이미 있는 필드
  C.runtime_id / runtime_version  == 운영자가 노드에 선언한 현재 런타임
```

더 큰 모델을 더 빠르게 서빙한 실측이 있으면, 더 작은 모델을 더 느린 SLO 로
요구하는 Job 은 통과한다. **그 반대 방향(더 큰 모델·더 빠른 SLO)은 실측이 없으니
거부한다.** 곡선을 그리지 않는다.

### 4.2 `evaluate_gpus()` 분기 — 기존 경로는 바이트 하나도 안 바뀐다

```text
기존 규칙(전 class · 전 노드 · 그대로)
  available_vram_bytes >= minimum_vram_bytes_per_gpu       filter.rs:215-229

추가 관문 — 다음 셋이 전부 참일 때만 열린다
  job.workload_class == Some(Inference)
  job.model_weight_bytes == Some(w), w > 0
  job.inference_slo == Some(slo)

  ├ 후보 GPU 중 available_vram_bytes >= w 인 것이 required_count 개 이상
  │     → 통과 (모델이 상주한다 — 오늘과 같은 세계)
  ├ 아니면, 후보에 R 을 지배하는 calibration 이 required_count 개 GPU 에 대해 있음
  │     → 통과 (elastic)
  ├ 아니면, 후보가 calibration 을 하나도 선언하지 않음
  │     → 거부  RejectionReason::ModelWeightsExceedVram { .. }
  └ 아니면 (calibration 은 있는데 지배하지 못함)
        → 거부  RejectionReason::InferenceSloNotCalibrated { closest: .. }
```

★ **새 필드가 없는 Job 은 새 관문에 들어오지 않는다.** 그래서 지금 있는 스케줄러
테스트 전부가 그대로 통과해야 한다 — 이것이 S3 의 첫 완료 기준이다.

★ **`minimum_vram_bytes_per_gpu` 의 의미가 바뀐다 — 계약 문서에 적는다.**
지금까지 제출자는 이 값에 "모델 크기" 를 넣었다. 새 필드가 생기면 이 값은
**상주 하한**(KV cache · 활성 expert · 런타임 버퍼)이고, 모델 전체 크기는
`model_weight_bytes` 다. 옛 방식으로 둘 다 모델 크기로 적으면 elastic 경로가
절대 안 열린다 — 잘못 동작하는 게 아니라 **오늘과 똑같이** 동작한다. 안전한 쪽으로 낡는다.

### 4.3 모르면 거부한다 — `MissingFact` 확장

```text
JobModelWeightBytes         SLO 는 있는데 모델 크기가 없다
JobInferenceSlo             모델 크기는 있는데 SLO 가 없다   (둘은 같이 온다)
ElasticRuntime { node_id }  calibration 은 있는데 노드가 현재 런타임을 선언 안 함
CalibrationGpu { gpu_id }   calibration 이 가리키는 GPU 가 inventory 에 없다
```

`import-inventory` 와 같은 규율이다 — "verified" 라는 말을 쓰지 않는다. calibration
은 **운영자가 반입한 선언**이고, 이 저장소가 증명하는 것은 "그 선언이 스키마에
맞고 inventory 와 정합하며 원자적으로 저장됐다" 까지다.

### 4.4 rank 가 깨지지 않는 이유

`rank_best_fit` 은 `minimum_vram_bytes_per_gpu` 로 VRAM 축을 잰다(`rank.rs:201-209`).
elastic 으로 통과한 후보도 기존 규칙(4.2 첫 줄)은 만족하므로 `available >= required.vram_per_gpu`
가 참이다. rank 에 `EligibleCandidateMismatch` 가 나지 않는다. **rank 는 모델 전체
크기를 모르고, 몰라도 된다** — 그것은 V-10 이 결정할 soft 축 문제다.

### 4.5 정수만 쓴다 (`CLAUDE.md` §1 · `RULE.md` §9.1)

```text
처리량   decode tokens per second × 1000   → uint64  *_tps_milli
지연     first-token latency 밀리초         → uint32  *_first_token_ms
크기     바이트                             → uint64
```

`tok/s` 를 float 로 두지 않는다. 1000 배 정수로 소수 셋째 자리까지 담는다.

## 5. 계약 변경 — S0 · S2

### 5.1 `WorkloadHint` 추가 필드 (초안 — S0 제안서에서 확정)

```protobuf
message WorkloadHint {
  // ... 기존 1~4, 10~12 그대로 ...

  // ── inference SLO (V-12). 0 = 선언 안 함 → 기존 이진 판정만 적용 ──
  // 모델 가중치 전체 크기(양자화 반영). est_peak_vram_bytes 와 다르다 —
  // 그것은 "상주 피크", 이것은 "전체". elastic 노드에서 둘이 갈라진다.
  uint64 model_weight_bytes             = 13;
  // 목표 decode 처리량 × 1000. float 금지 규칙.
  uint64 target_decode_tps_milli        = 14;
  // 목표 first-token 지연 상한(ms)
  uint32 target_first_token_ms          = 15;
  // 가중치를 GPU VRAM 밖(host RAM)에 두는 것을 제출자가 허용하는가.
  // false 면 model_weight_bytes 가 있어도 elastic 경로에 들어가지 않는다.
  bool   host_offload_allowed           = 16;
}
```

★ field number 13~16 은 `WorkloadHint` 에서 미사용이다(2026-09-07 `proto/common.proto:223-237` 확인). `reserved` 없음.

### 5.2 호환성 — 제안서가 답해야 하는 넷

| 항목 | 답 |
|---|---|
| `schema_version` 증가 | **예.** `JobManifest` 는 서명 대상이고 `workload = 21` 로 이 메시지를 품는다(`proto/job.proto:59`). `crates/protocol/src/constants.rs:61` `SCHEMA_VERSION` 2→3 |
| 기존 서명 무효화 | **아니오** — 기존 v2 Manifest 는 v2 로 계속 검증된다. 새 필드를 쓰려면 v3 로 서명해야 한다 |
| 멀티 바이너리 스큐 | **예.** v2 검증자는 v3 Manifest 에 `SCHEMA_TOO_NEW` 를 돌려준다(`RULE.md` §9.3, N-2 minor 창). 이 테스트가 S2 완료 기준이다 |
| 테스트 벡터 재생성 | **예.** `tests/vectors/canonical_v1.json` 52건 → 재생성 + `reference_canonical.py --verify` 교차. **개발 기계에서만**(x600 WSL python 에는 필요한 모듈이 없다 — `docs/manuals/작업_환경.md`) |

★ **이것이 V-12 가 지목한 병목이다.** 제안서 승인 없이 S2 를 시작하지 않는다
(`RULE.md` §3.5 순서 · §4.2 공용 파일).

### 5.3 inventory 쪽 — proto 아님

`AgentInventory`·bootstrap 문서는 proto 가 아니라 coordinator 의 Rust 구조체 + JSON 이다
(`import_inventory.rs` 모듈 주석). 그래서 calibration 기록 추가는 **`SUPPORTED_SCHEMA_VERSION`
1→2** 와 `inventory_store` 테이블 추가로 끝나고, proto·서명·벡터에 영향이 없다.

```text
AgentInventory (추가)
  elastic_runtime:   Option<ElasticRuntimeDeclaration { runtime_id, runtime_version }>
  calibrations:      Vec<ElasticCalibration>

ElasticCalibration
  gpu_id                         String     inventory 의 GPU 와 같은 식별자
  runtime_id · runtime_version   String
  model_weight_bytes             u64
  quantization_label             String     출처 표기용. 판정에 안 쓴다
  resident_vram_bytes_used       u64
  host_ram_bytes_used            u64
  measured_decode_tps_milli      u64
  measured_first_token_ms        u32
  prompt_set_digest              [u8; 32]   BLAKE3. 같은 프롬프트로 잰 것인지
  measured_at_unix_ms            u64
  pcie_link_gen · pcie_link_width Option<u32>  출처 보존. v1 판정에 안 쓴다
  raw_evidence_path              String     docs/evidence/_raw/ 상대 경로
```

`inventory_revision` 규칙은 그대로다 — calibration 이 바뀌면 revision 이 올라가고,
staging CAS 가 낡은 revision 으로 예약하는 것을 막는다(`model.rs` 주석).

## 6. S1 — calibration 실측 절차

### 6.1 어디서

**x600**(RTX 4070 SUPER 12GB · host RAM 23.1GB · `ENV-02`). 이유: 실물 GPU 가 있고,
VRAM(12GB)보다 큰 모델을 host RAM 으로 넘치게 만들 수 있는 **유일한 조합**이다.
remote5090(RTX 5090 32GB) 는 sudo 가 없어 런타임 설치가 막힐 수 있다.

★★ **x600 규칙을 지킨다**(`CLAUDE.md` · `docs/manuals/작업_환경.md`).

```text
작업은 E:\gputeer-work 아래에서만.  C: 에 임시 파일도 쓰지 않는다
WSL · Docker 작업은 사용자가 명령하기 전까지 하지 않는다
ssh x600 "bash ..." 는 WSL bash 를 부른다 — Windows 네이티브로 할 것
오래 도는 것은 x600 에서 (2026-09-07 사용자 지시)
```

런타임이 Windows 네이티브로 안 돌면 WSL 이 필요하고, 그건 **사용자가 명령해야
시작**한다. S1 착수 전에 이 지점을 사용자에게 묻는다.

### 6.2 런타임 선택 — 계획서가 정하지 않는다

선택 기준만 적는다. 실제 선택은 S1 첫 세션이 조사해 리포트에 남긴다.

```text
필수   가중치 일부를 host RAM 에 두고 GPU 와 오가며 추론할 수 있다
       (layer 단위 partial offload 든 MoE expert 단위든 무관)
필수   Windows 네이티브 또는 WSL2 에서 설치·실행이 된다
필수   decode tok/s 와 first-token 지연을 프로그램이 출력한다(사람이 눈으로 재지 않는다)
선호   MoE 모델을 expert 단위로 오프로드한다 (FreeToken 이 다루는 부류)
```

★ **FreeToken 구현체가 공개돼 있는지는 이 문서가 주장하지 않는다.** 조사해서
있으면 후보, 없으면 위 기준을 만족하는 다른 런타임으로 간다. 기준을 만족하면
어느 것이든 트리거 (b)를 충족한다.

### 6.3 무엇을 재는가

```text
고정 입력   모델 1개(가중치 > 12GB, 양자화 명시) · 프롬프트 세트 1개(BLAKE3 기록)
측정값      decode tok/s (×1000 정수) · first-token ms · 상주 VRAM 피크 · host RAM 피크
조건        런타임 id/version · 드라이버 595.79 · PCIe link gen/width (nvidia-smi -q)
반복        같은 조건 3회 이상. 최솟값을 기록한다 (최댓값·평균이 아니다 — SLO 는 하한 약속이다)
원문        docs/evidence/_raw/elastic_calibration_x600_<날짜>.txt  전부 그대로
```

★ **수치는 조건과 함께**(`RULE.md` §1.3). "40 tok/s" 가 아니라 "모델 X Q4 · 프롬프트
세트 다이제스트 Y · 3회 최솟값 · 런타임 Z vN" 이다.

### 6.4 산출물

- `docs/evidence/ENV-03_elastic_추론_calibration_x600.md` — 환경 실측 형식(`RULE.md` §5.1)
- 같은 값을 §5.3 `ElasticCalibration` JSON 으로도 남긴다 — S4 의 `import-inventory` 첫 입력이 된다
- `TODO_VISION.md` V-12 트리거 (b) **충족** 표기

## 7. 단계

| # | 단계 | 스트림 | 완료 기준 | 상태 |
|---|---|---|---|---|
| S0 | `WorkloadHint` 변경 제안서 `docs/contracts/proposals/2026-09-DD_HHmm_WorkloadHint_inference_SLO.md` | Protocol(제안) · 통합 책임자(승인) | 제안서가 `02_변경_제안_절차.md` 양식의 호환성 4문항에 답하고 **승인** 란이 채워짐. ★ 승인은 사용자 결정 | ⬜ |
| S1 | x600 calibration 실측 1회 | Runtime · QA | `ENV-03_*` evidence + `_raw` 원문 + `ElasticCalibration` JSON. 3회 최솟값. V-12 트리거 (b) 충족 | ⬜ |
| S2 | proto 필드 4개 · `SCHEMA_VERSION` 3 · 벡터 재생성 · `SCHEMA_TOO_NEW` 테스트 | Protocol | `cargo build`(protoc 검사) 통과 · `reference_canonical.py --verify` 52건+ 통과 · v2 검증자가 v3 Manifest 를 `SCHEMA_TOO_NEW` 로 거부하는 테스트 · `SCHEMA_FINGERPRINT.txt` 갱신 | ⬜ |
| S3 | scheduler 순수 커널 | Coordinator(scheduler) | ① **기존 scheduler 테스트 전부 무변경 통과**(새 필드 없는 Job 은 새 관문에 안 들어옴) ② §4.1 지배 판정 8조건 각각의 통과/실패 단위 테스트 ③ §4.2 분기 4갈래 통합 테스트 ④ `MissingFact` 4종 각각 ⑤ 시계·I/O 없음 | ⬜ |
| S4 | inventory 반입 + Manifest projection | Coordinator · CLI | `import-inventory` schema v2 가 calibration 을 원자 반입 · `inventory_store` 테이블 + 재시작 후 복원 · `manifest_requirements.rs` 가 필드 4개를 옮기고 `host_offload_allowed=false` 면 SLO 를 `None` 으로 둠 · `plan-job` → `scheduler-tick` 으로 elastic 후보가 실제 선택되는 selftest 1건 | ⬜ |
| S5 | negative test · evidence · 문서 | QA · 문서 | §8 negative 전부 · `DoD-NN` evidence PASS(schema v2, 독립 검수) · V-12 → 착수/완료 갱신 · `_열린_작업.md` A1-15 에서 V-12 분리 · `CLAUDE.md` §5 · `_주장_검사.md` | ⬜ |

★ **S0 와 S1 은 병렬이다.** 둘 다 코드가 아니고 서로 의존하지 않는다.
★ **S2 는 S0 승인 뒤, S3 는 S1 실측 뒤**다. 실측 없이 `ElasticCalibration` 필드를
정하면 재지 않은 것을 필드로 만들게 된다.
★ **S3·S4 는 `_열린_작업.md` §A0 검수 대기열이 비운 뒤**에 착수한다 — 실행 사슬
위에 쌓는 일은 아니지만, 같은 크레이트(`coordinator`)를 건드린다.

## 8. negative test (`RULE.md` §6 — 없으면 미완료)

```text
Protocol
  v2 검증자 + v3 Manifest                → SCHEMA_TOO_NEW, 필드 미사용
  v3 Manifest 에서 필드 13~16 위조        → 서명 불일치로 거부 (canonical 에 포함됨을 증명)
  target_decode_tps_milli 만 있고 model_weight_bytes = 0
                                         → 변환기가 JobInferenceSlo/JobModelWeightBytes 쌍 불일치로 거부

Scheduler 커널
  calibration 이 지배 조건 8개 중 정확히 하나만 미달 (8케이스)   → 거부, 사유에 그 조건
  calibration 은 있는데 gpu_id 가 inventory 에 없음              → MissingFact::CalibrationGpu
  calibration 의 GPU 가 healthy == Some(false)                    → 거부 (건강 판정이 먼저)
  host_offload_allowed = false + 모델 > VRAM                      → ModelWeightsExceedVram (elastic 경로 안 열림)
  training class + 같은 필드                                      → 새 관문 미진입, 기존 이진 판정 결과와 동일
  두 노드: 하나는 상주 가능, 하나는 elastic 지배                  → 둘 다 eligible, RankingRequired (rank 무변경)
  ★ 뮤테이션: 지배 조건 부등호 하나를 뒤집었을 때 깨지는 테스트가 있는가 — 8개 전부

Inventory
  calibration 의 runtime_version ≠ 노드 선언 runtime_version      → 반입 거부, DB 바이트 무변경
  같은 (node, gpu, prompt_digest, runtime) 중복                    → 거부 (어느 쪽이 정본인지 문서가 안 말한다)
  schema_version 1 문서에 calibrations 필드                       → 거부
  반입 후 재시작                                                   → 같은 calibration 복원, inventory_revision 유지
```

★ **거부 뒤 DB 무변경을 함께 본다**(`2026-09-01_2000` 계획서 규율).

## 9. 기준선과 다른 점

| 항목 | 기준선 | 이 계획 | 사유 |
|---|---|---|---|
| `WorkloadHint` 필드 | §15.3 은 1~4, 10~12 만 정의 | 13~16 추가 | §15.3 은 "VRAM admission 이 `est_peak_vram_bytes` 를 요구한다" 고 적었지 elastic 노드를 다루지 않았다. 기준선 자체는 **수정하지 않는다** — S0 제안서에 사유를 적고 여기 남긴다(`RULE.md` "상위 기준선과의 관계") |
| admission 의 VRAM 판정 | §13.2 · §10.5 는 스칼라 비교 | inference + SLO 선언 시 calibration 지배 판정 추가 | V-12 등록 사유 그대로 |

기준선 §10.5 Shared Admission 공식은 건드리지 않는다.

## 10. 정직하게 남기는 한계

```text
곡선이 아니다          실측 점 하나가 지배하는 요구만 통과한다. "8GB 노드가 35B 를
                      돌릴 수 있다" 같은 일반 명제를 이 코드는 주장하지 않는다
calibration 은 선언    운영자 반입이다. 노드가 거짓 calibration 을 넣으면 스케줄러는 믿는다.
                      import-inventory 와 같은 신뢰 경계이고, 그 한계도 같다
PCIe 는 출처만         링크가 바뀐 것(예: x16→x4)을 감지하지 못한다. 운영자가 재측정해야 한다
런타임 실행 없음       "놓을 수 있다" 까지다. 실제로 그 노드가 elastic 런타임을 띄우는 것은 별개
SLO 는 두 축만         decode tps · first-token ms. 컨텍스트 길이·배치 크기는 프롬프트 세트에
                      묶여 있고 필드가 아니다 — 같은 다이제스트 안에서만 비교가 성립한다
x600 하나              환경 매트릭스(RULE §7.2)가 1칸이다. Linux 는 remote5090 의 sudo 문제로 미정
```

## 11. 착수 전에 사용자에게 물어야 하는 것

1. **S0 승인** — `SCHEMA_VERSION` 2→3 은 벡터 52건 재생성과 스큐 창을 건드린다. 통합 책임자 결정.
2. **S1 의 WSL 사용 여부** — 고른 런타임이 Windows 네이티브로 안 돌면 x600 WSL(E:) 이 필요하다. 사용자 명령 없이 시작하지 않는다.
3. **calibration 대상 모델** — 12GB 를 넘는 공개 가중치 중 무엇을 쓸지. 라이선스와 다운로드 크기가 걸린다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-09-07 | 최초 작성. 사용자 결정으로 V-12 를 vision 에서 실행계획으로 올림. 독립 검수 전 초안 |
