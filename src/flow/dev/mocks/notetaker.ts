import type {
  Attendee,
  CalendarEvent,
  ChatMessage,
  Citation,
  Meeting,
  MeetingDetail,
  NotetakerFilter,
  NoteSummary,
  Segment,
} from "@/bindings";
import type { MockHandler } from "../mockTauri";

// Notetaker page in the preview, with neutral sample meetings.
// ?ntEmpty=1 starts with no notes; ?ntRecording=1 adds a meeting being recorded.
// Calendar access comes from ./notetakerSettings.ts (?calendar=full_access).
const params = new URLSearchParams(window.location.search);

const person = (name: string, extra: Partial<Attendee> = {}): Attendee => ({
  name,
  email: `${name.toLowerCase().replace(/[^a-z0-9]+/g, ".")}@example.com`,
  is_me: false,
  ...extra,
});

const ALEX = person("Arjun Mehta");
const JORDAN = person("Neha Kapoor");
const ROOM = person("Studio-Room-2B (6)");
const YOU = person("Asha Rao", { is_me: true });
const KAI = person("Kabir Shah");
const RILEY = person("Ritu Nair");
const NOVA = person("Naveen Iyer");
const KIT = person("Kavya Das");
const DREW = person("Dev Patel");
const UMA = person("Uma Menon");
const TEAM = person("Design team");

const STANDUP = [ALEX, ROOM, YOU, KAI, RILEY, NOVA, KIT, DREW];
const BIG = [
  UMA,
  JORDAN,
  ...Array.from({ length: 23 }, (_, i) => person(`Guest ${i + 1}`)),
];

const at = (dayOffset: number, h: number, m = 0) => {
  const d = new Date();
  d.setDate(d.getDate() + dayOffset);
  d.setHours(h, m, 0, 0);
  return d.getTime();
};

const event = (
  id: string,
  title: string,
  start: number,
  end: number,
  attendees: Attendee[],
): CalendarEvent => ({
  id,
  title,
  start_ms: start,
  end_ms: end,
  all_day: false,
  attendees,
  organizer: attendees[0] ?? null,
  meeting_url: "https://meet.example.com/abc-defg-hij",
  location: null,
  calendar: "Work",
});

const EVENTS: CalendarEvent[] = [
  event("m1", "Team Standup", at(2, 11, 30), at(2, 12), STANDUP),
  event("m2", "Design Sync", at(2, 18), at(2, 18, 30), BIG),
  event("m3", "Team Standup", at(3, 11, 30), at(3, 12), STANDUP),
  event("m4", "Roadmap Review", at(4, 15), at(4, 16), [YOU, KAI]),
];

const meeting = (
  id: string,
  title: string,
  start: number,
  minutes: number,
  extra: Partial<Meeting> = {},
): Meeting => ({
  id,
  title,
  title_auto: true,
  start_ms: start,
  end_ms: start + minutes * 60_000,
  app: "Google Meet",
  bundle_id: null,
  calendar_event_id: null,
  attendees: [],
  folder: null,
  status: "ready",
  summary_status: "ready",
  summary_error: null,
  mic_offset_ms: 0,
  system_offset_ms: 0,
  ...extra,
});

const lines = (
  rows: [speaker: string, seconds: number, text: string][],
): Segment[] =>
  rows.map(([speaker, s, text], i) => ({
    source: speaker === "Me" ? "mic" : "system",
    speaker,
    start_ms: s * 1000,
    end_ms: (rows[i + 1]?.[1] ?? s + 8) * 1000,
    text,
  }));

type Sample = {
  meeting: Meeting;
  segments: Segment[];
  thoughts: string;
  summary: NoteSummary | null;
};

const records: Sample[] = [
  {
    meeting: meeting("n1", "Storage Costs and Backup Planning", at(-1, 18, 17), 24, {
      folder: "Infrastructure",
    }),
    segments: lines([
      ["Me", 4, "Okay, let's start with the storage bill. It doubled since last quarter."],
      ["Ritu Nair", 12, "Most of that is logs. We keep every request log in the main database for ninety days."],
      ["Ritu Nair", 21, "Moving them to cheaper storage would cut the cost a lot, but search gets slower."],
      ["Kabir Shah", 33, "We also need to decide how often backups run. Right now it's once a night."],
      ["Me", 45, "Let's look at both and come back next week with numbers."],
    ]),
    thoughts: "Check how long we really need request logs.",
    summary: {
      title: null,
      overview:
        "The team weighed keeping logs in the database against cheaper storage; Ritu flagged the cost, and nothing was decided.",
      sections: [
        {
          heading: "Storage costs",
          bullets: [
            "The storage bill doubled since last quarter, mostly from request logs.",
            "Logs stay in the main database for ninety days.",
            "Cheaper storage would cut the cost but make log search slower.",
          ],
        },
        {
          heading: "Backups",
          bullets: ["Backups run once a night; the team will review how often they should run."],
        },
      ],
      key_points: [],
      decisions: [],
      action_items: [
        { text: "Bring cost numbers for both storage options", owner: "Ritu Nair", due: "Next week" },
        { text: "Propose a backup schedule", owner: "Kabir Shah", due: null },
      ],
    },
  },
  {
    meeting: meeting("n2", "Customer Feedback Review", at(-1, 17, 30), 41, {
      attendees: [RILEY, DREW, TEAM, ALEX, KAI, NOVA, KIT, YOU],
    }),
    segments: lines([
      ["Dev Patel", 3, "We got forty new survey answers this week."],
      ["Me", 11, "What was the most common request?"],
      ["Dev Patel", 16, "Faster search, then a dark theme for the mobile app."],
      ["Naveen Iyer", 27, "The dark theme is almost done. Search needs more work."],
    ]),
    thoughts: "",
    summary: {
      title: null,
      overview:
        "Dev shared this week's survey answers. Faster search and a dark theme were the top requests, and Naveen said the dark theme is nearly done.",
      sections: [
        {
          heading: "Survey results",
          bullets: ["Forty new answers this week.", "Top requests: faster search, then a mobile dark theme."],
        },
      ],
      key_points: ["Search speed is the most requested change."],
      decisions: ["Ship the dark theme first."],
      action_items: [{ text: "Share the survey summary with the team", owner: "Dev Patel", due: "Friday" }],
    },
  },
  {
    meeting: meeting("n3", "Quarterly Planning Notes", at(-1, 17, 12), 12, {
      summary_status: "unavailable",
      summary_error: "Ollama isn't running. Start it, then choose Try again.",
    }),
    segments: lines([
      ["Me", 2, "Three goals for next quarter: faster onboarding, fewer support tickets, and the new pricing page."],
      ["Them", 14, "The pricing page depends on the billing change, so it should come last."],
    ]),
    thoughts: "",
    summary: null,
  },
  {
    meeting: meeting("n4", "Onboarding Walkthrough", at(-2, 15, 5), 18, {
      folder: "Onboarding",
    }),
    segments: lines([
      ["Uma Menon", 5, "Let's walk through the first-day checklist."],
      ["Me", 13, "The second section still points to the old setup guide."],
    ]),
    thoughts: "",
    summary: {
      title: null,
      overview: "Uma walked through the first-day checklist; the second section still links to the old setup guide.",
      sections: [{ heading: "Checklist", bullets: ["Update the setup guide link in section two."] }],
      key_points: [],
      decisions: [],
      action_items: [],
    },
  },
];

if (params.get("ntRecording") === "1")
  records.unshift({
    meeting: meeting("n0", "New note", Date.now() - 4 * 60_000, 0, {
      end_ms: null,
      status: "recording",
      summary_status: "pending",
    }),
    segments: lines([
      ["Me", 3, "Thanks for joining. Let's go through the launch checklist."],
      ["Them", 11, "Sure. The release notes are ready for review."],
    ]),
    thoughts: "",
    summary: null,
  });

let library = params.get("ntEmpty") === "1" ? [] : records;

const find = (id: unknown) => library.find((r) => r.meeting.id === id);

const matches = (r: Sample, query: string) =>
  [r.meeting.title, r.thoughts, r.summary?.overview ?? "", ...r.segments.map((s) => s.text)]
    .join("\n")
    .toLowerCase()
    .includes(query.toLowerCase());

const chats: ChatMessage[] = [];
let nextMessage = 1;

const answerFor = (meetingId: string | null): { content: string; citations: Citation[] } => {
  const cited = meetingId ? library.filter((r) => r.meeting.id === meetingId) : library.slice(0, 3);
  const citations = cited.map(
    (r, i): Citation => ({
      marker: i + 1,
      meeting_id: r.meeting.id,
      meeting_title: r.meeting.title,
      start_ms: r.segments[1]?.start_ms ?? 0,
      quote: r.segments[1]?.text ?? "",
    }),
  );
  if (!citations.length)
    return { content: "I couldn't find any meetings to answer from yet.", citations };
  const content = [
    "Here's what came up in your recent meetings:",
    "",
    ...cited.map(
      (r, i) =>
        `- **${r.meeting.title}**: ${r.summary?.overview ?? "No notes were written for this meeting yet."} [${i + 1}]`,
    ),
    "",
    "Nothing in these meetings says a final decision was made on storage.",
  ].join("\n");
  return { content, citations };
};

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export const handlers: Record<string, MockHandler> = {
  notetaker_upcoming: () => EVENTS,
  notetaker_status: () => {
    const live = library.find((r) => r.meeting.status === "recording");
    return {
      recording: !!live,
      starting: false,
      meeting_id: live?.meeting.id ?? null,
      title: live?.meeting.title ?? null,
      started_ms: live?.meeting.start_ms ?? null,
      elapsed_ms: live ? Date.now() - live.meeting.start_ms : 0,
      mic: { state: live ? "capturing" : "off", detail: null },
      system: { state: live ? "capturing" : "off", detail: null },
      active_calls: [],
      processing: [],
      calendar_access: "full_access",
    };
  },
  notetaker_list: ({ filter }) => {
    const f = (filter ?? {}) as Partial<NotetakerFilter>;
    return library
      .filter((r) => !f.folder || r.meeting.folder === f.folder)
      .filter((r) => !f.query || matches(r, f.query))
      .map((r) => r.meeting);
  },
  notetaker_get: ({ id }): MeetingDetail | null => {
    const r = find(id);
    return r ? { meeting: r.meeting, segments: r.segments, thoughts: r.thoughts, summary: r.summary } : null;
  },
  notetaker_folders: () => {
    const counts = new Map<string, number>();
    for (const r of library)
      if (r.meeting.folder) counts.set(r.meeting.folder, (counts.get(r.meeting.folder) ?? 0) + 1);
    return [...counts].sort().map(([name, count]) => ({ name, count }));
  },
  notetaker_save_thoughts: ({ id, markdown }) => {
    const r = find(id);
    if (!r) throw "No such meeting";
    r.thoughts = String(markdown);
    return null;
  },
  notetaker_rename: ({ id, title }) => {
    const r = find(id);
    if (!r) throw "No such meeting";
    if (!String(title).trim()) throw "The title can't be empty";
    r.meeting = { ...r.meeting, title: String(title).trim(), title_auto: false };
    return null;
  },
  notetaker_move_to_folder: ({ id, folder }) => {
    const r = find(id);
    if (!r) throw "No such meeting";
    r.meeting = { ...r.meeting, folder: (folder as string | null) || null };
    return null;
  },
  notetaker_delete: ({ id }) => {
    library = library.filter((r) => r.meeting.id !== id);
    return null;
  },
  notetaker_regenerate: ({ id }) => {
    const r = find(id);
    if (!r) throw "No such meeting";
    throw "Ollama isn't running. Start it, then choose Try again.";
  },
  notetaker_start: () => {
    throw "Recording is not available in the preview";
  },
  notetaker_stop: () => {
    throw "Nothing is being recorded";
  },
  notetaker_ask: async ({ question, meetingId, threadId }) => {
    await wait(700);
    const thread = (threadId as string | null) ?? `t${Date.now()}`;
    const scope = (meetingId as string | null) ?? null;
    const now = Date.now();
    const q: ChatMessage = {
      id: nextMessage++,
      thread_id: thread,
      meeting_id: scope,
      role: "user",
      content: String(question),
      citations: [],
      created_ms: now,
    };
    const a: ChatMessage = {
      id: nextMessage++,
      thread_id: thread,
      meeting_id: scope,
      role: "assistant",
      ...answerFor(scope),
      created_ms: now + 1,
    };
    chats.push(q, a);
    return { thread_id: thread, question: q, answer: a };
  },
  notetaker_chats: ({ meetingId }) => {
    const threads = new Map<string, ChatMessage[]>();
    for (const m of chats)
      if (m.meeting_id === (meetingId ?? null))
        threads.set(m.thread_id, [...(threads.get(m.thread_id) ?? []), m]);
    return [...threads.values()]
      .map((msgs) => ({
        thread_id: msgs[0].thread_id,
        meeting_id: msgs[0].meeting_id,
        started_ms: msgs[0].created_ms,
        updated_ms: msgs[msgs.length - 1].created_ms,
        first_question: msgs[0].content,
      }))
      .reverse();
  },
  notetaker_chat: ({ threadId }) => chats.filter((m) => m.thread_id === threadId),
  notepad_open: () => null,
};
