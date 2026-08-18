---
schema_version: 2
id: P0-01
claim: "Windows Restricted Token(특권 전부 제거) 아래에서 CUDA 가 동작하고, Job Object 로 프로세스 트리를 종료하면 VRAM 이 반환된다 — 즉 S1(Restricted Native) 경로가 성립한다"
status: PASS
commit: 5dba36e62c230b3ec894cb8ca16623da2aec2c2a

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash — grep 기반 소스 일치성 확인, 하드웨어 재실측 아님)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-18T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "재실측 없이 provenance 를 정직하게 기록하는 접근 자체의 타당성 판단(RULE.md §7.3 해석) · probe 인용 소스 일치성 확인 · v2 필수 frontmatter 필드 보완 확인"
review_artifact: "docs/evidence/_raw/P0-01_review.txt"

raw_output_artifact: "docs/evidence/_raw/P0-01_v2_promotion_2026-08-18.txt"
raw_output_digest: "sha256:5464b5da3e631f4d7d07e6d70edb8665f3be73f88213c17b6a231ae51c5f8d9a"
raw_output_bytes: 2326

binary_digests:
  probe_script_sha256: "4AFC6DED273D76AE4158F386944353F6DAD9BA6BD63E27BCC404F7271D53CD32"
  probe_path: "tools/probes/p0_01_windows_s1_cuda.py"
protocol_versions:
  none: "해당 없음 - OS/런타임 계층 스파이크"
platform: "원격 x600 (<x600-hostname>) — Microsoft Windows 11 Pro build 26200 / NTFS"
hardware: "NVIDIA GeForce RTX 4070 SUPER 12282MiB / driver 595.79 / CUDA 13.2 / compute_cap 8.9 / AMD Ryzen 5 8600G / RAM 23.1GB"
network_profile: "SSH 경유 원격 실행 (x600 = 10.20.20.1). 실험 자체는 로컬 프로세스 생성만 사용"
command: |
  scp tools/probes/p0_01_windows_s1_cuda.py x600:p0_01_probe.py
  ssh x600 'python p0_01_probe.py'
raw_output: |
  P0-01 · Windows S1 Restricted Native + CUDA
  python: 3.14.2

  [A] 일반 토큰에서 CUDA 가 동작하는가 (대조군)
      cuda_available True / device_count 1
      device_name NVIDIA GeForce RTX 4070 SUPER
      matmul_ok True / vram_alloc_mb 9.12
      판정: PASS

  [B] Restricted Token 아래에서 CUDA 가 동작하는가 (P0-01 핵심)
      workdir=C:\Windows\Temp
        b0_alive    exit=0  ALIVE
        b1_import   exit=0  TORCH 2.13.0+cu126
        b2_avail    exit=0  CUDA_AVAIL True
        b3_compute  exit=0  cuda_available True | device_count 1 |
                            device_name NVIDIA GeForce RTX 4070 SUPER |
                            matmul_ok True | vram_alloc_mb 9.12
      판정: PASS

  [C] Job Object 종료 시 프로세스 트리와 VRAM 이 정리되는가
      AssignProcessToJobObject=True / HOLDING 도달=True
      VRAM used(MiB)  before=168  during=489  after=168
      판정: PASS

  [D] 비제한 프로세스의 호스트 민감 파일 접근 (기준선 측정)
      1/3 읽힘 — Chrome User Data\Local State
      판정: 정보

  probe 4개 · FAIL 0
artifacts:
  - docs/evidence/_raw/P0-01_probe.txt
  - tools/probes/p0_01_windows_s1_cuda.py
  - docs/evidence/_raw/P0-01_v2_promotion_2026-08-18.txt
  - docs/evidence/_raw/P0-01_review.txt
negative_tests:
  - "B 를 단계 사다리(b0 python 기동 -> b1 torch import -> b2 is_available -> b3 실제 연산)로 구성해, 어느 단계에서 깨지는지 특정할 수 있게 했다. 출력이 비었다는 사실만으로 'CUDA 실패'로 단정하는 것을 방지"
  - "C 에서 VRAM before/during/after 를 nvidia-smi 로 측정. during 이 before 보다 크지 않으면 INCONCLUSIVE 로 판정하도록 해, '증가를 관측하지 못한 채 반환 성공'을 통과로 세지 않게 했다"
  - "A 대조군을 먼저 두어 B 실패 시 '환경 문제'와 'Restricted Token 문제'를 구분할 수 있게 했다"
  - "1차 실행에서 OpenProcessToken 이 err=6 으로 실패 -> ctypes restype 미지정으로 인한 프로브 결함임을 확인하고 정정 (아래 '오판 정정' 참조)"
limitations:
  - "DISABLE_MAX_PRIVILEGE 만 적용했다. SidsToDisable(관리자 SID 비활성)·SidsToRestrict(제한 SID)는 적용하지 않았다. 기준선 §9.3 이 요구하는 'admin SID disabled' 는 미검증이다"
  - "전용 저권한 로컬 계정을 만들지 않았다. 현재 사용자(<x600-user>)의 토큰을 제한한 것이며, 별도 계정 + CreateProcessWithLogonW 경로는 미검증이다"
  - "NTFS ACL 로 호스트 파일 접근을 차단하는 것을 검증하지 않았다. D 는 '제한이 없으면 읽힌다'는 기준선 측정일 뿐이다"
  - "프로세스별 아웃바운드 방화벽 규칙을 검증하지 않았다. 기준선 §9.1 capability matrix 의 'network egress 제어' 항목은 여전히 미검증이다"
  - "자식/손자 프로세스가 제한을 상속하는지 확인하지 않았다"
  - "Windows Service(Session 0)에서의 동작을 검증하지 않았다. 실제 Agent 는 서비스로 동작할 예정이므로 이 경로는 별도 검증이 필요하다"
  - "단일 기계·단일 GPU·1회 측정이다. 재현성과 다른 드라이버 버전에서의 동작은 미검증이다"
  - "torch 2.13.0+cu126 한 조합만 확인했다. 다른 CUDA/torch 버전 조합은 미검증이다"
decision: "P0-01 을 PASS 로 판정한다. ADR-005(Windows virtualization OFF 지원)를 유지하고 Windows S1 을 로드맵에서 제거하지 않는다. 단 limitations 의 6개 미검증 항목(admin SID 비활성·전용 계정·NTFS ACL·방화벽·상속·Session 0)은 P0-01b 로 분리해 v0.2 전에 확인한다"
---

# P0-01 · Windows S1 Restricted Native + CUDA

## 무엇을 입증하려 했는가

기준선 §9.2 는 S1 을 **v0.2 의 필수 경로**로 두었고, §43.6 은 P0-01 실패 시
**ADR-005 수정 · Windows S1 제거 · §34.1 시나리오 재작성**을 요구한다.

즉 이 스파이크는 **아키텍처를 뒤집을 수 있는 항목**이다.
핵심 질문은 하나다 — **특권이 제거된 토큰에서 CUDA 가 초기화되는가.**

GPU 드라이버는 커널 객체 접근을 요구하고 WDDM 은 세션에 민감하므로,
"제한된 토큰에서는 CUDA 가 아예 안 뜬다" 가 충분히 있을 수 있는 결과였다.

## 어떻게 측정했는가

**단계 사다리**로 설계했다. 실패했을 때 *어디서* 깨졌는지 알아야 판정이 가능하다.

```text
b0_alive    python 프로세스가 뜨는가
b1_import   torch 를 import 할 수 있는가
b2_avail    torch.cuda.is_available()
b3_compute  실제 CUDA 연산 (512x512 matmul)
```

대조군(A, 일반 토큰)을 먼저 두어 "환경 문제" 와 "Restricted Token 문제" 를 분리했다.

## 결과

### B — Restricted Token 에서 CUDA 가 완전히 동작한다

```text
b0_alive    exit=0  ALIVE
b1_import   exit=0  TORCH 2.13.0+cu126
b2_avail    exit=0  CUDA_AVAIL True
b3_compute  exit=0  cuda_available True | device_count 1
                    device_name NVIDIA GeForce RTX 4070 SUPER
                    matmul_ok True | vram_alloc_mb 9.12
```

`CreateRestrictedToken(DISABLE_MAX_PRIVILEGE)` 로 **특권을 전부 제거한 토큰**에서
`CreateProcessAsUserW` 로 띄운 프로세스가 CUDA 를 초기화하고 실제 연산까지 수행했다.

### C — Job Object 종료 시 VRAM 이 반환된다

```text
VRAM used(MiB)   before=168   during=489   after=168
```

CUDA 로 4096×4096 텐서를 잡아 VRAM 이 **321MiB 증가**한 것을 관측한 뒤
`TerminateJobObject` 로 종료했고, **정확히 원래 값으로 복귀**했다.

`during > before` 를 확인하지 못하면 `INCONCLUSIVE` 로 판정하도록 설계했다.
증가를 관측하지 못한 채 "반환 성공" 을 통과로 세면 아무것도 증명하지 못하기 때문이다.

### D — 차단 대상 목록 확보 (기준선 측정)

제한 없는 프로세스에서 Chrome `Local State` 가 읽혔다.
x600 에는 `~/.ssh` 가 없어 SSH 키는 대상에서 빠졌다.
**이것은 차단을 검증한 것이 아니라 차단해야 할 목록을 확인한 것이다.**

## ★ 오판 정정 — 두 번 틀렸다

이 스파이크에서 **잘못된 결론을 두 번 냈다가 정정**했다. 기록해 둔다.

**1차 — `OpenProcessToken` err=6 을 "토큰 조작 불가" 로 오판**

`k32.GetCurrentProcess.restype` 을 지정하지 않아 ctypes 가 반환값을 `c_int` 로 잘랐다.
64비트에서 의사 핸들 `(HANDLE)-1` 이 truncate 되어 `ERROR_INVALID_HANDLE(6)` 이 났다.
**환경의 제약이 아니라 프로브의 결함이었다.**

**2차 — 빈 출력을 "CUDA 실패(FAIL-ARCHITECTURE)" 로 오판**

`CreateProcessAsUserW` 는 성공했으나 자식이 exit=1 로 끝나고 출력이 비었다.
이것을 CUDA 실패로 단정할 뻔했다. 실제 원인은 **작업 디렉터리와 출력 파일 경로**였고,
`C:\Windows\Temp` 로 바꾸고 단계 사다리를 넣자 **전 단계 통과**로 뒤집혔다.

> **교훈: "출력이 없다" 는 "실패했다" 가 아니다.**
> 어느 단계에서 깨졌는지 특정하지 못하면 판정하지 않는다.
> 이 실수를 잡지 못했다면 ADR-005 를 잘못 뒤집고 Windows S1 을 로드맵에서 제거했을 것이다.

## 이 실험이 증명하지 "않는" 것

- **`DISABLE_MAX_PRIVILEGE` 만 적용했다.** 기준선 §9.3 이 요구하는
  **관리자 SID 비활성(`SidsToDisable`)과 제한 SID(`SidsToRestrict`)는 미검증**이다.
  더 강한 제한에서도 CUDA 가 되는지는 모른다.
- **전용 저권한 계정을 만들지 않았다.** 현재 사용자 토큰을 제한한 것이다.
- **NTFS ACL 차단을 검증하지 않았다.** D 는 기준선 측정일 뿐이다.
- **프로세스별 아웃바운드 방화벽을 검증하지 않았다.**
- **자식/손자 프로세스의 제한 상속**을 확인하지 않았다.
- **Windows Service(Session 0)에서의 동작을 검증하지 않았다.**
  실제 Agent 는 서비스로 동작하므로 이 경로는 별도 검증이 필요하다.
- **단일 기계·1회 측정**이며 torch 2.13.0+cu126 한 조합만 확인했다.

## 결정

1. **P0-01 을 `PASS` 로 판정한다.**
2. **ADR-005 를 유지한다.** Windows S1 을 로드맵에서 제거하지 않는다.
   기준선 §33.3 v0.2 의 Windows S1 항목이 유효하다.
3. **미검증 6항목을 `P0-01b` 로 분리**해 v0.2 착수 전에 확인한다.
   특히 **Session 0 동작**과 **admin SID 비활성**이 남은 위험이다.
4. §9.1 capability matrix 의 S1 행 중 "network egress 제어" 는
   **여전히 미검증 표시를 유지한다** (검토 리포트 P1-15).

관련: `docs/decisions/ADR-026_...md` 는 무관. ADR-005 는 기준선 §38 참조.
계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md`

---

## ★ 이후 변경 (2026-08-18 01:30) — claim 범위 초과, 후속 검증(P0-01b) 미완

독립 검수(`agent:codex-cli`, read-only)가 재검수해 `CHANGES_REQUESTED`
로 판정했다. ★ 이 evidence 는 실제 NVIDIA GPU 하드웨어 실측이라
**이 세션(개발 기계는 Intel Iris Xe, NVIDIA GPU 없음)은 재실측할
수 없다** — 아래 정정은 코드·문서의 내적 일관성만으로 확인한
것이고, 하드웨어 재현은 확인 안 됨으로 명시한다.

### claim 을 이렇게 좁혀 읽는다

원래 claim 은 "S1(Restricted Native) 격리가 성립한다"로 넓게
읽힌다. 실제로 시험한 것은:

- 토큰: `CreateRestrictedToken(DISABLE_MAX_PRIVILEGE)` 만 적용했다
  (`tools/probes/p0_01_windows_s1_cuda.py:142-156`). 관리자 SID
  비활성·제한 SID(restricted SID) 는 시험하지 않았다.
- 프로세스: Job Object 에 **단일** Python 프로세스만 넣었다
  (`tools/probes/p0_01_windows_s1_cuda.py:281-289`). 자식·손자
  프로세스 트리는 검증하지 않았다.

> claim 은 "시험한 단일 호스트에서, privilege-strip 토큰 하나
> (`DISABLE_MAX_PRIVILEGE`)를 적용한 **단일** CUDA 프로세스의
> 초기화·연산과 Job Object 종료 후 VRAM 회수가 관측됐다"로 좁혀
> 읽는다. "완전한 S1 경로가 성립한다"는 더 넓은 주장이다.

### negative_tests 분류 정정

`A`·`B`·`C`·`b0_alive`~`b3_compute` 는 실재하는 프로브 단계다
(`tools/probes/p0_01_windows_s1_cuda.py:127-135,194-252,271-333`,
`docs/evidence/_raw/P0-01_probe.txt:5-28`). ★ "1차 실행에서
OpenProcessToken err=6" 은 재실행 가능한 negative test 이름이
아니라 **과거의 오판 서술**이다 — 지금 코드엔 수정된 `restype` 만
남아 있다(`tools/probes/p0_01_windows_s1_cuda.py:45-68`).

### P0-01b 후속 검증은 아직 없다

이 문서의 decision 이 "P0-01b 로 분리해 후속 검증한다"고 적었지만,
지금 저장소에 `P0-01b` evidence 나 probe 가 **없다** — 약속된
후속 검증이 완료됐다고 볼 근거가 없다. 관리자 SID 비활성·제한
SID·프로세스 트리 검증은 여전히 미착수로 남는다.

### review_outcome

`CHANGES_REQUESTED` → 위 정정으로 claim 범위·negative_tests 분류·
P0-01b 미완 사실을 반영했다. 원본 YAML 은 당시 기록이므로 고치지
않는다.

★ 2026-08-18 01:40 두 번째(최종) 재검수 — `agent:codex-cli` 가
**`ACCEPTED`** 로 판정했다. GPU 재실측 불가 명시, claim 범위 축소,
negative_tests 분류 정정, `P0-01b` 부재 확인 — 이전 지적 세 가지를
전부 반영했다고 확인했다. "하드웨어 결과의 재현성은 확인 안 됨이며,
문서가 이를 숨기지 않는다."

---

## ★ 이후 변경 (2026-08-18) — schema v2 승격, 하드웨어 재실측 없이

`DoD-01`~`08` 을 schema v2 로 승격하며 확립한 절차("오늘 재실행한
검증 + 오늘 새 독립 검수")를 이 evidence 에 그대로 적용할 수
없다는 것을 먼저 밝힌다 — **P0-01 은 원격 NVIDIA GPU 하드웨어(x600)
실측이고, 이 세션은 그 기계에 SSH 로 접근할 자율 권한이 없다**
(세션 안전 정책상 원격 시스템 접근은 사용자 판단이 필요한 범주로
분류되어 자동 모드 classifier 가 거부한다). 개발 기계 자체에도
NVIDIA GPU 가 없다(Intel Iris Xe). 이는 이미 2026-08-18 01:30
addendum 이 명시한 제약과 같다 — 오늘도 달라지지 않았다.

### 이번 v2 승격의 "재검증" 범위 — 소스 일치성만

하드웨어 재실측을 지어내지 않는다(`RULE.md` §7.3 의 비허위 원칙).
대신 probe 스크립트(`tools/probes/p0_01_windows_s1_cuda.py`)가
2026-08-18 addendum 이 인용한 파일:줄과 지금도 일치하는지만
확인했다 — `docs/evidence/_raw/P0-01_v2_promotion_2026-08-18.txt`
참조. 일치했다(변경 없음).

### v2 승격이 의미하는 것 — 재확인이지 재실측이 아니다

이 v2 승격은 "P0-01 의 하드웨어 결과가 오늘 다시 확인됐다"는
뜻이 **아니다.** "이 evidence 문서와 그 이전 addendum 들이
정확하고, RULE.md §7.3 이 요구하는 provenance 형식을 additive 하게
채웠다"는 뜻이다. 하드웨어 재현은 여전히 `P0-01b`(미착수)의
몫이다 — 사용자가 x600 접속을 승인하거나 직접 재실행해야 한다.

### review_outcome

아래 좁은 재검수가 이 접근(하드웨어 재실측 없이 소스 일치성만
확인하는 v2 승격)이 정직하고 충분한지를 판단한다.
