"""Boosted copies of the quiet-room whispers (_wq -> _wqb): the whole clip raised so
its loudest 10% of 32 ms windows sit at -26 dBFS, noise and all, as a gain step in
Lipik would do before speech detection. Run with a Python that has numpy.

    python whisper_boost.py <whisper clips.json>
"""
import json
import sys
from pathlib import Path

import numpy as np

from whisper_audio import read, write

TARGET_DBFS = -26.0
WINDOW = 512


def boost(x):
    n = len(x) // WINDOW
    levels = np.sqrt(np.mean(x[:n * WINDOW].reshape(n, WINDOW) ** 2, axis=1) + 1e-20)
    loud = np.percentile(levels, 90)
    return x * 10 ** (TARGET_DBFS / 20) / loud


clips_path = Path(sys.argv[1])
listed = []
for c in json.loads(clips_path.read_text()):
    if not c["clip"].endswith("_wq.wav"):
        continue
    name = c["clip"].replace("_wq.wav", "_wqb.wav")
    write(clips_path.parent / name, boost(read(clips_path.parent / c["clip"])))
    listed.append(dict(c, clip=name, voice=c["voice"].replace("-wq", "-wqb")))
(clips_path.parent / "clips-boosted.json").write_text(json.dumps(listed, indent=1))
print(len(listed), "boosted clips")
