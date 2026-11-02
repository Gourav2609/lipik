"""Hinglish scores for runs/cleanup-<tag>.jsonl: the strict one from score.py and a
second, fairer word error rate for Roman Hinglish. See HINGLISH-WEEK.md.

    python3 hinglish_score.py <tag> ...          # table
    python3 hinglish_score.py <tag> --dump       # every clip, both scores
    python3 hinglish_score.py <tag> --by-voice   # table per voice (Parler = Rohit + Divya)
    python3 hinglish_score.py --selftest         # the spelling pairs below

Strict ("fully right", unchanged, score.py): the case's rule checks (filler gone,
correction applied, "?" on questions, repeat removed), no Devanagari, nothing the
clean-up dropped or added, and loose WER at most 30% (training/bench/score.py: vowel
length folded, w=v, ph=f, z=j, doubled letters).

Hinglish WER (hWER, new, secondary): the same loose text, then on every word that is
not a common English word (textflow/common_words.txt):
  (words in textflow/hindi.rs and main/hum/ham/me/he/pe/par/na/to are folded even
  though they are also English words)
  1. ai -> e                       hai/he, hain/hen, main/men
  2. a final h after a vowel goes  yeh/ye, woh/wo, nah/na
  3. a final n after a vowel goes  nahin/nahi, hoon/hu, chalein/chale
  4. jh -> j, chh -> ch            mujhe/muje, chhe/che
  5. consonant + ia -> ya          kia/kya
  6. Hindi number words -> digits  aath/8, chhe/6 (not ek, do: also words)
and a word in the output that is two neighbouring reference words run together is
split ("hogaya" for "ho gaya", "kardo" for "kar do"). Punctuation, capitals and
digits-vs-words are ignored, as in loose. Both sides get the same treatment.
"""

import json
import re
import statistics
import sys
from pathlib import Path

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE.parent / "bench"))
from textnorm import edit_distance  # noqa: E402
from score import loose  # noqa: E402  (training/bench/score.py)

COMMON = set((HERE.parent.parent / "src-tauri/src/textflow/common_words.txt").read_text().split())
NUMBERS = {"teen": "three", "tin": "three", "char": "four", "chaar": "four", "panch": "five", "paanch": "five",
           "chhe": "six", "che": "six", "chhah": "six", "chah": "six", "saat": "seven", "sat": "seven",
           "aath": "eight", "ath": "eight", "nau": "nine", "das": "ten"}
NUMBERS = {k: loose(v) for k, v in NUMBERS.items()}
# Hindi words that are also English spellings ("main", "hum", "ho") still get folded.
HINDI = set(re.findall(r'"([a-z]+)"', (HERE.parent.parent / "src-tauri/src/textflow/hindi.rs").read_text()))
HINDI |= {"main", "mai", "mein", "hum", "ham", "me", "he", "pe", "par", "na", "to"}
HINDI |= {loose(w) for w in HINDI}
VOWELS = "aeiou"


def fold_word(w):
    if (w in COMMON and w not in HINDI) or not w.isalpha():
        return w
    if w in NUMBERS:
        return NUMBERS[w]
    w = w.replace("ai", "e").replace("chh", "ch").replace("jh", "j")
    w = re.sub(r"(?<=[^aeiou])ia", "ya", w)
    if len(w) > 2 and w[-1] == "h" and w[-2] in VOWELS:
        w = w[:-1]
    if len(w) > 2 and w[-1] == "n" and w[-2] in VOWELS:
        w = w[:-1]
    w = re.sub(r"(.)\1+", r"\1", w)
    return NUMBERS.get(w, w)


def hnorm(text):
    return [fold_word(w) for w in loose(text).split()]


def split_joined(ref, hyp):
    pairs = {ref[i] + ref[i + 1]: (ref[i], ref[i + 1]) for i in range(len(ref) - 1)}
    out = []
    for w in hyp:
        out.extend(pairs.get(w, (w,)))
    return out


def hwer(ref_text, hyp_text):
    ref = hnorm(ref_text)
    hyp = split_joined(ref, hnorm(hyp_text))
    return edit_distance(ref, hyp), len(ref)


def load(tag):
    return [json.loads(l) for l in (HERE / "runs" / f"cleanup-{tag}.jsonl").read_text().splitlines()]


def strict_results(tag):
    import importlib.util
    spec = importlib.util.spec_from_file_location("cleanup_score", HERE / "score.py")
    cleanup_score = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(cleanup_score)
    _, results = cleanup_score.score(tag)
    return {r["clip"]: r for r in results if r["category"] == "hinglish"}


def fair_pass(r, name):
    """The strict verdict with the 30% bar on hWER instead of loose WER (informational)."""
    x = r[name]
    reasons = [w.strip() for w in x["why"].split(";") if w.strip()]
    others = [w for w in reasons if not w.startswith("unreadable Hinglish")]
    e, n = hwer(r["expected"], x["text"])
    return not others and e / n <= 0.30


def voice_group(voice):
    return "Parler" if voice in ("Rohit", "Divya") else voice


def summary(tag, voice=None):
    rows = {r["clip"]: r for r in load(tag)}
    res = strict_results(tag)
    if voice:
        res = {c: r for c, r in res.items() if voice_group(rows[c]["voice"]) == voice}
    out = {"tag": tag}
    for name in ("raw", "best"):
        e = n = le = 0
        for clip, r in res.items():
            a, b = hwer(r["expected"], r[name]["text"])
            e, n = e + a, n + b
            le += edit_distance(loose(r["expected"]).split(), loose(r[name]["text"]).split())
        out[name] = {"pass": sum(r[name]["pass"] for r in res.values()), "n": len(res),
                     "hpass": sum(fair_pass(r, name) for r in res.values()),
                     "loose_wer": 100 * le / sum(len(loose(r["expected"]).split()) for r in res.values()),
                     "hwer": 100 * e / n}
    asr = [rows[c].get("asr_ms") for c in res if rows[c].get("asr_ms") is not None]
    roman = [rows[c].get("roman_ms", 0) for c in res]
    clean = [rows[c].get("latency_ms") or 0 for c in res]
    out["asr_ms"] = statistics.median(asr) if asr else None
    out["roman_ms"] = statistics.median(roman)
    out["clean_ms"] = statistics.median(clean)
    out["audio_s"] = statistics.median(rows[c]["audio_s"] for c in res) if "audio_s" in next(iter(rows.values())) else None
    return out


def dump(tag):
    rows = {r["clip"]: r for r in load(tag)}
    for clip, r in strict_results(tag).items():
        print(f"\n{clip}  expected: {r['expected']}")
        if rows[clip].get("asr_text") and rows[clip]["asr_text"] != r["raw"]["text"]:
            print(f"  speech: {rows[clip]['asr_text']}")
        for name in ("raw", "best"):
            x = r[name]
            e, n = hwer(r["expected"], x["text"])
            print(f"  {name:4} {'PASS' if x['pass'] else 'fail'} loose {x['wer']:5.1f} hWER {100 * e / n:5.1f} | "
                  f"{x['text']}  {'' if x['pass'] else '<- ' + x['why']}")


def selftest():
    same = [("hai", "he"), ("hain", "hai"), ("nahi", "nahin"), ("theek", "thik"), ("mujhe", "muje"),
            ("kya", "kia"), ("woh", "vo"), ("yeh", "ye"), ("aaunga", "aunga"), ("chalein", "chalen"),
            ("hoon", "hu"), ("pucho", "poocho"), ("aath", "8"), ("chhe", "6"), ("main", "mai")]
    differ = [("hai", "ho"), ("kal", "kab"), ("then", "the"), ("men", "man"), ("do", "two"), ("ek", "one"),
              ("karo", "kar")]
    bad = [(a, b) for a, b in same if hnorm(a) != hnorm(b)] + [(a, b) for a, b in differ if hnorm(a) == hnorm(b)]
    assert hwer("ho gaya", "hogaya") == (0, 2), hwer("ho gaya", "hogaya")
    print("selftest:", "ok" if not bad else f"FAILED {bad}")


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        selftest()
    else:
        tags = [a for a in sys.argv[1:] if not a.startswith("--")]
        if "--dump" in sys.argv:
            for t in tags:
                dump(t)
        else:
            print("| Run | Strict, clean-up off | Strict, on | Strict with the hWER bar, off / on | loose WER off / on | hWER off / on | speech ms | Roman ms | clean-up ms |")
            print("|---|---|---|---|---|---|---|---|---|")
            for t in tags:
                voices = sorted({voice_group(r["voice"]) for r in load(t)}) if "--by-voice" in sys.argv else [None]
                for v in voices:
                    s = summary(t, v)
                    print(f"| {t}{' ' + v if v else ''} | {s['raw']['pass']}/{s['raw']['n']} | {s['best']['pass']}/{s['best']['n']} | "
                          f"{s['raw']['hpass']} / {s['best']['hpass']} | "
                          f"{s['raw']['loose_wer']:.1f} / {s['best']['loose_wer']:.1f} | "
                          f"{s['raw']['hwer']:.1f} / {s['best']['hwer']:.1f} | {s['asr_ms']} | {s['roman_ms']} | {s['clean_ms']} |")
