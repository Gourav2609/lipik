import type { ReactNode } from "react";

export interface PageHeaderProps {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  className?: string;
}

/** Page title block: display title, one muted line, actions on the right, ruled underneath. */
export const PageHeader = ({
  title,
  description,
  actions,
  className = "",
}: PageHeaderProps) => (
  <header className={`fk-page-head ${className}`}>
    <div className="min-w-0 flex-1">
      <h1 className="fk-display text-[28px] leading-[38px]">{title}</h1>
      {description && (
        <p className="mt-[2px] text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
          {description}
        </p>
      )}
    </div>
    {actions && (
      <div className="flex shrink-0 items-center gap-[8px]">{actions}</div>
    )}
  </header>
);
