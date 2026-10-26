import { useEffect, useState } from "react";
import { AlertCircle, Check, ChevronDown, ChevronUp } from "lucide-react";
import { commands, type StreamStatus } from "@/bindings";
import type { Streams } from "./useNotepad";
import { APP_NAME } from "@/flow/brand";

const streamProblem = (s: StreamStatus, name: string) =>
  s.state === "silent" || s.state === "unavailable"
    ? (s.detail ??
      (s.state === "silent"
        ? `Nothing heard from the ${name} yet.`
        : `The ${name} isn't being recorded. Reconnecting…`))
    : null;

const StreamWarnings = ({ streams }: { streams: Streams | null }) => {
  if (!streams) return null;
  const problems = [
    streamProblem(streams.system, "call"),
    streamProblem(streams.mic, "microphone"),
  ].filter((p): p is string => !!p);
  return (
    <>
      {problems.map((text) => (
        <div key={text} className="np-stream-warn" role="status">
          <AlertCircle size={14} strokeWidth={1.75} />
          <span>{text}</span>
        </div>
      ))}
    </>
  );
};

/** Margin note on the live transcript; shrinks once the first words arrive. */
export const LiveNotices = ({
  heard,
  streams,
}: {
  heard: boolean;
  streams: Streams | null;
}) => {
  const [open, setOpen] = useState(!heard);
  const [tip, setTip] = useState(false);

  useEffect(() => setOpen(!heard), [heard]);

  return (
    <>
      <StreamWarnings streams={streams} />
      <div className={`np-banner ${open ? "open" : "shut"}`}>
        {open ? (
          <>
            <span key="open" className="np-banner-text">
              When the call ends, {APP_NAME} cleans up the transcript and adds
              speaker names.
            </span>
            <button
              type="button"
              className="np-btn ghost"
              onClick={() => commands.openMainPage("notetaker").catch(() => {})}
            >
              Open Meetings
            </button>
          </>
        ) : (
          <>
            <span key="shut" className="np-banner-text">
              Speaker names come after the call.
            </span>
            <button
              type="button"
              className="np-tool"
              aria-label="Show more"
              title="Show more"
              onClick={() => setOpen(true)}
            >
              <ChevronUp size={16} strokeWidth={1.75} />
            </button>
          </>
        )}
      </div>
      {tip && (
        <div className="np-consent-tip">
          Tell everyone on the call before you record. {APP_NAME} keeps the
          audio, transcript and notes on this Mac.
        </div>
      )}
      <div className="np-consent">
        Ask before you record other people.{" "}
        <button type="button" onClick={() => setTip((t) => !t)}>
          {tip ? "Hide" : "Why"}
        </button>
      </div>
    </>
  );
};

/** "Summary ready", then the calendar note (Summary tab only). */
export const SummaryNotices = ({
  toast,
  fresh,
  showPromo,
  onConnect,
}: {
  toast: boolean;
  fresh: boolean;
  showPromo: boolean;
  onConnect: () => void;
}) => {
  const [enter, setEnter] = useState(fresh);
  const [collapsed, setCollapsed] = useState(!fresh);

  useEffect(() => {
    if (!fresh) return;
    setEnter(true);
    setCollapsed(false);
  }, [fresh]);

  if (toast)
    return (
      <div className="np-toast" role="status">
        <Check size={16} strokeWidth={1.75} />
        Summary ready
      </div>
    );

  if (!showPromo) return null;

  const toggle = () => {
    setEnter(false);
    setCollapsed((c) => !c);
  };

  return (
    <div
      className={`np-promo${enter ? " enter" : ""}`}
      style={{ height: collapsed ? 88 : 252 }}
    >
      <div className="np-promo-head">
        <div className="np-promo-copy">
          <div className="np-promo-title">Add names to your notes</div>
          <div className="np-promo-body">
            Connect your calendar so summaries know who said what.
          </div>
        </div>
        <button type="button" className="np-btn primary" onClick={onConnect}>
          Connect
        </button>
        <button
          type="button"
          className="np-tool"
          aria-label={collapsed ? "Show example" : "Hide example"}
          title={collapsed ? "Show example" : "Hide example"}
          onClick={toggle}
        >
          {collapsed ? (
            <ChevronUp size={16} strokeWidth={1.75} />
          ) : (
            <ChevronDown size={16} strokeWidth={1.75} />
          )}
        </button>
      </div>
      <div className="np-promo-card" aria-hidden>
        <h4>Supplier review</h4>
        <span className="p">Priya</span> shared the new delivery dates;{" "}
        <span className="p">Arjun</span> agreed to the revised rates.
        <b>Next steps</b>
        <ul>
          <li>
            <span className="p">Priya</span> to send the updated schedule by
            Friday
          </li>
          <li>
            <span className="p">Arjun</span> to sign the order this week
          </li>
        </ul>
      </div>
    </div>
  );
};

export const ThoughtsNote = () => (
  <div className="np-thoughts-note">
    Your notes stay out of anything you share.
  </div>
);
