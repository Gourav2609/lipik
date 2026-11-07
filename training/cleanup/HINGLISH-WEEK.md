# Hinglish dictation week

Goal: get the Hinglish dictation number (0/20 in `RESULTS.md` and
`RESULTS-0.1.12.md`) up honestly. Test lines are made up and the voices are
synthetic (`say`); no private recordings are used.

## Day 1 (1 Nov 2026): baseline and diagnosis

No app changes. Lipik 0.1.13 (the installed release, branch `premium-design`).

### How the 20 clips were made

- 10 made-up lines (`cases.json`, `hng01`-`hng10`, from `make_cases.py`), each
  with the spoken line (with "um", "uh", a "nahi nahi" correction, a repeated
  "hum, hum") and the expected Roman Hinglish text.
- Each read by two macOS voices with `say -o` (`run.py audio`): **Lekha**
  (`hi_IN`, a Hindi voice) and **Aman** (`en_IN`). 16 kHz mono, 0.3 s
  silence before, 0.6 s after.
- Problem found today: the spoken lines are in Roman letters, and Lekha
  reads Roman letters with English letter sounds. Qwen3-ASR heard her
  "mujhe lagta hai" as "Magic Light High". See the check below.

### How "fully right" is scored (`score.py`)

Not exact text. A Hinglish clip passes when all of these hold:

1. the case's own checks: filler gone (hng01, hng02), "nahi nahi"
   correction applied (hng03), "?" on questions (hng05, hng06, hng10),
   repeated "hum" removed (hng07);
2. no Devanagari, no "Here's the cleaned text";
3. for clean-up output: nothing dropped (a word in the transcript and the
   expected text that the clean-up removed) and nothing added (a word in
   neither the transcript, the spoken line nor the expected text);
4. "loose" word error rate at most 30%. Loose (`training/bench/score.py`)
   lowercases, drops punctuation, spells numbers out, and folds vowel length
   (aa=a, ee=i, oo=u), w=v, ph=f, z=j, ei=e and doubled letters.

Is it fair for Roman Hinglish? Mostly, and it is lenient in places:

- Folded already: theek/thik, aaunga/aunga, pucho/poocho, woh/voh.
- Not folded: hai/he, hain/hai, nahi/nahin, mujhe/muje, kya/kia, yeh/ye,
  hoon/hu, and words run together ("hogaya" for "ho gaya", "boloki" for
  "bolo ki"), which count as 2 errors. Hindi number words ("aath") do not
  match digits ("8").
- Lenient: the 30% bar lets about 3 wrong words through in a 10-word line
  (hng02 passes with "Bill" for "build" and "logs" missing).
- The "added" check fails a clean-up that respells a misheard word
  ("kadu" -> "kardu") or splits a joined one ("AajRaat" -> "Aaj raat").

Measured effect: putting the 30% bar on the fairer score below instead of
loose WER moves the clean-up-on counts by 0 to +2 clips per path, and
Parakeet from 0 to 4. The scoring is not why the number is 0.

### A second, fairer score: hWER (`hinglish_score.py`)

Kept next to the strict one, never replacing it. Word error rate on the
loose text, then on every word that is not a common English word
(`textflow/common_words.txt`; Hindi words from `textflow/hindi.rs` plus
main/hum/ham/me/he/pe/par/na/to are folded anyway):

1. `ai` -> `e` (hai/he, hain/hen)
2. a final `h` after a vowel goes (yeh/ye, woh/wo)
3. a final `n` after a vowel goes (nahin/nahi, hoon/hu)
4. `jh` -> `j`, `chh` -> `ch` (mujhe/muje)
5. consonant + `ia` -> `ya` (kia/kya)
6. Hindi number words -> the number (aath = 8, chhe = 6; not ek or do)
7. an output word that is two neighbouring expected words run together is
   split ("hogaya" = "ho gaya").

Both sides get the same treatment. Known merge: main/mein/mai become one
word. `python3 hinglish_score.py --selftest` checks 15 pairs that must
match and 7 that must not.

### What was run (`hinglish_run.py`)

| Path                                | How                                                                                                                                                                                                                                                                                              |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| a. Parakeet English                 | portable byte-identical copy of the installed 0.1.13 `lipik` and `apple-speech`, `speech_engine: parakeet`; `--transcribe-file --model <Parakeet> --clean=<bundle> --json --repeat 3`                                                                                                            |
| b. Auto (Hinglish mode)             | the same copy with `speech_engine: auto`, the app's default: Parakeet and the bundled `apple-speech` helper (Apple `DictationTranscriber`, `en-IN`, dictionary words as context) run on the same audio and `pick.rs` keeps one. It picked Apple on 20 of 20                                      |
| c. Qwen3-ASR 1.7B + Roman step      | `lipik/Qwen3-ASR-1.7B` Q8_0 through transcribe-cpp 0.4.0 (the app's library), language `en` as the notetaker uses it; then the notetaker's Roman-letter step (its prompt read from `notetaker/hinglish.rs`, asked again once when it translated, letter rules if it fails) on the clean-up model |
| c'. Qwen3-ASR 0.6B + Roman step     | same, 0.6B Q8_0                                                                                                                                                                                                                                                                                  |
| d. Qwen3-ASR 1.7B + context + Roman | as c, with the context text "Hinglish: Hindi and English mixed, Hindi written in Roman letters like WhatsApp (hai, nahi, kya, karo, kal, abhi, mujhe, yaar, bhai, matlab). Names: Rivaan, Meher, Zubin, Tavish, Ishita, Oshin."                                                                  |

Clean-up for every path: Qwen3-8B Q4_K_M (the app's file) on a copy of
0.1.13's `llama-server` with engine.rs's arguments. Paths a and b go through
the real binary, which calls the server through `cli_shim.py` as in
`RESULTS-0.1.12.md`. The binary cannot take text as input, so c and d use
`run.py`'s mirror of the clean-up (`--prepass --parts --num-predict
--vocab-in-prompt`). Check: the mirror run on a's and b's own transcripts
gave the same pass counts and WER as the binary (0/20 and 7/20, identical
WER). The installed app and its data folder were not touched; one heavy job
at a time under the shared lock.

### Results

All 20 clips (10 Lekha, 10 Aman). Strict = `score.py`, unchanged. hWER and
loose WER are pooled over the 20 lines. Speed is the median per clip (clips
are 4.4 s of audio): speech = warm transcription (best of 3 for a and b,
including the wait for Apple in b; one model load then decode for c and d),
Roman = the Roman-letter call, clean-up = time in the model (0 when the app
skips it).

| Path                                | Strict, clean-up off | Strict, clean-up on | hWER off / on | loose WER off / on | Speech | Roman  | Clean-up |
| ----------------------------------- | -------------------- | ------------------- | ------------- | ------------------ | ------ | ------ | -------- |
| a. Parakeet English                 | 0/20                 | 0/20                | 49.5 / 48.6   | 61.2 / 60.3        | 55 ms  | -      | 564 ms   |
| b. Auto (Apple en-IN)               | 6/20                 | **7/20**            | 33.6 / 34.1   | 37.9 / 38.3        | 329 ms | -      | 546 ms   |
| c. Qwen3-ASR 1.7B + Roman           | 9/20                 | **11/20**           | 29.0 / 26.2   | 30.8 / 27.1        | 505 ms | 625 ms | 568 ms   |
| c'. Qwen3-ASR 0.6B + Roman          | 6/20                 | 6/20                | 36.4 / 35.5   | 41.1 / 40.2        | 190 ms | 290 ms | 591 ms   |
| d. Qwen3-ASR 1.7B + context + Roman | 9/20                 | **11/20**           | 27.6 / 24.3   | 28.5 / 24.8        | 401 ms | 569 ms | 594 ms   |

- a matches 0.1.12 exactly (same transcripts, same 61.2 / 60.3 WER).
- By voice, clean-up on: auto Lekha 2/10, Aman 5/10; Qwen 1.7B + context
  Lekha 5/10, Aman 6/10 (hWER 30.8 vs 17.8).
- With the 30% bar on hWER instead of loose WER (informational): a 4, b 7,
  c 12, c' 8, d 11.
- The context text made Qwen write one line straight in Roman letters
  ("Aaj rat dinner pe chalein kya? Main 8 baje free hoon.") and cut hWER by
  2 points; same pass count.
- Clean-up never translated: 0 of 47 real clean-up replies lost the Hindi
  (the 26 Oct problem is fixed), no HinglishLost rejections.

**Check of the test audio**: the same 10 lines written in Devanagari
(`hinglish_devanagari.json`) and read by Lekha (`hinglish_run.py devaudio`):

| Lekha clips                   | Auto, strict on | Auto hWER on | Parakeet, strict on |
| ----------------------------- | --------------- | ------------ | ------------------- |
| Roman lines (the test set)    | 2/10            | 43.0         | 0/10                |
| Devanagari lines (check only) | 7/10            | 16.8         | 0/10                |

### Why 0/20

1. **The benchmark measured the wrong path.** The test copy had
   `speech_engine: parakeet`; the app's default is `auto`, which on macOS 26
   hands Hindi to Apple's en-IN dictation. Parakeet only knows English, so
   it writes look-alike English words. Through auto, today's app is 7/20.
   0/20 is still true for Macs older than macOS 26 (no Apple engine).
2. **Half the audio is bad.** Lekha reading Roman letters is garbled; read
   from Devanagari the same voice goes from 2/10 to 7/10 on the auto path.
3. **The speech models still mishear** on clean audio.
4. **The clean-up misses Hinglish things**, mostly because short lines never
   reach the model.
5. **Spelling and scoring** cost little: 0-2 clips per path, 4 for Parakeet.

Three examples per failure type (made-up test lines):

Speech model, clean audio (auto path, Aman):

- "bhai woh PR merge kar de please, tests pass ho gaye hain" -> "Bhai Woh kiya Marge kar The please paas Ho Gayi Hai"
- "matlab deploy aaj shaam tak ho jayega na" -> "Matlab LayalAaj Shyam tak Ho Jayega"
- "aaj raat dinner pe chalein kya, main aath baje free hoon" -> "AajRaat din Pe Chale Kya Main 8 free hoon" ("baje" lost)

Test audio (Lekha reading Roman letters):

- "mujhe lagta hai ki hum, hum log Friday ko launch..." -> Qwen 1.7B: "Magic Light High ki hum hum Lord Friday..."
- "bhai woh PR merge kar de please, tests pass ho gaye hain" -> Apple: "Bhai Vapi merge kar De please test spouse Hogi haan"
- "matlab deploy aaj shaam tak ho jayega na" -> Apple: "Main deploy Aaj Shyam tak Ho Jaya Gana"; from Devanagari: "Matlab deploy Aaj Shyam tak Ho Jayega"

Clean-up:

- "main paanch baje aaunga, nahi nahi, chhe baje" (all paths): under 8 words, so the model is skipped and the rules don't know "nahi nahi"; on the Qwen path the model removed "nahi nahi" but kept "punch baje anga".
- "aaj raat dinner pe chalein kya..." (auto, Aman): the model's good reply "Aaj Raat din pe chale kya main 8 free hoon?" was rejected by the added-words check because it split "AajRaat".
- "ek kaam karo, Zubin ko call karo..." (auto, Lekha): the clean-up dropped "Ek kaam karo". Also "...ho jayega na" left without "?" on every path.

Roman spelling:

- Roman step changed words: "मुझे लगता है" -> "Main lagta hai"; "कर दो" -> "karo"; "शाम" -> "shaaM".
- Strict score counts joined words twice: Parakeet's "Meher ko boloki invoice kal takda..." fails only on that (hWER under 30%).
- hai/he, nahin/nahi variants appear (Apple writes "nahin", "Hai"); folded by loose only partly.

### Plan for the week

1. **Fix the measurement first (day 2).** Run the benchmark on the app's
   default (`auto`) and keep Parakeet-only as the pre-macOS-26 row. Add a
   second Lekha set read from Devanagari and more made-up lines (about 40,
   so one clip is not 5%), keep the old 20 clips and report them as they
   are. Report strict and hWER side by side. Expected: auto about 7/10 on
   clean audio instead of 0/20; no app change.
2. **Clean-up fixes for Hinglish (app).** Send short Hinglish lines with a
   correction cue ("nahi nahi") or a question ending ("kya", "na") to the
   model, or handle them in the rules; let the added-words check accept
   re-spacing ("AajRaat" -> "Aaj raat"); find why "Ek kaam karo" was
   dropped. Expected: +1 to +3 of 20 on the auto path with today's clips
   (hng10 Aman passes once its reply is accepted; hng03 and hng05 also need
   the speech fixed), more on clean audio.
3. **Better speech for Hindi (test before building).** Qwen3-ASR 1.7B +
   Roman step + context is the best path (11/20, hWER 24 vs auto 34), but
   costs a 2.2 GB model and about 1.1 s more per dictation (decode + Roman
   call), and the Roman step makes its own mistakes. Try it as the auto
   path's Hindi engine when the model is on the Mac, compared with Apple on
   the new clean-audio set. 0.6B is not good enough (6/20).

## Day 2 (1-2 Nov 2026): a fairer test set and clean-up fixes

### New test set

- 40 new made-up lines (`hinglish40.json`), written in Roman and Devanagari.
  Same names as before (Rivaan, Meher, Zubin, Tavish, Ishita, Oshin).
- Every voice reads the Devanagari line, so Hindi words get Hindi sounds
  (`hinglish40_audio.py`): Lekha (`say`), Indic Parler-TTS Rohit and Divya
  (20 lines each), and Gourav's cloned voice (Chatterbox, his OK). 120
  clips, kept in `training/benchmark/private/hinglish40/` (git-ignored).
- A different speech model (whisper-hindi2hinglish-apex, not one of the
  tested paths) heard each take; the best of 2 takes per line was kept.

### App changes (textflow, not released yet)

1. Hinglish questions get "?" in the rules: a Hindi question word first or
   last (anywhere in a sentence of 7 words or fewer), or a closing "na"
   after a verb. Not after pata/bata/pucho, not "kaise bhi", not after
   "bola ki".
2. A single "nahi" is no longer a correction cue (it is usually "not": "ya
   nahi"). "nahi nahi" is, and so is "nahi" before a number ("paanch baje,
   nahi, chhe baje"). Commas between cue words are allowed.
3. Short Hinglish dictations (under 8 words) with a correction go to the
   model; the rules can't apply them.
4. The added/dropped checks accept a split word ("AajRaat" -> "Aaj raat").
5. New reply check: a reply that leaves out a phrase or a real word with
   nothing in its place is rejected ("Ek kaam karo" dropped).

A separate review found 7 problems in the first version (comma between
"nahi, nahi", wrong "?" on "kal ki meeting kab hai" and "kaise bhi karke",
"Sorry yaar, late ho gaya" sent to the model, "okay so" counted as a lost
phrase, extra work in the check); all fixed with tests. `cargo test --lib
textflow`: 81 passed. `guards.py` (the Python copy) matches the Rust
examples, 215 of 216; the one miss (filler "UM") was already there before.

### Results (made-up lines, synthetic voices)

Strict = "fully right" (`score.py`), clean-up on. 0.1.13 = the released
app; new = this branch (`hw3-*` runs).

| Set                                   | Path                               | 0.1.13  | New     |
| ------------------------------------- | ---------------------------------- | ------- | ------- |
| English, 106 clips                    | Parakeet                           | 84/106  | 84/106  |
| Old 20 Hinglish clips                 | Auto (the app's default)           | 7/20    | 8/20    |
| New 120 clips (40 lines x 3 voices)   | Auto                               | 40/120  | 44/120  |
| New 120                               | Qwen3-ASR 1.7B + Roman             | 78/120  | 78/120  |
| New 120                               | Qwen3-ASR 1.7B + context + Roman   | 78/120  | 78/120  |
| Old 20                                | Qwen3-ASR 1.7B + context + Roman   | 11/20   | 13/20   |

- English: 4 of 126 outputs differ, all model wording ("₹1,500" vs
  "₹1500", a comma). The new reply check rejected no English reply.
- Auto by voice (new 120): Lekha 15 -> 16/40, Parler 13 -> 14/40, cloned
  voice 12 -> 14/40. The gain is mostly "?" on Hinglish questions. One clip
  got worse: the model split "Karoonga" into "karoon ga".
- Clean-up time on the auto path: median 424 -> 454 ms. On the Qwen paths
  it rose by 70-300 ms (more short lines go to the model) for no gain.

### What it means

- Speech is still the main problem: on the auto path the transcript alone
  passes 39 of 120, and the clean-up adds 5. Qwen3-ASR 1.7B passes 75 of
  120 before clean-up.
- Next (day 3): test Qwen3-ASR 1.7B as the auto path's Hindi engine on
  real app timing (load time, memory, the extra Roman step), since it is
  about 1.8x the auto path on this set. Next to it, a cheap test from an X
  reply (@idky1515): add ~100 common Roman Hindi words to the hints Apple's
  engine already gets (today only the user's dictionary words).

## Day 3 (2 Nov 2026): Apple hint words, and a wrong engine choice

### Test 1: common Hindi words as hints to Apple (idea from an X reply)

The auto path already sends the user's dictionary words to Apple's en-IN
engine as hint words. `apple_hints.py` ran Apple alone on the 120 new clips
three ways. The Hindi list is `textflow/hindi.rs` WORDS minus the four that
are also English (77 words, written before this test set; list hash
cba66e4b25959749, frozen before the runs).

| Apple en-IN, clean-up off | Fully right | hWER | Median time |
| ------------------------- | ----------- | ---- | ----------- |
| No hint words             | 46/120      | 32.2 | 141 ms      |
| Dictionary (16 words)     | 47/120      | 31.8 | 258 ms      |
| Dictionary + 77 Hindi     | 47/120      | 31.8 | 334 ms      |

Apple does use hints: the dictionary fixed names ("Tabish" -> "Tavish",
"Reva" -> "Rivaan"). The Hindi words changed 1 transcript of 120 (for the
worse: "Rivaan" -> "Rewind") and added about 75 ms. Not worth adding.

A reply on X pointed out that synthetic voices are clean, and real speech
sits nearer the point where hints tip the result. Re-run on 46 of Gourav's
own recordings that contain Hindi (his dictations and his side of
meetings; private, never published): the 77 Hindi hint words changed 0 of
46 transcripts (ignoring capitals and punctuation) and added about 75 ms
(median 288 -> 363 ms). These clips have no checked reference text, so no
pass counts, but with no transcript changed the hints can't help.

### Test 2: the auto choice kept cut-down English

Apple alone passes 47 of 120 before clean-up, the auto path only 39. On 18
clips auto kept Parakeet's text; on 9 of them Apple was right and Parakeet
wrong (1 the other way). Parakeet had kept only the English words, which
pass the "reads as English" check: "Yard staging server." for 4.5 s of
"Yaar, staging server phir se down hai, koi dekh sakta hai kya".

Fix (`apple_speech/pick.rs`): for recordings of 2.5 s or more, when
Parakeet's English text has under 1 spoken word per second (each digit
counts as a word), the app waits for Apple and uses Apple's text if it has
at least twice the words and 3 more. Measured before choosing the numbers:
synthetic English clips 1.4 words per second or more; Gourav's 34 real
English dictations (private, as the app saved them) 1.2 or more; the
cut-down Hindi lines 0.4 to 1.0, with Apple hearing 7 to 15 words against
Parakeet's 2 to 5.

A review of the first version (rate alone) found that live dictation keeps
about 2 s of audio around each spoken phrase, so English said with pauses
("Okay. So. Fix the login bug. Thanks.") can fall under 1 word per second
and would have switched to Apple. Hence the second condition: there Apple
hears about as many words, so Parakeet's text stays.

| Auto path, clean-up on              | Day 2 build | With the fix |
| ----------------------------------- | ----------- | ------------ |
| New 120 clips                       | 44/120      | **50/120**   |
| New 120, clean-up off               | 39/120      | 45/120       |
| Old 20 clips                        | 8/20        | 8/20         |
| English 106 clips (on auto)         | 84/106      | 84/106       |

On the English run no English clip switched because of the new check (3
email clips use Apple's text, as they already did through the "reads as
English" rule).

(The rate-only first version scored 51; the final one 50, with the same
transcripts: one clean-up reply differed.)

Running total on the new 120, auto path: 0.1.13 40 -> now 50. Qwen3-ASR
1.7B + Roman step: 78.

### Test 3: Qwen3-ASR 1.7B as the auto path's Hindi engine

`engine_sim.py` replays the app's choice (`apple_speech/pick.rs`) on each
clip from Parakeet's own transcript (`hw6-pk`), then takes the result from
the chosen engine's run. Replaying with Apple gives back the measured 50.

| Auto path, new 120 clips              | Clean-up off | Clean-up on |
| ------------------------------------- | ------------ | ----------- |
| Parakeet + Apple (0.1.14)             | 45/120       | 50/120      |
| Parakeet + Qwen3-ASR 1.7B + Roman     | 73/120       | **76/120**  |
| Parakeet + Qwen + hint text + Roman   | 66/120       | 74/120      |

English: the app sends 1 to 3 of the 106 English clips away from Parakeet
(email addresses); Qwen and Apple garble those about equally.

Cost, measured on this Mac (M-series, 24 GB, the 8B clean-up model loaded):

| | Apple | Qwen3-ASR 1.7B |
| --- | --- | --- |
| Download | none (part of macOS 26) | 2.2 GB |
| Load | kept loaded by the helper | 1.4 to 1.7 s, then kept |
| Memory | Apple's process | about 2.6 GB (2.2 GB weights) |
| Speech, synthetic clips (4.6 s) | 242 ms | 406 ms |
| Speech, Gourav's real clips (9 s median) | 288 ms | 865 ms (p90 1.9 s) |
| Roman-letter step | none (Apple writes Roman) | 513 ms synthetic; 2.1 s real (p90 4.6 s) |

On real 9 s dictations Qwen adds about 2.6 s before the text appears (most
of it the clean-up model writing the Hindi in Roman letters), against
about 0.3 s for Apple. Works on macOS older than 26, where there is no
Apple engine.

Real voice: Gourav's 46 Hindi recordings have no checked text, so
`training/benchmark/private/apple_vs_qwen/index.html` (private, not
published) shows Apple's and Qwen's text in random order for a blind pick.

Faster Roman step? The letter rules (instant) instead of the model's
rewrite: Qwen's transcripts pass 53/120 instead of 75/120 (clean-up off),
so the model step earns its time. A way to cut it: one model call that
writes Roman letters and cleans up together, instead of two in a row.
