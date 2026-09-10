#!/usr/bin/env bash
# ── WSL2 에 GPU 가 보이는가, 그리고 MPS 를 쓸 수 있는가 ──────────────
#
# ★ 이 스크립트가 있는 이유는 **문서가 한 번 거짓말을 했기 때문**이다.
#   `CLAUDE.md` 가 2026-08-30 부터 "GPU 는 WSL 에 노출되지 않는다" 고
#   적어 뒀는데 2026-09-08 에 재보니 거짓이었다. 환경에 대한 주장은
#   `scripts/claims_check.py` 가 못 잡는다 — 그건 **코드**를 grep 하지
#   환경을 재지 않는다. 그래서 환경 주장은 **다시 재는 것**만이 검사다.
#
# ★ MPS 판정은 두 갈래로 잡는다. 하나만 보면 "안 깔린 것" 과
#   "지원 안 하는 것" 을 구분할 수 없다.
#     (가) 드라이버 번들에 바이너리가 있나  (나) compute mode 를 바꿀 수 있나
#
# 쓰는 법 (x600):  E:\gputeer-work\wslgpu.cmd 가 이 파일을 부른다
#   내용:  @echo off
#          wsl -d Ubuntu -e bash /mnt/e/gputeer-work/wsl_gpu_probe.sh
#   ★ `ssh x600 "bash ..."` 로 부르지 마라 — 그건 Windows 의 bash.exe 를
#     부른다(`docs/manuals/작업_환경.md`). x600 원격 실행은 `.cmd` 로만.
# ────────────────────────────────────────────────────────────────────
set -u
SMI=/usr/lib/wsl/lib/nvidia-smi
[ -x "$SMI" ] || SMI="$(command -v nvidia-smi 2>/dev/null || echo /nonexistent)"

echo "=== 1. 여기가 WSL 인가 ==="
uname -a
grep -qi microsoft /proc/version && echo "  -> WSL 이다" || echo "  -> WSL 이 아니다(네이티브 리눅스)"

echo
echo "=== 2. GPU 가 보이는가 ==="
"$SMI" --query-gpu=name,memory.total,driver_version --format=csv,noheader 2>&1
echo "-- GPU 통로 --"; ls -l /dev/dxg /dev/nvidia0 2>&1 | grep -v "No such" || echo "  (없음)"

echo
echo "=== 3. ★ 보이는 것으로 끝내지 않는다 — 실제로 VRAM 을 잡는다 ==="
echo "    (nvidia-smi 만 되고 CUDA 는 안 되는 환경이 실제로 있다)"
python3 - <<'PY' 2>&1 | tail -8
import ctypes, sys
try:
    cu = ctypes.CDLL("libcuda.so.1")
except OSError as e:
    print("PROBE result=no_libcuda detail=%s" % e); sys.exit(0)
def chk(name, rc):
    if rc != 0:
        s = ctypes.c_char_p(); cu.cuGetErrorName(rc, ctypes.byref(s))
        print("PROBE result=failed at=%s rc=%d name=%s" % (name, rc, s.value)); sys.exit(0)
chk("cuInit", cu.cuInit(0))
dev = ctypes.c_int(); chk("cuDeviceGet", cu.cuDeviceGet(ctypes.byref(dev), 0))
ctx = ctypes.c_void_p(); chk("cuCtxCreate", cu.cuCtxCreate_v2(ctypes.byref(ctx), 0, dev))
free, total = ctypes.c_size_t(), ctypes.c_size_t()
chk("cuMemGetInfo", cu.cuMemGetInfo_v2(ctypes.byref(free), ctypes.byref(total)))
p = ctypes.c_void_p()
rc = cu.cuMemAlloc_v2(ctypes.byref(p), ctypes.c_size_t(1024*1024*1024))
if rc == 0: cu.cuMemFree_v2(p)
print("PROBE result=cuda_ok free_mib=%d total_mib=%d alloc_1gib=%s"
      % (free.value//1048576, total.value//1048576, "ok" if rc == 0 else "refused rc=%d" % rc))
PY

echo
echo "=== 4. MPS — (가) 드라이버 번들에 바이너리가 있나 ==="
found=$(ls /usr/lib/wsl/lib 2>/dev/null | grep -i mps; find /usr /opt -maxdepth 4 -name 'nvidia-cuda-mps*' 2>/dev/null)
if [ -n "$found" ]; then echo "$found"; else
  echo "  (없음) — WSL 은 드라이버 유저스페이스를 호스트 Windows 에서 가져온다."
  echo "  그 번들에 MPS 가 없다는 것은 '설치를 빠뜨렸다' 가 아니라 '안 준다' 다."
  echo "  ★ apt 로 리눅스 NVIDIA 드라이버를 깔아 채우지 마라 — NVIDIA 가"
  echo "    WSL 에서 금지하고, 지금 되는 통로가 깨진다."
fi

echo
echo "=== 5. MPS — (나) 전제인 compute mode 를 바꿀 수 있나 ==="
sudo -n "$SMI" -c EXCLUSIVE_PROCESS 2>&1 | head -4
echo "-- 지금 값 --"; "$SMI" --query-gpu=compute_mode --format=csv,noheader 2>&1

echo
echo "=== 6. 같은 원인으로 함께 막히는 것들 ==="
for q in mig.mode.current accounting.mode persistence_mode; do
  printf "  %-20s %s\n" "$q" "$("$SMI" --query-gpu=$q --format=csv,noheader 2>&1 | head -1)"
done
echo "  compute-apps        [$("$SMI" --query-compute-apps=pid,used_memory --format=csv,noheader 2>&1 | tr '\n' ' ')]"
echo
echo "=== DONE ==="
