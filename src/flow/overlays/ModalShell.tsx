import { useEffect, useRef, type CSSProperties, type ReactNode } from "react";
import { useEscape } from "./Popover";
import "./overlays.css";

/** Scrim and centred dialog sheet, shared by every dialog. */
export const ModalShell = ({
  onClose,
  width,
  radius = 20,
  style,
  className = "",
  children,
}: {
  onClose: () => void;
  width: number;
  radius?: number;
  style?: CSSProperties;
  className?: string;
  children: ReactNode;
}) => {
  const ref = useRef<HTMLDivElement>(null);
  useEscape(onClose);
  // Children with autoFocus have already taken focus by the time this runs.
  useEffect(() => {
    if (!ref.current?.contains(document.activeElement)) ref.current?.focus();
  }, []);
  return (
    <div
      className="fk-scrim-in fixed inset-0 z-50 flex items-center justify-center bg-[var(--fk-scrim)]"
      onMouseDown={onClose}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        className={`fk-dialog bg-[var(--fk-menu-bg)] shadow-[var(--fk-elev-3)] outline-none ${className}`}
        style={{ width, borderRadius: radius, ...style }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
};
