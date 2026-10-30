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

### Back up the release keys

Two files exist only on the release Mac. Keep a copy of each somewhere safe
(an encrypted drive or a password manager), outside this repo:

- The "Lipik Dev" certificate and its private key, from the login keychain
  (Keychain Access → My Certificates → Lipik Dev → Export, as a .p12 with a
  password). macOS ties Accessibility and Microphone permissions to this
  certificate, and the installer checks it. Signed with a new one, every
  release looks like a different app: each user has to grant permissions
  again, and `install.sh` refuses the download until its certificate hash is
  updated. If the certificate ever changes, update `LEAF` in
  `website/install.sh` (from `codesign -dr - Lipik.app`) and deploy the site
  in the same release: until both are live, every Terminal install fails.
- `~/.tauri/lipik-updater.key`. The app checks every update against it.
  Without it no new release can be installed with Update now, and every
  user has to download and install Lipik by hand once.

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
