param([switch]$NoPopup)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$debug = Join-Path $root "target\debug"
$hot = Join-Path $root "target\openruntime-hot\debug"
$logs = Join-Path $root "exports\logs"
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$backup = Join-Path $root "exports\backup\hot-cutover-$stamp"
$service = Join-Path $debug "codexmanager-service.exe"
$web = Join-Path $debug "codexmanager-web.exe"
$db = Join-Path $debug "codexmanager.db"
$hotService = Join-Path $hot "codexmanager-service.exe"
$hotWeb = Join-Path $hot "codexmanager-web.exe"

function Wait-Healthy([string]$Url, [int]$Seconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        try {
            if ((Invoke-WebRequest -UseBasicParsing $Url -TimeoutSec 2).StatusCode -eq 200) {
                return $true
            }
        } catch {}
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    return $false
}

function Stop-LiveProcesses {
    $expected = @($service, $web)
    Get-CimInstance Win32_Process |
        Where-Object { $expected -contains $_.ExecutablePath } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $listeners = Get-NetTCPConnection -State Listen -LocalPort 48763,48764 -ErrorAction SilentlyContinue
        if (-not $listeners) { return }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "live ports not released"
}

function Start-LiveProcesses {
    $env:CODEXMANAGER_DB_PATH = $db
    $env:CODEXMANAGER_SERVICE_ADDR = "localhost:48764"
    $serviceProcess = Start-Process -FilePath $service -WorkingDirectory $debug -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $logs "openruntime-service-48764.stdout.log") `
        -RedirectStandardError (Join-Path $logs "openruntime-service-48764.stderr.log") -PassThru
    if (-not (Wait-Healthy "http://127.0.0.1:48764/health" 20)) {
        throw "service health timeout"
    }
    $env:CODEXMANAGER_WEB_ADDR = "127.0.0.1:48763"
    $env:CODEXMANAGER_WEB_NO_OPEN = "1"
    $webProcess = Start-Process -FilePath $web -WorkingDirectory $debug -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $logs "openruntime-web-48763.stdout.log") `
        -RedirectStandardError (Join-Path $logs "openruntime-web-48763.stderr.log") -PassThru
    if (-not (Wait-Healthy "http://127.0.0.1:48763/api/runtime" 20)) {
        throw "web health timeout"
    }
    return @($serviceProcess.Id, $webProcess.Id)
}

foreach ($path in @($service, $web, $db, $hotService, $hotWeb)) {
    if (-not (Test-Path -LiteralPath $path)) { throw "missing cutover file: $path" }
}
New-Item -ItemType Directory -Force -Path $backup, $logs | Out-Null
# Windows permits reading a running executable, so rollback files are ready
# before the live processes are touched.
Copy-Item -LiteralPath $service -Destination (Join-Path $backup "codexmanager-service.exe") -Force
Copy-Item -LiteralPath $web -Destination (Join-Path $backup "codexmanager-web.exe") -Force

try {
    Stop-LiveProcesses
    Copy-Item -LiteralPath $db -Destination (Join-Path $backup "codexmanager.db") -Force
    if (Test-Path -LiteralPath "$db-wal") { Copy-Item -LiteralPath "$db-wal" -Destination (Join-Path $backup "codexmanager.db-wal") -Force }
    if (Test-Path -LiteralPath "$db-shm") { Copy-Item -LiteralPath "$db-shm" -Destination (Join-Path $backup "codexmanager.db-shm") -Force }
    Copy-Item -LiteralPath $hotService -Destination $service -Force
    Copy-Item -LiteralPath $hotWeb -Destination $web -Force
    $pids = Start-LiveProcesses
    Write-Output "cutover_ok backup=$backup service_pid=$($pids[0]) web_pid=$($pids[1])"
} catch {
    $failure = $_.Exception.Message
    try { Stop-LiveProcesses } catch {}
    Copy-Item -LiteralPath (Join-Path $backup "codexmanager-service.exe") -Destination $service -Force
    Copy-Item -LiteralPath (Join-Path $backup "codexmanager-web.exe") -Destination $web -Force
    $pids = Start-LiveProcesses
    throw "cutover failed and rolled back: $failure rollback_service_pid=$($pids[0]) rollback_web_pid=$($pids[1])"
}
