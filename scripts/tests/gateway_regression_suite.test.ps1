$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "gateway_regression_suite.ps1"
if (-not (Test-Path $scriptPath -PathType Leaf)) {
  throw "missing gateway_regression_suite.ps1 at $scriptPath"
}

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("codex_gateway_regression_test_" + [Guid]::NewGuid().ToString("N"))

try {
  & $scriptPath -DryRun -OutDir $tempRoot | Out-Null
  if (-not $?) {
    throw "gateway_regression_suite.ps1 -DryRun failed"
  }

  $summaryPath = Join-Path $tempRoot "summary.txt"
  $summaryJsonPath = Join-Path $tempRoot "summary.json"
  if (-not (Test-Path $summaryPath -PathType Leaf)) {
    throw "summary.txt not created"
  }
  if (-not (Test-Path $summaryJsonPath -PathType Leaf)) {
    throw "summary.json not created"
  }
  $summaryText = Get-Content $summaryPath -Raw
  if ($summaryText -notlike "*chat_tools_non_stream*") {
    throw "expected chat_tools_non_stream in summary"
  }
  if ($summaryText -notlike "*chat_tools_stream*") {
    throw "expected chat_tools_stream in summary"
  }
  if ($summaryText -notlike "*codex_streams*") {
    throw "expected codex_streams in summary"
  }
  if ($summaryText -notlike "*FailureCount: 0*") {
    throw "expected zero failures in dry-run summary"
  }

  Write-Host "gateway_regression_suite.ps1 dry-run looks ok"
} finally {
  if (Test-Path $tempRoot) {
    Remove-Item -Recurse -Force $tempRoot
  }
}
