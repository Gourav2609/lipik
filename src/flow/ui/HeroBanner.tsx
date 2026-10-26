import type { CSSProperties, ReactNode } from "react";
import { X } from "lucide-react";

export interface HeroBannerProps {
  title: ReactNode;
  subtitle?: ReactNode;
  /** Buttons under the subtitle. */
  actions?: ReactNode;
  /** Optional drawing in the right-hand slot (line art, not photos). */
  art?: ReactNode;
  /** Overrides the tinted ink fill. */
  background?: string;
  onClose?: () => void;
  className?: string;
  bodyClassName?: string;
  titleClassName?: string;
  style?: CSSProperties;
}

/** The margin note: a tinted card with an ink rule down its left edge. */
export const HeroBanner = ({
  title,
  subtitle,
  actions,
  art,
  background = "var(--fk-accent-soft)",
  onClose,
  className = "",
  bodyClassName = "py-[22px] pl-[28px] pr-[24px]",
  titleClassName = "text-[22px] leading-[30px]",
  style,
}: HeroBannerProps) => (
  <div
    className={`relative overflow-hidden rounded-[14px] text-[var(--fk-text)] before:absolute before:inset-y-0 before:left-0 before:w-[3px] before:bg-[var(--fk-accent)] ${className}`}
    style={{ background, ...style }}
  >
    {art && (
      <div className="pointer-events-none absolute inset-y-0 right-0 w-[40%]">
        {art}
      </div>
    )}
    <div className={`relative max-w-[560px] ${bodyClassName}`}>
      <h2 className={`fk-display ${titleClassName}`}>{title}</h2>
      {subtitle && (
        <p className="mt-[6px] text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
          {subtitle}
        </p>
      )}
      {actions && (
        <div className="mt-[16px] flex flex-wrap gap-[8px]">{actions}</div>
      )}
    </div>
    {onClose && (
      <button
        type="button"
        aria-label="Dismiss"
        onClick={onClose}
        className="absolute right-[12px] top-[12px] flex h-[26px] w-[26px] items-center justify-center rounded-[8px] text-[var(--fk-text-muted)] hover:bg-[var(--fk-active)] hover:text-[var(--fk-text)]"
      >
        <X size={15} strokeWidth={2} />
      </button>
    )}
  </div>
);

/** Button for a margin note: `strong` is the ink-filled main action. */
export const HeroButton = ({
  children,
  onClick,
  strong = false,
}: {
  children: ReactNode;
  onClick?: () => void;
  strong?: boolean;
}) => (
  <button
    type="button"
    onClick={onClick}
    className={`h-[32px] rounded-[8px] px-[12px] text-[14px] font-semibold leading-none transition-colors duration-[var(--fk-t-quick)] ${strong ? "bg-[var(--fk-accent)] text-[var(--fk-accent-text)] hover:bg-[var(--fk-accent-hover)]" : "bg-[var(--fk-menu-bg)] text-[var(--fk-text)] shadow-[var(--fk-elev-1)] hover:bg-[var(--fk-panel)]"}`}
  >
    {children}
  </button>
);
