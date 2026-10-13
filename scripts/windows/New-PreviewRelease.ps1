#Requires -Version 7.4
[CmdletBinding()]
param(
    [string]$RepoRoot = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $RepoRoot).Path
$output = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $output) { throw 'Output directory already exists; use a new release directory.' }
$version = (Get-Content -LiteralPath (Join-Path $root 'apps/tauri/package.json') -Raw | ConvertFrom-Json).version
$commit = git -C $root rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot determine source commit.' }
$dirty = git -C $root status --porcelain --untracked-files=normal
if ($LASTEXITCODE -ne 0 -or $dirty) { throw 'Commit reviewed sources before packaging a release.' }
$releaseRoot = Join-Path $root 'target/x86_64-pc-windows-msvc/release'
$installerName = "cxszn_menv_${version}_x64-setup.exe"
$sources = @{
    'cxszn-menv.exe' = (Join-Path $releaseRoot 'cxszn-menv.exe')
    'README.zh-CN.md' = (Join-Path $root 'docs/windows-preview.zh-CN.md')
    'THIRD-PARTY-NOTICES.md' = (Join-Path $root 'THIRD-PARTY-NOTICES.md')
}
$installerSource = Join-Path $releaseRoot "bundle/nsis/$installerName"
foreach ($source in @($sources.Values) + @($installerSource)) {
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Missing release input: $source" }
}
New-Item -ItemType Directory -Path $output | Out-Null
$zipRoot = Join-Path $output 'zip-content'
$folderName = "cxszn_menv-${version}-windows-x64"
$bundleRoot = Join-Path $zipRoot $folderName
New-Item -ItemType Directory -Path $bundleRoot | Out-Null
foreach ($name in $sources.Keys) { Copy-Item -LiteralPath $sources[$name] -Destination (Join-Path $bundleRoot $name) }
Copy-Item -LiteralPath $installerSource -Destination (Join-Path $output $installerName)
$files = foreach ($name in ($sources.Keys | Sort-Object)) {
    $file = Get-Item -LiteralPath (Join-Path $bundleRoot $name)
    [ordered]@{ name = $name; size_bytes = $file.Length; sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash }
}
$build = [ordered]@{
    product = 'cxszn_menv'; version = $version; source_commit = $commit.Trim()
    target = 'x86_64-pc-windows-msvc'; prerelease = $true; signed = $false
    executable_authenticode = (Get-AuthenticodeSignature -LiteralPath (Join-Path $bundleRoot 'cxszn-menv.exe')).Status.ToString()
    data_mode = 'current-user LOCALAPPDATA; not a portable data profile'; files = @($files)
}
$build | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $bundleRoot 'build-info.json') -Encoding utf8
$zipName = "cxszn_menv_${version}_windows-x64.zip"
$zipPath = Join-Path $output $zipName
[IO.Compression.ZipFile]::CreateFromDirectory($zipRoot, $zipPath, [IO.Compression.CompressionLevel]::Optimal, $false)
$archive = [IO.Compression.ZipFile]::OpenRead($zipPath)
try {
    $expected = @($sources.Keys) + @('build-info.json')
    if ($archive.Entries.Count -ne $expected.Count) { throw 'ZIP contains unexpected entries.' }
    foreach ($name in $expected) {
        $entry = $archive.GetEntry("$folderName/$name")
        if ($null -eq $entry) { throw "ZIP entry missing: $name" }
        $stream = $entry.Open()
        try { $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)) }
        finally { $stream.Dispose() }
        if ($hash -cne (Get-FileHash -LiteralPath (Join-Path $bundleRoot $name) -Algorithm SHA256).Hash) {
            throw "ZIP content hash mismatch: $name"
        }
    }
} finally { $archive.Dispose() }
$assets = foreach ($name in @($installerName, $zipName)) {
    $file = Get-Item -LiteralPath (Join-Path $output $name)
    [ordered]@{ name = $name; size_bytes = $file.Length; sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash }
}
$manifest = [ordered]@{
    product = 'cxszn_menv'; version = $version; source_commit = $commit.Trim(); prerelease = $true
    signed = $false; installer_authenticode = (Get-AuthenticodeSignature -LiteralPath (Join-Path $output $installerName)).Status.ToString()
    assets = @($assets); zip_verified = $true
}
$manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $output 'release-manifest.json') -Encoding utf8
$assets | ForEach-Object { "$($_.sha256.ToLowerInvariant())  $($_.name)" } | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding ascii
$manifest | ConvertTo-Json -Depth 6
