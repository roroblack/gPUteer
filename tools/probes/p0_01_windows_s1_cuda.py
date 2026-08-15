#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
P0-01 · Windows S1 (Restricted Native) + CUDA 스파이크

기준선 §9.2 / §32 P0-01.

핵심 질문: **Restricted Token 아래에서 CUDA 가 동작하는가?**

S1 은 v0.2 의 필수 경로이고, P0-01 이 실패하면 ADR-005 를 수정하고
Windows S1 을 로드맵에서 제거해야 한다 (기준선 §43.6).

측정 항목
  A. baseline        일반 토큰에서 CUDA 동작 (대조군)
  B. restricted      Restricted Token 에서 CUDA 동작    <- 핵심
  C. job object      프로세스 트리 강제 종료 + VRAM 반환
  D. filesystem      Restricted 프로세스의 호스트 파일 접근 차단
"""
import ctypes
import ctypes.wintypes as w
import json
import os
import subprocess
import sys
import tempfile
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


k32 = ctypes.WinDLL("kernel32", use_last_error=True)
adv = ctypes.WinDLL("advapi32", use_last_error=True)

# ★ 64비트에서 restype 을 지정하지 않으면 HANDLE 이 c_int 로 잘린다.
#   GetCurrentProcess() 의 의사 핸들 (HANDLE)-1 이 0xFFFFFFFF 로 truncate 되어
#   OpenProcessToken 이 ERROR_INVALID_HANDLE(6) 로 실패한다.
#   1차 실행에서 이 함정에 빠져 "토큰 조작 불가" 로 오판했다.
k32.GetCurrentProcess.restype = w.HANDLE
k32.GetCurrentProcess.argtypes = []
k32.CreateJobObjectW.restype = w.HANDLE
k32.OpenProcess.restype = w.HANDLE
k32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
k32.AssignProcessToJobObject.argtypes = [w.HANDLE, w.HANDLE]
k32.TerminateJobObject.argtypes = [w.HANDLE, ctypes.c_uint]
k32.CloseHandle.argtypes = [w.HANDLE]
k32.WaitForSingleObject.argtypes = [w.HANDLE, w.DWORD]
k32.GetExitCodeProcess.argtypes = [w.HANDLE, ctypes.POINTER(w.DWORD)]

adv.OpenProcessToken.argtypes = [w.HANDLE, w.DWORD, ctypes.POINTER(w.HANDLE)]
adv.OpenProcessToken.restype = w.BOOL
adv.CreateRestrictedToken.argtypes = [
    w.HANDLE, w.DWORD,
    w.DWORD, ctypes.c_void_p,
    w.DWORD, ctypes.c_void_p,
    w.DWORD, ctypes.c_void_p,
    ctypes.POINTER(w.HANDLE)]
adv.CreateRestrictedToken.restype = w.BOOL
adv.CreateProcessAsUserW.restype = w.BOOL

TOKEN_DUPLICATE = 0x0002
TOKEN_QUERY = 0x0008
TOKEN_ASSIGN_PRIMARY = 0x0001
TOKEN_ADJUST_DEFAULT = 0x0080
TOKEN_ADJUST_SESSIONID = 0x0100
DISABLE_MAX_PRIVILEGE = 0x1
CREATE_NO_WINDOW = 0x08000000
CREATE_SUSPENDED = 0x00000004
CREATE_BREAKAWAY_FROM_JOB = 0x01000000


class STARTUPINFOW(ctypes.Structure):
    _fields_ = [("cb", w.DWORD), ("lpReserved", w.LPWSTR), ("lpDesktop", w.LPWSTR),
                ("lpTitle", w.LPWSTR), ("dwX", w.DWORD), ("dwY", w.DWORD),
                ("dwXSize", w.DWORD), ("dwYSize", w.DWORD), ("dwXCountChars", w.DWORD),
                ("dwYCountChars", w.DWORD), ("dwFillAttribute", w.DWORD),
                ("dwFlags", w.DWORD), ("wShowWindow", w.WORD), ("cbReserved2", w.WORD),
                ("lpReserved2", ctypes.POINTER(ctypes.c_byte)),
                ("hStdInput", w.HANDLE), ("hStdOutput", w.HANDLE), ("hStdError", w.HANDLE)]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", w.HANDLE), ("hThread", w.HANDLE),
                ("dwProcessId", w.DWORD), ("dwThreadId", w.DWORD)]


STARTF_USESTDHANDLES = 0x00000100

CUDA_TEST = (
    "import torch,sys;"
    "ok=torch.cuda.is_available();"
    "print('cuda_available',ok);"
    "n=torch.cuda.device_count() if ok else 0;"
    "print('device_count',n);"
    "print('device_name',torch.cuda.get_device_name(0) if ok and n else 'NONE');"
    "x=torch.randn(512,512,device='cuda') if ok else None;"
    "y=(x@x).sum().item() if ok else None;"
    "print('matmul_ok',y is not None);"
    "print('vram_alloc_mb',round(torch.cuda.memory_allocated()/1048576,2) if ok else 0)"
)


def nvidia_used_mib():
    try:
        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"],
            capture_output=True, text=True, timeout=20)
        return int(out.stdout.strip().split("\n")[0])
    except Exception as e:  # noqa: BLE001
        return -1


# ══════════════════════════════════════════════════════════════════
# A. baseline
# ══════════════════════════════════════════════════════════════════

def probe_baseline():
    p = subprocess.run([sys.executable, "-c", CUDA_TEST],
                       capture_output=True, text=True, timeout=180)
    ok = "cuda_available True" in p.stdout
    rec("A", "일반 토큰에서 CUDA 가 동작하는가 (대조군)",
        "동작" if ok else "실패",
        (p.stdout + p.stderr).strip()[:900],
        "PASS — 대조군 확보" if ok else "BLOCKER — 대조군부터 실패. 환경 문제")
    return ok


# ══════════════════════════════════════════════════════════════════
# B. Restricted Token — P0-01 의 핵심
# ══════════════════════════════════════════════════════════════════

def _make_restricted_token():
    cur = w.HANDLE()
    access = (TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY
              | TOKEN_ADJUST_DEFAULT | TOKEN_ADJUST_SESSIONID)
    if not adv.OpenProcessToken(k32.GetCurrentProcess(), access, ctypes.byref(cur)):
        return None, "OpenProcessToken 실패 err=%d" % ctypes.get_last_error()

    restricted = w.HANDLE()
    # DISABLE_MAX_PRIVILEGE — 특권 전부 제거 (기준선 §9.3 'privilege strip')
    ok = adv.CreateRestrictedToken(
        cur, DISABLE_MAX_PRIVILEGE, 0, None, 0, None, 0, None,
        ctypes.byref(restricted))
    if not ok:
        return None, "CreateRestrictedToken 실패 err=%d" % ctypes.get_last_error()
    return restricted, None


def _run_under(token, payload, tag, workdir):
    """restricted token 으로 payload 를 실행하고 (exit_code, 출력, 오류) 반환."""
    outfile = os.path.join(workdir, "p0_01_%s.txt" % tag)
    if os.path.exists(outfile):
        os.remove(outfile)

    inner = payload.replace('"', '\\"')
    cmd = 'cmd.exe /c ""%s" -c "%s" > "%s" 2>&1"' % (sys.executable, inner, outfile)
    cmdline = ctypes.create_unicode_buffer(cmd)

    si = STARTUPINFOW()
    si.cb = ctypes.sizeof(si)
    pi = PROCESS_INFORMATION()

    created = adv.CreateProcessAsUserW(
        token, None, cmdline, None, None, False,
        CREATE_NO_WINDOW, None, workdir, ctypes.byref(si), ctypes.byref(pi))
    if not created:
        return None, "", "CreateProcessAsUserW 실패 err=%d" % ctypes.get_last_error()

    k32.WaitForSingleObject(pi.hProcess, 300000)
    code = w.DWORD()
    k32.GetExitCodeProcess(pi.hProcess, ctypes.byref(code))
    k32.CloseHandle(pi.hProcess)
    k32.CloseHandle(pi.hThread)

    out = ""
    if os.path.exists(outfile):
        out = open(outfile, "r", encoding="utf-8", errors="replace").read()
        os.remove(outfile)
    else:
        return code.value, "", "출력 파일이 생성되지 않았다 (%s)" % outfile
    return code.value, out, None


def probe_restricted_token():
    """단계 사다리로 어디서 깨지는지 좁힌다.

    출력이 비었다는 사실만으로 'CUDA 실패' 로 단정하면 안 된다.
    프로세스가 아예 안 뜬 것인지, python 이 안 뜬 것인지,
    torch import 가 막힌 것인지, CUDA 초기화만 막힌 것인지를 가른다.
    """
    token, err = _make_restricted_token()
    if token is None:
        rec("B", "Restricted Token 을 만들 수 있는가", "실패", err,
            "INCONCLUSIVE — 토큰 생성 단계에서 막힘")
        return None

    # restricted token 이 확실히 쓸 수 있는 작업 디렉터리
    workdir = "C:\\Windows\\Temp"
    if not os.path.isdir(workdir):
        workdir = tempfile.gettempdir()

    ladder = [
        ("b0_alive", "print('ALIVE')", "python 프로세스가 뜨는가"),
        ("b1_import", "import torch; print('TORCH', torch.__version__)", "torch 를 import 할 수 있는가"),
        ("b2_avail", "import torch; print('CUDA_AVAIL', torch.cuda.is_available())",
         "torch.cuda.is_available()"),
        ("b3_compute", CUDA_TEST, "실제 CUDA 연산"),
    ]

    lines = []
    reached = None
    for tag, payload, desc in ladder:
        code, out, e = _run_under(token, payload, tag, workdir)
        snippet = (out or "").strip().replace("\n", " | ")[:220]
        lines.append("  %-11s exit=%s  %s%s"
                     % (tag, code, snippet if snippet else "(출력 없음)",
                        "  [%s]" % e if e else ""))
        if e or code not in (0,):
            reached = tag
            break
        reached = tag

    full = "\n".join(lines)
    cuda_ok = "CUDA_AVAIL True" in full
    compute_ok = "matmul_ok True" in full

    if compute_ok:
        verdict = "PASS — Restricted Token 아래에서 CUDA 연산까지 동작. S1 경로 유지"
    elif cuda_ok:
        verdict = "PARTIAL — is_available 은 True 이나 연산이 실패. 추가 조사 필요"
    elif reached == "b0_alive":
        verdict = "INCONCLUSIVE — python 자체가 뜨지 않았다. 프로브 문제일 수 있다"
    elif reached == "b1_import":
        verdict = "FAIL-SCOPE — torch import 단계에서 막힘. CUDA 이전 문제"
    else:
        verdict = "FAIL-ARCHITECTURE — CUDA 초기화 실패. ADR-005 수정 검토"

    rec("B", "**Restricted Token 아래에서 CUDA 가 동작하는가** (P0-01 핵심)",
        "마지막 통과 단계: %s" % reached,
        "workdir=%s\n%s" % (workdir, full),
        verdict)
    return compute_ok


# ══════════════════════════════════════════════════════════════════
# C. Job Object — 프로세스 트리 종료 + VRAM 반환
# ══════════════════════════════════════════════════════════════════

HOLD_CUDA = (
    "import torch,time,sys;"
    "x=torch.randn(4096,4096,device='cuda');"
    "y=x@x;"
    "print('HOLDING',flush=True);"
    "time.sleep(120)"
)

JobObjectExtendedLimitInformation = 9
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000


def probe_job_object():
    before = nvidia_used_mib()

    hjob = k32.CreateJobObjectW(None, None)
    if not hjob:
        rec("C", "Job Object 로 프로세스 트리를 종료할 수 있는가", "실패",
            "CreateJobObjectW 실패 err=%d" % ctypes.get_last_error(),
            "FAIL — Job Object 생성 불가")
        return False

    p = subprocess.Popen([sys.executable, "-c", HOLD_CUDA],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                         text=True, creationflags=CREATE_NO_WINDOW)

    PROCESS_SET_QUOTA = 0x0100
    PROCESS_TERMINATE = 0x0001
    hproc = k32.OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, False, p.pid)
    assigned = bool(k32.AssignProcessToJobObject(hjob, hproc)) if hproc else False
    assign_err = ctypes.get_last_error() if not assigned else 0

    # CUDA 가 실제로 VRAM 을 잡을 때까지 대기
    holding = False
    t0 = time.time()
    while time.time() - t0 < 120:
        if p.poll() is not None:
            break
        line = p.stdout.readline() if p.stdout else ""
        if "HOLDING" in line:
            holding = True
            break
    time.sleep(3)
    during = nvidia_used_mib()

    k32.TerminateJobObject(hjob, 1)
    k32.CloseHandle(hjob)
    if hproc:
        k32.CloseHandle(hproc)

    try:
        p.wait(timeout=30)
    except subprocess.TimeoutExpired:
        p.kill()

    time.sleep(5)
    after = nvidia_used_mib()

    alive = p.poll() is None
    vram_grew = during > before + 100
    vram_released = after <= before + 100

    rec("C", "Job Object 종료 시 프로세스 트리와 VRAM 이 정리되는가",
        "종료 %s / VRAM 반환 %s" % ("성공" if not alive else "실패",
                                    "성공" if vram_released else "실패"),
        "AssignProcessToJobObject=%s%s\n"
        "HOLDING 도달=%s\n"
        "VRAM used(MiB)  before=%d  during=%d  after=%d\n"
        "VRAM 증가 관측=%s"
        % (assigned, "" if assigned else " (err=%d)" % assign_err,
           holding, before, during, after, vram_grew),
        "PASS" if (not alive and vram_released and vram_grew)
        else ("INCONCLUSIVE — VRAM 증가를 관측하지 못함" if not vram_grew
              else "FAIL — 정리 실패"))
    return not alive and vram_released


# ══════════════════════════════════════════════════════════════════
# D. 파일시스템 접근 (기준선 §22.7)
# ══════════════════════════════════════════════════════════════════

def probe_filesystem_reach():
    targets = [
        os.path.join(os.path.expanduser("~"), ".ssh", "id_ed25519"),
        os.path.join(os.path.expanduser("~"), ".ssh", "known_hosts"),
        os.path.join(os.path.expanduser("~"), "AppData", "Local", "Google",
                     "Chrome", "User Data", "Local State"),
    ]
    readable = []
    for t in targets:
        try:
            with open(t, "rb") as f:
                f.read(16)
            readable.append(t)
        except OSError:
            pass

    rec("D", "현재(비제한) 프로세스가 호스트 민감 파일을 읽을 수 있는가",
        "%d / %d 읽힘" % (len(readable), len(targets)),
        "읽힌 파일:\n" + ("\n".join("  " + r for r in readable) if readable else "  (없음)")
        + "\n\n※ 이것은 '제한이 없으면 읽힌다'는 기준선 측정이다.\n"
          "   S1 은 NTFS ACL 로 이를 차단해야 하며, 그 차단은 별도 검증이 필요하다.",
        "정보 — S1 ACL 설계의 대상 목록 확보")
    return readable


def main():
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:  # noqa: BLE001
        pass

    print("P0-01 · Windows S1 Restricted Native + CUDA")
    print("python: %s" % sys.version.split()[0])
    print("=" * 68)
    print()

    probe_baseline()
    probe_restricted_token()
    probe_job_object()
    probe_filesystem_reach()

    print("=" * 68)
    fails = [r for r in RESULTS if r["verdict"].startswith("FAIL")]
    print("probe %d개 · FAIL %d" % (len(RESULTS), len(fails)))
    print()
    print("JSON_BEGIN")
    print(json.dumps(RESULTS, ensure_ascii=False, indent=2))
    print("JSON_END")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
