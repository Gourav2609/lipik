import { forwardRef, type InputHTMLAttributes, type ReactNode } from "react";

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  icon?: ReactNode;
}

export const TextInput = forwardRef<HTMLInputElement, TextInputProps>(
  ({ icon, className = "", ...rest }, ref) => (
    <label
      className={`flex h-[38px] items-center gap-[8px] rounded-[10px] border border-transparent bg-[var(--fk-surface)] px-[12px] text-[var(--fk-text)] transition-[border-color,background-color,box-shadow] duration-[var(--fk-t-quick)] hover:border-[var(--fk-border)] focus-within:border-[var(--fk-accent)] focus-within:bg-[var(--fk-panel)] focus-within:shadow-[var(--fk-focus)] ${className}`}
    >
      {icon && (
        <span className="flex shrink-0 text-[var(--fk-text-faint)]">{icon}</span>
      )}
      <input
        ref={ref}
        className="h-full min-w-0 flex-1 bg-transparent text-[14.5px] outline-none placeholder:text-[var(--fk-text-faint)]"
        {...rest}
      />
    </label>
  ),
);
TextInput.displayName = "TextInput";
