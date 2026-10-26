import { Plus, RotateCw, Search, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
  HeroBanner,
  HeroButton,
  IconButton,
  ListGroup,
  ListRow,
  PageHeader,
  TextInput,
  Toggle,
} from "../ui";
import { DeleteButton } from "./scratchpad/DeleteButton";
import { NoteEditor } from "./scratchpad/NoteEditor";
import {
  formatUpdated,
  notePreview,
  openNewestOnLoad,
  noteTitle,
  useScratchpad,
} from "./scratchpad/store";
import "./scratchpad/scratchpad.css";

export default function Scratchpad() {
  const notes = useScratchpad((s) => s.notes);
  const loaded = useScratchpad((s) => s.loaded);
  const dropBlank = useScratchpad((s) => s.dropBlank);
  const [openId, setOpenId] = useState<string | null>(null);
  const open = notes.find((n) => n.id === openId);

  useEffect(() => {
    if (loaded && openNewestOnLoad) setOpenId(notes[0]?.id ?? null);
  }, [loaded]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    dropBlank();
  }, [dropBlank]);

  const back = () => {
    setOpenId(null);
    dropBlank();
  };

  return open ? (
    <NoteEditor note={open} onBack={back} />
  ) : (
    <ScratchpadHome onOpen={setOpenId} />
  );
}

const ScratchpadHome = ({ onOpen }: { onOpen: (id: string) => void }) => {
  const notes = useScratchpad((s) => s.notes);
  const addToBar = useScratchpad((s) => s.addToBar);
  const setAddToBar = useScratchpad((s) => s.setAddToBar);
  const createNote = useScratchpad((s) => s.createNote);
  const deleteNote = useScratchpad((s) => s.deleteNote);
  const reload = useScratchpad((s) => s.reload);
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const sorted = [...notes].sort((a, b) => b.updatedAt - a.updatedAt);
    if (!q) return sorted;
    return sorted.filter((n) =>
      `${n.title}\n${n.body}`.toLowerCase().includes(q),
    );
  }, [notes, query]);

  const newNote = () => onOpen(createNote());

  const closeSearch = () => {
    setSearching(false);
    setQuery("");
  };

  return (
    <div className="fk-page">
      <PageHeader
        title={
          <span className="flex items-center gap-[10px]">
            Drafts
            <span className="rounded-[4px] bg-[var(--fk-inverse-bg)] px-[6px] py-[4px] [font-family:var(--fk-font-sans)] text-[11px] font-semibold leading-none text-[var(--fk-inverse-text)]">
              Beta
            </span>
          </span>
        }
        description="Think out loud. Keep what's worth keeping."
        actions={
          <label
            title="Open Drafts from the pill, from any app"
            className="flex items-center gap-[10px] text-[14px] text-[var(--fk-text-muted)]"
          >
            Open from the pill
            <Toggle checked={addToBar} onChange={setAddToBar} label="Open Drafts from the pill" />
          </label>
        }
      />

      <HeroBanner
        title="A page for loose thoughts"
        subtitle="Dictate a list, a rough message or half an idea. It stays here until you need it."
        actions={
          <HeroButton strong onClick={newNote}>
            New draft
          </HeroButton>
        }
      />

      <div className="mt-[40px] flex h-[38px] items-center gap-[8px]">
        {searching ? (
          <TextInput
            autoFocus
            className="flex-1"
            icon={<Search size={16} strokeWidth={1.75} />}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && closeSearch()}
            placeholder="Search drafts"
          />
        ) : (
          <h2 className="flex-1 text-[16px] font-semibold leading-[22px]">
            Recent
            {notes.length > 0 && (
              <span className="fk-num ml-[8px] font-normal text-[var(--fk-text-faint)]">
                {notes.length}
              </span>
            )}
          </h2>
        )}
        <div className="flex items-center gap-[4px]">
          <IconButton
            label={searching ? "Close search" : "Search"}
            onClick={() => (searching ? closeSearch() : setSearching(true))}
          >
            {searching ? <X size={16} strokeWidth={1.75} /> : <Search size={16} strokeWidth={1.75} />}
          </IconButton>
          <IconButton label="New draft" onClick={newNote}>
            <Plus size={16} strokeWidth={1.75} />
          </IconButton>
          <IconButton label="Refresh" onClick={reload}>
            <RotateCw size={16} strokeWidth={1.75} />
          </IconButton>
        </div>
      </div>

      {shown.length === 0 ? (
        <p className="mt-[12px] border-t border-[var(--fk-border)] pt-[48px] text-center text-[14.5px] text-[var(--fk-text-muted)]">
          {query.trim()
            ? "No drafts match your search."
            : "No drafts yet. Start one with New draft."}
        </p>
      ) : (
        <ListGroup className="mt-[12px]">
          {shown.map((note) => {
            const preview = notePreview(note);
            return (
              <ListRow
                key={note.id}
                className="relative py-[12px] has-[:focus-visible]:bg-[var(--fk-surface)]"
              >
                <div className="flex items-baseline gap-[12px]">
                  <span className="truncate font-semibold">{noteTitle(note)}</span>
                  <span className="fk-num ml-auto shrink-0 text-[13px] text-[var(--fk-text-faint)] transition-opacity duration-[var(--fk-t-quick)] group-focus-within:opacity-0 group-hover:opacity-0">
                    {formatUpdated(note.updatedAt)}
                  </span>
                </div>
                {preview && (
                  <p className="mt-[2px] truncate text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
                    {preview}
                  </p>
                )}
                <button
                  type="button"
                  aria-label={`Open ${noteTitle(note)}`}
                  onClick={() => onOpen(note.id)}
                  className="absolute inset-0 outline-none focus-visible:shadow-none"
                />
                <div className="absolute right-[12px] top-[9px] text-[var(--fk-text-muted)] opacity-0 transition-opacity duration-[var(--fk-t-quick)] group-focus-within:opacity-100 group-hover:opacity-100">
                  <DeleteButton onConfirm={() => deleteNote(note.id)}>
                    <X size={16} strokeWidth={1.75} />
                  </DeleteButton>
                </div>
              </ListRow>
            );
          })}
        </ListGroup>
      )}
    </div>
  );
};
