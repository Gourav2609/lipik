"""LoRA fine-tune of Whisper on one speaker's exported Lipik clips.

LoRA (low-rank adaptation) freezes the base model and trains small extra
matrices beside its attention layers (about 1% of the weights). The result is
an adapter folder of a few tens of MB per person; merge.py folds it back into
a full model that Lipik can load.

    python train.py --data data/me                  # trains, saves runs/me/adapter
    python train.py --data data/me --dry-run        # 2 steps, saves nothing
    python train.py --data data/me --dry-run --random-init   # timing only, no download

Runs on Apple MPS, NVIDIA CUDA (Colab / Kaggle) or CPU. On MPS the GPU memory
is capped with --max-mem-gb so a run fails with "out of memory" instead of
pushing the Mac into swap.
"""

import argparse
import json
import math
import os
import random
import sys
import time
import wave
from pathlib import Path

os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")

import numpy as np  # noqa: E402
import torch  # noqa: E402

BASE = "openai/whisper-large-v3-turbo"
# Architecture of whisper-large-v3-turbo, for --random-init timing runs without a download.
TURBO_SHAPE = dict(d_model=1280, encoder_layers=32, decoder_layers=4, encoder_attention_heads=20,
                   decoder_attention_heads=20, encoder_ffn_dim=5120, decoder_ffn_dim=5120, num_mel_bins=128,
                   vocab_size=51866, max_source_positions=1500, max_target_positions=448)


def pick_device() -> str:
    if torch.cuda.is_available():
        return "cuda"
    if torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def read_wav(path: Path) -> np.ndarray:
    with wave.open(str(path)) as w:
        return np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float32) / 32768


def load_split(data: Path, split: str) -> list[dict]:
    path = data / f"{split}.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines() if line] if path.exists() else []


class Batcher:
    """Turns manifest rows into (log-mel features, label ids) batches."""

    def __init__(self, data: Path, feature_extractor, tokenizer, start_token: int):
        self.data, self.fe, self.tok, self.start = data, feature_extractor, tokenizer, start_token

    def labels(self, text: str) -> list[int]:
        if self.tok is None:  # --random-init: any ids of a plausible length
            return [random.randrange(50000) for _ in range(max(8, len(text.split()) * 2))]
        ids = self.tok(text).input_ids  # <|startoftranscript|><|en|><|transcribe|><|notimestamps|> ... <|endoftext|>
        return ids[1:] if ids and ids[0] == self.start else ids  # the model adds the start token itself

    def __call__(self, rows: list[dict]) -> dict:
        audio = [read_wav(self.data / r["audio"]) for r in rows]
        feats = self.fe(audio, sampling_rate=16000, return_tensors="pt").input_features
        ids = [self.labels(r["text"]) for r in rows]
        width = max(len(i) for i in ids)
        labels = torch.full((len(ids), width), -100, dtype=torch.long)
        for k, i in enumerate(ids):
            labels[k, : len(i)] = torch.tensor(i)
        return {"input_features": feats, "labels": labels}


def batches(rows: list[dict], size: int, shuffle: bool):
    order = list(range(len(rows)))
    if shuffle:
        random.shuffle(order)
    for i in range(0, len(order), size):
        yield [rows[j] for j in order[i : i + size]]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", type=Path, required=True, help="speaker folder made by export.py")
    ap.add_argument("--out", type=Path, help="default: runs/<speaker>")
    ap.add_argument("--base", default=BASE)
    ap.add_argument("--language", default="en", help="'en' makes Whisper write Hinglish in Roman letters")
    ap.add_argument("--epochs", type=int, default=5)
    ap.add_argument("--batch", type=int, default=2)
    ap.add_argument("--accum", type=int, default=8, help="gradient accumulation: effective batch = batch x accum")
    ap.add_argument("--lr", type=float, default=3e-4)
    ap.add_argument("--warmup", type=float, default=0.1, help="share of steps spent ramping the learning rate up")
    ap.add_argument("--rank", type=int, default=32)
    ap.add_argument("--alpha", type=int, default=64)
    ap.add_argument("--lora-dropout", type=float, default=0.05)
    ap.add_argument("--targets", default="q_proj,k_proj,v_proj,out_proj",
                    help="layers that get LoRA, in both encoder and decoder")
    ap.add_argument("--patience", type=int, default=2, help="stop after this many epochs without a better val loss")
    ap.add_argument("--dtype", choices=["fp32", "bf16"], default="bf16",
                    help="frozen base weights on MPS/CPU (LoRA weights stay fp32); fp32 needs over 4 GB more")
    ap.add_argument("--max-mem-gb", type=float, default=12.0, help="MPS memory cap")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--dry-run", action="store_true", help="run --steps steps, report time and memory, save nothing")
    ap.add_argument("--steps", type=int, default=2)
    ap.add_argument("--random-init", action="store_true", help="with --dry-run: turbo-shaped random weights")
    args = ap.parse_args()

    from peft import LoraConfig, get_peft_model
    from transformers import (WhisperConfig, WhisperFeatureExtractor, WhisperForConditionalGeneration,
                              WhisperProcessor)

    random.seed(args.seed)
    torch.manual_seed(args.seed)
    device = pick_device()
    if device == "mps":
        recommended = torch.mps.recommended_max_memory()
        torch.mps.set_per_process_memory_fraction(min(1.0, args.max_mem_gb * 2**30 / recommended))

    train_rows, val_rows = load_split(args.data, "train"), load_split(args.data, "val")
    if not train_rows:
        sys.exit(f"no training clips in {args.data}")
    speaker = train_rows[0].get("speaker", args.data.name)
    out = args.out or Path(__file__).resolve().parent / "runs" / speaker

    load_dtype = torch.bfloat16 if args.dtype == "bf16" and device != "cuda" else torch.float32
    if args.random_init:
        if not args.dry_run:
            sys.exit("--random-init is for timing with --dry-run only")
        model = WhisperForConditionalGeneration(WhisperConfig(**TURBO_SHAPE)).to(load_dtype)
        processor, tokenizer = None, None
        fe = WhisperFeatureExtractor(feature_size=TURBO_SHAPE["num_mel_bins"])
    else:
        processor = WhisperProcessor.from_pretrained(args.base)
        processor.tokenizer.set_prefix_tokens(language=args.language, task="transcribe", predict_timestamps=False)
        tokenizer, fe = processor.tokenizer, processor.feature_extractor
        model = WhisperForConditionalGeneration.from_pretrained(args.base, torch_dtype=load_dtype)

    # SpecAugment: hide short random stretches of time and frequency while training.
    model.config.apply_spec_augment = True
    model.config.mask_time_prob, model.config.mask_time_length = 0.05, 10
    model.config.mask_feature_prob, model.config.mask_feature_length = 0.05, 10
    model.config.use_cache = False
    model.gradient_checkpointing_enable(gradient_checkpointing_kwargs={"use_reentrant": False})

    lora = LoraConfig(r=args.rank, lora_alpha=args.alpha, lora_dropout=args.lora_dropout,
                      target_modules=args.targets.split(","), bias="none")
    model = get_peft_model(model, lora).to(device)
    model.print_trainable_parameters()

    collate = Batcher(args.data, fe, tokenizer, model.config.decoder_start_token_id)
    params = [p for p in model.parameters() if p.requires_grad]
    optimizer = torch.optim.AdamW(params, lr=args.lr, weight_decay=0.01)
    steps_per_epoch = math.ceil(len(train_rows) / (args.batch * args.accum))
    total_steps = args.steps if args.dry_run else steps_per_epoch * args.epochs
    warmup = max(1, int(total_steps * args.warmup))
    schedule = torch.optim.lr_scheduler.LambdaLR(
        optimizer, lambda s: min((s + 1) / warmup, max(0.0, (total_steps - s) / max(1, total_steps - warmup))))

    use_amp = device == "cuda"
    amp_dtype = torch.bfloat16 if use_amp and torch.cuda.is_bf16_supported() else torch.float16
    scaler = torch.amp.GradScaler("cuda", enabled=use_amp and amp_dtype == torch.float16)

    def loss_of(batch: dict) -> torch.Tensor:
        feats = batch["input_features"].to(device, next(model.parameters()).dtype if not use_amp else torch.float32)
        with torch.autocast("cuda", dtype=amp_dtype, enabled=use_amp):
            return model(input_features=feats, labels=batch["labels"].to(device)).loss

    def val_loss() -> float | None:
        if not val_rows:
            return None
        model.eval()
        total = n = 0
        with torch.no_grad():
            for rows in batches(val_rows, args.batch, shuffle=False):
                total += loss_of(collate(rows)).item() * len(rows)
                n += len(rows)
        model.train()
        return total / n

    if args.dry_run:
        model.train()
        clip_sec = []
        for step in range(args.steps):
            rows = [train_rows[(step * args.batch + k) % len(train_rows)] for k in range(args.batch)]
            t0 = time.time()
            loss = loss_of(collate(rows))
            loss.backward()
            optimizer.step()
            optimizer.zero_grad(set_to_none=True)
            if device == "mps":
                torch.mps.synchronize()
            took = time.time() - t0
            clip_sec.append(took / args.batch)
            print(f"step {step}: loss {loss.item():.3f}, {took:.2f} s for {args.batch} clip(s)")
        peak = torch.mps.driver_allocated_memory() / 2**30 if device == "mps" else (
            torch.cuda.max_memory_allocated() / 2**30 if device == "cuda" else float("nan"))
        steady = clip_sec[-1]
        print(json.dumps({"device": device, "dtype": str(load_dtype), "batch": args.batch,
                          "sec_per_clip_steady": round(steady, 3), "gpu_mem_gb": round(peak, 2),
                          "note": "every clip is padded to 30 s, so cost is per clip, not per second"}))
        return

    out.mkdir(parents=True, exist_ok=True)
    (out / "config.json").write_text(json.dumps({k: str(v) for k, v in vars(args).items()}, indent=2))
    log = open(out / "train_log.jsonl", "a")
    best, stale, step = float("inf"), 0, 0
    before = val_loss()
    print(f"val loss before training: {before}")
    log.write(json.dumps({"epoch": 0, "val_loss": before}) + "\n")
    model.train()
    for epoch in range(1, args.epochs + 1):
        t0, running, seen = time.time(), 0.0, 0
        for i, rows in enumerate(batches(train_rows, args.batch, shuffle=True)):
            loss = loss_of(collate(rows))
            scaler.scale(loss / args.accum).backward()
            running, seen = running + loss.item() * len(rows), seen + len(rows)
            if (i + 1) % args.accum == 0 or seen == len(train_rows):
                scaler.unscale_(optimizer)
                torch.nn.utils.clip_grad_norm_(params, 1.0)
                scaler.step(optimizer)
                scaler.update()
                optimizer.zero_grad(set_to_none=True)
                schedule.step()
                step += 1
            if (i + 1) % 20 == 0:
                print(f"epoch {epoch} clip {seen}/{len(train_rows)} loss {running / seen:.3f} "
                      f"({(time.time() - t0) / seen:.2f} s/clip)")
        vl = val_loss()
        entry = {"epoch": epoch, "train_loss": running / seen, "val_loss": vl,
                 "minutes": round((time.time() - t0) / 60, 1), "lr": schedule.get_last_lr()[0]}
        print(json.dumps(entry))
        log.write(json.dumps(entry) + "\n")
        log.flush()
        score = vl if vl is not None else entry["train_loss"]
        if score < best:
            best, stale = score, 0
            model.save_pretrained(out / "adapter")
            processor.save_pretrained(out / "adapter")
        else:
            stale += 1
            if stale >= args.patience:
                print("val loss stopped improving; keeping the best epoch")
                break
    print(f"adapter saved in {out / 'adapter'}")


if __name__ == "__main__":
    main()
