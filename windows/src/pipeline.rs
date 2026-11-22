//! Live audio pipeline: physical mic (WASAPI capture) -> DSP chain -> ring buffer -> virtual cable
//! playback endpoint (WASAPI render).
//!
//! Feedback guard (mirrors the macOS app): the output endpoint is resolved by its stable ID and must
//! be a virtual cable. If it can't be opened, or the mic is itself a virtual cable, nothing starts —
//! processed audio is never sent to a real speaker, and the pipeline never falls back to the default
//! playback device.

use crate::devices::is_virtual_name;
use crate::dsp::{self, Chain, Params, SAMPLE_RATE};
use crate::ring::RingBuffer;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use wasapi::{DeviceEnumerator, Direction, Role, SampleType, StreamMode, WaveFormat};

pub struct Config {
    /// Endpoint ID of the microphone; `None` uses the default recording device.
    pub input_id: Option<String>,
    /// Endpoint ID of the virtual cable playback side (required).
    pub output_id: String,
    pub params: Arc<Params>,
    pub engine: dsp::Engine,
    /// Test hook: feed this audio (48 kHz mono) instead of capturing a microphone.
    pub source_samples: Option<Arc<Vec<f32>>>,
}

pub struct Shared {
    peak_bits: AtomicU32,
    stop: AtomicBool,
    error: Mutex<Option<String>>,
    ring: RingBuffer,
}

impl Shared {
    fn set_error(&self, msg: String) {
        let mut e = self.error.lock().unwrap_or_else(|e| e.into_inner());
        if e.is_none() {
            *e = Some(msg);
        }
        self.stop.store(true, Relaxed);
    }
}

pub struct Pipeline {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl Pipeline {
    /// Opens both endpoints and starts the audio threads. Blocks briefly while they initialise.
    pub fn start(cfg: Config) -> Result<Pipeline, String> {
        if cfg.output_id.is_empty() {
            return Err("No virtual mic found. Install VB-CABLE from vb-audio.com/Cable".into());
        }
        let shared = Arc::new(Shared {
            peak_bits: AtomicU32::new(0),
            stop: AtomicBool::new(false),
            error: Mutex::new(None),
            ring: RingBuffer::new(SAMPLE_RATE),
        });
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let mut threads = vec![];

        // Render first: if the virtual mic can't be opened we never touch the microphone.
        {
            let (sh, tx, id) = (shared.clone(), tx.clone(), cfg.output_id.clone());
            threads.push(thread::Builder::new().name("sordino-render".into()).spawn(move || render_thread(sh, id, tx)).unwrap());
        }
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(abort(shared, threads, e)),
            Err(_) => return Err(abort(shared, threads, "Timed out opening the virtual mic".into())),
        }

        {
            let (sh, tx) = (shared.clone(), tx.clone());
            let (input, params, src, engine) = (cfg.input_id.clone(), cfg.params.clone(), cfg.source_samples.clone(), cfg.engine);
            threads.push(
                thread::Builder::new()
                    .name("sordino-capture".into())
                    .spawn(move || match src {
                        Some(samples) => file_source_thread(sh, samples, params, engine, tx),
                        None => capture_thread(sh, input, params, engine, tx),
                    })
                    .unwrap(),
            );
        }
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(abort(shared, threads, e)),
            Err(_) => return Err(abort(shared, threads, "Timed out opening the microphone".into())),
        }
        Ok(Pipeline { shared, threads })
    }

    /// Output peak since the last call (0..1), for the level meter.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.shared.peak_bits.swap(0, Relaxed))
    }

    /// Set once a device disappears or a stream fails; the owner should stop and rebuild.
    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// (underruns, overflows) of the capture->render ring buffer.
    pub fn ring_stats(&self) -> (u64, u64) {
        self.shared.ring.stats()
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.shared.stop.store(true, Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn abort(shared: Arc<Shared>, threads: Vec<JoinHandle<()>>, msg: String) -> String {
    shared.stop.store(true, Relaxed);
    for t in threads {
        let _ = t.join();
    }
    msg
}

fn bump_peak(shared: &Shared, peak: f32) {
    let cur = f32::from_bits(shared.peak_bits.load(Relaxed));
    if peak > cur {
        shared.peak_bits.store(peak.to_bits(), Relaxed);
    }
}

fn f32_from_bytes(bytes: &[u8], out: &mut Vec<f32>) {
    out.clear();
    out.extend(bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
}

// ---------------------------------------------------------------- render (virtual mic)

fn render_thread(shared: Arc<Shared>, id: String, ready: mpsc::Sender<Result<(), String>>) {
    let _ = wasapi::initialize_mta();
    let result = (|| -> Result<_, String> {
        let en = DeviceEnumerator::new().map_err(|e| format!("Audio system unavailable: {e}"))?;
        let dev = en.get_device(&id).map_err(|_| "The virtual mic is no longer available. Is VB-CABLE installed?".to_string())?;
        if dev.get_direction() != Direction::Render {
            return Err("Output must be a playback device".into());
        }
        let name = dev.get_friendlyname().unwrap_or_default();
        if !is_virtual_name(&name) {
            return Err(format!("\"{name}\" is a real playback device. Sordino only plays to a virtual mic, to avoid feedback."));
        }
        let mut client = dev.get_iaudioclient().map_err(|e| format!("Couldn't open the virtual mic: {e}"))?;
        let fmt = WaveFormat::new(32, 32, SampleType::Float, SAMPLE_RATE as usize, 2, None);
        let (period, _) = client.get_device_period().map_err(|e| e.to_string())?;
        client
            .initialize_client(&fmt, Direction::Render, StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: period.max(200_000) }, None)
            .map_err(|e| format!("Couldn't route audio to the virtual mic: {e}"))?;
        let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
        let render = client.get_audiorenderclient().map_err(|e| e.to_string())?;
        // Prefill with silence so playback starts with a full buffer.
        let frames = client.get_available_space_in_frames().map_err(|e| e.to_string())? as usize;
        render.write_to_device(&vec![0u8; frames * 8], None).map_err(|e| e.to_string())?;
        client.start_stream().map_err(|e| e.to_string())?;
        Ok((client, event, render))
    })();

    let (client, event, render) = match result {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    let mut mono: Vec<f32> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    while !shared.stop.load(Relaxed) {
        if event.wait_for_event(200).is_err() {
            continue; // timeout: nothing to do, re-check the stop flag
        }
        let frames = match client.get_available_space_in_frames() {
            Ok(n) => n as usize,
            Err(e) => {
                shared.set_error(format!("Virtual mic stopped: {e}"));
                break;
            }
        };
        if frames == 0 {
            continue;
        }
        mono.resize(frames, 0.0);
        shared.ring.read(&mut mono);
        bytes.clear();
        for s in &mono {
            let b = s.to_le_bytes();
            bytes.extend_from_slice(&b);
            bytes.extend_from_slice(&b);
        }
        if let Err(e) = render.write_to_device(&bytes, None) {
            shared.set_error(format!("Virtual mic stopped: {e}"));
            break;
        }
    }
    let _ = client.stop_stream();
}

// ---------------------------------------------------------------- capture (physical mic)

fn capture_thread(shared: Arc<Shared>, input: Option<String>, params: Arc<Params>, engine: dsp::Engine, ready: mpsc::Sender<Result<(), String>>) {
    let _ = wasapi::initialize_mta();
    let result = (|| -> Result<_, String> {
        let en = DeviceEnumerator::new().map_err(|e| format!("Audio system unavailable: {e}"))?;
        let dev = match &input {
            Some(id) => en.get_device(id).map_err(|_| "The selected microphone is not available".to_string())?,
            None => en.get_default_device_for_role(Direction::Capture, Role::Console).map_err(|_| "No microphone found".to_string())?,
        };
        if dev.get_direction() != Direction::Capture {
            return Err("Input must be a recording device".into());
        }
        let name = dev.get_friendlyname().unwrap_or_default();
        if is_virtual_name(&name) {
            return Err(format!("\"{name}\" is a virtual mic. Pick your real microphone in Sordino."));
        }
        let mut client = dev.get_iaudioclient().map_err(|e| format!("Couldn't open the microphone: {e}"))?;
        let fmt = WaveFormat::new(32, 32, SampleType::Float, SAMPLE_RATE as usize, 1, None);
        let (period, _) = client.get_device_period().map_err(|e| e.to_string())?;
        client
            .initialize_client(&fmt, Direction::Capture, StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: period.max(200_000) }, None)
            .map_err(|e| format!("Couldn't open the microphone (is it in use or blocked by Windows privacy settings?): {e}"))?;
        let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
        let cap = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
        client.start_stream().map_err(|e| e.to_string())?;
        Ok((client, event, cap))
    })();

    let (client, event, cap) = match result {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    let mut chain = Chain::with_engine(params, 90.0, engine);
    let mut raw: Vec<u8> = vec![0; 48_000 * 4]; // up to 1 s per read
    let mut samples: Vec<f32> = Vec::new();
    let mut processed: Vec<f32> = Vec::new();
    while !shared.stop.load(Relaxed) {
        if event.wait_for_event(200).is_err() {
            continue;
        }
        loop {
            match cap.get_next_packet_size() {
                Ok(Some(0)) | Ok(None) => break,
                Ok(Some(_)) => {}
                Err(e) => {
                    shared.set_error(format!("Microphone stopped: {e}"));
                    break;
                }
            }
            match cap.read_from_device(&mut raw) {
                Ok((frames, _)) if frames > 0 => {
                    f32_from_bytes(&raw[..frames as usize * 4], &mut samples);
                    processed.clear();
                    chain.process(&samples, &mut processed);
                    if !processed.is_empty() {
                        shared.ring.write(&processed);
                        bump_peak(&shared, chain.peak);
                    }
                }
                Ok(_) => break,
                Err(e) => {
                    shared.set_error(format!("Microphone stopped: {e}"));
                    break;
                }
            }
        }
    }
    let _ = client.stop_stream();
}

/// Plays a prepared sample buffer through the DSP chain in real time (test/diagnostic source).
fn file_source_thread(shared: Arc<Shared>, samples: Arc<Vec<f32>>, params: Arc<Params>, engine: dsp::Engine, ready: mpsc::Sender<Result<(), String>>) {
    let _ = ready.send(Ok(()));
    let mut chain = Chain::with_engine(params, 90.0, engine);
    let mut out = Vec::new();
    let chunk = dsp::FRAME;
    let start = std::time::Instant::now();
    let mut pos = 0usize;
    while !shared.stop.load(Relaxed) {
        // Pace to real time.
        let due = (start.elapsed().as_secs_f64() * SAMPLE_RATE as f64) as usize;
        while pos < due && pos < samples.len() {
            let end = (pos + chunk).min(samples.len());
            out.clear();
            chain.process(&samples[pos..end], &mut out);
            if !out.is_empty() {
                shared.ring.write(&out);
                bump_peak(&shared, chain.peak);
            }
            pos = end;
        }
        if pos >= samples.len() {
            // Keep the stream alive with silence after the file ends.
            out.clear();
            chain.process(&vec![0.0; chunk], &mut out);
            shared.ring.write(&out);
        }
        thread::sleep(Duration::from_millis(5));
    }
}
