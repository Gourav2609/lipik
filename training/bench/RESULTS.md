# Speech model benchmark: Indian English and Hinglish (Roman script)

Date: 2026-10-25. Silent run: audio made with `say -o`, never played; no mic.

## What was tested

- **40 sentences** (`sentences.json`): 15 Indian-English work sentences, 15
  Hinglish in Roman letters, 10 with numbers and dates (5 English, 5 Hinglish).
- **160 clips** (`make_audio.py`): English lines read by Rishi, Aman, Tara
  (en_IN) and Samantha (en_US, the non-Indian control); Hinglish lines read by
  Lekha (hi_IN) plus Rishi, Aman and Tara. Isha and Veena are not installed on
  this Mac, so Aman and Tara took their place. 16 kHz mono WAV, 0.3 s of
  silence before and 0.6 s after each clip. 639 s of audio in all.
- **Engine**: Lipik's own headless mode (`lipik --transcribe-file --repeat 3
--json`), so every model ran through the same transcribe-cpp code the app
  uses, on Metal. `run_bench.py` runs a portable copy of the debug binary from
  `app/` (a `portable` marker sends its settings and models to `app/Data`), so
  the running app and its settings were never touched. The language setting
  was written into that copy's settings before each model.
- **Models** (all Q8_0 GGUF from the app's catalog, hashes checked):
  Parakeet Unified en 0.6B (current default), Whisper large-v3-turbo with
  language `en` and `hi`, Qwen3-ASR 0.6B with `en`, `hi` and `auto`, Qwen3-ASR
  1.7B with `en` and `hi`. No whisper.cpp install was needed: the app's engine
  loads all of them.
- **Scores** (`score.py`):
  - WER = word error rate, CER = character error rate, both in %. Lower is better.
  - Strict: lowercase, no punctuation, hyphens inside words dropped, numbers
    spelled out in words on both sides ("3 pm" = "three pm").
  - Loose: strict, plus any Devanagari output turned into Roman letters by
    simple rules, and common Hinglish spelling variants folded on both sides
    (aa→a, ee→i, oo→u, w→v, ph→f, z→j, ei→e, doubled letters). This shows how
    close a model gets if Lipik added a Devanagari-to-Roman step. The rules
    are rough, so loose scores are a guide, not exact.
  - Speed: decode time is the best of 3 warm runs per clip. "x real time" =
    audio length / decode time.

## Results

### Indian English (15 sentences x 4 voices = 60 clips)

| Model + language          | WER     | CER     | WER Aman | WER Rishi | WER Tara | WER Samantha (US) |
| ------------------------- | ------- | ------- | -------- | --------- | -------- | ----------------- |
| **Parakeet en (default)** | **1.9** | 0.6     | 1.9      | 1.9       | **0.0**  | 3.8               |
| Qwen3-ASR 1.7B, en        | 2.1     | **0.4** | **1.3**  | **1.3**   | 1.9      | 3.8               |
| Whisper turbo, en         | 2.9     | 0.9     | 1.9      | 1.9       | 3.8      | 3.8               |
| Qwen3-ASR 0.6B, en / auto | 5.3     | 1.3     | 5.7      | 3.2       | 6.4      | 5.7               |
| Qwen3-ASR 0.6B, hi        | 51.6    | 22.2    | 49.7     | 47.1      | 63.1     | 46.5              |
| Whisper turbo, hi         | 117.4   | 70.7    | 98.7     | 120.4     | 116.6    | 133.8             |
| Qwen3-ASR 1.7B, hi        | 134.7   | 64.0    | 125.5    | 140.1     | 138.9    | 134.4             |

With language `hi`, all three multilingual models write English sentences in
Devanagari (Whisper sometimes translates), so `hi` cannot be the setting for
English dictation.

### Hinglish in Roman letters (15 sentences x 4 voices = 60 clips)

| Model + language      | WER      | CER      | Loose WER | Loose CER |
| --------------------- | -------- | -------- | --------- | --------- |
| Parakeet en (default) | **61.8** | **17.4** | 57.5      | **15.0**  |
| Qwen3-ASR 0.6B, en    | 67.9     | 35.8     | 49.3      | 25.8      |
| Whisper turbo, en     | 91.6     | 67.0     | 90.0      | 66.8      |
| Qwen3-ASR 1.7B, en    | 138.8    | 81.3     | **38.0**  | 18.7      |
| Qwen3-ASR 0.6B, auto  | 150.7    | 87.9     | 39.5      | 20.1      |
| Qwen3-ASR 0.6B, hi    | 152.0    | 88.7     | 39.6      | 20.0      |
| Qwen3-ASR 1.7B, hi    | 175.0    | 99.8     | 43.8      | 22.9      |
| Whisper turbo, hi     | 166.8    | 94.1     | 46.2      | 21.5      |

Per-voice scores for every model are in `python3 score.py`. Parakeet by
voice: Aman 53.6, Tara 58.6, Rishi 65.0, Lekha 70.0 (strict WER).

Script of the output on the 80 Hinglish clips (both groups):

| Model + language      | Clips written in Devanagari                                                |
| --------------------- | -------------------------------------------------------------------------- |
| Parakeet en           | 0 / 80 (always Roman, but guesses English words: "dal dokie", "late hair") |
| Whisper turbo, en     | 1 / 80 (mostly translates to English or makes things up)                   |
| Qwen3-ASR 0.6B, en    | 18 / 80                                                                    |
| Qwen3-ASR 1.7B, en    | 71 / 80 (the `en` hint does not stop it writing Hindi)                     |
| Qwen3-ASR 0.6B, auto  | 77 / 80 (English words stay in Roman: "slack पे message दाल दू")           |
| Every model with `hi` | 78-80 / 80                                                                 |

### Numbers and dates (10 sentences x 4 voices = 40 clips)

| Model + language     | WER      | CER      | Loose WER | Loose CER | WER Samantha (US, English lines only) |
| -------------------- | -------- | -------- | --------- | --------- | ------------------------------------- |
| Parakeet en          | **30.8** | **12.8** | 30.0      | 11.2      | **9.6**                               |
| Qwen3-ASR 0.6B, en   | 31.5     | 17.2     | 24.2      | 12.0      | 13.5                                  |
| Whisper turbo, en    | 32.8     | 19.8     | 32.0      | 18.6      | 11.5                                  |
| Qwen3-ASR 1.7B, en   | 60.2     | 30.7     | **19.8**  | **8.0**   | 11.5                                  |
| Qwen3-ASR 0.6B, auto | 86.5     | 47.2     | 31.0      | 16.5      | 13.5                                  |

Half of this group is Hinglish, which pulls every score up. On the English
number lines the errors are mostly format, not hearing: "15th March" comes out
as "15 March" or "March 15", "3 pm" as "3 p.m.".

### Speed (warm, Metal, M4 Pro, debug binary; the C++ engine is a release build)

| Model + language     | x real time | Median per clip (3.9 s audio) | Slowest clip | Model load |
| -------------------- | ----------- | ----------------------------- | ------------ | ---------- |
| **Parakeet en**      | **65x**     | **61 ms**                     | 105 ms       | 212 ms     |
| Qwen3-ASR 0.6B, en   | 29x         | 131 ms                        | 294 ms       | 308 ms     |
| Qwen3-ASR 0.6B, auto | 21x         | 164 ms                        | 331 ms       | 325 ms     |
| Qwen3-ASR 0.6B, hi   | 20x         | 203 ms                        | 332 ms       | 325 ms     |
| Qwen3-ASR 1.7B, en   | 12x         | 262 ms                        | 616 ms       | 655 ms     |
| Qwen3-ASR 1.7B, hi   | 9x          | 482 ms                        | 662 ms       | 655 ms     |
| Whisper turbo, en    | 6x          | 620 ms                        | 1,194 ms     | 253 ms     |
| Whisper turbo, hi    | 4x          | 675 ms                        | 2,516 ms     | 248 ms     |

Whisper pads every clip to a 30 s window, so short dictations pay ~0.6 s no
matter how short they are.

## Winners

- **(a) English accuracy: Parakeet en** (1.9% WER) with Qwen3-ASR 1.7B en a
  close second (2.1% WER, lowest CER). Whisper turbo is third.
- **(b) Hinglish in Roman letters:** no model is good.
  - Taking the output as it comes (no conversion step), **Parakeet** is the
    least bad: 61.8% WER, 17.4% CER. It is the only model that always writes
    Roman letters, but it hears Hindi words as English ones.
  - If Lipik added a Devanagari-to-Roman step, **Qwen3-ASR 1.7B** would lead
    (38.0% loose WER), with **Qwen3-ASR 0.6B auto** close behind (39.5%).
  - Whisper turbo is worst either way: with `en` it translates, with `hi` it
    writes Devanagari and is the slowest.
- **(c) Speed: Parakeet**, 65x real time, 61 ms per clip, about 2x faster than
  Qwen3-ASR 0.6B and 4x faster than Qwen3-ASR 1.7B.

## Recommendation

1. **Keep Parakeet Unified en 0.6B as the default.** It wins English accuracy
   and speed, and handles Indian English voices as well as the US control.
2. **Do not ship a "Hinglish mode" that is only a model + language switch.**
   No model and language pair writes Hinglish in Roman letters well: the ones
   that hear Hindi write Devanagari, and the ones that write Roman letters
   don't hear Hindi.
3. If Hinglish matters, the best path to test next is **Qwen3-ASR 0.6B with
   language `auto` plus a Devanagari-to-Roman step** (Lipik's text pipeline
   or the AI clean-up could do it). It keeps English words in Roman letters,
   decodes a 4 s clip in ~160 ms, and scored
   within 1.5 points of the 1.7B model. Qwen3-ASR 1.7B is a little more
   accurate but takes ~0.26-0.6 s per clip. The fine-tuned Whisper plan in
   `training/README.md` (trained to write Roman letters) is the other route.
4. Before building either, repeat this run on a real speaker's voice: ~20 real
   clips (10 English, 10 Hinglish) through `run_bench.py` with real reference
   text. That result should decide it, not this one.

## Limits of this test

- **Synthetic voices are not real voices.** macOS voices reading Roman
  Hinglish are unnatural: en_IN voices read it as English letters, and Lekha
  (hi_IN) reads Roman text with odd stress. Some Hinglish errors are in the
  audio, not the model. Real speech will score differently, likely better for
  the Hindi-capable models.
- Clean studio audio, no background noise, no real mic.
- One reference spelling per Hinglish word; the loose score only partly
  covers spelling variants, and the Devanagari-to-Roman rules are simple.
- English number lines read by `say` may not say a date the way the reference
  writes it, which inflates the numbers-group error for every model.
- Side finding: with no silence after the last word, Parakeet often dropped
  the final word (or a whole clip). English WER was 3.0% on unpadded audio vs
  1.9% padded (`runs-nopad/`). Real dictations end with silence when the user
  pauses before releasing the key, but a fast release could cut a word.

## Re-running

```sh
cd training/bench
python3 make_audio.py               # audio/ + audio/clips.json (silent)
cp ../../src-tauri/target/debug/lipik app/lipik   # refresh the portable copy
printf 'Lipik Portable Mode' > app/portable
ln -f models/*.gguf app/Data/models/              # plus the Parakeet and Whisper GGUFs
                                    # (Whisper turbo was deleted after this run; get it from
                                    # lipik/whisper-large-v3-turbo-gguf)
python3 run_bench.py [config ...]   # ~38 min for all 8 configs
python3 score.py                    # prints the tables
```

`models/`, `app/`, `audio/`, `runs/` and `runs-nopad/` are git-ignored.
