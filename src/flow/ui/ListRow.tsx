import type { HTMLAttributes, ReactNode } from "react";

export interface ListRowProps extends HTMLAttributes<HTMLDivElement> {
  actions?: ReactNode;
  /** Keep the actions visible instead of showing them only on hover. */
  showActions?: boolean;
}

/** One ruled line of a list; place rows inside <ListGroup>. */
export const ListRow = ({
  actions,
  showActions = false,
  className = "",
  children,
  ...rest
}: ListRowProps) => (
  <div
    className={`group flex min-h-[48px] items-center gap-[12px] border-b border-[var(--fk-border)] px-[12px] text-[15px] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-surface)] ${className}`}
    {...rest}
  >
    <div className="min-w-0 flex-1">{children}</div>
    {actions && (
      <div
        className={`flex shrink-0 items-center gap-[2px] text-[var(--fk-text-muted)] transition-opacity duration-[var(--fk-t-quick)] ${showActions ? "" : "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"}`}
      >
        {actions}
      </div>
    )}
  </div>
);

/** Ruled list: a hairline above the first row and under every row, no box. */
export const ListGroup = ({
  className = "",
  ...rest
}: HTMLAttributes<HTMLDivElement>) => (
  <div className={`border-t border-[var(--fk-border)] ${className}`} {...rest} />
);

/** Small square icon button used inside row actions. */
export const RowAction = ({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick?: () => void;
  children: ReactNode;
}) => (
  <button
    type="button"
    aria-label={label}
    title={label}
    onClick={onClick}
    className="flex h-[30px] w-[30px] items-center justify-center rounded-[8px] hover:bg-[var(--fk-surface-2)] hover:text-[var(--fk-text)]"
  >
    {children}
  </button>
);
