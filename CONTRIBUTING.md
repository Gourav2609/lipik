# Contributing to Lipik

Thanks for helping. Lipik is a macOS dictation app built on
[Handy](https://github.com/cjpais/Handy) by CJ Pais. Changes to the shared
recording and transcription engine may be better sent to Handy itself.

## Reporting a bug

Search this repository's Issues first. If it is new, open an issue with the
bug report template: what happened, your macOS version, and the log file
(`~/Library/Logs/com.gourav.lipik/lipik.log`) if you can.

## Ideas and feature requests

Open an issue that describes the problem you want solved. Talk it through
there before writing a large change.

## Setting up

You need [Rust](https://rustup.rs/), [Bun](https://bun.sh/) and the Xcode
Command Line Tools.

```bash
git clone <your fork> lipik
cd lipik
bun install
bun run tauri dev
```

See [BUILD.md](BUILD.md) for details and [docs/INSTALL.md](docs/INSTALL.md)
for signing.

## Making a change

1. Create a branch from `main`.
2. Keep the change small: one fix or feature per pull request.
3. Follow [docs/DESIGN.md](docs/DESIGN.md) for anything people see,
   including the wording rules in its "Voice and copy" section.
4. Run the checks:

   ```bash
   bun run lint
   bun run format:check
   cd src-tauri && cargo test --lib
   ```

5. Open a pull request and fill in the template.

## Licence

By contributing you agree that your work is released under the MIT licence
in [LICENSE](LICENSE).
