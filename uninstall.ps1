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

if (Test-Path $exe) {
    Get-Process sordino -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 500
    & $exe --restore-default 2>&1 | Out-Host
}
Remove-Item (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Sordino.lnk') -Force -ErrorAction SilentlyContinue
Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Sordino' -ErrorAction SilentlyContinue
Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $env:APPDATA 'Sordino') -Recurse -Force -ErrorAction SilentlyContinue
Write-Host 'Sordino removed.' -ForegroundColor Green
