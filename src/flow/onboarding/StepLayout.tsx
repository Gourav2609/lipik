import type { ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { Button } from "../ui";

export interface StepProps {
  next: () => void;
  back?: () => void;
}

/** One centred column: title, one line, optional lead, the preview card, controls, buttons. */
export const StepLayout = ({
  eyebrow,
  title,
  body,
  lead,
  visual,
  children,
  actions,
  back,
}: {
  eyebrow?: ReactNode;
  title: ReactNode;
  body?: ReactNode;
  lead?: ReactNode;
  visual?: ReactNode;
  children?: ReactNode;
  actions?: ReactNode;
  back?: () => void;
}) => (
  <div className="min-h-0 flex-1 overflow-y-auto">
    <div className="mx-auto flex min-h-full w-full max-w-[568px] flex-col justify-center px-[24px] pb-[48px] pt-[8px]">
      <div className="fk-ob-enter">
        {eyebrow && <div className="fk-eyebrow mb-[8px]">{eyebrow}</div>}
        <h1 className="fk-display text-[34px] leading-[42px] text-[var(--fk-text)]">
          {title}
        </h1>
        {body && (
          <p className="mt-[8px] text-[15px] leading-[24px] text-[var(--fk-text-muted)]">
            {body}
          </p>
        )}
        {lead && <div className="mt-[24px]">{lead}</div>}
      </div>
      {visual && <div className="fk-ob-enter-late mt-[24px]">{visual}</div>}
      {children && <div className="fk-ob-enter-late mt-[20px]">{children}</div>}
      <div className="mt-[32px] flex items-center gap-[8px]">
        {back && (
          <Button variant="secondary" onClick={back} aria-label="Back" className="w-[36px] !px-0">
            <ArrowLeft size={16} strokeWidth={1.75} />
          </Button>
        )}
        <div className="ml-auto flex items-center gap-[8px]">{actions}</div>
      </div>
    </div>
  </div>
);
