# Training Lipik's speech model on your own voice

This folder turns Lipik's saved dictations into a training set, fine-tunes
Whisper on it with LoRA, checks the result, and converts it into a file
Lipik can load. One adapter per person.

| File          | What it does                                                                                          |
| ------------- | ----------------------------------------------------------------------------------------------------- |
| `export.py`   | history.db + recordings → `data/<speaker>/` (16 kHz WAVs, `train.jsonl`, `val.jsonl`, `summary.json`) |
| `eval.py`     | WER / CER of one or more models on a split (scores only)                                              |
| `train.py`    | LoRA fine-tune; `--dry-run` runs 2 steps and saves nothing                                            |
| `merge.py`    | folds the adapter into the base model, converts to whisper.cpp `.bin`, installs it into Lipik         |
| `textnorm.py` | text clean-up and edit distance shared by export and eval                                             |

`data/`, `runs/`, `models/` and `corrections*.jsonl` are git-ignored. Your
voice and words never go into git.

## Status today (2026-10-25)

history.db has **5 dictations, 50 seconds (0.84 min) of audio**, all made
between 01:45 and 01:55 IST today. All 5 pass the filters (4 train, 1
validation). None has `post_processed_text`, so every label is Parakeet's raw
output. That is far too little to train on: it is enough to test the scripts,
not to change the model. See "How much data" below.

## Recommendation

**Fine-tune Whisper large-v3-turbo (MIT licence) with LoRA, using Hugging
Face transformers + PEFT on the Mac's GPU (MPS). Convert the merged model
with whisper.cpp's converter to a `.bin` and drop it in Lipik's models
folder.**

LoRA in one paragraph: the base model's 809 M weights stay frozen. Next to
each attention layer we train two small matrices (rank 32) whose product is
added to that layer's output. Only 13 M weights (1.6%) learn, so it needs
less memory, little data, and can't wander far from the base model. The
adapter is ~50 MB per person. For Lipik it must be merged back into a full
model, because transcribe.cpp has no LoRA loading (checked in
`transcribe-cpp-sys-0.4.0/src`).

### The three options compared

|                         | Whisper turbo + PEFT on MPS (**pick**)                                                                                                              | Whisper turbo on MLX                                                                                                  | Parakeet (NeMo)                                                                                                         |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ---- | ---------------------------------------------------------------------------------------- |
| Runs on this Mac        | Yes. Measured below                                                                                                                                 | Inference yes (`mlx-whisper`). No maintained training script; would need our own training loop                        | Not in practice. NeMo training targets CUDA; many ops lack MPS kernels, so it falls back to CPU                         |
| Speed per training step | 2.2 s per clip (measured)                                                                                                                           | Likely faster, not measured                                                                                           | CPU only: much slower                                                                                                   |
| Memory                  | 4.3 GB GPU at batch 2 (measured)                                                                                                                    | Lower                                                                                                                 | –                                                                                                                       |
| Back to Lipik           | merge → `convert-h5-to-ggml.py` → `.bin`. transcribe.cpp loads whisper.cpp `.bin` files and knows the turbo shape (`src/arch/whisper/bin_load.cpp`) | Needs our own MLX → PyTorch weight renaming first (conv weights are transposed in MLX). Untested by anyone we know of | Lipik's Parakeet GGUF is made by handy-computer's converter, which is not in this repo or the crate. No known path back |
| Hinglish in Roman       | Yes: multilingual model, `<                                                                                                                         | en                                                                                                                    | >` token, Roman targets                                                                                                 | Same | English-only tokenizer; Roman Hinglish is spellable, but the model has never heard Hindi |
| Licence                 | MIT                                                                                                                                                 | MIT                                                                                                                   | CC-BY-4.0                                                                                                               |

So Parakeet stays Lipik's model for plain English, and the fine-tuned
Whisper becomes a second model you switch to for Hinglish dictation. MLX is
used here only for fast scoring of the base model (`eval.py` `mlx:` backend)
and for finding cut points in long clips.

### Hinglish in Roman letters

Whisper writes the language its first tokens tell it to. With `<|hi|>` it
writes Devanagari; with `<|en|>` it writes Latin letters. So:

1. Training labels are Roman text ("kal ki meeting cancel kar do").
   `export.py` drops any row containing Indian scripts.
2. Every label starts `<|startoftranscript|><|en|><|transcribe|><|notimestamps|>`
   (`--language en`, the default; checked with the real tokenizer).
3. In Lipik, set the language to **English** when using the custom model.
   On "auto", Whisper may detect Hindi and switch to Devanagari.

The base model already writes Roman text with `<|en|>` but tends to turn
Hindi words into English look-alikes. Fine-tuning teaches it your spellings.

### The label problem (read this before training)

Until you edit a transcript in History, the "corrected" text is
`post_processed_text`: the raw text after your dictionary replacements,
snippets and optional AI clean-up. When nothing changed, the label is just
what Parakeet heard. Parakeet is English-only, so on Hinglish it writes the
wrong words, and training on those labels teaches Whisper the same mistakes.

So before training, fix the labels of a few hundred clips. Put your fixes in a
JSONL file and pass it to export:

```
{"file_name": "handy-1792873227.wav", "text": "kal ki meeting cancel kar do"}
```

```
python export.py --corrections corrections.jsonl
```

Rows from the corrections file win over everything else and skip the rewrite
filter. Easier: fix the transcript in Lipik's History page (pencil on the
row). The edit is saved in history.db's `corrected_text` column and is used
next. Because a History edit starts from the text Lipik pasted (after any AI
clean-up), it still gets a looser rewrite check against the raw words: more
than 60% changed is dropped, or more than 85% when the raw text has fewer than
8 words (fixing one name in a short clip is a big share). For heavy Hinglish
fixes that change most words, use the corrections file.

What export does with each row:

| Check          | Default                                                                                                                                                                                       | Reason                                    |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| text source    | corrections file → `corrected_text` (your History edits) → `post_processed_text` (for both, snippet expansions turned back into their triggers, using `textflow.json`) → `transcription_text` | prefer your fixes                         |
| too short      | < 1.0 s or < 2 words                                                                                                                                                                          | not worth a 30 s training window          |
| too long       | > 30 s dropped, or cut at pauses with `--split-long`                                                                                                                                          | Whisper sees 30 s at a time               |
| rewritten      | final text changes > 40% of the raw words (`--max-change`)                                                                                                                                    | AI rewrite, not a correction              |
| rewritten edit | History edit changes > 60% of the raw words, > 85% under 8 raw words (`--max-change-edited`, `--max-change-edited-short`)                                                                     | edit kept an AI rewrite                   |
| script         | any Devanagari/other Indian script                                                                                                                                                            | targets must be Roman                     |
| split          | newest 10% of clips → validation (`--val-from YYYY-MM-DD` instead)                                                                                                                            | by date, so validation is "future" speech |

`--split-long` runs the base Whisper with word timings, matches its words to
the label, and cuts only in a pause where the two agree, picking the widest
pause. Clips with no safe cut are dropped. Tested by forcing 8 s pieces: the
20 s and 10 s clips were cut into 4 and 2 pieces, and the base model scored
0–25% WER per piece (the same range as uncut clips), so the text lined up
with the audio.

## How much data

These are rules of thumb from published Whisper fine-tuning work, not
measured on your voice:

| Reviewed audio | Dictations (~8 s each) | What to expect                                                      |
| -------------- | ---------------------- | ------------------------------------------------------------------- |
| < 20 min       | < 150                  | Too little. LoRA will memorise sentences                            |
| 30–60 min      | 250–450                | **Minimum.** Should pick up your accent and frequent Hinglish words |
| 3–5 h          | 1,400–2,200            | **Good.** Stable gains, spellings settle                            |
| 10 h+          | 4,500+                 | Diminishing returns for one person                                  |

Validation should hold at least 10 min (~75 clips) for WER numbers you can
trust. At ~100 dictations a day, the minimum is 3–5 days of normal use.

## Running it

All commands from this folder. The local pip is broken by design, so use uv
(versions tested here: torch 2.14.1, transformers 5.20.2, peft 0.21.2,
mlx-whisper 0.4.3, Python 3.12):

```bash
# zsh does not split "$UV"-style strings, so use a function
uvp() { UV_INDEX_URL=https://pypi.org/simple UV_EXTRA_INDEX_URL= command uv run --no-config --python 3.12 \
  --with numpy --with torch==2.14.1 --with transformers==5.20.2 --with peft==0.21.2 --with mlx-whisper==0.4.3 "$@"; }

uvp python export.py --stats-only                     # counts only
uvp python export.py --corrections corrections.jsonl --split-long
uvp python eval.py --data data/me --system base=mlx:mlx-community/whisper-large-v3-turbo   # quick baseline
uvp python train.py --data data/me --dry-run       # 2 steps, nothing saved (downloads the 1.6 GB base once)
caffeinate -i uvp python train.py --data data/me   # the real run -> runs/me/adapter
uvp python eval.py --data data/me \
  --system base=hf:openai/whisper-large-v3-turbo \
  --system tuned=hf:openai/whisper-large-v3-turbo@runs/me/adapter
```

`train.py` defaults: rank 32, alpha 64, LoRA on q/k/v/out projections in
encoder and decoder, lr 3e-4 with 10% warm-up then linear decay, batch 2 ×
accumulation 8 (effective 16), 5 epochs, stops after 2 epochs without a
better validation loss and keeps the best one, light SpecAugment (hides
random short stretches of time and frequency). Base weights load in bf16 on
the Mac; the LoRA weights stay fp32. `--max-mem-gb 12` caps GPU memory so a
run fails with "out of memory" instead of pushing the Mac into swap.

Before a real run (heavy-job rule): close big apps, check free memory, and
expect the numbers below.

### Measured on this Mac (M4 Pro, 24 GB)

| What                                             | Result                                                             |
| ------------------------------------------------ | ------------------------------------------------------------------ |
| Training step, turbo-shaped model, bf16, batch 1 | 2.7 s per clip, 3.3 GB GPU                                         |
| Training step, batch 2 (default)                 | **2.2 s per clip, 4.3 GB GPU**, 3.8 GB process RSS                 |
| Training step, fp32, batch 1                     | needs more than 4 GB GPU (hit the 4 GB cap); not measured above it |
| Base model scoring (`mlx`, fp16)                 | 0.04× real time, 1.9 GB                                            |

The timing runs used random weights with the exact turbo shape (no
download), which costs the same compute as the real weights.

Whisper pads every clip to 30 s, so cost depends on the number of clips, not
their length:

| Average dictation     | Clips per hour of audio | One epoch per hour of audio | 5 epochs |
| --------------------- | ----------------------- | --------------------------- | -------- |
| 7 s (today's average) | ~510                    | ~19 min                     | ~1.6 h   |
| 15 s                  | ~240                    | ~9 min                      | ~45 min  |

Add a validation pass per epoch (forward only, cheaper than a training step).
Joining consecutive short clips into ~25 s training items would cut this
2–3× (not built).

### Back into Lipik

```bash
git clone --depth 1 https://github.com/ggml-org/whisper.cpp ~/src/whisper.cpp
git clone --depth 1 https://github.com/openai/whisper ~/src/whisper
cmake -S ~/src/whisper.cpp -B ~/src/whisper.cpp/build && cmake --build ~/src/whisper.cpp/build --target whisper-quantize -j

uvp python merge.py --adapter runs/me/adapter --out models/me-hf \
  --whisper-cpp ~/src/whisper.cpp --openai-whisper ~/src/whisper \
  --quant q5_0 --install whisper-turbo-me
```

This writes `~/Library/Application Support/com.gourav.lipik/models/whisper-turbo-me.bin`
(~550 MB at q5_0, ~1.6 GB at f16). Lipik lists any `.bin`/`.gguf` in that
folder as a custom model (`discover_custom_transcribe_models` in
`src-tauri/src/managers/model.rs`). Pick it, set language to English.

`merge.py` copies the base repo's `vocab.json` and `added_tokens.json` into
the merged folder because the converter reads them and transformers 5 no
longer saves them.

## Friends (one adapter each)

Each friend runs Lipik on their own Mac for a few days, then copies their
`~/Library/Application Support/com.gourav.lipik/` (`history.db` +
`recordings/` + `textflow.json`) to you, with their consent:

```bash
uvp python export.py --speaker priya --app-dir ~/lipik-data/priya --corrections corrections-priya.jsonl
uvp python train.py --data data/priya            # -> runs/priya/adapter
uvp python merge.py --adapter runs/priya/adapter --out models/priya-hf ... --install whisper-turbo-priya
```

The export prints minutes per speaker across `data/`. Each person gets a full
model file for their own Mac.

## Free cloud GPU fallback

`train.py` runs unchanged on NVIDIA (fp16 with loss scaling on T4, bf16 on
newer cards).

| Service             | GPU                  | Free limit                         | Speed vs this Mac |
| ------------------- | -------------------- | ---------------------------------- | ----------------- |
| Kaggle Notebooks    | 2× T4 or P100, 16 GB | 30 GPU hours a week, 12 h sessions | not measured      |
| Google Colab (free) | T4, 16 GB            | a few hours a day, can be cut off  | not measured      |

Upload `data/<speaker>/` as a **private** Kaggle dataset or to your own
Drive, run `pip install torch transformers==5.20.2 peft==0.21.2` and the same
`train.py` command, then download `runs/<speaker>/adapter` (~50 MB) and merge
on the Mac. Your voice recordings then sit on Google's servers; for friends'
data, ask them first.

## What is tested and what isn't

| Part                                                        | State                                                                             |
| ----------------------------------------------------------- | --------------------------------------------------------------------------------- | --- | --- | ---------- | --- | ------------ | ------------ | --------- | --- |
| `export.py` on the real history (normal and `--split-long`) | run                                                                               |
| `eval.py`, `mlx` backend, 3 real clips                      | run: base turbo 3.8% WER / 1.9% CER against Parakeet's text (not true references) |
| `train.py` labels with the real tokenizer                   | checked: `<                                                                       | en  | ><  | transcribe | ><  | notimestamps | >`+ text +`< | endoftext | >`  |
| `train.py --dry-run --random-init`                          | run (timings above)                                                               |
| `train.py` real run, `eval.py` `hf` backend                 | not run (needs the 1.6 GB HF download; no training yet by rule)                   |
| `merge.py` merge step                                       | run on a tiny random model: exactly the 12 LoRA layers changed                    |
| whisper.cpp conversion + loading in Lipik                   | not run; converter arguments checked against whisper.cpp master                   |
