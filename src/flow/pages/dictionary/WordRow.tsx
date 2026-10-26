import { ArrowRight, Pencil, Star, Trash2 } from "lucide-react";
import { ListRow, RowAction } from "../../ui";
import type { DictionaryEntry } from "./useDictionary";

export const WordRow = ({
  entry,
  forceHover = false,
  onEdit,
  onDelete,
  onStar,
}: {
  entry: DictionaryEntry;
  forceHover?: boolean;
  onEdit: () => void;
  onDelete: () => void;
  onStar: () => void;
}) => (
  <ListRow
    className={forceHover ? "bg-[var(--fk-surface)]" : ""}
    showActions={forceHover || entry.starred}
    actions={
      <>
        <span
          className={`flex items-center gap-[2px] transition-opacity duration-[var(--fk-t-quick)] ${forceHover ? "" : "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"}`}
        >
          <RowAction label="Edit" onClick={onEdit}>
            <Pencil size={16} strokeWidth={1.75} />
          </RowAction>
          <RowAction label="Delete" onClick={onDelete}>
            <Trash2 size={16} strokeWidth={1.75} />
          </RowAction>
        </span>
        <RowAction label={entry.starred ? "Unstar" : "Star"} onClick={onStar}>
          <Star
            size={16}
            strokeWidth={1.75}
            className={entry.starred ? "text-[var(--fk-accent)]" : ""}
            fill={entry.starred ? "var(--fk-accent-soft)" : "none"}
          />
        </RowAction>
      </>
    }
  >
    <div className="flex min-w-0 items-center gap-[8px] whitespace-pre">
      {entry.replacement !== undefined ? (
        <>
          <span className="truncate text-[var(--fk-text-muted)]">{entry.word}</span>
          <ArrowRight
            size={14}
            strokeWidth={1.75}
            className="shrink-0 text-[var(--fk-text-faint)]"
            aria-label="becomes"
          />
          <span className="truncate">{entry.replacement}</span>
        </>
      ) : (
        <span className="truncate">{entry.word}</span>
      )}
      {entry.learned && (
        <span
          role="img"
          aria-label="Learned automatically"
          title="Learned automatically"
          className="h-[6px] w-[6px] shrink-0 rounded-full bg-[var(--fk-accent)]"
        />
      )}
    </div>
  </ListRow>
);
