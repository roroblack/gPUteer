# gPUteer 노드 등록 정보 새로 넣기 (운영자 · Windows) — admit-node 가 받아 둔 가입 파일을 전부 지금 시각으로 다시 넣는다.
#
# ★ 왜 — 스케줄러는 등록 정보의 관측 시각이 --max-snapshot-age-ms 보다 오래되면 그 노드에 배치하지 않는다. 그 시각을 새로 하는
#   경로가 import-inventory 하나뿐이라, 이것을 돌리지 않으면 등록 하루 뒤부터 풀이 새 작업을 배치하지 않는다(결함 301).
#   운영자 기계의 작업 스케줄러에 하루 한 번(최대 나이보다 짧게) 건다:
#     schtasks /Create /TN gputeer-refresh-inventory /SC HOURLY /MO 6 /RL LIMITED `
#       /TR "powershell -NoProfile -ExecutionPolicy Bypass -Command \"& '<이 파일>' -EnvFile '<운영자 gputeer.env>' *>> '<로그 파일>'\""
#   ★ 결함 463 — 출력을 로그에 남긴다. 실패(ADMIT_INCOMPLETE · REFRESH_FAILED)는 로그와 작업 스케줄러의 "마지막 실행 결과" 로 본다.
# ★ 이것은 "노드가 살아 있다" 를 확인하지 않는다 — 운영자가 받은 선언을 다시 적을 뿐이다. 생존은 scheduler 의 --silent-after-ms 가 본다.
# ★ 결함 440 (재검수 115) — 이것은 하드웨어를 다시 읽지 않는다. 받아 둔 **선언**에 지금 시각을 찍을 뿐이라, 틀린 선언도 신선해진다.
#   그래서 **GPU 관측을 켜지 않은 노드**(CPU 전용 · 관측 전 판 Agent)용이다. 관측을 켠 노드는 인사의 관측이 신선도를 준다.
#   관측이 선언과 어긋난 노드는 이것으로 되살아나지 않는다 — Coordinator 가 불일치를 노드별로 기억하고, 맞는 관측이 올 때까지 배치에서 뺀다.
param(
    [Parameter(Mandatory = $true)][string]$EnvFile
)

$ErrorActionPreference = "Stop"
# ★ 결함 562 — 상승된 관리자 창에서 돌지 않는다(설치기의 465 와 같다). 환경 파일의 GPUTEER_BIN 은 같은 사용자가 고칠 수 있어, 상승 창이면
#   그 값이 관리자 권한으로 돈다.
$principal = New-Object System.Security.Principal.WindowsPrincipal([System.Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "REFRESH_ELEVATED: 관리자 권한(상승된 창)으로 돌리지 않는다 — 운영자의 일반 창 · /RL LIMITED 작업에서 돌린다(결함 562)"
}
$admittedDir = Join-Path (Split-Path -Parent (Resolve-Path -LiteralPath $EnvFile).Path) "admitted"
if (-not (Test-Path -LiteralPath $admittedDir -PathType Container)) { throw "REFRESH: $admittedDir 가 없다 — admit-node 로 먼저 받는다" }
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
# ★ 결함 479 (재검수 120) — 환경 파일은 잠근 **뒤에** 읽는다(admit 이 바꾸는 중인 파일을 읽지 않게).
$config = [ordered]@{}
foreach ($line in Get-Content -Encoding UTF8 $EnvFile) {
    if ($line -match '^\s*([A-Z_]+)=(.*)$') { $config[$Matches[1]] = $Matches[2].Trim() }
}
# ★ 결함 477 (재검수 120) — 사본을 바꿔 넣다 끊겨 남은 임시 파일(join-*.json.tmp)을 먼저 본다 — 활성 사본이 없으면 되살리고, 있으면 낡은 것이라 지운다.
foreach ($leftover in @(Get-ChildItem -LiteralPath $admittedDir -Filter "join-*.json.tmp" -File -ErrorAction SilentlyContinue)) {
    $target = $leftover.FullName.Substring(0, $leftover.FullName.Length - ".tmp".Length)
    if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $leftover.FullName -Force }
    else { [System.IO.File]::Move($leftover.FullName, $target); Write-Host "RECOVERED $(Split-Path -Leaf $target) — 바꿔 넣다 끊긴 임시 파일에서 되살렸다" }
}
# ★ 결함 452 (재검수 116) — admit-node 가 반입과 활성 사본 교체 사이에 끊겼으면 후보(.candidate-*)가 남는다. 그때 활성 사본(옛 선언)에 새 시각을 찍으면
#   방금 반입한 새 선언을 되돌린다 — 그 노드는 넣지 않는다(신선도가 끊겨 그 노드가 빠지는 쪽). admit-node 를 다시 돌리면 풀린다.
# ★ 결함 463 (재검수 117) — 후보가 남은 **그 노드만** 건너뛰고 나머지는 갱신한 뒤 실패로 끝낸다(전에는 후보 하나가 전체를 멈춰 풀 전체가 빠졌다).
$incomplete = @(Get-ChildItem -LiteralPath $admittedDir -Filter ".candidate-*.json" -File -Force -ErrorAction SilentlyContinue |
    ForEach-Object { $_.BaseName.Substring(".candidate-".Length) })
foreach ($node in $incomplete) {
    Write-Warning "ADMIT_INCOMPLETE $node — admitted\.candidate-$node.json 이 남아 있다. 그 노드의 가입 파일로 admit-node 를 다시 돌린다(이번 refresh 는 건너뛴다)"
}
$files = @(Get-ChildItem -LiteralPath $admittedDir -Filter "join-*.json" -File -ErrorAction SilentlyContinue)
if ($files.Count -eq 0 -and $incomplete.Count -eq 0) { throw "REFRESH: $admittedDir 에 받은 노드가 없다 — admit-node 로 먼저 받는다" }
$nowMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$failed = 0
foreach ($file in $files) {
    if ($file.BaseName.Substring("join-".Length) -in $incomplete) { continue }
    $join = Get-Content -Encoding UTF8 -Raw $file.FullName | ConvertFrom-Json
    $agent = @($join.agents)[0]
    # ★ 결함 467 — revision 은 max(지금, 이 사본의 revision + 1) · observed_at 은 지금.
    $previous = [long]$agent.inventory.inventory_revision
    $agent.inventory.inventory_revision = if ($nowMs -gt $previous) { $nowMs } else { $previous + 1 }
    $agent.inventory.observed_at_unix_ms = $nowMs
    # ★ 결함 474 (재검수 119) — 유일한 활성 사본을 제자리에서 잘라 쓰지 않는다. 임시 파일에 쓰고 바꿔 넣는다(쓰다 끊겨도 옛 사본이 남는다).
    $tmp = "$($file.FullName).tmp"
    [System.IO.File]::WriteAllText($tmp, ($join | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))
    # ★ 결함 477 (재검수 120) — PowerShell 5.1 의 Move-Item -Force 는 지우고 옮기는 두 단계라 원자적이지 않다. ReplaceFile 로 바꿔 넣는다.
    [System.IO.File]::Replace($tmp, $file.FullName, [NullString]::Value)
    & $config.GPUTEER_BIN import-inventory --inventory $file.FullName --inventory-db $config.GPUTEER_CONTROL_DB | Out-Host
    if ($LASTEXITCODE -ne 0) { $failed += 1; Write-Host "REFRESH_FAILED $($file.Name)" } else { Write-Host "REFRESHED $($agent.registry.node_id)" }
}
$lock.Dispose()
if ($incomplete.Count -gt 0) { throw "REFRESH: admit-node 가 끝나지 않은 노드가 있다($($incomplete -join ', ')) — 나머지는 갱신했다" }
if ($failed -gt 0) { throw "REFRESH: $failed 개 실패" }
