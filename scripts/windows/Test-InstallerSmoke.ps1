[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath
)

$ErrorActionPreference = 'Stop'
$resolvedInstaller = [System.IO.Path]::GetFullPath($InstallerPath)
if (-not (Test-Path -LiteralPath $resolvedInstaller -PathType Leaf)) {
    throw "Installer does not exist: $resolvedInstaller"
}

$installRoot = Join-Path $env:TEMP ('cxszn-menv-installer-smoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $installRoot | Out-Null
$installerExit = $null
$uninstallerExit = $null
$doctorOk = $false
$uninstallerPath = Join-Path $installRoot 'uninstall.exe'
$installedExecutable = Join-Path $installRoot 'cxszn-menv.exe'

try {
    $installer = Start-Process -FilePath $resolvedInstaller -ArgumentList @('/S', "/D=$installRoot") -Wait -PassThru
    $installerExit = $installer.ExitCode
    if ($installerExit -ne 0) {
        throw "Installer exited with code $installerExit."
    }
    if (-not (Test-Path -LiteralPath $installedExecutable -PathType Leaf)) {
        throw "Installed executable was not found: $installedExecutable"
    }
    $doctor = & $installedExecutable doctor --json 2>$null
    $doctorObject = $doctor | ConvertFrom-Json
    $doctorOk = [bool]$doctorObject.ok
    if (-not $doctorOk) {
        throw 'Installed release doctor returned ok=false.'
    }
    if (-not (Test-Path -LiteralPath $uninstallerPath -PathType Leaf)) {
        throw "Uninstaller was not found: $uninstallerPath"
    }
    $uninstaller = Start-Process -FilePath $uninstallerPath -ArgumentList '/S' -Wait -PassThru
    $uninstallerExit = $uninstaller.ExitCode
    if ($uninstallerExit -ne 0) {
        throw "Uninstaller exited with code $uninstallerExit."
    }
    [pscustomobject]@{
        ok = -not (Test-Path -LiteralPath $installRoot)
        installer_exit = $installerExit
        doctor_ok = $doctorOk
        uninstaller_exit = $uninstallerExit
        install_root = $installRoot
        root_exists_after_uninstall = (Test-Path -LiteralPath $installRoot)
    } | ConvertTo-Json -Compress
}
catch {
    [pscustomobject]@{
        ok = $false
        installer_exit = $installerExit
        doctor_ok = $doctorOk
        uninstaller_exit = $uninstallerExit
        install_root = $installRoot
        root_exists_after_uninstall = (Test-Path -LiteralPath $installRoot)
        error = $_.Exception.Message
    } | ConvertTo-Json -Compress
    exit 1
}
