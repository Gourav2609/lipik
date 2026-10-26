import { useState, type ReactNode } from "react";

/** Asks for a second click before deleting; leaving the button cancels. */
export const DeleteButton = ({
  onConfirm,
  className = "",
  children,
}: {
  onConfirm: () => void;
  className?: string;
  children: ReactNode;
}) => {
  const [armed, setArmed] = useState(false);
  return (
    <button
      type="button"
      aria-label={armed ? "Click again to delete" : "Delete draft"}
      title={armed ? "Click again to delete" : "Delete draft"}
      onClick={(e) => {
        e.stopPropagation();
        if (armed) onConfirm();
        else setArmed(true);
      }}
      onKeyDown={(e) => e.stopPropagation()}
      onMouseLeave={() => setArmed(false)}
      onBlur={() => setArmed(false)}
      className={`flex h-[30px] items-center justify-center rounded-[8px] transition-colors duration-[var(--fk-t-quick)] ${
        armed
          ? "bg-[var(--fk-live-soft)] px-[10px] text-[13px] font-semibold text-[var(--fk-live)]"
          : "w-[30px] hover:bg-[var(--fk-surface-2)] hover:text-[var(--fk-text)]"
      } ${className}`}
    >
      {armed ? "Delete" : children}
    </button>
  );
};
