# Sordino for Windows

Studio-clean voice for Discord, WhatsApp, Zoom and games on Windows 10 / 11 (64-bit).

Sordino lives in the system tray. It takes your real microphone, removes fans, keyboard clatter and
room noise with a neural network, and sends the result to **VB-CABLE**, a free virtual audio cable.
Your chat apps use the cable's *CABLE Output* as their microphone.

```
Mic ─► high-pass ─► DPDFNet (neural denoiser) ─► auto gain ─► limiter ─► CABLE Input ─► (apps pick "CABLE Output")
        WASAPI capture                                                      WASAPI render
```

## Install

Open **PowerShell** and run:

```powershell
irm https://raw.githubusercontent.com/Cosmos-0118/Sordino/main/install.ps1 | iex
```

It installs the VB-CABLE driver (Windows asks for administrator approval once; the download is
checked against a pinned SHA-256), then installs Sordino to `%LOCALAPPDATA%\Sordino`, adds a Start
Menu shortcut and starts it. If Windows asks you to restart after the driver install, restart and run
the installer again.

> The app is not code-signed, so if you download `sordino.exe` yourself Windows SmartScreen may warn
> you: click **More info → Run anyway**. The installer avoids this.

## Set up your apps

| App | Microphone / Input | Speaker / Output |
| --- | --- | --- |
| Discord, WhatsApp, Zoom, Teams, Meet | **CABLE Output (VB-Audio Virtual Cable)** | your speakers or headphones |
| Games | **CABLE Output** (or "Default" with *Use as Default Mic* on) | your speakers or headphones |

> ⚠️ **Never set an app's speaker/output to *CABLE Input*.** That sends the other person's voice back
> into your mic.

In Discord also turn off *Noise Suppression* and *Automatic Input Sensitivity*: Sordino already does
that work, and stacking both sounds robotic.

## The tray menu

Click the Sordino icon (bottom-right; it may be under the ^ arrow). The icon is green when on, grey
when off, amber on an error, and its microphone fills up as you speak.

| Item | What it does |
| --- | --- |
| **Sordino On** | Starts or stops processing. |
| **Microphone** | The real mic to clean up. |
| **Virtual Mic** | Where cleaned audio goes (the *CABLE Input* playback device). |
| **Noise Suppression** | The neural denoiser. Turn off to hear the difference. |
| **Voice Engine** | *Best quality* (DPDFNet-8, default, roughly half a CPU core), *Lighter on CPU* (DPDFNet-2) or *Minimal* (RNNoise: tiny, but it garbles speech under loud close noise such as a fan). |
| **Reduce Air Thumps** | 90 Hz high-pass: rumble, desk bumps, breath pops. |
| **Peak Limiter** | Stops sudden loud sounds clipping. |
| **Auto Gain** | Evens out your level while you talk (up to +18 dB; quiet words are lifted so they do not sink). |
| **Use as Default Mic** | Makes *CABLE Output* the Windows default recording device while Sordino runs, so apps pick it automatically. Your real default is restored when you quit. |
| **Open at Login** | Starts Sordino when you sign in. |

Settings are stored in `%APPDATA%\Sordino\settings.json`.

## Troubleshooting

**The other person hears a whine or echo.** Acoustic feedback: check no app has its *speaker* set to
*CABLE Input*, and prefer headphones. Unlike the Mac version, Sordino on Windows has no echo
cancellation, so use headphones if your speakers are loud. Sordino refuses to play to anything that
isn't a virtual cable, so it can't send your voice to your real speakers by itself.

**"VB-CABLE isn't installed" in the menu.** Run the installer again; restart Windows if asked.

**Nobody can hear me.** Sordino must be **On**, the tray mic should fill when you speak, and the app
must use **CABLE Output** as its microphone. Check *Settings → Privacy & security → Microphone* allows
desktop apps.

**I plugged in a headset and Windows switched my mic.** Sordino keeps capturing the mic chosen in its
menu and puts *CABLE Output* back as the default. To use the headset mic, pick it under **Microphone**.
Bluetooth headset mics sound worse while in use.

**My default microphone is still *CABLE Output* after a crash.** Run
`%LOCALAPPDATA%\Sordino\sordino.exe --restore-default`.

## Uninstall

```powershell
irm https://raw.githubusercontent.com/Cosmos-0118/Sordino/main/uninstall.ps1 | iex
```

Removes the app, settings and login entry and restores your default mic. VB-CABLE stays (other apps
may use it); remove it under *Settings → Apps → VB-Audio Virtual Cable*.

## For developers

Requires Rust (stable) with the MSVC toolchain (Visual Studio Build Tools, "Desktop development with
C++"). Run commands from PowerShell, not Git Bash (Git's `link.exe` shadows MSVC's).

```powershell
cd windows
cargo build --release                  # small exe; DPDFNet models are loaded from disk
# cargo build --release --features embed-models   # ~45 MB exe with the models compiled in
cargo test --release --lib             # unit tests
```

Noise-suppression regression test (simulated noisy speech at ~5 dB SNR: pink, white, fan, mains hum,
keyboard clicks). Needs `ffmpeg` on PATH:

```powershell
./tests/make_fixtures.ps1
cargo test --release --test denoise -- --nocapture
```

CLI (the exe is a tray app; when started from a shell, pipe it, e.g. `| Out-String`, to wait for it):

```powershell
sordino.exe --list-devices
sordino.exe --selftest [--mic TEXT] [--file in.wav] [--seconds 8] [--engine dpdfnet2]
sordino.exe --process-file in.wav out.wav [--engine rnnoise|dpdfnet2|dpdfnet8] [--agc]
sordino.exe --restore-default
```

`--selftest` prints peak level and ring-buffer under/overruns each second. Record **CABLE Output**
(`ffmpeg -f dshow -i "audio=CABLE Output (VB-Audio Virtual Cable)" out.wav`) while it runs to verify
the whole path. `tests/analyze.py` compares loudness before/after for any pair of WAVs.

| File | Role |
| --- | --- |
| `src/dsp.rs` | High-pass, engine selection, gate (RNNoise only), auto gain, limiter |
| `src/dpdfnet.rs` | DPDFNet streaming inference (STFT + `tract` ONNX) |
| `models/` | The DPDFNet ONNX models (published as release assets, installed by `install.ps1`) and their licence |
| `tests/bench/` | Python scripts that compared candidate denoisers |
| `src/pipeline.rs` | WASAPI capture → DSP → WASAPI render, feedback guard |
| `src/ring.rs` | Bridges the mic and cable clocks |
| `src/devices.rs` | Endpoint listing, "is this a virtual cable" rules |
| `src/defaultmic.rs` | Default recording device switching (`IPolicyConfig`) |
| `src/app.rs`, `src/icon.rs` | Tray menu, supervision/restart, tray icon |
| `src/settings.rs` | JSON settings, launch at login |

## Credits

- [VB-CABLE](https://vb-audio.com/Cable/) by VB-Audio, the virtual audio cable (donationware,
  installed separately, not bundled).
- [RNNoise](https://github.com/xiph/rnnoise) via [nnnoiseless](https://github.com/jneem/nnnoiseless) (BSD-3-Clause), the light fallback engine.
- [DPDFNet](https://github.com/ceva-ip/DPDFNet) by Ceva (Apache-2.0), a DeepFilterNet2-based speech enhancer, run with [tract](https://github.com/sonos/tract).
