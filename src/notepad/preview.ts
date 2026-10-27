import type {
  CalendarAccess,
  ChatMessage,
  ChatThread,
  Meeting,
  MeetingDetail,
  NoteSummary,
  Segment,
} from "@/bindings";
import { APP_NAME } from "@/flow/brand";
import { installMockTauri } from "@/flow/dev/mockTauri";

// Dev-only: open src/notepad/index.html?notepadPreview=1 in a plain browser.
//   &notetaker=listening|recording|processing|ready|unavailable|thoughts  state of the note
//   &scene=<name>                                                       plays a fixed sequence
//   &calendar=<CalendarAccess>                                          calendar access state
//   &failSaves=<n>                                                      the first n thoughts saves fail
// window.__notepadEmit(event, payload) fires a backend event by hand. The notepad's
// own sample answers the notetaker commands here, over the shared preview mocks.

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
type Listener = { event: string; handler: number };

const params = new URLSearchParams(window.location.search);

export const isNotepadPreview =
  !("__TAURI_INTERNALS__" in window) && params.get("notepadPreview") === "1";

const mode = params.get("notetaker") ?? "ready";
const now = Date.now();

const seg = (
  speaker: string,
  start: number,
  end: number,
  text: string,
  key = speaker === "Me" ? "me" : "them",
): Segment => ({
  source: speaker === "Me" ? "mic" : "system",
  speaker,
  speaker_key: key,
  start_ms: start * 1000,
  end_ms: end * 1000,
  text,
});

/** Text and speaker key: "me", or the call voice ("s1", "s2"). */
const LINES: [string, string][] = [
  [
    "Okay, let's get started. The main thing today is the spring launch and whether we can still hold the April date.",
    "me",
  ],
  [
    "We can, if the design review moves up a week. Maya said the new screens are almost done.",
    "s1",
  ],
  [
    "Good. Then I'd like the beta list frozen by Friday so support has time to prepare.",
    "me",
  ],
  [
    "That works. One risk is the pricing page. Legal hasn't signed off on it yet.",
    "s2",
  ],
  [
    "Let's flag that. I'll follow up with legal today and send a short note to the team.",
    "me",
  ],
  [
    "Sounds good. I'll book the review for Tuesday and share the agenda tomorrow.",
    "s1",
  ],
];

const LIVE = LINES.slice(0, 3).map(([t, k], i) =>
  seg(k === "me" ? "Me" : "Them", 2 + i * 8, 9 + i * 8, t),
);
/** After processing: two voices on the call, the first one named. */
const FINAL = LINES.map(([t, k], i) =>
  seg(
    k === "me" ? "Me" : k === "s1" ? "Jordan" : "Speaker 2",
    2 + i * 8,
    9 + i * 8,
    t,
    k,
  ),
);

const SUMMARY: NoteSummary = {
  title: "Spring launch planning",
  overview:
    "The team will keep the April launch date if the design review moves up a week. Maya's new screens are nearly done, and Jordan will run the review on Tuesday. The pricing page still needs sign-off from legal.",
  sections: [
    {
      heading: "Launch timeline",
      bullets: [
        "April date holds as long as the design review moves up a week.",
        "Beta list is frozen by Friday so support has time to prepare.",
      ],
    },
    {
      heading: "Risks",
      bullets: ["Pricing page is still waiting on legal sign-off."],
    },
  ],
  key_points: [],
  decisions: ["Keep the April launch date."],
  action_items: [
    {
      text: "Follow up with legal on the pricing page",
      owner: null,
      due: "Today",
    },
    {
      text: "Book the design review and share the agenda",
      owner: "Jordan",
      due: "Tomorrow",
    },
  ],
};

const meeting = (patch: Partial<Meeting>): Meeting => ({
  id: "np-sample",
  title: "Spring launch planning",
  title_auto: true,
  start_ms: now - 53_000,
  end_ms: now,
  app: null,
  bundle_id: null,
  calendar_event_id: null,
  attendees: [],
  folder: null,
  status: "ready",
  summary_status: "ready",
  summary_error: null,
  mic_offset_ms: 0,
  system_offset_ms: 0,
  ...patch,
});

const live = (start: number): Partial<Meeting> => ({
  title: "New note",
  start_ms: start,
  end_ms: null,
  status: "recording",
  summary_status: "pending",
});

const initial = (): MeetingDetail => {
  const d = (
    meetingPatch: Partial<Meeting>,
    rest: Partial<MeetingDetail> = {},
  ) => ({
    meeting: meeting(meetingPatch),
    segments: FINAL,
    thoughts: "",
    summary: SUMMARY,
    ...rest,
  });
  switch (mode) {
    case "listening":
      return d(live(now - 1000), { segments: [], summary: null });
    case "recording":
      return d(live(now - 44_000), { segments: LIVE, summary: null });
    case "processing":
      return d(
        { title: "New note", status: "processing", summary_status: "pending" },
        { segments: LIVE, summary: null },
      );
    case "unavailable":
      return d(
        {
          summary_status: "unavailable",
          summary_error: "Ollama isn't running. Start Ollama, then try again.",
        },
        { summary: null },
      );
    case "thoughts":
      return d(
        {},
        {
          thoughts:
            "Ask about the support plan for the beta\n- who answers tickets in the first week\n- where known issues are listed",
        },
      );
    default:
      return d({});
  }
};

let note = initial();
let calendar: CalendarAccess =
  (params.get("calendar") as CalendarAccess | null) ?? "not_determined";
const chats: ChatMessage[] = [];
let failSaves = Number(params.get("failSaves") ?? 0);

const emit = (event: string, payload: unknown) =>
  (
    window as unknown as { __notepadEmit?: (e: string, p: unknown) => void }
  ).__notepadEmit?.(event, payload);

const finish = () => {
  const placeholder =
    note.meeting.title_auto && note.meeting.title === "New note";
  note = {
    ...note,
    segments: FINAL,
    summary: SUMMARY,
    meeting: {
      ...note.meeting,
      title: placeholder ? SUMMARY.title! : note.meeting.title,
      end_ms: note.meeting.end_ms ?? Date.now(),
      status: "ready",
      summary_status: "ready",
      summary_error: null,
    },
  };
  emit("notetaker-note-ready", {
    meeting_id: note.meeting.id,
    summary_status: "ready",
    summary_error: null,
  });
};

const process = (fast = false) => {
  const id = note.meeting.id;
  note = {
    ...note,
    meeting: {
      ...note.meeting,
      status: "processing",
      summary_status: "pending",
    },
  };
  const k = fast ? 0.3 : 1;
  const at = (ms: number, run: () => void) => window.setTimeout(run, ms * k);
  at(1500, () =>
    emit("notetaker-processing", {
      meeting_id: id,
      stage: "speakers",
      progress: 0,
    }),
  );
  at(3000, () =>
    emit("notetaker-processing", {
      meeting_id: id,
      stage: "transcribing",
      progress: 0.4,
    }),
  );
  at(5000, () =>
    emit("notetaker-transcript", {
      meeting_id: id,
      segments: FINAL,
      replace: true,
    }),
  );
  at(7500, () =>
    emit("notetaker-processing", {
      meeting_id: id,
      stage: "summarizing",
      progress: 0,
    }),
  );
  at(10500, finish);
};

const answer = (question: string) => {
  const miss = /miss/i.test(question);
  const content = miss
    ? "1. **Launch date**: April holds if the design review moves up a week [1]\n2. **Beta list**: frozen by Friday so support can prepare [2]\n3. **Open risk**: legal hasn't signed off on the pricing page [3]"
    : "The team is keeping the **April launch date**, as long as the design review moves up a week [1]. Two things are still open:\n\n- The beta list has to be frozen by Friday [2]\n- Legal still needs to sign off on the pricing page [3]";
  const cites: [number, number][] = [
    [1, 1],
    [2, 2],
    [3, 3],
  ];
  return {
    content,
    citations: cites.map(([marker, i]) => ({
      marker,
      meeting_id: note.meeting.id,
      meeting_title: note.meeting.title,
      start_ms: FINAL[i].start_ms,
      quote: FINAL[i].text,
    })),
  };
};

const sleep = (ms: number) => new Promise((r) => window.setTimeout(r, ms));

const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  notepad_meeting: () => note.meeting.id,
  notepad_open: () => null,
  notepad_close: () => null,
  open_main_page: () => null,
  notetaker_status: () => ({
    recording: note.meeting.status === "recording",
    starting: false,
    meeting_id: note.meeting.id,
    title: note.meeting.title,
    started_ms: note.meeting.start_ms,
    elapsed_ms: Date.now() - note.meeting.start_ms,
    mic: { state: "capturing", detail: null },
    system: { state: "capturing", detail: null },
    active_calls: [],
    processing: note.meeting.status === "processing" ? [note.meeting.id] : [],
    calendar_access: calendar,
  }),
  notetaker_get: ({ id }) => (id === note.meeting.id ? note : null),
  notetaker_save_thoughts: async ({ markdown }) => {
    await sleep(200);
    if (failSaves > 0) {
      failSaves--;
      throw "database is locked";
    }
    note = { ...note, thoughts: String(markdown) };
    return null;
  },
  notetaker_rename: ({ title }) => {
    note = {
      ...note,
      meeting: { ...note.meeting, title: String(title), title_auto: false },
    };
    return null;
  },
  notetaker_rename_speaker: ({ speakerKey, name }) => {
    note = {
      ...note,
      segments: note.segments.map((s) =>
        s.speaker_key === speakerKey ? { ...s, speaker: String(name) } : s,
      ),
    };
    return null;
  },
  notetaker_move_to_folder: ({ folder }) => {
    note = {
      ...note,
      meeting: { ...note.meeting, folder: (folder as string) || null },
    };
    return null;
  },
  notetaker_delete: () => null,
  notetaker_stop: () => {
    const id = note.meeting.id;
    note = { ...note, meeting: { ...note.meeting, end_ms: Date.now() } };
    process();
    window.setTimeout(
      () => emit("notetaker-state", { recording: false, meeting_id: null }),
      50,
    );
    return id;
  },
  notetaker_start: ({ title }) => {
    note = {
      meeting: meeting({
        ...live(Date.now()),
        id: `np-${Date.now()}`,
        title: (title as string) || "New note",
        title_auto: !title,
      }),
      segments: [],
      thoughts: "",
      summary: null,
    };
    window.setTimeout(() => {
      emit("notetaker-state", { recording: true, meeting_id: note.meeting.id });
      emit("notepad-meeting", { meeting_id: note.meeting.id });
    }, 50);
    return note.meeting;
  },
  notetaker_regenerate: () => {
    process(true);
    return null;
  },
  notetaker_calendar_access: () => calendar,
  notetaker_request_calendar_access: () => {
    if (calendar === "not_determined") calendar = "full_access";
    return calendar;
  },
  notetaker_ask: async ({ question, meetingId, threadId }) => {
    await sleep(900);
    const thread = (threadId as string) || `thread-${Date.now()}`;
    const base = {
      thread_id: thread,
      meeting_id: (meetingId as string) ?? null,
      created_ms: Date.now(),
    };
    const q: ChatMessage = {
      ...base,
      id: chats.length + 1,
      role: "user",
      content: String(question),
      citations: [],
    };
    const a: ChatMessage = {
      ...base,
      id: chats.length + 2,
      role: "assistant",
      ...answer(String(question)),
    };
    chats.push(q, a);
    return { thread_id: thread, question: q, answer: a };
  },
  notetaker_chats: () => {
    const threads = new Map<string, ChatThread>();
    for (const m of chats) {
      const t = threads.get(m.thread_id);
      if (t) t.updated_ms = m.created_ms;
      else
        threads.set(m.thread_id, {
          thread_id: m.thread_id,
          meeting_id: m.meeting_id,
          started_ms: m.created_ms,
          updated_ms: m.created_ms,
          first_question: m.content,
        });
    }
    return [...threads.values()].reverse();
  },
  notetaker_chat: ({ threadId }) =>
    chats.filter((m) => m.thread_id === threadId),
};

if (isNotepadPreview) {
  installMockTauri({
    theme: params.get("theme") === "light" ? "light" : "dark",
  });
  const w = window as unknown as Record<string, unknown>;
  const internals = w.__TAURI_INTERNALS__ as { invoke: Invoke };
  const base = internals.invoke;
  const listeners = new Map<number, Listener>();
  let next = 1;

  internals.invoke = async (cmd, args = {}) => {
    if (cmd === "plugin:event|listen") {
      const id = next++;
      listeners.set(id, {
        event: String(args.event),
        handler: Number(args.handler),
      });
      return id;
    }
    if (cmd === "plugin:event|unlisten") {
      listeners.delete(Number(args.eventId));
      return null;
    }
    if (cmd in handlers) return handlers[cmd](args);
    return base(cmd, args);
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (_event: string, id: number) => listeners.delete(id),
  };
  w.__notepadEmit = (event: string, payload?: unknown) => {
    for (const [id, l] of listeners) {
      if (l.event !== event) continue;
      const cb = w[`_${l.handler}`] as ((e: unknown) => void) | undefined;
      cb?.({ event, id, payload });
    }
  };
}

// ?scene=<name> plays a fixed sequence for headless screenshots.
type Step = [wait: number, run: () => void];

if (isNotepadPreview) {
  const $ = (sel: string) => document.querySelector<HTMLElement>(sel);
  const click = (sel: string) => $(sel)?.click();
  const tabBtn = (label: string) => () =>
    [...document.querySelectorAll<HTMLElement>(".np-tab")]
      .find((b) => b.textContent?.includes(label))
      ?.click();
  const type = (text: string) => {
    const input = $(".np-ask input") as HTMLInputElement | null;
    if (!input) return;
    const set = Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )?.set;
    set?.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
  };
  const id = () => note.meeting.id;
  const more = (i: number): Segment =>
    seg("Them", 30 + i * 8, 37 + i * 8, LINES[3 + i][0]);

  const scenes: Record<string, Step[]> = {
    firstWords: [
      [
        600,
        () =>
          emit("notetaker-transcript", {
            meeting_id: id(),
            segments: LIVE.slice(0, 1),
            replace: false,
          }),
      ],
    ],
    liveTyping: [
      [
        600,
        () =>
          emit("notetaker-transcript", {
            meeting_id: id(),
            segments: [more(0)],
            replace: false,
          }),
      ],
    ],
    streaming: (() => {
      const final = more(2);
      const words = final.text.split(" ");
      const live =
        (n: number, active = true) =>
        () =>
          emit("notetaker-live-text", {
            meeting_id: id(),
            source: final.source,
            speaker: final.speaker,
            start_ms: final.start_ms,
            text:
              words.slice(0, Math.max(0, n - 2)).join(" ") + (n > 2 ? " " : ""),
            tentative: words.slice(Math.max(0, n - 2), n).join(" "),
            active,
          });
      const steps: Step[] = [[600, live(0)]];
      for (let n = 1; n <= words.length; n++) steps.push([250, live(n)]);
      steps.push([
        400,
        () =>
          emit("notetaker-transcript", {
            meeting_id: id(),
            segments: [final],
            replace: false,
          }),
      ]);
      steps.push([0, live(words.length, false)]);
      return steps;
    })(),
    stopped: [[600, () => click(".np-rec-btn.stop")]],
    silent: [
      [
        600,
        () =>
          emit("notetaker-stream-changed", {
            meeting_id: id(),
            mic: { state: "capturing", detail: null },
            system: {
              state: "silent",
              detail: `Nothing heard from the call yet. If others are talking, allow ${APP_NAME} under System Settings > Privacy & Security > System Audio Recording Only.`,
            },
          }),
      ],
    ],
    outOfOrder: [
      [
        600,
        () =>
          emit("notetaker-transcript", {
            meeting_id: id(),
            segments: [more(1)],
            replace: false,
          }),
      ],
      [
        100,
        () =>
          emit("notetaker-transcript", {
            meeting_id: id(),
            segments: [more(0), more(1)],
            replace: false,
          }),
      ],
    ],
    calendarDenied: [
      [600, () => click('[aria-label="More"]')],
      [
        100,
        () =>
          [...document.querySelectorAll<HTMLElement>(".np-menu-item")]
            .find((b) => b.textContent?.includes("Connect calendar"))
            ?.click(),
      ],
    ],
    stepDone: [
      [
        600,
        () =>
          emit("notetaker-processing", {
            meeting_id: id(),
            stage: "transcribing",
            progress: 0.4,
          }),
      ],
    ],
    ready: [[600, finish]],
    promo: [[600, finish]],
    promoCollapsed: [
      [600, finish],
      [3200, () => click(".np-promo-head .np-tool")],
    ],
    transcript: [[600, tabBtn("Transcript")]],
    thoughts: [[600, tabBtn("My notes")]],
    ask: [[600, () => type("Where did we land on the launch date?")]],
    miss: [[600, () => click(".np-miss")]],
    cite: [
      [600, () => type("Where did we land on the launch date?")],
      [1400, () => click(".np-cite")],
    ],
    menu: [[600, () => click('[aria-label="More"]')]],
    full: [
      [
        1500,
        () =>
          emit("notetaker-transcript", {
            meeting_id: id(),
            segments: LIVE,
            replace: false,
          }),
      ],
      [9000, () => click(".np-rec-btn.stop")],
    ],
  };

  if (params.get("still") === "1") {
    const style = document.createElement("style");
    style.textContent =
      "*,*::before,*::after{animation-duration:0s!important;animation-delay:0s!important;transition-duration:0s!important}";
    document.head.appendChild(style);
  }
  if (params.get("scene")) {
    window.requestAnimationFrame = (cb) =>
      window.setTimeout(() => cb(performance.now()), 16);
    window.cancelAnimationFrame = (n) => window.clearTimeout(n);
  }
  (window as unknown as { __sceneStart: number }).__sceneStart =
    performance.now();
  let at = 0;
  for (const [wait, run] of scenes[params.get("scene") ?? ""] ?? []) {
    at += wait;
    window.setTimeout(run, at);
  }
}
