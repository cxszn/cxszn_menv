[CmdletBinding()]
param(
    [string]$ExecutablePath = (Join-Path (Get-Location) 'target\x86_64-pc-windows-msvc\release\cxszn-menv.exe'),
    [Parameter(Mandatory = $true)][string]$PhpPath
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = [System.IO.Path]::GetFullPath($ExecutablePath)
$resolvedPhp = [System.IO.Path]::GetFullPath($PhpPath)
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
    throw "MEnv executable does not exist: $resolvedExecutable"
}
if (-not (Test-Path -LiteralPath $resolvedPhp -PathType Leaf)) {
    throw "PHP executable does not exist: $resolvedPhp"
}
if ([System.IO.Path]::GetFileName($resolvedExecutable) -ine 'cxszn-menv.exe') {
    throw 'The launcher source must be named cxszn-menv.exe.'
}

$root = Join-Path $env:TEMP ('cxszn-menv-cross-volume-launcher-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root -Force | Out-Null
try {
    $shim = Join-Path $root 'php.exe'
    Copy-Item -LiteralPath $resolvedExecutable -Destination $shim
    [IO.File]::WriteAllText((Join-Path $root 'php.target'), "1`n$resolvedPhp`n")

    $versionOutput = @(& $shim -n -v 2>&1) -join "`n"
    $iniOutput = @(& $shim --ini 2>&1) -join "`n"
    $versionLine = $versionOutput -split "`n" | Where-Object { $_ -match '^PHP\s+[0-9]+\.[0-9]+\.[0-9]+' } | Select-Object -First 1
    $result = [ordered]@{
        ok = ($LASTEXITCODE -eq 0 -and $null -ne $versionLine)
        executable = $resolvedExecutable
        php_path = $resolvedPhp
        php_version_line = if ($null -eq $versionLine) { $null } else { $versionLine.ToString() }
        php_ini = @($iniOutput -split "`n" | Select-Object -First 3)
        cross_volume = ([System.IO.Path]::GetPathRoot($resolvedExecutable) -ne [System.IO.Path]::GetPathRoot($resolvedPhp))
    }
    $result | ConvertTo-Json -Depth 4
    if (-not $result.ok) { exit 1 }
}
finally {
    if (Test-Path -LiteralPath $root) {
        [IO.Directory]::Delete($root, $true)
    }
}
