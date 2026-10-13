[CmdletBinding(DefaultParameterSetName = 'Thumbprint')]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,

    [Parameter(ParameterSetName = 'Thumbprint', Mandatory = $true)]
    [string]$CertificateThumbprint,

    [Parameter(ParameterSetName = 'Pfx', Mandatory = $true)]
    [string]$PfxPath,

    [Parameter(ParameterSetName = 'Pfx', Mandatory = $true)]
    [securestring]$PfxPassword
)

$ErrorActionPreference = 'Stop'

$resolvedInstallerPath = [System.IO.Path]::GetFullPath($InstallerPath)
if (-not (Test-Path -LiteralPath $resolvedInstallerPath -PathType Leaf)) {
    throw "Installer does not exist: $resolvedInstallerPath"
}
if ([System.IO.Path]::GetExtension($resolvedInstallerPath) -ne '.exe') {
    throw 'Only .exe installers may be signed by this helper.'
}

$temporaryCertificate = $null
try {
    if ($PSCmdlet.ParameterSetName -eq 'Pfx') {
        $resolvedPfxPath = [System.IO.Path]::GetFullPath($PfxPath)
        if (-not (Test-Path -LiteralPath $resolvedPfxPath -PathType Leaf)) {
            throw "PFX does not exist: $resolvedPfxPath"
        }
        $temporaryCertificate = Import-PfxCertificate `
            -FilePath $resolvedPfxPath `
            -CertStoreLocation 'Cert:\CurrentUser\My' `
            -Password $PfxPassword `
            -Exportable
        $CertificateThumbprint = $temporaryCertificate.Thumbprint
    }

    $certificate = Get-ChildItem "Cert:\CurrentUser\My\$CertificateThumbprint" -CodeSigningCert
    if ($null -eq $certificate) {
        throw "Code-signing certificate was not found: $CertificateThumbprint"
    }
    $result = Set-AuthenticodeSignature -FilePath $resolvedInstallerPath -Certificate $certificate -HashAlgorithm SHA256
    if ($result.Status -in @('NotSigned', 'HashMismatch')) {
        throw "Authenticode signing failed with status $($result.Status): $($result.StatusMessage)"
    }
    [pscustomobject]@{
        path = $resolvedInstallerPath
        status = $result.Status.ToString()
        signer = $certificate.Subject
        thumbprint = $certificate.Thumbprint
        note = 'Self-signed developer preview signature; Windows trust requires local certificate installation.'
    } | ConvertTo-Json -Compress
}
finally {
    if ($null -ne $temporaryCertificate) {
        Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($temporaryCertificate.Thumbprint)" -Force -ErrorAction SilentlyContinue
    }
    if ($null -ne $PfxPassword) {
        $PfxPassword.Dispose()
    }
}
