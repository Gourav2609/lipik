import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckGlyph } from "./parts";
import { SHRINK, TOAST_H, TOAST_W, t } from "./curves";
import { commands, type PillSide } from "@/bindings";
import type { Place } from "../Pill";

// "Transcript cancelled" toast, measured in S1 39.7-41.9 (ANIMATIONS.md §3 rows 7-8).
export const TOAST = { w: 244, h: 48, edge: 12.5, seedW: 22.5, seedH: 6 };
const R = 22.5;
const TRACK_Y = 46.5;
const dx = 7.5;
const TRACK = `M ${24 - dx} ${24 + Math.sqrt(R * R - dx * dx)} A ${R} ${R} 0 0 0 24 ${TRACK_Y} L ${TOAST.w - 24} ${TRACK_Y} A ${R} ${R} 0 0 0 ${TOAST.w - 24 + dx} ${24 + Math.sqrt(R * R - dx * dx)}`;
const TRACK_LEN = 2 * R * Math.asin(dx / R) + TOAST.w - 48;

const REVEAL_AT = 440;
const CHECK_AT = 67;
const SHRINK_AT = 108;
const COVER_END = 180;
const GONE_AT = 280;

type Stage = "seed" | "grow" | "shown" | "exit";

type Props = {
  side: PillSide;
  place: Place;
  undoMs: number;
  /** Cursor over the toast: the countdown waits. */
  hover: boolean;
  /** The pill may show again (processing pill fades in under the shrinking toast). */
  onCoverEnd: () => void;
  onDone: () => void;
};

export const Toast: React.FC<Props> = ({
  side,
  place,
  undoMs,
  hover,
  onCoverEnd,
  onDone,
}) => {
  const { t: tr } = useTranslation();
  const [stage, setStage] = useState<Stage>("seed");
  const [checked, setChecked] = useState(false);
  const undoing = useRef(false);
  const exiting = useRef(false);
  const born = useRef(performance.now());
  const trackMs = useRef(0);
  const timers = useRef<number[]>([]);
  const later = (ms: number, fn: () => void) =>
    timers.current.push(window.setTimeout(fn, ms));

  useEffect(() => {
    const raf = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        setStage("grow");
        later(REVEAL_AT, () => {
          trackMs.current = Math.max(
            1000,
            undoMs - (performance.now() - born.current),
          );
          setStage((s) => (s === "grow" ? "shown" : s));
        });
      }),
    );
    return () => {
      cancelAnimationFrame(raf);
      timers.current.forEach(clearTimeout);
    };
  }, []);

  const exit = (undo: boolean) => {
    if (exiting.current) return;
    exiting.current = true;
    setStage("exit");
    if (undo) later(CHECK_AT, () => setChecked(true));
    else commands.pillUndoExpired().catch(() => {});
    later(COVER_END, onCoverEnd);
    later(GONE_AT, onDone);
  };

  const grown = stage !== "seed";
  const w = stage === "exit" ? TOAST.seedW : grown ? TOAST.w : TOAST.seedW;
  const h = grown ? TOAST.h : TOAST.seedH;
  const surface: React.CSSProperties = {
    ...place(w, h, TOAST.edge),
    transition:
      stage === "exit"
        ? [
            t("width", SHRINK, SHRINK_AT),
            t("left", SHRINK, SHRINK_AT),
            `opacity ${SHRINK.ms}ms ${SHRINK.ease} ${SHRINK_AT}ms`,
            "background-color 50ms linear 33ms",
          ].join(", ")
        : [
            t("width", TOAST_W),
            t("left", TOAST_W),
            t("height", TOAST_H),
            t("top", TOAST_H),
            "background-color 230ms linear",
          ].join(", "),
  };

  return (
    <div className={`pill-toast ${stage}`} style={surface}>
      <div className="toast-content">
        <span className="toast-text">
          {tr("pill.cancelled", "Transcript cancelled")}
        </span>
        <button
          className="toast-undo"
          onClick={() => {
            if (undoing.current) return;
            undoing.current = true;
            commands.pillUndoCancel().then(
              (ok: unknown) => exit(ok !== false),
              () => exit(false),
            );
          }}
        >
          <span>
            {checked ? (
              <CheckGlyph size={11} stroke={1.4} />
            ) : (
              tr("pill.undo", "Undo")
            )}
          </span>
        </button>
        <svg
          className="toast-track"
          width={TOAST.w}
          height={TOAST.h}
          style={
            side === "bottom"
              ? { left: "50%", marginLeft: -TOAST.w / 2 }
              : { [side]: 0 }
          }
        >
          <path d={TRACK} stroke="#66625a" />
          {trackMs.current > 0 && (
            <path
              d={TRACK}
              stroke="#f6f2ea"
              strokeDasharray={`${TRACK_LEN} ${TRACK_LEN}`}
              style={
                {
                  "--track": `${TRACK_LEN}px`,
                  animation: `pill-track ${trackMs.current}ms linear forwards`,
                  animationPlayState:
                    hover || stage === "exit" ? "paused" : "running",
                } as React.CSSProperties
              }
              onAnimationEnd={() => exit(false)}
            />
          )}
        </svg>
      </div>
    </div>
  );
};
