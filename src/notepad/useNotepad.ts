import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  commands,
  events,
  type CalendarAccess,
  type MeetingDetail,
  type Segment,
  type StreamStatus,
} from "@/bindings";
import { wordCount } from "./format";

/** Index of the processing step shown: 0 saving audio, 1 transcript, 2 notes, 3 done. */
export type Step = 0 | 1 | 2 | 3;

/** Live words pop in faint one at a time, then the whole segment turns to ink at once.
 * `shown` holds the keys of fully revealed segments; `current` is the one popping in. */
export type Reveal = {
  shown: ReadonlySet<string>;
  current: string | null;
  words: number;
};

export type Streams = { mic: StreamStatus; system: StreamStatus };

/** `notetaker-live-text`: what one stream is hearing now (CONTRACT.md). */
export type LiveText = {
  meeting_id: string;
  source: "mic" | "system";
  speaker: string;
  start_ms: number;
  text: string;
  tentative: string;
  active: boolean;
};

/** The utterance each stream is hearing, until it becomes a segment. */
export type Partials = Partial<Record<LiveText["source"], LiveText>>;

export const segKey = (s: Segment) => `${s.source}:${s.start_ms}`;

const allShown = (segments: Segment[]) => ({
  shown: new Set(segments.map(segKey)) as ReadonlySet<string>,
  words: 0,
});

/** Live segments from the two streams arrive out of order; keep them in time order, once. */
const mergeSegments = (have: Segment[], add: Segment[]) => {
  const keys = new Set(have.map(segKey));
  const fresh = add.filter((s) => !keys.has(segKey(s)));
  if (!fresh.length) return have;
  return [...have, ...fresh].sort((a, b) => a.start_ms - b.start_ms);
};

const unwrap = <T>(
  r: { status: "ok"; data: T } | { status: "error"; error: string },
) => (r.status === "ok" ? r.data : null);

export const useNotepad = () => {
  const [meetingId, setMeetingId] = useState<string | null>(null);
  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [calendar, setCalendar] = useState<CalendarAccess>("unsupported");
  const [step, setStep] = useState<Step>(0);
  const [revealed, setRevealed] = useState<{
    shown: ReadonlySet<string>;
    words: number;
  }>(() => allShown([]));
  const [streams, setStreams] = useState<Streams | null>(null);
  const [partials, setPartials] = useState<Partials>({});
  /** Bumped when this meeting stops, so the page can switch to Summary. */
  const [stopped, setStopped] = useState(0);
  /** Bumped when notes arrive while the page watched them being made. */
  const [ready, setReady] = useState(0);
  const idRef = useRef<string | null>(null);
  const liveRef = useRef<string | null>(null);
  const pendingSegs = useRef<Segment[]>([]);
  const watching = useRef(false);
  /** Live words arrive as they are spoken; their segments then show at once. */
  const streamed = useRef(false);

  const refetch = useCallback(async (id = idRef.current) => {
    if (!id) return null;
    const r = await commands.notetakerGet(id).catch(() => null);
    const data = r ? unwrap(r) : null;
    if (idRef.current !== id) return null;
    if (data?.meeting.status === "recording") liveRef.current = id;
    const extra = pendingSegs.current;
    pendingSegs.current = [];
    const merged = data
      ? { ...data, segments: mergeSegments(data.segments, extra) }
      : null;
    setDetail(merged);
    if (data) setRevealed(allShown(data.segments));
    setLoaded(true);
    return data;
  }, []);

  useEffect(() => {
    idRef.current = meetingId;
    setDetail(null);
    setLoaded(false);
    setStreams(null);
    setPartials({});
    streamed.current = false;
    pendingSegs.current = [];
    watching.current = false;
    setStep(0);
    if (!meetingId) return;
    refetch(meetingId).then((d) => {
      if (d?.meeting.status === "processing") watching.current = true;
    });
    commands
      .notetakerStatus()
      .then((s) => {
        if (s.status !== "ok" || idRef.current !== meetingId) return;
        if (s.data.recording && s.data.meeting_id === meetingId)
          setStreams({ mic: s.data.mic, system: s.data.system });
      })
      .catch(() => {});
  }, [meetingId, refetch]);

  useEffect(() => {
    let alive = true;
    commands
      .notepadMeeting()
      .catch(() => null)
      .then(async (id) => {
        if (id) return id;
        const s = await commands.notetakerStatus().catch(() => null);
        return s?.status === "ok" ? s.data.meeting_id : null;
      })
      .then((id) => alive && id && setMeetingId((cur) => cur ?? id));
    const readCalendar = () =>
      commands
        .notetakerCalendarAccess()
        .then((a) => alive && setCalendar(a))
        .catch(() => {});
    readCalendar();
    window.addEventListener("focus", readCalendar);

    const subs = [
      events.notepadMeeting.listen((e) => setMeetingId(e.payload.meeting_id)),
      events.notetakerTranscript.listen((e) => {
        const p = e.payload;
        if (p.meeting_id !== idRef.current) return;
        if (p.replace) {
          pendingSegs.current = [];
          setRevealed(allShown(p.segments));
        } else if (streamed.current) {
          setRevealed((r) => ({
            shown: new Set([...r.shown, ...p.segments.map(segKey)]),
            words: r.words,
          }));
          setPartials((cur) => {
            const next = { ...cur };
            for (const seg of p.segments) {
              const src = seg.source as LiveText["source"];
              if (next[src]?.start_ms === seg.start_ms) delete next[src];
            }
            return next;
          });
        }
        setDetail((d) => {
          if (!d) {
            if (!p.replace)
              pendingSegs.current = mergeSegments(
                pendingSegs.current,
                p.segments,
              );
            return d;
          }
          return {
            ...d,
            segments: p.replace
              ? p.segments
              : mergeSegments(d.segments, p.segments),
          };
        });
      }),
      listen<LiveText>("notetaker-live-text", (e) => {
        const p = e.payload;
        if (p.meeting_id !== idRef.current) return;
        if (p.text || p.tentative) streamed.current = true;
        setPartials((cur) => {
          if (p.active) return { ...cur, [p.source]: p };
          if (cur[p.source]?.start_ms !== p.start_ms) return cur;
          const next = { ...cur };
          delete next[p.source];
          return next;
        });
      }),
      events.notetakerCallDetected.listen(() => {
        if (idRef.current && idRef.current === liveRef.current) refetch();
      }),
      events.notetakerStreamChanged.listen((e) => {
        const p = e.payload;
        if (p.meeting_id !== idRef.current) return;
        setStreams({ mic: p.mic, system: p.system });
      }),
      events.notetakerState.listen((e) => {
        const p = e.payload;
        if (p.recording) {
          liveRef.current = p.meeting_id;
          return;
        }
        const id = p.meeting_id ?? liveRef.current;
        liveRef.current = null;
        if (!id || id !== idRef.current) return;
        setStreams(null);
        setPartials({});
        watching.current = true;
        setStep(0);
        setStopped((n) => n + 1);
        refetch();
      }),
      events.notetakerProcessing.listen((e) => {
        const p = e.payload;
        if (p.meeting_id !== idRef.current || p.stage === "improving") return;
        watching.current = true;
        const next: Step = p.stage === "summarizing" ? 2 : 1;
        setStep((s) => (next > s ? next : s));
        setDetail((d) =>
          d && d.meeting.status !== "processing"
            ? {
                ...d,
                meeting: {
                  ...d.meeting,
                  status: "processing",
                  summary_status: "pending",
                },
              }
            : d,
        );
      }),
      events.notetakerNoteReady.listen((e) => {
        if (e.payload.meeting_id !== idRef.current) return;
        setStep(3);
        refetch().then(() => {
          if (watching.current) setReady((n) => n + 1);
          watching.current = false;
        });
      }),
    ];
    return () => {
      alive = false;
      window.removeEventListener("focus", readCalendar);
      subs.forEach((s) => s.then((off) => off()).catch(() => {}));
    };
  }, [refetch]);

  const segments: Segment[] = detail?.segments ?? [];
  const current = segments.find((s) => !revealed.shown.has(segKey(s))) ?? null;
  const currentKey = current ? segKey(current) : null;
  useEffect(() => {
    if (!current || !currentKey) return;
    if (document.hidden) {
      setRevealed(allShown(segments));
      return;
    }
    const total = wordCount(current.text);
    const backlog = segments.length - revealed.shown.size - 1;
    const full = revealed.words >= total;
    const wait = full
      ? backlog > 0
        ? 60
        : 450
      : backlog > 0
        ? 40
        : Math.min(320, Math.max(90, 6000 / Math.max(total, 1)));
    const t = window.setTimeout(
      () =>
        setRevealed((r) =>
          full
            ? { shown: new Set(r.shown).add(currentKey), words: 0 }
            : { shown: r.shown, words: r.words + 1 },
        ),
      wait,
    );
    return () => window.clearTimeout(t);
  }, [revealed, segments, current, currentKey]);

  const reveal: Reveal = {
    shown: revealed.shown,
    current: currentKey,
    words: revealed.words,
  };

  const patchMeeting = (patch: Partial<MeetingDetail["meeting"]>) =>
    setDetail((d) => (d ? { ...d, meeting: { ...d.meeting, ...patch } } : d));

  const actions = {
    stop: async () => {
      const r = await commands.notetakerStop().catch(() => null);
      return r?.status === "ok";
    },
    resume: async () => {
      const title = detail?.meeting.title_auto
        ? null
        : (detail?.meeting.title ?? null);
      const r = await commands.notetakerStart(title).catch(() => null);
      if (r?.status === "ok") setMeetingId(r.data.id);
      return r?.status === "ok"
        ? null
        : (r?.error ?? "Couldn't start recording");
    },
    rename: async (title: string) => {
      if (!idRef.current) return;
      patchMeeting({ title, title_auto: false });
      await commands.notetakerRename(idRef.current, title).catch(() => null);
    },
    /** Names every line of one speaker; returns why it failed, if it did. */
    renameSpeaker: async (
      speakerKey: string,
      name: string,
    ): Promise<string | null> => {
      const id = idRef.current;
      if (!id) return null;
      setDetail((d) =>
        d
          ? {
              ...d,
              segments: d.segments.map((s) =>
                s.speaker_key === speakerKey ? { ...s, speaker: name } : s,
              ),
            }
          : d,
      );
      const r = await commands
        .notetakerRenameSpeaker(id, speakerKey, name)
        .catch(() => null);
      if (r?.status === "ok") return null;
      refetch(id);
      return `Couldn't rename: ${r?.error ?? "try again"}`;
    },
    moveToFolder: async (folder: string | null) => {
      if (!idRef.current) return;
      patchMeeting({ folder });
      await commands
        .notetakerMoveToFolder(idRef.current, folder)
        .catch(() => null);
    },
    saveThoughts: async (id: string, markdown: string) => {
      if (id === idRef.current)
        setDetail((d) =>
          d && d.meeting.id === id ? { ...d, thoughts: markdown } : d,
        );
      const r = await commands
        .notetakerSaveThoughts(id, markdown)
        .catch((e) => ({ status: "error" as const, error: String(e) }));
      return r.status === "ok" ? null : r.error;
    },
    regenerate: async () => {
      if (!idRef.current) return;
      const r = await commands
        .notetakerRegenerate(idRef.current)
        .catch(() => null);
      if (r?.status !== "ok") return r?.error ?? "Couldn't start again";
      watching.current = true;
      setStep(0);
      patchMeeting({
        status: "processing",
        summary_status: "pending",
        summary_error: null,
      });
      return null;
    },
    remove: async () => {
      if (!idRef.current) return "No note open";
      const r = await commands.notetakerDelete(idRef.current).catch(() => null);
      if (r?.status !== "ok") return r?.error ?? "Couldn't delete the note";
      await commands.notepadClose().catch(() => null);
      setMeetingId(null);
      return null;
    },
    connectCalendar: async () => {
      const r = await commands
        .openMainPage("connect-calendar")
        .catch(() => null);
      return r?.status === "ok" ? null : "Couldn't open the calendar settings";
    },
    refetch,
  };

  return {
    meetingId,
    detail,
    loaded,
    calendar,
    step,
    reveal,
    streams,
    partials,
    stopped,
    ready,
    actions,
  };
};

export type Notepad = ReturnType<typeof useNotepad>;
