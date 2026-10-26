import type { ReactNode } from "react";
import { Globe, Mic, NotebookPen } from "lucide-react";
import { APP_NAME, BRAND } from "../brand";
import { Card, LipikMark } from "../ui";

const PILL_RIM = "rgba(141, 156, 255, 0.28)";
const PILL_MUTED = "#9aa3c7";
const PILL_SHADOW = "0 8px 24px -6px rgba(8, 10, 24, 0.45)";

const HANG = [5, 9, 14, 8, 16, 11, 6, 12, 15, 7, 10, 13, 4];

export type PillState = "idle" | "hover" | "recording" | "done";

/** A still copy of the pill, drawn to the pill spec in DESIGN.md §8. */
export const PillPreview = ({
  state,
  hotkey = "fn",
}: {
  state: PillState;
  hotkey?: string;
}) => {
  const line = state === "idle" || state === "done";
  const tile =
    state === "hover" ? { minWidth: 184, height: 34 } : { width: 152, height: 36 };
  return (
    <div
      className="fk-ob-pill relative flex shrink-0 items-center overflow-hidden"
      style={
        line
          ? {
              width: 48,
              height: 6,
              borderRadius: 3,
              background: state === "done" ? BRAND.inkLight : "rgba(18, 17, 15, 0.55)",
              boxShadow: "inset 0 0 0 1px rgba(238, 240, 255, 0.5)",
            }
          : {
              ...tile,
              borderRadius: 10,
              background: BRAND.night,
              color: BRAND.paper,
              boxShadow: `inset 0 0 0 1px ${PILL_RIM}, ${PILL_SHADOW}`,
            }
      }
    >
      {state === "recording" && (
        <>
          <span
            className="fk-ob-live absolute left-[9px] top-1/2 h-[6px] w-[6px] -translate-y-1/2 rounded-full"
            style={{ background: BRAND.live }}
          />
          <span
            className="absolute left-[24px] top-[11px] h-px w-[64px]"
            style={{ background: PILL_MUTED, opacity: 0.5 }}
          />
          {HANG.map((h, i) => (
            <span
              key={i}
              className="fk-ob-hang absolute top-[11px] w-[2px] rounded-b-[1px]"
              style={{
                left: 24 + i * 5,
                height: h,
                background: BRAND.paper,
                animationDuration: `${0.7 + (i % 4) * 0.18}s`,
                animationDelay: `${-(i % 5) * 0.21}s`,
              }}
            />
          ))}
          <span
            className="fk-num absolute right-[12px] top-1/2 -translate-y-1/2 text-[12px] leading-none"
            style={{ color: PILL_MUTED }}
          >
            0:04
          </span>
        </>
      )}
      {state === "hover" && (
        <div className="flex w-full items-center gap-[8px] px-[4px]">
          <span
            className="flex h-[26px] w-[26px] shrink-0 items-center justify-center rounded-[8px]"
            style={{ background: "rgba(238, 240, 255, 0.08)" }}
          >
            <Mic size={14} strokeWidth={1.75} />
          </span>
          <span className="flex flex-1 items-center justify-center gap-[4px] whitespace-nowrap text-[13px] font-medium leading-none">
            Hold
            <span
              className="rounded-[4px] px-[4px] py-[2px] text-[11px] font-semibold leading-none"
              style={{ boxShadow: `inset 0 0 0 1px ${PILL_RIM}`, color: PILL_MUTED }}
            >
              {hotkey}
            </span>
            to write
          </span>
          <span className="h-[18px] w-px shrink-0" style={{ background: PILL_RIM }} />
          <span className="flex h-[26px] w-[26px] shrink-0 items-center justify-center rounded-[8px]">
            <NotebookPen size={14} strokeWidth={1.75} />
          </span>
        </div>
      )}
    </div>
  );
};

export const Preview = ({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) => (
  <Card
    tone="panel"
    className={`relative flex h-[184px] items-center justify-center overflow-hidden ${className}`}
  >
    {children}
  </Card>
);

const RuledLine = ({ children }: { children?: ReactNode }) => (
  <div className="flex h-[32px] items-end border-b border-[var(--fk-border)] pb-[6px] text-[15px] leading-[22px] text-[var(--fk-text)]">
    {children}
  </div>
);

export const WritingVisual = ({
  pill,
  hotkey,
}: {
  pill: PillState;
  hotkey?: string;
}) => (
  <Preview className="flex-col !justify-between px-[32px] pb-[20px] pt-[16px]">
    <div className="w-full">
      <RuledLine>Thanks for the notes, Sam. I'll read them tonight</RuledLine>
      <RuledLine>
        and reply by tomorrow morning.
        <span className="fk-ob-caret ml-[2px] h-[18px] w-[2px] self-center bg-[var(--fk-accent)]" />
      </RuledLine>
    </div>
    <PillPreview state={pill} hotkey={hotkey} />
  </Preview>
);

export const MeterVisual = ({ level, live }: { level: number; live: boolean }) => {
  const count = 41;
  return (
    <Preview className="flex-col gap-[16px]">
      <div className="flex items-center gap-[8px] text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
        <span
          className={`h-[6px] w-[6px] rounded-full ${live ? "fk-ob-live bg-[var(--fk-live)]" : "bg-[var(--fk-border-strong)]"}`}
        />
        {live ? "Listening" : "Microphone off"}
      </div>
      <div className="relative h-[72px] w-[360px]">
        <span className="absolute inset-x-0 top-0 h-[2px] rounded-full bg-[var(--fk-accent)]" />
        {Array.from({ length: count }, (_, i) => {
          const centre = 1 - Math.abs(i - (count - 1) / 2) / (count / 2);
          const wobble = 0.55 + 0.45 * Math.sin(i * 1.7 + level * 9);
          const h = Math.max(4, Math.min(64, 4 + level * 80 * centre * wobble));
          return (
            <span
              key={i}
              className="absolute top-[1px] w-[3px] rounded-b-full bg-[var(--fk-accent)] transition-[height] duration-[var(--fk-t-quick)]"
              style={{ left: i * 9 - 1.5, height: h, opacity: 0.4 + 0.6 * centre }}
            />
          );
        })}
      </div>
    </Preview>
  );
};

const KeyCap = ({
  label,
  sub,
  w = 56,
  hot = false,
}: {
  label?: ReactNode;
  sub?: ReactNode;
  w?: number;
  hot?: boolean;
}) => (
  <div
    className={`flex h-[52px] shrink-0 flex-col justify-between rounded-[8px] border border-b-2 px-[8px] py-[6px] text-[13px] leading-none ${hot ? "fk-ob-key-hot border-[var(--fk-accent-hover)] bg-[var(--fk-accent)] text-[var(--fk-accent-text)]" : "border-[var(--fk-border-strong)] bg-[var(--fk-menu-bg)] text-[var(--fk-text-muted)]"}`}
    style={{ width: w }}
  >
    <span className="font-semibold">{label}</span>
    {sub && <span className="text-[11px]">{sub}</span>}
  </div>
);

export const KeyboardVisual = ({
  binding,
  hotkey,
}: {
  binding: string;
  hotkey: string;
}) => {
  const parts = binding.toLowerCase().split("+");
  const on = (...names: string[]) => names.some((n) => parts.includes(n));
  return (
    <Preview className="flex-col gap-[24px]">
      <div className="flex gap-[4px]">
        <KeyCap
          label="fn"
          sub={<Globe size={11} strokeWidth={1.75} />}
          hot={on("fn", "function")}
        />
        <KeyCap label="⌃" sub="control" hot={on("ctrl", "control")} />
        <KeyCap label="⌥" sub="option" hot={on("option", "alt")} />
        <KeyCap label="⌘" sub="command" w={72} hot={on("command", "cmd", "meta", "super")} />
        <KeyCap w={148} hot={on("space")} />
      </div>
      <PillPreview state="recording" hotkey={hotkey} />
    </Preview>
  );
};

export const MarkVisual = () => (
  <Preview className="flex-col gap-[12px]">
    <LipikMark size={64} tile />
    <div className="flex items-baseline gap-[8px]">
      <span className="fk-display text-[22px] font-semibold leading-[30px]">{APP_NAME}</span>
      <span lang="hi" className="fk-display text-[16px] leading-[30px] text-[var(--fk-text-muted)]">
        लिपिक
      </span>
    </div>
  </Preview>
);
