# Clean-up benchmark on Lipik 0.1.12 (the released app)

Date: 2026-10-31. Same 63 cases and 126 clips as `RESULTS.md` (`cases.json`,
`audio/clips.json`: English read by Rishi and Aman, Hinglish by Lekha and
Aman, made with `say -o`, never played), same scoring (`score.py`). The
last run before this one is `v013-builtin8b` (28 Oct, "re-run on 0.1.3").

## What was run

- **Binary**: the `lipik` from `/Applications/Lipik.app` (version 0.1.12,
  bundle `com.gourav.lipik`), copied byte for byte into a portable folder
  with a `portable` marker, so the installed app and its data folder were
  never touched. Settings = this folder's `app/Data/settings_store.json`
  (the 16 words in `vocab.json` as the dictionary, English).
- **Every clip goes through the real app code**: `lipik --transcribe-file
<clip> --model <Parakeet> --clean=<bundle id> --json`. So speech to text,
  filler removal, dictionary fixes, the rules step, the gate (which
  sentences go to the model), the prompt and the output checks are all
  0.1.12's own. `raw` = the transcript (what Lipik pastes with clean-up
  off), `best` = what it pastes with clean-up on.
- **Clean-up model**: 0.1.12's own `llama-server` with the same Qwen3 8B
  Q4_K_M file the app downloads and engine.rs's arguments. One difference
  from the app: a headless run is a new process per clip, so its built-in
  engine is never loaded in time and clean-up falls back to the rules
  (checked on one clip: "Clean-up used the rules only"). So the portable
  copy's engine is set to "Ollama" and points at `cli_shim.py`, which
  answers like Ollama and forwards each request to that `llama-server`
  with the body the built-in path sends (`engine::chat_body`). One warm-up
  request was sent first, as the app does at the start of a recording.
- The app sees the bundle id but not the window name, so the model is told
  "Work chat" / "Email" / "Messages" instead of "Slack" / "Mail".
- Scripts: `cli_run.py` (writes `runs/cleanup-v0112-cli.jsonl`, git-ignored)
  and `cli_shim.py`. Score: `python3 score.py v0112-cli`.

## The six numbers

| What                                    | 28 Oct (`v013-builtin8b`)    | 0.1.12 (`v0112-cli`)         |
| --------------------------------------- | ---------------------------- | ---------------------------- |
| English fully right, clean-up off -> on | 36/106 (34%) -> 86/106 (81%) | 30/106 (28%) -> 81/106 (76%) |
| "four, no, five" corrections            | 15/16                        | 15/16                        |
| Spoken "comma" / "new line"             | 9/10                         | 9/10                         |
| Lists                                   | 8/8                          | 8/8                          |
| Emails and links                        | 4/10                         | 4/10                         |
| Hinglish                                | 0/20                         | 0/20                         |

All 126 clips: 30/126 (24%) -> 81/126 (64%) (was 29% -> 68%).

Full table (`best` = clean-up on):

| Category           | Clips | Off          | On           | WER off | WER on |
| ------------------ | ----- | ------------ | ------------ | ------- | ------ |
| filler             | 12    | 4/12 (33%)   | 6/12 (50%)   | 11.1    | 7.6    |
| correction         | 16    | 0/16 (0%)    | 15/16 (94%)  | 50.9    | 4.4    |
| spoken punctuation | 10    | 0/10 (0%)    | 9/10 (90%)   | 37.5    | 2.8    |
| numbers            | 14    | 10/14 (71%)  | 12/14 (86%)  | 8.8     | 6.6    |
| email / URL        | 10    | 0/10 (0%)    | 4/10 (40%)   | 17.0    | 14.2   |
| list               | 8     | 0/8 (0%)     | 8/8 (100%)   | 22.0    | 2.5    |
| vocab names        | 10    | 3/10 (30%)   | 6/10 (60%)   | 9.0     | 5.0    |
| tech terms         | 10    | 5/10 (50%)   | 8/10 (80%)   | 6.8     | 4.2    |
| question           | 8     | 7/8 (88%)    | 7/8 (88%)    | 2.4     | 2.4    |
| run-on             | 8     | 1/8 (12%)    | 6/8 (75%)    | 1.7     | 2.0    |
| Hinglish           | 20    | 0/20 (0%)    | 0/20 (0%)    | 61.2    | 60.3   |
| **English**        | 106   | 30/106 (28%) | 81/106 (76%) | 14.1    | 4.9    |
| **All**            | 126   | 30/126 (24%) | 81/126 (64%) | 20.8    | 12.8   |

The model was called on 67 clips; wait on clips of 8+ words: median 500 ms,
p90 1,050 ms; none over the 4 s limit.

## Why English went from 86 to 81

The 28 Oct run reused the transcripts made on 26 Oct and only re-ran the
clean-up. This run transcribes every clip again with 0.1.12, so it also
measures what changed in speech to text and in the filler and dictionary
steps since then. 15 of 126 transcripts differ. 5 English clips went from
pass to fail, none from fail to pass:

- **"UM" in capitals is left in** (2 clips: fil01 and que04, Aman voice).
  Parakeet now writes the filler as "UM"; since commit 3ff5d53e the filler
  step keeps capitalised "UM"/"ERM" as acronyms, and the clean-up model
  keeps it too. Pasted: "UM so I was thinking we could maybe start the
  review a bit earlier tomorrow." 7 transcripts (all Aman voice) have a
  capital "UM"; clean-up removed it in the other 5.
- **A dictionary name is no longer fixed** (2 clips, voc05): "Oshin
  Dastur" is heard as "Ocean Dustua" / "Ocean Dastur" and stays that way
  ("Ocean" is a real word). On 26 Oct the transcriber's word correction
  turned it into "Oshin Dastur". The same change also stopped one wrong
  fix: cor06 used to read "ping Ishita Venkatraman contract" and now reads
  "ping Ishita about the contract".
- **Model wobble** (1 clip, run01 Aman): the reply dropped an "and"
  ("..., and the rest" became ". The rest"). The model samples at
  temperature 0.2, so a clip or two moves between runs.

The headline categories (corrections, spoken punctuation, lists, emails,
Hinglish) are unchanged.
