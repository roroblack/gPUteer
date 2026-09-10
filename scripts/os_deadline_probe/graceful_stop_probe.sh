# 규범은 "체크포인트 만들고 정지" 라고 했다. 시한이 그 여유를 주는가?
#   ★ 곧바로 SIGKILL 이면 체크포인트를 못 만든다. 그러면 규범대로 못 한다.
set -u
W=/mnt/e/gputeer-work/deadline; cd "$W"
export SYSTEMD_PAGER=cat
systemctl stop 'gp-*.scope' 2>/dev/null; systemctl reset-failed 2>/dev/null
rm -f ckpt_*.done; sleep 1

cat > work2.sh <<'WEOF'
TAG="$1"; PAUSE="$2"
save() {
  echo "  [작업] 정지 신호 받음 — 체크포인트 쓴다 (${PAUSE}초 걸림)"
  sleep "$PAUSE"
  echo "saved" > "/mnt/e/gputeer-work/deadline/ckpt_${TAG}.done"
  echo "  [작업] 체크포인트 완료"
  exit 0
}
trap save TERM
i=0; while [ $i -lt 300 ]; do i=$((i+1)); sleep 1; done
WEOF

echo "=== A. 대조군 — 체크포인트가 여유 안에 끝나는 경우 ==="
echo "    시한 4초 · 정지 여유 20초 · 체크포인트 2초"
timeout 60 systemd-run --scope --quiet --collect --unit=gp-ok \
  -p RuntimeMaxSec=4 -p TimeoutStopSec=20 bash work2.sh ok 2
echo "    체크포인트 파일: $([ -f ckpt_ok.done ] && echo '있다 ✔ 규범대로 됐다' || echo '★ 없다')"

echo
echo "=== B. 대조군 — 체크포인트가 여유를 넘기는 경우 ==="
echo "    시한 4초 · 정지 여유 3초 · 체크포인트 15초 (일부러 넘긴다)"
echo "    ★ 이게 없으면 '여유가 있다' 와 '여유가 무한하다' 를 구분 못 한다"
timeout 60 systemd-run --scope --quiet --collect --unit=gp-late \
  -p RuntimeMaxSec=4 -p TimeoutStopSec=3 bash work2.sh late 15
echo "    체크포인트 파일: $([ -f ckpt_late.done ] && echo '★ 있다 — 여유가 안 지켜졌다' || echo '없다 ✔ 여유를 넘기면 강제 종료된다')"

echo
echo "=== C. 신호를 무시하는 작업은 어떻게 되나 ==="
echo "    ★ 협조하지 않는 작업도 결국 죽어야 한다"
cat > work3.sh <<'WEOF'
trap '' TERM
i=0; while [ $i -lt 300 ]; do i=$((i+1)); sleep 1; done
WEOF
S=$(date +%s)
timeout 60 systemd-run --scope --quiet --collect --unit=gp-stub \
  -p RuntimeMaxSec=4 -p TimeoutStopSec=5 bash work3.sh
echo "    걸린 시간: $(( $(date +%s) - S ))초  (4+5=9초 근처면 SIGKILL 로 끝낸 것)"
echo "    남은 프로세스: $(pgrep -cf work3.sh 2>/dev/null || echo 0) 개"

echo
echo "=== D. 비-root 로도 되나 ==="
echo "    ★ 에이전트가 root 가 아닐 수 있다"
id -un
if id nobody >/dev/null 2>&1; then
  setpriv --reuid=nobody --regid=nogroup --clear-groups \
    timeout 30 systemd-run --scope --quiet --unit=gp-nr -p RuntimeMaxSec=3 sleep 30 2>&1 | head -3
  echo "    (위에 권한 오류가 나오면 비-root 에이전트는 이 방법을 못 쓴다)"
else
  echo "    nobody 계정이 없어 못 쟀다"
fi

systemctl stop 'gp-*.scope' 2>/dev/null; systemctl reset-failed 2>/dev/null
pkill -f 'work[23].sh' 2>/dev/null
echo "=== DONE ==="
