# Speech-to-text benchmark

Measures how well each dictation pipeline turns speech into text, on two clip
sets. Nothing here uses the microphone, plays audio or touches the installed
Lipik: every binary runs from a portable copy in `apps/` (git-ignored).

Two scores per clip:

- **WER** (word error rate, %): word edits needed to turn the output into the
  reference, divided by the reference's words. Both sides are lower-cased,
  punctuation is dropped and numbers are written as digits first
  (`training/textnorm.py`, `normalize_wer`). Lower is better.
- **Ready to send**: the output equals the reference exactly, capitals and
  punctuation included. Only a missing or extra final period is forgiven.

Plus seconds per clip, model time and peak memory.

## Pipelines

| Name in results | What runs |
| --- | --- |
| Lipik, final text | Release binary (`src-tauri/target/release/bundle/macos/Lipik.app`), Parakeet Unified en 0.6B Q8, then Lipik's text steps with an empty dictionary and clean-up through local Ollama `qwen3:8b`. `--clean=<app bundle id>` |
| Lipik, raw Parakeet text | Same run, the `text` field before the text steps |
| Handy upstream | cjpais/Handy `main`, release build, same Parakeet model, no clean-up |
| Apple Dictation en-IN | Apple's on-device `DictationTranscriber` (`apple.swift`) |
| Qwen3-ASR 1.7B, raw | Through Lipik's binary; mainly the third opinion for draft references |
| Whisper large-v3-turbo | Only if its GGUF is already on this Mac (it was not on 2026-10-27) |
| Lipik as typed at the time | `history.db` text, private dictations only (unpadded live audio) |

## Clip sets

**Private** (`private/`, git-ignored, never commit): my own voice.

- 43 dictations from `~/Library/Application Support/com.gourav.lipik/recordings/`.
- ~150 meeting pieces of 3–20 s cut from `notetaker/<id>/mic.wav` (me) and
  `system.wav` (others) at the segment times in `notetaker.db`, only meetings
  with status `ready`.
- Every clip gets 0.3 s of silence before and 0.6 s after (Parakeet drops the
  last word otherwise), 16 kHz mono 16-bit. Dictations are padded too, so all
  pipelines hear the same audio.
- References are **drafts** made by models (see below) until corrected by hand.
  Note: with no headphones the mic also picks up the other side of the call,
  so "my voice" pieces can contain other voices.

**Public** (`public/`): ~150 clips of AI4Bharat's Svarah (Indian-accented
English, CC BY 4.0, https://huggingface.co/datasets/ai4bharat/Svarah). Audio is
git-ignored; `clips.json` and `references.json` can be committed. Svarah's
references are plain transcripts, so read WER; ready to send is not meaningful
there.

## Draft references (private set)

`make_refs.py` builds one per clip:

1. Apple en-IN and Parakeet give the same words: keep Parakeet's text.
2. Otherwise run Qwen3-ASR 1.7B and keep whichever of the three is closest in
   words to the other two (ties: Qwen3-ASR, then Parakeet).

All are marked `"checked": false`. They favour the model they came from,
mostly Parakeet, so Parakeet-based rows look better than they are. Correct
them in `private/review.html` (open it in a browser; it works offline): listen,
fix the text, tick "checked", press Save, then move the downloaded
`references.json` to `private/references.json` and rerun `score.py`. Hand-checked
references are kept when `make_refs.py` runs again.

## How to rerun

```sh
cd training/benchmark

# 1. Private clips (reads Lipik's data read-only)
python3 prepare_private.py

# 2. Run the pipelines, one at a time. Reruns skip finished clips and retry failures.
python3 run.py --set private --contender lipik
python3 run.py --set private --contender handy
python3 run.py --set private --contender apple-en-IN
python3 make_refs.py        # writes private/need_third.txt for clips where Apple and Parakeet differ
python3 run.py --set private --contender qwen3-asr-1.7b   # all clips (or --ids private/need_third.txt)
python3 run.py --set private --contender history
python3 make_refs.py        # final drafts + private/review.html

# 3. Scores
python3 score.py            # RESULTS.md (public) and private/RESULTS.md
```

Wrap long runs in a timeout, e.g. `perl -e 'alarm 3600; exec @ARGV' python3 run.py ...`.
Each clip already runs under a 300 s limit.

### Public set (needs a Hugging Face login)

Svarah is gated. Log in at huggingface.co, open the dataset page, accept the
terms (access is granted straight away), and create a read token. Then:

```sh
python3 -m venv .venv
.venv/bin/pip install -i https://pypi.org/simple pyarrow huggingface_hub
HF_TOKEN=hf_... .venv/bin/python prepare_public.py --clips 150
for c in lipik handy apple-en-IN qwen3-asr-1.7b; do python3 run.py --set public --contender $c; done
python3 score.py
```

`prepare_public.py` fetches only ~15 row groups (about 100 rows each) of the
parquet files, not the whole 1.1 GB.

### Building Handy upstream

```sh
git -C ~/Documents/repos/flowkit fetch upstream
git -C ~/Documents/repos/flowkit worktree add --detach ~/Documents/repos/handy-bench upstream/main
cd ~/Documents/repos/handy-bench
export PATH="$(brew --prefix rustup)/bin:$HOME/.bun/bin:$PATH"
bun install && bun run build
cd src-tauri && CARGO_BUILD_JOBS=4 cargo build --release   # ~7 min
```

`run.py` copies `target/release/handy` into `apps/handy/` with a `portable`
marker and links the Parakeet model from `~/.cache/huggingface/hub`.

## Files

| File | What it does |
| --- | --- |
| `prepare_private.py` | Cuts the private clips into `private/audio/`, writes `private/clips.json` |
| `prepare_public.py` | Fetches the Svarah subset into `public/` |
| `run.py` | Runs one pipeline on one set; results in `runs/<set>-<pipeline>.jsonl` (`private/runs/` for the private set) |
| `apple.swift` | Apple DictationTranscriber over a list of WAV files |
| `make_refs.py` | Draft references + `private/review.html` |
| `score.py` | `RESULTS.md` and `private/RESULTS.md` |
