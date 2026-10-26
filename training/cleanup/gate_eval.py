"""What the model gate would change, from runs that called the model on every clip.

    python3 gate_eval.py base8 text-base8 holdout-base8 long-base8 longtext-base8
    python3 gate_eval.py --summary final8b-whole final8b      # pass rate and waits per run
    python3 gate_eval.py --estimate final8b                   # writes runs/cleanup-final8b-est.jsonl

For each clip: "now" is what the app pastes today (rules under 8 words, else the
model's reply if it passes the checks); "gated" pastes the rules text with a
final period when no sentence needs the model (guards.model_spans), else the same
as now. Latency counts 0 ms for clips that skip the model.
"""

import importlib.util
import json
import statistics
import sys
from collections import Counter
from pathlib import Path

from guards import MIN_MODEL_WORDS, apply, clean_up, clean_up_parts, end_mark, fillers, model_spans
# score.py imports training/bench/score.py as "score", so load this one under another name.
_spec = importlib.util.spec_from_file_location("cleanup_score", Path(__file__).with_name("score.py"))
_score = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_score)
all_cases, load, rule_check, VOCAB, HERE = _score.all_cases, _score.load, _score.rule_check, _score.VOCAB, _score.HERE


def p90(xs):
    xs = sorted(xs)
    return xs[int(0.9 * (len(xs) - 1))] if xs else 0


def evaluate(tag, show=False):
    cases = all_cases()
    vocab = json.loads((HERE / "holdout_vocab.json").read_text()) if "holdout" in tag else VOCAB
    rows = load(tag)
    n = now_pass = gated_pass = 0
    skipped, reasons, lat_now, lat_gated, lost, won = 0, Counter(), [], [], [], []
    for r in rows:
        case = cases[r["id"]]
        if case["category"] == "hinglish":
            continue
        transcript = fillers(r["raw"])
        rules_text = apply(transcript, vocab)
        reply = r["model_out"] if (r["latency_ms"] or 0) <= 4000 else None
        now, _ = clean_up(r["raw"], reply, vocab)
        calls = len(transcript.split()) >= MIN_MODEL_WORDS and any(c.isalnum() for c in transcript)
        spans = model_spans(rules_text, vocab) if calls else []
        if calls and not spans:
            gated = end_mark(rules_text)
            skipped += 1
        else:
            gated = now
        for a, b, why in spans:
            for w in why.split("+"):
                reasons[w] += 1
        ok_now, _ = rule_check(case, now, r["raw"], False)
        ok_gated, why_gated = rule_check(case, gated, r["raw"], False)
        n += 1
        now_pass += ok_now
        gated_pass += ok_gated
        if calls and r["latency_ms"] is not None:
            lat_now.append(r["latency_ms"])
            lat_gated.append(r["latency_ms"] if spans else 0)
        if ok_now and not ok_gated:
            lost.append((r["clip"], gated, why_gated))
        if ok_gated and not ok_now:
            won.append((r["clip"], gated))
    called = len(lat_now)
    print(f"{tag}: English clips {n}; model would be called on {called}, gate skips {skipped} "
          f"({100 * skipped / max(called, 1):.0f}%). Pass now {now_pass}/{n} -> gated {gated_pass}/{n}. "
          f"Latency (clips of 8+ words) median {statistics.median(lat_now or [0]):.0f} -> "
          f"{statistics.median(lat_gated or [0]):.0f} ms, p90 {p90(lat_now)} -> {p90(lat_gated)} ms")
    print("   reasons:", dict(reasons.most_common()))
    for clip, text, why in lost:
        print(f"   lost {clip}: {text!r} <- {why}")
    for clip, text in won:
        print(f"   won  {clip}: {text!r}")
    return n, now_pass, gated_pass




def summary(tags):
    """English pass (what is pasted) and the wait for the model, per run."""
    print("| Run | English pass | All clips: median / p90 | Under 0.4 s | 8+ words: median / p90 | Over 50 words: median / p90 / max |")
    print("|---|---|---|---|---|---|")
    for tag in tags:
        _, results = _score.score(tag)
        rows = {r["clip"]: r for r in load(tag)}
        eng = [x for x in results if x["category"] != "hinglish"]
        ok = sum(x["best"]["pass"] for x in eng)
        waits, eight, fifty = [], [], []
        for x in eng:
            r = rows[x["clip"]]
            words = len(fillers(r["raw"]).split())
            if "final" in r:
                ms = r["latency_ms"]
            else:
                ms = min(r["latency_ms"] or 0, 4000) if words >= MIN_MODEL_WORDS else 0
            waits.append(ms)
            if words >= MIN_MODEL_WORDS:
                eight.append(ms)
            if words > 50:
                fifty.append(ms)
        med = lambda xs: statistics.median(xs) if xs else 0  # noqa: E731
        print(f"| {tag} | {ok}/{len(eng)} ({100 * ok / len(eng):.0f}%) | {med(waits):.0f} / {p90(waits)} ms | "
              f"{100 * sum(w <= 400 for w in waits) / len(waits):.0f}% | {med(eight):.0f} / {p90(eight)} ms | "
              + (f"{med(fifty):.0f} / {p90(fifty)} / {max(fifty)} ms |" if fifty else "– |"))





# Quiet-machine timing, fitted on the morning runs (base8, 258 calls with nothing else
# using the GPU): one call takes about 157 ms + 30 ms per reply token (median error 112 ms).
CALL_MS, TOKEN_MS = 157, 30


def estimate(tag):
    """Write runs/cleanup-<tag>-est.jsonl: the same run with each call's time replaced by the
    quiet-machine estimate, for runs made while other programs slowed the GPU down."""
    out = []
    for r in load(tag):
        r = dict(r)
        if "calls" in r:
            # Replay the recorded replies, so a call that only ran over 4 s because of
            # the load is used.
            replies = iter(r["calls"])

            def call(text):
                c = next(replies)
                assert c["in"] == text
                c["ms"] = CALL_MS + TOKEN_MS * (c["out_tokens"] or 0)
                return c["out"] if c["ms"] <= 4000 else None

            vocab = json.loads((HERE / "holdout_vocab.json").read_text()) if "holdout" in tag else VOCAB
            r["final"], r["final_why"], _ = clean_up_parts(r["raw"], vocab, call)
            r["latency_ms"] = sum(c["ms"] for c in r["calls"])
        elif r.get("out_tokens") is not None:
            r["latency_ms"] = CALL_MS + TOKEN_MS * r["out_tokens"]
        out.append(json.dumps(r, ensure_ascii=False))
    (HERE / "runs" / f"cleanup-{tag}-est.jsonl").write_text("\n".join(out) + "\n")


if __name__ == "__main__":
    if sys.argv[1] == "--summary":
        summary(sys.argv[2:])
    elif sys.argv[1] == "--estimate":
        for t in sys.argv[2:]:
            estimate(t)
    else:
        for t in sys.argv[1:]:
            evaluate(t)
