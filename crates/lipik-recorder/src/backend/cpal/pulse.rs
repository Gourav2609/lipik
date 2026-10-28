//! Fills CPAL's PulseAudio gaps: a fragment size, and noticing a removed source.

use std::{
    ffi::CString,
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use futures::executor::block_on;
use pulseaudio::{ClientError, protocol::PulseError};

use crate::backend::{BackendError, BackendErrorKind};

/// How often the watch looks the source up.
const LOOKUP_INTERVAL: Duration = Duration::from_secs(1);

/// Power of two at or above 20 ms. The default is seconds-long fragments;
/// smaller ones drop PipeWire's whole-graph quantum and cost power.
/// Needs the patched CPAL: 0.18.2 stalls after one fixed-size fragment.
pub(super) fn fragment_frames(sample_rate: u32) -> u32 {
    (sample_rate / 50).max(1).next_power_of_two()
}

/// Watches a source on its own thread and connection, so a hung server can't
/// block the device thread.
pub(super) struct SourceWatch {
    /// A lookup found the source gone.
    removed: Arc<AtomicBool>,
    name: String,
    /// Shut down on drop to end the reactor thread, which otherwise leaks.
    socket: UnixStream,
    /// Dropped to stop the watch thread.
    _stop: mpsc::Sender<()>,
}

impl SourceWatch {
    /// `None`, logged, if it can't; the recorder works without the check.
    pub(super) fn start(source: &str) -> Option<Self> {
        let Ok(name) = CString::new(source) else {
            log::warn!("cannot watch PulseAudio source {source:?}: the name contains NUL");
            return None;
        };
        let socket = match connect_socket().and_then(|s| Ok((s.try_clone()?, s))) {
            Ok(socket) => socket,
            Err(e) => {
                log::warn!("cannot watch PulseAudio source {source}: {e}");
                return None;
            }
        };
        let removed = Arc::new(AtomicBool::new(false));
        let (stop, stopped) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("lipik-recorder-pulse-watch".into())
            .spawn({
                let removed = Arc::clone(&removed);
                move || watch(socket.0, name, &removed, &stopped)
            });
        if let Err(e) = spawned {
            log::warn!("cannot watch PulseAudio source {source}: {e}");
            return None;
        }
        Some(Self {
            removed,
            name: source.to_owned(),
            socket: socket.1,
            _stop: stop,
        })
    }

    /// `Err` when the source no longer exists. Never blocks.
    pub(super) fn check(&self) -> Result<(), BackendError> {
        if !self.removed.load(Ordering::Relaxed) {
            return Ok(());
        }
        Err(BackendError::new(
            BackendErrorKind::DeviceNotAvailable,
            format!(
                "the PulseAudio source {} was removed (the sound server moves its streams to another source)",
                self.name
            ),
        ))
    }
}

impl Drop for SourceWatch {
    /// Ends the watch thread wherever it is. Not joined, so it never blocks.
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

fn watch(socket: UnixStream, name: CString, removed: &AtomicBool, stopped: &mpsc::Receiver<()>) {
    let source = name.to_string_lossy().into_owned();
    let started = Instant::now();
    // No timeout: a hung handshake blocks only this thread.
    let cookie = pulseaudio::cookie_path_from_env().and_then(|path| std::fs::read(path).ok());
    let client = match pulseaudio::Client::new_unix(c"lipik-recorder-watch", socket, cookie) {
        Ok(client) => client,
        Err(e) => {
            log::warn!("cannot watch PulseAudio source {source}: {e}");
            return;
        }
    };
    log::debug!(
        "watching PulseAudio source {source} (connected in {:?})",
        started.elapsed()
    );
    let mut unsure_logged = false;
    let mut first_index = None;
    loop {
        match block_on(client.lookup_source_by_name(name.clone())) {
            Ok(index) if same_source(&mut first_index, index) => {}
            Ok(index) => {
                log::debug!(
                    "PulseAudio source {source} was removed and added again (index {first_index:?}, now {index})"
                );
                removed.store(true, Ordering::Relaxed);
                return;
            }
            Err(ClientError::ServerError(PulseError::NoEntity)) => {
                removed.store(true, Ordering::Relaxed);
                return;
            }
            // Not evidence the source is gone.
            Err(e) => {
                if !unsure_logged {
                    unsure_logged = true;
                    log::debug!("cannot check PulseAudio source {source}: {e}");
                }
            }
        }
        if stopped.recv_timeout(LOOKUP_INTERVAL) != Err(mpsc::RecvTimeoutError::Timeout) {
            return;
        }
    }
}

/// A replugged source returns under a new index; indexes are never reused.
fn same_source(first: &mut Option<u32>, index: u32) -> bool {
    *first.get_or_insert(index) == index
}

fn connect_socket() -> Result<UnixStream, ClientError> {
    let path = pulseaudio::socket_path_from_env().ok_or(ClientError::ServerUnavailable)?;
    Ok(UnixStream::connect(path)?)
}
