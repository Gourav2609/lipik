import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { TriangleAlert } from "lucide-react";
import { CheckGlyph, XGlyph } from "./parts";
import { POPUP_EXIT, POPUP_EXIT_FADE, POPUP_H, POPUP_W, t } from "./curves";
import type { Place } from "../Pill";

// "Started by mistake?" (S1 77.70-82.12, NOTETAKER_SPEC §8.1): grows out of the pill
// like the popup, holds black, then the content fades in. After a choice the label
// turns into a check, the content fades ≈70 ms and the surface shrinks into the pill.
export const MISTAKE = {
  w: 445,
  h: 171,
  edge: 15,
  seedW: 29,
  seedH: 2,
  lift: 2,
};
const REVEAL_AT = 400;
const CHECK_MS = 250;
const SHRINK_AT = 70;
const GONE_AT = SHRINK_AT + POPUP_EXIT.ms;

type Stage = "seed" | "grow" | "shown" | "exit";
type Choice = "keep" | "discard";

type Props = {
  place: Place;
  onChoice: (choice: Choice) => void;
  onDone: () => void;
};

export const Mistake: React.FC<Props> = ({ place, onChoice, onDone }) => {
  const { t: tr } = useTranslation();
  const [stage, setStage] = useState<Stage>("seed");
  const [chosen, setChosen] = useState<Choice | null>(null);
  const timers = useRef<number[]>([]);
  const later = (ms: number, fn: () => void) =>
    timers.current.push(window.setTimeout(fn, ms));

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
    };
  }, []);

  const choose = (choice: Choice, check = true) => {
    if (chosen) return;
    setChosen(choice);
    onChoice(choice);
    later(check ? CHECK_MS : 0, () => {
      setStage("exit");
      later(GONE_AT, onDone);
    });
  };

  const open = stage === "grow" || stage === "shown";
  const surface: React.CSSProperties = {
    ...place(
      open ? MISTAKE.w : MISTAKE.seedW,
      open ? MISTAKE.h : MISTAKE.seedH,
      MISTAKE.edge,
      MISTAKE.lift,
    ),
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
    <div className={`pill-popup pill-mistake ${stage}`} style={surface}>
      <div className="popup-content mistake-content">
        <div className="mistake-head">
          <TriangleAlert size={15} strokeWidth={1.75} absoluteStrokeWidth />
          <b>{tr("pill.startedByMistake", "Started by mistake?")}</b>
        </div>
        <button className="popup-close" onClick={() => choose("keep", false)}>
          <svg className="ring" width="26" height="26" viewBox="0 0 26 26">
            <circle cx="13" cy="13" r="12.25" className="ring-left" />
          </svg>
          <XGlyph size={8} stroke={1.3} />
        </button>
        <p className="mistake-body">
          {tr(
            "pill.fewWords",
            "Only a few words were captured. Keep this meeting or discard it.",
          )}
        </p>
        <div className="mistake-buttons">
          <button className="mistake-discard" onClick={() => choose("discard")}>
            {chosen === "discard" ? (
              <CheckGlyph size={11} stroke={1.5} />
            ) : (
              tr("pill.discard", "Discard")
            )}
          </button>
          <button className="mistake-keep" onClick={() => choose("keep")}>
            {chosen === "keep" ? (
              <CheckGlyph size={11} stroke={1.5} />
            ) : (
              tr("pill.keep", "Keep")
            )}
          </button>
        </div>
      </div>
    </div>
  );
};
