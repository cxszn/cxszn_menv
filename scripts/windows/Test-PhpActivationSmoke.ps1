[CmdletBinding()]
param(
    [string]$ExpectedVersion,
    [string]$DataRoot = (Join-Path $env:LOCALAPPDATA 'cxszn-menv')
)

$ErrorActionPreference = 'Stop'

function Invoke-CheckedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ArgumentList
    )

    try {
        $output = @(& $FilePath @ArgumentList 2>&1)
        $exitCode = $LASTEXITCODE
    } catch {
        $output = @($_.Exception.Message)
        $exitCode = 127
    }
    [pscustomobject]@{
        output = $output
        exit_code = $exitCode
    }
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
$shimDirectory = Join-Path $DataRoot 'bin'
$targetRecord = Join-Path $shimDirectory 'php.target'
$shimPath = Join-Path $shimDirectory 'php.exe'

# Do not mutate HKCU or the current user's persisted environment. This only
# models a newly-created terminal process by replacing this script's local
# child-process PATH for the duration of the checks.
$env:Path = (($userPath, $machinePath) | Where-Object { $_ -ne $null -and $_ -ne '' }) -join ';'

$where = Invoke-CheckedProcess -FilePath 'where.exe' -ArgumentList @('php.exe')
$php = Invoke-CheckedProcess -FilePath 'php.exe' -ArgumentList @('-n', '-v')
$ini = Invoke-CheckedProcess -FilePath 'php.exe' -ArgumentList @('--ini')

$target = if (Test-Path -LiteralPath $targetRecord -PathType Leaf) {
    (Get-Content -LiteralPath $targetRecord -ErrorAction Stop | Select-Object -Skip 1 -First 1).Trim()
} else {
    $null
}

$versionLine = ($php.output | Where-Object { $_ -match '^PHP\s+[0-9]+\.[0-9]+\.[0-9]+' } | Select-Object -First 1)
$version = if ($versionLine -match '^PHP\s+([^\s]+)') { $Matches[1] } else { $null }
$result = [ordered]@{
    ok = ($where.exit_code -eq 0 -and $php.exit_code -eq 0 -and $ini.exit_code -eq 0 -and $null -ne $version)
    user_has_shim_lease = (($userPath -split ';') -contains $shimDirectory)
    shim_exists = (Test-Path -LiteralPath $shimPath -PathType Leaf)
    target_record = $target
    where_php = @($where.output | ForEach-Object { $_.ToString() })
    php_version = $version
    php_version_line = if ($null -eq $versionLine) { $null } else { $versionLine.ToString() }
    php_ini = @($ini.output | ForEach-Object { $_.ToString() })
}

if (-not [string]::IsNullOrWhiteSpace($ExpectedVersion)) {
    $result.expected_version = $ExpectedVersion
    $result.version_matches = ($version -eq $ExpectedVersion)
    $result.ok = $result.ok -and $result.version_matches
}

$result | ConvertTo-Json -Depth 4
if (-not $result.ok) {
    exit 1
}
