//! Formats and channels: what `open` validates and resolves, and K-channel
//! delivery end to end.

use std::sync::{Arc, Mutex};

use super::support::{
    self, Chunks, fake, fake_with, open, open_with, start, stop_with_boundary, timeouts,
};
use crate::backend::{SampleFormat, fake::Gate};
use crate::{AudioChunk, Channels, CollectingSink, ErrorKind, Recorder, RecorderConfig, Sink};

#[test]
fn format_and_channel_errors_come_only_from_open() {
    let fake = fake(48_000, 2);
    let cases = [
        (
            RecorderConfig {
                channels: Channels::Only(2),
                ..Default::default()
            },
            ErrorKind::InvalidChannel,
        ),
        (
            RecorderConfig {
                sample_rate: Some(0),
                ..Default::default()
            },
            ErrorKind::UnsupportedFormat,
        ),
        (
            RecorderConfig {
                sample_rate: Some(10_000_000),
                ..Default::default()
            },
            ErrorKind::UnsupportedFormat,
        ),
        (
            RecorderConfig {
                frames_per_chunk: Some(0),
                ..Default::default()
            },
            ErrorKind::UnsupportedFormat,
        ),
        (
            RecorderConfig {
                frames_per_chunk: Some(usize::MAX),
                ..Default::default()
            },
            ErrorKind::UnsupportedFormat,
        ),
    ];
    for (config, kind) in cases {
        let error = open_with::<CollectingSink>(&fake, config.clone(), timeouts())
            .err()
            .unwrap_or_else(|| panic!("{config:?} opened"));
        assert_eq!(error.kind(), kind, "{config:?}");
        assert!(error.detail().is_some());
        assert!(!fake.is_streaming());
    }
    // In range, and an odd output rate, both open.
    for config in [
        RecorderConfig {
            channels: Channels::Only(1),
            ..Default::default()
        },
        RecorderConfig {
            sample_rate: Some(44_056),
            ..Default::default()
        },
    ] {
        open_with::<CollectingSink>(&fake, config, timeouts()).expect("opens");
    }
}

#[test]
fn stereo_channels_never_rotate_or_bleed_through_resampling_and_boundaries() {
    // Left carries a tone, right is silent: a rotation or bleed puts energy
    // on the right, which resampling otherwise leaves exactly zero.
    let fake = fake(48_000, 2);
    let recorder: Recorder<Chunks> = open(
        &fake,
        RecorderConfig {
            sample_rate: Some(16_000),
            channels: Channels::All,
            frames_per_chunk: Some(480),
            ..Default::default()
        },
    );
    let stereo = |frames: usize, offset: usize| -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let t = (i + offset) as f64 / 48_000.0;
                [
                    ((2.0 * std::f64::consts::PI * 440.0 * t).sin() * 0.5) as f32,
                    0.0,
                ]
            })
            .collect()
    };
    for cycle in 0..3 {
        start(&recorder, Chunks::default());
        for (i, frames) in [1, 777, 1024, 3, 4000].iter().enumerate() {
            assert!(fake.push(&stereo(*frames, i * 5000)));
        }
        let stopped = stop_with_boundary(&recorder, &fake, &stereo(13, 0)).unwrap();
        let chunks = stopped.sink;
        assert_eq!(chunks.channels, 2);
        let all = chunks.all();
        let right: Vec<f32> = all.iter().skip(1).step_by(2).copied().collect();
        let left: Vec<f32> = all.iter().step_by(2).copied().collect();
        assert!(
            right.iter().all(|&s| s == 0.0),
            "cycle {cycle}: right channel has signal"
        );
        assert!(
            left.iter().any(|&s| s.abs() > 0.3),
            "cycle {cycle}: left channel lost"
        );
    }
}

/// Blocks in its first chunk until the gate opens, so the ring overruns.
#[derive(Debug)]
struct SlowSink(Gate, Chunks, Arc<Mutex<bool>>);
impl Sink for SlowSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        if self.1.chunks.is_empty() {
            *self.2.lock().unwrap() = true;
            self.0.wait();
        }
        self.1.process_chunk(chunk);
    }
}

#[test]
fn an_overrun_is_counted_in_whole_frames_and_keeps_channels_aligned() {
    let fake = fake(16_000, 2);
    let recorder: Recorder<SlowSink> = open(
        &fake,
        RecorderConfig {
            channels: Channels::All,
            frames_per_chunk: Some(160),
            ..Default::default()
        },
    );
    let gate = Gate::default();
    let entered = Arc::new(Mutex::new(false));
    start(
        &recorder,
        SlowSink(gate.clone(), Chunks::default(), Arc::clone(&entered)),
    );
    // Left positive, right negative: an odd frame split swaps their signs.
    let block = |frames: usize| -> Vec<f32> {
        (0..frames)
            .flat_map(|i| [0.25 + i as f32 * 1e-6, -0.25])
            .collect()
    };
    assert!(fake.push(&block(160)));
    support::wait_until("the sink is blocked", || *entered.lock().unwrap());
    // Three seconds of audio into a two-second ring, in odd-sized blocks.
    for _ in 0..(3 * 16_000 / 999 + 1) {
        assert!(fake.push(&block(999)));
    }
    gate.open();
    let stopped = stop_with_boundary(&recorder, &fake, &block(1)).unwrap();

    assert!(stopped.dropped_frames > 0);
    assert!(!stopped.is_complete());
    let real = stopped.sink.1.real();
    assert!(
        real.as_chunks::<2>()
            .0
            .iter()
            .all(|&[l, r]| l > 0.0 && r < 0.0),
        "a frame was split"
    );
    let delivered = stopped.sink.1.valid_frames() as u64;
    let pushed = 160 + (3 * 16_000 / 999 + 1) as u64 * 999 + 1;
    assert_eq!(
        delivered + stopped.dropped_frames,
        pushed,
        "every frame delivered or counted"
    );
}

#[test]
fn integer_device_formats_are_converted() {
    let fake = fake_with(16_000, 1, SampleFormat::I16);
    let recorder: Recorder<CollectingSink> = open(
        &fake,
        RecorderConfig {
            frames_per_chunk: Some(4),
            ..Default::default()
        },
    );
    start(&recorder, CollectingSink::new());
    assert!(fake.push(&[i16::MIN, 0, 16_384]));
    let stopped = stop_with_boundary(&recorder, &fake, &[i16::MAX]).unwrap();
    assert_eq!(
        stopped.sink.into_samples(),
        vec![-1.0, 0.0, 0.5, i16::MAX as f32 / 32_768.0]
    );
}
