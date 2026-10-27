"""Draft references for the private set, and the page to correct them.

    python3 make_refs.py

There are no human-checked transcripts yet, so each clip gets a DRAFT
reference built from the models that ran on it:
  - Apple en-IN and Parakeet (Lipik's raw text) give the same words
    (after normalising): keep Parakeet's text.
  - They differ: take the one of Parakeet, Apple and Qwen3-ASR 1.7B that is
    closest in words to the other two. On a tie Qwen3-ASR wins, then Parakeet.
If Qwen3-ASR has not run on a disagreeing clip yet, the clip id is written to
private/need_third.txt; run `run.py --set private --contender qwen3-asr-1.7b
--ids private/need_third.txt`, then this script again.

Every reference is written with "checked": false. A reference already marked
"checked": true in private/references.json is kept as it is.

Writes private/references.json and private/review.html.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE.parent))
from textnorm import edit_distance, normalize_wer  # noqa: E402

PRIVATE = HERE / "private"
RUNS = PRIVATE / "runs"
MODELS = {"parakeet": "private-lipik.jsonl", "apple": "private-apple-en-IN.jsonl", "qwen": "private-qwen3-asr-1.7b.jsonl"}


def load(name):
    """Text per clip id; a clip whose last run failed maps to None."""
    path = RUNS / name
    if not path.exists():
        return {}
    rows = {}
    for line in path.read_text().splitlines():
        r = json.loads(line)
        rows[r["id"]] = None if "error" in r else r["text"].strip()
    return rows


def words(s):
    return normalize_wer(s).split()


def pick(texts):
    """The text with the smallest summed word distance to the others; ties go to the earlier key."""
    keys = list(texts)
    cost = {k: sum(edit_distance(words(texts[k]), words(texts[o])) for o in keys if o != k) for k in keys}
    best = min(keys, key=lambda k: cost[k])
    return best, texts[best]


def main():
    clips = json.loads((PRIVATE / "clips.json").read_text())
    hyps = {k: load(v) for k, v in MODELS.items()}
    old = {}
    if (PRIVATE / "references.json").exists():
        old = {r["id"]: r for r in json.loads((PRIVATE / "references.json").read_text())}

    refs, need_third, missing = [], [], []
    for c in clips:
        cid = c["id"]
        if old.get(cid, {}).get("checked"):
            refs.append(old[cid])
            continue
        p, a, q = hyps["parakeet"].get(cid), hyps["apple"].get(cid), hyps["qwen"].get(cid)
        qwen_failed = cid in hyps["qwen"] and q is None
        if p is None or a is None:
            missing.append(cid)
            continue
        if words(p) == words(a):
            text, how = p, "Apple and Parakeet agree; Parakeet's text"
        elif qwen_failed:
            text, how = p, "Apple and Parakeet differ; Qwen3-ASR failed on it, Parakeet's text"
        elif q is None:
            need_third.append(cid)
            text, how = p, "Apple and Parakeet differ; no third model yet, Parakeet's text"
        else:
            name, text = pick({"qwen": q, "parakeet": p, "apple": a})
            how = f"Apple and Parakeet differ; closest of three: {name}"
        refs.append({"id": cid, "category": c["category"], "text": text, "checked": False, "how": how,
                     "draft_from": {"parakeet": p, "apple": a, "qwen3-asr-1.7b": q}})

    (PRIVATE / "need_third.txt").write_text("".join(f"{i}\n" for i in need_third))
    (PRIVATE / "references.json").write_text(json.dumps(refs, indent=1, ensure_ascii=False))
    write_review(refs)
    hows = {}
    for r in refs:
        key = r.get("how", "checked by hand").split(";")[-1].strip()
        hows[key] = hows.get(key, 0) + 1
    print(f"{len(refs)} references ({sum(r.get('checked', False) for r in refs)} checked by hand)")
    for k, v in sorted(hows.items(), key=lambda kv: -kv[1]):
        print(f"  {v:4}  {k}")
    if need_third:
        print(f"{len(need_third)} clips still need the third model: see private/need_third.txt")
    if missing:
        print(f"{len(missing)} clips skipped: Parakeet or Apple has not run on them yet")


PAGE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Reference Review</title>
<style>
:root { --bg:#fbf9f4; --fg:#1d1d1b; --muted:#6b6b66; --line:#e3dfd5; --accent:#285a44; --card:#ffffff; }
@media (prefers-color-scheme: dark) { :root { --bg:#151614; --fg:#ecebe6; --muted:#9a9a93; --line:#2c2e2a; --accent:#7fbf9f; --card:#1c1d1b; } }
* { box-sizing:border-box; }
body { margin:0; background:var(--bg); color:var(--fg); font:15px/1.45 -apple-system, system-ui, sans-serif; }
header { position:sticky; top:0; background:var(--bg); border-bottom:1px solid var(--line); padding:12px 16px; display:flex; gap:12px; align-items:center; flex-wrap:wrap; z-index:1; }
header h1 { font-size:17px; margin:0 12px 0 0; }
button, select { font:inherit; padding:6px 12px; border-radius:6px; border:1px solid var(--line); background:var(--card); color:var(--fg); cursor:pointer; }
button.primary { background:var(--accent); color:#fff; border-color:var(--accent); }
main { max-width:900px; margin:0 auto; padding:16px; }
.clip { background:var(--card); border:1px solid var(--line); border-radius:10px; padding:12px 14px; margin-bottom:12px; }
.clip.checked { border-color:var(--accent); }
.meta { display:flex; gap:10px; align-items:center; color:var(--muted); font-size:13px; flex-wrap:wrap; }
.meta b { color:var(--fg); }
audio { width:100%; margin:8px 0; }
textarea { width:100%; min-height:60px; font:inherit; padding:8px; border-radius:6px; border:1px solid var(--line); background:var(--bg); color:var(--fg); resize:vertical; }
.hints { margin-top:6px; font-size:13px; color:var(--muted); }
.hints div { display:flex; gap:8px; align-items:baseline; margin-top:3px; }
.hints span.who { min-width:90px; }
.hints button { padding:1px 8px; font-size:12px; }
label.chk { display:flex; gap:6px; align-items:center; margin-left:auto; color:var(--fg); }
#count { color:var(--muted); font-size:13px; }
</style></head>
<body>
<header>
 <h1>Reference review</h1>
 <select id="filter"><option value="">All clips</option><option value="dictation">Dictations</option>
  <option value="meeting-me">Meetings: me</option><option value="meeting-others">Meetings: others</option>
  <option value="unchecked">Not checked yet</option></select>
 <span id="count"></span>
 <button class="primary" id="save">Save</button>
</header>
<main>
<p>Each box holds a <b>draft</b> reference made by speech models, not by a person. Listen, fix the text so it is
exactly what you would want typed (words, capitals and punctuation), then tick "checked". Edits are kept in this
browser while you work. Save downloads <code>references.json</code>: move it to
<code>training/benchmark/private/references.json</code> and run <code>python3 score.py</code>.</p>
<div id="list"></div>
</main>
<script>
const DATA = __DATA__;
const KEY = "lipik-ref-review";
let saved = {};
try { saved = JSON.parse(localStorage.getItem(KEY) || "{}"); } catch (e) { saved = {}; }
for (const r of DATA) if (saved[r.id]) Object.assign(r, saved[r.id]);
function remember(r) {
  saved[r.id] = { text: r.text, checked: r.checked };
  try { localStorage.setItem(KEY, JSON.stringify(saved)); } catch (e) {}
}
const list = document.getElementById("list");
function esc(s) { const d = document.createElement("div"); d.textContent = s == null ? "" : s; return d.innerHTML.replace(/"/g, "&quot;"); }
function render() {
  const f = document.getElementById("filter").value;
  list.innerHTML = "";
  let shown = 0;
  for (const r of DATA) {
    if (f === "unchecked" ? r.checked : (f && r.category !== f)) continue;
    shown++;
    const el = document.createElement("div");
    el.className = "clip" + (r.checked ? " checked" : "");
    const hints = Object.entries(r.draft_from || {}).filter(([, t]) => t != null)
      .map(([who, t]) => `<div><span class="who">${esc(who)}</span><button data-t="${esc(t)}">use</button><span>${esc(t) || "<i>(nothing)</i>"}</span></div>`).join("");
    el.innerHTML = `<div class="meta"><b>${esc(r.id)}</b><span>${esc(r.category)}</span><span>${esc(r.how || "")}</span>
      <label class="chk"><input type="checkbox" ${r.checked ? "checked" : ""}> checked</label></div>
      <audio controls preload="none" src="audio/${encodeURIComponent(r.id)}.wav"></audio>
      <textarea>${esc(r.text)}</textarea><div class="hints">${hints}</div>`;
    const ta = el.querySelector("textarea"), cb = el.querySelector("input");
    ta.addEventListener("input", () => { r.text = ta.value; if (!r.checked) { r.checked = true; cb.checked = true; el.classList.add("checked"); } remember(r); count(); });
    cb.addEventListener("change", () => { r.checked = cb.checked; el.classList.toggle("checked", cb.checked); remember(r); count(); });
    el.querySelectorAll(".hints button").forEach(b => b.addEventListener("click", () => { ta.value = b.dataset.t; ta.dispatchEvent(new Event("input")); }));
    list.appendChild(el);
  }
  count(shown);
}
function count(shown) {
  const done = DATA.filter(r => r.checked).length;
  document.getElementById("count").textContent = `${done} of ${DATA.length} checked`;
}
document.getElementById("filter").addEventListener("change", () => render());
document.getElementById("save").addEventListener("click", () => {
  const blob = new Blob([JSON.stringify(DATA, null, 1)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob); a.download = "references.json"; a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
});
render();
</script>
</body></html>
"""


def write_review(refs):
    data = json.dumps(refs, ensure_ascii=False).replace("</", "<\\/")
    (PRIVATE / "review.html").write_text(PAGE.replace("__DATA__", data))


if __name__ == "__main__":
    main()
