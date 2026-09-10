set -u
cd /mnt/e/gputeer-work/vramshim
export LD_LIBRARY_PATH=/usr/lib/wsl/lib:${LD_LIBRARY_PATH:-}
echo "=== 0. 빌드 ==="
gcc -shared -fPIC -o vramshim.so vramshim.c -ldl -lpthread 2>&1 && echo "  shim ok" || exit 1
gcc -o alloc_probe alloc_probe.c -L/usr/lib/wsl/lib -lcuda 2>&1 && echo "  probe ok" || exit 1

echo; echo "=== 1. 대조군 A — shim 없이 4GiB 를 잡고 안 푼다 ==="
echo "    ★ 여기서 실패하면 상한 실험 자체가 무의미하다"
./alloc_probe hold 8

echo; echo "=== 2. 대조군 B — shim 켜되 상한 없음 ==="
echo "    ★ shim 이 그냥 다 망가뜨리는 게 아님을 보인다"
LD_PRELOAD=./vramshim.so ./alloc_probe hold 8 2>&1 | grep -v "허용:"

echo; echo "=== 3. 본 실험 — 상한 2048MiB, 4GiB 를 잡고 안 푼다 ==="
LD_PRELOAD=./vramshim.so GPUTEER_VRAM_LIMIT_MIB=2048 ./alloc_probe hold 8 2>&1 | tail -4

echo; echo "=== 4. ★★ 새 대조군 — 잡았다 푸는 것을 8번 (동시 512MiB, 누적 4GiB) ==="
echo "    ★ 이게 1차 판이 빠뜨린 대조군이다."
echo "      상한이 '동시 사용량' 이면 통과해야 한다."
echo "      여기서 막히면 그건 상한이 아니라 정상 작업을 죽이는 것이다"
LD_PRELOAD=./vramshim.so GPUTEER_VRAM_LIMIT_MIB=2048 ./alloc_probe loop 8 2>&1 | tail -4

echo; echo "=== 5. 경계 — 상한 2048 에서 1024MiB 만 잡는다 ==="
LD_PRELOAD=./vramshim.so GPUTEER_VRAM_LIMIT_MIB=2048 ./alloc_probe hold 2 2>&1 | tail -2
echo; echo "=== DONE ==="
