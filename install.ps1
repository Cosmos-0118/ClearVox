# ClearVox one-click installer for Windows.
#
#   irm https://raw.githubusercontent.com/Cosmos-0118/ClearVox/main/install.ps1 | iex
#
# What it does:
#   1. Installs the VB-CABLE virtual audio driver (the "virtual mic"), if missing.
#      It is downloaded from vb-audio.com and checked against a pinned SHA-256.
#   2. Downloads the latest ClearVox release (or builds from source with CLEARVOX_FROM_SOURCE=1).
#   3. Installs ClearVox into %LOCALAPPDATA%\ClearVox, adds a Start Menu shortcut and starts it.
#
# Windows asks for administrator approval once, only for the audio driver.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Repo = if ($env:CLEARVOX_REPO) { $env:CLEARVOX_REPO } else { 'Cosmos-0118/ClearVox' }
$InstallDir = Join-Path $env:LOCALAPPDATA 'ClearVox'
$StartMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\ClearVox.lnk'

$CableUrl = 'https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack46.zip'
$CableSha256 = 'BC44E8974191A5A2427D6C979E2DAB08DBB09925F3F435DFA2F0BA3E45B6EB90'

function Step($m) { Write-Host "`n==> $m" -ForegroundColor White }
function Ok($m)   { Write-Host "[ok] $m" -ForegroundColor Green }
function Warn($m) { Write-Host "[!] $m" -ForegroundColor Yellow }
function Die($m)  { Write-Host "`n[x] $m" -ForegroundColor Red; throw $m }

Write-Host 'ClearVox installer' -ForegroundColor White
Write-Host 'Cleaner voice for Discord, WhatsApp, Zoom and games.' -ForegroundColor DarkGray

# ---------- checks ----------
if ($env:OS -ne 'Windows_NT') { Die 'ClearVox for Windows only runs on Windows.' }
if ([Environment]::OSVersion.Version.Build -lt 17763) { Die 'ClearVox needs Windows 10 (1809) or newer.' }
if (-not [Environment]::Is64BitOperatingSystem) { Die 'ClearVox needs 64-bit Windows.' }
Ok "Windows $([Environment]::OSVersion.Version)"

$Work = Join-Path ([IO.Path]::GetTempPath()) ("clearvox-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Work | Out-Null

try {
    # ---------- 1. virtual mic driver ----------
    Step 'Virtual microphone driver (VB-CABLE)'
    function Test-Cable {
        $names = @(Get-CimInstance Win32_SoundDevice -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
        return [bool]($names | Where-Object { $_ -match 'VB-Audio Virtual Cable|CABLE' })
    }
    if (Test-Cable) {
        Ok 'VB-CABLE is already installed.'
    } else {
        $zip = Join-Path $Work 'vbcable.zip'
        Write-Host 'Downloading VB-CABLE from vb-audio.com ...'
        Invoke-WebRequest -Uri $CableUrl -OutFile $zip -UseBasicParsing
        $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
        if ($hash -ne $CableSha256) { Die "VB-CABLE download failed its checksum ($hash). Not installing it." }
        Ok 'Checksum verified.'
        $dir = Join-Path $Work 'vbcable'
        Expand-Archive $zip $dir -Force
        Write-Host 'Windows will now ask for administrator approval to install the audio driver.'
        $p = Start-Process (Join-Path $dir 'VBCABLE_Setup_x64.exe') -ArgumentList '-i', '-h' -Verb RunAs -Wait -PassThru -WorkingDirectory $dir
        Start-Sleep -Seconds 3
        if (-not (Test-Cable)) {
            Warn 'The driver is not visible yet. If Windows asks you to restart, do that, then run this installer again.'
        } else {
            Ok 'VB-CABLE installed.'
        }
    }

    # ---------- 2. ClearVox ----------
    Step 'ClearVox'
    $stage = Join-Path $Work 'app'
    New-Item -ItemType Directory -Path $stage | Out-Null
    $localExe = $null
    $here = if ($PSScriptRoot) { $PSScriptRoot } else { $null }

    if ($env:CLEARVOX_FROM_SOURCE -eq '1') {
        if (-not $here -or -not (Test-Path (Join-Path $here 'windows\Cargo.toml'))) { Die 'CLEARVOX_FROM_SOURCE=1 needs to run from a checkout of the repository.' }
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Die 'Building from source needs Rust: https://rustup.rs' }
        Write-Host 'Building from source (this takes a few minutes the first time) ...'
        Push-Location (Join-Path $here 'windows')
        try { cargo build --release; if ($LASTEXITCODE -ne 0) { Die 'Build failed.' } } finally { Pop-Location }
        $localExe = Join-Path $here 'windows\target\release\clearvox.exe'
    } else {
        $base = "https://github.com/$Repo/releases/latest/download"
        $zip = Join-Path $Work 'ClearVox-windows.zip'
        try {
            Invoke-WebRequest -Uri "$base/ClearVox-windows.zip" -OutFile $zip -UseBasicParsing
            $expected = ((Invoke-WebRequest -Uri "$base/ClearVox-windows.zip.sha256" -UseBasicParsing).Content -split '\s+')[0].Trim().ToUpper()
            $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToUpper()
            if ($expected -and $expected -ne $actual) { Die 'The ClearVox download failed its checksum.' }
            Expand-Archive $zip $stage -Force
            $localExe = Join-Path $stage 'clearvox.exe'
        } catch {
            Die "Couldn't download a ClearVox release ($($_.Exception.Message)). If none is published yet, run from a checkout with CLEARVOX_FROM_SOURCE=1."
        }
    }
    if (-not (Test-Path $localExe)) { Die 'clearvox.exe was not found after download/build.' }

    # ---------- 3. install ----------
    Step 'Installing'
    Get-Process clearvox -ErrorAction SilentlyContinue | ForEach-Object {
        Write-Host 'Closing the running ClearVox ...'
        $_.CloseMainWindow() | Out-Null
        if (-not $_.WaitForExit(2000)) { Stop-Process -Id $_.Id -Force }
    }
    New-Item -ItemType Directory -Force $InstallDir | Out-Null
    Copy-Item $localExe (Join-Path $InstallDir 'clearvox.exe') -Force
    Ok "Installed to $InstallDir"

    $shell = New-Object -ComObject WScript.Shell
    New-Item -ItemType Directory -Force (Split-Path $StartMenu) | Out-Null
    $lnk = $shell.CreateShortcut($StartMenu)
    $lnk.TargetPath = Join-Path $InstallDir 'clearvox.exe'
    $lnk.WorkingDirectory = $InstallDir
    $lnk.Description = 'ClearVox - clean voice for chat apps'
    $lnk.Save()
    Ok 'Start Menu shortcut created.'

    Start-Process (Join-Path $InstallDir 'clearvox.exe')
    Write-Host ''
    Write-Host 'ClearVox is running in the system tray (bottom-right, maybe under the ^ arrow).' -ForegroundColor Green
    Write-Host 'In Discord / WhatsApp / Zoom, choose "CABLE Output (VB-Audio Virtual Cable)" as the microphone.'
    Write-Host 'Never set an app''s speaker/output to "CABLE Input".' -ForegroundColor Yellow
}
finally {
    Remove-Item $Work -Recurse -Force -ErrorAction SilentlyContinue
}
