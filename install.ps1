# Sordino one-click installer for Windows.
#
#   irm https://raw.githubusercontent.com/Cosmos-0118/Sordino/main/install.ps1 | iex
#
# What it does:
#   1. Installs the VB-CABLE virtual audio driver (the "virtual mic"), if missing.
#      It is downloaded from vb-audio.com and checked against a pinned SHA-256.
#   2. Downloads the latest Sordino release (or builds from source with SORDINO_FROM_SOURCE=1).
#   3. Lets you choose which DPDFNet voice models to download (checked against SHA-256) into
#      %LOCALAPPDATA%\Sordino\models. Set SORDINO_MODELS=dpdfnet8,dpdfnet2 | all | none to skip the prompt.
#   4. Installs Sordino into %LOCALAPPDATA%\Sordino, adds a Start Menu shortcut and starts it.
#
# Windows asks for administrator approval once, only for the audio driver.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Repo = if ($env:SORDINO_REPO) { $env:SORDINO_REPO } else { 'Cosmos-0118/Sordino' }
$InstallDir = Join-Path $env:LOCALAPPDATA 'Sordino'
$StartMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Sordino.lnk'

$CableUrl = 'https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack46.zip'
$CableSha256 = 'BC44E8974191A5A2427D6C979E2DAB08DBB09925F3F435DFA2F0BA3E45B6EB90'

# Voice models are published once under their own release (models.yml), not with every app release.
# Bump this when new or changed models are published (models-v2, ...).
$ModelsTag = 'models-v1'

function Step($m) { Write-Host "`n==> $m" -ForegroundColor White }
function Ok($m)   { Write-Host "[ok] $m" -ForegroundColor Green }
function Warn($m) { Write-Host "[!] $m" -ForegroundColor Yellow }
function Die($m)  { Write-Host "`n[x] $m" -ForegroundColor Red; throw $m }
# Published .sha256 files are served as application/octet-stream, so Invoke-WebRequest returns .Content as
# bytes (PowerShell 7) or text (5.1). Normalize to the hex digest.
function Get-ExpectedSha256($url) {
    $c = (Invoke-WebRequest -Uri $url -UseBasicParsing).Content
    if ($c -is [byte[]]) { $c = [Text.Encoding]::UTF8.GetString($c) }
    return (([string]$c).TrimStart([char]0xFEFF) -split '\s+' | Where-Object { $_ } | Select-Object -First 1).Trim().ToUpper()
}

Write-Host 'Sordino installer' -ForegroundColor White
Write-Host 'Cleaner voice for Discord, WhatsApp, Zoom and games.' -ForegroundColor DarkGray

# ---------- checks ----------
if ($env:OS -ne 'Windows_NT') { Die 'Sordino for Windows only runs on Windows.' }
if ([Environment]::OSVersion.Version.Build -lt 17763) { Die 'Sordino needs Windows 10 (1809) or newer.' }
if (-not [Environment]::Is64BitOperatingSystem) { Die 'Sordino needs 64-bit Windows.' }
Ok "Windows $([Environment]::OSVersion.Version)"

$Work = Join-Path ([IO.Path]::GetTempPath()) ("sordino-install-" + [Guid]::NewGuid().ToString('N'))
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

    # ---------- 2. Sordino ----------
    Step 'Sordino'
    $stage = Join-Path $Work 'app'
    New-Item -ItemType Directory -Path $stage | Out-Null
    $localExe = $null
    $here = if ($PSScriptRoot) { $PSScriptRoot } else { $null }

    if ($env:SORDINO_FROM_SOURCE -eq '1') {
        if (-not $here -or -not (Test-Path (Join-Path $here 'windows\Cargo.toml'))) { Die 'SORDINO_FROM_SOURCE=1 needs to run from a checkout of the repository.' }
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Die 'Building from source needs Rust: https://rustup.rs' }
        Write-Host 'Building from source (this takes a few minutes the first time) ...'
        Push-Location (Join-Path $here 'windows')
        try { cargo build --release; if ($LASTEXITCODE -ne 0) { Die 'Build failed.' } } finally { Pop-Location }
        $localExe = Join-Path $here 'windows\target\release\sordino.exe'
    } else {
        $base = "https://github.com/$Repo/releases/latest/download"
        $zip = Join-Path $Work 'Sordino-windows.zip'
        try {
            Invoke-WebRequest -Uri "$base/Sordino-windows.zip" -OutFile $zip -UseBasicParsing
            $expected = Get-ExpectedSha256 "$base/Sordino-windows.zip.sha256"
            $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToUpper()
            if ($expected -and $expected -ne $actual) { Die 'The Sordino download failed its checksum.' }
            Expand-Archive $zip $stage -Force
            $localExe = Join-Path $stage 'sordino.exe'
        } catch {
            Die "Couldn't download a Sordino release ($($_.Exception.Message)). If none is published yet, run from a checkout with SORDINO_FROM_SOURCE=1."
        }
    }
    if (-not (Test-Path $localExe)) { Die 'sordino.exe was not found after download/build.' }

    # ---------- 2b. voice models ----------
    # RNNoise is built into sordino.exe. The DPDFNet models are separate files you choose here;
    # they go to %LOCALAPPDATA%\Sordino\models and the tray menu lists the ones that are installed.
    Step 'Voice models'
    $Catalog = @(
        @{ Id = 'dpdfnet8'; File = 'dpdfnet8_48khz_hr.onnx'; Name = 'DPDFNet-8'; Note = 'best quality, about half a CPU core'; MB = 14 },
        @{ Id = 'dpdfnet2'; File = 'dpdfnet2_48khz_hr.onnx'; Name = 'DPDFNet-2'; Note = 'lighter on CPU, slightly less clean'; MB = 10 }
    )
    $ModelDir = Join-Path $InstallDir 'models'
    $modelsBase = "https://github.com/$Repo/releases/download/$ModelsTag"
    $have = @($Catalog | Where-Object { Test-Path (Join-Path $ModelDir $_.File) } | ForEach-Object { $_.Id })

    # Selection: SORDINO_MODELS="dpdfnet8,dpdfnet2" | "all" | "none" skips the prompt (for scripts/CI).
    $choice = $null
    if ($env:SORDINO_MODELS) {
        $choice = $env:SORDINO_MODELS.Trim().ToLower()
    } elseif ([Environment]::UserInteractive -and -not [Console]::IsInputRedirected) {
        Write-Host 'Choose which voice models to install (RNNoise, a small basic one, is always included):'
        for ($i = 0; $i -lt $Catalog.Count; $i++) {
            $m = $Catalog[$i]
            $tag = if ($have -contains $m.Id) { ' [installed]' } else { '' }
            Write-Host ("  {0}) {1,-10} {2} (~{3} MB){4}" -f ($i + 1), $m.Name, $m.Note, $m.MB, $tag)
        }
        Write-Host '  a) All of the above     n) None, use RNNoise only'
        $choice = (Read-Host 'Numbers separated by commas, or a / n  [Enter = all]').Trim().ToLower()
    }
    if (-not $choice) { $choice = 'all' }

    $picked = @()
    if ($choice -eq 'all' -or $choice -eq 'a') {
        $picked = $Catalog
    } elseif ($choice -ne 'none' -and $choice -ne 'n') {
        foreach ($t in ($choice -split '[,\s]+' | Where-Object { $_ })) {
            $m = $null
            for ($i = 0; $i -lt $Catalog.Count; $i++) {
                if ($Catalog[$i].Id -eq $t -or "$($i + 1)" -eq $t) { $m = $Catalog[$i] }
            }
            if ($m -and $picked -notcontains $m) { $picked += $m } elseif (-not $m) { Warn "Unknown model '$t' ignored." }
        }
    }

    $stageModels = Join-Path $stage 'models'
    New-Item -ItemType Directory -Force $stageModels | Out-Null
    foreach ($m in $picked) {
        $dest = Join-Path $stageModels $m.File
        try {
            if ($env:SORDINO_FROM_SOURCE -eq '1') {
                Copy-Item (Join-Path $here ('windows\models\' + $m.File)) $dest -Force
            } else {
                Write-Host "Downloading $($m.Name) (~$($m.MB) MB) ..."
                Invoke-WebRequest -Uri "$modelsBase/$($m.File)" -OutFile $dest -UseBasicParsing
                $expected = Get-ExpectedSha256 "$modelsBase/$($m.File).sha256"
                if (-not $expected) { throw 'no checksum published' }
                if ((Get-FileHash $dest -Algorithm SHA256).Hash.ToUpper() -ne $expected) { throw 'checksum mismatch' }
            }
            Ok "$($m.Name) ready."
        } catch {
            Remove-Item $dest -Force -ErrorAction SilentlyContinue
            Warn "Skipped $($m.Name): $($_.Exception.Message). Run the installer again to retry."
        }
    }
    if ($picked.Count -eq 0 -and $have.Count -eq 0) { Warn 'No DPDFNet model selected: Sordino will use RNNoise only. Re-run the installer to add more.' }

    # ---------- 3. install ----------
    Step 'Installing'
    Get-Process sordino -ErrorAction SilentlyContinue | ForEach-Object {
        Write-Host 'Closing the running Sordino ...'
        $_.CloseMainWindow() | Out-Null
        if (-not $_.WaitForExit(2000)) { Stop-Process -Id $_.Id -Force }
    }
    New-Item -ItemType Directory -Force $InstallDir | Out-Null
    Copy-Item $localExe (Join-Path $InstallDir 'sordino.exe') -Force
    $newModels = @(Get-ChildItem $stageModels -Filter *.onnx -ErrorAction SilentlyContinue)
    if ($newModels.Count -gt 0) {
        New-Item -ItemType Directory -Force $ModelDir | Out-Null
        $newModels | Copy-Item -Destination $ModelDir -Force
    }
    Ok "Installed to $InstallDir"

    $shell = New-Object -ComObject WScript.Shell
    New-Item -ItemType Directory -Force (Split-Path $StartMenu) | Out-Null
    $lnk = $shell.CreateShortcut($StartMenu)
    $lnk.TargetPath = Join-Path $InstallDir 'sordino.exe'
    $lnk.WorkingDirectory = $InstallDir
    $lnk.Description = 'Sordino - clean voice for chat apps'
    $lnk.Save()
    Ok 'Start Menu shortcut created.'

    Start-Process (Join-Path $InstallDir 'sordino.exe')
    Write-Host ''
    Write-Host 'Sordino is running in the system tray (bottom-right, maybe under the ^ arrow).' -ForegroundColor Green
    Write-Host 'In Discord / WhatsApp / Zoom, choose "CABLE Output (VB-Audio Virtual Cable)" as the microphone.'
    Write-Host 'Never set an app''s speaker/output to "CABLE Input".' -ForegroundColor Yellow
}
finally {
    Remove-Item $Work -Recurse -Force -ErrorAction SilentlyContinue
}
