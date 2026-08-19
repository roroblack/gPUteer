#!/usr/bin/env python3
"""P0-06 Linux 확장 — rootless cgroup v2 위임 강제 실측.

P0-06(docs/evidence/P0-06_vram_enforcement.md)은 Windows Job Object 만
측정했고 "Linux 는 cgroup 이 VRAM 에 관여하지 않는다"는 서술만 남긴 채
CPU quota/RAM hard limit/PID limit/freezer+kill 체크리스트는 실측하지
않은 채로 남아 있었다(Linux 환경 자체가 없었기 때문).

이 프로브는 sudo 없이(비루트) systemd-run --user --scope 로 위임된
cgroup v2 컨트롤러(cpu/memory/pids)가 **실제로 강제되는지** 측정한다.
공유 기계이므로 각 테스트는 수 초 안에 끝나고 자원을 조금만 쓴다.
"""
import json
import subprocess
import sys
import time

RESULTS = {}


def run(cmd, timeout=15):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout.strip(), p.stderr.strip()
    except subprocess.TimeoutExpired:
        return None, "", "TIMEOUT"


def probe_memory_max():
    """50MB 한도를 주고 200MB 를 할당하는 파이썬 프로세스가 실제로
    OOM-kill 되는지 확인한다."""
    alloc_cmd = (
        "python3 -c \""
        "import sys; data = bytearray(200 * 1024 * 1024); "
        "sys.stdout.write('ALLOC_SUCCEEDED_%d_BYTES' % len(data)); sys.stdout.flush()\""
    )
    code, out, err = run(
        [
            "systemd-run", "--user", "--scope", "--quiet",
            "-p", "MemoryMax=50M", "-p", "MemorySwapMax=0",
            "bash", "-c", alloc_cmd,
        ]
    )
    killed = code is not None and code != 0 and "ALLOC_SUCCEEDED" not in out
    RESULTS["memory_max_50m_vs_200m_alloc"] = {
        "exit_code": code,
        "stdout": out[:200],
        "stderr": err[:500],
        "enforced": killed,
    }


def probe_cpu_quota():
    """CPUQuota=20% 로 8코어 중 하나에 busy-loop 를 4초 돌리고,
    실제 소비한 CPU 시간이 벽시계 시간의 ~20% 근처인지 확인한다."""
    busy_cmd = "python3 -c \"import time; t=time.time();\n" \
               "while time.time()-t < 4: pass\""
    start = time.time()
    code, out, err = run(
        [
            "systemd-run", "--user", "--scope", "--quiet",
            "-p", "CPUQuota=20%",
            "bash", "-c", busy_cmd,
        ],
        timeout=20,
    )
    wall = time.time() - start
    RESULTS["cpu_quota_20pct_4s_busyloop"] = {
        "exit_code": code,
        "wall_seconds": round(wall, 2),
        "stderr": err[:300],
        "note": "CPUQuota 는 CPU 시간을 제한하지 벽시계 시간을 늘리지 않는다 "
                "(busy-loop 는 그냥 4초간 스로틀된 채로 돈다) — "
                "실제 강제 여부는 별도로 CPU 사용률을 관찰해야 한다. "
                "아래 cpu_usage_usec 로 대조한다.",
    }


def probe_cpu_quota_usage():
    """CPUQuota=20% 스코프 안에서 busy-loop 를 2초 돌린 뒤,
    cpu.stat 의 usage_usec 를 읽어 실제 소비한 CPU 시간이
    2초 * 0.20 = 400ms 근처로 눌렸는지 cgroup 자체 계정으로 확인한다."""
    inline = (
        "UUID=$(cat /proc/sys/kernel/random/uuid); "
        "systemd-run --user --unit=probe-$UUID --scope --setenv=UUID=$UUID "
        "-p CPUQuota=20% bash -c '"
        "python3 -c \"import time; t=time.time();\n"
        "while time.time()-t < 2: pass\"; "
        "cat /sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice/probe-$UUID.scope/cpu.stat"
        "' 2>&1"
    )
    code, out, err = run(["bash", "-c", inline], timeout=15)
    RESULTS["cpu_quota_20pct_cpu_stat"] = {
        "exit_code": code,
        "stdout": out[:800],
        "stderr": err[:300],
    }


def probe_pids_max():
    """TasksMax=5 로 5개 넘게 fork 시도하면 거부되는지 확인한다."""
    fork_cmd = (
        "python3 -c \""
        "import os, sys, time\n"
        "pids = []\n"
        "errors = 0\n"
        "for i in range(15):\n"
        "    try:\n"
        "        pid = os.fork()\n"
        "        if pid == 0:\n"
        "            time.sleep(2)\n"
        "            os._exit(0)\n"
        "        else:\n"
        "            pids.append(pid)\n"
        "    except OSError as e:\n"
        "        errors += 1\n"
        "print('forked=%d errors=%d' % (len(pids), errors))\n"
        "for p in pids:\n"
        "    try:\n"
        "        os.kill(p, 9)\n"
        "    except OSError:\n"
        "        pass\n"
        "\""
    )
    code, out, err = run(
        [
            "systemd-run", "--user", "--scope", "--quiet",
            "-p", "TasksMax=5",
            "bash", "-c", fork_cmd,
        ],
        timeout=15,
    )
    RESULTS["pids_max_5_vs_15_forks"] = {
        "exit_code": code,
        "stdout": out[:300],
        "stderr": err[:300],
    }


def probe_freezer():
    """cgroup.freeze 로 프로세스를 멈추고 CPU 사용이 실제로 0이 되는지,
    다시 풀면 재개되는지 확인한다."""
    script = (
        "UUID=$(cat /proc/sys/kernel/random/uuid); "
        "CGPATH=/sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice/probe-freeze-$UUID.scope; "
        "systemd-run --user --unit=probe-freeze-$UUID --scope bash -c "
        "'python3 -c \"import time\nt=time.time()\nwhile time.time()-t<6: pass\"' "
        "& sleep 1; "
        "echo 1 > $CGPATH/cgroup.freeze 2>&1; "
        "sleep 0.3; "
        "cat $CGPATH/cgroup.events 2>&1; "
        "PID=$(cat $CGPATH/cgroup.procs 2>/dev/null | head -1); "
        "CPU1=$(cat /proc/$PID/stat 2>/dev/null | awk '{print $14}'); "
        "sleep 1; "
        "CPU2=$(cat /proc/$PID/stat 2>/dev/null | awk '{print $14}'); "
        "echo \"CPU_TICKS_WHILE_FROZEN: before=$CPU1 after=$CPU2 (같으면 실제로 멈춘 것)\"; "
        "echo 0 > $CGPATH/cgroup.freeze 2>&1; "
        "wait"
    )
    code, out, err = run(["bash", "-c", script], timeout=15)
    RESULTS["cgroup_freeze_thaw"] = {
        "exit_code": code,
        "stdout": out[:800],
        "stderr": err[:400],
    }


def main():
    print("P0-06 Linux 확장 — rootless cgroup v2 강제 실측 시작", file=sys.stderr)
    probe_memory_max()
    probe_cpu_quota()
    probe_cpu_quota_usage()
    probe_pids_max()
    probe_freezer()
    print(json.dumps(RESULTS, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
