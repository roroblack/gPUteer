---
schema_version: 2
id: ENV-03
claim: "사용자가 임시로 제공한 원격 기계 remote5090 가 실제 네이티브 Linux + NVIDIA GPU 환경인지 실측하고, 이 저장소가 Linux 에서 처음으로 빌드·테스트됐을 때 무엇이 통과하고 무엇이 안 통과하는지 확정한다"
status: PASS
commit: fec5a383f2984e436668746712df254b2c141229

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + ssh + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-19T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high (1·2라운드) / medium (3라운드)"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "frontmatter·본문 수치가 raw 로그와 실제로 일치하는지(GPU 사양, cgroup CPU usage_usec/nr_throttled, 디스크 여유, fork 성공/실패 개수), k1c 서술이 write_once() 실제 구현·테스트 설계와 일치하는지, limitations 의 정직성, P0-06 과의 결론 정합성, decision 의 과장·축소 여부. 1라운드(p124) — CHANGES_REQUESTED(cgroup CPU 수치 오기재·디스크 용량 오기재·다른 로그인 사용자 과장·rustc 버전 대조 근거 부재·ContentMismatch 관측 raw 미기재·P0-06 addendum 의 freezer/workspace-quota 혼동·본문 자기모순 7건). raw 로그 재수집(rustup 설치 확인·로컬 버전 대조·nvcc/torch 부재 확인·k1c 전체 panic 메시지 4회분 포함) + 문서 정정 뒤 2라운드(p125) — CHANGES_REQUESTED(잔여 3건: thaw 후 재개 미측정을 '정상 진행'으로 과장, who/ps 재확인 시점 부정확, rustup 설치 로그 자체 부재를 명확히 구분 안 함). 추가 정정 뒤 3라운드(p126) — ACCEPTED"
review_artifact: "docs/evidence/_raw/ENV-03_review.txt"

raw_output_artifact: "docs/evidence/_raw/ENV-03_remote5090_2026-08-19.txt"
raw_output_digest: "sha256:2c449011e6046d6b892383ce857cd20ae4d46aacec070927c52b3a1369ba71cb"
raw_output_bytes: 23381

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14) — rustup 으로 사용자 권한 설치, 로컬 개발 기계와 버전 동일(raw 로그에서 직접 대조)"
protocol_versions:
  none: "해당 없음 — 환경 실측 + 기존 코드의 크로스플랫폼 빌드/테스트 확인. 프로토콜 변경 없음"
platform: "원격 remote5090 (호스트명 <remote5090-host>) — Ubuntu 24.04.3 LTS / kernel 7.0.0-28-generic / cgroup v2"
hardware: "NVIDIA GeForce RTX 5090 32607MiB (driver 580.173.02, CUDA 13.0) / 8 코어 / RAM 60GB / 디스크 406GB 여유(nvme0n1p2)"
network_profile: "SSH 경유(<remote5090-ssh-domain>, user <remote5090-user>, ~/.ssh/config 의 Host remote5090). 사용자 소유의 개인/작업용 기계를 임시로 빌린 것 — gPUteer 전용 기계가 아니다"
command: |
  ssh remote5090 'hostname; uname -a; nvidia-smi; df -h /; free -h; nproc'
  curl https://sh.rustup.rs | sh -s -- -y --default-toolchain stable   # 사용자 권한, sudo 없이
  git bundle create gputeer.bundle --all && scp gputeer.bundle remote5090:~/
  ssh remote5090 'git clone ~/gputeer.bundle ~/gputeer'
  ssh remote5090 'cargo build --workspace --exclude gputeer-runtime-windows'
  ssh remote5090 'cargo test --workspace --exclude gputeer-runtime-windows --no-fail-fast'
  ssh remote5090 'cargo build -p gputeer-runtime-windows'   # Windows 전용임을 재확인
raw_output: |
  (docs/evidence/_raw/ENV-03_remote5090_2026-08-19.txt 전문 참조)

  호스트: <remote5090-host> / Ubuntu 24.04.3 LTS / Linux 7.0.0-28-generic
  GPU: NVIDIA GeForce RTX 5090, 32607MiB, driver 580.173.02, CUDA 13.0
  sudo: 비밀번호 필요 — 자동 실행 불가
  uptime 은 3명의 로그인 사용자를 보고했다(who 자체는 이 시점에 <remote5090-user> 만 표시 —
  다른 세션이 유동적임을 시사). ps 로 <remote5090-user2> 소유의 next-server, mysql 서비스가
  실행 중임을 직접 확인했다 — 이 기계가 다른 용도로 실사용 중임을 확인했다

  cargo build --workspace --exclude gputeer-runtime-windows
    -> 성공. Finished `dev` profile

  cargo test --workspace --exclude gputeer-runtime-windows --no-fail-fast
    -> gputeer-checkpoint 의 codex_findings.rs 중
       k1c_concurrent_same_name_writers_are_not_actually_safe 만 FAILED
       (5회 재현 시도 전부 FAILED — 8스레드 x 10라운드 동안 ok_true > 1 관측 0회)
    -> 나머지 모든 테스트 바이너리(agent·checkpoint 나머지 8개·coordinator·crypto 13개·
       protocol 12개·runtime-policy) 전부 ok, 0 failed

  cargo build -p gputeer-runtime-windows (단독)
    -> error: gputeer-runtime-windows 는 Windows 전용이다 — Job Object 는 Win32 개념이다
       (crates/runtime-windows/src/lib.rs:445 의 compile_error!() — 의도된 설계)

  tools/probes/linux_cgroup_probe.py (sudo 없이 systemd-run --user --scope 로 위임된
  cgroup v2 컨트롤러 실측):
    memory: 50MB 한도, 200MB 할당 시도 -> SIGKILL (강제됨)
    cpu:    20% quota, 2초 busy-loop -> usage_usec=421896(~21%), nr_throttled=21 (강제됨)
    pids:   TasksMax=5, 15회 fork 시도 -> 정확히 4개 성공(부모+4=5)+11개 거부 (강제됨)
    freezer: freeze 중 CPU tick 불변(99->99) (강제됨) — thaw 이후
             진행 재개는 프로브가 별도로 측정하지 않았다

  작업 종료 후 GPU 재확인: utilization 0%, memory.used 15MiB/32607MiB — 유휴 상태로 복귀
artifacts:
  - docs/evidence/_raw/ENV-03_remote5090_2026-08-19.txt
  - docs/evidence/_raw/ENV-03_review.txt
  - tools/probes/linux_cgroup_probe.py
negative_tests:
  - "runtime-windows 를 단독으로 빌드해 실제로 compile_error 로 거부되는지 확인 — '워크스페이스 빌드가 됐다'는 사실이 '전체 크레이트가 크로스플랫폼이다'를 뜻하지 않음을 구분했다"
  - "k1c_concurrent_same_name_writers_are_not_actually_safe 를 5회(전체 스위트 1회 + 단독 재실행 4회) 반복해 우연한 1회성 결과가 아님을 확인했다 — 매번 동일하게 FAILED(race 미관측)"
  - "테스트 시작 전/후 두 번 nvidia-smi 로 GPU 사용량을 확인해 이 세션의 작업이 다른 사용자에게 관측 가능한 흔적을 남기지 않았음을 확인했다"
  - "다른 사용자 소유 프로세스(mysql, <remote5090-user2> 소유의 next-server)를 ps 로 직접 확인 — '기계가 비어 있다'고 가정하지 않았다. who 는 이 시점 <remote5090-user> 만 표시했지만 uptime 은 3명의 로그인 사용자를 보고해, 다른 세션이 유동적임을 확인했다"
limitations:
  - "★ 이 기계는 사용자 소유의 공유·비영구 기계다. 언제든 꺼지거나 접근 권한이 바뀔 수 있고, gPUteer 전용 CI/검증 서버로 '확보'된 것이 아니다 — 이번 세션 동안 일시적으로 빌려 쓴 것으로 취급한다. 반복 가능한 접근성은 검증하지 않았다"
  - "sudo 가 없어 root 권한이 필요한 검사(cgroup delegate 범위를 벗어난 systemd 서비스 단위 생성, 커널 모듈 등)는 시도하지 않았다"
  - "GPU VRAM 을 cgroup/네임스페이스로 세분 할당(MPS 등)하는 검증은 하지 않았다 — nvcc·torch 등 CUDA 개발 도구가 없고, 공유 기계에 무거운 패키지를 새로 설치하지 않기로 결정했다(사용자 지시)"
  - "gputeer 실제 GPU 실행 경로(Job 실행·스케줄러)는 아직 구현되지 않았으므로, 이 cgroup 실측이 gPUteer 런타임에 실제로 연결됐다는 뜻은 아니다 — 순수 OS 계층 격리 능력 확인이다"
  - "cargo test 실행 중 GPU 자체를 쓰는 테스트는 없다(CUDA 코드가 이 저장소에 아직 없음) — 이번 실측은 '이 기계가 Linux+GPU 라는 사실' 과 '이 저장소가 Linux 에서 빌드/테스트된다는 사실' 을 각각 확인했을 뿐, 둘을 통합 검증한 것은 아니다"
  - "짧은 세션(수십 분) 안의 관측이다 — 장시간 안정성, 재부팅 후 상태, 동시 다중 사용자 부하 하에서의 cgroup 강제는 확인하지 않았다"
decision: "remote5090 를 '반복 가능한 확보 환경'이 아니라 '기회가 됐을 때 쓸 수 있는 임시 Linux+GPU 접근'으로 기록한다. 이 저장소가 처음으로 Linux 에서 빌드·테스트됐고(gputeer-runtime-windows 제외, k1c 한 건만 제외하고 전부 통과), sudo 없이도 cgroup v2 로 memory/CPU/PID/freeze(kill 은 미시도) 4종이 전부 강제됨을 확인했다. 이걸로 CLAUDE.md/계획서가 반복해 온 'Linux 검증 환경이 없다'는 서술은 더 이상 그대로 유지할 수 없지만, D-3 을 완전히 '해소'로 닫지는 않는다 — 반복 접근성과 GPU VRAM 세분 할당 검증이 남아 있다. P0-06 의 Linux cgroup 항목에 이 결과를 addendum 으로 반영한다."
---

# ENV-03 · remote5090 원격 Linux+GPU 기계 실측

## 무엇을 입증하려 했는가

사용자가 `<remote5090-user>@<remote5090-ssh-domain>`(별칭 `remote5090`) 접속 정보를
제공했다. 이 저장소는 처음부터 "v0.1 주 타깃이 Linux 컨테이너
워커인데 한 번도 Linux 에서 돌려본 적이 없다"는 서술을 반복해
왔다(`CLAUDE.md`, `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md`
§5 의 D-3). 이 기계가 실제로 그 공백을 메울 수 있는지, 그리고
이 저장소가 Linux 에서 빌드/테스트될 때 무엇이 드러나는지 실측한다.

## 어떻게 측정했는가

1. SSH 로 접속해 호스트 사양·GPU·개발 도구·다른 사용자/서비스
   존재 여부를 확인했다.
2. `rustup` 으로 Rust 를 **사용자 권한**(sudo 없이)으로 설치했다 —
   설치 직후(raw 는 이 설치 로그 자체는 담지 않고, 설치 뒤 재확인
   시점의 `rustc`/`cargo --version` 출력만 담는다) 버전을 확인하니
   로컬 개발 기계와 동일한 1.97.1 이었다.
3. `git bundle`(1.2MB, `target/` 빌드 산출물 제외)로 저장소를 전송해
   클론했다 — 원격 저장소가 없어 이 방식을 썼다.
4. `cargo build`/`cargo test` 를 `--exclude gputeer-runtime-windows`
   로 실행했다 — 이 크레이트는 어떤 다른 크레이트도 의존하지 않는
   독립 크레이트이고(`grep` 으로 확인), `crates/runtime-windows/src/lib.rs:445`
   의 `compile_error!()` 로 Windows 전용임을 스스로 명시하고 있다.
5. `sudo` 없이 `systemd-run --user --scope` 로 위임되는 cgroup v2
   컨트롤러(memory/cpu/pids/freezer)를 `tools/probes/linux_cgroup_probe.py`
   로 실측했다.

**공유 기계 제약**: 작업 전에 `nvidia-smi`/`who`/`ps` 로 다른
사용량이 있는지 확인했고, 작업 종료 시점에는 `nvidia-smi`/`uptime`
으로 GPU 가 다시 유휴 상태인지만 재확인했다(who/ps 를 종료 시점에
다시 돌리지는 않았다). 무거운 패키지(CUDA 툴체인, PyTorch) 설치는
하지 않았다 — 사용자가 명시적으로 "자원 다 쓰지 말고 다른 작업에
우선순위를 줘라"고 지시했다.

## 결과

### 호스트 — 실제 네이티브 Linux + GPU

| 항목 | 실측 |
|---|---|
| OS | Ubuntu 24.04.3 LTS, kernel 7.0.0-28-generic |
| **GPU** | **NVIDIA GeForce RTX 5090**, 32,607MiB VRAM |
| Driver / CUDA | 580.173.02 / 13.0 |
| CPU / RAM | 8 코어 / 60GB |
| 디스크 | 406GB 여유 |
| sudo | 비밀번호 필요 — 자동 불가 |
| cgroup | v2, 컨트롤러: cpuset·cpu·io·memory·hugetlb·pids·rdma·misc·dmem |
| 다른 사용자·서비스 | `uptime` 3명 로그인 보고(`who` 는 이 시점 `<remote5090-user>` 만 표시), `<remote5090-user2>` 소유 `next-server`·`mysql` 실행 중(`ps` 로 확인) |

### 이 저장소의 첫 Linux 빌드·테스트

```text
cargo build --workspace --exclude gputeer-runtime-windows   -> 성공
cargo test  --workspace --exclude gputeer-runtime-windows   -> 1건만 FAILED
```

`gputeer-runtime-windows` 를 단독으로 빌드하면 의도한 대로
`compile_error!()` 로 거부된다 — Windows 전용 설계가 실제로 지켜지고
있음을 재확인했다.

**FAILED 된 유일한 테스트**:
`crates/checkpoint/tests/codex_findings.rs::k1c_concurrent_same_name_writers_are_not_actually_safe`.
이 테스트는 `write_once()`(`crates/checkpoint/src/atomic.rs:148-206`)가
같은 파일 이름으로 동시에 호출될 때 tmp 경로(`{name}.tmp`, `:166`)를
공유해서 안전하지 않다는 **결함을 고정하는 테스트**다 — "경쟁이
관측되면(`race_observed`) 그 결함이 아직 존재한다"는 역방향
assertion 이다(`:162-174`). 5회(전체 스위트 1회 + 단독 재실행 4회)
반복했지만 8스레드·10라운드 동안 단 한 번도 여러 스레드가 동시에
성공(`ok_true > 1`)하지 않았다 — 이 테스트는 매번 FAILED 했다.

**이것이 "버그가 고쳐졌다"는 뜻은 아니다.** tmp 경로가 호출마다
고유하지 않다는 코드 사실(`atomic.rs:166`)은 전혀 바뀌지 않았다 —
POSIX `rename()` 의 원자적 교체 시맨틱과 Windows 의 그것이 달라
**같은 결함이 플랫폼마다 다른 증상으로 관측된 것**으로 보인다.
Windows 에서는 여러 스레드의 rename 이 겹쳐 보였고(`DoD-08` evidence,
8스레드 중 3회 `Ok(true)` 관측), Linux 에서는 재현 시도 4회 중
raw 에 전체 panic 메시지를 남긴 사례들의 **마지막 라운드** 기준으로
8개 결과 중 대부분(예: 6/8)이 `ContentMismatch`, 나머지가 `Ok(true)`/
`Ok(false)` 각 1건이었다(`docs/evidence/_raw/ENV-03_remote5090_2026-08-19.txt`
의 k1c 섹션 참조 — 매 라운드 전체가 아니라 테스트 코드 구조상
**마지막 라운드만** 패닉 메시지에 남는다). 이 테스트나 `write_once()`
자체를 지금 당장 고치면 안 된다 — 먼저 "동시에 같은 이름으로 쓰는
걸 지원할지 말지"부터 계약으로 정해야 한다.

### cgroup v2 rootless 강제 실측 — 4종 전부 강제됨

`sudo` 없이 `systemd-run --user --scope` 로 위임된 컨트롤러만으로:

```text
memory   50MB 한도, 200MB 할당 시도 -> SIGKILL                    강제됨
cpu      20% quota, 2초 busy-loop -> 실제 소비 421,896us(~21%),
         nr_throttled=21회                                        강제됨
pids     TasksMax=5, 15회 fork 시도 -> 정확히 4개 성공(부모+4=5)   강제됨
freezer  freeze 중 CPU tick 불변(99->99)                          강제됨
```

GPU VRAM 세분 할당(MPS)은 CUDA 개발 도구가 없어 미실측이다.

## 이 실험이 증명하지 "않는" 것

- **remote5090 를 gPUteer 전용 환경으로 "확보"한 게 아니다.** 사용자
  소유의 개인/작업용 기계를 임시로 빌린 것이고, 다른 사용자·서비스가
  이미 돌고 있다. 반복 가능한 접근성은 검증하지 않았다.
- GPU VRAM 을 cgroup/MPS 로 세분 할당하는 검증은 하지 않았다.
- gPUteer 의 실제 GPU 실행 경로(Job 실행·스케줄러)는 아직
  구현되지 않았다 — 이번 cgroup 실측은 순수 OS 계층 격리 능력만
  확인했다. gPUteer 런타임에 연결됐다는 뜻이 아니다.
- k1c 의 결함 자체가 Linux 에서 사라졌다고 결론 내리지 않는다 —
  증상이 재현되지 않았을 뿐이다.
- 장시간 안정성, 재부팅 후 상태, 여러 사용자 동시 부하 하의 cgroup
  강제는 확인하지 않았다.

## 결정

1. `remote5090` 를 "임시 Linux+GPU 접근"으로 기록한다 — "확보"가
   아니다.
2. 이 저장소가 처음으로 Linux 에서 빌드·테스트됐다(`gputeer-runtime-windows`
   제외, k1c 한 건만 제외하고 전부 통과).
3. cgroup v2 로 memory/CPU/PID/freezer 4종이 sudo 없이도 전부
   강제됨을 확인했다 — `docs/evidence/P0-06_vram_enforcement.md`
   에 addendum 으로 반영한다.
4. k1c 결함 고정 테스트는 **지금 손대지 않는다** — 동시 동일-이름
   쓰기를 지원할지 여부를 계약으로 먼저 정해야 한다.
5. D-3 은 "완전 해소"가 아니라 "부분 해소 — 임시 접근 확보,
   반복성·VRAM 검증 미완료"로 갱신한다.

관련: `docs/evidence/P0-06_vram_enforcement.md` ·
`docs/evidence/ENV-02_원격_GPU_기계_실측.md` ·
`docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` §5
