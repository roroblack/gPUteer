#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
P0-06 · VRAM Enforcement Reality Check

기준선 §10.3 / ADR-015 / §32 P0-06.

**무엇을 강제할 수 있고 무엇을 강제할 수 없는지**를 확정한다.

기준선 §10.3 은 다음을 주장한다.

  소비자 GPU 에는 VRAM quota 를 강제할 수단이 없다.
    MIG            데이터센터 GPU 전용
    MPS mem limit  Linux 전용
    컨테이너/Job Object  시스템 RAM 만 제한. VRAM 무관
    PyTorch fraction     워크로드 협조 필요

  -> 그래서 GPU 할당 기본값은 Exclusive 다 (ADR-015)

이 주장이 **반증되면 ADR-015 를 철회하고 §10.2 기본값을 Shared 로 되돌린다.**
따라서 이 스파이크의 목적은 "강제할 수 있는가" 를 **적극적으로 시도해 보는 것**이다.
"안 될 것이다" 를 확인하는 것이 아니라 **되는 방법을 찾으려 애쓰는 것**이 옳은 설계다.
"""
import argparse
import ctypes
import json
import os
import subprocess
import sys
import time

RESULTS = []


def rec(probe, question, result, detail, verdict):
    RESULTS.append(dict(probe=probe, question=question, result=result,
                        detail=detail, verdict=verdict))
    print("[%s] %s" % (probe, question))
    print("    결과: %s" % result)
    for line in str(detail).strip().split("\n"):
        print("    %s" % line)
    print("    판정: %s" % verdict)
    print()


def nvsmi(query):
    try:
        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=" + query,
             "--format=csv,noheader,nounits"],
            capture_output=True, text=True, timeout=20)
        return out.stdout.strip()
    except Exception as e:  # noqa: BLE001
        return "ERR:%s" % e


def used_mib():
    v = nvsmi("memory.used")
    try:
        return int(v.split("\n")[0])
    except Exception:  # noqa: BLE001
        return -1


# ══════════════════════════════════════════════════════════════════
# A. PyTorch 협조적 제한 — set_per_process_memory_fraction
# ══════════════════════════════════════════════════════════════════

FRACTION_TEST = r"""
import torch, sys
total = torch.cuda.get_device_properties(0).total_memory
frac = 0.20
torch.cuda.set_per_process_memory_fraction(frac, 0)
cap = total * frac
print('total_mib', round(total/1048576))
print('cap_mib', round(cap/1048576))
# 상한을 넘겨 할당 시도
try:
    big = torch.empty(int(cap * 1.6 / 2), dtype=torch.float16, device='cuda')
    print('OVER_ALLOC_OK', round(big.numel()*2/1048576))
except RuntimeError as e:
    print('OVER_ALLOC_BLOCKED', str(e)[:120].replace('\n',' '))
"""

FRACTION_BYPASS = r"""
import torch, ctypes, sys
total = torch.cuda.get_device_properties(0).total_memory
torch.cuda.set_per_process_memory_fraction(0.20, 0)
# ★ 워크로드가 협조하지 않는 경우: 같은 프로세스에서 제한을 되돌린다
torch.cuda.set_per_process_memory_fraction(1.0, 0)
try:
    big = torch.empty(int(total * 0.5 / 2), dtype=torch.float16, device='cuda')
    print('RESET_BYPASS_OK', round(big.numel()*2/1048576))
except RuntimeError as e:
    print('RESET_BYPASS_BLOCKED', str(e)[:120].replace('\n',' '))
"""


def probe_pytorch_fraction():
    p = subprocess.run([sys.executable, "-c", FRACTION_TEST],
                       capture_output=True, text=True, timeout=300)
    out = (p.stdout + p.stderr).strip()
    blocked = "OVER_ALLOC_BLOCKED" in out

    p2 = subprocess.run([sys.executable, "-c", FRACTION_BYPASS],
                        capture_output=True, text=True, timeout=300)
    out2 = (p2.stdout + p2.stderr).strip()
    bypassed = "RESET_BYPASS_OK" in out2

    rec("A", "PyTorch set_per_process_memory_fraction 이 VRAM 을 제한하는가",
        "제한 %s / 우회 %s" % ("작동" if blocked else "실패",
                               "가능" if bypassed else "불가"),
        "%s\n\n[우회 시도 — 같은 프로세스에서 fraction 을 1.0 으로 되돌림]\n%s"
        % (out[:500], out2[:400]),
        "협조적 제한만 가능 — 워크로드가 되돌리면 무력화된다 (§10.3 주장 유지)"
        if (blocked and bypassed)
        else ("강제 가능? — §10.3 재검토 필요" if (blocked and not bypassed)
              else "제한 자체가 작동하지 않음"))
    return blocked, bypassed


# ══════════════════════════════════════════════════════════════════
# B. Job Object 메모리 제한이 VRAM 에 영향을 주는가
# ══════════════════════════════════════════════════════════════════

HOLD_VRAM = r"""
import torch, time, sys
mib = int(sys.argv[1]) if len(sys.argv) > 1 else 2048
x = torch.empty(mib * 1048576 // 2, dtype=torch.float16, device='cuda')
x.fill_(1.0)
torch.cuda.synchronize()
print('HELD', mib, flush=True)
time.sleep(60)
"""

JobObjectExtendedLimitInformation = 9
JOB_OBJECT_LIMIT_PROCESS_MEMORY = 0x00000100
JOB_OBJECT_LIMIT_JOB_MEMORY = 0x00000200
CREATE_NO_WINDOW = 0x08000000


class IO_COUNTERS(ctypes.Structure):
    _fields_ = [("ReadOperationCount", ctypes.c_ulonglong),
                ("WriteOperationCount", ctypes.c_ulonglong),
                ("OtherOperationCount", ctypes.c_ulonglong),
                ("ReadTransferCount", ctypes.c_ulonglong),
                ("WriteTransferCount", ctypes.c_ulonglong),
                ("OtherTransferCount", ctypes.c_ulonglong)]


class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
    _fields_ = [("PerProcessUserTimeLimit", ctypes.c_longlong),
                ("PerJobUserTimeLimit", ctypes.c_longlong),
                ("LimitFlags", ctypes.c_uint32),
                ("MinimumWorkingSetSize", ctypes.c_size_t),
                ("MaximumWorkingSetSize", ctypes.c_size_t),
                ("ActiveProcessLimit", ctypes.c_uint32),
                ("Affinity", ctypes.c_size_t),
                ("PriorityClass", ctypes.c_uint32),
                ("SchedulingClass", ctypes.c_uint32)]


class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION),
                ("IoInfo", IO_COUNTERS),
                ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t),
                ("PeakJobMemoryUsed", ctypes.c_size_t)]


def probe_job_object_vram():
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateJobObjectW.restype = ctypes.c_void_p
    k32.OpenProcess.restype = ctypes.c_void_p
    k32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
    k32.AssignProcessToJobObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    k32.SetInformationJobObject.argtypes = [ctypes.c_void_p, ctypes.c_int,
                                            ctypes.c_void_p, ctypes.c_uint32]
    k32.TerminateJobObject.argtypes = [ctypes.c_void_p, ctypes.c_uint]
    k32.CloseHandle.argtypes = [ctypes.c_void_p]

    # ★ 1차 시도에서 RAM 제한을 1GB 로 걸었더니 torch 자체가 뜨지 못해
    #   "VRAM 이 막힌 것" 과 "프로세스가 RAM 때문에 죽은 것" 을 구분할 수 없었다.
    #   torch 가 살 수 있는 RAM(4GB)을 주되, 그보다 큰 VRAM(6GB)을 잡게 한다.
    RAM_LIMIT_MIB = 4096
    VRAM_ALLOC_MIB = 6144

    hjob = k32.CreateJobObjectW(None, None)
    info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
    info.ProcessMemoryLimit = RAM_LIMIT_MIB * 1024 * 1024
    set_ok = bool(k32.SetInformationJobObject(
        hjob, JobObjectExtendedLimitInformation,
        ctypes.byref(info), ctypes.sizeof(info)))

    before = used_mib()
    p = subprocess.Popen([sys.executable, "-c", HOLD_VRAM, str(VRAM_ALLOC_MIB)],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                         text=True, creationflags=CREATE_NO_WINDOW)
    hproc = k32.OpenProcess(0x0100 | 0x0001, False, p.pid)
    assigned = bool(k32.AssignProcessToJobObject(hjob, hproc))

    held = False
    child_lines = []
    t0 = time.time()
    while time.time() - t0 < 120:
        if p.poll() is not None:
            break
        line = p.stdout.readline() if p.stdout else ""
        if line:
            child_lines.append(line.rstrip())
        if "HELD" in line:
            held = True
            break
    time.sleep(3)
    during = used_mib()

    k32.TerminateJobObject(hjob, 1)
    k32.CloseHandle(hjob)
    if hproc:
        k32.CloseHandle(hproc)
    try:
        rest, _ = p.communicate(timeout=30)
        if rest:
            child_lines.extend(rest.strip().split("\n"))
    except subprocess.TimeoutExpired:
        p.kill()
    exit_code = p.poll()

    grew = during - before
    # 판정: RAM 제한보다 큰 VRAM 을 잡았으면 Job Object 가 VRAM 을 지배하지 않는다
    vram_exceeded_ram_limit = held and grew > RAM_LIMIT_MIB

    child_out = "\n".join("      " + l for l in child_lines[-12:]) or "      (출력 없음)"

    if vram_exceeded_ram_limit:
        verdict = "§10.3 주장 확인 — Job Object 의 RAM 제한은 VRAM 을 지배하지 않는다"
    elif held:
        verdict = ("PARTIAL — VRAM 을 잡았으나 RAM 제한(%dMiB)을 넘지 못했다"
                   % RAM_LIMIT_MIB)
    else:
        verdict = ("INCONCLUSIVE — 자식이 HELD 에 도달하지 못했다. "
                   "'VRAM 이 막힌 것' 인지 '프로세스가 RAM 때문에 죽은 것' 인지 "
                   "구분되지 않는다 (아래 자식 출력 참조)")

    rec("B", "Job Object 의 메모리 제한이 VRAM 할당을 막는가",
        "RAM 제한 %dMiB / VRAM %dMiB 요청 / 실측 증가 %dMiB"
        % (RAM_LIMIT_MIB, VRAM_ALLOC_MIB, grew),
        "SetInformationJobObject=%s  AssignProcessToJobObject=%s\n"
        "HELD 도달=%s  exit_code=%s\n"
        "VRAM used  before=%d  during=%d  (증가 %d MiB)\n"
        "자식 프로세스 출력:\n%s"
        % (set_ok, assigned, held, exit_code, before, during, grew, child_out),
        verdict)
    return vram_exceeded_ram_limit


# ══════════════════════════════════════════════════════════════════
# C. 외부 프로세스 VRAM 관측 (기준선 §11.2 owner presence)
# ══════════════════════════════════════════════════════════════════

def probe_external_visibility():
    before = used_mib()
    p = subprocess.Popen([sys.executable, "-c", HOLD_VRAM, "2048"],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                         text=True, creationflags=CREATE_NO_WINDOW)
    held = False
    t0 = time.time()
    while time.time() - t0 < 90:
        if p.poll() is not None:
            break
        if "HELD" in (p.stdout.readline() if p.stdout else ""):
            held = True
            break
    time.sleep(3)

    # nvidia-smi 로 프로세스별 사용량이 보이는가
    try:
        proc_out = subprocess.run(
            ["nvidia-smi", "--query-compute-apps=pid,used_memory",
             "--format=csv,noheader,nounits"],
            capture_output=True, text=True, timeout=20).stdout.strip()
    except Exception as e:  # noqa: BLE001
        proc_out = "ERR:%s" % e

    during = used_mib()
    visible = str(p.pid) in proc_out
    # ★ PID 목록이 보이는 것과 프로세스별 사용량이 보이는 것은 다르다.
    #   WDDM 모드의 GeForce 는 used_memory 를 [N/A] 로 준다.
    per_proc_bytes = visible and "[N/A]" not in proc_out and "N/A" not in proc_out

    p.kill()
    p.wait(timeout=20)
    time.sleep(4)
    after = used_mib()

    # ★ PID 목록이 보이는 것과 프로세스별 사용량이 보이는 것은 다르다.
    #   WDDM 모드의 GeForce 는 used_memory 를 [N/A] 로 준다.
    per_proc_bytes = visible and "N/A" not in proc_out

    rec("C", "외부 프로세스의 VRAM 사용량을 관측할 수 있는가 (§11.2)",
        "PID 목록 %s / 프로세스별 사용량 %s"
        % ("보임" if visible else "안 보임",
           "보임" if per_proc_bytes else "**[N/A]**"),
        "HELD=%s\nVRAM(전체)  before=%d  during=%d  after=%d  (증가 %d)\n"
        "nvidia-smi --query-compute-apps (대상 pid=%d):\n%s\n"
        "→ 전체 사용량(memory.used)은 얻을 수 있으나 프로세스별 분해는 %s"
        % (held, before, during, after, during - before, p.pid,
           proc_out if proc_out else "(빈 출력)",
           "가능하다" if per_proc_bytes
           else "불가능하다 (WDDM 모드 GeForce 제약)"),
        "PASS — 프로세스별 VRAM 을 직접 얻을 수 있다" if per_proc_bytes else
        "PARTIAL — 전체 사용량만 얻을 수 있다. §10.5 의 external_process_vram 은 "
        "'전체 - gPUteer 자신의 할당' 으로 간접 산출해야 한다")
    return visible, per_proc_bytes


# ══════════════════════════════════════════════════════════════════
# D. MIG / MPS 지원 여부
# ══════════════════════════════════════════════════════════════════

def probe_mig_mps():
    mig = nvsmi("mig.mode.current")
    name = nvsmi("name")
    try:
        mps_ps = subprocess.run(["where", "nvidia-cuda-mps-control"],
                                capture_output=True, text=True, timeout=15)
        mps_present = mps_ps.returncode == 0
        mps_path = mps_ps.stdout.strip()
    except Exception:  # noqa: BLE001
        mps_present, mps_path = False, ""

    mig_supported = mig not in ("", "N/A", "[N/A]") and not mig.startswith("ERR")

    rec("D", "MIG / MPS 로 하드웨어 수준 분할이 가능한가",
        "MIG %s / MPS %s" % ("지원" if mig_supported else "미지원",
                             "있음" if mps_present else "없음"),
        "GPU: %s\nnvidia-smi mig.mode.current = %r\n"
        "nvidia-cuda-mps-control: %s\n"
        "(MPS 는 Windows 미지원. MIG 는 데이터센터 GPU 전용)"
        % (name, mig, mps_path if mps_present else "없음"),
        "§10.3 주장 확인 — 소비자 GPU 에 하드웨어 분할 수단 없음"
        if not mig_supported and not mps_present
        else "§10.3 재검토 필요")
    return mig_supported, mps_present


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.parse_args()
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:  # noqa: BLE001
        pass

    print("P0-06 · VRAM Enforcement Reality Check")
    print("GPU: %s" % nvsmi("name"))
    print("driver: %s" % nvsmi("driver_version"))
    print("=" * 70)
    print()

    probe_pytorch_fraction()
    probe_job_object_vram()
    probe_external_visibility()
    probe_mig_mps()

    print("=" * 70)
    print("JSON_BEGIN")
    print(json.dumps(RESULTS, ensure_ascii=False, indent=2))
    print("JSON_END")
    return 0


if __name__ == "__main__":
    sys.exit(main())
