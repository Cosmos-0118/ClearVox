//! Voice DSP chain: high-pass -> RNNoise -> gate -> auto gain -> peak limiter.
//!
//! Everything runs on 48 kHz mono `f32` audio in [-1, 1] in blocks of [`FRAME`] (10 ms) samples,
//! which is what RNNoise expects. The chain is stateful and streaming: feed it any number of
//! samples and it returns whole frames as they become available.

use nnnoiseless::DenoiseState;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME: usize = DenoiseState::FRAME_SIZE; // 480 samples = 10 ms

/// Which neural denoiser runs inside the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// RNNoise: tiny and fast.
    Rnnoise,
    /// DeepFilterNet3: stronger on non-stationary noise, needs the `dfn` build feature.
    DeepFilter,
}

impl Engine {
    pub fn available(self) -> bool {
        match self {
            Engine::Rnnoise => true,
            Engine::DeepFilter => cfg!(feature = "dfn"),
        }
    }
}

enum Denoiser {
    Rnn(Box<DenoiseState<'static>>),
    #[cfg(feature = "dfn")]
    Dfn(Box<df::tract::DfTract>),
}

impl Denoiser {
    fn new(engine: Engine) -> Self {
        match engine {
            #[cfg(feature = "dfn")]
            Engine::DeepFilter => {
                let rp = df::tract::RuntimeParams::default_with_ch(1);
                match df::tract::DfTract::new(df::tract::DfParams::default(), &rp) {
                    Ok(m) => return Denoiser::Dfn(Box::new(m)),
                    Err(_) => {} // fall back to RNNoise
                }
                Denoiser::Rnn(DenoiseState::new())
            }
            _ => Denoiser::Rnn(DenoiseState::new()),
        }
    }

    /// `input`/`output` are in [-1, 1]. Returns a speech probability in 0..1.
    fn run(&mut self, input: &[f32; FRAME], output: &mut [f32; FRAME]) -> f32 {
        match self {
            Denoiser::Rnn(st) => {
                let mut scaled = [0.0f32; FRAME];
                for i in 0..FRAME {
                    scaled[i] = input[i] * 32768.0;
                }
                let vad = st.process_frame(output, &scaled);
                for s in output.iter_mut() {
                    *s /= 32768.0;
                }
                vad
            }
            #[cfg(feature = "dfn")]
            Denoiser::Dfn(m) => {
                let noisy = ndarray::ArrayView2::from_shape((1, FRAME), input).unwrap();
                let mut enh = ndarray::ArrayViewMut2::from_shape((1, FRAME), output).unwrap();
                match m.process(noisy, enh.view_mut()) {
                    Ok(lsnr) => ((lsnr + 5.0) / 20.0).clamp(0.0, 1.0),
                    Err(_) => {
                        output.copy_from_slice(input);
                        0.5
                    }
                }
            }
        }
    }

    fn is_rnnoise(&self) -> bool {
        matches!(self, Denoiser::Rnn(_))
    }
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

/// 2nd-order Butterworth high-pass (RBJ cookbook).
struct HighPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl HighPass {
    fn new(freq: f32) -> Self {
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

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        // Transposed direct form II.
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Look-ahead peak limiter: ceiling -1 dBFS, 1 ms look-ahead, ~120 ms release.
struct Limiter {
    delay: Vec<f32>,
    pos: usize,
    gain: f32,
}

const LIMIT: f32 = 0.891; // -1 dBFS
const LOOKAHEAD: usize = 48; // 1 ms

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
            // Reach the target within the look-ahead time so the peak is never exceeded.
            self.gain += (target - self.gain) * 0.25;
            if self.gain < target {
                self.gain = target;
            }
        } else {
            self.gain += (target - self.gain) * (1.0 / (0.12 * SAMPLE_RATE as f32));
        }
        (self.delay[self.pos] * self.gain).clamp(-1.0, 1.0)
    }
}

/// Slow automatic level control, only adapting while RNNoise says someone is speaking.
struct Agc {
    gain: f32,
}

impl Agc {
    const TARGET_RMS: f32 = 0.08; // about -22 dBFS
    const MAX_GAIN: f32 = 4.0; // +12 dB
    const MIN_GAIN: f32 = 0.35; // -9 dB

    fn new() -> Self {
        Self { gain: 1.0 }
    }

    /// Updates the gain and returns the gain this frame starts from (it ramps to `self.gain`).
    fn update(&mut self, frame: &[f32], vad: f32, enabled: bool) -> f32 {
        let prev = self.gain;
        if enabled && vad > 0.7 {
            let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
            if rms > 0.003 {
                let want = (Self::TARGET_RMS / rms).clamp(Self::MIN_GAIN, Self::MAX_GAIN);
                // Cut quickly (loud talker), boost slowly.
                let k = if want < self.gain { 0.15 } else { 0.01 };
                self.gain += (want - self.gain) * k;
            }
        } else if !enabled {
            self.gain += (1.0 - self.gain) * 0.1;
        }
        prev
    }
}

/// VAD-driven expander: quietens the residue RNNoise leaves between words.
struct Gate {
    gain: f32,
    hold: u32,
}

impl Gate {
    const FLOOR: f32 = 0.12; // about -18 dB
    const HOLD_FRAMES: u32 = 20; // 200 ms

    fn new() -> Self {
        Self { gain: 1.0, hold: 0 }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.hold = Self::HOLD_FRAMES;
    }

    /// Returns (gain at start of frame, gain at end of frame).
    fn update(&mut self, vad: f32) -> (f32, f32) {
        let start = self.gain;
        if vad > 0.35 {
            self.hold = Self::HOLD_FRAMES;
        } else if self.hold > 0 {
            self.hold -= 1;
        }
        let target = if self.hold > 0 { 1.0 } else { Self::FLOOR };
        let k = if target > self.gain { 0.7 } else { 0.12 };
        self.gain += (target - self.gain) * k;
        (start, self.gain)
    }
}

/// The complete mic -> clean voice processor.
pub struct Chain {
    params: Arc<Params>,
    hp: HighPass,
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
            hp: HighPass::new(hp_hz),
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
            // Keep filter state quiet so enabling it later doesn't click.
            self.hp.z1 = 0.0;
            self.hp.z2 = 0.0;
        }

        // The denoiser always runs so its state stays warm; the crossfade handles toggling.
        let input = *f;
        let mut wet = [0.0f32; FRAME];
        let vad = self.dn.run(&input, &mut wet);
        if self.warmup {
            // The first RNNoise frame carries fade-in artifacts.
            self.warmup = false;
            wet.fill(0.0);
        }
        self.vad = vad;

        let w0 = self.wet;
        let w1 = if dn_on { 1.0 } else { 0.0 };
        self.wet = w1;
        for i in 0..FRAME {
            let t = (i + 1) as f32 / FRAME as f32;
            let w = w0 + (w1 - w0) * t;
            f[i] = f[i] * (1.0 - w) + wet[i] * w;
        }

        if dn_on && self.dn.is_rnnoise() && p.gate.load(Relaxed) {
            let (g0, g1) = self.gate.update(vad);
            for i in 0..FRAME {
                let t = (i + 1) as f32 / FRAME as f32;
                f[i] *= g0 + (g1 - g0) * t;
            }
        } else {
            self.gate.reset();
        }

        let a0 = self.agc.update(f, if dn_on { vad } else { 1.0 }, agc_on);
        let a1 = self.agc.gain;
        let mut peak = 0.0f32;
        for i in 0..FRAME {
            let t = (i + 1) as f32 / FRAME as f32;
            let mut s = f[i] * (a0 + (a1 - a0) * t);
            s = if lim_on { self.lim.run(s) } else { s.clamp(-1.0, 1.0) };
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
