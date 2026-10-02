# gPUteer 스케줄러 루프 (신뢰망) — Windows
#
# 등록(로그온 시, 이 사용자로) 예:
#   schtasks /Create /TN "gputeer-scheduler" /SC ONLOGON /RL LIMITED `
#     /TR "powershell -NoProfile -ExecutionPolicy Bypass -File <이 파일 경로> -EnvFile <gputeer.env 경로>"
#
# ★ 값의 뜻 — docs/runbooks/신뢰망_설치_운영.md §4. Lease TTL 은 Agent 의 갱신 간격보다 충분히 길게.
param([Parameter(Mandatory = $true)][string]$EnvFile)

$ErrorActionPreference = "Stop"
# ★ 결함 562 — 상승된 관리자 창에서 돌지 않는다(설치기의 465 와 같다). 환경 파일의 GPUTEER_BIN 은 같은 사용자가 고칠 수 있어, 상승 창이면
#   그 값이 관리자 권한으로 돈다.
$principal = New-Object System.Security.Principal.WindowsPrincipal([System.Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "SCHEDULER_ELEVATED: do not run from an elevated (administrator) window - run in a normal window (defect 562)"
}
$config = @{}
foreach ($line in Get-Content -Encoding UTF8 $EnvFile) {
    if ($line -match '^\s*([A-Z_]+)=(.*)$') { $config[$Matches[1]] = $Matches[2] }
}

& $config.GPUTEER_BIN scheduler-loop --interval-ms 1000 --max-ticks 0 `
    --control-db $config.GPUTEER_CONTROL_DB `
    --submitter-keyring $config.GPUTEER_SUBMITTER_KEYRING `
    --submitter-member $config.GPUTEER_SUBMITTER_MEMBER `
    --max-snapshot-age-ms 86400000 `
    --best-fit-axes vram,gpu_count,cpu,ram,workspace `
    --coordinator-id $config.GPUTEER_COORDINATOR_ID --coordinator-term 1 `
    --lease-ttl-ms 120000 --lease-renew-after-ms 40000 --lease-max-total-duration-seconds 86400 `
    --silent-after-ms 60000 `
    --failover-grace-ms 30000 --coordinator-key-file $config.GPUTEER_COORDINATOR_SEED_FILE `
    --shared-checkpoint-root $config.GPUTEER_SHARED_ROOT `
    --pool-agents $config.GPUTEER_POOL_AGENTS `
    --pool-mode true
exit $LASTEXITCODE
