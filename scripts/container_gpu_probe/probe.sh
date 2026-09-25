#!/bin/sh
# 컨테이너에 GPU 를 넘기는 경로(--container-gpu)를 실제 런타임으로 잰다 — x600 의 WSL2 에서 돌리려고 만들었다(④ · 2026-09-25).
#
# 쓰는 법 (WSL 안에서):
#   sh probe.sh <결과 폴더> [이미지(기본 ubuntu:24.04)] [gputeer 리눅스 실행 파일(있으면 node-doctor 점검도)]
#
# 지키는 것
#   ★ 결과는 <결과 폴더> 에만 쓴다 — x600 에서는 /mnt/e/gputeer-work/... (E: · C: 금지)
#   ★ 사용자의 컨테이너 · 이미지 · 볼륨을 건드리지 않는다: 만드는 컨테이너는 전부 `gputeer-probe-*` 이름 · --rm 이고,
#     이미지는 **없던 것일 때만** 받고 끝나면 지운다. 전역 정리(prune)를 하지 않는다. 시작과 끝의 컨테이너 수를 적는다
#   ★ 격리 옵션은 Agent 의 create_args(crates/agent/src/container.rs)와 같다 — GPU 인자만 바꿔 가며 잰다
set -u
OUT_DIR=${1:?결과 폴더를 준다}
IMAGE=${2:-ubuntu:24.04}
GPUTEER=${3:-}
mkdir -p "$OUT_DIR"
LOG="$OUT_DIR/container_gpu_probe_$(date +%Y%m%d_%H%M%S).txt"
exec >"$LOG" 2>&1
echo "== 시각 $(date '+%Y-%m-%d %H:%M:%S %z') · 이미지 $IMAGE"
uname -srm
RT=$(command -v docker || true)
[ -n "$RT" ] || { echo "RESULT ENVIRONMENT-BLOCKED docker 없음"; exit 2; }
echo "== docker"; docker version --format 'client {{.Client.Version}} · server {{.Server.Version}}'
docker info --format 'runtimes={{json .Runtimes}} default={{.DefaultRuntime}} security={{json .SecurityOptions}} cdi={{json .CDISpecDirs}}' 2>&1
echo "== toolkit"; (nvidia-ctk --version 2>&1 || echo "nvidia-ctk 없음") | head -n 3
ls -l /etc/cdi /var/run/cdi 2>&1
echo "== WSL 에서 보이는 GPU"; nvidia-smi -L 2>&1
BEFORE=$(docker ps -aq | wc -l); echo "== 시작할 때 컨테이너 수 $BEFORE (건드리지 않는다)"

HAD_IMAGE=; docker image inspect "$IMAGE" >/dev/null 2>&1 && HAD_IMAGE=1
[ -n "$HAD_IMAGE" ] || docker pull -q "$IMAGE"
PINNED=$(docker image inspect --format '{{index .RepoDigests 0}}' "$IMAGE" 2>/dev/null)
echo "== 고정 참조 $PINNED (원래 있던 이미지: ${HAD_IMAGE:-아니오})"

HARDEN="--read-only --tmpfs=/tmp --cap-drop=ALL --security-opt=no-new-privileges --network=none --ipc=private --pids-limit=4096 --memory=1073741824 --memory-swap=1073741824 --workdir=/tmp --user=$(id -u):$(id -g) --label=gputeer.managed=1 --label=gputeer.probe=1"
probe() {
    label=$1; shift
    name="gputeer-probe-$label-$$"
    echo "== [$label] docker run $HARDEN $* --entrypoint nvidia-smi $PINNED -L"
    # shellcheck disable=SC2086
    docker run --rm --name "$name" $HARDEN "$@" --entrypoint nvidia-smi "$PINNED" -L
    echo "RESULT [$label] exit=$?"
}
probe gpus-quoted --gpus '"device=0"'          # 결함 300 수정 뒤 Agent 가 넘기는 모양
probe gpus-plain --gpus device=0               # 수정 전 모양(한 장이면 같은 뜻이어야 한다)
probe no-gpu                                   # 대조 — GPU 를 안 넘기면 안에서 못 봐야 한다
probe gpus-two --gpus '"device=0,1"'           # 한 장 기계 — 없는 1번을 달라면 거부돼야 한다(여러 장 모양의 문법 확인)
probe gpus-all --gpus all                      # 번호 없이 전부 — WSL 의 CDI 사양이 이것만 줄 수 있다
if [ -d /etc/cdi ] || [ -d /var/run/cdi ]; then
    echo "== CDI 장치 목록"; (nvidia-ctk cdi list 2>&1 || true)
    probe cdi --device=nvidia.com/gpu=0        # CDI 경로(podman 이 쓰는 모양 — docker 25+ 도 받는다)
    probe cdi-all --device=nvidia.com/gpu=all  # WSL 의 CDI 사양은 번호별 장치 없이 all 하나다(2026-09-25 x600)
fi

if [ -n "$GPUTEER" ]; then
    echo "== node-doctor --container-gpu-probe-image (Agent 의 실제 create 인자)"
    NODE_DIR="$OUT_DIR/doctor-node"; mkdir -p "$NODE_DIR"
    SEED="$OUT_DIR/doctor.seed"; [ -e "$SEED" ] || "$GPUTEER" keygen --out "$SEED"
    "$GPUTEER" node-doctor --seed-file "$SEED" --node-dir "$NODE_DIR" --connect 127.0.0.1:9 --gpu-pin 0 \
        --container-runtime "$RT" --container-runtime-kind docker --container-gpu-probe-image "$PINNED"
    echo "RESULT [node-doctor] exit=$? (coordinator_tcp FAIL 은 의도 — 여기서는 Coordinator 를 띄우지 않는다)"
fi

[ -n "$HAD_IMAGE" ] || { docker image rm "$IMAGE" >/dev/null && echo "== 받았던 이미지를 지웠다 $IMAGE"; }
AFTER=$(docker ps -aq | wc -l); echo "== 끝날 때 컨테이너 수 $AFTER (시작 $BEFORE)"
echo "LOG $LOG"
