<#
.SYNOPSIS
  Authenticode-signs files with MYLE's certificate: the app exe, the
  uninstaller and the setup (scripts/build-setup.ps1, .github/workflows/release.yml).

.DESCRIPTION
  The certificate comes from the environment, one of:
    MYLE_SIGN_PFX       path to a .pfx file (a local build)
    CERT_B64            the .pfx itself, base64 (release.yml, from the
                        WIN_CSC_LINK secret, set on the signing step only):
                        written to a temporary file that is deleted again
  and its password in MYLE_SIGN_PASSWORD or CERT_PASS.

  The release workflow builds with no secret in the environment and runs this
  in a step of its own, so the certificate is never readable by the build's
  tools (npm packages, build scripts, the compiler).
#>
param([Parameter(Mandatory = $true)][string[]]$Path)

$ErrorActionPreference = "Stop"

$password = if ($env:MYLE_SIGN_PASSWORD) { $env:MYLE_SIGN_PASSWORD } else { "$env:CERT_PASS" }
$password = $password.Trim().Trim('"').Trim("'")
if ($env:GITHUB_ACTIONS -and $password) { Write-Host "::add-mask::$password" }

$temporary = $null
$pfx = $env:MYLE_SIGN_PFX
if (-not $pfx -and $env:CERT_B64) {
  $temporary = Join-Path ([IO.Path]::GetTempPath()) ("myle-sign-" + [guid]::NewGuid().ToString("N") + ".pfx")
  [IO.File]::WriteAllBytes($temporary, [Convert]::FromBase64String($env:CERT_B64.Trim()))
  $pfx = $temporary
}

try {
  if (-not $pfx -or -not (Test-Path -LiteralPath $pfx)) {
    throw "No certificate: set MYLE_SIGN_PFX (a .pfx file) or CERT_B64."
  }

  # signtool ships with the Windows SDK; take the newest x64 build.
  $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending |
    Select-Object -First 1
  if (-not $signtool) { throw "signtool.exe was not found (Windows SDK missing)." }

  foreach ($file in $Path) {
    $arguments = @(
      "sign", "/fd", "sha256",
      "/f", $pfx,
      "/p", $password,
      "/tr", "http://timestamp.digicert.com", "/td", "sha256",
      $file
    )
    # The timestamp server is occasionally busy; a signature without one would
    # stop validating when the certificate expires, so retry instead of skipping.
    $signed = $false
    for ($attempt = 1; $attempt -le 3 -and -not $signed; $attempt++) {
      & $signtool.FullName @arguments | Out-Host
      $signed = $LASTEXITCODE -eq 0
      if (-not $signed) { Start-Sleep -Seconds (5 * $attempt) }
    }
    if (-not $signed) { throw "signtool could not sign $file" }
  }
} finally {
  if ($temporary) { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
}
