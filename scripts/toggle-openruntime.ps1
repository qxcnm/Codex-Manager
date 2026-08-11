param(
    [ValidateSet("Toggle", "Start", "Stop", "Status")]
    [string]$Action = "Toggle",
    [switch]$NoPopup
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$TargetDir = Join-Path $Root "target\debug"
$ExportDir = Join-Path $Root "exports\logs"
$ServiceExe = Join-Path $TargetDir "codexmanager-service.exe"
$WebExe = Join-Path $TargetDir "codexmanager-web.exe"
$Database = Join-Path $TargetDir "codexmanager.db"
$ServicePort = 48764
$WebPort = 48763

function Show-Result([string]$message, [string]$title = "OpenRuntime") {
    if ($NoPopup) {
        Write-Output $message
        return
    }
    $shell = New-Object -ComObject WScript.Shell
    [void]$shell.Popup($message, 4, $title, 64)
}

function Get-Listener([int]$port) {
    @(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue)
}

function Get-OwnedProcesses {
    $expected = @($ServiceExe, $WebExe)
    $result = @()
    foreach ($port in @($ServicePort, $WebPort)) {
        foreach ($listener in Get-Listener $port) {
            $process = Get-CimInstance Win32_Process -Filter "ProcessId=$($listener.OwningProcess)" -ErrorAction SilentlyContinue
            if ($process -and $expected -contains $process.ExecutablePath) {
                $result += $process
            }
        }
    }
    @($result | Sort-Object ProcessId -Unique)
}

function Wait-Http([string]$url, [int]$seconds = 15) {
    $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
    do {
        try {
            $response = Invoke-WebRequest -UseBasicParsing -Uri $url -TimeoutSec 2
            if ($response.StatusCode -eq 200) { return $true }
        } catch {
            Start-Sleep -Milliseconds 300
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    return $false
}

function Stop-OpenRuntime {
    $owned = @(Get-OwnedProcesses)
    if ($owned.Count -eq 0) {
        Show-Result "OpenRuntime 当前未运行。"
        return
    }
    foreach ($process in $owned) {
        Stop-Process -Id $process.ProcessId -Force
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(8)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ((Get-Listener $ServicePort).Count -eq 0 -and (Get-Listener $WebPort).Count -eq 0) {
            Show-Result "OpenRuntime 已停止。`n旧服务 48760 未受影响。"
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "OpenRuntime 进程已停止，但端口尚未释放。"
}

function Assert-PortAvailable([int]$port) {
    $listeners = @(Get-Listener $port)
    if ($listeners.Count -gt 0) {
        $pidList = ($listeners.OwningProcess | Sort-Object -Unique) -join ", "
        throw "端口 $port 已被其他进程占用（PID: $pidList）。"
    }
}

function Start-OpenRuntime {
    $owned = @(Get-OwnedProcesses)
    if ($owned.Count -gt 0) {
        if ((Get-Listener $ServicePort).Count -gt 0 -and (Get-Listener $WebPort).Count -gt 0) {
            Show-Result "OpenRuntime 已经在运行。`n管理页面：http://127.0.0.1:48763/"
            return
        }
        foreach ($process in $owned) {
            Stop-Process -Id $process.ProcessId -Force
        }
        Start-Sleep -Milliseconds 600
    }

    foreach ($path in @($ServiceExe, $WebExe, $Database)) {
        if (-not (Test-Path -LiteralPath $path)) {
            throw "缺少运行文件：$path"
        }
    }
    Assert-PortAvailable $ServicePort
    Assert-PortAvailable $WebPort
    New-Item -ItemType Directory -Force -Path $ExportDir | Out-Null

    $env:CODEXMANAGER_DB_PATH = $Database
    $env:CODEXMANAGER_SERVICE_ADDR = "localhost:$ServicePort"
    $service = Start-Process -FilePath $ServiceExe `
        -WorkingDirectory $TargetDir `
        -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $ExportDir "openruntime-service-48764.stdout.log") `
        -RedirectStandardError (Join-Path $ExportDir "openruntime-service-48764.stderr.log") `
        -PassThru
    if (-not (Wait-Http "http://127.0.0.1:$ServicePort/health" 15)) {
        if (-not $service.HasExited) { Stop-Process -Id $service.Id -Force }
        throw "API 服务启动超时，请查看 exports\logs。"
    }

    $env:CODEXMANAGER_WEB_ADDR = "127.0.0.1:$WebPort"
    $env:CODEXMANAGER_WEB_NO_OPEN = "1"
    $web = Start-Process -FilePath $WebExe `
        -WorkingDirectory $TargetDir `
        -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $ExportDir "openruntime-web-48763.stdout.log") `
        -RedirectStandardError (Join-Path $ExportDir "openruntime-web-48763.stderr.log") `
        -PassThru
    if (-not (Wait-Http "http://127.0.0.1:$WebPort/api/runtime" 15)) {
        if (-not $web.HasExited) { Stop-Process -Id $web.Id -Force }
        if (-not $service.HasExited) { Stop-Process -Id $service.Id -Force }
        throw "管理网页启动超时，请查看 exports\logs。"
    }

    Show-Result "OpenRuntime 已启动。`n管理页面：http://127.0.0.1:48763/`nAPI：http://127.0.0.1:48764/"
}

try {
    $running = @(Get-OwnedProcesses).Count -gt 0
    switch ($Action) {
        "Start" { Start-OpenRuntime }
        "Stop" { Stop-OpenRuntime }
        "Status" {
            if ($running) { Show-Result "OpenRuntime 正在运行。" } else { Show-Result "OpenRuntime 当前未运行。" }
        }
        default {
            if ($running) { Stop-OpenRuntime } else { Start-OpenRuntime }
        }
    }
} catch {
    $message = "操作失败：$($_.Exception.Message)"
    if ($NoPopup) {
        Write-Error $message
    } else {
        $shell = New-Object -ComObject WScript.Shell
        [void]$shell.Popup($message, 8, "OpenRuntime 一键启停", 16)
    }
    exit 1
}
