"""Build a speech fine-tuning dataset from Lipik's dictation history.

Reads history.db (through a temporary copy, the app keeps it open) and the WAV
files next to it, and writes:

    <out>/<speaker>/audio/*.wav     16 kHz mono 16-bit
    <out>/<speaker>/train.jsonl     {"audio", "text", "duration", "timestamp", ...}
    <out>/<speaker>/val.jsonl       the newest clips (split by date, never shuffled)
    <out>/<speaker>/summary.json    counts only, no text

Text choice per clip, best first:
    1. --corrections file (your own fixes, keyed by file_name)
    2. corrected_text (your edit in Lipik's History; snippet expansions are undone;
       dropped when it changes too much of the raw text, see --max-change-edited)
    3. post_processed_text (after replacements; snippet expansions are undone)
    4. transcription_text (what the speech model wrote)

Usage:
    uv run ... python export.py --out data
    uv run ... python export.py --out data --speaker priya --app-dir ~/lipik-priya
    uv run ... --with mlx-whisper python export.py --out data --split-long
"""

import argparse
import datetime as dt
import difflib
import getpass
import json
import math
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import wave
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from textnorm import has_indic_script, normalize, word_change_ratio  # noqa: E402

SR = 16000
DEFAULT_APP_DIR = Path.home() / "Library/Application Support/com.gourav.lipik"
ALIGN_MODEL = "mlx-community/whisper-large-v3-turbo"
SHORT_RAW_WORDS = 8


def copy_db(db_path: Path, dest: Path) -> None:
    src = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    dst = sqlite3.connect(dest)
    try:
        src.backup(dst)
    finally:
        src.close()
        dst.close()


def load_rows(db_path: Path) -> list[dict]:
    with tempfile.TemporaryDirectory() as tmp:
        copy = Path(tmp) / "history.db"
        copy_db(db_path, copy)
        con = sqlite3.connect(copy)
        con.row_factory = sqlite3.Row
        cols = {r[1] for r in con.execute("PRAGMA table_info(transcription_history)")}
        wanted = ["id", "file_name", "timestamp", "transcription_text", "post_processed_text",
                  "corrected_text", "post_process_requested", "duration_ms"]
        select = ", ".join(c if c in cols else f"NULL AS {c}" for c in wanted)
        rows = [dict(r) for r in con.execute(f"SELECT {select} FROM transcription_history ORDER BY timestamp")]
        con.close()
    return rows


def load_snippets(app_dir: Path) -> list[tuple[str, str]]:
    path = app_dir / "textflow.json"
    if not path.exists():
        return []
    data = json.loads(path.read_text())
    pairs = [(s["trigger"], s["expansion"]) for s in data.get("snippets", []) if s.get("expansion")]
    return sorted(pairs, key=lambda p: -len(p[1]))


def load_corrections(path: Path | None) -> dict[str, str]:
    if not path:
        return {}
    out = {}
    for line in path.read_text().splitlines():
        if line.strip():
            item = json.loads(line)
            out[item["file_name"]] = item["text"]
    return out


def read_wav(path: Path) -> np.ndarray:
    """Return float32 mono 16 kHz samples; converts other formats with ffmpeg."""
    try:
        with wave.open(str(path)) as w:
            if w.getframerate() == SR and w.getnchannels() == 1 and w.getsampwidth() == 2:
                return np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float32) / 32768
    except wave.Error:
        pass
    raw = subprocess.run(
        ["ffmpeg", "-nostdin", "-v", "error", "-i", str(path), "-ac", "1", "-ar", str(SR), "-f", "s16le", "-"],
        check=True, capture_output=True,
    ).stdout
    return np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768


def write_wav(path: Path, samples: np.ndarray) -> None:
    pcm = (np.clip(samples, -1, 1) * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def undo_snippets(text: str, snippets: list) -> str:
    for trigger, expansion in snippets:
        text = text.replace(expansion, trigger)
    return text.strip()


def pick_text(row: dict, corrections: dict, snippets: list) -> tuple[str, str]:
    if row["file_name"] in corrections:
        return corrections[row["file_name"]].strip(), "corrected"
    edited = row["corrected_text"]
    if edited and edited.strip():
        return undo_snippets(edited, snippets), "corrected"
    final = row["post_processed_text"]
    if final and final.strip():
        return undo_snippets(final, snippets), "final"
    return (row["transcription_text"] or "").strip(), "raw"


def split_points(samples: np.ndarray, text: str, max_sec: float, model: str) -> list[tuple[int, int, str]] | None:
    """Cut a long clip into pieces of at most max_sec, cutting only in pauses
    between words that the base model and the text agree on. Returns
    (start_sample, end_sample, text) pieces, or None when no safe cut exists."""
    import mlx_whisper

    result = mlx_whisper.transcribe(samples, path_or_hf_repo=model, language="en",
                                    word_timestamps=True, condition_on_previous_text=False, verbose=None)
    hyp = [(normalize(w["word"]), w["start"], w["end"])
           for seg in result["segments"] for w in seg.get("words", []) if normalize(w["word"])]
    tokens = text.split()
    units = [(u, t) for t, tok in enumerate(tokens) for u in normalize(tok).split()]
    matcher = difflib.SequenceMatcher(a=[u for u, _ in units], b=[h[0] for h in hyp], autojunk=False)
    unit_to_hyp = {}
    for block in matcher.get_matching_blocks():
        for k in range(block.size):
            unit_to_hyp[block.a + k] = block.b + k

    # A cut after token t is safe when the last unit of t and the first unit of
    # t+1 both matched, to neighbouring hypothesis words.
    cuts = []  # (time, token_index_after_cut)
    for ui in range(len(units) - 1):
        if units[ui][1] != units[ui + 1][1] and ui in unit_to_hyp and ui + 1 in unit_to_hyp:
            hj = unit_to_hyp[ui]
            if unit_to_hyp[ui + 1] == hj + 1:
                gap_start, gap_end = hyp[hj][2], hyp[hj + 1][1]
                cuts.append(((gap_start + gap_end) / 2, units[ui + 1][1], gap_end - gap_start))

    total = len(samples) / SR
    pieces, t0, tok0 = [], 0.0, 0
    while total - t0 > max_sec:
        window = [c for c in cuts if t0 + max_sec * 0.4 <= c[0] <= t0 + max_sec]
        if not window:
            return None
        cut_time, cut_tok, _ = max(window, key=lambda c: c[2])  # widest pause
        pieces.append((int(t0 * SR), int(cut_time * SR), " ".join(tokens[tok0:cut_tok])))
        t0, tok0 = cut_time, cut_tok
    pieces.append((int(t0 * SR), len(samples), " ".join(tokens[tok0:])))
    return pieces


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=Path, default=Path(__file__).resolve().parent / "data")
    ap.add_argument("--speaker", default=getpass.getuser(), help="one folder (and later one adapter) per person")
    ap.add_argument("--app-dir", type=Path, default=DEFAULT_APP_DIR,
                    help="folder holding history.db and recordings/ (a friend's copy for --speaker)")
    ap.add_argument("--corrections", type=Path, help='JSONL of {"file_name": ..., "text": ...} fixes')
    ap.add_argument("--min-sec", type=float, default=1.0)
    ap.add_argument("--max-sec", type=float, default=30.0, help="Whisper sees at most 30 s at a time")
    ap.add_argument("--min-words", type=int, default=2)
    ap.add_argument("--max-change", type=float, default=0.4,
                    help="drop rows whose final text changes more than this share of the raw words")
    ap.add_argument("--max-change-edited", type=float, default=0.6,
                    help="the same check for History edits (corrected_text), against the raw words")
    ap.add_argument("--max-change-edited-short", type=float, default=0.85,
                    help=f"--max-change-edited when the raw text has fewer than {SHORT_RAW_WORDS} words")
    ap.add_argument("--split-long", action="store_true",
                    help="cut clips over --max-sec at pauses (needs mlx-whisper and the base model)")
    ap.add_argument("--align-model", default=ALIGN_MODEL)
    ap.add_argument("--val-fraction", type=float, default=0.1, help="newest share of clips held out")
    ap.add_argument("--val-from", help="YYYY-MM-DD: clips from this local date on go to validation instead")
    ap.add_argument("--stats-only", action="store_true", help="count and report, write nothing")
    args = ap.parse_args()

    db_path, rec_dir = args.app_dir / "history.db", args.app_dir / "recordings"
    rows = load_rows(db_path)
    snippets = load_snippets(args.app_dir)
    corrections = load_corrections(args.corrections)

    dropped: dict[str, int] = {}
    sources: dict[str, int] = {}
    total_sec = 0.0
    kept = []  # (row, samples, text, source)

    def drop(reason: str) -> None:
        dropped[reason] = dropped.get(reason, 0) + 1

    for row in rows:
        path = rec_dir / row["file_name"]
        if not path.exists():
            drop("audio_missing")
            continue
        samples = read_wav(path)
        sec = len(samples) / SR
        total_sec += sec
        text, source = pick_text(row, corrections, snippets)
        if not text:
            drop("empty_text")
        elif sec < args.min_sec or len(normalize(text).split()) < args.min_words:
            drop("too_short")
        elif has_indic_script(text):
            drop("not_roman_script")
        elif source == "final" and word_change_ratio(row["transcription_text"] or "", text) > args.max_change:
            drop("rewritten_not_corrected")
        elif (source == "corrected" and row["file_name"] not in corrections
              and word_change_ratio(row["transcription_text"] or "", text) > edit_limit(row, args)):
            drop("rewritten_edit")
        elif sec > args.max_sec and not args.split_long:
            drop("too_long")
        else:
            kept.append((row, samples, text, source))

    # Split by date on whole clips, so pieces of one clip never straddle train/val.
    kept.sort(key=lambda k: k[0]["timestamp"])
    if args.val_from:
        cutoff = dt.datetime.fromisoformat(args.val_from).timestamp()
        n_val = sum(1 for k in kept if ts_seconds(k[0]["timestamp"]) >= cutoff)
    else:
        n_val = math.ceil(len(kept) * args.val_fraction) if len(kept) >= 2 else 0
    split_of = ["train"] * (len(kept) - n_val) + ["val"] * n_val

    spk_dir = args.out / args.speaker
    if not args.stats_only:
        if spk_dir.exists():
            if not (spk_dir / "summary.json").exists():
                sys.exit(f"{spk_dir} exists and was not made by export.py; refusing to overwrite")
            shutil.rmtree(spk_dir)
        (spk_dir / "audio").mkdir(parents=True)

    files = {"train": [], "val": []}
    seconds = {"train": 0.0, "val": 0.0}
    for (row, samples, text, source), split in zip(kept, split_of):
        if len(samples) / SR > args.max_sec:
            pieces = split_points(samples, text, args.max_sec, args.align_model)
            if pieces is None:
                drop("too_long_no_safe_cut")
                continue
        else:
            pieces = [(0, len(samples), text)]
        sources[source] = sources.get(source, 0) + 1
        stem = Path(row["file_name"]).stem
        for i, (a, b, piece_text) in enumerate(pieces):
            name = f"{stem}.wav" if len(pieces) == 1 else f"{stem}_{i:02d}.wav"
            dur = (b - a) / SR
            if not args.stats_only:
                write_wav(spk_dir / "audio" / name, samples[a:b])
            files[split].append({
                "audio": f"audio/{name}", "text": piece_text, "duration": round(dur, 3),
                "timestamp": ts_seconds(row["timestamp"]), "speaker": args.speaker,
                "source": source, "history_id": row["id"], "piece": i if len(pieces) > 1 else None,
            })
            seconds[split] += dur

    summary = {
        "speaker": args.speaker,
        "history_rows": len(rows),
        "history_audio_minutes": round(total_sec / 60, 2),
        "kept_clips": sum(sources.values()),
        "dropped": dropped,
        "text_source": sources,
        "train": {"items": len(files["train"]), "minutes": round(seconds["train"] / 60, 2)},
        "val": {"items": len(files["val"]), "minutes": round(seconds["val"] / 60, 2)},
        "created": dt.datetime.now().isoformat(timespec="seconds"),
    }
    if not args.stats_only:
        for split, items in files.items():
            with open(spk_dir / f"{split}.jsonl", "w") as f:
                for item in items:
                    f.write(json.dumps(item, ensure_ascii=False) + "\n")
        (spk_dir / "summary.json").write_text(json.dumps(summary, indent=2))

    print(json.dumps(summary, indent=2))
    report_all(args.out, summary if args.stats_only else None)


def edit_limit(row: dict, args: argparse.Namespace) -> float:
    raw_words = len(normalize(row["transcription_text"] or "").split())
    return args.max_change_edited_short if raw_words < SHORT_RAW_WORDS else args.max_change_edited


def ts_seconds(ts: int) -> int:
    return ts // 1000 if ts > 10**12 else ts


def report_all(out: Path, extra: dict | None) -> None:
    summaries = [json.loads(p.read_text()) for p in sorted(out.glob("*/summary.json"))] if out.exists() else []
    if extra:
        summaries = [s for s in summaries if s["speaker"] != extra["speaker"]] + [extra]
    print("\nspeaker            train min   val min   train h   kept/rows")
    for s in summaries:
        print(f"{s['speaker']:<18} {s['train']['minutes']:9.1f} {s['val']['minutes']:9.1f}"
              f" {s['train']['minutes'] / 60:9.2f}   {s['kept_clips']}/{s['history_rows']}")


if __name__ == "__main__":
    main()
