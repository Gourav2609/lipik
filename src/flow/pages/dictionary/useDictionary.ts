import { useCallback, useEffect, useMemo, useState } from "react";
import { commands, type Dictionary } from "@/bindings";
import {
  compareIdsNewestFirst,
  errorText,
  onTextflowChanged,
  unwrap,
} from "../textflow";

export interface DictionaryEntry {
  key: string;
  id: string;
  word: string;
  replacement?: string;
  starred: boolean;
  learned: boolean;
}

export type SortMode = "recent" | "alpha";

export const normalizeWord = (word: string) =>
  word
    .replace(/[<>"']/g, "")
    .replace(/\s+/g, " ")
    .trim();

export const MAX_WORD_LENGTH = 50;

export interface EntryInput {
  word: string;
  replacement?: string;
}

const EMPTY: Dictionary = { words: [], replacements: [] };

// Replacements have no starred flag in the backend, so their stars stay in this browser.
const STARS_KEY = "lipik.dictionary.starredReplacements";
// Words, replacements and stars the page kept here before the backend stored them.
const OLD_META_KEY = "lipik.dictionary.v1";

const readStars = (): string[] => {
  try {
    const raw = localStorage.getItem(STARS_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed)
      ? parsed.filter((v) => typeof v === "string")
      : [];
  } catch {
    return [];
  }
};

const saveStars = (ids: string[]) => {
  try {
    localStorage.setItem(STARS_KEY, JSON.stringify(ids));
  } catch {
    // Not persisted; the stars last for this session.
  }
};

// How the backend compares phrases: case and punctuation ignored.
const phraseKey = (phrase: string) =>
  phrase
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();

const migrateOldMeta = async () => {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(OLD_META_KEY);
  } catch {
    return;
  }
  if (!raw) return;
  try {
    const old = JSON.parse(raw) as {
      replacements?: { from: string; to: string }[];
      starred?: string[];
    };
    for (const r of old.replacements ?? []) {
      await commands.addReplacement(r.from, r.to);
    }
    const starred = new Set(old.starred ?? []);
    const dict = await commands.getDictionary();
    const have = new Set(dict.replacements.map((r) => phraseKey(r.from)));
    if (
      (old.replacements ?? []).some(
        (r) => phraseKey(r.from) && !have.has(phraseKey(r.from)),
      )
    )
      throw new Error("some replacements were not saved");
    for (const w of dict.words) {
      if (starred.has(`w:${w.word}`) && !w.starred) {
        await commands.updateDictionaryWord(w.id, w.word, true);
      }
    }
    const starredFrom = new Set(
      [...starred].filter((k) => k.startsWith("r:")).map((k) => k.slice(2)),
    );
    saveStars(
      dict.replacements
        .filter((r) => starredFrom.has(r.from.toLowerCase()))
        .map((r) => r.id),
    );
    localStorage.removeItem(OLD_META_KEY);
  } catch (err) {
    console.error("Failed to move dictionary extras to the backend:", err);
  }
};

const sameWord = (a: string, b: string) =>
  a.localeCompare(b, undefined, { sensitivity: "accent" }) === 0;

export const useDictionary = () => {
  const [dict, setDict] = useState<Dictionary>(EMPTY);
  const [stars, setStarsState] = useState<string[]>(readStars);
  const [loaded, setLoaded] = useState(false);

  const refresh = useCallback(async () => {
    setDict(await commands.getDictionary());
    setStarsState(readStars());
  }, []);

  useEffect(() => {
    void migrateOldMeta()
      .then(refresh)
      .catch((err) => console.error("Failed to load dictionary:", err))
      .finally(() => setLoaded(true));
    return onTextflowChanged("dictionary", () => void refresh());
  }, [refresh]);

  const setStars = useCallback((ids: string[]) => {
    setStarsState(ids);
    saveStars(ids);
  }, []);

  const entries = useMemo<DictionaryEntry[]>(
    () => [
      ...dict.words.map((w) => ({
        key: `w:${w.id}`,
        id: w.id,
        word: w.word,
        starred: w.starred,
        learned: w.auto_learned,
      })),
      ...dict.replacements.map((r) => ({
        key: `r:${r.id}`,
        id: r.id,
        word: r.from,
        replacement: r.to,
        starred: stars.includes(r.id),
        learned: !!r.auto_learned,
      })),
    ],
    [dict, stars],
  );

  const has = useCallback(
    (input: EntryInput) =>
      input.replacement
        ? dict.replacements.some((r) => sameWord(r.from, input.word))
        : dict.words.some((w) => sameWord(w.word, input.word)),
    [dict],
  );

  /** Adds or (with `previous`) replaces an entry. Returns an error message, or null when saved. */
  const save = useCallback(
    async (input: EntryInput, previous?: DictionaryEntry) => {
      const word = normalizeWord(input.word);
      const replacement = input.replacement?.trim();
      if (!word || word.length > MAX_WORD_LENGTH)
        return "Enter a word or phrase.";
      const starred = previous?.starred ?? false;
      try {
        if (
          previous &&
          (previous.replacement !== undefined) === !!replacement
        ) {
          setDict(
            unwrap(
              replacement
                ? await commands.updateReplacement(
                    previous.id,
                    word,
                    replacement,
                  )
                : await commands.updateDictionaryWord(
                    previous.id,
                    word,
                    starred,
                  ),
            ),
          );
          return null;
        }
        let next = unwrap(
          replacement
            ? await commands.addReplacement(word, replacement)
            : await commands.addDictionaryWord(word),
        );
        if (starred) {
          if (replacement) setStars([...stars, next.replacements[0].id]);
          else {
            const added = next.words[0];
            next = unwrap(
              await commands.updateDictionaryWord(added.id, added.word, true),
            );
          }
        }
        if (previous) {
          next = unwrap(
            previous.replacement !== undefined
              ? await commands.deleteReplacement(previous.id)
              : await commands.deleteDictionaryWord(previous.id),
          );
        }
        setDict(next);
        return null;
      } catch (err) {
        return errorText(err);
      }
    },
    [setStars, stars],
  );

  /** Adds many entries at once (import). Returns how many were new. */
  const addMany = useCallback(async (inputs: EntryInput[]) => {
    let added = 0;
    for (const input of inputs) {
      const word = normalizeWord(input.word);
      const replacement = input.replacement?.trim();
      if (!word || word.length > MAX_WORD_LENGTH) continue;
      const res = replacement
        ? await commands.addReplacement(word, replacement)
        : await commands.addDictionaryWord(word);
      if (res.status === "ok") {
        setDict(res.data);
        added += 1;
      }
    }
    return added;
  }, []);

  const remove = useCallback(
    async (entry: DictionaryEntry) => {
      try {
        setDict(
          unwrap(
            entry.replacement !== undefined
              ? await commands.deleteReplacement(entry.id)
              : await commands.deleteDictionaryWord(entry.id),
          ),
        );
        if (entry.starred && entry.replacement !== undefined) {
          setStars(stars.filter((id) => id !== entry.id));
        }
      } catch (err) {
        console.error("Failed to delete dictionary entry:", err);
      }
    },
    [setStars, stars],
  );

  const toggleStar = useCallback(
    async (entry: DictionaryEntry) => {
      if (entry.replacement !== undefined) {
        setStars(
          entry.starred
            ? stars.filter((id) => id !== entry.id)
            : [...stars, entry.id],
        );
        return;
      }
      try {
        setDict(
          unwrap(
            await commands.updateDictionaryWord(
              entry.id,
              entry.word,
              !entry.starred,
            ),
          ),
        );
      } catch (err) {
        console.error("Failed to star word:", err);
      }
    },
    [setStars, stars],
  );

  return { entries, loaded, has, save, addMany, remove, toggleStar, refresh };
};

export const sortEntries = (entries: DictionaryEntry[], mode: SortMode) =>
  [...entries].sort((a, b) =>
    mode === "alpha"
      ? a.word.localeCompare(b.word, undefined, { sensitivity: "base" })
      : compareIdsNewestFirst(a.id, b.id),
  );

const ARROW = /\s*(?:->|→)\s*/;
const HEADER_CELLS = new Set([
  "word",
  "words",
  "term",
  "terms",
  "phrase",
  "from",
  "to",
  "replacement",
]);

const splitCells = (line: string) => {
  const cells: string[] = [];
  let cell = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (c === '"' && quoted && line[i + 1] === '"') {
      cell += c;
      i++;
    } else if (c === '"') quoted = !quoted;
    else if (c === "," && !quoted) {
      cells.push(cell);
      cell = "";
    } else cell += c;
  }
  return [...cells, cell].map((c) => c.trim()).filter(Boolean);
};

/**
 * Lines of comma-separated cells (CSV quoting allowed); a cell written as
 * "from -> to" or "from → to" is a replacement, kept only when `withReplacements`.
 */
export const parseImport = (
  text: string,
  withReplacements: boolean,
): EntryInput[] => {
  const rows = text.split(/\r?\n/).map(splitCells);
  if (
    rows[0]?.length &&
    rows[0].every((c) => HEADER_CELLS.has(c.toLowerCase()))
  )
    rows.shift();
  return rows.flat().flatMap((cell): EntryInput[] => {
    if (!ARROW.test(cell)) return [{ word: cell }];
    const [word, replacement] = cell.split(ARROW, 2);
    if (!replacement?.trim()) return [{ word }];
    return withReplacements ? [{ word, replacement }] : [];
  });
};
