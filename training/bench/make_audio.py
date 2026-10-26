"""Write one 16 kHz mono WAV per (sentence, voice) with macOS `say -o`. Never plays audio.

Each clip gets 0.3 s of silence before and 0.6 s after, like a real dictation:
Parakeet drops the last word of audio that stops mid-word.
"""

import json
import subprocess
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "audio"
EN_VOICES = ["Rishi", "Aman", "Tara", "Samantha"]
HG_VOICES = ["Lekha", "Rishi", "Aman", "Tara"]


def voices_for(s):
    if s["group"] == "hinglish" or s.get("lang") == "hg":
        return HG_VOICES
    return EN_VOICES


def main():
    OUT.mkdir(exist_ok=True)
    sentences = json.loads((HERE / "sentences.json").read_text())
    clips = []
    for s in sentences:
        for voice in voices_for(s):
            wav = OUT / f"{s['id']}_{voice}.wav"
            if not wav.exists():
                aiff = wav.with_suffix(".aiff")
                subprocess.run(["say", "-v", voice, "-o", str(aiff), s.get("say", s["ref"])], check=True)
                subprocess.run(
                    ["ffmpeg", "-v", "error", "-y", "-i", str(aiff), "-af", "adelay=300,apad=pad_dur=0.6",
                     "-ar", "16000", "-ac", "1",
                     "-c:a", "pcm_s16le", str(wav)],
                    check=True,
                )
                aiff.unlink()
            clips.append({"clip": wav.name, "id": s["id"], "voice": voice})
    (OUT / "clips.json").write_text(json.dumps(clips, indent=1))
    print(len(clips), "clips")


if __name__ == "__main__":
    main()
