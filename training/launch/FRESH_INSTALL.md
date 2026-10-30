# Fresh-install audit, Lipik 0.1.12 (31 Oct 2026, before the Show HN on 2 Nov)

What a stranger meets between "I saw the HN post" and "my first sentence typed".
Checked on Gourav's Mac (M-series, 24 GB, macOS 26.6.2) without touching
/Applications/Lipik.app or the running app. Code read on `premium-design`; the
onboarding, model and installer code there is the same as the 0.1.12 release
commit (b4c99ad7) except one notetaker file.

## Ranked issues

Severity: **blocks** = the person can't install or can't dictate; **confuses** =
they can get through but may give up; **cosmetic**.

| # | Severity | What happens | Recommended fix |
|---|---|---|---|
| 1 | blocks (update / re-run) | The installer's "is Lipik running" check is `pgrep -xq lipik`. That also matches the `lipik --mcp` helpers that Claude, Cursor or Codex start after the person connects an AI app (onboarding step 8). They never quit, so `close_lipik` waits 10 s and fails with "Couldn't close Lipik. Quit it from its menu bar icon", even after the person quit it. On this Mac right now: 7 `lipik --mcp` processes + 1 `--transcribe-worker`. | Wait only for the main app: `pgrep -f 'Lipik.app/Contents/MacOS/lipik$'`. The MCP helpers don't need to stop: replacing the app while they run is fine (the AI app restarts them). |
| 2 | blocks (rare on first install, likelier on update) | Install step is `rm -rf $DEST/Lipik.app` then `ditto`. If the old app can't be removed (installed by another macOS account, or macOS "App Management" stopping Terminal from changing an app — not tested, needs the friend's Mac), the script exits 1 with raw `rm: Permission denied` lines and no message. A half-finished `rm` leaves a broken app. Reproduced with a read-only copy in a scratch folder. | `ditto` into `$DEST/.Lipik.app.new` first, then move the old app aside and the new one in; on any failure call `fail` with "Drag Lipik from Applications to the Bin, then run this again." |
| 3 | confuses (blocks for non-English speakers) | The default speech model is Parakeet **English only** (`defaultModel.ts`), and setup never asks the language. The site hero says "in your language". A French or Hindi speaker's first try comes out as English nonsense. | Pick the default from the Mac's preferred language: English → Parakeet Unified; the 25 European languages → `parakeet-tdt-0.6b-v3` (740 MB, same speed class); anything else → `whisper-large-v3-turbo` (886 MB). Or add a language menu on the Welcome step. |
| 4 | confuses | The `.dmg` route on macOS 15/26 needs 4 steps through System Settings (see below). Documented on the site and in INSTALL.md, but each step loses people. | Keep the Terminal line as the first thing on the page (it is in the hero). Under the Download button show the 4 steps with one screenshot. Later: Apple Developer ID + notarisation ($99/yr, Gourav's call). |
| 5 | confuses | Pressing fn before the speech model has finished downloading does nothing at all: `actions.rs:442-451` returns with only a log line, and the pill shows nothing. On a slow line, someone who skips Practice thinks fn is broken. | Show it on the pill: "Speech model downloading… 42%". |
| 6 | confuses | The clean-up model (5 GB or 2.5 GB) can download at the same time as the 731 MB speech model, so they share the line and Practice waits longer. | Start the clean-up download after the speech model is in, or say "starts after the speech model" on the button. |
| 7 | confuses (check on device) | Steps "Your key" and "You're set" say "Tap it twice to keep going hands-free". With the default mode (hold-or-toggle, 300 ms) one short tap already keeps it listening and the next tap stops it, so tapping twice would start and stop. | Try it once on device; if so, change the copy to "Tap it once to keep going hands-free, tap again to stop." |
| 8 | confuses | Clean-up gives up after 4 s and pastes the rules-only text (`pipeline.rs:15`). Gourav's own log has 5 such fallbacks with the 8B model on a 24 GB Mac. On an 8 GB M1 with the 4B model, long dictations may hit this often, so "clean text" looks like a plain transcript. | Measure on an 8 GB Mac (checklist). Allow more time for longer text, and warm the server right after the download finishes. |
| 9 | confuses | If the speech model download fails, setup says only "The speech model didn't download." with no reason. There is no free-disk check before it (clean-up and Hinglish downloads have one). | Show the error text as the clean-up step does; add the same disk check. |
| 10 | confuses | Someone who clicked "Don't Allow" for the microphone in an earlier launch clicks Allow, macOS shows nothing (it never asks twice), and after 30 s the step says "macOS didn't answer". The "open System Settings instead" fallback only works within the same launch (`asked` is kept in memory). | When the status is denied, open Privacy & Security → Microphone directly and say so. |
| 11 | confuses | The Microphone step, when the mic can't be read, says "You can check your microphone again in the next step". The next step is "Your key". | Change to "You can pick another microphone in Settings." |
| 12 | confuses | Installer `curl` has no retry and no stall timeout: if GitHub's connection stalls, the progress bar freezes forever. | `curl --retry 3 --connect-timeout 15 --speed-time 30 --speed-limit 20000`. |
| 13 | confuses | At the real window size (1280x800), the AI apps step with several apps found pushes the Continue button below the fold (screenshot taken). Skip in the corner still works. | Keep the action buttons fixed at the bottom of the window, or show at most 3 rows. |
| 14 | confuses | Setup is 11 steps, and two (AI apps for meetings, email updates) come before the person has used meetings at all. | Move AI apps to Settings, or after the first meeting. |
| 15 | unknown (check on device) | On a Mac where "Press 🌐 key to" is "Show Emoji & Symbols" or "Change Input Source", releasing fn may still open the emoji picker. The key listener blocks the fn event when it is bound (`lipik-keys` macOS listener, Default tap mode), but this was not tested on such a Mac. | Checklist step 6. If it opens, add a line on the "Your key" step: "Set 'Press 🌐 key to' to Do Nothing" with a button that opens Keyboard settings. |
| 16 | cosmetic | `LIPIK_DEMO=1` shows version 0.1.4 and 22.6 MB (hard-coded). | Read the version from the GitHub API in demo too, or update the numbers. |
| 17 | cosmetic | `uname -m` says `x86_64` when Terminal runs under Rosetta on an M-series Mac, so the installer would wrongly say "needs Apple silicon". | Use `sysctl -n hw.optional.arm64`. |
| 18 | cosmetic | Info.plist says macOS 10.15, but the app binary needs 11.0, llama-server 13.3 and Notetaker 14.2. Every Apple silicon Mac ships with 11 or later, and the app already handles clean-up and Notetaker on older systems, so nobody is blocked. | Set `minimumSystemVersion` to 11.0. |
| 19 | cosmetic | INSTALL.md says "If Lipik is already open, quit it first", but the script closes it itself. | Fix the doc line (after #1). |
| 20 | cosmetic | The installer checks nothing about the downloaded .dmg beyond HTTPS. | After mounting, `codesign --verify --strict` and compare the certificate leaf hash. |

## 1. Installer (`website/install.sh`)

- The live https://lipik.pages.dev/install.sh is the same file as the repo copy.
- URL `releases/latest/download/Lipik.dmg` redirects to v0.1.12 (22,824,084 bytes). Download: **5.0 s (4.6 MB/s)**. The whole scripted install took **14.9 s**.
- **Quarantine:** a file downloaded by `curl` gets no `com.apple.quarantine` flag, and `ditto` from the mounted .dmg adds none (checked: 0 extended attributes on the installed copy). macOS only runs Gatekeeper on quarantined apps, so this route shows **no security warning**, as the site says. `spctl --assess` still rejects the app (expected: no Apple Developer ID).
- No install-folder setting, so I ran a copy of the script with `DEST` pointed at a scratch folder and the close/open lines removed:

| Case | Result |
|---|---|
| Fresh install | OK, 14.9 s, prints "Lipik 0.1.12 is ready." |
| Run again over it (with a terminal, fancy output) | OK, replaces the app |
| Download fails (404 URL) | Clear message: "The download failed. Check your connection and try again.", exit 1 |
| Old app can't be removed | Raw `rm: Permission denied` lines, exit 1, no message (issue #2) |
| Intel Mac | `check_mac` fails → "Lipik needs a Mac with Apple silicon (M1 or later)." (read, not run) |
| No write access to /Applications (standard user) | Installs into `~/Applications` instead (read, not run). If an admin already put Lipik in /Applications, the person ends up with two copies. |
| Lipik running | Asks it to quit via AppleScript, waits 10 s; fails if `lipik --mcp` helpers are alive (issue #1) |
| macOS too old | No check. Not a real risk on Apple silicon (issue #18) |
| Network drop mid-download | `curl -f` fails → same clear message. A stall with no drop hangs forever (issue #12) |
| GitHub API rate limit (60/hour per IP, e.g. office networks) | Only the version number disappears from the text; install still works |

- `LIPIK_DEMO=1 sh website/install.sh` runs in 2.4 s and touches nothing.

## 2. The .dmg route

Checked on the downloaded `Lipik.dmg`, mounted read-only in a scratch folder:

- Contents: `Lipik.app` (52 MB), an `Applications` shortcut, a volume icon.
- `arm64` only. Binary minimum macOS 11.0; `apple-speech` 12.0; `llama-server` 13.3. Info.plist `LSMinimumSystemVersion` = 10.15.
- Signed with the self-signed certificate "Lipik Dev", hardened runtime, no Team ID, not notarised. `codesign --verify --deep --strict`: OK. `spctl --assess`: **rejected** (origin=Lipik Dev).
- Designated requirement = bundle id + the certificate's leaf hash, so Accessibility and Microphone permissions carry over to updates as long as the same certificate signs every release. The "Lipik Dev" certificate and key exist only in Gourav's login keychain: **back them up** like the updater key; if lost, every user must re-grant permissions.
- Entitlements: microphone, audio input, calendars.

What a stranger on macOS 15 (Sequoia) or 26 sees:

1. Clicks "Download Lipik.dmg"; the browser marks the file as downloaded from the internet.
2. Opens the .dmg, drags Lipik onto Applications.
3. Double-clicks Lipik: macOS says "Lipik" Not Opened, "Apple could not verify 'Lipik' is free of malware…", with **Done** and **Move to Trash**. Right-click → Open no longer helps on 15+.
4. Clicks Done, opens System Settings → Privacy & Security, scrolls to the bottom, finds "'Lipik' was blocked to protect your Mac" and clicks **Open Anyway**. This button only shows for about an hour after the blocked launch.
5. Enters the password or Touch ID, then clicks **Open Anyway** once more in the next dialog.
6. Lipik opens setup.

If they run Lipik straight from the mounted .dmg, macOS runs it from a hidden read-only copy; updates and permissions then behave oddly. The Applications shortcut in the .dmg makes this less likely.

## 3. First-run setup (from code + web preview screenshots)

New user (no `onboarding_completed`) → the 11-step `FlowOnboarding`. When it opens, it starts downloading the default speech model in the background.

| # | Step | What can stop the person | If denied or skipped |
|---|---|---|---|
| 1 | Welcome: pick where you'll use it | Continue is greyed until one box is ticked | — |
| 2 | Permissions: Microphone, Accessibility | Continue is greyed until both are allowed; Accessibility sends them to System Settings to flip a switch; 30 s later, "macOS didn't answer" | "Set up later": after setup the app re-checks every 2 s and turns on fn once Accessibility is on; the pill asks for missing permissions |
| 3 | Microphone test (level meter, device menu) | Uses the web view's own microphone stream; if it can't read the mic the copy points to the wrong step (issue #11) | Skip in the corner |
| 4 | Your key (fn), Change key | "Tap it twice" copy (issue #7) | — |
| 5 | Clean-up model | Button "Download the clean-up model (5.0 GB)" (or 2.5 GB). Can move on while it downloads. macOS older than 13.3 → Ollama route ("Get Ollama") | "Skip for now": rules-only clean-up still runs |
| 6 | Practice: hold fn, read a line | Continue greyed until a dictation arrives; shows "Downloading the speech model… N%" until it is ready; fn does nothing until then | Skip |
| 7 | The pill | — | Skip |
| 8 | AI apps (Claude, Cursor, Codex, ChatGPT) | Continue below the fold with several apps (issue #13) | Skip |
| 9 | Privacy (says the anonymous usage count is on) | — | — |
| 10 | Email updates (ticked, email empty) | Continue with an empty box just moves on | Skip |
| 11 | You're set → Open Lipik | Shows speech model progress if still downloading | — |

Permissions not asked in setup: Calendar and System Audio Recording (Notetaker only, asked when first used), Speech Recognition (Apple's engine, for Hinglish on macOS 26). Input Monitoring is not needed (the fn listener is an active event tap, covered by Accessibility).

**Downloads and their safety nets (verified in code):**

- Speech model `parakeet-unified-en-0.6b` Q8_0, 731 MB from `huggingface.co/lipik/…`: 4 tries (4 parallel connections, then 1), resumes a partial file, 60 s stall watchdog, SHA-256 check (a bad file is deleted and downloaded again). No free-disk check.
- Clean-up model from Hugging Face: Qwen3 8B Q4_K_M (5.03 GB, `Qwen/Qwen3-8B-GGUF`) on Macs with 16 GB or more, Qwen3 4B Instruct Q4_K_M (2.50 GB, `unsloth/…`) below. Free-disk check, resume, SHA-256, cancel button. Runs in `llama-server` on 127.0.0.1, stops after an hour idle.
- Hinglish meeting model Qwen3-ASR-1.7B Q8_0 (2.19 GB), only when a meeting is redone in Hinglish; free-disk check.

**8 GB vs 16 GB:** at exactly 16 GB the 8B model is chosen. On this Mac `llama-server` with the 8B model uses 4.3 GB of memory, plus about 1 GB for the speech model. On an 8 GB Mac the 4B model should need about 2.5–3 GB; with a browser open that will swap. Not measured on an 8 GB Mac: it's in the checklist.

**Screenshots:** all 11 steps were taken at 1280x800 from the Vite preview (`?flowPreview=1&theme=light&onboarding=<step>`) in headless Chrome with `--mute-audio`, then the server was stopped. Copy problems found there are issues #7, #11, #13 and #14.

## 4. Download times on this connection

One file at a time, a 300 MB ranged `curl` from the exact URLs the app uses (single connection, written to /dev/null):

| File | Size | Measured | Time for the whole file here | at 50 Mbit/s (6 MB/s) | at 20 Mbit/s (2.5 MB/s) |
|---|---|---|---|---|---|
| Lipik.dmg (GitHub) | 23 MB | 4.6 MB/s | 5 s | 4 s | 9 s |
| Speech model, Parakeet | 731 MB | 30.5 MB/s | 24 s | 2 min | 5 min |
| Clean-up, Qwen3 4B Instruct | 2.50 GB | 31.3 MB/s | 1.3 min | 7 min | 17 min |
| Clean-up, Qwen3 8B | 5.03 GB | 30.1 MB/s | 2.8 min | 14 min | 34 min |
| Hinglish, Qwen3-ASR-1.7B | 2.19 GB | 18.8 MB/s | 1.9 min | 6 min | 15 min |

The app uses 4 connections for the speech model, so it may be faster than one-connection `curl`. Time to first token: 0.8–1.0 s for every Hugging Face file.

## 5. Old first-run errors in the log

`~/Library/Logs/com.gourav.lipik/lipik.log` (7 MB) only starts on 25 Oct, so the colleague's fresh-install download storm (before 0.1.6) isn't in it. There are no download or SHA-256 errors in it. Warnings, counted:

- 353 × "Notetaker system audio stopped; reconnecting"
- 99 × hands-free `fn+space` "cannot be expressed via Carbon" (the Secure Input fallback)
- 7 × clean-up fell back to rules after 4 s (5 built-in engine, 2 Ollama)
- 4 × "Qwen3-ASR-1.7B is not in the catalog" (29 Oct 16:56)

Fixes confirmed in the 0.1.12 release commit (b4c99ad7):

- 9699180b: a fresh install no longer starts the same download dozens of times. The core ignores a second request for a model already downloading, and setup remembers it asked.
- 2f7936e3 + b0857ffd: the Hinglish model is found by its model-list id; download progress and errors are shown.
- dca18ad9: setup goes on while the clean-up model downloads.
- 02e8ae8b: meeting notes wait for a downloading model.

## 6. Ten-minute check on a new macOS account or a friend's Mac

Before you start, note: Mac model, memory (Apple menu → About This Mac), macOS version, and the connection speed (fast.com).

1. **(1 min) Install from Terminal.** Paste `curl -fsSL https://lipik.pages.dev/install.sh | sh`. Note: total time, whether macOS shows any warning, where it installed (/Applications or ~/Applications), whether Lipik opens by itself.
2. **(1 min) Permissions step.** Click Allow on Microphone, then Accessibility. Note: how many system dialogs appeared, whether System Settings opened at the right switch, and how many seconds after flipping the switch the row turned "Allowed". Did a second, different microphone prompt appear on the Microphone test step?
3. **(30 s) Microphone test.** Speak. Note: did the lines move and "Lipik can hear you" appear?
4. **(1 min) Clean-up.** Note the size the button shows (2.5 GB on 8 GB Macs, 5.0 GB on 16 GB). Click it, then Continue straight away.
5. **(2 min) Practice.** Note how long "Downloading the speech model… N%" stayed, then hold fn and read the line. Note: the seconds from letting go to text, and whether the text is cleaned (no "um", punctuation).
6. **(30 s) fn and the Globe key.** In Notes, hold fn, speak, let go. Note: did the emoji picker or an input-source switch pop up? Check System Settings → Keyboard → "Press 🌐 key to" and write down its value.
7. **(30 s) Hands-free.** Tap fn quickly twice, then once, the way the "Your key" copy says. Note what each does.
8. **(1 min) Clean-up after it lands.** When clean-up says ready, dictate a 3-sentence paragraph with an "um" and a "no, wait" in Slack or Mail. Note: cleaned or not, and the seconds it took. On an 8 GB Mac also open Activity Monitor → Memory and note `llama-server` memory and Memory Pressure colour.
9. **(1 min) Update path.** With Lipik running, run the curl line again. Note: did it close Lipik and finish, and did fn still work afterwards without re-granting Accessibility? If an AI app was connected in setup, note whether it said "Couldn't close Lipik".
10. **(1 min) The .dmg route (a second account, or after dragging Lipik to the Bin).** Download from https://lipik.pages.dev with Safari, drag to Applications, open. Note: each dialog's exact wording on this macOS version, and how long it took to find "Open Anyway".

Send back: the notes above, plus `~/Library/Logs/com.gourav.lipik/lipik.log` from that account if anything failed.
