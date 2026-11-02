"""Audio for the 40 made-up Hinglish lines in hinglish40.json (Hinglish week, day 2).
Every voice reads the Devanagari line (`spoken_dev`), so Hindi words get Hindi sounds.
Clips go to training/benchmark/private/hinglish40/ (git-ignored): 16 kHz mono, 0.3 s of
silence before and 0.6 s after, as the day-1 clips.

    python3 hinglish40_audio.py lekha                    # macOS Lekha (hi_IN), one take
    PARLER_DIR=... <parler venv python> hinglish40_audio.py parler    # Indic Parler-TTS, Rohit / Divya, 2 takes
    CLONE_REF=... <chatterbox venv python> hinglish40_audio.py cb A B # Chatterbox Multilingual (hi), lines A..B-1,
                                                                      # a cloned voice, 2 takes
    python3 hinglish40_audio.py check                    # a third speech model hears every take
                                                         # (whisper-hindi2hinglish-apex, Roman letters,
                                                         # not one of the tested paths); best take per
                                                         # line and voice -> final clip
    python3 hinglish40_audio.py clips                    # clips.json for hinglish_run.py

Parler and Chatterbox run one process at a time (Chatterbox grows by about 1 GB a take,
so it is run in slices of 3 lines). Takes are kept in takes/ next to the clips.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parent / "benchmark/private/hinglish40"
TAKES = OUT / "takes"
LINES = json.loads((HERE / "hinglish40.json").read_text())
# A local Indic Parler-TTS folder (model/ + hf-cache/) and a Hindi voice sample to clone.
PARLER = Path(os.environ.get("PARLER_DIR", "parler"))
CB_REF = Path(os.environ.get("CLONE_REF", "ref_hi.wav"))
PARLER_SPEAKERS = ["Rohit", "Divya"]
SEEDS = (11, 12)
JUDGE = HERE.parent / "benchmark/private/model_choice/models/whisper-hindi2hinglish-apex-F16.gguf"
TCPP = HERE.parent / "benchmark/private/model_choice/tools/target/release/tcpp"
# The judge's hWER above this: the clip is reported as hard to hear.
MAX_JUDGE_HWER = 0.5


def finish(src, dst):
    """16 kHz mono, quiet ends trimmed, 0.3 s before, 0.6 s after, peak at -1 dB."""
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", src, "-af",
                    "silenceremove=start_periods=1:start_threshold=-45dB,areverse,"
                    "silenceremove=start_periods=1:start_threshold=-45dB,areverse,"
                    "adelay=300,apad=pad_dur=0.6,loudnorm=I=-20:TP=-1",
                    "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", dst], check=True)


def lekha():
    TAKES.mkdir(parents=True, exist_ok=True)
    for ln in LINES:
        aiff = TAKES / f"{ln['id']}_Lekha__s0.aiff"
        subprocess.run(["say", "-v", "Lekha", "-o", aiff, ln["spoken_dev"]], check=True)
        finish(aiff, TAKES / f"{ln['id']}_Lekha__s0.wav")
        aiff.unlink()
        print(ln["id"], flush=True)


def parler():
    os.environ.setdefault("HF_HOME", str(PARLER / "hf-cache"))
    import soundfile as sf
    import torch
    from parler_tts import ParlerTTSForConditionalGeneration
    from transformers import AutoTokenizer
    TAKES.mkdir(parents=True, exist_ok=True)
    model = ParlerTTSForConditionalGeneration.from_pretrained(PARLER / "model").to("cpu")
    tok = AutoTokenizer.from_pretrained(PARLER / "model")
    desc_tok = AutoTokenizer.from_pretrained(model.config.text_encoder._name_or_path)
    for i, ln in enumerate(LINES):
        who = PARLER_SPEAKERS[i % 2]
        desc = desc_tok(f"{who} speaks at a moderate pace in a natural, conversational tone. "
                        "The recording is very clear and close-sounding, with no background noise.",
                        return_tensors="pt")
        for seed in SEEDS:
            wav = TAKES / f"{ln['id']}_{who}__s{seed}.wav"
            if wav.exists():
                continue
            torch.manual_seed(seed)
            p = tok(ln["spoken_dev"], return_tensors="pt")
            y = model.generate(input_ids=desc.input_ids, attention_mask=desc.attention_mask,
                               prompt_input_ids=p.input_ids, prompt_attention_mask=p.attention_mask)
            raw = wav.with_suffix(".raw.wav")
            sf.write(raw, y.cpu().numpy().squeeze(), model.config.sampling_rate)
            finish(raw, wav)
            raw.unlink()
            print(wav.name, flush=True)


def cb(a, b):
    os.environ.setdefault("HF_HOME", str(Path.home() / "Documents/repos/buildthis-saas/.hf-cache"))
    import soundfile as sf
    import torch
    from chatterbox.mtl_tts import ChatterboxMultilingualTTS
    TAKES.mkdir(parents=True, exist_ok=True)
    tts = ChatterboxMultilingualTTS.from_pretrained(device="mps")
    for ln in LINES[int(a):int(b)]:
        for seed in SEEDS:
            wav = TAKES / f"{ln['id']}_GouravClone__s{seed}.wav"
            if wav.exists():
                continue
            torch.manual_seed(seed)
            y = tts.generate(ln["spoken_dev"], language_id="hi", audio_prompt_path=str(CB_REF))
            raw = wav.with_suffix(".raw.wav")
            sf.write(raw, y.squeeze(0).cpu().numpy(), tts.sr)
            finish(raw, wav)
            raw.unlink()
            print(wav.name, flush=True)


def check():
    from hinglish_score import hwer
    lines = {ln["id"]: ln for ln in LINES}
    listing, heard_p = OUT / "judge.todo.tsv", OUT / "judge.jsonl"
    takes = sorted(TAKES.glob("*.wav"))
    listing.write_text("".join(f"{w.name}\t{w}\n" for w in takes))
    heard_p.unlink(missing_ok=True)
    subprocess.run(["perl", "-e", "alarm shift; exec @ARGV", "1800", TCPP, JUDGE, "en", "-", listing, heard_p],
                   check=True)
    listing.unlink()
    heard = {r["id"]: r["text"] for r in map(json.loads, heard_p.read_text().splitlines())}
    best = {}
    for w in takes:
        cid, voice = w.name.split("__")[0].split("_", 1)
        e, n = hwer(lines[cid]["spoken"], heard[w.name])
        score = e / n
        if (cid, voice) not in best or score < best[(cid, voice)][0]:
            best[(cid, voice)] = (score, w.name)
    chosen = {}
    for (cid, voice), (score, name) in sorted(best.items()):
        (OUT / f"{cid}_{voice}.wav").write_bytes((TAKES / name).read_bytes())
        chosen[f"{cid}_{voice}.wav"] = {"take": name, "judge_heard": heard[name], "judge_hwer": round(score, 3)}
    (OUT / "chosen.json").write_text(json.dumps(chosen, ensure_ascii=False, indent=1))
    by_voice = {}
    for clip, r in chosen.items():
        v = clip.split("_", 1)[1][:-4]
        by_voice.setdefault("Parler" if v in PARLER_SPEAKERS else v, []).append(r["judge_hwer"])
    for v, hs in sorted(by_voice.items()):
        print(f"{v:12} {len(hs)} clips, judge hWER mean {100 * sum(hs) / len(hs):.1f}%, "
              f"over {100 * MAX_JUDGE_HWER:.0f}%: {sum(h > MAX_JUDGE_HWER for h in hs)}")


def clips():
    chosen = json.loads((OUT / "chosen.json").read_text())
    out = [{"clip": clip, "id": clip.split("_")[0], "voice": clip.split("_", 1)[1][:-4],
            "judge_hwer": r["judge_hwer"]} for clip, r in sorted(chosen.items())]
    (OUT / "clips.json").write_text(json.dumps(out, indent=1) + "\n")
    print(len(out), "clips")


if __name__ == "__main__":
    step, *args = sys.argv[1:]
    {"lekha": lekha, "parler": parler, "cb": cb, "check": check, "clips": clips}[step](*args)
