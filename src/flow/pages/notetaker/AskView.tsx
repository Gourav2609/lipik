import { useEffect, useRef, useState } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronLeft, FileText } from "lucide-react";
import { Button } from "../../ui";
import { commands, type ChatMessage, type Citation } from "@/bindings";
import { errorText, unwrap } from "../textflow";
import { clockDuration } from "./data";

export type AskRequest = { question: string; n: number };

const SUGGESTIONS = [
  "Summarize this week's meetings",
  "Who am I waiting to hear from?",
  "What did we decide this week?",
];

const MAX_SOURCES = 5;

type Pending = { question: string; error: string | null };

const withCitationLinks = (text: string) =>
  text.replace(/\[(\d+)\](?!\()/g, "[$1](#cite-$1)");

const CitationChip = ({
  citation,
  onOpen,
}: {
  citation: Citation;
  onOpen: (c: Citation) => void;
}) => (
  <button
    type="button"
    onClick={() => onOpen(citation)}
    title={[
      citation.meeting_title,
      citation.start_ms > 0 && clockDuration(citation.start_ms),
      citation.quote && `“${citation.quote}”`,
    ]
      .filter(Boolean)
      .join(" · ")}
    className="fk-num mx-[2px] inline-flex h-[18px] min-w-[18px] -translate-y-[1px] items-center justify-center rounded-[4px] bg-[var(--fk-accent-soft)] px-[4px] align-middle text-[11px] font-semibold leading-none text-[var(--fk-accent)] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-accent)] hover:text-[var(--fk-accent-text)]"
  >
    {citation.marker}
  </button>
);

const Answer = ({
  message,
  onOpenCitation,
}: {
  message: ChatMessage;
  onOpenCitation: (c: Citation) => void;
}) => {
  const byMarker = new Map(message.citations.map((c) => [c.marker, c]));
  const components: Components = {
    a: ({ href, children }) => {
      const cite = href?.startsWith("#cite-")
        ? byMarker.get(Number(href.slice(6)))
        : undefined;
      if (cite) return <CitationChip citation={cite} onOpen={onOpenCitation} />;
      if (href?.startsWith("#cite-")) return <>{children}</>;
      return (
        <button
          type="button"
          onClick={() => href?.startsWith("https://") && void openUrl(href)}
          className="underline underline-offset-2"
        >
          {children}
        </button>
      );
    },
    p: ({ children }) => <p className="mb-[12px] last:mb-0">{children}</p>,
    ul: ({ children }) => (
      <ul className="mb-[12px] list-disc pl-[22px] last:mb-0">{children}</ul>
    ),
    ol: ({ children }) => (
      <ol className="mb-[12px] list-decimal pl-[22px] last:mb-0">{children}</ol>
    ),
    li: ({ children }) => <li className="pl-[2px]">{children}</li>,
    strong: ({ children }) => (
      <strong className="font-semibold">{children}</strong>
    ),
    h1: ({ children }) => <p className="mb-[8px] font-semibold">{children}</p>,
    h2: ({ children }) => <p className="mb-[8px] font-semibold">{children}</p>,
    h3: ({ children }) => <p className="mb-[8px] font-semibold">{children}</p>,
  };
  const sources = [
    ...new Map(message.citations.map((c) => [c.meeting_id, c])).values(),
  ].slice(0, MAX_SOURCES);
  return (
    <div className="nt-fade">
      <div className="text-[15px] leading-[26px]">
        <ReactMarkdown components={components}>
          {withCitationLinks(message.content)}
        </ReactMarkdown>
      </div>
      {sources.length > 0 && (
        <div className="mt-[16px]">
          <div className="fk-eyebrow">Sources</div>
          <div className="nt-ruled-top mt-[8px]">
            {sources.map((c) => (
              <button
                type="button"
                key={c.meeting_id}
                onClick={() => onOpenCitation({ ...c, start_ms: 0 })}
                className="nt-line flex h-[40px] w-full items-center gap-[8px] px-[12px] text-left text-[14px]"
              >
                <FileText size={16} strokeWidth={1.75} className="shrink-0 text-[var(--fk-text-muted)]" />
                <span className="truncate">{c.meeting_title}</span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

const Question = ({ text }: { text: string }) => (
  <p className="nt-fade border-l-2 border-[var(--fk-accent)] pl-[12px] text-[16px] font-semibold leading-[24px]">
    {text}
  </p>
);

const Thinking = () => (
  <div className="flex flex-col gap-[8px]" aria-label="Reading your meetings">
    <span className="nt-sheen text-[15px]">Reading your meetings…</span>
    <span className="nt-skeleton h-[10px] w-[90%] rounded-[4px]" />
    <span className="nt-skeleton h-[10px] w-[70%] rounded-[4px]" />
  </div>
);

/** A chat about all meetings, or about one (`meetingId`). */
export const AskView = ({
  threadId,
  meetingId,
  meetingTitle,
  request,
  onRequestTaken,
  onThread,
  onBack,
  onOpenCitation,
}: {
  threadId: string | null;
  meetingId: string | null;
  meetingTitle?: string;
  request: AskRequest | null;
  onRequestTaken: () => void;
  onThread: (threadId: string) => void;
  onBack: () => void;
  onOpenCitation: (c: Citation) => void;
}) => {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [pending, setPending] = useState<Pending | null>(null);
  const thread = useRef(threadId);
  const end = useRef<HTMLDivElement>(null);
  const taken = useRef(0);

  const ask = async (q: string) => {
    setPending({ question: q, error: null });
    try {
      const res = unwrap(
        await commands.notetakerAsk(q, meetingId, thread.current),
      );
      thread.current = res.thread_id;
      setMessages((m) => [...m, res.question, res.answer]);
      setPending(null);
      onThread(res.thread_id);
    } catch (err) {
      setPending({ question: q, error: errorText(err) });
    }
  };

  useEffect(() => {
    if (threadId === thread.current && messages.length) return;
    thread.current = threadId;
    setMessages([]);
    setPending(null);
    if (!threadId) return;
    let live = true;
    commands
      .notetakerChat(threadId)
      .then((r) => live && setMessages(unwrap(r)))
      .catch(() => live && setPending({ question: "", error: "Couldn't open this chat. Try again from Earlier questions." }));
    return () => {
      live = false;
    };
  }, [threadId]);

  useEffect(() => {
    if (!request || taken.current === request.n) return;
    taken.current = request.n;
    onRequestTaken();
    void ask(request.question);
  }, [request?.n]);

  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [messages.length, pending?.question]);

  const empty = !messages.length && !pending;

  return (
    <div>
      <Button
        variant="ghost"
        size="sm"
        className="-ml-[8px] text-[var(--fk-text-muted)]"
        icon={<ChevronLeft size={16} strokeWidth={1.75} />}
        onClick={onBack}
      >
        Back
      </Button>
      <h1 className="fk-display mt-[20px] text-[28px] leading-[38px]">
        {meetingTitle ? `Ask about ${meetingTitle}` : "Ask your meetings"}
      </h1>
      {empty && (
        <div className="nt-fade mt-[4px]">
          <p className="text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
            Answers come from your transcripts, summaries and notes, with links
            to where they were said.
          </p>
          {!meetingTitle && (
            <div className="mt-[20px] flex flex-wrap gap-[8px]">
              {SUGGESTIONS.map((s) => (
                <Button
                  key={s}
                  variant="secondary"
                  size="sm"
                  className="font-medium"
                  onClick={() => void ask(s)}
                >
                  {s}
                </Button>
              ))}
            </div>
          )}
        </div>
      )}
      <div className="mt-[24px] border-t border-[var(--fk-border)]" />
      <div className="mt-[24px] flex flex-col gap-[20px]">
        {messages.map((m) =>
          m.role === "user" ? (
            <Question key={m.id} text={m.content} />
          ) : (
            <Answer key={m.id} message={m} onOpenCitation={onOpenCitation} />
          ),
        )}
        {pending?.question && <Question text={pending.question} />}
        {pending &&
          (pending.error ? (
            <div className="border-l-2 border-[var(--fk-live)] pl-[12px] text-[15px] leading-[22px]">
              <p>{pending.error}</p>
              {pending.question && (
                <Button
                  variant="secondary"
                  size="sm"
                  className="mt-[8px]"
                  onClick={() => void ask(pending.question)}
                >
                  Try again
                </Button>
              )}
            </div>
          ) : (
            <Thinking />
          ))}
      </div>
      <div ref={end} />
    </div>
  );
};
