# Job Object RAM 제한이 VRAM 할당 상한을 어떻게 결정하는가
import ctypes, subprocess, sys, time, json

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.CreateJobObjectW.restype = ctypes.c_void_p
k32.OpenProcess.restype = ctypes.c_void_p
k32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
k32.AssignProcessToJobObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
k32.SetInformationJobObject.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
k32.TerminateJobObject.argtypes = [ctypes.c_void_p, ctypes.c_uint]
k32.CloseHandle.argtypes = [ctypes.c_void_p]

class IOC(ctypes.Structure):
    _fields_=[("a",ctypes.c_ulonglong)]*0 or [("ReadOperationCount",ctypes.c_ulonglong),
      ("WriteOperationCount",ctypes.c_ulonglong),("OtherOperationCount",ctypes.c_ulonglong),
      ("ReadTransferCount",ctypes.c_ulonglong),("WriteTransferCount",ctypes.c_ulonglong),
      ("OtherTransferCount",ctypes.c_ulonglong)]
class BLI(ctypes.Structure):
    _fields_=[("PerProcessUserTimeLimit",ctypes.c_longlong),("PerJobUserTimeLimit",ctypes.c_longlong),
      ("LimitFlags",ctypes.c_uint32),("MinimumWorkingSetSize",ctypes.c_size_t),
      ("MaximumWorkingSetSize",ctypes.c_size_t),("ActiveProcessLimit",ctypes.c_uint32),
      ("Affinity",ctypes.c_size_t),("PriorityClass",ctypes.c_uint32),("SchedulingClass",ctypes.c_uint32)]
class ELI(ctypes.Structure):
    _fields_=[("BasicLimitInformation",BLI),("IoInfo",IOC),("ProcessMemoryLimit",ctypes.c_size_t),
      ("JobMemoryLimit",ctypes.c_size_t),("PeakProcessMemoryUsed",ctypes.c_size_t),
      ("PeakJobMemoryUsed",ctypes.c_size_t)]

CHILD = r"""
import torch, sys
mib = int(sys.argv[1])
try:
    x = torch.empty(mib*1048576//2, dtype=torch.float16, device='cuda'); x.fill_(1.0)
    torch.cuda.synchronize(); print("OK", mib)
except Exception as e:
    print("FAIL", mib, type(e).__name__)
"""

def run(ram_limit_mib, vram_mib):
    hjob = None
    if ram_limit_mib:
        hjob = k32.CreateJobObjectW(None, None)
        info = ELI()
        info.BasicLimitInformation.LimitFlags = 0x00000100  # PROCESS_MEMORY
        info.ProcessMemoryLimit = ram_limit_mib * 1024 * 1024
        k32.SetInformationJobObject(hjob, 9, ctypes.byref(info), ctypes.sizeof(info))
    p = subprocess.Popen([sys.executable, "-c", CHILD, str(vram_mib)],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                         creationflags=0x08000000)
    if hjob:
        h = k32.OpenProcess(0x0100|0x0001, False, p.pid)
        k32.AssignProcessToJobObject(hjob, h)
        k32.CloseHandle(h)
    out, _ = p.communicate(timeout=180)
    if hjob:
        k32.TerminateJobObject(hjob, 1); k32.CloseHandle(hjob)
    return "OK" if out.strip().startswith("OK") else "FAIL"

print("RAM제한(MiB) x VRAM요청(MiB) -> 결과")
rows=[]
for ram in (None, 8192, 6144, 4096, 3072):
    line=[]
    for vram in (1024, 2048, 3072, 4096, 6144):
        r = run(ram, vram)
        line.append("%s:%s" % (vram, r))
        rows.append({"ram_limit": ram, "vram_req": vram, "result": r})
    print("  RAM=%-6s  %s" % (ram if ram else "무제한", "  ".join(line)))
print()
print(json.dumps(rows))
