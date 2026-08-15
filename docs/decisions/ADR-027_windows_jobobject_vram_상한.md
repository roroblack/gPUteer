# ADR-027 · Windows Job Object 는 VRAM 을 간접 상한한다

- **상태:** 제안 (기준선 §10.3 · §10.4 · §10.5 수정 요청)
- **날짜:** 2026-08-16
- **관련:** 기준선 §10.3 · ADR-015 · `docs/evidence/P0-06_vram_enforcement.md`

## 배경

기준선 §10.3 은 ADR-015(GPU 할당 기본값 = Exclusive)의 **유일한 근거**로
다음 표를 제시한다.

| 수단 | §10.3 의 판정 |
|---|---|
| MIG | GeForce 미지원 |
| MPS memory limit | Linux 전용 |
| 컨테이너 memory limit | 시스템 RAM 만. **VRAM 무관** |
| **Windows Job Object** | 시스템 RAM 만. **VRAM 무관** |
| PyTorch fraction | 워크로드 협조 필요 |

P0-06 에서 **네 번째 행이 Windows 에서 틀렸음**을 실측으로 확인했다.

## 결정

**§10.3 을 정정한다.** Windows WDDM 에서 Job Object 프로세스 메모리 제한은
**총 커밋 상한을 통해 VRAM 할당을 간접적으로 상한한다.**

**단 ADR-015(Exclusive 기본값)는 유지한다.**

## 근거

### 실측

Job Object 프로세스 메모리 제한을 바꿔가며 VRAM 할당을 시도했다 (5×5 스윕).

```text
RAM=무제한   1024:OK  2048:OK  3072:OK  4096:OK  6144:OK
RAM=8192    1024:OK  2048:OK  3072:OK  4096:OK  6144:OK
RAM=6144    1024:OK  2048:OK  3072:OK  4096:OK  6144:FAIL
RAM=4096    1024:OK  2048:OK  3072:FAIL 4096:FAIL 6144:FAIL
RAM=3072    1024:OK  2048:FAIL 3072:FAIL 4096:FAIL 6144:FAIL
```

```text
VRAM 최대 할당량  ≈  Job Object RAM 제한  −  약 2000 MiB
```

대조 실험(Job Object 없이 동일 할당)에서 **8192MiB 까지 성공**했으므로,
실패 원인이 대형 할당 한계가 아니라 Job Object 임이 확정된다.

### 기전 (추론)

WDDM 에서 비디오 메모리는 가상화·페이지 가능하며 시스템 메모리 커밋으로 뒷받침된다.
따라서 프로세스 커밋 상한이 VRAM 할당까지 지배한다.
**드라이버 내부를 확인하지는 않았으므로 이것은 관측으로부터의 추론이다.**

### 그럼에도 ADR-015 를 유지하는 이유

| # | 이유 |
|---|---|
| 1 | **상한이 코스하다.** ±2GB 오차는 §10.5 admission 이 요구하는 정밀도에 못 미친다 |
| 2 | **Windows 전용이다.** v0.1 주 타깃인 Linux 컨테이너에는 적용되지 않는다 (전용 VRAM 모델, cgroup 은 VRAM 미관여) |
| 3 | **VRAM quota 가 아니라 총 커밋 상한이다.** 프로세스가 시스템 RAM 을 많이 쓰면 VRAM 몫이 줄어든다. "이 Job 에 정확히 8GB" 를 지정할 수 없다 |
| 4 | **다중 프로세스 동시 제한이 미검증이다.** Shared 모드의 실제 시나리오가 바로 그것인데 확인하지 않았다 |
| 5 | 오버헤드(~2000MiB)가 프레임워크·모델 의존적이다 |

## 수정안

### §10.3 표 (수정 후)

| 수단 | 소비자 GPU 가용성 | 판정 |
|---|---|---|
| MIG | 데이터센터 GPU 전용. GeForce 미지원 | ❌ |
| MPS memory limit | Volta+ 이나 **MPS 는 Linux 전용** | ❌ Windows / △ Linux |
| 컨테이너 memory limit (Linux cgroup) | 시스템 RAM 만. VRAM 무관 | ❌ |
| **Windows Job Object** | **총 커밋 상한을 통해 VRAM 을 간접 상한한다 (P0-06)** | **△ 코스한 강제 가능** |
| PyTorch `set_per_process_memory_fraction` | 워크로드가 되돌릴 수 있다 (실증됨) | △ 협조 전제 |

★ **Job Object 는 워크로드 협조를 요구하지 않는 유일한 수단이다.**
이 점에서 PyTorch fraction 과 성격이 다르다.

### §10.4 Shared 허용 조건 (추가)

```text
다음 중 하나를 만족할 때만 Shared 허용

A. 동일 소유자의 Job 간            OOM 피해가 자기 자신에게 한정
B. Linux + MPS memory limit 확인   실제 강제 가능
C. Windows + Job Object 커밋 상한  ← 신규 (ADR-027)
     단 다음 조건을 함께 만족해야 한다
       - 각 Job 에 (예약 VRAM + 프레임워크 오버헤드 + 안전마진) 을 커밋 상한으로 설정
       - 오버헤드는 워크로드 프로필에서 실측치를 쓴다 (기본 2048 MiB)
       - 다중 프로세스 동시 제한이 검증된 뒤에만 활성화 (P0-06b)
D. MIG 지원 GPU                    Partitioned 로 처리
```

### §10.5 `external_process_vram` 산출 (명시)

P0-06 에서 `nvidia-smi --query-compute-apps` 의 `used_memory` 가
**WDDM 모드 GeForce 에서 `[N/A]`** 임을 확인했다. 프로세스별 직접 조회가 불가능하다.

```text
external_process_vram = memory.used(전체)  −  gPUteer 가 아는 자신의 할당 합

주의: 이 값은 gPUteer 가 추적하지 못하는 자신의 오버헤드(CUDA 컨텍스트 등)를
      외부 사용량으로 잘못 계상할 수 있다. 보수적 방향이므로 admission 에는 안전하다.
```

## 대안과 기각 사유

| 대안 | 기각 사유 |
|---|---|
| ADR-015 를 철회하고 Shared 를 기본값으로 | 상한이 코스하고 Windows 전용이다. Linux(v0.1 주 타깃)에는 적용 불가 |
| Job Object 상한을 정밀 VRAM quota 로 사용 | 총 커밋 상한이라 시스템 RAM 사용량에 따라 흔들린다 |
| §10.3 을 그대로 두고 이 발견을 무시 | 문서가 사실과 다르면 나중에 같은 조사를 반복한다 |

## 결과

**가능해지는 것**

- Windows S1/S2 노드에서 **워크로드 협조 없이** VRAM 을 코스하게 상한할 수 있다
- 악의적·부주의한 Job 이 GPU 를 통째로 먹는 최악 시나리오를 완화할 수 있다

**포기하는 것**

- 정밀한 VRAM quota 는 여전히 불가능하다
- Linux 에는 대응 수단이 없다

**바꿔야 하는 것**

- 기준선 §10.3 표 · §10.4 허용 조건 · §10.5 산출식
- `crates/runtime-windows` 구현 시 Job Object 커밋 상한 설정 로직
- `tests/security/` 에 "커밋 상한을 넘는 VRAM 할당이 거부되는가" negative test

## 되돌리는 조건

- 다중 프로세스 동시 제한(P0-06b)에서 상한이 서로 간섭하는 것으로 확인되면 §10.4-C 를 철회한다
- TCC 모드나 향후 WDDM 변경으로 관계가 깨지면 재검토한다
- 오버헤드 편차가 2000MiB 를 크게 넘는 워크로드가 발견되면 마진을 재산정한다

## 미해결

- **정확한 상한 특정.** 스윕 간격 1024MiB 로는 "제한 − 약 2000MiB" 근사만 얻었다
- **다중 프로세스 동시 제한 미검증** → P0-06b
- **Linux 대응 수단 없음.** v0.1 에서 Shared 를 쓰려면 MPS 검증이 선행되어야 한다
- 기전이 추론이다. 드라이버 문서로 확인하지 않았다

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-16 | 최초 작성. P0-06 실측 기반 |
