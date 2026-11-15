<#
.SYNOPSIS
  Downloads and verifies the pinned Ludusavi runtime and manifest used by Game Saves.

.DESCRIPTION
  Generated files are placed in backend/resources/ludusavi and are intentionally
  ignored by Git. Release builds run this script before Tauri bundles the app.

.PARAMETER Force
  Download and replace the generated files even when every local hash already matches.
#>
[CmdletBinding()]
param([switch]$Force)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ludusaviVersion = "0.31.0"
$manifestCommit = "95d9b7af64cbe3fc9585395c511aa8c17f821463"

$runtimeArchive = [ordered]@{
  Uri    = "https://github.com/mtkennerly/ludusavi/releases/download/v$ludusaviVersion/ludusavi-v$ludusaviVersion-win64.zip"
  Sha256 = "f47a8ad8c708f01d2eb124704973beffab205e292f5287a10fc4a101f8d68706"
}
$legalArchive = [ordered]@{
  Uri    = "https://github.com/mtkennerly/ludusavi/releases/download/v$ludusaviVersion/ludusavi-v$ludusaviVersion-legal.zip"
  Sha256 = "9fce11fc44942efdd5969928d89b106c229f9a041d7a713c424a6da981660ebf"
}
$manifest = [ordered]@{
  Uri    = "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/$manifestCommit/data/manifest.yaml"
  Sha256 = "4f105f0664e3ed33cf2a89c78e485b33481511f0d20d4bfd6bca7fc12be32424"
}
$license = [ordered]@{
  Uri    = "https://raw.githubusercontent.com/mtkennerly/ludusavi/v$ludusaviVersion/LICENSE"
  Sha256 = "dba5a9bdc2280142b90a68fcd76134f48e6c765f14bfde7356b19b55370b27aa"
}

$expectedOutputs = [ordered]@{
  "ludusavi.exe"            = "5192fd1ef31b5718cdb787d3f41abdcca8682ba4cbad31812d44ca6630ec2c0c"
  "manifest.yaml"           = $manifest.Sha256
  "LICENSE"                 = $license.Sha256
  "THIRD-PARTY-NOTICES.txt" = "c126d61d62fe2ad5ec8cf05a8bae6fad7b78354c9a19f14a0179c9af7ed3bc49"
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$outputDir = Join-Path $repoRoot "backend\resources\ludusavi"

function Get-Sha256([string]$Path) {
  # Do not depend on Get-FileHash: some stripped-down Windows PowerShell
  # environments do not auto-load Microsoft.PowerShell.Utility.
  $stream = [System.IO.File]::OpenRead($Path)
  $sha = [System.Security.Cryptography.SHA256]::Create()
  try {
    $bytes = $sha.ComputeHash($stream)
    return -join ($bytes | ForEach-Object { $_.ToString("x2") })
  }
  finally {
    $sha.Dispose()
    $stream.Dispose()
  }
}

function Assert-Sha256([string]$Path, [string]$Expected) {
  $actual = Get-Sha256 $Path
  if ($actual -ne $Expected) {
    throw "SHA-256 mismatch for '$Path'. Expected $Expected, got $actual."
  }
}

function Test-OutputReady {
  foreach ($item in $expectedOutputs.GetEnumerator()) {
    $path = Join-Path $outputDir $item.Key
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
      return $false
    }
    if ((Get-Sha256 $path) -ne $item.Value) {
      return $false
    }
  }
  return $true
}

function Get-VerifiedDownload([string]$Uri, [string]$Sha256, [string]$Destination) {
  Write-Host "Downloading $Uri"
  Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $Destination -Headers @{
    "User-Agent" = "MYLE-build/$ludusaviVersion"
  }
  Assert-Sha256 $Destination $Sha256
}

function Copy-ZipEntry([string]$ArchivePath, [string]$EntryName, [string]$Destination) {
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
  try {
    # Not $matches or $input: those are PowerShell's own automatic variables.
    $entries = @($archive.Entries | Where-Object { $_.FullName -ceq $EntryName })
    if ($entries.Count -ne 1) {
      throw "Expected exactly one '$EntryName' entry in '$ArchivePath'; found $($entries.Count)."
    }

    $source = $entries[0].Open()
    try {
      $output = [System.IO.File]::Create($Destination)
      try {
        $source.CopyTo($output)
      }
      finally {
        $output.Dispose()
      }
    }
    finally {
      $source.Dispose()
    }
  }
  finally {
    $archive.Dispose()
  }
}

if (-not $Force -and (Test-OutputReady)) {
  Write-Host "Pinned Game Saves resources are already present and verified."
  exit 0
}

$stageRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("myle-game-saves-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $stageRoot | Out-Null

try {
  $runtimeZip = Join-Path $stageRoot "ludusavi-win64.zip"
  $legalZip = Join-Path $stageRoot "ludusavi-legal.zip"
  $stagedExe = Join-Path $stageRoot "ludusavi.exe"
  $stagedManifest = Join-Path $stageRoot "manifest.yaml"
  $stagedLicense = Join-Path $stageRoot "LICENSE"
  $stagedNotices = Join-Path $stageRoot "THIRD-PARTY-NOTICES.txt"

  Get-VerifiedDownload $runtimeArchive.Uri $runtimeArchive.Sha256 $runtimeZip
  Copy-ZipEntry $runtimeZip "ludusavi.exe" $stagedExe
  Assert-Sha256 $stagedExe $expectedOutputs["ludusavi.exe"]

  Get-VerifiedDownload $legalArchive.Uri $legalArchive.Sha256 $legalZip
  Copy-ZipEntry $legalZip "ludusavi-v$ludusaviVersion-legal.txt" $stagedNotices
  Assert-Sha256 $stagedNotices $expectedOutputs["THIRD-PARTY-NOTICES.txt"]

  Get-VerifiedDownload $manifest.Uri $manifest.Sha256 $stagedManifest
  Get-VerifiedDownload $license.Uri $license.Sha256 $stagedLicense

  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
  foreach ($item in $expectedOutputs.GetEnumerator()) {
    $source = Join-Path $stageRoot $item.Key
    $destination = Join-Path $outputDir $item.Key
    Copy-Item -LiteralPath $source -Destination $destination -Force
    Assert-Sha256 $destination $item.Value
  }

  Write-Host "Prepared Ludusavi $ludusaviVersion and manifest $manifestCommit in $outputDir"
}
finally {
  if (Test-Path -LiteralPath $stageRoot) {
    Remove-Item -LiteralPath $stageRoot -Recurse -Force
  }
}
