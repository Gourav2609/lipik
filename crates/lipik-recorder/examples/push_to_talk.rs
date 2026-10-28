//! Push-to-talk: press Enter to start, Enter again to stop.
//!
//! The pieces a real application wires together: one loop that owns all
//! state, a sink that says when audio starts flowing, failure notices from
//! the failure handler, and opening a new recorder after one fails.

use std::io::BufRead;
use std::sync::mpsc::{self, Sender};

use lipik_recorder::{AudioChunk, Error, Recorder, RecorderConfig, Sink};

enum AppEvent {
    Key,
    /// Stdin closed (Ctrl-D).
    Quit,
    /// The sink got its first chunk: audio is flowing.
    Ready,
    /// Recorder `id` failed.
    Failed {
        id: u64,
        error: Error,
    },
}

/// Stands in for a VAD + transcription sink.
struct SpeechSink {
    events: Sender<AppEvent>,
    frames: usize,
}

impl Sink for SpeechSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        if self.frames == 0 {
            let _ = self.events.send(AppEvent::Ready);
        }
        self.frames += chunk.valid_frames;
    }
}

fn open(id: u64, events: &Sender<AppEvent>) -> Result<Recorder<SpeechSink>, Error> {
    let events = events.clone();
    let recorder = Recorder::open_with_failure_handler(RecorderConfig::speech(), move |error| {
        // Runs on a library thread: hand the news to the main loop.
        let _ = events.send(AppEvent::Failed { id, error });
    })?;
    println!("using {}", recorder.info().device.name);
    Ok(recorder)
}

fn finish(recorder: &Recorder<SpeechSink>) {
    match recorder.stop() {
        Ok(stopped) => {
            // The failure handler already reported a failed microphone. Say
            // the audio is incomplete, then use what was captured.
            if !stopped.is_complete() {
                eprintln!(
                    "incomplete recording ({} frames dropped)",
                    stopped.dropped_frames
                );
            }
            println!("<{} frames of speech>", stopped.sink.frames);
        }
        Err(e) => eprintln!("recording lost: {e}"),
    }
}

fn main() {
    let (tx, rx) = mpsc::channel();
    let keys = tx.clone();
    std::thread::spawn(move || {
        for _ in std::io::stdin().lock().lines() {
            let _ = keys.send(AppEvent::Key);
        }
        let _ = keys.send(AppEvent::Quit);
    });
    println!("Enter to talk, Enter to stop. Ctrl-D to quit.");

    // All state lives here, owned by this one loop. `recording` never drifts
    // from the recorder: a recording that ends on its own still waits for
    // `stop`, so it is true exactly from a successful `start` to `stop`.
    let mut next_id = 0;
    let mut recorder: Option<(u64, Recorder<SpeechSink>)> = None;
    let mut recording = false;

    for event in rx {
        match event {
            AppEvent::Key if recording => {
                let (_, r) = recorder.as_ref().unwrap();
                finish(r);
                recording = false;
            }
            AppEvent::Key => {
                if recorder.is_none() {
                    next_id += 1;
                    match open(next_id, &tx) {
                        Ok(r) => recorder = Some((next_id, r)),
                        Err(e) => {
                            eprintln!("can't open microphone: {e}");
                            continue;
                        }
                    }
                }
                let (_, r) = recorder.as_ref().unwrap();
                match r.start(SpeechSink {
                    events: tx.clone(),
                    frames: 0,
                }) {
                    Ok(()) => {
                        recording = true;
                        println!("connecting...");
                    }
                    Err(e) => {
                        // The recorder failed before its notice arrived. The
                        // next key press opens a new one.
                        eprintln!("can't record: {e}");
                        recorder = None;
                    }
                }
            }
            AppEvent::Quit => {
                if recording && let Some((_, r)) = &recorder {
                    finish(r);
                }
                break;
            }
            AppEvent::Ready => println!("listening"),
            AppEvent::Failed { id, error } => {
                // Ignore a late notice from a recorder already replaced.
                if let Some((current, r)) = &recorder
                    && *current == id
                {
                    eprintln!("microphone failed: {error}");
                    if recording {
                        finish(r); // keep what was captured
                        recording = false;
                    }
                    recorder = None;
                }
            }
        }
    }
}
