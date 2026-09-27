# gPUteer 노드 받기 (운영자 · Windows) — 팀원이 보낸 join-<노드>.json 을 풀에 넣는다.
#
# 하는 일
#   1. 가입 파일을 읽어 노드 id · 공개키를 확인한다(같은 id 가 다른 키로 이미 있으면 거부 · 같은 키가 다른 id 로 있으면 거부)
#   2. import-inventory 로 control DB 에 넣는다 — 관측 시각을 **지금**, revision 을 지금(ms)으로 다시 적는다(결함 301)
#   3. 운영자 gputeer.env 의 GPUTEER_POOL_AGENTS 에 `<노드>=<공개키>` 를 더한다 — 먼저 _backup\<시각>\ 에 복사한다
#   4. 받은 가입 파일을 <env 폴더>\admitted\ 에 둔다 — refresh-inventory 가 매일 다시 넣는다
#
# 예:  powershell -NoProfile -ExecutionPolicy Bypass -File admit-node.ps1 -EnvFile <운영자 gputeer.env> -JoinFile .\join-alice-gpu0.json
#
# ★ 이 명령은 서명을 검증하지 않는다(import-inventory 와 같다) — 가입 파일을 **누가 보냈는지** 운영자가 직접 확인한다.
#   공개키를 전화 · 대면으로 한 번 맞춰 보는 것을 권한다(파일을 바꿔치기하면 남의 키가 풀에 들어온다).
# ★ Coordinator · scheduler 는 --pool-agents 를 시작할 때만 읽는다 — 받은 뒤 **둘 다 다시 띄운다**. 다른 Agent 들도
#   GPUTEER_POOL_AGENTS 를 새 줄로 바꾸고 다시 띄워야 이 노드의 체크포인트에서 이어받을 수 있다(출력의 POOL_AGENTS 줄).
param(
    [Parameter(Mandatory = $true)][string]$EnvFile,
    [Parameter(Mandatory = $true)][string]$JoinFile
)

$ErrorActionPreference = "Stop"
$envText = Get-Content -Encoding UTF8 $EnvFile
$config = [ordered]@{}
foreach ($line in $envText) {
    if ($line -match '^\s*([A-Z_]+)=(.*)$') { $config[$Matches[1]] = $Matches[2].Trim() }
}
foreach ($key in @("GPUTEER_BIN", "GPUTEER_CONTROL_DB")) {
    if (-not $config.Contains($key) -or $config[$key] -eq "" -or $config[$key].StartsWith("<")) { throw "ADMIT_ARGS: $EnvFile 에 $key 가 채워져 있지 않다" }
}

$join = Get-Content -Encoding UTF8 -Raw $JoinFile | ConvertFrom-Json
if ($join.schema_version -ne 1 -or @($join.agents).Count -ne 1) { throw "JOIN_REJECTED: 노드 하나짜리 schema_version 1 문서가 아니다" }
$agent = @($join.agents)[0]
$nodeId = "$($agent.registry.node_id)"
$key = "$($agent.registry.verifying_key_hex)"
if ($nodeId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$') { throw "JOIN_REJECTED: 노드 id 형식 오류 '$nodeId'" }
if ($key -notmatch '^[0-9a-f]{64}$') { throw "JOIN_REJECTED: 공개키가 64자리 16진수가 아니다" }
if ("$($agent.inventory.node_id)" -ne "" -and "$($agent.inventory.node_id)" -ne $nodeId) { throw "JOIN_REJECTED: inventory 의 node_id 가 다르다" }

$pool = if ($config.Contains("GPUTEER_POOL_AGENTS") -and -not $config.GPUTEER_POOL_AGENTS.StartsWith("<")) { $config.GPUTEER_POOL_AGENTS } else { "" }
$entries = [ordered]@{}
foreach ($entry in $pool.Split(";")) {
    if ($entry.Trim() -eq "") { continue }
    $pair = $entry.Split("=", 2)
    $entries[$pair[0].Trim()] = $pair[1].Trim()
}
if ($entries.Contains($nodeId) -and $entries[$nodeId] -ne $key) { throw "JOIN_REJECTED: $nodeId 가 다른 공개키로 이미 풀에 있다 — 키를 바꾸려면 운영자가 직접 정리한다" }
foreach ($other in $entries.Keys) {
    if ($other -ne $nodeId -and $entries[$other] -eq $key) { throw "JOIN_REJECTED: 같은 공개키를 $other 가 이미 쓴다(POOL_AGENTS_DUPLICATE_KEY)" }
}

$admittedDir = Join-Path (Split-Path -Parent (Resolve-Path -LiteralPath $EnvFile).Path) "admitted"
New-Item -ItemType Directory -Force -Path $admittedDir | Out-Null
# ★ 결함 466 (재검수 118) — admit-node · refresh-inventory 는 받아 둔 가입 파일을 같이 고친다. 동시에 돌면 refresh 가 옛 사본을 다시 활성 사본으로
#   썼다 — admitted\.lock 을 CreateNew 로 잡고 돈다(닫으면 지워진다). 잡혀 있으면 멈춘다.
$lockPath = Join-Path $admittedDir ".lock"
try {
    $lock = New-Object System.IO.FileStream($lockPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None, 1, [System.IO.FileOptions]::DeleteOnClose)
} catch {
    throw "ADMITTED_LOCKED: $lockPath 이 있다 — admit-node · refresh-inventory 가 이미 돈다. 도는 것이 없는데 남았으면 지우고 다시 돌린다"
}
trap { if ($lock) { $lock.Dispose() }; break }
$stamp = Get-Date -Format "yyyy-MM-dd_HHmm"
$admitted = Join-Path $admittedDir "join-$nodeId.json"
# 관측 시각을 지금으로 — 받은 시각이 곧 운영자가 이 선언을 받아들인 시각이다.
# ★ 결함 467 (재검수 118) — revision 은 벽시계와 뗀다: max(지금, 받아 둔 사본의 revision + 1). 운영자 시계가 한 번 미래로 갔다 돌아와도 스스로 풀린다.
$nowMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$revision = $nowMs
if (Test-Path -LiteralPath $admitted) {
    $previous = [long](@((Get-Content -Encoding UTF8 -Raw $admitted | ConvertFrom-Json).agents)[0].inventory.inventory_revision)
    if ($revision -le $previous) { $revision = $previous + 1 }
}
$agent.inventory.inventory_revision = $revision
$agent.inventory.observed_at_unix_ms = $nowMs
# ★ 결함 443 (재검수 115) — **반입이 성공한 뒤에만** 활성 사본을 바꾼다. 전에는 먼저 덮고 반입했다 — 거부될 가입 파일이 활성 사본이 되면
#   refresh-inventory 가 매번 실패하고 정상 선언이 낡아 노드가 빠졌다. 후보는 join-*.json 이 아닌 이름이라 refresh 가 줍지 않는다.
$candidate = Join-Path $admittedDir ".candidate-$nodeId.json"
[System.IO.File]::WriteAllText($candidate, ($join | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))

# ★ 5.1 에서 Stop 이면 실행 파일의 stderr 한 줄이 곧 예외다 — 그러면 후보를 못 지우고 빠져나간다. 이 호출만 Continue 로 두고 종료 코드로 판정한다.
$ErrorActionPreference = "Continue"
& $config.GPUTEER_BIN import-inventory --inventory $candidate --inventory-db $config.GPUTEER_CONTROL_DB 2>&1 | ForEach-Object { "$_" } | Out-Host
$importExit = $LASTEXITCODE
$ErrorActionPreference = "Stop"
if ($importExit -ne 0) {
    Remove-Item -LiteralPath $candidate -Force
    throw "import-inventory 실패 — 풀 목록도 받아 둔 가입 파일(admitted\)도 바꾸지 않았다"
}
if (Test-Path -LiteralPath $admitted) {
    $backup = Join-Path $admittedDir (Join-Path "_backup" $stamp)
    New-Item -ItemType Directory -Force -Path $backup | Out-Null
    Copy-Item -LiteralPath $admitted -Destination $backup
}
Move-Item -LiteralPath $candidate -Destination $admitted -Force

if (-not $entries.Contains($nodeId)) {
    $entries[$nodeId] = $key
    $newPool = ($entries.Keys | ForEach-Object { "$_=$($entries[$_])" }) -join ";"
    $envDir = Split-Path -Parent (Resolve-Path -LiteralPath $EnvFile).Path
    $backup = Join-Path $envDir (Join-Path "_backup" $stamp)
    New-Item -ItemType Directory -Force -Path $backup | Out-Null
    Copy-Item -LiteralPath $EnvFile -Destination $backup
    $replaced = $false
    $out = foreach ($line in $envText) {
        if ($line -match '^\s*GPUTEER_POOL_AGENTS=') { $replaced = $true; "GPUTEER_POOL_AGENTS=$newPool" } else { $line }
    }
    if (-not $replaced) { $out = @($out) + "GPUTEER_POOL_AGENTS=$newPool" }
    [System.IO.File]::WriteAllText((Resolve-Path -LiteralPath $EnvFile).Path, ((@($out) -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "BACKUP $EnvFile -> $backup"
    Write-Host "POOL_AGENTS $newPool"
} else {
    Write-Host "POOL_AGENTS_UNCHANGED $nodeId 는 같은 키로 이미 있다(등록 정보만 새로 넣었다)"
}
Write-Host "ADMITTED $nodeId"
$lock.Dispose()
Write-Host "NEXT Coordinator · scheduler 를 다시 띄운다. 다른 Agent 들의 GPUTEER_POOL_AGENTS 도 위 줄로 바꾸고 다시 띄운다."
