"""Word and character error rates (WER / CER) on an exported split, for one
or more speech models, so a fine-tuned model can be compared with its base.

A system is NAME=BACKEND:MODEL[@ADAPTER]
    mlx:<repo or dir>                fast on Apple silicon, base models only
    hf:<repo or dir>                 Hugging Face transformers (MPS / CUDA / CPU)
    hf:<base repo>@<adapter dir>     base + LoRA adapter from train.py
    hf:<merged dir>                  output of merge.py

Compare before and after with the same backend, so decoding is identical:
    python eval.py --data data/me \\
        --system base=hf:openai/whisper-large-v3-turbo \\
        --system tuned=hf:openai/whisper-large-v3-turbo@runs/me/adapter

Prints scores only. Transcripts are written only when --save-hyp is given.
"""

import argparse
import json
import sys
import time
import wave
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from textnorm import edit_distance, normalize  # noqa: E402

DEFAULT_SYSTEM = "base=mlx:mlx-community/whisper-large-v3-turbo"


def read_wav(path: Path) -> np.ndarray:
    with wave.open(str(path)) as w:
        return np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float32) / 32768


class MlxSystem:
    def __init__(self, model: str, language: str):
        import mlx_whisper

        self.mlx_whisper, self.model, self.language = mlx_whisper, model, language

    def transcribe(self, samples: np.ndarray) -> str:
        out = self.mlx_whisper.transcribe(samples, path_or_hf_repo=self.model, language=self.language,
                                          temperature=0.0, condition_on_previous_text=False, verbose=None)
        return out["text"].strip()


class HfSystem:
    def __init__(self, model: str, adapter: str | None, language: str):
        import torch
        from transformers import WhisperForConditionalGeneration, WhisperProcessor

        self.torch = torch
        self.device = "cuda" if torch.cuda.is_available() else "mps" if torch.backends.mps.is_available() else "cpu"
        dtype = torch.float16 if self.device != "cpu" else torch.float32
        self.processor = WhisperProcessor.from_pretrained(adapter or model)
        net = WhisperForConditionalGeneration.from_pretrained(model, torch_dtype=dtype)
        if adapter:
            from peft import PeftModel

            net = PeftModel.from_pretrained(net, adapter)
        self.net = net.to(self.device).eval()
        self.dtype, self.language = dtype, language

    def transcribe(self, samples: np.ndarray) -> str:
        feats = self.processor.feature_extractor(samples, sampling_rate=16000, return_tensors="pt").input_features
        with self.torch.no_grad():
            ids = self.net.generate(input_features=feats.to(self.device, self.dtype), language=self.language,
                                    task="transcribe", num_beams=1, do_sample=False, max_new_tokens=440)
        return self.processor.batch_decode(ids, skip_special_tokens=True)[0].strip()


def build(spec: str, language: str):
    name, _, rest = spec.partition("=")
    backend, _, model = rest.partition(":")
    model, _, adapter = model.partition("@")
    if backend == "mlx":
        if adapter:
            sys.exit("mlx backend has no adapter support; use hf, or merge.py first")
        return name, MlxSystem(model, language)
    if backend == "hf":
        return name, HfSystem(model, adapter or None, language)
    sys.exit(f"unknown backend in {spec!r}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", type=Path, required=True, help="a speaker folder made by export.py")
    ap.add_argument("--split", default="val", choices=["val", "train", "all"])
    ap.add_argument("--system", action="append", help="NAME=BACKEND:MODEL[@ADAPTER], repeatable")
    ap.add_argument("--language", default="en", help="Whisper language token; 'en' keeps Hinglish in Roman letters")
    ap.add_argument("--limit", type=int, help="only the first N clips (smoke test)")
    ap.add_argument("--per-clip", action="store_true", help="also print per-clip scores (numbers only)")
    ap.add_argument("--save-hyp", type=Path, help="write transcripts to this JSONL (contains your words)")
    args = ap.parse_args()

    splits = ["train", "val"] if args.split == "all" else [args.split]
    items = [json.loads(line) for s in splits for line in (args.data / f"{s}.jsonl").read_text().splitlines() if line]
    items = items[: args.limit] if args.limit else items
    if not items:
        sys.exit("no clips in that split")
    audio = [read_wav(args.data / it["audio"]) for it in items]
    refs = [normalize(it["text"]) for it in items]

    results, saved = {}, []
    for spec in args.system or [DEFAULT_SYSTEM]:
        name, system = build(spec, args.language)
        system.transcribe(audio[0][:16000])  # warm-up, so load time is not counted
        w_err = w_tot = c_err = c_tot = 0
        secs = sum(len(a) for a in audio) / 16000
        t0 = time.time()
        for i, (it, samples, ref) in enumerate(zip(items, audio, refs)):
            hyp_raw = system.transcribe(samples)
            hyp = normalize(hyp_raw)
            we, ce = edit_distance(ref.split(), hyp.split()), edit_distance(list(ref), list(hyp))
            w_err, w_tot, c_err, c_tot = w_err + we, w_tot + len(ref.split()), c_err + ce, c_tot + len(ref)
            if args.per_clip:
                print(f"  {name:<10} {it['audio']:<40} {it['duration']:6.1f}s  "
                      f"WER {we / max(1, len(ref.split())):6.1%}  CER {ce / max(1, len(ref)):6.1%}")
            saved.append({"system": name, "audio": it["audio"], "hyp": hyp_raw})
        took = time.time() - t0
        results[name] = {"spec": spec, "clips": len(items), "audio_sec": round(secs, 1),
                         "wer": w_err / max(1, w_tot), "cer": c_err / max(1, c_tot),
                         "ref_words": w_tot, "decode_sec": round(took, 1), "rtf": round(took / secs, 3)}
        del system

    print(f"\n{'system':<10} {'clips':>5} {'audio s':>8} {'WER':>7} {'CER':>7} {'RTF':>6}")
    for name, r in results.items():
        print(f"{name:<10} {r['clips']:>5} {r['audio_sec']:>8} {r['wer']:>7.1%} {r['cer']:>7.1%} {r['rtf']:>6}")
    if len(results) > 1:
        names = list(results)
        base, *others = names
        for o in others:
            print(f"{o} vs {base}: WER {results[o]['wer'] - results[base]['wer']:+.1%}, "
                  f"CER {results[o]['cer'] - results[base]['cer']:+.1%}")
    if args.save_hyp:
        with open(args.save_hyp, "w") as f:
            for row in saved:
                f.write(json.dumps(row, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
