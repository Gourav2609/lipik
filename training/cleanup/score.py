"""Score raw transcripts and clean-up output against cases.json.

    python3 score.py [tag ...]          # default: every runs/cleanup-*.jsonl
    python3 score.py qwen3-8b-app --dump # also print every clip with its verdicts
    python3 score.py qwen3-8b-v7p --fails # only the clips the new setup ("best") fails
    python3 score.py qwen3-8b-app qwen3-8b-v7p --compare  # raw / Lipik today / new setup

Outputs scored per clip: raw, app (8-word floor, raw on short clips, as Lipik did
before the rules step), model (model output on every clip), rules (filler and rules
steps alone), best (guards.clean_up, what the app pastes now: rules under 8 words,
else the model's reply if it passes the checks, else rules), best_all (clean_up with
no word floor).

WER: lowercase, punctuation stripped, numbers spelled out in words on both
sides ($, ₹ and % become words; "hundred and" = "hundred"; emails and URLs
are read out: "." between letters = "dot", "@" = "at", "/" = "slash",
"_" = "underscore"). So WER counts wrong, missing and extra words; it does
not see formatting. Formatting is what the rule checks are for.

Rule checks: the case's own checks (CHECKS) plus, for every case, no filler
words left, no "Here's the cleaned text" notes and no Devanagari; for clean-up
output also nothing dropped (a word in both the transcript and the expected
text that the clean-up removed) and nothing invented (a word in neither the
transcript, the spoken line nor the expected text). judgments.json can
override a verdict after reading the outputs: {"<clip>|<raw|app|model>": [true|false, "reason"]}.

Three outputs are scored per clip: raw (what Lipik pastes with clean-up off),
app (what Lipik pastes with clean-up on: raw when it skips the call), model
(the model's output on every clip, even ones the app would skip).
"""

import json
import re
import statistics
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE.parent / "bench"))
from textnorm import edit_distance, has_indic_script  # noqa: E402
from score import loose, strict  # noqa: E402  (training/bench/score.py)
from guards import apply, clean_up, fillers  # noqa: E402

VOCAB = json.loads((HERE / "vocab.json").read_text())
OUTPUTS = ["raw", "app", "model", "rules", "best", "best_all"]

FILLERS = r"\b(um+|uh+|uhm|er|hmm+)\b"
# A Hinglish output passes only if it is close enough to read: loose WER
# (spelling variants folded, see training/bench/score.py) at most 30%.
HINGLISH_READABLE = 0.30

# Per-case checks: (reason shown on failure, "has" | "not", regex)
CHECKS = {
    "fil02": [("'you know'/'like' left", "not", r"(?i)\byou know\b|\blike\b")],
    "fil03": [("'hum/hmm' left", "not", r"(?i)\b(hum|hmm)\b")],
    "fil04": [("'you am'/'a' noise left", "not", r"(?i)\byou am\b|^a hey")],
    "fil05": [("repeated words left", "not", r"(?i)\bi,? i\b|\bthe,? the\b")],
    "cor01": [("correction not applied", "not", r"(?i)\b(4|four)\b"), ("no 5", "has", r"\b(5|five)\b"),
              ("no question mark", "has", r"\?")],
    "cor02": [("correction not applied", "not", r"(?i)zubin|sorry|meant"), ("no Tavish", "has", r"Tavish")],
    "cor03": [("correction not applied", "not", r"(?i)tuesday|actually")],
    "cor04": [("correction not applied", "not", r"(?i)\b20\b|twenty|scratch"), ("no 50", "has", r"\b(50|fifty)\b")],
    "cor05": [("correction not applied", "not", r"(?i)\b6\b|\bsix\b|i mean"), ("no 7", "has", r"\b(7|seven)\b")],
    "cor06": [("correction not applied", "not", r"(?i)\bmeh|no wait"), ("no Ishita", "has", r"Ishita\b"),
              ("no question mark", "has", r"\?")],
    "cor07": [("correction not applied", "not", r"(?i)\b10\b|\bten\b|sorry"), ("no 12 lakh", "has", r"(?i)(12|twelve) lakh")],
    "cor08": [("repeated words left", "not", r"(?i)we should,? we should")],
    "pun01": [("spoken punctuation left", "not", r"(?i)comm|period"), ("no comma", "has", r"update,")],
    "pun02": [("spoken punctuation left", "not", r"(?i)new line|period"), ("no line breaks", "has", r"\n")],
    "pun03": [("spoken punctuation left", "not", r"(?i)question mark"), ("no ?", "has", r"\?")],
    "pun04": [("spoken punctuation left", "not", r"(?i)exclamation"), ("no !", "has", r"!")],
    "pun05": [("spoken punctuation left", "not", r"(?i)comma|period"), ("no commas", "has", r"red, blue")],
    "num01": [("money not formatted", "has", r"\$2,?450")],
    "num02": [("time not 3:30 PM", "has", r"(?i)3:30\s?(pm|p\.m\.)")],
    "num03": [("percent not formatted", "has", r"12%")],
    "num04": [("date not formatted", "has", r"March 14(th)?,? 2027")],
    "num05": [("rupees not formatted", "has", r"(₹\s?1,?500|1,?500 rupees|Rs\.? ?1,?500)")],
    "num06": [("phone number not as digits", "has", r"9876543210|98765 43210")],
    "num07": [("version not 2.4.1", "has", r"2\.4\.1")],
    "eml01": [("email wrong", "has", r"(?i)meher\.kapadia@velocore\.io")],
    "eml02": [("URL wrong", "has", r"(?i)docs\.brightlane\.dev/setup")],
    "eml03": [("email wrong", "has", r"(?i)rivaan_m@quorvia\.com"), ("dropped 'My email is'", "has", r"(?i)my email is")],
    "eml04": [("URL wrong", "has", r"(?i)\bquorvia\.com/pricing")],
    "eml05": [("email wrong", "has", r"(?i)tavish@brightlane\.dev")],
    "lst01": [("not a numbered list", "has", r"\n\s*1[.)][\s\S]*\n\s*2[.)][\s\S]*\n\s*3[.)]")],
    "lst02": [("not a bullet list", "has", r"\n\s*[-•*][\s\S]*\n\s*[-•*][\s\S]*\n\s*[-•*]"),
              ("'bullet' left", "not", r"(?i)bullet")],
    "lst03": [("not a numbered list", "has", r"\n\s*1[.)][\s\S]*\n\s*2[.)][\s\S]*\n\s*3[.)]")],
    "lst04": [("not a list", "has", r"\n\s*(1[.)]|[-•*])[\s\S]*\n\s*(2[.)]|[-•*])")],
    "voc01": [("name wrong", "has", r"Ishita Venkatraman"), ("Velocore wrong", "has", r"Velocore")],
    "voc02": [("Quorvia wrong", "has", r"Quorvia"), ("names wrong", "has", r"Tavish and Rivaan"),
              ("no question mark", "has", r"\?")],
    "voc03": [("names wrong", "has", r"Brightlane[\s\S]*Meher Kapadia")],
    "voc04": [("names wrong", "has", r"Zubin[\s\S]*Quorvia")],
    "voc05": [("name wrong", "has", r"Oshin Dastur")],
    "tec01": [("terms wrong", "has", r"\bRedis\b[\s\S]*\bPostgreSQL\b")],
    "tec02": [("terms wrong", "has", r"Next\.js[\s\S]*\bAPI\b")],
    "tec03": [("term wrong", "has", r"Kubernetes"), ("no question mark", "has", r"\?")],
    "tec04": [("terms wrong", "has", r"GraphQL[\s\S]*orders API")],
    "tec05": [("terms wrong", "has", r"TypeScript[\s\S]*\benv\b[\s\S]*Supabase")],
    "que01": [("no question mark", "has", r"\?$"), ("repeat left", "not", r"(?i)the,? the")],
    "que02": [("no question mark", "has", r"\?$")],
    "que03": [("no question mark", "has", r"\?$")],
    "que04": [("no question mark", "has", r"\?$")],
    "run01": [("not split into sentences", "has", r"[.:;][\s\S]*[.!?][\s\S]+")],
    "run02": [("not split into sentences", "has", r"\.\s[\s\S]*\?$")],
    "run03": [("no final period", "has", r"\.$")],
    "run04": [("not split into sentences", "has", r"\.\s[\s\S]*\.\s"), ("time not 8:15", "has", r"8:15")],
    "hng01": [("filler left", "not", r"(?i)\bum")],
    "hng02": [("filler left", "not", r"(?i)\buh\b")],
    "hng03": [("correction not applied", "not", r"(?i)\b(paanch|punch|5)\b|nahi")],
    "hng05": [("no question mark", "has", r"\?")],
    "hng06": [("no question mark", "has", r"\?")],
    "hng07": [("repeated word left", "not", r"(?i)\b(hum|ham),? hum\b")],
    "hng10": [("no question mark", "has", r"\?")],
}


def wer_norm(text):
    t = text.replace("\n", " ")
    t = re.sub(r"\b([ap])\.\s?m\b\.?", r"\1m", t, flags=re.I)
    t = re.sub(r"\$\s?(\d[\d,]*)", r"\1 dollars", t)
    t = re.sub(r"(?<=[A-Za-z0-9])\.(?=[A-Za-z])", " dot ", t)
    t = t.replace("@", " at ").replace("_", " underscore ")
    t = re.sub(r"(?<=\w)/(?=\w)", " slash ", t)
    t = strict(t)
    return re.sub(r"\b(hundred|thousand) and\b", r"\1", t)


def tokens(text):
    return set(wer_norm(text).split())


def all_cases():
    cs = {c["id"]: c for c in json.loads((HERE / "cases.json").read_text())}
    for c in json.loads((HERE / "holdout.json").read_text()):
        cs[c["id"]] = c
        CHECKS[c["id"]] = ([("missing " + p, "has", p) for p in c["has"]] +
                           [("left in " + p, "not", p) for p in c["not"]])
    return cs


def rule_check(case, out, raw, is_raw):
    fails = []
    if re.search(FILLERS, out, re.I):
        fails.append("filler left")
    if re.search(r"(?i)here('s| is) the cleaned|cleaned text", out):
        fails.append("added a note")
    if has_indic_script(out):
        fails.append("Devanagari")
    for reason, kind, pattern in CHECKS.get(case["id"], []):
        hit = re.search(pattern, out.strip()) is not None
        if hit != (kind == "has"):
            fails.append(reason)
    if case["category"] == "hinglish":
        errs, n = wer_counts(case["expected"], out, True)
        if errs / n > HINGLISH_READABLE:
            fails.append(f"unreadable Hinglish (loose WER {100 * errs / n:.0f}%)")
    if not is_raw:
        exp, rw, o = tokens(case["expected"]), tokens(raw), tokens(out)
        dropped = (exp & rw) - o
        invented = o - rw - exp - tokens(case["spoken"])
        if dropped:
            fails.append("dropped: " + " ".join(sorted(dropped)))
        if invented:
            fails.append("added: " + " ".join(sorted(invented)))
    return not fails, "; ".join(fails)


def wer_counts(ref, hyp, hinglish):
    f = loose if hinglish else wer_norm
    r, h = f(ref).split(), f(hyp).split()
    return edit_distance(r, h), len(r)


def load(tag):
    return [json.loads(l) for l in (HERE / "runs" / f"cleanup-{tag}.jsonl").read_text().splitlines()]


def score(tag, dump=False):
    cases = all_cases()
    vocab = json.loads((HERE / "holdout_vocab.json").read_text()) if "holdout" in tag else VOCAB
    overrides = {}
    jpath = HERE / f"judgments-{tag}.json"
    if jpath.exists():
        overrides = json.loads(jpath.read_text())
    rows = load(tag)
    cats = defaultdict(lambda: defaultdict(lambda: [0, 0, 0, 0]))  # out -> cat -> [pass, n, errs, words]
    results = []
    for r in rows:
        case = cases[r["id"]]
        model_out = r["model_out"] if (r["latency_ms"] or 0) <= 4000 else None
        best, best_why = clean_up(r["raw"], model_out, vocab)
        best_all, best_all_why = clean_up(r["raw"], model_out, vocab, min_words=0)
        outs = {"raw": r["raw"], "app": r["app_final"], "model": r["model_out"] or r["raw"],
                "rules": apply(fillers(r["raw"]), vocab), "best": best, "best_all": best_all}
        res = {"clip": r["clip"], "id": r["id"], "category": case["category"], "target": case["app"],
               "expected": case["expected"], "path": r["app_path"], "latency_ms": r["latency_ms"],
               "best_why": best_why, "best_all_why": best_all_why, "words": len(r["raw"].split())}
        for name, out in outs.items():
            ok, why = rule_check(case, out, r["raw"], name == "raw")
            key = f"{r['clip']}|{name}"
            if key in overrides:
                ok, why = overrides[key][0], "(reviewed) " + overrides[key][1]
            errs, n = wer_counts(case["expected"], out, case["category"] == "hinglish")
            for cat in (case["category"], "ALL") + (() if case["category"] == "hinglish" else ("ENGLISH",)):
                t = cats[name][cat]
                t[0] += ok
                t[1] += 1
                t[2] += errs
                t[3] += n
            res[name] = {"text": out, "pass": ok, "why": why, "wer": 100 * errs / max(n, 1)}
        results.append(res)
    if dump == "fails":
        for res in results:
            if not res["best"]["pass"]:
                print(f"{res['clip']:<16} [{res['target']}] {res['best_why'][:40]}\n   raw : {res['raw']['text']!r}\n"
                      f"   best: {res['best']['text']!r}  <- {res['best']['why']}\n   exp : {res['expected']!r}")
    elif dump:
        for res in results:
            print(f"\n{res['clip']} [{res['category']}, {res['target']}, {res['path']}, {res['latency_ms']} ms]")
            print(f"  expected: {res['expected']!r}")
            print(f"  best path: {res['best_why']}")
            for name in OUTPUTS:
                x = res[name]
                print(f"  {name:5} {'PASS' if x['pass'] else 'FAIL'} {x['wer']:5.1f} {x['text']!r}  {x['why']}")
    return cats, results


def table(tag, cats, outputs=("raw", "app", "model", "rules", "best", "best_all")):
    order = ["filler", "correction", "spoken_punct", "numbers", "email_url", "list", "vocab", "tech",
             "question", "run_on", "hinglish", "ENGLISH", "ALL"]
    print(f"\n### {tag}\n")
    print("| Category | Clips | " + " | ".join(f"Pass: {o}" for o in outputs) + " | WER raw | WER best |")
    print("|---|---|" + "---|" * len(outputs) + "---|---|")
    pc = lambda t: f"{t[0]}/{t[1]} ({100 * t[0] / t[1]:.0f}%)"  # noqa: E731
    wr = lambda t: f"{100 * t[2] / t[3]:.1f}"  # noqa: E731
    for cat in order:
        if not cats["raw"][cat][1]:
            continue
        name = f"**{cat}**" if cat in ("ALL", "ENGLISH") else cat
        cells = " | ".join(pc(cats[o][cat]) for o in outputs)
        print(f"| {name} | {cats['raw'][cat][1]} | {cells} | {wr(cats['raw'][cat])} | {wr(cats['best'][cat])} |")


def guard_report(results):
    """How often the guards fired, and how many good clean-ups they threw away."""
    for key, out in (("best_why", "best"), ("best_all_why", "best_all")):
        rej = [r for r in results if r[key].startswith("rejected")]
        good = [r for r in rej if r["model"]["pass"]]
        caught = [r for r in rej if not r["model"]["pass"]]
        bad_through = [r for r in results if r[key] == "cleaned" and not r["model"]["pass"]]
        print(f"{out}: guard rejected {len(rej)} model outputs; {len(good)} of them had passed the rules "
              f"(wrongly rejected), {len(caught)} had failed (caught); failing outputs let through: {len(bad_through)}")
        for r in good:
            print(f"   wrongly rejected {r['clip']}: {r[key]} | {r['model']['text']!r}")


def latency(tag):
    rows = load(tag)
    lat = sorted(r["latency_ms"] for r in rows if r["latency_ms"] is not None)
    called = sorted(r["latency_ms"] for r in rows if r["app_path"] == "cleaned")
    called8 = sorted(r["latency_ms"] for r in rows if r["latency_ms"] is not None and len(r["raw"].split()) >= 8)
    p90 = lambda xs: xs[int(0.9 * (len(xs) - 1))]  # noqa: E731
    print(f"\n{tag}: all calls n={len(lat)} median {statistics.median(lat):.0f} ms, p90 {p90(lat)} ms, max {lat[-1]} ms; "
          f"clips the app would clean n={len(called)} median {statistics.median(called):.0f} ms, p90 {p90(called)} ms; "
          f"over 4 s: {sum(x > 4000 for x in lat)}; model called on 8+ words n={len(called8)} median "
          f"{statistics.median(called8):.0f} ms, p90 {p90(called8)} ms")
    paths = defaultdict(int)
    for r in rows:
        paths[r["app_path"].split(":")[0] if r["app_path"].startswith("skipped: ") and "words" in r["app_path"] else r["app_path"]] += 1
    print("app path:", dict(paths))


def compare(cur, new):
    """Raw / Lipik today (the app column of `cur`) / the new setup (best column of `new`)."""
    order = ["filler", "correction", "spoken_punct", "numbers", "email_url", "list", "vocab", "tech",
             "question", "run_on", "hinglish", "ENGLISH", "ALL"]
    c, _ = score(cur)
    n, _ = score(new)
    pc = lambda t: f"{t[0]}/{t[1]} ({100 * t[0] / t[1]:.0f}%)"  # noqa: E731
    wr = lambda t: f"{100 * t[2] / t[3]:.1f}"  # noqa: E731
    print(f"\n{cur} -> {new}\n")
    print("| Category | Clips | Raw (clean-up off) | Lipik today | New setup | WER raw / today / new |")
    print("|---|---|---|---|---|---|")
    for cat in order:
        if not c["raw"][cat][1]:
            continue
        name = f"**{cat}**" if cat in ("ENGLISH", "ALL") else cat
        print(f"| {name} | {c['raw'][cat][1]} | {pc(c['raw'][cat])} | {pc(c['app'][cat])} | {pc(n['best'][cat])} | "
              f"{wr(c['raw'][cat])} / {wr(c['app'][cat])} / {wr(n['best'][cat])} |")


def main():
    if "--compare" in sys.argv:
        tags = [a for a in sys.argv[1:] if not a.startswith("--")]
        compare(tags[0], tags[1])
        return
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    tags = args or sorted(p.stem[len("cleanup-"):] for p in (HERE / "runs").glob("cleanup-*.jsonl"))
    for tag in tags:
        cats, results = score(tag, dump="fails" if "--fails" in sys.argv else "--dump" in sys.argv)
        table(tag, cats)
        latency(tag)
        guard_report(results)


if __name__ == "__main__":
    main()
