---
schema_version: 2
id: DoD-56
claim: "`crates/runtime-nvml` 이 `libloading` 으로 NVML 을 런타임에 열어 실제 GPU 사실(UUID·이름·total/free/used VRAM·compute capability·MIG 모드·드라이버/CUDA 버전)을 읽고, NVML 부재를 빈 목록이 아니라 typed error 로 보고하며, MIG 를 지원 안 함/꺼짐/켜짐 세 상태로 구분하고, 장치 목록을 UUID 순으로 정렬해 열거 순서에 무관한 스냅샷을 만든다. 실물 RTX 4070 SUPER(x600)에서 읽은 값이 같은 기계의 `nvidia-smi` 출력과 UUID·이름·드라이버 버전·total VRAM·compute capability 에서 일치함을 교차 확인했고, GPU 없는 개발 기계에서는 정확히 `GPU_PROBE_UNKNOWN` 으로 실패함을 확인했다"
status: PASS
commit: 690ccf138205690880b8ea84ca63f1c270acbc33

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — crates/runtime-nvml 신설, gputeer gpu-probe 신설, x600 원격 실측"
executor_model: "claude-opus-5"
executed_at: "2026-08-29T22:35:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 2라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(CHANGES_REQUESTED)가 진짜 결함 3건을 찾았다 — (a) `workload_run_root` 가 경로 문자열에 접미사를 이어 붙여 `"C:/data/cp/"`·`"."`·`"C:/"` 에서 결과가 체크포인트 루트 안으로 떨어져 `startup_gc()` 의 스캔 대상이 됨, (b) `NAME_BUFFER` 가 64 로 `nvmlDeviceGetName` 의 실제 상한 96 이 아니라 긴 이름 장치 하나가 관측 전체를 실패시킬 수 있음, (c) 작업 디렉터리 삭제가 성공 경로에만 있어 수집·확정 실패 시 제출자 stdout 이 남음. 세 건 전부 수정 뒤 2라운드가 반례 4개의 실제 계산 결과·`Path::file_name()` 의 후행 구분자 무시 전제·신규 테스트의 비공허성·버퍼 상한 3종(96/96/80)·`UnterminatedString` 이 NUL 부재 경로에만 쓰임·모든 `?` 반환 경로가 `remove_dir_if_present` 에 도달함을 코드로 확인하고 기존 빌드 산출물로 신규 테스트 4건을 직접 실행해 ACCEPTED. 검수 환경이 read-only 라 `cargo test --workspace` 자체는 재실행하지 못했음을 스스로 밝혔다"
review_artifact: "docs/evidence/_raw/DoD-56_review_round2.txt"

artifacts:
  - crates/runtime-nvml/src/lib.rs
  - crates/runtime-nvml/src/ffi.rs
  - crates/runtime-nvml/Cargo.toml
  - crates/cli/src/gpu_probe.rs
  - docs/evidence/_raw/DoD-56_nvml_observation_x600_2026-08-29.txt
  - docs/evidence/_raw/DoD-56_review_round1.txt
  - docs/evidence/_raw/DoD-56_review_round2.txt
negative_tests:
  - "missing_nvml_is_an_error_not_an_empty_list: NVML 를 열 수 없는 기계에서 `observe()` 가 빈 목록이 아니라 `NvmlError::LibraryUnavailable` 를 돌려줌을 고정한다 — 개발 기계에서 실제로 그 분기를 타며 통과했다"
  - "mig_states_are_three_not_two: `None`(지원 안 함)·`Some(false)`(꺼짐)·`Some(true)`(켜짐) 세 상태가 `is_partitioned()` 에서 각각 다르게 판정됨을 확인한다 — 앞 둘을 합치면 지원 안 하는 장치를 'MIG 꺼진 정상 장치' 로 단정하게 된다"
  - "개발 기계 gpu-probe: exit 1 + GPU_PROBE_UNKNOWN — 'GPU 0개' 가 아니라 '확인 불가' 로 보고됨을 실행으로 확인"
limitations:
  - "MIG 가 켜진 장치를 실물로 본 적이 없다 — x600 의 4070 SUPER 는 MIG 미지원이라 `Some(true)` 경로는 합성 값 단위 테스트로만 고정했다"
  - "다중 GPU 를 실물로 본 적이 없다 — x600 은 1장이다. UUID 정렬의 실효성은 단위 테스트로만 확인했다"
  - "Linux 에서 한 번도 실행하지 않았다 — `libnvidia-ml.so` 경로는 코드에만 있고 실측이 없다"
  - "used VRAM 의 정확성을 증명하지 않는다 — `nvidia-smi` 와 서로 다른 시점에 조회해 값이 달랐다(283 MiB 대 0 MiB). 같은 순간 동시 조회가 아니므로 이 차이는 결함도 일치도 아니다"
  - "provenance 를 판정하지 않는다 — 읽은 값이 권위 있는 관측인지 정하지 않으며, `scheduler` 의 `ProvenanceGate` 를 채우는 소비자가 아직 없다"
  - "GPU 배치·점유·해제·격리를 하지 않는다 — 조회 전용이다"
  - "핵심 주장에 대한 뮤테이션 검증이 아직 없다 — 다음 라운드 작업이다"
decision: "`crates/runtime-nvml` 을 `Runtime` 스트림의 신규 크레이트로 채택하고, `scheduler` 타입이 아니라 자기 타입을 반환하게 한다 — 변환 지점이 곧 provenance 를 정하는 지점이므로 자동으로 넘어가면 서명·멤버십 검증을 우회하는 뒷문이 된다. 링크가 아니라 `libloading` 런타임 로드를 쓴다 — 링크하면 GPU 없는 기계에서 빌드 자체가 불가능해진다. NVML 부재는 빈 목록이 아니라 typed error 로 둔다 — 'GPU 0개' 와 '확인 불가' 를 구분하지 않으면 스케줄러가 후자를 전자로 착각한다."
raw_output_artifact: "docs/evidence/_raw/DoD-56_nvml_observation_x600_2026-08-29.txt"
raw_output_digest: "sha256:1f5fc95ece606a3ff126bfbc4021cadf0e61ab5c9599ca4991525154c392dcde"
raw_output_bytes: 1168

binary_digests:
  toolchain: "C:\\Users\\playdata2\\.cargo\\bin\\cargo.exe (Rust 1.97.1) — release 빌드 산출물 target/release/gputeer.exe 를 x600 으로 scp 전송해 실행"
protocol_versions:
  schema_version: "proto 변경 없음 — 이 크레이트는 서명 대상 메시지를 만들지도 소비하지도 않는다"
  canonical_spec: "canonical/domain_tag 무관 — 서명·저장·상태 전이를 하지 않는다"
platform: "관측 대상: Windows 11 / x600. 단위 테스트: Windows 11 개발 기계(NVIDIA GPU 없음, Intel Iris Xe)"
hardware: "NVIDIA GeForce RTX 4070 SUPER · driver 595.79 · CUDA driver 13020(13.2) · total 12878610432 bytes(12282 MiB) · compute capability 8.9 · MIG 미지원"
network_profile: "x600 으로의 SSH/SCP 만 사용. 이 크레이트 자체는 네트워크를 쓰지 않는다"
command: |
  # x600 (실물 GPU)
  ssh x600 "F:\gputeer-work\gputeer.exe gpu-probe"
  ssh x600 "nvidia-smi --query-gpu=uuid,name,driver_version,memory.total,memory.used,compute_cap --format=csv"
  # 개발 기계 (GPU 없음)
  cargo run -q -p gputeer-cli -- gpu-probe
  cargo test -p gputeer-runtime-nvml
raw_output: |
  (docs/evidence/_raw/DoD-56_nvml_observation_x600_2026-08-29.txt 전문 참조)

  x600: GPU_PROBE_OK driver_version=595.79 cuda_driver_version=13020 devices=1
        uuid=GPU-09a269a7-50a8-f5be-2a00-d20a1c281c93 total_vram_bytes=12878610432
        compute_capability=8.9 mig=(지원 안 함)
  nvidia-smi 교차 확인: 같은 UUID · 같은 이름 · 595.79 · 12282 MiB · 8.9
  개발 기계: GPU_PROBE_UNKNOWN (exit 1) — 빈 목록이 아니라 오류
  cargo test -p gputeer-runtime-nvml: 2 passed, 0 failed
---

# DoD-56 — NVML 실제 GPU 관측 계층

## 무엇을 채웠는가

`DoD-55` 가 만든 `gpu_scope_candidate()` 는 GPU 관측을 **입력으로 받는**
순수 kernel 이다. 계산은 있는데 그 입력을 실제 하드웨어에서 만드는 곳이
없었다. 이 조각이 그 자리를 채운다.

## 교차 확인 — 무엇이 일치했고 무엇이 다른가

같은 순간이 아닌 두 번의 조회를 비교한 것이므로, 일치한 항목과 다른
항목을 나눠 적는다.

| 항목 | `gpu-probe` | `nvidia-smi` | 판정 |
|---|---|---|---|
| UUID | `GPU-09a269a7-...` | 같음 | 일치 |
| 이름 | NVIDIA GeForce RTX 4070 SUPER | 같음 | 일치 |
| 드라이버 | 595.79 | 595.79 | 일치 |
| total VRAM | 12878610432 bytes | 12282 MiB | 일치(= 12282 MiB) |
| compute capability | 8.9 | 8.9 | 일치 |
| used VRAM | 296747008 bytes(283 MiB) | 0 MiB | **다름** |

★ **used VRAM 이 다른 것을 "일치" 로 세지 않는다.** 두 명령이 서로 다른
시점에 돌았고 그 사이 화면 출력·드라이버 작업으로 VRAM 사용량이 바뀔 수
있다. 이 값은 시간에 따라 변하는 관측치이므로 두 시점의 값이 같아야 할
이유가 없다 — 다만 **이 실측은 used VRAM 의 정확성을 증명하지 않는다.**
증명하려면 같은 순간에 두 방법으로 읽어야 하고, 그건 이 조각의 범위가
아니다.

## 확인되지 않은 것

```text
MIG 켜진 장치의 실제 동작   x600 의 4070 SUPER 는 MIG 미지원이다.
                            Some(true) 경로는 단위 테스트로만 고정했고
                            실물로 확인한 적이 없다
다중 GPU                    x600 은 1장이다. UUID 정렬은 단위 테스트로만
                            확인했고 실물 2장 이상으로 본 적이 없다
Linux                       libnvidia-ml.so 경로는 코드에만 있고
                            Linux 에서 한 번도 실행하지 않았다
provenance                  이 계층은 읽기만 한다. 읽은 값이 권위 있는
                            관측인지는 정하지 않으며, scheduler 의
                            ProvenanceGate 를 채우는 소비자는 아직 없다
GPU 배치·점유·해제          전혀 안 한다
```

## 스스로 찾아 고친 것

첫 구현이 `nvmlDeviceGetBusId` 라는 **NVML 에 존재하지 않는 심볼**로 PCI
bus ID 를 읽으려 했다. x600 실측에서 `(모름)` 으로 나와 드러났다.
`nvmlPciInfo_t` 구조체로 제대로 읽을 수도 있었지만, 그 구조체는 버전마다
레이아웃이 달라 크기를 틀리면 스택을 밟는다. **`scheduler` 의
`ScopeGpuObservation` 에는 PCI 필드 자체가 없어 이 값을 쓰는 소비자가
없으므로**, 위험을 감수하고 읽는 대신 필드를 제거했다.

## 뮤테이션 — 실제로 돌린 2건

둘 다 **독립 검수가 지적한 결함을 되돌려** 그 수정을 고정하는 검사가
진짜로 잡는지 확인한 것이다.

| # | 되돌린 것 | 잡힌 곳 | 실패 문구 |
|---|---|---|---|
| 1 | `workload_run_root` 를 문자열 이어붙이기로 환원 | `workload_run_root_is_never_inside_the_checkpoint_root` | `"C:/data/checkpoints/"` 의 작업 루트가 체크포인트 루트 안에 생겼다 |
| 2 | 작업 디렉터리 삭제를 `Ok(())` 로 무력화 | selftest 시나리오 83 | 작업 디렉터리가 끝난 뒤에도 남아 있다 |

1번은 검수가 제시한 반례를 그대로 재현했다. 둘 다 원복 뒤 통과했다.

## 뮤테이션으로 확인하지 **않은** 것

이 조각의 NVML 쪽 주장들은 뮤테이션으로 검증하지 못했다. 그 검사들은
GPU 없는 개발 기계에서 도는 계약만 고정하므로, 뮤테이션을 넘기려면
매번 x600 으로 바이너리를 보내 돌려야 한다. 즉 다음 세 주장은
**실측 1회**로 받침될 뿐 자동 검사로 고정되지 않았다.

```text
UUID 정렬이 열거 순서에 무관하다      GPU 1장으로는 증명되지 않는다
버퍼 상한 96 이 충분하다               63바이트 넘는 이름을 본 적 없다
MIG 켜짐을 PARTITIONED 로 보고한다    지원 장치가 없어 합성값 단위 테스트뿐
```
