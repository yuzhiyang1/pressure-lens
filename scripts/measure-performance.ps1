param(
  [Parameter(Mandatory = $true)]
  [string]$Executable,
  [int]$WarmupSeconds = 30,
  [int]$SampleSeconds = 30,
  [double]$MaxCpuPercent = 3.0,
  [double]$MaxPrivateMemoryMb = 450,
  [double]$MaxWorkingSetMb = 700,
  [double]$MaxGrowthMb = 30,
  [switch]$KeepRunning
)

$ErrorActionPreference = "Stop"
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path

function Get-ProcessTreeIds {
  param([int]$RootId)

  $all = Get-CimInstance Win32_Process
  $ids = [System.Collections.Generic.HashSet[int]]::new()
  $ids.Add($RootId) | Out-Null
  do {
    $changed = $false
    foreach ($process in $all) {
      if ($ids.Contains([int]$process.ParentProcessId) -and $ids.Add([int]$process.ProcessId)) {
        $changed = $true
      }
    }
  } while ($changed)
  return @($ids)
}

function Get-TreeMetrics {
  param([int]$RootId)

  $ids = Get-ProcessTreeIds -RootId $RootId
  $processes = foreach ($id in $ids) {
    Get-Process -Id $id -ErrorAction SilentlyContinue
  }
  [pscustomobject]@{
    CpuMilliseconds = (($processes | Measure-Object -Property CPU -Sum).Sum * 1000)
    PrivateMemoryMb = [math]::Round((($processes | Measure-Object -Property PrivateMemorySize64 -Sum).Sum / 1MB), 2)
    WorkingSetMb = [math]::Round((($processes | Measure-Object -Property WorkingSet64 -Sum).Sum / 1MB), 2)
    ProcessCount = @($processes).Count
  }
}

$root = Start-Process -FilePath $resolvedExecutable -WindowStyle Hidden -PassThru
try {
  Start-Sleep -Seconds $WarmupSeconds
  if ($root.HasExited) {
    throw "Pressure Lens 在性能采样前异常退出，退出码 $($root.ExitCode)"
  }

  $baseline = Get-TreeMetrics -RootId $root.Id
  $startedAt = [datetime]::UtcNow
  $peakPrivate = $baseline.PrivateMemoryMb
  $peakWorkingSet = $baseline.WorkingSetMb

  for ($index = 0; $index -lt $SampleSeconds; $index += 2) {
    Start-Sleep -Seconds ([math]::Min(2, $SampleSeconds - $index))
    $sample = Get-TreeMetrics -RootId $root.Id
    $peakPrivate = [math]::Max($peakPrivate, $sample.PrivateMemoryMb)
    $peakWorkingSet = [math]::Max($peakWorkingSet, $sample.WorkingSetMb)
  }

  $finishedAt = [datetime]::UtcNow
  $final = Get-TreeMetrics -RootId $root.Id
  $elapsedSeconds = [math]::Max(($finishedAt - $startedAt).TotalSeconds, 0.001)
  $cpuSeconds = ($final.CpuMilliseconds - $baseline.CpuMilliseconds) / 1000
  $cpuPercent = [math]::Round(
    ($cpuSeconds / $elapsedSeconds / [Environment]::ProcessorCount * 100),
    2
  )
  $growthMb = [math]::Round($final.PrivateMemoryMb - $baseline.PrivateMemoryMb, 2)

  $result = [ordered]@{
    cpuPercent = $cpuPercent
    peakPrivateMemoryMb = $peakPrivate
    peakWorkingSetMb = $peakWorkingSet
    privateMemoryGrowthMb = $growthMb
    processCount = $final.ProcessCount
    sampleSeconds = $SampleSeconds
    budgets = [ordered]@{
      maxCpuPercent = $MaxCpuPercent
      maxPrivateMemoryMb = $MaxPrivateMemoryMb
      maxWorkingSetMb = $MaxWorkingSetMb
      maxGrowthMb = $MaxGrowthMb
    }
  }

  New-Item -ItemType Directory -Force output | Out-Null
  $result | ConvertTo-Json -Depth 4 | Set-Content output\performance-baseline.json -Encoding utf8
  $result | ConvertTo-Json -Depth 4

  if (
    $cpuPercent -gt $MaxCpuPercent -or
    $peakPrivate -gt $MaxPrivateMemoryMb -or
    $peakWorkingSet -gt $MaxWorkingSetMb -or
    $growthMb -gt $MaxGrowthMb
  ) {
    throw "常驻性能超出预算，请检查 output/performance-baseline.json"
  }
}
finally {
  if (-not $KeepRunning -and -not $root.HasExited) {
    # 只终止本脚本启动的精确 PID；CI 环境没有用户数据需要保留。
    Stop-Process -Id $root.Id -Force -ErrorAction SilentlyContinue
  }
}
