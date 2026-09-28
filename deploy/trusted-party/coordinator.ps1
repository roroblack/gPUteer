# gPUteer 풀 Coordinator (신뢰망) — Windows
#
# 등록(로그온 시, 이 사용자로) 예:
#   schtasks /Create /TN "gputeer-coordinator" /SC ONLOGON /RL LIMITED `
#     /TR "powershell -NoProfile -ExecutionPolicy Bypass -File <이 파일 경로> -EnvFile <gputeer.env 경로>"
#
# 값은 gputeer.env.template 을 저장소 밖에 복사해 채운 파일에서 읽는다. 채운 파일은 커밋하지 않는다.
param([Parameter(Mandatory = $true)][string]$EnvFile)

$ErrorActionPreference = "Stop"
# ★ 결함 562 — 상승된 관리자 창에서 돌지 않는다(설치기의 465 와 같다). 환경 파일의 GPUTEER_BIN 은 같은 사용자가 고칠 수 있어, 상승 창이면
#   그 값이 관리자 권한으로 돈다.
$principal = New-Object System.Security.Principal.WindowsPrincipal([System.Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "COORDINATOR_ELEVATED: do not run from an elevated (administrator) window - run in a normal window (defect 562)"
}
$config = @{}
foreach ($line in Get-Content -Encoding UTF8 $EnvFile) {
    if ($line -match '^\s*([A-Z_]+)=(.*)$') { $config[$Matches[1]] = $Matches[2] }
}

& $config.GPUTEER_BIN coordinator-stub --pool-mode true `
    --pool-agents $config.GPUTEER_POOL_AGENTS `
    --listen $config.GPUTEER_LISTEN `
    --own-seed-file $config.GPUTEER_COORDINATOR_SEED_FILE `
    --coordinator-device-id $config.GPUTEER_COORDINATOR_ID `
    --grant-from-control-db $config.GPUTEER_CONTROL_DB `
    --lease-db $config.GPUTEER_CONTROL_DB `
    --liveness-db $config.GPUTEER_CONTROL_DB `
    --submitter-keyring $config.GPUTEER_SUBMITTER_KEYRING `
    --accept-report-sessions true --release-on-exit-report true `
    --shared-checkpoint-root $config.GPUTEER_SHARED_ROOT `
    --replay-db $config.GPUTEER_REPLAY_DB `
    --max-connections 0 --accept-timeout-ms 0
exit $LASTEXITCODE
