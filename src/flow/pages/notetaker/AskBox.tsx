import { useEffect, useState } from "react";
import { ArrowUp, History, MessageSquareText } from "lucide-react";
import { isFlowPreview } from "../../dev/preview";
import { Button } from "../../ui";

const PROMPTS = [
  "List the action items from yesterday's calls",
  "Brief me before my next meeting",
  "Which meetings this week mentioned the budget?",
  "Draft a follow-up email for my last call",
  "What changed since last week's review?",
  "Pull out every date someone promised",
];

const otherPrompt = (current: number) => {
  const n = Math.floor(Math.random() * (PROMPTS.length - 1));
  return n >= current ? n + 1 : n;
};

export const AskBox = ({
  onAsk,
  onPastChats,
  placeholder,
  busy = false,
}: {
  onAsk: (question: string) => void;
  onPastChats?: () => void;
  placeholder?: string;
  busy?: boolean;
}) => {
  const [value, setValue] = useState("");
  const [prompts, setPrompts] = useState(() => ({
    now: isFlowPreview ? 0 : Math.floor(Math.random() * PROMPTS.length),
    before: null as number | null,
    swaps: 0,
  }));

  const typing = value !== "";

  useEffect(() => {
    if (placeholder || typing) return;
    const id = window.setInterval(
      () =>
        setPrompts((p) => ({
          now: otherPrompt(p.now),
          before: p.now,
          swaps: p.swaps + 1,
        })),
      6000,
    );
    return () => window.clearInterval(id);
  }, [placeholder, typing]);

  const submit = () => {
    const question = value.trim();
    if (!question || busy) return;
    setValue("");
    onAsk(question);
  };

  const prompt = PROMPTS[prompts.now];

  return (
    <div className="flex h-[48px] items-center gap-[12px] rounded-[14px] bg-[var(--fk-menu-bg)] pl-[16px] pr-[8px] shadow-[var(--fk-elev-2)] transition-shadow duration-[var(--fk-t-quick)] focus-within:shadow-[var(--fk-elev-2),var(--fk-focus)]">
      <MessageSquareText
        size={18}
        strokeWidth={1.75}
        className="shrink-0 text-[var(--fk-accent)]"
      />
      <div className="relative h-full min-w-0 flex-1">
        <input
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            setPrompts((p) => (p.before === null ? p : { ...p, before: null }));
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
            if (e.key === "Tab" && !value && !placeholder) {
              e.preventDefault();
              setValue(prompt);
            }
          }}
          onKeyUp={(e) => e.key === "Escape" && e.currentTarget.blur()}
          placeholder={placeholder}
          aria-label="Ask about your meetings"
          className="h-full w-full bg-transparent text-[15px] text-[var(--fk-text)] outline-none placeholder:text-[var(--fk-text-faint)]"
        />
        {!placeholder && !value && (
          <div
            aria-hidden
            className="pointer-events-none absolute inset-0 flex items-center overflow-hidden whitespace-nowrap text-[15px] text-[var(--fk-text-faint)]"
          >
            {prompts.before !== null && (
              <span
                key={`out-${prompts.swaps}`}
                className="nt-ph-out absolute left-0"
              >
                {PROMPTS[prompts.before]}
              </span>
            )}
            <span
              key={`in-${prompts.swaps}`}
              className={`truncate ${prompts.before === null ? "" : "nt-ph-in"}`}
            >
              {prompt}
            </span>
          </div>
        )}
      </div>
      {typing ? (
        <button
          type="button"
          aria-label="Ask"
          title="Ask"
          disabled={busy}
          onClick={submit}
          className="flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-[8px] bg-[var(--fk-accent)] text-[var(--fk-accent-text)] transition-colors duration-[var(--fk-t-quick)] enabled:hover:bg-[var(--fk-accent-hover)] disabled:opacity-45"
        >
          <ArrowUp size={16} strokeWidth={1.75} />
        </button>
      ) : (
        onPastChats && (
          <Button
            variant="ghost"
            size="sm"
            className="text-[var(--fk-text-muted)]"
            icon={<History size={14} strokeWidth={1.75} />}
            onClick={onPastChats}
          >
            Earlier questions
          </Button>
        )
      )}
    </div>
  );
};
