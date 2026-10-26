import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { AlertCircle } from "lucide-react";

const SAVE_MS = 600;
const RETRY_MS = 4000;
const LIST = /^(\s*)([-*]|\d+\.)\s(.*)$/;
const GONE = /No meeting with id/;

type Save = (id: string, markdown: string) => Promise<string | null>;

/** One save at a time per meeting, always of the newest text, shared by every
 * mounted copy of the tab so an old retry can never write over newer text. */
type Queue = {
  latest: string | null;
  running: boolean;
  retry?: number;
  onFailed?: (failed: boolean) => void;
};
const queues = new Map<string, Queue>();
const queueFor = (id: string) => {
  let q = queues.get(id);
  if (!q) queues.set(id, (q = { latest: null, running: false }));
  return q;
};

const runQueue = async (id: string, save: Save) => {
  const q = queueFor(id);
  if (q.running) return;
  q.running = true;
  window.clearTimeout(q.retry);
  q.retry = undefined;
  while (q.latest !== null) {
    const text = q.latest;
    q.latest = null;
    const error = await save(id, text);
    if (!error) {
      q.onFailed?.(false);
      continue;
    }
    if (GONE.test(error)) {
      q.latest = null;
      q.onFailed?.(false);
      break;
    }
    if (q.latest === null) q.latest = text;
    q.onFailed?.(true);
    q.retry = window.setTimeout(() => void runQueue(id, save), RETRY_MS);
    break;
  }
  q.running = false;
};

const enqueue = (id: string, text: string, save: Save) => {
  queueFor(id).latest = text;
  void runQueue(id, save);
};

type Props = {
  meetingId: string;
  thoughts: string;
  summaryReady: boolean;
  onSave: Save;
  onOpenSummary: () => void;
};

/** Plain markdown notes: lists continue on Enter, Tab indents, saved while typing. */
export const ThoughtsTab = ({
  meetingId,
  thoughts,
  summaryReady,
  onSave,
  onOpenSummary,
}: Props) => {
  const [text, setText] = useState(thoughts);
  const [failed, setFailed] = useState(false);
  const [focused, setFocused] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);
  const pending = useRef<string | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const saveRef = useRef(onSave);
  saveRef.current = onSave;

  const flush = () => {
    window.clearTimeout(timer.current);
    const text = pending.current;
    if (text === null) return;
    pending.current = null;
    enqueue(meetingId, text, saveRef.current);
  };

  useEffect(() => {
    const q = queueFor(meetingId);
    q.onFailed = setFailed;
    return () => {
      if (q.onFailed === setFailed) q.onFailed = undefined;
    };
  }, [meetingId]);

  const focusedRef = useRef(false);
  focusedRef.current = focused;

  useEffect(() => {
    if (pending.current !== null || focusedRef.current) return;
    setText(thoughts);
  }, [thoughts]);

  useEffect(() => {
    const onHide = () => flush();
    window.addEventListener("beforeunload", onHide);
    return () => {
      window.removeEventListener("beforeunload", onHide);
      flush();
    };
  }, [meetingId]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${el.scrollHeight}px`;
  }, [text, focused]);

  const change = (next: string) => {
    setText(next);
    pending.current = next;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(flush, SAVE_MS);
  };

  const replaceRange = (
    el: HTMLTextAreaElement,
    from: number,
    to: number,
    insert: string,
  ) => {
    const next = el.value.slice(0, from) + insert + el.value.slice(to);
    change(next);
    const caret = from + insert.length;
    requestAnimationFrame(() => el.setSelectionRange(caret, caret));
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    const start = el.selectionStart;
    const lineStart = el.value.lastIndexOf("\n", start - 1) + 1;
    const line = el.value.slice(lineStart, start);
    const m = LIST.exec(line);
    if (e.key === "Enter" && !e.shiftKey && m && el.selectionEnd === start) {
      e.preventDefault();
      if (!m[3].trim()) {
        replaceRange(el, lineStart, start, "");
        return;
      }
      const marker = /^\d+\./.test(m[2]) ? `${parseInt(m[2], 10) + 1}.` : m[2];
      replaceRange(el, start, start, `\n${m[1]}${marker} `);
    }
    if (e.key === "Tab" && m) {
      e.preventDefault();
      if (e.shiftKey) {
        const cut = Math.min(2, m[1].length);
        if (cut) replaceRange(el, lineStart, lineStart + cut, "");
      } else {
        replaceRange(el, lineStart, lineStart, "  ");
      }
    }
  };

  const showReady = !text && !focused && summaryReady;

  return (
    <>
      {failed && (
        <div className="np-save-error" role="status">
          <AlertCircle size={14} strokeWidth={1.75} /> Couldn't save. Trying
          again…
        </div>
      )}
      {showReady ? (
        <p className="np-soft" onClick={() => ref.current?.focus()}>
          <span>
            Your summary is ready. Click here to start your own notes.
          </span>
          <button
            type="button"
            className="np-btn ghost"
            onClick={(e) => {
              e.stopPropagation();
              onOpenSummary();
            }}
          >
            Read summary
          </button>
        </p>
      ) : null}
      <textarea
        ref={ref}
        className="np-thoughts"
        style={
          showReady
            ? { position: "absolute", opacity: 0, pointerEvents: "none" }
            : undefined
        }
        value={text}
        placeholder="Write your notes. Start a line with - for a list."
        spellCheck
        aria-label="My notes"
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          flush();
        }}
        onChange={(e) => change(e.target.value)}
        onKeyDown={onKeyDown}
      />
    </>
  );
};
