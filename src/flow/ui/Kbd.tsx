import type { ReactNode } from "react";

/** A key cap. `lg` is for keys set inside a display headline. */
export const Kbd = ({
  children,
  size = "md",
  className = "",
}: {
  children: ReactNode;
  size?: "md" | "lg";
  className?: string;
}) => (
  <kbd
    className={`inline-flex items-center justify-center border border-b-2 border-[var(--fk-border-strong)] bg-[var(--fk-menu-bg)] font-[inherit] font-semibold leading-none text-[var(--fk-text)] ${size === "lg" ? "h-[30px] min-w-[30px] rounded-[8px] px-[8px] text-[15px]" : "h-[22px] min-w-[22px] rounded-[6px] px-[6px] text-[12.5px]"} ${className}`}
  >
    {children}
  </kbd>
);
