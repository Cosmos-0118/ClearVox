// Prints per-0.5 s VAD, input and output RMS for a WAV: cargo run --release --example trace -- in.wav [--no-gate]
use sordino::{dsp, wavio};
fn rms(x: &[f32]) -> f32 { (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt() }
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let x = wavio::read_mono_48k(std::path::Path::new(&a[1])).unwrap();
    let p = dsp::Params::new(true, true, true, false);
    p.gate.store(!a.iter().any(|s| s == "--no-gate"), std::sync::atomic::Ordering::Relaxed);
    let mut c = dsp::Chain::new(p);
    let mut out = vec![];
    let mut line = String::new();
    for (i, f) in x.chunks(dsp::FRAME).enumerate() {
        if f.len() < dsp::FRAME { break; }
        out.clear();
        c.process(f, &mut out);
        if i % 50 == 0 { line.clear(); }
        line += &format!("{:.2} ", c.vad);
        if i % 50 == 49 {
            let s = (i - 49) * dsp::FRAME;
            println!("{:5.1}s in {:6.1} out {:6.1} dB vad {}", s as f32 / 48000.0, 20.0 * rms(&x[s..s + 50 * dsp::FRAME]).log10(), 20.0 * rms(&out).log10(), &line[..line.len().min(40)]);
        }
    }
}
