import { useEffect, useState, type ReactNode } from "react";
import { Check, Copy, ThumbsDown, ThumbsUp } from "lucide-react";
import type { MeetingDetail } from "@/bindings";
import { APP_NAME } from "@/flow/brand";
import { markText, meetingLength, wordCount } from "./format";
import type { Step } from "./useNotepad";

type Props = {
  detail: MeetingDetail;
  step: Step;
  names: string[];
  onCopy: () => void;
  onRegenerate: () => Promise<string | null | undefined>;
};

export const isGenerating = (d: MeetingDetail) =>
  d.meeting.status === "processing" ||
  (d.meeting.status !== "recording" && d.meeting.summary_status === "pending");

export const SummaryTab = ({
  detail,
  step,
  names,
  onCopy,
  onRegenerate,
}: Props) => {
  const { meeting, summary } = detail;

  if (meeting.status === "recording")
    return (
      <p className="np-soft plain">
        The summary is written when the meeting ends.
      </p>
    );

  if (isGenerating(detail))
    return (
      <>
        <ProcessingCard detail={detail} target={step} />
        <Skeleton />
        <Skeleton />
      </>
    );

  if (meeting.status === "failed" || !summary)
    return (
      <ErrorCard
        message={
          meeting.summary_error ??
          (meeting.status === "failed"
            ? `${APP_NAME} couldn't finish this meeting's notes.`
            : "This meeting has no summary yet.")
        }
        onRetry={onRegenerate}
      />
    );

  return <SummaryBody detail={detail} names={names} onCopy={onCopy} />;
};

const Skeleton = () => (
  <div className="np-skel" aria-hidden>
    <i />
    <i />
    <i style={{ width: "80%" }} />
    <i style={{ width: "88%" }} />
  </div>
);

const HOLD_DONE_MS = 1400;
const ROTATE_MS = 3400;

const stepMessages = (detail: MeetingDetail, step: number) => {
  const minutes = Math.max(1, Math.round(meetingLength(detail) / 60_000));
  const words = detail.segments.reduce((n, s) => n + wordCount(s.text), 0);
  if (step === 0)
    return [`Saving ${minutes} ${minutes === 1 ? "minute" : "minutes"} of audio`];
  if (step === 1)
    return [
      "Writing the final transcript",
      ...(words > 0 ? [`Checking ${words.toLocaleString()} words`] : []),
      "Matching words to speakers",
    ];
  return ["Writing the summary", "Finding decisions and next steps"];
};

const DONE_TEXT = ["Audio saved", "Transcript ready", "Summary ready"];

/** Three steps. A finished step shows in green for a moment before the next one starts. */
const ProcessingCard = ({
  detail,
  target,
}: {
  detail: MeetingDetail;
  target: Step;
}) => {
  const [shown, setShown] = useState({
    step: Math.min(target, 2),
    done: false,
  });
  const [line, setLine] = useState(0);

  useEffect(() => {
    if (target <= shown.step || target === 3) return;
    if (!shown.done) {
      setShown((s) => ({ ...s, done: true }));
      return;
    }
    const t = window.setTimeout(
      () => setShown((s) => ({ step: s.step + 1, done: false })),
      HOLD_DONE_MS,
    );
    return () => window.clearTimeout(t);
  }, [target, shown]);

  useEffect(() => {
    setLine(0);
    if (shown.done) return;
    const t = window.setInterval(() => setLine((n) => n + 1), ROTATE_MS);
    return () => window.clearInterval(t);
  }, [shown]);

  const messages = stepMessages(detail, shown.step);
  const text = shown.done
    ? DONE_TEXT[shown.step]
    : messages[line % messages.length];

  return (
    <div className={`np-step${shown.done ? " done" : ""}`} role="status">
      <div className="np-step-row">
        {shown.done && (
          <span className="np-step-icon">
            <Check size={16} strokeWidth={1.75} />
          </span>
        )}
        <StepText text={text} />
        <span className="np-step-n">Step {shown.step + 1} of 3</span>
      </div>
      <div className="np-steps" aria-hidden>
        {[0, 1, 2].map((i) => (
          <i
            key={i}
            className={
              i < shown.step || (i === shown.step && shown.done) ? "on" : ""
            }
          />
        ))}
      </div>
    </div>
  );
};

/** A new line rises into place; the old one fades out. */
const StepText = ({ text }: { text: string }) => {
  const [cur, setCur] = useState(text);
  const [prev, setPrev] = useState<string | null>(null);
  const [first, setFirst] = useState(true);

  useEffect(() => {
    if (text === cur) return;
    setPrev(cur);
    setCur(text);
    setFirst(false);
    const t = window.setTimeout(() => setPrev(null), 140);
    return () => window.clearTimeout(t);
  }, [text, cur]);

  return (
    <span className="np-step-msg">
      {prev && (
        <span key={`out-${prev}`} className="out">
          {prev}
        </span>
      )}
      <span key={`in-${cur}`} className={first ? undefined : "in"}>
        {cur}
      </span>
    </span>
  );
};

const ErrorCard = ({
  message,
  onRetry,
}: {
  message: string;
  onRetry: () => Promise<string | null | undefined>;
}) => {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <div className="np-error" role="alert">
      <span>{error ?? message}</span>
      <button
        type="button"
        className="np-btn"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          setError((await onRetry()) ?? null);
          setBusy(false);
        }}
      >
        Try again
      </button>
    </div>
  );
};

const Text = ({
  text,
  names,
  query,
}: {
  text: string;
  names: string[];
  query: string;
}) => (
  <>
    {markText(text, names, query).map((p, i) =>
      p.kind === "hit" ? (
        <mark key={i} className="np-hit">
          {p.text}
        </mark>
      ) : p.kind === "name" ? (
        <span key={i} className="np-name">
          {p.text}
        </span>
      ) : (
        p.text
      ),
    )}
  </>
);

const SummaryBody = ({
  detail,
  names,
  onCopy,
}: {
  detail: MeetingDetail;
  names: string[];
  onCopy: () => void;
}) => {
  const summary = detail.summary!;
  const [vote, setVote] = useState<"up" | "down" | null>(null);
  const t = (s: string) => <Text text={s} names={names} query="" />;

  const sections: { heading: string; items: ReactNode[] }[] = [
    ...summary.sections.map((s) => ({
      heading: s.heading ?? "",
      items: (s.bullets ?? []).map(t),
    })),
    { heading: "Key points", items: summary.key_points.map(t) },
    { heading: "Decisions", items: summary.decisions.map(t) },
    {
      heading: "Next steps",
      items: summary.action_items.map((a) => (
        <>
          {a.owner && <span className="np-name">{a.owner}: </span>}
          {t(a.text)}
          {a.due && <span className="np-due"> · {a.due}</span>}
        </>
      )),
    },
  ].filter((s) => s.items.length);

  return (
    <>
      <div className="np-sum">
        {summary.overview.trim() && <p>{t(summary.overview.trim())}</p>}
        {sections.map((s, i) => (
          <section key={i}>
            {s.heading && <h3>{t(s.heading)}</h3>}
            <ul>
              {s.items.map((item, j) => (
                <li key={j}>{item}</li>
              ))}
            </ul>
          </section>
        ))}
      </div>
      <div className="np-feedback">
        <span>Was this summary useful?</span>
        <button
          type="button"
          className={`np-tool${vote === "up" ? " on" : ""}`}
          aria-label="Useful"
          title="Useful"
          aria-pressed={vote === "up"}
          onClick={() => setVote((v) => (v === "up" ? null : "up"))}
        >
          <ThumbsUp size={16} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          className={`np-tool${vote === "down" ? " on" : ""}`}
          aria-label="Not useful"
          title="Not useful"
          aria-pressed={vote === "down"}
          onClick={() => setVote((v) => (v === "down" ? null : "down"))}
        >
          <ThumbsDown size={16} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          className="np-tool"
          aria-label="Copy summary"
          title="Copy summary"
          onClick={onCopy}
        >
          <Copy size={16} strokeWidth={1.75} />
        </button>
      </div>
    </>
  );
};
