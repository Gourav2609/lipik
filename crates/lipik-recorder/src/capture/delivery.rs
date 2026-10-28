//! The delivery thread: drains the ring, resamples, cuts exact chunks, and
//! runs the application's sink.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

use rtrb::Consumer;

use super::FrameResampler;
use super::engine::Shared;
use super::transport::drain_available_samples;
use crate::{AudioChunk, EndReason, Error, ErrorKind, Sink, Stopped};

const CONSUMER_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_DRAIN_CHUNK: Duration = Duration::from_millis(50);

pub(crate) enum DeliveryCmd<S> {
    /// Begin a recording into this sink. Carries the send time for logging.
    Start(S, Instant),
    /// End the recording and send it back.
    Stop(mpsc::Sender<Stopped<S>>),
    /// Discard any recording and exit.
    Shutdown,
}

/// What to do with a chunk drained from the ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChunkDisposition {
    /// Process as active recording audio, including during the final stop drain.
    Capture,
    /// Consume idle audio without processing it.
    Discard,
}

/// Everything the delivery thread needs, built and validated at open.
pub(crate) struct DeliveryPipeline {
    pub consumer: Consumer<f32>,
    pub resampler: FrameResampler,
    /// Device rate: the ring's rate.
    pub in_sample_rate: u32,
    /// K: samples per frame in the ring and in every chunk.
    pub channels: usize,
    pub out_sample_rate: u32,
    pub pause_ack_timeout: Duration,
}

pub(crate) fn run<S: Sink>(
    pipeline: DeliveryPipeline,
    shared: &Arc<Shared>,
    cmd_rx: mpsc::Receiver<DeliveryCmd<S>>,
) {
    let DeliveryPipeline {
        mut consumer,
        resampler,
        in_sample_rate,
        channels,
        out_sample_rate,
        pause_ack_timeout,
    } = pipeline;
    let max_drain_samples =
        ((in_sample_rate as u128 * MAX_DRAIN_CHUNK.as_millis()) / 1_000).max(1) as usize * channels;
    let mut processor = Processor {
        shared: Arc::clone(shared),
        resampler,
        channels,
        in_sample_rate,
        out_sample_rate,
        max_drain_samples,
        pause_ack_timeout,
        stream_running_at: Instant::now(),
        first_audio_logged: false,
        active: None,
    };

    loop {
        processor.beat();
        #[cfg(test)]
        if processor
            .shared
            .transport
            .panic_delivery
            .swap(false, Ordering::AcqRel)
        {
            panic!("injected delivery thread panic");
        }
        // Check commands before each bounded drain, so Stop can't wait behind a backlog.
        let mut command = if consumer.slots() > 0 {
            match cmd_rx.try_recv() {
                Ok(command) => Some(command),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        } else {
            match cmd_rx.recv_timeout(CONSUMER_POLL_INTERVAL) {
                Ok(command) => Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        };

        loop {
            if let Some(cmd) = command.take() {
                match cmd {
                    DeliveryCmd::Start(sink, sent_at) => {
                        processor.begin(sink, sent_at, consumer.slots() > 0)
                    }
                    DeliveryCmd::Stop(reply_tx) => {
                        if let Some(stopped) = processor.stop(&mut consumer) {
                            let _ = reply_tx.send(stopped);
                        }
                    }
                    DeliveryCmd::Shutdown => {
                        processor
                            .shared
                            .transport
                            .pause_requested
                            .store(true, Ordering::Release);
                        if processor.active.take().is_some() {
                            log::debug!("recorder closed during a recording; discarded it");
                        }
                        return;
                    }
                }
            }

            command = match cmd_rx.try_recv() {
                Ok(command) => Some(command),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            };
        }

        // A failure ends the recording here; it then waits for its `stop`.
        if let Some(error) = processor.shared.failure()
            && processor.capturing()
        {
            processor.end_on_failure(&mut consumer, error);
        }

        let disposition = if processor.capturing() {
            ChunkDisposition::Capture
        } else {
            ChunkDisposition::Discard
        };
        let _drained = processor.drain(&mut consumer, disposition);
        #[cfg(test)]
        processor
            .shared
            .transport
            .frames_drained
            .fetch_add(_drained / channels, Ordering::Release);

        let overrun_frames = processor
            .shared
            .transport
            .overrun_frames
            .swap(0, Ordering::AcqRel);
        if processor.capturing() {
            processor.observe_overrun(overrun_frames);
        }
    }
}

/// One recording in progress: the lent sink and its statistics.
struct Active<S> {
    sink: S,
    /// Set when the recording ended before `stop`: a failure or a sink panic.
    ended: Option<EndReason>,
    dropped_frames: u64,
    overrun_episodes: u32,
    started: Instant,
    first_chunk_after: Option<Duration>,
    /// Frames drained from the ring into this recording.
    input_frames: u64,
    /// Real frames delivered to the sink (the sum of `valid_frames`).
    output_frames: u64,
    /// Exact-zero frames, for the stop log. See `AudioChunk::is_digital_silence`.
    zero_run: u64,
    longest_zero_run: u64,
    heard_nonzero: bool,
}

impl<S> Active<S> {
    fn observe_silence(&mut self, raw: &[f32], channels: usize) {
        for frame in raw.chunks_exact(channels) {
            if frame.iter().all(|&s| s == 0.0) {
                self.zero_run += 1;
            } else {
                self.longest_zero_run = self.longest_zero_run.max(self.zero_run);
                self.zero_run = 0;
                self.heard_nonzero = true;
            }
        }
    }
}

struct Processor<S> {
    shared: Arc<Shared>,
    resampler: FrameResampler,
    channels: usize,
    in_sample_rate: u32,
    out_sample_rate: u32,
    max_drain_samples: usize,
    pause_ack_timeout: Duration,
    stream_running_at: Instant,
    first_audio_logged: bool,
    active: Option<Active<S>>,
}

impl<S: Sink> Processor<S> {
    fn beat(&self) {
        self.shared.heartbeat.fetch_add(1, Ordering::Relaxed);
    }

    /// Whether drained audio belongs to a recording.
    fn capturing(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.ended.is_none())
    }

    fn begin(&mut self, sink: S, sent_at: Instant, audio_queued: bool) {
        log::debug!(
            "start processed {:?} after it was sent; the recording begins with {} audio",
            sent_at.elapsed(),
            if audio_queued {
                "the queued"
            } else {
                "the next"
            }
        );
        // Overruns while idle don't count.
        self.shared
            .transport
            .overrun_frames
            .store(0, Ordering::Release);
        self.shared.xruns.store(0, Ordering::Relaxed);
        self.resampler.reset();
        self.active = Some(Active {
            sink,
            // A failure that raced `start` ends the recording at once.
            ended: self.shared.failure().map(EndReason::RecorderFailed),
            dropped_frames: 0,
            overrun_episodes: 0,
            started: Instant::now(),
            first_chunk_after: None,
            input_frames: 0,
            output_frames: 0,
            zero_run: 0,
            longest_zero_run: 0,
            heard_nonzero: false,
        });
        #[cfg(test)]
        self.shared
            .transport
            .starts_applied
            .fetch_add(1, Ordering::Release);
    }

    /// Drains up to one bounded chunk. Returns the samples consumed.
    fn drain(&mut self, consumer: &mut Consumer<f32>, disposition: ChunkDisposition) -> usize {
        self.drain_at_most(consumer, disposition, self.max_drain_samples)
    }

    fn drain_at_most(
        &mut self,
        consumer: &mut Consumer<f32>,
        disposition: ChunkDisposition,
        max_samples: usize,
    ) -> usize {
        self.beat();
        let max_samples = max_samples.min(self.max_drain_samples);
        let channels = self.channels;
        drain_available_samples(consumer, max_samples, channels, |raw| {
            self.process_raw_chunk(raw, disposition)
        })
    }

    fn process_raw_chunk(&mut self, raw: &[f32], disposition: ChunkDisposition) {
        if !self.first_audio_logged {
            self.first_audio_logged = true;
            log::debug!(
                "first audio arrived {:?} after the stream started",
                self.stream_running_at.elapsed()
            );
        }

        if disposition == ChunkDisposition::Discard {
            return;
        }
        let Some(active) = self.active.as_mut() else {
            return;
        };
        active.input_frames += (raw.len() / self.channels) as u64;
        active.observe_silence(raw, self.channels);
        let format = (self.out_sample_rate, self.channels as u16);
        let result = self.resampler.push(raw, |samples, valid| {
            deliver(active, samples, valid, format)
        });
        if let Err(e) = result {
            self.fail_processing(e.to_string());
        }
    }

    /// Counts frames the ring dropped. Warns once per recording.
    fn observe_overrun(&mut self, frames: u64) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if frames == 0 {
            return;
        }
        active.dropped_frames = active.dropped_frames.saturating_add(frames);
        active.overrun_episodes += 1;
        if active.overrun_episodes == 1 {
            log::warn!(
                "microphone capture ring dropped {frames} frames; continuing the active recording"
            );
        }
    }

    /// A resampling failure is a library bug; the recorder fails.
    fn fail_processing(&mut self, detail: String) {
        let error = self
            .shared
            .fail(self.shared.error(ErrorKind::Processing).with_detail(detail));
        if let Some(active) = self.active.as_mut() {
            active.ended.get_or_insert(EndReason::RecorderFailed(error));
        }
    }

    fn finish_tail(&mut self) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let format = (self.out_sample_rate, self.channels as u16);
        let result = self
            .resampler
            .finish(|samples, valid| deliver(active, samples, valid, format));
        if let Err(e) = result {
            self.fail_processing(e.to_string());
        }
    }

    /// Drains only what's in the ring now: a failed stream may still be writing.
    fn drain_all(&mut self, consumer: &mut Consumer<f32>) {
        let mut remaining = consumer.slots();
        while remaining > 0 {
            let drained = self.drain_at_most(consumer, ChunkDisposition::Capture, remaining);
            if drained == 0 {
                break;
            }
            remaining -= drained;
        }
        let overrun = self
            .shared
            .transport
            .overrun_frames
            .swap(0, Ordering::AcqRel);
        self.observe_overrun(overrun);
    }

    fn end_on_failure(&mut self, consumer: &mut Consumer<f32>, error: Error) {
        self.drain_all(consumer);
        self.finish_tail();
        if let Some(active) = self.active.as_mut() {
            active.ended.get_or_insert(EndReason::RecorderFailed(error));
        }
    }

    /// Ends the recording and hands it back. `None` if there is none.
    fn stop(&mut self, consumer: &mut Consumer<f32>) -> Option<Stopped<S>> {
        debug_assert!(self.active.is_some(), "Stop without a recording");
        self.active.as_ref()?;
        if self.capturing() {
            let overrun = self
                .shared
                .transport
                .overrun_frames
                .swap(0, Ordering::AcqRel);
            self.observe_overrun(overrun);

            // Pause after one boundary block and drain up to the acknowledgement.
            // A failed stream never acknowledges.
            let transport = &self.shared.transport;
            let mut acknowledged = false;
            if self.shared.failure().is_none() {
                transport.pause_acknowledged.store(false, Ordering::Relaxed);
                transport.pause_requested.store(true, Ordering::Release);
                let pause_started = Instant::now();
                loop {
                    if self
                        .shared
                        .transport
                        .pause_acknowledged
                        .load(Ordering::Acquire)
                    {
                        acknowledged = true;
                        break;
                    }
                    if self.shared.failure().is_some() {
                        break;
                    }
                    if pause_started.elapsed() >= self.pause_ack_timeout {
                        // Callbacks have stopped: the stream stalled.
                        let error = self.shared.error(ErrorKind::Stalled).with_detail(format!(
                            "the audio callback did not run for {:.1} s while stopping",
                            self.pause_ack_timeout.as_secs_f64()
                        ));
                        self.shared.fail(error);
                        break;
                    }
                    let drained = self.drain(consumer, ChunkDisposition::Capture);
                    if drained == 0 {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }

            // Everything left, boundary block included, is this recording's.
            self.drain_all(consumer);
            self.finish_tail();

            if acknowledged {
                // Resume now so the next recording doesn't lose its first block.
                let transport = &self.shared.transport;
                transport.pause_acknowledged.store(false, Ordering::Relaxed);
                transport.pause_requested.store(false, Ordering::Release);
            }
        }

        let failure = self.shared.failure();
        let mut active = self.active.take()?;
        let longest_zero_run = active.longest_zero_run.max(active.zero_run);
        let end_reason = active.ended.take().unwrap_or(match failure {
            Some(error) => EndReason::RecorderFailed(error),
            None => EndReason::StopCalled,
        });
        let rate = self.in_sample_rate as f64;
        if active.input_frames > 0 && !active.heard_nonzero {
            log::warn!(
                "the whole recording ({:.1} s) was exact digital silence: microphone access denied, a muted device, or a stale stream",
                active.input_frames as f64 / rate
            );
        } else if longest_zero_run >= self.in_sample_rate as u64 {
            // Debug: noise-gated and Bluetooth mics send exact zeros between words.
            log::debug!(
                "the recording contained {:.1} s of exact digital silence: a muted device or a stalled stream",
                longest_zero_run as f64 / rate
            );
        }
        // Logged only; `is_complete` doesn't see xruns.
        let xruns = self.shared.xruns.swap(0, Ordering::Relaxed);
        if xruns > 0 {
            log::warn!(
                "the platform reported {xruns} xruns during the recording: audio may be missing \
                 before the library received it, not counted in dropped_frames"
            );
        }
        log_stop(&active, &end_reason);
        Some(Stopped {
            sink: active.sink,
            end_reason,
            dropped_frames: active.dropped_frames,
        })
    }
}

/// One line per recording: info when complete, warn when it ended early or
/// dropped audio (a failure is also logged where it happened).
fn log_stop<S>(active: &Active<S>, end_reason: &EndReason) {
    let elapsed = active.started.elapsed().as_secs_f64();
    let first = match active.first_chunk_after {
        Some(after) => format!("first audio after {} ms", after.as_millis()),
        None => "no audio".to_owned(),
    };
    let dropped = if active.dropped_frames > 0 {
        format!(
            ", {} dropped in {} overruns",
            active.dropped_frames, active.overrun_episodes
        )
    } else {
        String::new()
    };
    let details = format!("{first}, {} frames{dropped}", active.output_frames);
    match end_reason {
        EndReason::StopCalled if active.dropped_frames == 0 => {
            log::info!("recording stopped after {elapsed:.1} s: {details}");
        }
        EndReason::StopCalled => {
            log::warn!("recording stopped after {elapsed:.1} s with audio dropped: {details}");
        }
        EndReason::RecorderFailed(error) => {
            let kind = error.kind();
            log::warn!("recording ended early after {elapsed:.1} s ({kind:?}): {details}");
        }
        EndReason::SinkPanicked(_) => {
            log::warn!("recording ended early after {elapsed:.1} s (SinkPanicked): {details}");
        }
    }
}

/// Hands one chunk to the sink. A panic ends the recording.
fn deliver<S: Sink>(active: &mut Active<S>, samples: &[f32], valid: usize, format: (u32, u16)) {
    if active.ended.is_some() {
        return;
    }
    let chunk = AudioChunk {
        samples,
        sample_rate: format.0,
        channels: format.1,
        valid_frames: valid,
    };
    let sink = &mut active.sink;
    match catch_unwind(AssertUnwindSafe(|| sink.process_chunk(chunk))) {
        Ok(()) => {
            if active.first_chunk_after.is_none() {
                active.first_chunk_after = Some(active.started.elapsed());
            }
            active.output_frames += valid as u64;
        }
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            log::error!("the sink panicked; ending the recording: {message}");
            active.ended = Some(EndReason::SinkPanicked(message));
        }
    }
}

pub(super) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}
