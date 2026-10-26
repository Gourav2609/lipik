import React, { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { APP_NAME } from "@/flow/brand";
import { CheckGlyph, XGlyph } from "./parts";
import { PERM_H, PERM_REVEAL, PERM_SETTLE, PERM_W, t } from "./curves";
import type { Place } from "../Pill";

// Permission card growing out of the pill (reference notes,
// permissions1 26.09-33.78). Black surface, red warning, cream action button and an
// X inside a countdown ring that pauses while the cursor is on the overlay.
export const PERM = {
  w: 340,
  h: 148,
  bigW: 345,
  bigH: 150,
  edge: 14,
  seedW: 28,
  seedH: 5,
};
const REVEAL_AT = 410;
const SETTLE_DELAY = 20;
const RING_MS = 10200;
const CHECK_HOLD = 600;
const FADE_MS = 70;
const SHRINK_MS = 60;
const RING_R = 14.25;
const RING_C = 2 * Math.PI * RING_R;
const ASKED_MIC = "lipik.perm.micAsked";

export type PermissionKind = "microphone" | "accessibility";

const COPY: Record<
  PermissionKind,
  { title: string; body: string; action: string }
> = {
  microphone: {
    title: "Microphone Permission Required",
    body: `${APP_NAME} needs microphone access to work.`,
    action: "Grant Permission",
  },
  accessibility: {
    title: "Accessibility Permission Required",
    body: `${APP_NAME} needs accessibility access to type.`,
    action: "Open Settings",
  },
};

const pane = (name: string) =>
  openUrl(`x-apple.systempreferences:com.apple.preference.security?${name}`);

const micAskedBefore = () => {
  try {
    return localStorage.getItem(ASKED_MIC) === "1";
  } catch {
    return false;
  }
};

/** Asks macOS for the permission; does not wait for the user's answer. */
const ask = (kind: PermissionKind) => {
  if (kind === "microphone") {
    if (micAskedBefore()) {
      pane("Privacy_Microphone").catch(() => {});
      return;
    }
    try {
      localStorage.setItem(ASKED_MIC, "1");
    } catch {
      /* next click asks again instead of opening Settings */
    }
    requestMicrophonePermission().catch(() => {});
    return;
  }
  requestAccessibilityPermission().catch(() => {});
  pane("Privacy_Accessibility").catch(() => {});
};

type Stage = "seed" | "grow" | "shown" | "settled" | "fade" | "shrink";

type Props = {
  kind: PermissionKind;
  place: Place;
  hover: boolean;
  preview?: boolean;
  onDone: () => void;
};

export const Permission: React.FC<Props> = ({
  kind,
  place,
  hover,
  preview = false,
  onDone,
}) => {
  const [stage, setStage] = useState<Stage>("seed");
  const [done, setDone] = useState(false);
  const timers = useRef<number[]>([]);
  const leaving = useRef(false);
  const later = (ms: number, fn: () => void) =>
    timers.current.push(window.setTimeout(fn, ms));
  const copy = COPY[kind];

  useEffect(() => {
    const raf = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        setStage("grow");
        later(REVEAL_AT, () => {
          setStage((s) => (s === "grow" ? "shown" : s));
          later(SETTLE_DELAY, () =>
            setStage((s) => (s === "shown" ? "settled" : s)),
          );
        });
      }),
    );
    return () => {
      cancelAnimationFrame(raf);
      timers.current.forEach(clearTimeout);
    };
  }, []);

  const leave = (hold = 0) => {
    if (leaving.current) return;
    leaving.current = true;
    later(hold, () => {
      setStage("fade");
      later(FADE_MS, () => {
        setStage("shrink");
        later(SHRINK_MS, onDone);
      });
    });
  };

  const act = () => {
    if (done) return;
    setDone(true);
    if (!preview) ask(kind);
    leave(CHECK_HOLD);
  };

  const size = (): [number, number] => {
    switch (stage) {
      case "seed":
      case "shrink":
        return [PERM.seedW, PERM.seedH];
      case "grow":
      case "shown":
        return [PERM.bigW, PERM.bigH];
      case "fade":
        return [PERM.bigW, PERM.h];
      default:
        return [PERM.w, PERM.h];
    }
  };
  const [w, h] = size();
  const curve =
    stage === "grow"
      ? [
          t("width", PERM_W),
          t("left", PERM_W),
          t("height", PERM_H),
          t("top", PERM_H),
        ]
      : stage === "settled"
        ? ["width", "left", "height", "top"].map((p) => t(p, PERM_SETTLE))
        : stage === "fade"
          ? [`width ${FADE_MS}ms ease-out`, `left ${FADE_MS}ms ease-out`]
          : stage === "shrink"
            ? ["width", "left", "height", "top", "opacity"].map(
                (p) => `${p} ${SHRINK_MS}ms ease-in`,
              )
            : [];
  const surface: React.CSSProperties = {
    ...place(w, h, PERM.edge),
    transition: curve.join(", ") || "none",
  };
  const ringRunning = stage !== "seed" && !leaving.current;

  return (
    <div className={`pill-perm ${stage}`} style={surface} role="alertdialog">
      <div
        className="perm-content"
        style={
          stage === "shown" || stage === "settled"
            ? { opacity: 1, transition: t("opacity", PERM_REVEAL) }
            : stage === "fade"
              ? { opacity: 0, transition: `opacity ${FADE_MS}ms ease` }
              : undefined
        }
      >
        <svg
          className="perm-icon"
          width="16.5"
          height="16.5"
          viewBox="0 0 16.5 16.5"
          fill="none"
        >
          <circle
            cx="8.25"
            cy="8.25"
            r="7.4"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="M8.25 4.6v4.5"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
          <circle cx="8.25" cy="11.7" r="0.95" fill="currentColor" />
        </svg>
        <b className="perm-title">{copy.title}</b>
        <button
          className="perm-close"
          aria-label="Close"
          onClick={() => leave()}
        >
          <svg className="ring" width="30" height="30" viewBox="0 0 30 30">
            {stage !== "seed" && (
              <circle
                cx="15"
                cy="15"
                r={RING_R}
                transform="rotate(-90 15 15)"
                strokeDasharray={`${RING_C} ${RING_C}`}
                style={
                  {
                    "--ring": `${RING_C}px`,
                    animation: `pill-ring ${RING_MS}ms linear forwards`,
                    animationPlayState:
                      hover || !ringRunning || preview ? "paused" : "running",
                  } as React.CSSProperties
                }
                onAnimationEnd={() => leave()}
              />
            )}
          </svg>
          <XGlyph size={11} stroke={1.6} />
        </button>
        <p className="perm-body">{copy.body}</p>
        <button
          className={`perm-action ${done ? "done" : ""}`}
          style={kind === "microphone" ? { width: 140.5 } : undefined}
          onClick={act}
        >
          {done ? <CheckGlyph size={13} stroke={1.7} /> : copy.action}
        </button>
      </div>
    </div>
  );
};
