import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import {
  commands,
  events,
  type Attendee,
  type CalendarAccess,
  type CalendarEvent,
  type Meeting as BackendMeeting,
  type MeetingDetail,
  type MeetingStatus,
  type ProcessingStage,
  type Segment,
  type SummaryStatus,
} from "@/bindings";
import { CALENDAR_CHANGED } from "../../overlays/ConnectCalendar";
import { errorText, unwrap } from "../textflow";

export interface Person {
  id: string;
  name: string;
  /** Image URL; when missing the UI draws a letter avatar. */
  avatarUrl?: string;
  isYou?: boolean;
}

/** An upcoming calendar meeting. */
export interface Meeting {
  id: string;
  title: string;
  /** Unix ms. */
  start: number;
  end: number;
  calendarColor?: string;
  attendees: Person[];
  joinUrl?: string;
}

/** A recorded meeting. */
export interface Note {
  id: string;
  title: string;
  /** Unix ms. */
  createdAt: number;
  endedAt: number | null;
  attendees: Person[];
  folder: string | null;
  status: MeetingStatus;
  summaryStatus: SummaryStatus;
  summaryError: string | null;
}

export interface Folder {
  id: string;
  name: string;
  noteCount: number;
}

export type JoinMode = "join-and-start" | "join-only" | "start-only";

const toPerson = (a: Attendee, i: number): Person => {
  const name = a.name?.trim() || a.email || "Unknown contact";
  return { id: a.email ?? `${name}-${i}`, name, isYou: a.is_me };
};

const toMeeting = (e: CalendarEvent): Meeting => ({
  id: e.id,
  title: e.title || "Untitled meeting",
  start: e.start_ms,
  end: e.end_ms,
  attendees: e.attendees.map(toPerson),
  joinUrl: e.meeting_url ?? undefined,
});

export const toNote = (m: BackendMeeting): Note => ({
  id: m.id,
  title: m.title,
  createdAt: m.start_ms,
  endedAt: m.end_ms,
  attendees: m.attendees.map(toPerson),
  folder: m.folder,
  status: m.status,
  summaryStatus: m.summary_status,
  summaryError: m.summary_error,
});

/** Runs a backend call; shows the error as a toast and returns null on failure. */
export const attempt = async <T>(run: () => Promise<T>): Promise<T | null> => {
  try {
    return await run();
  } catch (err) {
    toast(errorText(err));
    return null;
  }
};

/** Runs a backend call that returns nothing; false (and a toast) on failure. */
const succeeds = async (run: () => Promise<unknown>) =>
  (await attempt(async () => {
    await run();
    return true;
  })) ?? false;

export const listNotes = async (query: string | null, folder: string | null) =>
  unwrap(
    await commands.notetakerList({
      query,
      folder,
      after_ms: null,
      before_ms: null,
      limit: null,
      offset: null,
    }),
  ).map(toNote);

export const startNotetaker = (title: string | null) =>
  attempt(async () => unwrap(await commands.notetakerStart(title)));

export const stopNotetaker = () =>
  attempt(async () => unwrap(await commands.notetakerStop()));

export const joinMeeting = async (meeting: Meeting, mode: JoinMode) => {
  const url = meeting.joinUrl;
  if (mode !== "start-only" && url) await attempt(() => openUrl(url));
  if (mode !== "join-only") await startNotetaker(meeting.title);
};

export const openInWindow = (id: string) =>
  succeeds(async () => unwrap(await commands.notepadOpen(id)));

export const renameNote = (id: string, title: string) =>
  succeeds(async () => unwrap(await commands.notetakerRename(id, title)));

export const deleteNote = (id: string) =>
  succeeds(async () => unwrap(await commands.notetakerDelete(id)));

export const moveNote = (id: string, folder: string | null) =>
  succeeds(async () =>
    unwrap(await commands.notetakerMoveToFolder(id, folder)),
  );

export const regenerateNote = (id: string) =>
  succeeds(async () => unwrap(await commands.notetakerRegenerate(id)));

export const isBusy = (note: Note) =>
  note.status === "recording" || note.status === "processing";

type Loadable<T> = { data: T; loading: boolean };

const useLoad = <T>(load: () => Promise<T>, initial: T) => {
  const [state, setState] = useState<Loadable<T>>({
    data: initial,
    loading: true,
  });
  const latest = useRef(0);
  const reload = useCallback(() => {
    const request = ++latest.current;
    const settle = (update: (s: Loadable<T>) => Loadable<T>) => {
      if (request === latest.current) setState(update);
    };
    load()
      .then((data) => settle(() => ({ data, loading: false })))
      .catch(() => settle((s) => ({ ...s, loading: false })));
  }, [load]);
  useEffect(() => reload(), [reload]);
  return [state, reload] as const;
};

const loadCalendar = async () => {
  const access = await commands.notetakerCalendarAccess();
  if (access !== "full_access") return { access, meetings: [] as Meeting[] };
  const now = Date.now();
  const meetings = unwrap(await commands.notetakerUpcoming())
    .filter((e) => !e.all_day && e.end_ms > now)
    .map(toMeeting);
  return { access, meetings };
};

const loadNotes = () => listNotes(null, null);

const loadFolders = async () =>
  unwrap(await commands.notetakerFolders()).map(
    (f): Folder => ({ id: f.name, name: f.name, noteCount: f.count }),
  );

const loadRecording = async () => {
  const status = unwrap(await commands.notetakerStatus());
  return status.recording ? status.meeting_id : null;
};

const CALENDAR_REFRESH_MS = 5 * 60 * 1000;

export const useNotetakerData = () => {
  const [calendar, reloadCalendar] = useLoad(loadCalendar, {
    access: "full_access" as CalendarAccess,
    meetings: [] as Meeting[],
  });
  const [notes, reloadNotes] = useLoad(loadNotes, [] as Note[]);
  const [folders, reloadFolders] = useLoad(loadFolders, [] as Folder[]);
  const [recording, reloadRecording] = useLoad(loadRecording, null);

  const reloadLibrary = useCallback(() => {
    reloadNotes();
    reloadFolders();
  }, [reloadNotes, reloadFolders]);

  useEffect(() => {
    const id = window.setInterval(reloadCalendar, CALENDAR_REFRESH_MS);
    window.addEventListener(CALENDAR_CHANGED, reloadCalendar);
    const stages = new Map<string, ProcessingStage>();
    const stops = [
      events.notetakerState.listen(() => {
        reloadRecording();
        reloadNotes();
      }),
      events.notetakerNoteReady.listen(({ payload }) => {
        stages.delete(payload.meeting_id);
        reloadNotes();
      }),
      events.notetakerProcessing.listen(({ payload }) => {
        if (stages.get(payload.meeting_id) === payload.stage) return;
        stages.set(payload.meeting_id, payload.stage);
        reloadNotes();
      }),
    ];
    return () => {
      window.clearInterval(id);
      window.removeEventListener(CALENDAR_CHANGED, reloadCalendar);
      stops.forEach((s) => void s.then((stop) => stop()));
    };
  }, [reloadCalendar, reloadNotes, reloadRecording]);

  return {
    meetings: { data: calendar.data.meetings, loading: calendar.loading },
    calendarAccess: calendar.data.access,
    notes,
    folders,
    recordingId: recording.data,
    refreshCalendar: reloadCalendar,
    reloadLibrary,
  };
};

export type Processing = { stage: ProcessingStage; progress: number };

const segmentKey = (s: Segment) => `${s.source}:${s.start_ms}`;

/** Adds segments not already shown, keeping the list in time order. */
const mergeSegments = (current: Segment[], added: Segment[]) => {
  const seen = new Set(current.map(segmentKey));
  const fresh = added.filter((s) => !seen.has(segmentKey(s)));
  if (!fresh.length) return current;
  return [...current, ...fresh].sort((a, b) => a.start_ms - b.start_ms);
};

/** One meeting's transcript, thoughts and notes, kept current from events. */
export const useMeetingDetail = (id: string) => {
  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [processing, setProcessing] = useState<Processing | null>(null);
  const [improving, setImproving] = useState(false);
  const latest = useRef(0);
  const live = useRef<Segment[]>([]);
  const replaced = useRef<Segment[] | null>(null);

  const reload = useCallback(() => {
    const request = ++latest.current;
    commands
      .notetakerGet(id)
      .then((r) => {
        if (request !== latest.current) return;
        const d = unwrap(r);
        setDetail(
          d && {
            ...d,
            segments: mergeSegments(
              replaced.current ?? d.segments,
              live.current,
            ),
          },
        );
        setLoading(false);
      })
      .catch(() => request === latest.current && setLoading(false));
  }, [id]);

  useEffect(() => {
    setLoading(true);
    setProcessing(null);
    setImproving(false);
    live.current = [];
    replaced.current = null;
    reload();
    const stops = [
      events.notetakerTranscript.listen(({ payload }) => {
        if (payload.meeting_id !== id) return;
        if (payload.replace) {
          live.current = [];
          replaced.current = payload.segments;
          setDetail((d) => d && { ...d, segments: payload.segments });
          return;
        }
        live.current = [...live.current, ...payload.segments];
        setDetail(
          (d) =>
            d && {
              ...d,
              segments: mergeSegments(d.segments, payload.segments),
            },
        );
      }),
      events.notetakerProcessing.listen(({ payload }) => {
        if (payload.meeting_id !== id) return;
        if (payload.stage === "improving") setImproving(true);
        else
          setProcessing({ stage: payload.stage, progress: payload.progress });
      }),
      events.notetakerNoteReady.listen(({ payload }) => {
        if (payload.meeting_id !== id) return;
        setProcessing(null);
        setImproving(false);
        reload();
      }),
      events.notetakerState.listen(reload),
    ];
    return () => stops.forEach((s) => void s.then((stop) => stop()));
  }, [id, reload]);

  return { detail, loading, processing, improving, reload };
};

const DAY = 24 * 60 * 60 * 1000;

export const startOfDay = (ms: number) => {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
};

/** "Mon 26 Oct", with the year when it differs from this year. */
export const dayLabel = (ms: number, now = Date.now()) =>
  new Date(ms).toLocaleDateString("en-GB", {
    weekday: "short",
    day: "numeric",
    month: "short",
    ...(new Date(ms).getFullYear() === new Date(now).getFullYear()
      ? {}
      : { year: "numeric" }),
  });

const dayDiff = (ms: number, now: number) =>
  Math.round((startOfDay(ms) - startOfDay(now)) / DAY);

/** "Today", "Yesterday", else "Mon 26 Oct". */
export const relativeDayLabel = (ms: number, now = Date.now()) => {
  const diff = dayDiff(ms, now);
  if (diff === 0) return "Today";
  if (diff === -1) return "Yesterday";
  if (diff === 1) return "Tomorrow";
  return dayLabel(ms, now);
};

export const relativeDayName = (ms: number, now = Date.now()) => {
  const diff = dayDiff(ms, now);
  if (diff === 0) return "Today";
  if (diff === -1) return "Yesterday";
  if (diff === 1) return "Tomorrow";
  return new Date(ms).toLocaleDateString("en-US", { weekday: "long" });
};

/** "Today", "Yesterday", else "26 Oct" (with the year when it differs). */
export const shortDay = (ms: number, now = Date.now()) => {
  const diff = dayDiff(ms, now);
  if (diff === 0) return "Today";
  if (diff === -1) return "Yesterday";
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear();
  return new Date(ms).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    ...(sameYear ? {} : { year: "numeric" }),
  });
};

const clock = new Intl.DateTimeFormat([], {
  hour: "numeric",
  minute: "2-digit",
});

export const timeRange = (start: number, end: number) =>
  `${clock.format(start)} – ${clock.format(end)}`;

export const timeOfDay = (ms: number) => clock.format(ms);

/** "0:53", "12:05", "1:02:09". */
export const clockDuration = (ms: number) => {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
};

/** "45 min", "1 hr 5 min", "Under a minute". */
export const wordDuration = (ms: number) => {
  const min = Math.round(ms / 60000);
  if (min < 1) return "Under a minute";
  const h = Math.floor(min / 60);
  const m = min % 60;
  if (!h) return `${m} min`;
  return m ? `${h} hr ${m} min` : `${h} hr`;
};

export const groupByDay = <T>(items: T[], at: (item: T) => number) => {
  const groups: { day: number; items: T[] }[] = [];
  for (const item of items) {
    const day = startOfDay(at(item));
    const last = groups[groups.length - 1];
    if (last && last.day === day) last.items.push(item);
    else groups.push({ day, items: [item] });
  }
  return groups;
};
