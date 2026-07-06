// Native WASAPI loopback capture (Windows) — game/system audio for clips with zero user setup.
//
// ffmpeg's dshow input can only record system audio through a virtual loopback DEVICE ("Stereo
// Mix", virtual-audio-capturer) that most machines don't have — which is why Medal/ShadowPlay
// ship their own loopback capture instead of asking users to install one. This module does the
// same natively: cpal opens an *input* stream on the default *output* device (WASAPI's loopback
// mode — whatever the user hears, any headphones/speakers) and the raw PCM is piped into the
// capture ffmpeg's stdin as an extra `-f f32le/-f s16le ... -i pipe:0` input, mixed with the mic
// track by the existing amix path in clipper.rs.
//
// Two WASAPI realities shape the design:
// - The data callback runs on cpal's audio thread and must never block, so samples hop through a
//   bounded channel to a writer thread that owns the pipe (a full pipe write inside the callback
//   would stall the audio engine).
// - Loopback delivers NO packets while the machine is silent (the engine only pumps while
//   something plays), but ffmpeg needs a continuous byte stream to keep the audio track aligned
//   with the video — so the writer paces itself against the wall clock and pads with silence
//   whenever real packets fall behind.

use crate::clipper::PipeAudioSpec;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Write;
use std::process::ChildStdin;
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

/// PCM spec of the default output device, when WASAPI loopback capture looks possible on it.
/// `None` (no output device, exotic sample format) sends the caller down the dshow
/// loopback-device fallback path in `capture_input_args`.
pub fn default_output_spec() -> Option<PipeAudioSpec> {
    let device = cpal::default_host().default_output_device()?;
    let config = device.default_output_config().ok()?;
    let format = match config.sample_format() {
        cpal::SampleFormat::F32 => "f32le",
        cpal::SampleFormat::I16 => "s16le",
        _ => return None,
    };
    Some(PipeAudioSpec {
        sample_rate: config.sample_rate().0,
        channels: config.channels(),
        format,
    })
}

/// Starts loopback capture feeding `stdin`. Detached on purpose: the thread's lifetime is tied
/// to the ffmpeg process itself — when the capture is killed (session end, watchdog respawn, mic
/// toggle restart), the pipe breaks and the thread exits on its next write. No handle to store,
/// no shutdown ordering to get wrong.
pub fn start(stdin: ChildStdin, spec: PipeAudioSpec) {
    if let Err(e) = std::thread::Builder::new()
        .name("wasapi-loopback".into())
        .spawn(move || run(stdin, spec))
    {
        // stdin is dropped here — ffmpeg sees EOF on the audio input rather than blocking on a
        // pipe nothing will ever write to.
        eprintln!("loopback: failed to spawn writer thread: {e}");
    }
}

fn run(mut stdin: ChildStdin, spec: PipeAudioSpec) {
    // Guard against the default output device changing between `default_output_spec` (which
    // fixed the format in ffmpeg's args) and now — a mismatched stream would pipe misinterpreted
    // PCM. Bailing drops stdin, so ffmpeg just sees EOF on the audio input.
    match default_output_spec() {
        Some(current)
            if current.sample_rate == spec.sample_rate
                && current.channels == spec.channels
                && current.format == spec.format => {}
        _ => {
            eprintln!("loopback: default output device changed before capture started — no game audio");
            return;
        }
    }
    let Some(device) = cpal::default_host().default_output_device() else { return };
    let Ok(config) = device.default_output_config() else { return };

    // Bounded and non-blocking on both sides: the callback drops packets when the writer is
    // behind (the pacing below fills any hole with silence), the writer drains without waiting.
    let (tx, rx) = sync_channel::<Vec<u8>>(64);
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut bytes = Vec::with_capacity(data.len() * 4);
                for s in data {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
                let _ = tx.try_send(bytes);
            },
            |e| eprintln!("loopback: stream error: {e}"),
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.config(),
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let mut bytes = Vec::with_capacity(data.len() * 2);
                for s in data {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
                let _ = tx.try_send(bytes);
            },
            |e| eprintln!("loopback: stream error: {e}"),
            None,
        ),
        _ => return,
    };
    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            eprintln!("loopback: failed to open WASAPI loopback stream: {e}");
            return;
        }
    };
    if let Err(e) = stream.play() {
        eprintln!("loopback: failed to start WASAPI loopback stream: {e}");
        return;
    }

    // Writer loop: forward real packets, pad with silence when the engine goes quiet. Total
    // bytes written stay glued to the wall clock, so the audio track can't drift against the
    // video across a long session. PCM zeros are silence in both f32le and s16le.
    let frame_bytes = spec.channels as usize * if spec.format == "f32le" { 4 } else { 2 };
    let byte_rate = spec.sample_rate as u64 * frame_bytes as u64;
    let silence = vec![0u8; (byte_rate / 10) as usize / frame_bytes * frame_bytes]; // ~100ms
    let started = Instant::now();
    let mut written: u64 = 0;
    loop {
        std::thread::sleep(Duration::from_millis(10));
        while let Ok(chunk) = rx.try_recv() {
            if stdin.write_all(&chunk).is_err() {
                return; // capture ffmpeg is gone — this is the thread's exit signal
            }
            written += chunk.len() as u64;
        }
        // 50ms of slack before padding kicks in, so real packets that are merely late don't get
        // silence spliced in front of them; catch-up is capped at 1s per tick.
        let target = started.elapsed().as_millis() as u64 * byte_rate / 1000;
        if target > written + byte_rate / 20 {
            let mut need =
                ((target - written).min(byte_rate) as usize) / frame_bytes * frame_bytes;
            while need > 0 {
                let n = need.min(silence.len());
                if stdin.write_all(&silence[..n]).is_err() {
                    return;
                }
                written += n as u64;
                need -= n;
            }
        }
    }
}
