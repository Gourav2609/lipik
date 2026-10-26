import { listen } from "@tauri-apps/api/event";
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import "@fontsource-variable/figtree";
import "./Pill.css";
import { commands, events, type PillSide } from "@/bindings";
import { LipikMark } from "@/flow/ui/LipikMark";
import type { ModelStateEvent } from "@/lib/types/events";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import {
  CheckGlyph,
  ChevronGlyph,
  MicGlyph,
  RecordIcon,
  Spinner,
  XGlyph,
} from "./pill/parts";
import {
  DONE,
  NOTE_IN,
  NOTE_OUT,
  NOTE_RING,
  PILL,
  PROC,
  REC,
  SNAP,
  TIP_SLIDE,
  t,
  type Curve,
} from "./pill/curves";
import { Toast, TOAST } from "./pill/Toast";
import { Popup, POPUP } from "./pill/Popup";
import {
  Menu,
  MENU,
  MENU_H,
  MIC_ROW_TOP,
  subHeight,
  type MenuAction,
} from "./pill/Menu";
import { Meetings, MEETINGS, MEETINGS_OUT } from "./pill/Meetings";
import { NoteCard, CARD, type Card } from "./pill/NoteCard";
import { Mistake, MISTAKE } from "./pill/Mistake";
import { Permission, PERM, type PermissionKind } from "./pill/Permission";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import {
  deleteMeeting,
  hidePillFor,
  notetakerStatus,
  openMainPage,
  openNotepad,
  pasteLastTranscript,
  pillHiddenUntil,
  startNotetaker,
  startedByMistake,
  stopNotetaker,
} from "./pill/backend";

type Phase = "idle" | "recording" | "streaming" | "transcribing" | "processing";
type Side = PillSide;
type Mode =
  | "idle"
  | "hover"
  | "rec"
  | "recClick"
  | "proc"
  | "note"
  | "drag"
  | "gone";
type Shape = Exclude<Mode, "gone">;
type Rect = { x: number; y: number; w: number; h: number };
type Point = { x: number; y: number };
/** Places a `w` x `h` surface that grows out of the pill, `edge` off the docking edge. */
export type Place = (
  w: number,
  h: number,
  edge: number,
  up?: number,
) => React.CSSProperties;

// Geometry in points, measured from screen recordings (reference notes,
// ANIMATIONS.md §2). `w` is across the docking edge, `h` along it, `edge` the gap
// to the docking screen edge.
const GEO: Record<Shape, { w: number; h: number; edge: number }> = {
  idle: { w: 8, h: 40, edge: 6 },
  hover: { w: 30, h: 50, edge: 6 },
  rec: { w: 28, h: 70, edge: 6 },
  recClick: { w: 28, h: 100, edge: 7 },
  proc: { w: 28, h: 95, edge: 6 },
  note: { w: 30, h: 69, edge: 6 },
  drag: { w: 8, h: 40, edge: 6 },
};
/** The bottom slot sits higher off its edge (S3 §5.1: idle pill 14 pt up). */
const BOTTOM_LIFT = 8;
const ACTIVE: Mode[] = ["rec", "recClick", "proc"];
const MIC_W = 30;
/** The card follows the Hub window by 160 ms (permissions1 25.93 -> 26.09). */
const PERM_STARTUP_DELAY = 160;
const PERM_GAP = 400;
const PREVIEW =
  import.meta.env.DEV &&
  new URLSearchParams(window.location.search).get("pillPreview") === "1";
const MIC_H = 50;
const REC_GAP = 2;
const LABEL_GAP = 3.5;
const TIP_ABOVE = 6;
const TIP_DROP = MIC_H / 2 + REC_GAP + MIC_W / 2;
const BARS = 10;
// The bar shape is a fixed 1 s loop that starts with the recording and is
// scaled by the one live level (ANIMATIONS.md "Waveform dynamics"): each row is
// the widths top to bottom over the centre pair, every 1/12 s.
const WAVE_KEYS = [
  [0.5, 0.71, 0.94, 1.08, 1.07, 0.93, 0.83, 0.68, 0.69, 0.51],
  [0.46, 0.59, 0.77, 0.95, 1.03, 0.97, 0.8, 0.68, 0.53, 0.5],
  [0.5, 0.56, 0.69, 0.85, 0.98, 1.02, 0.88, 0.72, 0.56, 0.4],
  [0.43, 0.68, 0.69, 0.8, 0.95, 1.05, 1.01, 0.81, 0.64, 0.44],
  [0.48, 0.65, 0.84, 0.83, 0.94, 1.06, 1.1, 1.0, 0.74, 0.53],
  [0.57, 0.66, 0.86, 0.98, 0.95, 1.05, 1.13, 1.11, 0.92, 0.62],
  [0.62, 0.75, 0.78, 1.02, 1.01, 0.99, 1.05, 1.08, 0.97, 0.72],
  [0.67, 0.75, 0.82, 0.82, 1.06, 0.94, 0.92, 0.94, 0.9, 0.74],
  [0.76, 0.88, 0.92, 0.91, 0.89, 1.11, 0.93, 0.89, 0.85, 0.73],
  [0.8, 1.03, 1.07, 1.08, 1.01, 0.99, 1.13, 0.91, 0.82, 0.73],
  [0.69, 0.98, 1.15, 1.1, 1.06, 0.94, 0.94, 0.96, 0.76, 0.62],
  [0.59, 0.86, 1.07, 1.14, 1.02, 0.98, 0.81, 0.84, 0.74, 0.54],
];
const WAVE_LEAD = 130;
/** Ratio of bar `i` at `ms` into a 1 s loop of keys (periodic Catmull-Rom). */
const loopRatio = (keys: number[][], ms: number, i: number) => {
  const n = keys.length;
  const u = ((((ms / 1000) % 1) + 1) % 1) * n;
  const k = Math.floor(u);
  const f = u - k;
  const p0 = keys[(k + n - 1) % n][i];
  const p1 = keys[k % n][i];
  const p2 = keys[(k + 1) % n][i];
  const p3 = keys[(k + 2) % n][i];
  const a = 2 * p0 - 5 * p1 + 4 * p2 - p3;
  const b = 3 * (p1 - p2) + p3 - p0;
  return p1 + 0.5 * f * (p2 - p0 + f * (a + f * b));
};
const LEVEL_TAU = 175;
// Bar width follows the mic's RMS in dBFS (the value after the 16 buckets):
// silence at the floor is a 2 pt dot, the ceiling is full width. The floor
// sits above the room noise and the ceiling follows the recent peak, so quiet
// voices still reach ~10-12 pt.
const DB_FLOOR = -55;
const DB_CEIL = -15;
const DB_RANGE_MIN = 15;
const PEAK_HEADROOM = -3;
const PEAK_DECAY = 3;
const NOISE_RISE = 1;
const NOISE_GAP = 6;
const LEVEL_CURVE = 0.8;
const freshLoudness = () => ({ peak: -30, noise: Infinity, at: 0 });
const loudnessLevel = (s: ReturnType<typeof freshLoudness>, db: number) => {
  const now = performance.now();
  const dt = s.at ? (now - s.at) / 1000 : 0;
  s.at = now;
  s.peak = Math.max(s.peak - PEAK_DECAY * dt, db);
  s.noise = Math.min(s.noise + NOISE_RISE * dt, db);
  const floor = Math.max(DB_FLOOR, s.noise + NOISE_GAP);
  const ceil = Math.max(
    floor + DB_RANGE_MIN,
    Math.min(DB_CEIL, s.peak + PEAK_HEADROOM),
  );
  const k = Math.min(1, Math.max(0, (db - floor) / (ceil - floor)));
  return k ** LEVEL_CURVE;
};
const ARM_MS = 150;
const CLICK_WAIT = 1500;
const POPUP_DELAY = 1050;
const DRAG_SLOP = 3;
const HOT_DELAY = 250;
const LEAVE_MS = 50;
// Drag (S3 §5.1-5.2), in display points: where the circle lands on each slot
// and the drop targets around them.
const SLOT_SIDE_X = 18;
const SLOT_BOTTOM_Y = 23;
const TARGET_SIDE = { w: 86, h: 124, edge: 10 };
const TARGET_BOTTOM = { w: 134, h: 93, edge: 6.5 };
const SIDES: Side[] = ["right", "left", "bottom"];
/** The circle is replaced by the idle pill this long after release (S3 298.750). */
const DROP_MS = 330;
const RESTORE_MS = 300;
// Chevron tab and Upcoming meetings (S1 61.6, 108.3, 109.05): the tab reaches 60 pt
// from the edge, centred on the record button; tooltips move out to sit 4 pt beside it.
const TAB = { along: 25, across: 39, edge: 21 };
const TAB_OUT = TAB.edge + TAB.across;
const TAB_AWAY = TAB_OUT + 4 - (GEO.hover.edge + MIC_W + LABEL_GAP);
const MEETINGS_CLOSE_MS = 100;
// Notetaker pill (notetaker1 80.0-98.7, NOTETAKER_UI.md §2): the centre bar follows
// the mic level (`notetaker-level`) from 3.5 to 17.5 pt and the others are a 1 s
// loop of ratios to it (ANIMATIONS.md "Waveform dynamics"); grey and short while
// stopping, the stop circle 18 pt below the centre, tooltips 4 pt beside the pill,
// the 28 pt ">" button 4 pt under it.
const NOTE_KEYS = [
  [0.21, 0.57, 1, 0.55, 0.2],
  [0.21, 0.55, 1, 0.51, 0.24],
  [0.25, 0.57, 1, 0.54, 0.23],
  [0.26, 0.67, 1, 0.64, 0.24],
  [0.22, 0.64, 1, 0.66, 0.25],
  [0.23, 0.53, 1, 0.7, 0.25],
  [0.3, 0.67, 1, 0.91, 0.34],
  [0.31, 0.69, 1, 0.84, 0.34],
  [0.32, 0.69, 1, 0.73, 0.32],
  [0.29, 0.72, 1, 0.63, 0.28],
  [0.24, 0.66, 1, 0.52, 0.23],
  [0.22, 0.59, 1, 0.46, 0.19],
];
const NOTE_QUIET = 3.5;
const NOTE_LOUD = 17.5;
const NOTE_STOPPED = [2, 4, 6, 4, 2];
const NOTE_STOP_SHIFT = 18;
const NOTE_TIP_EDGE = 40;
const NOTE_MORE_SIZE = 28;
const NOTE_MORE = GEO.note.h / 2 + 4 + NOTE_MORE_SIZE / 2;
// Mic-source label (S3 310.88): 11 pt beside the pill, ~100 ms after fn down.
const MIC_LABEL_EDGE = 46;
const MIC_LABEL_DELAY = 100;

const NOTEPAD = { kind: "Window", label: "notepad" } as const;

const CURVES = {
  "--ease-pill": PILL.ease,
  "--ease-rec": REC.ease,
  "--ease-proc": PROC.ease,
  "--ease-tip": TIP_SLIDE.ease,
} as React.CSSProperties;

const box = (r: Rect): React.CSSProperties => ({
  left: r.x,
  top: r.y,
  width: r.w,
  height: r.h,
});

const sizeT = (c: Curve, bottom: boolean, along = true) => {
  const across = bottom ? ["height", "top"] : ["width", "left"];
  const length = bottom ? ["width", "left"] : ["height", "top"];
  return [...across, ...(along ? length : [])].map((p) => t(p, c)).join(", ");
};

const transitionFor = (from: Mode, to: Mode, bottom: boolean): string => {
  if (to === "gone")
    return bottom
      ? "width 33ms linear, left 33ms linear, opacity 33ms linear"
      : "height 33ms linear, top 33ms linear, opacity 33ms linear";
  if (from === "gone") return "opacity 100ms linear";
  if (from === "drag" || to === "drag") return "none";
  if (to === "note") return sizeT(NOTE_IN, bottom);
  if (from === "note" && to === "idle")
    return `${sizeT(NOTE_OUT, bottom, false)}, background-color 1ms linear 16ms, border-color 1ms linear 16ms`;
  if (to === "idle" && ACTIVE.includes(from))
    return `${sizeT(DONE, bottom)}, background-color 1ms linear 16ms, border-color 1ms linear 16ms`;
  if (from === "idle" && to === "hover")
    return `${sizeT(PILL, bottom)}, background-color 180ms ease, border-color ${PILL.ms}ms ease`;
  if (from === "hover" && to === "idle") return sizeT(PILL, bottom, false);
  if (to === "rec")
    return `${sizeT(REC, bottom)}, background-color ${REC.ms}ms ease, border-color ${REC.ms}ms ease`;
  if (to === "proc") return sizeT(PROC, bottom);
  return `${sizeT(PILL, bottom)}, background-color 150ms ease, border-color 150ms ease`;
};

/** Slot centres and drop targets in display points (the panel covers the display while dragging). */
const slots = () => {
  const s = window.screen as Screen & { availLeft?: number; availTop?: number };
  const left = s.availLeft ?? 0;
  const top = s.availTop ?? 0;
  const right = left + s.availWidth;
  const bottom = top + s.availHeight;
  const midX = left + s.availWidth / 2;
  const midY = top + s.availHeight / 2;
  const centre: Record<Side, Point> = {
    left: { x: left + SLOT_SIDE_X, y: midY },
    right: { x: right - SLOT_SIDE_X, y: midY },
    bottom: { x: midX, y: bottom - SLOT_BOTTOM_Y },
  };
  const sideTarget = (x: number): Rect => ({
    x,
    y: midY - TARGET_SIDE.h / 2,
    w: TARGET_SIDE.w,
    h: TARGET_SIDE.h,
  });
  const target: Record<Side, Rect> = {
    left: sideTarget(left + TARGET_SIDE.edge),
    right: sideTarget(right - TARGET_SIDE.edge - TARGET_SIDE.w),
    bottom: {
      x: midX - TARGET_BOTTOM.w / 2,
      y: bottom - TARGET_BOTTOM.edge - TARGET_BOTTOM.h,
      w: TARGET_BOTTOM.w,
      h: TARGET_BOTTOM.h,
    },
  };
  return { centre, target };
};

const nearest = (p: Point): Side => {
  const { centre } = slots();
  const d = (s: Side) => Math.hypot(centre[s].x - p.x, centre[s].y - p.y);
  return SIDES.reduce((a, b) => (d(b) < d(a) ? b : a));
};

type Drag = Point & { slot: Side; released: boolean; landed: boolean };
type NoteHot = "pad" | "stop" | "more" | null;

const grow = (r: Rect, n: number): Rect => ({
  x: r.x - n,
  y: r.y - n,
  w: r.w + 2 * n,
  h: r.h + 2 * n,
});

const union = (a: Rect, b: Rect): Rect => {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return {
    x,
    y,
    w: Math.max(a.x + a.w, b.x + b.w) - x,
    h: Math.max(a.y + a.h, b.y + b.h) - y,
  };
};

const Pill: React.FC = () => {
  const { t: tr } = useTranslation();
  const [phase, setPhase] = useState<Phase>("idle");
  const [hover, setHover] = useState(false);
  const [micHover, setMicHover] = useState(false);
  const [hotReady, setHotReady] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [recHover, setRecHover] = useState(false);
  const [xHover, setXHover] = useState(false);
  const [undoHover, setUndoHover] = useState(false);
  const [side, setSide] = useState<Side>("right");
  const [drag, setDrag] = useState<Drag | null>(null);
  const [view, setView] = useState({
    w: window.innerWidth,
    h: window.innerHeight,
  });
  const [armed, setArmed] = useState(false);
  const [working, setWorking] = useState(false);
  const [modelLoading, setModelLoading] = useState(false);
  const [shortcut, setShortcut] = useState("fn");
  const [clickPending, setClickPending] = useState(false);
  const [clickRec, setClickRec] = useState(false);
  const [toast, setToast] = useState<{ key: number; undoMs: number } | null>(
    null,
  );
  const [toastCovers, setToastCovers] = useState(false);
  const [popup, setPopup] = useState<{ key: number; text: string } | null>(
    null,
  );
  const [popupH, setPopupH] = useState(POPUP.minH);
  const [menu, setMenu] = useState(false);
  const [menuHot, setMenuHot] = useState<string | null>(null);
  const [mic, setMic] = useState<{
    name: string;
    recommended: boolean;
    delay: number;
  } | null>(null);
  const [note, setNote] = useState(false);
  const [noteStopping, setNoteStopping] = useState(false);
  const [pillOff, setPillOff] = useState(false);
  const [noteHot, setNoteHot] = useState<NoteHot>(null);
  const [tabHover, setTabHover] = useState(false);
  const [meetings, setMeetings] = useState<{
    at: "rec" | "more";
    closing: boolean;
  } | null>(null);
  const [meetingsSize, setMeetingsSize] = useState({
    w: MEETINGS.w,
    h: MEETINGS.h,
  });
  const [noteDim, setNoteDim] = useState(false);
  const [card, setCard] = useState<Card | null>(null);
  const [cardHot, setCardHot] = useState(false);
  const [cardMenu, setCardMenu] = useState(false);
  const [mistake, setMistake] = useState<{ id: string; key: number } | null>(
    null,
  );
  const [perm, setPerm] = useState<{
    kind: PermissionKind;
    key: number;
  } | null>(null);
  const permQueue = useRef<PermissionKind[]>([]);
  const axAfterRec = useRef(false);

  const swallowClick = useRef(false);
  const press = useRef<{
    x: number;
    y: number;
    canDrag: boolean;
    moved: boolean;
  } | null>(null);
  const phaseRef = useRef(phase);
  const toastOpen = useRef(false);
  const recStart = useRef(0);
  const hideAt = useRef(0);
  const clickAt = useRef(0);
  const pendingText = useRef<string | null>(null);
  const timers = useRef<Record<string, number | undefined>>({});
  const level = useRef({ target: 0, value: 0 });
  const loudness = useRef(freshLoudness());
  const noteLevel = useRef({ target: 0, value: 0 });
  const noteLoudness = useRef(freshLoudness());
  const bars = useRef<(HTMLElement | null)[]>([]);
  const noteBars = useRef<(HTMLElement | null)[]>([]);
  const noteId = useRef<string | null>(null);
  const discardId = useRef<string | null>(null);

  const later = useCallback((name: string, ms: number, fn: () => void) => {
    clearTimeout(timers.current[name]);
    timers.current[name] = window.setTimeout(fn, ms);
  }, []);

  const askFor = useCallback((kinds: PermissionKind[]) => {
    for (const k of kinds)
      if (!permQueue.current.includes(k)) permQueue.current.push(k);
    setPerm((cur) => {
      if (cur) return cur;
      const kind = permQueue.current.shift();
      return kind ? { kind, key: performance.now() } : null;
    });
  }, []);

  const permDone = useCallback(() => {
    setPerm(null);
    if (permQueue.current.length)
      later("perm", PERM_GAP, () => askFor([]));
  }, [askFor, later]);

  useEffect(() => {
    if (platform() !== "macos") return;
    let cancelled = false;
    commands
      .getAppSettings()
      .then(async (r) => {
        if (cancelled || r.status !== "ok" || !r.data.onboarding_completed)
          return;
        const [mic, ax] = await Promise.all([
          checkMicrophonePermission(),
          checkAccessibilityPermission(),
        ]);
        const missing: PermissionKind[] = [];
        if (!mic) missing.push("microphone");
        if (!ax) missing.push("accessibility");
        if (missing.length && !cancelled)
          later("perm", PERM_STARTUP_DELAY, () => askFor(missing));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [askFor, later]);

  const showPopupLater = useCallback(
    (text: string) => {
      const since = performance.now() - hideAt.current;
      const wait = hideAt.current ? Math.max(0, POPUP_DELAY - since) : 0;
      later("popup", wait, () => setPopup({ key: performance.now(), text }));
    },
    [later],
  );

  useEffect(() => {
    const onResize = () =>
      setView({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener("resize", onResize);
    const loadShortcut = () =>
      commands
        .getAppSettings()
        .then((r) => {
          if (r.status !== "ok") return;
          const binding = r.data.bindings?.transcribe?.current_binding;
          if (binding) setShortcut(formatKeyCombination(binding, "macos"));
          setSide(r.data.pill_side ?? "right");
        })
        .catch(() => {});
    loadShortcut();
    const subs = [
      // The backend stops sending mouse events as soon as the cursor leaves, so
      // mouseleave may never reach the buttons.
      listen<boolean>("pill-hover", (e) => {
        setHover(e.payload);
        if (e.payload) loadShortcut();
        else {
          setMicHover(false);
          setRecHover(false);
          setXHover(false);
          setUndoHover(false);
          setTabHover(false);
          setNoteHot(null);
          setCardHot(false);
          setMenu(false);
          setMenuHot(null);
          setMeetings((m) => m && { ...m, closing: true });
          later("meetings", MEETINGS_CLOSE_MS, () => setMeetings(null));
        }
      }),
      listen<[number, number]>("pill-cursor", (e) => {
        const el = document.elementFromPoint(e.payload[0], e.payload[1]);
        setMicHover(!!el?.closest(".pill-body"));
        setRecHover(!!el?.closest(".pill-rec"));
        setXHover(!!el?.closest(".pill-x"));
        setUndoHover(!!el?.closest(".toast-undo"));
        setTabHover(!!el?.closest(".pill-tab"));
        setCardHot(!!el?.closest(".note-card"));
        setMenuHot(el?.closest("[data-row]")?.getAttribute("data-row") ?? null);
        setNoteHot(
          el?.closest(".note-stop")
            ? "stop"
            : el?.closest(".note-more")
              ? "more"
              : el?.closest(".pill-body.note")
                ? "pad"
                : null,
        );
      }),
      listen<Phase>("show-overlay", (e) => {
        const live = e.payload === "recording" || e.payload === "streaming";
        const wasLive =
          phaseRef.current === "recording" || phaseRef.current === "streaming";
        if (live && !wasLive) {
          if (platform() === "macos")
            checkAccessibilityPermission()
              .then((ok) => {
                axAfterRec.current = !ok;
              })
              .catch(() => {});
          recStart.current = performance.now();
          level.current = { target: 0, value: 0 };
          loudness.current = freshLoudness();
          setArmed(false);
          setClickRec(
            clickAt.current > 0 &&
              performance.now() - clickAt.current < CLICK_WAIT,
          );
          clearTimeout(timers.current.popup);
          pendingText.current = null;
          setPopup(null);
          if (toastOpen.current) commands.pillUndoExpired().catch(() => {});
          toastOpen.current = false;
          setToast(null);
          setToastCovers(false);
          setMenu(false);
          setMeetings(null);
        }
        if (!live) setMic(null);
        clickAt.current = 0;
        setClickPending(false);
        setWorking(false);
        phaseRef.current = e.payload;
        setPhase(e.payload);
      }),
      listen("hide-overlay", () => {
        if (phaseRef.current !== "idle") hideAt.current = performance.now();
        phaseRef.current = "idle";
        setPhase("idle");
        setMic(null);
        setWorking(false);
        setClickRec(false);
        if (axAfterRec.current) {
          axAfterRec.current = false;
          askFor(["accessibility"]);
        }
        if (pendingText.current !== null) {
          showPopupLater(pendingText.current);
          pendingText.current = null;
        }
      }),
      listen("recording-ready", () =>
        later(
          "arm",
          Math.max(0, ARM_MS - (performance.now() - recStart.current)),
          () => setArmed(true),
        ),
      ),
      listen<{ text: string; inserted: boolean }>("pill-result", (e) => {
        if (e.payload.inserted || !e.payload.text) return;
        if (phaseRef.current === "idle") showPopupLater(e.payload.text);
        else pendingText.current = e.payload.text;
      }),
      listen<{ undo_ms: number }>("pill-cancelled", (e) => {
        clickAt.current = 0;
        setClickPending(false);
        clearTimeout(timers.current.popup);
        setPopup(null);
        toastOpen.current = true;
        setToast({ key: performance.now(), undoMs: e.payload.undo_ms });
        setToastCovers(true);
      }),
      listen<{ name: string; recommended: boolean }>("pill-mic", (e) => {
        const since = performance.now() - recStart.current;
        setMic({
          name: e.payload.name,
          recommended: e.payload.recommended,
          delay:
            since > 1000
              ? MIC_LABEL_DELAY
              : Math.max(0, MIC_LABEL_DELAY - since),
        });
      }),
      listen<{ recording: boolean; meeting_id: string | null }>(
        "notetaker-state",
        (e) => {
          const id = noteId.current;
          if (e.payload?.recording) {
            noteId.current = e.payload.meeting_id;
            noteLevel.current = { target: 0, value: 0 };
            noteLoudness.current = freshLoudness();
            setNote(true);
            setNoteStopping(false);
            setCard(null);
            return;
          }
          noteId.current = null;
          const end = () => {
            setNote(false);
            setNoteStopping(false);
            setNoteDim(false);
          };
          if (!id) return end();
          setNoteStopping(true);
          startedByMistake(id).then((show) => {
            if (show) setMistake({ id, key: performance.now() });
            end();
          });
        },
      ),
      events.notetakerCallDetected.listen(async (e) => {
        if (noteId.current) return;
        const settings = await commands
          .notetakerGetSettings()
          .catch(() => null);
        if (settings?.auto_start || noteId.current) return;
        setCardMenu(false);
        setCard({
          kind: "call",
          key: performance.now(),
          app: e.payload.app,
          bundleId: e.payload.bundle_id,
        });
      }),
      events.notetakerCallEnded.listen((e) =>
        setCard((c) =>
          c?.kind === "call" && c.bundleId === e.payload.bundle_id ? null : c,
        ),
      ),
      events.notetakerMeetingSoon.listen((e) => {
        if (noteId.current) return;
        setCard((c) =>
          c?.kind === "call"
            ? c
            : { kind: "soon", key: performance.now(), event: e.payload.event },
        );
      }),
      events.notetakerNoteReady.listen((e) => {
        if (e.payload.meeting_id !== discardId.current) return;
        discardId.current = null;
        deleteMeeting(e.payload.meeting_id);
      }),
      // The notepad in front (its Space) dims the notetaker pill (notetaker1 73.0).
      listen("tauri://focus", () => setNoteDim(true), { target: NOTEPAD }),
      listen("tauri://blur", () => setNoteDim(false), { target: NOTEPAD }),
      listen<number | null>("pill-hidden", (e) =>
        setPillOff(e.payload != null),
      ),
      listen<string>("pill-side", (e) => {
        const s = String(e.payload).toLowerCase() as Side;
        setSide(SIDES.includes(s) ? s : "right");
      }),
      events.streamPhaseEvent.listen((e) =>
        setWorking(e.payload.phase === "working"),
      ),
      listen<ModelStateEvent>("model-state-changed", (e) => {
        const type = e.payload.event_type;
        if (type === "loading_started") setModelLoading(true);
        else if (type === "loading_completed" || type === "loading_failed")
          setModelLoading(false);
      }),
      listen<number[]>("mic-level", (e) => {
        const b = e.payload;
        const db = b[16];
        if (db === undefined) {
          const mean = b.reduce((a, v) => a + (v || 0), 0) / (b.length || 1);
          level.current.target = Math.min(1, mean * 3);
          return;
        }
        level.current.target = loudnessLevel(loudness.current, db);
      }),
      listen<number>("notetaker-level", (e) => {
        noteLevel.current.target = loudnessLevel(
          noteLoudness.current,
          e.payload,
        );
      }),
    ];
    const pending = timers.current;
    return () => {
      window.removeEventListener("resize", onResize);
      Object.values(pending).forEach(clearTimeout);
      subs.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [later, showPopupLater, askFor]);

  const live = (phase === "recording" || phase === "streaming") && !working;
  const busy = phase === "transcribing" || phase === "processing" || working;
  const bottom = side === "bottom";

  let mode: Mode;
  const permShown = !!perm && !popup && !mistake && !toast;
  if (popup || mistake || permShown || (toast && toastCovers)) mode = "gone";
  else if (drag) mode = "drag";
  else if (live) mode = clickRec ? "recClick" : "rec";
  else if (busy) mode = "proc";
  else if (clickPending) mode = "recClick";
  else if (note) mode = "note";
  else if (pillOff) mode = "gone";
  else if (hover) mode = "hover";
  else mode = "idle";

  const modeRef = useRef<Mode>(mode);
  const transRef = useRef("none");
  const lastGeo = useRef(GEO[mode === "gone" ? "idle" : mode]);
  if (modeRef.current !== mode) {
    transRef.current = transitionFor(modeRef.current, mode, bottom);
    modeRef.current = mode;
  }
  const geo = mode === "gone" ? { ...lastGeo.current, h: 0 } : GEO[mode];
  if (mode !== "gone") lastGeo.current = geo;

  const { w: width, h: height } = view;
  const cx = width / 2;
  const cy = height / 2;
  const lift = bottom ? BOTTOM_LIFT : 0;
  /** `along` x `across` points, `edge` off the docking edge, centred on the pill and moved `shift` along the edge. */
  const slab = (
    along: number,
    across: number,
    edge: number,
    shift = 0,
  ): Rect =>
    bottom
      ? {
          x: cx - along / 2 + shift,
          y: height - edge - across,
          w: along,
          h: across,
        }
      : {
          x: side === "right" ? width - edge - across : edge,
          y: cy - along / 2 + shift,
          w: across,
          h: along,
        };
  const surface = (w: number, h: number, edge: number, up = 0): Rect =>
    bottom
      ? { x: cx - w / 2, y: height - edge - h, w, h }
      : {
          x: side === "right" ? width - edge - w : edge,
          y: cy - up - h / 2,
          w,
          h,
        };
  const place: Place = (w, h, edge, up) => box(surface(w, h, edge, up));
  /** A label `gap` off the docking edge, centred `shift` along it from the pill's centre. */
  const label = (gap: number, shift = 0): React.CSSProperties =>
    bottom
      ? { left: cx + shift, bottom: gap + lift }
      : { [side]: gap, top: cy + shift - 17 };
  /** Moves a label `by` along the edge and `away` from it. */
  const labelMove = (by: number, away = 0) =>
    bottom
      ? `translate(calc(-50% + ${by}px), ${-away}px)`
      : `translate(${side === "right" ? -away : away}px, ${by}px)`;
  /** A `w` x `h` panel `edge` off the docking edge, centred `shift` along it. */
  const panel = (w: number, h: number, edge: number, shift: number): Rect =>
    bottom
      ? { x: cx + shift - w / 2, y: height - edge - h, w, h }
      : {
          x: side === "right" ? width - edge - w : edge,
          y: cy + shift - h / 2,
          w,
          h,
        };
  const meetingsRect = (w: number, h: number): Rect => {
    const atRec = meetings?.at !== "more";
    const edge =
      (atRec ? TAB_OUT : GEO.note.edge + GEO.note.w) + MEETINGS.gap + lift;
    return panel(
      bottom ? w : Math.min(w, width - edge - 1),
      h,
      edge,
      atRec ? TIP_DROP : NOTE_MORE,
    );
  };
  const cardRect = panel(CARD.w, CARD.h, CARD.edge + lift, 0);
  const menuRect = surface(MENU.w, MENU_H, MENU.edge + lift);
  const subRect = (rows: number): Rect => {
    const h = subHeight(rows);
    const x =
      side === "right"
        ? menuRect.x - MENU.subGap - MENU.subW
        : menuRect.x + MENU.w + MENU.subGap;
    const w = Math.min(MENU.subW, width - Math.max(0, x) - 2);
    const y = Math.min(
      height - h - 4,
      Math.max(4, menuRect.y + MIC_ROW_TOP - MENU.pad),
    );
    return { x: Math.max(2, x), y, w, h };
  };

  const prevMode = useRef<Mode>(mode);
  useLayoutEffect(() => {
    const from = prevMode.current;
    prevMode.current = mode;
    if (from === "hover" && mode === "idle") {
      setLeaving(true);
      later("leave", LEAVE_MS, () => setLeaving(false));
    } else setLeaving(false);
    if (mode === "hover") later("hot", HOT_DELAY, () => setHotReady(true));
    else {
      clearTimeout(timers.current.hot);
      setHotReady(false);
    }
  }, [mode, later]);

  // Waveform: one shared level smoothed with a ~175 ms time constant times the
  // 1 s shape loop, which restarts with each recording.
  const reacting = (mode === "rec" || mode === "recClick") && armed;
  useEffect(() => {
    const prop = bottom ? "height" : "width";
    const setSizes = (fn: (i: number) => number) =>
      bars.current.forEach((el, i) => {
        if (!el) return;
        el.style.width = el.style.height = "";
        el.style[prop] = `${fn(i)}px`;
      });
    if (!reacting) {
      setSizes(() => 2);
      return;
    }
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const l = level.current;
      l.value +=
        (l.target - l.value) * (1 - Math.exp(-(now - last) / LEVEL_TAU));
      last = now;
      const t = now - recStart.current + WAVE_LEAD;
      const centre = 2 + 12 * l.value;
      setSizes((i) =>
        Math.min(14, Math.max(2, centre * loopRatio(WAVE_KEYS, t, i))),
      );
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [reacting, mode, bottom]);

  useEffect(() => {
    if (mode !== "note") return;
    const prop = bottom ? "height" : "width";
    const setSizes = (fn: (i: number) => number) =>
      noteBars.current.forEach((el, i) => {
        if (!el) return;
        el.style.width = el.style.height = "";
        el.style[prop] = `${fn(i)}px`;
      });
    if (noteStopping) {
      setSizes((i) => NOTE_STOPPED[i]);
      return;
    }
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const l = noteLevel.current;
      l.value +=
        (l.target - l.value) * (1 - Math.exp(-(now - last) / LEVEL_TAU));
      last = now;
      const centre = NOTE_QUIET + (NOTE_LOUD - NOTE_QUIET) * l.value;
      setSizes((i) => Math.max(1, centre * loopRatio(NOTE_KEYS, now, i)));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [mode, bottom, noteStopping]);

  useEffect(() => {
    notetakerStatus().then((s) => {
      if (!s?.recording) return;
      noteId.current = s.meeting_id;
      setNote(true);
    });
    pillHiddenUntil().then((until) => until != null && setPillOff(true));
  }, []);

  const dragging = drag !== null;
  // Tell the backend which part of the panel should take the mouse.
  useEffect(() => {
    const hit = (r: Rect, pad = 0) =>
      commands.setPillHitRect(
        r.x - pad,
        r.y - pad,
        r.w + 2 * pad,
        r.h + 2 * pad,
      );
    const withCards = (r: Rect) => {
      let out = r;
      if (meetings)
        out = union(
          out,
          grow(meetingsRect(meetingsSize.w, meetingsSize.h), MEETINGS_OUT + 2),
        );
      else if (card) {
        const c = grow(cardRect, CARD.out);
        out = union(
          out,
          cardMenu ? { ...c, h: c.h + CARD.menuGap + CARD.menuH } : c,
        );
      }
      hit(out);
    };
    const noteMore = GEO.note.h / 2 + 4 + NOTE_MORE_SIZE + 2;
    // An open menu takes the whole strip, so a click beside it closes it.
    if (dragging || menu) commands.setPillHitRect(0, 0, width, height);
    else if (popup) hit(surface(POPUP.w, popupH, POPUP.edge, POPUP.lift), 2);
    else if (mistake)
      hit(surface(MISTAKE.w, MISTAKE.h, MISTAKE.edge, MISTAKE.lift), 2);
    else if (permShown) hit(surface(PERM.bigW, PERM.bigH, PERM.edge), 2);
    else if (toast) hit(surface(TOAST.w, TOAST.h, TOAST.edge), 2);
    else if (mode === "gone") commands.setPillHitRect(-1, -1, 0, 0);
    else if (ACTIVE.includes(mode))
      withCards(slab(GEO[mode as Shape].h + 4, 40 + lift, 0));
    else if (mode === "note")
      withCards(
        hover
          ? slab(
              GEO.note.h / 2 + 2 + noteMore,
              40 + lift,
              0,
              (noteMore - GEO.note.h / 2 - 2) / 2,
            )
          : slab(GEO.note.h + 4, 40 + lift, 0),
      );
    else if (hover)
      withCards(slab(MIC_H / 2 + REC_GAP + MIC_W + 30, 140 + lift, 0, 17.5));
    else
      withCards(slab(GEO.idle.h + 4, GEO.idle.edge + GEO.idle.w + 2 + lift, 0));
  }, [
    dragging,
    menu,
    meetings,
    meetingsSize,
    card,
    cardMenu,
    mistake,
    permShown,
    popup,
    popupH,
    toast,
    mode,
    hover,
    width,
    height,
    side,
  ]);

  // The panel covers the whole display while dragging; once the circle has
  // landed it stays until the panel is back to its strip.
  const covers =
    width >= window.screen.width - 1 && height >= window.screen.height - 1;
  useLayoutEffect(() => {
    if (drag?.landed && !covers) setDrag(null);
  }, [drag?.landed, covers]);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 0) return;
      press.current = {
        x: e.screenX,
        y: e.screenY,
        canDrag: mode === "idle" || mode === "hover",
        moved: false,
      };
      const onMove = (ev: MouseEvent) => {
        const p = press.current;
        if (!p?.canDrag) return;
        const at = { x: ev.screenX, y: ev.screenY };
        if (!p.moved && Math.hypot(at.x - p.x, at.y - p.y) < DRAG_SLOP) return;
        if (!p.moved) commands.setPillDragging(true).catch(() => {});
        p.moved = true;
        setDrag({ ...at, slot: nearest(at), released: false, landed: false });
      };
      const onUp = (ev: MouseEvent) => {
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        const p = press.current;
        press.current = null;
        if (!p?.moved) {
          if (mode === "note") {
            if (!noteStopping) openNotepad(noteId.current);
          } else if (phaseRef.current === "idle") {
            clickAt.current = performance.now();
            setClickPending(true);
            later("click", CLICK_WAIT, () => setClickPending(false));
            commands.pillToggleDictation().catch(() => {});
          } else if (
            phaseRef.current === "recording" ||
            phaseRef.current === "streaming"
          ) {
            commands.pillToggleDictation().catch(() => {});
          }
          return;
        }
        const slot = nearest({ x: ev.screenX, y: ev.screenY });
        setDrag({
          ...slots().centre[slot],
          slot,
          released: true,
          landed: false,
        });
        later("drop", DROP_MS, () => {
          setSide(slot);
          setDrag((d) => d && { ...d, landed: true });
          commands
            .setPillSide(slot)
            .then(() => commands.setPillDragging(false))
            .catch(() => {});
          later("restore", RESTORE_MS, () => setDrag(null));
        });
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    },
    [mode, noteStopping, later],
  );

  const showTip = mode === "hover" && hover && !menu && !meetings && !card;
  const loadingLabel = mode === "proc" && modelLoading;
  const hot = micHover && hotReady && mode === "hover";
  const drop = drag && covers ? slots() : null;
  const showTab =
    mode === "hover" && !menu && !meetings && (recHover || tabHover);
  const tipAway = showTip && (recHover || tabHover);
  const noteOn = mode === "note" && hover && !noteStopping;
  const noteTip = noteOn && !meetings ? noteHot : null;
  const notePadHot = noteOn && (noteHot === "pad" || noteHot === "stop");
  const micLive = (mode === "rec" || mode === "recClick") && mic;

  const onMenuAction = (action: MenuAction) => {
    setMenu(false);
    if (action === "hide") hidePillFor(60);
    else if (action === "settings") openMainPage("settings");
    else if (action === "history") openMainPage("dictation");
    else pasteLastTranscript();
  };
  const onMeetingsSize = useCallback(
    (w: number, h: number) =>
      setMeetingsSize((s) => (s.w === w && s.h === h ? s : { w, h })),
    [],
  );
  const closeMeetings = () => {
    setMeetings((m) => m && { ...m, closing: true });
    later("meetings", MEETINGS_CLOSE_MS, () => setMeetings(null));
  };
  const closeCard = useCallback(() => {
    setCard(null);
    setCardMenu(false);
    setCardHot(false);
  }, []);
  const toggleMeetings = (at: "rec" | "more") => {
    clearTimeout(timers.current.meetings);
    setMeetings((m) => (m && !m.closing ? null : { at, closing: false }));
  };

  return (
    <div
      className={`strip side-${side}`}
      style={CURVES}
      data-hot={recHover ? "rec" : undoHover ? "undo" : undefined}
      onContextMenu={(e) => {
        e.preventDefault();
        if (menu) return setMenu(false);
        const el = e.target as HTMLElement;
        if (
          (mode === "idle" || mode === "hover") &&
          el.closest(".pill-body, .pill-rec, .pill-tab")
        ) {
          setMeetings(null);
          setMenuHot(null);
          setMenu(true);
        }
      }}
      onMouseDownCapture={(e) => {
        swallowClick.current = false;
        if (!menu || (e.target as HTMLElement).closest(".pill-menu")) return;
        e.stopPropagation();
        e.preventDefault();
        swallowClick.current = true;
        setMenu(false);
      }}
      onClickCapture={(e) => {
        if (!swallowClick.current) return;
        swallowClick.current = false;
        e.stopPropagation();
      }}
    >
      {showTab && (
        <div
          className={`pill-tab ${tabHover ? "hot" : ""}`}
          style={box(slab(TAB.along, TAB.across, TAB.edge + lift, TIP_DROP))}
          onMouseEnter={() => setTabHover(true)}
          onMouseLeave={() => setTabHover(false)}
          onClick={() => toggleMeetings("rec")}
        >
          <ChevronGlyph />
        </div>
      )}
      <div
        className={`pill-body ${mode} ${hot ? "hot" : ""} ${leaving ? "leaving" : ""} ${notePadHot ? "note-hover" : ""} ${noteOn && noteHot === "stop" ? "stop-hot" : ""} ${noteStopping ? "stopping" : ""} ${noteDim && !hover && mode === "note" ? "dim" : ""}`}
        style={{
          ...box(slab(geo.h, geo.w, geo.edge + lift)),
          opacity:
            mode === "gone" || mode === "drag"
              ? 0
              : noteDim && !hover && mode === "note"
                ? 0.5
                : 1,
          transition:
            mode === "note"
              ? `${transRef.current}, background-color 0ms, border-color ${notePadHot || noteStopping ? `${NOTE_RING.ms}ms ${NOTE_RING.ease}` : "0ms"}`
              : transRef.current,
        }}
        onMouseDown={onMouseDown}
        onMouseEnter={() => setMicHover(true)}
        onMouseLeave={() => setMicHover(false)}
      >
        <div className="mic">
          <MicGlyph />
        </div>
        <div className="bars">
          {Array.from({ length: BARS }, (_, i) => (
            <i
              key={i}
              ref={(el) => {
                bars.current[i] = el;
              }}
              className={reacting ? "on" : ""}
            />
          ))}
        </div>
        <div className="spin">
          <Spinner />
        </div>
        <button
          className="pill-x"
          onMouseDown={(e) => e.stopPropagation()}
          onMouseEnter={() => setXHover(true)}
          onMouseLeave={() => setXHover(false)}
          onClick={() => {
            setXHover(false);
            commands.pillCancel().catch(() => {});
          }}
        >
          <XGlyph size={7} stroke={1.5} />
        </button>
        <button
          className="pill-ok"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={() => commands.pillFinish().catch(() => {})}
        >
          <CheckGlyph size={9} stroke={1.8} />
        </button>
        <div className="note-wave">
          {NOTE_KEYS[0].map((_, i) => (
            <i
              key={i}
              ref={(el) => {
                noteBars.current[i] = el;
              }}
            />
          ))}
        </div>
        <button
          className="note-stop"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={() => {
            setNoteStopping(true);
            stopNotetaker().then((ok) => ok || setNoteStopping(false));
          }}
        >
          <i />
        </button>
        <div className="note-spin">
          <Spinner grey />
        </div>
      </div>

      <div
        className={`pill-more note-more ${noteOn ? "in" : ""} ${noteOn && noteHot === "more" ? "hot" : ""}`}
        style={box(
          slab(
            NOTE_MORE_SIZE,
            NOTE_MORE_SIZE,
            GEO.note.edge + (GEO.note.w - NOTE_MORE_SIZE) / 2 + lift,
            NOTE_MORE,
          ),
        )}
        onClick={() => toggleMeetings("more")}
      >
        <ChevronGlyph />
      </div>

      <div
        className={`pill-rec ${mode === "hover" ? "in" : ""}`}
        style={box(slab(MIC_W, MIC_W, GEO.hover.edge + lift, TIP_DROP))}
        onMouseEnter={() => setRecHover(true)}
        onMouseLeave={() => setRecHover(false)}
        onClick={() => startNotetaker(null)}
      >
        <RecordIcon />
      </div>

      <div
        className={`pill-label ${showTip ? "tip" : ""} ${showTip || loadingLabel ? "in" : ""}`}
        style={{
          ...label(GEO.hover.edge + MIC_W + (bottom ? TIP_ABOVE : LABEL_GAP)),
          transform: labelMove(tipAway ? TIP_DROP : 0, tipAway ? TAB_AWAY : 0),
        }}
      >
        {loadingLabel ? (
          tr("overlay.loadingModel")
        ) : tabHover ? (
          tr("pill.upcomingMeetings", "Upcoming meetings")
        ) : recHover ? (
          tr("pill.startNotetaker", "Start meeting notes")
        ) : (
          <>
            {tr("pill.dictate", "Dictate")} <b>{shortcut}</b>
          </>
        )}
      </div>

      <div
        className={`pill-label cancel ${mode === "recClick" && xHover ? "in" : ""}`}
        style={{
          ...label(
            GEO.recClick.edge +
              GEO.recClick.w +
              (bottom ? TIP_ABOVE : LABEL_GAP),
            -GEO.recClick.h / 2 + 14,
          ),
          transform: labelMove(0),
        }}
      >
        {tr("pill.cancel", "Cancel")}
      </div>

      {(
        [
          ["pad", 0, tr("pill.showNotepad", "Show Notepad")],
          ["stop", NOTE_STOP_SHIFT, tr("pill.stopNotetaker", "Stop meeting notes")],
          ["more", NOTE_MORE, tr("pill.upcomingMeetings", "Upcoming meetings")],
        ] as const
      ).map(([key, shift, text]) => (
        <div
          key={key}
          className={`pill-label note-tip ${noteTip === key ? "in" : ""}`}
          style={{
            ...label(NOTE_TIP_EDGE + (bottom ? TIP_ABOVE : 0)),
            transform: labelMove(shift),
          }}
        >
          {text}
        </div>
      ))}

      {micLive && (
        <div
          className="pill-mic"
          style={{
            ...(bottom
              ? {
                  left: cx,
                  bottom: lift + MIC_LABEL_EDGE,
                  transform: "translateX(-50%)",
                }
              : { [side]: MIC_LABEL_EDGE, top: cy - 22 }),
            animationDelay: `${mic.delay}ms`,
          }}
        >
          {tr("pill.usingMic", {
            name: mic.name,
            defaultValue: "Using {{name}}",
          })}
          {mic.recommended && ` ${tr("pill.recommended", "(recommended)")}`}
        </div>
      )}

      {menu && (
        <Menu
          style={box(menuRect)}
          subStyle={(rows) => box(subRect(rows))}
          hot={menuHot}
          onHot={setMenuHot}
          onAction={onMenuAction}
          onMic={(name) => {
            setMenu(false);
            commands.setSelectedMicrophone(name).catch(() => {});
          }}
        />
      )}

      {meetings && (
        <Meetings
          place={(w, h) => box(meetingsRect(w, h))}
          closing={meetings.closing}
          onSize={onMeetingsSize}
          onClose={closeMeetings}
        />
      )}

      {card && !meetings && mode !== "gone" && mode !== "drag" && (
        <NoteCard
          key={card.key}
          card={card}
          style={box(cardRect)}
          hot={cardHot}
          menu={cardMenu}
          onMenu={setCardMenu}
          onClose={closeCard}
        />
      )}

      {mistake && (
        <Mistake
          key={mistake.key}
          place={place}
          onChoice={(choice) => {
            if (choice !== "discard") return;
            deleteMeeting(mistake.id).then((ok) => {
              if (!ok) discardId.current = mistake.id;
            });
          }}
          onDone={() =>
            setMistake((cur) => (cur?.key === mistake.key ? null : cur))
          }
        />
      )}

      {permShown && perm && (
        <Permission
          key={perm.key}
          kind={perm.kind}
          place={place}
          hover={hover}
          preview={PREVIEW}
          onDone={permDone}
        />
      )}

      {drop && drag && (
        <>
          <div className={`pill-dim ${drag.released ? "out" : ""}`} />
          {SIDES.map((s) => (
            <div
              key={s}
              className={`pill-target ${s} ${s === drag.slot ? "active" : ""} ${drag.released ? "out" : ""}`}
              style={box(drop.target[s])}
            />
          ))}
          <div
            className="pill-drag"
            style={{
              left: drag.x,
              top: drag.y,
              transition: drag.released
                ? `${t("left", SNAP)}, ${t("top", SNAP)}`
                : "none",
            }}
          >
            <LipikMark size={14} />
          </div>
        </>
      )}
      {toast && (
        <Toast
          key={toast.key}
          side={side}
          place={place}
          undoMs={toast.undoMs}
          hover={hover}
          onCoverEnd={() => setToastCovers(false)}
          onDone={() => {
            toastOpen.current = false;
            setToast((cur) => (cur?.key === toast.key ? null : cur));
          }}
        />
      )}
      {popup && (
        <Popup
          key={popup.key}
          place={place}
          text={popup.text}
          hover={hover}
          onHeight={setPopupH}
          onDone={() =>
            setPopup((cur) => (cur?.key === popup.key ? null : cur))
          }
        />
      )}
    </div>
  );
};

export default Pill;
