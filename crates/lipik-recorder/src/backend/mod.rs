//! The `Backend` trait between the engine and the platform: `cpal/` in
//! production, `fake.rs` in tests. Everything platform-specific lives here.

pub(crate) mod cpal;
#[cfg(any(test, feature = "test-backend"))]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod fake;
#[cfg(target_os = "macos")]
mod headset_macos;
pub(crate) mod permission;

use std::{borrow::Cow, fmt};

use dasp_sample::{I24, Sample, U24};

use crate::{InputDevice, Permission};

pub(crate) trait Backend: Send + Sync + 'static {
    fn list_input_devices(&self) -> Result<Vec<InputDevice>, BackendError>;

    /// Resolves `id`, or the default for `None`, and reads its OS format.
    fn open_device(&self, id: Option<&str>) -> Result<Box<dyn OpenDevice>, BackendError>;

    fn permission_status(&self) -> Permission;

    /// Whether a denied microphone delivers silence (CoreAudio) rather than
    /// failing the stream (WASAPI), so the engine must check permission.
    fn denial_is_silent(&self) -> bool;
}

/// A resolved device whose input stream has not been built yet.
pub(crate) trait OpenDevice {
    fn info(&self) -> &InputDevice;

    /// The device's OS format; the library never asks for another.
    fn format(&self) -> DeviceFormat;

    /// Builds and starts the stream. `data` runs on the real-time thread;
    /// `error` may too.
    fn start(
        self: Box<Self>,
        data: DataCallback,
        error: ErrorCallback,
    ) -> Result<Box<dyn InputStream>, BackendError>;
}

/// A running input stream. Dropping it stops the stream and releases the
/// device. Not `Send`: it is created, owned, and dropped on one thread.
pub(crate) trait InputStream {
    /// Starts or stops holding the headset (`take_headset`). Best effort.
    fn hold_headset(&mut self, _hold: bool) {}

    /// `Err` if the device is gone but the stream wasn't failed (PulseAudio
    /// moves it). Called every watchdog tick; must not block.
    fn check_device(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
}

/// Receives each block of interleaved device samples. Called on the
/// real-time thread: must not allocate, lock, log, or block.
pub(crate) type DataCallback = Box<dyn FnMut(InputData<'_>) + Send + 'static>;

/// Receives stream errors. May be called on the real-time thread.
pub(crate) type ErrorCallback = Box<dyn FnMut(BackendError) + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
}

/// Device sample formats the engine accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SampleFormat {
    U8,
    I8,
    U16,
    I16,
    U24,
    I24,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

/// One callback's samples, in the device's format.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InputData<'a> {
    U8(&'a [u8]),
    I8(&'a [i8]),
    U16(&'a [u16]),
    I16(&'a [i16]),
    U24(&'a [U24]),
    I24(&'a [I24]),
    U32(&'a [u32]),
    I32(&'a [i32]),
    U64(&'a [u64]),
    I64(&'a [i64]),
    F32(&'a [f32]),
    F64(&'a [f64]),
}

/// A device sample type, converted to `f32` as `cpal::Sample` does.
pub(crate) trait InputSample: Copy + Send + 'static {
    fn to_f32(self) -> f32;
    fn wrap(data: &[Self]) -> InputData<'_>;
}

macro_rules! input_sample {
    ($($t:ty => $variant:ident),*) => {$(
        impl InputSample for $t {
            #[inline]
            fn to_f32(self) -> f32 {
                self.to_sample::<f32>()
            }
            fn wrap(data: &[Self]) -> InputData<'_> {
                InputData::$variant(data)
            }
        }
    )*};
}

input_sample!(
    u8 => U8, i8 => I8, u16 => U16, i16 => I16, U24 => U24, I24 => I24, u32 => U32,
    i32 => I32, u64 => U64, i64 => I64,
    f32 => F32, f64 => F64
);

/// A classified platform error. The message is verbatim, except survivable
/// errors, which use fixed text to avoid allocating on the audio thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackendError {
    pub kind: BackendErrorKind,
    pub message: Cow<'static, str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendErrorKind {
    /// No such device, no default device, or the device disappeared.
    DeviceNotAvailable,
    DeviceBusy,
    PermissionDenied,
    /// The device's format can't be captured.
    UnsupportedConfig,
    /// The stream can no longer run as built.
    StreamInvalidated,
    /// Rerouted to a new default. Fails the recorder: it's no longer the
    /// device that was opened.
    DeviceChanged,
    /// A buffer overrun or underrun; the stream keeps running.
    Xrun,
    /// Real-time scheduling was refused; the stream keeps running.
    RealtimeDenied,
    Other,
}

impl BackendErrorKind {
    /// Whether the stream keeps running unchanged after this error.
    pub fn stream_survives(self) -> bool {
        matches!(self, Self::Xrun | Self::RealtimeDenied)
    }
}

impl BackendError {
    pub fn new(kind: BackendErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}
