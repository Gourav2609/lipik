"""Promised dictations, end to end: speech -> Parakeet -> Lipik's text steps.

Runs a portable copy of the dev binary in app/ (`--transcribe-file --clean`),
so the installed app and its settings are never touched. Spoken lines are
rendered with `say -o` (never played); real recordings live in audio/private/
and are not committed.

    python3 run.py                 # every case, every vocabulary setup
    python3 run.py --setup none    # only the empty-dictionary run
"""

import argparse
import json
import subprocess
from pathlib import Path

HERE = Path(__file__).parent
APP = HERE / "app"
AUDIO = HERE / "audio"
RUNS = HERE / "runs"
PARAKEET = "handy-computer/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf"
VOICES = ["Rishi", "Aman", "Samantha", "Daniel"]
DEMO_WORDS = ["Rivaan", "Quavia", "Northwind"]
SETUPS = {
    "none": [],
    "demo": DEMO_WORDS,
    "demo+me": DEMO_WORDS + ["Gourav", "Keychain", "gourav@keychain.com"],
}
SNIPPETS = [{"id": "s1", "trigger": "my address", "expansion": "14 Orchard Lane, Flat 3B, Pune 411001"}]


def make_audio(case, voice):
    wav = AUDIO / f"{case['id']}_{voice}.wav"
    if not wav.exists():
        aiff = wav.with_suffix(".aiff")
        subprocess.run(["say", "-v", voice, "-o", str(aiff), case["say"]], check=True)
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(aiff), "-af", "adelay=300,apad=pad_dur=0.6",
                        "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(wav)], check=True)
        aiff.unlink()
    return wav


def configure(words):
    data = APP / "Data"
    store = json.loads((data / "settings_store.json").read_text())
    store["settings"].update({"custom_words": words, "selected_language": "en", "selected_model": PARAKEET})
    (data / "settings_store.json").write_text(json.dumps(store, indent=2))
    (data / "textflow.json").write_text(json.dumps({
        "dictionary": {"words": [{"id": f"w{i}", "word": w, "starred": False, "auto_learned": False, "shared": False}
                                 for i, w in enumerate(words)], "replacements": []},
        "snippets": SNIPPETS,
        "style": {"personal": "casual", "work": "formal", "email": "formal", "other": "formal", "auto_cleanup": True},
        "transforms": {}, "pending_fixes": [], "cleanup_default_applied": True,
    }, indent=2))


def loose(s):
    s = s.strip().rstrip(".?!")
    return s[:1].lower() + s[1:]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--setup", action="append")
    ap.add_argument("--only", action="append", help="case id")
    args = ap.parse_args()
    AUDIO.mkdir(exist_ok=True)
    RUNS.mkdir(exist_ok=True)
    cases = [c for c in json.loads((HERE / "cases.json").read_text()) if not args.only or c["id"] in args.only]
    for setup in args.setup or SETUPS:
        configure(SETUPS[setup])
        out = RUNS / f"{setup}.jsonl"
        out.unlink(missing_ok=True)
        exact = ok = total = 0
        for c in cases:
            clips = [(AUDIO / c["wav"], "real")] if "wav" in c else [(make_audio(c, v), v) for v in VOICES]
            for wav, voice in clips:
                proc = subprocess.run(["perl", "-e", "alarm shift; exec @ARGV", "240", str(APP / "lipik"),
                                       "--transcribe-file", str(wav), "--model", PARAKEET,
                                       f"--clean={c['app']}", "--json"], capture_output=True, text=True)
                lines = [l for l in proc.stdout.splitlines() if l.startswith("{")]
                r = json.loads(lines[-1]) if lines else {"text": "", "final": "ERROR " + proc.stderr[-200:]}
                final = r.get("final") or ""
                row = {"setup": setup, "id": c["id"], "voice": voice, "heard": r["text"],
                       "prepared": r.get("prepared"), "final": final, "expect": c["expect"],
                       "exact": final == c["expect"], "ok": loose(final) == loose(c["expect"])}
                total += 1
                exact += row["exact"]
                ok += row["ok"]
                with out.open("a") as f:
                    f.write(json.dumps(row) + "\n")
                mark = "PASS" if row["exact"] else ("near" if row["ok"] else "FAIL")
                print(f"{setup:8} {mark:4} {c['id']:18} {voice:8} heard={r['text']!r} -> {final!r}", flush=True)
        print(f"== {setup}: exact {exact}/{total}, ignoring first capital + end mark {ok}/{total}\n", flush=True)


if __name__ == "__main__":
    main()
