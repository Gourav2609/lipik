import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { FileUp, Pencil, Plus, RefreshCw, Search, Trash2 } from "lucide-react";
import { isFlowPreview } from "../dev/preview";
import {
  Button,
  HeroBanner,
  HeroButton,
  IconButton,
  ListGroup,
  ListRow,
  PageHeader,
  RowAction,
  SectionTabs,
  TextInput,
} from "../ui";
import { SnippetDialog } from "./snippets/SnippetDialog";
import {
  parseSnippetImport,
  byNewest,
  snippetStore,
  useHeroDismissed,
  useSnippets,
  useSnippetsLoaded,
  type Snippet,
} from "./snippets/store";

type Sort = "newest" | "alpha";

const SORTS: { id: Sort; label: string }[] = [
  { id: "newest", label: "Newest" },
  { id: "alpha", label: "A to Z" },
];

const EXAMPLES = [
  ["my address", "14 MG Road, Bengaluru 560001"],
  ["meeting link", "meet.example.com/asha"],
  ["sign off", "Thanks, Asha"],
];

// Preview-only knobs for screenshots: snipHover=<row>, snipSort=alpha, q=<search>,
// snipDialog=new|edit.
const preview = new URLSearchParams(isFlowPreview ? window.location.search : "");

const RuledExamples = () => (
  <div className="hidden h-full items-center justify-end pr-[56px] xl:flex">
    <div className="w-[320px] border-t border-[color-mix(in_srgb,var(--fk-accent)_22%,transparent)]">
      {EXAMPLES.map(([cue, text]) => (
        <div
          key={cue}
          className="grid h-[36px] grid-cols-[104px_minmax(0,1fr)] items-center gap-[12px] border-b border-[color-mix(in_srgb,var(--fk-accent)_22%,transparent)] px-[4px] text-[14px]"
        >
          <span className="truncate font-medium text-[var(--fk-accent)]">{cue}</span>
          <span className="truncate text-[var(--fk-text-muted)]">{text}</span>
        </div>
      ))}
    </div>
  </div>
);

export default function Snippets() {
  const snippets = useSnippets();
  const heroDismissed = useHeroDismissed();
  const loaded = useSnippetsLoaded();
  const [query, setQuery] = useState(preview.get("q") ?? "");
  const [sort, setSort] = useState<Sort>(
    preview.get("snipSort") === "alpha" ? "alpha" : "newest",
  );
  const [editing, setEditing] = useState<Snippet | "new" | null>(
    preview.get("snipDialog") === "new" ? "new" : null,
  );
  const fileRef = useRef<HTMLInputElement>(null);
  const forcedHover = Number(preview.get("snipHover") ?? -1);

  const searching = query.trim() !== "";
  const showHero = (!heroDismissed || (loaded && snippets.length === 0)) && !searching;

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = snippets.filter(
      (s) =>
        !q ||
        s.trigger.toLowerCase().includes(q) ||
        s.expansion.toLowerCase().includes(q),
    );
    return sort === "alpha"
      ? [...list].sort((a, b) => a.trigger.localeCompare(b.trigger))
      : [...list].sort(byNewest);
  }, [snippets, query, sort]);

  useEffect(() => {
    if (preview.get("snipDialog") === "edit" && snippets[0]) setEditing(snippets[0]);
  }, [loaded]);

  const importFile = async (file: File) => {
    const added = await snippetStore.addMany(parseSnippetImport(await file.text()));
    toast(
      added
        ? `Added ${added} ${added === 1 ? "snippet" : "snippets"}`
        : "No new snippets in that file",
    );
  };

  const remove = (s: Snippet) => {
    snippetStore.remove(s.id);
    toast("Snippet deleted", {
      action: { label: "Undo", onClick: () => snippetStore.restore(s) },
    });
  };

  return (
    <div className="fk-page">
      <PageHeader
        title="Snippets"
        description="Say a short cue, get the full text."
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
              onClick={() => setEditing("new")}
            >
              New snippet
            </Button>
          </>
        }
      />
      <input
        ref={fileRef}
        type="file"
        accept=".csv,.tsv,.txt,text/csv,text/plain"
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) void importFile(file);
          e.target.value = "";
        }}
      />

      {showHero && (
        <HeroBanner
          className="mb-[40px]"
          art={<RuledExamples />}
          onClose={snippets.length > 0 ? snippetStore.dismissHero : undefined}
          title="Stop retyping the same text"
          subtitle="Save text you write often, then say its cue to insert it."
          actions={
            <HeroButton strong onClick={() => setEditing("new")}>
              New snippet
            </HeroButton>
          }
        />
      )}

      {loaded && (snippets.length > 0 || searching) && (
        <div className="mb-[16px] flex items-center gap-[12px]">
          <TextInput
            className="w-[320px]"
            icon={<Search size={16} strokeWidth={1.75} />}
            placeholder="Search snippets"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setQuery("")}
          />
          <span className="fk-num text-[13px] text-[var(--fk-text-faint)]">
            {visible.length === snippets.length
              ? `${snippets.length} ${snippets.length === 1 ? "snippet" : "snippets"}`
              : `${visible.length} of ${snippets.length}`}
          </span>
          <div className="ml-auto flex items-center gap-[8px]">
            <SectionTabs tabs={SORTS} value={sort} onChange={setSort} />
            <IconButton label="Refresh" onClick={snippetStore.reload}>
              <RefreshCw size={16} strokeWidth={1.75} />
            </IconButton>
          </div>
        </div>
      )}

      {visible.length > 0 ? (
        <ListGroup>
          {visible.map((s, i) => (
            <ListRow
              key={s.id}
              className={i === forcedHover ? "bg-[var(--fk-surface)]" : ""}
              showActions={i === forcedHover}
              actions={
                <>
                  <RowAction label="Edit" onClick={() => setEditing(s)}>
                    <Pencil size={16} strokeWidth={1.75} />
                  </RowAction>
                  <RowAction label="Delete" onClick={() => remove(s)}>
                    <Trash2 size={16} strokeWidth={1.75} />
                  </RowAction>
                </>
              }
            >
              <div className="grid grid-cols-[minmax(0,200px)_minmax(0,1fr)] items-center gap-[24px]">
                <span className="truncate font-medium">{s.trigger}</span>
                <span className="truncate text-[var(--fk-text-muted)]">
                  {s.expansion}
                </span>
              </div>
            </ListRow>
          ))}
        </ListGroup>
      ) : (
        ((loaded && !showHero) || searching) && (
          <p className="border-t border-[var(--fk-border)] py-[48px] text-center text-[15px] text-[var(--fk-text-muted)]">
            {searching
              ? `Nothing matches “${query.trim()}”.`
              : "No snippets yet. Add one for text you write often."}
          </p>
        )
      )}

      {editing && (
        <SnippetDialog
          initial={editing === "new" ? undefined : editing}
          onClose={() => setEditing(null)}
          onSave={(draft) =>
            editing === "new"
              ? snippetStore.add(draft)
              : snippetStore.update(editing.id, draft)
          }
        />
      )}
    </div>
  );
}
