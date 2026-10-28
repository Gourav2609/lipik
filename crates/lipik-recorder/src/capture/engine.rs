//! The recorder engine: start, stop, and close over three threads.
//!
//! - device: owns the platform stream and runs the watchdog. Never runs
//!   application code, so it can always close the stream.
//! - delivery (`delivery.rs`): drains the ring and runs the sink.
//! - notification: short-lived, calls the failure handler once.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle, ThreadId},
    time::{Duration, Instant},
};

use rtrb::RingBuffer;

use super::FrameResampler;
use super::delivery::{self, DeliveryCmd, DeliveryPipeline, panic_message};
use super::transport::{CaptureTransportState, Routing, write_input_to_ring};
use super::watchdog::{self, CheckTime, Watchdog};
use crate::backend::{Backend, BackendError, BackendErrorKind, InputData, InputStream};
use crate::{
    Channels, Error, ErrorKind, Format, InputDevice, Permission, RecorderConfig, RecorderInfo,
    Sink, StartError, Stopped,
};

/// Absorbs consumer stalls.
const AUDIO_RING_SECONDS: usize = 2;

/// Output rates outside this range are `UnsupportedFormat`.
const MIN_OUTPUT_RATE: u32 = 1_000;
const MAX_OUTPUT_RATE: u32 = 768_000;
/// Chunks longer than this are `UnsupportedFormat`.
const MAX_CHUNK_SECONDS: usize = 10;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Timeouts {
    /// `open` waits this long for the platform to build and start the stream.
    pub open: Duration,
    /// `close` waits this long for the platform to tear the stream down.
    pub close: Duration,
    /// `close` waits this long for the delivery thread to leave the sink.
    pub delivery_exit: Duration,
    /// A device that delivers no audio for this long after `open` fails with
    /// `NoAudio`. Bluetooth devices can take seconds.
    pub no_audio: Duration,
    /// A device that delivered audio and then made no callback for this long
    /// fails with `Stalled`.
    pub stall: Duration,
    /// A delivery thread (usually a sink) that makes no progress for this
    /// long fails the recorder with `SinkStalled`.
    pub heartbeat: Duration,
    /// `stop` waits this long for the callback to acknowledge the pause.
    pub pause_ack: Duration,
    /// `stop`'s overall deadline. Exceeds `pause_ack`, so a stall is seen first.
    pub stop: Duration,
    /// How often the device thread's watchdog checks progress.
    pub watchdog_tick: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            open: Duration::from_secs(10),
            close: Duration::from_secs(5),
            delivery_exit: Duration::from_secs(1),
            no_audio: Duration::from_secs(10),
            stall: Duration::from_secs(5),
            heartbeat: Duration::from_secs(10),
            pause_ack: Duration::from_secs(2),
            stop: Duration::from_secs(5),
            watchdog_tick: Duration::from_millis(50),
        }
    }
}

impl Timeouts {
    fn validate(&self) {
        assert!(
            self.stop > self.pause_ack,
            "stop's deadline must exceed the pause-acknowledgement bound"
        );
    }
}

/// State shared by the caller, the callback, and both threads.
pub(crate) struct Shared {
    pub transport: CaptureTransportState,
    /// The recorder's failure. First writer wins; never cleared.
    failure: OnceLock<Error>,
    /// The opened device, for error context.
    device: OnceLock<InputDevice>,
    /// When the stream started, for error context.
    started_at: OnceLock<Instant>,
    /// Advanced by the delivery thread on every loop and drain.
    pub heartbeat: AtomicU64,
    /// Xruns this recording: audio the platform lost before the callback.
    /// Logged at `stop`, not counted in `dropped_frames`.
    pub xruns: AtomicU64,
    /// When the current recording started; `None` while idle.
    recording_since: Mutex<Option<Instant>>,
    /// Held while failing on a suspension, so `stop` sees the gap or the failure.
    pub(crate) watchdog_checked_at: Mutex<CheckTime>,
    device_tx: mpsc::Sender<DeviceMsg>,
}

impl Shared {
    fn new(device_tx: mpsc::Sender<DeviceMsg>) -> Self {
        Self {
            transport: CaptureTransportState::default(),
            failure: OnceLock::new(),
            device: OnceLock::new(),
            started_at: OnceLock::new(),
            heartbeat: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            recording_since: Mutex::new(None),
            watchdog_checked_at: Mutex::new(CheckTime::now()),
            device_tx,
        }
    }

    pub(super) fn recording_since(&self) -> Option<Instant> {
        *self.recording_since.lock().unwrap()
    }

    pub fn failure(&self) -> Option<Error> {
        self.failure.get().cloned()
    }

    /// An error of `kind` carrying the device and the time into the stream.
    pub fn error(&self, kind: ErrorKind) -> Error {
        let mut error = Error::new(kind);
        if let Some(device) = self.device.get() {
            error = error.with_device(device.clone());
        }
        if let Some(started) = self.started_at.get() {
            error = error.with_elapsed(started.elapsed());
        }
        error
    }

    /// Fails the recorder, unless it already failed. Returns the recorder's
    /// failure: `error`, or the one that came first. Doesn't log: the device
    /// thread's loop logs the failure once, as it tears the stream down.
    pub fn fail(&self, error: Error) -> Error {
        if self.failure.set(error).is_ok() {
            let _ = self.device_tx.send(DeviceMsg::Failed);
        }
        self.failure.get().cloned().expect("failure was just set")
    }
}

enum DeviceMsg {
    /// A platform error from the stream's error callback.
    StreamError(BackendError),
    /// The recorder failed; tear down.
    Failed,
    /// Close: tear down and exit.
    Close,
    /// Start or stop holding the headset (`take_headset`).
    HoldHeadset(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenState {
    Pending,
    Done,
    /// `open` gave up waiting; a late stream must release itself.
    Cancelled,
}

type Handler = Box<dyn FnOnce(Error) + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Idle,
    Recording,
    /// A `stop` is collecting the recording.
    Stopping,
}

struct Threads {
    device: JoinHandle<()>,
    /// Disconnects when the device thread exits.
    device_exit: mpsc::Receiver<()>,
    delivery: JoinHandle<()>,
    /// Disconnects when the delivery thread exits.
    delivery_exit: mpsc::Receiver<()>,
    device_tx: mpsc::Sender<DeviceMsg>,
}

pub(crate) struct Engine<S> {
    pub(crate) shared: Arc<Shared>,
    backend: Arc<dyn Backend>,
    info: RecorderInfo,
    delivery_tx: mpsc::Sender<DeliveryCmd<S>>,
    /// The thread that runs the sink, where `stop` cannot work.
    delivery_thread: ThreadId,
    slot: Mutex<Slot>,
    threads: Mutex<Option<Threads>>,
    timeouts: Timeouts,
    take_headset: bool,
}

/// What the device thread hands back from a successful open.
struct Opened {
    info: RecorderInfo,
    pipeline: DeliveryPipeline,
}

impl<S: Sink> Engine<S> {
    pub fn open(
        backend: Arc<dyn Backend>,
        config: RecorderConfig,
        handler: Option<Handler>,
        timeouts: Timeouts,
    ) -> Result<Self, Error> {
        let opening = Instant::now();
        timeouts.validate();
        validate_request(&config)?;
        let take_headset = config.take_headset;

        let (device_tx, device_rx) = mpsc::channel();
        let shared = Arc::new(Shared::new(device_tx.clone()));
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<Opened, Error>>(1);
        let open_state = Arc::new(Mutex::new(OpenState::Pending));
        let (device_exit_tx, device_exit) = mpsc::channel::<()>();

        let engine_backend = Arc::clone(&backend);
        let device = {
            let shared = Arc::clone(&shared);
            let open_state = Arc::clone(&open_state);
            let error_tx = device_tx.clone();
            thread::Builder::new()
                .name("lipik-recorder-device".into())
                .spawn(move || {
                    let _exit = device_exit_tx;
                    let opened = open_stream(&*backend, &config, &shared, error_tx, &timeouts);
                    let mut state = open_state.lock().unwrap();
                    match opened {
                        Ok((stream, opened)) => {
                            if *state == OpenState::Cancelled {
                                drop(state);
                                log::warn!(
                                    "the platform finished opening a stream after open timed out; releasing it"
                                );
                                drop(stream);
                                log::debug!("released the late-opened stream");
                                return;
                            }
                            *state = OpenState::Done;
                            let _ = init_tx.send(Ok(opened));
                            drop(state);
                            run_device(&*backend, stream, shared, device_rx, timeouts, handler);
                        }
                        Err(error) => {
                            *state = OpenState::Done;
                            let _ = init_tx.send(Err(error));
                        }
                    }
                })
                .map_err(|e| {
                    Error::new(ErrorKind::Backend).with_detail(format!("cannot spawn a thread: {e}"))
                })?
        };

        let result = match init_rx.recv_timeout(timeouts.open) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let mut state = open_state.lock().unwrap();
                if *state == OpenState::Done {
                    // Finished just as the wait expired.
                    drop(state);
                    init_rx.recv().expect("the device thread sent its result")
                } else {
                    *state = OpenState::Cancelled;
                    drop(state);
                    let error = shared.error(ErrorKind::OpenTimedOut).with_detail(format!(
                        "no result within {:.1} s",
                        timeouts.open.as_secs_f64()
                    ));
                    log::warn!(
                        "{error}; the opening thread is detached and will release the stream if the platform call ever completes"
                    );
                    return Err(error);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::new(ErrorKind::Processing)
                .with_detail("the device thread exited during open")),
        };
        let Opened { info, pipeline } = match result {
            Ok(opened) => opened,
            Err(error) => {
                let _ = device.join();
                log::warn!("open failed: {error}");
                return Err(error);
            }
        };

        let (delivery_tx, delivery_rx) = mpsc::channel();
        let (delivery_exit_tx, delivery_exit) = mpsc::channel::<()>();
        let delivery = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("lipik-recorder-delivery".into())
                .spawn(move || {
                    let _exit = delivery_exit_tx;
                    // Sink panics are caught per chunk, so this one is ours.
                    let run = AssertUnwindSafe(|| delivery::run(pipeline, &shared, delivery_rx));
                    if let Err(payload) = catch_unwind(run) {
                        shared.fail(shared.error(ErrorKind::Processing).with_detail(format!(
                            "the delivery thread panicked: {}",
                            panic_message(&*payload)
                        )));
                    }
                })
        };
        let delivery = match delivery {
            Ok(delivery) => delivery,
            Err(e) => {
                let _ = device_tx.send(DeviceMsg::Close);
                return Err(shared
                    .error(ErrorKind::Backend)
                    .with_detail(format!("cannot spawn a thread: {e}")));
            }
        };

        let delivery_thread = delivery.thread().id();

        log::info!(
            "opened {} ({}) in {} ms at {} Hz, {} ch; delivering {} Hz, {} ch, {}-frame chunks",
            info.device.name,
            info.device.backend,
            opening.elapsed().as_millis(),
            info.device_format.sample_rate,
            info.device_format.channels,
            info.format.sample_rate,
            info.format.channels,
            info.frames_per_chunk
        );

        Ok(Self {
            shared,
            backend: engine_backend,
            info,
            delivery_tx,
            delivery_thread,
            slot: Mutex::new(Slot::Idle),
            threads: Mutex::new(Some(Threads {
                device,
                device_exit,
                delivery,
                delivery_exit,
                device_tx,
            })),
            timeouts,
            take_headset,
        })
    }

    pub fn info(&self) -> &RecorderInfo {
        &self.info
    }

    pub fn start(&self, sink: S) -> Result<(), StartError<S>> {
        let mut slot = self.slot.lock().unwrap();
        if *slot != Slot::Idle {
            return Err(StartError {
                error: Error::new(ErrorKind::AlreadyRecording),
                sink,
            });
        }
        if let Some(error) = self.shared.failure() {
            return Err(StartError { error, sink });
        }
        // Revoked while open; see `denial_is_silent`.
        if self.backend.denial_is_silent() && self.backend.permission_status() == Permission::Denied
        {
            let error = self.shared.fail(permission_denied(
                self.shared.error(ErrorKind::PermissionDenied),
            ));
            return Err(StartError { error, sink });
        }
        if let Err(mpsc::SendError(cmd)) = self
            .delivery_tx
            .send(DeliveryCmd::Start(sink, Instant::now()))
        {
            let DeliveryCmd::Start(sink, _) = cmd else {
                unreachable!("the unsent command is the start just sent")
            };
            let error = self.shared.fail(
                self.shared
                    .error(ErrorKind::Processing)
                    .with_detail("the delivery thread exited"),
            );
            return Err(StartError { error, sink });
        }
        *slot = Slot::Recording;
        *self.shared.recording_since.lock().unwrap() = Some(Instant::now());
        if self.take_headset {
            let _ = self.shared.device_tx.send(DeviceMsg::HoldHeadset(true));
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<Stopped<S>, Error> {
        // Waiting for the delivery thread here would wait for itself.
        if thread::current().id() == self.delivery_thread {
            log::warn!("stop called from inside the sink; the recording continues");
            return Err(Error::new(ErrorKind::StopFromSink));
        }
        {
            let mut slot = self.slot.lock().unwrap();
            if *slot != Slot::Recording {
                return Err(Error::new(ErrorKind::NotRecording));
            }
            *slot = Slot::Stopping;
        }
        watchdog::fail_if_suspended(&self.shared, &self.timeouts);
        let result = self.collect();
        if self.take_headset {
            let _ = self.shared.device_tx.send(DeviceMsg::HoldHeadset(false));
        }
        *self.shared.recording_since.lock().unwrap() = None;
        *self.slot.lock().unwrap() = Slot::Idle;
        result
    }

    fn collect(&self) -> Result<Stopped<S>, Error> {
        // A stalled sink holds the recording on an unresponsive thread.
        if let Some(error) = self.shared.failure()
            && error.kind() == ErrorKind::SinkStalled
        {
            return Err(error);
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        if self.delivery_tx.send(DeliveryCmd::Stop(reply_tx)).is_err() {
            return Err(self.shared.fail(
                self.shared
                    .error(ErrorKind::Processing)
                    .with_detail("the delivery thread exited"),
            ));
        }
        match reply_rx.recv_timeout(self.timeouts.stop) {
            Ok(stopped) => Ok(stopped),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let stalled = self
                    .shared
                    .error(ErrorKind::SinkStalled)
                    .with_detail(format!(
                        "stop did not complete within {:.1} s",
                        self.timeouts.stop.as_secs_f64()
                    ));
                let failure = self.shared.fail(stalled.clone());
                // The loop logs a first failure; this recording is lost either way.
                if failure.kind() != ErrorKind::SinkStalled {
                    log::error!("recording lost: {stalled}");
                }
                Err(failure)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(self.shared.failure().unwrap_or_else(|| {
                    self.shared.fail(
                        self.shared
                            .error(ErrorKind::Processing)
                            .with_detail("the delivery thread exited"),
                    )
                }))
            }
        }
    }
}

impl<S> Engine<S> {
    /// Close: discard any recording, tear the stream down, and wait (bounded)
    /// for both threads. Idempotent.
    pub fn shutdown(&self) -> Result<(), Error> {
        let Some(threads) = self.threads.lock().unwrap().take() else {
            return Ok(());
        };
        let _ = self.delivery_tx.send(DeliveryCmd::Shutdown);
        let _ = threads.device_tx.send(DeviceMsg::Close);
        let current = thread::current().id();

        let result = match threads.device_exit.recv_timeout(self.timeouts.close) {
            Err(mpsc::RecvTimeoutError::Disconnected) | Ok(()) => {
                let _ = threads.device.join();
                Ok(())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let error = self
                    .shared
                    .error(ErrorKind::CloseTimedOut)
                    .with_detail(format!(
                        "the platform did not release the stream within {:.1} s",
                        self.timeouts.close.as_secs_f64()
                    ));
                log::warn!(
                    "{error}; the device thread is detached and the stream may stay open until the process exits"
                );
                Err(error)
            }
        };

        // Dropped from inside the sink: don't wait for ourselves.
        if threads.delivery.thread().id() == current {
            log::debug!("closed from inside the sink; the delivery thread exits after it returns");
        } else {
            match threads
                .delivery_exit
                .recv_timeout(self.timeouts.delivery_exit)
            {
                Err(mpsc::RecvTimeoutError::Disconnected) | Ok(()) => {
                    let _ = threads.delivery.join();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    log::warn!("the delivery thread is still inside the sink; it is detached")
                }
            }
        }
        if result.is_ok() {
            log::debug!("recorder closed");
        }
        result
    }
}

/// Checks what can be checked before knowing the device.
fn validate_request(config: &RecorderConfig) -> Result<(), Error> {
    if let Some(rate) = config.sample_rate
        && !(MIN_OUTPUT_RATE..=MAX_OUTPUT_RATE).contains(&rate)
    {
        return Err(
            Error::new(ErrorKind::UnsupportedFormat).with_detail(format!(
                "sample_rate {rate} Hz is outside {MIN_OUTPUT_RATE}..={MAX_OUTPUT_RATE} Hz"
            )),
        );
    }
    if config.frames_per_chunk == Some(0) {
        return Err(Error::new(ErrorKind::UnsupportedFormat)
            .with_detail("frames_per_chunk must be at least 1"));
    }
    Ok(())
}

/// The default chunk: about 10 ms.
fn default_frames_per_chunk(sample_rate: u32) -> usize {
    ((sample_rate as usize + 50) / 100).max(1)
}

/// Adds where to grant access to the platform's message.
fn permission_denied(error: Error) -> Error {
    let hint = if cfg!(target_os = "windows") {
        "microphone access is denied for desktop apps (Settings > Privacy & security > Microphone: \
         \"Microphone access\", \"Let apps access your microphone\", and \"Let desktop apps access \
         your microphone\" must all be on)"
    } else {
        "microphone access is denied for this app (macOS: System Settings > Privacy & Security > Microphone)"
    };
    let detail = match error.detail() {
        Some(platform) => format!("{hint}: {platform}"),
        None => hint.to_owned(),
    };
    error.with_detail(detail)
}

/// Maps a platform error during open, or while listing devices.
pub(crate) fn open_error(error: BackendError, device: Option<&InputDevice>) -> Error {
    let kind = match error.kind {
        BackendErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        BackendErrorKind::DeviceNotAvailable => ErrorKind::DeviceUnavailable,
        BackendErrorKind::DeviceBusy => ErrorKind::DeviceBusy,
        _ => ErrorKind::Backend,
    };
    let mut error = Error::new(kind).with_detail(error.message.into_owned());
    if let Some(device) = device {
        error = error.with_device(device.clone());
    }
    if kind == ErrorKind::PermissionDenied {
        error = permission_denied(error);
    }
    error
}

/// Maps a platform error reported while the stream runs.
fn runtime_error(backend: &dyn Backend, shared: &Shared, error: BackendError) -> Error {
    let kind = match error.kind {
        BackendErrorKind::DeviceNotAvailable => ErrorKind::DeviceLost,
        BackendErrorKind::StreamInvalidated | BackendErrorKind::DeviceChanged => {
            ErrorKind::StreamInvalidated
        }
        BackendErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        _ => ErrorKind::Backend,
    };
    // WASAPI fails a revoked stream with the unplug code; relabel only that.
    let kind = if kind == ErrorKind::DeviceLost
        && !backend.denial_is_silent()
        && backend.permission_status() == Permission::Denied
    {
        ErrorKind::PermissionDenied
    } else {
        kind
    };
    let error = shared.error(kind).with_detail(error.message.into_owned());
    if kind == ErrorKind::PermissionDenied {
        permission_denied(error)
    } else {
        error
    }
}

/// Runs on the device thread: resolves the device, builds the pipeline, and
/// starts the stream.
fn open_stream(
    backend: &dyn Backend,
    config: &RecorderConfig,
    shared: &Arc<Shared>,
    error_tx: mpsc::Sender<DeviceMsg>,
    timeouts: &Timeouts,
) -> Result<(Box<dyn InputStream>, Opened), Error> {
    let device = backend
        .open_device(config.device.as_deref())
        .map_err(|e| open_error(e, None))?;
    let info = device.info().clone();
    let _ = shared.device.set(info.clone());
    // See `denial_is_silent`.
    if backend.denial_is_silent() && backend.permission_status() == Permission::Denied {
        return Err(permission_denied(shared.error(ErrorKind::PermissionDenied)));
    }
    let device_format = device.format();
    let channels = device_format.channels as usize;
    let in_rate = device_format.sample_rate;
    if channels == 0 || in_rate == 0 {
        return Err(shared.error(ErrorKind::Backend).with_detail(format!(
            "the device reports {channels} channels at {in_rate} Hz"
        )));
    }

    let routing = match config.channels {
        Channels::All => Routing::All,
        Channels::MixToMono => Routing::MixToMono,
        Channels::Only(n) if (n as usize) < channels => Routing::Only(n as usize),
        Channels::Only(n) => {
            return Err(shared.error(ErrorKind::InvalidChannel).with_detail(format!(
                "channel {n} requested; the device has {channels} (numbered from 0)"
            )));
        }
    };
    let k = routing.output_channels(channels);
    let out_rate = config.sample_rate.unwrap_or(in_rate);
    let frames_per_chunk = config
        .frames_per_chunk
        .unwrap_or_else(|| default_frames_per_chunk(out_rate));
    if frames_per_chunk > out_rate as usize * MAX_CHUNK_SECONDS {
        return Err(shared.error(ErrorKind::UnsupportedFormat).with_detail(format!(
            "frames_per_chunk {frames_per_chunk} is more than {MAX_CHUNK_SECONDS} s at {out_rate} Hz"
        )));
    }
    let resampler = FrameResampler::new(in_rate as usize, out_rate as usize, frames_per_chunk, k)
        .map_err(|e| {
        shared
            .error(ErrorKind::UnsupportedFormat)
            .with_detail(format!(
                "cannot resample {in_rate} Hz to {out_rate} Hz: {e}"
            ))
    })?;

    // Whole K-sample frames, so reads and writes stay aligned across the wrap.
    let ring_capacity = in_rate as usize * AUDIO_RING_SECONDS * k;
    let (mut producer, mut consumer) = RingBuffer::new(ring_capacity);

    // Pre-fault the ring's pages before the callback runs.
    {
        let chunk = producer
            .write_chunk(ring_capacity)
            .expect("new audio ring has its full capacity available");
        chunk.commit_all();
    }
    {
        let chunk = consumer
            .read_chunk(ring_capacity)
            .expect("pre-filled audio ring is readable");
        chunk.commit_all();
    }

    let callback_shared = Arc::clone(shared);
    let data = move |data: InputData<'_>| {
        let producer = &mut producer;
        let transport = &callback_shared.transport;
        match data {
            InputData::U8(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::I8(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::U16(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::I16(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::U24(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::I24(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::U32(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::I32(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::U64(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::I64(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::F32(d) => write_input_to_ring(d, channels, routing, producer, transport),
            InputData::F64(d) => write_input_to_ring(d, channels, routing, producer, transport),
        }
    };
    let error_shared = Arc::clone(shared);
    let error = move |error: BackendError| {
        // May run on the audio thread: count survivable errors, send the rest.
        if error.kind.stream_survives() {
            // WASAPI flags a discontinuity before any audio; nothing is missing.
            let delivered = error_shared.transport.callbacks.load(Ordering::Relaxed) > 0;
            if error.kind == BackendErrorKind::Xrun && delivered {
                error_shared.xruns.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            let _ = error_tx.send(DeviceMsg::StreamError(error));
        }
    };

    let started = Instant::now();
    let stream = device
        .start(Box::new(data), Box::new(error))
        .map_err(|e| open_error(e, Some(&info)))?;
    let _ = shared.started_at.set(Instant::now());
    log::debug!("stream built and started in {:?}", started.elapsed());

    let format = Format {
        sample_rate: out_rate,
        channels: k as u16,
    };
    Ok((
        stream,
        Opened {
            info: RecorderInfo {
                device: info,
                device_format: Format {
                    sample_rate: in_rate,
                    channels: device_format.channels,
                },
                format,
                frames_per_chunk,
            },
            pipeline: DeliveryPipeline {
                consumer,
                resampler,
                in_sample_rate: in_rate,
                channels: k,
                out_sample_rate: out_rate,
                pause_ack_timeout: timeouts.pause_ack,
            },
        },
    ))
}

/// The device thread after a successful open.
fn run_device(
    backend: &dyn Backend,
    stream: Box<dyn InputStream>,
    shared: Arc<Shared>,
    device_rx: mpsc::Receiver<DeviceMsg>,
    timeouts: Timeouts,
    mut handler: Option<Handler>,
) {
    let mut stream = Some(stream);
    let supervise = AssertUnwindSafe(|| {
        supervise(
            backend,
            &mut stream,
            &shared,
            &device_rx,
            &timeouts,
            &mut handler,
        )
    });
    let Err(payload) = catch_unwind(supervise) else {
        return;
    };
    // The watchdog died with it, so fail now.
    let first = shared.failure().is_none();
    let error = shared.fail(shared.error(ErrorKind::Processing).with_detail(format!(
        "the device thread panicked: {}",
        panic_message(&*payload)
    )));
    // A live stream means the loop hadn't logged an earlier failure yet.
    if first || stream.is_some() {
        log::error!("recorder failed: {error}");
    }
    if catch_unwind(AssertUnwindSafe(|| teardown(&mut stream))).is_err() {
        log::error!("releasing the stream panicked");
    }
    if let Some(handler) = handler.take() {
        notify(handler, error);
    }
}

/// The device thread's loop, until close.
fn supervise(
    backend: &dyn Backend,
    stream: &mut Option<Box<dyn InputStream>>,
    shared: &Shared,
    device_rx: &mpsc::Receiver<DeviceMsg>,
    timeouts: &Timeouts,
    handler: &mut Option<Handler>,
) {
    let mut watchdog = Watchdog::new(shared);
    loop {
        match device_rx.recv_timeout(timeouts.watchdog_tick) {
            Ok(DeviceMsg::StreamError(error)) => {
                if stream.is_some() {
                    let error = runtime_error(backend, shared, error);
                    shared.fail(error);
                }
            }
            Ok(DeviceMsg::HoldHeadset(hold)) => {
                if let Some(stream) = stream.as_mut() {
                    stream.hold_headset(hold);
                }
            }
            Ok(DeviceMsg::Failed) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(DeviceMsg::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                teardown(stream);
                return;
            }
        }

        if stream.is_some() && shared.failure.get().is_none() {
            watchdog.check(shared, timeouts);
        }

        if let Some(open) = stream.as_mut()
            && shared.failure.get().is_none()
            && let Err(error) = open.check_device()
        {
            shared.fail(runtime_error(backend, shared, error));
        }

        if let Some(error) = shared.failure()
            && stream.is_some()
        {
            log::log!(error.kind().failure_level(), "recorder failed: {error}");
            teardown(stream);
            if let Some(handler) = handler.take() {
                notify(handler, error);
            }
        }
    }
}

fn teardown(stream: &mut Option<Box<dyn InputStream>>) {
    if let Some(stream) = stream.take() {
        let started = Instant::now();
        drop(stream);
        log::debug!("stream released in {:?}", started.elapsed());
    }
}

/// Calls the handler on its own thread, so a slow one delays nothing else.
fn notify(handler: Handler, error: Error) {
    let spawned = thread::Builder::new()
        .name("lipik-recorder-notify".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(move || handler(error)));
            if result.is_err() {
                log::error!("the failure handler panicked");
            }
        });
    if let Err(e) = spawned {
        log::error!("cannot spawn the failure notification thread: {e}");
    }
}
