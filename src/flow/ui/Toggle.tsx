export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
}

export const Toggle = ({ checked, onChange, disabled, label }: ToggleProps) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    disabled={disabled}
    onClick={() => onChange(!checked)}
    className={`relative h-[20px] w-[36px] shrink-0 rounded-full transition-colors duration-[var(--fk-t-base)] disabled:opacity-45 ${checked ? "bg-[var(--fk-toggle-on)]" : "bg-[var(--fk-toggle-off)]"}`}
  >
    <span
      className={`absolute left-[2px] top-[2px] h-[16px] w-[16px] rounded-full shadow-[0_1px_2px_rgba(0,0,0,0.18)] transition-transform duration-[var(--fk-t-base)] ease-[var(--fk-ease)] ${checked ? "translate-x-[16px] bg-[var(--fk-toggle-knob-on)]" : "bg-[var(--fk-toggle-knob-off)]"}`}
    />
  </button>
);
