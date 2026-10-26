import type { ReactNode } from "react";
import "./settings.css";

/** Ruled title block and scrolling body used by every settings tab. */
export const SettingsPage = ({
  title,
  intro,
  children,
}: {
  title: ReactNode;
  /** One sentence under the title. */
  intro?: ReactNode;
  children?: ReactNode;
}) => (
  <div className="fk-set fk-scroll h-full px-[40px] pb-[40px] pt-[32px]">
    <header className="border-b border-[var(--fk-border)] pb-[16px]">
      <h1 className="fk-display text-[28px] leading-[38px]">{title}</h1>
      {intro && (
        <p className="mt-[2px] max-w-[560px] text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
          {intro}
        </p>
      )}
    </header>
    {children && (
      <div className="mt-[24px] flex flex-col gap-[32px]">{children}</div>
    )}
  </div>
);
