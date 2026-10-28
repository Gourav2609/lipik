# Building Lipik

Lipik is built and tested on macOS (Apple silicon and Intel). The Windows and
Linux build code comes from the upstream app credited in README.md and is not
tested here; that project's own BUILD.md covers those platforms.

## You need

- [Rust](https://rustup.rs/) (latest stable)
- [Bun](https://bun.sh/)
- Xcode Command Line Tools: `xcode-select --install`
- For Apple's on-device dictation: the macOS 26 SDK (Xcode 26 or its Command
  Line Tools). With an older SDK the build makes a stub helper and Lipik uses
  the local speech model only.
- [CMake](https://cmake.org/) (`brew install cmake`) and the internet on the
  first build: `scripts/fetch-llama-server.sh` (run by the build) downloads
  llama.cpp's pinned source and builds `llama-server`, the built-in clean-up
  engine, once (about a minute).

## Run and build

```bash
git clone <this repository> lipik
cd lipik
bun install
bun run tauri dev      # development build with live reload
bun run tauri build    # release .app and .dmg
```

The release bundle is written to `src-tauri/target/release/bundle/`, and the
executable inside it is `lipik`.

If CMake fails with a policy error, run the same command with
`CMAKE_POLICY_VERSION_MINIMUM=3.5` in front.

## Signing

`src-tauri/tauri.conf.json` signs with a local certificate named "Lipik Dev".
Without it, build with an ad-hoc signature:

```bash
APPLE_SIGNING_IDENTITY=- bun run tauri build
```

[docs/INSTALL.md](docs/INSTALL.md) has the details.

## Checks

```bash
bun run lint
bun run format:check
cd src-tauri && cargo test --lib
```

## Accessibility stays "on" after a rebuild

Each ad-hoc build has a new code signature, so macOS may keep showing Lipik
as allowed under **System Settings → Privacy & Security → Accessibility**
while the new build is refused. Quit Lipik, reset only that permission, and
open it again:

```bash
osascript -e 'tell application id "com.gourav.lipik" to quit' || true
tccutil reset Accessibility com.gourav.lipik
open /Applications/Lipik.app
```

Grant Accessibility again when asked. Microphone and other permissions are
not touched.
