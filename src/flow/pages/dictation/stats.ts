import type { HistoryEntry } from "@/bindings";

const DAY_MS = 24 * 60 * 60 * 1000;

export const formatCount = (n: number) => {
  if (n < 1000) return String(n);
  if (n < 999_950) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}K`;
  return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
};

const dayStart = (ms: number) => {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
};

export const formatTime = (seconds: number) =>
  new Date(seconds * 1000).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });

export const dayLabel = (day: number) => {
  const today = dayStart(Date.now());
  if (day === today) return "Today";
  if (day === dayStart(today - DAY_MS / 2)) return "Yesterday";
  const date = new Date(day);
  return date.toLocaleDateString("en-GB", {
    weekday: "short",
    day: "numeric",
    month: "short",
    ...(date.getFullYear() === new Date().getFullYear()
      ? {}
      : { year: "numeric" }),
  });
};

export interface DayGroup {
  day: number;
  entries: HistoryEntry[];
}

export const groupByDay = (entries: HistoryEntry[]): DayGroup[] => {
  const groups: DayGroup[] = [];
  for (const entry of entries) {
    const day = dayStart(entry.timestamp * 1000);
    const last = groups[groups.length - 1];
    if (last?.day === day) last.entries.push(entry);
    else groups.push({ day, entries: [entry] });
  }
  return groups;
};
