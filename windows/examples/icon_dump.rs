// Dumps the tray icon states as raw RGBA (4 states side by side) for eyeballing: icon_dump out.rgba
fn main() {
    let states = [(true, false, 0.0), (true, false, 0.8), (false, false, 0.0), (true, true, 0.0)];
    let mut rows = vec![vec![]; 32];
    for (on, err, lvl) in states {
        let px = sordino::icon::pixels(on, err, lvl);
        for y in 0..32 { rows[y].extend_from_slice(&px[y * 128..(y + 1) * 128]); }
    }
    std::fs::write(std::env::args().nth(1).unwrap(), rows.concat()).unwrap();
}
