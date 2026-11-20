//! Mono float ring buffer bridging the capture clock to the render clock.
//! Self-corrects drift: drops old audio when too full, re-primes after an underrun.
//! (Same policy as the macOS `RingBuffer.swift`: 40 ms cushion, drop above 120 ms.)

use std::sync::Mutex;

struct Inner {
    buf: Vec<f32>,
    r: usize,
    w: usize,
    primed: bool,
    underruns: u64,
    overflows: u64,
}

pub struct RingBuffer {
    inner: Mutex<Inner>,
    target: usize,
    max_fill: usize,
}

impl RingBuffer {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f64;
        let cap = (sr * 0.5) as usize;
        Self {
            inner: Mutex::new(Inner { buf: vec![0.0; cap], r: 0, w: 0, primed: false, underruns: 0, overflows: 0 }),
            target: (sr * 0.04) as usize,
            max_fill: (sr * 0.12) as usize,
        }
    }

    pub fn write(&self, data: &[f32]) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let cap = g.buf.len();
        // A single write bigger than the buffer would wrap over itself; keep only its tail.
        let data = if data.len() > cap { &data[data.len() - cap..] } else { data };
        let w = g.w;
        for (i, s) in data.iter().enumerate() {
            g.buf[(w + i) % cap] = *s;
        }
        g.w += data.len();
        if g.w - g.r > self.max_fill {
            g.r = g.w - self.target;
            g.overflows += 1;
        }
    }

    /// Fills `out`; writes silence while priming or after an underrun.
    pub fn read(&self, out: &mut [f32]) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let n = out.len();
        let avail = g.w - g.r;
        if !g.primed && avail >= self.target {
            g.primed = true;
        }
        if !g.primed || avail < n {
            if avail < n && g.primed {
                g.underruns += 1;
            }
            if avail < n {
                g.primed = false;
            }
            out.fill(0.0);
            return;
        }
        let cap = g.buf.len();
        let r = g.r;
        for (i, o) in out.iter_mut().enumerate() {
            *o = g.buf[(r + i) % cap];
        }
        g.r += n;
    }

    /// (underruns, overflows) since creation, for diagnostics.
    pub fn stats(&self) -> (u64, u64) {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        (g.underruns, g.overflows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_until_primed_then_passes_audio() {
        let rb = RingBuffer::new(48_000);
        let mut out = [1.0f32; 100];
        rb.read(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
        rb.write(&vec![0.5; 2000]); // above the 40 ms (1920) cushion
        rb.read(&mut out);
        assert!(out.iter().all(|&s| s == 0.5));
    }

    #[test]
    fn drops_old_audio_when_too_full() {
        let rb = RingBuffer::new(48_000);
        rb.write(&vec![0.1; 10_000]); // above 120 ms (5760)
        assert_eq!(rb.stats().1, 1);
        let mut out = [0.0f32; 10];
        rb.read(&mut out);
        assert!(out.iter().all(|&s| s == 0.1));
    }

    #[test]
    fn underrun_reprimes() {
        let rb = RingBuffer::new(48_000);
        rb.write(&vec![0.3; 2000]);
        let mut out = vec![0.0f32; 2500];
        rb.read(&mut out); // wants more than available -> silence, underrun
        assert_eq!(rb.stats().0, 1);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn oversized_write_does_not_panic() {
        let rb = RingBuffer::new(48_000);
        rb.write(&vec![0.2; 100_000]);
        let mut out = [0.0f32; 10];
        rb.read(&mut out);
    }
}
