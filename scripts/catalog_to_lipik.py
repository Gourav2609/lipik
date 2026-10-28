"""
Point the bundled catalog at Lipik's own copies of the models on Hugging Face.

For every model, look up `lipik/<repo name>` (the copies made by
copy_models_to_hf.py). When that repo holds the model's default-quant file with
the catalog's sha256, the model's `repository` becomes `lipik/<repo name>` and
its `revision` that repo's current commit. Models without a verified copy are
dropped, together with their benchmark `results`.

The file is edited as text, so its formatting stays as it is. Reruns are safe:
models already on `lipik/` are checked again and keep or update their pin.

Run:  python3 scripts/catalog_to_lipik.py [--keep-missing] [--dry-run]
  --keep-missing  leave models without a copy yet untouched instead of
                  dropping them (for runs while the copy job is still going)
"""

import argparse
import json
import re
import urllib.error
import urllib.request
from pathlib import Path

ORG = "lipik"
CATALOG = Path(__file__).parent.parent / "src-tauri/src/catalog/catalog.json"
BLOCK_END = re.compile(r"    \},?")


def lipik_copy(model):
    """The lipik repo and its commit when it holds the default file, else None."""
    repo = f"{ORG}/{model['repository'].split('/', 1)[1]}"
    try:
        with urllib.request.urlopen(
            f"https://huggingface.co/api/models/{repo}?blobs=true", timeout=30
        ) as response:
            info = json.load(response)
    except urllib.error.HTTPError as e:
        # Hugging Face answers 401 for a repo that doesn't exist.
        if e.code in (401, 404):
            return None
        raise
    file = model["files"][model["quant"]]
    for sibling in info.get("siblings", []):
        if sibling["rfilename"] == file["filename"]:
            if (sibling.get("lfs") or {}).get("sha256") == file["sha256"]:
                return repo, info["sha"]
    return None


def remove_block(lines, first):
    """Delete the 4-space-indented array/object element starting at line `first`."""
    last = next(j for j in range(first + 1, len(lines)) if BLOCK_END.fullmatch(lines[j]))
    if not lines[last].endswith(","):
        lines[first - 1] = lines[first - 1].rstrip(",")
    del lines[first : last + 1]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep-missing", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    text = CATALOG.read_text()
    expected = json.loads(text)
    lines = text.split("\n")
    moved = kept = 0
    dropped = []

    for model in json.loads(text)["models"]:
        copy = lipik_copy(model)
        if copy is None:
            if args.keep_missing:
                print(f"wait  {model['id']} (no copy on {ORG}/ yet)")
            else:
                print(f"drop  {model['id']}")
                dropped.append(model["id"])
            continue
        repo, revision = copy
        if (model["repository"], model["revision"]) == (repo, revision):
            kept += 1
            continue
        print(f"move  {model['id']} -> {repo}@{revision[:8]}")
        moved += 1
        repo_line = lines.index(f'      "repository": "{model["repository"]}",')
        if lines[repo_line + 1] != f'      "revision": "{model["revision"]}",':
            raise SystemExit(f"{model['id']}: revision doesn't follow repository")
        lines[repo_line] = f'      "repository": "{repo}",'
        lines[repo_line + 1] = f'      "revision": "{revision}",'
        target = next(m for m in expected["models"] if m["id"] == model["id"])
        target["repository"], target["revision"] = repo, revision

    for model_id in dropped:
        first = lines.index(f"      \"id\": {json.dumps(model_id)},") - 1
        if lines[first] != "    {":
            raise SystemExit(f"{model_id}: unexpected layout")
        remove_block(lines, first)
        expected["models"] = [m for m in expected["models"] if m["id"] != model_id]
        if model_id in expected.get("results", {}):
            remove_block(lines, lines.index(f"    {json.dumps(model_id)}: {{"))
            del expected["results"][model_id]

    detector = expected.get("languageDetector")
    if detector and all(m["id"] != detector for m in expected["models"]):
        print(f"warning: languageDetector {detector} was dropped")

    new_text = "\n".join(lines)
    if json.loads(new_text) != expected:
        raise SystemExit("edited catalog doesn't match the intended changes; nothing written")
    print(
        f"{moved} moved, {kept} already on {ORG}/, {len(dropped)} dropped, "
        f"{len(expected['models'])} left"
    )
    if not args.dry_run and new_text != text:
        CATALOG.write_text(new_text)
        print(f"wrote {CATALOG}")


if __name__ == "__main__":
    main()
