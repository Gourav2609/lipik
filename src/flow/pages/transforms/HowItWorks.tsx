import type { ReactNode } from "react";
import { X } from "lucide-react";
import { ModalShell } from "../../overlays/ModalShell";
import { Button, IconButton } from "../../ui";
import { OptKeys } from "./Keycap";
import { APP_NAME } from "@/flow/brand";

const STEPS: ReactNode[] = [
  `Select text in any app, or dictate it with ${APP_NAME}.`,
  <>
    Press <OptKeys keyLabel="1" /> or the number of another rewrite.
  </>,
  `${APP_NAME} rewrites the text with that prompt and puts it back in place.`,
  <>
    Press <OptKeys keyLabel="o" /> to see what changed.
  </>,
];

export const HowItWorks = ({ onClose }: { onClose: () => void }) => (
  <ModalShell onClose={onClose} width={480} className="p-[24px]">
    <div className="flex items-center justify-between">
      <h2 className="fk-display text-[22px] leading-[30px]">
        How rewrite works
      </h2>
      <IconButton label="Close" onClick={onClose}>
        <X size={18} strokeWidth={1.75} />
      </IconButton>
    </div>
    <ol className="mt-[16px] border-t border-[var(--fk-border)]">
      {STEPS.map((step, i) => (
        <li
          key={i}
          className="flex gap-[12px] border-b border-[var(--fk-border)] py-[12px] text-[15px] leading-[24px]"
        >
          <span className="fk-num w-[16px] shrink-0 font-semibold text-[var(--fk-accent)]">
            {i + 1}
          </span>
          <span>{step}</span>
        </li>
      ))}
    </ol>
    <p className="mt-[16px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
      Turn on Rewrite every dictation to run one rewrite on everything you
      dictate.
    </p>
    <div className="mt-[24px] flex justify-end">
      <Button onClick={onClose}>Done</Button>
    </div>
  </ModalShell>
);
