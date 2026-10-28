//! Recorder lifecycle over the fake backend: exact boundaries, the recording
//! slot, repeated recordings, close and drop.

use std::{sync::Arc, thread};

use super::support::{
    self, Chunks, fake, open, push_idle, ramp, start, stop_with_boundary, wait_until,
};
use crate::{Channels, CollectingSink, ErrorKind, Recorder, RecorderConfig};

/// 16 kHz mono in, 16 kHz mono out: no resampling, so samples pass through
/// unchanged and positions can be checked exactly.
fn passthrough(frames_per_chunk: usize) -> RecorderConfig {
    RecorderConfig {
        frames_per_chunk: Some(frames_per_chunk),
        ..RecorderConfig::default()
    }
}

#[test]
fn a_recording_delivers_exactly_the_audio_between_start_and_the_boundary() {
    let fake = fake(16_000, 1);
    let recorder: Recorder<Chunks> = open(&fake, passthrough(160));
    push_idle(&recorder, &fake, &ramp(0, 500));

    start(&recorder, Chunks::default());
    let first = ramp(1000, 1234);
    assert!(fake.push(&first));
    let boundary = ramp(5000, 77);
    let stopped = stop_with_boundary(&recorder, &fake, &boundary).unwrap();

    assert!(stopped.is_complete());
    let chunks = stopped.sink;
    let expected: Vec<f32> = first.iter().chain(&boundary).copied().collect();
    assert_eq!(chunks.real(), expected);
    assert_eq!(chunks.valid_frames(), 1234 + 77);
    // Every chunk is full size; only the last is padded, with zeros.
    assert!(chunks.chunks.iter().all(|(s, _)| s.len() == 160));
    let (last, rest) = chunks.chunks.split_last().unwrap();
    assert!(rest.iter().all(|(_, v)| *v == 160));
    assert_eq!(last.1, (1234 + 77) % 160);
    assert!(last.0[last.1..].iter().all(|&s| s == 0.0));
    assert_eq!((chunks.sample_rate, chunks.channels), (16_000, 1));
}

#[test]
fn repeated_recordings_on_a_warm_recorder_neither_lose_nor_leak_audio() {
    let fake = fake(16_000, 1);
    let recorder: Recorder<Chunks> = open(&fake, passthrough(480));
    let mut expected_all = Vec::new();
    for cycle in 0..5 {
        push_idle(&recorder, &fake, &ramp(9000 + cycle, 300));
        start(&recorder, Chunks::default());
        let audio = ramp(cycle * 10, 200 + cycle * 7);
        assert!(fake.push(&audio));
        let boundary = [0.75f32 + cycle as f32];
        let stopped = stop_with_boundary(&recorder, &fake, &boundary).unwrap();
        let mut expected = audio.clone();
        expected.extend_from_slice(&boundary);
        assert_eq!(stopped.sink.real(), expected, "cycle {cycle}");
        // The pause is cleared before stop returns.
        assert!(
            !support::transport(&recorder)
                .pause_requested
                .load(std::sync::atomic::Ordering::Acquire)
        );
        expected_all.push(expected);
    }
    // A start immediately after stop loses nothing: the next block is the
    // first of the recording.
    start(&recorder, Chunks::default());
    assert!(fake.push(&[0.5f32, 0.25]));
    let stopped = stop_with_boundary(&recorder, &fake, &[0.125f32]).unwrap();
    assert_eq!(stopped.sink.real(), vec![0.5, 0.25, 0.125]);
}

#[test]
fn start_while_recording_fails_with_already_recording_and_returns_the_sink() {
    let fake = fake(16_000, 1);
    let recorder: Recorder<CollectingSink> = open(&fake, passthrough(160));
    start(&recorder, CollectingSink::new());

    let error = recorder.start(CollectingSink::new()).unwrap_err();
    assert_eq!(error.error.kind(), ErrorKind::AlreadyRecording);
    let _sink_came_back: CollectingSink = error.sink;

    stop_with_boundary(&recorder, &fake, &[0.0f32]).unwrap();
    // After stop, the slot is free again.
    start(&recorder, CollectingSink::new());
}

#[test]
fn concurrent_stops_give_the_recording_to_exactly_one() {
    let fake = fake(16_000, 1);
    let recorder: Arc<Recorder<CollectingSink>> = Arc::new(open(&fake, passthrough(160)));
    for _ in 0..10 {
        start(&recorder, CollectingSink::new());
        let results: Vec<_> = thread::scope(|scope| {
            let stops: Vec<_> = (0..4).map(|_| scope.spawn(|| recorder.stop())).collect();
            // Keep callbacks coming so the pause handshake completes.
            let mut results = Vec::new();
            for stop in stops {
                while !stop.is_finished() {
                    fake.push(&[0.0f32; 16]);
                    thread::yield_now();
                }
                results.push(stop.join().unwrap());
            }
            results
        });
        let won = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(won, 1);
        assert!(
            results
                .iter()
                .filter_map(|r| r.as_ref().err())
                .all(|e| e.kind() == ErrorKind::NotRecording)
        );
    }
}

#[test]
fn dropping_the_recorder_closes_it() {
    let fake = fake(16_000, 1);
    {
        let recorder: Recorder<CollectingSink> = open(&fake, passthrough(160));
        start(&recorder, CollectingSink::new());
        assert!(fake.is_streaming());
    }
    assert!(!fake.is_streaming());
}

#[test]
fn close_without_any_audio_is_prompt() {
    let fake = fake(48_000, 2);
    let recorder: Recorder<CollectingSink> = open(&fake, RecorderConfig::speech());
    let started = std::time::Instant::now();
    recorder.close().expect("close");
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn closing_from_inside_the_sink_does_not_deadlock() {
    use std::sync::Mutex;

    /// Holds the only handle to its own recorder and drops it mid-chunk.
    struct Closer(Arc<Mutex<Option<Recorder<Closer>>>>);
    impl crate::Sink for Closer {
        fn process_chunk(&mut self, _: crate::AudioChunk<'_>) {
            let recorder = self.0.lock().unwrap().take();
            drop(recorder);
        }
    }

    let fake = fake(16_000, 1);
    let slot = Arc::new(Mutex::new(None));
    let recorder: Recorder<Closer> = open(&fake, passthrough(160));
    start(&recorder, Closer(Arc::clone(&slot)));
    *slot.lock().unwrap() = Some(recorder);

    // The first full chunk runs the sink, which closes the recorder from
    // the delivery thread.
    assert!(fake.push(&ramp(0, 320)));
    support::wait_until("the sink closed the recorder", || !fake.is_streaming());
}

#[test]
fn stop_from_inside_the_sink_fails_at_once_and_the_recording_continues() {
    use std::sync::{Mutex, Weak, mpsc};
    use std::time::Duration;

    /// Calls `stop` on its own recorder from its first chunk.
    struct Stopper {
        recorder: Arc<Mutex<Weak<Recorder<Stopper>>>>,
        result: Option<mpsc::Sender<ErrorKind>>,
        samples: Vec<f32>,
    }
    impl crate::Sink for Stopper {
        fn process_chunk(&mut self, chunk: crate::AudioChunk<'_>) {
            if let Some(tx) = self.result.take() {
                let recorder = self.recorder.lock().unwrap().upgrade().unwrap();
                let kind = recorder.stop().err().map(|e| e.kind());
                let _ = tx.send(kind.expect("stop from the sink must fail"));
            }
            self.samples
                .extend_from_slice(&chunk.samples[..chunk.valid_frames]);
        }
    }

    let fake = fake(16_000, 1);
    let recorder: Arc<Recorder<Stopper>> = Arc::new(open(&fake, passthrough(160)));
    let handle = Arc::new(Mutex::new(Arc::downgrade(&recorder)));
    let (tx, rx) = mpsc::channel();
    start(
        &recorder,
        Stopper {
            recorder: handle,
            result: Some(tx),
            samples: Vec::new(),
        },
    );

    assert!(fake.push(&ramp(0, 320)));
    // Well inside the test stop deadline (2 s): the call does not wait for
    // the delivery thread it is running on.
    let kind = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("stop from the sink returned promptly");
    assert_eq!(kind, ErrorKind::StopFromSink);

    // The recording and the recorder are unaffected.
    let stopped = stop_with_boundary(&recorder, &fake, &ramp(320, 160)).unwrap();
    assert!(stopped.is_complete(), "{:?}", stopped.end_reason);
    assert_eq!(stopped.sink.samples, ramp(0, 480));
    start(
        &recorder,
        Stopper {
            recorder: Arc::new(Mutex::new(Weak::new())),
            result: None,
            samples: Vec::new(),
        },
    );
}

#[test]
fn collecting_sink_keeps_only_real_audio() {
    let fake = fake(16_000, 2);
    let recorder: Recorder<CollectingSink> = open(
        &fake,
        RecorderConfig {
            channels: Channels::All,
            frames_per_chunk: Some(100),
            ..RecorderConfig::default()
        },
    );
    start(&recorder, CollectingSink::new());
    let audio: Vec<f32> = ramp(0, 2 * 130);
    assert!(fake.push(&audio));
    let stopped = stop_with_boundary(&recorder, &fake, &[0.5f32, -0.5]).unwrap();
    let mut expected = audio;
    expected.extend_from_slice(&[0.5, -0.5]);
    assert_eq!(stopped.sink.into_samples(), expected);
}

#[test]
fn take_headset_holds_the_headset_only_while_recording() {
    let fake = fake(16_000, 1);
    let recorder: Recorder<Chunks> = open(
        &fake,
        RecorderConfig {
            take_headset: true,
            ..passthrough(160)
        },
    );
    for cycle in 1..=2 {
        start(&recorder, Chunks::default());
        wait_until("the headset is held", || {
            fake.headset_holds().len() == 2 * cycle - 1
        });
        stop_with_boundary(&recorder, &fake, &[0.5f32]).unwrap();
        wait_until("the headset is released", || {
            fake.headset_holds().len() == 2 * cycle
        });
    }
    assert_eq!(fake.headset_holds(), [true, false, true, false]);
}
