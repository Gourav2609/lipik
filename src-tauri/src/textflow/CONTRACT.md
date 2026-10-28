# Text pipeline contract (as implemented)

Rust lives in `src-tauri/src/textflow/`. Commands are tauri-specta commands, so
`bindings.ts` exposes them as `commands.<camelCase>` (for example
`commands.addDictionaryWord(word)`). Commands that return `Result` come back as
`{ status: "ok", data } | { status: "error", error }`.

## Where data is stored

| Data                                                  | File (app data folder)                               | Why                                                                                                                                                                                                                                                                             |
| ----------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dictionary, replacements, snippets, style, transforms | `textflow.json`                                      | Small, read on every dictation. Kept in memory and written (temp file + rename) on every change, so the dictation path never touches disk or parses JSON. Kept out of `settings_store.json` because `get_settings()` parses the whole settings object many times per dictation. |
| Scratchpad notes                                      | `scratchpad.db` (SQLite, table `notes`)              | Note bodies can be long and are saved often; one row per note.                                                                                                                                                                                                                  |
| Insights numbers                                      | `history.db`, new columns on `transcription_history` | Next to each dictation's audio and text.                                                                                                                                                                                                                                        |

On the first run `textflow.json` is created and Lipik's existing
`custom_words` are imported as dictionary words. A `textflow.json` that can't
be read or parsed is moved to `textflow.json.bad-<unix time>` and the words
are imported again from `custom_words`; nothing overwrites the bad file.

## Dictionary

| Command                                                              | Returns              |
| -------------------------------------------------------------------- | -------------------- |
| `get_dictionary()`                                                   | `Dictionary`         |
| `add_dictionary_word(word: string)`                                  | `Result<Dictionary>` |
| `update_dictionary_word(id: string, word: string, starred: boolean)` | `Result<Dictionary>` |
| `delete_dictionary_word(id: string)`                                 | `Result<Dictionary>` |
| `add_replacement(from: string, to: string)`                          | `Result<Dictionary>` |
| `update_replacement(id: string, from: string, to: string)`           | `Result<Dictionary>` |
| `delete_replacement(id: string)`                                     | `Result<Dictionary>` |

```ts
Dictionary = { words: DictionaryWord[]; replacements: Replacement[] }
DictionaryWord = { id: string; word: string; starred: boolean; auto_learned: boolean; shared: boolean }
Replacement = { id: string; from: string; to: string; auto_learned: boolean }
```

- New entries are added at the top of the list.
- Duplicates (same words, ignoring case and punctuation) are rejected with an
  error message ready to show. Phrases with symbols ("C#", "C++", ".NET")
  are compared on their exact characters, so "C#" and "C++" are different.
- Every change to `words` rewrites Lipik's `custom_words` setting, which the
  transcriber already uses (vocabulary prompt or fuzzy correction). The
  rewrite waits 300 ms so a bulk import writes the settings once.
- Replacements apply after transcription, whole words only, ignoring case.
  A phrase with symbols matches only those exact characters ("C#" never
  matches a bare "c", ".NET" never matches "net" or "ASP.NET").
  A match at a capitalised position keeps the capital ("Btw" -> "By the way").
- `shared` is always `false`: there is no team sharing.
- `auto_learned` is `true` on words and replacements learned from a History
  edit (`update_history_text`), and goes back to `false` when the user edits
  the entry's text. Learning compares the old shown text with the new one
  word by word and uses only spans where 1-4 words became 1-3 words; an edit
  that changes more than half of an 8+ word text learns nothing.
  - A new word is learned at once (at most 5 per edit, "'s" removed) unless
    it is a common English word (`common_words.txt`), shorter than 2 or
    longer than 40 characters, starts with a digit, is a URL or email, or is
    already in the dictionary. Words of 3 letters or fewer are learned only
    when written in capitals (acronyms), since the fuzzy word match would
    turn "hi" into a learned "hai".
  - A fix ("lip pick" -> "Lipik") becomes a replacement once two different
    dictations had it, never when the misheard side is one common English
    word or already has a replacement. Up to 200 waiting fixes are kept in
    `textflow.json` (`pending_fixes`).
  - Logs carry counts only.

## Snippets

| Command                                                          | Returns             |
| ---------------------------------------------------------------- | ------------------- |
| `get_snippets()`                                                 | `Snippet[]`         |
| `add_snippet(trigger: string, expansion: string)`                | `Result<Snippet[]>` |
| `update_snippet(id: string, trigger: string, expansion: string)` | `Result<Snippet[]>` |
| `delete_snippet(id: string)`                                     | `Result<Snippet[]>` |

`Snippet = { id: string; trigger: string; expansion: string }`

The trigger matches whole words, ignoring case and any punctuation the
transcriber put between its words ("my, email" matches "my email"). The
longest trigger wins. The expansion is inserted exactly as typed (line breaks
kept, clean-up never edits it). When the whole dictation is just the trigger,
only the expansion is pasted (no stray period).

## Style

| Command                                                | Returns                 |
| ------------------------------------------------------ | ----------------------- |
| `get_style_settings()`                                 | `StyleSettings`         |
| `set_style(category: StyleCategory, style: StyleKind)` | `Result<StyleSettings>` |
| `set_auto_cleanup(enabled: boolean)`                   | `Result<StyleSettings>` |

```ts
StyleCategory = "personal" | "work" | "email" | "other"
StyleKind = "formal" | "casual" | "very_casual"
StyleSettings = { personal: StyleKind; work: StyleKind; email: StyleKind; other: StyleKind; auto_cleanup: boolean }
```

Defaults: all `formal`, `auto_cleanup: true`. Stores saved by earlier builds,
where clean-up was off by default, get it switched on once at start-up
(`cleanup_default_applied` in `textflow.json` records that this was done, so
turning it off afterwards sticks; dev builds wrote it as
`cleanup_on_by_default`, which is still read).

The app that was in front when recording started decides the category
(bundle id, plus the window title for browsers):

| App type | Style category | Examples                                                                       |
| -------- | -------------- | ------------------------------------------------------------------------------ |
| personal | personal       | Messages, WhatsApp, Telegram, Discord, Signal; WhatsApp/Messenger/Discord tabs |
| work     | work           | Slack, Teams, Linear, Notion, Jira; matching browser tabs, GitHub              |
| email    | email          | Mail, Outlook, Spark, Superhuman; Gmail/Outlook/Inbox tabs                     |
| ai       | other          | ChatGPT, Claude, Cursor, Windsurf, Perplexity; their tabs                      |
| document | other          | Notes, Pages, Word, Obsidian, TextEdit; Google Docs tabs                       |
| other    | other          | everything else                                                                |

### How clean-up works

With `auto_cleanup` on, every dictation goes through two steps after
dictionary replacements and snippets. Fillers ("um", "uh", "hmm", English
"erm") are not part of clean-up: the transcriber's filler step removes them
before, following the user's filler switch and the output language ("um" is
Portuguese and German too; "mm" after a number and all-caps words like "ER"
stay). The research behind the steps is in `training/cleanup/`
(`RESULTS.md`, `GUARDS.md`).

1. **Rules** (`rules.rs`, under 1 ms, no model). Turns spoken punctuation
   into marks ("comma", "question mark", "new line", "new paragraph";
   "period" only at the end or before a line break; none of them right after
   "a", "the", "my" and the like, so "a new line of shoes" stays, nor
   "colon" before words like "surgery"); keeps only the corrected number in
   "four, no, five" (-> "5"; "no one" is never a correction); joins spoken
   emails and web addresses ("leo dot park at northwind dot co" ->
   "leo.park@northwind.co"; only with a known ending like .com or .io, not
   after "a"/"the", and an ending that is also an English word, like .in or
   .app, needs an "@", a second dot or a "/"); writes "3.30 p.m." as
   "3:30 PM" (a bare "1.10" stays); gives dictionary words their saved
   spelling (a dictionary word made of common English words, like "Linear",
   leaves an all-lowercase match alone); fixes spacing (not inside web
   addresses, so "search?q=x" keeps no space) and capitals; keeps "...";
   trims leading and trailing line breaks; and ends a one-sentence question
   with "?" (not "When you have time, ..." or "What we do is ...").
   For dictations under 8 words this is the whole clean-up.
2. **Which parts need the model** (`gate.rs`, under 1 ms), for 8 words or
   more in the formal style. The rules text is read sentence by sentence;
   a sentence ends at a line break, or at ".", "?" or "!" followed by a
   space or the end of the text (so "2.5" and "docs.google.com" don't end
   one). A sentence needs the model when it has:
   - a correction cue ("no", "nope", "sorry", "actually", "wait", "I mean",
     "I meant", "meant", "scratch that", "nahi", "rather", "make that";
     the checks below use the same list). When the sentence starts with
     one, maybe after one lead-in word ("Actually no, Friday works
     better.", "Oh wait, ...", "Okay, scratch that, ...", "Well,
     actually, ..."), the sentence before it goes too, and when it is only
     the cue ("Scratch that.", "No wait.") the sentence after it as well;
   - "you know" (not "do you know"), "like" used as a filler ("it was like
     really nice", ", like,"), "hum"/"hmm", or words said twice ("I I",
     "we should, we should");
   - spoken punctuation or address words the rules left ("period" in the
     middle, "dot", "slash");
   - a number to write as digits: a number word above ten, before a unit
     ("eight percent", "two hundred") or after "at"/"by" ("at eleven"),
     or a time said as one number ("by 815");
   - spelled-out letters ("a p i", "n variable");
   - a word that looks like a dictionary word but is spelled differently
     ("superbase" for Supabase, "graph ql" for GraphQL);
   - Hindi words (Hinglish);
   - more than 20 words with no comma (a run-on to split);
   - a question without "?" ("Can you check the logs.", "..., can you
     check.").
     A list ("number one", "bullet", "first ... second,", "1. ... 2.") sends
     the whole text. Neighbouring sentences go together. In a sentence of
     more than 16 words whose only problems are fillers, repeats, numbers,
     letters or dictionary words, only the comma-separated pieces around them
     go (one piece either side), as long as that is under 70% of the
     sentence. When a piece is put back, a reply whose first word is a
     common English word starts in lowercase if the piece did, and a piece
     without an end mark gets none (and no trailing comma).

   - Nothing needs the model: the rules text is pasted with a final period
     (when it ends in a word) and the model is not called.
   - The parts are 70% or more of the words: the whole rules text goes in
     one request, as before.
   - Otherwise each part goes in its own request, one after the other, and
     each reply is checked against its part (below); a rejected or failed
     part keeps its rules text. A sentence's reply without an end mark gets
     the question-mark rule, else the sentence's own end mark. All requests
     share one time limit (4 s, or 8 s when the dictation waits for the
     model, below). Parts that merge (a correction reaching back over
     pieces of the sentence before) become one part; if parts ever came
     out overlapping or out of order, the whole text is sent instead.

   Other styles (casual, very casual) change punctuation and capitals
   everywhere, so the whole text always goes to the model.

3. **Model**. The built-in engine or Ollama (below, "Clean-up engine").
   Ollama: the Notetaker's `ollama_url` /
   `ollama_model` settings, default `qwen3:8b` at `127.0.0.1:11434`,
   `think: false`, `temperature` 0.2, `keep_alive` 1 h, `num_predict`
   capped at the longest reply the checks accept: 1.5x the text + 40
   characters, at 3 characters per token for plain ASCII text and one
   token per byte otherwise (Devanagari, emoji), + 16; a reply Ollama
   stopped at the cap counts as a failed call) gets the rules text (or a
   part of it), not the raw transcript. The request is the instructions and the
   worked examples from `cleanup_prompt.txt` (the benchmark in
   `training/cleanup/` reads the same file), sent as a system message plus
   example user/assistant turns that are the same on every call, so Ollama
   keeps them cached. Only the last user message changes:

   ```
   App: Slack
   Vocabulary: <dictionary words that sound like a word of the dictation>
   Style: casual. ...          (only when the category's style is not formal)
   Keep markers like [[S0]] exactly as they are.   (only with snippets)
   <transcript>
   ...
   </transcript>
   ```

   A dictionary of up to 40 words is sent whole, because the model also
   fixes words the transcriber misheard too badly to match. A bigger one
   sends only the words that look or sound like a word of the dictation
   (or two or three neighbouring words run together): equal, contained in
   it, or at most one letter change per 3 letters away; at most 40, so the
   request stays within 4096 tokens. The line is left out when nothing is
   sent. The app name is the app's own name, with the
   kind of site for a browser tab ("Safari (Email)"); a History retry, which
   has no stored name, sends the app type ("Work chat"). `num_ctx` is 4096
   (the prompt is about 1,600 tokens), or 16384 (the Notetaker's size) for
   a dictation too long for that; a request uses the size the model is
   already loaded with when that is large enough, since a different size
   makes Ollama reload the model (about 3 s). The warm-up at recording
   start loads the model at the same size and sends the same request up to
   the words: instructions, examples, and the last message with the app
   that was in front when recording started, the dictionary (when it has 40
   words or fewer) and the style line, with "ok" as the transcript. Ollama
   then has all of it cached and the clean-up only reads the dictation
   (about 0.2 s less than a warm-up with another app name). The warm-up
   reads the front app without its window title, which can be slow, so a
   browser tab is named without its kind of site there and that part is
   read again at clean-up.

The checks run after dictionary replacements and snippets: the reply is
compared with that text (before the rules step), or for a part with the
part's rules text. The rules text is pasted instead when the reply:

- is empty, or much longer than that text (1.5x + 40 characters);
- uses a script the text does not (a Devanagari "ठीक है" for a spoken "ok");
- has lost most of the Hindi words of a Hinglish transcript (2 or more
  different words like hai, ko, ki, nahi, kya, and fewer than half kept):
  the model translated it;
- adds 2 or more words the speaker never said (allowed: punctuation and
  case changes, digits for spoken numbers, joined words like "A P I" ->
  "API", email and web address pieces, the dictionary words sent, "PM"/"AM");
- drops 3 or more of the speaker's words. Fillers, spoken punctuation and
  number words don't count. When the speaker corrected themselves (the
  text has one of the correction cues above), dropping words is expected,
  so the limit is 40% of
  the words (at least 3) instead; for a part, 40% of the words of the
  whole dictation, as a reply for the whole text would have.

Words are compared in any script (Devanagari words with their vowel signs
count as whole words). An accepted reply is pasted as written when it ends
with ".", "!" or "?"; one with no end mark gets the question-mark rule (a
piece from inside a sentence gets neither). When parts were sent, the
joined text gets a final period when it ends in a word. The
rules text is also pasted when the model is not loaded yet (a dictation
never waits for the first load; with an auto-apply transform on, or for
text that needs the larger size while the model is loaded smaller, it
waits, with the 8 s limit), or when Ollama errors or takes more than 4 s.
Trailing spaces at line ends of the model's reply (a Markdown line break)
are removed before snippets are put back, so snippet text is pasted
exactly.
Logs give the reason for a rejection, never the words; the clean-up time
is logged with the number of parts, the words sent and the reasons the
parts were sent.

The AI model setting (Settings → Meetings) is a free text field. Don't use
the plain `qwen3:4b` tag: on Ollama it is now a thinking model that ignores
`think: false` and takes 40-110 s. `qwen3:4b-instruct` works and is about
1.4 times as fast as `qwen3:8b`, with more misses (78% against 83% of the
benchmark's English clips, 60% against 70% of its long dictations).

### Clean-up engine (`engine.rs`)

The setting `cleanup_engine` (`"builtin"` or `"ollama"`, Settings →
Meetings → Clean-up engine) picks what runs the model, for clean-up,
transforms, meeting notes and Ask.

- **Built-in** (the default for new users): llama.cpp's `llama-server`
  (release b11935, MIT), built by `scripts/fetch-llama-server.sh` as one
  static file (Metal shaders inside, no web page, no HTTPS) and shipped next
  to the app binary as a tauri `externalBin`, signed with the app. It is a
  separate process because transcribe.cpp already links its own ggml and
  two copies in one process crash. It is offered only on macOS 13.3 or
  later (the helper's minimum); older Macs use Ollama. The model goes by
  the Mac's memory: 16 GB or more gets Qwen3-8B Q4_K_M
  (`Qwen/Qwen3-8B-GGUF` at `7c41481f`, 5.0 GB), less gets
  Qwen3-4B-Instruct-2507 Q4_K_M (`unsloth/Qwen3-4B-Instruct-2507-GGUF` at
  `a06e946b`, 2.5 GB), never plain Qwen3-4B (it thinks out loud). Files and
  SHA-256 are in `engine.rs`; they are saved in `models/cleanup/` in the
  app data folder.
  - The server starts on a free 127.0.0.1 port with `--ctx-size 4096`,
    `--parallel 1`, `--flash-attn auto`, prompt caching on, the host-memory
    prompt cache off, `--reasoning off`, `--offline`, `--alias
lipik-cleanup` (so `/v1/models` and `/props` name the alias, not the
    file path), a CORS origin no web page has, and a random API key (env
    `LLAMA_API_KEY`, sent as a bearer token). It counts as loaded once
    `GET /health` answers 200 (up to 120 s).
  - A request that needs more context (a long dictation, or meeting notes
    at 16384) restarts the server at that size. The restart cuts off a
    request still running on the smaller server (for example a warm-up), which
    then fails like any failed call.
  - It starts with the warm-up when a dictation starts recording, or on the
    first request that may wait; a request finds a dead server and starts a
    new one. A server that does not load (it exits, or `/health` never
    answers) is not started again for 5 minutes: requests fail at once in
    that time. It stops after 1 h without requests (like Ollama's
    keep-alive), when the user switches to Ollama, and when the app quits;
    at start-up the app stops servers an earlier crash left running (same
    binary path, parent gone).
  - Requests: `POST /v1/chat/completions` with the same messages,
    `temperature` 0.2, `max_tokens` = the `num_predict` cap above,
    `cache_prompt: true` and
    `chat_template_kwargs: {enable_thinking: false}`; a reply with
    `finish_reason: "length"` under a cap is a failed call, and a `<think>`
    block is removed before anything else reads the reply. Notes and Ask
    add `response_format: {type: "json_object"}`.
- **Ollama** (advanced): today's path, unchanged. Switching to the built-in
  engine asks Ollama to unload its model (`keep_alive: 0`).
- No setting yet (first start of this version): requests go to Ollama until
  the built-in model file is there, so people who already use Ollama never
  lose clean-up (without Ollama the request fails at once and the rules
  text is pasted). Onboarding and Settings offer the built-in engine.
  While the setting is unset, Ollama is asked every 30 s: Ollama with the
  clean-up model (not plain `qwen3:4b`) and no built-in model saves Ollama
  as the choice; a downloaded built-in model saves built-in. Nothing is
  saved while a built-in download runs.

### Setting up the model (`cleanup_model.rs`)

Onboarding (the Clean-up step) and Settings → Meetings use these:

| Command                                     | Returns              |
| ------------------------------------------- | -------------------- |
| `cleanup_model_status()`                    | `CleanupModelStatus` |
| `pull_cleanup_model()`                      | `Result<boolean>`    |
| `cancel_cleanup_model_pull()`               | nothing              |
| `set_cleanup_engine(engine: CleanupEngine)` | nothing              |

`CleanupModelStatus` has the `engine`, whether this build has the built-in
engine (`builtin_available`), the `state`, the `model` name, the download
`progress` and, for the built-in engine, the download `size` in bytes.

- Built-in: `downloading` while a download runs, `verifying` while its
  SHA-256 is checked (about 10 s for 5 GB), else `ready` when the model
  file is there, else `no_model`. The download uses the app's
  resumable downloader (`managers/model/download.rs`) from the pinned
  Hugging Face URL into `<file>.partial`: a cancel or a broken connection
  keeps the partial file and the next download continues it; the size and
  SHA-256 are checked before it is renamed. It needs the model's size plus
  1 GB of free disk. Progress comes as the same `cleanup-model-pull`
  events.
- Ollama: `downloading` while a download runs (with its progress and the
  model being downloaded); otherwise `GET {ollama_url}/api/tags` with a
  1.5 s limit: no answer = `no_ollama`, plain `qwen3:4b` = `slow_model`,
  the model listed = `ready` (a name without a tag means `:latest`), else
  `no_model`. The UI checks again every 3 s, and every 30 s once `ready`.
- Download: `POST {ollama_url}/api/pull {model, name, stream: true}` (older
  Ollama reads `name`) for the configured model. Progress is summed over
  the model's files and sent as `cleanup-model-pull` events
  (`{completed, total}` bytes) when it moves by 0.1%. It refuses plain
  `qwen3:4b`. An `error` line, a broken connection or 60 s without a line
  is an error; on `success` the model is warmed up when clean-up or
  auto-apply is on. `Ok(false)` means cancelled; cancelling closes the
  connection to Ollama. Saving a different model in Settings cancels the
  download. The download keeps going if the user leaves the screen.

With auto-apply on too, the transform runs after clean-up as a separate
call (below), so a dictation can wait for two model calls: up to 8 s for
clean-up (it waits for the model when a transform is on) plus up to 8 s for
the transform.

## Transforms

| Command                                                                                                                 | Returns                     |
| ----------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| `get_transforms()`                                                                                                      | `TransformSettings`         |
| `save_transform(id: string \| null, name: string, description: string, prompt: string, shortcut_index: number \| null)` | `Result<TransformSettings>` |
| `delete_transform(id: string)`                                                                                          | `Result<TransformSettings>` |
| `reset_transforms()`                                                                                                    | `Result<TransformSettings>` |
| `set_transforms_opt_in(enabled: boolean)`                                                                               | `Result<TransformSettings>` |
| `set_auto_apply(enabled: boolean, transform_id: string \| null)`                                                        | `Result<TransformSettings>` |

```ts
TransformSettings = { opt_in: boolean; auto_apply: AutoApply; items: Transform[] }
AutoApply = { enabled: boolean; transform_id: string | null }
Transform = { id: string; name: string; description: string; prompt: string; shortcut_index: number | null } // 1..9 = ⌥1..⌥9
```

- Defaults: `opt_in: false`, auto-apply off, items Clear writing (`id: "polish"`, ⌥1)
  and Prompt writer (`id: "prompt_engineer"`, ⌥2).
- `save_transform` with `id: null` creates one; with no shortcut it gets the
  first free number. Giving a shortcut another transform has moves it.
  `reset_transforms` restores the two defaults and turns auto-apply off.
- While `opt_in` is on, ⌥n is registered as a global shortcut for each
  transform with a shortcut. Pressing it: waits for the keys to be released,
  sends ⌘C, runs the transform on the copied selection via Ollama (60 s
  limit, processing pill shown), pastes the result over the selection, then
  puts the user's clipboard back (text, or an image when there was no text).
- Auto-apply during dictation has an 8 s limit; its output is dropped (the
  text pastes without it) when it is empty or more than 4x + 400 characters
  longer than the input.
- Event `transform-status`: `{ state: "running" | "done" | "no_selection" | "error"; name: string; error: string | null }`.

## Scratchpad

| Command                                                      | Returns                         |
| ------------------------------------------------------------ | ------------------------------- |
| `list_notes()`                                               | `Result<Note[]>` (newest first) |
| `get_note(id: string)`                                       | `Result<Note \| null>`          |
| `save_note(id: string \| null, title: string, body: string)` | `Result<string>` (the id)       |
| `delete_note(id: string)`                                    | `Result<null>`                  |

`Note = { id: string; title: string; body: string; updated_at: number }` (unix ms).
`save_note` keeps a caller-chosen id, so notes already in the page's
localStorage can be moved over with their ids.

## Insights

`get_insights()` -> `Result<Insights>`

```ts
Insights = {
  wpm: number | null;          // words per minute of recorded audio; null until a dictation has a duration
  total_words: number;
  fixes_total: number;         // words_corrected + dictionary_fixes
  words_corrected: number;     // word edits made by clean-up and transforms
  dictionary_fixes: number;    // replacements applied + dictionary words in the final text
  by_category: { [appType: string]: number }; // dictations per app type: ai, email, work, personal, document, other (all keys present)
  apps_used: number;           // distinct app bundle ids
  streak_days: number;         // consecutive days with words, ending today (or yesterday)
  longest_streak: number;
  daily_words: { date: string; words: number }[]; // local days, "YYYY-MM-DD", oldest first
}
```

Each new history row also stores `app_bundle_id`, `category`, `app_type`,
`words`, `duration_ms`, `fixes`, `dictionary_fixes` (history migration 5).
`fixes` is measured from the text after replacements and snippets to the
final text, so it does not count dictionary fixes twice. Rows from before
this change count toward words and streaks only. When the final text differs
from the raw transcript it is stored in `post_processed_text`. When a model
step will run, the row is saved with the raw transcript before it starts and
updated afterwards, so a cancel during the wait still keeps the transcript.
Retrying a history entry re-runs the pipeline with the entry's stored app
(not the app in front now) and refreshes its numbers.

## Recording retention

`RecordingRetentionPeriod` gains `"forever"` (keep everything; same effect as
the old `"never"`, which meant "never auto-delete"). It is the default for new
installs. Settings schema 3 moves stores on the old default
(`preserve_limit` with `history_limit` 5) to `"forever"` once.
`update_recording_retention_period("forever")` is accepted.

## Pill menu

| Command                          | What it does                                                                                                                                                                 |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pill_hide_for(minutes: number)` | Hides the pill window for that long (0 shows it now). Emits `pill-hidden`. A dictation in that time still shows the pill, which hides again 400 ms after the dictation ends. |
| `pill_hidden_until()`            | `number \| null`: unix ms the pill is hidden until.                                                                                                                          |
| `paste_last_transcript()`        | `Result<null>`: pastes the latest non-empty transcript (final text) into the focused app.                                                                                    |
| `open_main_page(page: string)`   | `Result<null>`: shows the main window and emits `flow-open-page` (payload: page id) to it.                                                                                   |
| microphones                      | Use Lipik's existing `get_available_microphones()` and `set_selected_microphone(device_name)` (`"default"` = system default).                                                |

Events:

- `pill-mic` `{ name: string; recommended: boolean }`: on every recording
  start. `recommended` is true for the built-in microphone, whose name is
  then given as "Built-in mic".
- `pill-hidden` `number | null`: unix ms the pill is hidden until, or null when shown again.
- `textflow-changed` `"dictionary" | "snippets" | "style" | "transforms"`: after any change.

## Order after transcription

replacements -> snippets -> (the built-in post-process, only for the
post-process shortcut) or style clean-up (if on) -> transform auto-apply (if
on) -> history -> paste / popup.

With clean-up and auto-apply off, the added work before the paste is the
replacement and snippet pass on in-memory data (measured: 0.3-0.5 ms for a
100-word dictation with 50 replacements and 30 snippets in a debug build).
The Insights numbers (dictionary word counts, word diff) are computed on a
background thread after the history row is saved.
