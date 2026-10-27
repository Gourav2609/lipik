"""Clean-up benchmark: messy speech -> Parakeet (through Lipik's headless mode) -> Ollama clean-up.

    python3 run.py audio                      # say -o, never played
    python3 run.py asr                        # portable lipik copy in app/, Parakeet en
    python3 run.py cleanup --vocab-in-prompt --prepass --tag r3          # the app's setup, qwen3:8b
    python3 run.py cleanup --model qwen3:4b-instruct --vocab-in-prompt --prepass --tag q4
    python3 run.py cleanup --prompt-file prompt_v6.txt --vocab-in-prompt --prepass --tag v6
    python3 run.py cleanup ... --input spoken --tag text-r3   # skip ASR: clean the spoken line
    python3 run.py cleanup ... --input spoken --cases holdout.json --vocab holdout_vocab.json --tag holdout-r3
    python3 run.py prompt --vocab-in-prompt   # print the messages sent
    python3 run.py cleanup --api openai --url http://127.0.0.1:8080 --model qwen3-4b-builtin ...
        # Lipik's built-in engine (llama-server, started with engine.rs's arguments):
        # POST /v1/chat/completions with the app's request body; --key for its API key

The default prompt is the app's own, src-tauri/src/textflow/cleanup_prompt.txt.
A prompt file has a "### EXAMPLES" section: it is sent as a fixed system
prompt plus example turns, with the app name and the dictionary words that
sound like the dictation (as the app picks them) in the user message, at a
fixed num_ctx (--num-ctx, 4096). --prepass gives the model what the app gives
it: the transcript after the English filler step and the rules step
(guards.py mirrors the app code). Results: runs/asr-<asr>.jsonl and
runs/cleanup-<tag>.jsonl; reruns skip clips already done.
"""

import argparse
import json
import statistics
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

from guards import (PIPELINE_RS, PROMPT_FILE, _strings, clean_up_parts, fillers, model_input, relevant_vocabulary,
                    strip_wrapping)

HERE = Path(__file__).parent
REPO = HERE.parent.parent
APP = HERE / "app"
AUDIO = HERE / "audio"
RUNS = HERE / "runs"
PARAKEET = "handy-computer/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf"
ASR_MODELS = {"parakeet-en": (PARAKEET, "en")}
EN_VOICES = ["Rishi", "Aman"]
HG_VOICES = ["Lekha", "Aman"]

# pipeline.rs / llm.rs constants
MIN_CLEANUP_WORDS = 8
CLEANUP_TIMEOUT_S = 4.0
KEEP_ALIVE = "1h"


CASES_FILE, VOCAB_FILE = "cases.json", "vocab.json"


def cases():
    return {c["id"]: c for c in json.loads((HERE / CASES_FILE).read_text())}


def suffix():
    """"" for cases.json, "-long" for long.json: other case files get their own clip and ASR lists."""
    return "" if CASES_FILE == "cases.json" else "-" + Path(CASES_FILE).stem


def clips():
    return json.loads((AUDIO / f"clips{suffix()}.json").read_text())


# ---------- audio ----------

def make_audio(_args):
    AUDIO.mkdir(exist_ok=True)
    out = []
    for c in cases().values():
        for voice in HG_VOICES if c["category"] == "hinglish" else EN_VOICES:
            wav = AUDIO / f"{c['id']}_{voice}.wav"
            if not wav.exists():
                aiff = wav.with_suffix(".aiff")
                subprocess.run(["say", "-v", voice, "-o", str(aiff), c["spoken"]], check=True)
                subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(aiff),
                                "-af", "adelay=300,apad=pad_dur=0.6", "-ar", "16000", "-ac", "1",
                                "-c:a", "pcm_s16le", str(wav)], check=True)
                aiff.unlink()
            out.append({"clip": wav.name, "id": c["id"], "voice": voice})
    (AUDIO / f"clips{suffix()}.json").write_text(json.dumps(out, indent=1) + "\n")
    print(len(out), "clips")


# ---------- ASR ----------

def run_asr(args):
    model, language = ASR_MODELS[args.asr]
    settings_path = APP / "Data" / "settings_store.json"
    store = json.loads(settings_path.read_text()) if settings_path.exists() else {"settings": {}}
    store["settings"]["selected_language"] = language
    store["settings"]["custom_words"] = json.loads((HERE / "vocab.json").read_text())
    settings_path.write_text(json.dumps(store, indent=2))
    RUNS.mkdir(exist_ok=True)
    out = RUNS / f"asr-{args.asr}{suffix()}.jsonl"
    done = {json.loads(l)["clip"] for l in out.read_text().splitlines()} if out.exists() else set()
    todo = [c for c in clips() if c["clip"] not in done][: args.limit or None]
    with out.open("a") as f:
        for c in todo:
            proc = subprocess.run(
                ["perl", "-e", "alarm shift; exec @ARGV", "120", str(APP / "lipik"),
                 "--transcribe-file", str(AUDIO / c["clip"]), "--model", model,
                 "--repeat", "3", "--json"],
                capture_output=True, text=True)
            lines = [l for l in proc.stdout.splitlines() if l.startswith("{")]
            if not lines:
                err = proc.stderr.strip().splitlines()[-1:] or ["no output"]
                print(f"{c['clip']}: failed: {err[0]}", file=sys.stderr)
                continue
            r = json.loads(lines[-1])
            f.write(json.dumps({"clip": c["clip"], "id": c["id"], "voice": c["voice"],
                                "text": r["text"], "audio_secs": r["audio_secs"],
                                "load_ms": r["load_ms"], "transcribe_ms": r["transcribe_ms"],
                                "best_ms": r["best_ms"]}) + "\n")
            f.flush()
            print(c["clip"], "|", r["text"])


# ---------- clean-up prompt ----------

EXAMPLE_VOCAB = _strings(PIPELINE_RS, "EXAMPLE_VOCABULARY")


def user_message(app_name, text, vocab):
    """The app name and the vocabulary go in the user message, so the system prompt and
    the examples are the same for every call (prompt cache)."""
    lines = [f"App: {app_name}"]
    if vocab:
        lines.append("Vocabulary: " + ", ".join(vocab))
    return "\n".join(lines) + f"\n<transcript>\n{text}\n</transcript>"


def fewshot_prompt(prompt_file):
    """(system text, [(app, input, output)]) from a prompt file: '>>> App' + input,
    '<<<' + output after '### EXAMPLES'."""
    text = Path(prompt_file or PROMPT_FILE).read_text()
    if "### EXAMPLES" not in text:
        raise SystemExit(f"{prompt_file}: no '### EXAMPLES' section")
    system, ex = text.split("### EXAMPLES", 1)
    examples = []
    for block in ex.split(">>> ")[1:]:
        head, body = block.split("\n", 1)
        inp, out = body.split("<<<\n", 1)
        examples.append((head.strip(), inp.strip(), out.strip()))
    return system.strip(), examples


def build_messages(app_name, text, prompt_file, vocab):
    """vocab: the dictionary words sent with this dictation, or None for no vocabulary."""
    system, examples = fewshot_prompt(prompt_file)
    msgs = [{"role": "system", "content": system}]
    for app, inp, out in examples:
        msgs.append({"role": "user", "content": user_message(app, inp, EXAMPLE_VOCAB if vocab is not None else None)})
        msgs.append({"role": "assistant", "content": out})
    msgs.append({"role": "user", "content": user_message(app_name, text, vocab)})
    return msgs


# ---------- Ollama or llama-server ----------

API = "ollama"
KEY = None


def post(url, body, timeout=120):
    headers = {"Content-Type": "application/json"}
    if KEY:
        headers["Authorization"] = f"Bearer {KEY}"
    req = urllib.request.Request(url, json.dumps(body).encode(), headers)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def strip_think(s):
    t = s.lstrip()
    if t.startswith("<think>") and "</think>" in t:
        return t[t.index("</think>") + len("</think>"):].lstrip()
    return s


def too_long(output, inp, times=1.5, extra=40):
    return len(output) > len(inp) * times + extra


def openai_chat(url, messages, think, max_tokens):
    """The built-in engine's request (engine.rs chat_body), its reply in Ollama's shape."""
    body = {"messages": messages, "stream": False, "temperature": 0.2, "cache_prompt": True,
            "chat_template_kwargs": {"enable_thinking": bool(think)}}
    if max_tokens:
        body["max_tokens"] = max_tokens
    t = time.perf_counter()
    r = post(f"{url}/v1/chat/completions", body)
    ms = (time.perf_counter() - t) * 1000
    choice, timings, usage = r["choices"][0], r.get("timings", {}), r.get("usage", {})
    shaped = {"message": {"content": choice["message"].get("content") or "",
                          "thinking": choice["message"].get("reasoning_content")},
              "done_reason": choice.get("finish_reason"),
              "prompt_eval_count": usage.get("prompt_tokens"), "eval_count": usage.get("completion_tokens"),
              "prompt_eval_duration": timings.get("prompt_ms", 0) * 1e6,
              "eval_duration": timings.get("predicted_ms", 0) * 1e6, "load_duration": 0}
    return strip_think(shaped["message"]["content"]).strip(), ms, shaped


def chat(url, model, messages, context, think, num_predict=None):
    if API == "openai":
        return openai_chat(url, messages, think, num_predict)
    options = {"temperature": 0.2, "num_ctx": context}
    if num_predict:
        options["num_predict"] = num_predict
    body = {"model": model, "stream": False, "keep_alive": KEEP_ALIVE,
            "options": options, "messages": messages}
    if think is not None:
        body["think"] = think
    t = time.perf_counter()
    r = post(f"{url}/api/chat", body)
    ms = (time.perf_counter() - t) * 1000
    return strip_think(r["message"]["content"]).strip(), ms, r


def num_predict(text):
    """Room for a reply up to the longest the checks accept (1.5x + 40 characters): about
    3 characters per token for English, a token per byte for other scripts (pipeline.rs)."""
    size = len(text.encode())
    return int((size * 1.5 + 40) / (3 if text.isascii() else 1)) + 16


def warm_up(url, model, messages, think, context):
    if API == "openai":
        openai_chat(url, messages, think, 1)
        return
    body = {"model": model, "stream": False, "keep_alive": KEEP_ALIVE,
            "options": {"num_ctx": context, "num_predict": 1}, "messages": messages}
    if think is not None:
        body["think"] = think
    post(f"{url}/api/chat", body, timeout=300)


def run_cleanup(args):
    cs = cases()
    if args.input == "spoken":
        # The spoken line as text: what a perfect transcriber would give.
        asr = {f"{c['id']}_text": {"text": c["spoken"]} for c in cs.values()}
        clip_list = [{"clip": k, "id": k[:-5], "voice": "text"} for k in asr]
    else:
        asr = {json.loads(l)["clip"]: json.loads(l) for l in (RUNS / f"asr-{args.asr}{suffix()}.jsonl").read_text().splitlines()}
        clip_list = clips()
    full_vocab = json.loads((HERE / VOCAB_FILE).read_text())
    out = RUNS / f"cleanup-{args.tag}.jsonl"
    done = {json.loads(l)["clip"] for l in out.read_text().splitlines()} if out.exists() else set()
    todo = [c for c in clip_list if c["clip"] in asr and c["clip"] not in done][: args.limit or None]
    if not todo:
        print("nothing to do")
        return
    think = args.think if args.model.startswith("qwen3") else None
    t = time.perf_counter()
    # One fixed context size: a size change reloads the model.
    context = args.num_ctx
    no_vocab = [] if args.vocab_in_prompt else None
    warm_up(args.url, args.model, build_messages("Notes", "ok", args.prompt_file, no_vocab), think, context)
    print(f"warm-up {time.perf_counter() - t:.1f} s")
    # One real call, thrown away, so the first timed call is warm.
    first = asr[todo[0]["clip"]]["text"]
    chat(args.url, args.model, build_messages(cs[todo[0]["id"]]["app"], first, args.prompt_file, no_vocab),
         context, think)
    whole_f = (RUNS / f"cleanup-{args.tag}-whole.jsonl").open("a") if args.pair else None
    with out.open("a") as f:
        for c in todo:
            case = cs[c["id"]]
            raw = asr[c["clip"]]["text"]
            model_in = model_input(raw, full_vocab) if args.prepass else raw
            vocab = relevant_vocabulary(full_vocab, fillers(raw)) if args.vocab_in_prompt else None
            messages = build_messages(case["app"], model_in, args.prompt_file, vocab)
            row = {"clip": c["clip"], "id": c["id"], "voice": c["voice"], "raw": raw,
                   "model": args.model, "prompt": args.prompt_file or "app", "num_ctx": context,
                   "model_in": model_in, "vocab_sent": vocab}
            if args.pair:
                # Today's app: its warm-up (app "Notes", no dictionary), then the whole text.
                warm_up(args.url, args.model, build_messages("Notes", "ok", args.prompt_file, no_vocab), think, context)
                content, ms, r = chat(args.url, args.model, messages, context, think)
                whole_f.write(json.dumps(dict(row, model_out=strip_wrapping(content), latency_ms=round(ms),
                                              out_tokens=r.get("eval_count"), app_final=None, app_path="whole"),
                                         ensure_ascii=False) + "\n")
                whole_f.flush()
                # The gated app: its warm-up names the app and sends the dictionary.
                warm_up(args.url, args.model, build_messages(case["app"], "ok", args.prompt_file, vocab), think, context)
            if args.parts:
                calls = []

                def call(text):
                    content, ms, r = chat(args.url, args.model, build_messages(case["app"], text, args.prompt_file, vocab),
                                          context, think, num_predict(text) if args.num_predict else None)
                    calls.append({"in": text, "out": strip_wrapping(content), "ms": round(ms),
                                  "out_tokens": r.get("eval_count"), "prompt_ms": round(r.get("prompt_eval_duration", 0) / 1e6)})
                    if ms > CLEANUP_TIMEOUT_S * 1000 or r.get("done_reason") == "length":
                        return None
                    return strip_wrapping(content)

                final, why, _ = clean_up_parts(raw, full_vocab, call, args.whole_share)
                row.update(final=final, final_why=why, calls=calls, model_out=None,
                           latency_ms=sum(x["ms"] for x in calls), app_final=final, app_path=why)
                f.write(json.dumps(row, ensure_ascii=False) + "\n")
                f.flush()
                print(f"{c['clip']:<16} {row['latency_ms']} ms {len(calls)} calls | {why} | {final!r}")
                continue
            try:
                content, ms, r = chat(args.url, args.model, messages, context, think,
                                      num_predict(model_in) if args.num_predict else None)
                model_out = strip_wrapping(content)
                ns = lambda k: round(r.get(k, 0) / 1e6)  # noqa: E731
                row.update(model_out=model_out, latency_ms=round(ms),
                           prompt_tokens=r.get("prompt_eval_count"), out_tokens=r.get("eval_count"),
                           load_ms=ns("load_duration"), prompt_ms=ns("prompt_eval_duration"),
                           eval_ms=ns("eval_duration"), think_seen="<think>" in r["message"]["content"]
                           or bool(r["message"].get("thinking")))
            except Exception as e:  # noqa: BLE001
                model_out, ms = None, None
                row.update(model_out=None, latency_ms=None, error=str(e))
            words = len(raw.split())
            if not any(ch.isalnum() for ch in raw) or words < MIN_CLEANUP_WORDS:
                final, why = raw, f"skipped: {words} words"
            elif model_out is None:
                final, why = raw, "skipped: error"
            elif ms > CLEANUP_TIMEOUT_S * 1000:
                final, why = raw, "skipped: over 4 s"
            elif not model_out or too_long(model_out, raw):
                final, why = raw, "rejected: empty or too long"
            else:
                final, why = model_out, "cleaned"
            row.update(app_final=final, app_path=why)
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
            f.flush()
            print(f"{c['clip']:<16} {row['latency_ms']} ms | {raw!r} -> {model_out!r}")
    lat = [json.loads(l)["latency_ms"] for l in out.read_text().splitlines()]
    lat = sorted(x for x in lat if x is not None)
    print(f"median {statistics.median(lat):.0f} ms, p90 {lat[int(0.9 * (len(lat) - 1))]} ms")
    if args.unload and API == "ollama":
        post(f"{args.url}/api/generate", {"model": args.model, "keep_alive": 0})


def show_prompt(args):
    text = "um so this is a test for quorvia"
    vocab = relevant_vocabulary(json.loads((HERE / VOCAB_FILE).read_text()), text) if args.vocab_in_prompt else None
    for m in build_messages("Slack", model_input(text), args.prompt_file, vocab):
        print(f"--- {m['role']}\n{m['content']}")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("step", choices=["audio", "asr", "cleanup", "prompt"])
    p.add_argument("--asr", default="parakeet-en", choices=list(ASR_MODELS))
    p.add_argument("--model", default="qwen3:8b")
    p.add_argument("--url", default="http://127.0.0.1:11434")
    p.add_argument("--api", choices=["ollama", "openai"], default="ollama",
                   help="openai = an OpenAI-style server such as Lipik's built-in llama-server")
    p.add_argument("--key", help="--api openai: the server's API key")
    p.add_argument("--prompt-file")
    p.add_argument("--input", choices=["asr", "spoken"], default="asr",
                   help="spoken = clean the spoken line itself, to separate clean-up errors from ASR errors")
    p.add_argument("--vocab-in-prompt", action="store_true")
    p.add_argument("--prepass", action="store_true",
                   help="give the model the transcript after the filler and rules steps, as the app does")
    p.add_argument("--think", action="store_true")
    p.add_argument("--parts", action="store_true", help="the gated path: model only on the sentences that need it")
    p.add_argument("--whole-share", type=float, default=0.7, help="--parts: send the whole text when this share needs the model")
    p.add_argument("--num-predict", action="store_true", help="cap the reply length to what the checks accept")
    p.add_argument("--pair", action="store_true",
                   help="with --parts: before each gated clip, run it the old way too (old warm-up, whole text, "
                        "no cap) into runs/cleanup-<tag>-whole.jsonl, so both see the same machine load")
    p.add_argument("--num-ctx", type=int, default=4096, help="context size for few-shot prompts")
    p.add_argument("--tag", default="qwen3-8b-app")
    p.add_argument("--limit", type=int, default=0)
    p.add_argument("--unload", action="store_true", help="unload the model from Ollama when done")
    p.add_argument("--cases", default="cases.json", help="holdout.json for the held-out lines (spoken input)")
    p.add_argument("--vocab", default="vocab.json")
    args = p.parse_args()
    global CASES_FILE, VOCAB_FILE, API, KEY
    CASES_FILE, VOCAB_FILE = args.cases, args.vocab
    API, KEY = args.api, args.key
    if args.step == "audio":
        make_audio(args)
    elif args.step == "asr":
        run_asr(args)
    elif args.step == "cleanup":
        run_cleanup(args)
    else:
        show_prompt(args)


if __name__ == "__main__":
    main()
