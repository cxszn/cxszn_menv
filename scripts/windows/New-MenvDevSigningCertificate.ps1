[CmdletBinding()]
param(
    [string]$Subject = 'CN=MEnv Developer Preview',
    [string]$PfxPath = (Join-Path (Get-Location) 'artifacts\signing\menv-dev-preview.pfx'),
    [int]$ValidityYears = 2
)

$ErrorActionPreference = 'Stop'

if ($ValidityYears -lt 1 -or $ValidityYears -gt 5) {
    throw 'ValidityYears must be between 1 and 5.'
}

$resolvedPfxPath = [System.IO.Path]::GetFullPath($PfxPath)
$parent = Split-Path -Parent $resolvedPfxPath
New-Item -ItemType Directory -Force -Path $parent | Out-Null

$password = Read-Host 'Enter a password for the developer PFX (it is never written to the repository)' -AsSecureString
$certificate = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $Subject `
    -CertStoreLocation 'Cert:\CurrentUser\My' `
    -HashAlgorithm SHA256 `
    -KeyAlgorithm RSA `
    -KeyLength 3072 `
    -KeyExportPolicy Exportable `
    -NotAfter (Get-Date).AddYears($ValidityYears)

try {
    Export-PfxCertificate -Cert $certificate -FilePath $resolvedPfxPath -Password $password -ChainOption EndEntityCertOnly | Out-Null
    [pscustomobject]@{
        subject = $certificate.Subject
        thumbprint = $certificate.Thumbprint
        pfx_path = $resolvedPfxPath
        not_after = $certificate.NotAfter.ToUniversalTime().ToString('o')
        trust = 'local developer certificate; not suitable for public release'
    } | ConvertTo-Json -Compress
}
finally {
    $password.Dispose()
}
