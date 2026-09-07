@echo off
rem ── coordinator-agent-selftest 반복 실행 (소크) ─────────────────────
rem
rem ★ 왜 반복하나
rem   이 저장소의 교착 버그(DoD-22 · DoD-23 · DoD-27)는 전부 **특정
rem   시나리오 조합에서만** 드러났고, 셋 다 "기존 시나리오들이 그 조합을
rem   우연히 피해가 안 드러났었다" 로 기록돼 있다. 한 번 통과하는 것과
rem   늘 통과하는 것은 다르다.
rem
rem ★★ **실패한 회차의 출력을 반드시 남긴다.** 2026-09-07 첫 소크가
rem   200회 중 2회 실패를 잡고도 출력을 덮어써서 원인을 못 봤다.
rem   소크의 목적은 드문 실패를 잡는 것인데 잡고도 증거를 버리면
rem   아무것도 안 한 것이다.
rem
rem ★ 소크가 도는 동안 **같은 디렉터리에서 다른 실행을 겹치지 마라.**
rem   포트·파일이 부딪혀 실패가 생기면 그것이 제품 결함인지 구분이 안 된다.
rem
rem 쓰는 법:  scripts\soak_selftest.cmd <반복 횟수>
rem ────────────────────────────────────────────────────────────────────
setlocal enabledelayedexpansion
if "%~1"=="" (echo 사용법: %~n0 ^<반복 횟수^> & exit /b 2)

if not exist target\release\gputeer.exe (
  echo target\release\gputeer.exe 가 없다 — 먼저 빌드하라:
  echo   cargo build --release -p gputeer-cli
  exit /b 1
)

if exist soakfail rmdir /s /q soakfail
mkdir soakfail

echo ===== SOAK: %1 runs =====
set /a FAIL=0
for /L %%i in (1,1,%1) do (
  target\release\gputeer.exe coordinator-agent-selftest > soakfail\run_%%i.txt 2>&1
  if errorlevel 1 (
    set /a FAIL+=1
    echo RUN %%i FAILED
  ) else (
    rem 통과한 회차는 지운다 — 300회분을 남기면 실패를 못 찾는다
    del soakfail\run_%%i.txt
  )
)
echo SOAK_DONE runs=%1 failures=!FAIL!
echo ===== 남은 실패 출력 =====
dir /b soakfail
