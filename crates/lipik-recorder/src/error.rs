use std::fmt;
use std::time::Duration;

use crate::InputDevice;

/// Match on [`Error::kind`] to decide what to do; log `Display` to explain it.
#[derive(Debug, Clone)]
pub struct Error(Box<Inner>);

#[derive(Debug, Clone)]
struct Inner {
    kind: ErrorKind,
    device: Option<InputDevice>,
    elapsed: Option<Duration>,
    detail: Option<String>,
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.0.kind
    }

    /// The device involved, if one had been resolved.
    pub fn device(&self) -> Option<&InputDevice> {
        self.0.device.as_ref()
    }

    /// How long the stream had been running. `None` before it started, and
    /// for misuse errors.
    pub fn elapsed(&self) -> Option<Duration> {
        self.0.elapsed
    }

    /// The platform's own message, verbatim (a CoreAudio status, a WASAPI
    /// HRESULT, an ALSA error), or which requested setting was unsupported.
    pub fn detail(&self) -> Option<&str> {
        self.0.detail.as_deref()
    }

    pub(crate) fn new(kind: ErrorKind) -> Self {
        Self(Box::new(Inner {
            kind,
            device: None,
            elapsed: None,
            detail: None,
        }))
    }

    pub(crate) fn with_device(mut self, device: InputDevice) -> Self {
        self.0.device = Some(device);
        self
    }

    pub(crate) fn with_elapsed(mut self, elapsed: Duration) -> Self {
        self.0.elapsed = Some(elapsed);
        self
    }

    pub(crate) fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.0.detail = Some(detail.into());
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let device = self
            .0
            .device
            .as_ref()
            .filter(|_| self.0.kind.is_about_device());
        if let Some(device) = device {
            write!(f, "{} ({}) ", device.name, device.backend)?;
        }
        f.write_str(self.0.kind.describe(device.is_some()))?;
        if let Some(elapsed) = self.0.elapsed {
            write!(f, " {:.1} s into the stream", elapsed.as_secs_f64())?;
        }
        if let Some(detail) = &self.0.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// What went wrong, and so what the application should do.
///
/// | Kind | What to do |
/// |---|---|
/// | `DeviceUnavailable`, `DeviceBusy`, `OpenTimedOut` | Try again, or let the user pick another device. |
/// | `PermissionDenied` | Send the user to the system's privacy settings, then open a new recorder. |
/// | `UnsupportedFormat`, `InvalidChannel` | Change the [`RecorderConfig`](crate::RecorderConfig). |
/// | `NoAudio`, `DeviceLost`, `StreamInvalidated`, `Stalled`, `Backend` | Open a new recorder; this one stays failed. |
/// | `AlreadyRecording`, `NotRecording`, `StopFromSink`, `SinkStalled` | A bug in the application. |
/// | `Processing` | A bug in this library; please report it. |
/// | `CloseTimedOut` | Nothing to recover; log it. The platform may hold the device until the process exits. |
/// | Anything else (the enum is non-exhaustive) | Treat as a failed recorder. |
///
/// A failure is reported to the failure handler, every later `start`, and
/// `stop`'s [`EndReason::RecorderFailed`](crate::EndReason). Don't match on
/// [`Error::detail`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    // From `open`.
    /// No such device, or no input device at all.
    DeviceUnavailable,
    /// Another application has exclusive use of the device.
    DeviceBusy,
    /// Microphone access is off in the system's privacy settings. From
    /// `open`, or while open if access is revoked.
    PermissionDenied,
    /// A `sample_rate` or `frames_per_chunk` the library cannot produce.
    UnsupportedFormat,
    /// `Channels::Only(n)` with `n` out of range for the device.
    InvalidChannel,
    /// The platform did not finish opening the device in time.
    OpenTimedOut,

    // A recorder failing. The recorder stays failed; open a new one.
    /// The device opened but never delivered audio.
    NoAudio,
    /// The device disappeared: unplugged, disabled, or Bluetooth dropped.
    DeviceLost,
    /// The stream can't run as built: its config or the default device
    /// changed, or the sound server restarted.
    StreamInvalidated,
    /// The device stopped delivering audio without an error, or the system
    /// slept during the recording.
    Stalled,
    /// The sink stopped returning and is lost.
    SinkStalled,
    /// Any other platform error; `detail` has the platform's message.
    Backend,
    /// A resampling or framing failure. A bug in this library.
    Processing,

    // Misuse and lifecycle.
    /// `start` before the previous recording was stopped.
    AlreadyRecording,
    /// `stop` with no recording to stop.
    NotRecording,
    /// `stop` called from inside the sink; call it from another thread.
    StopFromSink,
    /// The platform did not finish tearing the stream down in time.
    CloseTimedOut,
}

impl ErrorKind {
    fn is_about_device(self) -> bool {
        !matches!(
            self,
            Self::SinkStalled
                | Self::Processing
                | Self::AlreadyRecording
                | Self::NotRecording
                | Self::StopFromSink
        )
    }

    /// The level a recorder failing with this kind is logged at: warn when
    /// the device, system, or platform changed under it; error when the sink
    /// or this library is at fault.
    pub(crate) fn failure_level(self) -> log::Level {
        match self {
            Self::NoAudio
            | Self::DeviceLost
            | Self::StreamInvalidated
            | Self::Stalled
            | Self::PermissionDenied
            | Self::Backend => log::Level::Warn,
            Self::SinkStalled | Self::Processing => log::Level::Error,
            // Never a recorder's failure: from `open`, or misuse.
            Self::DeviceUnavailable
            | Self::DeviceBusy
            | Self::UnsupportedFormat
            | Self::InvalidChannel
            | Self::OpenTimedOut
            | Self::AlreadyRecording
            | Self::NotRecording
            | Self::StopFromSink
            | Self::CloseTimedOut => log::Level::Error,
        }
    }

    /// The phrase `Display` uses, after the device's name if there is one.
    fn describe(self, after_device: bool) -> &'static str {
        match (self, after_device) {
            (Self::DeviceUnavailable, true) => "is not available",
            (Self::DeviceUnavailable, false) => "no input device is available",
            (Self::DeviceBusy, true) => "is in use by another application",
            (Self::DeviceBusy, false) => "the input device is in use by another application",
            (Self::PermissionDenied, true) => "access was denied",
            (Self::PermissionDenied, false) => "microphone access was denied",
            (Self::UnsupportedFormat, true) => "cannot deliver the requested format",
            (Self::UnsupportedFormat, false) => "the requested format is not supported",
            (Self::InvalidChannel, true) => "has no such channel",
            (Self::InvalidChannel, false) => "the requested channel does not exist",
            (Self::OpenTimedOut, true) => "did not finish opening in time",
            (Self::OpenTimedOut, false) => "the input device did not finish opening in time",
            (Self::NoAudio, true) => "never delivered audio",
            (Self::NoAudio, false) => "the input device never delivered audio",
            (Self::DeviceLost, true) => "disconnected",
            (Self::DeviceLost, false) => "the input device disconnected",
            (Self::StreamInvalidated, true) => "stream was invalidated",
            (Self::StreamInvalidated, false) => "the input stream was invalidated",
            (Self::Stalled, true) => "stopped delivering audio",
            (Self::Stalled, false) => "the input device stopped delivering audio",
            (Self::SinkStalled, _) => "the sink stopped returning",
            (Self::Backend, true) => "reported an error",
            (Self::Backend, false) => "the audio backend reported an error",
            (Self::Processing, _) => "audio processing failed (a bug in lipik-recorder)",
            (Self::AlreadyRecording, _) => "a recording is already active",
            (Self::NotRecording, _) => "there is no recording to stop",
            (Self::StopFromSink, _) => "stop cannot be called from inside the sink",
            (Self::CloseTimedOut, true) => "did not finish closing in time",
            (Self::CloseTimedOut, false) => "the input device did not finish closing in time",
        }
    }
}
