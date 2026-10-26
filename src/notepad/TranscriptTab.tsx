import { memo, useEffect, useMemo, useState } from "react";
import { Copy, Languages, UserPlus } from "lucide-react";
import type { Segment } from "@/bindings";
import { HeaderCard } from "./HeaderCard";
import { clock, markText, speakerLabel } from "./format";
import { LiveBars } from "./icons";
import {
  segKey,
  type LiveText,
  type Partials,
  type Reveal,
} from "./useNotepad";
import { APP_NAME } from "@/flow/brand";

type Props = {
  segments: Segment[];
  live: boolean;
  startMs: number;
  lengthMs: number;
  reveal: Reveal;
  /** What each stream is hearing right now (live only). */
  partials: Partials;
  /** The call app the recording is linked to, if any. */
  callApp: string | null;
  focusMs: { ms: number; at: number } | null;
  onCopy: () => void;
  onSoon: (what: string) => void;
};

const Marked = ({ text, query }: { text: string; query: string }) => (
  <>
    {markText(text, [], query).map((p, i) =>
      p.kind === "hit" ? (
        <mark key={i} className="np-hit">
          {p.text}
        </mark>
      ) : (
        p.text
      ),
    )}
  </>
);

const Elapsed = ({ since }: { since: number }) => {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const tick = () => !document.hidden && setNow(Date.now());
    const t = window.setInterval(tick, 1000);
    document.addEventListener("visibilitychange", tick);
    return () => {
      window.clearInterval(t);
      document.removeEventListener("visibilitychange", tick);
    };
  }, []);
  return <span>{clock(now - since)}</span>;
};

type BubbleProps = {
  seg: Segment;
  index: number;
  live: boolean;
  flash: boolean;
  query: string;
  /** Words shown so far while this segment pops in; undefined once revealed. */
  words?: number;
};

const Bubble = memo(
  ({ seg, index, live, flash, query, words }: BubbleProps) => (
    <div
      className={`np-bubble${flash ? " flash" : ""}`}
      data-seg={index}
    >
      {words !== undefined ? (
        <span className="tent">
          {seg.text.trim().split(/\s+/).slice(0, words).join(" ")}
        </span>
      ) : (
        <Marked text={seg.text.trim()} query={query} />
      )}
      {!live && (
        <span className="np-bubble-flag">{clock(seg.start_ms)}</span>
      )}
    </div>
  ),
);

/** Words still being spoken: settled words in ink, the rest faint, dots
 * until the first word is heard. */
const PartialBubble = ({ live }: { live: LiveText }) => (
  <div className="np-bubble partial">
    {live.text || live.tentative ? (
      <>
        {live.text}
        {live.tentative && <span className="tent">{live.tentative}</span>}
      </>
    ) : (
      <span className="np-dots" aria-label="Hearing speech">
        <i />
        <i />
        <i />
      </span>
    )}
  </div>
);

type Group = { speaker: string; items: { seg: Segment; index: number }[] };

export const TranscriptTab = (p: Props) => {
  const [query, setQuery] = useState("");
  const [flash, setFlash] = useState<number | null>(null);

  const { shown, current } = p.reveal;
  const revealing = p.reveal.words > 0 ? current : null;
  const groups = useMemo(() => {
    const out: Group[] = [];
    p.segments.forEach((seg, index) => {
      const key = segKey(seg);
      if (!shown.has(key) && key !== revealing) return;
      const last = out[out.length - 1];
      if (last && last.speaker === seg.speaker) last.items.push({ seg, index });
      else out.push({ speaker: seg.speaker, items: [{ seg, index }] });
    });
    return out;
  }, [p.segments, shown, revealing]);
  const speakers = useMemo(
    () => new Set(p.segments.map((s) => s.speaker)).size,
    [p.segments],
  );

  useEffect(() => {
    if (!p.focusMs) return;
    const { ms } = p.focusMs;
    let best = -1;
    p.segments.forEach((s, i) => {
      if (s.start_ms <= ms + 500) best = i;
    });
    if (best < 0) best = 0;
    setFlash(best);
    requestAnimationFrame(() =>
      document
        .querySelector(`[data-seg="${best}"]`)
        ?.scrollIntoView({ block: "center", behavior: "smooth" }),
    );
    const t = window.setTimeout(() => setFlash(null), 1800);
    return () => window.clearTimeout(t);
  }, [p.focusMs]);

  const hearing = useMemo(
    () =>
      Object.values(p.partials)
        .filter((l): l is LiveText => !!l)
        .sort((a, b) => a.start_ms - b.start_ms),
    [p.partials],
  );

  const label = p.live ? (
    <span>
      <Elapsed since={p.startMs} />
      {p.callApp && ` · ${p.callApp} call`}
    </span>
  ) : (
    <span>
      {speakers} {speakers === 1 ? "speaker" : "speakers"} · {clock(p.lengthMs)}
    </span>
  );

  return (
    <>
      <HeaderCard
        icon={p.live ? <span className="np-dot live" aria-hidden /> : null}
        label={label}
        query={query}
        onQuery={setQuery}
        placeholder="Search transcript"
      >
        {p.live && (
          <button
            type="button"
            className="np-tool"
            aria-label="Translate"
            title="Translate"
            onClick={() => p.onSoon(`Live translation isn't in ${APP_NAME} yet`)}
          >
            <Languages size={16} strokeWidth={1.75} />
          </button>
        )}
        <button
          type="button"
          className="np-tool"
          aria-label="Copy transcript"
          title="Copy transcript"
          onClick={p.onCopy}
          disabled={!p.segments.length}
        >
          <Copy size={16} strokeWidth={1.75} />
        </button>
        {!p.live && (
          <button
            type="button"
            className="np-tool"
            aria-label="Name a speaker"
            title="Name a speaker"
            onClick={() =>
              p.onSoon(`Naming speakers by hand isn't in ${APP_NAME} yet`)
            }
          >
            <UserPlus size={16} strokeWidth={1.75} />
          </button>
        )}
      </HeaderCard>

      {!groups.length &&
        (p.live ? (
          !hearing.length && (
            <div className="np-empty listening">
              <LiveBars />
              Listening…
            </div>
          )
        ) : (
          <div className="np-empty">Nothing was transcribed in this meeting.</div>
        ))}

      {groups.map((g, gi) => (
        <div className="np-group" key={`${gi}-${segKey(g.items[0].seg)}`}>
          <div className={`np-speaker${g.speaker === "Me" ? " me" : ""}`}>
            {speakerLabel(g.speaker, p.live)}
            <small>{clock(g.items[0].seg.start_ms)}</small>
          </div>
          {g.items.map(({ seg, index }) => {
            const key = segKey(seg);
            return (
              <Bubble
                key={key}
                seg={seg}
                index={index}
                live={p.live}
                flash={flash === index}
                query={query}
                words={key === revealing ? p.reveal.words : undefined}
              />
            );
          })}
        </div>
      ))}

      {p.live &&
        hearing.map((h, i) => {
          const prev =
            i > 0 ? hearing[i - 1].speaker : groups[groups.length - 1]?.speaker;
          return (
            <div
              className={prev === h.speaker ? "np-group-tail" : "np-group"}
              key={`live-${h.source}-${h.start_ms}`}
            >
              {prev !== h.speaker && (
                <div className={`np-speaker${h.speaker === "Me" ? " me" : ""}`}>
                  {speakerLabel(h.speaker, true)}
                </div>
              )}
              <PartialBubble live={h} />
            </div>
          );
        })}
    </>
  );
};
