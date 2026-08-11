param([int]$DelaySeconds = 45)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$debug = Join-Path $root "target\debug"
$hot = Join-Path $root "target\openruntime-hot\debug\codexmanager-service.exe"
$hotWeb = Join-Path $root "target\openruntime-hot\debug\codexmanager-web.exe"
$service = Join-Path $debug "codexmanager-service.exe"
$web = Join-Path $debug "codexmanager-web.exe"
$db = Join-Path $debug "codexmanager.db"
$logs = Join-Path $root "exports\logs"
$marker = Join-Path $logs "openruntime-hot-activation.log"
New-Item -ItemType Directory -Force -Path $logs | Out-Null
function Log([string]$message) { "$(Get-Date -Format o) $message" | Add-Content -LiteralPath $marker }
try {
    Log "scheduled delay=$DelaySeconds"
    Start-Sleep -Seconds $DelaySeconds
    $listener = Get-NetTCPConnection -State Listen -LocalPort 48764 -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $listener) { Log "abort: service listener missing"; exit 1 }
    $process = Get-CimInstance Win32_Process -Filter "ProcessId=$($listener.OwningProcess)"
    if (-not $process -or $process.ExecutablePath -ne $service) {
        Log "abort: unexpected service path=$($process.ExecutablePath)"
        exit 1
    }
    $webListener = Get-NetTCPConnection -State Listen -LocalPort 48763 -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $webListener) { Log "abort: web listener missing"; exit 1 }
    $webProcess = Get-CimInstance Win32_Process -Filter "ProcessId=$($webListener.OwningProcess)"
    if (-not $webProcess -or $webProcess.ExecutablePath -ne $web) {
        Log "abort: unexpected web path=$($webProcess.ExecutablePath)"
        exit 1
    }
    Stop-Process -Id $webProcess.ProcessId -Force
    Stop-Process -Id $process.ProcessId -Force
    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    while ((Get-NetTCPConnection -State Listen -LocalPort 48764 -ErrorAction SilentlyContinue) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
    }
    if (Get-NetTCPConnection -State Listen -LocalPort 48764 -ErrorAction SilentlyContinue) { throw "old service did not release 48764" }
    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    while ((Get-NetTCPConnection -State Listen -LocalPort 48763 -ErrorAction SilentlyContinue) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
    }
    if (Get-NetTCPConnection -State Listen -LocalPort 48763 -ErrorAction SilentlyContinue) { throw "old web did not release 48763" }
    Copy-Item -LiteralPath $hot -Destination $service -Force
    Copy-Item -LiteralPath $hotWeb -Destination $web -Force
    $env:CODEXMANAGER_DB_PATH = $db
    $env:CODEXMANAGER_SERVICE_ADDR = "localhost:48764"
    $out = Join-Path $logs "openruntime-service-48764.stdout.log"
    $err = Join-Path $logs "openruntime-service-48764.stderr.log"
    $started = Start-Process -FilePath $service -WorkingDirectory $debug -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(18)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $response = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:48764/health" -TimeoutSec 2
            if ($response.StatusCode -eq 200) { break }
        } catch { Start-Sleep -Milliseconds 300 }
    }
    if ([DateTime]::UtcNow -ge $deadline) { throw "new service health check timed out" }

    $env:CODEXMANAGER_WEB_ADDR = "127.0.0.1:48763"
    $env:CODEXMANAGER_WEB_NO_OPEN = "1"
    $webOut = Join-Path $logs "openruntime-web-48763.stdout.log"
    $webErr = Join-Path $logs "openruntime-web-48763.stderr.log"
    $startedWeb = Start-Process -FilePath $web -WorkingDirectory $debug -WindowStyle Hidden -RedirectStandardOutput $webOut -RedirectStandardError $webErr -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(18)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $response = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:48763/api/runtime" -TimeoutSec 2
            if ($response.StatusCode -eq 200) {
                Log "activated service_pid=$($started.Id) web_pid=$($startedWeb.Id)"
                exit 0
            }
        } catch { Start-Sleep -Milliseconds 300 }
    }
    throw "new web health check timed out"
} catch {
    Log "failed: $($_.Exception.Message)"
    exit 1
}
