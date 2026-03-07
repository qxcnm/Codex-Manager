param(
    [string]$Base = "http://localhost:48760",
    [string]$ApiKey = "",
    [string]$Model = "gpt-5.3-codex",
    [int]$TimeoutSeconds = 90,
    [string]$OutDir = "",
    [switch]$SkipTools,
    [switch]$SkipStreams,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-OutDir {
    param([string]$InputOutDir)

    if ($InputOutDir -and $InputOutDir.Trim().Length -gt 0) {
        $dir = $InputOutDir
    } else {
        $stamp = Get-Date -Format "yyyyMMdd_HHmmss"
        $desktop = [Environment]::GetFolderPath("Desktop")
        $dir = Join-Path $desktop "codex_gateway_regression_$stamp"
    }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function Resolve-ApiKey {
    param([string]$ExplicitApiKey)

    if ($ExplicitApiKey -and $ExplicitApiKey.Trim().Length -gt 0) {
        return $ExplicitApiKey.Trim()
    }
    foreach ($name in @("CODEX_API_KEY", "OPENAI_API_KEY")) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if ($value -and $value.Trim().Length -gt 0) {
            return $value.Trim()
        }
    }
    throw "ApiKey is empty. Pass -ApiKey or set CODEX_API_KEY/OPENAI_API_KEY."
}

function Join-ArgLine {
    param([string[]]$Args)
    return ($Args | ForEach-Object {
        if ($_ -match '\s') { '"' + $_ + '"' } else { $_ }
    }) -join " "
}

function Invoke-Probe {
    param(
        [string]$Name,
        [string]$ScriptPath,
        [string[]]$Args,
        [string]$LogPath,
        [switch]$EnableDryRun
    )

    $commandLine = "& `"$ScriptPath`" " + (Join-ArgLine -Args $Args)
    if ($EnableDryRun) {
        "DRY RUN: $commandLine" | Tee-Object -FilePath $LogPath | Out-Null
        return [pscustomobject]@{
            name = $Name
            success = $true
            exit_code = 0
            log_path = $LogPath
            dry_run = $true
        }
    }

    $output = & $ScriptPath @Args 2>&1
    $output | Tee-Object -FilePath $LogPath | Out-Null
    $success = ($LASTEXITCODE -eq 0)

    return [pscustomobject]@{
        name = $Name
        success = $success
        exit_code = $LASTEXITCODE
        log_path = $LogPath
        dry_run = $false
    }
}

$suiteOutDir = Resolve-OutDir -InputOutDir $OutDir
$apiKey = if ($DryRun) { "<dry-run>" } else { Resolve-ApiKey -ExplicitApiKey $ApiKey }
$scriptRoot = $PSScriptRoot

$steps = New-Object System.Collections.Generic.List[object]

if (-not $SkipTools) {
    $toolsScript = Join-Path $scriptRoot "chat_tools_hit_probe.ps1"
    $steps.Add([pscustomobject]@{
        name = "chat_tools_non_stream"
        script = $toolsScript
        args = @(
            "-Base", $Base,
            "-ApiKey", $apiKey,
            "-Model", $Model,
            "-TimeoutSeconds", "$TimeoutSeconds"
        )
        log = (Join-Path $suiteOutDir "chat_tools_non_stream.txt")
    })
    $steps.Add([pscustomobject]@{
        name = "chat_tools_stream"
        script = $toolsScript
        args = @(
            "-Base", $Base,
            "-ApiKey", $apiKey,
            "-Model", $Model,
            "-TimeoutSeconds", "$TimeoutSeconds",
            "-Stream"
        )
        log = (Join-Path $suiteOutDir "chat_tools_stream.txt")
    })
}

if (-not $SkipStreams) {
    $streamScript = Join-Path $scriptRoot "codex_stream_probe.ps1"
    $steps.Add([pscustomobject]@{
        name = "codex_streams"
        script = $streamScript
        args = @(
            "-Base", $Base,
            "-ApiKey", $apiKey,
            "-Model", $Model,
            "-TimeoutSeconds", "$TimeoutSeconds",
            "-OutDir", (Join-Path $suiteOutDir "codex_stream_probe")
        )
        log = (Join-Path $suiteOutDir "codex_stream_probe.txt")
    })
}

if ($steps.Count -eq 0) {
    throw "No probes selected. Remove -SkipTools or -SkipStreams."
}

$results = foreach ($step in $steps) {
    Invoke-Probe -Name $step.name -ScriptPath $step.script -Args $step.args -LogPath $step.log -EnableDryRun:$DryRun
}

$failed = @($results | Where-Object { -not $_.success })

$summary = [pscustomobject]@{
    timestamp = (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
    base = $Base
    model = $Model
    out_dir = $suiteOutDir
    dry_run = $DryRun.IsPresent
    executed = @($results)
    failures = @($failed)
}

$summaryJsonPath = Join-Path $suiteOutDir "summary.json"
$summaryTxtPath = Join-Path $suiteOutDir "summary.txt"
$summary | ConvertTo-Json -Depth 100 | Set-Content -Path $summaryJsonPath -Encoding UTF8

$text = @()
$text += "Base: $Base"
$text += "Model: $Model"
$text += "OutDir: $suiteOutDir"
$text += "DryRun: $($DryRun.IsPresent)"
$text += ""
$text += "Steps:"
foreach ($result in $results) {
    $text += "  - $($result.name): success=$($result.success) exit_code=$($result.exit_code) log=$($result.log_path)"
}
$text += ""
$text += "FailureCount: $($failed.Count)"
$text += "SummaryJson: $summaryJsonPath"
$text += "SummaryTxt: $summaryTxtPath"
$text -join [Environment]::NewLine | Set-Content -Path $summaryTxtPath -Encoding UTF8

Write-Host "Done."
Write-Host "Output directory: $suiteOutDir"
Write-Host "Summary: $summaryTxtPath"

if ($failed.Count -gt 0) {
    exit 1
}
