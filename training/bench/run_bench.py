"""Transcribe every clip with every model config through the app's headless mode.

Uses a portable copy of the dev binary in app/ (a `portable` marker next to it
sends settings and models to app/Data), so the running app is never touched.
One model process at a time. Results append to runs/<config>.jsonl; reruns skip
clips already done.

    python3 run_bench.py [config ...]
"""

import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).parent
APP = HERE / "app"
RUNS = HERE / "runs"
PARAKEET = "handy-computer/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf"
WHISPER = "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf"
QWEN06 = "handy-computer/Qwen3-ASR-0.6B-gguf/Qwen3-ASR-0.6B-Q8_0.gguf"
QWEN17 = "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q8_0.gguf"

CONFIGS = {
    "parakeet-en": (PARAKEET, "en"),
    "whisper-turbo-en": (WHISPER, "en"),
    "whisper-turbo-hi": (WHISPER, "hi"),
    "qwen3-0.6b-en": (QWEN06, "en"),
    "qwen3-0.6b-hi": (QWEN06, "hi"),
    "qwen3-0.6b-auto": (QWEN06, "auto"),
    "qwen3-1.7b-en": (QWEN17, "en"),
    "qwen3-1.7b-hi": (QWEN17, "hi"),
}


def run_config(name):
    model, language = CONFIGS[name]
    (APP / "Data" / "settings_store.json").write_text(
        json.dumps({"settings": {"selected_language": language}})
    )
    RUNS.mkdir(exist_ok=True)
    out = RUNS / f"{name}.jsonl"
    done = set()
    if out.exists():
        done = {json.loads(line)["clip"] for line in out.read_text().splitlines()}
    clips = json.loads((HERE / "audio" / "clips.json").read_text())
    start = time.time()
    with out.open("a") as f:
        for c in clips:
            if c["clip"] in done:
                continue
            proc = subprocess.run(
                ["perl", "-e", "alarm shift; exec @ARGV", "180", str(APP / "lipik"),
                 "--transcribe-file", str(HERE / "audio" / c["clip"]),
                 "--model", model, "--repeat", "3", "--json"],
                capture_output=True, text=True,
            )
            lines = [l for l in proc.stdout.splitlines() if l.startswith("{")]
            if not lines:
                err = proc.stderr.strip().splitlines()[-1:] or ["no output"]
                print(f"{name} {c['clip']}: failed: {err[0]}", file=sys.stderr)
                continue
            r = json.loads(lines[-1])
            f.write(json.dumps({
                "clip": c["clip"], "text": r["text"], "audio_secs": r["audio_secs"],
                "load_ms": r["load_ms"], "transcribe_ms": r["transcribe_ms"],
                "best_ms": r["best_ms"], "backend": r["bound_backend"],
            }) + "\n")
            f.flush()
    print(f"{name}: {time.time() - start:.0f} s")


if __name__ == "__main__":
    for name in sys.argv[1:] or CONFIGS:
        run_config(name)
