//! The CPAL backend. Devices open at their OS default format.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Instant,
};

mod error;
#[cfg(target_os = "linux")]
mod pulse;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use error::{map_error, map_error_during};

use super::{
    Backend, BackendError, BackendErrorKind, DataCallback, DeviceFormat, ErrorCallback,
    InputSample, InputStream, OpenDevice, SampleFormat,
};
use crate::{InputDevice, Permission};

pub(crate) struct CpalBackend {
    id: cpal::HostId,
    /// `None` for PulseAudio, which connects per operation (`host`).
    shared: Option<cpal::Host>,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<cpal::Host>();
    assert_send_sync::<cpal::Device>();
};

impl CpalBackend {
    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<CpalBackend>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| Arc::new(Self::new())))
    }

    /// cpal's default host: on Linux, PulseAudio if running, else ALSA.
    pub fn new() -> Self {
        let host = cpal::default_host();
        let id = host.id();
        Self {
            id,
            shared: (!is_pulseaudio(id)).then_some(host),
        }
    }

    /// PulseAudio gets a fresh connection per operation: a server restart
    /// kills old ones for good.
    fn host(&self) -> Result<HostRef<'_>, BackendError> {
        match &self.shared {
            Some(host) => Ok(HostRef::Shared(host)),
            None => cpal::host_from_id(self.id)
                .map(HostRef::Own)
                .map_err(|e| map_error_during("Failed to connect to PulseAudio", e)),
        }
    }
}

enum HostRef<'a> {
    Shared(&'a cpal::Host),
    Own(cpal::Host),
}

impl std::ops::Deref for HostRef<'_> {
    type Target = cpal::Host;

    fn deref(&self) -> &cpal::Host {
        match self {
            HostRef::Shared(host) => host,
            HostRef::Own(host) => host,
        }
    }
}

fn is_pulseaudio(host: cpal::HostId) -> bool {
    #[cfg(target_os = "linux")]
    if host == cpal::HostId::PulseAudio {
        return true;
    }
    let _ = host;
    false
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_owned())
        .unwrap_or_else(|_| "Unknown".into())
}

/// ALSA PCM names can move between cards.
fn id_is_stable(host: cpal::HostId) -> bool {
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd"))]
    if host == cpal::HostId::Alsa {
        return false;
    }
    let _ = host;
    true
}

/// ALSA's `null` PCM delivers zeros as fast as it's read; hidden.
fn is_null_pcm(id: &cpal::DeviceId) -> bool {
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd"))]
    if id.host() == cpal::HostId::Alsa {
        return id.id() == "null";
    }
    let _ = id;
    false
}

/// PulseAudio names every monitor source `<sink>.monitor`.
fn is_monitor(id: &cpal::DeviceId) -> bool {
    #[cfg(target_os = "linux")]
    if id.host() == cpal::HostId::PulseAudio {
        return id.id().ends_with(".monitor");
    }
    let _ = id;
    false
}

/// `None` on ALSA, where reading configs opens the PCM.
fn channels_without_opening(host: cpal::HostId, device: &cpal::Device) -> Option<u16> {
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd"))]
    if host == cpal::HostId::Alsa {
        return None;
    }
    let _ = host;
    device.default_input_config().ok().map(|c| c.channels())
}

/// Devices without a backend ID get one from name and occurrence ("USB Mic#1").
fn enumerate(
    host: &cpal::Host,
    with_channels: bool,
) -> Result<Vec<(InputDevice, cpal::Device)>, BackendError> {
    let default_id = host.default_input_device().and_then(|d| d.id().ok());
    let backend = host.id().name().to_owned();
    let mut occurrences = HashMap::<String, u32>::new();
    let mut out = Vec::new();
    for device in host.input_devices().map_err(map_error)? {
        if device.id().is_ok_and(|id| is_null_pcm(&id)) {
            continue;
        }
        let name = device_name(&device);
        let occurrence = occurrences.entry(name.clone()).or_default();
        let (id, stable, is_default, monitor) = match device.id() {
            Ok(id) => (
                id.to_string(),
                id_is_stable(id.host()),
                default_id.as_ref() == Some(&id),
                is_monitor(&id),
            ),
            Err(_) => (format!("{name}#{occurrence}"), false, false, false),
        };
        out.push((
            InputDevice {
                id,
                name,
                occurrence: *occurrence,
                backend: backend.clone(),
                is_default,
                id_is_stable: stable,
                channels: if with_channels {
                    channels_without_opening(host.id(), &device)
                } else {
                    None
                },
                is_monitor: monitor,
            },
            device,
        ));
        *occurrence += 1;
    }
    Ok(out)
}

impl Backend for CpalBackend {
    fn permission_status(&self) -> Permission {
        super::permission::permission_status()
    }

    fn denial_is_silent(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn list_input_devices(&self) -> Result<Vec<InputDevice>, BackendError> {
        let host = self.host()?;
        Ok(enumerate(&host, true)?
            .into_iter()
            .map(|(info, _)| info)
            .collect())
    }

    fn open_device(&self, id: Option<&str>) -> Result<Box<dyn OpenDevice>, BackendError> {
        let resolve_started = Instant::now();
        let host = self.host()?;
        let host = &*host;
        let (mut info, device) = match id {
            Some(id) => enumerate(host, false)?
                .into_iter()
                .find(|(info, _)| info.id == id)
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorKind::DeviceNotAvailable,
                        format!("No input device with ID {id:?}"),
                    )
                })?,
            None => {
                let default = host.default_input_device().ok_or_else(|| {
                    BackendError::new(
                        BackendErrorKind::DeviceNotAvailable,
                        "No input device found",
                    )
                })?;
                let id = default.id().ok();
                // Open the resolved device, not "default": WASAPI invalidates
                // a default stream when the user switches.
                enumerate(host, false)
                    .ok()
                    .and_then(|devices| {
                        devices
                            .into_iter()
                            .find(|(_, d)| id.is_some() && d.id().ok() == id)
                    })
                    .unwrap_or_else(|| {
                        let info = InputDevice {
                            id: id.as_ref().map_or_else(String::new, ToString::to_string),
                            name: device_name(&default),
                            occurrence: 0,
                            backend: host.id().name().to_owned(),
                            is_default: true,
                            is_monitor: id.as_ref().is_some_and(is_monitor),
                            id_is_stable: id.is_some_and(|id| id_is_stable(id.host())),
                            channels: None,
                        };
                        (info, default)
                    })
            }
        };

        log::debug!("resolve_device={:?}", resolve_started.elapsed());

        let config_started = Instant::now();
        let config = device
            .default_input_config()
            .map_err(|e| map_error_during("Failed to fetch preferred config", e))?;
        log::debug!("fetch_config={:?}", config_started.elapsed());

        let sample_format = match config.sample_format() {
            cpal::SampleFormat::U8 => SampleFormat::U8,
            cpal::SampleFormat::I8 => SampleFormat::I8,
            cpal::SampleFormat::U16 => SampleFormat::U16,
            cpal::SampleFormat::I16 => SampleFormat::I16,
            cpal::SampleFormat::U24 => SampleFormat::U24,
            cpal::SampleFormat::I24 => SampleFormat::I24,
            cpal::SampleFormat::U32 => SampleFormat::U32,
            cpal::SampleFormat::I32 => SampleFormat::I32,
            cpal::SampleFormat::U64 => SampleFormat::U64,
            cpal::SampleFormat::I64 => SampleFormat::I64,
            cpal::SampleFormat::F32 => SampleFormat::F32,
            cpal::SampleFormat::F64 => SampleFormat::F64,
            sample_format => {
                return Err(BackendError::new(
                    BackendErrorKind::UnsupportedConfig,
                    format!("Unsupported sample format: {sample_format:?}"),
                ));
            }
        };

        info.channels = Some(config.channels());

        Ok(Box::new(CpalOpenDevice {
            host: host.id(),
            info,
            device_id: device.id().ok(),
            device,
            config,
            format: DeviceFormat {
                sample_rate: config.sample_rate(),
                channels: config.channels(),
                sample_format,
            },
        }))
    }
}

struct CpalOpenDevice {
    /// PulseAudio needs a fragment size and a source watch.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    host: cpal::HostId,
    info: InputDevice,
    device: cpal::Device,
    device_id: Option<cpal::DeviceId>,
    config: cpal::SupportedStreamConfig,
    format: DeviceFormat,
}

struct CpalStream {
    _stream: cpal::Stream,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    device_id: Option<cpal::DeviceId>,
    /// Silent output holding the headset (`take_headset`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    headset: Option<cpal::Stream>,
    #[cfg(target_os = "linux")]
    watch: Option<pulse::SourceWatch>,
}

impl InputStream for CpalStream {
    #[cfg(target_os = "linux")]
    fn check_device(&mut self) -> Result<(), BackendError> {
        match &mut self.watch {
            Some(watch) => watch.check(),
            None => Ok(()),
        }
    }

    #[cfg(target_os = "macos")]
    fn hold_headset(&mut self, hold: bool) {
        if !hold {
            if self.headset.take().is_some() {
                log::debug!("released the headset");
            }
            return;
        }
        if self.headset.is_some() {
            return;
        }
        let Some(id) = &self.device_id else {
            return;
        };
        let Some(output_uid) = super::headset_macos::bluetooth_output_uid(id.id()) else {
            return;
        };
        let started = Instant::now();
        match silent_output(&cpal::DeviceId::new(id.host(), &output_uid)) {
            Ok(stream) => {
                log::info!(
                    "holding the headset: silent output on {output_uid} (started in {:?})",
                    started.elapsed()
                );
                self.headset = Some(stream);
            }
            Err(e) => log::warn!("cannot hold the headset ({output_uid}): {e}"),
        }
    }
}

/// Starts a stream playing silence on the output device `id`.
#[cfg(target_os = "macos")]
fn silent_output(id: &cpal::DeviceId) -> Result<cpal::Stream, String> {
    let host = cpal::host_from_id(id.host()).map_err(|e| e.to_string())?;
    let device = host
        .device_by_id(id)
        .ok_or_else(|| "the output device is gone".to_owned())?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let format = config.sample_format();
    // All-zero bytes are silence only for signed and float samples.
    if !matches!(
        format,
        cpal::SampleFormat::F32
            | cpal::SampleFormat::F64
            | cpal::SampleFormat::I8
            | cpal::SampleFormat::I16
            | cpal::SampleFormat::I32
    ) {
        return Err(format!("unsupported output sample format {format:?}"));
    }
    let stream = device
        .build_output_stream_raw(
            config.config(),
            format,
            |data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| data.bytes_mut().fill(0),
            // Best effort; the input stream reports anything that matters.
            |_| {},
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

impl OpenDevice for CpalOpenDevice {
    fn info(&self) -> &InputDevice {
        &self.info
    }

    fn format(&self) -> DeviceFormat {
        self.format
    }

    fn start(
        self: Box<Self>,
        data: DataCallback,
        error: ErrorCallback,
    ) -> Result<Box<dyn InputStream>, BackendError> {
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut config = self.config.config();
        #[cfg(target_os = "linux")]
        let watch = match &self.device_id {
            Some(id) if self.host == cpal::HostId::PulseAudio => pulse::SourceWatch::start(id.id()),
            _ => None,
        };
        #[cfg(target_os = "linux")]
        if self.host == cpal::HostId::PulseAudio {
            let frames = pulse::fragment_frames(config.sample_rate);
            log::debug!("requesting {frames}-frame fragments from PulseAudio");
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        let build_started = Instant::now();
        let device = &self.device;
        let stream = match self.format.sample_format {
            SampleFormat::U8 => build_stream::<u8>(device, config, data, error),
            SampleFormat::I8 => build_stream::<i8>(device, config, data, error),
            SampleFormat::U16 => build_stream::<u16>(device, config, data, error),
            SampleFormat::I16 => build_stream::<i16>(device, config, data, error),
            SampleFormat::U24 => build_stream::<cpal::U24>(device, config, data, error),
            SampleFormat::I24 => build_stream::<cpal::I24>(device, config, data, error),
            SampleFormat::U32 => build_stream::<u32>(device, config, data, error),
            SampleFormat::I32 => build_stream::<i32>(device, config, data, error),
            SampleFormat::U64 => build_stream::<u64>(device, config, data, error),
            SampleFormat::I64 => build_stream::<i64>(device, config, data, error),
            SampleFormat::F32 => build_stream::<f32>(device, config, data, error),
            SampleFormat::F64 => build_stream::<f64>(device, config, data, error),
        }
        .map_err(|e| map_error_during("Failed to build input stream", e))?;
        let build_elapsed = build_started.elapsed();

        let play_started = Instant::now();
        stream
            .play()
            .map_err(|e| map_error_during("Failed to start microphone stream", e))?;
        log::debug!(
            "build_stream={:?} play={:?}",
            build_elapsed,
            play_started.elapsed()
        );
        Ok(Box::new(CpalStream {
            _stream: stream,
            device_id: self.device_id,
            headset: None,
            #[cfg(target_os = "linux")]
            watch,
        }))
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut data: DataCallback,
    mut error: ErrorCallback,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample + InputSample,
{
    device.build_input_stream(
        config,
        move |samples: &[T], _: &cpal::InputCallbackInfo| data(T::wrap(samples)),
        // Allocates only for fatal errors; see `map_error`.
        move |e: cpal::Error| error(map_error(e)),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real hardware. Run with `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs audio devices"]
    fn lists_and_resolves_real_devices() {
        let backend = CpalBackend::new();
        let devices = backend.list_input_devices().expect("list");
        assert!(!devices.is_empty(), "no input devices");
        assert!(devices.iter().filter(|d| d.is_default).count() <= 1);
        for device in &devices {
            let open = backend
                .open_device(Some(&device.id))
                .expect("resolve by id");
            eprintln!(
                "{} {} [{}] id={} stable={} {:?}",
                if device.is_default { "*" } else { " " },
                device.name,
                device.backend,
                device.id,
                device.id_is_stable,
                open.format()
            );
            assert_eq!(open.info(), device);
        }
        let default = backend.open_device(None).expect("default device");
        assert!(default.info().is_default);
    }
}
