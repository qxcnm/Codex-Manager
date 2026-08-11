param(
    [int]$Port = 48765
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$stage = Join-Path $root "target\openruntime-smoke"
$backup = Join-Path $root "exports\backup\service-only-20260803-105740"
$candidate = Join-Path $root "target\openruntime-hot\debug\codexmanager-service.exe"

New-Item -ItemType Directory -Force -Path $stage | Out-Null
foreach ($name in @("codexmanager.db", "codexmanager.db-wal", "codexmanager.db-shm")) {
    $source = Join-Path $backup $name
    if (Test-Path -LiteralPath $source) {
        Copy-Item -LiteralPath $source -Destination (Join-Path $stage $name) -Force
    }
}

$env:CODEXMANAGER_DB_PATH = Join-Path $stage "codexmanager.db"
$env:CODEXMANAGER_SERVICE_ADDR = "127.0.0.1:$Port"
$process = Start-Process -FilePath $candidate -WorkingDirectory $stage -WindowStyle Hidden `
    -RedirectStandardOutput (Join-Path $stage "stdout.log") `
    -RedirectStandardError (Join-Path $stage "stderr.log") -PassThru

Start-Sleep -Seconds 3
Write-Output "candidate_pid=$($process.Id) exited=$($process.HasExited)"
try {
    $response = Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$Port/health" -TimeoutSec 5
    Write-Output "health_status=$($response.StatusCode)"
} catch {
    Write-Output "health_error=$($_.Exception.Message)"
}
Write-Output "stderr:"
Get-Content -LiteralPath (Join-Path $stage "stderr.log") -Tail 100 -ErrorAction SilentlyContinue
Write-Output "stdout:"
Get-Content -LiteralPath (Join-Path $stage "stdout.log") -Tail 30 -ErrorAction SilentlyContinue
