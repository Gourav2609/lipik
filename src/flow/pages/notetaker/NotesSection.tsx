import { useEffect, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Ellipsis,
  Folder as FolderIcon,
  Search,
} from "lucide-react";
import { Button, IconButton, SectionTabs, TextInput } from "../../ui";
import { AvatarStack } from "./Avatars";
import {
  groupByDay,
  listNotes,
  relativeDayLabel,
  timeOfDay,
  type Folder,
  type Note,
} from "./data";
import { NoteMenu } from "./NoteMenu";

export type NotesTab = "past" | "folders";

const TABS: { id: NotesTab; label: string }[] = [
  { id: "past", label: "All notes" },
  { id: "folders", label: "Folders" },
];

const RowStatus = ({ note }: { note: Note }) => {
  if (note.status === "recording")
    return (
      <span className="flex items-center gap-[6px] text-[13px] text-[var(--fk-live)]">
        <span className="nt-live-dot" />
        Recording
      </span>
    );
  if (note.status === "processing")
    return (
      <span className="flex items-center gap-[6px] text-[13px] text-[var(--fk-accent)]">
        <span className="nt-ink-dot" />
        Writing notes
      </span>
    );
  return null;
};

type RowActions = {
  folders: Folder[];
  selectedId: string | null;
  pickedId: string | null;
  onSelect: (id: string) => void;
  onOpen: (id: string) => void;
  onChanged: () => void;
};

const NoteRow = ({
  note,
  folders,
  selectedId,
  pickedId,
  onSelect,
  onOpen,
  onChanged,
}: RowActions & { note: Note }) => {
  const [hover, setHover] = useState(false);
  const [menu, setMenu] = useState<DOMRect | null>(null);
  const selected = note.id === selectedId;
  const activate = () =>
    note.id === pickedId ? onOpen(note.id) : onSelect(note.id);
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={activate}
      onDoubleClick={() => onOpen(note.id)}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter") onOpen(note.id);
        else if (e.key === " ") {
          e.preventDefault();
          onSelect(note.id);
        }
      }}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      data-selected={selected || undefined}
      data-open={menu ? true : undefined}
      className="nt-line flex h-[52px] w-full cursor-default items-center pl-[12px] pr-[8px] text-left"
    >
      <span className="fk-num w-[72px] shrink-0 whitespace-nowrap text-[13px] text-[var(--fk-text-muted)]">
        {timeOfDay(note.createdAt)}
      </span>
      <span className="min-w-0 flex-1 truncate text-[15px] leading-[22px]">
        {note.title}
      </span>
      <span className="ml-[12px] flex shrink-0 items-center gap-[12px]">
        <RowStatus note={note} />
        {!(hover || menu) && <AvatarStack people={note.attendees} size={20} />}
      </span>
      {(hover || menu) && (
        <IconButton
          label="Note options"
          aria-expanded={!!menu}
          className="ml-[8px]"
          onClick={(e) => {
            e.stopPropagation();
            setMenu(menu ? null : e.currentTarget.getBoundingClientRect());
          }}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <Ellipsis size={16} strokeWidth={1.75} />
        </IconButton>
      )}
      {menu && (
        <div onClick={(e) => e.stopPropagation()}>
          <NoteMenu
            note={note}
            folders={folders}
            anchor={menu}
            onClose={() => setMenu(null)}
            onOpen={() => onOpen(note.id)}
            onChanged={onChanged}
          />
        </div>
      )}
    </div>
  );
};

const Skeleton = () => (
  <div className="nt-ruled-top mt-[32px]" aria-label="Loading">
    {[0, 1, 2].map((i) => (
      <div
        key={i}
        className="flex h-[52px] items-center gap-[24px] border-b border-[var(--fk-border)] pl-[12px]"
      >
        <span className="nt-skeleton h-[10px] w-[48px] rounded-[4px]" />
        <span
          className="nt-skeleton h-[10px] rounded-[4px]"
          style={{ width: [280, 220, 250][i] }}
        />
      </div>
    ))}
  </div>
);

const Empty = ({ text }: { text: string }) => (
  <div className="mt-[24px] flex items-center gap-[16px] border-y border-[var(--fk-border)] py-[16px] pl-[12px]">
    <p className="min-w-0 flex-1 text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
      {text}
    </p>
  </div>
);

const NoteList = ({ notes, ...actions }: RowActions & { notes: Note[] }) => (
  <div className="pt-[16px]">
    {groupByDay(notes, (n) => n.createdAt).map((group, i) => (
      <div key={group.day} className={i ? "mt-[12px]" : ""}>
        <div className="fk-eyebrow flex h-[32px] items-end pb-[8px] pl-[12px]">
          {relativeDayLabel(group.day)}
        </div>
        <div className="nt-ruled-top">
          {group.items.map((n) => (
            <NoteRow key={n.id} note={n} {...actions} />
          ))}
        </div>
      </div>
    ))}
  </div>
);

/** Notes from the backend for a search or a folder; null while loading. */
const useNoteQuery = (
  query: string,
  folder: string | null,
  notes: Note[],
): Note[] | null => {
  const [found, setFound] = useState<Note[] | null>(null);
  const q = query.trim();
  useEffect(() => {
    if (!q && !folder) return;
    let live = true;
    setFound(null);
    const id = window.setTimeout(
      () =>
        listNotes(q || null, folder)
          .then((list) => live && setFound(list))
          .catch(() => live && setFound([])),
      q ? 200 : 0,
    );
    return () => {
      live = false;
      window.clearTimeout(id);
    };
  }, [q, folder, notes]);
  return q || folder ? found : notes;
};

export const NotesSection = ({
  tab,
  onTab,
  notes,
  folders,
  selectedId,
  pickedId,
  onSelect,
  onOpen,
  onChanged,
}: {
  tab: NotesTab;
  onTab: (tab: NotesTab) => void;
  notes: { data: Note[]; loading: boolean };
  folders: { data: Folder[]; loading: boolean };
  selectedId: string | null;
  pickedId: string | null;
  onSelect: (id: string) => void;
  onOpen: (id: string) => void;
  onChanged: () => void;
}) => {
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const [folder, setFolder] = useState<string | null>(null);
  const inFolder = tab === "folders" ? folder : null;
  const found = useNoteQuery(
    query,
    inFolder,
    notes.data,
  );
  const actions: RowActions = {
    folders: folders.data,
    selectedId,
    pickedId,
    onSelect,
    onOpen,
    onChanged,
  };

  let body;
  if (tab === "folders" && !inFolder && !query.trim()) {
    body = folders.loading ? (
      <Skeleton />
    ) : folders.data.length ? (
      <div className="nt-ruled-top mt-[24px]">
        {folders.data.map((f) => (
          <button
            type="button"
            key={f.id}
            onClick={() => setFolder(f.name)}
            className="nt-line flex h-[48px] w-full items-center pl-[12px] pr-[12px] text-left"
          >
            <FolderIcon
              size={16}
              strokeWidth={1.75}
              className="mr-[12px] text-[var(--fk-text-muted)]"
            />
            <span className="min-w-0 flex-1 truncate text-[15px]">
              {f.name}
            </span>
            <span className="fk-num mr-[8px] text-[13px] text-[var(--fk-text-faint)]">
              {f.noteCount}
            </span>
            <ChevronRight
              size={16}
              strokeWidth={1.75}
              className="text-[var(--fk-text-faint)]"
            />
          </button>
        ))}
      </div>
    ) : (
      <Empty text="No folders yet. Add a note to a folder from its options." />
    );
  } else if (notes.loading || !found) {
    body = <Skeleton />;
  } else {
    body = found.length ? (
      <NoteList notes={found} {...actions} />
    ) : (
      <Empty
        text={
          query.trim()
            ? "No notes match your search."
            : "No notes yet. Start recording here or from the pill."
        }
      />
    );
  }

  return (
    <section className="mt-[40px]">
      <div className="mb-[12px] flex h-[30px] items-center">
        <h2 className="text-[16px] font-semibold leading-[22px]">Notes</h2>
        {!notes.loading && tab === "past" && !query.trim() && (
          <span className="fk-num ml-[8px] text-[14px] text-[var(--fk-text-faint)]">
            {notes.data.length}
          </span>
        )}
      </div>
      <SectionTabs
        tabs={TABS}
        value={tab}
        onChange={(t) => {
          setFolder(null);
          onTab(t);
        }}
        actions={
          <IconButton
            label="Search notes"
            active={searching}
            onClick={() => {
              setSearching((v) => !v);
              setQuery("");
            }}
          >
            <Search size={16} strokeWidth={1.75} />
          </IconButton>
        }
      />
      {searching && (
        <div className="nt-fade pt-[16px]">
          <TextInput
            autoFocus
            icon={<Search size={16} strokeWidth={1.75} />}
            placeholder="Search titles, transcripts and notes"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      )}
      {inFolder && (
        <Button
          variant="ghost"
          size="sm"
          className="mt-[16px] -ml-[8px]"
          icon={<ChevronLeft size={16} strokeWidth={1.75} />}
          onClick={() => setFolder(null)}
        >
          {inFolder}
        </Button>
      )}
      {body}
    </section>
  );
};
