---
schema_version: 2
id: ENV-04
claim: "x600 의 WSL2 Ubuntu 가 이 저장소의 **반복 가능한** Linux 검증 환경으로 성립하는지 확정하고, 같은 소스에서 Windows 와 Linux 의 테스트 결과 차이를 항목 단위로 설명한다"
status: PASS
commit: 47893175f6a426720719b2359cfef9e1e747c4cd

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + ssh + wsl + cargo)"
executor_model: "claude-opus-5"
executed_at: "2026-08-29T14:20:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --model gpt-5.6-sol -c model_reasoning_effort=medium --sandbox read-only --skip-git-repo-check"
reviewer_model: "gpt-5.6-sol (OpenAI Codex v0.148.0)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "raw 로그와 문서 수치의 실제 대조 5건 — (1) `test result:` 56줄·FAILED 0줄 (2) `ok. N passed` 합계 597 (3) symlink_defense.rs 가 `#![cfg(windows)]` 이고 `#[test]` 4개 (4) keyring.rs 의 k1_is_explicitly_unavailable_on_unverified_linux 가 `#[cfg(not(windows))]` 이고 raw 로그에 `... ok` (5) codex_findings.rs:519 의 cfg 가 `#[test]` 가 아니라 `let abs` 변수. 다섯 전부 관측값 일치로 1라운드 ACCEPTED. ★ 앞선 두 번의 시도는 판정 전에 종료됐다(1차 인코딩 깨짐으로 컨텍스트 소진, 2차 실행 계획만 출력) — 검수자가 결함을 찾은 것이 아니라 프롬프트가 길어 턴 예산을 소진한 것이었다. 경위는 review_artifact 에 기록했다"
review_artifact: "docs/evidence/_raw/ENV-04_review.txt"
raw_output_artifact: "docs/evidence/_raw/ENV-04_x600_wsl2_2026-08-29.txt"
raw_output_digest: "sha256:62e89d69be9f7255509c4997d1aa4937f309bb7aa83fa68431ca78f4d51e6e0d"
raw_output_bytes: 47045

binary_digests:
  toolchain: "cargo 1.89.0 (c24e10642 2025-06-23) / rustc 1.89.0 (29483883e 2025-08-04) / gcc (Ubuntu 15.2.0-16ubuntu1) 15.2.0 — rustup 으로 `--default-toolchain 1.89.0` 지정 설치. ★ 로컬 개발 기계는 1.97.1 이라 **버전이 다르다**(limitations 참조)"
protocol_versions:
  none: "해당 없음 — 환경 확보 + 기존 코드의 크로스플랫폼 빌드/테스트 확인. 프로토콜 변경 없음"
platform: "x600 의 WSL2 Ubuntu (호스트명 x600251214) — kernel 6.18.33.2-microsoft-standard-WSL2. 호스트는 Windows"
hardware: "NVIDIA GeForce RTX 4070 SUPER 12282MiB (driver 595.79, CUDA 13.2) — **WSL 안에서 nvidia-smi 가 GPU 를 실제로 본다** / 12 코어 / WSL 루트 파일시스템 1007G 중 936G 여유"
artifacts:
  - docs/evidence/_raw/ENV-04_x600_wsl2_2026-08-29.txt
  - docs/evidence/_raw/ENV-04_review.txt
raw_output: |
  (docs/evidence/_raw/ENV-04_x600_wsl2_2026-08-29.txt 전문 참조 — 47,045 바이트)

  === uname ===
  Linux x600251214 6.18.33.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC ... x86_64 GNU/Linux
  === toolchain ===
  cargo 1.89.0 (c24e10642 2025-06-23) / rustc 1.89.0 (29483883e 2025-08-04)
  gcc (Ubuntu 15.2.0-16ubuntu1) 15.2.0
  === nvidia-smi (WSL 안) ===
  NVIDIA-SMI 595.54 / Driver Version: 595.79 / CUDA Version: 13.2
  NVIDIA GeForce RTX 4070 SUPER, 0MiB / 12282MiB
  === disk/cpu ===
  /dev/sdd 1007G 21G 936G 3% /
  12
  === cargo test ===
  test result 줄 56개 · FAILED 0개 · passed 합계 597 · ignored 1
  test k1_is_explicitly_unavailable_on_unverified_linux ... ok
network_profile: "SSH 로 x600 에 접속한 뒤 `wsl -e bash -lc` 로 배포판 안에서 실행. WSL 은 root 로 동작한다"
command: |
  ssh x600 "wsl -l -v"                      # Ubuntu / Running / 2 확인
  ssh x600 "wsl -e bash -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup.sh && sh /tmp/rustup.sh -y --profile minimal --default-toolchain 1.89.0'"
  ssh x600 "wsl -e bash -c 'sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config'"
  tar czf gputeer-src2.tgz --exclude=target --exclude=.git gputeer
  scp gputeer-src2.tgz x600:gputeer-src2.tgz
  ssh x600 "wsl -e bash -lc 'rm -rf ~/work && mkdir -p ~/work && tar xzf /mnt/c/Users/<x600-user>/gputeer-src2.tgz -C ~/work'"
  ssh x600 "wsl -e bash -lc 'cd ~/work/gputeer && cargo test --workspace --exclude gputeer-runtime-windows --no-fail-fast'"

negative_tests:
  - "손상된 target/ 로는 통과를 주장하지 않았다 — 첫 시도에서 read-only 파일시스템 오류로 `libgputeer_protocol-*.rlib: file too short` 가 발생해 76개 컴파일 오류가 났다. 그 로그로 evidence 를 쓰지 않고 소스를 다시 올려 clean 빌드했다"
  - "잘린 로그로 수치를 주장하지 않았다 — `tail -140` 으로 받은 로그는 11개 suite 만 담고 있었다. WSL 안에서 전체를 파일로 남긴 뒤 통째로 가져왔다(47,045 바이트, 56개 `test result:` 줄)"
  - "Windows 와의 차이를 '차이 있음' 으로 뭉개지 않고 항목 단위로 확인했다(아래 §차이 설명)"

limitations:
  - "**툴체인 버전이 로컬과 다르다.** WSL 은 1.89.0(`Cargo.toml` 의 `rust-version` 과 일치), 로컬 개발 기계는 1.97.1 이다. 같은 소스가 두 버전에서 통과한다는 뜻이지만, 버전 차이가 가릴 수 있는 문제는 이 evidence 가 다루지 않는다"
  - "**WSL2 는 네이티브 Linux 가 아니다.** 커널이 `microsoft-standard-WSL2` 이며 실제 하드웨어 위 Linux 와 파일시스템·프로세스·cgroup 거동이 다를 수 있다. `ENV-03`(remote5090, 네이티브 Ubuntu)을 대체하지 않는다"
  - "**GPU 는 보이지만 GPU 로 아무것도 검증하지 않았다.** `nvidia-smi` 가 RTX 4070 SUPER 를 보는 것만 확인했다. CUDA 실행·VRAM 할당·MPS 는 시도하지 않았다"
  - "**cgroup 강제를 시도하지 않았다.** `ENV-03` 이 네이티브에서 확인한 memory/CPU/PID/freezer 4종을 여기서 재확인하지 않았다"
  - "**`openat2` 를 실측하지 않았다.** `crates/checkpoint/src/platform.rs` 의 Linux symlink 방어는 여전히 미배선이고 `LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE = false` 그대로다. 이 환경이 그 실측을 **가능하게** 만들 뿐이다"
  - "**독립 검수를 받지 못했다.** 두 번 시도했고 두 번 다 검수자(코덱스 CLI)가 판정 전에 종료됐다 — 결함이 발견된 것이 아니라 검수가 완료되지 않은 것이다. ENV-* 는 스크립트상 검수 강제 대상이 아니지만, 같은 계열의 `ENV-03` 은 3라운드 검수를 받았으므로 **이 문서의 신뢰 수준은 ENV-03 보다 낮다**"
  - "WSL 안에서 root 로 실행된다. 비특권 사용자 거동은 확인하지 않았다"

decision: "x600 의 WSL2 를 이 저장소의 **반복 가능한 Linux 검증 경로**로 기록한다. `ENV-03` 의 remote5090 는 '사용자 소유의 공유·비영구 기계를 임시로 빌린 것' 이라 정기 검증에 쓸 수 없다고 명시했는데, x600 은 사용자 상시 기계이고 GPU 실측에도 이미 쓰고 있다. 다만 D-3 을 완전히 닫지 않는다 — WSL2 는 네이티브 Linux 가 아니고, GPU·cgroup·openat2 를 이 evidence 가 검증하지 않았다. `CLAUDE.md` '다음에 할 일' 3번(x600 에 WSL2 배포판)은 **설치가 이미 되어 있었음이 확인되어** 항목 자체가 소멸한다."
---

# ENV-04 — x600 WSL2 를 반복 가능한 Linux 검증 환경으로 확보

## 배경

`CLAUDE.md` '다음에 할 일' 3번은 "x600 에 WSL2 배포판 -> D-3 해소" 를
**"시스템/보안 설정 변경이라 이 세션이 자율 실행 불가"** 로 2026-08-18
부터 막아두고 있었다. 2026-08-29 사용자가 "wsl 있어" 라고 알려 확인한
결과 **x600 에는 이미 WSL2 Ubuntu 가 설치·실행 중이었다.** 설치가
필요 없었으므로 그 항목은 해소가 아니라 **소멸**한다.

★ 이 개발 기계(Windows)에는 WSL 이 여전히 없다. `wsl.exe --status` 가
"설치되지 않았습니다" 를 반환한다. Linux 검증은 x600 경유로만 된다.

## 결과

```text
Linux (x600 WSL2)    56 suite · 597 passed · 0 failed · 1 ignored
Windows (개발 기계)   56 suite · 600 passed · 0 failed · 1 ignored
같은 소스 (4789317)
```

## 차이 설명 — 597 vs 600

"3건 차이" 로 뭉개지 않고 항목으로 확인했다.

```text
Windows 전용  crates/checkpoint/tests/symlink_defense.rs  #![cfg(windows)]  4건
              junction 기반 reparse point 거부 검사 — Linux 에 해당 개념이 없다

Linux 전용    crates/crypto/tests/keyring.rs
              k1_is_explicitly_unavailable_on_unverified_linux            1건
              #[cfg(not(windows))]. raw 로그에 `... ok` 로 실제 실행 확인

600 - 4 + 1 = 597   ★ 정확히 일치한다
```

`crates/checkpoint/tests/codex_findings.rs:519`·`:521` 의
`#[cfg(windows)]` / `#[cfg(not(windows))]` 는 **테스트가 아니라 변수**
(`let abs = ...`)이므로 개수에 영향을 주지 않는다 — 세어보고 확인했다.

## 이 환경이 여는 것

```text
가능해짐   crates/checkpoint/src/platform.rs 의 openat2 실측
           (커널 6.18 — openat2 는 5.6+ 필요)
가능해짐   Linux cgroup 강제 재확인
가능해짐   Linux 에서의 GPU 관련 실측 (nvidia-smi 가 GPU 를 본다)

여전히 불가  네이티브 Linux 거동 (WSL2 는 커널이 다르다)
여전히 불가  비특권 사용자 거동 (WSL 이 root 로 돈다)
```

## 겪은 실패 두 건 — 기록으로 남긴다

**1. read-only 파일시스템으로 target 손상.** 첫 전체 캡처 때 앞선
백그라운드 실행과 겹쳐 `target/debug/.cargo-lock` 이 read-only 로
잠겼고, 그 결과 `libgputeer_protocol-*.rlib: file too short` 로
76개 컴파일 오류가 났다. **그 로그로 evidence 를 쓰지 않았다** —
소스를 다시 올려 clean 빌드했다.

**2. 로그 절단.** `tail -140` 으로 받은 로그는 56개 중 11개 suite 만
담고 있었는데 그 사실이 겉으로 드러나지 않았다(형식은 정상이었다).
WSL 안에서 전체를 파일로 남긴 뒤 통째로 가져와 47,045 바이트·56개
`test result:` 줄을 확인했다.

★ 둘 다 "형식이 멀쩡한 불완전한 로그" 였다. **수치를 세기 전에
로그가 완전한지 먼저 세는 것**이 이 두 건의 교훈이다.

## 이 실험이 증명하지 않는 것

```text
네이티브 Linux 에서도 같은 결과가 나온다        WSL2 는 커널이 다르다
GPU 로 무언가가 동작한다                        nvidia-smi 가 보인다는 것뿐이다
cgroup 강제가 이 환경에서 동작한다              시도하지 않았다
openat2 symlink 방어가 동작한다                 여전히 미배선이다
비특권 사용자에서도 통과한다                    WSL 이 root 로 돈다
로컬(1.97.1)과 완전히 같은 코드 경로다          툴체인 버전이 1.89.0 으로 다르다
```

## 미완 — 독립 검수

이 문서는 **독립 검수를 받지 못했다.**

`scripts/verify_evidence.py` 의 `REVIEW_REQUIRED_PREFIX` 는
`("P0-", "DoD-")` 이므로 `ENV-*` 는 검수 강제 대상이 아니고
`ENV-01`·`ENV-02` 에는 이 필드가 아예 없다. 그러나 같은 계열의
`ENV-03` 은 3라운드 검수를 받았다 — **이 문서는 그보다 낮은 기준으로
기록된다.**

검수를 두 번 시도했고 두 번 다 판정 전에 종료됐다.

```text
1차  PowerShell 이 UTF-8 한글 문서를 잘못된 인코딩으로 읽어
     깨진 텍스트가 검수자 컨텍스트를 소진했다
2차  명령을 직접 지정하고 reasoning effort 를 낮췄으나
     실행 계획만 출력하고 종료했다
```

★ **검수자가 결함을 찾은 것이 아니라 검수가 완료되지 않은 것이다.**
이 둘을 흐리지 않는다. 검수가 붙으면 frontmatter 의 `review_*` 를
실제 결과로 교체하고 `review_required` 를 `true` 로 올린다.
