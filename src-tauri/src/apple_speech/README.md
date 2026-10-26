# Apple on-device speech engine

A second speech engine next to the local model (Parakeet by default): Apple's
`DictationTranscriber` from the `SpeechAnalyzer` framework, macOS 26 and later.
It runs fully on the Mac, works offline once the locale's speech assets are on
disk, and writes Hindi words in Roman letters with the `en-IN` locale, which the
local English model cannot do.

## Settings

| Field           | Values                                                         | Default   |
| --------------- | -------------------------------------------------------------- | --------- |
| `speech_engine` | `"parakeet"`, `"apple"`, `"auto"`                              | `"auto"`  |
| `apple_locale`  | any locale Apple dictation supports, e.g. `"en-IN"`, `"en-US"` | `"en-IN"` |

- `parakeet`: only the selected local model, as before. (The value is named
  after the default model but means whichever local model is selected.)
- `apple`: Apple only. If Apple fails or returns nothing, the local model's
  result is used instead, so dictation never silently breaks. The local model
  is still loaded when recording starts, for that fallback.
- `auto` (Hinglish mode): both engines run at the same time on the same audio,
  then one result is kept (rule below).

Apple is skipped, and the local model used alone, when:
the Mac is older than macOS 26 or the helper is not bundled; _Translate to
English_ is on; or, in `auto` only, the selected local model is not English-only
(Parakeet V3, Whisper, Canary and other multilingual models write other
languages correctly, and the rule below would replace them with Apple's
English) or the dictation language is set to something other than
auto-detect, English or Hindi.

The live preview while speaking still comes from the local model; the text that
is pasted is the one picked here. The notetaker always uses the local model.

## The auto rule

`pick::local_text_looks_english` decides. The local model's text is kept when:

1. it is not empty,
2. none of its words is a common Hindi word written in Roman letters
   (`hai`, `ki`, `ke`, `nahi`, `kya`, `kar`, `karo`, ... about 80 words); the
   four that are also English or part of an English phrase (`agar`, `mere`,
   `se`, `wale`: "a mere formality", "per se") only count when two of them
   appear, and
3. at least 75% of its words are common English words (`textflow/common_words.txt`,
   the SCOWL list) or words in the user's dictionary.

All-caps acronyms of up to 5 letters (PR, API, JIRA) and words with digits are
not counted. Otherwise Apple's text is used.

Why it works: the local model only knows English, so on Hindi speech it writes
made-up words ("Kalki meeting Kansal Kadu, Rahulko Bata Dena") or Hindi words it
happened to spell right. Either one fails the rule.

Latency: when the local text passes the rule it is returned at once and Apple's
result is thrown away, so English dictation waits for nothing extra. Only
dictations that fail the rule wait for Apple, which started at the same moment
as the local model.

## Benchmark (training/bench, 160 synthetic clips, 2026-10-25)

Same 160 `say` clips as `training/bench/RESULTS.md`. Apple went through the
bundled helper with the app's request format; the auto column uses the Rust
`pick.rs` rule on the stored Parakeet outputs. WER in %, strict scoring.

| Engine           | English (80 clips) | Hinglish (80 clips) | Hinglish, loose |
| ---------------- | ------------------ | ------------------- | --------------- |
| Parakeet (local) | **2.4**            | 61.3                | 57.7            |
| Apple en-IN      | 10.9               | **31.0**            | **29.4**        |
| auto             | **2.4**            | 33.0                | 31.2            |

- auto kept the local text on 80 of 80 English clips and switched to Apple on
  75 of 80 Hinglish clips (77 before the shared-word change; that cost 0.2
  points of Hinglish WER).
- Speed (warm, M4 Pro, clips of ~4 s): local 61 ms median; Apple 109 ms median,
  260 ms p90, about 29x real time. auto: 70 ms median; on the Hinglish clips
  that switched, Apple added 95 ms median (412 ms max) over the local model.
- Helper: starts in ~6 ms, locale check ~60 ms, first decode after start
  ~250 ms, 18 MB resident. A 31 s Hinglish recording took 1.8 s.

## How the helper works

`SpeechAnalyzer` is a Swift-only async API, so it runs in a small command-line
helper, `src-tauri/apple-speech/main.swift`, not inside the Rust process.

- **Build**: `build.rs` (`build_apple_speech_helper`) compiles it with `swiftc`
  on every macOS build into `src-tauri/apple-speech/bin/apple-speech-<target triple>`
  (git-ignored). `tauri.macos.conf.json` lists it as an `externalBin`, so tauri
  copies it next to the app binary (`target/debug/apple-speech` in dev,
  `Contents/MacOS/apple-speech` in the bundle) and signs it with the app. With
  an SDK older than macOS 26 a stub is built that reports itself unsupported;
  with the macOS 26 SDK a compile error fails the build.
  To build it by hand:
  `xcrun swiftc -O -target arm64-apple-macos12.0 src-tauri/apple-speech/main.swift -o apple-speech`.
- **Lifetime**: started on first use (or in the background at app start when
  the engine is `apple` or `auto`) and kept running; the model stays loaded
  (`modelRetention: .processLifetime`). Switching the engine to `parakeet` stops
  it without waiting for a running request. Requests run one at a time; a
  request's time limit (30 s + a quarter of the audio length) counts from when
  it was made, including time spent waiting behind another one, and running
  past it kills the helper; the next request starts a new one. A dictation the
  user cancels kills a running Apple job; an Apple job whose result auto mode
  no longer needs is dropped if it has not started. A helper that exits before
  saying hello (macOS 10.15 and 11 cannot load it) is remembered as
  unsupported for the session.
- **Permissions**: none at run time. `SpeechAnalyzer` with buffers we pass in
  needs neither microphone nor Speech Recognition authorization (checked on
  macOS 26.6 from a plain command-line process).
  `NSSpeechRecognitionUsageDescription` is in `Info.plist` in case a later macOS
  asks.
- **Assets**: Apple's speech models are per locale and shared by the system.
  If the locale is installed but not yet reserved for this app, the helper
  reserves it (`AssetInventory.reserve`, offline, instant). If it is not
  installed, transcription fails with `assets_missing` until
  `get_apple_speech_status(download: true)` fetches it (needs a network once).

### Protocol (stdin/stdout)

The helper first prints `{"ready":true,"supported":true|false}`. Each request is
one JSON line, followed for `transcribe` by `samples` little-endian `f32` values
(16 kHz mono); each reply is one JSON line with the same `id`.

```
-> {"id":1,"op":"prepare","locale":"en-IN","download":false}
<- {"id":1,"ok":true,"locale":"en_IN"}
-> {"id":2,"op":"transcribe","locale":"en-IN","samples":64000,"words":["Rahul"]}<64000 x f32>
<- {"id":2,"ok":true,"text":"Kal ki meeting cancel karo","ms":180}
<- {"id":3,"ok":false,"code":"assets_missing","error":"..."}
```

Error codes: `unsupported_os`, `unsupported_locale`, `assets_missing`,
`bad_request`, `failed`. `words` (the user's dictionary) go to Apple as
contextual strings. Recordings over 60 s use the `longDictation` preset,
shorter ones `shortDictation`.

## Commands for the UI

- `change_speech_engine_setting(engine: "parakeet" | "apple" | "auto")`
- `change_apple_locale_setting(locale: string)`
- `get_apple_speech_status(download: bool) -> { supported, locale, ready, code, error }`:
  starts the helper, checks the locale and its assets (downloading them when
  `download` is true) and warms the model. `supported: false` means hide the
  Apple options.

## Not covered yet

- The audio is sent to Apple when the recording stops, not while it is being
  recorded, so a long Hinglish dictation waits for Apple's full decode
  (~1.8 s for 30 s of speech).
- Universal (`universal-apple-darwin`) builds need a lipo'd
  `apple-speech-universal-apple-darwin`; only per-arch builds are handled.
