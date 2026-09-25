# 2026-09-25 17:10 — 결함 303: WSL2 docker 29 에서 Agent 의 컨테이너 GPU 인자가 전부 거부된다

## 1. 위치

```text
crates/agent/src/container.rs  gpu_args() — docker 는 `--gpus "device=<n>"`, podman 은 `--device=nvidia.com/gpu=<n>`
crates/agent/src/lib.rs        parse_container_runtime() — GPU 를 어떻게 청할지 고를 수 없다(런타임 종류로 고정)
```

## 2. 재현 (실측)

x600 WSL2 · docker 29.7.2 · NVIDIA Container Toolkit 1.19.0 설치 직후(사용자 허가 · `nvidia-ctk cdi generate` 만 · daemon.json 은 그대로 ·
docker 재시작 없음). 격리 옵션은 Agent 의 create_args 와 같다. `docs/evidence/_raw/컨테이너_GPU_실측_x600_2026-09-25.txt` 3회차.

```text
--gpus "device=0" · --gpus device=0 · --gpus "device=0,1" · --gpus all   exit 125  AMD CDI spec not found
--device=nvidia.com/gpu=0                                                 exit 125  unresolvable CDI devices nvidia.com/gpu=0
--device=nvidia.com/gpu=all                                               exit 0    GPU 0: NVIDIA GeForce RTX 4070 SUPER
(대조) GPU 안 넘김                                                        exit 127  nvidia-smi 없음
nvidia-ctk cdi list                                                       nvidia.com/gpu=all  (장치 1개 — 번호별 이름이 없다)
node-doctor --container-gpu-probe-image (Agent 인자)                      FAIL  AMD CDI spec not found
```

## 3. 실측

각 모양 1회(결정적 거부 · 성공이라 반복 편차를 잴 대상이 아니다). 한 기계 · 한 docker 판이다.
- docker 29 의 `--gpus` 는 nvidia 런타임 등록(`nvidia-ctk runtime configure` → daemon.json 변경 + docker 재시작)이 있어야 할 수 있다 —
  **재지 않았다**(재시작하면 사용자 컨테이너가 영향을 받아 하지 않았다). "AMD" 가 나오는 이유도 확인하지 않았다(docker 의 제조사 추측으로 보인다 — 예상)
- WSL 의 CDI 사양은 GPU 를 번호로 나누지 않는다 — WSL 은 호스트 드라이버의 dxg 장치 하나로 GPU 를 준다(예상 · 생성 로그가 `/usr/lib/wsl` 만 골랐다)

## 4. 위험도

- [x] 가용성 저하만 — 이 조합의 노드에서 모든 컨테이너 GPU Job 이 start 에서 실패한다(ACK 뒤 실행 실패 · 호스트로 새지는 않는다)

## 5. ★ 내가 잘못 보고했던 수치의 정정

없음. 결함 302 에서 "CDI 사양이 있으면 docker 29 의 --gpus 가 된다" 쪽으로 읽힐 문장을 썼다 — **아니었다**(CDI 사양이 있어도 --gpus 는 거부됐다).
결함 302 문서의 ① 은 틀린 예상이었다.

## 6. 조치

- [x] Agent 에 GPU 를 청하는 방식을 고르게 한다 — `--container-gpu-request gpus|cdi|cdi-all`(기본: docker=gpus · podman=cdi — 전과 같다)
  - `cdi`     장치마다 `--device=nvidia.com/gpu=<n>`(docker 25+ · podman)
  - `cdi-all` `--device=nvidia.com/gpu=all` — **GPU 가 한 장인 노드에서만** 받는다: `--gpu-pin 0` 이고 NVML 이 GPU 를 정확히 한 장 볼 때만
    시작한다(여러 장이면 고정이 새어 다른 GPU 도 넘어간다 · NVML 을 못 열면 모르니 거부)
- [x] node-doctor 도 같은 선택을 받아 실행 점검에 쓴다
- [ ] 멀티 GPU WSL 노드 — 번호별 CDI 가 없으니 `cdi-all` 로는 못 나눈다. 재지 못했다(GPU 한 장 기계뿐)
