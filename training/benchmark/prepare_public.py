"""Download ~150 clips of Svarah (AI4Bharat, CC BY 4.0) into public/.

    HF_TOKEN=hf_... .venv/bin/python prepare_public.py [--clips 150]

Svarah is gated on Hugging Face: log in, open
https://huggingface.co/datasets/ai4bharat/Svarah, accept the terms, and make
a read token. Only a few row groups of the three parquet files are fetched
(byte ranges), never the whole 1.1 GB set.

Writes public/audio/<id>.wav (git-ignored; 16 kHz mono, 0.3 s silence before
and 0.6 s after) and public/clips.json + public/references.json (committed).
Category = the speaker's first language, as Svarah lists it.
"""

import argparse
import json
import os
import random
import subprocess
from pathlib import Path

import pyarrow.parquet as pq
from huggingface_hub import HfFileSystem

HERE = Path(__file__).parent
PUBLIC = HERE / "public"
AUDIO = PUBLIC / "audio"
REPO = "datasets/ai4bharat/Svarah"
PAD = "adelay=300,apad=pad_dur=0.6"


def to_wav(data, dst):
    subprocess.run(["perl", "-e", "alarm shift; exec @ARGV", "60", "ffmpeg", "-v", "error", "-y", "-i", "pipe:0",
                    "-af", PAD, "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(dst)], input=data, check=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--clips", type=int, default=150)
    ap.add_argument("--seed", type=int, default=7)
    args = ap.parse_args()
    if not os.environ.get("HF_TOKEN"):
        raise SystemExit("Set HF_TOKEN (Svarah is gated; see the top of this file).")
    fs = HfFileSystem(token=os.environ["HF_TOKEN"])
    files = sorted(fs.glob(f"{REPO}/data/test-*.parquet"))
    rng = random.Random(args.seed)
    groups = []
    for f in files:
        meta = pq.ParquetFile(fs.open(f, "rb")).metadata
        groups += [(f, g, meta.row_group(g).num_rows) for g in range(meta.num_row_groups)]
    rng.shuffle(groups)
    per_group = 10
    AUDIO.mkdir(parents=True, exist_ok=True)
    clips, refs = [], []
    for f, g, rows in groups:
        if len(clips) >= args.clips:
            break
        table = pq.ParquetFile(fs.open(f, "rb")).read_row_group(g).to_pylist()
        for i in sorted(rng.sample(range(rows), min(per_group, rows))):
            if len(clips) >= args.clips:
                break
            row = table[i]
            text = (row.get("text") or "").strip()
            if not text or not 1 <= (row.get("duration") or 0) <= 30:
                continue
            audio = row["audio_filepath"]
            cid = f"sv-{Path(f).stem[-12:-8]}-{g:03d}-{i:04d}"
            to_wav(audio["bytes"], AUDIO / f"{cid}.wav")
            clips.append({"id": cid, "category": (row.get("primary_language") or "unknown").strip().lower(),
                          "seconds": round(row["duration"], 2), "source": f"{Path(f).name} row group {g} row {i}",
                          "original_file": Path(audio.get("path") or "").name, "gender": row.get("gender"),
                          "app": ""})
            refs.append({"id": cid, "category": clips[-1]["category"], "text": text, "checked": True})
        print(f"{len(clips)} clips", flush=True)
    (PUBLIC / "clips.json").write_text(json.dumps(clips, indent=1, ensure_ascii=False))
    (PUBLIC / "references.json").write_text(json.dumps(refs, indent=1, ensure_ascii=False))


if __name__ == "__main__":
    main()
