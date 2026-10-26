import { useEffect, useMemo, useState } from "react";
import { commands, events, type Insights } from "@/bindings";
import { isFlowPreview } from "../../dev/preview";
import { unwrap } from "../textflow";
import { APP_TYPE_IDS, type AppType, type AppUsage } from "./AppUsage";

export const dayKey = (d: Date) =>
  `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`;

const parseDayKey = (key: string) => {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, m - 1, d);
};

const addDays = (d: Date, n: number) =>
  new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);

const startOfDay = (d: Date) =>
  new Date(d.getFullYear(), d.getMonth(), d.getDate());

/**
 * Words per local day (keyed by dayKey()) that the page counted itself before
 * the backend kept every dictation. History then held only the latest rows,
 * so these days can hold words the backend no longer has. Read only.
 */
const OLD_LEDGER_KEY = "fk.insights.ledger.v1";

const readOldLedgerDays = (): Record<string, number> => {
  if (isFlowPreview) return {};
  try {
    const parsed = JSON.parse(localStorage.getItem(OLD_LEDGER_KEY) ?? "null");
    return parsed?.days && typeof parsed.days === "object" ? parsed.days : {};
  } catch {
    return {};
  }
};

/** Backend days are "YYYY-MM-DD"; the page keys days with dayKey(). */
const backendDays = (daily: Insights["daily_words"]) => {
  const days: Record<string, number> = {};
  for (const { date, words } of daily) {
    const [y, m, d] = date.split("-").map(Number);
    if (words > 0) days[dayKey(new Date(y, m - 1, d))] = words;
  }
  return days;
};

/** A day the old ledger counted can only be missing words in the backend, never extra. */
const mergeDays = (a: Record<string, number>, b: Record<string, number>) => {
  const out = { ...a };
  for (const [k, v] of Object.entries(b)) out[k] = Math.max(out[k] ?? 0, v);
  return out;
};

export interface InsightsStats {
  totalWords: number;
  /** Words per local day, keyed by dayKey(); days with 0 words are absent. */
  wordsByDay: Map<string, number>;
  currentStreak: number;
  longestStreak: number;
  /** Days that make up the current streak, keyed by dayKey(). */
  streakDays: Set<string>;
  firstDay: Date | null;
  today: Date;
}

export const computeStats = (
  days: Record<string, number>,
  today: Date,
): InsightsStats => {
  const wordsByDay = new Map(Object.entries(days));
  let totalWords = 0;
  for (const words of wordsByDay.values()) totalWords += words;

  const streakDays = new Set<string>();
  let cursor = wordsByDay.has(dayKey(today)) ? today : addDays(today, -1);
  while (wordsByDay.has(dayKey(cursor))) {
    streakDays.add(dayKey(cursor));
    cursor = addDays(cursor, -1);
  }

  const sorted = [...wordsByDay.keys()]
    .map(parseDayKey)
    .sort((a, b) => a.getTime() - b.getTime());
  let longestStreak = 0;
  let run = 0;
  sorted.forEach((d, i) => {
    // Compare calendar days, not 24 h steps, so DST changes don't break a run.
    const follows = i > 0 && dayKey(addDays(sorted[i - 1], 1)) === dayKey(d);
    run = follows ? run + 1 : 1;
    longestStreak = Math.max(longestStreak, run);
  });

  return {
    totalWords,
    wordsByDay,
    currentStreak: streakDays.size,
    longestStreak,
    streakDays,
    firstDay: sorted[0] ?? null,
    today,
  };
};

const useToday = () => {
  const [today, setToday] = useState(() => startOfDay(new Date()));
  useEffect(() => {
    const untilTomorrow = addDays(today, 1).getTime() - Date.now() + 1000;
    const timer = setTimeout(() => setToday(startOfDay(new Date())), untilTomorrow);
    return () => clearTimeout(timer);
  }, [today]);
  return today;
};

// Insights numbers are written on a background thread after the history row.
const AFTER_HISTORY_MS = 1500;

export interface InsightsData {
  /** False until the first getInsights() call settles; the entry motion waits for it. */
  loaded: boolean;
  stats: InsightsStats;
  /** null until a dictation has the new per-dictation numbers. */
  wpm: number | null;
  wordsCorrected: number | null;
  dictionaryFixes: number | null;
  usage: AppUsage | null;
}

export const useInsightsStats = (): InsightsData => {
  const [insights, setInsights] = useState<Insights | null>(null);
  const [settled, setSettled] = useState(false);
  const [oldDays] = useState(readOldLedgerDays);
  const today = useToday();

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const load = () =>
      commands
        .getInsights()
        .then((res) => {
          if (alive) setInsights(unwrap(res));
        })
        .catch((err) => console.error("Failed to load insights:", err))
        .finally(() => {
          if (alive) setSettled(true);
        });
    void load();
    const unlisten = events.historyUpdatePayload.listen(({ payload }) => {
      if (payload.action === "toggled") return;
      clearTimeout(timer);
      timer = setTimeout(load, AFTER_HISTORY_MS);
    });
    return () => {
      alive = false;
      clearTimeout(timer);
      unlisten.then((fn) => fn());
    };
  }, []);

  return useMemo(() => {
    const days = mergeDays(oldDays, backendDays(insights?.daily_words ?? []));
    const stats = computeStats(days, today);
    const measured = insights !== null && insights.wpm !== null;
    const counts = Object.fromEntries(
      APP_TYPE_IDS.map((t) => [t, insights?.by_category[t] ?? 0]),
    ) as Record<AppType, number>;
    return {
      loaded: settled,
      stats,
      wpm: measured ? Math.round(insights.wpm!) : null,
      wordsCorrected: measured ? insights.words_corrected : null,
      dictionaryFixes: measured ? insights.dictionary_fixes : null,
      usage: measured ? { counts, appsUsed: insights.apps_used } : null,
    };
  }, [insights, settled, oldDays, today]);
};
