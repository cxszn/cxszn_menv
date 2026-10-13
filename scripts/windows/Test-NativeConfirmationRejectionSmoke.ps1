#Requires -Version 7.0
[CmdletBinding()]
param(
    [string]$ExecutablePath = (Join-Path (Get-Location) 'target\x86_64-pc-windows-msvc\release\cxszn-menv.exe')
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = [IO.Path]::GetFullPath($ExecutablePath)
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
    throw "MEnv executable does not exist: $resolvedExecutable"
}
$evidenceRoot = Join-Path $env:TEMP ('cxszn-menv-confirmation-rejection-' + [guid]::NewGuid().ToString('N'))
$dataRoot = Join-Path $evidenceRoot 'localappdata'
$appRoot = Join-Path $dataRoot 'cxszn-menv'
$fixtureRoot = Join-Path $evidenceRoot 'fixture'
$iniPath = Join-Path $fixtureRoot 'php.ini'
$legacyPath = Join-Path $appRoot 'state-rust.json'
$planPath = Join-Path $appRoot 'cli-change-plan.json'
$runtimeId = 'php:win:confirmation-rejection-fixture'
$idempotencyKey = 'confirmation-rejection-' + [guid]::NewGuid().ToString('N')
$previousLocalAppData = $env:LOCALAPPDATA
$previousUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
New-Item -ItemType Directory -Path $appRoot, $fixtureRoot | Out-Null

function Invoke-HeadlessJsonCommand {
    param([string[]]$Arguments, [string]$InputText = '')
    $start = [Diagnostics.ProcessStartInfo]::new($resolvedExecutable)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WorkingDirectory = $evidenceRoot
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'Native CLI process did not start.' }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        try {
            if ($InputText.Length -gt 0) { $process.StandardInput.Write($InputText) }
        }
        catch [IO.IOException] {
            # A rejected console handle may close the pipe before reading input.
        }
        finally { $process.StandardInput.Close() }
        if (-not $process.WaitForExit(30000)) {
            $process.Kill($true)
            $process.WaitForExit()
            throw 'Native CLI command timed out.'
        }
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        $lines = @($stdout -split '\r?\n' | Where-Object { $_.Trim().Length -gt 0 })
        if ($lines.Count -ne 1) { throw 'Native CLI stdout must contain exactly one JSON envelope.' }
        try { $value = $stdout | ConvertFrom-Json } catch { throw 'Native CLI stdout was not a JSON envelope.' }
        # Never persist raw confirm output: an unsafe older binary might issue a token.
        [pscustomobject]@{ value = $value; exit_code = $process.ExitCode; stderr_empty = [string]::IsNullOrWhiteSpace($stderr) }
    }
    finally { $process.Dispose() }
}

$result = [ordered]@{
    ok = $false
    executable = $resolvedExecutable
    executable_sha256 = (Get-FileHash -LiteralPath $resolvedExecutable -Algorithm SHA256).Hash
    evidence_root = $evidenceRoot
}
try {
    $env:LOCALAPPDATA = $dataRoot
    [IO.File]::WriteAllText($iniPath, "; isolated confirmation fixture`r`nmemory_limit=128M`r`n", [Text.UTF8Encoding]::new($false))
    $fixture = [ordered]@{
        schemaVersion = '1'
        runtimes = @([ordered]@{
            id = $runtimeId; family = 'php'; version = '8.5.0'
            executablePath = (Join-Path $fixtureRoot 'php.exe'); rootPath = $fixtureRoot
            architecture = 'x64'; threadSafety = 'nts'; ownership = 'manual'; availability = 'manual'
            activation = 'disabled'; health = 'unknown'
            ini = [ordered]@{ source = 'explicit'; path = $iniPath }
            legacy = $false; diagnostics = @(); firstSeenAt = 'fixture'; lastCheckedAt = 'fixture'
        })
        pathLease = [ordered]@{ scope = 'user'; enabled = $false; path = $null; runtimeId = $null; entries = @(); hash = ''; updatedAt = $null }
        operations = @()
        updatedAt = 'confirmation-rejection-fixture'
    }
    $fixture | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $legacyPath -Encoding utf8
    $sourceHash = (Get-FileHash -LiteralPath $iniPath -Algorithm SHA256).Hash
    $legacyHash = (Get-FileHash -LiteralPath $legacyPath -Algorithm SHA256).Hash
    $before = Invoke-HeadlessJsonCommand -Arguments @('doctor', '--json')
    if ($before.value.ok -ne $true -or $before.exit_code -ne 0) { throw 'Fixture state could not be read.' }
    if (@($before.value.result.runtimes).Count -ne 1 -or $before.value.result.runtimes[0].id -ne $runtimeId) {
        throw 'Native CLI did not load the isolated runtime fixture.'
    }
    $plan = Invoke-HeadlessJsonCommand -Arguments @('change', 'plan', '--action', 'backup-ini', '--family', 'php', '--runtime-id', $runtimeId, '--idempotency-key', $idempotencyKey, '--json')
    if ($plan.value.ok -ne $true -or $plan.exit_code -ne 0) { throw 'Source-bound backup plan failed.' }
    $result.source_bound_plan = $plan.value.result.backupSource.path -ceq $iniPath `
        -and $plan.value.result.backupSource.sha256 -eq $sourceHash
    if (-not $result.source_bound_plan) { throw 'Backup plan did not bind the fixture source and digest.' }
    $planId = $plan.value.result.planId
    $planHash = (Get-FileHash -LiteralPath $planPath -Algorithm SHA256).Hash
    $confirmArgs = @('change', 'confirm', '--plan-id', $planId, '--json')
    $pipedApproval = Invoke-HeadlessJsonCommand -Arguments $confirmArgs -InputText "approve $planId`n"
    $pipedEof = Invoke-HeadlessJsonCommand -Arguments $confirmArgs
    $result.piped_approval_code = $pipedApproval.value.error.code
    $result.piped_eof_code = $pipedEof.value.error.code
    $result.piped_approval_rejected = $pipedApproval.value.ok -eq $false `
        -and $pipedApproval.value.error.code -eq 'CONFIRMATION_REQUIRED' -and $pipedApproval.exit_code -ne 0
    $result.piped_eof_rejected = $pipedEof.value.ok -eq $false `
        -and $pipedEof.value.error.code -eq 'CONFIRMATION_REQUIRED' -and $pipedEof.exit_code -ne 0

    # This value is deliberately unissued; no successful approval is automated.
    $forgedApply = Invoke-HeadlessJsonCommand -Arguments @('change', 'apply', '--plan-id', $planId, '--confirmation-token', 'unissued-smoke-value', '--idempotency-key', $idempotencyKey, '--json')
    $result.forged_apply_code = $forgedApply.value.error.code
    $result.forged_apply_rejected = $forgedApply.value.ok -eq $false `
        -and $forgedApply.value.error.code -eq 'CONFIRMATION_INVALID' -and $forgedApply.exit_code -ne 0
    $after = Invoke-HeadlessJsonCommand -Arguments @('doctor', '--json')
    $result.state_unchanged = $after.value.ok -eq $true -and $after.exit_code -eq 0 `
        -and ($before.value.result | ConvertTo-Json -Depth 20 -Compress) -ceq ($after.value.result | ConvertTo-Json -Depth 20 -Compress)
    $result.no_operation_recorded = @($after.value.result.operations).Count -eq 0
    $result.source_unchanged = (Get-FileHash -LiteralPath $iniPath -Algorithm SHA256).Hash -ceq $sourceHash
    $result.legacy_source_preserved = (Get-FileHash -LiteralPath $legacyPath -Algorithm SHA256).Hash -ceq $legacyHash
    $result.plan_preserved = (Test-Path -LiteralPath $planPath -PathType Leaf) `
        -and (Get-FileHash -LiteralPath $planPath -Algorithm SHA256).Hash -ceq $planHash
    $result.no_backup_directory = -not (Test-Path -LiteralPath (Join-Path $appRoot 'backups'))
    $result.stderr_separate_and_empty = @($before, $plan, $pipedApproval, $pipedEof, $forgedApply, $after).stderr_empty -notcontains $false
    $result.ok = $result.source_bound_plan -and $result.piped_approval_rejected -and $result.piped_eof_rejected `
        -and $result.forged_apply_rejected -and $result.state_unchanged -and $result.no_operation_recorded `
        -and $result.source_unchanged -and $result.legacy_source_preserved -and $result.plan_preserved `
        -and $result.no_backup_directory -and $result.stderr_separate_and_empty
}
catch {
    $result.ok = $false
    $result.error = $_.Exception.Message
}
finally {
    $env:LOCALAPPDATA = $previousLocalAppData
    $result.process_environment_restored = $env:LOCALAPPDATA -ceq $previousLocalAppData
    $result.user_path_unchanged = [Environment]::GetEnvironmentVariable('Path', 'User') -ceq $previousUserPath
    $result.ok = $result.ok -and $result.process_environment_restored -and $result.user_path_unchanged
    $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'result.json') -Encoding utf8
    $result | ConvertTo-Json -Depth 5
}
if (-not $result.ok) { exit 1 }
