# Records your microphone (raw, no Sordino) and runs the recording through every engine so the
# results can be compared. Run it from PowerShell:   ./tests/record_ab.ps1
#
# It asks you to stay quiet for 4 s (room/fan noise), speak normally for 12 s, then speak loudly
# for 6 s. Output goes to %USERPROFILE%\Documents\Sordino-AB.
param(
    [string]$Mic = '',          # part of the microphone name; default: the first non-CABLE capture device
    [string]$Exe = ''
)
$ErrorActionPreference = 'Stop'
$out = Join-Path $env:USERPROFILE 'Documents\Sordino-AB'
New-Item -ItemType Directory -Force $out | Out-Null
if (-not $Exe) {
    $Exe = Join-Path $env:LOCALAPPDATA 'Sordino\sordino.exe'
    $dev = Join-Path $PSScriptRoot '..\target\release\sordino.exe'
    if (Test-Path $dev) { $Exe = $dev }
}
if (-not (Test-Path $Exe)) { throw "sordino.exe not found: $Exe" }
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) { throw 'ffmpeg is needed on PATH for recording.' }

# Pick the microphone.
$names = (& ffmpeg -hide_banner -list_devices true -f dshow -i dummy 2>&1 | Select-String '"(.+)" \(audio\)' | ForEach-Object { $_.Matches[0].Groups[1].Value })
$names = @($names | Where-Object { $_ -notmatch 'CABLE' })
if ($Mic) { $names = @($names | Where-Object { $_ -like "*$Mic*" }) }
if ($names.Count -eq 0) { throw 'No microphone found.' }
$micName = $names[0]
Write-Host "Recording from: $micName" -ForegroundColor Cyan

if (Get-Process sordino -ErrorAction SilentlyContinue) { Write-Host 'Note: Sordino is running; that is fine, this recording is the raw microphone.' -ForegroundColor DarkGray }

$raw = Join-Path $out 'raw.wav'
$ffArgs = "-y -hide_banner -loglevel error -f dshow -i `"audio=$micName`" -t 22 -ar 48000 -ac 1 `"$raw`""
$p = Start-Process ffmpeg -ArgumentList $ffArgs -PassThru -WindowStyle Hidden
function Beep($n, $hz = 880) { 1..$n | ForEach-Object { [Console]::Beep($hz, 200); Start-Sleep -Milliseconds 120 } }
function Phase($title, $color, $seconds, $beeps) {
    Clear-Host
    Beep $beeps
    for ($i = $seconds; $i -ge 1; $i--) {
        Clear-Host
        Write-Host ''
        Write-Host "   $title" -ForegroundColor $color
        Write-Host ''
        Write-Host "   $i seconds left" -ForegroundColor $color
        Start-Sleep 1
    }
}
Start-Sleep 1
Phase 'STAY QUIET (fan running)  - 1 beep' Yellow 4 1
Phase 'SPEAK NORMALLY, like on a call  - 2 beeps' Green 12 2
Phase 'SPEAK LOUDLY  - 3 beeps' Magenta 6 3
Clear-Host
[Console]::Beep(440, 600)
$p.WaitForExit(5000) | Out-Null
Write-Host 'Done recording. Processing...' -ForegroundColor Cyan

foreach ($e in 'rnnoise', 'dpdfnet2', 'dpdfnet8') {
    & $Exe --process-file $raw (Join-Path $out "$e.wav") --engine $e --agc | Out-Null
}
Write-Host "`nFiles in $out :" -ForegroundColor Green
Get-ChildItem $out -Filter *.wav | Select-Object Name, Length | Format-Table -AutoSize | Out-String | Write-Host
Write-Host 'Done. Tell Claude. You can close this window.' -ForegroundColor Green
