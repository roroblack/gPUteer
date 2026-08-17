---
id: ENV-02
claim: "SSH 로 접근 가능한 원격 기계 x600 의 GPU·툴체인·가상화를 실측하고, ENV-01 에서 BLOCKED 로 판정한 P0 스파이크 중 어느 것이 해제되는지 확정한다"
status: PASS
commit: 2fd847628b4cfec54cbafc41cb5c4b5a9c79f66a
binary_digests:
  none: "환경 조사만 수행 — 실행 바이너리 없음"
protocol_versions:
  none: "해당 없음 — 구현 미착수"
platform: "원격 x600: Microsoft Windows 11 Pro build 26200 / NTFS · 로컬: Windows 11 Pro build 26200"
hardware: "x600: NVIDIA GeForce RTX 4070 SUPER 12282MiB (driver 595.79, CUDA 13.2, compute_cap 8.9) + AMD Radeon 760M(내장) / AMD Ryzen 5 8600G / RAM 23.1GB"
network_profile: "x600 = 10.20.20.1 (사설 대역, 로컬과 동일 LAN 추정) · runpod-gpu = <external-gpu-public-ip>:<port> (외부, 접속 거부됨)"
command: |
  cat ~/.ssh/config
  ssh -o BatchMode=yes x600 'nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version,compute_cap --format=csv'
  ssh -o BatchMode=yes x600 'nvidia-smi | findstr /C:"CUDA Version"'
  ssh -o BatchMode=yes x600 'powershell -NoProfile -NonInteractive -EncodedCommand <base64>'
  ssh -o BatchMode=yes -o ConnectTimeout=12 runpod-gpu 'echo CONNECTED'
raw_output: |
  === ssh x600 (10.20.20.1, user <x600-user>) ===
  name, memory.total [MiB], memory.free [MiB], driver_version, compute_cap
  NVIDIA GeForce RTX 4070 SUPER, 12282 MiB, 11832 MiB, 595.79, 8.9
  | NVIDIA-SMI 595.79    Driver Version: 595.79    CUDA Version: 13.2 |

  HOST=<x600-hostname>   USER=<x600-user>
  OS=Microsoft Windows 11 Pro build 26200
  GPU: NVIDIA GeForce RTX 4070 SUPER | driver 32.0.15.9579
  GPU: AMD Radeon 760M Graphics (integrated)
  CPU: AMD Ryzen 5 8600G w/ Radeon 760M Graphics
  RAM: 23.1 GB
  HypervisorPresent=True
  Hyper-V Requirements: A hypervisor has been detected.
  wsl --list --verbose : (빈 출력 - 배포판 미설치)
  Python 3.14.2
  DISK  C: NTFS free 8.1/237.5 GB   F: NTFS free 168/953.9 GB
        D: NTFS free 11.1/60.4 GB   E: NTFS free 74.4/195.3 GB
  TOOLS nvidia-smi => C:\WINDOWS\system32\nvidia-smi.exe
        python     => Python314
        git        => C:\Program Files\Git\cmd\git.exe
        wsl        => C:\WINDOWS\system32\wsl.exe
        cargo/rustc/rustup => NOT FOUND
        docker     => NOT FOUND

  === ssh runpod-gpu (<external-gpu-public-ip>:<port>) ===
  ssh: connect to host <external-gpu-public-ip> port <port>: Connection refused
artifacts:
  - docs/evidence/_raw/ENV-02_probe.txt
negative_tests:
  - "runpod-gpu 접속을 시도해 Connection refused 를 확인 — '설정에 있으니 쓸 수 있다'는 가정을 반증했다. 인스턴스가 종료된 상태다"
  - "wsl 명령 존재만으로 WSL2 사용 가능으로 판정하지 않고 `wsl --list --verbose` 를 실행해 배포판이 하나도 없음을 확인"
  - "GPU 를 Win32_VideoController 와 nvidia-smi 두 경로로 교차 확인 — 내장 AMD 와 외장 NVIDIA 가 함께 존재함을 확인"
  - "HypervisorPresent 를 CIM 과 systeminfo 두 경로로 확인"
limitations:
  - "x600 의 SSH 기본 셸이 cmd.exe 라 PowerShell 을 base64 인코딩으로 우회했다. 이 방식이 장기 자동화에 적합한지는 검토하지 않았다"
  - "x600 이 로컬과 동일 LAN 인지 확정하지 않았다. 10.20.20.1 은 사설 대역이나 라우팅 경로를 추적하지 않았다"
  - "x600 의 실사용자(<x600-user>)가 이 기계를 어떤 시간대에 쓰는지 모른다. 장시간 점유 테스트의 가용 시간은 미확인이다"
  - "WSL2 배포판 설치 가능 여부(디스크·권한)는 확인했으나 실제 설치를 시도하지 않았다"
  - "C: 여유 공간이 8.1GB 로 매우 적다. 체크포인트 대용량 테스트는 F: 를 써야 하며, C: 기준 동작은 미검증이다"
  - "runpod-gpu 는 재기동하면 IP·포트가 바뀔 수 있다. 이 조사 시점의 정보는 재사용할 수 없다"
decision: "P0-01·P0-02·P0-06·P0-07 의 ENVIRONMENT-BLOCKED 를 해제하고 x600 을 GPU 검증 기계로 지정. P0-04·P0-04b·P0-05 는 여전히 BLOCKED (독립 네트워크 2곳 이상 필요, runpod 종료). 실행계획 v1 §5 의 결정 D-2 는 선택지 (b) 로 해소"
---

# ENV-02 · 원격 GPU 기계(x600) 실측

## 무엇을 입증하려 했는가

`ENV-01` 에서 이 개발 기계에 NVIDIA GPU 가 없어 **P0 스파이크 8종 중 7종을
`ENVIRONMENT-BLOCKED`** 로 판정했다. 사용자가 SSH 로 접근 가능한 기계(`x600`)의 존재를
알려왔으므로, **어느 BLOCKED 판정이 해제되는지**를 실측으로 확정한다.

## 어떻게 측정했는가

`~/.ssh/config` 에서 두 호스트를 확인했다.

```text
x600         10.20.20.1          user <x600-user>
runpod-gpu   <external-gpu-public-ip>:<port> user root
```

두 곳 모두 접속을 시도했다. **설정에 있다는 사실만으로 사용 가능으로 판정하지 않았다** —
실제로 `runpod-gpu` 는 종료되어 있었다.

x600 의 SSH 기본 셸이 `cmd.exe` 라 PowerShell 스크립트를 **UTF-16LE base64 로 인코딩**해
인용 부호 문제를 우회했다. (1차 시도는 `'driver' is not recognized` 로 실패했다.)

## 결과

### x600 — 사용 가능

| 항목 | 실측 |
|---|---|
| **GPU** | **NVIDIA GeForce RTX 4070 SUPER** |
| **VRAM** | **12,282 MiB** (조사 시점 여유 11,832 MiB) |
| **Driver** | **595.79** |
| **CUDA** | **13.2** |
| **Compute capability** | **8.9** (Ada Lovelace) |
| 보조 GPU | AMD Radeon 760M (내장) |
| CPU | AMD Ryzen 5 8600G |
| RAM | 23.1 GB |
| OS | Windows 11 Pro build 26200 |
| **가상화** | **HypervisorPresent=True** |
| WSL | `wsl.exe` 존재, **배포판 0개** |
| Python | 3.14.2 |
| Rust | **없음** |
| Docker | **없음** |
| 디스크 | C: 8.1GB 여유 / **F: 168GB 여유** |

### runpod-gpu — 사용 불가

```text
ssh: connect to host <external-gpu-public-ip> port <port>: Connection refused
```

인스턴스가 종료된 상태다. RunPod 은 재기동 시 IP·포트가 바뀌므로
**이 조사 시점의 접속 정보는 재사용할 수 없다.**

### BLOCKED 판정 갱신

| 스파이크 | ENV-01 | ENV-02 | 근거 |
|---|---|---|---|
| **P0-01** Windows S1 + CUDA | BLOCKED | **해제** | x600 = Windows 11 + RTX 4070 SUPER |
| **P0-02** AppContainer + CUDA | BLOCKED | **해제** | 동일 |
| P0-03 Checkpoint durability | 실행 가능 | 실행 가능 | 변동 없음 (GPU 무관) |
| P0-04 Raft over WAN | BLOCKED | **BLOCKED 유지** | 독립 네트워크 3곳 필요. 현재 로컬+x600 = 사설 대역 2대 |
| P0-04b degraded Raft | BLOCKED | **BLOCKED 유지** | 동일 |
| P0-05 Worker P2P NAT | BLOCKED | **BLOCKED 유지** | 서로 다른 실제 회선 2곳 필요. runpod 종료 |
| **P0-06** VRAM enforcement | BLOCKED | **부분 해제** | Windows 경로 가능. **Linux/MPS 경로는 여전히 불가** (WSL2 배포판 없음) |
| **P0-07** Runtime estimation | BLOCKED | **해제** | GPU 벤치마크 가능 |

**7건 BLOCKED → 3건 BLOCKED + 1건 부분** 으로 개선되었다.

### 주목할 제약

- **C: 여유가 8.1GB 뿐이다.** 체크포인트 대용량 테스트는 **F: (168GB)** 를 써야 한다.
  경로 선택이 테스트 결과를 바꿀 수 있으므로 evidence 에 사용 드라이브를 명시해야 한다.
- **WSL 배포판이 없다.** S3(Linux 컨테이너) 경로를 쓰려면 설치가 선행되어야 한다.
  가상화는 켜져 있으므로 설치 자체는 가능할 것으로 보인다(미시도).
- **Rust 가 양쪽 기계 모두 없다.** D-1 은 여전히 미해결이다.

## 이 실험이 증명하지 "않는" 것

- **x600 이 로컬과 동일 LAN 인지 확정하지 않았다.** 10.20.20.1 은 사설 대역이지만
  라우팅 경로를 추적하지 않았다. P0-04/05 의 "독립 네트워크" 판정에 영향을 준다.
- **x600 의 가용 시간대를 모른다.** 실사용자(<x600-user>)가 언제 쓰는지 확인하지 않았다.
  장시간 점유가 필요한 테스트(24시간 Raft 등)의 실행 가능성은 미확인이다.
- **WSL2 배포판 설치를 시도하지 않았다.** 디스크·권한상 가능해 보이나 확인하지 않았다.
- **C: 기준 동작을 검증하지 않았다.** F: 를 쓸 계획이므로 C: 저용량 상황은 미측정이다.
- **SSH base64 우회 방식이 장기 자동화에 적합한지 검토하지 않았다.**
- runpod-gpu 는 **종료 상태만 확인**했다. 재기동 시 성능·사양은 알 수 없다.

## 결정

1. **x600 을 GPU 검증 기계로 지정한다.** 실행계획 v1 §5 의 결정 **D-2 를 선택지 (b) 로 해소**.
2. **P0-01 · P0-02 · P0-07 의 BLOCKED 를 해제**하고, P0-06 은 Windows 경로만 해제한다.
3. **P0-04 · P0-04b · P0-05 는 BLOCKED 를 유지한다.** 독립 네트워크가 여전히 없다.
   `RULE.md` §7.1 에 따라 이들을 `PASS` 로 세지 않는다.
4. 작업 디렉터리는 **F: 드라이브**로 한다. C: 여유가 8.1GB 뿐이다.
5. **D-1(Rust 툴체인)은 미해결로 남는다.** 양쪽 기계 모두 없다.

관련: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` §5 · `docs/evidence/ENV-01_개발환경_실측.md`

---

## ★ 이후 변경 (2026-08-18 08:10) — x600 재접속으로 재확인, "Rust 없음" 은 stale

독립 검수(`agent:codex-cli`, read-only)가 재검수했다 — 검수자의
샌드박스는 네트워크가 막혀 있어 x600 에 직접 접속하지 못했고,
그래서 "현재 x600 상태는 확인 안 됨"으로 판정했다. 이 세션은
`~/.ssh/config` 의 `x600` 접속을 이미 이번 세션 중(P0-07 재실측)
써 봤으므로, 재검수가 못 한 실제 재접속 재확인을 직접 했다.

```text
명령: ssh x600 "systeminfo | findstr ..."
      ssh x600 "nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv"
      ssh x600 "rustc --version" / "cargo --version"  (PATH 상)
      ssh x600 "C:\Users\<x600-user>\.cargo\bin\cargo.exe --version"  (직접 경로)

결과 (2026-08-18 08:10):
  OS               Microsoft Windows 11 Pro, 10.0.26200 Build 26200  (evidence 기록과 일치)
  GPU              NVIDIA GeForce RTX 4070 SUPER, driver 595.79, 12282 MiB  (evidence 기록과 일치)
  cargo/rustc(PATH)  미검출
  cargo(.cargo\bin 직접 경로)  cargo 1.97.1 (c980f4866 2026-06-30)  ★ 존재한다
```

### 정정

**"Rust 없음"(`:41`,`:104`) 은 지금 거짓이다** — 그리고 `ENV-01`
과 똑같은 패턴이다: PATH 에 없다는 관측을 "설치 안 됨"으로
잘못 결론지었다. 실제로는 `C:\Users\<x600-user>\.cargo\bin\cargo.exe`
에 cargo 1.97.1 이 있다. `HISTORY.md` 의 "2026-08-16 07:00 — P0
스파이크 3건" 항목이 x600 에서 실제 cargo 빌드·테스트를 수행했다고
기록하는 것과도 일치한다 — 이 evidence 가 그 사실을 반영하지
못한 채 stale 로 남아 있었다.

★ 2026-08-18 08:20 재검수가 지적: 이 인용이 원래 `:60`(frontmatter
`decision` 필드)을 가리켰는데 틀렸다 — 실제 D-1 문장은 `:158`
(limitations 목록)에 있다. `:158` 의 "D-1(Rust 툴체인)은 미해결로
남는다. 양쪽 기계 모두 없다"도 같은 이유로 stale — 양쪽 다 PATH
미등록일 뿐 설치되어 있다.

OS·GPU·드라이버 스펙(`:10`-`:12`)은 지금 재확인해도 그대로다 —
이 evidence 가 쓰인 시점(2026-08-16)과 지금(2026-08-18) 사이에
바뀐 것이 없다.

`base64` 방식 관련 limitation(`:54`)도 stale 이다 — 그 뒤
`scp` 로 파일을 직접 전송하는 방식으로 바뀌었다(이 세션의
P0-07 재실측이 실제로 `scp` 를 썼다, `docs/history/HISTORY.md`
"2026-08-18 06:48" 항목 참조). RunPod 접속정보 재사용 불가
limitation(`:59`)은 지금도 유효하다.

### review_outcome

`CHANGES_REQUESTED` → 위 정정으로 Rust 상태·D-1 결정·base64
limitation 을 반영했다. 원본 YAML 은 당시 기록이므로 고치지
않는다.

★ 2026-08-18 08:20 두 번째 재검수 — `agent:codex-cli` 의 샌드박스는
네트워크가 막혀 x600 접속을 재현하지 못했다(hostname resolution
실패) — 그 사실 자체는 이 문서가 이미 명시하고 있다고 확인했다.
남은 실제 결함은 인용 오류 하나였다: D-1 문장을 `:60`(frontmatter
`decision` 필드)으로 잘못 짚었다 — 실제는 `:158`(limitations
목록). 위에서 고쳤다. 이 세 번째 수정 자체는 아직 재검수를
거치지 않았다.
