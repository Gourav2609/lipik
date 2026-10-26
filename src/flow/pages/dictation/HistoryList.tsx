import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Bookmark,
  Check,
  Copy,
  Ellipsis,
  Pause,
  Pencil,
  Play,
  Search,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { HistoryEntry } from "@/bindings";
import { copyToClipboard } from "@/components/settings/history/clipboard";
import { Popover } from "../../overlays/Popover";
import { Button, IconButton, Kbd } from "../../ui";
import { dayLabel, formatTime, groupByDay } from "./stats";
import { entryText, getAudioUrl, releaseAudioUrl } from "./useHistory";

const MENU_HEIGHT = 74;
const ICON = { size: 16, strokeWidth: 1.75 };
const PAGE_ROWS = 100;

const MoreMenu = ({
  anchor,
  onClose,
  onRetry,
  onDelete,
}: {
  anchor: DOMRect;
  onClose: () => void;
  onRetry: () => void;
  onDelete: () => void;
}) => {
  const { t: tr } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const item =
    "flex h-[32px] w-full items-center rounded-[8px] px-[10px] text-left text-[14px] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-hover)]";
  const below = anchor.bottom + 4 + MENU_HEIGHT <= window.innerHeight;
  return (
    <Popover
      onClose={onClose}
      className="fk-menu w-[200px] p-[4px]"
      style={{
        top: below ? anchor.bottom + 4 : anchor.top - 4 - MENU_HEIGHT,
        left: anchor.right - 200,
        transformOrigin: below ? "top right" : "bottom right",
      }}
    >
      <button type="button" role="menuitem" className={item} onClick={onRetry}>
        {tr("dictation.retryTranscription", "Transcribe again")}
      </button>
      <button
        type="button"
        role="menuitem"
        className={`${item} text-[var(--fk-live)] ${confirming ? "bg-[var(--fk-live-soft)] font-semibold" : ""}`}
        onClick={confirming ? onDelete : () => setConfirming(true)}
      >
        {confirming ? "Delete for good" : "Delete"}
      </button>
    </Popover>
  );
};

const Editor = ({
  initial,
  onSave,
  onCancel,
}: {
  initial: string;
  onSave: (text: string) => Promise<void>;
  onCancel: () => void;
}) => {
  const { t: tr } = useTranslation();
  const [value, setValue] = useState(initial);
  const [saving, setSaving] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);
  const canSave = value.trim().length > 0 && value.trim() !== initial && !saving;

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight + 2}px`;
  }, [value]);

  useEffect(() => {
    const el = ref.current;
    el?.focus();
    el?.setSelectionRange(el.value.length, el.value.length);
  }, []);

  const save = async () => {
    if (!canSave) return;
    setSaving(true);
    await onSave(value);
    setSaving(false);
  };

  return (
    <div className="flex flex-col gap-[8px]">
      <textarea
        ref={ref}
        rows={1}
        value={value}
        aria-label="Edit transcript"
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void save();
          }
        }}
        className="-ml-[13px] -mt-[9px] w-[calc(100%+13px)] resize-none overflow-hidden rounded-[10px] border border-[var(--fk-border)] bg-[var(--fk-panel)] px-[12px] py-[8px] text-[15px] leading-[22px] text-[var(--fk-text)] outline-none transition-[border-color,box-shadow] duration-[var(--fk-t-quick)] focus:border-[var(--fk-accent)] focus:shadow-[var(--fk-focus)]"
      />
      <div className="flex items-center justify-end gap-[8px]">
        <span className="mr-auto flex items-center gap-[4px] text-[13px] text-[var(--fk-text-faint)]">
          <Kbd>⌘</Kbd>
          <Kbd>↵</Kbd>
          <span className="mr-[8px]">{tr("dictation.editHintSave", "to save")}</span>
          <Kbd>esc</Kbd>
          <span>{tr("dictation.editHintCancel", "to cancel")}</span>
        </span>
        <Button variant="secondary" size="sm" onClick={onCancel}>
          {tr("dictation.cancel", "Cancel")}
        </Button>
        <Button size="sm" disabled={!canSave} onClick={() => void save()}>
          {tr("dictation.save", "Save")}
        </Button>
      </div>
    </div>
  );
};

interface RowProps {
  entry: HistoryEntry;
  forceHover: boolean;
  forceEdit: boolean;
  forceMenu: boolean;
  playing: boolean;
  onPlay: () => void;
  onToggleSaved: () => void;
  onRetry: () => Promise<void>;
  onDelete: () => void;
  onUpdateText: (text: string) => Promise<string[]>;
}

const Row = ({
  entry,
  forceHover,
  forceEdit,
  forceMenu,
  playing,
  onPlay,
  onToggleSaved,
  onRetry,
  onDelete,
  onUpdateText,
}: RowProps) => {
  const [copied, setCopied] = useState(false);
  const [menu, setMenu] = useState<DOMRect | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [editing, setEditing] = useState(forceEdit);
  const noticeTimer = useRef<number | undefined>(undefined);
  const copiedTimer = useRef<number | undefined>(undefined);
  const rowRef = useRef<HTMLDivElement>(null);
  const text = entryText(entry);
  const active = forceHover || menu !== null;

  useEffect(() => {
    if (!forceMenu) return;
    const frame = requestAnimationFrame(() => {
      const more = rowRef.current?.querySelector('button[aria-label="More"]');
      if (more) setMenu(more.getBoundingClientRect());
    });
    return () => cancelAnimationFrame(frame);
  }, [forceMenu]);

  useEffect(
    () => () => {
      window.clearTimeout(noticeTimer.current);
      window.clearTimeout(copiedTimer.current);
    },
    [],
  );

  const flash = (message: string) => {
    window.clearTimeout(noticeTimer.current);
    setNotice(message);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 4000);
  };

  const saveText = async (next: string) => {
    try {
      const learned = await onUpdateText(next);
      setEditing(false);
      if (learned.length) flash(`Added to Vocabulary: ${learned.join(", ")}`);
    } catch (error) {
      console.error(error);
      flash("Couldn't save the edit. Try again.");
    }
  };

  const copy = async () => {
    if (text && (await copyToClipboard(text))) {
      setCopied(true);
      window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setCopied(false), 2000);
    }
  };

  const retry = async () => {
    window.clearTimeout(noticeTimer.current);
    setNotice("Transcribing again…");
    try {
      await onRetry();
      setNotice(null);
    } catch (error) {
      console.error(error);
      flash("Couldn't transcribe this again. Try again later.");
    }
  };

  return (
    <div
      ref={rowRef}
      className={`group relative flex min-h-[48px] items-start border-b border-[var(--fk-border)] py-[13px] pl-[12px] pr-[12px] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-surface)] ${active ? "bg-[var(--fk-surface)]" : ""}`}
    >
      {entry.saved && (
        <span
          aria-hidden
          className="absolute inset-y-[10px] left-0 w-[2px] rounded-[1px] bg-[var(--fk-accent)]"
        />
      )}
      <div className="fk-num w-[72px] shrink-0 whitespace-nowrap pt-[1px] text-[13px] leading-[20px] text-[var(--fk-text-faint)]">
        {formatTime(entry.timestamp)}
      </div>
      <div className="min-w-0 max-w-[680px] flex-1">
        {editing ? (
          <Editor initial={text} onSave={saveText} onCancel={() => setEditing(false)} />
        ) : (
          <p
            className={`select-text whitespace-pre-wrap break-words text-[15px] leading-[22px] ${text ? "" : "text-[var(--fk-text-faint)]"}`}
          >
            {text || "No words were heard."}
          </p>
        )}
        {notice && (
          <p role="status" className="mt-[4px] text-[13px] leading-[18px] text-[var(--fk-accent)]">
            {notice}
          </p>
        )}
      </div>
      <div
        className={`absolute -top-[17px] right-[12px] z-[15] flex items-center gap-[2px] rounded-[10px] bg-[var(--fk-menu-bg)] p-[2px] shadow-[var(--fk-elev-2)] transition-opacity duration-[var(--fk-t-quick)] ${editing ? "hidden" : active ? "" : "pointer-events-none opacity-0 group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100"}`}
      >
        <IconButton label={playing ? "Pause" : "Play recording"} active={playing} onClick={onPlay}>
          {playing ? <Pause {...ICON} /> : <Play {...ICON} />}
        </IconButton>
        <IconButton label={copied ? "Copied" : "Copy"} onClick={copy}>
          {copied ? <Check {...ICON} /> : <Copy {...ICON} />}
        </IconButton>
        <IconButton label="Edit" onClick={() => setEditing(true)}>
          <Pencil {...ICON} />
        </IconButton>
        <IconButton
          label={entry.saved ? "Remove bookmark" : "Bookmark"}
          active={entry.saved}
          onClick={onToggleSaved}
        >
          <Bookmark {...ICON} />
        </IconButton>
        <IconButton
          label="More"
          active={menu !== null}
          onClick={(e) => setMenu(e.currentTarget.getBoundingClientRect())}
        >
          <Ellipsis {...ICON} />
        </IconButton>
      </div>
      {menu && (
        <MoreMenu
          anchor={menu}
          onClose={() => setMenu(null)}
          onRetry={() => {
            setMenu(null);
            retry();
          }}
          onDelete={() => {
            setMenu(null);
            onDelete();
          }}
        />
      )}
    </div>
  );
};

export interface HistoryListProps {
  entries: HistoryEntry[];
  loading: boolean;
  onToggleSaved: (id: number) => void;
  onRetry: (id: number) => Promise<void>;
  onDelete: (id: number) => void;
  onUpdateText: (id: number, text: string) => Promise<string[]>;
  /** Preview only: show one row (by index) in its hover state. */
  hoverIndex?: number;
  /** Preview only: open one row (by index) in the editor. */
  editIndex?: number;
  /** Preview only: open the More menu of one row (by index). */
  menuIndex?: number;
  /** Preview only: start with the search field open on this query. */
  initialQuery?: string;
}

export const HistoryList = ({
  entries,
  loading,
  onToggleSaved,
  onRetry,
  onDelete,
  onUpdateText,
  hoverIndex,
  editIndex,
  menuIndex,
  initialQuery,
}: HistoryListProps) => {
  const [searching, setSearching] = useState(initialQuery !== undefined);
  const [query, setQuery] = useState(initialQuery ?? "");
  const [playingId, setPlayingId] = useState<number | null>(null);
  const [rows, setRows] = useState(PAGE_ROWS);
  const audioRef = useRef<{ audio: HTMLAudioElement; url: string } | null>(null);
  const moreRef = useRef<HTMLDivElement>(null);

  const stop = () => {
    if (!audioRef.current) return;
    audioRef.current.audio.pause();
    releaseAudioUrl(audioRef.current.url);
    audioRef.current = null;
    setPlayingId(null);
  };

  useEffect(() => () => stop(), []);

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? entries.filter((e) => entryText(e).toLowerCase().includes(q)) : entries;
  }, [entries, query]);
  const groups = useMemo(() => groupByDay(matches.slice(0, rows)), [matches, rows]);
  const hasMore = matches.length > rows;

  useEffect(() => {
    const el = moreRef.current;
    if (!el || !hasMore) return;
    const observer = new IntersectionObserver(
      ([hit]) => hit.isIntersecting && setRows((n) => n + PAGE_ROWS),
      { root: el.closest(".fk-scroll"), rootMargin: "600px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasMore, groups]);

  const play = async (entry: HistoryEntry) => {
    const wasPlaying = playingId === entry.id;
    stop();
    if (wasPlaying) return;
    const url = await getAudioUrl(entry.file_name);
    if (!url) return;
    const audio = new Audio(url);
    audioRef.current = { audio, url };
    audio.onended = stop;
    setPlayingId(entry.id);
    audio.play().catch(stop);
  };

  const remove = (entry: HistoryEntry) => {
    if (playingId === entry.id) stop();
    onDelete(entry.id);
  };

  const closeSearch = () => {
    setSearching(false);
    setQuery("");
  };

  const searchControl = searching ? (
    <label className="flex h-[34px] w-[260px] items-center gap-[8px] rounded-[10px] border border-transparent bg-[var(--fk-surface)] px-[10px] transition-[border-color,box-shadow] duration-[var(--fk-t-quick)] hover:border-[var(--fk-border)] focus-within:border-[var(--fk-accent)] focus-within:bg-[var(--fk-panel)] focus-within:shadow-[var(--fk-focus)]">
      <Search {...ICON} className="shrink-0 text-[var(--fk-text-faint)]" />
      <input
        autoFocus
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && closeSearch()}
        placeholder="Search dictations"
        className="h-full min-w-0 flex-1 bg-transparent text-[14px] outline-none focus-visible:shadow-none placeholder:text-[var(--fk-text-faint)]"
      />
      <button
        type="button"
        aria-label="Close search"
        title="Close search"
        onClick={closeSearch}
        className="flex text-[var(--fk-text-faint)] hover:text-[var(--fk-text)]"
      >
        <X size={14} strokeWidth={1.75} />
      </button>
    </label>
  ) : (
    <IconButton label="Search" onClick={() => setSearching(true)}>
      <Search size={18} strokeWidth={1.75} />
    </IconButton>
  );

  const sectionHead = (
    <div className="sticky top-0 z-20 flex h-[52px] items-center justify-between gap-[16px] bg-[var(--fk-panel)]">
      <h2 className="text-[16px] font-semibold leading-[22px]">
        History
        {entries.length > 0 && (
          <span className="fk-num ml-[8px] font-normal text-[var(--fk-text-faint)]">
            {query ? `${matches.length} of ${entries.length}` : entries.length}
          </span>
        )}
      </h2>
      {searchControl}
    </div>
  );

  const dayHead = (label: string) => (
    <div className="fk-eyebrow sticky top-[52px] z-10 flex h-[36px] items-end bg-[var(--fk-panel)] pb-[8px]">
      {label}
    </div>
  );

  if (loading) return null;

  if (groups.length === 0) {
    return (
      <section>
        {sectionHead}
        <p className="border-y border-[var(--fk-border)] py-[24px] text-[15px] leading-[22px] text-[var(--fk-text-muted)]">
          {query
            ? `Nothing matches “${query.trim()}”.`
            : "No dictations yet. They appear here after you dictate in any app."}
        </p>
      </section>
    );
  }

  let index = 0;
  return (
    <section>
      {sectionHead}
      {groups.map((group, g) => (
        <div key={group.day} className={g === 0 ? "" : "mt-[16px]"}>
          {dayHead(dayLabel(group.day))}
          <div className="border-t border-[var(--fk-border)]">
            {group.entries.map((entry) => (
              <Row
                key={entry.id}
                entry={entry}
                forceHover={index === hoverIndex}
                forceMenu={index === menuIndex}
                forceEdit={index++ === editIndex}
                playing={playingId === entry.id}
                onPlay={() => play(entry)}
                onToggleSaved={() => onToggleSaved(entry.id)}
                onRetry={() => onRetry(entry.id)}
                onDelete={() => remove(entry)}
                onUpdateText={(text) => onUpdateText(entry.id, text)}
              />
            ))}
          </div>
        </div>
      ))}
      {hasMore && <div ref={moreRef} className="h-px" />}
    </section>
  );
};
