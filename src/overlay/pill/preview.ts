import { installMockTauri } from "@/flow/dev/mockTauri";

// Dev-only: open the overlay page in a plain browser with ?pillPreview=1 and drive
// the pill from a page script:
//   window.__pillEmit("show-overlay", "recording")
//   window.__pillEmit("mic-level", Array(16).fill(0.2))
//   window.__pillEmit("pill-result", { text: "…", inserted: false })
// Commands the pill sends land in window.__pillCalls; the last hit area in
// window.__pillHitRect.

type Listener = { event: string; handler: number };
type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

const params = new URLSearchParams(window.location.search);

export const isPillPreview =
  !("__TAURI_INTERNALS__" in window) && params.get("pillPreview") === "1";

if (isPillPreview) {
  installMockTauri({ theme: "dark" });
  const w = window as unknown as Record<string, unknown>;
  const internals = w.__TAURI_INTERNALS__ as { invoke: Invoke };
  const base = internals.invoke;
  const listeners = new Map<number, Listener>();
  const calls: { cmd: string; args: Record<string, unknown>; at: number }[] =
    [];
  let next = 1;

  w.__pillCalls = calls;
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
    calls.push({ cmd, args, at: performance.now() });
    if (cmd === "set_pill_hit_rect") w.__pillHitRect = args;
    return base(cmd, args);
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (_event: string, id: number) => listeners.delete(id),
  };
  w.__pillEmit = (event: string, payload?: unknown) => {
    for (const [id, l] of listeners) {
      if (l.event !== event) continue;
      const cb = w[`_${l.handler}`] as ((e: unknown) => void) | undefined;
      cb?.({ event, id, payload });
    }
  };
}

// ?pillScene=<name> plays a fixed sequence for headless screenshots; &bg=181818 paints
// a backdrop like the dark desktop in the recordings.
type Step = [wait: number, run: () => void];

const SAMPLE_EVENT = () => ({
  id: "e1",
  title: "Weekly design sync",
  start_ms: Date.now() + 150_000,
  end_ms: Date.now() + 30 * 60_000,
  all_day: false,
  attendees: [],
  organizer: null,
  meeting_url: "https://meet.example.com/abc-defg-hij",
  location: null,
  calendar: null,
});

if (isPillPreview) {
  const w = window as unknown as Record<string, unknown>;
  const emit = (event: string, payload?: unknown) =>
    (w.__pillEmit as (e: string, p?: unknown) => void)(event, payload);
  const centre = (sel: string) => {
    const r = document.querySelector(sel)?.getBoundingClientRect();
    return r ? [r.x + r.width / 2, r.y + r.height / 2] : [0, 0];
  };
  const point = (sel: string) => emit("pill-cursor", centre(sel));
  const fire = (sel: string, type: string) => {
    const [x, y] = centre(sel);
    document.querySelector(sel)?.dispatchEvent(
      new MouseEvent(type, {
        bubbles: true,
        clientX: x,
        clientY: y,
        button: type === "contextmenu" ? 2 : 0,
      }),
    );
  };
  const hover: Step[] = [
    [300, () => emit("pill-hover", true)],
    [50, () => point(".pill-body")],
  ];
  const menu: Step[] = [
    ...hover,
    [300, () => fire(".pill-body", "contextmenu")],
    [100, () => point('[data-row="hide"]')],
  ];
  const recTab: Step[] = [...hover, [300, () => point(".pill-rec")]];
  const tabHot: Step[] = [...recTab, [200, () => point(".pill-tab")]];
  const note: Step[] = [
    [300, () => emit("notetaker-state", { recording: true, meeting_id: "m1" })],
  ];
  const stopped: Step = [
    300,
    () => emit("notetaker-state", { recording: false, meeting_id: null }),
  ];
  const soon: Step[] = [
    [
      300,
      () =>
        emit("notetaker-meeting-soon", {
          event: SAMPLE_EVENT(),
          starts_in_seconds: 150,
        }),
    ],
  ];
  const hidden: Step[] = [
    [300, () => emit("pill-hidden", Date.now() + 3_600_000)],
  ];
  let voice = 0;
  const speak = () => {
    clearInterval(voice);
    voice = window.setInterval(() => {
      const t = performance.now() / 1000;
      const loud = 0.55 + 0.45 * Math.sin(t * 5.3) * Math.sin(t * 1.7);
      const buckets = Array.from({ length: 16 }, (_, i) =>
        Math.max(0, loud * (1 - i / 18) * (0.6 + 0.4 * Math.sin(t * 9 + i))),
      );
      emit("mic-level", [...buckets, -48 + 34 * loud]);
    }, 33);
  };
  const rec: Step[] = [
    [300, () => emit("show-overlay", "recording")],
    [50, () => emit("recording-ready")],
    [10, speak],
  ];
  const proc: Step[] = [
    ...rec,
    [2500, () => clearInterval(voice)],
    [10, () => emit("show-overlay", "transcribing")],
  ];
  const scenes: Record<string, Step[]> = {
    rec,
    recOver: [...rec, [2000, () => emit("pill-hover", true)]],
    proc,
    toast: [...rec, [800, () => emit("pill-cancelled", { undo_ms: 5000 })]],
    hover,
    hidden,
    hiddenRec: [...hidden, [300, () => emit("show-overlay", "recording")]],
    menu,
    menuMic: [...menu, [100, () => point('[data-row="mic"]')]],
    mic: [
      [300, () => emit("show-overlay", "recording")],
      [10, () => emit("pill-mic", { name: "Built-in mic", recommended: true })],
    ],
    recTab,
    tabHot,
    meetings: [...tabHot, [100, () => fire(".pill-tab", "click")]],
    note,
    notePad: [...note, ...hover],
    noteStop: [...note, ...hover, [100, () => point(".note-stop")]],
    noteStopping: [...note, [100, () => fire(".note-stop", "click")]],
    noteLoud: [...note, [100, () => emit("notetaker-level", -20)]],
    noteMore: [...note, ...hover, [100, () => point(".note-more")]],
    noteMeetings: [
      ...note,
      ...hover,
      [100, () => point(".note-more")],
      [100, () => fire(".note-more", "click")],
    ],
    noteDim: [...note, [100, () => emit("tauri://focus")]],
    noteEnd: [...note, stopped],
    mistake: [...note, stopped],
    reloadMistake: [[300, () => {}], stopped],
    call: [
      [
        300,
        () =>
          emit("notetaker-call-detected", {
            app: "Zoom",
            bundle_id: "us.zoom.xos",
          }),
      ],
    ],
    soon,
    soonMenu: [...soon, [400, () => fire(".card-more", "click")]],
    meetingsHover: [
      ...tabHot,
      [100, () => fire(".pill-tab", "click")],
      [1400, () => point(".meetings-connect")],
    ],
    popup: [
      [
        300,
        () => emit("pill-result", { text: "Hello there.", inserted: false }),
      ],
    ],
    permMic: [],
    permAx: [],
    permBoth: [],
    permMicDone: [[3500, () => fire(".perm-action", "click")]],
  };
  const scene = params.get("pillScene") ?? "";
  const side = params.get("side");
  const words = ["mistake", "reloadMistake"].includes(
    params.get("pillScene") ?? "",
  )
    ? 3
    : 200;
  const mocks: Record<string, (real: unknown) => unknown> = {
    get_available_microphones: () => [
      { index: "default", name: "Default", is_default: true },
      { index: "0", name: "MacBook Pro Microphone", is_default: false },
      { index: "1", name: "USB Audio Device", is_default: false },
    ],
    get_selected_microphone: () => "MacBook Pro Microphone",
    get_app_settings: (real) =>
      side ? { ...(real as object), pill_side: side } : real,
    notetaker_status: () =>
      params.get("pillScene") === "reloadMistake"
        ? { recording: true, meeting_id: "m1" }
        : { recording: false, meeting_id: null },
    notetaker_get_settings: () => ({ auto_start: false }),
    "plugin:macos-permissions|check_microphone_permission": () =>
      !["permMic", "permBoth", "permMicDone"].includes(scene),
    "plugin:macos-permissions|check_accessibility_permission": () =>
      !["permAx", "permBoth"].includes(scene),
    notetaker_stop: () => "m1",
    notetaker_calendar_access: () =>
      params.get("cal") ? "full_access" : "not_determined",
    notetaker_upcoming: () =>
      Array.from({ length: Number(params.get("cal") ?? 0) - 1 }, (_, i) => ({
        ...SAMPLE_EVENT(),
        id: `e${i}`,
        title: ["Weekly design sync", "Project check-in", "Team lunch"][i % 3],
        start_ms: Date.now() + (i + 1) * 45 * 60_000,
      })),
    notetaker_get: () => ({
      meeting: { id: "m1", start_ms: Date.now() - 40_000, end_ms: Date.now() },
      segments: [
        {
          source: "mic",
          speaker: "Me",
          start_ms: 0,
          end_ms: 4000,
          text: Array(words).fill("word").join(" "),
        },
      ],
      thoughts: "",
      summary: null,
    }),
  };
  const internals = w.__TAURI_INTERNALS__ as {
    invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  };
  const inner = internals.invoke;
  internals.invoke = (cmd, args) =>
    cmd in mocks
      ? inner(cmd, args)
          .catch(() => null)
          .then(mocks[cmd])
      : inner(cmd, args);

  // Headless Chrome with a virtual time budget draws few frames, so drive
  // requestAnimationFrame from timers there.
  if (params.get("pillScene")) {
    window.requestAnimationFrame = (cb) =>
      window.setTimeout(() => cb(performance.now()), 16);
    window.cancelAnimationFrame = (id) => window.clearTimeout(id);
  }
  // &still=1 ends CSS animations at once: headless virtual time can stall them.
  if (params.get("still")) {
    const style = document.createElement("style");
    style.textContent =
      ".note-card, .card-menu, .pill-meetings, .pill-tab { animation: none !important; }";
    document.head.append(style);
  }
  const bg = params.get("bg");
  if (bg) document.documentElement.style.background = `#${bg}`;
  const steps = scenes[params.get("pillScene") ?? ""] ?? [];
  if (side) steps.unshift([0, () => emit("pill-side", side)]);
  // Headless capture can fire right after the last timer; leave time for transitions.
  steps.push([1500, () => {}]);
  let at = 500;
  for (const [wait, run] of steps) {
    at += wait;
    window.setTimeout(run, at);
  }
}
