import "../flow.css";
import "./onboarding.css";
import { useEffect, useState } from "react";
import { APP_NAME } from "../brand";
import { Button, LipikMark } from "../ui";
import { isFlowPreview, LOOK } from "../dev/preview";
import { FakeTrafficLights } from "../TopBar";
import { useResolvedTheme } from "../useResolvedTheme";
import { startDefaultModelSetup } from "./defaultModel";
import { STEPS, type StepId } from "./steps";

const SEEN_KEY = "lipik.onboarding.done";

export const hasSeenOnboarding = () => {
  try {
    return localStorage.getItem(SEEN_KEY) === "1";
  } catch {
    return false;
  }
};

const markSeen = () => {
  try {
    localStorage.setItem(SEEN_KEY, "1");
  } catch {
    /* private mode: onboarding just shows again next launch */
  }
};

export const FlowOnboarding = ({
  onDone,
  initialStep,
}: {
  onDone: () => void;
  initialStep?: StepId;
}) => {
  const theme = useResolvedTheme();
  const [index, setIndex] = useState(() =>
    Math.max(0, STEPS.findIndex((s) => s.id === initialStep)),
  );
  const step = STEPS[index];
  const Body = step.Component;

  useEffect(() => {
    startDefaultModelSetup();
  }, []);

  const next = () => {
    if (index < STEPS.length - 1) setIndex(index + 1);
    else {
      if (!isFlowPreview) markSeen();
      onDone();
    }
  };
  const back = index > 0 ? () => setIndex(index - 1) : undefined;

  return (
    <div
      className="fk relative flex h-screen w-screen select-none flex-col overflow-hidden"
      data-fk-theme={theme}
      data-look={LOOK}
    >
      <header data-tauri-drag-region className="relative h-[52px] shrink-0">
        <div
          data-tauri-drag-region
          className="absolute left-[96px] top-1/2 flex -translate-y-1/2 items-center gap-[8px]"
        >
          <LipikMark size={20} className="shrink-0 text-[var(--fk-accent)]" />
          <span className="fk-display text-[18px] font-semibold leading-none">{APP_NAME}</span>
        </div>
        <div
          className="absolute left-1/2 top-1/2 flex -translate-x-1/2 -translate-y-1/2 gap-[6px]"
          role="progressbar"
          aria-valuemin={1}
          aria-valuemax={STEPS.length}
          aria-valuenow={index + 1}
          aria-label={`Step ${index + 1} of ${STEPS.length}`}
        >
          {STEPS.map((s, i) => (
            <span
              key={s.id}
              className={`h-[3px] w-[16px] rounded-[2px] transition-colors duration-[var(--fk-t-base)] ${i === index ? "bg-[var(--fk-accent)]" : "bg-[var(--fk-border-strong)]"}`}
            />
          ))}
        </div>
        {step.skippable && (
          <Button
            variant="ghost"
            size="sm"
            className="absolute right-[12px] top-[11px] !text-[var(--fk-text-muted)]"
            onClick={next}
          >
            Skip
          </Button>
        )}
      </header>
      <Body key={step.id} next={next} back={back} />
      {isFlowPreview && <FakeTrafficLights />}
    </div>
  );
};
