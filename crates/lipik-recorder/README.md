# lipik-recorder

Vendored from handy-recorder 0.1.0 (MIT). See LICENSE for the original copyright notice.

Simple cross-platform microphone capture for Rust.

Opens a mic at whatever format the OS has it set to, and hands your code fixed-size chunks of audio at the rate and channel layout you asked for, on an ordinary thread. It tells you when audio was lost or the device broke instead of quietly giving you silence.

- macOS (CoreAudio), Windows (WASAPI), Linux (PulseAudio / pipewire-pulse, ALSA fallback)
- resampling, mix-to-mono or single-channel selection, fixed chunk sizes
- device loss, stalls, no-audio, and sleep are detected and reported

## Usage

```toml
[dependencies]
lipik-recorder = "0.1"
```

```rust
use lipik_recorder::{CollectingSink, Recorder, RecorderConfig};

let recorder = Recorder::open_with_failure_handler(RecorderConfig::speech(), |error| {
    eprintln!("microphone failed: {error}");
})?;

recorder.start(CollectingSink::new())?;
std::thread::sleep(std::time::Duration::from_secs(5));
let stopped = recorder.stop()?;

let samples = stopped.sink.into_samples(); // 16 kHz mono f32
```

`RecorderConfig::speech()` is 16 kHz mono in 480-frame chunks. Implement `Sink` to stream audio somewhere instead of collecting it.

More in `examples/`:

```
cargo run --example list_devices
cargo run --example record
cargo run --example push_to_talk
```

## macOS < 14.2

CPAL links two CoreAudio functions that only exist on macOS 14.2+. If your binary needs to launch on older macOS, weak-link CoreAudio, e.g. in `.cargo/config.toml`:

```toml
[target.'cfg(target_os = "macos")']
rustflags = ["-C", "link-arg=-Wl,-weak_framework,CoreAudio"]
```

This goes away once CPAL loads those functions at runtime.
