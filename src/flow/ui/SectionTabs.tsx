import type { ReactNode } from "react";

export interface SectionTab<T extends string> {
  id: T;
  label: ReactNode;
}

export interface SectionTabsProps<T extends string> {
  tabs: SectionTab<T>[];
  value: T;
  onChange: (id: T) => void;
  actions?: ReactNode;
  className?: string;
}

export const SectionTabs = <T extends string>({
  tabs,
  value,
  onChange,
  actions,
  className = "",
}: SectionTabsProps<T>) => (
  <div className={`flex items-center gap-[12px] ${className}`}>
    <div
      role="tablist"
      className="inline-flex h-[36px] items-stretch gap-[2px] rounded-[10px] bg-[var(--fk-surface-2)] p-[3px]"
    >
      {tabs.map((tab) => {
        const active = tab.id === value;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(tab.id)}
            className={`flex items-center rounded-[8px] px-[12px] text-[14px] font-medium leading-none transition-colors duration-[var(--fk-t-quick)] ${active ? "bg-[var(--fk-menu-bg)] text-[var(--fk-text)] shadow-[var(--fk-elev-1)]" : "text-[var(--fk-text-muted)] hover:text-[var(--fk-text)]"}`}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
    {actions && (
      <div className="ml-auto flex items-center gap-[4px]">{actions}</div>
    )}
  </div>
);
