import type { HistoryEntry } from "@/bindings";
import type { MockHandler } from "../mockTauri";

// Sample history: nothing today, a long multi-paragraph entry yesterday,
// then short rows over the previous days.
const LONG = [
  "Picking up from the planning call on Monday. The first draft said the rollout would take four to five weeks, but the second estimate was closer to three. If we keep the review step, I think three and a half is a fair number to share with the team.",
  "Both teams should agree on the same checklist before Friday, so nobody has to guess which version is current. If something is still missing by then, let's raise it in the weekly sync instead of waiting on email.",
  "The next question is what it takes to get the new layout ready for the demo on Monday. I think the steps are right. Can we get the design review on the calendar this week? Do we have someone who can test it on a smaller screen? If not, I can do a quick pass myself tonight.",
  "Let's also send a short summary to the people who missed the call. They mostly need the dates and the two open questions, not the whole discussion.",
  "After that, the plan is simple. First we finish the last spacing fixes on the settings screen, then we record a short walkthrough of the new flow. Once that is done, we can share it with the group and ask for feedback by Thursday evening. If anything big comes up, we move the demo by a day rather than rushing it, because a calm demo is worth more than an early one.",
  "That is one step. The second is the release notes. We said the old export screen was not working at all, so it is worth checking why that happened and whether the fix covers every case. If it does, we mention it in the notes; if not, we keep it out until it does.",
].join("\n\n");

const ENTRIES: [number, number, number, string][] = [
  [1, 10, 12, LONG],
  [1, 10, 8, "So, what did we decide about the new colors for the settings screen?"],
  [1, 10, 7, "Nothing is blocking me on the review. I can see zero open comments."],
  [1, 0, 44, "Look at how the sidebar collapses on a narrow window and see how quickly it settles. What exactly are we missing here?"],
  [1, 0, 41, "How is the new search working here? Is it fast, or is it slow for you?"],
  [1, 0, 41, "So, can you show me?"],
  [2, 20, 36, "Also, make sure the backup runs every night and not every hour, okay? Keep the old copies for one week."],
  [2, 20, 29, "And don't limit yourself to ten examples. You can keep adding them until the list feels complete. When it gets too long, we can trim it down later, but for now just keep going."],
  [2, 20, 28, "Add a short note to the release page. Even small fixes are worth listing, because people read the notes before they update, right? Just keep each one to a line."],
  [2, 20, 17, "If I don't reply in twenty minutes, go ahead and send it."],
  [2, 20, 9, "Can we start a short guide on keyboard shortcuts for new users?"],
  [2, 20, 9, ""],
  [2, 19, 56, "Can you pick ten sample files and check that every one of them opens?"],
  [2, 19, 38, "Can we move faster if we skip the export step and only work on the preview part? Would that save us a day?"],
  [2, 19, 32, "This build is taking a long time today."],
  [2, 19, 32, "Why can't we sort the list by date instead?"],
  [2, 19, 30, "Share a list with me of the pages that were changed this week so I can review them in the right order."],
  [2, 19, 28, "Can you open the other settings window, please?"],
  [2, 16, 58, "Okay, I think this is fine. Let's keep it."],
  [2, 16, 56, "And what about the welcome screen and the empty states for search? We also need to look at those before the demo, right?"],
];

const at = (daysAgo: number, hour: number, minute: number) => {
  const d = new Date();
  d.setDate(d.getDate() - daysAgo);
  d.setHours(hour, minute, 0, 0);
  return Math.floor(d.getTime() / 1000);
};

const history: HistoryEntry[] = ENTRIES.map(([daysAgo, h, m, text], i) => ({
  id: 500 - i,
  file_name: `sample-${i}.wav`,
  timestamp: at(daysAgo, h, m),
  saved: false,
  title: `Recording ${i + 1}`,
  transcription_text: text,
  post_processed_text: null,
  post_process_prompt: null,
  post_process_requested: false,
}));

// A silent 8 kHz WAV whose length matches a ~130 wpm reading of the entry,
// so the page's wpm stat runs through the same audio-duration path as the app.
const silentWav = (seconds: number) => {
  const rate = 8000;
  const n = Math.round(seconds * rate);
  const buf = new DataView(new ArrayBuffer(44 + n));
  const str = (o: number, s: string) =>
    [...s].forEach((c, i) => buf.setUint8(o + i, c.charCodeAt(0)));
  str(0, "RIFF");
  buf.setUint32(4, 36 + n, true);
  str(8, "WAVEfmt ");
  buf.setUint32(16, 16, true);
  buf.setUint16(20, 1, true);
  buf.setUint16(22, 1, true);
  buf.setUint32(24, rate, true);
  buf.setUint32(28, rate, true);
  buf.setUint16(32, 1, true);
  buf.setUint16(34, 8, true);
  str(36, "data");
  buf.setUint32(40, n, true);
  for (let i = 0; i < n; i++) buf.setUint8(44 + i, 128);
  return URL.createObjectURL(new Blob([buf], { type: "audio/wav" }));
};

const page = new URLSearchParams(window.location.search).get("page");
const isDictation = page == null || page === "dictation";

export const handlers: Record<string, MockHandler> = isDictation
  ? {
      get_history_entries: ({ cursor }) => ({
        entries: cursor == null ? history : [],
        has_more: false,
      }),
      get_audio_file_path: ({ fileName }) => {
        const entry = history.find((e) => e.file_name === fileName);
        if (!entry) return "";
        const words = entry.transcription_text.split(/\s+/).length;
        return silentWav((words / 130) * 60);
      },
      update_history_text: ({ id, text }) => {
        const entry = history.find((e) => e.id === id);
        if (!entry) return null;
        const next = String(text).trim();
        entry.corrected_text = next === entry.transcription_text.trim() ? null : next;
        return { entry: { ...entry }, learned_words: [], learned_replacements: [] };
      },
    }
  : {};
