import type { ButtonHTMLAttributes, ReactNode } from "react";

export interface IconButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  active?: boolean;
  children: ReactNode;
}

export const IconButton = ({
  label,
  active = false,
  className = "",
  children,
  type = "button",
  ...rest
}: IconButtonProps) => (
  <button
    type={type}
    aria-label={label}
    title={label}
    className={`flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-[8px] transition-colors duration-[var(--fk-t-quick)] ${active ? "bg-[var(--fk-active)] text-[var(--fk-accent)]" : "text-[var(--fk-text-muted)] hover:bg-[var(--fk-hover)] hover:text-[var(--fk-text)]"} ${className}`}
    {...rest}
  >
    {children}
  </button>
);
