[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$PreviousInstallerPath,
    [Parameter(Mandatory = $true)][string]$CurrentInstallerPath
)

$ErrorActionPreference = 'Stop'
$previous = [System.IO.Path]::GetFullPath($PreviousInstallerPath)
$current = [System.IO.Path]::GetFullPath($CurrentInstallerPath)
foreach ($installer in @($previous, $current)) {
    if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
        throw "Installer does not exist: $installer"
    }
}

$installRoot = Join-Path $env:TEMP ('cxszn-menv-upgrade-smoke-' + [guid]::NewGuid().ToString('N'))
$executable = Join-Path $installRoot 'cxszn-menv.exe'
$uninstaller = Join-Path $installRoot 'uninstall.exe'
$results = [ordered]@{ previous = $null; current = $null; ok = $false }
New-Item -ItemType Directory -Path $installRoot -Force | Out-Null

try {
    foreach ($stage in @(
        @{ name = 'previous'; path = $previous },
        @{ name = 'current'; path = $current }
    )) {
        $installerProcess = Start-Process -FilePath $stage.path -ArgumentList @('/S', "/D=$installRoot") -Wait -PassThru
        if ($installerProcess.ExitCode -ne 0) {
            throw "$($stage.name) installer exited with code $($installerProcess.ExitCode)."
        }
        if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
            throw "Installed executable was not found after $($stage.name) install."
        }
        $doctorText = @(& $executable doctor --json 2>&1) -join "`n"
        $doctor = $doctorText | ConvertFrom-Json
        if ($doctor.ok -ne $true) {
            throw "$($stage.name) release doctor returned ok=false."
        }
        $results[$stage.name] = [ordered]@{
            installer_exit = $installerProcess.ExitCode
            doctor_ok = $doctor.ok
            executable = $executable
        }
    }

    if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
        throw 'Uninstaller was not found after upgrade.'
    }
    $uninstallerProcess = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
    if ($uninstallerProcess.ExitCode -ne 0) {
        throw "Uninstaller exited with code $($uninstallerProcess.ExitCode)."
    }
    $results.ok = -not (Test-Path -LiteralPath $installRoot)
    $results.uninstaller_exit = $uninstallerProcess.ExitCode
    $results.install_root = $installRoot
    $results | ConvertTo-Json -Depth 5 -Compress
    if (-not $results.ok) { exit 1 }
}
catch {
    $results.ok = $false
    $results.error = $_.Exception.Message
    $results.install_root = $installRoot
    $results | ConvertTo-Json -Depth 5 -Compress
    exit 1
}
finally {
    if (Test-Path -LiteralPath $installRoot) {
        try { [IO.Directory]::Delete($installRoot, $true) } catch { }
    }
}
