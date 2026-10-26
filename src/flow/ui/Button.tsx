import type { ButtonHTMLAttributes, ReactNode } from "react";

export type ButtonVariant = "primary" | "secondary" | "accent" | "ghost" | "danger";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "md" | "sm";
  icon?: ReactNode;
}

const VARIANTS: Record<ButtonVariant, string> = {
  primary:
    "bg-[var(--fk-accent)] text-[var(--fk-accent-text)] enabled:hover:bg-[var(--fk-accent-hover)]",
  secondary:
    "bg-[var(--fk-btn-secondary)] text-[var(--fk-text)] enabled:hover:bg-[var(--fk-btn-secondary-hover)]",
  accent:
    "bg-[var(--fk-accent-soft)] text-[var(--fk-accent)] enabled:hover:bg-[var(--fk-active)]",
  ghost: "text-[var(--fk-text)] enabled:hover:bg-[var(--fk-hover)]",
  danger:
    "bg-[var(--fk-live-soft)] text-[var(--fk-live)] enabled:hover:brightness-95",
};

const SIZES = {
  md: "h-[36px] px-[14px] rounded-[10px] text-[14.5px] gap-[8px]",
  sm: "h-[30px] px-[11px] rounded-[8px] text-[13.5px] gap-[6px]",
};

export const Button = ({
  variant = "primary",
  size = "md",
  icon,
  className = "",
  children,
  type = "button",
  ...rest
}: ButtonProps) => (
  <button
    type={type}
    className={`inline-flex items-center justify-center whitespace-nowrap font-semibold leading-none transition-[background-color,filter] duration-[var(--fk-t-quick)] disabled:opacity-45 ${VARIANTS[variant]} ${SIZES[size]} ${className}`}
    {...rest}
  >
    {icon}
    {children}
  </button>
);
