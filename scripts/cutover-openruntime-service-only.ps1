param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$debug = Join-Path $root "target\debug"
$candidate = Join-Path $root "target\openruntime-hot\debug\codexmanager-service.exe"
$service = Join-Path $debug "codexmanager-service.exe"
$db = Join-Path $debug "codexmanager.db"
$logs = Join-Path $root "exports\logs"
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$backup = Join-Path $root "exports\backup\service-only-$stamp"

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

function Current-ServiceProcess {
    $listener = Get-NetTCPConnection -State Listen -LocalPort 48764 -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $listener) { return $null }
    return Get-CimInstance Win32_Process -Filter "ProcessId=$($listener.OwningProcess)"
}

function Stop-ServiceOnly {
    $process = Current-ServiceProcess
    if (-not $process) { return }
    if ($process.ExecutablePath -ne $service) {
        throw "unexpected 48764 process path: $($process.ExecutablePath)"
    }
    Stop-Process -Id $process.ProcessId -Force
    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    do {
        if (-not (Get-NetTCPConnection -State Listen -LocalPort 48764 -ErrorAction SilentlyContinue)) {
            return
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "48764 did not release"
}

function Start-ServiceOnly {
    $env:CODEXMANAGER_DB_PATH = $db
    $env:CODEXMANAGER_SERVICE_ADDR = "localhost:48764"
    $process = Start-Process -FilePath $service -WorkingDirectory $debug -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $logs "openruntime-service-48764.stdout.log") `
        -RedirectStandardError (Join-Path $logs "openruntime-service-48764.stderr.log") -PassThru
    if (-not (Wait-Healthy "http://127.0.0.1:48764/health" 25)) {
        throw "48764 health timeout"
    }
    return $process.Id
}

foreach ($path in @($candidate, $service, $db)) {
    if (-not (Test-Path -LiteralPath $path)) { throw "missing cutover file: $path" }
}
$resolvedDebug = (Resolve-Path -LiteralPath $debug).Path.TrimEnd('\')
$expectedDebug = [IO.Path]::GetFullPath($debug).TrimEnd('\')
if ($resolvedDebug -ne $expectedDebug) { throw "unexpected debug directory: $resolvedDebug" }

New-Item -ItemType Directory -Force -Path $backup, $logs | Out-Null
Copy-Item -LiteralPath $service -Destination (Join-Path $backup "codexmanager-service.exe") -Force

try {
    Stop-ServiceOnly
    foreach ($name in @("codexmanager.db", "codexmanager.db-wal", "codexmanager.db-shm")) {
        $source = Join-Path $debug $name
        if (Test-Path -LiteralPath $source) {
            Copy-Item -LiteralPath $source -Destination (Join-Path $backup $name) -Force
        }
    }
    Copy-Item -LiteralPath $candidate -Destination $service -Force
    # $PID is PowerShell's built-in read-only current-process variable
    # (variable names are case-insensitive). Using $pid here made every
    # successful candidate startup enter the rollback path.
    $servicePid = Start-ServiceOnly
    if (-not (Wait-Healthy "http://127.0.0.1:48763/api/runtime" 10)) {
        throw "existing 48763 web shell cannot reach new service"
    }
    Write-Output "service_cutover_ok backup=$backup service_pid=$servicePid"
} catch {
    $failure = $_.Exception.Message
    try { Stop-ServiceOnly } catch {}
    Copy-Item -LiteralPath (Join-Path $backup "codexmanager-service.exe") -Destination $service -Force
    foreach ($name in @("codexmanager.db", "codexmanager.db-wal", "codexmanager.db-shm")) {
        $target = Join-Path $debug $name
        $saved = Join-Path $backup $name
        if (Test-Path -LiteralPath $saved) {
            Copy-Item -LiteralPath $saved -Destination $target -Force
        } elseif (Test-Path -LiteralPath $target) {
            Remove-Item -LiteralPath $target -Force
        }
    }
    $rollbackPid = Start-ServiceOnly
    throw "service cutover failed and rolled back: $failure rollback_pid=$rollbackPid"
}
