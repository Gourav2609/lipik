//! Helpers for driving a `Recorder` over the fake backend deterministically.

use std::{
    sync::{Arc, atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};

use crate::backend::{DeviceFormat, InputSample, SampleFormat, fake::FakeBackend};
use crate::capture::engine::{Engine, Timeouts};
use crate::capture::transport::CaptureTransportState;
use crate::{AudioChunk, Error, Recorder, RecorderConfig, Sink, Stopped};

pub const WAIT: Duration = Duration::from_secs(5);

/// Short bounds, except watchdog bounds, which slow CI could trip.
pub fn timeouts() -> Timeouts {
    Timeouts {
        open: Duration::from_secs(2),
        close: Duration::from_secs(2),
        delivery_exit: Duration::from_millis(500),
        no_audio: Duration::from_secs(60),
        stall: Duration::from_secs(60),
        heartbeat: Duration::from_secs(60),
        pause_ack: Duration::from_millis(500),
        stop: Duration::from_secs(2),
        watchdog_tick: Duration::from_millis(5),
    }
}

pub fn fake(sample_rate: u32, channels: u16) -> FakeBackend {
    fake_with(sample_rate, channels, SampleFormat::F32)
}

pub fn fake_with(sample_rate: u32, channels: u16, sample_format: SampleFormat) -> FakeBackend {
    FakeBackend::new(DeviceFormat {
        sample_rate,
        channels,
        sample_format,
    })
}

pub fn open<S: Sink>(fake: &FakeBackend, config: RecorderConfig) -> Recorder<S> {
    open_with(fake, config, timeouts()).expect("open")
}

pub fn open_with<S: Sink>(
    fake: &FakeBackend,
    config: RecorderConfig,
    timeouts: Timeouts,
) -> Result<Recorder<S>, Error> {
    open_handled(fake, config, None, timeouts)
}

/// Opens over the fake backend with an optional failure handler.
pub fn open_handled<S: Sink>(
    fake: &FakeBackend,
    config: RecorderConfig,
    handler: Option<Box<dyn FnOnce(Error) + Send + 'static>>,
    timeouts: Timeouts,
) -> Result<Recorder<S>, Error> {
    Ok(Recorder {
        engine: Engine::open(Arc::new(fake.clone()), config, handler, timeouts)?,
    })
}

/// Opens with a failure handler that forwards to the returned receiver.
pub fn open_notified<S: Sink>(
    fake: &FakeBackend,
    config: RecorderConfig,
    timeouts: Timeouts,
) -> (Recorder<S>, mpsc::Receiver<Error>) {
    let (tx, rx) = mpsc::channel();
    let recorder = open_handled(
        fake,
        config,
        Some(Box::new(move |error| {
            let _ = tx.send(error);
        })),
        timeouts,
    )
    .expect("open");
    (recorder, rx)
}

pub fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_micros(200));
    }
}

pub fn transport<S>(recorder: &Recorder<S>) -> &CaptureTransportState {
    &recorder.engine.shared.transport
}

/// Starts a recording and waits until the delivery thread has applied it,
/// so the next pushed block belongs to the recording.
pub fn start<S: Sink>(recorder: &Recorder<S>, sink: S) {
    let before = transport(recorder).starts_applied.load(Ordering::Acquire);
    recorder.start(sink).map_err(|e| e.error).expect("start");
    wait_until("start applied", || {
        transport(recorder).starts_applied.load(Ordering::Acquire) > before
    });
}

/// Pushes idle audio and waits until the delivery thread has discarded it.
pub fn push_idle<S: Sink, T: InputSample>(recorder: &Recorder<S>, fake: &FakeBackend, block: &[T]) {
    let channels = fake_channels(recorder);
    let before = transport(recorder).frames_drained.load(Ordering::Acquire);
    assert!(fake.push(block));
    wait_until("idle audio drained", || {
        transport(recorder).frames_drained.load(Ordering::Acquire)
            >= before + block.len() / channels
    });
}

fn fake_channels<S: Sink>(recorder: &Recorder<S>) -> usize {
    recorder.info().device_format.channels as usize
}

/// Stops, delivering `boundary` as the block the callback is writing when
/// the stop request arrives (it belongs to the recording).
pub fn stop_with_boundary<S: Sink, T: InputSample>(
    recorder: &Recorder<S>,
    fake: &FakeBackend,
    boundary: &[T],
) -> Result<Stopped<S>, Error> {
    thread::scope(|scope| {
        let stopping = scope.spawn(|| recorder.stop());
        wait_until("pause requested", || {
            transport(recorder).pause_requested.load(Ordering::Acquire)
        });
        assert!(fake.push(boundary));
        stopping.join().unwrap()
    })
}

/// A sink that keeps every chunk exactly as delivered.
#[derive(Debug, Default)]
pub struct Chunks {
    pub chunks: Vec<(Vec<f32>, usize)>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Chunks {
    /// The real audio: each chunk's valid frames, interleaved.
    pub fn real(&self) -> Vec<f32> {
        let channels = self.channels as usize;
        self.chunks
            .iter()
            .flat_map(|(samples, valid)| samples[..valid * channels].iter().copied())
            .collect()
    }

    /// Everything delivered, padding included.
    pub fn all(&self) -> Vec<f32> {
        self.chunks
            .iter()
            .flat_map(|(s, _)| s.iter().copied())
            .collect()
    }

    pub fn valid_frames(&self) -> usize {
        self.chunks.iter().map(|(_, v)| v).sum()
    }
}

impl Sink for Chunks {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        self.sample_rate = chunk.sample_rate;
        self.channels = chunk.channels;
        self.chunks
            .push((chunk.samples.to_vec(), chunk.valid_frames));
    }
}

/// A ramp of distinct, exactly representable values, for checking which
/// samples arrived.
pub fn ramp(start: usize, len: usize) -> Vec<f32> {
    (start..start + len)
        .map(|i| (i % 1000) as f32 / 1000.0 + 0.001)
        .collect()
}
