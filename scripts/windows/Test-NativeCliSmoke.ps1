#Requires -Version 7.0
[CmdletBinding()]
param(
    [string]$ExecutablePath = (Join-Path (Get-Location) 'target\x86_64-pc-windows-msvc\release\cxszn-menv.exe'),
    [Parameter(Mandatory = $true)][string]$PhpPath
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = [System.IO.Path]::GetFullPath($ExecutablePath)
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
    throw "MEnv executable does not exist: $resolvedExecutable"
}

$evidenceRoot = Join-Path $env:TEMP ('cxszn-menv-native-cli-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $evidenceRoot | Out-Null
$previousLocalAppData = $env:LOCALAPPDATA
$previousUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')

function Invoke-JsonCommand {
    param([string[]]$Arguments)
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
        $process.StandardInput.Close()
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
    $env:LOCALAPPDATA = $evidenceRoot
    $doctor = Invoke-JsonCommand @('doctor', '--json')
    $catalog = Invoke-JsonCommand @('catalog', 'list', '--kind', 'php', '--json')
    $verify = Invoke-JsonCommand @('runtime', 'verify', '--family', 'php', '--path', $PhpPath, '--json')
    $unsupported = Invoke-JsonCommand @('runtime', 'enable', '--family', 'node', '--runtime-id', 'node:win:smoke', '--json')
    $registerPlan = Invoke-JsonCommand @('runtime', 'add', '--family', 'php', '--path', $PhpPath, '--idempotency-key', ('native-cli-smoke-' + [guid]::NewGuid().ToString('N')), '--json')
    if (-not $registerPlan.value.ok) { throw 'Native PHP registration plan failed.' }
    $registerConfirm = Invoke-JsonCommand @('change', 'confirm', '--plan-id', $registerPlan.value.result.plan.planId, '--json')
    $operationGet = Invoke-JsonCommand @('operation', 'get', '--id', 'native-cli-smoke-missing', '--json')
    $operationCancel = Invoke-JsonCommand @('operation', 'cancel', '--id', 'native-cli-smoke-missing', '--json')
    $after = Invoke-JsonCommand @('doctor', '--json')

    $result.doctor_ok = $doctor.value.ok -eq $true -and $doctor.exit_code -eq 0
    $result.catalog_ok = $catalog.value.ok -eq $true -and $catalog.exit_code -eq 0
    $result.catalog_count = @($catalog.value.result).Count
    $result.verify_ok = $verify.value.ok -eq $true -and $verify.exit_code -eq 0
    $result.verify_version = $verify.value.result.version
    $result.unsupported_node_code = $unsupported.value.error.code
    $result.register_plan_ok = $registerPlan.value.ok -eq $true -and $registerPlan.exit_code -eq 0
    $result.register_confirm_code = $registerConfirm.value.error.code
    $result.headless_confirmation_rejected = $registerConfirm.value.ok -eq $false `
        -and $registerConfirm.value.error.code -eq 'CONFIRMATION_REQUIRED' -and $registerConfirm.exit_code -ne 0
    $result.operation_get_missing_code = $operationGet.value.error.code
    $result.operation_cancel_missing_code = $operationCancel.value.error.code
    $result.stderr_separate_and_empty = @($doctor, $catalog, $verify, $unsupported, $registerPlan, $registerConfirm, $operationGet, $operationCancel, $after).stderr_empty -notcontains $false
    $result.state_unchanged = $after.value.ok -eq $true -and $after.exit_code -eq 0 `
        -and ($doctor.value.result | ConvertTo-Json -Depth 20 -Compress) -ceq ($after.value.result | ConvertTo-Json -Depth 20 -Compress)
    $result.plan_preserved = Test-Path -LiteralPath (Join-Path $evidenceRoot 'cxszn-menv\cli-register-plan.json') -PathType Leaf
    $result.ok = $result.doctor_ok -and $result.catalog_ok -and $result.catalog_count -ge 1 `
        -and $result.verify_ok -and $unsupported.value.ok -eq $false -and $unsupported.exit_code -ne 0 `
        -and $result.unsupported_node_code -eq 'UNSUPPORTED_ACTION' -and $result.register_plan_ok `
        -and $result.headless_confirmation_rejected -and $result.stderr_separate_and_empty `
        -and $operationGet.value.ok -eq $false -and $operationGet.exit_code -ne 0 `
        -and $result.operation_get_missing_code -eq 'OPERATION_NOT_FOUND' `
        -and $operationCancel.value.ok -eq $false -and $operationCancel.exit_code -ne 0 `
        -and $result.operation_cancel_missing_code -eq 'OPERATION_NOT_FOUND' `
        -and $result.state_unchanged -and $result.plan_preserved
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
    $result | ConvertTo-Json -Compress
}
if (-not $result.ok) { exit 1 }
