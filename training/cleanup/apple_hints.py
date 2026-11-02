"""Hinglish week, day 3: does Apple's en-IN engine hear Hindi better when the common
Roman Hindi words (textflow/hindi.rs WORDS, minus the ones that are also English) are
added to the hint words it gets? Today the app sends only the user's dictionary.

    python3 apple_hints.py <apple-speech binary> <tag> [--hindi | --none]

Writes runs/asr-<tag>.jsonl (the rows hinglish_run.py cleanup and hinglish_score.py
read) for the clips in HCLIPS (default audio/clips.json), Apple only, en-IN, with the
16 words of vocab.json as the dictionary, plus the Hindi words with --hindi, or no
words at all with --none (does Apple use the words?).
"""

import json
import os
import re
import struct
import subprocess
import sys
import time
import wave
from pathlib import Path

HERE = Path(__file__).resolve().parent
HINDI_RS = (HERE / "../../src-tauri/src/textflow/hindi.rs").read_text()


def rs_list(name):
    return re.findall(r'"([^"]+)"', re.search(rf"pub const {name}: &\[&str\] = &\[(.*?)\];", HINDI_RS, re.S)[1])


HINDI = [w for w in rs_list("WORDS") if w not in rs_list("ALSO_ENGLISH")]
VOCAB = json.loads((HERE / "vocab.json").read_text())
CLIPS = HERE / "audio" / os.environ.get("HCLIPS", "clips.json")
cases = {c["id"]: c for c in json.loads((HERE / "cases.json").read_text())}
cases |= {c["id"]: dict(c, category="hinglish") for c in json.loads((HERE / "hinglish40.json").read_text())}
clips = [c for c in json.loads(CLIPS.read_text()) if cases[c["id"]]["category"] == "hinglish"]


def samples(path):
    with wave.open(str(path)) as w:
        assert w.getframerate() == 16000 and w.getnchannels() == 1 and w.getsampwidth() == 2
        raw = w.readframes(w.getnframes())
    return [s / 32768 for s in struct.unpack(f"<{len(raw) // 2}h", raw)]


def main(binary, tag, hindi, none):
    words = [] if none else VOCAB + (HINDI if hindi else [])
    p = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    assert json.loads(p.stdout.readline()).get("supported"), "Apple speech not supported here"

    def ask(req, pcm=None):
        p.stdin.write((json.dumps(req) + "\n").encode())
        if pcm is not None:
            p.stdin.write(struct.pack(f"<{len(pcm)}f", *pcm))
        p.stdin.flush()
        return json.loads(p.stdout.readline())

    print(ask({"id": 0, "op": "prepare", "locale": "en-IN", "download": False}))
    ask({"id": 0, "op": "transcribe", "locale": "en-IN", "samples": 16000, "words": words}, [0.0] * 16000)
    out = HERE / "runs" / f"asr-{tag}.jsonl"
    out.unlink(missing_ok=True)
    for i, c in enumerate(clips, 1):
        pcm = samples(CLIPS.parent / c["clip"])
        t = time.perf_counter()
        r = ask({"id": i, "op": "transcribe", "locale": "en-IN", "samples": len(pcm), "words": words}, pcm)
        ms = round((time.perf_counter() - t) * 1000)
        text = r.get("text", "")
        row = {"clip": c["clip"], "id": c["id"], "voice": c["voice"], "text": text, "asr_text": text,
               "asr_ms": ms, "audio_s": len(pcm) / 16000, "error": None if r.get("ok") else r.get("error"),
               "context": "none" if none else "dictionary + Hindi words" if hindi else "dictionary"}
        with out.open("a") as f:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
        print(f"{c['clip']:<24} {ms:>5} ms | {text}", flush=True)
    p.stdin.close()
    p.wait(timeout=10)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], "--hindi" in sys.argv, "--none" in sys.argv)
