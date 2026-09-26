# gPUteer 노드 등록 정보 새로 넣기 (운영자 · Windows) — admit-node 가 받아 둔 가입 파일을 전부 지금 시각으로 다시 넣는다.
#
# ★ 왜 — 스케줄러는 등록 정보의 관측 시각이 --max-snapshot-age-ms 보다 오래되면 그 노드에 배치하지 않는다. 그 시각을 새로 하는
#   경로가 import-inventory 하나뿐이라, 이것을 돌리지 않으면 등록 하루 뒤부터 풀이 새 작업을 배치하지 않는다(결함 301).
#   운영자 기계의 작업 스케줄러에 하루 한 번(최대 나이보다 짧게) 건다:
#     schtasks /Create /TN gputeer-refresh-inventory /SC HOURLY /MO 6 /RL LIMITED `
#       /TR "powershell -NoProfile -ExecutionPolicy Bypass -File <이 파일> -EnvFile <운영자 gputeer.env>"
# ★ 이것은 "노드가 살아 있다" 를 확인하지 않는다 — 운영자가 받은 선언을 다시 적을 뿐이다. 생존은 scheduler 의 --silent-after-ms 가 본다.
# ★ 결함 440 (재검수 115) — 이것은 하드웨어를 다시 읽지 않는다. 받아 둔 **선언**에 지금 시각을 찍을 뿐이라, 틀린 선언도 신선해진다.
#   그래서 **GPU 관측을 켜지 않은 노드**(CPU 전용 · 관측 전 판 Agent)용이다. 관측을 켠 노드는 인사의 관측이 신선도를 준다.
#   관측이 선언과 어긋난 노드는 이것으로 되살아나지 않는다 — Coordinator 가 불일치를 노드별로 기억하고, 맞는 관측이 올 때까지 배치에서 뺀다.
param(
    [Parameter(Mandatory = $true)][string]$EnvFile
)

$ErrorActionPreference = "Stop"
$config = [ordered]@{}
foreach ($line in Get-Content -Encoding UTF8 $EnvFile) {
    if ($line -match '^\s*([A-Z_]+)=(.*)$') { $config[$Matches[1]] = $Matches[2].Trim() }
}
$admittedDir = Join-Path (Split-Path -Parent (Resolve-Path -LiteralPath $EnvFile).Path) "admitted"
$files = @(Get-ChildItem -LiteralPath $admittedDir -Filter "join-*.json" -File -ErrorAction SilentlyContinue)
if ($files.Count -eq 0) { throw "REFRESH: $admittedDir 에 받은 노드가 없다 — admit-node 로 먼저 받는다" }
$nowMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$failed = 0
foreach ($file in $files) {
    $join = Get-Content -Encoding UTF8 -Raw $file.FullName | ConvertFrom-Json
    $agent = @($join.agents)[0]
    $agent.inventory.inventory_revision = $nowMs
    $agent.inventory.observed_at_unix_ms = $nowMs
    [System.IO.File]::WriteAllText($file.FullName, ($join | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))
    & $config.GPUTEER_BIN import-inventory --inventory $file.FullName --inventory-db $config.GPUTEER_CONTROL_DB | Out-Host
    if ($LASTEXITCODE -ne 0) { $failed += 1; Write-Host "REFRESH_FAILED $($file.Name)" } else { Write-Host "REFRESHED $($agent.registry.node_id)" }
}
if ($failed -gt 0) { throw "REFRESH: $failed 개 실패" }
