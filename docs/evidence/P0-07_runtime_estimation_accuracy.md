---
id: P0-07
claim: "기준선 §12.3 Stage-2 의 50-step calibration 이 전체 실행시간을 상대오차 σ <= 0.20 으로 예측한다"
status: PASS
commit: 5dba36e62c230b3ec894cb8ca16623da2aec2c2a
binary_digests:
  probe_path: "tools/probes/p0_07_runtime_estimation.py"
  torch: "2.13.0+cu126"
protocol_versions:
  none: "해당 없음 - 성능 측정 스파이크"
platform: "원격 x600 (<x600-hostname>) — Microsoft Windows 11 Pro build 26200"
hardware: "NVIDIA GeForce RTX 4070 SUPER 12282MiB / driver 595.79 / CUDA 13.2 / compute_cap 8.9 / AMD Ryzen 5 8600G / RAM 23.1GB"
network_profile: "SSH 경유 원격 실행. 측정 자체는 로컬 GPU 연산만 사용"
command: |
  scp tools/probes/p0_07_runtime_estimation.py x600:p0_07_probe.py
  ssh x600 'python p0_07_probe.py'                          # RUN 1 기본
  ssh x600 'python p0_07_probe.py --scaling'                # RUN 2 warmup=10
  ssh x600 'python p0_07_probe.py --scaling --warmup 300'   # RUN 3 warmup=300
raw_output: |
  ##### RUN 1 — 기본 (warmup=10, total=400, trials=3, 워크로드 5종) #####
  [transformer_train] 평균 |오차| = 0.4%  최대 = 1.0%
  [cnn_train]         평균 |오차| = 2.5%  최대 = 3.9%
  [inference]         평균 |오차| = 0.3%  최대 = 0.6%
  [matmul]            평균 |오차| = 3.4%  최대 = 5.1%
  [memory_bound]      평균 |오차| = 4.2%  최대 = 4.8%
  종합: 15 시행 · 평균 2.2% · 최대 5.1% · σ = 0.0208
  DoD (σ <= 0.20): PASS

  ##### RUN 2 — 지속시간 스케일링, warmup=10 #####
  [transformer_train] calibration median = 8.333 ms
       400 step  예측    3.33s | 실제    3.08s | 오차  -7.7% | drift  -8.4%
      2000 step  예측   16.66s | 실제   15.24s | 오차  -8.5% | drift  -9.3%
     10000 step  예측   83.33s | 실제   75.85s | 오차  -9.0% | drift -10.0%
     40000 step  예측  333.30s | 실제  302.34s | 오차  -9.3% | drift -10.2%
  [matmul] calibration median = 0.709 ms
       400 step  오차 +5.1% | drift +5.0%
      2000 step  오차 +5.9% | drift +5.7%
     10000 step  오차 +6.4% | drift +6.4%
     40000 step  오차 +6.8% | drift +6.8%
  종합: 최대 |오차| 9.3% · 최대 |drift| 10.2%

  ##### RUN 3 — 지속시간 스케일링, warmup=300 #####
  [transformer_train] calibration median = 7.571 ms
       400 step  오차 +0.2% | drift -0.2%
      2000 step  오차 +0.9% | drift +0.1%
     10000 step  오차 +0.4% | drift -0.5%
     40000 step  오차 +0.4% | drift -0.4%
  [matmul] calibration median = 0.750 ms
       400 step  오차 +0.0% | drift -0.1%
      2000 step  오차 +0.4% | drift +0.1%
     10000 step  오차 +0.8% | drift +0.7%
     40000 step  오차 +1.2% | drift +0.9%
  종합: 최대 |오차| 1.2% · 최대 |drift| 0.9%
artifacts:
  - docs/evidence/_raw/P0-07_probe.txt
  - tools/probes/p0_07_runtime_estimation.py
negative_tests:
  - "지속시간 스케일링(400 -> 40,000 step)을 측정해 '짧은 구간에서 정확하다'가 '긴 구간을 보장한다'가 아님을 확인. RUN 1(총 3초)의 σ=0.021 이 RUN 2(총 5분)에서 9.3% 오차로 악화되는 것을 관측"
  - "step drift 를 오차와 분리 측정해 오차의 원인이 '드리프트 누적'이 아니라 'calibration 구간 편향'임을 특정 (drift -10.2% vs error -9.3% 로 거의 일치)"
  - "warmup 을 10 -> 300 으로 바꾼 대조 실험으로 편향 가설을 검증. 오차가 9.3% -> 1.2% 로 8배 감소하여 가설이 확인됨"
  - "특성이 다른 워크로드 5종(transformer/cnn/inference/matmul/memory-bound)을 사용해 단일 워크로드 우연을 배제"
  - "transformer 는 시간이 지날수록 빨라지고(-10%) matmul 은 느려지는(+7%) 반대 방향 드리프트를 관측 — 단일 보정계수로 처리할 수 없음을 확인"
limitations:
  - "★ 단일 GPU 에서만 측정했다. 기준선 §12.3 의 핵심인 '노드 간 외삽 오차'(가정 0.25)는 전혀 검증하지 못했다. CUDA GPU 가 x600 한 대뿐이다"
  - "★ 유휴 기계에서 측정했다. 소유자가 GPU 를 동시에 쓰는 상황(§11 owner presence)의 오차는 미측정이다. 이것이 실제 운용의 지배적 변수일 수 있다"
  - "최장 측정이 약 5분(40,000 step)이다. 실제 Job 은 수 시간 돌며, 그 구간의 thermal throttling 은 미검증이다"
  - "데이터 로딩을 포함하지 않았다. 모든 워크로드가 GPU 상주 텐서를 재사용하므로 I/O 변동이 제거되어 있다. 실제 학습은 dataloader 변동이 크다"
  - "배치 크기·모델 크기가 calibration 과 본 실행에서 동일하다. 실제로는 다를 수 있다"
  - "단일 드라이버(595.79)·단일 torch(2.13.0+cu126) 조합이다"
  - "RUN 3 은 툴 타임아웃으로 파이프 캡처가 잘려, 동일 명령을 별도 실행해 얻은 출력을 raw 에 옮겨 적었다. 재현 명령은 command 필드에 있다"
decision: "P0-07 을 PASS 로 판정한다(σ=0.021, DoD 0.20 대비 충분). ADR-007 을 유지하고 §13.4 chance-constrained selection 을 재설계하지 않는다. 단 §12.3 의 calibration 절차에 'warmup >= 300 step' 을 추가할 것을 요청한다 — warmup 10 에서는 최대 오차 9.3%, 300 에서는 1.2% 로 8배 차이가 난다. 노드 간 외삽 오차는 GPU 2종 이상 확보 시 P0-07b 로 별도 측정한다"
---

# P0-07 · Runtime Estimation 정확도

## 무엇을 입증하려 했는가

기준선 §13.4 의 chance-constrained selection **전체가 σ 값 위에 서 있다.**

```text
P(성공) = Φ( ln(deadline / T_est) / σ_ln ) × A × survival^(T/1h)
```

§12.3 은 Stage-2(50-step calibration)의 상대오차를 **σ/μ = 0.15** 로 가정했다.
이 가정이 틀리면 §43.6 에 따라 **ADR-007 을 수정하고 deadline 을 best-effort 로
재정의하며 §13.4 를 전면 재설계**해야 한다.

DoD 는 **σ <= 0.20** 이다.

## 어떻게 측정했는가

세 번의 실행으로 좁혀 들어갔다.

```text
RUN 1  워크로드 5종 × 3회 × 400 step        기본 정확도
RUN 2  지속시간 400 -> 40,000 step 스케일링   "짧은 구간 정확도"가 긴 구간을 보장하는가
RUN 3  RUN 2 를 warmup 300 으로 반복          원인 가설 검증
```

**RUN 2 가 핵심이다.** 실제 Job 은 수 시간 돌지만 calibration 은 50 step 이다.
짧은 구간에서 정확하다는 사실이 긴 구간을 보장하지 않으므로 반드시 확인해야 했다.

워크로드는 특성이 다르게 5종을 골랐다 — transformer(attention) · CNN(conv) ·
inference(no-grad) · matmul(순수 GEMM) · memory-bound(대역폭). 단일 워크로드의 우연을 배제한다.

## 결과

### RUN 1 — 기본 정확도는 매우 좋다

| 워크로드 | 평균 \|오차\| | 최대 |
|---|---|---|
| transformer_train | 0.4% | 1.0% |
| cnn_train | 2.5% | 3.9% |
| inference | 0.3% | 0.6% |
| matmul | 3.4% | 5.1% |
| memory_bound | 4.2% | 4.8% |

**σ = 0.0208.** 기준선 가정(0.15)보다 7배 좋고 DoD(0.20)를 크게 통과한다.

### RUN 2 — 그러나 긴 구간에서는 나빠진다

```text
transformer_train (warmup=10)
     400 step   오차  -7.7%
    2000 step   오차  -8.5%
   10000 step   오차  -9.0%
   40000 step   오차  -9.3%   (약 5분)
```

**RUN 1 의 σ=0.021 을 그대로 믿으면 안 된다.** RUN 1 은 총 3초짜리 실행이라
calibration 구간과 측정 구간이 거의 같은 상태였다.

### 발견 — 원인은 드리프트 누적이 아니라 calibration 편향

오차와 step drift 를 분리 측정한 것이 결정적이었다.

```text
transformer   drift -10.2%  vs  error -9.3%
matmul        drift  +6.8%  vs  error +6.8%
```

**거의 정확히 일치한다.** 즉 시간이 갈수록 오차가 *누적*되는 것이 아니라,
**calibration 구간의 step time 자체가 정상 상태와 다른 것**이다.
오차는 400 step 에서 이미 거의 최종값이고 이후 완만히 수렴한다(-7.7% → -9.3%).

warmup 10 step 후의 50 step 이 **아직 정상 상태가 아니었다.**

### RUN 3 — 가설 검증: warmup 을 늘리면 사라진다

| warmup | 최대 \|오차\| | 최대 \|drift\| |
|---|---|---|
| 10 | **9.3%** | 10.2% |
| **300** | **1.2%** | **0.9%** |

**8배 개선.** 40,000 step(5분) 구간에서도 오차가 1.2% 에 머문다.
가설이 확인되었고, 이것이 이 스파이크의 가장 실용적인 산출물이다.

### 부수 관찰 — 드리프트 방향이 워크로드마다 반대다

```text
transformer   시간이 갈수록 빨라진다 (-10%)   cuDNN 오토튜닝·커널 캐시 정착으로 추정
matmul        시간이 갈수록 느려진다 (+7%)    클럭/열 영향으로 추정
```

**단일 보정계수로 처리할 수 없다.** warmup 을 늘려 정상 상태에서 재는 것이 옳다.
(각각의 기전은 확인하지 않았다 — 추정이다.)

## 이 실험이 증명하지 "않는" 것

- **★ 노드 간 외삽을 전혀 검증하지 못했다.** CUDA GPU 가 x600 한 대뿐이다.
  기준선 §12.3 은 노드 간 외삽 오차를 0.25 로 가정하는데 **이것이 미검증으로 남는다.**
  §13.4 는 다른 노드의 완료시간을 예측해야 하므로 **이 공백이 가장 크다.**
- **★ 유휴 기계에서만 측정했다.** 소유자가 GPU 를 동시에 쓰는 상황(§11)의 오차는 모른다.
  실제 운용에서는 이것이 지배적 변수일 수 있다.
- **최장 5분**이다. 수 시간 구간의 thermal throttling 은 미검증이다.
- **데이터 로딩이 없다.** 모든 워크로드가 GPU 상주 텐서를 재사용해 I/O 변동이 제거되어 있다.
  실제 학습의 dataloader 변동은 이 측정에 포함되지 않았다.
- 배치·모델 크기가 calibration 과 본 실행에서 **동일**하다.
- 단일 드라이버·단일 torch 조합이다.

## 결정

1. **P0-07 을 `PASS` 로 판정한다.** σ=0.021 로 DoD(0.20)를 충족한다.
2. **ADR-007 을 유지한다.** §13.4 chance-constrained selection 을 재설계하지 않는다.
3. **§12.3 에 `warmup >= 300 step` 을 추가할 것을 요청한다.**
   현재 문서에는 warmup 규정이 없고, 10 step 으로 하면 최대 오차가 9.3% 로 커진다.
4. **§12.3 의 Stage-2 σ 가정(0.15)은 보수적으로 유지한다.**
   측정값 0.021 은 유휴·단일노드·무 I/O 조건이므로 그대로 쓰면 낙관적이다.
5. **노드 간 외삽 오차는 `P0-07b` 로 분리**한다. GPU 2종 이상 확보가 선행 조건이다.

관련: 기준선 §12.3 · §13.4 · ADR-007
계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md`
