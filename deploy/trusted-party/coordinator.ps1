# gPUteer 풀 Coordinator (신뢰망) — Windows
#
# 등록(로그온 시, 이 사용자로) 예:
#   schtasks /Create /TN "gputeer-coordinator" /SC ONLOGON /RL LIMITED `
#     /TR "powershell -NoProfile -ExecutionPolicy Bypass -File <이 파일 경로> -EnvFile <gputeer.env 경로>"
#
# 값은 gputeer.env.template 을 저장소 밖에 복사해 채운 파일에서 읽는다. 채운 파일은 커밋하지 않는다.
param([Parameter(Mandatory = $true)][string]$EnvFile)

$ErrorActionPreference = "Stop"
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
    --max-connections 0 --accept-timeout-ms 0
exit $LASTEXITCODE
