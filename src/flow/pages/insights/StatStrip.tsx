import type { ReactNode } from "react";
import type { InsightsStats } from "./useInsightsStats";

/** `null` = no dictation has recorded this value yet. */
const fmt = (n: number | null) =>
  n === null ? "—" : n.toLocaleString("en-US");

const PENDING = "Shows after your next dictation";

const Figure = ({
  value,
  label,
  note,
}: {
  value: number | null;
  label: string;
  note: ReactNode;
}) => (
  <div className="min-w-0 border-l border-[var(--fk-border)] px-[24px] first:border-l-0 first:pl-0">
    <div className="fk-display fk-num text-[32px] leading-[36px]">
      {fmt(value)}
    </div>
    <div className="mt-[8px] text-[14.5px] font-medium leading-[20px]">
      {label}
    </div>
    <div className="mt-[2px] truncate text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
      {note}
    </div>
  </div>
);

const since = (d: Date) =>
  `Since ${d.getDate()} ${d.toLocaleString("en-US", { month: "short" })} ${d.getFullYear()}`;

const days = (n: number) => `${n} ${n === 1 ? "day" : "days"}`;

export const StatStrip = ({
  stats,
  wpm,
  wordsCorrected,
  dictionaryFixes,
}: {
  stats: InsightsStats;
  wpm: number | null;
  wordsCorrected: number | null;
  dictionaryFixes: number | null;
}) => (
  <section
    aria-label="Totals"
    className="grid grid-cols-4 border-b border-[var(--fk-border)] pb-[24px]"
  >
    <Figure
      value={stats.totalWords}
      label="Words written"
      note={stats.firstDay ? since(stats.firstDay) : "Nothing written yet"}
    />
    <Figure
      value={stats.currentStreak}
      label="Days in a row"
      note={`Longest ${days(stats.longestStreak)}`}
    />
    <Figure
      value={wpm}
      label="Words per minute"
      note={wpm === null ? PENDING : "Your average while dictating"}
    />
    <Figure
      value={wordsCorrected}
      label="Words cleaned up"
      note={
        dictionaryFixes === null ? (
          PENDING
        ) : (
          <>
            <span className="fk-num">{fmt(dictionaryFixes)}</span> spelled from
            your vocabulary
          </>
        )
      }
    />
  </section>
);
