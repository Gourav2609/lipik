//! The capture engine: moves audio from the real-time callback to an ordinary
//! thread, resamples it into fixed-size chunks, and watches for failures.

pub(crate) mod delivery;
pub(crate) mod engine;
mod resampler;
pub(crate) mod transport;
pub(crate) mod watchdog;

pub(crate) use resampler::FrameResampler;

#[cfg(test)]
mod smoke {
    use std::time::Duration;

    use crate::{CollectingSink, Recorder, RecorderConfig};

    /// Real hardware. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs a microphone"]
    fn records_one_second_from_the_default_microphone() {
        let recorder: Recorder<CollectingSink> =
            Recorder::open(RecorderConfig::speech()).expect("open default microphone");
        eprintln!("recording from {:?}", recorder.info());
        recorder.start(CollectingSink::new()).expect("start");
        std::thread::sleep(Duration::from_secs(1));
        let stopped = recorder.stop().expect("stop");
        recorder.close().expect("close");

        assert!(stopped.is_complete(), "{:?}", stopped.end_reason);
        let samples = stopped.sink.into_samples();
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        eprintln!("captured {} samples at 16 kHz, peak {peak}", samples.len());
        // About one second; a Bluetooth device may start late.
        assert!(samples.len() >= 12_000, "only {} samples", samples.len());
    }
}
