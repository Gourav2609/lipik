import { useEffect } from "react";
import type { OverlayProps } from "./types";
import "./overlays.css";

const SHOWN_MS = 1800;

const STROKES = [
  { d: "M6.5 6.4v4.6", delay: 420 },
  { d: "M10.5 6.4v11.6", delay: 500 },
  { d: "M14.5 6.4v7.4", delay: 580 },
  { d: "M18.5 6.4v3", delay: 660 },
];

/** The Lipik mark writes itself once in the middle of the window, then fades. */
export default function Confetti({ onClose }: OverlayProps) {
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      onClose();
      return;
    }
    const id = window.setTimeout(onClose, SHOWN_MS);
    return () => window.clearTimeout(id);
  }, [onClose]);

  return (
    <div
      aria-hidden
      className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center"
    >
      <div className="fk-ink-moment flex flex-col items-center gap-[8px] rounded-[var(--fk-r-xl)] bg-[var(--fk-menu-bg)] px-[32px] pb-[20px] pt-[28px] shadow-[var(--fk-elev-2)]">
        <svg
          width={88}
          height={88}
          viewBox="0 0 24 24"
          fill="none"
          stroke="var(--fk-accent)"
          strokeLinecap="round"
        >
          <path
            className="fk-ink-stroke"
            d="M3 5.4h18"
            strokeWidth={2.6}
            pathLength={1}
          />
          {STROKES.map(({ d, delay }) => (
            <path
              key={d}
              className="fk-ink-stroke"
              d={d}
              strokeWidth={2.3}
              pathLength={1}
              style={{ animationDelay: `${delay}ms` }}
            />
          ))}
        </svg>
        <span className="fk-display text-[18px] leading-[26px] text-[var(--fk-text-muted)]">
          लिपिक
        </span>
      </div>
    </div>
  );
}
