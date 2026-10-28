//! Cross-platform microphone capture.
//!
//! Lipik Recorder owns the real-time audio path. It gives applications audio
//! in the format they asked for, on an ordinary thread, reporting when audio
//! was lost or the stream broke.
//!
//! # Concepts
//!
//! - A [`Recorder`] is an open microphone; `start` and `stop` bracket each
//!   recording.
//! - A [`Sink`] is application code that receives the recording's audio as
//!   fixed-size [`AudioChunk`]s. It is lent to the library by `start` and
//!   handed back by `stop`.
//! - Errors: match on [`ErrorKind`]; its docs say what to do for each.
//!
//! # Example
//!
//! ```no_run
//! use lipik_recorder::{CollectingSink, Recorder, RecorderConfig};
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), lipik_recorder::Error> {
//! let recorder = Recorder::open_with_failure_handler(RecorderConfig::speech(), |error| {
//!     // Runs on a library thread. `stop` still returns the audio captured so far.
//!     eprintln!("microphone failed: {error}");
//! })?;
//!
//! recorder.start(CollectingSink::new())?;
//! std::thread::sleep(Duration::from_secs(5));
//!
//! let stopped = recorder.stop()?;
//! if !stopped.is_complete() {
//!     // The mic failed or audio was dropped. What was captured is still here.
//!     eprintln!("incomplete recording: {:?}", stopped.end_reason);
//! }
//! let samples = stopped.sink.into_samples();
//! # Ok(())
//! # }
//! ```

mod backend;
mod capture;
mod device;
mod error;
mod sink;
mod types;

#[cfg(feature = "test-backend")]
#[doc(hidden)]
pub mod testing;
#[cfg(test)]
mod tests;

pub use device::{InputDevice, Permission, list_input_devices, permission_status};
pub use error::{Error, ErrorKind};
pub use sink::{AudioChunk, CollectingSink, Sink};
pub use types::{Channels, EndReason, Format, RecorderConfig, RecorderInfo};

use std::fmt;

use backend::cpal::CpalBackend;
use capture::engine::{Engine, Timeouts};

/// An open microphone, recording into sinks of type `S`. Owned, `Send`,
/// `Sync`, lifetime-free: store it in a struct, or share it with an `Arc`.
///
/// Every recording uses the same sink type; use an enum or `Box<dyn Sink>`
/// for several kinds.
///
/// Dropping it is `close`.
pub struct Recorder<S> {
    pub(crate) engine: Engine<S>,
}

impl<S: Sink> Recorder<S> {
    /// Opens the device at its OS format. Audio may take seconds to flow
    /// (Bluetooth); the sink's first chunk marks it. A device that never
    /// delivers fails with `NoAudio`.
    ///
    /// Without a failure handler, failures surface only at the next `start` or
    /// `stop`; prefer [`open_with_failure_handler`](Self::open_with_failure_handler).
    pub fn open(config: RecorderConfig) -> Result<Self, Error> {
        Ok(Self {
            engine: Engine::open(CpalBackend::shared(), config, None, Timeouts::default())?,
        })
    }

    /// `open`, plus a handler called the moment this recorder fails.
    pub fn open_with_failure_handler(
        config: RecorderConfig,
        handler: impl FnOnce(Error) + Send + 'static,
    ) -> Result<Self, Error> {
        Ok(Self {
            engine: Engine::open(
                CpalBackend::shared(),
                config,
                Some(Box::new(handler)),
                Timeouts::default(),
            )?,
        })
    }

    /// What was opened: the device, the format it runs at, and the format
    /// the sink receives.
    pub fn info(&self) -> &RecorderInfo {
        self.engine.info()
    }

    /// Starts a recording into `sink`. Never waits for audio or touches the
    /// device. On error the sink comes back.
    pub fn start(&self, sink: S) -> Result<(), StartError<S>> {
        self.engine.start(sink)
    }

    /// Ends the recording and returns the sink, even after a failure or sink
    /// panic; `end_reason` says what ended it. A recording that ended on its
    /// own still needs this `stop` before the next `start`.
    ///
    /// Errors: `NotRecording`, `StopFromSink` (the recording continues), or the
    /// sink is lost (usually `SinkStalled`).
    pub fn stop(&self) -> Result<Stopped<S>, Error> {
        self.engine.stop()
    }

    /// Closes the microphone. An active recording is discarded; call `stop`
    /// first to keep it.
    pub fn close(self) -> Result<(), Error> {
        self.engine.shutdown()
    }
}

impl<S> Drop for Recorder<S> {
    fn drop(&mut self) {
        if let Err(error) = self.engine.shutdown() {
            log::warn!("closing the recorder on drop failed: {error}");
        }
    }
}

/// `start` failed. The sink is handed back.
pub struct StartError<S> {
    /// `AlreadyRecording`, or the error the recorder failed with.
    pub error: Error,
    pub sink: S,
}

/// For callers that do not need the sink back: `?` drops it.
impl<S> From<StartError<S>> for Error {
    fn from(e: StartError<S>) -> Error {
        e.error
    }
}

impl<S> fmt::Debug for StartError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StartError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl<S> fmt::Display for StartError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot start recording: {}", self.error)
    }
}

impl<S> std::error::Error for StartError<S> {}

/// What `stop` returns: the sink, what ended the recording, and what was lost.
#[derive(Debug)]
pub struct Stopped<S> {
    pub sink: S,
    pub end_reason: EndReason,
    /// Frames lost because the ring was full, usually a slow sink. Counted at
    /// the device's rate, before resampling.
    pub dropped_frames: u64,
}

impl<S> Stopped<S> {
    /// `StopCalled` and no dropped frames.
    pub fn is_complete(&self) -> bool {
        matches!(self.end_reason, EndReason::StopCalled) && self.dropped_frames == 0
    }
}

// Public handles are `Send`, and the recorder is also `Sync`.
const _: () = {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}
    assert_send::<Recorder<CollectingSink>>();
    assert_sync::<Recorder<CollectingSink>>();
    assert_send::<Stopped<CollectingSink>>();
    assert_send::<StartError<CollectingSink>>();
    assert_send::<Error>();
};
