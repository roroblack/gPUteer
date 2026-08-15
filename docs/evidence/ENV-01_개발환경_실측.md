---
id: ENV-01
claim: "이 개발 기계의 툴체인·GPU·파일시스템을 실측하고, 어떤 P0 스파이크가 실행 가능한지 확정한다"
status: PASS
commit: workdir-uncommitted-2026-08-15
binary_digests:
  none: "실행 바이너리 없음 — 환경 조사만 수행"
protocol_versions:
  none: "해당 없음 — 구현 미착수"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS"
hardware: "Intel Iris Xe Graphics (내장, AdapterRAM 1073741824, driver 30.0.100.9805) / NVIDIA GPU 없음"
network_profile: "해당 없음 — 로컬 환경 조사"
command: |
  Get-Command cargo,rustc,rustup,nvidia-smi,nvcc
  Get-CimInstance Win32_VideoController
  Get-CimInstance Win32_OperatingSystem
  Get-Volume
  python --version
  python -c "import blake3; print('ok')"
raw_output: |
  === Get-Command ===
  cargo        NOT FOUND
  rustc        NOT FOUND
  rustup       NOT FOUND
  nvidia-smi   NOT FOUND
  nvcc         NOT FOUND

  === Win32_VideoController ===
  Intel(R) Iris(R) Xe Graphics | AdapterRAM=1073741824 | Driver=30.0.100.9805

  === OS ===
  Microsoft Windows 11 Pro build 26200

  === Filesystem ===
  C: NTFS size=222.4GB

  === Python ===
  Python 3.12.7
  blake3 import: ok
artifacts:
  - docs/evidence/_raw/ENV-01_probe.txt
negative_tests:
  - "표준 설치 경로 3곳을 직접 확인해 PATH 누락 가능성을 배제: ~/.cargo/bin/cargo.exe (False), Program Files/NVIDIA Corporation/NVSMI/nvidia-smi.exe (False), System32/nvidia-smi.exe (False)"
  - "Get-Command 실패를 PATH 문제로 오판하지 않기 위해 WMI 로 하드웨어를 직접 조회 — 물리 GPU 가 Intel 내장 1개뿐임을 확인"
limitations:
  - "WSL2 내부 환경은 조사하지 않았다. WSL2 에 별도 툴체인이 있을 수 있다"
  - "외장 GPU(eGPU) 연결 가능 여부는 조사하지 않았다"
  - "이 결과는 이 기계 한 대에만 해당한다. 팀의 다른 기계는 다를 수 있다"
  - "Linux 검증 환경의 존재 여부는 조사 범위 밖이다"
decision: "GPU 를 요구하는 P0-01/02/06/07 을 ENVIRONMENT-BLOCKED 로 판정. P0-03 을 첫 스파이크로 확정 (파일시스템 문제이므로 GPU 불필요). Rust 툴체인 설치를 사용자 결정 D-1 로 상신"
---

# ENV-01 · 개발 환경 실측

## 무엇을 입증하려 했는가

구현에 착수하기 전에 **이 기계에서 무엇을 검증할 수 있는지** 확정한다.
기준선 §32 의 P0 스파이크 8종 중 어느 것이 실행 가능하고 어느 것이 `ENVIRONMENT-BLOCKED` 인지를
추측이 아니라 실측으로 가른다.

## 어떻게 측정했는가

두 경로로 교차 확인했다.

1. `Get-Command` — PATH 기준 조회
2. **표준 설치 경로 직접 확인** — PATH 누락으로 인한 오판 방지
3. **WMI 하드웨어 직접 조회** — 드라이버 미설치로 `nvidia-smi` 만 없는 경우와,
   물리 GPU 자체가 없는 경우를 구분

3번이 중요하다. `nvidia-smi` 가 없다는 사실만으로는 "드라이버가 없다"인지
"GPU 가 없다"인지 알 수 없다. `Win32_VideoController` 조회 결과 물리 어댑터가
**Intel Iris Xe 하나뿐**이므로 후자로 확정된다.

## 결과

| 항목 | 실측 |
|---|---|
| NVIDIA GPU | **없음** (Intel Iris Xe 내장 1개) |
| CUDA 툴체인 | **없음** (`nvcc` · `nvidia-smi` 부재) |
| Rust 툴체인 | **없음** (`cargo`/`rustc`/`rustup` 전부, `~/.cargo/bin` 미존재) |
| OS | Windows 11 Pro build 26200 |
| 파일시스템 | NTFS (C:, 222.4GB) |
| Python | 3.12.7 + `blake3` 사용 가능 |

### P0 스파이크 실행 가능성

| 스파이크 | 판정 | 사유 |
|---|---|---|
| P0-01 Windows S1 + CUDA | `ENVIRONMENT-BLOCKED` | NVIDIA GPU 없음 |
| P0-02 AppContainer + CUDA | `ENVIRONMENT-BLOCKED` | 동일 |
| **P0-03 Checkpoint durability** | **실행 가능** | **파일시스템 문제. GPU 불필요** (Rust 필요) |
| P0-04 Raft over WAN | `ENVIRONMENT-BLOCKED` | 다중 리전 노드 없음 |
| P0-04b degraded Raft | `ENVIRONMENT-BLOCKED` | 다중 회선 없음 |
| P0-05 Worker P2P NAT | `ENVIRONMENT-BLOCKED` | 서로 다른 실제 회선 2곳 없음 |
| P0-06 VRAM enforcement | `ENVIRONMENT-BLOCKED` | NVIDIA GPU 없음 |
| P0-07 Runtime estimation | `ENVIRONMENT-BLOCKED` | NVIDIA GPU 없음 |

**8종 중 7종이 BLOCKED 다.** `RULE.md` §7.1 에 따라 이들을 `PASS` 로 계상하지 않는다.

## 이 실험이 증명하지 "않는" 것

- **WSL2 내부는 조사하지 않았다.** WSL2 배포판에 별도 툴체인이 있을 수 있다.
- **eGPU 연결 가능성**을 조사하지 않았다.
- 이 결과는 **이 기계 한 대**에만 해당한다. 팀의 다른 기계는 다를 수 있다.
- **Linux 검증 환경의 존재 여부**는 조사 범위 밖이다. v0.1 주 타깃이 Linux 컨테이너 워커이므로
  이것은 별도로 확인해야 한다.

## 결정

1. **P0-03 을 첫 스파이크로 확정한다.** GPU 가 필요 없고, 기준선 §43.6 에서 실패 시
   영향이 가장 큰 항목(`아키텍처 재검토`)이므로 먼저 친다.
2. **P0-03 착수 전에 Windows 파일시스템 원자성을 먼저 조사한다** (`P0-03a`).
   규범 문서가 요구하는 `fsync(dir)` 은 POSIX 개념이라 Windows 에서 성립하는지 확인이 필요하다.
3. **Rust 툴체인 설치를 사용자 결정으로 상신한다** (계획서 §5 D-1).
4. **NVIDIA GPU 환경 확보 방법을 사용자 결정으로 상신한다** (D-2).
   미확보 시 v0.1 게이트를 통과할 수 없다.

관련 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md`
