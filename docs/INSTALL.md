# Installing Lipik

Lipik runs on Macs with Apple silicon (M1 and later). There is no ready-made
build for Intel Macs; you can build one from source (see the end of this page).
The meeting notetaker needs macOS 14.2 or later; dictation works on older
versions.

## Quickest: one line in Terminal

Open **Terminal** (in Applications → Utilities), paste this and press Return:

```bash
curl -fsSL https://lipik.pages.dev/install.sh | sh
```

It checks your Mac, downloads the latest release from GitHub, copies
**Lipik** into **Applications** and opens it. A download made from Terminal
isn't marked as "downloaded from the internet", so macOS shows no security
warning and you can skip step 2 below. If Lipik is already open, the script
closes it for you; run the same line again any time to update. Your settings, history and
meeting notes stay. The script is
[website/install.sh](https://github.com/Gourav2609/lipik/blob/main/website/install.sh)
if you want to read it first.

## 1. Download

Or install by hand:

Download [Lipik.dmg](https://github.com/Gourav2609/lipik/releases/latest/download/Lipik.dmg)
from the [Releases page](https://github.com/Gourav2609/lipik/releases).

Open the `.dmg` and drag **Lipik** into **Applications**.

## 2. Open it the first time

Lipik is not signed with an Apple Developer ID yet, so macOS blocks the
first launch. You only need to do this once.

**macOS 14 (Sonoma) and earlier**

1. In Finder, open **Applications**.
2. Right-click (or Control-click) **Lipik** and choose **Open**.
3. Click **Open** in the dialog.

**macOS 15 (Sequoia) and later**

Right-click → Open no longer skips the check. Instead:

1. Double-click **Lipik**. macOS says it cannot be opened. Click **Done**.
2. Open **System Settings → Privacy & Security**.
3. Scroll down to the message about Lipik and click **Open Anyway**.
4. Confirm with your password or Touch ID, then click **Open Anyway** again.

If macOS says the app "is damaged and can't be opened", the download was
quarantined. Remove the flag and try again:

```bash
xattr -dr com.apple.quarantine /Applications/Lipik.app
```

## 3. Grant permissions

Lipik asks for these when it first needs them. You can also turn them on in
**System Settings → Privacy & Security**.

| Permission                                                                                   | Why                                                                          |
| -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| **Microphone**                                                                               | To hear you while you hold `fn`.                                             |
| **Accessibility**                                                                            | To type the text into the app you are using and to read the `fn` key.        |
| **Screen & System Audio Recording** (shown as _System Audio Recording Only_ on recent macOS) | Notetaker only: to record the other people in a call from your Mac's audio.  |
| **Calendars**                                                                                | Notetaker only: to show upcoming meetings, name notes and list who attended. |

If dictation stops typing after an update, remove Lipik from the
Accessibility list with the **−** button and add it again.

## 4. Pick a speech model

On first launch Lipik offers a speech model to download. Each model is stored
on your Mac and runs offline. You can switch models later in Settings.

On macOS 26 or later, **Settings → General → Speech engine** can also use
Apple's on-device dictation:

| Choice                                | What it does                                                                                                                                                                                       |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Auto (Hinglish mode)**, the default | Runs both. Keeps the local model's text when it reads as English, otherwise uses Apple's. Only works with an English-only local model; with a multilingual model Lipik uses the local model alone. |
| **Local model only**                  | Never uses Apple.                                                                                                                                                                                  |
| **Apple dictation**                   | Uses Apple's text, and the local model's if Apple returns nothing.                                                                                                                                 |

Apple's speech files are per language (English (India) by default). If they
are not on your Mac yet, the setting shows a **Download** button; this is the
only step that needs a network connection.

## 5. Optional: local AI clean-up with Ollama

Clean-up, rewrite shortcuts and meeting notes use a local model through
[Ollama](https://ollama.com). Clean-up is on by default. Without Ollama,
dictation still works and Lipik applies only its built-in rules: it removes
"um" and "uh", turns spoken "comma", "question mark" and "new line" into
marks, keeps only the corrected word in "four, no, five", and writes spoken
emails, web addresses and times the usual way.

Lipik's first-run setup checks for Ollama and can download the model for
you (about 5 GB). If you skip it, the same check and download button are in
**Settings → Meetings**. To set it up by hand instead:

```bash
brew install ollama        # or download the app from ollama.com
ollama serve               # skip if the Ollama app is already running
ollama pull qwen3:8b
```

Lipik talks to Ollama at `http://127.0.0.1:11434` and uses `qwen3:8b` by
default. You can pick another model in **Settings → Meetings**; dictation
clean-up uses the same model. `qwen3:4b-instruct` is a faster choice (about
40% less wait, slightly more misses). Don't use plain `qwen3:4b`: it is a
thinking model and takes far too long for dictation.

## Build from source

You need:

- [Rust](https://rustup.rs/) (latest stable)
- [Bun](https://bun.sh/)
- Xcode Command Line Tools: `xcode-select --install`

```bash
git clone <this repository> lipik
cd lipik
bun install
bun run tauri build
```

The app and `.dmg` are written to `src-tauri/target/release/bundle/`. Use
`bun run tauri dev` to run a development build with live reload. See
[BUILD.md](../BUILD.md) for more detail.

The Apple dictation helper (`src-tauri/apple-speech/main.swift`) needs the
macOS 26 SDK, which comes with Xcode 26 or its Command Line Tools. With an
older SDK the build makes a stub helper instead, and Lipik uses the local
model only.

The project signs with a local certificate named "Lipik Dev"
(`bundle.macOS.signingIdentity` in `src-tauri/tauri.conf.json`). If you do
not have it, the build stops at code signing. Build with an ad-hoc signature
(`-`) instead. Tauri reads the `APPLE_SIGNING_IDENTITY` environment variable
in place of the identity in `tauri.conf.json`:

```bash
APPLE_SIGNING_IDENTITY=- bun run tauri build
```

Or override the config for one build:

```bash
bun run tauri build --config '{"bundle":{"macOS":{"signingIdentity":"-"}}}'
```

A build you make yourself is not signed with a Developer ID either, so the
first-launch steps above apply to it too.
