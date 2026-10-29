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

## More of what it does

- **Local AI clean-up.** On by default. Built-in rules turn spoken
  punctuation, corrections like "four, no, five", emails and times into
  written text, and a built-in local model (Qwen3 8B, or Qwen3 4B Instruct
  on Macs with less than 16 GB of memory, run by llama.cpp inside Lipik)
  removes filler words, fixes punctuation and writes in the tone you set
  for personal messages, work chat and email. A reply that adds words you
  didn't say, drops your words or translates Hinglish is thrown away.
  No other app is needed; [Ollama](https://ollama.com) stays an optional
  choice in Settings, and Macs older than macOS 13.3 use it. It only uses
  a cloud provider if you set one up yourself.
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
- **Your meetings in your AI apps.** Claude, Claude Code, Cursor, Codex,
  Windsurf, VS Code and Gemini CLI can read your meeting notes through MCP,
  on your Mac and read-only. Connect them during setup or in
  **Settings → AI apps**. Each note has **Copy link for AI apps**
  (`lipik://meeting/<id>`) to ask about one meeting. See
  [AI apps](#ai-apps-mcp) below.
- **Hinglish support.** Hindi and English mixed in one sentence, written in
  Roman script. On macOS 26 or later Lipik also runs Apple's on-device
  dictation and uses its text when the local model's text does not read as
  English. Meeting notes notice Hinglish by themselves and write it in Roman
  letters; the first Hinglish meeting downloads a Hinglish speech model
  (Qwen3-ASR 1.7B, about 2 GB). Everything still stays on your Mac.
- **Voice training scripts.** Fine-tune a Whisper model on your own saved
  dictations so it learns your voice and vocabulary. See
  [training/README.md](training/README.md).

## Screenshots

_Coming soon._

## Install

Paste this into Terminal and press Return:

```bash
curl -fsSL https://lipik.pages.dev/install.sh | sh
```

It downloads the latest release, puts **Lipik** in **Applications** and opens
it, with no macOS security warning to click through. Macs with Apple silicon
(M1 and later). Run it again any time to update; settings and history stay.

Or download [Lipik.dmg](https://github.com/Gourav2609/lipik/releases/latest/download/Lipik.dmg)
and drag **Lipik** into **Applications**. Lipik isn't signed with an Apple
Developer ID yet, so macOS blocks a downloaded copy the first time:

- macOS 15 and later: double-click Lipik, click **Done**, then go to
  **System Settings → Privacy & Security** and click **Open Anyway**.
- macOS 14 and earlier: right-click Lipik in Applications, choose **Open**,
  then **Open**.

Then allow **Microphone** and **Accessibility** when asked. Clean-up is
optional: one button in setup (or **Settings → Meetings**) downloads the
clean-up model (5 GB, or 2.5 GB on Macs with less than 16 GB of memory), which
runs inside Lipik with no extra app. Without it, Lipik still types what you
said and fixes "um", spoken punctuation, numbers and emails with its built-in
rules.

[docs/INSTALL.md](docs/INSTALL.md) has the details.

## What leaves your Mac

Your voice, your text and your meeting notes stay on your Mac. Lipik only
connects to the internet for:

- one-time model downloads from Hugging Face (speech, clean-up, speakers,
  and the Hinglish speech model the first time a meeting is in Hinglish)
- Apple's English (India) speech files, through macOS, for Hinglish
  dictation only
- every few hours, a request to GitHub for the latest version number (Settings →
  Privacy → Updates turns it off)
- every 3 hours, an anonymous usage count to lipik.pages.dev: a random
  install ID, the app version and the macOS version, so we know how many
  people use Lipik (Settings → Privacy → Usage count turns it off). The
  server notes the country and does not keep the IP address. The code is
  [src-tauri/src/usage_ping.rs](src-tauri/src/usage_ping.rs) and
  [website/worker.js](website/worker.js).
- your email, only if you type it during setup to hear about new versions
- cloud clean-up, only if you add your own API key

## AI apps (MCP)

Lipik runs as an MCP server when an AI app starts it with `--mcp`:

```
/Applications/Lipik.app/Contents/MacOS/lipik --mcp
```

It offers three read-only tools: `list_meetings`, `search_meetings` and
`get_meeting` (summary, decisions, next steps, your own notes and the
transcript). It reads the meeting notes on your Mac and changes nothing.
AI apps can't see your dictations.

**Settings → AI apps** adds Lipik to Claude, Claude Code, Cursor, Codex,
Windsurf, VS Code and Gemini CLI in one click. Lipik changes only its own
`lipik` entry in each app's settings, and keeps the file as it was before
the first change next to it (`<file>.before-lipik`). For any other MCP app,
use the command above. ChatGPT can't run tools on your Mac: open a note and
use **Copy as Markdown** instead.

Claude Code by hand:

```
claude mcp add --scope user lipik -- /Applications/Lipik.app/Contents/MacOS/lipik --mcp
```

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

The site at [lipik.pages.dev](https://lipik.pages.dev) lives in
[`website/`](website): plain HTML, CSS and JavaScript, no build step. It is a
Cloudflare Pages project (`lipik`); its `/api/*` and `/install.sh` are passed
on to the `lipik-site` Worker (`website/worker.js`), which keeps the usage
counts and sends the old lipik.refflux.dev addresses to the new site. Every
change merged to `main` that touches `website/` deploys the Worker through
`.github/workflows/website.yml`; deploy the pages with
`website/pages/build.sh && cd website/pages && npx wrangler@4 pages deploy`.
To preview it locally, run `python3 -m http.server -d website 8742` and open
http://127.0.0.1:8742.

## Credits

Built on [Handy](https://github.com/cjpais/Handy) by CJ Pais (MIT). Thanks
also to the authors of transcribe.cpp, whisper.cpp, ggml and Silero VAD.
Meeting notes tell speakers apart with [speakrs](https://github.com/avencera/speakrs)
(Apache 2.0) and the pyannote community-1 models: segmentation-3.0 (MIT) and
WeSpeaker ResNet34 with PLDA (CC BY 4.0, by the pyannote and WeSpeaker
authors).

## Licence

MIT. See [LICENSE](LICENSE) for the full text and [NOTICE](NOTICE) for the
software Lipik is based on.
