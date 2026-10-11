[CmdletBinding()]
param(
    [string]$InstallerPath,
    [string]$CatalogPath,
    [string]$OutputPath,
    [switch]$RequireWindows11,
    [switch]$Json
)

# This command is a read-only preflight and never claims Windows acceptance.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function New-Check {
    param(
        [Parameter(Mandatory = $true)][string]$Id,
        [Parameter(Mandatory = $true)][ValidateSet('pass', 'warn', 'fail')][string]$Status,
        [Parameter(Mandatory = $true)][string]$Message,
        [hashtable]$Data = @{}
    )

    [pscustomobject]@{
        id = $Id
        status = $Status
        message = $Message
        data = [pscustomobject]$Data
    }
}

function Get-Sha256Text {
    param([AllowNull()][string]$Value)

    $bytes = [Text.Encoding]::UTF8.GetBytes($Value ?? '')
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return ([Convert]::ToHexString($sha.ComputeHash($bytes))).ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function Get-PathSummary {
    param([ValidateSet('User', 'Machine')][string]$Scope)

    $value = [Environment]::GetEnvironmentVariable('Path', $Scope)
    $entries = @($value -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $lease = if ($env:LOCALAPPDATA) { "$env:LOCALAPPDATA\cxszn-menv\bin" } else { '' }
    $normalEntries = @($entries | ForEach-Object { $_.Trim().TrimEnd('\').ToLowerInvariant() })
    $normalLease = $lease.TrimEnd('\').ToLowerInvariant()

    [pscustomobject]@{
        sha256 = Get-Sha256Text $value
        entry_count = $entries.Count
        contains_menv_lease = [bool]($normalLease -and ($normalEntries -contains $normalLease))
    }
}

function Get-CommandState {
    param([Parameter(Mandatory = $true)][string]$Name)

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    [pscustomobject]@{
        name = $Name
        available = [bool]$command
        path = if ($command -and $command.Source) { [IO.Path]::GetFileName($command.Source) } else { $null }
    }
}

$checks = [System.Collections.Generic.List[object]]::new()
$isWindowsHost = $false
$os = $null

try {
    $isWindowsHost = $env:OS -eq 'Windows_NT'
    if ($isWindowsHost) {
        $os = Get-CimInstance -ClassName Win32_OperatingSystem
    }
}
catch {
    $checks.Add((New-Check -Id 'platform.query' -Status 'fail' -Message '无法读取 Windows 操作系统信息。' -Data @{ error = $_.Exception.Message }))
}

if (-not $isWindowsHost) {
    $status = if ($RequireWindows11) { 'fail' } else { 'warn' }
    $checks.Add((New-Check -Id 'platform.windows11' -Status $status -Message '当前主机不是 Windows；未执行 Windows API 预检。'))
}
else {
    $caption = [string]$os.Caption
    $build = [string]$os.BuildNumber
    $isWindows11 = $caption -match 'Windows 11'
    $is64Bit = [Environment]::Is64BitOperatingSystem
    $platformStatus = if ($isWindows11 -and $is64Bit) { 'pass' } elseif ($RequireWindows11) { 'fail' } else { 'warn' }
    $checks.Add((New-Check -Id 'platform.windows11' -Status $platformStatus -Message $(if ($isWindows11 -and $is64Bit) { 'Windows 11 x64 已识别。' } else { '目标不是 Windows 11 x64。' }) -Data @{ caption = $caption; build = $build; architecture = $(if ($is64Bit) { 'x64' } else { 'x86' }) }))
}

$commands = @('node', 'pnpm', 'pwsh', 'cargo', 'rustc', 'msbuild', 'signtool')
$commandStates = @($commands | ForEach-Object { Get-CommandState $_ })
$checks.Add((New-Check -Id 'toolchain.readonly' -Status 'pass' -Message '已读取本机工具链可见性；此步骤不安装工具。' -Data @{ commands = $commandStates }))

$webView2ClientId = '{F3017226-FE2A-4295-8BDF-00B3A9CE8BA1}'
$webViewRegistryPaths = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$webView2ClientId",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$webView2ClientId",
    "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$webView2ClientId"
)
$webViewFound = $false
if ($isWindowsHost) {
    foreach ($registryPath in $webViewRegistryPaths) {
        if (Test-Path -LiteralPath $registryPath) {
            $webViewFound = $true
            break
        }
    }
}
$checks.Add((New-Check -Id 'webview2.readonly' -Status $(if ($webViewFound) { 'pass' } else { 'warn' }) -Message $(if ($webViewFound) { '检测到 WebView2 注册表入口。' } else { '未确认 WebView2；需要在安装前补齐并记录版本。' })))

$userPath = Get-PathSummary -Scope User
$machinePath = Get-PathSummary -Scope Machine
$pathStatus = if ($machinePath.contains_menv_lease) { 'fail' } else { 'pass' }
$checks.Add((New-Check -Id 'path.readonly' -Status $pathStatus -Message $(if ($machinePath.contains_menv_lease) { '机器 PATH 已包含 MEnv lease，违反用户级边界。' } else { '已读取用户/机器 PATH 哈希；机器 PATH 未发现 MEnv lease。' }) -Data @{ user = $userPath; machine = $machinePath }))

$dataRoot = if ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA 'cxszn-menv' } else { $null }
$managedRootExists = [bool]($dataRoot -and (Test-Path -LiteralPath $dataRoot))
$managedFileCount = 0
if ($managedRootExists) {
    $managedFileCount = @(Get-ChildItem -LiteralPath $dataRoot -File -Recurse -Force -ErrorAction SilentlyContinue).Count
}
$checks.Add((New-Check -Id 'data-root.readonly' -Status 'pass' -Message '已读取 MEnv 数据根目录状态；此步骤不创建或删除文件。' -Data @{ exists = $managedRootExists; file_count = $managedFileCount }))

if ($InstallerPath) {
    if (Test-Path -LiteralPath $InstallerPath -PathType Leaf) {
        $installerHash = (Get-FileHash -LiteralPath $InstallerPath -Algorithm SHA256).Hash.ToLowerInvariant()
        $signature = if (Get-Command Get-AuthenticodeSignature -ErrorAction SilentlyContinue) { Get-AuthenticodeSignature -LiteralPath $InstallerPath } else { $null }
        $checks.Add((New-Check -Id 'installer.readonly' -Status 'pass' -Message '已读取安装包哈希和签名状态；没有执行安装包。' -Data @{ file_name = [IO.Path]::GetFileName($InstallerPath); sha256 = $installerHash; signature_status = if ($signature) { [string]$signature.Status } else { 'unavailable' }; signer = if ($signature -and $signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { $null } }))
    }
    else {
        $checks.Add((New-Check -Id 'installer.readonly' -Status 'fail' -Message '指定的安装包不存在。' -Data @{ path_supplied = $true }))
    }
}
else {
    $checks.Add((New-Check -Id 'installer.readonly' -Status 'warn' -Message '未提供安装包路径；预检不能证明 Tauri installer 已构建。' -Data @{ path_supplied = $false }))
}

if ($CatalogPath) {
    if (-not (Test-Path -LiteralPath $CatalogPath -PathType Leaf)) {
        $checks.Add((New-Check -Id 'catalog.readonly' -Status 'fail' -Message '指定的 catalog 文件不存在。'))
    }
    else {
        try {
            $catalog = Get-Content -LiteralPath $CatalogPath -Raw | ConvertFrom-Json
            $signatureKey = if ($catalog.signature) { [string]$catalog.signature.key_id } else { $null }
            $checks.Add((New-Check -Id 'catalog.readonly' -Status $(if ($catalog.fixture -eq $true -or -not $catalog.signature) { 'warn' } else { 'pass' }) -Message $(if ($catalog.fixture -eq $true -or -not $catalog.signature) { 'catalog 可解析，但 fixture/无签名不能用于生产下载验收。' } else { 'catalog 可解析并包含签名元数据；密码学验证仍需应用 verifier。' }) -Data @{ schema_version = [string]$catalog.schema_version; catalog_id = [string]$catalog.catalog_id; catalog_revision = $catalog.catalog_revision; fixture = [bool]$catalog.fixture; signature_key_id = $signatureKey; artifact_count = @($catalog.artifacts).Count; expires_at = [string]$catalog.expires_at }))
        }
        catch {
            $checks.Add((New-Check -Id 'catalog.readonly' -Status 'fail' -Message 'catalog 不是可解析的 JSON。' -Data @{ error = $_.Exception.Message }))
        }
    }
}
else {
    $checks.Add((New-Check -Id 'catalog.readonly' -Status 'warn' -Message '未提供 catalog 路径；预检不能证明签名下载链路。' -Data @{ path_supplied = $false }))
}

$hasFailure = @($checks | Where-Object status -eq 'fail').Count -gt 0
$result = [pscustomobject]@{
    schema_version = '1'
    generated_at = [DateTime]::UtcNow.ToString('o')
    evidence_scope = 'readonly-preflight'
    acceptance_status = 'preflight_only'
    host = [pscustomobject]@{
    is_windows = $isWindowsHost
        os_caption = if ($os) { [string]$os.Caption } else { $null }
        os_build = if ($os) { [string]$os.BuildNumber } else { $null }
        is_64bit = [Environment]::Is64BitOperatingSystem
    }
    checks = $checks
    exit_code = if ($hasFailure) { 1 } else { 0 }
}

if ($OutputPath) {
    $parent = Split-Path -Parent $OutputPath
    if ($parent) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    $result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $OutputPath -Encoding UTF8
}

if ($Json) {
    $result | ConvertTo-Json -Depth 12
}
else {
    $result.checks | Format-Table id, status, message -AutoSize
    Write-Output ("Acceptance status: {0}; evidence scope: {1}" -f $result.acceptance_status, $result.evidence_scope)
}

if ($hasFailure) {
    exit 1
}
