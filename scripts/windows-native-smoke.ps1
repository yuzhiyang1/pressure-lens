param(
  [Parameter(Mandatory = $true)]
  [string]$Executable
)

$ErrorActionPreference = "Stop"
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$primary = Start-Process -FilePath $resolvedExecutable -WindowStyle Hidden -PassThru
try {
  Start-Sleep -Seconds 10
  if ($primary.HasExited) {
    throw "首个实例异常退出，退出码 $($primary.ExitCode)"
  }

  $secondary = Start-Process -FilePath $resolvedExecutable -WindowStyle Hidden -PassThru
  if (-not $secondary.WaitForExit(10000)) {
    Stop-Process -Id $secondary.Id -Force -ErrorAction SilentlyContinue
    throw "第二个实例未在 10 秒内退出，单实例保护失效"
  }

  $dataDirectory = Join-Path $env:LOCALAPPDATA "dev.pressurelens.desktop"
  $database = Join-Path $dataDirectory "pressure-lens.sqlite3"
  if (-not (Test-Path -LiteralPath $database)) {
    throw "本地 Journal 未创建：$database"
  }

  $log = Get-ChildItem -LiteralPath (Join-Path $dataDirectory "logs") -File -ErrorAction SilentlyContinue |
    Where-Object Name -Like "pressure-lens*" |
    Select-Object -First 1
  if (-not $log) {
    throw "文件日志未创建"
  }

  [pscustomobject]@{
    primaryPid = $primary.Id
    secondaryExited = $secondary.HasExited
    database = $database
    log = $log.FullName
  } | ConvertTo-Json
}
finally {
  if (-not $primary.HasExited) {
    # 只清理由脚本创建的进程；异常退出恢复由 Rust 测试覆盖。
    Stop-Process -Id $primary.Id -Force -ErrorAction SilentlyContinue
  }
}
