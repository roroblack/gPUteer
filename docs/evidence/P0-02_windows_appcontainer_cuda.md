---
schema_version: 2
id: P0-02
claim: "기준선 §32 의 `P0-02`(Windows AppContainer + CUDA, S2 실험적)를 실측했다. **x600(RTX 4070 SUPER · 그 Windows 빌드 · package identity 없는 AppContainer)에서** 프로파일 생성·고유 SID·컨테이너 안 프로세스 실행·Python 실행·C 확장 로드(`zlib`·`_socket`)를 확인했다. **가둠은 개발 기계에서 따로 쟀다** — 컨테이너 안에서 호스트 임시 파일을 `type` 하면 종료 코드 1, 바깥에서는 0, 보안 속성을 빼면 안쪽도 0 이다(`P0-02_가둠_대조_재실측_2026-09-10.txt`). ★ 거부 **이유**(오류 코드·메시지)는 원문에 없다 — 종료 코드가 보안 속성에 따라 갈린다는 것까지다. 그러나 x600 에서 **`torch.cuda.is_available()` 까지 도달하지 못했다** — `torch/__init__.py:14` 의 `import ctypes` 가 `_ctypes.pyd` 초기화 실패(1114)로 죽는다. DLL 을 하나씩 열어 보니 **시험한 DLL 중 `ole32.dll` 로드에서도 1114 가 관측됐다** — `_ctypes` 실패와의 인과는 확인하지 않았다. `combase.dll` 은 **로드**됐다(COM 기능 실행은 안 쟀다). ★ 그러므로 이 스파이크는 **전체 질문에 답하지 못했다** — `status: INCONCLUSIVE`. 잰 좁은 구성에 대한 판정은 decision 에 가른다"
status: INCONCLUSIVE
commit: 52d72b9

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — SSH 로 x600 원격 실측. AppContainer primitive(신규 300여 줄)·프로브 2종 신설, 여섯 차례 측정"
executor_model: "claude-opus-5"
executed_at: "2026-09-05 (x600 1~5차 측정) / 2026-09-10 (개발 기계 가둠 재실측) — 시·분은 원문에 없다"
# ★ 초안일 때는 초안 작성 시각(2026-09-06T00:20:00+09:00)을 넣어 두었다. 측정 시각이 아니라서
#   evidence 로 옮기며 날짜만 남겼다. 정확한 시각은 지어내지 않는다(재검수 27·29)

review_required: false
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec -m gpt-6-astra --sandbox read-only -c model_reasoning_effort=high (CLI 0.153.4 — 29 원문 1~10행) — 2026-09-10 재검수 29(P0-02 단독 4차)가 ACCEPTED. 앞선 검수 8·16·19·23·25·27 은 CHANGES_REQUESTED, 21·22 는 이 초안에 추가 지적 없음"
reviewer_model: "gpt-6-astra"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: >
  `RULE.md` §7.3 은 **`P0-*`·`DoD-*` 의 `status: PASS`** 에 대해서만 독립
  검수를 강제한다. 이 문서는 `INCONCLUSIVE` 라 **강제 대상이 아니다** —
  그래도 받는다.

  ★ 강제가 아닌데 왜 받나. 여기 적힌 판단 중 **"AppContainer 자체는
  동작한다"** 가 반증에 취약하기 때문이다. x600 의 여섯 차례 측정(Python·DLL)이 전부 한
  기계·한 Windows 빌드에서 나왔고, `ole32` 실패가 이 빌드 고유이면
  이 문서의 방향 자체가 틀린다.

  검수자에게 특히 볼 것:
    1. "되는 것" 목록이 실제로 재현되는가 — 특히 가둠 테스트가
       컨테이너 밖에서도 통과하지 않는가(첫 판이 정확히 그랬다)
    2. 시험한 네 변경에 각각 **대조**가 있는가, 아니면 "해 봤는데 안
       됐다" 인가
    3. 결론이 측정을 넘어서지 않는가 — package identity 가설을 어디선가
       결론처럼 쓰고 있지 않은가
review_artifact: "docs/evidence/_raw/검수_2026-09-10/29_재검수_P0-02_단독_4차_ACCEPTED.txt"

raw_output_artifact: "docs/evidence/_raw/P0-02_appcontainer_cuda_5차_원인지목.txt"
raw_output_digest: "sha256:5323ea3b58318027374af28f056aa51c77f67c1541d48e9f1c817db4835b06d3"
raw_output_bytes: 4346

artifacts:
  - "docs/evidence/_raw/P0-02_appcontainer_cuda_5차_원인지목.txt"
  - "docs/evidence/_raw/검수_2026-09-10/29_재검수_P0-02_단독_4차_ACCEPTED.txt"
  - "docs/evidence/_raw/P0-02_appcontainer_cuda_1차.txt"
  - "docs/evidence/_raw/P0-02_appcontainer_cuda_2차.txt"
  - "docs/evidence/_raw/P0-02_appcontainer_cuda_3차_가설소거.txt"
  - "docs/evidence/_raw/P0-02_appcontainer_cuda_4차_감별.txt"
  - "docs/evidence/_raw/P0-02_가둠_대조_재실측_2026-09-10.txt"
  - "docs/evidence/_raw/DoD-56_nvml_observation_x600_2026-08-29.txt"
  - "docs/evidence/_raw/ENV-02_probe.txt"
  - "docs/evidence/_raw/방화벽_경로단위_차단_실측.txt"
  - "crates/runtime-windows/src/appcontainer.rs"
  - "crates/runtime-windows/src/bin/p0_02_probe.rs"
  - "crates/runtime-windows/src/bin/dll_probe.rs"

binary_digests:
  toolchain: "측정 당시 cargo 버전은 원문에 기록 없음 (재검수 25 — 근거 없는 버전을 뺐다)"
protocol_versions:
  schema_version: "proto 변경 없음"
  canonical_spec: "canonical 변경 없음"
platform: >
  **x600 (Windows 11, RTX 4070 SUPER, driver 595.79, CUDA 13.2)** 에서
  ★ P0-02 원문에는 에디션·빌드가 없다. 앞선 조사 `ENV-02_probe.txt` 는 Windows 11 Pro
    build 26200 으로 기록했다 — 측정 당시에도 같았는지는 확인하지 않았다(재검수 25 —
    전에는 "이 원문들에 없다" 고 적었는데 새로 인용한 ENV-02 에 있었다).
  SSH 로 원격 실측했다. 개발 기계(GPU 없음)에서는 AppContainer primitive 의
  단위 테스트만 돌렸다.

  ★ **x600 의 Python·DLL 측정은 한 기계 · 한 Windows 빌드에서만 쟀다**(가둠 대조는
  개발 기계에서 따로 쟀다). `ole32` 초기화 실패가 이
  빌드 고유인지 일반적인지 확인하지 않았다(`CLAUDE.md` §4 — 표본이 작으면
  작다고 말한다).
hardware: >
  RTX 4070 SUPER 실물. UUID·드라이버·VRAM 은 `DoD-56_nvml_observation_x600_2026-08-29.txt`,
  compute capability 8.9 는 `ENV-02_probe.txt` 에 있다 — **이 스파이크와 다른 날의
  측정**이다. 이 스파이크의 원문(1차)이 받치는 것은 컨테이너 **밖** 기준선의
  `torch 2.11.0+cu128` · `cuda_available=True` · `device_count=1` · 장치 이름 ·
  `matmul_sum=262144.0` · `compute=ok` 다. ★ 행렬 크기는 원문에 없다(재검수 23 —
  전에는 "64x64" 라 적었다)
network_profile: "없음 — 프로세스 생성과 DLL 로드만 잰다"
command: |
  # ★ 아래는 **재현용 명령 설명**이다. 실행 디렉터리와 `P0_02_CODE`·`P0_02_RAW_CMD` 사용
  #   기록은 원문(1~5차)에 없다. TEMP/TMP 를 바꿔 자식에게 물려준 서술은 3차(23~24행)·
  #   4차(5행)에 있지만, 자식이 그 값을 받아 쓴 것을 보인 출력은 없다(재검수 27 — 전에는
  #   "환경변수 사용 기록은 없다" 고 적어 3차·4차 원문과 충돌했다)
  #   원문이 받치는 경로는 둘뿐이다: 설치형 Python 은 C:, embeddable·TEMP 는 E: (1차·3차. 재검수 25)
  target\debug\p0_02_probe.exe <python 경로> [<temp 경로>] [<capability SID>]
  # 환경변수
  #   P0_02_CODE     컨테이너 안에서 돌릴 Python 한 줄
  #   P0_02_RAW_CMD  명령줄 전체 교체(dll_probe 를 돌릴 때)
  target\debug\dll_probe.exe <dll 경로> [...]

raw_output: |
  === 컨테이너 밖 (기준선) ===
  torch=2.11.0+cu128 ; cuda_available=True ; device_count=1 ;
  device_name=NVIDIA GeForce RTX 4070 SUPER ; matmul_sum=262144.0 ; compute=ok

  === 컨테이너 안 — 되는 것 ===
  import sys           ok (exit 0)
  import zlib          ok (exit 0)   <- .pyd, C 확장
  import _socket       ok (exit 0)   <- .pyd, C 확장
  호스트 임시 파일 type  안쪽 exit 1 · 바깥 exit 0 · 보안 속성 빼면 안쪽 0  <- 가둠(종료 코드까지)
                       원문 P0-02_가둠_대조_재실측_2026-09-10.txt (개발 기계)

  === 컨테이너 안 — DLL 단위 ===
  libffi-8.dll      ok=true
  python313.dll     ok=true
  vcruntime140.dll  ok=true
  oleaut32.dll      ok=true
  combase.dll       ok=true      <- **로드만** 됐다. COM 기능 실행은 안 쟀다
  rpcrt4.dll        ok=true
  shcore.dll        ok=true
  advapi32.dll      ok=true
  ole32.dll         ok=false error=1114  (ERROR_DLL_INIT_FAILED)
  _ctypes.pyd       ok=false error=1114

  === 시험한 네 변경 — 전부 같은 실패가 이어졌다 (가설을 **소거한 것이 아니다**) ===
  상위 경로 순회 권한   C:\Users\<x600-user> 이하 4개에 RX        -> 같은 실패
                        (자식이 그 경로를 실제로 순회했는지 보인 출력은 없다)
  컨테이너 TEMP 부재    E: 에 만들어 (F) + 환경 주입        -> 같은 실패
                        (자식이 그 값을 받아 거기 썼는지 보인 출력은 없다)
  capability 부재       표준 SID 둘 주입                    -> 같은 실패
                        (★ **이 둘로 안 풀렸다**까지다. COM capability 는 안 해봤다)
  Python 배포판         embeddable 3.13.7 새로 받음         -> 같은 실패
                        (안팎 대조가 가장 분명하다. 두 CPython 3.13 밖으로는 일반화 못 한다)

negative_tests: >
  이 스파이크의 negative test 는 **가둠**이다 —
  `the_container_cannot_read_a_file_the_host_can` 이 컨테이너 안에서
  호스트 임시 파일을 `type` 하면 **0 이 아닌 종료 코드**가 나오는지 본다
  (★ 거부 **이유**까지는 단언하지 않는다 — 재검수 16). **대조를 같이 둔다**(바깥
  에서는 읽힌다) — 없으면 "경로가 틀려서 실패" 로도 통과한다.
  ★ 이 테스트는 뮤테이션으로 검증했다: 보안 속성 적용을 통째로 건너뛰면
  실제로 실패한다. ★ 2026-09-10 검수 8 이 "그 결과가 원문에 없다" 고 짚어
  **다시 돌려 원문을 남겼다**(개발 기계) — 정상: 바깥 exit 0 · 안쪽 exit 1 /
  보안 속성 부착만 뺀 뮤테이션: 안쪽 exit 0 -> 테스트 실패.
  `docs/evidence/_raw/P0-02_가둠_대조_재실측_2026-09-10.txt`. x600 에서는 다시
  재지 않았다. 처음 쓴 버전(`cmd /c exit 42` 의 종료 코드만 확인)은
  **컨테이너 밖에서도 통과**해 아무것도 재지 못했고, 그 뮤테이션이 그것을
  드러냈다.

  그 밖에: 프로파일 이름이 같으면 SID 도 같은지(재시작 뒤 방화벽 규칙이
  엉뚱한 컨테이너를 가리키지 않게), 프로파일 폴더가 생기는지.

limitations: >
  ★★ **이 스파이크는 자기 질문에 답하지 못했다.** `P0-02` 는
  "AppContainer 에서 CUDA 가 되는가" 를 묻는데, **CUDA 근처에도 가지
  못했다** — 그 앞의 `import ctypes` 에서 막힌다.

  ★ **`ole32` 로드가 왜 1114 인지, 그것이 `_ctypes` 실패의 원인인지
  모른다.** 도구(`dll_probe`)는 `LoadLibraryW` 하나와 Win32 오류 코드뿐이라
  실패한 초기화 함수의 위치 · 인과 · 다른 장애의 부재를 확정하지 못한다
  (2026-09-10 검수 8. 전에는 "막힌 것은 ole32 하나" 라고 적었다). Process
  Monitor 등으로 로드 실패 과정의 접근·오류를 조사해 원인과 실패 위치를 확인해야
  한다(재검수 19 — 전에는 "실제 거부된 접근을 봐야 안다" 고 거부를 전제했다).

  ★★ **2026-09-07 독립 검수 지적 — 두 범위를 갈라 적어야 한다.**

      전체 P0-02 질문("AppContainer 에서 CUDA 가 되는가")
        -> INCONCLUSIVE. CUDA 를 부르지도 못했다

      **오늘 실제로 잰 구성**
        x600 의 그 Windows 빌드 · package identity 없는 bare
        AppContainer · 시험한 두 CPython 3.13 배포판
        -> CUDA 호출 **전에** 막혔다.
           ★ 여섯 번은 **여섯 단계의 탐색**이었다 — 1차는 스크립트가 시작조차
             못 했고(0xC0000135), 2차부터 `_ctypes` 초기화 실패, 4차는 성공한
             import 들, 5·6차는 DLL 로드 감별이다. 같은 실패를 여섯 번 재현한
             것이 아니다(검수 8. 전에는 "여섯 번 전부 같은 자리에서 죽었다"
             고 적었다)

      검수 판단: 판정 보류가 핑계는 아니다. 다만 `FAIL-SCOPE` 를 쓸
      수 있는 경우도 **이 좁은 구성으로 scope 를 명시할 때뿐**이고,
      S2 전체나 AppContainer 일반에 대한 `FAIL-SCOPE` 근거는 없다.

  ★ **유력한 가설을 결론으로 쓰지 않는다.** 우리 컨테이너는
  `CreateAppContainerProfile` 로 만든 **package identity 없는 맨
  AppContainer** 이고, Store 앱은 package identity 를 갖고 COM 을 쓴다 —
  그 차이일 수 있다. 그러나 **확인하지 않았으므로 추측으로만 적는다.**

  ★ x600 의 Python·DLL 측정은 **한 기계 · 한 Windows 빌드**의 결과다. 다른
  빌드에서 같은지 모른다. 가둠 대조는 개발 기계에서 쟀다.

  ★ `P0-02` 체크리스트 중 **안 한 것**: filesystem allowlist 의 완전한
  형태, outbound 제한 실측, "Create Process in Sandbox API 현재 상태
  재확인". GPU 드라이버 capability 식별은 **시작도 못 했다** — 그 앞에서
  막혔기 때문이다.

  ★ C: 의 Python 트리(27112 files) · 상위 4개 경로 · ALL APPLICATION PACKAGES 에
  준 권한은 원복했다. **E: 의 actemp · pyembed 는 재현용으로 남겼다**(3차 원문
  70~73행. 재검수 23 — 전에는 "전부 원복" 이라 적었다). 남은 `S-1-15-3-65536-...` 는
  Windows 가 원래 두는 capability ACE 이고 우리가 만든 것이 아니다.

decision: >
  **`P0-02` 의 DoD 를 아직 적용하지 않는다.**

      성공 -> S2 설계 유지, 검증된 환경에서만 enable
      실패 -> S2 를 EXPERIMENTAL 로 유지, S1 이 유일한 Windows 네이티브 경로

  ★★ **범위를 둘로 가른다**(2026-09-10 검수 8 — "실패 원인을 모르는
  것과 시험한 구성에서 실패를 관측한 것은 양립한다"):

      전체 질문(AppContainer 에서 CUDA 가 되는가)
        -> INCONCLUSIVE. CUDA 를 부르지도 못했다
      잰 구성(x600 · 그 빌드 · package identity 없음 · 시험한 두 CPython 3.13 ·
              `p0_02_probe.exe`(1차 원문)로 띄운 아래 두 import 시도. 재검수 27 —
              전에는 원문에 사용 기록이 없는 `P0_02_CODE` 를 구성에 넣었다)
        -> 설치형 Python     `import torch` 도중 `import ctypes` 에서 실패(2차 원문)
           embeddable 3.13.7 직접 실행한 `import ctypes` 에서 실패(3차 원문)
           — 어느 쪽도 CUDA 호출 **전에** 막혔다(재검수 25 — 전에는 두 배포판을
             "torch -> ctypes 경로" 하나로 묶었다)
           **이 범위로는 FAIL-SCOPE 를 뒷받침한다**

  ★ 전에는 "package identity 가능성이 남았으니 지금 실패로 확정하면 재지
    않은 것을 결론으로 쓰는 것" 이라고 적었다. **범위 없이 쓴 보류**였다 —
    원인을 모르는 것은 좁은 구성의 실패 관측을 막지 않는다.
  ★ 그래도 S2 **전체**에 대한 판정은 아니다. package identity · COM
    capability · 다른 Windows 빌드를 재지 않았다.

  ★ 그 좁은 범위의 판정은 **이미 가능하다**(위). 다음 걸음은 그것을
    뒤집는 것이 아니라 **다른 구성**을 따로 재는 것이고, 그 결과가 S2 전체
    판단에 쓰인다(재검수 16 — 전에는 "회피 불가면 FAIL-SCOPE" 로 조건을 걸어
    앞의 판정과 충돌했다):
    1. Process Monitor 등으로 `ole32` 로드 실패 과정의 접근·오류를 조사해
       실패 위치를 확인한다(★ 전에는 "DllMain 이 무엇에서 거부되는지" 라고
       적었다 — 실패 함수와 '거부' 를 조사 전에 전제했다)
    2. package identity · COM capability · 다른 Windows 빌드 구성을 따로 잰다
    3. 그 결과로 S2 전체를 EXPERIMENTAL 로 확정할지 정한다

  ★ 그 전까지 **S2 의 상태를 바꾸지 않는다.** 지금 상태(EXPERIMENTAL)가
  이 측정과 모순되지 않는다.
---

# P0-02 — Windows AppContainer + CUDA

## 한 줄

AppContainer 는 만들어지고(x600) 호스트 파일을 못 읽는 것으로 보이는데
(종료 코드 수준, 개발 기계), **그 안에서 torch 를
import 할 수 없다.** 시험한 DLL 중 `ole32.dll` 로드에서 1114 가 관측됐다 —
`_ctypes` 실패와의 인과는 확인하지 않았다.

## 왜 이걸 쟀나

2026-09-05 사용자가 **개발 기계**에서 방화벽을 실측해 `netsh advfirewall` 의
**경로 단위** 아웃바운드 차단이 동작함을 확인했다(`방화벽_경로단위_차단_실측.txt`
— ★ 이 evidence 의 측정이 아니라 배경이다. 재검수 25). 그러나 그것으로는 `NetworkPolicy` 를
강제할 수 없다 — 남의 PC 에서 `python.exe` 를 통째로 막으면 소유자의
다른 작업까지 끊긴다(`CLAUDE.md` §0.1).

AppContainer 는 **프로파일마다** SID 를 준다(같은 이름이면 같은 SID 다).
**Job 마다 별도 프로파일을 쓰는 설계라면** 그 SID 에 규칙을 걸어 그 Job 만
막는 것을 검토할 수 있다 — ★ Job 별 차단도 outbound 제한도 **재지 않았다**
(재검수 19 — 전에는 "프로세스마다 고유 SID · 그 Job 하나만 막힌다" 고 보장처럼
적었다). 그래서 방화벽 강제의 선행 **후보**이고, 그것이 이
스파이크를 지금 한 이유다.

## 어떻게 좁혔나

`import _ctypes` 는 여러 DLL 을 한 번에 시도해 **실패 지점을 감춘다.**
그래서 `LoadLibraryW` 하나만 하고 Win32 오류 코드를 찍는 `dll_probe` 를
만들어 컨테이너 안에서 DLL 을 하나씩 열었다. 그렇게 하지 않았으면
"`_ctypes` 가 안 된다" 에서 멈췄을 것이다.

## 이 실험이 증명하지 않는 것

**"AppContainer 에서 CUDA 가 안 된다" 를 증명하지 않았다.** CUDA 를
시도조차 못 했다.

**"AppContainer 에서 네이티브 코드가 안 돈다" 도 아니다.** C 확장
(`zlib`·`_socket`)은 로드된다. `combase.dll` 도 **로드**된다(기능 실행은
안 쟀다). 시험한 DLL 중 1114 가 나온 것은 `ole32` 와 `_ctypes` 다 — 다른
장애가 없다는 뜻은 아니다.

**Python·DLL 측정은 x600 한 기계 · 한 Windows 빌드**의 결과다. 가둠 대조는
개발 기계에서 쟀다(x600 에서 다시 재지 않았다).
