# gPUteer 초대 파일 만들기 (운영자 · Windows) — 팀원이 install-node 에 넘길 파일 하나를 만든다.
#
# 들어가는 것은 **공개 정보뿐**이다: Coordinator 주소 · id · 공개키 · 제출자 공개키 · 공유 저장소 경로 · 지금 풀의 노드 공개키 목록.
# 시드(비밀 키)는 넣지 않는다.
#
# 예:  powershell -NoProfile -ExecutionPolicy Bypass -File make-invite.ps1 -EnvFile <운영자 gputeer.env> -Out .\invite.env
#
# ★ 초대 파일에는 Coordinator 주소가 들어간다 — 저장소 · 공개 채널에 올리지 않는다(팀원에게 직접 건넨다).
param(
    [Parameter(Mandatory = $true)][string]$EnvFile,
    [Parameter(Mandatory = $true)][string]$Out,
    # 팀원 PC 에서 공유 저장소가 다른 경로로 보이면 그 경로를 준다(기본은 운영자 기계의 경로).
    [string]$SharedRootForNodes = ""
)

$ErrorActionPreference = "Stop"
$config = [ordered]@{}
foreach ($line in Get-Content -Encoding UTF8 $EnvFile) {
    if ($line -match '^\s*([A-Z_]+)=(.*)$') { $config[$Matches[1]] = $Matches[2].Trim() }
}
foreach ($key in @("GPUTEER_CONNECT", "GPUTEER_COORDINATOR_ID", "GPUTEER_COORDINATOR_PUBKEY", "GPUTEER_SUBMITTER_PUBKEY", "GPUTEER_SHARED_ROOT")) {
    if (-not $config.Contains($key) -or $config[$key] -eq "" -or $config[$key].StartsWith("<")) {
        throw "INVITE_ARGS: $EnvFile 에 $key 가 채워져 있지 않다"
    }
}
$shared = if ($SharedRootForNodes -ne "") { $SharedRootForNodes } else { $config.GPUTEER_SHARED_ROOT }
$pool = if ($config.Contains("GPUTEER_POOL_AGENTS") -and -not $config.GPUTEER_POOL_AGENTS.StartsWith("<")) { $config.GPUTEER_POOL_AGENTS } else { "" }
if (Test-Path -LiteralPath $Out) { throw "INVITE_ARGS: $Out 가 이미 있다 — 덮지 않는다(다른 이름을 준다)" }
$lines = @(
    "# gPUteer 초대 파일 — $(Get-Date -Format 'yyyy-MM-dd HH:mm') · 비밀 없음. 저장소 · 공개 채널에 올리지 않는다(주소가 들어 있다)",
    "GPUTEER_INVITE_VERSION=1",
    "GPUTEER_CONNECT=$($config.GPUTEER_CONNECT)",
    "GPUTEER_COORDINATOR_ID=$($config.GPUTEER_COORDINATOR_ID)",
    "GPUTEER_COORDINATOR_PUBKEY=$($config.GPUTEER_COORDINATOR_PUBKEY)",
    "GPUTEER_SUBMITTER_PUBKEY=$($config.GPUTEER_SUBMITTER_PUBKEY)",
    "GPUTEER_SHARED_ROOT=$shared",
    "GPUTEER_POOL_AGENTS=$pool"
)
[System.IO.File]::WriteAllText($Out, (($lines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "INVITE_WRITTEN $Out"
