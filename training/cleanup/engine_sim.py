"""Hinglish week: which engine should the auto path send non-English dictation to?
Replays the app's choice (apple_speech/pick.rs) on each clip from Parakeet's own
transcript, then takes the clip's result from the chosen engine's run:

    python3 engine_sim.py <parakeet run> <apple run> [<other engine run> ...]

A run is a cleanup-<tag>.jsonl (hinglish_score.py strict scoring, clean-up on and
off). The first other engine stands in for Apple: Apple's text is replaced by that
engine's, both in the "much longer" check and in the result.
"""

import json
import re
import sys
import wave
from pathlib import Path

import hinglish_score as hs

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
PICK = (REPO / "src-tauri/src/apple_speech/pick.rs").read_text()
HINDI_RS = (REPO / "src-tauri/src/textflow/hindi.rs").read_text()
COMMON = {l.strip() for l in (REPO / "src-tauri/src/textflow/common_words.txt").read_text().splitlines()
          if l.strip() and not l.startswith("#")}
VOCAB = {w.lower() for e in json.loads((HERE / "vocab.json").read_text()) for w in e.split()}


def rs_list(name):
    return set(re.findall(r'"([^"]+)"', re.search(rf"pub const {name}: &\[&str\] = &\[(.*?)\];", HINDI_RS, re.S)[1]))


def rs_num(name):
    return float(re.search(rf"const {name}: \w+ = ([\d.]+);", PICK)[1])


HINDI, ALSO_ENGLISH = rs_list("WORDS"), rs_list("ALSO_ENGLISH")
MIN_SHARE, MIN_RATE, MIN_SECS = rs_num("MIN_ENGLISH_SHARE"), rs_num("MIN_WORDS_PER_SECOND"), rs_num("MIN_RATE_SECONDS")
LONGER, EXTRA = rs_num("MIN_LONGER_TIMES"), rs_num("MIN_EXTRA_WORDS")


def looks_english(text):
    counted = english = shared = 0
    for raw in text.split():
        w = re.sub(r"^[^\w'’]+|[^\w'’]+$", "", raw).replace("’", "'")
        if not w or any(c.isdigit() for c in w) or (len(w) <= 5 and w.isupper()):
            continue
        low = w.lower()
        low = (low[:-2] if low.endswith("'s") else low).strip("'")
        if not low:
            continue
        if low in ALSO_ENGLISH:
            shared += 1
            if shared >= 2:
                return False
        elif low in HINDI:
            return False
        counted += 1
        if all(p in COMMON or p in VOCAB for p in low.split("-") if p) or low in VOCAB:
            english += 1
    return counted > 0 and english >= MIN_SHARE * counted


def too_short(text, secs):
    if secs < MIN_SECS:
        return False
    spoken = sum(max(1, sum(c.isdigit() for c in w)) for w in text.split())
    return spoken < MIN_RATE * secs


def much_longer(local, other):
    l, a = len(local.split()), len(other.split())
    return a >= LONGER * l and a >= l + EXTRA


def seconds(clip):
    with wave.open(str(hs_audio(clip))) as w:
        return w.getnframes() / w.getframerate()


def hs_audio(clip):
    import os
    base = Path(os.environ.get("HCLIPS", HERE / "audio" / "clips.json")).parent
    return base / clip


def main(pk_tag, other_tags):
    pk = {json.loads(l)["clip"]: json.loads(l) for l in (HERE / f"runs/cleanup-{pk_tag}.jsonl").open()}
    scores = {t: hs.strict_results(t) for t in [pk_tag, *other_tags]}
    rows = {t: {json.loads(l)["clip"]: json.loads(l) for l in (HERE / f"runs/cleanup-{t}.jsonl").open()}
            for t in other_tags}
    for t in other_tags:
        res = {"raw": 0, "best": 0}
        to_other = 0
        for clip, p in pk.items():
            other = rows[t][clip]["raw"]
            keep = looks_english(p["raw"]) and (not too_short(p["raw"], seconds(clip))
                                                or not much_longer(p["raw"], other))
            src = pk_tag if keep else t
            to_other += not keep
            for k in res:
                res[k] += bool(scores[src][clip][k]["pass"])
        n = len(pk)
        print(f"Parakeet + {t:<22} sent to it {to_other:>3}/{n}   clean-up off {res['raw']:>3}/{n}   on {res['best']:>3}/{n}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
