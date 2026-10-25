<#
.SYNOPSIS
  Installs, updates and uninstalls a built setup silently, and checks what
  each step leaves on disk and in the registry. CI runs it on every change.

.EXAMPLE
  ./scripts/smoke-test-setup.ps1 -Setup backend/target/release/bundle/setup/MYLE.exe
#>
param([Parameter(Mandatory = $true)][string]$Setup)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$config = Get-Content (Join-Path $root "backend/tauri.conf.json") -Raw | ConvertFrom-Json
$version = (Get-Content (Join-Path $root "package.json") -Raw | ConvertFrom-Json).version
$binary = $config.mainBinaryName
$publisher = $config.bundle.publisher
$productKey = "HKCU:\Software\$publisher\MakeYourLifeEasier"
$publisherKey = "HKCU:\Software\$publisher"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\MakeYourLifeEasier"
# RUNNER_TEMP on CI; %TEMP% may be spelled with 8.3 short names elsewhere.
$base = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$dir = Join-Path $base "myle-setup-smoke-test\$binary"
$baseFull = [IO.Path]::GetFullPath($base).TrimEnd('\')
$testRootFull = [IO.Path]::GetFullPath((Join-Path $baseFull "myle-setup-smoke-test")).TrimEnd('\')
$dir = [IO.Path]::GetFullPath($dir)
if (-not $dir.StartsWith("$testRootFull\", [StringComparison]::OrdinalIgnoreCase)) {
  throw "Refusing to use an unexpected smoke-test install path: $dir"
}
$backup = Join-Path $baseFull ("myle-setup-smoke-test-restore-" + [guid]::NewGuid().ToString("N"))
if (-not ([IO.Path]::GetFullPath($backup)).StartsWith("$baseFull\", [StringComparison]::OrdinalIgnoreCase)) {
  throw "Refusing to use an unexpected smoke-test backup path: $backup"
}
$shortcutPaths = [ordered]@{
  Desktop = Join-Path ([Environment]::GetFolderPath("Desktop")) "$($config.productName).lnk"
  StartMenu = Join-Path ([Environment]::GetFolderPath("Programs")) "$($config.productName).lnk"
  Startup = Join-Path ([Environment]::GetFolderPath("Startup")) "$($config.productName).lnk"
}

function Assert([bool]$Condition, [string]$What) {
  if (-not $Condition) { throw "FAILED: $What" }
  Write-Host "ok  $What"
}

function Invoke-Silently([string]$Program, [string[]]$Arguments) {
  $process = Start-Process -FilePath $Program -ArgumentList $Arguments -Wait -PassThru
  return $process.ExitCode
}

$registryEntries = @(
  @{ Name = "product"; Path = $productKey; NativePath = "HKEY_CURRENT_USER\Software\$publisher\MakeYourLifeEasier" }
  @{ Name = "uninstall"; Path = $uninstallKey; NativePath = "HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Uninstall\MakeYourLifeEasier" }
)
New-Item -ItemType Directory -Path $backup -Force | Out-Null
$shortcutWasPresent = @{}
foreach ($name in $shortcutPaths.Keys) {
  $shortcutWasPresent[$name] = Test-Path -LiteralPath $shortcutPaths[$name]
  if ($shortcutWasPresent[$name]) {
    Copy-Item -LiteralPath $shortcutPaths[$name] -Destination (Join-Path $backup "$name.lnk") -Force
  }
}
$registryWasPresent = @{}
foreach ($entry in $registryEntries) {
  $registryWasPresent[$entry.Name] = Test-Path -LiteralPath $entry.Path
  if ($registryWasPresent[$entry.Name]) {
    $savedKey = Join-Path $backup "$($entry.Name).reg"
    & reg.exe export $entry.NativePath $savedKey /y | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not back up registry key $($entry.Path)" }
  }
}
$publisherWasPresent = Test-Path -LiteralPath $publisherKey

try {
  # The installer writes shortcuts into the current user's real known folders.
  # Temporarily clear these exact names so the test can check a clean install
  # into $dir without colliding with the user's existing app or unrelated link.
  foreach ($path in $shortcutPaths.Values) {
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
  }
  if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }

  # A dead Start menu link under our name, as an earlier install leaves
  # behind: the setup must take the name over, not add a second entry.
  $dead = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcutPaths.StartMenu)
  $dead.TargetPath = Join-Path $dir "gone\$binary.exe"
  $dead.Save()

# Install
Assert ((Invoke-Silently $Setup @("/S", "/D=$dir")) -eq 0) "silent install exits 0"
foreach ($file in @("$binary.exe", "uninstall.exe", "install.json")) {
  Assert (Test-Path (Join-Path $dir $file)) "installed $file"
}
foreach ($resource in $config.bundle.resources.PSObject.Properties) {
  Assert (Test-Path (Join-Path $dir $resource.Value)) "installed $($resource.Value)"
}
$entry = Get-ItemProperty $uninstallKey
Assert ($entry.DisplayVersion -eq $version) "Installed apps lists version $version"
Assert ($entry.DisplayName -eq "MYLE") "Installed apps displays MYLE"
Assert ($entry.UninstallString -like "*uninstall.exe*") "Installed apps knows the uninstaller"
Assert ($entry.QuietUninstallString -like "*/S") "Installed apps has a quiet uninstall"
$shell = New-Object -ComObject WScript.Shell
$expectedExe = Join-Path $dir "$binary.exe"
function Assert-Shortcuts([string]$ExpectedTarget, [string[]]$Names) {
  foreach ($name in $Names) {
    $path = $shortcutPaths[$name]
    Assert (Test-Path -LiteralPath $path) "$name shortcut created"
    $link = $shell.CreateShortcut($path)
    Assert ($link.TargetPath -ieq $ExpectedTarget) "$name shortcut opens the installed app (found '$($link.TargetPath)')"
    $expectedArguments = if ($name -eq "Startup") { "--autostart" } else { "" }
    Assert ($link.Arguments -eq $expectedArguments) "$name shortcut has the expected arguments"
    $expectedIcon = Join-Path (Split-Path -Parent $ExpectedTarget) "MYLE.ico"
    Assert ($link.IconLocation -ieq "$expectedIcon,0") "$name shortcut uses the stable installed icon"
  }
}
# A new install: Desktop and Start menu; starting with Windows is opt-in.
Assert-Shortcuts $expectedExe @("Desktop", "StartMenu")
Assert (-not (Test-Path -LiteralPath $shortcutPaths.Startup)) "no Startup shortcut unless chosen"
$startMenuFallback = Join-Path ([Environment]::GetFolderPath("Programs")) "$($config.productName) - $publisher.lnk"
Assert (-not (Test-Path -LiteralPath $startMenuFallback)) "no second Start menu entry beside the replaced dead link"
$choices = Get-ItemProperty -LiteralPath $productKey
foreach ($name in @("DesktopShortcut", "StartMenuShortcut")) {
  Assert ($choices.$name -eq 2) "shortcut $name remembered as wanted and made"
}
Assert ($choices.StartupShortcut -eq 0) "starting with Windows remembered as off"
Assert ($choices.TaskbarShortcut -eq 1) "the default taskbar choice is remembered separately"
Assert ($choices.TaskbarPinPending -eq 1) "the taskbar request waits for the app and Windows confirmation"
# A completed/dismissed request must not clear the selected setup checkbox.
Set-ItemProperty -LiteralPath $productKey -Name TaskbarPinPending -Value 0 -Type DWord

# The user switches "Start with Windows" on in Settings, which writes the
# same shortcut the setup does and records it as made (backend/src/startup.rs).
$link = $shell.CreateShortcut($shortcutPaths.Startup)
$link.TargetPath = $expectedExe
$link.Arguments = "--autostart"
$link.WorkingDirectory = $dir
$link.IconLocation = "$expectedExe,0"
$link.Description = $config.productName
$link.Save()
Set-ItemProperty -LiteralPath $productKey -Name StartupShortcut -Value 2 -Type DWord

# Simulate old links which retain our icon and working folder but have lost
# their executable target.
foreach ($name in @("StartMenu", "Startup")) {
  $link = $shell.CreateShortcut($shortcutPaths[$name])
  $link.TargetPath = ""
  $link.Save()
}
# A missing Desktop shortcut is repaired during the update.
Remove-Item -LiteralPath $shortcutPaths.Desktop

# Simulate an existing installation with the old executable name. The new
# setup must replace it with a working compatibility copy for the old updater.
$legacyExe = Join-Path $dir "MakeYourLifeEasier.exe"
[IO.File]::WriteAllBytes($legacyExe, [byte[]](1, 2, 3, 4))

# Update over it, as the in-app updater does (without relaunching)
Assert ((Invoke-Silently $Setup @("/S", "/UPDATE")) -eq 0) "silent update exits 0"
Assert ((Get-FileHash $legacyExe).Hash -eq (Get-FileHash $expectedExe).Hash) "old executable launches the new build"
Assert ((Get-Content (Join-Path $dir "install.json") -Raw) -match '"MakeYourLifeEasier.exe"') "legacy executable is tracked for uninstall"
Assert (-not (Get-ChildItem $dir -Recurse -Filter "*.myle-*")) "no leftovers from the update"
Assert-Shortcuts $expectedExe @("Desktop", "StartMenu", "Startup")
Assert ((Get-ItemProperty -LiteralPath $productKey).DesktopShortcut -eq 2) "the missing Desktop shortcut is repaired and remembered"
Assert ((Get-ItemProperty -LiteralPath $productKey).TaskbarShortcut -eq 1) "the update keeps the selected taskbar checkbox"
Assert ((Get-ItemProperty -LiteralPath $productKey).TaskbarPinPending -eq 0) "the update does not repeat a handled taskbar request"

# Explicitly disabled access/startup choices stay disabled after another update.
foreach ($name in @("DesktopShortcut", "StartMenuShortcut", "StartupShortcut", "TaskbarShortcut", "TaskbarPinPending")) {
  Set-ItemProperty -LiteralPath $productKey -Name $name -Value 0 -Type DWord
}
foreach ($path in $shortcutPaths.Values) { Remove-Item -LiteralPath $path -ErrorAction SilentlyContinue }
Assert ((Invoke-Silently $Setup @("/S", "/UPDATE")) -eq 0) "an update with opted-out shortcuts exits 0"
foreach ($path in $shortcutPaths.Values) {
  Assert (-not (Test-Path -LiteralPath $path)) "an unchecked shortcut is not recreated on update"
}
Assert ((Get-ItemProperty -LiteralPath $productKey).TaskbarShortcut -eq 0) "the update keeps taskbar disabled"
Assert ((Get-ItemProperty -LiteralPath $productKey).TaskbarPinPending -eq 0) "the update does not queue a declined taskbar choice"

# Uninstall
Assert ((Invoke-Silently (Join-Path $dir "uninstall.exe") @("/S")) -eq 0) "silent uninstall exits 0"
Assert (-not (Test-Path $uninstallKey)) "Installed apps entry removed"
foreach ($name in $shortcutPaths.Keys) {
  Assert (-not (Test-Path -LiteralPath $shortcutPaths[$name])) "$name shortcut removed"
}
# The uninstaller runs from a copy in %TEMP%, which removes the original
# and the folder once the original has exited.
$deadline = (Get-Date).AddSeconds(30)
while ((Test-Path $dir) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
if (Test-Path $dir) {
  $left = Get-ChildItem $dir -Recurse -Force | ForEach-Object { $_.FullName.Substring($dir.Length) }
  Write-Host "Still in the install folder: $($left -join ', ')"
  Get-Process | Where-Object { $_.Path -like "*$binary*" -or $_.Path -like "*uninstall*" } |
    ForEach-Object { Write-Host "Running: $($_.Id) $($_.Path)" }
}
Assert (-not (Test-Path $dir)) "install folder removed"
Write-Host "Setup smoke test passed."
}
finally {
  # Restore the caller's shortcuts and registry entries if any assertion fails.
  $restoreErrors = [System.Collections.Generic.List[string]]::new()
  if (Test-Path -LiteralPath $dir) {
    Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
  }
  foreach ($name in $shortcutPaths.Keys) {
    $path = $shortcutPaths[$name]
    $savedLink = Join-Path $backup "$name.lnk"
    try {
      # Remove only the shortcut created by this test before restoring the
      # original bytes. This also restores unrelated same-named shortcuts.
      if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
      if ($shortcutWasPresent[$name]) {
        Copy-Item -LiteralPath $savedLink -Destination $path -Force
      }
    } catch {
      $restoreErrors.Add("$name shortcut: $_")
      Write-Warning "Could not restore $name shortcut from $savedLink`: $_"
    }
  }
  foreach ($entry in $registryEntries) {
    if (Test-Path -LiteralPath $entry.Path) {
      Remove-Item -LiteralPath $entry.Path -Recurse -Force
    }
    if ($registryWasPresent[$entry.Name]) {
      $savedKey = Join-Path $backup "$($entry.Name).reg"
      & reg.exe import $savedKey | Out-Null
      if ($LASTEXITCODE -ne 0) { throw "Could not restore registry key $($entry.Path)" }
    }
  }
  if ($publisherWasPresent -and -not (Test-Path -LiteralPath $publisherKey)) {
    New-Item -Path $publisherKey -Force | Out-Null
  } elseif (-not $publisherWasPresent -and (Test-Path -LiteralPath $publisherKey)) {
    $publisherItem = Get-Item -LiteralPath $publisherKey
    $publisherChildren = $publisherItem.GetSubKeyNames()
    $publisherValues = $publisherItem.GetValueNames()
    if ($publisherChildren.Length -eq 0 -and $publisherValues.Length -eq 0) {
      Remove-Item -LiteralPath $publisherKey -Force
    }
  }
  if ($restoreErrors.Count -eq 0) {
    Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue
  } else {
    Write-Warning "Restore files are being kept at $backup. $($restoreErrors -join '; ')"
  }
}
