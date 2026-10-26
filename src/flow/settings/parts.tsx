import { useCallback, useSyncExternalStore, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";

/** A group of settings with an optional title above it. */
export const Section = ({
  title,
  children,
}: {
  title?: ReactNode;
  children: ReactNode;
}) => (
  <section>
    {title && (
      <h2 className="mb-[12px] text-[16px] font-semibold leading-[22px]">
        {title}
      </h2>
    )}
    {children}
  </section>
);

/** Flat tinted card holding ruled rows. */
export const SettingsCard = ({
  className = "",
  children,
}: {
  className?: string;
  children: ReactNode;
}) => (
  <div
    className={`rounded-[var(--fk-r-lg)] bg-[var(--fk-surface)] px-[20px] ${className}`}
  >
    {children}
  </div>
);

/** One setting: title, optional description, control on the right. */
export const SettingRow = ({
  title,
  description,
  control,
  className = "py-[16px]",
}: {
  title: ReactNode;
  description?: ReactNode;
  control?: ReactNode;
  className?: string;
}) => (
  <div
    className={`flex min-h-[64px] items-center justify-between gap-[24px] border-b border-[var(--fk-border)] last:border-b-0 ${className}`}
  >
    <div className="min-w-0 max-w-[420px]">
      <div className="text-[15px] font-medium leading-[22px]">{title}</div>
      {description && (
        <div className="mt-[2px] text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
          {description}
        </div>
      )}
    </div>
    {control && <div className="flex shrink-0 items-center">{control}</div>}
  </div>
);

/** Classes shared by the text fields and selects that sit inside a settings card. */
export const FIELD_CLASS =
  "h-[38px] rounded-[var(--fk-r-md)] border border-[var(--fk-border)] bg-[var(--fk-menu-bg)] text-[14.5px] text-[var(--fk-text)] outline-none transition-[border-color,box-shadow] duration-[var(--fk-t-quick)] placeholder:text-[var(--fk-text-faint)] enabled:hover:border-[var(--fk-border-strong)] focus:border-[var(--fk-accent)] focus:shadow-[var(--fk-focus)] disabled:opacity-45";

export interface SelectOption {
  value: string;
  label: string;
}

/** Native select with a chevron on the right. */
export const Select = ({
  value,
  options,
  onChange,
  disabled,
  label,
  width = 208,
}: {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  label?: string;
  width?: number;
}) => (
  <div className="relative" style={{ width }}>
    <select
      aria-label={label}
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      className={`${FIELD_CLASS} w-full appearance-none truncate pl-[12px] pr-[36px]`}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
    <ChevronDown
      size={16}
      strokeWidth={1.75}
      className="pointer-events-none absolute right-[12px] top-1/2 -translate-y-1/2 text-[var(--fk-text-muted)]"
    />
  </div>
);

const listeners = new Map<string, Set<() => void>>();
const memory = new Map<string, string | null>();

/** Shared store over localStorage, so every reader of a key sees each write. */
export const localStore = {
  get: (key: string): string | null => {
    if (memory.has(key)) return memory.get(key) ?? null;
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set: (key: string, raw: string | null) => {
    memory.set(key, raw);
    try {
      if (raw === null) localStorage.removeItem(key);
      else localStorage.setItem(key, raw);
    } catch {
      // storage blocked: the in-memory copy serves this session
    }
    listeners.get(key)?.forEach((l) => l());
  },
  subscribe: (key: string, listener: () => void) => {
    const set = listeners.get(key) ?? new Set();
    set.add(listener);
    listeners.set(key, set);
    return () => set.delete(listener);
  },
};

/** Raw string at `key`, re-rendering on writes from any component. */
export const useStoredRaw = (key: string) =>
  useSyncExternalStore(
    useCallback((l: () => void) => localStore.subscribe(key, l), [key]),
    () => localStore.get(key),
  );

const parse = <T,>(raw: string | null, fallback: T): T => {
  if (raw === null) return fallback;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
};

/** Preference kept only in this browser profile (no backend setting yet). */
export const useLocalSetting = <T,>(name: string, fallback: T) => {
  const key = `lipik.settings.${name}`;
  const value = parse(useStoredRaw(key), fallback);
  const update = useCallback(
    (next: T) => localStore.set(key, JSON.stringify(next)),
    [key],
  );
  return [value, update] as const;
};
