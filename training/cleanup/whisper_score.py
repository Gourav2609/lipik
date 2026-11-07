"""Fully-right counts for the whispered test runs, split by how the clip was made.

    python3 whisper_score.py <tag> ...     (HCLIPS as for the run, for Hinglish runs)

Groups: normal voice, whispered (_w), whispered quiet in a room (_wq), and the
macOS Whisper voice. English = every category but hinglish.
"""
import collections
import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("cleanup_score", HERE / "score.py")
cs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cs)


def group(clip):
    stem = clip[:-4]
    if stem.endswith("_wq"):
        return "whispered, quiet room"
    if stem.endswith("_w"):
        return "whispered"
    if stem.endswith("_Whisper"):
        return "macOS Whisper voice"
    return "normal"


for tag in sys.argv[1:]:
    _, results = cs.score(tag)
    n = collections.Counter()
    ok = collections.Counter()
    for r in results:
        key = (group(r["clip"]), "Hinglish" if r["category"] == "hinglish" else "English")
        n[key] += 1
        ok[key, "raw"] += bool(r["raw"]["pass"])
        ok[key, "best"] += bool(r["best"]["pass"])
    for key in sorted(n):
        print(f"{tag:<10} {key[1]:<9} {key[0]:<22} clean-up off {ok[key, 'raw']:>3}/{n[key]:<3}  on {ok[key, 'best']:>3}/{n[key]}")
