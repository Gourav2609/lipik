import { ChevronRight } from "lucide-react";
import { formatCount } from "./stats";

const Figure = ({ value, label }: { value: string; label: string }) => (
  <span className="flex items-baseline gap-[6px] whitespace-nowrap border-l border-[var(--fk-border)] pl-[16px] first:border-l-0 first:pl-0">
    <span className="fk-display fk-num text-[20px] leading-[28px] text-[var(--fk-text)]">
      {value}
    </span>
    <span className="text-[13px] leading-[18px]">{label}</span>
  </span>
);

/** Totals beside the greeting; the full numbers live on the Stats page. */
export const HomeStats = ({
  totalWords,
  wpm,
  streak,
  onOpen,
}: {
  totalWords: number;
  wpm: number | null;
  streak: number;
  onOpen: () => void;
}) => (
  <button
    type="button"
    onClick={onOpen}
    title="Open stats"
    className="group flex items-center gap-[16px] rounded-[10px] py-[4px] pl-[12px] pr-[8px] text-[var(--fk-text-muted)] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-hover)]"
  >
    <Figure
      value={formatCount(totalWords)}
      label={totalWords === 1 ? "word" : "words"}
    />
    <Figure value={wpm === null ? "—" : String(wpm)} label="words a minute" />
    <Figure
      value={String(streak)}
      label={streak === 1 ? "day in a row" : "days in a row"}
    />
    <ChevronRight
      size={16}
      strokeWidth={1.75}
      className="shrink-0 transition-colors duration-[var(--fk-t-quick)] group-hover:text-[var(--fk-accent)]"
    />
  </button>
);
