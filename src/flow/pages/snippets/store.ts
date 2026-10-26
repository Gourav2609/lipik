import { useSyncExternalStore } from "react";
import { toast } from "sonner";
import { commands, type Result, type Snippet as SnippetRow } from "@/bindings";
import {
  compareIdsNewestFirst,
  errorText,
  onTextflowChanged,
  unwrap,
} from "../textflow";

export type SnippetScope = "personal" | "team";

export interface Snippet {
  id: string;
  trigger: string;
  expansion: string;
  scope: SnippetScope;
}

const HERO_KEY = "lipik.snippets.heroDismissed";
// Snippets the page kept here before the backend stored them.
const OLD_SNIPPETS_KEY = "lipik.snippets.v1";

const readHeroDismissed = () => {
  try {
    return localStorage.getItem(HERO_KEY) === "1";
  } catch {
    return false;
  }
};

let snippets: Snippet[] = [];
let loaded = false;
let heroDismissed = readHeroDismissed();
const listeners = new Set<() => void>();

const emit = () => listeners.forEach((l) => l());

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

const commit = (rows: SnippetRow[]) => {
  snippets = rows.map((r) => ({ ...r, scope: "personal" }));
  loaded = true;
  emit();
};

const run = async (call: Promise<Result<SnippetRow[], string>>) => {
  try {
    commit(unwrap(await call));
  } catch (err) {
    toast(errorText(err));
  }
};

export type SnippetDraft = Pick<Snippet, "trigger" | "expansion" | "scope">;

/** Triggers that differ only in case, spacing or punctuation are the same trigger. */
export const triggerKey = (trigger: string) =>
  trigger
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();

/** Newest first, the order the backend keeps them in. */
export const byNewest = (a: Snippet, b: Snippet) =>
  compareIdsNewestFirst(a.id, b.id);

const addAll = async (drafts: SnippetDraft[]) => {
  const seen = new Set(snippets.map((s) => triggerKey(s.trigger)));
  let added = 0;
  for (const d of drafts) {
    const key = triggerKey(d.trigger);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    const res = await commands.addSnippet(d.trigger, d.expansion);
    if (res.status === "ok") {
      commit(res.data);
      added += 1;
    }
  }
  return added;
};

const migrateOld = async () => {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(OLD_SNIPPETS_KEY);
  } catch {
    return;
  }
  if (!raw) return;
  try {
    const old: unknown = JSON.parse(raw);
    if (Array.isArray(old)) {
      const drafts = old
        .filter(
          (s) =>
            typeof s?.trigger === "string" && typeof s?.expansion === "string",
        )
        .sort((a, b) => (a.createdAt ?? 0) - (b.createdAt ?? 0))
        .map((s) => ({
          trigger: s.trigger,
          expansion: s.expansion,
          scope: "personal" as const,
        }));
      await addAll(drafts);
      const have = new Set(
        (await commands.getSnippets()).map((s) => triggerKey(s.trigger)),
      );
      if (
        drafts.some(
          (d) => triggerKey(d.trigger) && !have.has(triggerKey(d.trigger)),
        )
      )
        throw new Error("some snippets were not saved");
    }
    localStorage.removeItem(OLD_SNIPPETS_KEY);
  } catch (err) {
    console.error("Failed to move snippets to the backend:", err);
  }
};

export const snippetStore = {
  hasTrigger(trigger: string, exceptId?: string) {
    const key = triggerKey(trigger);
    return snippets.some(
      (s) => s.id !== exceptId && triggerKey(s.trigger) === key,
    );
  },
  add(draft: SnippetDraft) {
    void run(commands.addSnippet(draft.trigger, draft.expansion));
  },
  /** Adds drafts whose trigger is new; resolves to how many were added. */
  addMany: addAll,
  restore(snippet: Snippet) {
    if (!snippets.some((s) => s.id === snippet.id))
      void run(commands.addSnippet(snippet.trigger, snippet.expansion));
  },
  update(id: string, draft: SnippetDraft) {
    void run(commands.updateSnippet(id, draft.trigger, draft.expansion));
  },
  remove(id: string) {
    void run(commands.deleteSnippet(id));
  },
  reload() {
    commands
      .getSnippets()
      .then(commit)
      .catch((err) => console.error("Failed to load snippets:", err));
  },
  dismissHero() {
    heroDismissed = true;
    try {
      localStorage.setItem(HERO_KEY, "1");
    } catch {
      // Not persisted; the banner comes back next launch.
    }
    emit();
  },
};

if (typeof window !== "undefined") {
  void migrateOld().then(snippetStore.reload);
  onTextflowChanged("snippets", snippetStore.reload);
  window.addEventListener("storage", (e) => {
    if (e.key !== HERO_KEY && e.key !== null) return;
    heroDismissed = readHeroDismissed();
    emit();
  });
}

export const useSnippets = () =>
  useSyncExternalStore(subscribe, () => snippets);

export const useSnippetsLoaded = () =>
  useSyncExternalStore(subscribe, () => loaded);

export const useHeroDismissed = () =>
  useSyncExternalStore(subscribe, () => heroDismissed);

/** Splits CSV/TSV text into records; quoted fields may hold delimiters, quotes ("") and line breaks. */
const parseDelimited = (text: string, delim: string): string[][] => {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  let atStart = true;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        field += '"';
        i++;
      } else if (c === '"') quoted = false;
      else field += c;
    } else if (c === '"' && atStart && !field.trim()) {
      quoted = true;
      field = "";
      atStart = false;
    } else if (c === delim) {
      row.push(field);
      field = "";
      atStart = true;
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
      atStart = true;
    } else {
      field += c;
      if (c !== " ") atStart = false;
    }
  }
  row.push(field);
  rows.push(row);
  return rows;
};

const HEADER_TRIGGER =
  /^(triggers?|snippets?|shortcuts?|phrases?|names?|keywords?|abbreviations?)$/i;
const HEADER_EXPANSION =
  /^(expansions?|text|content|values?|replacements?|expands? to|snippets?)$/i;

/** Parses "trigger,expansion" or "trigger<TAB>expansion" rows; extra columns are joined back into the expansion. */
export const parseSnippetImport = (text: string): SnippetDraft[] => {
  const firstLine = text.split(/\r?\n/, 1)[0] ?? "";
  const delim = firstLine.includes("\t") ? "\t" : ",";
  const out: SnippetDraft[] = [];
  parseDelimited(text, delim).forEach((fields, i) => {
    if (fields.length < 2) return;
    const trigger = fields[0].trim();
    const expansion = fields.slice(1).join(delim).trim();
    if (!trigger || !expansion) return;
    if (
      i === 0 &&
      HEADER_TRIGGER.test(trigger) &&
      HEADER_EXPANSION.test(expansion)
    )
      return;
    out.push({ trigger, expansion, scope: "personal" });
  });
  return out;
};
