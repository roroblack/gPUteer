---
id: P0-06
claim: "기준선 §10.3 의 주장 — 소비자 GPU 에는 VRAM quota 를 강제할 수단이 없다 — 을 검증하고, ADR-015(Exclusive 기본값)를 유지할지 판정한다"
status: FAIL-SCOPE
commit: 5dba36e62c230b3ec894cb8ca16623da2aec2c2a
binary_digests:
  probe_a_d: "tools/probes/p0_06_vram_enforcement.py"
  probe_sweep: "tools/probes/p0_06b_jobobject_vram_sweep.py"
  torch: "2.13.0+cu126"
protocol_versions:
  none: "해당 없음 - OS/드라이버 계층 스파이크"
platform: "원격 x600 (<x600-hostname>) — Microsoft Windows 11 Pro build 26200 / WDDM 모드"
hardware: "NVIDIA GeForce RTX 4070 SUPER 12282MiB / driver 595.79 / CUDA 13.2 / RAM 23.1GB"
network_profile: "SSH 경유 원격 실행. 측정은 로컬 GPU/프로세스만 사용"
command: |
  scp tools/probes/p0_06_vram_enforcement.py x600:p0_06_probe.py
  ssh x600 'python p0_06_probe.py'
  scp tools/probes/p0_06b_jobobject_vram_sweep.py x600:jo_sweep.py
  ssh x600 'set PYTHONIOENCODING=utf-8 && python jo_sweep.py'
raw_output: |
  [A] PyTorch set_per_process_memory_fraction
      total_mib 12282 / cap_mib 2456 (fraction 0.20)
      OVER_ALLOC_BLOCKED  CUDA out of memory. Tried to allocate 3.84 GiB.
      [우회] 같은 프로세스에서 fraction 을 1.0 으로 되돌림
      RESET_BYPASS_OK 6141
      판정: 협조적 제한만 가능 — 워크로드가 되돌리면 무력화

  [B] Job Object 메모리 제한 (RAM 4096MiB, VRAM 6144MiB 요청)
      HELD 도달=False  exit_code=1
      자식 출력:
        torch.OutOfMemoryError: CUDA out of memory. Tried to allocate 6.00 GiB.
        GPU 0 has a total capacity of 11.99 GiB of which 10.81 GiB is free.
      판정(1차): INCONCLUSIVE

  [C] 외부 프로세스 VRAM 관측
      VRAM(전체) before=168 during=2405 after=168
      nvidia-smi --query-compute-apps: "34448, [N/A]" / "26360, [N/A]"
      판정: PARTIAL — PID 목록은 보이나 프로세스별 사용량은 [N/A]

  [D] MIG / MPS
      nvidia-smi mig.mode.current = '[N/A]'
      nvidia-cuda-mps-control: 없음
      판정: §10.3 주장 확인 — 하드웨어 분할 수단 없음

  ##### 대조 실험 1 — Job Object 없이 대형 할당 #####
      free_mib 11071 total_mib 12282
      ALLOC_OK 2048 / ALLOC_OK 4096 / ALLOC_OK 6144 / ALLOC_OK 8192

  ##### 대조 실험 2 — RAM 제한 x VRAM 요청 스윕 #####
      RAM=무제한   1024:OK  2048:OK  3072:OK  4096:OK  6144:OK
      RAM=8192    1024:OK  2048:OK  3072:OK  4096:OK  6144:OK
      RAM=6144    1024:OK  2048:OK  3072:OK  4096:OK  6144:FAIL
      RAM=4096    1024:OK  2048:OK  3072:FAIL 4096:FAIL 6144:FAIL
      RAM=3072    1024:OK  2048:FAIL 3072:FAIL 4096:FAIL 6144:FAIL
artifacts:
  - docs/evidence/_raw/P0-06_probe.txt
  - tools/probes/p0_06_vram_enforcement.py
  - tools/probes/p0_06b_jobobject_vram_sweep.py
negative_tests:
  - "A 에서 '제한이 걸리는가'만 보지 않고 '워크로드가 되돌릴 수 있는가'를 적극 시도해 RESET_BYPASS_OK 를 확인. 협조적 제한임을 실증"
  - "B 의 1차 INCONCLUSIVE 를 결론으로 삼지 않고 대조 실험(Job Object 없이 같은 크기 할당)을 수행해 8192MiB 까지 성공함을 확인 — '대형 할당 한계'와 'Job Object 효과'를 분리"
  - "대조 실험 2 에서 RAM 제한 5수준 x VRAM 요청 5수준 = 25조합을 스윕해 단조 관계를 확인. 단일 조합의 우연을 배제"
  - "C 에서 'PID 가 보인다'와 '사용량이 보인다'를 구분해 [N/A] 를 발견. 1차 판정(PASS)을 PARTIAL 로 정정"
  - "D 에서 MIG 미지원을 nvidia-smi 조회로 직접 확인 (추정하지 않음)"
limitations:
  - "★ Windows WDDM 모드에서만 측정했다. Linux 는 GPU 메모리 모델이 달라(전용 VRAM, cgroup 은 VRAM 미관여) 이 결과가 적용되지 않는다"
  - "★ VRAM 상한이 정확히 얼마인지 특정하지 못했다. 스윕 간격이 1024MiB 라 'RAM 제한 - 약 2000MiB' 라는 근사만 얻었다"
  - "오버헤드(~2000MiB)는 torch + CUDA 컨텍스트 기준이다. 다른 프레임워크·모델에서는 달라진다"
  - "프로세스가 시스템 RAM 을 많이 쓰면 VRAM 몫이 줄어든다. 즉 이것은 VRAM quota 가 아니라 '총 커밋 상한'이다"
  - "단일 GPU(RTX 4070 SUPER)·단일 드라이버(595.79)·WDDM 모드 한 조합이다. TCC 모드는 미검증"
  - "여러 프로세스가 동시에 서로 다른 Job Object 제한을 받는 상황을 검증하지 않았다"
  - "제한 초과 시 프로세스가 죽는지 할당만 실패하는지 구분하지 않았다 (관측된 것은 CUDA OOM 예외)"
  - "fragmentation 이 상한에 미치는 영향을 분리 측정하지 않았다"
decision: "§10.3 의 'Job Object 는 시스템 RAM 만 제한하고 VRAM 과 무관하다' 는 서술이 Windows WDDM 에서 **틀렸음**을 확인했다. ADR-027 을 신설해 §10.3 을 정정한다. 다만 상한이 코스하고(±2GB) Windows 전용이며 VRAM quota 가 아닌 총 커밋 상한이므로 **ADR-015(Exclusive 기본값)는 유지한다**. Shared 허용 조건(§10.4)에 'Windows + Job Object 커밋 상한' 을 세 번째 경로로 추가할 것을 요청한다"
---

# P0-06 · VRAM Enforcement Reality Check

## 무엇을 입증하려 했는가

기준선 §10.3 은 **ADR-015(GPU 할당 기본값 = Exclusive)의 유일한 근거**다.

> 소비자 GPU 에는 VRAM quota 를 강제할 수단이 없다.
> MIG 는 데이터센터 전용, MPS 는 Linux 전용,
> **컨테이너·Job Object 는 시스템 RAM 만 제한하고 VRAM 과 무관하며**,
> PyTorch fraction 은 워크로드 협조를 요구한다.

§43.6 은 이 주장이 **반증되면 ADR-015 를 철회하고 §10.2 기본값을 Shared 로 되돌리라**고 정한다.

따라서 이 스파이크의 목적은 "안 될 것이다" 를 확인하는 것이 아니라
**강제할 방법을 적극적으로 찾아보는 것**이었다.

## 어떻게 측정했는가

네 경로를 각각 시도하고, 예상 밖 결과가 나오면 **대조 실험으로 원인을 분리**했다.

## 결과

### A — PyTorch fraction 은 협조적 제한이다 (§10.3 확인)

```text
set_per_process_memory_fraction(0.20)  ->  cap 2456 MiB
상한 초과 할당 시도                     ->  OVER_ALLOC_BLOCKED
같은 프로세스에서 fraction 을 1.0 으로 되돌림 -> RESET_BYPASS_OK 6141 MiB
```

**제한은 걸리지만 워크로드가 한 줄로 되돌릴 수 있다.** §10.3 의 서술이 맞다.

### D — MIG / MPS 없음 (§10.3 확인)

```text
nvidia-smi mig.mode.current = '[N/A]'    GeForce 는 MIG 미지원
nvidia-cuda-mps-control      없음         MPS 는 Windows 미지원
```

### ★ B — §10.3 이 틀렸다: Job Object 가 VRAM 을 제한한다

1차 실행에서 RAM 제한 4096MiB 를 걸고 VRAM 6144MiB 를 요청하자 실패했다.
**여기서 멈추면 "프로세스가 RAM 때문에 죽었다" 로 잘못 해석하기 쉽다.**

대조 실험을 했다.

```text
대조 1 — Job Object 없이 동일 할당
  ALLOC_OK 2048 / 4096 / 6144 / 8192      (전부 성공)
```

즉 **대형 할당 자체는 문제가 없다.** Job Object 가 원인이다.

경계를 특정하기 위해 5×5 스윕을 돌렸다.

```text
RAM=무제한   1024:OK  2048:OK  3072:OK  4096:OK  6144:OK
RAM=8192    1024:OK  2048:OK  3072:OK  4096:OK  6144:OK
RAM=6144    1024:OK  2048:OK  3072:OK  4096:OK  6144:FAIL
RAM=4096    1024:OK  2048:OK  3072:FAIL 4096:FAIL 6144:FAIL
RAM=3072    1024:OK  2048:FAIL 3072:FAIL 4096:FAIL 6144:FAIL
```

**깨끗한 단조 관계다.**

```text
VRAM 최대 할당량  ≈  Job Object RAM 제한  −  약 2000 MiB
```

기전은 **WDDM 의 GPU 메모리 모델**로 보인다. WDDM 에서 비디오 메모리는
가상화·페이지 가능하며 시스템 메모리 커밋으로 뒷받침된다.
따라서 프로세스 커밋 상한이 VRAM 할당까지 지배한다.
(기전은 관측으로부터의 추론이며 드라이버 내부를 확인하지는 않았다.)

**결정적 차이: 이것은 워크로드 협조가 필요 없다.**
PyTorch fraction 과 달리 자식이 되돌릴 수 없다.

### C — 프로세스별 VRAM 은 볼 수 없다 (판정 정정)

1차에 `PASS` 로 판정했다가 정정했다.

```text
nvidia-smi --query-compute-apps=pid,used_memory
  34448, [N/A]
  26360, [N/A]
```

**PID 목록은 보이지만 사용량이 `[N/A]` 다.** WDDM 모드 GeForce 의 제약이다.
전체 사용량(`memory.used`)은 얻을 수 있으므로,
§10.5 의 `external_process_vram` 은 **"전체 − gPUteer 자신의 할당"** 으로
간접 산출해야 한다. 직접 조회는 불가능하다.

## 이 실험이 증명하지 "않는" 것

- **★ Windows WDDM 전용 결과다.** Linux 는 전용 VRAM 모델이고 cgroup 은 VRAM 에
  관여하지 않으므로 **이 결과가 적용되지 않는다.** v0.1 주 타깃이 Linux 컨테이너이므로
  이 발견이 v0.1 에 주는 이득은 없다.
- **★ 정확한 상한을 특정하지 못했다.** 스윕 간격이 1024MiB 라 "제한 − 약 2000MiB"
  근사만 얻었다. 실제 운용에 쓰려면 더 촘촘한 측정이 필요하다.
- **오버헤드 2000MiB 는 torch+CUDA 기준**이다. 프레임워크·모델이 바뀌면 달라진다.
- **이것은 VRAM quota 가 아니라 총 커밋 상한이다.** 프로세스가 시스템 RAM 을 많이 쓰면
  VRAM 몫이 줄어든다. "이 Job 에 정확히 8GB VRAM" 을 지정할 수 없다.
- 단일 GPU·단일 드라이버·WDDM 모드 한 조합이다. TCC 모드는 미검증.
- **여러 프로세스가 동시에 서로 다른 제한을 받는 상황**을 검증하지 않았다.
  Shared 모드의 실제 시나리오가 바로 그것이다.
- fragmentation 의 영향을 분리하지 않았다.

## 결정

1. **status = `FAIL-SCOPE`.** §10.3 의 서술 일부가 틀렸으므로 문서 수정이 필요하지만,
   범위를 좁히면(Windows 한정·코스한 상한) 나머지 결론은 유지된다.
2. **ADR-027 을 신설해 §10.3 을 정정한다.**
   "Job Object 는 VRAM 과 무관" → "Windows WDDM 에서는 총 커밋 상한을 통해
   VRAM 을 간접적으로 상한한다".
3. **ADR-015(Exclusive 기본값)는 유지한다.** 이유:
   - 상한이 코스하다 (±2GB). §10.5 admission 이 요구하는 정밀도에 못 미친다
   - Windows 전용이다. v0.1 주 타깃인 Linux 에는 적용되지 않는다
   - VRAM quota 가 아니라 총 커밋 상한이라 시스템 RAM 사용량에 따라 흔들린다
   - 다중 프로세스 동시 제한이 미검증이다
4. **§10.4 Shared 허용 조건에 세 번째 경로를 추가할 것을 요청한다.**
   현재 (A) 동일 소유자 (B) Linux+MPS 두 가지인데,
   **(C) Windows + Job Object 커밋 상한 (보수적 마진 적용)** 을 추가한다.
5. **§10.5 의 `external_process_vram` 산출 방식을 명시할 것을 요청한다.**
   프로세스별 조회가 불가능하므로 "전체 − 자신의 할당" 간접 산출임을 문서화한다.

관련: `docs/decisions/ADR-027_windows_jobobject_vram_상한.md`
계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md`

---

## ★ 이후 변경 (2026-08-18 02:00) — claim 정밀화, runtime-policy 와의 관계 명시

독립 검수(`agent:codex-cli`, read-only)가 재검수해 `CHANGES_REQUESTED`
로 판정했다. ★ 이 evidence 는 실제 GPU 하드웨어(x600) 실측이라 이
세션(Intel Iris Xe, NVIDIA GPU 없음)은 재실측할 수 없다 — 아래
정정은 코드·문서의 내적 일관성만으로 확인한 것이다.

### claim 을 이렇게 정밀화한다

원래 claim ("소비자 GPU 에는 VRAM quota 를 강제할 수단이 없다")
은 이 evidence 자신이 뒤에서 찾은 사실(Windows WDDM Job Object 가
간접적인 총 커밋 상한으로 VRAM 에 영향을 준다, `:116-149`)과
표면적으로 충돌하는 것처럼 읽힐 수 있다. 실제로는 evidence 본문이
이미 정밀하게 구분해 뒀다 — "이것은 VRAM quota 가 아니라 총
커밋 상한이다"(`:169-178`). claim 을 다음처럼 명시한다:

> "**정밀한(hard) VRAM quota 를 강제할 수단은 없다.** Windows
> WDDM 에서는 Job Object 의 총 커밋 상한으로 **간접** 제한이
> 가능하지만, 그 상한은 VRAM 전용이 아니라 시스템 RAM 사용량에
> 따라 흔들리는 코스한(±2GB) 총 커밋 상한이다."

★ 2026-08-18 02:15 정정 — 처음엔 "frontmatter `claim` 필드에도
반영한다"고 적었는데 **거짓이었다**(재검수가 지적했다) — frontmatter
의 `claim` 은 실제로 고치지 않았다. 정확히 말하면: 이것은 원래
claim 을 뒤집는 것이 아니라, evidence 본문 자체가 이미 이렇게
정밀하게 결론 내렸다는 것을 **이 addendum 이 명시적으로 해석해
보여주는 것**이다 — frontmatter `claim`(`:3`)은 당시 기록 그대로
남아 있고, 이 addendum 이 그것을 어떻게 좁혀 읽어야 하는지를
알려줄 뿐이다.

### negative_tests/프로브 이름은 실재 확인됨

`tools/probes/p0_06_vram_enforcement.py:99,172,263,323` 와
`tools/probes/p0_06b_jobobject_vram_sweep.py:38-68`(5×5 sweep) 전부
확인했다.

### runtime-policy 크레이트와의 관계 (새 limitation)

이 evidence 이후 `crates/runtime-policy` 크레이트가 신설되어
VRAM 판정을 `VramEnforcement`/`HostProtectionClaim` 으로 분류한다
(`crates/runtime-policy/src/vram.rs`). ★ **그 크레이트는 판정만
하고 실제 Job Object 를 생성·설정하지 않는다** — 이 evidence 의
OS 호출(`CreateJobObject`/`SetInformationJobObject`)은 여전히
독립 Python probe 에서만 수행됐고, Rust 코드에는 연결된 적이
없다(`crates/runtime-policy/src/vram.rs:19-26`,
`crates/runtime-policy/src/lib.rs:36-43`). 이 limitation 을
추가한다.

기존 limitation(코스한 상한·Windows 전용·VRAM quota 아닌 총 커밋
상한 등, `:65-73`)은 지금도 유효하다.

### review_outcome

★ 2026-08-18 02:00 최초 정정 `CHANGES_REQUESTED` → claim 정밀화와
runtime-policy 관계를 반영했다. 원본 YAML 은 당시 기록이므로
고치지 않는다.

★ 2026-08-18 02:15 두 번째 재검수 — `agent:codex-cli` 가 인용은
전부 실재를 확인했지만 "frontmatter 에도 반영했다"는 문장이
거짓이라고 지적했다(실제로는 addendum 의 해석일 뿐, `claim` 필드
자체는 안 고쳤다). 위에서 "이 addendum 이 명시적으로 해석해
보여주는 것"으로 고쳤다. `status: FAIL-SCOPE` 는 본문 결론과
이미 일치하므로 바꾸지 않는다(재검수도 이 점은 동의했다).

★ 2026-08-18 02:30 세 번째(최종) 재검수 — `agent:codex-cli` 가
**`ACCEPTED`** 로 판정했다. frontmatter `claim`(`:3`)·`status`(`:4`)
가 그대로 보존되어 있고, 거짓 문장을 명시적으로 철회한 것을
확인했다. "남은 변경 요청은 없다."

## ★ 이후 변경 (2026-08-18 14:15) — `crates/runtime-windows` 신설로 limitation 부분 해소

위 "runtime-policy 크레이트와의 관계" 절이 적어 둔 limitation —
"`crates/runtime-policy` 는 판정만 하고 실제 Job Object 를
생성·설정하지 않는다" — 이 부분적으로 해소됐다.

`crates/runtime-windows`(신규)가 `windows_commit_cap()` 의 판정을
실제 `CreateJobObjectW`/`SetInformationJobObject`/
`AssignProcessToJobObject` 호출로 연결했고,
`crates/runtime-windows/tests/commit_cap.rs` 가 이 저장소(로컬
개발 기계, Windows 11, GPU 없음)에서 실제로 자식 프로세스를
`VirtualAlloc` 루프로 돌려 커밋 상한이 걸리는지, negative
control(Job Object 없이 돌린 같은 fixture)과 대조해 확인했다.
뮤테이션 테스트(`AssignProcessToJobObject` 호출을 일시 무력화)로
이 실측 테스트 자체가 공허하지 않음도 확인했다.

**중요한 실측 발견 — 이 evidence 의 "이것은 VRAM quota 가 아니라
총 커밋 상한이다"(`:177-178`) 라는 결론이 Rust 구현에서도 다시
확인됐다.** `JOB_OBJECT_LIMIT_JOB_MEMORY` 는 딱딱한 상한이 아니라
**소프트** 제한이다 — `PeakJobMemoryUsed` 가 설정한 `JobMemoryLimit`
을 5회 연속 측정 모두에서 약 700~850KiB 만큼 넘었다(64MiB 상한
기준). `crates/runtime-policy/src/vram.rs::guarantees_hard_limit()`
가 `WindowsCommitCap` 에도 `false` 를 반환하도록 미리 정해 둔
판단이 옳았다는 것을 이번에는 **Rust/Win32 직접 호출**로도
재확인한 것이다.

**여전히 해소되지 않은 부분**: 이 evidence 자체(x600 실제 GPU
하드웨어, torch+CUDA, WDDM VRAM 간접 상한)는 재실측하지 않았다 —
이번 실측은 **로컬 개발 기계에서 RAM 커밋만** 확인했다(GPU 가
없다). "RAM 커밋 상한이 VRAM 에도 간접적으로 적용된다"는 이
evidence 의 핵심 발견(WDDM 메모리 모델 추론, `:146-149`)은 여전히
x600 실측(2026-08-15)에만 근거한다. `limitations` 목록(`:65-73`)
중 나머지(정밀 상한 미특정·다중 프로세스 미검증·TCC 모드 미검증
등)도 전부 그대로 유효하다.

관련: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
와 같은 세션의 `crates/runtime-windows` 신설 작업.
`docs/history/HISTORY.md` 2026-08-18 14:15 항목 참조.
