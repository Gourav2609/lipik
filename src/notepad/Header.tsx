import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Folder, FolderPlus } from "lucide-react";
import type { Meeting } from "@/bindings";
import { dayLabel, timeOfDay } from "./format";

const PLACEHOLDER = "New note";
const CHAR_MS = 33;
const LINE = 38;

const shownTitle = (m: Meeting) =>
  m.title_auto && m.title === PLACEHOLDER ? "" : m.title;

type Props = {
  meeting: Meeting;
  onRename: (title: string) => void;
  onFolder: (folder: string | null) => void;
};

export const Header = ({ meeting, onRename, onFolder }: Props) => (
  <div className="np-head">
    <Title meeting={meeting} onRename={onRename} />
    <div className="np-meta">
      <span
        className="np-num"
        title={new Date(meeting.start_ms).toLocaleString([], {
          dateStyle: "full",
          timeStyle: "short",
        })}
      >
        {dayLabel(meeting.start_ms)}, {timeOfDay(meeting.start_ms)}
      </span>
      {meeting.end_ms && (
        <>
          <span aria-hidden>·</span>
          <span className="np-num">
            {Math.max(1, Math.round((meeting.end_ms - meeting.start_ms) / 60_000))}{" "}
            min
          </span>
        </>
      )}
      <span aria-hidden>·</span>
      <FolderChip folder={meeting.folder} onFolder={onFolder} />
    </div>
  </div>
);

/** Editable display title. A title the notes make up types in, ~30 characters a second. */
const Title = ({ meeting, onRename }: Omit<Props, "onFolder">) => {
  const target = shownTitle(meeting);
  const [value, setValue] = useState(target);
  const [typed, setTyped] = useState<number | null>(null);
  const [editing, setEditing] = useState(false);
  const last = useRef({ id: meeting.id, title: target });
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const prev = last.current;
    last.current = { id: meeting.id, title: target };
    if (editing) return;
    setValue(target);
    const generated =
      prev.id === meeting.id &&
      meeting.title_auto &&
      target &&
      target !== prev.title;
    setTyped(generated ? 0 : null);
  }, [meeting.id, meeting.title_auto, target, editing]);

  useEffect(() => {
    if (typed === null) return;
    if (typed >= value.length) {
      setTyped(null);
      return;
    }
    const t = window.setTimeout(() => setTyped(typed + 1), CHAR_MS);
    return () => window.clearTimeout(t);
  }, [typed, value]);

  const text = typed === null ? value : value.slice(0, typed);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.max(1, Math.round(el.scrollHeight / LINE)) * LINE}px`;
  }, [text]);

  const commit = () => {
    setEditing(false);
    const title = value.replace(/\s+/g, " ").trim();
    if (!title) {
      setValue(target);
      return;
    }
    if (title !== target) onRename(title);
  };

  return (
    <textarea
      ref={ref}
      className="np-title"
      rows={1}
      spellCheck={false}
      aria-label="Note title"
      placeholder={PLACEHOLDER}
      value={text}
      readOnly={typed !== null}
      onFocus={() => setEditing(true)}
      onBlur={commit}
      onChange={(e) => setValue(e.target.value.replace(/\n/g, ""))}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          e.currentTarget.blur();
        }
        if (e.key === "Escape") {
          setValue(target);
          setTimeout(() => ref.current?.blur());
        }
      }}
    />
  );
};

const FolderChip = ({
  folder,
  onFolder,
}: {
  folder: string | null;
  onFolder: (folder: string | null) => void;
}) => {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(folder ?? "");

  useEffect(() => setValue(folder ?? ""), [folder]);

  const commit = () => {
    setEditing(false);
    const name = value.trim();
    if (name !== (folder ?? "")) onFolder(name || null);
  };

  if (editing)
    return (
      <span className="np-chip editing">
        <Folder size={14} strokeWidth={1.75} />
        <input
          autoFocus
          value={value}
          placeholder="Folder name"
          aria-label="Folder name"
          onChange={(e) => setValue(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            if (e.key === "Escape") {
              setValue(folder ?? "");
              setEditing(false);
            }
          }}
        />
      </span>
    );

  return (
    <button
      type="button"
      className={`np-chip${folder ? "" : " empty"}`}
      onClick={() => setEditing(true)}
    >
      {folder ? (
        <Folder size={14} strokeWidth={1.75} />
      ) : (
        <FolderPlus size={14} strokeWidth={1.75} />
      )}
      {folder ?? "Add to folder"}
    </button>
  );
};
