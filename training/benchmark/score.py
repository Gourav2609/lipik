"""Score every finished run against the references and write the results pages.

    python3 score.py

Public set  -> RESULTS.md (numbers only; safe to commit).
Private set -> private/RESULTS.md (git-ignored).

WER = word error rate in %: word edits needed to turn the output into the
reference, divided by the reference's word count, over all clips together.
Both sides are lower-cased, punctuation is dropped and numbers are written as
digits first (training/textnorm.py normalize_wer). Lower is better.
Ready to send = % of clips where the output equals the reference exactly,
capitals and punctuation included; only a final period may differ.
Clips whose reference is empty (no speech) are left out.
"""

import json
import statistics
import sys
from pathlib import Path

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE.parent))
from textnorm import edit_distance, normalize_wer, ready_to_send  # noqa: E402

# (row name, run file contender, field)
ROWS = [
    ("Lipik, final text (Parakeet + clean-up)", "lipik", "final"),
    ("Lipik, raw Parakeet text", "lipik", "text"),
    ("Handy upstream (Parakeet, no clean-up)", "handy", "text"),
    ("Apple Dictation en-IN", "apple-en-IN", "text"),
    ("Qwen3-ASR 1.7B, raw", "qwen3-asr-1.7b", "text"),
    ("Whisper large-v3-turbo, raw", "whisper-turbo", "text"),
    ("Lipik as typed at the time (history.db)", "history", "text"),
]
CATEGORY_NAMES = {"dictation": "Dictations", "meeting-me": "Meetings: my voice (mic)",
                  "meeting-others": "Meetings: other people (system audio)"}


def load_runs(runs, set_name, contender):
    path = runs / f"{set_name}-{contender}.jsonl"
    if not path.exists():
        return None
    return {r["id"]: r for r in map(json.loads, path.read_text().splitlines())}


def score(refs, rows, field):
    edits = words = ready = n = errors = 0
    secs, decode, peak = [], [], []
    for ref in refs:
        r = rows.get(ref["id"])
        if r is None:
            continue
        n += 1
        if "error" in r:
            errors += 1
        hyp = r.get(field) or ""
        rw = normalize_wer(ref["text"]).split()
        edits += edit_distance(rw, normalize_wer(hyp).split())
        words += len(rw)
        ready += ready_to_send(hyp, ref["text"])
        if r.get("wall_s") is not None:
            secs.append(r["wall_s"])
        if r.get("decode_s") is not None:
            decode.append(r["decode_s"])
        if r.get("peak_mb"):
            peak.append(r["peak_mb"])
    if not n:
        return None
    return {"n": n, "wer": 100 * edits / max(words, 1), "ready": 100 * ready / n, "errors": errors,
            "secs": statistics.median(secs) if secs else None, "decode": statistics.median(decode) if decode else None,
            "peak": max(peak) if peak else None}


def fmt(v, digits=1, suffix=""):
    return "–" if v is None else f"{v:.{digits}f}{suffix}"


def table(refs, runs, set_name):
    lines = ["| Pipeline | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |",
             "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"]
    any_row = False
    for name, contender, field in ROWS:
        rows = load_runs(runs, set_name, contender)
        if rows is None:
            continue
        s = score(refs, rows, field)
        if s is None:
            continue
        any_row = True
        err = f" ({s['errors']} failed)" if s["errors"] else ""
        lines.append(f"| {name} | {s['n']}{err} | {fmt(s['wer'])} | {fmt(s['ready'], 0)} | {fmt(s['secs'], 2)} | "
                     f"{fmt(s['decode'], 2)} | {fmt(s['peak'], 0)} |")
    return "\n".join(lines) if any_row else "_No runs yet._"


NOTES = ("Seconds per clip = the whole command for one clip, so for Lipik, Handy and Qwen3-ASR it includes loading "
         "the model each time (the app keeps it loaded) and, for Lipik, the clean-up. Model time = speech-to-text "
         "only. Peak memory = the largest the benchmark process got, model worker included; Apple's model runs "
         "inside a macOS system service, so its number leaves the model out.")


def section(refs, runs, set_name):
    out = [NOTES, "", f"### All clips ({len(refs)})", "", table(refs, runs, set_name), ""]
    cats = sorted({r["category"] for r in refs})
    for cat in cats:
        sub = [r for r in refs if r["category"] == cat]
        out += [f"### {CATEGORY_NAMES.get(cat, cat)} ({len(sub)} clips)", "", table(sub, runs, set_name), ""]
    return out


def private_page():
    path = HERE / "private/references.json"
    if not path.exists():
        return None
    allrefs = json.loads(path.read_text())
    refs = [r for r in allrefs if normalize_wer(r["text"])]
    checked = sum(r.get("checked", False) for r in refs)
    hows = {}
    for r in refs:
        if not r.get("checked"):
            key = r.get("how", "").split(";")[-1].strip()
            hows[key] = hows.get(key, 0) + 1
    how_lines = [f"- {v} clips: {k}" for k, v in sorted(hows.items(), key=lambda kv: -kv[1])]
    out = ["# Private benchmark: my own voice", "",
           "Git-ignored. Never commit or share this file.", ""]
    if checked < len(refs):
        out += [f"**DRAFT REFERENCES.** {len(refs) - checked} of {len(refs)} references were made by models, not "
                "checked by a person. Correct them in `private/review.html`, then rerun `score.py`.", "",
                "How the draft references were made:", *how_lines, "",
                "Draft references favour the models they came from. Most come from **Parakeet** (the model inside "
                "both Lipik and Handy), so Parakeet-based rows look better than they are, and Apple and Qwen3-ASR "
                "look worse. The scores show how close each pipeline is to the draft, not to the truth. "
                "Ready to send is the weakest number here: the draft keeps Parakeet's own capitals and "
                "punctuation, which is not what a person would choose.", ""]
    empty = len(allrefs) - len(refs)
    if empty:
        out += [f"{empty} clips with no speech in the reference are left out.", ""]
    out += section(refs, HERE / "private/runs", "private")
    return "\n".join(out) + "\n"


def public_page():
    path = HERE / "public/references.json"
    out = ["# Speech-to-text benchmark: public set (Svarah)", "",
           "Indian-accented English from AI4Bharat's Svarah test set (CC BY 4.0). Scores are numbers only; "
           "the private set (my own voice) is scored in `private/RESULTS.md`, which is git-ignored.", "",
           "WER = word error rate in %, after lower-casing, dropping punctuation and writing numbers as digits. "
           "Lower is better. Ready to send = output equals the reference exactly, capitals and punctuation "
           "included (a final period may differ). Svarah's references are plain transcripts, not text a person "
           "would send, so **ready to send is not meaningful on this set**: read WER.", ""]
    if not path.exists():
        out += ["_Not run yet: the Svarah clips are not downloaded. See README.md._"]
        return "\n".join(out) + "\n"
    refs = [r for r in json.loads(path.read_text()) if normalize_wer(r["text"])]
    out += section(refs, HERE / "runs", "public")
    return "\n".join(out) + "\n"


def main():
    (HERE / "RESULTS.md").write_text(public_page())
    print("wrote RESULTS.md")
    page = private_page()
    if page:
        (HERE / "private/RESULTS.md").write_text(page)
        print("wrote private/RESULTS.md")


if __name__ == "__main__":
    main()
