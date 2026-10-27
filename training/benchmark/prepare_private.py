"""Build the private clip set from Lipik's own data (read-only) into private/.

    python3 prepare_private.py

Dictations: every WAV in recordings/, with the app it was typed into and what
Lipik typed at the time (history.db). Meetings: ~150 pieces of 3-20 s cut
from notetaker/<id>/{mic,system}.wav at the segment times in notetaker.db,
only for meetings whose status is 'ready'. Every clip gets 0.3 s of silence
before and 0.6 s after, as 16 kHz mono 16-bit WAV.

Writes private/clips.json and private/audio/*.wav. Nothing here is committed.
"""

import json
import sqlite3
import subprocess
from pathlib import Path

HERE = Path(__file__).parent
PRIVATE = HERE / "private"
AUDIO = PRIVATE / "audio"
LIPIK = Path.home() / "Library/Application Support/com.gourav.lipik"
MIN_MS, MAX_MS = 3000, 20000
SYSTEM_CLIPS = 65
PAD = "adelay=300,apad=pad_dur=0.6"


def cut(src, dst, start=None, dur=None):
    if dst.exists():
        return
    cmd = ["ffmpeg", "-v", "error", "-y"]
    if start is not None:
        cmd += ["-ss", f"{start:.3f}", "-t", f"{dur:.3f}"]
    cmd += ["-i", str(src), "-af", PAD, "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(dst)]
    subprocess.run(["perl", "-e", "alarm shift; exec @ARGV", "60", *cmd], check=True)


def ro(path):
    return sqlite3.connect(f"file:{path}?mode=ro", uri=True)


def dictations():
    db = ro(LIPIK / "history.db")
    rows = db.execute("""SELECT file_name, transcription_text, post_processed_text, app_bundle_id, duration_ms
                         FROM transcription_history ORDER BY timestamp""").fetchall()
    clips = []
    for i, (name, raw, post, bundle, dur) in enumerate(rows, 1):
        src = LIPIK / "recordings" / name
        if not src.exists():
            continue
        cid = f"d{i:02d}"
        cut(src, AUDIO / f"{cid}.wav")
        clips.append({"id": cid, "category": "dictation", "source_file": f"recordings/{name}",
                      "app": bundle or "", "seconds": round((dur or 0) / 1000, 2),
                      "typed_at_the_time": post if post is not None else raw})
    return clips


def spread(items, n):
    if len(items) <= n:
        return items
    step = len(items) / n
    return [items[int(i * step)] for i in range(n)]


def meetings():
    db = ro(LIPIK / "notetaker/notetaker.db")
    ready = db.execute("SELECT id, mic_offset_ms, system_offset_ms FROM meetings WHERE status = 'ready' ORDER BY start_ms").fetchall()
    picked = {"mic": [], "system": []}
    for mid, mic_off, sys_off in ready:
        for source, offset in (("mic", mic_off), ("system", sys_off)):
            passes = [p for (p,) in db.execute(
                "SELECT DISTINCT pass FROM segments WHERE meeting_id = ? AND source = ?", (mid, source))]
            use = "final" if "final" in passes else (passes[0] if passes else None)
            if use is None:
                continue
            segs = db.execute("""SELECT start_ms, end_ms FROM segments
                                 WHERE meeting_id = ? AND source = ? AND pass = ? AND end_ms - start_ms BETWEEN ? AND ?
                                 ORDER BY start_ms""", (mid, source, use, MIN_MS, MAX_MS)).fetchall()
            picked[source].append((mid, offset or 0, use, segs))
    # All of the user's own mic pieces; system pieces split evenly over the meetings that have them.
    per_meeting = max(1, SYSTEM_CLIPS // max(1, len(picked["system"])))
    clips = []
    for source, groups in picked.items():
        for mid, offset, use, segs in groups:
            if source == "system":
                segs = spread(segs, per_meeting)
            wav = LIPIK / "notetaker" / mid / f"{source}.wav"
            for n, (s, e) in enumerate(segs, 1):
                cid = f"m{mid[-4:]}-{'me' if source == 'mic' else 'sys'}-{n:03d}"
                start = max(0.0, (s - offset) / 1000)
                cut(wav, AUDIO / f"{cid}.wav", start, (e - s) / 1000)
                clips.append({"id": cid, "category": "meeting-me" if source == "mic" else "meeting-others",
                              "source_file": f"notetaker/{mid}/{source}.wav", "pass": use,
                              "start_s": round(start, 3), "seconds": round((e - s) / 1000, 2), "app": ""})
    return clips


def main():
    AUDIO.mkdir(parents=True, exist_ok=True)
    clips = dictations() + meetings()
    (PRIVATE / "clips.json").write_text(json.dumps(clips, indent=1, ensure_ascii=False))
    counts = {}
    for c in clips:
        counts[c["category"]] = counts.get(c["category"], 0) + 1
    print(f"{len(clips)} clips: {counts}")


if __name__ == "__main__":
    main()
