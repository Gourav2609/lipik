# Lipik

**Private voice dictation for macOS that runs on your own computer.**

Hold `fn`, speak, and let go. Lipik turns what you said into clean text and
types it into whatever app you are in: an email, a chat, a code editor, a
document. The speech model runs locally, so your voice never leaves your Mac.

_Lipik_ (लिपिक) is Hindi for a scribe: the person who writes things down for
you.

## What it does

- **Dictation anywhere.** Hold `fn` to talk, or tap a shortcut to start and
  stop. A small pill at the bottom of the screen shows that Lipik is listening,
  and the text lands where your cursor is.
- **Local speech-to-text.** Choose from Whisper, Parakeet and other models.
  Each model is downloaded once and then runs offline.
- **History.** Every dictation is kept on your Mac. Copy it again, correct a
  word, or play back the recording.

## What Lipik adds on top of Handy

Lipik started as a fork of [Handy](https://github.com/cjpais/Handy). It keeps
Handy's recording and transcription engine and adds:

- **Local AI clean-up.** On by default. Built-in rules turn spoken
  punctuation, corrections like "four, no, five", emails and times into
  written text, and a local model in [Ollama](https://ollama.com) removes
  filler words, fixes punctuation and writes in the tone you set for
  personal messages, work chat and email. A reply that adds words you
  didn't say, drops your words or translates Hinglish is thrown away.
  By default the clean-up runs on that local model. It only uses a cloud
  provider if you set one up yourself.
- **Dictionary and snippets.** Teach Lipik names and words it gets wrong, and
  set short phrases that expand into longer text.
- **Rewrite shortcuts.** Select text anywhere and press `⌥1` to `⌥9` to
  rewrite it with the local model. Two come built in (Clear writing, which
  fixes grammar and cuts extra words, and Prompt writer, which turns notes into a
  prompt for an AI assistant), and you can add your own.
- **Meeting notetaker.** Records you from the microphone and the other people
  from your Mac's audio, so no bot joins the call. Afterwards it writes a
  transcript and notes, and you can ask questions about past meetings. Needs
  macOS 14.2 or later.
- **Hinglish support.** Hindi and English mixed in one sentence, written in
  Roman script. On macOS 26 or later Lipik also runs Apple's on-device
  dictation and uses its text when the local model's text does not read as
  English. Everything still stays on your Mac.
- **Voice training scripts.** Fine-tune a Whisper model on your own saved
  dictations so it learns your voice and vocabulary. See
  [training/README.md](training/README.md).

## Screenshots

_Coming soon._

## Install

1. Download [Lipik.dmg](https://github.com/Gourav2609/lipik/releases/latest/download/Lipik.dmg)
   (Apple silicon, M1 and later) and drag **Lipik** into **Applications**.
2. Open it once. Lipik isn't signed with an Apple Developer ID yet, so macOS
   blocks the first launch:
   - macOS 15 and later: double-click Lipik, click **Done**, then go to
     **System Settings → Privacy & Security** and click **Open Anyway**.
   - macOS 14 and earlier: right-click Lipik in Applications, choose **Open**,
     then **Open**.
3. Allow **Microphone** and **Accessibility** when asked.
4. Optional, for clean-up: install the free [Ollama](https://ollama.com) app
   and run `ollama pull qwen3:8b` (about 5 GB). Without it, Lipik still types
   what you said and fixes "um", spoken punctuation, numbers and emails with
   its built-in rules.

[docs/INSTALL.md](docs/INSTALL.md) has the details.

## Build from source

You need [Rust](https://rustup.rs/), [Bun](https://bun.sh/) and the Xcode
Command Line Tools (`xcode-select --install`).

```bash
git clone <this repository> lipik
cd lipik
bun install
bun run tauri dev      # run while developing
bun run tauri build    # build the .app and .dmg
```

The build output is in `src-tauri/target/release/bundle/`. [BUILD.md](BUILD.md)
has more detail, including the files the app expects next to it during
development.

## Website

The site at [lipik.refflux.dev](https://lipik.refflux.dev) lives in
[`website/`](website): plain HTML, CSS and JavaScript, no build step. Every
change merged to `main` that touches `website/` is deployed by
`.github/workflows/website.yml`. To preview it locally, run
`python3 -m http.server -d website 8742` and open http://127.0.0.1:8742.

## Credits

Lipik is built on [Handy](https://github.com/cjpais/Handy) by CJ Pais (MIT).
Handy's recording, voice activity detection, model management and
transcription engine are the base of this app. Thanks to CJ and everyone who
contributes to Handy, and to the authors of transcribe.cpp, whisper.cpp, ggml
and Silero VAD.

## Licence

MIT. See [LICENSE](LICENSE) for the full text and [NOTICE](NOTICE) for the
software Lipik is based on.
