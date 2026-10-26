import { useRef, type KeyboardEvent } from "react";
import { ListGroup, Toggle } from "../../ui";
import { CLEANUP_LEVELS, CLEANUP_SPOKEN } from "./data";
import { useStyleStore } from "./store";

const STEP: Record<string, number> = {
  ArrowDown: 1,
  ArrowRight: 1,
  ArrowUp: -1,
  ArrowLeft: -1,
};

export const AutoCleanup = () => {
  const enabled = useStyleStore((s) => s.autoCleanup);
  const setEnabled = useStyleStore((s) => s.setAutoCleanup);
  const level = useStyleStore((s) => s.cleanupLevel);
  const setLevel = useStyleStore((s) => s.setCleanupLevel);
  const refs = useRef<(HTMLButtonElement | null)[]>([]);

  const onKeyDown = (e: KeyboardEvent, i: number) => {
    const step = STEP[e.key];
    if (!step) return;
    e.preventDefault();
    const next = (i + step + CLEANUP_LEVELS.length) % CLEANUP_LEVELS.length;
    setLevel(CLEANUP_LEVELS[next].id);
    refs.current[next]?.focus();
  };

  return (
    <section aria-labelledby="tone-cleanup">
      <div className="flex items-start gap-[16px]">
        <div className="flex-1">
          <h2
            id="tone-cleanup"
            className="flex items-center text-[16px] font-semibold leading-[22px]"
          >
            Clean up as you speak
            <span className="ml-[8px] rounded-[4px] bg-[var(--fk-inverse-bg)] px-[6px] py-[4px] text-[11px] font-semibold leading-none text-[var(--fk-inverse-text)]">
              Beta
            </span>
          </h2>
          <p className="mt-[2px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
            Takes out filler words and false starts before the text is typed.
          </p>
        </div>
        <Toggle
          checked={enabled}
          onChange={setEnabled}
          label="Clean up as you speak"
        />
      </div>

      <div
        className={`mt-[20px] transition-opacity duration-[var(--fk-t-base)] ${enabled ? "" : "opacity-50"}`}
      >
        <p className="text-[14px] leading-[21px] text-[var(--fk-text-muted)]">
          You say: {CLEANUP_SPOKEN}
        </p>
        <ListGroup
          role="radiogroup"
          aria-label="Clean-up level"
          aria-disabled={!enabled}
          className="mt-[16px]"
        >
          {CLEANUP_LEVELS.map((l, i) => {
            const active = l.id === level;
            return (
              <button
                key={l.id}
                ref={(el) => {
                  refs.current[i] = el;
                }}
                type="button"
                role="radio"
                aria-checked={active}
                tabIndex={active ? 0 : -1}
                disabled={!enabled}
                onClick={() => setLevel(l.id)}
                onKeyDown={(e) => onKeyDown(e, i)}
                className="grid w-full grid-cols-[20px_168px_minmax(0,1fr)] items-start gap-[16px] border-b border-[var(--fk-border)] px-[12px] py-[16px] text-left transition-colors duration-[var(--fk-t-quick)] enabled:hover:bg-[var(--fk-surface)]"
              >
                <span
                  className={`mt-[2px] flex h-[18px] w-[18px] items-center justify-center rounded-full border transition-colors duration-[var(--fk-t-base)] ${active ? "border-[var(--fk-accent)]" : "border-[var(--fk-border-strong)]"}`}
                >
                  {active && (
                    <span className="h-[8px] w-[8px] rounded-full bg-[var(--fk-accent)]" />
                  )}
                </span>
                <span>
                  <span className="block text-[15px] font-semibold leading-[22px]">
                    {l.name}
                  </span>
                  <span className="mt-[2px] block text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
                    {l.description}
                  </span>
                </span>
                <span
                  className={`border-l-2 pl-[12px] text-[14px] leading-[21px] transition-colors duration-[var(--fk-t-base)] ${active ? "border-[var(--fk-accent)]" : "border-[var(--fk-border)]"}`}
                >
                  {l.result}
                </span>
              </button>
            );
          })}
        </ListGroup>
      </div>
    </section>
  );
};
