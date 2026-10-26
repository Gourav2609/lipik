import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { LipikMark } from "@/flow/ui/LipikMark";
import { CheckGlyph, XGlyph } from "./parts";
import { POPUP_EXIT, POPUP_EXIT_FADE, POPUP_H, POPUP_W, t } from "./curves";
import { commands } from "@/bindings";
import type { Place } from "../Pill";

// "Select a textbox first" popup, measured in S2 19.95-25.45 (ANIMATIONS.md §3 rows 6-10).
export const POPUP = {
  w: 442,
  minH: 169,
  edge: 15,
  seedW: 29,
  seedH: 2,
  lift: 1.5,
};
const TEXT_W = POPUP.w - 2 - 16.5 - 24;
/** Outer height around the transcript: 55.5 above, gap + Copy (31) + 20.5 below, 1 pt border. */
const CHROME_H = 129;
const MAX_TEXT_H = 200;
const REVEAL_AT = 360;
/** Not recorded: the ring never ran out in S2. */
const RING_MS = 10000;
const LEAVE_CLOSE_MS = 750;
/** Exit (S2 25.233): content fades with the surface held, then the surface shrinks and fades. */
const SHRINK_AT = 100;
const GONE_AT = SHRINK_AT + POPUP_EXIT.ms;
const RING_R = 12.25;
const RING_C = 2 * Math.PI * RING_R;

type Stage = "seed" | "grow" | "shown" | "exit";

type Props = {
  place: Place;
  text: string;
  hover: boolean;
  onHeight: (h: number) => void;
  onDone: () => void;
};

export const Popup: React.FC<Props> = ({
  place,
  text,
  hover,
  onHeight,
  onDone,
}) => {
  const { t: tr } = useTranslation();
  const [stage, setStage] = useState<Stage>("seed");
  const [copied, setCopied] = useState(false);
  const [height, setHeight] = useState(POPUP.minH);
  const measure = useRef<HTMLDivElement>(null);
  const timers = useRef<number[]>([]);
  const leave = useRef<number | undefined>(undefined);
  const wasHover = useRef(false);
  const exiting = useRef(false);
  const later = (ms: number, fn: () => void) =>
    timers.current.push(window.setTimeout(fn, ms));

  useLayoutEffect(() => {
    const textH = Math.min(MAX_TEXT_H, measure.current?.offsetHeight ?? 40);
    const h = Math.max(POPUP.minH, textH + CHROME_H);
    setHeight(h);
    onHeight(h);
  }, [text, onHeight]);

  useEffect(() => {
    const raf = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        setStage("grow");
        later(REVEAL_AT, () => setStage((s) => (s === "grow" ? "shown" : s)));
      }),
    );
    return () => {
      cancelAnimationFrame(raf);
      timers.current.forEach(clearTimeout);
      clearTimeout(leave.current);
    };
  }, []);

  const exit = () => {
    if (exiting.current) return;
    exiting.current = true;
    setStage("exit");
    later(GONE_AT, onDone);
  };

  useEffect(() => {
    if (hover) {
      wasHover.current = true;
      clearTimeout(leave.current);
    } else if (wasHover.current) {
      leave.current = window.setTimeout(exit, LEAVE_CLOSE_MS);
    }
  }, [hover]);

  const open = stage === "grow" || stage === "shown";
  const w = open ? POPUP.w : POPUP.seedW;
  const h = open ? height : POPUP.seedH;
  const surface: React.CSSProperties = {
    ...place(w, h, POPUP.edge, POPUP.lift),
    transition:
      stage === "exit"
        ? [
            t("width", POPUP_EXIT, SHRINK_AT),
            t("left", POPUP_EXIT, SHRINK_AT),
            t("height", POPUP_EXIT, SHRINK_AT),
            t("top", POPUP_EXIT, SHRINK_AT),
            t("opacity", POPUP_EXIT_FADE, SHRINK_AT),
          ].join(", ")
        : [
            t("width", POPUP_W),
            t("left", POPUP_W),
            t("height", POPUP_H),
            t("top", POPUP_H),
          ].join(", "),
  };

  return (
    <>
      <div
        ref={measure}
        className="popup-text popup-measure"
        style={{ width: TEXT_W }}
      >
        {text}
      </div>
      <div className={`pill-popup ${stage}`} style={surface}>
        <div className="popup-content">
          <LipikMark size={14} className="popup-logo" />
          <span className="popup-hint">
            {tr("pill.selectTextbox", "Select a textbox first, then dictate")}
          </span>
          <button className="popup-close" onClick={onDone}>
            <svg className="ring" width="26" height="26" viewBox="0 0 26 26">
              <circle
                cx="13"
                cy="13"
                r={RING_R}
                className="ring-track"
                transform="rotate(-90 13 13)"
              />
              {stage !== "seed" && stage !== "grow" && (
                <circle
                  cx="13"
                  cy="13"
                  r={RING_R}
                  className="ring-left"
                  transform="rotate(-90 13 13)"
                  strokeDasharray={`${RING_C} ${RING_C}`}
                  style={
                    {
                      "--ring": `${RING_C}px`,
                      animation: `pill-ring ${RING_MS}ms linear forwards`,
                      animationPlayState:
                        hover || stage === "exit" ? "paused" : "running",
                    } as React.CSSProperties
                  }
                  onAnimationEnd={exit}
                />
              )}
            </svg>
            <XGlyph size={8} stroke={1.3} />
          </button>
          <div className="popup-text" style={{ maxHeight: MAX_TEXT_H }}>
            {text}
          </div>
          <button
            className={`popup-copy ${copied ? "copied" : ""}`}
            onClick={() =>
              commands
                .pillCopyText(text)
                .then((r) => setCopied(r.status === "ok"))
                .catch(() => {})
            }
          >
            {copied ? (
              <>
                <CheckGlyph size={11} stroke={1.5} />
                {tr("pill.copied", "Copied!")}
              </>
            ) : (
              tr("pill.copy", "Copy")
            )}
          </button>
        </div>
      </div>
    </>
  );
};
