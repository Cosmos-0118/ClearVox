# Sordino uninstaller for Windows.
#
#   irm https://raw.githubusercontent.com/Cosmos-0118/Sordino/main/uninstall.ps1 | iex
#
# Removes the app, its settings, the Start Menu shortcut and the start-at-login entry, and
# puts your real microphone back as the Windows default. VB-CABLE is kept because other apps
# may use it; remove it under Settings > Apps > "VB-Audio Virtual Cable".

$ErrorActionPreference = 'Continue'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Sordino'
$exe = Join-Path $InstallDir 'sordino.exe'
$failed = $false

# Deletes a path, retrying while a dying process or antivirus still holds it. Returns $true when it is gone.
function Remove-Retry($path) {
    for ($i = 0; $i -lt 15; $i++) {
        if (-not (Test-Path $path)) { return $true }
        Remove-Item $path -Recurse -Force -ErrorAction SilentlyContinue
        if (-not (Test-Path $path)) { return $true }
        Start-Sleep -Milliseconds 400
    }
    return -not (Test-Path $path)
}

# End every running Sordino (the tray app has no window to close) and wait until it is really gone.
for ($i = 0; $i -lt 3 -and (Get-Process sordino -ErrorAction SilentlyContinue); $i++) {
    Write-Host 'Closing the running Sordino ...'
    Get-Process sordino -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    for ($j = 0; $j -lt 25 -and (Get-Process sordino -ErrorAction SilentlyContinue); $j++) { Start-Sleep -Milliseconds 200 }
}
if (Get-Process sordino -ErrorAction SilentlyContinue) {
    Write-Host 'Sordino would not close. Quit it from the tray icon (right-click > Quit) and run this again.' -ForegroundColor Red
    return
}
if (Test-Path $exe) { try { & $exe --restore-default 2>&1 | Out-Host } catch { } }
Remove-Item (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Sordino.lnk') -Force -ErrorAction SilentlyContinue
Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Sordino' -ErrorAction SilentlyContinue
foreach ($d in @($InstallDir, (Join-Path $env:APPDATA 'Sordino'))) {
    if (-not (Remove-Retry $d)) { $failed = $true; Write-Host "Couldn't remove $d (a file is locked). Close any program using it, or restart Windows, and run this again." -ForegroundColor Yellow }
}
if ($failed) { exit 1 }
Write-Host 'Sordino removed.' -ForegroundColor Green
