# 운영체제에 시한을 걸면, 띄운 놈과 무관하게 발동하는가
#
# ★ 1차는 내 스크립트 결함으로 멈췄다 — 손자를 무한 루프로 만들어서
#   시한 없는 대조군의 범위가 영영 안 끝났다. 손자에 상한을 준다.
# ★ 어제 교훈: 대조군은 개수가 아니라 **경로가 갈라져야** 뜻이 있다.
#   D 가 그 경로다 — 띄운 놈을 죽여도 시한이 발동하는지.
set -u
W=/mnt/e/gputeer-work/deadline
mkdir -p "$W"; cd "$W"
export SYSTEMD_PAGER=cat

# 먼저 1차에서 남은 것을 치운다
systemctl stop 'gp-*.scope' 2>/dev/null; systemctl reset-failed 2>/dev/null
pkill -f 'deadline/work.sh' 2>/dev/null; sleep 1

cat > work.sh <<'WEOF'
DUR="${1:-60}"
# 손자. ★ 상한을 준다 — 안 주면 시한 없는 대조군이 영영 안 끝난다
( sleep 300 ) &
echo $! > "$2"
trap 'exit 0' TERM INT
i=0; while [ $i -lt "$DUR" ]; do i=$((i+1)); sleep 1; done
echo "WORK: 끝까지 다 돌았다 ($i 초)"
WEOF

run() { timeout 90 systemd-run --scope --quiet --collect "$@"; }

echo "=== 0. systemd ==="
systemctl --version 2>&1 | head -1
[ -d /run/systemd/system ] && echo "  PID 1 이다" || { echo "  ★ 없다"; exit 1; }

echo; echo "=== A. 대조군 — 시한 없이 5초 작업 ==="
echo "    ★ 여기서 안 끝나면 실험이 무의미하다"
S=$(date +%s); run --unit=gp-a bash work.sh 5 /tmp/gc_a.pid; echo "    걸린 시간: $(( $(date +%s) - S ))초"

echo; echo "=== B. 대조군 — 시한 30초, 작업 5초 (시한이 넉넉) ==="
echo "    ★ 시한이 그냥 다 죽이는 게 아님을 보인다"
S=$(date +%s); run --unit=gp-b -p RuntimeMaxSec=30 bash work.sh 5 /tmp/gc_b.pid; echo "    걸린 시간: $(( $(date +%s) - S ))초"

echo; echo "=== C. 본 실험 — 시한 5초, 작업 60초 ==="
S=$(date +%s); run --unit=gp-c -p RuntimeMaxSec=5 bash work.sh 60 /tmp/gc_c.pid; echo "    걸린 시간: $(( $(date +%s) - S ))초   (5초 근처여야 한다)"
echo "    C 의 손자가 남았나: $(kill -0 $(cat /tmp/gc_c.pid 2>/dev/null) 2>/dev/null && echo '★ 살아 있다' || echo '정리됨')"

echo; echo "=== D. ★★ 핵심 — 띄운 놈을 죽여도 시한이 발동하는가 ==="
echo "    이게 '운영체제가 강제' 와 '띄운 놈이 강제' 를 가른다"
setsid bash -c "systemd-run --scope --quiet --collect --unit=gp-d -p RuntimeMaxSec=8 bash $W/work.sh 60 /tmp/gc_d.pid" >/dev/null 2>&1 &
L=$!
sleep 3
echo "    껍데기(pid $L)를 SIGKILL"
kill -9 $L 2>/dev/null; sleep 1
echo "    죽인 직후 작업: $(systemctl is-active gp-d.scope 2>&1)   (active 여야 정상)"
echo "    시한(8초)까지 기다린다..."
sleep 9
echo "    시한 뒤:        $(systemctl is-active gp-d.scope 2>&1)"
echo "    남은 work.sh:   $(pgrep -cf 'work.sh 60' 2>/dev/null || echo 0) 개   (0 이어야 한다)"
echo "    D 의 손자:      $(kill -0 $(cat /tmp/gc_d.pid 2>/dev/null) 2>/dev/null && echo '★ 살아 있다 — 트리를 다 못 죽였다' || echo '정리됨')"

echo; echo "=== 정리 ==="
systemctl stop 'gp-*.scope' 2>/dev/null; systemctl reset-failed 2>/dev/null
pkill -f 'deadline/work.sh' 2>/dev/null
echo "=== DONE ==="
