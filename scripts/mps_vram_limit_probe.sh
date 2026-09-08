#!/usr/bin/env bash
# ── MPS 로 VRAM 상한이 실제로 걸리는가 (Linux 전용) ──────────────────
#
# `proto/common.proto:168` 이 `GPU_ALLOCATION_MODE_SHARED` 의 조건을
# "Linux+MPS memory limit **확인 시에만**" 이라고 적어 뒀다.
# 그 확인을 한 적이 없다. 이 스크립트가 그것을 잰다.
#
# ★★ **남의 작업이 돌고 있으면 절대 돌리지 마라.**
#   MPS 데몬은 GPU 전역 상태를 바꾼다. 남의 작업을 죽일 수 있다
#   (`CLAUDE.md` §0.1 — 남의 하드웨어를 인질로 잡지 않는다).
#   이 스크립트는 시작할 때 그것을 직접 확인하고, 남의 프로세스가
#   보이면 **아무것도 하지 않고 끝낸다.**
#
# ★ 대조군이 핵심이다. "상한을 걸었더니 실패했다" 만으로는
#   **원래 안 되는 것**과 구분이 안 된다. 상한 없이 같은 할당이
#   성공하는지를 먼저 본다.
#
# 쓰는 법:  bash scripts/mps_vram_limit_probe.sh <상한MiB> <시도MiB>
#   예:     bash scripts/mps_vram_limit_probe.sh 2048 4096
# ────────────────────────────────────────────────────────────────────
set -u

LIMIT_MIB="${1:-2048}"
TRY_MIB="${2:-4096}"

echo "=== 0. 안전 확인 — 남의 작업이 GPU 를 쓰고 있나 ==="
ME="$(id -un)"
BUSY=0
while IFS=, read -r pid mem; do
  pid="$(echo "$pid" | tr -d ' ')"
  [ -z "$pid" ] && continue
  owner="$(ps -o user= -p "$pid" 2>/dev/null | tr -d ' ')"
  echo "  pid=$pid owner=${owner:-?} mem=${mem}"
  if [ -n "$owner" ] && [ "$owner" != "$ME" ]; then
    BUSY=1
  fi
done < <(nvidia-smi --query-compute-apps=pid,used_memory --format=csv,noheader)

if [ "$BUSY" -eq 1 ]; then
  echo
  echo "★ 남의 작업이 GPU 를 쓰고 있다. 아무것도 하지 않고 끝낸다."
  echo "  MPS 데몬은 전역 상태를 바꾸므로 그 작업을 죽일 수 있다."
  exit 3
fi
echo "  (내 것 외에 없음 — 계속)"

PY_PROBE="$(mktemp /tmp/mps_probe_XXXX.py)"
cat > "$PY_PROBE" <<'PYEOF'
import os, sys
try:
    import torch
except Exception as e:
    print(f"PROBE result=no_torch detail={e}")
    sys.exit(2)
mib = int(sys.argv[1])
try:
    # 연속 버퍼 하나로 잡는다 — 조각내면 상한 판정이 흐려진다.
    t = torch.empty(mib * 1024 * 1024 // 4, dtype=torch.float32, device="cuda")
    torch.cuda.synchronize()
    print(f"PROBE result=allocated mib={mib} limit_env={os.environ.get('CUDA_MPS_PINNED_DEVICE_MEM_LIMIT','(none)')}")
    del t
except Exception as e:
    print(f"PROBE result=refused mib={mib} limit_env={os.environ.get('CUDA_MPS_PINNED_DEVICE_MEM_LIMIT','(none)')} detail={type(e).__name__}: {e}")
PYEOF

echo
echo "=== 1. 대조군 — MPS 없이 ${TRY_MIB}MiB 를 잡아 본다 ==="
echo "    (여기서 실패하면 상한 실험 자체가 무의미하다)"
python3 "$PY_PROBE" "$TRY_MIB"

echo
echo "=== 2. MPS 데몬 기동 ==="
nvidia-cuda-mps-control -d && echo "  기동됨" || { echo "  기동 실패"; rm -f "$PY_PROBE"; exit 1; }

cleanup() {
  echo
  echo "=== 4. 정리 — MPS 데몬 종료 ==="
  echo quit | nvidia-cuda-mps-control 2>/dev/null && echo "  종료됨" || echo "  종료 실패 — 손으로 확인하라"
  rm -f "$PY_PROBE"
}
trap cleanup EXIT

echo
echo "=== 3. 상한 ${LIMIT_MIB}MiB 를 걸고 ${TRY_MIB}MiB 를 시도한다 ==="
echo "    상한이 진짜면 refused, 아니면 allocated 가 나온다"
CUDA_MPS_PINNED_DEVICE_MEM_LIMIT="0=${LIMIT_MIB}M" python3 "$PY_PROBE" "$TRY_MIB"

echo
echo "=== 3b. 같은 상한에서 상한보다 **작은** 양은 되는가 ==="
echo "    (되어야 한다. 안 되면 상한이 아니라 그냥 고장난 것이다)"
SMALL=$(( LIMIT_MIB / 2 ))
CUDA_MPS_PINNED_DEVICE_MEM_LIMIT="0=${LIMIT_MIB}M" python3 "$PY_PROBE" "$SMALL"
