//! Record five seconds of speech-format audio from the default device.

use std::time::Duration;

use lipik_recorder::{CollectingSink, EndReason, Recorder, RecorderConfig};

fn main() -> Result<(), lipik_recorder::Error> {
    // Fails with a reason if there is no mic, permission is denied, etc.
    let recorder = Recorder::open_with_failure_handler(RecorderConfig::speech(), |error| {
        // Runs on a library thread the moment the mic fails.
        eprintln!("microphone failed: {error}");
    })?;
    println!(
        "recording from {} for 5 seconds...",
        recorder.info().device.name
    );

    recorder.start(CollectingSink::new())?;
    std::thread::sleep(Duration::from_secs(5));
    let stopped = recorder.stop()?;

    // A recording that went wrong still returns the audio it captured.
    if !stopped.is_complete() {
        let why = match &stopped.end_reason {
            EndReason::StopCalled => "audio was dropped",
            EndReason::RecorderFailed(_) => "the microphone failed",
            EndReason::SinkPanicked(_) => "the sink panicked",
        };
        eprintln!(
            "incomplete recording: {why}, {} frames dropped",
            stopped.dropped_frames
        );
    }

    let samples = stopped.sink.into_samples();
    println!("captured {} samples", samples.len());
    Ok(())
}
