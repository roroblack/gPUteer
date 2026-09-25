# gPUteer 노드 설치 (팀원 PC · Windows) — 초대 파일 하나로 GPU 한 장을 풀에 붙일 준비를 한다.
#
# 하는 일 (런북 §1 · §2 · §5 · §9 를 한 번에)
#   1. 키를 만든다(이미 있으면 그대로 쓴다 — 덮지 않는다. 잃으면 이 노드의 신원이 바뀐다)
#   2. 설정 파일 두 개를 쓴다(gputeer.env · agent-<노드>.env) — 이미 있으면 _backup\<시각>\ 에 먼저 복사한다
#   3. node-doctor 로 점검한다 — FAIL 이 있으면 여기서 멈춘다(무엇을 고칠지 그대로 보여 준다)
#   4. 운영자에게 보낼 가입 파일(join-<노드>.json)을 만든다 — 공개키와 이 노드가 내놓는 자원. 비밀은 들어가지 않는다
#   5. -Register 를 주면 로그온 시 Agent 가 뜨도록 작업 스케줄러에 등록한다(그 PC 주인의 사용자로 — §0.1)
#
# 예:
#   powershell -NoProfile -ExecutionPolicy Bypass -File install-node.ps1 -Invite .\invite.env `
#     -NodeId alice-gpu0 -OwnerMemberId alice -GpuPin 0 -CpuCores 8 -RamGiB 16 -WorkspaceGiB 100 `
#     -ContainerRuntime docker -ContainerRuntimeKind docker -ContainerGpu
#
# ★ 이 스크립트가 쓰는 파일은 전부 저장소 **밖**(기본 %LOCALAPPDATA%\gputeer)이다. 채운 파일을 저장소에 넣지 않는다.
# ★ 자원 수치(CPU · RAM · 디스크)는 **소유자가 내놓는 양**이다 — 기계 전체가 아니다. 지어내지 않으려고 기본값을 두지 않는다.
param(
    [Parameter(Mandatory = $true)][string]$Invite,
    [Parameter(Mandatory = $true)][string]$NodeId,
    [Parameter(Mandatory = $true)][string]$OwnerMemberId,
    [Parameter(Mandatory = $true)][string]$GpuPin,
    [Parameter(Mandatory = $true)][int]$CpuCores,
    [Parameter(Mandatory = $true)][int]$RamGiB,
    [Parameter(Mandatory = $true)][int]$WorkspaceGiB,
    [int]$OwnerPanelPort = 7610,
    [string]$ConfigDir = (Join-Path $env:LOCALAPPDATA "gputeer"),
    [string]$NodeDir = "",
    [string]$Bin = "",
    [string]$SharedRoot = "",
    [string]$ContainerRuntime = "",
    [string]$ContainerRuntimeKind = "",
    [switch]$ContainerGpu,
    # gpus | cdi | cdi-all (결함 303 — WSL2 docker 는 cdi-all 만 됐다). 비우면 런타임 기본값
    [string]$ContainerGpuRequest = "",
    [string]$GpuProbeImage = "",
    [string]$WorkloadClasses = "TRAINING",
    [string]$GpuModel = "",
    [long]$GpuVramMiB = 0,
    # 시드는 평문 파일이다(K0). 더 높게 적으려면 그 보호를 실제로 걸었을 때만 바꾼다.
    [string]$KeyProtection = "K0",
    # 인사에 GPU 관측을 싣지 않는다 — Coordinator 가 아직 옛 판일 때만(런북 §5 · 결함 301)
    [switch]$NoAttestGpus,
    [switch]$Register
)

$ErrorActionPreference = "Stop"

function Read-EnvFile([string]$path) {
    $map = [ordered]@{}
    foreach ($line in Get-Content -Encoding UTF8 $path) {
        if ($line -match '^\s*([A-Z_]+)=(.*)$') { $map[$Matches[1]] = $Matches[2].Trim() }
    }
    return $map
}

function Backup-IfExists([string]$path, [string]$stamp) {
    if (Test-Path -LiteralPath $path) {
        $dir = Join-Path (Split-Path -Parent $path) (Join-Path "_backup" $stamp)
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Copy-Item -LiteralPath $path -Destination $dir
        Write-Host "BACKUP $path -> $dir"
    }
}

function Write-Utf8NoBom([string]$path, [string[]]$lines) {
    [System.IO.File]::WriteAllText($path, (($lines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
}

# 노드 id 는 파일 이름 · 작업 이름 · 라벨에 들어간다 — 좁힌다.
if ($NodeId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$') { throw "INSTALL_ARGS: -NodeId 는 영문 · 숫자 · . _ - 만(64자 이하) — 받은 값 '$NodeId'" }
if ($OwnerMemberId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$') { throw "INSTALL_ARGS: -OwnerMemberId 형식 오류 '$OwnerMemberId'" }
if ($GpuPin -notmatch '^[0-9]+(,[0-9]+)*$') { throw "INSTALL_ARGS: -GpuPin 은 장치 번호다(예 0 · 0,1) — 받은 값 '$GpuPin'" }
if ($CpuCores -le 0 -or $RamGiB -le 0 -or $WorkspaceGiB -le 0) { throw "INSTALL_ARGS: -CpuCores · -RamGiB · -WorkspaceGiB 는 0 보다 커야 한다" }
if (($ContainerRuntime -eq "") -ne ($ContainerRuntimeKind -eq "")) { throw "INSTALL_ARGS: -ContainerRuntime 과 -ContainerRuntimeKind 는 함께 준다" }
if ($ContainerRuntimeKind -ne "" -and $ContainerRuntimeKind -notin @("podman", "docker")) { throw "INSTALL_ARGS: -ContainerRuntimeKind 는 podman 또는 docker" }
if ($ContainerGpu -and $ContainerRuntime -eq "") { throw "INSTALL_ARGS: -ContainerGpu 는 -ContainerRuntime 과 함께 준다" }
if ($ContainerGpuRequest -ne "" -and $ContainerGpuRequest -notin @("gpus", "cdi", "cdi-all")) { throw "INSTALL_ARGS: -ContainerGpuRequest 는 gpus · cdi · cdi-all" }
if ($ContainerGpuRequest -ne "" -and -not $ContainerGpu) { throw "INSTALL_ARGS: -ContainerGpuRequest 는 -ContainerGpu 와 함께 준다" }
if ($GpuProbeImage -ne "" -and -not $ContainerGpu) { throw "INSTALL_ARGS: -GpuProbeImage 는 -ContainerGpu 와 함께 준다" }
if ($KeyProtection -notin @("K0", "K1", "K2")) { throw "INSTALL_ARGS: -KeyProtection 은 K0 · K1 · K2" }

$inviteMap = Read-EnvFile $Invite
foreach ($key in @("GPUTEER_INVITE_VERSION", "GPUTEER_CONNECT", "GPUTEER_COORDINATOR_ID", "GPUTEER_COORDINATOR_PUBKEY",
        "GPUTEER_SUBMITTER_PUBKEY", "GPUTEER_SHARED_ROOT")) {
    if (-not $inviteMap.Contains($key) -or $inviteMap[$key] -eq "") { throw "INVITE_REJECTED: 초대 파일에 $key 가 없다" }
}
if ($inviteMap.GPUTEER_INVITE_VERSION -ne "1") { throw "INVITE_REJECTED: 모르는 초대 판 $($inviteMap.GPUTEER_INVITE_VERSION)" }

if ($Bin -eq "") { $Bin = (Get-Command gputeer -ErrorAction SilentlyContinue).Source }
if (-not $Bin -or -not (Test-Path -LiteralPath $Bin)) { throw "INSTALL_ARGS: gputeer 실행 파일이 없다 — -Bin <경로> 로 준다(빌드: cargo build --release -p gputeer-cli)" }
$Bin = (Resolve-Path -LiteralPath $Bin).Path
if ($NodeDir -eq "") { $NodeDir = Join-Path $ConfigDir (Join-Path "nodes" $NodeId) }
if ($SharedRoot -eq "") { $SharedRoot = $inviteMap.GPUTEER_SHARED_ROOT }

New-Item -ItemType Directory -Force -Path $ConfigDir, $NodeDir | Out-Null
$stamp = Get-Date -Format "yyyy-MM-dd_HHmm"

# 1. 키 — 있으면 쓰고, 없으면 만든다.
$seed = Join-Path $ConfigDir "$NodeId.seed"
if (Test-Path -LiteralPath $seed) {
    Write-Host "SEED_KEPT $seed"
} else {
    & $Bin keygen --out $seed | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "keygen 실패" }
}

# 2. 설정 파일.
$poolAgents = if ($inviteMap.Contains("GPUTEER_POOL_AGENTS")) { $inviteMap.GPUTEER_POOL_AGENTS } else { "" }
$common = Join-Path $ConfigDir "gputeer.env"
$agentEnv = Join-Path $ConfigDir "agent-$NodeId.env"
Backup-IfExists $common $stamp
Backup-IfExists $agentEnv $stamp
Write-Utf8NoBom $common @(
    "# install-node.ps1 이 $stamp 에 썼다. 저장소에 넣지 않는다.",
    "GPUTEER_BIN=$Bin",
    "GPUTEER_CONNECT=$($inviteMap.GPUTEER_CONNECT)",
    "GPUTEER_COORDINATOR_ID=$($inviteMap.GPUTEER_COORDINATOR_ID)",
    "GPUTEER_COORDINATOR_PUBKEY=$($inviteMap.GPUTEER_COORDINATOR_PUBKEY)",
    "GPUTEER_SUBMITTER_PUBKEY=$($inviteMap.GPUTEER_SUBMITTER_PUBKEY)",
    "GPUTEER_SHARED_ROOT=$SharedRoot",
    "GPUTEER_POOL_AGENTS=$poolAgents"
)
Write-Utf8NoBom $agentEnv @(
    "GPUTEER_NODE_ID=$NodeId",
    "GPUTEER_NODE_SEED_FILE=$seed",
    "GPUTEER_NODE_DIR=$NodeDir",
    "GPUTEER_GPU_PIN=$GpuPin",
    "GPUTEER_OWNER_PANEL_PORT=$OwnerPanelPort",
    "GPUTEER_ATTEST_GPUS=$(if ($NoAttestGpus) { '' } else { 'true' })",
    "GPUTEER_CONTAINER_RUNTIME=$ContainerRuntime",
    "GPUTEER_CONTAINER_RUNTIME_KIND=$ContainerRuntimeKind",
    "GPUTEER_CONTAINER_GPU=$(if ($ContainerGpu) { 'true' } else { '' })",
    "GPUTEER_CONTAINER_GPU_REQUEST=$ContainerGpuRequest"
)
Write-Host "CONFIG_WRITTEN $common $agentEnv"

# 3. 점검.
$doctorArgs = @("node-doctor", "--seed-file", $seed, "--node-dir", $NodeDir, "--connect", $inviteMap.GPUTEER_CONNECT,
    "--shared-checkpoint-root", $SharedRoot, "--owner-panel-port", "$OwnerPanelPort", "--gpu-pin", $GpuPin)
if ($ContainerRuntime -ne "") { $doctorArgs += @("--container-runtime", $ContainerRuntime, "--container-runtime-kind", $ContainerRuntimeKind) }
if ($ContainerGpuRequest -ne "") { $doctorArgs += @("--container-gpu-request", $ContainerGpuRequest) }
if ($GpuProbeImage -ne "") { $doctorArgs += @("--container-gpu-probe-image", $GpuProbeImage) }
$doctor = & $Bin @doctorArgs 2>&1 | ForEach-Object { "$_" }
$doctorExit = $LASTEXITCODE
$doctor | Out-Host
if ($doctorExit -ne 0) { throw "NODE_DOCTOR_FAILED: 위 FAIL 을 고치고 같은 명령을 다시 돌린다(키 · 설정은 그대로 이어 쓴다)" }
$seedLine = $doctor | Where-Object { $_ -like "CHECK seed *" } | Select-Object -First 1
if ($seedLine -notmatch '([0-9a-f]{64})') { throw "node-doctor 출력에서 공개키를 읽지 못했다: $seedLine" }
$publicKey = $Matches[1]

# 4. 가입 파일 — GPU 는 nvidia-smi 로 읽는다(못 읽으면 -GpuModel · -GpuVramMiB 로 준다. 지어내지 않는다).
$gpus = @()
foreach ($index in $GpuPin.Split(",")) {
    $model = $GpuModel; $vramMiB = $GpuVramMiB
    if ($model -eq "" -or $vramMiB -le 0) {
        $smi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
        if (-not $smi) { throw "GPU_UNKNOWN: nvidia-smi 가 없다 — -GpuModel · -GpuVramMiB 로 직접 준다" }
        $row = & $smi.Source --query-gpu=name,memory.total --format=csv,noheader,nounits -i $index
        if ($LASTEXITCODE -ne 0 -or -not $row) { throw "GPU_UNKNOWN: nvidia-smi 가 GPU $index 를 읽지 못했다" }
        $parts = "$row".Split(",")
        if ($model -eq "") { $model = $parts[0].Trim() }
        if ($vramMiB -le 0) { $vramMiB = [long]$parts[1].Trim() }
    }
    $gpus += [ordered]@{ gpu_id = "$NodeId-gpu-$index"; model = $model; healthy = $true; available_vram_bytes = $vramMiB * 1MB }
}
$isContained = $ContainerRuntime -ne ""
$nowMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$join = [ordered]@{
    schema_version = 1
    agents = @([ordered]@{
        registry = [ordered]@{
            node_id = $NodeId; device_id = $NodeId; owner_member_id = $OwnerMemberId; verifying_key_hex = $publicKey
            node_state = "ONLINE"; risk_state = "NORMAL"
            # 컨테이너만 받는 노드 = S3 · CONTAINED. 호스트에서 돌리는 노드 = S0 · RESTRICTED(같은 사용자 권한 · 메모리 상한뿐 — 결함 267)
            security_tier = $(if ($isContained) { "S3" } else { "S0" })
            isolation_class = $(if ($isContained) { "CONTAINED" } else { "RESTRICTED" })
            key_protection = $KeyProtection
        }
        inventory = [ordered]@{
            inventory_revision = $nowMs; observed_at_unix_ms = $nowMs
            gpus = $gpus
            available_cpu_cores = $CpuCores; available_ram_bytes = [long]$RamGiB * 1GB; available_workspace_bytes = [long]$WorkspaceGiB * 1GB
            allowed_workload_classes = @($WorkloadClasses.Split(",") | ForEach-Object { $_.Trim() })
            third_party_workloads_opt_in = $true
        }
    })
}
$joinPath = Join-Path $ConfigDir "join-$NodeId.json"
Backup-IfExists $joinPath $stamp
[System.IO.File]::WriteAllText($joinPath, ($join | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "JOIN_FILE $joinPath"
Write-Host "PUBLIC_KEY $publicKey"

# 5. 등록 — 로그온 시, 이 사용자로(Owner Panel 이 이 사용자에게 보여야 한다).
$agentScript = Join-Path (Split-Path -Parent $PSScriptRoot) "agent.ps1"
$taskName = "gputeer-agent-$NodeId"
$taskRun = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$agentScript`" -EnvFile `"$common`" -AgentEnvFile `"$agentEnv`""
if ($Register) {
    & schtasks /Create /F /TN $taskName /SC ONLOGON /RL LIMITED /TR $taskRun | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "schtasks 등록 실패" }
    Write-Host "REGISTERED $taskName (다음 로그온부터 뜬다 · 지금 띄우려면: schtasks /Run /TN $taskName)"
} else {
    Write-Host "NOT_REGISTERED — 등록하려면 -Register 를 붙여 다시 돌리거나: schtasks /Create /TN $taskName /SC ONLOGON /RL LIMITED /TR '$taskRun'"
}
Write-Host "NEXT 운영자에게 $joinPath 를 보낸다(비밀 없음). 운영자가 admit-node 로 받은 뒤 Agent 가 일을 받는다."
