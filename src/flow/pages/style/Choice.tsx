import { useRef, type KeyboardEvent } from "react";

const STEP: Record<string, number> = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };

/** Segmented radio group: a tinted track with the chosen option as a raised sheet. */
export const Choice = <T extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { id: T; name: string; hint?: string }[];
  value: T;
  onChange: (id: T) => void;
}) => {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const selected = Math.max(0, options.findIndex((o) => o.id === value));

  const onKeyDown = (e: KeyboardEvent, i: number) => {
    const step = STEP[e.key];
    if (!step) return;
    e.preventDefault();
    const next = (i + step + options.length) % options.length;
    onChange(options[next].id);
    refs.current[next]?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex h-[32px] items-stretch gap-[2px] rounded-[10px] bg-[var(--fk-surface-2)] p-[3px]"
    >
      {options.map((o, i) => {
        const active = i === selected;
        return (
          <button
            key={o.id}
            ref={(el) => {
              refs.current[i] = el;
            }}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={active ? 0 : -1}
            title={o.hint}
            onClick={() => onChange(o.id)}
            onKeyDown={(e) => onKeyDown(e, i)}
            className={`flex items-center rounded-[8px] px-[10px] text-[13.5px] font-medium leading-none transition-colors duration-[var(--fk-t-quick)] ${active ? "bg-[var(--fk-menu-bg)] text-[var(--fk-text)] shadow-[var(--fk-elev-1)]" : "text-[var(--fk-text-muted)] hover:text-[var(--fk-text)]"}`}
          >
            {o.name}
          </button>
        );
      })}
    </div>
  );
};
