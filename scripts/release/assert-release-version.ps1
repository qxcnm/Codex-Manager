param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$RootCargoTomlPath = 'Cargo.toml',
    [string]$CargoTomlPath = 'apps/src-tauri/Cargo.toml',
    [string]$WorkspaceCratesRoot = 'crates',
    [string]$TauriConfigPath = 'apps/src-tauri/tauri.conf.json'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-TomlSectionBody {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Content,
        [Parameter(Mandatory = $true)]
        [string]$SectionName,
        [Parameter(Mandatory = $true)]
        [string]$SourcePath
    )

    $escapedSection = [regex]::Escape($SectionName)
    $pattern = "(?ms)^\[$escapedSection\]\s*(.*?)(?=^\[|\z)"
    $match = [regex]::Match($Content, $pattern)
    if (-not $match.Success) {
        throw "section [$SectionName] not found in $SourcePath"
    }
    return $match.Groups[1].Value
}

function Get-WorkspaceVersion {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path $Path -PathType Leaf)) {
        throw "root Cargo.toml not found: $Path"
    }
    $content = Get-Content $Path -Raw
    $section = Get-TomlSectionBody -Content $content -SectionName 'workspace.package' -SourcePath $Path
    $match = [regex]::Match($section, '(?m)^\s*version\s*=\s*"([^"]+)"')
    if (-not $match.Success) {
        throw "failed to read [workspace.package].version from $Path"
    }
    return $match.Groups[1].Value
}

function Get-PackageVersionEntry {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path $Path -PathType Leaf)) {
        throw "Cargo.toml not found: $Path"
    }
    $content = Get-Content $Path -Raw
    $section = Get-TomlSectionBody -Content $content -SectionName 'package' -SourcePath $Path

    $workspaceMatch = [regex]::Match($section, '(?m)^\s*version\.workspace\s*=\s*true\s*$')
    if ($workspaceMatch.Success) {
        return @{
            mode  = 'workspace'
            value = $null
        }
    }

    $versionMatch = [regex]::Match($section, '(?m)^\s*version\s*=\s*"([^"]+)"')
    if ($versionMatch.Success) {
        return @{
            mode  = 'explicit'
            value = $versionMatch.Groups[1].Value
        }
    }

    throw "failed to read [package] version from $Path"
}

$tagInput = $Tag
if ([string]::IsNullOrWhiteSpace($tagInput)) {
    throw 'release tag is required'
}

$normalizedTag = if ($tagInput.StartsWith('v')) { $tagInput } else { "v$tagInput" }
if ($normalizedTag -notmatch '^v(\d+\.\d+\.\d+)(?:[-+].*)?$') {
    throw "tag must look like vX.Y.Z or vX.Y.Z-suffix, got: $normalizedTag"
}
$tagVersion = $Matches[1]

$workspaceVersion = Get-WorkspaceVersion -Path $RootCargoTomlPath

$cargoVersionEntry = Get-PackageVersionEntry -Path $CargoTomlPath
if ($cargoVersionEntry.mode -ne 'explicit') {
    throw "$CargoTomlPath must declare an explicit [package].version"
}
$cargoVersion = $cargoVersionEntry.value

if (-not (Test-Path $TauriConfigPath -PathType Leaf)) {
    throw "tauri config not found: $TauriConfigPath"
}
$tauriConf = (Get-Content $TauriConfigPath -Raw) | ConvertFrom-Json
$tauriVersion = $tauriConf.version
if ([string]::IsNullOrWhiteSpace($tauriVersion)) {
    throw "$TauriConfigPath missing version"
}

if ($workspaceVersion -ne $cargoVersion) {
    throw "version mismatch: $RootCargoTomlPath=$workspaceVersion $CargoTomlPath=$cargoVersion"
}
if ($cargoVersion -ne $tauriVersion) {
    throw "version mismatch: $CargoTomlPath=$cargoVersion $TauriConfigPath=$tauriVersion"
}
if ($cargoVersion -ne $tagVersion) {
    throw "tag/version mismatch: tag=$normalizedTag expects $tagVersion, but app version is $cargoVersion"
}

$workspaceCrateManifests = @()
if (Test-Path $WorkspaceCratesRoot -PathType Container) {
    $workspaceCrateManifests = Get-ChildItem $WorkspaceCratesRoot -Directory |
        ForEach-Object {
            $manifest = Join-Path $_.FullName 'Cargo.toml'
            if (Test-Path $manifest -PathType Leaf) {
                $manifest
            }
        }
}

if ($workspaceCrateManifests.Count -eq 0) {
    throw "no workspace crate manifests found under $WorkspaceCratesRoot"
}

foreach ($manifest in $workspaceCrateManifests) {
    $entry = Get-PackageVersionEntry -Path $manifest
    if ($entry.mode -eq 'workspace') {
        continue
    }
    if ($entry.value -ne $workspaceVersion) {
        throw "workspace crate version mismatch: $manifest=$($entry.value) expected $workspaceVersion"
    }
}

Write-Host "Version OK: workspace=$workspaceVersion app=$cargoVersion tauri=$tauriVersion crates=$($workspaceCrateManifests.Count) tag=$normalizedTag"
