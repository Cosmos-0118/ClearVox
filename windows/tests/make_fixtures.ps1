# Builds the test audio used by `cargo test` (tests/denoise.rs) into windows/target/fixtures.
# The test mixes each noise into clean.wav at a fixed SNR itself:
#   clean.wav        speech (Windows TTS) with natural pauses, 2 s of silence before and after
#   noise_*.wav      simulated noises, same length
# Needs: Windows PowerShell/pwsh with System.Speech (built in) and ffmpeg on PATH.
param([string]$Out = "$PSScriptRoot\..\target\fixtures")

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) { throw 'ffmpeg not found on PATH' }

# 1. Speech via Windows TTS (16-bit 48 kHz mono so no resampling is needed later).
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$synth.Rate = 0
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(48000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$raw = Join-Path $Out 'tts.wav'
$synth.SetOutputToWaveFile($raw, $fmt)
$synth.Speak('Hello, this is a test of the noise suppression. Can you hear me clearly? The quick brown fox jumps over the lazy dog. Please keep your voice steady while the fan is running in the background.')
$synth.Dispose()

# 2. Pad with 2 s of silence on both sides -> clean.wav
& ffmpeg -y -loglevel error -i $raw -af 'adelay=2000:all=1,apad=pad_dur=2' -ar 48000 -ac 1 -c:a pcm_s16le (Join-Path $Out 'clean.wav')
$dur = [double](& ffprobe -v error -show_entries format=duration -of csv=p=0 (Join-Path $Out 'clean.wav'))

# 3. Noises, all exactly as long as clean.wav.
function Make-Noise($name, $filter) {
    & ffmpeg -y -loglevel error -f lavfi -i $filter -t $dur -ar 48000 -ac 1 -c:a pcm_s16le (Join-Path $Out "noise_$name.wav")
}
Make-Noise 'pink'  "anoisesrc=color=pink:amplitude=0.5:sample_rate=48000"
Make-Noise 'white' "anoisesrc=color=white:amplitude=0.3:sample_rate=48000"
# Fan-like: low-passed brown noise plus a 120 Hz motor hum.
Make-Noise 'fan'   "anoisesrc=color=brown:amplitude=0.6:sample_rate=48000,lowpass=f=900[a];sine=f=120:sample_rate=48000,volume=0.15[b];[a][b]amix=inputs=2:normalize=0"
# Mains hum 50 Hz + harmonics.
Make-Noise 'hum'   "sine=f=50:sample_rate=48000,volume=0.25[a];sine=f=150:sample_rate=48000,volume=0.12[b];sine=f=250:sample_rate=48000,volume=0.06[c];[a][b][c]amix=inputs=3:normalize=0"
# Keyboard-like clicks: short bursts of filtered noise at ~6 per second.
Make-Noise 'clicks' "anoisesrc=color=white:amplitude=0.9:sample_rate=48000,highpass=f=1500,volume='if(lt(mod(t,0.17),0.012),1,0)':eval=frame"

Write-Host "Fixtures written to $Out"
