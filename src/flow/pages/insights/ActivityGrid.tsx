import { useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { IconButton } from "../../ui";
import { dayKey, type InsightsStats } from "./useInsightsStats";

const WEEKS = 18;
const CELL = 16;
const GAP = 4;
const LABEL_W = 32;
const DAY_LABELS = ["", "Mon", "", "Wed", "", "Fri", ""];
const LEVELS = [28, 50, 75, 100].map(
  (p) => `color-mix(in srgb, var(--fk-accent) ${p}%, var(--fk-surface-2))`,
);

const level = (words: number, max: number) => {
  const r = words / max;
  if (r >= 0.5) return 3;
  if (r >= 0.25) return 2;
  if (r >= 0.1) return 1;
  return 0;
};

const month = (d: Date) => d.toLocaleString("en-US", { month: "short" });
const dayMonth = (d: Date) => `${d.getDate()} ${month(d)}`;
const dayLabel = (d: Date) =>
  `${d.toLocaleString("en-US", { weekday: "short" })} ${dayMonth(d)}`;

const StreakDot = ({ onInk }: { onInk: boolean }) => (
  <span
    className="h-[4px] w-[4px] rounded-full"
    style={{ background: onInk ? "var(--fk-panel)" : "var(--fk-accent)" }}
  />
);

export const ActivityGrid = ({
  stats,
  loaded,
}: {
  stats: InsightsStats;
  loaded: boolean;
}) => {
  const [page, setPage] = useState(0);
  const { today } = stats;
  const todayKey = dayKey(today);
  const firstSunday = new Date(
    today.getFullYear(),
    today.getMonth(),
    today.getDate() - today.getDay() - (WEEKS - 1 + page * WEEKS) * 7,
  );
  const max = Math.max(1, ...stats.wordsByDay.values());
  const hasOlder = stats.firstDay !== null && stats.firstDay < firstSunday;

  const weeks = Array.from({ length: WEEKS }, (_, w) =>
    Array.from(
      { length: 7 },
      (_, d) =>
        new Date(
          firstSunday.getFullYear(),
          firstSunday.getMonth(),
          firstSunday.getDate() + w * 7 + d,
        ),
    ),
  );
  const lastDay = weeks[WEEKS - 1][6];

  return (
    <section aria-label="Activity">
      <div className="flex h-[30px] items-center gap-[8px]">
        <h2 className="text-[16px] font-semibold leading-[22px]">Activity</h2>
        <span className="fk-num text-[13px] text-[var(--fk-text-faint)]">
          {dayMonth(firstSunday)} – {dayMonth(page === 0 ? today : lastDay)}
        </span>
        <div className="ml-auto flex gap-[4px]">
          <IconButton
            label="Earlier"
            disabled={!hasOlder}
            onClick={() => setPage(page + 1)}
            className="disabled:pointer-events-none disabled:opacity-40"
          >
            <ChevronLeft size={16} strokeWidth={1.75} />
          </IconButton>
          <IconButton
            label="Later"
            disabled={page === 0}
            onClick={() => setPage(page - 1)}
            className="disabled:pointer-events-none disabled:opacity-40"
          >
            <ChevronRight size={16} strokeWidth={1.75} />
          </IconButton>
        </div>
      </div>

      <div className="mt-[16px] border-t border-[var(--fk-border)] pt-[16px]">
        <div className="relative h-[16px]">
          {weeks.map((week, w) =>
            week[0].getDate() <= 7 ? (
              <span
                key={w}
                className="absolute top-0 text-[12px] leading-[16px] text-[var(--fk-text-muted)]"
                style={{ left: LABEL_W + w * (CELL + GAP) }}
              >
                {month(week[0])}
              </span>
            ) : null,
          )}
        </div>

        <div className="mt-[8px] flex">
          <div className="flex flex-col" style={{ width: LABEL_W, gap: GAP }}>
            {DAY_LABELS.map((d, i) => (
              <div
                key={i}
                className="flex items-center text-[12px] leading-none text-[var(--fk-text-faint)]"
                style={{ height: CELL }}
              >
                {d}
              </div>
            ))}
          </div>
          <div className="flex" style={{ gap: GAP }}>
            {weeks.map((week, w) => (
              <div key={w} className="flex flex-col" style={{ gap: GAP }}>
                {week.map((day, d) => {
                  const key = dayKey(day);
                  const words = stats.wordsByDay.get(key) ?? 0;
                  const future = day > today;
                  const lvl = words ? level(words, max) : -1;
                  return (
                    <div
                      key={d}
                      title={
                        future
                          ? undefined
                          : `${dayLabel(day)}: ${words.toLocaleString("en-US")} words`
                      }
                      className={`ins-cell flex items-center justify-center rounded-[4px]${loaded ? " ins-in" : ""}`}
                      style={{
                        width: CELL,
                        height: CELL,
                        background:
                          lvl >= 0 ? LEVELS[lvl] : "var(--fk-surface-2)",
                        boxShadow:
                          key === todayKey && !words
                            ? "inset 0 0 0 1.5px var(--fk-border-strong)"
                            : undefined,
                        animationDelay: `${w * 16}ms`,
                      }}
                    >
                      {stats.streakDays.has(key) && (
                        <StreakDot onInk={lvl >= 2} />
                      )}
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
        </div>

        <div className="mt-[20px] flex items-center justify-between text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
          <div className="flex items-center gap-[4px]">
            <span className="mr-[4px]">Fewer words</span>
            {["var(--fk-surface-2)", ...LEVELS].map((c) => (
              <span
                key={c}
                className="rounded-[4px]"
                style={{ width: 12, height: 12, background: c }}
              />
            ))}
            <span className="ml-[4px]">More</span>
          </div>
          <div className="flex items-center gap-[8px]">
            <span
              className="flex items-center justify-center rounded-[4px]"
              style={{ width: 12, height: 12, background: LEVELS[3] }}
            >
              <StreakDot onInk />
            </span>
            Days in a row
          </div>
        </div>
      </div>
    </section>
  );
};
