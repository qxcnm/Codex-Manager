param(
    [int]$ExpectedPid = 0
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$debug = Join-Path $root "target\debug"
$service = Join-Path $debug "codexmanager-service.exe"
$candidate = Join-Path $root "target\openruntime-hot\debug\codexmanager-service.exe"
$db = Join-Path $debug "codexmanager.db"
$logs = Join-Path $root "exports\logs"
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$backup = Join-Path $root "exports\backup\service-fast-$stamp"

function Wait-Healthy([int]$Seconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        try {
            if ((Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:48764/health" -TimeoutSec 2).StatusCode -eq 200) {
                return $true
            }
        } catch {}
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    return $false
}

function Port-OwnerPid {
    $line = netstat -ano -p TCP | Select-String -Pattern '^\s*TCP\s+\S+:48764\s+\S+\s+LISTENING\s+(\d+)\s*$' |
        Select-Object -First 1
    if (-not $line) { return 0 }
    return [int]$line.Matches[0].Groups[1].Value
}

function Wait-PortReleased([int]$Seconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        if ((Port-OwnerPid) -eq 0) { return $true }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    return $false
}

function Start-ServiceBinary {
    $env:CODEXMANAGER_DB_PATH = $db
    $env:CODEXMANAGER_SERVICE_ADDR = "localhost:48764"
    Start-Process -FilePath $service -WorkingDirectory $debug -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $logs "openruntime-service-48764.stdout.log") `
        -RedirectStandardError (Join-Path $logs "openruntime-service-48764.stderr.log") -PassThru
}

foreach ($path in @($candidate, $service, $db)) {
    if (-not (Test-Path -LiteralPath $path)) { throw "missing cutover file: $path" }
}

$ownerPid = Port-OwnerPid
if ($ExpectedPid -gt 0 -and $ownerPid -ne $ExpectedPid) {
    throw "48764 pid changed: expected=$ExpectedPid actual=$ownerPid"
}
if ($ownerPid -le 0) { throw "48764 is not listening" }
$process = Get-CimInstance Win32_Process -Filter "ProcessId=$ownerPid"
if ([IO.Path]::GetFullPath($process.ExecutablePath) -ne [IO.Path]::GetFullPath($service)) {
    throw "unexpected 48764 process path: $($process.ExecutablePath)"
}

New-Item -ItemType Directory -Force -Path $backup, $logs | Out-Null
Copy-Item -LiteralPath $service -Destination (Join-Path $backup "codexmanager-service.exe") -Force

try {
    Stop-Process -Id $ownerPid -Force
    if (-not (Wait-PortReleased 15)) { throw "48764 did not release" }
    foreach ($name in @("codexmanager.db", "codexmanager.db-wal", "codexmanager.db-shm")) {
        $source = Join-Path $debug $name
        if (Test-Path -LiteralPath $source) {
            Copy-Item -LiteralPath $source -Destination (Join-Path $backup $name) -Force
        }
    }
    Copy-Item -LiteralPath $candidate -Destination $service -Force
    $newProcess = Start-ServiceBinary
    if (-not (Wait-Healthy 30)) { throw "candidate health timeout" }
    Write-Output "service_cutover_ok backup=$backup service_pid=$($newProcess.Id)"
} catch {
    $failure = $_.Exception.Message
    $currentPid = Port-OwnerPid
    if ($currentPid -gt 0) {
        Stop-Process -Id $currentPid -Force -ErrorAction SilentlyContinue
        [void](Wait-PortReleased 10)
    }
    Copy-Item -LiteralPath (Join-Path $backup "codexmanager-service.exe") -Destination $service -Force
    foreach ($name in @("codexmanager.db", "codexmanager.db-wal", "codexmanager.db-shm")) {
        $saved = Join-Path $backup $name
        if (Test-Path -LiteralPath $saved) {
            Copy-Item -LiteralPath $saved -Destination (Join-Path $debug $name) -Force
        }
    }
    $rollback = Start-ServiceBinary
    if (-not (Wait-Healthy 30)) {
        throw "cutover failed and rollback health failed: $failure"
    }
    throw "cutover failed and rolled back: $failure rollback_pid=$($rollback.Id)"
}
