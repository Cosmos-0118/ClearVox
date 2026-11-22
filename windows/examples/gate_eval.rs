// Compares DPDFNet with and without idle skipping on WAV files.
use clearvox::dpdfnet::{Dpdfnet, Size, HOP};
use clearvox::wavio;
fn run(x: &[f32], skip: bool) -> (Vec<f32>, u64) {
    let mut d = Dpdfnet::new(Size::Eight).unwrap();
    d.skip_idle = skip;
    let mut out = vec![];
    let mut o = [0.0f32; HOP];
    for c in x.chunks_exact(HOP) { let mut f = [0.0; HOP]; f.copy_from_slice(c); d.process(&f, &mut o).unwrap(); out.extend_from_slice(&o); }
    (out, d.skipped)
}
fn main() {
    for p in std::env::args().skip(1) {
        let x = wavio::read_mono_48k(std::path::Path::new(&p)).unwrap();
        let (a, _) = run(&x, false);
        let (b, sk) = run(&x, true);
        let n = a.len();
        let e: f64 = a.iter().map(|&v| (v as f64).powi(2)).sum();
        let d: f64 = a.iter().zip(&b).map(|(&u, &v)| ((u - v) as f64).powi(2)).sum();
        let maxd = a.iter().zip(&b).map(|(&u, &v)| (u - v).abs()).fold(0.0f32, f32::max);
        let maxa = a.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        println!("{:40} skipped {:5.1}%  diff {:6.1} dB rel  maxdiff {:.5} (peak {:.3})", p.rsplit(['/', '\\']).next().unwrap(), 100.0 * sk as f64 / (n / HOP) as f64, 10.0 * ((d + 1e-20) / (e + 1e-20)).log10(), maxd, maxa);
    }
}
