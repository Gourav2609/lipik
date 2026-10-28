//! A fake microphone for bindings' test suites, behind the `test-backend`
//! feature. Not for applications, and not covered by semver.
//!
//! The caller plays the platform's audio thread: [`FakeMic::push`] delivers
//! one callback block to the running stream, on the calling thread.

use std::sync::Arc;
use std::time::Duration;

use crate::backend::fake::{FakeBackend, Gate};
use crate::backend::{BackendError, BackendErrorKind, DeviceFormat, SampleFormat};
use crate::capture::engine::{Engine, Timeouts};
use crate::{Error, Recorder, RecorderConfig, Sink};

/// One fake device with f32 samples. Clones share the device.
#[derive(Clone)]
pub struct FakeMic {
    backend: FakeBackend,
}

/// A platform error, for [`FakeMic::fail_next_open`] or
/// [`FakeMic::report_error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeError {
    /// The device is gone: `DeviceUnavailable` at open, `DeviceLost` while
    /// running.
    DeviceNotAvailable,
    DeviceBusy,
    PermissionDenied,
    /// `Backend`.
    Other,
}

impl FakeError {
    fn backend_error(self) -> BackendError {
        let kind = match self {
            Self::DeviceNotAvailable => BackendErrorKind::DeviceNotAvailable,
            Self::DeviceBusy => BackendErrorKind::DeviceBusy,
            Self::PermissionDenied => BackendErrorKind::PermissionDenied,
            Self::Other => BackendErrorKind::Other,
        };
        BackendError::new(kind, "reported by the fake microphone")
    }
}

/// A platform call held until released, like a driver that hangs.
#[derive(Clone, Debug)]
pub struct Held(Gate);

impl Held {
    pub fn release(&self) {
        self.0.open();
    }
}

impl FakeMic {
    /// A device running at `sample_rate` with `channels` f32 channels.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            backend: FakeBackend::new(DeviceFormat {
                sample_rate,
                channels,
                sample_format: SampleFormat::F32,
            }),
        }
    }

    /// Opens a recorder on this device. Open and close time out after
    /// `open_timeout`; no-audio and stall detection are off (a minute), so
    /// a test decides when audio arrives.
    pub fn open<S: Sink>(
        &self,
        config: RecorderConfig,
        handler: Option<Box<dyn FnOnce(Error) + Send + 'static>>,
        open_timeout: Duration,
    ) -> Result<Recorder<S>, Error> {
        let timeouts = Timeouts {
            open: open_timeout,
            close: open_timeout,
            delivery_exit: Duration::from_millis(500),
            no_audio: Duration::from_secs(60),
            stall: Duration::from_secs(60),
            heartbeat: Duration::from_secs(60),
            pause_ack: Duration::from_millis(500),
            stop: Duration::from_secs(2),
            watchdog_tick: Duration::from_millis(5),
        };
        Ok(Recorder {
            engine: Engine::open(Arc::new(self.backend.clone()), config, handler, timeouts)?,
        })
    }

    /// Delivers one block of interleaved samples. False if no stream runs.
    pub fn push(&self, samples: &[f32]) -> bool {
        self.backend.push(samples)
    }

    /// Reports a stream error. False if no stream runs.
    pub fn report_error(&self, error: FakeError) -> bool {
        self.backend.report_error(error.backend_error())
    }

    /// Makes the next open fail with `error`.
    pub fn fail_next_open(&self, error: FakeError) {
        self.backend.fail_next_open(error.backend_error());
    }

    /// Makes the next stream start hang until released.
    pub fn hang_next_start(&self) -> Held {
        Held(self.backend.hang_next_start())
    }

    /// Makes stream teardown hang until released.
    pub fn hang_teardown(&self) -> Held {
        Held(self.backend.hang_teardown())
    }

    pub fn is_streaming(&self) -> bool {
        self.backend.is_streaming()
    }
}
