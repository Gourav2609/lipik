import { ChevronLeft, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";
import { DeleteButton } from "./DeleteButton";
import { formatUpdated, useScratchpad, type Note } from "./store";

export const NoteEditor = ({
  note,
  onBack,
}: {
  note: Note;
  onBack: () => void;
}) => {
  const updateNote = useScratchpad((s) => s.updateNote);
  const deleteNote = useScratchpad((s) => s.deleteNote);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const titleRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!note.title && !note.body) titleRef.current?.focus();
  }, [note.id]);

  useEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [note.body]);

  return (
    <div
      className="fk-page select-text"
      onKeyDown={(e) => e.key === "Escape" && onBack()}
    >
      <div className="mx-auto max-w-[760px]">
        <div className="flex h-[36px] items-center justify-between">
          <button
            type="button"
            onClick={onBack}
            className="-ml-[8px] flex h-[30px] items-center gap-[4px] rounded-[8px] pl-[4px] pr-[10px] text-[14px] font-medium text-[var(--fk-text-muted)] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-hover)] hover:text-[var(--fk-text)]"
          >
            <ChevronLeft size={18} strokeWidth={1.75} />
            Drafts
          </button>
          <div className="flex items-center gap-[12px] text-[var(--fk-text-muted)]">
            <span className="fk-num text-[13px] text-[var(--fk-text-faint)]">
              Edited {formatUpdated(note.updatedAt)}
            </span>
            <DeleteButton
              onConfirm={() => {
                deleteNote(note.id);
                onBack();
              }}
            >
              <Trash2 size={16} strokeWidth={1.75} />
            </DeleteButton>
          </div>
        </div>
        <input
          ref={titleRef}
          value={note.title}
          onChange={(e) => updateNote(note.id, { title: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              bodyRef.current?.focus();
            }
          }}
          placeholder="Untitled"
          className="fk-display mt-[24px] w-full border-b border-[var(--fk-border)] bg-transparent pb-[16px] text-[28px] leading-[38px] text-[var(--fk-text)] outline-none placeholder:text-[var(--fk-text-faint)] focus-visible:shadow-none"
        />
        <textarea
          ref={bodyRef}
          value={note.body}
          onChange={(e) => updateNote(note.id, { body: e.target.value })}
          placeholder="Start writing, or hold fn and speak."
          className="fk-ruled mt-[16px] min-h-[320px] w-full resize-none overflow-hidden bg-transparent text-[16px] leading-[28px] text-[var(--fk-text)] outline-none placeholder:text-[var(--fk-text-faint)] focus-visible:shadow-none"
        />
      </div>
    </div>
  );
};
