"""Fold a LoRA adapter into its base Whisper model and, optionally, convert the
result to a whisper.cpp .bin that Lipik loads from its models folder.

    python merge.py --adapter runs/me/adapter --out models/me-hf
    python merge.py --adapter runs/me/adapter --out models/me-hf \\
        --whisper-cpp ~/src/whisper.cpp --openai-whisper ~/src/whisper \\
        --quant q5_0 --install whisper-turbo-me

Lipik's engine (transcribe.cpp) has no LoRA support, so every person gets
a full model file. --install copies it into Lipik's models folder, where the
app lists any .bin / .gguf file as a custom model.
"""

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

import torch

LIPIK_MODELS = Path.home() / "Library/Application Support/com.gourav.lipik/models"
TOKENIZER_FILES = ["vocab.json", "added_tokens.json", "merges.txt", "normalizer.json", "tokenizer.json",
                   "tokenizer_config.json", "special_tokens_map.json", "preprocessor_config.json",
                   "generation_config.json"]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--adapter", type=Path, required=True)
    ap.add_argument("--base", help="default: the base recorded in the adapter")
    ap.add_argument("--out", type=Path, required=True, help="merged Hugging Face model folder")
    ap.add_argument("--whisper-cpp", type=Path, help="whisper.cpp checkout (for convert-h5-to-ggml.py, whisper-quantize)")
    ap.add_argument("--openai-whisper", type=Path, help="github.com/openai/whisper checkout (mel filter file)")
    ap.add_argument("--quant", default="q5_0", help="whisper.cpp quant type, or 'f16' to skip quantising")
    ap.add_argument("--install", metavar="NAME", help="copy the .bin into Lipik's models folder as NAME.bin")
    args = ap.parse_args()

    from huggingface_hub import snapshot_download
    from peft import PeftConfig, PeftModel
    from transformers import WhisperForConditionalGeneration

    base = args.base or PeftConfig.from_pretrained(args.adapter).base_model_name_or_path
    model = WhisperForConditionalGeneration.from_pretrained(base, torch_dtype=torch.float32)
    model = PeftModel.from_pretrained(model, args.adapter).merge_and_unload()
    model.config.apply_spec_augment = False
    model.config.use_cache = True
    model.to(torch.float16).save_pretrained(args.out)

    # LoRA does not touch the tokenizer; copy the base files verbatim, since the
    # whisper.cpp converter reads vocab.json and added_tokens.json directly.
    base_dir = Path(base) if Path(base).exists() else Path(snapshot_download(base, allow_patterns=TOKENIZER_FILES))
    for name in TOKENIZER_FILES:
        if (base_dir / name).exists():
            shutil.copy(base_dir / name, args.out / name)
    print(f"merged model: {args.out}")

    if not args.whisper_cpp:
        return
    if not args.openai_whisper:
        sys.exit("--openai-whisper is needed for the conversion")
    ggml_dir = args.out / "ggml"
    ggml_dir.mkdir(exist_ok=True)
    subprocess.run([sys.executable, str(args.whisper_cpp / "models/convert-h5-to-ggml.py"),
                    str(args.out), str(args.openai_whisper), str(ggml_dir)], check=True)
    result = ggml_dir / "ggml-model.bin"  # f16
    if args.quant != "f16":
        quantized = ggml_dir / f"ggml-model-{args.quant}.bin"
        subprocess.run([str(args.whisper_cpp / "build/bin/whisper-quantize"), str(result), str(quantized), args.quant],
                       check=True)
        result = quantized
    print(f"whisper.cpp model: {result} ({result.stat().st_size / 2**20:.0f} MB)")

    if args.install:
        LIPIK_MODELS.mkdir(parents=True, exist_ok=True)
        target = LIPIK_MODELS / f"{args.install}.bin"
        shutil.copy(result, target)
        print(f"installed: {target} (pick it in Lipik's model list; set language to English)")


if __name__ == "__main__":
    main()
