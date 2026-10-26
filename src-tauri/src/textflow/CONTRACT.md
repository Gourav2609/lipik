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

On the first run `textflow.json` is created and Handy's existing
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
- Every change to `words` rewrites Handy's `custom_words` setting, which the
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
2. **Model**, for 8 words or more. Ollama (the Notetaker's `ollama_url` /
   `ollama_model` settings, default `qwen3:8b` at `127.0.0.1:11434`,
   `think: false`, `temperature` 0.2, `keep_alive` 1 h) gets the rules
   text, not the raw transcript. The request is the instructions and the
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
   start loads the model at the same size and sends the same instructions
   and examples.

The checks run after dictionary replacements and snippets: the reply is
compared with that text (before the rules step). The rules text is pasted
instead when the reply:

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
  text has "no", "actually", "I mean", "scratch that", "sorry", "wait",
  "nahi" or "meant"), dropping words is expected, so the limit is 40% of
  the words (at least 3) instead.

Words are compared in any script (Devanagari words with their vowel signs
count as whole words). An accepted reply is pasted as written when it ends
with ".", "!" or "?"; one with no end mark gets the question-mark rule. The
rules text is also pasted when the model is not loaded yet (a dictation
never waits for the first load; with an auto-apply transform on, or for
text that needs the larger size while the model is loaded smaller, it
waits, with the 8 s limit), or when Ollama errors or takes more than 4 s.
Logs give the reason for a rejection, never the words.

The Summary model setting is a free text field. Don't use the plain
`qwen3:4b` tag: on Ollama it is now a thinking model that ignores
`think: false` and takes 40-110 s. `qwen3:4b-instruct` works and is about
40% faster than `qwen3:8b`, with slightly more misses.

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
| microphones                      | Use Handy's existing `get_available_microphones()` and `set_selected_microphone(device_name)` (`"default"` = system default).                                                |

Events:

- `pill-mic` `{ name: string; recommended: boolean }`: on every recording
  start. `recommended` is true for the built-in microphone, whose name is
  then given as "Built-in mic".
- `pill-hidden` `number | null`: unix ms the pill is hidden until, or null when shown again.
- `textflow-changed` `"dictionary" | "snippets" | "style" | "transforms"`: after any change.

## Order after transcription

replacements -> snippets -> (Handy post-process, only for the
post-process shortcut) or style clean-up (if on) -> transform auto-apply (if
on) -> history -> paste / popup.

With clean-up and auto-apply off, the added work before the paste is the
replacement and snippet pass on in-memory data (measured: 0.3-0.5 ms for a
100-word dictation with 50 replacements and 30 snippets in a debug build).
The Insights numbers (dictionary word counts, word diff) are computed on a
background thread after the history row is saved.
