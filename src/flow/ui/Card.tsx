import type { HTMLAttributes } from "react";

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  /** "panel": a raised white sheet. "surface": a flat tinted area. */
  tone?: "panel" | "surface";
}

export const Card = ({
  tone = "panel",
  className = "",
  ...rest
}: CardProps) => (
  <div
    className={`rounded-[14px] ${tone === "panel" ? "bg-[var(--fk-menu-bg)] shadow-[var(--fk-elev-1)]" : "bg-[var(--fk-surface)]"} ${className}`}
    {...rest}
  />
);
