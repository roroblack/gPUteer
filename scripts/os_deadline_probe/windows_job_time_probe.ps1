# Does the Windows kernel enforce JOB_OBJECT_LIMIT_JOB_TIME with no live supervisor?
#   KILL_ON_JOB_CLOSE is deliberately NOT set - otherwise we could not tell
#   whether the kernel timer killed it or the closing handle did.
$ErrorActionPreference = "Stop"
$W = "E:\gputeer-work\wintime"
New-Item -ItemType Directory -Force -Path $W | Out-Null

Add-Type -Namespace GP -Name Job -MemberDefinition @'
[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern IntPtr CreateJobObjectW(IntPtr a, string name);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint len);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr proc);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool CloseHandle(IntPtr h);
'@

function New-JobCpuLimit([double]$seconds) {
  $job = [GP.Job]::CreateJobObjectW([IntPtr]::Zero, $null)
  if ($job -eq [IntPtr]::Zero) { throw "CreateJobObject failed" }
  $size = 144
  $buf = [Runtime.InteropServices.Marshal]::AllocHGlobal($size)
  [Runtime.InteropServices.Marshal]::Copy((New-Object byte[] $size), 0, $buf, $size)
  [Runtime.InteropServices.Marshal]::WriteInt64($buf, 8, [long]($seconds * 10000000))
  [Runtime.InteropServices.Marshal]::WriteInt32($buf, 16, 0x00000004)
  $ok = [GP.Job]::SetInformationJobObject($job, 9, $buf, $size)
  $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
  [Runtime.InteropServices.Marshal]::FreeHGlobal($buf)
  if (-not $ok) { throw "SetInformationJobObject failed err=$err" }
  return $job
}

$burn = Join-Path $W "burn.ps1"
Set-Content -Path $burn -Encoding ASCII -Value @'
$e = (Get-Date).AddSeconds(120)
$x = 0.0
while ((Get-Date) -lt $e) { for ($i=0; $i -lt 2000000; $i++) { $x = $x + [math]::Sqrt($i) } }
'@

$short = Join-Path $W "short.ps1"
Set-Content -Path $short -Encoding ASCII -Value @'
$e = (Get-Date).AddSeconds(3)
$x = 0.0
while ((Get-Date) -lt $e) { for ($i=0; $i -lt 2000000; $i++) { $x = $x + [math]::Sqrt($i) } }
'@

Write-Output "=== A. CONTROL - burn CPU with no limit, watch 8s ==="
Write-Output "    if it does not run here the experiment is meaningless"
$p = Start-Process powershell -ArgumentList "-NoProfile","-File",$burn -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 8
$p.Refresh()
Write-Output "    alive after 8s : $(-not $p.HasExited)   (must be True)"
Write-Output "    cpu seconds    : $([math]::Round($p.TotalProcessorTime.TotalSeconds,1))"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue

Write-Output ""
Write-Output "=== B. MAIN - cpu limit 5s, then the launcher lets go ==="
Write-Output "    KILL_ON_JOB_CLOSE is NOT set. Only the timer can kill it."
$job = New-JobCpuLimit 5
$p2 = Start-Process powershell -ArgumentList "-NoProfile","-File",$burn -PassThru -WindowStyle Hidden
$assigned = [GP.Job]::AssignProcessToJobObject($job, $p2.Handle)
Write-Output "    assigned to job: $assigned  (pid $($p2.Id))"
[void][GP.Job]::CloseHandle($job)
Write-Output "    handle closed - only the kernel holds this job now"
$sw = [Diagnostics.Stopwatch]::StartNew()
$died = $false
while ($sw.Elapsed.TotalSeconds -lt 40) {
  Start-Sleep -Milliseconds 500
  $p2.Refresh()
  if ($p2.HasExited) { $died = $true; break }
}
if ($died) {
  Write-Output "    KILLED after $([math]::Round($sw.Elapsed.TotalSeconds,1))s  exitcode=$($p2.ExitCode)"
  Write-Output "    (1816 = ERROR_NOT_ENOUGH_QUOTA -> the kernel timer did it)"
} else {
  Write-Output "    STILL ALIVE after 40s - the kernel timer did NOT fire"
  Stop-Process -Id $p2.Id -Force -ErrorAction SilentlyContinue
}

Write-Output ""
Write-Output "=== C. CONTROL - generous limit must not disturb a short job ==="
Write-Output "    cpu limit 60s, job runs 3s"
$job3 = New-JobCpuLimit 60
$p3 = Start-Process powershell -ArgumentList "-NoProfile","-File",$short -PassThru -WindowStyle Hidden
[void][GP.Job]::AssignProcessToJobObject($job3, $p3.Handle)
[void][GP.Job]::CloseHandle($job3)
$p3.WaitForExit(40000) | Out-Null
$p3.Refresh()
Write-Output "    exitcode: $($p3.ExitCode)   (0 = finished undisturbed)"

Write-Output ""
Write-Output "=== DONE ==="
