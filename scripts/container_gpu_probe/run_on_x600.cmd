@echo off
rem 컨테이너 GPU 실측(④)을 x600 에서 돌린다. 이 폴더(container_gpu_probe)를 E:\gputeer-work\container_gpu_probe 에 두고 실행한다.
rem ★ E: 아래에만 쓴다(C: 금지). WSL 을 끄지 않는다(wsl --shutdown 금지). 사용자 컨테이너는 건드리지 않는다 — probe.sh 머리말.
rem 인자: [이미지] [WSL 안의 gputeer 실행 파일 경로(선택)]
setlocal
set IMAGE=%~1
if "%IMAGE%"=="" set IMAGE=ubuntu:24.04
rem Windows 체크아웃에서 복사하면 줄끝이 CRLF 다 — WSL 의 sh 가 읽을 수 있게 CR 을 떼고(sed) 넘긴다.
wsl -d Ubuntu -- sh -c "sed 's/\r$//' /mnt/e/gputeer-work/container_gpu_probe/probe.sh | sh -s -- /mnt/e/gputeer-work/container_gpu_probe/results %IMAGE% %~2"
echo exit=%ERRORLEVEL%
wsl -d Ubuntu -- sh -c "ls -t /mnt/e/gputeer-work/container_gpu_probe/results/*.txt | head -n 1 | xargs cat"
endlocal
