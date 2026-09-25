@echo off
rem 컨테이너 GPU 실측(④)을 x600 에서 돌린다. 이 폴더(container_gpu_probe)를 E:\gputeer-work\container_gpu_probe 에 두고 실행한다.
rem ★ E: 아래에만 쓴다(C: 금지). WSL 을 끄지 않는다(wsl --shutdown 금지). 사용자 컨테이너는 건드리지 않는다 — probe.sh 머리말.
rem 인자: [이미지] [WSL 안의 gputeer 실행 파일 경로(선택)]
rem ★ 2026-09-25 — ssh 로 cmd /c 를 부르면 마지막 인자 끝에 따옴표 하나가 붙어 온다(물결 치환으로도 안 떨어진다).
rem   그대로 넘기면 WSL 의 bash 가 "unexpected EOF" 로 멈춘다 — 인자에서 따옴표 문자를 지우고 넘긴다.
rem   probe.sh 는 .gitattributes 로 LF 다(CRLF 면 sh 가 못 읽는다).
setlocal
set "IMAGE=%~1"
set "GPUTEER=%~2"
if defined IMAGE set "IMAGE=%IMAGE:"=%"
if defined GPUTEER set "GPUTEER=%GPUTEER:"=%"
if not defined IMAGE set "IMAGE=ubuntu:24.04"
wsl -d Ubuntu -- sh /mnt/e/gputeer-work/container_gpu_probe/probe.sh /mnt/e/gputeer-work/container_gpu_probe/results %IMAGE% %GPUTEER%
echo exit=%ERRORLEVEL%
endlocal
