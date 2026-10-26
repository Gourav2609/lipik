"""Python mirror of Lipik's clean-up code, for the benchmark.

    fillers(text)                        the transcriber's English filler step
                                         (src-tauri/src/audio_toolkit/text.rs)
    apply(text, vocab)                   the rules step (src-tauri/src/textflow/rules.rs)
    check(transcript, reply, vocab)      None, or why the reply is rejected (rules.rs)
    relevant_vocabulary(vocab, text)     dictionary words sent to the model (pipeline.rs)
    accept(transcript, reply, rules_text, vocab)
                                         what is pasted for a model reply (pipeline.rs)
    clean_up(raw, reply, vocab)          the whole path: (text to paste, why)

Word lists and limits are read from the Rust files, and both sides run the
examples in src-tauri/src/textflow/rules_examples.json:

    python3 guards.py
"""

import json
import re
import unicodedata
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
TEXTFLOW = REPO / "src-tauri/src/textflow"
RULES_RS = (TEXTFLOW / "rules.rs").read_text()
PIPELINE_RS = (TEXTFLOW / "pipeline.rs").read_text()
HINDI_RS = (TEXTFLOW / "hindi.rs").read_text()
FILLERS_RS = (REPO / "src-tauri/src/audio_toolkit/text.rs").read_text()
EXAMPLES = json.loads((TEXTFLOW / "rules_examples.json").read_text())
PROMPT_FILE = TEXTFLOW / "cleanup_prompt.txt"


def _strings(src, name):
    body = re.search(rf"const {name}: [^=]+= &?\[(.*?)\];", src, re.S)[1]
    return re.findall(r'"([^"]*)"', body)


def _pairs(src, name):
    body = re.search(rf"const {name}: [^=]+= &\[(.*?)\];", src, re.S)[1]
    return {w: int(v.replace("_", "")) for w, v in re.findall(r'\("([^"]+)", ([\d_]+)\)', body)}


def _number(src, name):
    return float(re.search(rf"const {name}: \w+ = ([\d._]+);", src)[1].replace("_", ""))


UNITS = _strings(RULES_RS, "UNITS")
TENS = _strings(RULES_RS, "TENS")
ORDINAL_UNITS = _strings(RULES_RS, "ORDINAL_UNITS")
ORDINAL_TENS = _strings(RULES_RS, "ORDINAL_TENS")
HINDI_NUMBERS = _pairs(RULES_RS, "HINDI_NUMBERS")
SCALES = _pairs(RULES_RS, "SCALES")
FREE_WORDS = set(_strings(RULES_RS, "FREE_WORDS"))
TLDS = set(_strings(RULES_RS, "TLDS"))
WORD_TLDS = set(_strings(RULES_RS, "WORD_TLDS"))
DROPPABLE = set(_strings(RULES_RS, "DROPPABLE"))
NOT_A_USER = set(_strings(RULES_RS, "NOT_A_USER"))
DETERMINERS = set(_strings(RULES_RS, "DETERMINERS"))
COLON_NOUNS = set(_strings(RULES_RS, "COLON_NOUNS"))
SUBJECT_PRONOUNS = set(_strings(RULES_RS, "SUBJECT_PRONOUNS"))
SCRIPTS = [s.upper() for s in _strings(RULES_RS, "SCRIPTS")]
MAX_ADDED_WORDS = int(_number(RULES_RS, "MAX_ADDED_WORDS"))
MAX_DROPPED_WORDS = int(_number(RULES_RS, "MAX_DROPPED_WORDS"))
CUE_MAX_DROPPED_SHARE = _number(RULES_RS, "CUE_MAX_DROPPED_SHARE")
CUE_MIN_DROPPED = int(_number(RULES_RS, "CUE_MIN_DROPPED"))
TOO_LONG_TIMES = _number(RULES_RS, "TOO_LONG_TIMES")
TOO_LONG_EXTRA = int(_number(RULES_RS, "TOO_LONG_EXTRA"))
MAX_JOINED_NUMBERS = int(_number(RULES_RS, "MAX_JOINED_NUMBERS"))
QUESTION_AUX = re.search(r'const QUESTION_AUX: &str = "([^"]+)";', RULES_RS)[1]
MIN_MODEL_WORDS = int(_number(PIPELINE_RS, "MIN_MODEL_WORDS"))
MAX_VOCABULARY = int(_number(PIPELINE_RS, "MAX_VOCABULARY"))
HINDI_WORDS = set(_strings(HINDI_RS, "WORDS")) - set(_strings(HINDI_RS, "ALSO_ENGLISH"))
UNIVERSAL_FILLERS = _strings(FILLERS_RS, "UNIVERSAL_FILLER_WORDS")
ENGLISH_FILLERS = re.findall(r'"([^"]*)"', re.search(r'"en" => &\[(.*?)\]', FILLERS_RS)[1])
FILLER_WORDS = set(UNIVERSAL_FILLERS) | set(ENGLISH_FILLERS)
COMMON_WORDS = {line for line in (TEXTFLOW / "common_words.txt").read_text().splitlines() if line}


# ---------- the transcriber's English filler step ----------

def _is_acronym(token):
    letters = [c for c in token if c.isalpha()]
    return len(letters) >= 2 and all(c.isupper() for c in letters)


def _opens_sentence(kept):
    t = kept.rstrip()
    return not t or t[-1] in ".!?…"


def _remove_filler_matches(text, pattern, keep_acronyms):
    kept, resume, owed = "", 0, False

    def push(segment):
        nonlocal kept, owed
        if owed:
            for i, c in enumerate(segment):
                if c.isalnum():
                    owed = False
                    kept += segment[:i] + c.upper() + segment[i + 1:]
                    return
        kept += segment

    for m in pattern.finditer(text):
        if keep_acronyms and _is_acronym(m[0]):
            continue
        push(text[resume:m.start()])
        owed = owed or (m[0][:1].isupper() and _opens_sentence(kept))
        resume = m.end()
    push(text[resume:])
    return kept


def _normalize(text):
    out, ws, i = [], text.split(), 0
    while i < len(ws):
        n = 1
        if ws[i].isalpha():
            while i + n < len(ws) and ws[i + n].lower() == ws[i].lower():
                n += 1
        out.append(ws[i])
        i += n if n >= 3 else 1
    return re.sub(r"\s{2,}", " ", " ".join(out)).strip()


def fillers(text):
    """remove_filler_words with English evidence, then normalize_transcription_output."""
    for words_, keep in ((UNIVERSAL_FILLERS, False), (ENGLISH_FILLERS, True)):
        for w in words_:
            text = _remove_filler_matches(text, re.compile(rf"(?i)\b{re.escape(w)}\b[,.]?"), keep)
    return _normalize(text)


# ---------- words and numbers ----------

def _cardinal(w):
    if w in UNITS:
        return UNITS.index(w)
    if w in TENS:
        return (TENS.index(w) + 2) * 10
    return None


def _ordinal(w):
    if w in ORDINAL_UNITS:
        return ORDINAL_UNITS.index(w) + 1
    if w in ORDINAL_TENS:
        return (ORDINAL_TENS.index(w) + 2) * 10
    return None


def number_word(w):
    for v in (_cardinal(w), 0 if w == "oh" else None, _ordinal(w), HINDI_NUMBERS.get(w)):
        if v is not None:
            return v
    return None


def _is_word_char(c):
    return c.isalnum() or c == "_"


def _is_token_char(c):
    return c == "'" or unicodedata.category(c)[0] in "LMN"


def _is_digits(s):
    return s != "" and all("0" <= c <= "9" for c in s)


def _no_leading_zeros(s):
    return s.lstrip("0") or "0"


def words(text):
    chars = text.lower().replace("’", "'")
    joined = []
    for i, c in enumerate(chars):
        follow = chars[i + 1:i + 4]
        after = chars[i + 4:i + 5]
        if (c == "," and i > 0 and _is_digits(chars[i - 1]) and len(follow) == 3 and _is_digits(follow)
                and not (after and _is_word_char(after))):
            continue
        joined.append(c)
    out, cur = [], ""
    for c in joined + [" "]:
        if _is_token_char(c):
            cur += c
        else:
            w = cur.strip("'")
            if w:
                out.append(w)
            cur = ""
    return out


def spoken_numbers(tokens):
    out, cur, total, active = [], 0, 0, False

    def flush():
        nonlocal cur, total, active
        if active:
            out.append(str(total + cur))
        cur, total, active = 0, 0, False

    for t in tokens:
        v = number_word(t)
        if _is_digits(t):
            flush()
            out.append(_no_leading_zeros(t))
        elif v is not None:
            if not active:
                cur, active = v, True
            elif cur >= 100 and cur % 100 == 0:
                cur += v
            elif 20 <= cur < 100 and cur % 10 == 0 and v < 10:
                cur += v
            elif cur == 0 and total:
                cur = v
            else:
                flush()
                cur, active = v, True
        elif t in SCALES and active:
            if SCALES[t] == 100:
                cur = (cur or 1) * 100
            else:
                total += (cur or 1) * SCALES[t]
                cur = 0
        elif t == "and" and active:
            continue
        else:
            flush()
    flush()
    return out


def number_ok(token, said):
    if token.lstrip("0") == "" and "0" in said:
        return True
    t = _no_leading_zeros(token)
    if t in said:
        return True
    for i in range(len(said)):
        joined = ""
        for s in said[i:i + MAX_JOINED_NUMBERS]:
            joined += s
            if joined in (token, t):
                return True
            if len(joined) >= len(token):
                break
    return False


# ---------- checks ----------

def added_words(transcript, reply, vocab=()):
    t_words = words(transcript)
    allowed = set(t_words) | FREE_WORDS
    for entry in vocab:
        allowed |= set(words(entry))
    for n in range(2, 5):
        for i in range(len(t_words) - n + 1):
            allowed.add("".join(t_words[i:i + n]))
    said = spoken_numbers(t_words)
    for m in re.finditer(r"[0-9]+(?:[:.,-][0-9]+)+", reply):
        if number_ok(re.sub(r"[^0-9]", "", m[0]), said):
            allowed |= set(re.findall(r"[0-9]+", m[0]))
    added = []
    for w in words(reply):
        if w in allowed or (_is_digits(w) and number_ok(w, said)) or w in added:
            continue
        added.append(w)
    return added


def dropped_words(transcript, reply):
    reply_words = words(reply)
    joined = "".join(reply_words)
    reply_words = set(reply_words)
    missing = []
    for w in words(transcript):
        if (w in reply_words or w in DROPPABLE or w in FILLER_WORDS or number_word(w) is not None
                or _is_digits(w) or w in missing):
            continue
        if len(w) <= 2 and w in joined:
            continue
        missing.append(w)
    return missing


def hinglish_lost(transcript, reply):
    before = set(words(transcript)) & HINDI_WORDS
    return len(before) >= 2 and len(before & set(words(reply))) * 2 < len(before)


def _scripts(text):
    found = set()
    for c in text:
        try:
            first = unicodedata.name(c).split()[0]
        except ValueError:
            continue
        found.add("HAN" if first == "CJK" else first)
    return found & set(SCRIPTS)


def new_script(transcript, reply):
    return bool(_scripts(reply) - _scripts(transcript))


CORRECTION_CUES = re.compile(r"(?i)\b(no|actually|i mean|scratch that|sorry|wait|nahi|meant)\b")


def check(transcript, reply, vocab=()):
    """None when the reply may be pasted, else the reason (rules::Why)."""
    if len(reply) > len(transcript) * TOO_LONG_TIMES + TOO_LONG_EXTRA:
        return "TooLong"
    if new_script(transcript, reply):
        return "NewScript"
    if hinglish_lost(transcript, reply):
        return "HinglishLost"
    added = added_words(transcript, reply, vocab)
    if len(added) > MAX_ADDED_WORDS:
        return "Added: " + " ".join(added)
    dropped = dropped_words(transcript, reply)
    if CORRECTION_CUES.search(transcript):
        limit = max(len(words(transcript)) * CUE_MAX_DROPPED_SHARE, CUE_MIN_DROPPED)
    else:
        limit = MAX_DROPPED_WORDS
    if len(dropped) > limit:
        return "Dropped: " + " ".join(dropped)
    return None


# ---------- rules ----------

def _replace_checked(pattern, text, ok, rep):
    out, copied, pos = [], 0, 0
    while pos <= len(text):
        m = pattern.search(text, pos)
        if not m:
            break
        if ok(m):
            out.append(text[copied:m.start()])
            out.append(rep(m))
            copied = m.end()
            pos = m.end() if m.end() > m.start() else m.end() + 1
        else:
            pos = m.start() + 1
    out.append(text[copied:])
    return "".join(out)


def _word_before(text, i):
    before = text[:i].rstrip()
    j = len(before)
    while j > 0 and _is_word_char(before[j - 1]):
        j -= 1
    return before[j:].lower()


def _word_after(text, i):
    after = text[i:].lstrip()
    j = 0
    while j < len(after) and _is_word_char(after[j]):
        j += 1
    return after[:j].lower()


def _after_determiner(text, i):
    return _word_before(text, i) in DETERMINERS


SPOKEN_MARKS = [(re.compile(rf"(?i)[\s,.;:!?]*\b(?:{said})\b[,.;:!?]*"), mark) for said, mark in [
    ("new paragraph", "\n\n"), ("new line|newline", "\n"), ("question mark", "?"),
    ("exclamation (?:mark|point)", "!"), ("full stop", "."), ("comma", ","), ("colon", ":")]]
SPOKEN_PERIOD = re.compile(r"(?i)[\s,]*\bperiod\b\.?")


def _spoken_punctuation(text):
    for pattern, mark in SPOKEN_MARKS:
        def ok(m, text=text, mark=mark):
            organ = mark == ":" and _word_after(text, m.end()) in COLON_NOUNS
            return not _after_determiner(text, m.start()) and not organ
        text = _replace_checked(pattern, text, ok, lambda m, mark=mark: mark)

    def period_ok(m):
        rest = text[m.end():]
        spaces = rest[:len(rest) - len(rest.lstrip())]
        return not _after_determiner(text, m.start()) and ("\n" in spaces or len(spaces) == len(rest))
    return _replace_checked(SPOKEN_PERIOD, text, period_ok, lambda m: ".")


ADDRESS = re.compile(r"(?i)[a-z0-9][\w.-]*(?:\s+(?:dot|underscore|dash|at|slash)\s+[a-z0-9][\w.-]*|/[\w-]+)+")
SYMBOLS = {"dot": ".", "underscore": "_", "dash": "-", "at": "@", "slash": "/"}


def _join_address(span):
    pieces = re.split(r"(?i)\s+(dot|underscore|dash|at|slash)\s+", span)
    first = pieces[0].lower()
    if first in DETERMINERS:
        return None
    if len(pieces) >= 3 and pieces[1].lower() == "at" and first in NOT_A_USER:
        rest = _join_address(" ".join(pieces[2:]))
        return None if rest is None else f"{pieces[0]} at {rest}"
    out = pieces[0]
    for i in range(1, len(pieces), 2):
        out += SYMBOLS[pieces[i].lower()] + pieces[i + 1]
    out = out.rstrip(".").lower()
    trail = span[len(span.rstrip(".")):]
    email, path = "@" in out, "/" in out
    if out.count("@") > 1 or (email and path):
        return None
    host = out.split("@")[-1].split("/")[0]
    if "." not in host:
        return None
    name, tld = host.rsplit(".", 1)
    if tld not in TLDS:
        return None
    if tld in WORD_TLDS and not email and not path and "." not in name:
        return None
    return out + trail


def _emails_and_urls(text):
    def ok(m):
        before = text[m.start() - 1] if m.start() else ""
        free = not before or not (_is_word_char(before) or before in ".@/")
        return free and not _after_determiner(text, m.start())
    return _replace_checked(ADDRESS, text, ok, lambda m: _join_address(m[0]) or m[0])


_NUM = r"(?:\d+(?:[.:]\d+)?|" + "|".join(sorted(UNITS + TENS, key=len, reverse=True)) + r")"
NUMBER_CORRECTION = re.compile(rf"(?i)\b{_NUM}[,.]?\s+no[,.]?\s+(?:sorry[,.]?\s+|wait[,.]?\s+)?{_NUM}\b")


def _number_corrections(text):
    def kept(m):
        return re.split(r"\s", m[0])[-1]

    def rep(m):
        v = _cardinal(kept(m).lower())
        return kept(m) if v is None else str(v)
    return _replace_checked(NUMBER_CORRECTION, text, lambda m: kept(m).lower() != "one", rep)


def _times(text):
    t = re.sub(r"(?i)\b(\d{1,2})[.:](\d{2})\s*([ap])\.?\s?m\b\.?",
               lambda m: f"{m[1]}:{m[2]} {m[3].upper()}M", text)
    return re.sub(r"(?i)\b(\d{1,2})\s*([ap])\.\s?m\.", lambda m: f"{m[1]} {m[2].upper()}M", t)


def _is_common_word(word):
    parts = [p for p in re.split(r"[-/]", word.lower().replace("’", "'")) if p]
    return bool(parts) and all(p in COMMON_WORDS for p in parts)


def _vocab_case(text, vocab):
    for entry in vocab:
        if not entry or entry.lower() not in text.lower():
            continue
        ordinary = all(_is_common_word(w) for w in entry.split())

        def ok(m, text=text, ordinary=ordinary):
            before = text[m.start() - 1] if m.start() else ""
            free_before = not before or not (_is_word_char(before) or before in ".@/")
            rest = text[m.end():m.end() + 2]
            if not rest:
                free_after = True
            elif _is_word_char(rest[0]) or rest[0] in "@/":
                free_after = False
            elif rest[0] == ".":
                free_after = not (len(rest) > 1 and _is_word_char(rest[1]))
            else:
                free_after = True
            lowercase = not any(c.isupper() for c in m[0])
            return free_before and free_after and not (ordinary and lowercase)
        text = _replace_checked(re.compile(re.escape(entry), re.I), text, ok, lambda m, e=entry: e)
    return text


SENTENCE_GOES_ON = re.compile(r"[.?!]\s+\S")
OPENER = re.compile(r"(?i)^((hey|hi|hello|okay|ok|so)\b[,]?\s+([A-Z][a-z]+,\s+)?)+")
QUESTION_START = re.compile(
    r"(?i)^((what's|where's|how's|who's|when's|why's|is|are|isn't|aren't)\b|"
    rf"(what|when|where|who|why|how|which)\s+{QUESTION_AUX}\b|"
    r"(can|could|would|will|should|shall|do|does|did|have|has)(n't)?\s+(i|you|we|they|he|she|someone|anyone)\b)")
QUESTION_WORD_MIDDLE = re.compile(rf"(?i)^(what|when|where|who|why|how|which)\s+(\S+)\s+{QUESTION_AUX}\b")


def _starts_like_question(text):
    if QUESTION_START.match(text):
        return True
    m = QUESTION_WORD_MIDDLE.match(text)
    return bool(m) and m[2].lower() not in SUBJECT_PRONOUNS


def question_end(text):
    if ("\n" in text or SENTENCE_GOES_ON.search(text) or text.endswith(("?", "…", ".."))
            or not _starts_like_question(OPENER.sub("", text, count=1))):
        return text
    return (text[:-1] if text.endswith((".", "!")) else text) + "?"


MARK_BEFORE_LETTER = re.compile(r"[,?!][A-Za-z]")
WEB_ADDRESS_PART = re.compile(r"[/@]|\.[A-Za-z]{2,}")
SENTENCE_START = re.compile(r"(?:^|\n|[.?!]\s)[a-z]")


def _space_after_marks(text):
    def ok(m):
        token = re.split(r"\s", text[:m.start()])[-1]
        return not WEB_ADDRESS_PART.search(token)
    return _replace_checked(MARK_BEFORE_LETTER, text, ok, lambda m: m[0][0] + " " + m[0][1])


def _collapse_marks(text):
    def rep(m):
        run = m[0]
        return "..." if len(run) >= 3 and set(run) == {"."} else run[0]
    return re.sub(r"[.?!]+", rep, text)


def _capitalize_sentences(text):
    def ok(m):
        return not (m[0].startswith(".") and m.start() > 0 and text[m.start() - 1] == ".")
    return _replace_checked(SENTENCE_START, text, ok, lambda m: m[0][:-1] + m[0][-1].upper())


def _lowercase_leading_address(text):
    if text[:1].isascii() and text[:1].isupper():
        rest = text[1:]
        j = 0
        while j < len(rest) and (_is_word_char(rest[j]) or rest[j] == "."):
            j += 1
        if rest[j:].startswith(("@", "/")):
            return text[0].lower() + rest
    return text


def apply(text, vocab=()):
    t = _spoken_punctuation(text.strip())
    t = _number_corrections(t)
    t = _emails_and_urls(t)
    t = _times(t)
    t = _vocab_case(t, vocab)
    t = re.sub(r"[ \t]+([,.?!:])", r"\1", t)
    t = _space_after_marks(t)
    t = _collapse_marks(t)
    t = re.sub(r",([.?!])", r"\1", t)
    t = re.sub(r"[ \t]*\n[ \t]*", "\n", t)
    t = re.sub(r"[ \t]{2,}", " ", t)
    t = t.strip(" \t\n\r,")
    return _lowercase_leading_address(question_end(_capitalize_sentences(t)))


# ---------- what is pasted (pipeline.rs) ----------

def _phrase_words(text):
    out, cur = [], ""
    s = text.replace("’", "'")
    for i, c in enumerate(s + " "):
        inner = c == "'" and 0 < i < len(s) - 1 and s[i - 1].isalnum() and s[i + 1].isalnum()
        if c.isalnum() or inner:
            cur += c
        elif cur:
            out.append(cur.lower())
            cur = ""
    return out


def _levenshtein(a, b):
    row = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        prev, row[0] = row[0], i
        for j, cb in enumerate(b, 1):
            prev, row[j] = row[j], min(row[j] + 1, row[j - 1] + 1, prev + (ca != cb))
    return row[-1]


def _sounds_like(word, heard):
    return word == heard or (len(word) >= 4 and len(heard) >= 4
                             and (word in heard or _levenshtein(word, heard) * 3 <= len(word)))


def relevant_vocabulary(vocab, text):
    said = _phrase_words(text)
    heard = set(said)
    for n in (2, 3):
        heard |= {"".join(said[i:i + n]) for i in range(len(said) - n + 1)}
    out = [e for e in vocab if any(_sounds_like(w, h) for w in _phrase_words(e) for h in heard)]
    return out[:MAX_VOCABULARY]


def strip_wrapping(out):
    out = out.strip()
    if out.startswith("<transcript>") and out.endswith("</transcript>"):
        out = out[len("<transcript>"):-len("</transcript>")].strip()
    if len(out) >= 2 and out.startswith('"') and out.endswith('"'):
        return out[1:-1]
    return out


def accept(transcript, reply, rules_text, vocab=()):
    """(text to paste, why) for a model reply."""
    reply = strip_wrapping(reply)
    if not reply:
        return rules_text, "rejected: empty"
    why = check(transcript, reply, vocab)
    if why:
        return rules_text, "rejected: " + why
    return (reply if reply.endswith((".", "!", "?")) else question_end(reply)), "cleaned"


def clean_up(raw, reply, vocab=(), min_words=MIN_MODEL_WORDS):
    """The app's clean-up of a raw transcript, given the model's reply (None when the call
    failed or timed out). The filler step runs first, as in the transcriber."""
    transcript = fillers(raw)
    if not any(c.isalnum() for c in transcript):
        return transcript, "no words"
    rules_text = apply(transcript, vocab)
    if len(transcript.split()) < min_words:
        return rules_text, "short: rules"
    if reply is None:
        return rules_text, "no reply: rules"
    return accept(transcript, reply, rules_text, relevant_vocabulary(vocab, transcript))


def model_input(raw, vocab=()):
    """What the app sends the model for a raw transcript."""
    return apply(fillers(raw), vocab)


# ---------- the shared examples ----------

def _test():
    vocab = EXAMPLES["vocab"]
    bad, n = 0, 0

    def expect(name, got, want, case):
        nonlocal bad, n
        n += 1
        if got != want:
            bad += 1
            print(f"{name} {case!r} -> {got!r}, want {want!r}")

    for text, want in EXAMPLES["words"]:
        expect("words", words(text), want, text)
    for t, r, want in EXAMPLES["added"]:
        expect("added_words", added_words(t, r, vocab), want, t)
    for t, r, want in EXAMPLES["hinglish"]:
        expect("hinglish_lost", hinglish_lost(t, r), want, r)
    for t, r, want in EXAMPLES["dropped"]:
        expect("dropped_words", dropped_words(t, r), want, t)
    for t, r, want in EXAMPLES["check"]:
        expect("check", (check(t, r, vocab) or "ok").split(":")[0], want, (t, r))
    for t, want in EXAMPLES["apply"]:
        expect("apply", apply(t, vocab), want, t)
    for t, want in EXAMPLES["fillers"]:
        expect("fillers + apply", apply(fillers(t), vocab), want, t)
    for t, r, want in EXAMPLES["paste"]:
        expect("accept", accept(t, r, apply(t, vocab), vocab)[0], want, (t, r))
    print(f"{n - bad}/{n} examples pass")
    return bad == 0


if __name__ == "__main__":
    raise SystemExit(0 if _test() else 1)
