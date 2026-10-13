[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [string]$PhpPath = 'R:\App\Env\php\php8.5.11\php.exe'
)

$ErrorActionPreference = 'Stop'
$resolvedInstaller = [System.IO.Path]::GetFullPath($InstallerPath)
$resolvedPhp = [System.IO.Path]::GetFullPath($PhpPath)
if (-not (Test-Path -LiteralPath $resolvedInstaller -PathType Leaf)) {
    throw "Installer does not exist: $resolvedInstaller"
}
if (-not (Test-Path -LiteralPath $resolvedPhp -PathType Leaf)) {
    throw "PHP executable does not exist: $resolvedPhp"
}
if ([Console]::IsInputRedirected -or [Console]::IsErrorRedirected) {
    throw 'Run this acceptance script in an interactive Windows console with stdin and stderr attached. Native confirmation requires a human to review the plan and type its approval phrase.'
}
Write-Host 'This acceptance script installs the package and, after interactive approval, changes the current user PHP activation/PATH lease. Run only for an explicitly approved acceptance session.'

$installRoot = Join-Path $env:TEMP ('cxszn-menv-package-enable-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $installRoot -Force | Out-Null
$uninstaller = Join-Path $installRoot 'uninstall.exe'
$result = [ordered]@{
    ok = $false
    installer = $resolvedInstaller
    install_root = $installRoot
    install_exit = $null
    apply_ok = $false
    apply_status = $null
    activation_smoke_ok = $false
    where_php = @()
    php_version = $null
    uninstall_exit = $null
    temp_root_removed = $false
}

try {
    $installer = Start-Process -FilePath $resolvedInstaller -ArgumentList @('/S', "/D=$installRoot") -WindowStyle Hidden -Wait -PassThru
    $result.install_exit = $installer.ExitCode
    if ($installer.ExitCode -ne 0) { throw "Installer exited with code $($installer.ExitCode)." }

    $executable = Join-Path $installRoot 'cxszn-menv.exe'
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw "Installed executable was not found: $executable"
    }
    $state = @(& $executable doctor --json 2>&1) -join "`n" | ConvertFrom-Json
    if (-not $state.ok) { throw 'Packaged doctor returned ok=false.' }

    $runtimeId = 'php:win:' + $resolvedPhp.ToLowerInvariant()
    $idempotencyKey = 'packaged-enable-' + [guid]::NewGuid().ToString('N')
    $plan = (@(& $executable runtime enable --family php --runtime-id $runtimeId --idempotency-key $idempotencyKey --json 2>&1) -join "`n") | ConvertFrom-Json
    if (-not $plan.ok) { throw 'Packaged enable plan failed.' }
    Write-Host 'Review the following native plan summary and type the displayed approve <full-plan-UUID> phrase yourself. Other input cancels; no automatic approval is supplied.'
    # Capture only stdout. The native prompt must retain its console stderr handle.
    $confirmation = (@(& $executable change confirm --plan-id $plan.result.planId --json) -join "`n") | ConvertFrom-Json
    if (-not $confirmation.ok) { throw 'Packaged confirmation was refused or cancelled. Stdin and stderr must be real Windows console handles; do not pipe approval or redirect/merge stderr.' }
    $apply = (@(& $executable change apply --plan-id $plan.result.planId --confirmation-token $confirmation.result.value --idempotency-key $idempotencyKey --json 2>&1) -join "`n") | ConvertFrom-Json
    $result.apply_ok = [bool]$apply.ok
    if (-not $apply.ok) { throw 'Packaged enable apply failed.' }
    $result.apply_status = $apply.result.status

    $smokeScript = Join-Path $PSScriptRoot 'Test-PhpActivationSmoke.ps1'
    $smoke = (@(pwsh -NoProfile -File $smokeScript -ExpectedVersion (($resolvedPhp | Split-Path -Parent | Split-Path -Leaf) -replace '^php', '') 2>&1) -join "`n") | ConvertFrom-Json
    $result.activation_smoke_ok = [bool]$smoke.ok
    $result.where_php = @($smoke.where_php)
    $result.php_version = $smoke.php_version
    if (-not $smoke.ok) { throw 'Packaged activation smoke failed.' }

    $uninstallerProcess = Start-Process -FilePath $uninstaller -ArgumentList '/S' -WindowStyle Hidden -Wait -PassThru
    $result.uninstall_exit = $uninstallerProcess.ExitCode
    if ($uninstallerProcess.ExitCode -ne 0) { throw "Uninstaller exited with code $($uninstallerProcess.ExitCode)." }
    $result.ok = $true
}
catch {
    $result.error = $_.Exception.Message
}
finally {
    if (Test-Path -LiteralPath $installRoot) {
        Remove-Item -LiteralPath $installRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    $result.temp_root_removed = -not (Test-Path -LiteralPath $installRoot)
    $result | ConvertTo-Json -Depth 5
}

if (-not $result.ok) { exit 1 }
