"""Score runs/*.jsonl against sentences.json and print markdown tables.

strict: lowercase, no punctuation, hyphens inside words dropped (stand-up =
standup), numbers spelled out in English words.
loose: strict, plus Devanagari turned into Roman letters by simple rules and
common Hinglish spelling variants folded (aa->a, ee->i, oo->u, w->v, ph->f, z->j, ei->e, doubled
letters), applied to reference and output alike.
"""

import json
import re
import sys
import unicodedata
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from textnorm import edit_distance, has_indic_script, normalize  # noqa: E402

HERE = Path(__file__).parent
ORDER = ["parakeet-en", "whisper-turbo-en", "whisper-turbo-hi", "qwen3-0.6b-en",
         "qwen3-0.6b-hi", "qwen3-0.6b-auto", "qwen3-1.7b-en", "qwen3-1.7b-hi",
         "apple-en-IN", "apple-dict-en-IN", "apple-dict-hi-IN"]

ONES = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen",
        "nineteen"]
TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]
ORDINAL = {"one": "first", "two": "second", "three": "third", "five": "fifth", "eight": "eighth",
           "nine": "ninth", "twelve": "twelfth"}


def words(n):
    if n < 20:
        return ONES[n]
    if n < 100:
        return TENS[n // 10] + ("" if n % 10 == 0 else " " + ONES[n % 10])
    if n < 1000:
        rest = n % 100
        return ONES[n // 100] + " hundred" + ("" if rest == 0 else " " + words(rest))
    if n < 1_000_000:
        rest = n % 1000
        return words(n // 1000) + " thousand" + ("" if rest == 0 else " " + words(rest))
    return " ".join(ONES[int(d)] for d in str(n))


def ordinal(n):
    w = words(n).split()
    last = w[-1]
    if last in ORDINAL:
        w[-1] = ORDINAL[last]
    elif last.endswith("y"):
        w[-1] = last[:-1] + "ieth"
    else:
        w[-1] = last + "th"
    return " ".join(w)


def spell_numbers(text):
    text = re.sub(r"(\d),(\d)", r"\1\2", text)
    text = re.sub(r"(\d{1,2}):(\d{2})", lambda m: words(int(m[1])) + ("" if m[2] == "00" else " " + words(int(m[2]))), text)
    text = re.sub(r"%", " percent", text)
    text = re.sub(r"₹\s*(\d+)", r"\1 rupees", text)
    text = re.sub(r"\b(\d+)(st|nd|rd|th)\b", lambda m: ordinal(int(m[1])), text, flags=re.I)
    text = re.sub(r"\b(\d+)\b", lambda m: words(int(m[1])), text)
    text = re.sub(r"\bp\.?\s?m\.?(?=\W|$)", "pm", text, flags=re.I)
    text = re.sub(r"\ba\.?\s?m\.?(?=\W|$)", "am", text)
    return text


def strict(text):
    text = re.sub(r"(?<=[a-zA-Z])-(?=[a-zA-Z])", "", text)
    return normalize(spell_numbers(text)).replace("'", "")


CONS = {"क": "k", "ख": "kh", "ग": "g", "घ": "gh", "ङ": "n", "च": "ch", "छ": "chh", "ज": "j",
        "झ": "jh", "ञ": "n", "ट": "t", "ठ": "th", "ड": "d", "ढ": "dh", "ण": "n", "त": "t",
        "थ": "th", "द": "d", "ध": "dh", "न": "n", "प": "p", "फ": "ph", "ब": "b", "भ": "bh",
        "म": "m", "य": "y", "र": "r", "ल": "l", "व": "v", "श": "sh", "ष": "sh", "स": "s",
        "ह": "h"}
VOWELS = {"अ": "a", "आ": "aa", "इ": "i", "ई": "ee", "उ": "u", "ऊ": "oo", "ए": "e", "ऐ": "ai",
          "ओ": "o", "औ": "au", "ऋ": "ri", "ऑ": "o"}
MATRAS = {"ा": "aa", "ि": "i", "ी": "ee", "ु": "u", "ू": "oo", "े": "e", "ै": "ai", "ो": "o",
          "ौ": "au", "ृ": "ri", "ॉ": "o"}
DIGITS = {chr(0x966 + i): str(i) for i in range(10)}


def romanize(text):
    out = []
    for word in text.split():
        w = unicodedata.normalize("NFD", "".join(DIGITS.get(ch, ch) for ch in word)).replace("\u093c", "")
        s = ""
        i = 0
        while i < len(w):
            ch = w[i]
            nxt = w[i + 1] if i + 1 < len(w) else ""
            if ch in CONS:
                s += CONS[ch]
                if nxt in MATRAS:
                    s += MATRAS[nxt]
                    i += 1
                elif nxt == "्":
                    i += 1
                elif i + 1 < len(w) and nxt not in "ंँः":
                    s += "a"
                elif nxt in "ंँः" and i + 2 < len(w):
                    s += "a"
            elif ch in VOWELS:
                s += VOWELS[ch]
            elif ch in "ंँ":
                s += "n"
            elif ch in "ःऽ।॥":
                pass
            else:
                s += ch
            i += 1
        out.append(s)
    return " ".join(out)


def loose(text):
    t = strict(romanize(text) if has_indic_script(text) else text)
    t = t.replace("aa", "a").replace("ee", "i").replace("oo", "u").replace("w", "v").replace("ph", "f").replace("z", "j").replace("ei", "e")
    t = re.sub(r"(.)\1+", r"\1", t)
    return t


def errors(ref, hyp):
    r, h = ref.split(), hyp.split()
    rc, hc = ref.replace(" ", ""), hyp.replace(" ", "")
    return edit_distance(r, h), len(r), edit_distance(list(rc), list(hc)), len(rc)


def main():
    sentences = {s["id"]: s for s in json.loads((HERE / "sentences.json").read_text())}
    voice_of = {c["clip"]: c["voice"] for c in json.loads((HERE / "audio" / "clips.json").read_text())}
    tally = defaultdict(lambda: [0] * 8)
    speed = defaultdict(lambda: [0.0, 0.0, []])
    devanagari = defaultdict(lambda: [0, 0])
    configs = [c for c in ORDER if (HERE / "runs" / f"{c}.jsonl").exists()]
    for cfg in configs:
        for line in (HERE / "runs" / f"{cfg}.jsonl").read_text().splitlines():
            r = json.loads(line)
            sid = r["clip"].split("_")[0]
            s = sentences[sid]
            voice = voice_of[r["clip"]]
            lang = "hinglish" if s["group"] == "hinglish" or s.get("lang") == "hg" else "english"
            keys = [(cfg, s["group"], "all"), (cfg, s["group"], voice), (cfg, "lang:" + lang, "all")]
            e1 = errors(strict(s["ref"]), strict(r["text"]))
            e2 = errors(loose(s["ref"]), loose(r["text"]))
            for k in keys:
                t = tally[k]
                for i, v in enumerate(e1 + e2):
                    t[i] += v
            speed[cfg][0] += r["audio_secs"]
            speed[cfg][1] += r["best_ms"] / 1000
            speed[cfg][2].append(r["load_ms"])
            devanagari[(cfg, lang)][0] += has_indic_script(r["text"])
            devanagari[(cfg, lang)][1] += 1

    def pct(a, b):
        return f"{100 * a / b:.1f}" if b else "-"

    for group in ["english", "hinglish", "numbers"]:
        print(f"\n### {group}\n")
        voices = sorted({k[2] for k in tally if k[1] == group and k[2] != "all"})
        print("| Model + language | WER | CER | loose WER | loose CER | " + " | ".join(f"WER {v}" for v in voices) + " |")
        print("|---" * (5 + len(voices)) + "|")
        for cfg in configs:
            t = tally[(cfg, group, "all")]
            row = [cfg, pct(t[0], t[1]), pct(t[2], t[3]), pct(t[4], t[5]), pct(t[6], t[7])]
            row += [pct(tally[(cfg, group, v)][0], tally[(cfg, group, v)][1]) for v in voices]
            print("| " + " | ".join(row) + " |")

    print("\n### By spoken language (all groups)\n")
    print("| Model + language | English WER | English CER | Hinglish WER | Hinglish CER | Hinglish loose WER | Hinglish loose CER | Hinglish outputs in Devanagari |")
    print("|---" * 8 + "|")
    for cfg in configs:
        e, h = tally[(cfg, "lang:english", "all")], tally[(cfg, "lang:hinglish", "all")]
        d = devanagari[(cfg, "hinglish")]
        print(f"| {cfg} | {pct(e[0], e[1])} | {pct(e[2], e[3])} | {pct(h[0], h[1])} | {pct(h[2], h[3])} | "
              f"{pct(h[4], h[5])} | {pct(h[6], h[7])} | {d[0]}/{d[1]} |")

    print("\n### Speed (warm, best of 3 runs per clip, Metal)\n")
    print("| Model + language | Audio | Decode time | x real time | Median load |")
    print("|---|---|---|---|---|")
    for cfg in configs:
        a, d, loads = speed[cfg]
        loads.sort()
        print(f"| {cfg} | {a:.0f} s | {d:.1f} s | {a / d:.0f}x | {loads[len(loads) // 2]} ms |")


if __name__ == "__main__":
    main()
