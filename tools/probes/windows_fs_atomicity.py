#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
P0-03a · Windows 파일시스템 원자성 조사

목적:
  기준선 §18.2 와 docs/protocol/state-machines.md §4 는 체크포인트 확정 절차로
  다음을 규범으로 요구한다.

      <name>.tmp 기록 -> fsync(file) -> rename(name) -> fsync(dir)

  그런데 `fsync(dir)` 은 POSIX 개념이다. Windows 에 대응물이 있는지,
  없다면 규범을 어떻게 고쳐야 하는지를 Rust 구현 착수 전에 확정한다.

  RULE.md §3.5 — 계약을 코드보다 먼저 고친다.

사용법:
    python tools/probes/windows_fs_atomicity.py
    python tools/probes/windows_fs_atomicity.py --json
"""
import argparse
import ctypes
import hashlib
import io
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import threading
import time

RESULTS = []


def record(probe, question, result, detail, verdict):
    RESULTS.append({
        "probe": probe, "question": question,
        "result": result, "detail": detail, "verdict": verdict,
    })
    print("[%s] %s" % (probe, question))
    print("    결과: %s" % result)
    for line in detail.strip().split("\n"):
        print("    %s" % line)
    print("    판정: %s" % verdict)
    print()


# ══════════════════════════════════════════════════════════════════════
# Probe 1 — rename 확정이 Windows 에서 성립하는가
#
# POSIX 와 Windows 의 결정적 차이:
#   POSIX   다른 프로세스가 파일을 열고 있어도 rename 이 성공한다.
#           옛 inode 는 마지막 fd 가 닫힐 때까지 살아 있다.
#   Windows MoveFileEx(REPLACE_EXISTING) 는 대상 파일을 누군가
#           FILE_SHARE_DELETE 없이 열고 있으면 ERROR_ACCESS_DENIED(5) 로
#           실패한다.
#
# 세 시나리오로 나눠 측정한다.
#   1a  동시 독자 없음                      -> 기준선
#   1b  독자가 FILE_SHARE_DELETE 없이 열기  -> 실패하는가
#   1c  독자가 FILE_SHARE_DELETE 로 열기    -> 원자적인가
# ══════════════════════════════════════════════════════════════════════

_K32 = None
if platform.system() == "Windows":
    _K32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _K32.CreateFileW.restype = ctypes.c_void_p
    _K32.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32,
                                 ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32,
                                 ctypes.c_void_p]
    _K32.ReadFile.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32,
                              ctypes.POINTER(ctypes.c_uint32), ctypes.c_void_p]
    _K32.CloseHandle.argtypes = [ctypes.c_void_p]

GENERIC_READ = 0x80000000
FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002
FILE_SHARE_DELETE = 0x00000004
OPEN_EXISTING = 3
INVALID_HANDLE = ctypes.c_void_p(-1).value


def _read_with_share(path, share_mode, size):
    """지정한 공유 모드로 파일을 열어 읽는다. (내용, 오류코드) 반환."""
    h = _K32.CreateFileW(path, GENERIC_READ, share_mode, None,
                         OPEN_EXISTING, 0, None)
    if h == INVALID_HANDLE or h is None:
        return None, ctypes.get_last_error()
    try:
        buf = ctypes.create_string_buffer(size)
        got = ctypes.c_uint32(0)
        ok = _K32.ReadFile(ctypes.c_void_p(h), buf, size, ctypes.byref(got), None)
        if not ok:
            return None, ctypes.get_last_error()
        return buf.raw[:got.value], 0
    finally:
        _K32.CloseHandle(ctypes.c_void_p(h))


def _replace_loop(tmpdir, target, payload_a, payload_b, iterations, stats):
    for i in range(iterations):
        src = os.path.join(tmpdir, "stage_%d.tmp" % (i % 2))
        payload = payload_b if i % 2 == 0 else payload_a
        with io.open(src, "wb") as f:
            f.write(payload)
            f.flush()
            os.fsync(f.fileno())
        try:
            os.replace(src, target)
            stats["replace_ok"] += 1
        except PermissionError as e:
            stats["replace_denied"] += 1
            stats["last_error"] = "WinError %s" % getattr(e, "winerror", "?")
            try:
                os.remove(src)
            except OSError:
                pass
        except OSError as e:
            stats["replace_other_error"] += 1
            stats["last_error"] = repr(e)


def probe_replace_atomicity(tmpdir, iterations=2000):
    SIZE = 65536
    payload_a = b"A" * SIZE
    payload_b = b"B" * SIZE
    is_win = platform.system() == "Windows"

    # ---- 1a. 동시 독자 없음 ----
    t1 = os.path.join(tmpdir, "t_none.bin")
    io.open(t1, "wb").write(payload_a)
    s1 = {"replace_ok": 0, "replace_denied": 0, "replace_other_error": 0, "last_error": None}
    _replace_loop(tmpdir, t1, payload_a, payload_b, iterations, s1)
    ok_1a = s1["replace_denied"] == 0 and s1["replace_other_error"] == 0
    record("P1a", "동시 독자가 없을 때 rename 확정이 성공하는가",
           "성공 %d / %d" % (s1["replace_ok"], iterations),
           "거부: %d · 기타 오류: %d" % (s1["replace_denied"], s1["replace_other_error"]),
           "PASS" if ok_1a else "FAIL — 기본 rename 조차 실패한다")

    if not is_win:
        record("P1b/P1c", "Windows 전용 공유 모드 시나리오", "건너뜀",
               "이 플랫폼은 Windows 가 아니다", "N/A")
        return ok_1a

    # ---- 1b. 독자가 FILE_SHARE_DELETE 없이 연 상태 ----
    t2 = os.path.join(tmpdir, "t_nodel.bin")
    io.open(t2, "wb").write(payload_a)
    s2 = {"replace_ok": 0, "replace_denied": 0, "replace_other_error": 0, "last_error": None}
    obs2 = {"ok": 0, "openfail": 0}
    stop2 = threading.Event()

    def reader_nodelete():
        # Python io.open 의 기본 공유 모드에는 FILE_SHARE_DELETE 가 없다
        while not stop2.is_set():
            try:
                with io.open(t2, "rb") as f:
                    f.read()
                obs2["ok"] += 1
            except OSError:
                obs2["openfail"] += 1

    th2 = [threading.Thread(target=reader_nodelete, daemon=True) for _ in range(3)]
    for t in th2:
        t.start()
    _replace_loop(tmpdir, t2, payload_a, payload_b, iterations, s2)
    stop2.set()
    for t in th2:
        t.join(timeout=2)

    blocked = s2["replace_denied"] > 0
    record("P1b", "독자가 FILE_SHARE_DELETE 없이 열고 있으면 rename 이 실패하는가",
           "실패 %d회 관측" % s2["replace_denied"] if blocked else "실패 없음",
           "교체 시도 %d · 성공 %d · **거부 %d** · 기타 %d\n"
           "  마지막 오류: %s\n"
           "  독자 열기 성공 %d · 열기 실패 %d"
           % (iterations, s2["replace_ok"], s2["replace_denied"],
              s2["replace_other_error"], s2["last_error"],
              obs2["ok"], obs2["openfail"]),
           "FINDING — POSIX 와 다르다. 재시도 또는 공유 모드 강제가 필요" if blocked
           else "정보 — 이 환경에서는 재현되지 않음")

    # ---- 1c. 독자가 FILE_SHARE_DELETE 로 연 상태 ----
    t3 = os.path.join(tmpdir, "t_del.bin")
    io.open(t3, "wb").write(payload_a)
    s3 = {"replace_ok": 0, "replace_denied": 0, "replace_other_error": 0, "last_error": None}
    obs3 = {"a": 0, "b": 0, "partial": 0, "openfail": 0}
    share_all = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
    stop3 = threading.Event()

    def reader_sharedelete():
        while not stop3.is_set():
            data, err = _read_with_share(t3, share_all, SIZE)
            if data is None:
                obs3["openfail"] += 1
            elif data == payload_a:
                obs3["a"] += 1
            elif data == payload_b:
                obs3["b"] += 1
            else:
                obs3["partial"] += 1

    th3 = [threading.Thread(target=reader_sharedelete, daemon=True) for _ in range(3)]
    for t in th3:
        t.start()
    _replace_loop(tmpdir, t3, payload_a, payload_b, iterations, s3)
    stop3.set()
    for t in th3:
        t.join(timeout=2)

    atomic = (s3["replace_denied"] == 0 and s3["replace_other_error"] == 0
              and obs3["partial"] == 0)
    record("P1c", "독자가 FILE_SHARE_DELETE 로 열면 rename 이 원자적인가",
           "원자적" if atomic else "문제 관측",
           "교체 시도 %d · 성공 %d · 거부 %d · 기타 %d\n"
           "  독자 관측 — 온전한 A: %d · 온전한 B: %d · **부분 내용: %d** · 열기 실패: %d"
           % (iterations, s3["replace_ok"], s3["replace_denied"],
              s3["replace_other_error"], obs3["a"], obs3["b"],
              obs3["partial"], obs3["openfail"]),
           "PASS — 공유 모드만 지키면 rename 확정 사용 가능" if atomic
           else "FAIL — 규범 §18.2 수정 필요")

    # ---- 1d. POSIX 시맨틱 rename (SetFileInformationByHandle) ----
    # MoveFileEx 는 열린 파일을 대체하지 못한다. Windows 10 1607+ 는
    # FileRenameInfoEx + FILE_RENAME_FLAG_POSIX_SEMANTICS 로 POSIX 동작을 제공한다.
    ok_1d = probe_posix_rename(tmpdir, payload_a, payload_b, SIZE,
                               min(iterations, 1000))

    return ok_1a and (atomic or ok_1d)


DELETE_ACCESS = 0x00010000
GENERIC_WRITE = 0x40000000
FileRenameInfoEx = 22
FILE_RENAME_FLAG_REPLACE_IF_EXISTS = 0x00000001
FILE_RENAME_FLAG_POSIX_SEMANTICS = 0x00000002


class FILE_RENAME_INFO(ctypes.Structure):
    _fields_ = [
        ("Flags", ctypes.c_uint32),
        ("_pad", ctypes.c_uint32),
        ("RootDirectory", ctypes.c_void_p),
        ("FileNameLength", ctypes.c_uint32),
        ("FileName", ctypes.c_wchar * 512),
    ]


def _posix_rename(src, dst):
    """SetFileInformationByHandle + FileRenameInfoEx(POSIX_SEMANTICS)."""
    _K32.SetFileInformationByHandle.argtypes = [
        ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
    _K32.SetFileInformationByHandle.restype = ctypes.c_int

    h = _K32.CreateFileW(src, DELETE_ACCESS | GENERIC_WRITE,
                         FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                         None, OPEN_EXISTING, 0, None)
    if h == INVALID_HANDLE or h is None:
        return False, ctypes.get_last_error()
    try:
        info = FILE_RENAME_INFO()
        info.Flags = FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS
        info.RootDirectory = None
        target = os.path.abspath(dst)
        info.FileName = target
        info.FileNameLength = len(target) * 2
        ok = _K32.SetFileInformationByHandle(
            ctypes.c_void_p(h), FileRenameInfoEx,
            ctypes.byref(info), ctypes.sizeof(info))
        return bool(ok), (0 if ok else ctypes.get_last_error())
    finally:
        _K32.CloseHandle(ctypes.c_void_p(h))


def probe_posix_rename(tmpdir, payload_a, payload_b, size, iterations):
    target = os.path.join(tmpdir, "t_posix.bin")
    io.open(target, "wb").write(payload_a)

    stats = {"ok": 0, "denied": 0, "other": 0, "last": None}
    obs = {"a": 0, "b": 0, "partial": 0, "openfail": 0}
    stop = threading.Event()

    # 최악 조건: 독자가 FILE_SHARE_DELETE 없이 연다 (P1b 와 동일)
    def reader():
        while not stop.is_set():
            try:
                with io.open(target, "rb") as f:
                    data = f.read()
                if data == payload_a:
                    obs["a"] += 1
                elif data == payload_b:
                    obs["b"] += 1
                else:
                    obs["partial"] += 1
            except OSError:
                obs["openfail"] += 1

    ths = [threading.Thread(target=reader, daemon=True) for _ in range(3)]
    for t in ths:
        t.start()

    for i in range(iterations):
        src = os.path.join(tmpdir, "pstage_%d.tmp" % (i % 2))
        payload = payload_b if i % 2 == 0 else payload_a
        with io.open(src, "wb") as f:
            f.write(payload)
            f.flush()
            os.fsync(f.fileno())
        ok, err = _posix_rename(src, target)
        if ok:
            stats["ok"] += 1
        elif err == 5:
            stats["denied"] += 1
            stats["last"] = "ACCESS_DENIED(5)"
            try:
                os.remove(src)
            except OSError:
                pass
        else:
            stats["other"] += 1
            stats["last"] = "GetLastError=%d" % err
            try:
                os.remove(src)
            except OSError:
                pass

    stop.set()
    for t in ths:
        t.join(timeout=2)

    good = stats["ok"] == iterations and obs["partial"] == 0
    record("P1d",
           "POSIX 시맨틱 rename(FileRenameInfoEx)이 열린 파일을 대체하는가",
           "성공 %d / %d" % (stats["ok"], iterations),
           "**독자는 FILE_SHARE_DELETE 없이 연 상태 (P1b 와 동일한 최악 조건)**\n"
           "  성공 %d · ACCESS_DENIED %d · 기타 %d (%s)\n"
           "  독자 관측 — 온전한 A: %d · 온전한 B: %d · **부분 내용: %d** · 열기 실패: %d"
           % (stats["ok"], stats["denied"], stats["other"], stats["last"],
              obs["a"], obs["b"], obs["partial"], obs["openfail"]),
           "PASS — MoveFileEx 대신 이 API 를 쓰면 POSIX 동작을 얻는다" if good
           else "FAIL — 이 API 로도 해결되지 않는다")
    return good


# ══════════════════════════════════════════════════════════════════════
# Probe 2 — 디렉터리 fsync 대응물이 있는가
#
# POSIX: open(dir) + fsync(fd)
# Windows: 일반 CreateFile 로는 디렉터리를 열 수 없다.
#          FILE_FLAG_BACKUP_SEMANTICS 가 필요하고, FlushFileBuffers 가
#          디렉터리 핸들에서 동작하는지는 문서화되어 있지 않다.
# ══════════════════════════════════════════════════════════════════════

def probe_dir_fsync(tmpdir):
    # 2a. POSIX 방식 시도
    posix_err = None
    try:
        fd = os.open(tmpdir, os.O_RDONLY)
        try:
            os.fsync(fd)
            posix_ok = True
        finally:
            os.close(fd)
    except OSError as e:
        posix_ok = False
        posix_err = "%s: %s" % (type(e).__name__, e)

    # 2b. Win32 FILE_FLAG_BACKUP_SEMANTICS + FlushFileBuffers
    win32_ok, win32_err = False, None
    if platform.system() == "Windows":
        GENERIC_READ = 0x80000000
        FILE_SHARE_ALL = 0x00000007
        OPEN_EXISTING = 3
        FILE_FLAG_BACKUP_SEMANTICS = 0x02000000
        INVALID_HANDLE = ctypes.c_void_p(-1).value

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.CreateFileW.restype = ctypes.c_void_p
        k32.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32,
                                    ctypes.c_uint32, ctypes.c_void_p,
                                    ctypes.c_uint32, ctypes.c_uint32,
                                    ctypes.c_void_p]
        k32.FlushFileBuffers.restype = ctypes.c_int
        k32.FlushFileBuffers.argtypes = [ctypes.c_void_p]
        k32.CloseHandle.argtypes = [ctypes.c_void_p]

        # FlushFileBuffers 는 핸들에 쓰기 권한을 요구한다.
        # 읽기 전용으로만 시도하면 ACCESS_DENIED 가 나므로 두 조합을 모두 본다.
        attempts = [("GENERIC_READ", GENERIC_READ),
                    ("GENERIC_READ|GENERIC_WRITE", GENERIC_READ | 0x40000000),
                    ("FILE_WRITE_DATA(0x2)", 0x0002)]
        notes = []
        for label, access in attempts:
            h = k32.CreateFileW(tmpdir, access, FILE_SHARE_ALL, None,
                                OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, None)
            if h == INVALID_HANDLE or h is None:
                notes.append("%s -> CreateFileW 실패 (err=%d)"
                             % (label, ctypes.get_last_error()))
                continue
            rc = k32.FlushFileBuffers(ctypes.c_void_p(h))
            if rc:
                win32_ok = True
                notes.append("%s -> FlushFileBuffers 성공" % label)
            else:
                notes.append("%s -> FlushFileBuffers 실패 (err=%d)"
                             % (label, ctypes.get_last_error()))
            k32.CloseHandle(ctypes.c_void_p(h))
            if win32_ok:
                break
        win32_err = " / ".join(notes)

    detail = ("POSIX os.open(dir)+os.fsync(): %s%s\n"
              "Win32 CreateFileW(BACKUP_SEMANTICS)+FlushFileBuffers(): %s%s"
              % ("성공" if posix_ok else "실패",
                 "" if posix_ok else "  (%s)" % posix_err,
                 "성공" if win32_ok else "실패",
                 "" if win32_ok else "  (%s)" % (win32_err or "미시도")))

    if posix_ok:
        verdict = "규범 그대로 사용 가능"
    elif win32_ok:
        verdict = "PARTIAL — Win32 경로로 대체 가능. 규범에 플랫폼별 구현을 명시해야 함"
    else:
        verdict = "FAIL — 디렉터리 fsync 불가. 규범 §18.2 수정 필요"

    record("P2", "디렉터리 fsync 대응물이 Windows 에 있는가",
           "POSIX 불가 / Win32 %s" % ("가능" if win32_ok else "불가"),
           detail, verdict)
    return posix_ok, win32_ok


# ══════════════════════════════════════════════════════════════════════
# Probe 3 — 쓰기 도중 강제 종료 시 무엇이 남는가
# ══════════════════════════════════════════════════════════════════════

WRITER_SRC = r'''
import io, os, sys, time
d = sys.argv[1]
tmp = os.path.join(d, "ckpt.bin.tmp")
final = os.path.join(d, "ckpt.bin")
with io.open(tmp, "wb") as f:
    for i in range(2000):
        f.write(b"X" * 65536)
        f.flush()
        if i == 50:
            io.open(os.path.join(d, "READY"), "wb").close()
        time.sleep(0.001)
    os.fsync(f.fileno())
os.replace(tmp, final)
'''


def probe_kill_during_write(tmpdir):
    d = os.path.join(tmpdir, "killtest")
    os.makedirs(d, exist_ok=True)
    script = os.path.join(tmpdir, "writer.py")
    io.open(script, "w", encoding="utf-8").write(WRITER_SRC)

    p = subprocess.Popen([sys.executable, script, d],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ready = os.path.join(d, "READY")
    for _ in range(300):
        if os.path.exists(ready):
            break
        time.sleep(0.01)
    time.sleep(0.05)
    p.kill()
    p.wait(timeout=5)

    files = sorted(os.listdir(d))
    has_tmp = "ckpt.bin.tmp" in files
    has_final = "ckpt.bin" in files
    tmp_size = os.path.getsize(os.path.join(d, "ckpt.bin.tmp")) if has_tmp else 0

    ok = has_tmp and not has_final
    record("P3", "쓰기 도중 강제 종료 시 확정 파일이 생기는가",
           "tmp 만 남음" if ok else "예상과 다름",
           "남은 파일: %s\n"
           "  ckpt.bin.tmp 존재: %s (%d bytes)\n"
           "  ckpt.bin(확정) 존재: %s"
           % (files, has_tmp, tmp_size, has_final),
           "PASS — 미완성 데이터가 확정 이름을 갖지 않는다" if ok
           else "FAIL — 확정 파일이 조기 노출됨")
    return ok


# ══════════════════════════════════════════════════════════════════════
# Probe 4 — 매니페스트 없는 데이터 파일을 GC 가 식별할 수 있는가
# ══════════════════════════════════════════════════════════════════════

def probe_manifest_last(tmpdir):
    d = os.path.join(tmpdir, "gctest")
    os.makedirs(d, exist_ok=True)

    # 정상: 데이터 확정 -> 매니페스트 마지막
    good = os.path.join(d, "ckpt-good")
    os.makedirs(good, exist_ok=True)
    for n in ("model.bin", "optim.bin"):
        with io.open(os.path.join(good, n), "wb") as f:
            f.write(b"D" * 1024)
            os.fsync(f.fileno())
    with io.open(os.path.join(good, "manifest.json"), "w") as f:
        json.dump({"files": ["model.bin", "optim.bin"]}, f)
        f.flush()
        os.fsync(f.fileno())

    # 비정상: 데이터만 있고 매니페스트 없음 (kill 시뮬레이션)
    bad = os.path.join(d, "ckpt-partial")
    os.makedirs(bad, exist_ok=True)
    with io.open(os.path.join(bad, "model.bin"), "wb") as f:
        f.write(b"D" * 1024)

    def classify(p):
        return "COMPLETE" if os.path.exists(os.path.join(p, "manifest.json")) else "PARTIAL"

    g, b = classify(good), classify(bad)
    ok = g == "COMPLETE" and b == "PARTIAL"

    record("P4", "매니페스트 유무로 PARTIAL 을 식별할 수 있는가",
           "식별 가능" if ok else "식별 불가",
           "ckpt-good    -> %s\nckpt-partial -> %s" % (g, b),
           "PASS — 매니페스트-마지막 규칙이 GC 판정 근거로 충분" if ok
           else "FAIL — 다른 판정 기준 필요")
    return ok


# ══════════════════════════════════════════════════════════════════════
# Probe 5 — 파일시스템 종류
# ══════════════════════════════════════════════════════════════════════

def probe_filesystem(tmpdir):
    fs = "unknown"
    if platform.system() == "Windows":
        buf = ctypes.create_unicode_buffer(256)
        drive = os.path.splitdrive(os.path.abspath(tmpdir))[0] + "\\"
        ok = ctypes.windll.kernel32.GetVolumeInformationW(
            ctypes.c_wchar_p(drive), None, 0, None, None, None, buf, 256)
        if ok:
            fs = buf.value
    record("P5", "테스트가 수행된 파일시스템은 무엇인가", fs,
           "경로: %s" % tmpdir,
           "정보 — NTFS 외 파일시스템은 별도 검증 필요")
    return fs


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--iterations", type=int, default=3000)
    args = ap.parse_args()

    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    print("P0-03a · Windows 파일시스템 원자성 조사")
    print("platform: %s %s" % (platform.system(), platform.release()))
    print("python:   %s" % sys.version.split()[0])
    print("=" * 66)
    print()

    tmpdir = tempfile.mkdtemp(prefix="gputeer_fsprobe_")
    try:
        fs = probe_filesystem(tmpdir)
        probe_replace_atomicity(tmpdir, args.iterations)
        probe_dir_fsync(tmpdir)
        probe_kill_during_write(tmpdir)
        probe_manifest_last(tmpdir)
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)

    print("=" * 66)
    fails = [r for r in RESULTS if r["verdict"].startswith("FAIL")]
    partials = [r for r in RESULTS if r["verdict"].startswith("PARTIAL")]
    print("probe %d개 · FAIL %d · PARTIAL %d" % (len(RESULTS), len(fails), len(partials)))

    if args.json:
        print()
        print(json.dumps({"filesystem": fs, "results": RESULTS},
                         ensure_ascii=False, indent=2))
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
