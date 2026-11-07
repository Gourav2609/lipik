"""Whispered copies of the test clips, to see whether Lipik still works when you can't
speak out loud. Run with a Python that has numpy and scipy.

    python whisper_audio.py <clips.json> <out dir>

For each clip, two copies (16 kHz mono, same names with a suffix):

- `_w`:  whispered. Every 25 ms the voice's spectral shape is kept (16-pole linear
  prediction) but the buzz of the vocal cords is replaced with noise, which is
  how a whisper sounds; low rumble under 250 Hz is cut. Same loudness as the clip.
- `_wq`: the whisper as a laptop mic hears it in an office: 45 dB below full scale
  (about 20 dB quieter than normal speech) over steady pink room noise 10 dB
  below that.

Writes clips.json next to the copies, one entry per copy, with "voice" set to
<voice>-w or <voice>-wq so the scorers can split them.
"""

import json
import sys
import wave
from pathlib import Path

import numpy as np
from scipy.linalg import solve_toeplitz
from scipy.signal import butter, lfilter, sosfilt

RATE = 16000
FRAME, HOP, ORDER = 400, 160, 16
QUIET_DBFS, NOISE_BELOW_DB = -45.0, 10.0
rng = np.random.default_rng(7)


def read(path):
    with wave.open(str(path)) as w:
        assert w.getframerate() == RATE and w.getnchannels() == 1 and w.getsampwidth() == 2
        return np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float64) / 32768


def write(path, x):
    x = np.clip(x, -1, 1)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes((x * 32767).astype("<i2").tobytes())


def rms(x):
    return np.sqrt(np.mean(x ** 2) + 1e-20)


def whisper(x):
    pre = lfilter([1, -0.97], [1], x)
    win = np.hanning(FRAME)
    out = np.zeros(len(x) + FRAME)
    norm = np.zeros(len(x) + FRAME)
    for start in range(0, max(1, len(x) - FRAME), HOP):
        seg = pre[start:start + FRAME] * win
        if len(seg) < FRAME:
            break
        r = np.correlate(seg, seg, "full")[FRAME - 1:FRAME + ORDER]
        if r[0] < 1e-9:
            continue
        r[0] *= 1.0001
        a = solve_toeplitz(r[:ORDER], r[1:ORDER + 1])
        lpc = np.concatenate(([1.0], -a))
        gain = np.sqrt(max(r[0] - np.dot(a, r[1:ORDER + 1]), 1e-12) / FRAME)
        y = lfilter([gain], lpc, rng.standard_normal(FRAME)) * win
        out[start:start + FRAME] += y
        norm[start:start + FRAME] += win ** 2
    y = out[:len(x)] / np.maximum(norm[:len(x)], 1e-3)
    y = lfilter([1], [1, -0.97], y)
    y = sosfilt(butter(4, 250, "highpass", fs=RATE, output="sos"), y)
    return y * (rms(x) / rms(y))


def pink(n):
    white = rng.standard_normal(n)
    return lfilter([0.049922035, -0.095993537, 0.050612699, -0.004408786],
                   [1, -2.494956002, 2.017265875, -0.522189400], white)


def quiet_in_room(w):
    speech = np.abs(w) > 0.02 * np.max(np.abs(w))
    level = rms(w[speech]) if speech.any() else rms(w)
    w = w * 10 ** (QUIET_DBFS / 20) / level
    noise = pink(len(w))
    noise *= 10 ** ((QUIET_DBFS - NOISE_BELOW_DB) / 20) / rms(noise)
    return w + noise


def main(clips_path, out_dir):
    clips_path, out_dir = Path(clips_path), Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    listed = []
    for c in json.loads(clips_path.read_text()):
        x = read(clips_path.parent / c["clip"])
        w = whisper(x)
        stem = c["clip"][:-4]
        for suffix, y in (("w", w), ("wq", quiet_in_room(w))):
            name = f"{stem}_{suffix}.wav"
            write(out_dir / name, y)
            listed.append(dict(c, clip=name, voice=f"{c['voice']}-{suffix}"))
    (out_dir / "clips.json").write_text(json.dumps(listed, indent=1))
    print(len(listed), "clips in", out_dir)


if __name__ == "__main__":
    main(*sys.argv[1:3])
