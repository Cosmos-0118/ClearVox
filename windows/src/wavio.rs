//! Tiny WAV helpers for the offline tools and tests (48 kHz mono f32 in memory).

use anyhow::{bail, Result};
use std::path::Path;

/// Reads a WAV file as mono f32 in [-1, 1]. Multi-channel files are averaged to mono.
/// Fails unless the file is 48 kHz (the pipeline's native rate).
pub fn read_mono_48k(path: &Path) -> Result<Vec<f32>> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    if spec.sample_rate != crate::dsp::SAMPLE_RATE {
        bail!("{}: expected 48000 Hz, got {} Hz (convert with ffmpeg -ar 48000)", path.display(), spec.sample_rate);
    }
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    Ok(samples.chunks(ch).map(|c| c.iter().sum::<f32>() / ch as f32).collect())
}

/// Writes mono f32 samples as a 16-bit 48 kHz WAV.
pub fn write_mono_48k(path: &Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: crate::dsp::SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)?;
    }
    w.finalize()?;
    Ok(())
}
