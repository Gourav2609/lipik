# Whisper dictation test (3 Nov 2026)

Question: can people use Lipik where they can't talk out loud (open office, café,
train) by whispering? Everything here is synthetic: the test clips are macOS
voices reading made-up lines, and the whispers are made from them.

## How the whispers were made

`whisper_audio.py` makes two copies of each test clip:

- whispered (`_w`): the voice's spectral shape is kept every 25 ms but the
  vocal-cord buzz is replaced with noise, which is how a whisper sounds; same
  loudness as the clip (a whisper close to the mic);
- quiet room (`_wq`): that whisper at -45 dBFS (about 20 dB quieter than
  normal speech) over steady room noise 10 dB below it (a whisper reaching a
  laptop mic in an office).

Plus the macOS "Whisper" voice reading the 53 English lines, and
`whisper_boost.py`: the quiet-room copies raised so their loudest parts sit at
-26 dBFS (`_wqb`), as a gain step in Lipik would.

Speech model and clean-up: Lipik 0.1.14 code, auto engine, through
`cli_run.py` / `hinglish_run.py` (no speech detector on this path). Speech
detector: the app's Silero model at its 0.3 threshold
(`vad::silero::tests::speech_share_of_listed_clips`).

## Results

Fully right, clean-up on (`whisper_score.py`):

| Clips | Normal voice | Whispered | Quiet room | Quiet room, boosted |
| --- | --- | --- | --- | --- |
| English, 106 | 84 | 81 | 69 | 70 |
| Hinglish, 120 | 50 | 10 | 0 | 1 |
| English, macOS Whisper voice (53) | - | 36 | - | - |

Share of each clip the speech detector marks as speech (median), and clips
where it finds under 10%:

| Clips | Normal | Whispered | Quiet room | Quiet room, boosted |
| --- | --- | --- | --- | --- |
| English | 0.79, 0 | 0.79, 0 | 0.48, 21 of 106 | 0.79, 0 |
| Hinglish | 0.71, 0 | 0.68, 0 | 0.01, 78 of 140 | 0.66, 11 of 140 |

## What it means

1. English whispered close to the mic works almost as well as speaking (81
   vs 84).
2. A quiet whisper far from the mic is mostly thrown away by the speech
   detector before the speech model hears it. Raising quiet recordings to a
   normal level first fixes the detector (0.48 -> 0.79), but the room noise
   still costs accuracy (70 vs 84).
3. Whispered Hinglish doesn't work: Apple's en-IN engine, which hears the
   Hindi, garbles whispers (10/120). Qwen3-ASR on whispers is not tested yet.
4. Real whispers may differ from these. Next: about 20 lines whispered by a
   real person, on the laptop mic and on AirPods.
