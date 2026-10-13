[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallerPath
)

$ErrorActionPreference = 'Stop'
$resolvedInstaller = [System.IO.Path]::GetFullPath($InstallerPath)
if (-not (Test-Path -LiteralPath $resolvedInstaller -PathType Leaf)) {
    throw "Installer does not exist: $resolvedInstaller"
}

$installRoot = Join-Path $env:TEMP ('cxszn-menv-sqlite-migration-' + [guid]::NewGuid().ToString('N'))
$dataRoot = Join-Path $env:TEMP ('cxszn-menv-sqlite-data-' + [guid]::NewGuid().ToString('N'))
$legacyRoot = Join-Path $dataRoot 'cxszn-menv'
$legacyState = Join-Path $legacyRoot 'state-rust.json'
$sqliteState = Join-Path $legacyRoot 'state-rust.sqlite3'
$previousLocalAppData = $env:LOCALAPPDATA
$result = [ordered]@{
    ok = $false
    install_exit = $null
    doctor_ok = $false
    legacy_preserved = $false
    sqlite_created = $false
    uninstaller_exit = $null
    temp_root_removed = $false
}

try {
    New-Item -ItemType Directory -Path $legacyRoot -Force | Out-Null
    $legacy = [ordered]@{
        schemaVersion = '1'
        runtimes = @()
        pathLease = [ordered]@{
            scope = 'user'
            enabled = $false
            path = $null
            runtimeId = $null
            entries = @()
            hash = ''
            updatedAt = $null
        }
        operations = @()
        updatedAt = 'legacy-migration-smoke'
    }
    $legacy | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $legacyState -Encoding utf8

    $installer = Start-Process -FilePath $resolvedInstaller -ArgumentList @('/S', "/D=$installRoot") -Wait -PassThru
    $result.install_exit = $installer.ExitCode
    if ($installer.ExitCode -ne 0) { throw "Installer exited with code $($installer.ExitCode)." }
    $executable = Join-Path $installRoot 'cxszn-menv.exe'
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { throw 'Installed executable was not found.' }

    $env:LOCALAPPDATA = $dataRoot
    $doctor = ((@(& $executable doctor --json 2>&1) -join "`n") | ConvertFrom-Json)
    $result.doctor_ok = [bool]$doctor.ok
    $result.legacy_preserved = Test-Path -LiteralPath $legacyState -PathType Leaf
    $result.sqlite_created = Test-Path -LiteralPath $sqliteState -PathType Leaf
    if (-not ($result.doctor_ok -and $result.legacy_preserved -and $result.sqlite_created)) {
        throw 'SQLite migration evidence was incomplete.'
    }

    $env:LOCALAPPDATA = $previousLocalAppData
    $uninstaller = Join-Path $installRoot 'uninstall.exe'
    $uninstallerProcess = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
    $result.uninstaller_exit = $uninstallerProcess.ExitCode
    if ($uninstallerProcess.ExitCode -ne 0) { throw "Uninstaller exited with code $($uninstallerProcess.ExitCode)." }
    $result.ok = $true
}
catch {
    $result.error = $_.Exception.Message
}
finally {
    $env:LOCALAPPDATA = $previousLocalAppData
    if (Test-Path -LiteralPath $installRoot) { Remove-Item -LiteralPath $installRoot -Recurse -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $dataRoot) { Remove-Item -LiteralPath $dataRoot -Recurse -Force -ErrorAction SilentlyContinue }
    $result.temp_root_removed = -not (Test-Path -LiteralPath $installRoot) -and -not (Test-Path -LiteralPath $dataRoot)
    $result | ConvertTo-Json -Depth 5
}

if (-not $result.ok) { exit 1 }
