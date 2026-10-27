"""Run one speech-to-text contender over one clip set, one clip at a time.

    python3 run.py --set private --contender lipik
    python3 run.py --set public --contender apple-en-IN
    python3 run.py --set private --contender qwen3-asr-1.7b --ids private/need_third.txt

Results append to runs/<set>-<contender>.jsonl (private set: private/runs/, so
its text never leaves the git-ignored folder). Clips that already finished
without an error are skipped, so a rerun picks up where the last one stopped
and retries failures; delete the file to start over.

Contenders:
  lipik            Lipik release binary, Parakeet, then Lipik's text steps
                   (empty dictionary, clean-up through local Ollama qwen3:8b).
                   Keeps both `text` (Parakeet raw) and `final` (after the steps).
  handy            Upstream Handy (cjpais/Handy main) release binary, Parakeet, no clean-up.
  apple-en-IN      Apple's on-device DictationTranscriber, locale en-IN.
  qwen3-asr-1.7b   Qwen3-ASR 1.7B through Lipik's binary, raw. Used as the third
                   opinion for draft references.
  whisper-turbo    Whisper large-v3-turbo through Lipik's binary, raw. Only if the
                   GGUF is already on this Mac.
  history          What Lipik typed at the time (history.db), private dictations only.

Every binary runs from a portable copy in apps/ (a `portable` marker sends its
settings and models to apps/<name>/Data), so the installed app is never touched.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import time
from pathlib import Path

HERE = Path(__file__).parent
REPO = HERE.parent.parent
APPS = HERE / "apps"
LIPIK_BIN = REPO / "src-tauri/target/release/bundle/macos/Lipik.app/Contents/MacOS/lipik"
HANDY_BIN = Path.home() / "Documents/repos/handy-bench/src-tauri/target/release/handy"
TEMPLATE_SETTINGS = REPO / "training/dictation/app/Data/settings_store.json"
LOCAL_MODELS = REPO / "training/bench/models"
HF_HUB = Path.home() / ".cache/huggingface/hub"

PARAKEET = "handy-computer/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf"
QWEN17 = "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q8_0.gguf"
WHISPER = "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf"
CLIP_TIMEOUT = 300


def set_paths(name):
    base = HERE / name
    runs = base / "runs" if name == "private" else HERE / "runs"
    return base / "clips.json", base / "audio", runs


def timed(cmd, timeout):
    """Run cmd under a hard timeout; return (stdout, stderr, wall seconds, peak RSS in MB)."""
    t0 = time.time()
    proc = subprocess.run(["/usr/bin/time", "-l", "perl", "-e", "alarm shift; exec @ARGV", str(timeout), *cmd],
                          capture_output=True, text=True)
    wall = time.time() - t0
    m = re.search(r"(\d+)\s+maximum resident set size", proc.stderr)
    return proc.stdout, proc.stderr, wall, (int(m[1]) / 1e6 if m else None)


def portable(name, binary, marker):
    """apps/<name>/: a copy of binary next to a `portable` marker and a Data/ folder."""
    app = APPS / name
    exe = app / binary.name
    (app / "Data").mkdir(parents=True, exist_ok=True)
    if not exe.exists() or exe.stat().st_mtime < binary.stat().st_mtime:
        shutil.copy2(binary, exe)
    (app / "portable").write_text(marker)
    return app, exe


def configure_lipik(app, model):
    """Same as training/dictation/run.py configure(), with an empty dictionary and no snippets."""
    data = app / "Data"
    store_path = data / "settings_store.json"
    if not store_path.exists():
        shutil.copy2(TEMPLATE_SETTINGS, store_path)
    store = json.loads(store_path.read_text())
    store["settings"].update({"custom_words": [], "selected_language": "en", "selected_model": model})
    store_path.write_text(json.dumps(store, indent=2))
    (data / "textflow.json").write_text(json.dumps({
        "dictionary": {"words": [], "replacements": []},
        "snippets": [],
        "style": {"personal": "casual", "work": "formal", "email": "formal", "other": "formal", "auto_cleanup": True},
        "transforms": {}, "pending_fixes": [], "cleanup_default_applied": True,
    }, indent=2))
    models = data / "models"
    models.mkdir(exist_ok=True)
    for gguf in LOCAL_MODELS.glob("*.gguf"):
        if not (models / gguf.name).exists():
            os.link(gguf, models / gguf.name)


def setup_handy():
    if not HANDY_BIN.exists():
        raise SystemExit(f"Handy is not built: {HANDY_BIN} is missing (see README).")
    app, exe = portable("handy", HANDY_BIN, "Handy Portable Mode")
    # Upstream portable mode looks for models under Data/huggingface; point it at the shared cache.
    hub = app / "Data/huggingface/hub"
    hub.mkdir(parents=True, exist_ok=True)
    repo = "models--handy-computer--parakeet-unified-en-0.6b-gguf"
    if not (hub / repo).exists():
        (hub / repo).symlink_to(HF_HUB / repo)
    return exe


def binary_contender(exe, model, wav, clean=None):
    cmd = [str(exe), "--transcribe-file", str(wav), "--model", model, "--json"]
    if clean is not None:
        cmd.append(f"--clean={clean}")
    out, err, wall, rss = timed(cmd, CLIP_TIMEOUT)
    lines = [line for line in out.splitlines() if line.startswith("{")]
    if not lines:
        reason = [line for line in err.splitlines() if line.startswith("error:")]
        return {"text": "", "final": "", "error": reason[-1][:300] if reason else "no output (timeout or crash)",
                "wall_s": round(wall, 2), "peak_mb": rss}
    r = json.loads(lines[-1])
    final = r.get("final")
    return {"text": r.get("text") or "", "final": final if final is not None else (r.get("text") or ""),
            "prepared": r.get("prepared"), "load_s": r.get("load_ms", 0) / 1000, "decode_s": r.get("best_ms", 0) / 1000,
            "wall_s": round(wall, 2), "peak_mb": rss}


def run_apple(todo, audio, out):
    swift_bin = APPS / "apple-transcribe"
    src = HERE / "apple.swift"
    if not swift_bin.exists() or swift_bin.stat().st_mtime < src.stat().st_mtime:
        APPS.mkdir(exist_ok=True)
        subprocess.run(["perl", "-e", "alarm shift; exec @ARGV", "600", "swiftc", "-O", "-o", str(swift_bin), str(src)],
                       check=True)
    raw = out.with_suffix(".apple-raw.jsonl")
    listing = out.with_suffix(".todo.tsv")
    listing.write_text("".join(f"{c['id']}\t{audio / (c['id'] + '.wav')}\n" for c in todo))
    _, err, wall, rss = timed([str(swift_bin), "en-IN", str(listing), str(raw)], 60 * 60)
    listing.unlink()
    if err and "error" in err.lower():
        print(err[-500:])
    rows = {}
    if raw.exists():
        for line in raw.read_text().splitlines():
            r = json.loads(line)
            rows[r["id"]] = r
    with out.open("a") as f:
        for c in todo:
            r = rows.get(c["id"])
            if r is None:
                continue
            row = {"id": c["id"], "category": c["category"], "text": r["text"], "final": r["text"],
                   "decode_s": round(r["seconds"], 3), "wall_s": round(r["seconds"], 3), "peak_mb": rss}
            if r.get("error"):
                row["error"] = r["error"]
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
    raw.unlink(missing_ok=True)
    print(f"apple: {len(rows)} clips in {wall:.0f} s, peak {rss:.0f} MB")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--set", required=True, choices=["private", "public"])
    ap.add_argument("--contender", required=True,
                    choices=["lipik", "handy", "apple-en-IN", "qwen3-asr-1.7b", "whisper-turbo", "history"])
    ap.add_argument("--ids", help="file with one clip id per line: run only these")
    ap.add_argument("--limit", type=int, help="run at most this many clips")
    args = ap.parse_args()

    clips_path, audio, runs = set_paths(args.set)
    clips = json.loads(clips_path.read_text())
    if args.ids:
        wanted = set(Path(args.ids).read_text().split())
        clips = [c for c in clips if c["id"] in wanted]
    runs.mkdir(parents=True, exist_ok=True)
    out = runs / f"{args.set}-{args.contender}.jsonl"
    old = [json.loads(line) for line in out.read_text().splitlines()] if out.exists() else []
    done = {r["id"] for r in old if "error" not in r}
    todo = [c for c in clips if c["id"] not in done][: args.limit]
    print(f"{args.set}/{args.contender}: {len(done)} done, {len(todo)} to run")
    if not todo:
        return

    if args.contender == "history":
        with out.open("a") as f:
            for c in todo:
                if "typed_at_the_time" in c:
                    f.write(json.dumps({"id": c["id"], "category": c["category"], "text": c["typed_at_the_time"],
                                        "final": c["typed_at_the_time"]}, ensure_ascii=False) + "\n")
        return
    if args.contender == "apple-en-IN":
        run_apple(todo, audio, out)
        return

    if args.contender == "handy":
        exe, model, clean = setup_handy(), PARAKEET, None
    else:
        model = {"lipik": PARAKEET, "qwen3-asr-1.7b": QWEN17, "whisper-turbo": WHISPER}[args.contender]
        app, exe = portable("lipik", LIPIK_BIN, "Lipik Portable Mode")
        configure_lipik(app, model)
        clean = None
        if args.contender == "whisper-turbo" and not (app / "Data/models" / Path(WHISPER).name).exists() \
                and not list(HF_HUB.glob("models--handy-computer--whisper-large-v3-turbo-gguf")):
            raise SystemExit("Whisper large-v3-turbo GGUF is not on this Mac; not downloading it.")

    for n, c in enumerate(todo, 1):
        wav = audio / f"{c['id']}.wav"
        if args.contender == "lipik":
            clean = c.get("app", "")
        r = binary_contender(exe, model, wav, clean)
        row = {"id": c["id"], "category": c["category"], **r}
        with out.open("a") as f:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
        flag = " ERROR" if "error" in r else ""
        print(f"[{n}/{len(todo)}] {c['id']} {r['wall_s']:.1f}s peak {r['peak_mb'] or 0:.0f} MB{flag}", flush=True)


if __name__ == "__main__":
    main()
