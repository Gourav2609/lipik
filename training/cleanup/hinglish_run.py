"""Hinglish week, day 1: the 20 Hinglish clips (hng01-hng10, Lekha + Aman voices)
through each speech path, written as runs/cleanup-<tag>.jsonl rows that score.py and
hinglish_score.py read. See HINGLISH-WEEK.md.

    python3 hinglish_run.py cli <portable dir> <shim log> <tag>
        a portable copy of the released lipik: --transcribe-file --clean=<bundle> --json
        --repeat 3 (speech engine = whatever the copy's settings say: parakeet or auto)
    python3 hinglish_run.py tcpp <model.gguf> <language> <context|-> <tag>
        Qwen3-ASR through transcribe-cpp 0.4.0 (the app's engine library), one load
    python3 hinglish_run.py roman <tag> <llama url> <key>
        the notetaker's Roman-letter step on every Devanagari transcript in asr-<tag>
    python3 hinglish_run.py devaudio
        diagnostic: Lekha reading the lines in Devanagari -> audio/clips-hdev.json;
        run cli/tcpp on it with HCLIPS=clips-hdev.json
    python3 hinglish_run.py cleanup <asr tag> <llama url> <key> <out tag>
        run.py's mirror of the app's clean-up (--prepass --parts --num-predict
        --vocab-in-prompt) on asr-<asr tag>

HCLIPS picks the clip list: a name in audio/ (default clips.json) or a path; the clips
sit next to it. Day 2's 40 new lines (hinglish40.json):
    HCLIPS=$PWD/../benchmark/private/hinglish40/clips.json
"""

import json
import os
import re
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).parent
REPO = HERE.parent.parent
RUNS = HERE / "runs"
sys.path.insert(0, str(HERE.parent / "bench"))
PARAKEET = "lipik/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf"
BUNDLE = {"Slack": "com.tinyspeck.slackmacgap", "Mail": "com.apple.mail", "Messages": "com.apple.MobileSMS",
          "Notes": "com.apple.Notes", "Claude Code": "com.mitchellh.ghostty"}
TCPP = REPO / "training/benchmark/private/model_choice/tools/target/release/tcpp"
DEV = re.compile(r"[ऀ-ॿ]")

cases = {c["id"]: c for c in json.loads((HERE / "cases.json").read_text())}
cases |= {c["id"]: dict(c, category="hinglish") for c in json.loads((HERE / "hinglish40.json").read_text())}
CLIPS = HERE / "audio" / os.environ.get("HCLIPS", "clips.json")
AUDIO = CLIPS.parent
clips = [c for c in json.loads(CLIPS.read_text()) if cases[c["id"]]["category"] == "hinglish"]


def alarm(secs, *cmd):
    return ["perl", "-e", "alarm shift; exec @ARGV", str(secs), *map(str, cmd)]


def devaudio():
    """Lekha reading the Devanagari lines (hinglish_devanagari.json) -> audio/clips-hdev.json."""
    lines = json.loads((HERE / "hinglish_devanagari.json").read_text())
    out = []
    for cid, text in lines.items():
        if cid.startswith("_"):
            continue
        wav = HERE / "audio" / f"{cid}_LekhaDev.wav"
        aiff = wav.with_suffix(".aiff")
        subprocess.run(["say", "-v", "Lekha", "-o", aiff, text], check=True)
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", aiff, "-af", "adelay=300,apad=pad_dur=0.6",
                        "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", wav], check=True)
        aiff.unlink()
        out.append({"clip": wav.name, "id": cid, "voice": "LekhaDev"})
    (HERE / "audio" / "clips-hdev.json").write_text(json.dumps(out, indent=1) + "\n")


def cli(portable, shimlog, tag):
    portable, shimlog = Path(portable), Path(shimlog)
    out = RUNS / f"cleanup-{tag}.jsonl"
    out.unlink(missing_ok=True)
    shimlog.touch()
    for c in clips:
        before = len(shimlog.read_text().splitlines())
        t = time.perf_counter()
        proc = subprocess.run(alarm(180, portable / "lipik", "--transcribe-file", AUDIO / c["clip"],
                                    "--model", PARAKEET, f"--clean={BUNDLE[cases[c['id']]['app']]}", "--json",
                                    "--repeat", "3"), capture_output=True, text=True)
        wall = (time.perf_counter() - t) * 1000
        lines = [l for l in proc.stdout.splitlines() if l.startswith("{")]
        if not lines:
            sys.exit(f"{c['clip']}: FAILED rc={proc.returncode} {proc.stderr.strip()[-400:]}")
        r = json.loads(lines[-1])
        calls = [json.loads(l) for l in shimlog.read_text().splitlines()[before:]]
        path = "cleaned" if calls else "rules"
        row = {"clip": c["clip"], "id": c["id"], "voice": c["voice"], "raw": r["text"], "prepared": r.get("prepared"),
               "model": f"lipik CLI {portable.name}", "final": r["final"], "final_why": path, "calls": calls,
               "model_out": None, "latency_ms": sum(x["ms"] for x in calls), "asr_ms": r["best_ms"],
               "asr_runs_ms": r["transcribe_ms"], "audio_s": r["audio_secs"], "wall_ms": round(wall),
               "app_final": r["final"], "app_path": path}
        with out.open("a") as f:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
        print(f"{c['clip']:<16} asr {r['best_ms']} ms, {len(calls)} calls | {r['text']!r} -> {r['final']!r}", flush=True)


def tcpp(model, language, context, tag):
    listing = RUNS / f"{tag}.todo.tsv"
    raw = RUNS / f"{tag}.tcpp.jsonl"
    raw.unlink(missing_ok=True)
    listing.write_text("".join(f"{c['clip']}\t{AUDIO / c['clip']}\n" for c in clips))
    proc = subprocess.run(alarm(900, TCPP, model, language, context, listing, raw), capture_output=True, text=True)
    listing.unlink()
    if proc.returncode:
        sys.exit(proc.stderr[-1000:])
    print(proc.stderr.splitlines()[0])
    rows = {r["id"]: r for r in map(json.loads, raw.read_text().splitlines())}
    with (RUNS / f"asr-{tag}.jsonl").open("w") as f:
        for c in clips:
            r = rows[c["clip"]]
            f.write(json.dumps({"clip": c["clip"], "id": c["id"], "voice": c["voice"], "text": r["text"],
                                "asr_text": r["text"], "asr_ms": round(1000 * r["decode_s"]), "audio_s": r["audio_s"],
                                "error": r["error"], "context": None if context == "-" else context},
                               ensure_ascii=False) + "\n")
            print(f"{c['clip']:<16} {1000 * r['decode_s']:.0f} ms | {r['text']}")


# ---- the notetaker's Roman-letter step (src-tauri/src/notetaker/hinglish.rs) ----

def _rs_strings(src, name):
    body = re.search(rf"const {name}: [^=]+= &\[(.*?)\];", src, re.S)[1]
    return re.findall(r'"([^"]*)"', body)


HINGLISH_RS = (REPO / "src-tauri/src/notetaker/hinglish.rs").read_text()
ROMAN_PROMPT = re.search(r'fn prompt\(names: &\[String\]\) -> String \{\s*let mut prompt = String::from\(\s*"(.*?)",\s*\);',
                         HINGLISH_RS, re.S)[1].replace("\\\n", "")
HINDI_GLUE = _rs_strings(HINGLISH_RS, "HINDI_GLUE")
HINDI_WORDS = set(_rs_strings((REPO / "src-tauri/src/textflow/hindi.rs").read_text(), "WORDS"))
HINDI_WORDS |= set(_rs_strings((REPO / "src-tauri/src/notetaker/diarize.rs").read_text(), "HINGLISH_WORDS"))
HINDI_WORDS |= {"vo", "wo", "ye", "hum", "ham", "na", "bhi"}
RETRY = (" Your last answer translated the Hindi into English: keep every Hindi word in Hindi, "
         "only in Roman letters.")


def words_of(text):
    return [w for w in re.split(r"[^\wऀ-ॿ]+", text) if w]


def translated(line, reply):
    glue = sum(w in HINDI_GLUE for w in words_of(line))
    hindi = sum(w.lower() in HINDI_WORDS for w in words_of(reply))
    return glue >= 3 and hindi * 3 < glue


def chat_text(url, key, user):
    """engine::chat_body with one user message, as ai::Model::chat_text sends it."""
    body = {"messages": [{"role": "user", "content": user}], "stream": False, "temperature": 0.2,
            "cache_prompt": True, "chat_template_kwargs": {"enable_thinking": False}}
    req = urllib.request.Request(f"{url}/v1/chat/completions", json.dumps(body).encode(),
                                 {"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=20) as r:
        out = json.loads(r.read())["choices"][0]["message"]["content"]
    return re.sub(r"<think>.*?</think>", "", out, flags=re.S).strip()


def romanize_line(url, key, line):
    def ask(extra):
        text = chat_text(url, key, f"{ROMAN_PROMPT}{extra}\n\n<transcript>\n{line}\n</transcript>")
        text = text.replace("<transcript>", "").replace("</transcript>", "").strip()
        if len(words_of(text)) <= len(words_of(line)) * 0.6:
            raise ValueError("the reply dropped words")
        return text
    first = ask("")
    if not translated(line, first):
        return first, "model"
    again = ask(RETRY)
    if translated(line, again):
        raise ValueError("the reply translated the line")
    return again, "model (asked twice)"


def roman(tag, url, key):
    from score import romanize as letters  # training/bench/score.py, stands in for transliterate()
    path = RUNS / f"asr-{tag}.jsonl"
    rows = [json.loads(l) for l in path.read_text().splitlines()]
    with (RUNS / f"asr-{tag}-roman.jsonl").open("w") as f:
        for r in rows:
            src = r["asr_text"]
            t = time.perf_counter()
            how = "no Devanagari"
            text = src
            if DEV.search(src):
                try:
                    text, how = romanize_line(url, key, src)
                except Exception as e:  # noqa: BLE001
                    text, how = letters(src), f"letter rules ({e})"
            ms = round(1000 * (time.perf_counter() - t))
            f.write(json.dumps(dict(r, text=text, roman_how=how, roman_ms=ms), ensure_ascii=False) + "\n")
            print(f"{r['clip']:<16} {ms} ms {how} | {text}")


def cleanup(asr_tag, url, key, out_tag):
    import run
    run.ASR_MODELS[asr_tag] = (None, None)
    run.cases = lambda: cases
    run.clips = lambda: clips
    (RUNS / f"cleanup-{out_tag}.jsonl").unlink(missing_ok=True)
    sys.argv = ["run.py", "cleanup", "--api", "openai", "--url", url, "--key", key, "--model", "lipik-cleanup",
                "--asr", asr_tag, "--vocab-in-prompt", "--prepass", "--parts", "--num-predict", "--tag", out_tag]
    run.main()
    asr = {r["clip"]: r for r in map(json.loads, (RUNS / f"asr-{asr_tag}.jsonl").read_text().splitlines())}
    path = RUNS / f"cleanup-{out_tag}.jsonl"
    rows = [json.loads(l) for l in path.read_text().splitlines()]
    path.write_text("".join(json.dumps(dict(r, asr_ms=asr[r["clip"]]["asr_ms"],
                                            roman_ms=asr[r["clip"]].get("roman_ms", 0),
                                            asr_text=asr[r["clip"]]["asr_text"]), ensure_ascii=False) + "\n"
                            for r in rows))


if __name__ == "__main__":
    step, *args = sys.argv[1:]
    {"cli": cli, "tcpp": tcpp, "roman": roman, "cleanup": cleanup, "devaudio": devaudio}[step](*args)
