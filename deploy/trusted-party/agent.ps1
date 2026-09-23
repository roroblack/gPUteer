# gPUteer Agent 루프 (신뢰망, GPU 하나 = 노드 하나) — Windows
#
# 등록(로그온 시, **그 PC 주인의 사용자로** — Owner Panel 이 같은 사용자에게 보여야 한다) 예:
#   schtasks /Create /TN "gputeer-agent-<node>" /SC ONLOGON /RL LIMITED `
#     /TR "powershell -NoProfile -ExecutionPolicy Bypass -File <이 파일 경로> -EnvFile <gputeer.env> -AgentEnvFile <agent-<node>.env>"
#
# ★ LocalSystem 서비스로 돌리지 않는다 — 소유자가 자기 GPU 를 되찾는 패널(127.0.0.1)이 그 사용자에게 있어야 한다(§0.1).
param(
    [Parameter(Mandatory = $true)][string]$EnvFile,
    [Parameter(Mandatory = $true)][string]$AgentEnvFile
)

$ErrorActionPreference = "Stop"
$config = @{}
foreach ($file in @($EnvFile, $AgentEnvFile)) {
    foreach ($line in Get-Content -Encoding UTF8 $file) {
        if ($line -match '^\s*([A-Z_]+)=(.*)$') { $config[$Matches[1]] = $Matches[2] }
    }
}
$nodeDir = $config.GPUTEER_NODE_DIR

& $config.GPUTEER_BIN agent-loop --interval-ms 5000 --max-rounds 0 -- `
    --connect $config.GPUTEER_CONNECT `
    --own-seed-file $config.GPUTEER_NODE_SEED_FILE `
    --peer-pubkey $config.GPUTEER_COORDINATOR_PUBKEY `
    --coordinator-device-id $config.GPUTEER_COORDINATOR_ID `
    --agent-device-id $config.GPUTEER_NODE_ID `
    --fence-db (Join-Path $nodeDir "fence.sqlite3") `
    --checkpoint-root (Join-Path $nodeDir "checkpoints") `
    --submitter-pubkey $config.GPUTEER_SUBMITTER_PUBKEY `
    --i-understand-this-executes-untrusted-code true `
    --report-over-session true --renew-during-execution-ms 30000 --max-reconnect-attempts 1 `
    --shared-checkpoint-root $config.GPUTEER_SHARED_ROOT `
    --pool-peer-keys $config.GPUTEER_POOL_AGENTS `
    --owner-panel-port $config.GPUTEER_OWNER_PANEL_PORT `
    --gpu-pin $config.GPUTEER_GPU_PIN
exit $LASTEXITCODE
