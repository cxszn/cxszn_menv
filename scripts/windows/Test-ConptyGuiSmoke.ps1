[CmdletBinding()]
param(
    [string]$ExecutablePath
)

$ExecutablePath = if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
    Join-Path $PSScriptRoot '..\..\target\debug\examples\conpty_smoke.exe'
} else {
    $ExecutablePath
}
$resolved = [System.IO.Path]::GetFullPath($ExecutablePath)
if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
    throw "ConPTY smoke executable was not found: $resolved"
}

$process = Start-Process -FilePath $resolved -WindowStyle Hidden -Wait -PassThru
$result = [ordered]@{
    executable = $resolved
    exit_code = $process.ExitCode
    passed = ($process.ExitCode -eq 0)
}
$result | ConvertTo-Json -Compress
if (-not $result.passed) {
    exit 1
}
