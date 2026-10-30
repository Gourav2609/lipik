"""Run the 126 clean-up benchmark clips through a portable copy of the released
lipik binary (--transcribe-file --clean=<bundle> --json) and write rows that
training/cleanup/score.py reads (runs/cleanup-<tag>.jsonl).

    python3 cli_run.py <portable dir> <shim log> <tag>
"""

import json
import subprocess
import sys
import time
from pathlib import Path

P, SHIMLOG, TAG = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
BENCH = Path(__file__).parent
PARAKEET = "lipik/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf"
BUNDLE = {"Slack": "com.tinyspeck.slackmacgap", "Mail": "com.apple.mail", "Messages": "com.apple.MobileSMS",
          "Notes": "com.apple.Notes", "Claude Code": "com.mitchellh.ghostty"}

cases = {c["id"]: c for c in json.loads((BENCH / "cases.json").read_text())}
clips = json.loads((BENCH / "audio/clips.json").read_text())
out = BENCH / "runs" / f"cleanup-{TAG}.jsonl"
done = {json.loads(l)["clip"] for l in out.read_text().splitlines()} if out.exists() else set()
SHIMLOG.touch()
for c in clips:
    if c["clip"] in done:
        continue
    before = len(SHIMLOG.read_text().splitlines())
    t = time.perf_counter()
    proc = subprocess.run(["perl", "-e", "alarm shift; exec @ARGV", "180", str(P / "lipik"),
                           "--transcribe-file", str(BENCH / "audio" / c["clip"]), "--model", PARAKEET,
                           f"--clean={BUNDLE[cases[c['id']]['app']]}", "--json"],
                          capture_output=True, text=True)
    wall = (time.perf_counter() - t) * 1000
    lines = [l for l in proc.stdout.splitlines() if l.startswith("{")]
    if not lines:
        print(f"{c['clip']}: FAILED rc={proc.returncode} {proc.stderr.strip()[-300:]}", file=sys.stderr)
        sys.exit(1)
    r = json.loads(lines[-1])
    calls = [json.loads(l) for l in SHIMLOG.read_text().splitlines()[before:]]
    path = "cleaned" if calls else "rules"
    row = {"clip": c["clip"], "id": c["id"], "voice": c["voice"], "raw": r["text"], "prepared": r.get("prepared"),
           "model": "lipik 0.1.12 CLI", "final": r["final"], "final_why": path, "calls": calls, "model_out": None,
           "latency_ms": sum(x["ms"] for x in calls) if calls else 0, "wall_ms": round(wall),
           "app_final": r["final"], "app_path": path}
    with out.open("a") as f:
        f.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(f"{c['clip']:<18} {len(calls)} calls {round(wall)} ms | {r['final']!r}", flush=True)
