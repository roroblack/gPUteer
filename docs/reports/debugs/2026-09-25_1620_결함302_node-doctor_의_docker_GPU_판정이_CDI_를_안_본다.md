# 2026-09-25 16:20 — 결함 302: node-doctor 의 docker GPU 판정이 CDI 를 보지 않는다

## 1. 위치

```text
crates/cli/src/node_doctor.rs  check_container_runtime() — docker 는 `docker info` 의 Runtimes 에 "nvidia" 가 있는지만 본다
```

## 2. 재현

x600 WSL2 · docker 29.7.2 실측(`docs/evidence/_raw/컨테이너_GPU_실측_x600_2026-09-25.txt`):

```text
docker run ... --gpus "device=0" ...
docker: Error response from daemon: failed to discover GPU vendor from CDI: no known GPU vendor found
```

docker 29 는 `--gpus` 를 **CDI 사양**(`/etc/cdi` · `/var/run/cdi`)으로 푼다. `docker info` 는 `CDISpecDirs=["/etc/cdi","/var/run/cdi"]` 를 보였다.

## 3. 실측

- x600: Toolkit 이 없어 Runtimes 에도 nvidia 가 없고 CDI 사양도 없다 — 이 기계에서는 지금 판정(WARN)이 맞았다
- 어긋나는 두 경우는 **예상**이다(Toolkit 을 깐 기계로 재지 않았다):
  ① CDI 사양만 만든 기계(`nvidia-ctk cdi generate`) — `--gpus` 는 되는데 Runtimes 에 nvidia 가 없어 WARN "넘길 수 없다" 로 잘못 말한다
  ② nvidia 런타임만 등록하고 CDI 사양이 없는 docker 29 — Runtimes 에 nvidia 가 있어 OK 인데 `--gpus` 는 위 오류로 실패할 수 있다

## 4. 위험도

- [x] 가용성 저하만 — 점검이 틀린 안내를 준다. Agent 동작은 바뀌지 않는다(실행 확인은 `--container-gpu-probe-image` 가 한다)

## 5. ★ 내가 잘못 보고했던 수치의 정정

없음.

## 6. 조치

- [x] docker 도 CDI 사양 파일을 보고, 런타임 · CDI 중 무엇이 보였는지 적는다. 둘 다 "보였다" 까지일 뿐이라는 문구와 함께
      실행 확인(`--container-gpu-probe-image`)을 권한다
- [ ] Toolkit 을 깐 기계로 ① ② 를 재는 것 — x600 에 Toolkit 을 깔지는 사용자 결정(시스템 패키지 · 외부 저장소 추가)
