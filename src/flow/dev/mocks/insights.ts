import type { HistoryEntry, Insights } from "@/bindings";
import type { MockHandler } from "../mockTauri";

// Richer history for ?page=insights so the streak grid and totals have
// something to show: a 7-day run ending yesterday and an older 10-day run.
// Other pages keep the default 8 entries.

const TEXTS = [
  "Please move the planning call to the afternoon and add the updated agenda to the shared folder before then.",
  "The new layout looks good on the large screen, but the side menu still overlaps the content on smaller windows.",
  "Thanks for the quick turnaround. I will read through the notes tonight and send my comments in the morning.",
  "Remember to water the plants, pick up the dry cleaning and book a table for Saturday evening.",
  "Let's keep the first release small, ship it to a few people, and collect feedback before adding more settings.",
  "Write a short summary of the test results and list the three issues we still need to look at next week.",
];

// Share of the busiest day's words, by days before today.
const DAY_SHARE: Record<number, number> = {
  1: 0.3, 2: 0.55, 3: 0.2, 4: 1, 5: 0.6, 6: 0.12, 7: 0.4,
  16: 0.5,
  30: 0.25, 31: 0.7, 32: 0.45, 33: 0.9, 34: 0.15, 35: 0.35, 36: 0.8,
  37: 0.5, 38: 0.3, 39: 0.6,
  52: 0.2,
};
const BUSIEST_DAY_WORDS = 1400;

const makeEntries = (): HistoryEntry[] => {
  const now = new Date();
  const entries: HistoryEntry[] = [];
  let id = 5000;
  for (const [ago, share] of Object.entries(DAY_SHARE)) {
    const day = new Date(now.getFullYear(), now.getMonth(), now.getDate() - Number(ago), 9);
    let words = Math.round(BUSIEST_DAY_WORDS * share);
    let i = 0;
    while (words > 0) {
      const text = TEXTS[(id + i) % TEXTS.length];
      words -= text.split(" ").length;
      entries.push({
        id: id--,
        file_name: `insights-${id}.wav`,
        timestamp: Math.floor(day.getTime() / 1000) + i * 300,
        saved: false,
        title: `Recording ${id}`,
        transcription_text: text,
        post_processed_text: null,
        post_process_prompt: null,
        post_process_requested: false,
      });
      i++;
    }
  }
  return entries.sort((a, b) => b.id - a.id);
};

const params = new URLSearchParams(window.location.search);
const isInsights = params.get("page") === "insights";
const entries = makeEntries();

const pad = (n: number) => String(n).padStart(2, "0");
const localDate = (ts: number) => {
  const d = new Date(ts * 1000);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

// Mirrors get_insights for the sample rows (the page works out streaks from
// daily_words itself). Rows made before the backend
// recorded durations have no wpm, fixes or app, so those stay empty unless
// ?insightsData=1 asks for sample numbers.
const makeInsights = (): Insights => {
  const byDay = new Map<string, number>();
  for (const e of entries) {
    const words = e.transcription_text.split(/\s+/).filter(Boolean).length;
    byDay.set(localDate(e.timestamp), (byDay.get(localDate(e.timestamp)) ?? 0) + words);
  }
  const daily_words = [...byDay].sort(([a], [b]) => a.localeCompare(b)).map(([date, words]) => ({ date, words }));
  const total_words = daily_words.reduce((sum, d) => sum + d.words, 0);
  const sample = params.get("insightsData") === "1";
  return {
    wpm: sample ? 142 : null,
    total_words,
    fixes_total: sample ? 57 : 0,
    words_corrected: sample ? 38 : 0,
    dictionary_fixes: sample ? 19 : 0,
    by_category: sample
      ? { ai: 42, email: 18, work: 61, personal: 25, document: 9, other: 14 }
      : { ai: 0, email: 0, work: 0, personal: 0, document: 0, other: 0 },
    apps_used: sample ? 11 : 0,
    streak_days: 0,
    longest_streak: 0,
    daily_words,
  };
};

// Home shows the same totals as Stats, so get_insights answers on every page.
export const handlers: Record<string, MockHandler> = isInsights
  ? {
      get_history_entries: ({ cursor, limit }) => {
        const from = typeof cursor === "number" ? entries.findIndex((e) => e.id < cursor) : 0;
        const size = typeof limit === "number" ? limit : entries.length;
        const page = from < 0 ? [] : entries.slice(from, from + size);
        return { entries: page, has_more: from >= 0 && from + size < entries.length };
      },
      get_insights: makeInsights,
    }
  : { get_insights: makeInsights };
