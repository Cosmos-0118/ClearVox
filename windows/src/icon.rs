//! Procedurally drawn tray icons: a mic on a coloured badge. The mic capsule fills as you speak.

use tray_icon::Icon;

const N: usize = 32;

fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - (ax + t * dx)).powi(2) + (py - (ay + t * dy)).powi(2)).sqrt()
}

/// `level` is 0..=1 (output loudness); `on` selects the colour; `error` draws an amber badge.
pub fn render(on: bool, error: bool, level: f32) -> Icon {
    Icon::from_rgba(pixels(on, error, level), N as u32, N as u32).expect("valid icon buffer")
}

/// 32x32 RGBA pixels of the icon.
pub fn pixels(on: bool, error: bool, level: f32) -> Vec<u8> {
    let mut px = vec![0u8; N * N * 4];
    let badge = if error { [217, 119, 6] } else if on { [20, 150, 110] } else { [107, 114, 128] };
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded-square badge.
            let (cx, cy) = ((fx - 16.0).abs() - 11.0, (fy - 16.0).abs() - 11.0);
            let d = (cx.max(0.0).powi(2) + cy.max(0.0).powi(2)).sqrt() + cx.max(cy).min(0.0) - 5.0;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let mut rgb = [badge[0] as f32, badge[1] as f32, badge[2] as f32];

            // Mic capsule: segment (16,9)-(16,15), radius 4.2. Fill from the bottom with level.
            let dc = dist_to_segment(fx, fy, 16.0, 9.5, 16.0, 15.0) - 4.2;
            let cap = (0.5 - dc).clamp(0.0, 1.0);
            let filled = fy > 20.0 - level.clamp(0.0, 1.0) * 14.0;
            // Stand: U-shaped arc, stem, base.
            let arc = {
                let r = ((fx - 16.0).powi(2) + (fy - 15.0).powi(2)).sqrt();
                if fy >= 15.0 { (0.5 - (r - 7.8).abs() + 0.9).clamp(0.0, 1.0) } else { 0.0 }
            };
            let stem = (0.5 - dist_to_segment(fx, fy, 16.0, 22.0, 16.0, 25.5) + 0.9).clamp(0.0, 1.0);
            let base = (0.5 - dist_to_segment(fx, fy, 12.0, 25.5, 20.0, 25.5) + 0.9).clamp(0.0, 1.0);
            let ink = arc.max(stem).max(base);
            let capsule_mix = cap * if filled { 1.0 } else { 0.35 };
            let w = ink.max(capsule_mix);
            for c in rgb.iter_mut() {
                *c = *c * (1.0 - w) + 255.0 * w;
            }
            // "Off" slash.
            if !on && !error {
                let s = dist_to_segment(fx, fy, 7.0, 7.0, 25.0, 25.0);
                let sl = (0.5 - (s - 1.2)).clamp(0.0, 1.0);
                for c in rgb.iter_mut() {
                    *c = *c * (1.0 - sl) + 255.0 * sl;
                }
            }
            let i = (y * N + x) * 4;
            px[i] = rgb[0] as u8;
            px[i + 1] = rgb[1] as u8;
            px[i + 2] = rgb[2] as u8;
            px[i + 3] = (cover * 255.0) as u8;
        }
    }
    px
}
