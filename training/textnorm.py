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
