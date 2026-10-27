"""Text normalisation and edit distance shared by export.py and eval.py."""

import re
import unicodedata

_PUNCT = re.compile(r"[^\w\s']", re.UNICODE)
_SPACE = re.compile(r"\s+")
# Devanagari, Gurmukhi, Gujarati, Bengali, Tamil, Telugu, Kannada, Malayalam, Oriya.
_INDIC = re.compile(r"[ऀ-ൿ]")


def normalize(text: str) -> str:
    """Lowercase, drop punctuation, collapse spaces. Used only for scoring and filters."""
    text = unicodedata.normalize("NFKC", text).lower()
    text = text.replace("’", "'")
    text = _PUNCT.sub(" ", text)
    return _SPACE.sub(" ", text).strip()


def has_indic_script(text: str) -> bool:
    return bool(_INDIC.search(text))


def edit_distance(ref: list, hyp: list) -> int:
    prev = list(range(len(hyp) + 1))
    for i, r in enumerate(ref, 1):
        cur = [i] + [0] * len(hyp)
        for j, h in enumerate(hyp, 1):
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (r != h))
        prev = cur
    return prev[-1]


def word_change_ratio(a: str, b: str) -> float:
    """Word edits needed to turn a into b, divided by the longer length."""
    wa, wb = normalize(a).split(), normalize(b).split()
    longest = max(len(wa), len(wb))
    return edit_distance(wa, wb) / longest if longest else 0.0


_UNITS = {w: i for i, w in enumerate(
    "zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen "
    "fifteen sixteen seventeen eighteen nineteen".split())}
_TENS = {w: 10 * i for i, w in enumerate("_ _ twenty thirty forty fifty sixty seventy eighty ninety".split()) if i > 1}
_SCALES = {"thousand": 1_000, "million": 1_000_000, "billion": 1_000_000_000, "lakh": 100_000, "crore": 10_000_000}
_DIGIT_COMMA = re.compile(r"(?<=\d),(?=\d{3}\b)")
_DIGIT_POINT = re.compile(r"(?<=\d)\.(?=\d)")
_CURRENCY = re.compile(r"([$₹])\s?(\d[\d.]*)")


def _number_runs(words: list) -> list:
    """Replace runs of spoken number words with digits ("twenty five" -> "25").

    A unit after a unit starts a new number, so "one two three" stays three numbers.
    """
    out, cur, total, last = [], None, 0, None

    def flush():
        nonlocal cur, total, last
        if cur is not None or last is not None:
            out.append(str(total + (cur or 0)))
        cur, total, last = None, 0, None

    i = 0
    while i < len(words):
        w = words[i]
        nxt = words[i + 1] if i + 1 < len(words) else None
        if w in _UNITS or w in _TENS:
            v = _UNITS.get(w, _TENS.get(w))
            joins = last is None or last in ("hundred", "scale") or (last == "tens" and w in _UNITS and v < 10)
            if not joins:
                flush()
            cur = (cur or 0) + v
            last = "tens" if w in _TENS else "unit"
        elif w == "hundred" and (last is not None or (out and out[-1] == "a")):
            if last is None:
                out.pop()
            cur = max(cur or 0, 1) * 100
            last = "hundred"
        elif w in _SCALES and (last is not None or (out and out[-1] == "a")):
            if last is None:
                out.pop()
            total += max(cur or 0, 1) * _SCALES[w]
            cur, last = 0, "scale"
        elif w == "and" and last in ("hundred", "scale") and nxt in _UNITS.keys() | _TENS.keys():
            pass
        elif w == "point" and last is not None and nxt in _UNITS and _UNITS[nxt] < 10:
            whole = total + (cur or 0)
            frac = []
            while i + 1 < len(words) and words[i + 1] in _UNITS and _UNITS[words[i + 1]] < 10:
                i += 1
                frac.append(str(_UNITS[words[i]]))
            out.append(f"{whole}.{''.join(frac)}")
            cur, total, last = None, 0, None
        else:
            flush()
            out.append(w)
        i += 1
    flush()
    return out


def normalize_wer(text: str) -> str:
    """normalize(), plus numbers written as digits on both sides ("twenty-five", "25" -> "25").

    Used by the speech benchmark; "%" is read as "percent", "$5" as "5 dollars", "1,000" as "1000", "2.5" stays "2.5".
    """
    text = unicodedata.normalize("NFKC", text)
    text = _DIGIT_COMMA.sub("", text).replace("%", " percent")
    text = _CURRENCY.sub(lambda m: f"{m[2]} {'dollars' if m[1] == '$' else 'rupees'}", text)
    text = _DIGIT_POINT.sub("DECIMALPOINT", text)
    words = normalize(text).replace("decimalpoint", ".").split()
    return " ".join(_number_runs(words))


def ready_to_send(hyp: str, ref: str) -> bool:
    """Exact match, case and punctuation included; only a missing or extra final period is forgiven."""
    def trim(s):
        s = s.strip()
        return s[:-1].rstrip() if s.endswith(".") and not s.endswith("..") else s
    return trim(hyp) == trim(ref)
