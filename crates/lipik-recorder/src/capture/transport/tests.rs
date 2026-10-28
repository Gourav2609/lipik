//! Callback tests, against the callback as a free function. Consumer-loop
//! scenarios are tested through `Recorder` in `crate::tests`.

use std::sync::atomic::Ordering;

use rtrb::RingBuffer;

use super::{CaptureTransportState, Routing, write_input_to_ring};

#[test]
fn callback_downmixes_or_selects_multichannel_input() {
    let transport = CaptureTransportState::default();
    let (mut average_tx, mut average_rx) = RingBuffer::<f32>::new(4);
    write_input_to_ring(
        &[1.0f32, 3.0, -1.0, 1.0],
        2,
        Routing::MixToMono,
        &mut average_tx,
        &transport,
    );
    let mut averaged = [0.0; 2];
    average_rx
        .pop_entire_slice(&mut averaged)
        .expect("averaged samples");
    assert_eq!(averaged, [2.0, 0.0]);

    let (mut selected_tx, mut selected_rx) = RingBuffer::<f32>::new(4);
    write_input_to_ring(
        &[1.0f32, 3.0, -1.0, 1.0],
        2,
        Routing::Only(1),
        &mut selected_tx,
        &transport,
    );
    let mut selected = [0.0; 2];
    selected_rx
        .pop_entire_slice(&mut selected)
        .expect("selected samples");
    assert_eq!(selected, [3.0, 1.0]);
}

#[test]
fn callback_forwards_boundary_block_then_stays_silent_until_resumed() {
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(8);
    let transport = CaptureTransportState::default();

    // The block in hand when a pause is first observed was captured before
    // the stop, so it is forwarded and only then acknowledged.
    transport.pause_requested.store(true, Ordering::Release);
    write_input_to_ring(
        &[1.0f32, 2.0],
        1,
        Routing::MixToMono,
        &mut producer,
        &transport,
    );
    assert!(transport.pause_acknowledged.load(Ordering::Acquire));
    assert_eq!(consumer.slots(), 2);

    // Later blocks while paused are dropped and are not counted as overruns.
    write_input_to_ring(&[3.0f32], 1, Routing::MixToMono, &mut producer, &transport);
    assert_eq!(consumer.slots(), 2);
    assert_eq!(transport.overrun_frames.load(Ordering::Relaxed), 0);

    // Clearing the pause, as the consumer does before stop() returns, resumes capture.
    transport.pause_acknowledged.store(false, Ordering::Relaxed);
    transport.pause_requested.store(false, Ordering::Release);
    write_input_to_ring(&[4.0f32], 1, Routing::MixToMono, &mut producer, &transport);
    let mut output = [0.0; 3];
    consumer.pop_entire_slice(&mut output).expect("samples");
    assert_eq!(output, [1.0, 2.0, 4.0]);
    assert!(!transport.pause_acknowledged.load(Ordering::Acquire));
}

#[test]
fn overrun_drops_whole_frames_and_counts_frames() {
    // Five free slots hold two stereo frames; the fifth slot stays empty
    // rather than taking half of the third frame.
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(5);
    let transport = CaptureTransportState::default();

    write_input_to_ring(
        &[1.0f32, -1.0, 2.0, -2.0, 3.0, -3.0],
        2,
        Routing::All,
        &mut producer,
        &transport,
    );

    assert_eq!(consumer.slots(), 4);
    let mut output = [0.0; 4];
    consumer.pop_entire_slice(&mut output).expect("samples");
    assert_eq!(output, [1.0, -1.0, 2.0, -2.0]);
    assert_eq!(transport.overrun_frames.load(Ordering::Relaxed), 1);
}
