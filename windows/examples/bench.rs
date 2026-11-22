// Per-frame cost of the denoiser: cargo run --release --example bench -- [dpdfnet2|dpdfnet8] [seconds]
use clearvox::dpdfnet::{Dpdfnet, Size, HOP};
use clearvox::wavio;
use std::time::Instant;
#[allow(deprecated)]
fn ftz() {
    #[cfg(target_arch = "x86_64")]
    unsafe { use std::arch::x86_64::*; _mm_setcsr(_mm_getcsr() | 0x8040); }
}
fn main() {
    if std::env::var("FTZ").is_ok() { ftz(); println!("FTZ/DAZ on"); }
    let mode = std::env::var("MODE").unwrap_or("speech".into());
    let a: Vec<String> = std::env::args().collect();
    let size = if a.get(1).map(|s| s.as_str()) == Some("dpdfnet2") { Size::Two } else { Size::Eight };
    let secs: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let x = wavio::read_mono_48k(std::path::Path::new("target/fixtures/clean.wav")).unwrap();
    let t0 = Instant::now();
    let mut d = Dpdfnet::new(size).unwrap();
    d.skip_idle = std::env::var("SKIP").map(|v| v != "0").unwrap_or(false);
    println!("load {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
    let n = secs * 100;
    let mut times = Vec::with_capacity(n);
    let mut out = [0.0f32; HOP];
    let mut all = Vec::new();
    let mut seed = 12345u64;
    for i in 0..n + 200 {
        let off = (i * HOP) % (x.len() - HOP);
        let mut f = [0.0f32; HOP];
        f.copy_from_slice(&x[off..off + HOP]);
        match mode.as_str() {
            "silence" => f.fill(0.0),
            "n70" | "n50" => { let g = if mode == "n70" { 3e-4 } else { 3e-3 }; for v in f.iter_mut() { seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); *v = ((seed >> 40) as f32 / 8388608.0 - 1.0) * g; } }
            _ => {}
        }
        let t = Instant::now();
        d.process(&f, &mut out).unwrap();
        if i >= 200 { times.push(t.elapsed().as_secs_f64() * 1e3); all.extend_from_slice(&out); }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean: f64 = times.iter().sum::<f64>() / times.len() as f64;
    println!("{:?}: mean {:.3} ms  p50 {:.3}  p99 {:.3}  max {:.3}  => {:.2}% of one core (real-time)", size, mean, times[times.len()/2], times[times.len()*99/100], times[times.len()-1], mean / 10.0 * 100.0);
    let sum: f64 = all.iter().map(|&s| (s as f64).abs()).sum();
    println!("skipped {} / {}", d.skipped, n + 200);
    println!("checksum {:.4}", sum);
    if let Some(p) = a.get(3) { wavio::write_mono_48k(std::path::Path::new(p), &all).ok(); }
}
