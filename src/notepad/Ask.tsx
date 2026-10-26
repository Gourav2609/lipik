import { useEffect, useRef, useState, type ReactNode } from "react";
import { Copy, History, SquarePen, X } from "lucide-react";
import { commands, type ChatThread, type Citation } from "@/bindings";
import { clock, dayLabel, timeOfDay } from "./format";

type Msg = {
  role: "user" | "assistant";
  content: string;
  citations: Citation[];
  pending?: boolean;
  error?: boolean;
};

const MISS_QUESTION =
  "Sum up the last few minutes of this meeting as a short numbered list of the main points.";

export const useAsk = (meetingId: string | null, onAnswered: () => void) => {
  const [open, setOpen] = useState(false);
  const [history, setHistory] = useState<ChatThread[] | null>(null);
  const [thread, setThread] = useState<string | null>(null);
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const busy = msgs.some((m) => m.pending);
  const idRef = useRef(meetingId);
  idRef.current = meetingId;

  useEffect(() => {
    setOpen(false);
    setHistory(null);
    setThread(null);
    setMsgs([]);
  }, [meetingId]);

  const ask = async (question: string, shown = question) => {
    if (!meetingId || busy) return;
    setOpen(true);
    setHistory(null);
    setMsgs((m) => [
      ...m,
      { role: "user", content: shown, citations: [] },
      { role: "assistant", content: "", citations: [], pending: true },
    ]);
    const r = await commands
      .notetakerAsk(question, meetingId, thread)
      .catch((e) => ({
        status: "error" as const,
        error: String(e),
      }));
    if (idRef.current !== meetingId) return;
    const answer: Msg =
      r.status === "ok"
        ? {
            role: "assistant",
            content: r.data.answer.content,
            citations: r.data.answer.citations,
          }
        : { role: "assistant", content: r.error, citations: [], error: true };
    if (r.status === "ok") setThread(r.data.thread_id);
    setMsgs((m) => [...m.slice(0, -1), answer]);
    if (r.status === "ok") onAnswered();
  };

  const showHistory = async () => {
    setOpen(true);
    const r = await commands.notetakerChats(meetingId).catch(() => null);
    if (idRef.current !== meetingId) return;
    setHistory(r?.status === "ok" ? r.data : []);
  };

  const openThread = async (id: string) => {
    const r = await commands.notetakerChat(id).catch(() => null);
    if (r?.status !== "ok" || idRef.current !== meetingId) return;
    setThread(id);
    setHistory(null);
    setMsgs(
      r.data.map((m) => ({
        role: m.role === "user" ? "user" : "assistant",
        content: m.content,
        citations: m.citations,
      })),
    );
  };

  const newChat = () => {
    setThread(null);
    setMsgs([]);
    setHistory(null);
  };

  return {
    open,
    setOpen,
    history,
    msgs,
    busy,
    ask,
    askMissed: () => ask(MISS_QUESTION, "Catch me up"),
    showHistory,
    openThread,
    newChat,
  };
};

export type AskState = ReturnType<typeof useAsk>;

const inline = (
  text: string,
  cites: Citation[],
  onCite: (c: Citation) => void,
) => {
  const out: ReactNode[] = [];
  const re = /\*\*(.+?)\*\*|\[(\d+)\]/g;
  let at = 0;
  for (const m of text.matchAll(re)) {
    const i = m.index ?? 0;
    if (i > at) out.push(text.slice(at, i));
    if (m[1]) out.push(<strong key={i}>{m[1]}</strong>);
    else {
      const c = cites.find((x) => x.marker === Number(m[2]));
      out.push(
        c ? (
          <button
            key={i}
            type="button"
            className="np-cite"
            title={c.quote}
            onClick={() => onCite(c)}
          >
            {c.start_ms > 0 ? clock(c.start_ms) : "notes"}
          </button>
        ) : (
          m[0]
        ),
      );
    }
    at = i + m[0].length;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
};

/** Paragraphs, "-" and "1." lists, **bold**, and [n] citations. */
const Markdown = ({
  text,
  cites,
  onCite,
}: {
  text: string;
  cites: Citation[];
  onCite: (c: Citation) => void;
}) => {
  const blocks: ReactNode[] = [];
  let list: { ordered: boolean; items: string[] } | null = null;
  const flushList = () => {
    if (!list) return;
    const items = list.items.map((it, i) => (
      <li key={i}>{inline(it, cites, onCite)}</li>
    ));
    blocks.push(
      list.ordered ? (
        <ol key={blocks.length}>{items}</ol>
      ) : (
        <ul key={blocks.length}>{items}</ul>
      ),
    );
    list = null;
  };
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    const bullet = /^[-*]\s+(.*)$/.exec(line);
    const num = /^\d+[.)]\s+(.*)$/.exec(line);
    if (bullet || num) {
      const ordered = !!num;
      if (list && list.ordered !== ordered) flushList();
      if (!list) list = { ordered, items: [] };
      list.items.push((bullet ?? num)![1]);
      continue;
    }
    flushList();
    if (!line) continue;
    const heading = /^#{1,6}\s+(.*)$/.exec(line);
    blocks.push(
      <p key={blocks.length}>
        {heading ? <strong>{heading[1]}</strong> : inline(line, cites, onCite)}
      </p>,
    );
  }
  flushList();
  return <>{blocks}</>;
};

export const AskDrawer = ({
  ask,
  onCite,
  onCopy,
}: {
  ask: AskState;
  onCite: (c: Citation) => void;
  onCopy: (text: string) => void;
}) => {
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [ask.msgs, ask.history]);

  if (!ask.open) return null;

  return (
    <div
      className="np-drawer"
      role="dialog"
      aria-label="Ask about this meeting"
    >
      <div className="np-drawer-head">
        <span>{ask.history ? "Earlier questions" : "Ask about this meeting"}</span>
        <button
          type="button"
          className="np-tool"
          aria-label="Earlier questions"
          title="Earlier questions"
          onClick={ask.showHistory}
        >
          <History size={16} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          className="np-tool"
          aria-label="New chat"
          title="New chat"
          onClick={ask.newChat}
        >
          <SquarePen size={16} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          className="np-tool"
          aria-label="Close"
          title="Close"
          onClick={() => ask.setOpen(false)}
        >
          <X size={16} strokeWidth={1.75} />
        </button>
      </div>
      <div className="np-drawer-list" ref={listRef}>
        {ask.history ? (
          ask.history.length ? (
            ask.history.map((t) => (
              <button
                key={t.thread_id}
                type="button"
                className="np-thread"
                onClick={() => ask.openThread(t.thread_id)}
              >
                {t.first_question}
                <small>
                  {dayLabel(t.updated_ms)}, {timeOfDay(t.updated_ms)}
                </small>
              </button>
            ))
          ) : (
            <div className="np-a error">No earlier questions about this meeting.</div>
          )
        ) : ask.msgs.length ? (
          ask.msgs.map((m, i) =>
            m.role === "user" ? (
              <div key={i} className="np-q">
                {m.content}
              </div>
            ) : m.pending ? (
              <div key={i} className="np-a">
                <span className="np-thinking">Reading the transcript…</span>
              </div>
            ) : (
              <div key={i} className={`np-a${m.error ? " error" : ""}`}>
                <Markdown
                  text={m.content}
                  cites={m.citations}
                  onCite={onCite}
                />
                {!m.error && (
                  <div className="np-a-tools">
                    <button
                      type="button"
                      className="np-tool"
                      aria-label="Copy answer"
                      title="Copy answer"
                      onClick={() =>
                        onCopy(m.content.replace(/\s*\[\d+\]/g, ""))
                      }
                    >
                      <Copy size={14} strokeWidth={1.75} />
                    </button>
                  </div>
                )}
              </div>
            ),
          )
        ) : (
          <div className="np-a error">
            Ask about decisions, owners or a quick recap of this meeting.
          </div>
        )}
      </div>
    </div>
  );
};

type BarProps = {
  live: boolean;
  ask: AskState;
  onStop: () => Promise<boolean>;
  onResume: () => Promise<string | null>;
  onError: (message: string) => void;
};

export const BottomBar = ({
  live,
  ask,
  onStop,
  onResume,
  onError,
}: BarProps) => {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = () => {
    const q = value.trim();
    if (!q || ask.busy) return;
    setValue("");
    void ask.ask(q);
  };

  return (
    <div className="np-bottom">
      {live ? (
        <button
          type="button"
          className="np-rec-btn stop"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            if (!(await onStop())) onError("Couldn't stop the recording");
            setBusy(false);
          }}
        >
          <span className="np-stop-sq" />
          Stop
        </button>
      ) : (
        <button
          type="button"
          className="np-rec-btn resume"
          disabled={busy}
          title="Record again as a new note"
          onClick={async () => {
            setBusy(true);
            const e = await onResume();
            if (e) onError(e);
            setBusy(false);
          }}
        >
          <span className="np-rec-dot" />
          Record
        </button>
      )}
      <div className="np-ask">
        <input
          value={value}
          placeholder="Ask about this meeting"
          aria-label="Ask about this meeting"
          onChange={(e) => setValue(e.target.value)}
          onFocus={() => ask.msgs.length && ask.setOpen(true)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
            if (e.key === "Escape") ask.setOpen(false);
          }}
        />
        {live && (
          <button
            type="button"
            className="np-miss"
            onClick={ask.askMissed}
            disabled={ask.busy}
          >
            Catch me up
          </button>
        )}
      </div>
    </div>
  );
};
