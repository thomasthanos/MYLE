<#
.SYNOPSIS
  Makes MYLE's release certificate, "ThomasT", once.

.DESCRIPTION
  A self-signed code signing certificate that is not a certificate authority:
  even with its key, nobody can vouch for a website with it, only sign
  programs. Valid for ten years.

  Writes:
    backend/installer/ThomasT.cer   The public part. Commit it: the setup
                                    offers to trust it for the Windows user
                                    (backend/installer/src/publisher.rs), and a
                                    test keeps its thumbprint in the updater's
                                    RELEASE_SIGNERS.
    <OutDir>\ThomasT.pfx            The private key, protected by the password
                                    asked for here. Never commit it. It goes
                                    into the WIN_CSC_LINK secret (base64) and
                                    the password into WIN_CSC_KEY_PASSWORD, but
                                    only after a release that lists the new
                                    thumbprint is out (signed with the old
                                    certificate); otherwise installed copies
                                    refuse every later update.

  The key is removed from this PC's certificate store once exported, so the
  .pfx is its only copy: keep it somewhere safe.

.EXAMPLE
  ./scripts/new-release-certificate.ps1
  ./scripts/new-release-certificate.ps1 -OutDir D:\Keys
#>
param([string]$OutDir = [Environment]::GetFolderPath("Desktop"))

$ErrorActionPreference = "Stop"

$root = Split-Path $PSScriptRoot -Parent
$cerPath = Join-Path $root "backend\installer\ThomasT.cer"
$pfxPath = Join-Path $OutDir "ThomasT.pfx"
# A second certificate with the same name would replace the one installed
# copies and trusting users already know.
if (Test-Path $cerPath) { throw "$cerPath already exists. Delete it first only if no release was signed with it." }
if (Test-Path $pfxPath) { throw "$pfxPath already exists." }
if (-not (Test-Path $OutDir)) { throw "$OutDir does not exist." }

$password = Read-Host -AsSecureString "Password for ThomasT.pfx"
$again = Read-Host -AsSecureString "The same password again"
$plain = { param($secure) [Net.NetworkCredential]::new("", $secure).Password }
if ((& $plain $password) -cne (& $plain $again)) { throw "The passwords differ." }
if ((& $plain $password).Length -lt 12) { throw "Use a password of at least 12 characters." }

$certificate = New-SelfSignedCertificate `
  -Type CodeSigningCert `
  -Subject "CN=ThomasT" `
  -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
  -KeyExportPolicy Exportable `
  -NotAfter (Get-Date).AddYears(10) `
  -CertStoreLocation Cert:\CurrentUser\My `
  -TextExtension @("2.5.29.19={critical}{text}ca=0")
try {
  Export-PfxCertificate -Cert $certificate -FilePath $pfxPath -Password $password | Out-Null
  Export-Certificate -Cert $certificate -FilePath $cerPath -Type CERT | Out-Null
} finally {
  Remove-Item "Cert:\CurrentUser\My\$($certificate.Thumbprint)" -DeleteKey
}

Write-Host ""
Write-Host "Public certificate: $cerPath"
Write-Host "Private key:        $pfxPath"
Write-Host "Thumbprint:         $($certificate.Thumbprint)"
Write-Host "Valid until:        $($certificate.NotAfter.ToString('yyyy-MM-dd'))"
