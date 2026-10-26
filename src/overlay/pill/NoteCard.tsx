import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Video } from "lucide-react";
import type { CalendarEvent } from "@/bindings";
import { LipikMark } from "@/flow/ui/LipikMark";
import { XGlyph } from "./parts";
import { openLink, startNotetaker } from "./backend";

// "Meeting detected" and the scheduled-meeting reminder. Not seen in a recording;
// drawn as a meeting reminder (notetaker1
// 56.4, NOTETAKER_UI.md §5): a 430 x 80 dark card, camera icon, title, a status
// line with a green dot, and a cream "Join Meeting / & take notes" split
// button. Cards that do not grow out of the pill fade and un-blur in (ANIMATIONS §1)
// and close in one frame.
export const CARD = { w: 430, h: 80, edge: 46, out: 10, menuH: 82, menuGap: 6 };
/** A detected call's card closes on its own (paused under the cursor). */
const CALL_MS = 15_000;
/** A reminder stays until 5 min after the start, never past the end (NOTETAKER_SPEC §5.1). */
const SOON_KEEP_MS = 5 * 60_000;
const RING_R = 11.25;
const RING_C = 2 * Math.PI * RING_R;

export type Card =
  | { kind: "call"; key: number; app: string; bundleId: string }
  | { kind: "soon"; key: number; event: CalendarEvent };

type Props = {
  card: Card;
  style: React.CSSProperties;
  hot: boolean;
  menu: boolean;
  onMenu: (open: boolean) => void;
  onClose: () => void;
};

const minutesTo = (ms: number) => Math.floor((ms - Date.now()) / 60_000);

export const NoteCard: React.FC<Props> = ({
  card,
  style,
  hot,
  menu,
  onMenu,
  onClose,
}) => {
  const { t: tr } = useTranslation();
  const [, setNow] = useState(0);
  const event = card.kind === "soon" ? card.event : null;
  const link = event?.meeting_url ?? null;

  useEffect(() => {
    if (!event) return;
    const closeAt = Math.min(event.start_ms + SOON_KEEP_MS, event.end_ms);
    const id = window.setInterval(() => {
      if (Date.now() >= closeAt && !hot && !menu) onClose();
      else setNow(Date.now());
    }, 1000);
    return () => clearInterval(id);
  }, [event, hot, menu, onClose]);

  const start = (title: string | null) => {
    onClose();
    startNotetaker(title);
  };
  const join = () => {
    if (link) openLink(link);
  };

  const title =
    card.kind === "call"
      ? tr("pill.meetingDetected", "Meeting detected")
      : card.event.title;
  const status =
    card.kind === "call"
      ? tr("pill.callIn", { app: card.app, defaultValue: "{{app}} call" })
      : minutesTo(card.event.start_ms) < 1
        ? tr("pill.meetingNow", "Now")
        : tr("pill.meetingIn", {
            count: minutesTo(card.event.start_ms),
            defaultValue: "In {{count}} min",
          });

  return (
    <div className={`note-card ${card.kind}`} style={style}>
      <Video className="card-icon" size={15} fill="currentColor" />
      <div className="card-text">
        <span className="card-title">{title}</span>
        <span className="card-status">
          <i />
          {status}
        </span>
      </div>
      <div className="card-act">
        <button
          className="card-main"
          onClick={() => {
            if (card.kind === "soon") join();
            start(event ? event.title : null);
          }}
        >
          <span className="card-logo">
            <LipikMark size={12} />
          </span>
          {card.kind === "soon" && link ? (
            <span className="card-two">
              <b>{tr("pill.joinMeeting", "Join Meeting")}</b>
              <small>{tr("pill.andStartNotetaker", "& take notes")}</small>
            </span>
          ) : (
            <b>{tr("pill.startNotetaker", "Start meeting notes")}</b>
          )}
        </button>
        {card.kind === "soon" && link && (
          <button className="card-more" onClick={() => onMenu(!menu)}>
            <ChevronDown size={16} strokeWidth={1.75} />
          </button>
        )}
      </div>
      <button className="card-close" onClick={onClose}>
        {card.kind === "call" && (
          <svg className="ring" width="24" height="24" viewBox="0 0 24 24">
            <circle
              cx="12"
              cy="12"
              r={RING_R}
              className="ring-left"
              transform="rotate(-90 12 12)"
              strokeDasharray={`${RING_C} ${RING_C}`}
              style={
                {
                  "--ring": `${RING_C}px`,
                  animation: `pill-ring ${CALL_MS}ms linear forwards`,
                  animationPlayState: hot ? "paused" : "running",
                } as React.CSSProperties
              }
              onAnimationEnd={onClose}
            />
          </svg>
        )}
        <XGlyph size={8} stroke={1.3} />
      </button>
      {menu && event && (
        <div className="card-menu">
          <button
            onClick={() => {
              onClose();
              join();
            }}
          >
            {tr("pill.joinOnly", "Join meeting")}
          </button>
          <button onClick={() => start(event.title)}>
            {tr("pill.startNotetaker", "Start meeting notes")}
          </button>
        </div>
      )}
    </div>
  );
};
