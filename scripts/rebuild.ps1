[CmdletBinding()]
param(
  [ValidateSet("nsis", "msi")]
  [string]$Bundle = "nsis",
  [switch]$NoBundle,
  [switch]$CleanDist,
  [switch]$Portable,
  [string]$PortableDir,
  [switch]$AllPlatforms,
  [string]$GithubToken,
  [string]$WorkflowFile = "release-all.yml",
  [string]$GitRef,
  [string]$ReleaseTag,
  [ValidateSet("build-and-publish", "build-artifacts", "publish-artifacts")]
  [string]$ReleaseMode = "build-and-publish",
  [string]$ArtifactsRunId,
  [bool]$PublishContainers = $true,
  [ValidateSet("auto", "true", "false")]
  [string]$Prerelease = "auto",
  [bool]$DownloadArtifacts = $true,
  [string]$ArtifactsDir,
  [ValidateRange(5, 300)]
  [int]$PollIntervalSec = 10,
  [ValidateRange(1, 360)]
  [int]$TimeoutMin = 60,
  [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $scriptDir
$appsRoot = Join-Path $root "apps"
$frontendRoot = $appsRoot
$tauriDir = Join-Path $appsRoot "src-tauri"
$rootTarget = Join-Path $root "target"
$targetDir = if ($env:CARGO_TARGET_DIR) {
  if ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
    $env:CARGO_TARGET_DIR
  } else {
    Join-Path $root $env:CARGO_TARGET_DIR
  }
} else {
  # Cargo inherits the repository .cargo/config.toml even when invoked from
  # apps/src-tauri, so the local target is rooted at the repository by default.
  $rootTarget
}
$distDir = Join-Path $frontendRoot "out"
$tauriConfig = Join-Path $tauriDir "tauri.conf.json"

$appName = "CodexManager"
if (Test-Path $tauriConfig) {
  $appName = (Get-Content $tauriConfig -Raw | ConvertFrom-Json).productName
}

$portableRoot = if ($PortableDir) { $PortableDir } else { Join-Path $root "portable" }
$portableExe = Join-Path $portableRoot "$appName-portable.exe"
$legacyPortableExe = Join-Path $portableRoot "$appName.exe"
$legacyPortableMarker = Join-Path $portableRoot ".codexmanager-portable"
$appExe = Join-Path $targetDir "release\\$appName.exe"
$artifactsRoot = if ($ArtifactsDir) { $ArtifactsDir } else { Join-Path $root "artifacts" }

<#
函数 `Write-Step`

作者: gaohongshun

时间: 2026-04-02

# 参数
- Message: 参数 Message

# 返回
返回函数执行结果
#>
function Write-Step {
  param([string]$Message)
  Write-Output $Message
}

<#
函数 `Remove-Dir`

作者: gaohongshun

时间: 2026-04-02

# 参数
- Path: 参数 Path

# 返回
返回函数执行结果
#>
function Remove-Dir {
  param([string]$Path)
  $workspacePath = [IO.Path]::GetFullPath($root).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  $candidatePath = [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  $relativePath = [IO.Path]::GetRelativePath($workspacePath, $candidatePath)
  $configuredTarget = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($targetDir).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) } else { $null }
  $isExplicitExternalTarget = $null -ne $configuredTarget -and $candidatePath -eq $configuredTarget
  $workspaceFromCandidate = [IO.Path]::GetRelativePath($candidatePath, $workspacePath)
  $candidateIsAncestor = $workspaceFromCandidate -eq "." -or
    (-not $workspaceFromCandidate.StartsWith("..", [StringComparison]::Ordinal) -and -not [IO.Path]::IsPathRooted($workspaceFromCandidate))
  $candidateIsFilesystemRoot = [string]::Equals($candidatePath, [IO.Path]::GetPathRoot($candidatePath).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar), [StringComparison]::OrdinalIgnoreCase)
  if ($candidateIsFilesystemRoot -or $candidateIsAncestor -or
      (-not $isExplicitExternalTarget -and ($relativePath.StartsWith("..", [StringComparison]::Ordinal) -or [IO.Path]::IsPathRooted($relativePath)))) {
    throw "refusing to recursively remove a path outside the repository build tree: $candidatePath"
  }
  if (Test-Path -LiteralPath $candidatePath) {
    $item = Get-Item -LiteralPath $candidatePath -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw "refusing to recursively remove a reparse point: $candidatePath"
    }
  }
  if (-not (Test-Path $Path)) {
    Write-Step "skip: $Path not found"
    return
  }
  if ($DryRun) {
    Write-Step "DRY RUN: remove $Path"
    return
  }
  Remove-Item -LiteralPath $candidatePath -Recurse -Force
}

<#
函数 `Run-Cargo`

作者: gaohongshun

时间: 2026-04-02

# 参数
- CommandLine: 参数 CommandLine
- Action: 参数 Action

# 返回
返回函数执行结果
#>
function Run-Cargo {
  param([string]$CommandLine, [scriptblock]$Action)
  if ($DryRun) {
    Write-Step "DRY RUN: $CommandLine"
    return
  }
  & $Action
  if ($LASTEXITCODE -ne 0) {
    throw "command failed: $CommandLine"
  }
}

<#
函数 `Get-GitHubRepoInfo`

作者: gaohongshun

时间: 2026-04-02

# 参数
无

# 返回
返回函数执行结果
#>
function Get-GitHubRepoInfo {
  $remote = (& git remote get-url origin 2>$null) -join ""
  if ([string]::IsNullOrWhiteSpace($remote)) {
    throw "git remote origin not found"
  }
  if ($remote -match "github\.com[:/](?<owner>[^/]+)/(?<repo>[^/.]+?)(?:\.git)?$") {
    return @{
      owner = $matches.owner
      repo = $matches.repo
    }
  }
  throw "origin is not a GitHub repository: $remote"
}

<#
函数 `Resolve-GitHubToken`

作者: gaohongshun

时间: 2026-04-02

# 参数
无

# 返回
返回函数执行结果
#>
function Resolve-GitHubToken {
  if (-not [string]::IsNullOrWhiteSpace($GithubToken)) {
    return $GithubToken.Trim()
  }
  foreach ($name in @("GITHUB_TOKEN", "GH_TOKEN")) {
    $value = [Environment]::GetEnvironmentVariable($name)
    if (-not [string]::IsNullOrWhiteSpace($value)) {
      return $value.Trim()
    }
  }
  throw "GitHub token required for -AllPlatforms. Pass -GithubToken or set GITHUB_TOKEN."
}

<#
函数 `Invoke-GitHubApi`

作者: gaohongshun

时间: 2026-04-02

# 参数
无

# 返回
返回函数执行结果
#>
function Invoke-GitHubApi {
  param(
    [ValidateSet("GET", "POST")]
    [string]$Method,
    [string]$Uri,
    [string]$Token,
    [object]$Body
  )
  $headers = @{
    Authorization          = "Bearer $Token"
    Accept                 = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent"           = "codexmanager-rebuild-script"
  }
  if ($Method -eq "GET") {
    return Invoke-RestMethod -Method Get -Uri $Uri -Headers $headers
  }
  $json = if ($null -eq $Body) { $null } else { $Body | ConvertTo-Json -Depth 10 -Compress }
  return Invoke-RestMethod -Method Post -Uri $Uri -Headers $headers -ContentType "application/json" -Body $json
}

<#
函数 `Resolve-WorkflowDefinition`

作者: gaohongshun

时间: 2026-04-02

# 参数
- Repo: 参数 Repo
- Token: 参数 Token
- WorkflowFile: 参数 WorkflowFile

# 返回
返回函数执行结果
#>
function Resolve-WorkflowDefinition {
  param(
    [hashtable]$Repo,
    [string]$Token,
    [string]$WorkflowFile
  )

  $workflowUri = "https://api.github.com/repos/$($Repo.owner)/$($Repo.repo)/actions/workflows/$WorkflowFile"
  try {
    return Invoke-GitHubApi -Method GET -Uri $workflowUri -Token $Token -Body $null
  } catch {
    $listUri = "https://api.github.com/repos/$($Repo.owner)/$($Repo.repo)/actions/workflows?per_page=100"
    $workflowList = @()
    try {
      $resp = Invoke-GitHubApi -Method GET -Uri $listUri -Token $Token -Body $null
      if ($null -ne $resp.workflows) {
        $workflowList = $resp.workflows | ForEach-Object { $_.path } | Sort-Object
      }
    } catch {
      # no-op; keep fallback message
    }

    if ($workflowList.Count -gt 0) {
      throw "workflow '$WorkflowFile' not found. Available workflows: $($workflowList -join ', ')"
    }
    throw "workflow '$WorkflowFile' not found."
  }
}

<#
函数 `Invoke-LocalWindowsBuild`

作者: gaohongshun

时间: 2026-04-02

# 参数
无

# 返回
返回函数执行结果
#>
function Invoke-LocalWindowsBuild {
  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not found in PATH"
  }
  if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    Write-Warning "pnpm not found; tauri beforeBuildCommand may fail."
  }

  Push-Location $root
  try {
    Remove-Dir $targetDir
    if ($CleanDist) {
      Remove-Dir $distDir
    }

    Push-Location $tauriDir
    try {
      if ($NoBundle) {
        Run-Cargo "cargo tauri build --no-bundle" { cargo tauri build --no-bundle }
      } else {
        Run-Cargo "cargo tauri build --bundles $Bundle" { cargo tauri build --bundles $Bundle }
      }
    } finally {
      Pop-Location
    }

    if ($Portable) {
      if ($DryRun) {
        Write-Step "DRY RUN: stage portable -> $portableRoot"
        Write-Step "DRY RUN: remove legacy portable exe -> $legacyPortableExe"
        Write-Step "DRY RUN: remove legacy marker -> $legacyPortableMarker"
        Write-Step "DRY RUN: copy $appExe -> $portableExe"
      } else {
        if (-not (Test-Path $portableRoot)) {
          New-Item -ItemType Directory -Force $portableRoot | Out-Null
        }
        if (-not (Test-Path $appExe)) {
          throw "missing app exe: $appExe"
        }
        if ((Test-Path $legacyPortableExe) -and ($legacyPortableExe -ne $portableExe)) {
          Remove-Item -Force $legacyPortableExe
        }
        if (Test-Path $legacyPortableMarker) {
          Remove-Item -Force $legacyPortableMarker
        }
        Copy-Item -Force $appExe $portableExe
      }
    }
  } finally {
    Pop-Location
  }
}

<#
函数 `Invoke-AllPlatformBuild`

作者: gaohongshun

时间: 2026-04-02

# 参数
无

# 返回
返回函数执行结果
#>
function Invoke-AllPlatformBuild {
  $repo = Get-GitHubRepoInfo
  $token = Resolve-GitHubToken
  if ([string]::IsNullOrWhiteSpace($GitRef)) {
    $GitRef = (& git branch --show-current 2>$null) -join ""
  }
  if ([string]::IsNullOrWhiteSpace($GitRef)) {
    $GitRef = (& git describe --tags --exact-match 2>$null) -join ""
  }
  if ([string]::IsNullOrWhiteSpace($GitRef)) {
    throw "cannot resolve git ref. Pass -GitRef explicitly."
  }

  if ([string]::IsNullOrWhiteSpace($ReleaseTag)) {
    $ReleaseTag = (& git describe --tags --exact-match 2>$null) -join ""
  }
  if ([string]::IsNullOrWhiteSpace($ReleaseTag)) {
    throw "release tag required for -AllPlatforms. Pass -ReleaseTag (e.g. v0.0.6)."
  }
  if ($ReleaseMode -eq "publish-artifacts" -and [string]::IsNullOrWhiteSpace($ArtifactsRunId)) {
    throw "ArtifactsRunId is required when -ReleaseMode publish-artifacts is used."
  }

  # Map legacy workflow names to the single release entry for backward compatibility.
  $workflowAlias = @{
    "release-windows.yml"         = "release-all.yml"
    "release-linux.yml"           = "release-all.yml"
    "release-macos-beta.yml"      = "release-all.yml"
    "release-service-windows.yml" = "release-all.yml"
    "release-service-linux.yml"   = "release-all.yml"
    "release-service-macos.yml"   = "release-all.yml"
    "ci-verify.yml"               = "release-all.yml"
  }
  if ($workflowAlias.ContainsKey($WorkflowFile)) {
    Write-Step "workflow alias: $WorkflowFile -> $($workflowAlias[$WorkflowFile])"
    $WorkflowFile = $workflowAlias[$WorkflowFile]
  }

  $resolvedSha = (& git rev-parse --verify "$GitRef^{commit}" 2>$null) -join ""
  if ([string]::IsNullOrWhiteSpace($resolvedSha)) {
    throw "cannot resolve git ref to commit: $GitRef"
  }

  $dispatchUri = "https://api.github.com/repos/$($repo.owner)/$($repo.repo)/actions/workflows/$WorkflowFile/dispatches"
  $runsUri = "https://api.github.com/repos/$($repo.owner)/$($repo.repo)/actions/workflows/$WorkflowFile/runs?event=workflow_dispatch&per_page=50"
  $prereleaseInput = $Prerelease.ToLowerInvariant()
  $dispatchBody = @{
    ref = $GitRef
    inputs = @{
      mode               = $ReleaseMode
      tag                = $ReleaseTag
      ref                = $GitRef
      prerelease         = $prereleaseInput
      artifacts_run_id   = if ($ArtifactsRunId) { $ArtifactsRunId } else { "" }
      publish_containers = if ($PublishContainers) { "true" } else { "false" }
    }
  }

  if ($DryRun) {
    Write-Step "DRY RUN: using workflow .github/workflows/$WorkflowFile"
    Write-Step "DRY RUN: dispatch workflow $WorkflowFile on ref=$GitRef tag=$ReleaseTag mode=$ReleaseMode prerelease=$prereleaseInput artifacts_run_id=$ArtifactsRunId publish_containers=$PublishContainers"
    Write-Step "DRY RUN: resolved target sha $resolvedSha"
    Write-Step "DRY RUN: POST $dispatchUri"
    Write-Step "DRY RUN: payload $($dispatchBody | ConvertTo-Json -Depth 10 -Compress)"
    if ($DownloadArtifacts) {
      Write-Step "DRY RUN: download artifacts -> $artifactsRoot"
    }
    return
  }

  $workflow = Resolve-WorkflowDefinition -Repo $repo -Token $token -WorkflowFile $WorkflowFile
  Write-Step "using workflow: $($workflow.path)"
  Write-Step "dispatching workflow: $WorkflowFile (ref=$GitRef tag=$ReleaseTag mode=$ReleaseMode prerelease=$prereleaseInput artifacts_run_id=$ArtifactsRunId publish_containers=$PublishContainers)"
  Invoke-GitHubApi -Method POST -Uri $dispatchUri -Token $token -Body $dispatchBody | Out-Null

  $deadline = (Get-Date).ToUniversalTime().AddMinutes($TimeoutMin)
  $dispatchedAt = (Get-Date).ToUniversalTime().AddSeconds(-5)
  $run = $null

  while ((Get-Date).ToUniversalTime() -lt $deadline) {
    Start-Sleep -Seconds $PollIntervalSec
    $runs = Invoke-GitHubApi -Method GET -Uri $runsUri -Token $token -Body $null
    if ($null -eq $runs.workflow_runs) {
      continue
    }
    $run = $runs.workflow_runs |
      Where-Object {
        [DateTime]::Parse($_.created_at).ToUniversalTime() -ge $dispatchedAt -and
        $_.head_sha -eq $resolvedSha
      } |
      Sort-Object { [DateTime]::Parse($_.created_at).ToUniversalTime() } -Descending |
      Select-Object -First 1
    if ($null -eq $run) {
      Write-Step "waiting workflow run: ref=$GitRef sha=$resolvedSha"
      continue
    }

    Write-Step ("workflow run: id={0} status={1} conclusion={2} head_sha={3}" -f $run.id, $run.status, $run.conclusion, $run.head_sha)
    if ($run.status -eq "completed") {
      break
    }
  }

  if ($null -eq $run) {
    throw "workflow run not found within timeout"
  }
  if ($run.status -ne "completed") {
    throw "workflow did not complete within timeout"
  }
  if ($run.conclusion -ne "success") {
    throw "workflow failed: conclusion=$($run.conclusion)"
  }

  if (-not $DownloadArtifacts) {
    Write-Step "workflow succeeded"
    return
  }
  if ($ReleaseMode -eq "publish-artifacts") {
    Write-Step "workflow succeeded; publish-artifacts mode reused artifacts from run $ArtifactsRunId and does not upload new artifacts"
    return
  }

  if (-not (Test-Path $artifactsRoot)) {
    New-Item -ItemType Directory -Force $artifactsRoot | Out-Null
  }

  $artifactsUri = "https://api.github.com/repos/$($repo.owner)/$($repo.repo)/actions/runs/$($run.id)/artifacts?per_page=100"
  $artifactsResp = Invoke-GitHubApi -Method GET -Uri $artifactsUri -Token $token -Body $null
  if ($null -eq $artifactsResp.artifacts -or $artifactsResp.artifacts.Count -eq 0) {
    throw "workflow succeeded but no artifacts were found"
  }

  $headers = @{
    Authorization          = "Bearer $token"
    Accept                 = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent"           = "codexmanager-rebuild-script"
  }

  foreach ($artifact in $artifactsResp.artifacts) {
    if ($artifact.expired -eq $true) {
      continue
    }
    $zipName = "{0}-{1}.zip" -f $artifact.name, $artifact.id
    $zipPath = Join-Path $artifactsRoot $zipName
    Write-Step "download artifact: $($artifact.name) -> $zipPath"
    Invoke-WebRequest -Uri $artifact.archive_download_url -Headers $headers -OutFile $zipPath | Out-Null
  }
}

Push-Location $root
try {
  if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw "git not found in PATH"
  }

  if ($AllPlatforms) {
    Invoke-AllPlatformBuild
  } else {
    Invoke-LocalWindowsBuild
  }
} finally {
  Pop-Location
}

Write-Step "done"
