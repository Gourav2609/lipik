/// Application code that receives audio from the library.
///
/// Called on the delivery thread, never on the real-time audio thread. Rules:
///
/// - **Return promptly.** A slow sink drops audio (`Stopped::dropped_frames`);
///   one that never returns fails the recorder with `SinkStalled`, and the
///   sink is lost.
/// - **The sink comes back.** `Recorder::stop` returns it, even after a panic.
/// - **Errors are the application's.** A sink records its own errors. A panic
///   ends the recording with `EndReason::SinkPanicked` (needs `panic = "unwind"`).
///
/// The first `process_chunk` call of a recording means audio is flowing.
pub trait Sink: Send + 'static {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>);
}

/// Lets a recorder take `Box<dyn Sink>`.
impl<S: Sink + ?Sized> Sink for Box<S> {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        (**self).process_chunk(chunk);
    }
}

/// Exactly `frames_per_chunk` frames of interleaved audio.
#[derive(Debug, Clone, Copy)]
pub struct AudioChunk<'a> {
    /// Interleaved samples, `frames_per_chunk * channels` long.
    pub samples: &'a [f32],
    pub sample_rate: u32,
    pub channels: u16,
    /// Frames of real audio. Equal to `frames_per_chunk` except in the final
    /// chunk, whose remainder is zero padding.
    pub valid_frames: usize,
}

impl AudioChunk<'_> {
    /// Every real sample is exactly zero. A run of these usually means a muted
    /// or denied mic, or a headset connected to another device.
    pub fn is_digital_silence(&self) -> bool {
        let real = self.valid_frames * self.channels as usize;
        self.samples[..real].iter().all(|&s| s == 0.0)
    }
}

/// Collects a recording's real audio (padding excluded) in memory.
#[derive(Debug, Default)]
pub struct CollectingSink {
    samples: Vec<f32>,
}

impl CollectingSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// The collected interleaved samples.
    pub fn into_samples(self) -> Vec<f32> {
        self.samples
    }
}

impl Sink for CollectingSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        let real = chunk.valid_frames * chunk.channels as usize;
        self.samples.extend_from_slice(&chunk.samples[..real]);
    }
}
