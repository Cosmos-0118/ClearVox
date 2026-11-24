//! Voice DSP chain: high-pass -> DPDFNet / RNNoise -> gate -> auto gain -> peak limiter.
//!
//! Everything runs on 48 kHz mono `f32` audio in [-1, 1] in blocks of [`FRAME`] (10 ms) samples,
//! which is what RNNoise expects. The chain is stateful and streaming: feed it any number of
//! samples and it returns whole frames as they become available.

use crate::dpdfnet::{Dpdfnet, Size};
use nnnoiseless::DenoiseState;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME: usize = DenoiseState::FRAME_SIZE; // 480 samples = 10 ms

/// Which neural denoiser runs inside the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// RNNoise: tiny and fast, but it garbles speech under loud, close noise such as a fan.
    Rnnoise,
    /// DPDFNet-2: clean voice, light on the CPU.
    Dpdfnet2,
    /// DPDFNet-8: smoother, fewer artifacts, about twice the CPU of DPDFNet-2. The default.
    Dpdfnet8,
}

impl Engine {
    pub fn from_name(s: &str) -> Option<Engine> {
        match s {
            "rnnoise" | "light" => Some(Engine::Rnnoise),
            "dpdfnet2" | "balanced" | "studio" => Some(Engine::Dpdfnet2),
            "dpdfnet8" | "best" => Some(Engine::Dpdfnet8),
            _ => None,
        }
    }

    /// True when the model this engine needs is installed (RNNoise is built in).
    pub fn is_available(self) -> bool {
        match self {
            Engine::Rnnoise => true,
            Engine::Dpdfnet2 => Size::Two.is_installed(),
            Engine::Dpdfnet8 => Size::Eight.is_installed(),
        }
    }

    /// This engine if its model is installed, otherwise the best engine that is
    /// (DPDFNet-8, then DPDFNet-2, then RNNoise).
    pub fn or_best_available(self) -> Engine {
        if self.is_available() {
            return self;
        }
        [Engine::Dpdfnet8, Engine::Dpdfnet2].into_iter().find(|e| e.is_available()).unwrap_or(Engine::Rnnoise)
    }
}

enum Denoiser {
    Rnn {
        st: Box<DenoiseState<'static>>,
        bp: Biquad,
    },
    Dpd {
        model: Box<Dpdfnet>,
        /// Lightweight RNNoise instance used strictly as a sidechain neural VAD on
        /// level-normalized DPDFNet output (< 1% CPU, 0 ms latency, zero extra DLLs).
        side_vad: Box<DenoiseState<'static>>,
        bp: Biquad,
    },
}

struct VadFeatures {
    /// Neural speech probability in 0..1 (RNNoise native or sidechain).
    prob: f32,
    /// Post-denoiser frame RMS.
    rms: f32,
    /// Fraction of frame energy in the vocal / whisper formant band (~300–4500 Hz).
    voice_band_ratio: f32,
}

impl Denoiser {
    fn new(engine: Engine) -> Self {
        let bp = Biquad::bandpass(1200.0, 0.45);
        let size = match engine {
            Engine::Rnnoise => return Denoiser::Rnn { st: DenoiseState::new(), bp },
            Engine::Dpdfnet2 => Size::Two,
            Engine::Dpdfnet8 => Size::Eight,
        };
        match Dpdfnet::new(size) {
            Ok(m) => Denoiser::Dpd { model: Box::new(m), side_vad: DenoiseState::new(), bp },
            Err(e) => {
                log::error!("DPDFNet failed to load ({e}); falling back to RNNoise");
                Denoiser::Rnn { st: DenoiseState::new(), bp }
            }
        }
    }

    /// `input`/`output` are in [-1, 1]. Returns neural VAD probability + spectral cues.
    fn run(&mut self, input: &[f32; FRAME], output: &mut [f32; FRAME]) -> VadFeatures {
        match self {
            Denoiser::Rnn { st, bp } => {
                let mut scaled = [0.0f32; FRAME];
                for i in 0..FRAME {
                    scaled[i] = input[i] * 32768.0;
                }
                let prob = st.process_frame(output, &scaled);
                let mut e_tot = 0.0f32;
                let mut e_band = 0.0f32;
                for s in output.iter_mut() {
                    *s /= 32768.0;
                    e_tot += *s * *s;
                    let b = bp.run(*s);
                    e_band += b * b;
                }
                let rms = (e_tot / FRAME as f32).sqrt();
                let voice_band_ratio = e_band / (e_tot + 1e-12);
                VadFeatures { prob, rms, voice_band_ratio }
            }
            Denoiser::Dpd { model, side_vad, bp } => {
                if model.process(input, output).is_err() {
                    output.copy_from_slice(input);
                    let rms = frame_rms(output);
                    return VadFeatures { prob: 0.5, rms, voice_band_ratio: 0.5 };
                }
                let mut e_tot = 0.0f32;
                let mut e_band = 0.0f32;
                for &s in output.iter() {
                    e_tot += s * s;
                    let b = bp.run(s);
                    e_band += b * b;
                }
                let rms = (e_tot / FRAME as f32).sqrt();
                let voice_band_ratio = e_band / (e_tot + 1e-12);
                // Normalize the cleaned frame into RNNoise's sweet spot (~-22 dBFS, max +36 dB)
                // so soft voices and whispers get an accurate neural VAD score, while true
                // digital silence (< -76 dBFS) stays at 0 without amplifying float residue.
                let mut side_in = [0.0f32; FRAME];
                let mut side_out = [0.0f32; FRAME];
                let prob = if rms < 1.5e-4 {
                    let _ = side_vad.process_frame(&mut side_out, &side_in);
                    0.0
                } else {
                    let gain = (0.08 / rms).clamp(1.0, 64.0) * 32768.0;
                    for i in 0..FRAME {
                        side_in[i] = (output[i] * gain).clamp(-32767.0, 32767.0);
                    }
                    side_vad.process_frame(&mut side_out, &side_in)
                };
                VadFeatures { prob, rms, voice_band_ratio }
            }
        }
    }

    fn is_dpdfnet(&self) -> bool {
        matches!(self, Denoiser::Dpd { .. })
    }
}

#[inline]
fn frame_rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[inline]
fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1e-7).log10()
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Live-adjustable switches, shared between the UI and the audio thread.
pub struct Params {
    pub denoise: AtomicBool,
    pub highpass: AtomicBool,
    pub limiter: AtomicBool,
    pub agc: AtomicBool,
    pub gate: AtomicBool,
}

impl Params {
    pub fn new(denoise: bool, highpass: bool, limiter: bool, agc: bool) -> Arc<Self> {
        Arc::new(Self {
            denoise: AtomicBool::new(denoise),
            highpass: AtomicBool::new(highpass),
            limiter: AtomicBool::new(limiter),
            agc: AtomicBool::new(agc),
            gate: AtomicBool::new(true),
        })
    }
}

/// 2nd-order RBJ biquad filter (transposed direct form II).
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn highpass(freq: f32) -> Self {
        let w0 = 2.0 * PI * freq / SAMPLE_RATE as f32;
        let alpha = w0.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let cw = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 + cw) / 2.0 / a0,
            b1: -(1.0 + cw) / a0,
            b2: (1.0 + cw) / 2.0 / a0,
            a1: -2.0 * cw / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn bandpass(freq: f32, q: f32) -> Self {
        let w0 = 2.0 * PI * freq / SAMPLE_RATE as f32;
        let alpha = w0.sin() / (2.0 * q);
        let cw = w0.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b1: 0.0,
            b2: -alpha / a0,
            a1: -2.0 * cw / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn peaking_eq(freq: f32, q: f32, gain_db: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / SAMPLE_RATE as f32;
        let alpha = w0.sin() / (2.0 * q);
        let cw = w0.cos();
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cw / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cw / a0,
            a2: (1.0 - alpha / a) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Unity-compensated pre-denoiser leveler + quiet-room whisper cue.
/// Lifts quiet voices/whispers into DPDFNet's training dynamic range (up to +12 dB, bounded by
/// ambient noise floor and peak headroom) before STFT, then divides back out after DPDFNet's
/// 1-hop lag so overall gain stays exact. Also tracks the ambient floor, input vocal-band ratio,
/// and crest factor so unvoiced whispers that DPDFNet over-suppresses can be rescued cleanly
/// without leaking stationary room noise or keyboard clicks.
struct PreLeveler {
    gain: f32,
    prev_gain: f32,
    prev_prev_gain: f32,
    noise_floor: f32,
    rms_env: f32,
    in_bp: Biquad,
    whisper_streak: u32,
    rescue_blend: f32,
}

impl PreLeveler {
    const TARGET_RMS: f32 = 0.055; // -25 dBFS into denoiser
    const MAX_GAIN: f32 = 4.0; // +12 dB into denoiser
    const MAX_NOISE_OUT: f32 = 0.035; // never push ambient noise above -29 dBFS into DPDFNet
    const PEAK_CEIL: f32 = 0.70; // keep peak headroom before STFT

    fn new() -> Self {
        Self {
            gain: 1.0,
            prev_gain: 1.0,
            prev_prev_gain: 1.0,
            noise_floor: 0.002,
            rms_env: 0.05,
            in_bp: Biquad::bandpass(1800.0, 0.55), // whisper formant / intelligibility band (~600–4800 Hz)
            whisper_streak: 0,
            rescue_blend: 0.0,
        }
    }

    /// Computes pre-gain for this frame and returns `(pre_start, pre_end, inv_start, inv_end, rescue_blend)`.
    /// `inv_*` compensates the previous frame's gain (matching DPDFNet's 1-hop latency).
    fn update(&mut self, frame: &[f32], active: bool) -> (f32, f32, f32, f32, f32) {
        let pre_start = self.gain;
        let inv_start = 1.0 / self.prev_prev_gain;
        let inv_end = 1.0 / self.prev_gain;
        self.prev_prev_gain = self.prev_gain;
        self.prev_gain = self.gain;

        if !active {
            self.gain += (1.0 - self.gain) * 0.1;
            self.whisper_streak = 0;
            self.rescue_blend *= 0.7;
            return (pre_start, self.gain, inv_start, inv_end, 0.0);
        }

        let mut e_tot = 0.0f32;
        let mut e_band = 0.0f32;
        let mut peak = 0.0f32;
        for &s in frame {
            e_tot += s * s;
            let b = self.in_bp.run(s);
            e_band += b * b;
            peak = peak.max(s.abs());
        }
        let rms = (e_tot / frame.len().max(1) as f32).sqrt();
        let band_ratio = e_band / (e_tot + 1e-12);
        let crest = peak / (rms + 1e-9);

        // Asymmetric ambient floor tracker: falls quickly on quiet frames, rises very slowly.
        if rms < self.noise_floor {
            self.noise_floor += (rms - self.noise_floor) * 0.08;
        } else {
            self.noise_floor += (rms - self.noise_floor) * 0.0004;
        }
        self.noise_floor = self.noise_floor.clamp(1e-5, 0.15);

        // Active RMS envelope (fast attack, ~1.5 s release).
        if rms > self.rms_env {
            self.rms_env += (rms - self.rms_env) * 0.30;
        } else {
            self.rms_env += (rms - self.rms_env) * 0.007;
        }

        let mut want = if self.rms_env < Self::TARGET_RMS && rms > 1e-4 {
            (Self::TARGET_RMS / self.rms_env.max(rms).max(1e-4)).clamp(1.0, Self::MAX_GAIN)
        } else {
            1.0
        };
        // Don't overload DPDFNet in loud-fan rooms.
        let noise_cap = (Self::MAX_NOISE_OUT / self.noise_floor).clamp(1.0, Self::MAX_GAIN);
        want = want.min(noise_cap);
        // Immediate peak headroom safety cap.
        if peak > 1e-4 {
            let peak_cap = (Self::PEAK_CEIL / peak).clamp(1.0, Self::MAX_GAIN);
            want = want.min(peak_cap);
        }

        let k = if want < self.gain { 0.45 } else { 0.04 };
        self.gain += (want - self.gain) * k;

        // Quiet-room unvoiced whisper detector:
        // - Must sit at least +8 dB (2.5x) above ambient room floor (rejects fans/AC/hiss/hum)
        // - Quiet level (< -28 dBFS) with moderate crest factor (< 4.8, rejects sharp keystrokes)
        // - Concentrated in whisper formant band (band_ratio >= 0.50)
        // - Sustained for >= 5 frames (50 ms, rejects impulsive 10-30 ms clicks)
        let snr_ok = rms > self.noise_floor * 2.5 && rms > 4e-4 && rms < 0.04;
        let whisper_like = snr_ok && crest < 4.8 && band_ratio >= 0.50;
        if whisper_like {
            self.whisper_streak = self.whisper_streak.saturating_add(1);
        } else {
            self.whisper_streak = self.whisper_streak.saturating_sub(2);
        }
        let target_rescue = if self.whisper_streak >= 5 { 0.45 } else { 0.0 };
        let rk = if target_rescue > self.rescue_blend { 0.35 } else { 0.25 };
        self.rescue_blend += (target_rescue - self.rescue_blend) * rk;

        (pre_start, self.gain, inv_start, inv_end, self.rescue_blend)
    }
}

/// Look-ahead peak limiter: ceiling -1 dBFS, 4 ms look-ahead, ~120 ms release.
struct Limiter {
    delay: Vec<f32>,
    pos: usize,
    gain: f32,
}

const LIMIT: f32 = 0.891; // -1 dBFS
const LOOKAHEAD: usize = 192; // 4 ms

impl Limiter {
    fn new() -> Self {
        Self { delay: vec![0.0; LOOKAHEAD], pos: 0, gain: 1.0 }
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        self.delay[self.pos] = x;
        self.pos = (self.pos + 1) % LOOKAHEAD;
        // The oldest sample is at `pos`. The peak over the whole look-ahead window sets the gain.
        let mut peak = 0.0f32;
        for &s in &self.delay {
            peak = peak.max(s.abs());
        }
        let target = if peak > LIMIT { LIMIT / peak } else { 1.0 };
        if target < self.gain {
            // Reach `target` smoothly within the 4 ms look-ahead window without overshoot.
            self.gain += (target - self.gain) * 0.08;
            if self.gain < target {
                self.gain = target;
            }
        } else {
            self.gain += (target - self.gain) * (1.0 / (0.12 * SAMPLE_RATE as f32));
        }
        (self.delay[self.pos] * self.gain).clamp(-1.0, 1.0)
    }
}

/// dB-domain AGC2 speech leveler + fast upward compressor for whispers and soft syllables.
/// Adapts strictly while the neural speech gate reports active speech, and gently relaxes extreme
/// gain during silence so the next loud word doesn't clip.
struct Agc {
    gain_db: f32,
    upward_db: f32, // fast extra upward compression (0..=UPWARD_MAX_DB) for whispers / quiet endings
    power: f32,     // smoothed active-speech power
}

impl Agc {
    const TARGET_DB: f32 = -20.0; // ~0.10 RMS: clear Discord / Valorant level
    const MAX_GAIN_DB: f32 = 28.0; // enough headroom to lift -50 dBFS whispers cleanly
    const MIN_GAIN_DB: f32 = -9.0;
    const MIN_RMS: f32 = 0.00025; // -72 dBFS: safe because gated by neural VAD
    const UPWARD_KNEE_DB: f32 = -25.0;
    const UPWARD_RATIO: f32 = 0.65; // ~2.85:1 upward compression below knee
    const UPWARD_MAX_DB: f32 = 12.0;
    const TOTAL_MAX_DB: f32 = 34.0;

    fn new() -> Self {
        Self { gain_db: 0.0, upward_db: 0.0, power: 0.0025 }
    }

    /// Current effective linear gain.
    fn eff(&self) -> f32 {
        db_to_lin((self.gain_db + self.upward_db).min(Self::TOTAL_MAX_DB))
    }

    /// 0..1 blend factor for adaptive whisper presence EQ (flat on normal speech).
    fn presence_blend(&self, enabled: bool) -> f32 {
        if !enabled {
            return 0.0;
        }
        ((self.upward_db - 2.0) / 6.0).clamp(0.0, 1.0)
    }

    /// Updates gain in dB and returns the linear gain this frame starts from.
    fn update(&mut self, frame: &[f32], speech_active: bool, enabled: bool) -> f32 {
        let prev = self.eff();
        let p = frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32;
        let frame_rms = p.sqrt();

        if enabled && speech_active && frame_rms > Self::MIN_RMS {
            self.power += (p - self.power) * 0.35;
            let smoothed_db = lin_to_db(self.power.sqrt());
            let want_db = (Self::TARGET_DB - smoothed_db).clamp(Self::MIN_GAIN_DB, Self::MAX_GAIN_DB);
            let delta = want_db - self.gain_db;
            // Cut quickly on loud talkers, boost smoothly on quiet talkers / whispers.
            let step = if delta < 0.0 {
                (delta * 0.25).max(-0.6)
            } else {
                (delta * 0.08).min(0.22)
            };
            self.gain_db = (self.gain_db + step).clamp(Self::MIN_GAIN_DB, Self::MAX_GAIN_DB);

            // Fast upward compression: lifts short whispered callouts and soft word endings within ~25 ms.
            let post_macro_db = lin_to_db(frame_rms) + self.gain_db;
            let want_up = if post_macro_db < Self::UPWARD_KNEE_DB {
                ((Self::UPWARD_KNEE_DB - post_macro_db) * Self::UPWARD_RATIO).min(Self::UPWARD_MAX_DB)
            } else {
                0.0
            };
            let k = if want_up > self.upward_db { 0.35 } else { 0.12 };
            self.upward_db += (want_up - self.upward_db) * k;
        } else if !enabled {
            self.gain_db += (0.0 - self.gain_db) * 0.12;
            self.upward_db += (0.0 - self.upward_db) * 0.2;
        } else {
            // Silence / trailing hold: never boost noise; slowly relax extreme gain (> +6 dB)
            // back toward a safe baseline so a sudden loud syllable won't slam the limiter.
            if self.gain_db > 6.0 {
                self.gain_db = (self.gain_db - 0.03).max(6.0);
            }
            self.upward_db *= 0.80;
            if self.upward_db < 0.05 {
                self.upward_db = 0.0;
            }
        }
        prev
    }
}

/// Hybrid neural-VAD zero-squelch hysteresis gate for all engines (DPDFNet and RNNoise).
/// Closes cleanly down to exact digital zero (`0.0`) after speech ends so downstream game VOX
/// detectors (Valorant, Discord) never flash on residual noise, while still opening on soft
/// unvoiced whispers via sustained vocal-band persistence.
struct Gate {
    gain: f32,
    hold: u32,
    open: bool,
    voiced_streak: u32,
    whisper_streak: u32,
    noise_rms: f32,
}

impl Gate {
    const HOLD_FRAMES: u32 = 28; // 280 ms hold bridges pauses & trailing syllables
    const RELEASE_MUL: f32 = 0.84; // smooth ~150-350 ms release tail
    const ZERO_SNAP: f32 = 1e-4; // -80 dBFS -> snap to exact 0.0 digital silence

    fn new() -> Self {
        Self { gain: 0.0, hold: 0, open: false, voiced_streak: 0, whisper_streak: 0, noise_rms: 1e-5 }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.hold = Self::HOLD_FRAMES;
        self.open = true;
        self.voiced_streak = 0;
        self.whisper_streak = 0;
    }

    /// Returns `(gain_start, gain_end, speech_active)`.
    /// `speech_active` is true during active speech (not the trailing hold tail) so AGC never
    /// pumps residual room noise after the user finishes speaking.
    fn update(&mut self, f: &VadFeatures, whisper_rescued: bool) -> (f32, f32, bool) {
        let start = self.gain;

        // 1. Voiced speech trigger: high confidence opens immediately (0-frame lag); moderate
        //    confidence requires 2 consecutive frames (20 ms) to reject stray 1-frame click blips.
        let strong_voiced = f.prob >= 0.65 && f.rms > 3.5e-4;
        if f.prob >= 0.42 && f.rms > 3.5e-4 {
            self.voiced_streak = self.voiced_streak.saturating_add(1);
        } else {
            self.voiced_streak = 0;
        }
        let voiced = strong_voiced || self.voiced_streak >= 2;

        // 2. Sustained quiet / unvoiced whisper trigger (requires 40 ms persistence so 10-30 ms
        //    keyboard ticks / mouse clicks / DPDFNet glitches never open the gate).
        let whisper_frame = f.rms > 5.0e-4 // > -66 dBFS
            && f.rms > self.noise_rms * 3.5
            && (f.prob >= 0.14 || f.voice_band_ratio >= 0.52 || whisper_rescued);
        if whisper_frame {
            self.whisper_streak = self.whisper_streak.saturating_add(1);
        } else {
            self.whisper_streak = 0;
        }
        let whisper_open = whisper_rescued || self.whisper_streak >= 4;

        // 3. Lower stay-open threshold (hysteresis prevents chattering on quiet syllables).
        let stay_open = self.open
            && ((f.prob >= 0.22 && f.rms > 2.5e-4)
                || (f.rms > 4.0e-4 && f.rms > self.noise_rms * 2.5 && (f.prob >= 0.10 || f.voice_band_ratio >= 0.48)));

        let detected = voiced || whisper_open || stay_open;
        if detected {
            self.open = true;
            self.hold = Self::HOLD_FRAMES;
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.open = false;
            // Track post-denoiser residual floor only during true closed silence.
            let clamped = f.rms.min(0.003);
            self.noise_rms += (clamped - self.noise_rms) * 0.05;
        }

        if self.open {
            self.gain = 1.0;
        } else {
            self.gain *= Self::RELEASE_MUL;
            if self.gain < Self::ZERO_SNAP {
                self.gain = 0.0;
            }
        }

        // AGC adapts on active speech frames (plus a tiny 30 ms grace) — never on trailing silence.
        let speech_active = detected || (self.open && self.hold >= Self::HOLD_FRAMES - 3);
        (start, self.gain, speech_active)
    }
}

/// The complete mic -> clean voice processor.
pub struct Chain {
    params: Arc<Params>,
    hp: Biquad,
    presence: Biquad,
    pre: PreLeveler,
    prev_dry: [f32; FRAME], // 1-hop delayed HPF'd input matching DPDFNet's 10 ms lag for whisper rescue
    dn: Denoiser,
    gate: Gate,
    agc: Agc,
    lim: Limiter,
    pending: Vec<f32>,
    wet: f32, // 0..1 crossfade between dry and denoised
    warmup: bool,
    pub vad: f32,
    pub peak: f32, // peak of the last processed frame (post chain)
}

impl Chain {
    pub fn new(params: Arc<Params>) -> Self {
        Self::with_highpass(params, 90.0)
    }

    pub fn with_highpass(params: Arc<Params>, hp_hz: f32) -> Self {
        Self::with_engine(params, hp_hz, Engine::Rnnoise)
    }

    pub fn with_engine(params: Arc<Params>, hp_hz: f32, engine: Engine) -> Self {
        Self {
            params,
            hp: Biquad::highpass(hp_hz),
            presence: Biquad::peaking_eq(3000.0, 0.9, 3.0),
            pre: PreLeveler::new(),
            prev_dry: [0.0; FRAME],
            dn: Denoiser::new(engine),
            gate: Gate::new(),
            agc: Agc::new(),
            lim: Limiter::new(),
            pending: Vec::with_capacity(FRAME * 4),
            wet: 1.0,
            warmup: true,
            vad: 0.0,
            peak: 0.0,
        }
    }

    /// Feeds samples and appends every finished 10 ms frame to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(input);
        let mut done = 0;
        while self.pending.len() - done >= FRAME {
            let mut frame = [0.0f32; FRAME];
            frame.copy_from_slice(&self.pending[done..done + FRAME]);
            done += FRAME;
            self.frame(&mut frame);
            out.extend_from_slice(&frame);
        }
        self.pending.drain(..done);
    }

    fn frame(&mut self, f: &mut [f32; FRAME]) {
        let p = &self.params;
        let hp_on = p.highpass.load(Relaxed);
        let dn_on = p.denoise.load(Relaxed);
        let agc_on = p.agc.load(Relaxed);
        let lim_on = p.limiter.load(Relaxed);
        let gate_on = p.gate.load(Relaxed);

        // Reject NaN/inf from a misbehaving device so they can't poison the filters.
        for s in f.iter_mut() {
            if !s.is_finite() {
                *s = 0.0;
            }
        }
        if hp_on {
            for s in f.iter_mut() {
                *s = self.hp.run(*s);
            }
        } else {
            self.hp.reset();
        }

        // Pre-level quiet input into DPDFNet's dynamic range, then restore unity gain after its
        // 1-hop lag so whispers aren't crushed while overall level stays calibrated.
        let use_pre = dn_on && self.dn.is_dpdfnet();
        let (pre0, pre1, inv0, inv1, rescue_blend) = self.pre.update(f, use_pre);
        let mut dn_in = *f;
        if use_pre {
            for i in 0..FRAME {
                let t = (i + 1) as f32 / FRAME as f32;
                dn_in[i] = (dn_in[i] * (pre0 + (pre1 - pre0) * t)).clamp(-1.0, 1.0);
            }
        }

        // The denoiser always runs so its state stays warm; the crossfade handles toggling.
        let mut wet = [0.0f32; FRAME];
        let mut feat = self.dn.run(&dn_in, &mut wet);
        if use_pre {
            for i in 0..FRAME {
                let t = (i + 1) as f32 / FRAME as f32;
                wet[i] *= inv0 + (inv1 - inv0) * t;
            }
            // Quiet-room unvoiced whisper rescue (attenuation limit): when sustained soft vocal-band
            // speech sits clearly above the ambient room floor (+8 dB) but DPDFNet over-attenuated
            // it as breathy noise, blend a bounded amount of the time-aligned HPF'd frame back in.
            if rescue_blend > 0.02 {
                let wet_rms = frame_rms(&wet);
                let dry_rms = frame_rms(&self.prev_dry);
                if dry_rms > 4e-4 && wet_rms < dry_rms * 0.5 {
                    let mix = rescue_blend * (1.0 - (wet_rms / dry_rms).clamp(0.0, 1.0));
                    for i in 0..FRAME {
                        wet[i] = wet[i] * (1.0 - mix) + self.prev_dry[i] * mix;
                    }
                }
            }
            feat.rms = frame_rms(&wet);
        }
        self.prev_dry = *f;

        if self.warmup {
            // The first frame carries fade-in artifacts.
            self.warmup = false;
            wet.fill(0.0);
            feat.prob = 0.0;
            feat.rms = 0.0;
        }
        self.vad = feat.prob;

        let w0 = self.wet;
        let w1 = if dn_on { 1.0 } else { 0.0 };
        self.wet = w1;
        for i in 0..FRAME {
            let t = (i + 1) as f32 / FRAME as f32;
            let w = w0 + (w1 - w0) * t;
            f[i] = f[i] * (1.0 - w) + wet[i] * w;
        }

        // Gate decision runs for ALL engines when denoise & gate are enabled.
        let whisper_rescued = use_pre && rescue_blend > 0.15;
        let (g0, g1, speech_active) = if dn_on && gate_on {
            self.gate.update(&feat, whisper_rescued)
        } else {
            self.gate.reset();
            let active = if dn_on { feat.prob > 0.35 || feat.rms > 0.002 } else { true };
            (1.0, 1.0, active)
        };

        // AGC2 runs before the zero-squelch gate multiplication so when the gate is closed (0.0),
        // VB-CABLE receives exact digital silence regardless of AGC gain.
        let a0 = self.agc.update(f, speech_active, agc_on);
        let a1 = self.agc.eff();
        let eq_blend = self.agc.presence_blend(agc_on);

        // Fast 3 ms attack ramp when opening from closed so word onsets are never dulled.
        let open_ramp_len = if g1 > g0 { (SAMPLE_RATE as usize * 3) / 1000 } else { FRAME };
        let mut peak = 0.0f32;
        for i in 0..FRAME {
            let tf = (i + 1) as f32 / FRAME as f32;
            let tg = ((i + 1) as f32 / open_ramp_len as f32).min(1.0);
            let g = g0 + (g1 - g0) * tg;
            let mut s = f[i] * (a0 + (a1 - a0) * tf);
            if eq_blend > 0.0 {
                let eq = self.presence.run(s);
                s += (eq - s) * eq_blend;
            } else {
                // Keep EQ state warm on silence/normal speech without jumping when blended in.
                let _ = self.presence.run(s);
            }
            s *= g;
            s = if lim_on { self.lim.run(s) } else { s.clamp(-1.0, 1.0) };
            // Guarantee exact digital zero once the gate has fully closed and limiter tail drained.
            if g == 0.0 && s.abs() < 1e-6 {
                s = 0.0;
            }
            f[i] = s;
            peak = peak.max(s.abs());
        }
        self.peak = peak;
    }
}

/// Offline helper: runs a whole buffer through a fresh chain. The output is a whole number of
/// frames and slightly delayed relative to the input (RNNoise latency plus limiter look-ahead).
pub fn process_buffer(params: Arc<Params>, input: &[f32]) -> Vec<f32> {
    process_buffer_hp(params, input, 90.0)
}

pub fn process_buffer_hp(params: Arc<Params>, input: &[f32], hp_hz: f32) -> Vec<f32> {
    process_buffer_engine(params, input, hp_hz, Engine::Rnnoise)
}

pub fn process_buffer_engine(params: Arc<Params>, input: &[f32], hp_hz: f32, engine: Engine) -> Vec<f32> {
    let mut chain = Chain::with_engine(params, hp_hz, engine);
    let mut out = Vec::with_capacity(input.len() + FRAME);
    chain.process(input, &mut out);
    // Flush the tail with silence so the last partial frame is produced.
    let pad = (FRAME - input.len() % FRAME) % FRAME;
    chain.process(&vec![0.0; pad], &mut out);
    out
}
