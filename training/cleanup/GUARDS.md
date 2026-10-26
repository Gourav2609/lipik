# Clean-up rules and checks

The app code is the reference: `src-tauri/src/textflow/rules.rs` (rules step and
checks) and `pipeline.rs` (what is sent and what is pasted). `guards.py` mirrors it
for the benchmark and reads its word lists and limits from those files. The
examples live in one file, `src-tauri/src/textflow/rules_examples.json`; the Rust
tests (`cargo test --lib textflow`) and `python3 guards.py` both run every one of
them, so the two sides can't drift apart unnoticed.

## Where this sits

```
transcript
  │  filler step (transcriber, user's switch): uh, uhm, hmm ...; English um, ah, eh, erm
  │  dictionary replacements and snippets
  │
  ├─ fewer than 8 words ─────────────► apply(text) ───────────────────────────► paste
  │
  └─ 8+ words ─► apply(text) ─► model ─► check(text, reply) ─┬─ ok ──► reply ───► paste
                                                             └─ fail ─► apply(text) ► paste
```

- `apply` is deterministic and takes well under 1 ms. The model sees its output.
- `check` compares the reply with the text before `apply`, so nothing the rules
  step did can hide an invented word.
- An accepted reply is pasted as written when it ends with `.`, `!` or `?`; with no
  end mark it gets the question-mark rule.
- When the call fails, times out (4 s) or is rejected, the `apply` text is pasted.
- The vocabulary sent is only the dictionary words that look or sound like a word of
  the dictation (or 2-3 neighbouring words run together): equal, contained in it, or
  at most one edit per 3 letters away, words of 4+ letters; at most 40.

## 1. Words

`words(text)`: lowercase; `’` -> `'`; a comma between digits is removed when 3 digits
follow (`1,500` -> `1500`); tokens are runs of letters, combining marks, digits and
apostrophes, trimmed of apostrophes. So `rivaan_m@quorvia.com` -> `rivaan, m,
quorvia, com`, `3:30` -> `3, 30`, and a Devanagari word keeps its vowel signs.

## 2. Checks, in order

1. **TooLong**: the reply is longer than 1.5 x the text + 40 characters.
2. **NewScript**: the reply uses a script (Latin, Devanagari, other Indic scripts,
   Arabic, Hebrew, Cyrillic, Greek, Han, kana, Hangul, Thai) the text does not.
3. **HinglishLost**: the text has 2+ different Hindi words (`hindi.rs`, minus the
   ones that are also English: agar, mere, se, wale) and the reply keeps fewer than
   half of them.
4. **Added**: 2+ different reply words the speaker never said. Allowed: words of the
   text, of the vocabulary sent, 2-4 neighbouring words joined ("A P I" -> `api`),
   `pm`/`am`, digits for numbers said (number words combine: "two thousand four
   hundred and fifty" -> 2450; 2-12 said numbers may be written together: phone
   numbers, "twenty twenty seven" -> 2027; "oh" counts as zero), and the digit
   pieces of "8:15" / "2.4.1" when the joined digits were said.
5. **Dropped**: different words of the text missing from the reply, not counting
   fillers (the filler step's English list), spoken punctuation and address words,
   number words, digits, "like", "you know", "a", "and", and 1-2 letter words found
   inside the reply's words joined. The limit is 2 dropped words; when the text has a
   correction cue (`no, actually, i mean, scratch that, sorry, wait, nahi, meant`) it
   is 40% of the text's words, at least 3.

## 3. The rules step (`apply`)

In this order:

1. **Spoken punctuation**: `new paragraph`, `new line`/`newline`, `question mark`,
   `exclamation mark|point`, `full stop`, `comma`, `colon` become the mark, replacing
   the punctuation around them; `period` only at the very end or before a line
   break. Not after a determiner or possessive (a, an, the, this, that, my, our,
   your, his, her, its, their, some, any), and `colon` not before cancer, surgery,
   screening, polyp(s), cleanse, health, exam, biopsy.
2. **Number corrections**: `<number>[,.] no[,.] [sorry|wait] <number>` -> the second
   number in digits ("four, no, five" -> "5"). Numbers are digits or zero..ninety
   ("oh" is not one), and the kept number is never "one" ("no one").
3. **Emails and web addresses**: pieces joined by spoken `dot underscore dash at
slash` (or `/path`) are joined and lowercased when the result ends in a known
   domain (com org net io dev ai app co in uk me so xyz gg tech), has at most one
   `@` and not both `@` and `/`. Not when the span follows a determiner or starts
   with one. An ending that is also an English word (in so me co app ai it is us at
   to do go be) needs an `@`, a second dot or a `/`. "<common word> at ..." keeps
   "<word> at" and joins the rest.
4. **Times**: `3.30 p.m.` / `3:30 pm` -> `3:30 PM`; `5 p.m.` -> `5 PM`. A bare
   `1.10` stays.
5. **Vocabulary case**: a case-insensitive match of a dictionary word gets its
   spelling, not inside an address; an entry made only of common English words
   (`common_words.txt`) leaves an all-lowercase match alone.
6. **Spacing**: no space before `, . ? ! :`; one space after `, ? !` before a letter,
   except inside a web address; a run of marks keeps its first (`...` stays); `,.`
   -> `.`; no spaces around line breaks; single spaces; spaces, commas and line
   breaks trimmed at both ends.
7. **Capitals** at the start, after a line break and after `. ? !` + space (not
   after `...`).
8. **Question mark** on a one-sentence text that starts like a question after
   openers (hey, hi, hello, okay, ok, so, a capitalised name): `what's ... aren't`;
   a question word + helper verb ("what time is", "how much does"); a question word
   - one word + helper verb, unless that word is a subject pronoun ("which one
     should" yes, "when you have", "what we do" no); `can could ... has` + a pronoun.
     A final `.` or `!` becomes `?`; nothing changes after `...`.
9. An address at the very start keeps its lowercase first letter.

## 4. Measured effect

See `RESULTS.md` (Round 2 and Round 3).
