[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ConfigPath,
    [string]$ServiceAddress,
    [string]$WebAddress,
    [string]$ProviderBaseUrl,
    [switch]$ProbeListeners,
    [switch]$RequireTokenFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-EndpointParts([string]$Address) {
    if ([string]::IsNullOrWhiteSpace($Address)) {
        return $null
    }
    if ($Address -match '^\[(?<host>[^\]]+)\]:(?<port>\d+)$') {
        $hostName = $Matches.host
        $port = [int]$Matches.port
    } elseif ($Address -match '^(?<host>[^:]+):(?<port>\d+)$') {
        $hostName = $Matches.host
        $port = [int]$Matches.port
    } else {
        return $null
    }
    if ($port -lt 1 -or $port -gt 65535) {
        return $null
    }
    [pscustomobject]@{ Host = $hostName; Port = $port }
}

if (-not (Test-Path -LiteralPath $ConfigPath -PathType Leaf)) {
    [pscustomobject]@{
        result = 'HANDOFF'
        reason = 'production configuration file was not provided'
        config_path = [IO.Path]::GetFullPath($ConfigPath)
        secrets_loaded = $false
    } | ConvertTo-Json -Depth 4
    exit 2
}

$required = @(
    'CODEXMANAGER_STORAGE_BACKEND',
    'CODEXMANAGER_DATABASE_URL',
    'CODEXMANAGER_UPSTREAM_BASE_URL',
    'CODEXMANAGER_SERVICE_ADDR',
    'CODEXMANAGER_WEB_ADDR',
    'CODEXMANAGER_LOGIN_ADDR',
    'CODEXMANAGER_RPC_TOKEN_FILE'
)
$present = @{}
$values = @{}
foreach ($line in Get-Content -LiteralPath $ConfigPath) {
    if ($line -match '^\s*(?<name>[A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?<value>.*)$' -and
        -not [string]::IsNullOrWhiteSpace($Matches.value.Trim().Trim('"', "'"))) {
        $present[$Matches.name] = $true
        $values[$Matches.name] = $Matches.value.Trim().Trim('"', "'")
    }
}
$missing = @($required | Where-Object { -not $present.ContainsKey($_) })
$placeholderPattern = '(?i)(REPLACE_ME|CHANGE_ME|TODO|YOUR_[A-Z0-9_]+|<[^>]+>)'
$placeholders = @($required | Where-Object {
    $present.ContainsKey($_) -and $values[$_] -match $placeholderPattern
})
$semanticErrors = @()
if ($present.ContainsKey('CODEXMANAGER_STORAGE_BACKEND') -and
    $values['CODEXMANAGER_STORAGE_BACKEND'].ToLowerInvariant() -ne 'mysql') {
    $semanticErrors += 'CODEXMANAGER_STORAGE_BACKEND must be mysql for controlled production deployment'
}
if ($present.ContainsKey('CODEXMANAGER_DATABASE_URL') -and
    $values['CODEXMANAGER_DATABASE_URL'] -notmatch '(?i)^mysql(?:\+[^:]+)?://') {
    $semanticErrors += 'CODEXMANAGER_DATABASE_URL must use a mysql:// URL for controlled production deployment'
}
foreach ($addressName in @('CODEXMANAGER_SERVICE_ADDR', 'CODEXMANAGER_WEB_ADDR', 'CODEXMANAGER_LOGIN_ADDR')) {
    if ($present.ContainsKey($addressName) -and $null -eq (Get-EndpointParts $values[$addressName])) {
        $semanticErrors += "$addressName must use host:port syntax"
    }
}
$tokenFileResult = $null
if ($RequireTokenFile -and $present.ContainsKey('CODEXMANAGER_RPC_TOKEN_FILE')) {
    $tokenPath = [Environment]::ExpandEnvironmentVariables($values['CODEXMANAGER_RPC_TOKEN_FILE'])
    $tokenFileResult = [pscustomobject]@{
        path_supplied = $true
        exists = [bool](Test-Path -LiteralPath $tokenPath -PathType Leaf)
        non_empty = if (Test-Path -LiteralPath $tokenPath -PathType Leaf) {
            ([IO.FileInfo]$tokenPath).Length -gt 0
        } else { $false }
    }
}
$listenerResults = @()
if ($ProbeListeners) {
    foreach ($address in @($ServiceAddress, $WebAddress)) {
        $parts = Get-EndpointParts $address
        if ($null -eq $parts) {
            $listenerResults += [pscustomobject]@{ address = $address; reachable = $false; reason = 'address not supplied as host:port' }
            continue
        }
        $ok = Test-NetConnection -ComputerName $parts.Host -Port $parts.Port -InformationLevel Quiet -WarningAction SilentlyContinue
        $listenerResults += [pscustomobject]@{ address = $address; reachable = [bool]$ok }
    }
}

$tokenFileMissing = $RequireTokenFile -and ($null -eq $tokenFileResult -or -not $tokenFileResult.exists -or -not $tokenFileResult.non_empty)
$unreachableListener = $ProbeListeners -and @($listenerResults | Where-Object { -not $_.reachable }).Count -gt 0
$result = if ($missing.Count -eq 0 -and $placeholders.Count -eq 0 -and $semanticErrors.Count -eq 0 -and -not $tokenFileMissing -and -not $unreachableListener) {
    'READY_FOR_AUTHENTICATED_ACCEPTANCE'
} else { 'HANDOFF' }
[pscustomobject]@{
    result = $result
    config_path = [IO.Path]::GetFullPath($ConfigPath)
    secrets_loaded = $false
    required_variables_present = ($missing.Count -eq 0)
    missing_variables = $missing
    placeholder_variables = $placeholders
    semantic_errors = $semanticErrors
    token_file = $tokenFileResult
    listeners = $listenerResults
    provider_probe = if ([string]::IsNullOrWhiteSpace($ProviderBaseUrl)) {
        'not_run: requires explicit production credentials and acceptance scope'
    } else {
        'not_run: provider URL supplied as an explicit probe input; no request was sent'
    }
    note = 'Values, credentials, and provider responses are intentionally omitted from this report.'
} | ConvertTo-Json -Depth 5

if ($missing.Count -ne 0 -or $placeholders.Count -ne 0 -or $semanticErrors.Count -ne 0 -or $tokenFileMissing -or $unreachableListener) { exit 2 }
