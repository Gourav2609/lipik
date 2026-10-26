import type { Dictionary } from "@/bindings";
import type { MockHandler } from "../mockTauri";

// In-memory dictionary for the preview. Ids rise with age order, like the
// backend's time-based ids, so "newest first" sorts the same way.
let dict: Dictionary = {
  words: [
    ["7", "Neha Kapoor"],
    ["6", "Koramangala"],
    ["5", "API", true],
    ["4", "tokenizer", true],
    ["3", "Lipik"],
    ["1", "team.inbox@example.com"],
  ].map(([id, word, learned]) => ({
    id: id as string,
    word: word as string,
    starred: false,
    auto_learned: !!learned,
    shared: false,
  })),
  replacements: [{ id: "2", from: "btw", to: "by the way" }],
};

let nextId = 100;
const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

// Result commands: the bindings wrap the returned value, and a thrown string becomes the error.
const change = (fn: () => string | null): Dictionary => {
  const error = fn();
  if (error) throw error;
  return dict;
};

export const handlers: Record<string, MockHandler> = {
  get_dictionary: () => dict,
  add_dictionary_word: ({ word }) =>
    change(() => {
      const w = String(word);
      if (dict.words.some((x) => same(x.word, w)))
        return `"${w}" is already in your dictionary`;
      dict = {
        ...dict,
        words: [
          {
            id: String(nextId++),
            word: w,
            starred: false,
            auto_learned: false,
            shared: false,
          },
          ...dict.words,
        ],
      };
      return null;
    }),
  update_dictionary_word: ({ id, word, starred }) =>
    change(() => {
      dict = {
        ...dict,
        words: dict.words.map((w) =>
          w.id === id ? { ...w, word: String(word), starred: !!starred } : w,
        ),
      };
      return null;
    }),
  delete_dictionary_word: ({ id }) =>
    change(() => {
      dict = { ...dict, words: dict.words.filter((w) => w.id !== id) };
      return null;
    }),
  add_replacement: ({ from, to }) =>
    change(() => {
      const f = String(from);
      if (dict.replacements.some((r) => same(r.from, f)))
        return `"${f}" already has a replacement`;
      dict = {
        ...dict,
        replacements: [
          { id: String(nextId++), from: f, to: String(to) },
          ...dict.replacements,
        ],
      };
      return null;
    }),
  update_replacement: ({ id, from, to }) =>
    change(() => {
      dict = {
        ...dict,
        replacements: dict.replacements.map((r) =>
          r.id === id ? { ...r, from: String(from), to: String(to) } : r,
        ),
      };
      return null;
    }),
  delete_replacement: ({ id }) =>
    change(() => {
      dict = {
        ...dict,
        replacements: dict.replacements.filter((r) => r.id !== id),
      };
      return null;
    }),
};
