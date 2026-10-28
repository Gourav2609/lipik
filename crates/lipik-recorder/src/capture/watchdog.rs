//! The device thread's watchdog: detects failures nothing reports (no audio,
//! stalled callbacks or delivery, suspension during a recording).

use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use super::engine::{Shared, Timeouts};
use crate::{Error, ErrorKind};

pub(super) struct Watchdog {
    opened_at: Instant,
    callbacks: u64,
    last_callback_at: Instant,
    heartbeat: u64,
    last_heartbeat_at: Instant,
    /// An idle stall was logged and not yet resolved.
    idle_stall_logged: bool,
}

impl Watchdog {
    pub(super) fn new(shared: &Shared) -> Self {
        let now = Instant::now();
        // Time spent opening is not a suspension.
        *shared.watchdog_checked_at.lock().unwrap() = CheckTime::now();
        Self {
            opened_at: now,
            callbacks: 0,
            last_callback_at: now,
            heartbeat: 0,
            last_heartbeat_at: now,
            idle_stall_logged: false,
        }
    }

    pub(super) fn check(&mut self, shared: &Shared, timeouts: &Timeouts) {
        let checked = CheckTime::now();
        let now = checked.instant;
        let recording_since = shared.recording_since();
        let (suspended, frozen) = {
            let mut checked_at = shared.watchdog_checked_at.lock().unwrap();
            let suspended = suspension(*checked_at, checked, recording_since, timeouts.stall);
            // How far `Instant` moved while frozen (all of a sleep on Windows).
            let frozen = now.saturating_duration_since(checked_at.instant);
            *checked_at = checked;
            // Failed under the lock: `stop` reads the same timestamp.
            if let Suspension::DuringRecording(gap) = suspended {
                shared.fail(suspended_during_recording(shared, gap, timeouts.stall));
            }
            (suspended, frozen)
        };
        if let Suspension::WhileIdle(gap) = suspended {
            // The whole process was frozen; don't count the gap against anything.
            log::info!(
                "the process was suspended for {:.1} s while idle (system sleep?)",
                gap.as_secs_f64()
            );
            self.opened_at += frozen;
            self.last_callback_at += frozen;
            self.last_heartbeat_at += frozen;
        }
        let callbacks = shared.transport.callbacks.load(Ordering::Relaxed);
        if callbacks != self.callbacks {
            if self.idle_stall_logged {
                self.idle_stall_logged = false;
                log::debug!(
                    "audio callbacks resumed after {:.1} s",
                    (now - self.last_callback_at).as_secs_f64()
                );
            }
            self.callbacks = callbacks;
            self.last_callback_at = now;
        }
        let heartbeat = shared.heartbeat.load(Ordering::Relaxed);
        if heartbeat != self.heartbeat {
            self.heartbeat = heartbeat;
            self.last_heartbeat_at = now;
        }

        if matches!(suspended, Suspension::DuringRecording(_)) {
            return;
        }
        let error = if callbacks == 0 {
            let waited = now - self.opened_at;
            (waited >= timeouts.no_audio).then(|| {
                shared.error(ErrorKind::NoAudio).with_detail(format!(
                    "no audio {:.1} s after the stream started (bound {:.1} s)",
                    waited.as_secs_f64(),
                    timeouts.no_audio.as_secs_f64()
                ))
            })
        } else {
            // macOS pauses callbacks around sleep, so only a recording stalls,
            // counted from the later of the last callback and its start.
            match recording_since {
                Some(since) => {
                    let silent = now - self.last_callback_at.max(since);
                    (silent >= timeouts.stall).then(|| {
                        shared.error(ErrorKind::Stalled).with_detail(format!(
                            "no audio callback for {:.1} s during a recording, after {callbacks} callbacks (bound {:.1} s)",
                            silent.as_secs_f64(),
                            timeouts.stall.as_secs_f64()
                        ))
                    })
                }
                None => {
                    let silent = now - self.last_callback_at;
                    if silent >= timeouts.stall && !self.idle_stall_logged {
                        self.idle_stall_logged = true;
                        log::debug!(
                            "no audio callbacks for {:.1} s while idle (system sleep?); \
                             not a failure unless it lasts into a recording",
                            silent.as_secs_f64()
                        );
                    }
                    None
                }
            }
        };
        let error = error.or_else(|| {
            let stuck = now - self.last_heartbeat_at;
            (stuck >= timeouts.heartbeat).then(|| {
                shared.error(ErrorKind::SinkStalled).with_detail(format!(
                    "the delivery thread made no progress for {:.1} s (bound {:.1} s)",
                    stuck.as_secs_f64(),
                    timeouts.heartbeat.as_secs_f64()
                ))
            })
        });
        if let Some(error) = error {
            shared.fail(error);
        }
    }
}

/// Callbacks can resume before the watchdog's next check, so `stop` checks too.
/// Skipped once failed: the watchdog has stopped checking.
pub(super) fn fail_if_suspended(shared: &Shared, timeouts: &Timeouts) {
    let recording_since = shared.recording_since();
    if shared.failure().is_some() {
        return;
    }
    // Held while failing, as in `Watchdog::check`.
    let checked_at = shared.watchdog_checked_at.lock().unwrap();
    if let Suspension::DuringRecording(gap) = suspension(
        *checked_at,
        CheckTime::now(),
        recording_since,
        timeouts.stall,
    ) {
        shared.fail(suspended_during_recording(shared, gap, timeouts.stall));
    }
}

fn suspended_during_recording(shared: &Shared, gap: Duration, stall: Duration) -> Error {
    shared.error(ErrorKind::Stalled).with_detail(format!(
        "the process was suspended for {:.1} s during a recording (system sleep?), \
         so the audio has a gap (bound {:.1} s)",
        gap.as_secs_f64(),
        stall.as_secs_f64()
    ))
}

/// A check time on `Instant` and on a clock that counts sleep.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckTime {
    pub instant: Instant,
    /// Time since an arbitrary fixed point, sleep included.
    with_sleep: Duration,
}

impl CheckTime {
    pub(crate) fn now() -> Self {
        let with_sleep = with_sleep_now();
        Self {
            instant: Instant::now(),
            with_sleep,
        }
    }

    /// Time from `earlier` to `self`, sleep included.
    fn since(&self, earlier: &Self) -> Duration {
        self.with_sleep.saturating_sub(earlier.with_sleep)
    }
}

/// Linux's `Instant` stops during suspend; `CLOCK_BOOTTIME` doesn't.
#[cfg(target_os = "linux")]
fn with_sleep_now() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // Cannot fail with a valid clock ID and pointer.
    unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Windows' `Instant` counts sleep; on macOS the stall check sees it instead.
#[cfg(not(target_os = "linux"))]
fn with_sleep_now() -> Duration {
    static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed()
}

#[derive(Debug, PartialEq, Eq)]
enum Suspension {
    None,
    WhileIdle(Duration),
    DuringRecording(Duration),
}

/// Whether the process was frozen since the last check. Windows and Linux
/// freeze callbacks and watchdog together, so only a sleep clock sees the gap.
fn suspension(
    last_check: CheckTime,
    now: CheckTime,
    recording_since: Option<Instant>,
    stall: Duration,
) -> Suspension {
    let gap = now.since(&last_check);
    if gap < stall {
        Suspension::None
    } else if recording_since.is_some_and(|since| since <= last_check.instant) {
        Suspension::DuringRecording(gap)
    } else {
        // Idle, or a recording started after wake, before this check.
        Suspension::WhileIdle(gap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A check `with_sleep` into the clock that counts sleep, at `instant`.
    fn at(instant: Instant, with_sleep: Duration) -> CheckTime {
        CheckTime {
            instant,
            with_sleep,
        }
    }

    #[test]
    fn a_suspension_fails_only_a_recording_that_spans_it() {
        let stall = Duration::from_secs(5);
        let before = Instant::now();
        let t0 = before + Duration::from_secs(1);
        let last_check = at(t0, Duration::from_secs(1));
        // Windows: `Instant` counts the sleep.
        let woke = at(t0 + Duration::from_secs(100), Duration::from_secs(101));
        let gap = Duration::from_secs(100);

        assert_eq!(
            suspension(last_check, woke, Some(before), stall),
            Suspension::DuringRecording(gap)
        );
        assert_eq!(
            suspension(last_check, woke, None, stall),
            Suspension::WhileIdle(gap)
        );
        // Started after wake, before this check ran.
        assert_eq!(
            suspension(last_check, woke, Some(woke.instant), stall),
            Suspension::WhileIdle(gap)
        );
        // Linux: `Instant` stopped during the sleep; only the other clock
        // moved.
        let woke_linux = at(t0 + Duration::from_millis(50), Duration::from_secs(101));
        assert_eq!(
            suspension(last_check, woke_linux, Some(before), stall),
            Suspension::DuringRecording(gap)
        );
        // An ordinary tick.
        assert_eq!(
            suspension(
                last_check,
                at(t0 + Duration::from_millis(50), Duration::from_millis(1050)),
                Some(before),
                stall
            ),
            Suspension::None
        );
    }

    #[test]
    fn the_sleep_clock_advances_with_instant_while_awake() {
        let a = CheckTime::now();
        std::thread::sleep(Duration::from_millis(20));
        let b = CheckTime::now();
        assert!(b.since(&a) >= Duration::from_millis(20));
        assert!(b.since(&a) < Duration::from_secs(5));
    }
}
