import { useState, type ReactNode } from "react";
import { Check, ChevronLeft } from "lucide-react";
import { Popover } from "../../overlays/Popover";
import { Button, TextInput } from "../../ui";
import {
  deleteNote,
  isBusy,
  moveNote,
  openInWindow,
  regenerateNote,
  renameNote,
  type Folder,
  type Note,
} from "./data";

export const MenuItem = ({
  onClick,
  disabled,
  danger,
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  children: ReactNode;
}) => (
  <button
    type="button"
    role="menuitem"
    onClick={onClick}
    disabled={disabled}
    className={`flex h-[32px] w-full items-center gap-[8px] rounded-[8px] px-[10px] text-left text-[14px] transition-colors duration-[var(--fk-t-quick)] enabled:hover:bg-[var(--fk-hover)] disabled:opacity-45 ${danger ? "text-[var(--fk-live)] enabled:hover:bg-[var(--fk-live-soft)]" : ""}`}
  >
    {children}
  </button>
);

const SubHeader = ({
  label,
  onBack,
}: {
  label: string;
  onBack: () => void;
}) => (
  <button
    type="button"
    onClick={onBack}
    className="fk-eyebrow mb-[4px] flex h-[28px] w-full items-center gap-[4px] rounded-[8px] px-[6px] hover:bg-[var(--fk-hover)] hover:text-[var(--fk-text)]"
  >
    <ChevronLeft size={14} />
    {label}
  </button>
);

export type NoteMenuMode = "main" | "rename" | "folder" | "delete";

const WIDTH = 240;

/** Options for one note: open, rename, folder, regenerate, delete. */
export const NoteMenu = ({
  note,
  folders,
  anchor,
  initialMode = "main",
  onClose,
  onOpen,
  onChanged,
  onDeleted,
}: {
  note: Note;
  folders: Folder[];
  anchor: DOMRect;
  initialMode?: NoteMenuMode;
  onClose: () => void;
  onOpen?: () => void;
  onChanged: () => void;
  onDeleted?: () => void;
}) => {
  const [mode, setMode] = useState<NoteMenuMode>(initialMode);
  const [title, setTitle] = useState(note.title);
  const [folderName, setFolderName] = useState("");

  const run = async (action: () => Promise<boolean>) => {
    onClose();
    if (await action()) onChanged();
  };

  const rename = () => {
    const next = title.trim();
    if (!next || next === note.title) return onClose();
    void run(() => renameNote(note.id, next));
  };

  const move = (folder: string | null) => {
    if (folder === note.folder) return onClose();
    void run(() => moveNote(note.id, folder));
  };

  const remove = async () => {
    onClose();
    if (!(await deleteNote(note.id))) return;
    onDeleted?.();
    onChanged();
  };

  const top = Math.min(anchor.bottom + 6, window.innerHeight - 300);
  const left = Math.max(8, anchor.right - WIDTH);

  return (
    <Popover
      onClose={onClose}
      className="p-[4px]"
      style={{ top, left, width: WIDTH }}
    >
      {mode === "main" && (
        <>
          {onOpen && (
            <MenuItem
              onClick={() => {
                onClose();
                onOpen();
              }}
            >
              Open note
            </MenuItem>
          )}
          <MenuItem onClick={() => void run(() => openInWindow(note.id))}>
            Open in window
          </MenuItem>
          <MenuItem onClick={() => setMode("rename")}>Rename</MenuItem>
          <MenuItem onClick={() => setMode("folder")}>
            {note.folder ? "Move to folder" : "Add to folder"}
          </MenuItem>
          <MenuItem
            disabled={isBusy(note)}
            onClick={() => void run(() => regenerateNote(note.id))}
          >
            Write summary again
          </MenuItem>
          <div className="mx-[8px] my-[4px] h-px bg-[var(--fk-border)]" />
          <MenuItem danger onClick={() => setMode("delete")}>
            Delete note
          </MenuItem>
        </>
      )}
      {mode === "rename" && (
        <div className="p-[4px]">
          <SubHeader label="Rename" onBack={() => setMode("main")} />
          <TextInput
            autoFocus
            value={title}
            aria-label="Note title"
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && rename()}
            onFocus={(e) => e.currentTarget.select()}
          />
        </div>
      )}
      {mode === "folder" && (
        <div>
          <SubHeader label="Folder" onBack={() => setMode("main")} />
          {folders.map((f) => (
            <MenuItem key={f.id} onClick={() => move(f.name)}>
              <span className="min-w-0 flex-1 truncate">{f.name}</span>
              {f.name === note.folder && <Check size={15} />}
            </MenuItem>
          ))}
          {note.folder && (
            <MenuItem onClick={() => move(null)}>Remove from folder</MenuItem>
          )}
          <div className="p-[4px]">
            <TextInput
              autoFocus
              value={folderName}
              placeholder="New folder name"
              aria-label="New folder name"
              onChange={(e) => setFolderName(e.target.value)}
              onKeyDown={(e) =>
                e.key === "Enter" &&
                folderName.trim() &&
                move(folderName.trim())
              }
            />
          </div>
        </div>
      )}
      {mode === "delete" && (
        <div className="p-[8px]">
          <p className="text-[14px] leading-[20px]">
            Delete “{note.title}”? Its audio, transcript and notes go with it.
            This can't be undone.
          </p>
          <div className="mt-[12px] flex justify-end gap-[4px]">
            <Button variant="ghost" size="sm" onClick={() => setMode("main")}>
              Keep
            </Button>
            <Button
              variant="danger"
              size="sm"
              role="menuitem"
              onClick={() => void remove()}
            >
              Delete
            </Button>
          </div>
        </div>
      )}
    </Popover>
  );
};
