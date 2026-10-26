import { useEffect, useMemo, useRef, useState, type ChangeEvent } from "react";
import { FileUp, Plus, RefreshCw, Search } from "lucide-react";
import { toast } from "sonner";
import { APP_NAME } from "@/flow/brand";
import { isFlowPreview } from "../dev/preview";
import {
  Button,
  IconButton,
  ListGroup,
  PageHeader,
  SectionTabs,
  TextInput,
} from "../ui";
import { DictionaryHero } from "./dictionary/DictionaryHero";
import { WordDialog } from "./dictionary/WordDialog";
import { WordRow } from "./dictionary/WordRow";
import {
  parseImport,
  sortEntries,
  useDictionary,
  type DictionaryEntry,
  type SortMode,
} from "./dictionary/useDictionary";

const SORTS: { id: SortMode; label: string }[] = [
  { id: "recent", label: "Newest" },
  { id: "alpha", label: "A to Z" },
];

const HERO_KEY = "lipik.dictionary.heroDismissed";

const readHeroDismissed = () => {
  try {
    return !isFlowPreview && localStorage.getItem(HERO_KEY) === "1";
  } catch {
    return false;
  }
};

// Preview-only knobs for screenshots: dictHover=<row>, dictSort=alpha, q=<search>,
// dictDialog=add|replace|edit, dictHero=0.
const preview = new URLSearchParams(isFlowPreview ? window.location.search : "");

type Dialog =
  | { mode: "add"; replacement?: boolean }
  | { mode: "edit"; entry: DictionaryEntry };

const countLabel = (shown: number, total: number) =>
  shown === total
    ? `${total} ${total === 1 ? "word" : "words"}`
    : `${shown} of ${total}`;

export default function Dictionary() {
  const dict = useDictionary();
  const [sort, setSort] = useState<SortMode>(
    preview.get("dictSort") === "alpha" ? "alpha" : "recent",
  );
  const [query, setQuery] = useState(preview.get("q") ?? "");
  const [heroDismissed, setHeroDismissed] = useState(
    () => readHeroDismissed() || preview.get("dictHero") === "0",
  );
  const [dialog, setDialog] = useState<Dialog | null>(() => {
    const d = preview.get("dictDialog");
    if (d === "add" || d === "1") return { mode: "add" };
    if (d === "replace") return { mode: "add", replacement: true };
    return null;
  });
  const fileRef = useRef<HTMLInputElement>(null);
  const forcedHover = Number(preview.get("dictHover") ?? -1);

  useEffect(() => {
    if (preview.get("dictDialog") !== "edit") return;
    const entry = dict.entries.find((e) => e.replacement !== undefined);
    if (entry) setDialog({ mode: "edit", entry });
  }, [dict.loaded]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = q
      ? dict.entries.filter(
          (e) =>
            e.word.toLowerCase().includes(q) ||
            e.replacement?.toLowerCase().includes(q),
        )
      : dict.entries;
    return sortEntries(list, sort);
  }, [dict.entries, query, sort]);

  const dismissHero = () => {
    setHeroDismissed(true);
    try {
      localStorage.setItem(HERO_KEY, "1");
    } catch {
      // Not persisted; the note comes back next launch.
    }
  };

  const onImport = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    const added = await dict.addMany(parseImport(await file.text(), true));
    toast(
      added
        ? `Added ${added} ${added === 1 ? "word" : "words"}`
        : "No new words in that file",
    );
  };

  const searching = query.trim() !== "";
  const showHero = dict.loaded && !heroDismissed && !searching;

  return (
    <div className="fk-page">
      <PageHeader
        title="Vocabulary"
        description={`Names and terms ${APP_NAME} should always spell right.`}
        actions={
          <>
            <Button
              variant="secondary"
              icon={<FileUp size={16} strokeWidth={1.75} />}
              onClick={() => fileRef.current?.click()}
            >
              Import
            </Button>
            <Button
              icon={<Plus size={16} strokeWidth={1.75} />}
              onClick={() => setDialog({ mode: "add" })}
            >
              Add word
            </Button>
          </>
        }
      />
      <input
        ref={fileRef}
        type="file"
        accept=".txt,.csv,text/plain,text/csv"
        className="hidden"
        onChange={onImport}
      />

      {showHero && (
        <DictionaryHero
          className="mb-[40px]"
          onAddNew={() => setDialog({ mode: "add" })}
          onClose={dismissHero}
        />
      )}

      {dict.loaded && (dict.entries.length > 0 || searching) && (
        <div className="mb-[16px] flex items-center gap-[12px]">
          <TextInput
            className="w-[320px]"
            icon={<Search size={16} strokeWidth={1.75} />}
            placeholder="Search vocabulary"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setQuery("")}
          />
          <span className="fk-num text-[13px] text-[var(--fk-text-faint)]">
            {countLabel(visible.length, dict.entries.length)}
          </span>
          <div className="ml-auto flex items-center gap-[8px]">
            <SectionTabs tabs={SORTS} value={sort} onChange={setSort} />
            <IconButton label="Refresh" onClick={() => void dict.refresh()}>
              <RefreshCw size={16} strokeWidth={1.75} />
            </IconButton>
          </div>
        </div>
      )}

      {!dict.loaded ? null : visible.length > 0 ? (
        <ListGroup>
          {visible.map((entry, i) => (
            <WordRow
              key={entry.key}
              entry={entry}
              forceHover={i === forcedHover}
              onEdit={() => setDialog({ mode: "edit", entry })}
              onDelete={() => void dict.remove(entry)}
              onStar={() => void dict.toggleStar(entry)}
            />
          ))}
        </ListGroup>
      ) : (
        <p className="border-t border-[var(--fk-border)] py-[48px] text-center text-[15px] text-[var(--fk-text-muted)]">
          {searching
            ? `Nothing matches “${query.trim()}”.`
            : "No words yet. Add a name or term you use often."}
        </p>
      )}

      {dialog && (
        <WordDialog
          initial={
            dialog.mode === "edit"
              ? { word: dialog.entry.word, replacement: dialog.entry.replacement }
              : undefined
          }
          startAsReplacement={dialog.mode === "add" && !!dialog.replacement}
          onSave={(input) =>
            dict.save(input, dialog.mode === "edit" ? dialog.entry : undefined)
          }
          onClose={() => setDialog(null)}
        />
      )}
    </div>
  );
}
