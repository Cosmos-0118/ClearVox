# ClearVox

**Studio-clean voice for Discord, WhatsApp, Zoom and games — on any Mac, and on Windows.**

> 🪟 **On Windows?** Jump to [ClearVox for Windows](#clearvox-for-windows).

ClearVox runs your microphone through Apple's own voice processing (the same noise and echo
removal FaceTime uses) and hands the cleaned-up result to every app as a virtual microphone.
It lives in your menu bar and needs no configuration beyond picking a mic in your chat app.

- 🎙️ **Apple Voice Isolation / Voice Processing**: removes keyboard clatter, fans and room noise.
- 🔁 **Works everywhere**: any app that lets you choose a microphone can use it.
- 🎚️ **Extras**: a 90 Hz rumble filter, a peak limiter that stops clipping, and auto gain.
- 🪶 **Native and lightweight**: a real macOS menu, no background services, no account.

> Requires macOS 13 Ventura or newer, on Apple Silicon or Intel.

---

## Install (one click)

### Option A: Terminal one-liner (recommended)

Open **Terminal** (press ⌘ Space, type *Terminal*, press Return), paste this line and press Return:

```bash
curl -fsSL https://raw.githubusercontent.com/Cosmos-0118/ClearVox/main/install.sh | bash
```

### Option B: Double-click

1. On this page, click **Code → Download ZIP** and open the downloaded file.
2. **Right-click** `Install ClearVox.command` and choose **Open**, then click **Open** again.
   (Right-clicking is only needed the first time, because the file comes from the internet.)

### What the installer does

1. Installs **BlackHole 2ch**, the free, open-source virtual audio driver ClearVox sends your
   cleaned voice into. It's downloaded from its official site and checksum-verified.
2. Downloads the latest ClearVox release, or builds it from source if none is published yet.
   Building may prompt you to install Apple's Command Line Tools.
3. Installs `ClearVox.app` into **Applications** and starts it.

You'll be asked for your Mac password once, to install the audio driver. Nothing appears while
you type it; that's normal.

---

## Set up your apps

After installing, click **OK** when macOS asks for microphone access. Then, in each app:

| App | Microphone / Input | Speaker / Output |
| --- | --- | --- |
| Discord | **BlackHole 2ch** | your speakers or headphones |
| WhatsApp | **BlackHole 2ch** | your speakers or headphones |
| Zoom / Meet / Teams | **BlackHole 2ch** | your speakers or headphones |
| Games | **BlackHole 2ch** (or "Default" with *Use as Default Mic* on) | your speakers or headphones |

> ⚠️ **Never set an app's speaker/output to BlackHole.** That sends the other person's voice back
> into your mic.

**Discord:** also turn off *Noise Suppression* and *Automatic Input Sensitivity* in
*Voice & Video* settings. ClearVox already does that work, and stacking both makes you sound robotic.

---

## Using ClearVox

Click the waveform icon in the menu bar.

| Setting | What it does |
| --- | --- |
| **On/Off switch** | Starts or stops processing. The menu bar icon shows a crossed-out mic when off. |
| **Microphone** | The real mic to clean up. |
| **Virtual Mic** | Where cleaned audio goes. Leave it on BlackHole 2ch. |
| **Voice Isolation** | Apple's strongest noise removal. Can act as your Mac's default mic, so apps pick it up automatically. Best with **headphones**. |
| **Voice Processing** | Noise removal **plus echo cancellation**. Use this on **laptop speakers**. Apps must select *BlackHole 2ch* explicitly. |
| **Mic Mode…** | Opens macOS's Mic Mode picker. Choose *Voice Isolation* there for the strongest effect. |
| **Use as Default Mic** | (Voice Isolation only) Makes ClearVox the Mac-wide default input while running. Your real mic is restored when you quit. |
| **Reduce Air Thumps** | Removes low rumble and breath pops below 90 Hz. |
| **Peak Limiter** | Stops sudden loud sounds from distorting. |
| **Auto Gain** | (Voice Processing only) Apple's automatic level control. |
| **Open at Login** | Starts ClearVox when you log in. |

---

## Troubleshooting

**The other person hears a high-pitched whine or howl.**
That's acoustic feedback: your speakers are playing into your mic. Check that no app has its
*output/speaker* set to BlackHole, then either wear headphones or switch ClearVox to
**Voice Processing**, which cancels echo. ClearVox also stops itself automatically if its audio
ever gets routed to real speakers.

**I connected AirPods / a headset. Does it still work?**
Yes. ClearVox keeps using the mic picked in its menu. If macOS switches the system input to the
headset's mic, ClearVox switches it back. To use the headset's own mic, pick it under
**Microphone**. Bluetooth headset mics lower the headset's sound quality while in use, so the
Mac's built-in mic often sounds better.

**"BlackHole isn't installed" in the menu.**
Run the installer again. If it still doesn't appear, restart your Mac.

**Nobody can hear me.**
- Make sure ClearVox is **On**, and the level meter moves when you speak.
- In the app, the microphone must be **BlackHole 2ch**.
- Check *System Settings → Privacy & Security → Microphone* and allow **ClearVox**.

**"ClearVox can't be opened because it is from an unidentified developer."**
Right-click ClearVox in Applications, choose **Open**, then **Open** again. The installer normally
prevents this.

**My Mac's default mic is still BlackHole after quitting.**
Choose your real mic in *System Settings → Sound → Input*. ClearVox restores it on a normal quit,
but not if it was force-quit.

---

## Uninstall

Run this in Terminal, or double-click `Uninstall ClearVox.command`:

```bash
curl -fsSL https://raw.githubusercontent.com/Cosmos-0118/ClearVox/main/uninstall.sh | bash
```

This removes the app, its settings and its microphone permission. BlackHole is kept because other
apps may use it; to remove it as well, download `uninstall.sh` and run `bash uninstall.sh --blackhole`.

---

## For developers

### Build from source

Requires Xcode 15.3+ or the Command Line Tools (Swift 5.9+).

```bash
cd mac
./build.sh                 # native build  -> mac/build/ClearVox.app
UNIVERSAL=1 ./build.sh     # arm64 + x86_64
open build/ClearVox.app
```

Run `CLEARVOX_FROM_SOURCE=1 ./install.sh` to install your local build with the full setup.

Diagnostics: `mac/build/ClearVox.app/Contents/MacOS/ClearVox --selftest` runs the pipeline for
8 seconds and prints levels, devices and ring-buffer underruns/overflows.

### How it works

```
Mic ─► Apple voice processing ─► high-pass ─► limiter ─► BlackHole 2ch ─► Discord / WhatsApp / …
       Voice Isolation: AVCaptureSession         AVAudioEngine            (virtual mic)
       Voice Processing: VPIO (AVAudioEngine)
```

| File | Role |
| --- | --- |
| `mac/Sources/ClearVox/ClearVoxApp.swift` | Menu bar item and native `NSMenu` UI |
| `mac/Sources/ClearVox/Pipeline.swift` | Audio engines, device routing, feedback guard, settings |
| `mac/Sources/ClearVox/MicCapture.swift` | Voice Isolation capture of a specific mic |
| `mac/Sources/ClearVox/RingBuffer.swift` | Bridges capture and playback clocks |
| `mac/Sources/ClearVox/AudioDevices.swift` | CoreAudio device listing and default-device control |
| `install.sh` / `uninstall.sh` | One-click installer and uninstaller |

### Releasing

Push a version tag. GitHub Actions builds a universal, ad-hoc-signed `ClearVox.zip` and attaches
it to a release, which the installer then downloads, so users don't need Xcode.

```bash
git tag v1.0.0 && git push origin v1.0.0
```

### Windows

See [`windows/README.md`](windows/README.md). Rust, WASAPI, DPDFNet (with RNNoise as the light fallback).

---

## ClearVox for Windows

Windows 10 / 11 (64-bit). Same idea as on Mac: your mic is cleaned up and handed to every app as a
virtual microphone, using the free [VB-CABLE](https://vb-audio.com/Cable/) driver. Open **PowerShell**
and run:

```powershell
irm https://raw.githubusercontent.com/Cosmos-0118/ClearVox/main/install.ps1 | iex
```

Then pick **CABLE Output (VB-Audio Virtual Cable)** as the microphone in Discord, WhatsApp, Zoom and
games. ClearVox sits in the system tray. Full guide, troubleshooting and uninstall:
[`windows/README.md`](windows/README.md).

---

## Credits

- [BlackHole](https://github.com/ExistentialAudio/BlackHole) by Existential Audio, the virtual
  audio driver (GPL-3.0). It's installed separately and not bundled with ClearVox.
- Apple's voice processing frameworks (AVFoundation / Core Audio).
