import type { MeetingDetail, NoteSummary, Segment } from "@/bindings";
import { isDefaultSpeaker } from "@/lib/speakers";

export const clock = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
};

export const dayLabel = (ms: number) => {
  const day = new Date(ms);
  const today = new Date();
  const start = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const diff = Math.round((start(today) - start(day)) / 86_400_000);
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  return day.toLocaleDateString("en-GB", {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: day.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
};

export const timeOfDay = (ms: number) =>
  new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

export const speakerLabel = (speaker: string, live: boolean) => {
  if (speaker === "Me") return "You";
  if (speaker === "Them") return live ? "Them" : "Others";
  return speaker;
};

/** Consecutive segments from one speaker share one label. */
export const groupSegments = (segments: Segment[]) => {
  const groups: {
    speaker: string;
    items: { seg: Segment; index: number }[];
  }[] = [];
  segments.forEach((seg, index) => {
    const last = groups[groups.length - 1];
    const prev = last?.items[last.items.length - 1].seg;
    const same =
      !!prev &&
      (prev.speaker_key || prev.speaker) === (seg.speaker_key || seg.speaker);
    if (last && same) last.items.push({ seg, index });
    else groups.push({ speaker: seg.speaker, items: [{ seg, index }] });
  });
  return groups;
};

export const meetingLength = (detail: MeetingDetail) => {
  const { meeting, segments } = detail;
  if (meeting.end_ms) return meeting.end_ms - meeting.start_ms;
  return segments.reduce((max, s) => Math.max(max, s.end_ms), 0);
};

export const wordCount = (text: string) =>
  text.split(/\s+/).filter(Boolean).length;

/** Names worth colouring in the notes: attendees, named speakers, owners. */
export const peopleNames = (detail: MeetingDetail) => {
  const names = new Set<string>();
  const add = (name: string | null | undefined) => {
    if (!name) return;
    const clean = name.trim();
    if (clean.length < 2 || isDefaultSpeaker(clean)) return;
    names.add(clean);
    const first = clean.split(/\s+/)[0];
    if (first.length > 1) names.add(first);
  };
  detail.meeting.attendees.forEach((a) => add(a.name));
  detail.segments.forEach((s) => add(s.speaker));
  detail.summary?.action_items.forEach((a) => add(a.owner));
  return [...names].sort((a, b) => b.length - a.length);
};

export const escapeRegExp = (s: string) =>
  s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

export type Piece = { text: string; kind?: "name" | "hit" };

/** Splits text into plain runs, people's names and search hits. */
export const markText = (
  text: string,
  names: string[],
  query: string,
): Piece[] => {
  const parts: string[] = [];
  if (query.length >= 2) parts.push(escapeRegExp(query));
  if (names.length)
    parts.push(`\\b(?:${names.map(escapeRegExp).join("|")})(?:'s)?\\b`);
  if (!parts.length) return [{ text }];
  const re = new RegExp(parts.join("|"), "gi");
  const out: Piece[] = [];
  let at = 0;
  for (const m of text.matchAll(re)) {
    const i = m.index ?? 0;
    if (i > at) out.push({ text: text.slice(at, i) });
    const isHit =
      query.length >= 2 && m[0].toLowerCase() === query.toLowerCase();
    out.push({ text: m[0], kind: isHit ? "hit" : "name" });
    at = i + m[0].length;
  }
  if (at < text.length) out.push({ text: text.slice(at) });
  return out;
};

export const transcriptText = (segments: Segment[]) =>
  groupSegments(segments)
    .map(
      (g) =>
        `${speakerLabel(g.speaker, false)}: ${g.items.map((i) => i.seg.text.trim()).join(" ")}`,
    )
    .join("\n\n");

export const summaryMarkdown = (summary: NoteSummary) => {
  const lines: string[] = [summary.overview.trim(), ""];
  const list = (heading: string, items: string[]) => {
    if (!items.length) return;
    lines.push(`## ${heading}`, ...items.map((b) => `- ${b}`), "");
  };
  summary.sections.forEach((s) => list(s.heading ?? "Notes", s.bullets ?? []));
  list("Key points", summary.key_points);
  list("Decisions", summary.decisions);
  list(
    "Next steps",
    summary.action_items.map(
      (a) =>
        `${a.owner ? `${a.owner}: ` : ""}${a.text}${a.due ? ` (by ${a.due})` : ""}`,
    ),
  );
  return lines.join("\n").trim();
};

export const noteMarkdown = (detail: MeetingDetail, withThoughts: boolean) => {
  const { meeting, summary, segments, thoughts } = detail;
  const out = [`# ${meeting.title || "Untitled"}`, ""];
  if (withThoughts && thoughts.trim())
    out.push("## My notes", "", thoughts.trim(), "");
  if (summary) out.push(summaryMarkdown(summary), "");
  if (segments.length) out.push("## Transcript", "", transcriptText(segments));
  return out.join("\n").trim();
};

export const copyText = async (text: string) => {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
};
